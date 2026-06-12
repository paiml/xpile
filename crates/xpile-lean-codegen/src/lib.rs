//! Lean 4 (executable) backend.
//!
//! Emits the executable subset of Lean 4 from meta-HIR. Theorem
//! statements are NOT emitted here — those go through the proof lane
//! (`xpile-lean-contract-backend`).
//!
//! Surface mapping (Python → Lean):
//!   I64                  → Int
//!   Bool                 → Bool
//!   `pub fn name(...)`   → `def name (...) : T := body`
//!   `let x: T = v;`      → `let x := v;`  (Lean infers `T` from `v`)
//!   `if c { a } else { b }` (expr) → `if c then a else b`
//!   `a // b` (FloorDiv)  → `Int.fdiv a b`  (Python floor semantics)
//!   `a %  b` (Mod)       → `Int.fmod a b`
//!   logical `and`/`or`   → `&&` / `||`
//!   unary `-x` / `not x` → `(-x)` / `(!x)`
//!   call `f(args)`       → `(f arg1 arg2)` (Lean juxtaposition, parenthesized)
//!
//! Layer 2 contract for the Lean→Rust direction is
//! `contracts/xlate-lean-to-rust-v1.yaml`. The companion contract for
//! Rust/meta-HIR → Lean is reserved for a later authoring pass.

use std::fmt::Write;
use xpile_backend::{Artifact, Backend, BackendConfig, BackendError, QuorumStatus, Target};
use xpile_meta_hir::{
    BinOp, Block, Expr, FloatOp, Function, Item, Module, Param, Stmt, Type, UnOp,
};

#[derive(Debug, thiserror::Error)]
pub enum LeanCodegenError {
    #[error("unsupported item: {0}")]
    Unsupported(String),
    #[error("formatting error: {0}")]
    Format(#[from] std::fmt::Error),
}

pub fn emit_module(module: &Module) -> Result<String, LeanCodegenError> {
    let mut out = String::new();
    writeln!(
        out,
        "-- xpile-generated from {:?} module {}",
        module.source_lang, module.name
    )?;
    writeln!(out)?;
    for item in &module.items {
        match item {
            Item::Function(f) => {
                if function_has_while(f) {
                    emit_function_with_while_helpers(&mut out, f)?;
                } else {
                    emit_function(&mut out, f)?;
                }
            }
        }
    }
    Ok(out)
}

fn function_has_while(f: &Function) -> bool {
    f.body.stmts.iter().any(|s| matches!(s, Stmt::While { .. }))
}

fn emit_function(out: &mut String, f: &Function) -> Result<(), LeanCodegenError> {
    emit_contract_citations(out, f)?;
    write!(out, "def {}", f.name)?;
    for p in &f.params {
        write!(out, " (")?;
        emit_param(out, p)?;
        write!(out, ")")?;
    }
    write!(out, " : ")?;
    emit_type(out, &f.return_type)?;
    writeln!(out, " :=")?;
    emit_block(out, &f.body)?;
    Ok(())
}

/// PMAT-011: emit Lean structured attributes for each applicable
/// contract. `@[xpile_contract "<ID>"]` is the form named in
/// `sub/contract-frontend-trait.md`'s citation grid — Lean's elaborator
/// parses it, so the citation bridge can use Lean's name resolution
/// rather than regex over body text.
fn emit_contract_citations(out: &mut String, f: &Function) -> Result<(), LeanCodegenError> {
    for id in f.applicable_contracts() {
        writeln!(out, "@[xpile_contract \"{id}\"]")?;
    }
    Ok(())
}

/// Lean has no mutation, so a Python `while` lowers to a `partial def`
/// helper that threads loop-state variables as parameters and recurses
/// with their updated values. PMAT-010.
///
/// Shape supported at v0.1.0:
///   * Exactly one `Stmt::While` in the function body.
///   * The while must be the *last* statement before `trailing_return`.
///   * `trailing_return` must be `Expr::Ident(<name>)` where `<name>`
///     is in the loop's mutated set (the variable the helper returns).
///   * The while body must contain only `Stmt::Assign` (no nested Let /
///     If / While). Reassignments produce the new value passed to the
///     recursive call.
///
/// Anything outside this shape returns `LeanCodegenError::Unsupported`
/// with a message naming what it tripped on, so a user reading the
/// error knows whether to rewrite their Python or to wait for a
/// follow-up that broadens the encoding.
fn emit_function_with_while_helpers(
    out: &mut String,
    f: &Function,
) -> Result<(), LeanCodegenError> {
    let while_indices: Vec<usize> = f
        .body
        .stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            if matches!(s, Stmt::While { .. }) {
                Some(i)
            } else {
                None
            }
        })
        .collect();
    if while_indices.len() != 1 {
        return Err(LeanCodegenError::Unsupported(format!(
            "function `{}` has {} while-loops; Lean codegen supports exactly one per function at v0.1.0",
            f.name,
            while_indices.len()
        )));
    }
    let while_idx = while_indices[0];
    if while_idx != f.body.stmts.len() - 1 {
        return Err(LeanCodegenError::Unsupported(format!(
            "function `{}` has statements after its while loop; Lean codegen requires the while to be the last pre-return statement at v0.1.0",
            f.name
        )));
    }
    let return_var = match &f.body.trailing_return {
        Expr::Ident(name) => name.clone(),
        _ => {
            return Err(LeanCodegenError::Unsupported(format!(
                "function `{}` has a non-Ident trailing return after its while loop; Lean codegen supports `return <name>` only at v0.1.0",
                f.name
            )));
        }
    };

    let pre_stmts = &f.body.stmts[..while_idx];
    let (cond, while_body) = match &f.body.stmts[while_idx] {
        Stmt::While { cond, body } => (cond, body),
        _ => unreachable!(),
    };

    // Loop state — names assigned anywhere in the body. Source order
    // preserves discovery order for predictable signatures.
    let mut loop_state: Vec<String> = Vec::new();
    for stmt in while_body {
        match stmt {
            Stmt::Assign { name, .. } => {
                if !loop_state.contains(name) {
                    loop_state.push(name.clone());
                }
            }
            // PMAT-478 (R9): if/else inside a while is not encodable in
            // the v0.2.0 partial-def loop shape.
            Stmt::If { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has an if/else statement inside a while loop; Lean codegen does not compose Stmt::If with the partial-def loop encoding at v0.2.0",
                    f.name
                )));
            }
            // PMAT-479 (R10): early return inside a while is not encodable.
            Stmt::Return(_) => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has an early `return` inside a while loop; Lean codegen keeps the single-trailing-return shape at v0.2.0",
                    f.name
                )));
            }
            Stmt::Let { name, .. } => {
                // v0.1.0 frontend produces only Assigns inside loop
                // bodies; treat a Let as a fresh binding the loop
                // re-initializes (also goes into loop_state).
                if !loop_state.contains(name) {
                    loop_state.push(name.clone());
                }
            }
            Stmt::Assert { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has `assert` inside a while loop; Lean codegen at v0.1.0 doesn't translate assert through partial def",
                    f.name
                )));
            }
            Stmt::While { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has a nested while; Lean codegen at v0.1.0 doesn't translate nested loops",
                    f.name
                )));
            }
            // PMAT-039: shell commands aren't legal inside a typed
            // Lean function — they belong to the bashrs domain. Reach
            // here only if a Module was somehow constructed with a
            // Function containing both a while loop AND a Cmd
            // (impossible from bashrs-frontend, which produces flat
            // command sequences without loops; defensive arm).
            Stmt::Cmd { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has Stmt::Cmd inside a while loop; \
                     C-BASHRS-POSIX-IDEMPOTENCE governs shell commands — \
                     Lean codegen does not lower them",
                    f.name
                )));
            }
            // PMAT-041: same disposition as Cmd.
            Stmt::Pipeline { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has Stmt::Pipeline inside a while loop; \
                     C-BASHRS-POSIX-IDEMPOTENCE governs shell pipelines — \
                     Lean codegen does not lower them",
                    f.name
                )));
            }
            // PMAT-048: same disposition.
            Stmt::ShellLoop { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has Stmt::ShellLoop inside a while loop; \
                     C-BASHRS-POSIX-IDEMPOTENCE governs shell loops — \
                     Lean codegen does not lower them",
                    f.name
                )));
            }
            // PMAT-051: same disposition.
            Stmt::ShellAssign { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has Stmt::ShellAssign inside a while loop; \
                     C-BASHRS-POSIX-IDEMPOTENCE governs shell assignment — \
                     Lean codegen does not lower it",
                    f.name
                )));
            }
            // PMAT-458: for-each inside a while loop — composing a
            // for-each within a partial-def while-helper would need
            // monadic encoding (forM in some monad over the closing
            // partial def). Deferred to v0.3.0.
            Stmt::ForEach { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has Stmt::ForEach inside a while loop; \
                     Lean codegen at v0.2.0 first cut doesn't compose for-each with while",
                    f.name
                )));
            }
            // PMAT-460: list.append() inside a while loop — same
            // monadic-encoding gap as ForEach. Deferred.
            Stmt::ListAppend { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has Stmt::ListAppend inside a while loop; \
                     Lean codegen at v0.2.0 first cut doesn't compose in-place mutation with while",
                    f.name
                )));
            }
            // PMAT-461: indexed assignment inside a while loop — same.
            Stmt::IndexAssign { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has Stmt::IndexAssign inside a while loop; \
                     Lean codegen at v0.2.0 first cut doesn't compose in-place mutation with while",
                    f.name
                )));
            }
            // PMAT-466: dict keyed assignment inside a while — same
            // disposition as IndexAssign.
            Stmt::DictSet { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has Stmt::DictSet inside a while loop; \
                     Lean codegen at v0.2.0 first cut doesn't compose in-place mutation with while",
                    f.name
                )));
            }
        }
    }

    if !loop_state.contains(&return_var) {
        return Err(LeanCodegenError::Unsupported(format!(
            "function `{}` returns `{}` which is not mutated by its while loop; Lean codegen requires the return value to be in the loop state at v0.1.0",
            f.name, return_var
        )));
    }

    // Free vars = names referenced in {cond, body assigns} but not in
    // loop_state. Preserve discovery order.
    let mut all_refs: Vec<String> = Vec::new();
    collect_idents(cond, &mut all_refs);
    for stmt in while_body {
        if let Stmt::Assign { value, .. } = stmt {
            collect_idents(value, &mut all_refs);
        }
        if let Stmt::Let { value, .. } = stmt {
            collect_idents(value, &mut all_refs);
        }
    }
    let mut free_vars: Vec<String> = Vec::new();
    for r in &all_refs {
        if !loop_state.contains(r) && !free_vars.contains(r) {
            free_vars.push(r.clone());
        }
    }

    // Emit the helper. v0.1.0's type lattice is I64 / Bool, and the
    // loop state and free vars are I64 for every fixture we ship. Use
    // I64 as the default; refine later if Bool-typed loop state appears.
    let helper_name = format!("{}_loop_0", f.name);
    // The helper's body executes the same arithmetic constructs as
    // the outer function, so it shares the contract citation.
    emit_contract_citations(out, f)?;
    write!(out, "partial def {} ", helper_name)?;
    for name in &loop_state {
        write!(out, "({} : ", name)?;
        emit_type(out, &Type::I64)?;
        write!(out, ") ")?;
    }
    for name in &free_vars {
        write!(out, "({} : ", name)?;
        emit_type(out, &lookup_var_type(name, &f.params))?;
        write!(out, ") ")?;
    }
    write!(out, ": ")?;
    emit_type(out, &f.return_type)?;
    writeln!(out, " :=")?;
    write!(out, "  if ")?;
    emit_expr(out, cond)?;
    writeln!(out, " then")?;
    // Body: each Assign / Let becomes a `let name := value` (Lean
    // shadows, so reassigning the same name is fine).
    for stmt in while_body {
        match stmt {
            Stmt::Assign { name, value } | Stmt::Let { name, value, .. } => {
                write!(out, "    let {name} := ")?;
                emit_expr(out, value)?;
                writeln!(out)?;
            }
            _ => unreachable!("validated above"),
        }
    }
    // Recursive call with current values of loop_state and free_vars.
    write!(out, "    {helper_name}")?;
    for name in loop_state.iter().chain(free_vars.iter()) {
        write!(out, " {name}")?;
    }
    writeln!(out)?;
    writeln!(out, "  else")?;
    writeln!(out, "    {return_var}")?;
    writeln!(out)?;

    // Emit the outer function. Body: pre-stmts as Lean lets, then the
    // helper call. Citation appears here too — the outer function
    // delegates to the helper but is still the user-facing site of
    // the arithmetic claim.
    emit_contract_citations(out, f)?;
    write!(out, "def {}", f.name)?;
    for p in &f.params {
        write!(out, " (")?;
        emit_param(out, p)?;
        write!(out, ")")?;
    }
    write!(out, " : ")?;
    emit_type(out, &f.return_type)?;
    writeln!(out, " :=")?;
    for stmt in pre_stmts {
        emit_stmt(out, stmt)?;
    }
    write!(out, "  {helper_name}")?;
    for name in loop_state.iter().chain(free_vars.iter()) {
        write!(out, " {name}")?;
    }
    writeln!(out)?;
    Ok(())
}

/// Look up a free var's type — params have explicit types; otherwise
/// the v0.1.0 lattice defaults to I64 (the only numeric type yet, and
/// pre-stmt Lets all infer I64 currently).
fn lookup_var_type(name: &str, params: &[Param]) -> Type {
    for p in params {
        if p.name == name {
            return p.ty.clone();
        }
    }
    Type::I64
}

/// Recursively collect identifier names from an expression. Used by
/// the while-helper analyzer to find free vars referenced in the cond
/// and body. Preserves insertion order so emitted helper signatures
/// are deterministic.
fn collect_idents(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Ident(n) => {
            if !out.contains(n) {
                out.push(n.clone());
            }
        }
        Expr::LitInt(_) | Expr::LitBool(_) | Expr::LitFloat(_) => {}
        // PMAT-477 (R8): float arithmetic — recurse into operands.
        Expr::FloatBinOp { lhs, rhs, .. } => {
            collect_idents(lhs, out);
            collect_idents(rhs, out);
        }
        Expr::BinOp { lhs, rhs, .. } | Expr::Concat { lhs, rhs } => {
            collect_idents(lhs, out);
            collect_idents(rhs, out);
        }
        // PMAT-455: list literal — recurse into each element.
        Expr::ListLit(elems) => {
            for e in elems {
                collect_idents(e, out);
            }
        }
        // PMAT-462: dict literal — recurse into each key + value.
        Expr::DictLit(pairs) => {
            for (k, v) in pairs {
                collect_idents(k, out);
                collect_idents(v, out);
            }
        }
        // PMAT-457: indexed access — recurse into both sides.
        Expr::Index { collection, index } => {
            collect_idents(collection, out);
            collect_idents(index, out);
        }
        // PMAT-466: dict ops — recurse into all sub-expressions so
        // loop-state ident discovery stays correct even though Lean
        // emit refuses these constructs downstream.
        Expr::DictGet { dict, key } | Expr::DictContains { dict, key } => {
            collect_idents(dict, out);
            collect_idents(key, out);
        }
        Expr::DictGetOr { dict, key, default } => {
            collect_idents(dict, out);
            collect_idents(key, out);
            collect_idents(default, out);
        }
        // PMAT-459: len(x) — recurse into inner.
        Expr::Len(inner) => collect_idents(inner, out),
        Expr::UnOp { operand, .. } => collect_idents(operand, out),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            collect_idents(cond, out);
            collect_idents(then_expr, out);
            collect_idents(else_expr, out);
        }
        Expr::Call { args, .. } => {
            for a in args {
                collect_idents(a, out);
            }
        }
        // PMAT-042: shell-string Expr variants carry no idents. The
        // while-helper analyzer only walks the cond + assignments of
        // a while loop body, none of which contain shell strings, so
        // we never reach this in practice — defensive arm.
        Expr::LitStr(_) | Expr::QuotedString { .. } => {}
        // PMAT-045: shell-variable refs likewise carry no Rust-level
        // idents (the name is shell-side, not meta-HIR-bound).
        Expr::ShellVar(_) => {}
        // PMAT-047: command substitution composes a Stmt; the
        // while-loop analyzer never reaches here in practice (no
        // shell-domain stmts inside while loops in current
        // frontends), so no recursion needed at v0.1.0.
        Expr::CommandSubstitution(_) => {}
        // PMAT-055: shell special params likewise carry no
        // Rust-level idents.
        Expr::ShellSpecial(_) => {}
    }
}

fn emit_param(out: &mut String, p: &Param) -> Result<(), LeanCodegenError> {
    write!(out, "{} : ", p.name)?;
    emit_type(out, &p.ty)?;
    Ok(())
}

/// Escape a string for emission inside a Lean `"..."` literal.
/// PMAT-449 — Lean's string literal syntax handles `\\` and `\"`
/// the same way as Rust; richer escapes (Unicode, octal) are v0.2.0
/// later sub-tracks.
fn escape_lean_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            other => out.push(other),
        }
    }
    out
}

fn emit_type(out: &mut String, t: &Type) -> Result<(), LeanCodegenError> {
    match t {
        Type::I64 => out.push_str("Int"),
        // PMAT-477 (R8): Python `float` → Lean `Float`.
        Type::F64 => out.push_str("Float"),
        Type::Bool => out.push_str("Bool"),
        // Lean's Int is already unbounded — same shape as BigInt.
        Type::BigInt => out.push_str("Int"),
        // PMAT-449 (v0.2.0 Track 1.A): Lean's built-in String.
        Type::Str => out.push_str("String"),
        // PMAT-455 (v0.2.0 Track 1.B): Lean's built-in `List T`.
        // PMAT-462 fixup: parenthesize the element so nested
        // `list[list[T]]` lowers to `List (List Int)` rather than
        // `List List Int` (which Lean parses as application of two
        // separate types).
        Type::List(elem_ty) => {
            out.push_str("List (");
            emit_type(out, elem_ty)?;
            out.push(')');
        }
        // PMAT-462 (v0.2.0 Track 1.C): Python `dict[K, V]` → Lean
        // `List (K × V)` first cut. The product type `K × V` is
        // Lean's native pair / `Prod K V`. A subsequent v0.3.0
        // sub-track upgrades to `Std.HashMap` once iteration /
        // lookup encoding lands.
        Type::Dict(k_ty, v_ty) => {
            out.push_str("List (");
            emit_type(out, k_ty)?;
            out.push_str(" × ");
            emit_type(out, v_ty)?;
            out.push(')');
        }
        // PMAT-046: bashrs-domain types refused.
        Type::ShellString | Type::ExitCode => {
            return Err(LeanCodegenError::Unsupported(format!(
                "Lean code backend does not lower {t:?} — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs the bashrs type domain"
            )));
        }
    }
    Ok(())
}

fn emit_block(out: &mut String, block: &Block) -> Result<(), LeanCodegenError> {
    emit_stmts_then_trailing(out, &block.stmts, &block.trailing_return)
}

/// Recursive emit (PMAT-027 / PMAT-009-FOLLOWUP). Each `Stmt::Assert`
/// opens an `if cond then <rest of body> else panic!` form that
/// contains *everything after the assert*, preserving Python's
/// evaluation order. Without this, an assert between two Lets would
/// reference a name not yet defined when checked.
///
/// Non-assert stmts (Let, Assign) emit linearly via `emit_stmt`;
/// they fall through to the recursive tail.
///
/// Shape on `safe_div` (the asserted.py fixture):
///
/// ```lean
/// def safe_div (a : Int) (b : Int) : Int :=
///   if (b != (0: Int)) then
///   if (a >= (0: Int)) then
///   (Int.fdiv a b)
///   else panic! "xpile: assertion failed (contract C-PY-INT-ARITH)"
///   else panic! "xpile: assertion failed (contract C-PY-INT-ARITH)"
/// ```
///
/// Lean accepts this with no closing brace because each `if-then-else`
/// is a single term; the recursive emit just keeps appending the
/// `else panic!` tails after the inner body completes.
fn emit_stmts_then_trailing(
    out: &mut String,
    stmts: &[Stmt],
    trailing: &Expr,
) -> Result<(), LeanCodegenError> {
    if stmts.is_empty() {
        write!(out, "  ")?;
        emit_expr(out, trailing)?;
        writeln!(out)?;
        return Ok(());
    }
    match &stmts[0] {
        Stmt::Assert { cond } => {
            write!(out, "  if (")?;
            emit_expr(out, cond)?;
            writeln!(out, ") then")?;
            emit_stmts_then_trailing(out, &stmts[1..], trailing)?;
            writeln!(
                out,
                "  else panic! \"xpile: assertion failed (contract C-PY-INT-ARITH)\""
            )?;
            Ok(())
        }
        other => {
            emit_stmt(out, other)?;
            emit_stmts_then_trailing(out, &stmts[1..], trailing)
        }
    }
}

fn emit_stmt(out: &mut String, stmt: &Stmt) -> Result<(), LeanCodegenError> {
    match stmt {
        // Lean has no `mut` — let-bindings are already immutable.
        // Reassignment via `Stmt::Assign` works because Lean's `let`
        // allows shadowing: emit it as another `let name := value`.
        // PMAT-479 (R10): early returns need a match/monadic encoding;
        // Lean keeps the single-trailing-return shape. The decy C
        // frontend (which produces these) targets Rust.
        Stmt::Return(_) => Err(LeanCodegenError::Unsupported(
            "Stmt::Return (early return) is not lowered by the Lean backend — \
             Lean uses a single trailing return; use `--target rust` or `--target ruchy`"
                .into(),
        )),
        // PMAT-478 (R9): the executable-Lean encoding routes branching
        // through the if-*expression* form (see emit_if_expr), not a
        // statement-if; the decy C frontend (which produces Stmt::If)
        // targets Rust, so refuse here with a clear pointer.
        Stmt::If { .. } => Err(LeanCodegenError::Unsupported(
            "Stmt::If (statement-form if/else) is not lowered by the Lean backend — \
             Lean uses the if-expression form; use `--target rust` or `--target ruchy`"
                .into(),
        )),
        Stmt::Let { name, value, .. } | Stmt::Assign { name, value } => {
            write!(out, "  let {name} := ")?;
            emit_expr(out, value)?;
            writeln!(out)?;
            Ok(())
        }
        // `while` requires mutation/iteration. The Lean encoding is a
        // `partial def` with tail recursion; not implemented at v0.1.0.
        // Layer-2 equivalence with the Rust/Ruchy emission would
        // ultimately come from contracts/xlate-lean-to-rust-v1.yaml.
        Stmt::While { .. } => Err(LeanCodegenError::Unsupported(
            "`while` loops require partial def / tail-recursion in Lean — not yet implemented (PMAT-006 follow-up)"
                .into(),
        )),
        // PMAT-458 (v0.2.0 Track 1.B): for-each over collections.
        // Lean's idiomatic encoding is `xs.forM (fun var => body)` in
        // some monad, or List recursion. Both require monadic
        // structure that v0.2.0 first cut doesn't yet thread. Deferred
        // to v0.3.0+ alongside other Lean iteration work.
        Stmt::ForEach { .. } => Err(LeanCodegenError::Unsupported(
            "`for x in xs:` (Stmt::ForEach) requires monadic-iteration encoding in Lean — \
             not yet implemented at v0.2.0 first cut (PMAT-458 follow-up); \
             use `--target rust` or `--target ruchy` for iteration"
                .into(),
        )),
        // PMAT-460 (v0.2.0 Track 1.B): list.append() mutation. Lean
        // has no in-place mutation; the encoding would need a
        // state-monad rewrite of the surrounding function. Same
        // posture as Stmt::ForEach — deferred to v0.3.0.
        Stmt::ListAppend { list_name, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{list_name}.append(...)` (Stmt::ListAppend) requires state-monad encoding in Lean — \
             not yet implemented at v0.2.0 first cut (PMAT-460 follow-up); \
             use `--target rust` or `--target ruchy` for in-place mutation"
        ))),
        // PMAT-461 (v0.2.0 Track 1.B): indexed assignment — same
        // monadic-encoding gap as ListAppend / ForEach.
        Stmt::IndexAssign { list_name, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{list_name}[i] = v` (Stmt::IndexAssign) requires state-monad encoding in Lean — \
             not yet implemented at v0.2.0 first cut (PMAT-461 follow-up); \
             use `--target rust` or `--target ruchy` for in-place mutation"
        ))),
        // PMAT-466 (v0.2.0 Track 1.C): dict keyed assignment — same
        // state-monad encoding gap as IndexAssign / ListAppend.
        Stmt::DictSet { dict_name, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{dict_name}[k] = v` (Stmt::DictSet) requires state-monad encoding in Lean — \
             not yet implemented at v0.2.0 first cut (PMAT-466 follow-up); \
             use `--target rust` or `--target ruchy` for in-place mutation"
        ))),
        // Stmt::Assert is handled by emit_stmts_then_trailing — should
        // never reach this match arm. The unreachable here catches a
        // future refactor that bypasses the recursive emit.
        Stmt::Assert { .. } => unreachable!(
            "Stmt::Assert handled in emit_stmts_then_trailing — emit_stmt called directly"
        ),
        // PMAT-039 / XPILE-BASHRS-MERGER-001 Layer B: Lean has no
        // notion of shell-command invocation. `C-BASHRS-POSIX-IDEMPOTENCE`
        // governs `Stmt::Cmd`; any cross-domain refinement would lower
        // via the bashrs domain, not the Lean one.
        Stmt::Cmd { program, args } => Err(LeanCodegenError::Unsupported(format!(
            "Lean backend does not lower Stmt::Cmd (`{program}` with {} arg(s)) — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell commands; \
             use `--target shell` instead",
            args.len()
        ))),
        // PMAT-041: same disposition as Cmd.
        Stmt::Pipeline { stages } => Err(LeanCodegenError::Unsupported(format!(
            "Lean backend does not lower Stmt::Pipeline ({} stages) — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell pipelines; \
             use `--target shell` instead",
            stages.len()
        ))),
        // PMAT-048: same disposition.
        Stmt::ShellLoop { .. } => Err(LeanCodegenError::Unsupported(
            "Lean backend does not lower Stmt::ShellLoop — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell loops; \
             use `--target shell`"
                .into(),
        )),
        // PMAT-051: same disposition.
        Stmt::ShellAssign { name, .. } => Err(LeanCodegenError::Unsupported(format!(
            "Lean backend does not lower Stmt::ShellAssign (`{name}=…`) — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell assignment; \
             use `--target shell`"
        ))),
    }
}

fn emit_expr(out: &mut String, e: &Expr) -> Result<(), LeanCodegenError> {
    match e {
        Expr::Ident(name) => write!(out, "{}", name)?,
        Expr::LitInt(v) => write!(out, "({}: Int)", v)?,
        // PMAT-477 (R8): Python `float` → Lean `Float` literal +
        // plain-infix arithmetic (Lean `Float` supports `+ - * /`).
        Expr::LitFloat(v) => write!(out, "({}: Float)", v)?,
        Expr::FloatBinOp { op, lhs, rhs } => {
            let sym = match op {
                FloatOp::Add => "+",
                FloatOp::Sub => "-",
                FloatOp::Mul => "*",
                FloatOp::Div => "/",
            };
            out.push('(');
            emit_expr(out, lhs)?;
            write!(out, " {sym} ")?;
            emit_expr(out, rhs)?;
            out.push(')');
        }
        // PMAT-456 (v0.2.0 Track 1.B): bool literal — Lean
        // capitalises the constructors (`True` / `False`).
        Expr::LitBool(b) => write!(out, "{}", if *b { "True" } else { "False" })?,
        Expr::BinOp { op, lhs, rhs } => emit_binop(out, *op, lhs, rhs)?,
        // PMAT-451 (v0.2.0 Track 1.A): Lean's `String` concatenation
        // is the `++` operator (`String.append`). Strings are
        // unbounded in Lean — no overflow concept, mirrors the proof-
        // lane shadow of the Rust/Ruchy `format!()` emission.
        Expr::Concat { lhs, rhs } => {
            out.push('(');
            emit_expr(out, lhs)?;
            out.push_str(" ++ ");
            emit_expr(out, rhs)?;
            out.push(')');
        }
        // PMAT-455 (v0.2.0 Track 1.B): Lean's built-in `List` literal
        // syntax — `[a, b, c]`.
        Expr::ListLit(elems) => {
            out.push('[');
            for (i, e) in elems.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit_expr(out, e)?;
            }
            out.push(']');
        }
        // PMAT-462 (v0.2.0 Track 1.C): Lean dict literal as a list
        // of pairs — `[(k1, v1), (k2, v2), ...]`. First-cut model;
        // when the v0.3.0 Std.HashMap encoding lands the lowering
        // wraps this in an `Std.HashMap.ofList`.
        Expr::DictLit(pairs) => {
            out.push('[');
            for (i, (k, v)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push('(');
                emit_expr(out, k)?;
                out.push_str(", ");
                emit_expr(out, v)?;
                out.push(')');
            }
            out.push(']');
        }
        // PMAT-457 (v0.2.0 Track 1.B): Lean's `xs[i]!` syntax —
        // panics on out-of-range with a clear error message. We
        // coerce the i64 index to `Nat` via `.toNat`.
        Expr::Index { collection, index } => {
            emit_expr(out, collection)?;
            out.push('[');
            emit_expr(out, index)?;
            out.push_str(".toNat]!");
        }
        // PMAT-466 (v0.2.0 Track 1.C): dict read / get-with-default /
        // membership. The `List (K × V)` first-cut model has no
        // panic-on-absent lookup or O(1) keyed access; a faithful
        // encoding needs the `Std.HashMap` upgrade that also unblocks
        // Lean iteration/mutation. Deferred to v0.3.0; refuse clearly.
        Expr::DictGet { .. } | Expr::DictGetOr { .. } | Expr::DictContains { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "dict operations (`d[k]`, `d.get(k, default)`, `k in d`) require the \
                 Std.HashMap Lean encoding — not yet implemented at v0.2.0 first cut \
                 (PMAT-466 follow-up, alongside Lean iteration/mutation); \
                 use `--target rust` or `--target ruchy`"
                    .into(),
            ));
        }
        // PMAT-459 (v0.2.0 Track 1.B): Lean's `.length` returns Nat;
        // coerce to Int via `(... : Int)` ascription.
        Expr::Len(inner) => {
            out.push_str("((");
            emit_expr(out, inner)?;
            out.push_str(").length : Int)");
        }
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => emit_if_expr(out, cond, then_expr, else_expr)?,
        Expr::Call { callee, args } => emit_call(out, callee, args)?,
        Expr::UnOp { op, operand } => emit_unop(out, *op, operand)?,
        // PMAT-449 (v0.2.0 Track 1.A): Python `str` literal → Lean
        // `String` literal. Lean's string syntax is the same as
        // Rust's, but the escape set is slightly more restricted at
        // v0.2.0 first pass.
        Expr::LitStr(s) => {
            write!(out, "\"{}\"", escape_lean_str(s))?;
        }
        // PMAT-042: `QuotedString` stays bashrs-only; Lean refuses.
        Expr::QuotedString { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Lean backend does not lower Expr::QuotedString — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs quoted shell strings; \
                 use `--target shell`"
                    .into(),
            ));
        }
        // PMAT-045: see twin arm above.
        Expr::ShellVar(name) => {
            return Err(LeanCodegenError::Unsupported(format!(
                "Lean backend does not lower Expr::ShellVar (${name}) — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs shell variable refs"
            )));
        }
        // PMAT-047: same disposition.
        Expr::CommandSubstitution(_) => {
            return Err(LeanCodegenError::Unsupported(
                "Lean backend does not lower Expr::CommandSubstitution — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs shell substitution"
                    .into(),
            ));
        }
        // PMAT-055: same disposition.
        Expr::ShellSpecial(name) => {
            return Err(LeanCodegenError::Unsupported(format!(
                "Lean backend does not lower Expr::ShellSpecial (${name}) — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs shell special params"
            )));
        }
    }
    Ok(())
}

/// Lean 4 `if c then a else b` — an expression.
fn emit_if_expr(
    out: &mut String,
    cond: &Expr,
    then_expr: &Expr,
    else_expr: &Expr,
) -> Result<(), LeanCodegenError> {
    write!(out, "if ")?;
    emit_expr(out, cond)?;
    write!(out, " then ")?;
    emit_expr(out, then_expr)?;
    write!(out, " else ")?;
    emit_expr(out, else_expr)?;
    Ok(())
}

/// Lean calls use juxtaposition: `(f a b)`, not `f(a, b)`.
fn emit_call(out: &mut String, callee: &str, args: &[Expr]) -> Result<(), LeanCodegenError> {
    write!(out, "({callee}")?;
    for a in args {
        write!(out, " ")?;
        emit_expr(out, a)?;
    }
    write!(out, ")")?;
    Ok(())
}

fn emit_unop(out: &mut String, op: UnOp, operand: &Expr) -> Result<(), LeanCodegenError> {
    let sym = match op {
        UnOp::Neg => "-",
        UnOp::Not => "!",
    };
    write!(out, "({sym}")?;
    emit_expr(out, operand)?;
    write!(out, ")")?;
    Ok(())
}

/// Binary ops:
///   - Arithmetic add/sub/mul: `+ - *`
///   - Comparisons: `== != < <= > >=`
///   - Logical: `&& ||`
///   - Python-floor division and modulo: `Int.fdiv` / `Int.fmod`
///     (NOT Lean's `/` and `%` on Int — those use T-division and
///     diverge from Python on negative operands.)
fn emit_binop(out: &mut String, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<(), LeanCodegenError> {
    match op {
        BinOp::Add => emit_infix(out, lhs, " + ", rhs),
        BinOp::Sub => emit_infix(out, lhs, " - ", rhs),
        BinOp::Mul => emit_infix(out, lhs, " * ", rhs),
        BinOp::Eq => emit_infix(out, lhs, " == ", rhs),
        BinOp::NotEq => emit_infix(out, lhs, " != ", rhs),
        BinOp::Lt => emit_infix(out, lhs, " < ", rhs),
        BinOp::LtEq => emit_infix(out, lhs, " <= ", rhs),
        BinOp::Gt => emit_infix(out, lhs, " > ", rhs),
        BinOp::GtEq => emit_infix(out, lhs, " >= ", rhs),
        BinOp::And => emit_infix(out, lhs, " && ", rhs),
        BinOp::Or => emit_infix(out, lhs, " || ", rhs),
        BinOp::FloorDiv => emit_prefix2(out, "Int.fdiv", lhs, rhs),
        BinOp::Mod => emit_prefix2(out, "Int.fmod", lhs, rhs),
        // Bitwise: Lean 4 core provides Int.land / Int.lor / Int.xor for
        // the bool-ops and Int has HShiftLeft / HShiftRight instances
        // taking Nat. We coerce rhs via `.toNat` for shifts (matches
        // Python's "shift amount must be non-negative" check; if rhs is
        // negative the resulting toNat is 0, which differs from Python's
        // ValueError — leaving as a known Lean fidelity gap, callable
        // from any equivalence theorem against the Rust emission via the
        // `C-PY-INT-ARITH` contract).
        BinOp::BitAnd => emit_prefix2(out, "Int.land", lhs, rhs),
        BinOp::BitOr => emit_prefix2(out, "Int.lor", lhs, rhs),
        BinOp::BitXor => emit_prefix2(out, "Int.xor", lhs, rhs),
        BinOp::Shl => emit_shift(out, lhs, "<<<", rhs),
        BinOp::Shr => emit_shift(out, lhs, ">>>", rhs),
        // Lean's `^` is `HPow.hPow`. For `Int`, the standard library
        // resolves `(a : Int) ^ (n : Nat) : Int` — coerce rhs via .toNat,
        // same trade-off as shifts (negative exponent silently → 0).
        BinOp::Pow => emit_shift(out, lhs, "^", rhs),
    }
}

fn emit_shift(out: &mut String, lhs: &Expr, op: &str, rhs: &Expr) -> Result<(), LeanCodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs)?;
    write!(out, " {op} (")?;
    emit_expr(out, rhs)?;
    write!(out, ").toNat)")?;
    Ok(())
}

fn emit_infix(out: &mut String, lhs: &Expr, op: &str, rhs: &Expr) -> Result<(), LeanCodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs)?;
    out.push_str(op);
    emit_expr(out, rhs)?;
    write!(out, ")")?;
    Ok(())
}

fn emit_prefix2(
    out: &mut String,
    name: &str,
    lhs: &Expr,
    rhs: &Expr,
) -> Result<(), LeanCodegenError> {
    write!(out, "({name} ")?;
    emit_expr(out, lhs)?;
    write!(out, " ")?;
    emit_expr(out, rhs)?;
    write!(out, ")")?;
    Ok(())
}

pub struct LeanBackend;

impl Backend for LeanBackend {
    fn name(&self) -> &'static str {
        "lean"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Lean]
    }

    fn lower(&self, module: &Module, _config: &BackendConfig) -> Result<Artifact, BackendError> {
        let primary = emit_module(module).map_err(|e| BackendError::Lower(e.to_string()))?;
        Ok(Artifact {
            primary,
            sidecars: Vec::new(),
            citations: Vec::new(),
            quorum_status: QuorumStatus::Single {
                emitter: "xpile-lean-codegen".to_string(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xpile_meta_hir::{Module, SourceLang};

    fn module_with(name: &str, items: Vec<Item>) -> Module {
        Module {
            name: name.into(),
            source_lang: SourceLang::Python,
            items,
            ffi_boundaries: Vec::new(),
        }
    }

    fn add_fn() -> Function {
        Function {
            name: "add".into(),
            params: vec![
                Param {
                    name: "a".into(),
                    ty: Type::I64,
                    mutable: false,
                },
                Param {
                    name: "b".into(),
                    ty: Type::I64,
                    mutable: false,
                },
            ],
            return_type: Type::I64,
            body: Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(Expr::Ident("a".into())),
                rhs: Box::new(Expr::Ident("b".into())),
            }
            .into(),
        }
    }

    #[test]
    fn emits_def_with_int_signature() {
        let m = module_with("fixture", vec![Item::Function(add_fn())]);
        let lean = emit_module(&m).expect("emit ok");
        assert!(
            lean.contains("def add (a : Int) (b : Int) : Int :="),
            "expected Lean def signature in:\n{lean}"
        );
        assert!(!lean.contains("pub fn"));
        assert!(!lean.contains("fun "));
    }

    #[test]
    fn floordiv_uses_int_fdiv_not_division_operator() {
        let f = Function {
            name: "fdiv".into(),
            params: vec![
                Param {
                    name: "a".into(),
                    ty: Type::I64,
                    mutable: false,
                },
                Param {
                    name: "b".into(),
                    ty: Type::I64,
                    mutable: false,
                },
            ],
            return_type: Type::I64,
            body: Expr::BinOp {
                op: BinOp::FloorDiv,
                lhs: Box::new(Expr::Ident("a".into())),
                rhs: Box::new(Expr::Ident("b".into())),
            }
            .into(),
        };
        let m = module_with("fixture", vec![Item::Function(f)]);
        let lean = emit_module(&m).expect("emit ok");
        assert!(
            lean.contains("Int.fdiv"),
            "Python `//` must use Int.fdiv (floor semantics): got\n{lean}"
        );
        assert!(!lean.contains(" / "));
    }

    #[test]
    fn emits_call_via_juxtaposition_not_paren_form() {
        let f = Function {
            name: "caller".into(),
            params: vec![Param {
                name: "x".into(),
                ty: Type::I64,
                mutable: false,
            }],
            return_type: Type::I64,
            body: Expr::Call {
                callee: "double".into(),
                args: vec![Expr::Ident("x".into())],
            }
            .into(),
        };
        let m = module_with("fixture", vec![Item::Function(f)]);
        let lean = emit_module(&m).expect("emit ok");
        assert!(
            lean.contains("(double x)"),
            "expected `(double x)` juxtaposition form, got:\n{lean}"
        );
        assert!(!lean.contains("double(x)"));
    }
}
