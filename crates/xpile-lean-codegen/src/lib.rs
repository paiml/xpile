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
//!                          ⚠ PMAT-1394: BOTH only when the divisor is a
//!                          PROVABLY-NONZERO literal. Lean is total, so
//!                          `Int.fdiv a 0` is `0` and `Int.fmod a 0` is `a`
//!                          where Python raises ZeroDivisionError — and
//!                          `lean` exits 0, so the divergence is silent. Any
//!                          other divisor (runtime, or the literal `0`) is
//!                          REFUSED; same for float `/`, `//` and `%`. See
//!                          `reject_zero_divisor`.
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
    BinOp, Block, Expr, FloatOp, Function, Item, Module, Param, SourceLang, Stmt, Type, UnOp,
};

#[derive(Debug, thiserror::Error)]
pub enum LeanCodegenError {
    #[error("unsupported item: {0}")]
    Unsupported(String),
    #[error("formatting error: {0}")]
    Format(#[from] std::fmt::Error),
}

/// PMAT-1418: refuse any source language this backend was not written for.
///
/// The surface mapping at the top of this file is **Python → Lean**, and it is
/// the only one implemented: Python `int` is unbounded, so `Int` is faithful,
/// and Python `//` floors, so `Int.fdiv` is faithful. Nothing enforced that.
/// `emit_module` READ `module.source_lang` — to print it into the header
/// comment — and then emitted Python semantics regardless, so every other
/// frontend that reached this backend got a **silent wrong answer** rather
/// than a refusal. Measured at `e617d97c` on `tests/fixtures/c_int_arith.c`,
/// comparing this backend against xpile's own C-honest Rust emit of the same
/// source:
///
/// | expression      | `--target rust` (C semantics) | `--target lean` (this backend) |
/// |-----------------|-------------------------------|--------------------------------|
/// | `half(-7)`      | `-3` (C `/` truncates)        | `-4`  — `Int.fdiv` **floors**  |
/// | `poly(50000)`   | `-1794867295` (i32 wraps)     | `2500100001` — `Int` is unbounded |
/// | `factorial(13)` | `1932053504` (i32 wraps)      | does not elaborate: `fail to show termination`, so Lean falls back to the **`sorry` axiom** |
///
/// The first two are silent — `lean` exits 0 and prints a different number.
/// The third is worse in kind: the PROOF lane emitted a module resting on
/// `sorry`, in a repo whose contract claim is zero real `sorry` and zero
/// `axiom`. All three were stamped `@[xpile_contract "C-PY-INT-ARITH"]` — the
/// *Python* contract, whose own YAML (`contracts/c-int-arith-v1.yaml:10`)
/// says the C one is "Distinct from C-PY-INT-ARITH (Python int)". The Rust
/// and Ruchy backends have real C paths and cite `C-C-INT-ARITH`.
///
/// `Shell` is refused here too, and that is not redundant with the per-`Stmt`
/// shell refusals below: those guard the *statements*, so an **item-less**
/// shell module (a comment-only or empty `.sh`) had no statement to refuse and
/// fell through the loop to emit a header-only "Lean module" at exit 0 — a
/// negative over an empty enumeration passes for free.
///
/// The match is deliberately exhaustive with no `_` arm: a new `SourceLang`
/// variant must make an explicit decision here rather than silently inheriting
/// Python semantics, which is precisely how `C` and `Wasm` arrived.
fn reject_non_python_source(module: &Module) -> Result<(), LeanCodegenError> {
    let mismatch = match module.source_lang {
        // The implemented surface. Python `int` is unbounded and `//` floors,
        // which is exactly what `Int` / `Int.fdiv` mean.
        SourceLang::Python => return Ok(()),
        SourceLang::C | SourceLang::Cpp | SourceLang::Cuda => {
            "C `int` is 32-bit and WRAPS on overflow, and C `/` truncates toward zero, \
             while Lean `Int` is arbitrary-precision and this backend emits `Int.fdiv`, \
             which FLOORS — so `half(-7)` yields -4 here and -3 in C. \
             Contract C-C-INT-ARITH (not C-PY-INT-ARITH) governs C integer semantics \
             and this backend has no path that satisfies it"
        }
        SourceLang::Wasm => {
            "WASM `i32`/`i64` are fixed-width and WRAP on overflow, while Lean `Int` is \
             arbitrary-precision, so an overflowing lifted function evaluates to a \
             different value here than it does under `wasm-interp`"
        }
        SourceLang::Rust => {
            "Rust integers are fixed-width and panic or wrap on overflow, while Lean \
             `Int` is arbitrary-precision"
        }
        SourceLang::Shell => {
            "shell has no typed integer surface and contract C-BASHRS-POSIX-IDEMPOTENCE \
             governs shell constructs; an item-less shell module would otherwise emit an \
             empty Lean module at exit 0"
        }
        SourceLang::Ruchy => {
            "the Ruchy frontend has no parser, so no Ruchy module is faithful here"
        }
        SourceLang::Lean => {
            "a Lean source module is not re-emitted through the Python→Lean surface; \
             the Lean code lane lowers to Rust instead"
        }
    };
    Err(LeanCodegenError::Unsupported(format!(
        "Lean backend does not lower a {:?} source module — {mismatch}. \
         The Python→Lean surface is the only one implemented; emitting it for \
         {:?} would produce a SILENT WRONG VALUE rather than a refusal. \
         Use `--target rust` (which has a real {:?} path) instead.",
        module.source_lang, module.source_lang, module.source_lang
    )))
}

pub fn emit_module(module: &Module) -> Result<String, LeanCodegenError> {
    reject_non_python_source(module)?;
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
            // PMAT-502bj: module-level constant → `def NAME : T := value`.
            Item::Const { name, ty, value } => {
                write!(out, "def {name} : ")?;
                emit_type(&mut out, ty)?;
                out.push_str(" := ");
                emit_expr(&mut out, value)?;
                out.push('\n');
            }
            // PMAT-505a: a dataclass→struct has no first-cut Lean encoding (a
            // Lean `structure` lift is deferred); refuse like other deferred
            // Lean constructs.
            Item::Struct { name, .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "class/dataclass `{name}` → Lean `structure` is not yet supported — use `--target rust` or `--target ruchy`"
                )));
            }
            // PMAT-513: an `Enum` class → Lean `inductive` is deferred; refuse.
            Item::Enum { name, .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "enum `{name}` → Lean `inductive` is not yet supported — use `--target rust` or `--target ruchy`"
                )));
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

/// PMAT-011 / PMAT-1405: emit the applicable contracts as a Lean **docstring**
/// attached to the declaration — `/-- xpile-contract: <ID>[, <ID>]* -/`.
///
/// WHY A DOCSTRING AND NOT `@[xpile_contract "<ID>"]` (PMAT-1405, 2026-07-27).
/// The attribute form was chosen for the citation grid in
/// `sub/contract-frontend-trait.md` because the INTENT is a *structured* form
/// Lean's elaborator resolves BY NAME, rather than a regex over body text. That
/// intent was never realised and the cost was paid on every default invocation:
///
///   * `xpile_contract` is registered as a Lean attribute NOWHERE, so `lean`
///     rejected the emit with a PARSE error (`unexpected token; expected ']'`).
///     `--contracts on` is the CLI DEFAULT, so `xpile transpile x.py --target
///     lean` exited 0 writing a file its own toolchain could not read — and
///     `--target lean` was the only backend whose default output was not
///     consumable by the tool it targets.
///   * The promised name resolution therefore returned NOTHING: no elaborated
///     environment ever contained the citation, because no such file elaborated.
///
/// A docstring delivers the intent the attribute only promised, and does it
/// today with no prelude:
///
///   * it PARSES and elaborates standalone — MEASURED, not asserted, by
///     `crates/xpile/tests/lean_default_emit_witness.rs`, which runs `lean` on
///     the DEFAULT (`--contracts on`) emit plus `by decide` obligations;
///   * it is STRUCTURED and resolvable BY DECLARATION NAME out of the elaborated
///     environment via `Lean.findDocString? env `f` — the same witness asserts
///     that retrieval returns the citation text, so the grid's structured claim
///     is now backed by Lean's own API rather than by a regex over the source;
///   * the emit stays SELF-CONTAINED. The previously-planned fix (register the
///     attribute in a separately-imported prelude — an in-file
///     `registerBuiltinAttribute` fails) would have forced an `import` into
///     every emitted file and cost that standalone-ness.
///
/// So the docstring strictly dominates both the status quo and the prelude
/// plan, and is not the "switch to a comment" that `audit-design.md` §7 rejected
/// — a line comment is invisible to the elaborated environment, which is exactly
/// the property being preserved here.
///
/// SCOPE: this is the CODE lane (`--target lean`, meta-HIR → Lean `def`) only.
/// The CONTRACT-RENDERING lane (`xpile-lean-contract-backend`, contract YAML →
/// Lean theorem text) keeps `@[xpile_contract …]`: that form is specified by
/// `contracts/xlate-rust-fn-to-lean-thm-v1.yaml` and
/// `contracts/xpile-contract-backend-trait-v1.yaml`, and its output is never
/// elaborated as a live attribute (`contracts/lean/*` mention it in PROSE only),
/// so it has no parse failure to fix.
///
/// Lean permits at most ONE docstring per declaration — two stacked `/-- … -/`
/// blocks are a parse error (measured) — so multiple applicable contracts are
/// emitted as one comma-separated docstring, not one block each.
fn emit_contract_citations(out: &mut String, f: &Function) -> Result<(), LeanCodegenError> {
    let ids = f.applicable_contracts();
    if ids.is_empty() {
        return Ok(());
    }
    writeln!(out, "/-- xpile-contract: {} -/", ids.join(", "))?;
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
            // PMAT-494b: tuple unpacking inside a while — the Lean lane
            // does not support tuples at v0.2.0.
            Stmt::LetTuple { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has tuple unpacking inside a while loop; the Lean lane does not support tuples at v0.2.0",
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
            Stmt::FileWrite { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has file I/O (Stmt::FileWrite) inside a while loop; the Lean lane has no effectful file model",
                    f.name
                )));
            }
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
            Stmt::ShellIf { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has Stmt::ShellIf inside a while loop; \
                     C-BASHRS-POSIX-IDEMPOTENCE governs shell conditionals — \
                     Lean codegen does not lower them",
                    f.name
                )));
            }
            Stmt::ShellCase { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has Stmt::ShellCase inside a while loop; \
                     C-BASHRS-POSIX-IDEMPOTENCE governs shell case statements — \
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
            // PMAT-495: paired for-loop inside a while — same gap.
            Stmt::ForEachPair { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has Stmt::ForEachPair (enumerate/zip) inside a while loop; \
                     the Lean lane does not support paired for-loops at v0.2.0",
                    f.name
                )));
            }
            // PMAT-562: three-way zip inside a while — same gap.
            Stmt::ForEachZip3 { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has Stmt::ForEachZip3 (3-way zip) inside a while loop; \
                     the Lean lane does not support paired/zip for-loops at v0.2.0",
                    f.name
                )));
            }
            // PMAT-460: list.append() inside a while loop — same
            // monadic-encoding gap as ForEach. Deferred. PMAT-502ap/aq/ar:
            // in-place list mutators (.sort/.reverse/.clear) + .extend + .insert.
            // PMAT-1016A: a statement-position side-effect call (mutating
            // method / void fn) — same monadic-encoding gap.
            Stmt::SideEffectCall { .. }
            | Stmt::ListAppend { .. }
            | Stmt::SetAdd { .. }
            | Stmt::SetRemove { .. }
            | Stmt::ListMutate { .. }
            | Stmt::ListExtend { .. }
            | Stmt::DictUpdate { .. }
            | Stmt::ListInsert { .. }
            | Stmt::IndexAppend { .. }
            | Stmt::DictSetdefaultAppend { .. }
            | Stmt::ListRemoveValue { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has in-place mutation (.append/.add/.remove/.discard/.sort/.reverse/.clear/.extend/.insert/.update) inside a while loop; \
                     Lean codegen at v0.2.0 first cut doesn't compose in-place mutation with while",
                    f.name
                )));
            }
            // PMAT-461/730: indexed / nested-subscript assignment inside a while
            // loop — same in-place-mutation gap.
            Stmt::IndexAssign { .. }
            | Stmt::NestedSubscriptAssign { .. }
            | Stmt::FieldIndexAssign { .. } => {
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
            // PMAT-506c: struct field assignment — the Lean lane refuses structs
            // (see emit_stmt); refuse here too.
            Stmt::FieldAssign { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has Stmt::FieldAssign (struct field assignment) inside a while loop; \
                     struct values are not supported in the Lean lane",
                    f.name
                )));
            }
            // PMAT-502at: del coll[key] inside a while loop — same gap.
            Stmt::DelItem { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has Stmt::DelItem (del coll[key]) inside a while loop; \
                     Lean codegen at v0.2.0 first cut doesn't compose in-place mutation with while",
                    f.name
                )));
            }
            // PMAT-503a: `raise` is unsupported in the Lean lane entirely
            // (panic has no total-function encoding); refuse it here too.
            Stmt::Raise { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has a `raise` inside a while loop; \
                     Python exceptions are not supported in the Lean lane",
                    f.name
                )));
            }
            // PMAT-1058: statement-form try/except — same no-panic-model refusal.
            Stmt::TryCatch { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` has a `try`/`except` inside a while loop; \
                     Python exceptions are not supported in the Lean lane",
                    f.name
                )));
            }
            // PMAT-504: a closure binding inside a while loop is unsupported.
            Stmt::ClosureLet { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` binds a closure inside a while loop; \
                     first-class functions are not supported in the Lean lane",
                    f.name
                )));
            }
            // PMAT-736: a named inner fn inside a while loop is unsupported.
            Stmt::NestedFn { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` defines a nested function inside a while loop; \
                     nested functions are not supported in the Lean lane",
                    f.name
                )));
            }
            // PMAT-502bk: loop-control inside a while loop is unsupported.
            Stmt::Continue | Stmt::Break => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` uses `continue`/`break`; loop control is not \
                     supported in the Lean lane",
                    f.name
                )));
            }
            // PMAT-502bw: `print(...)` is an IO effect; pure Lean `def`s
            // have no IO, so the Lean lane refuses it.
            Stmt::Print { .. } => {
                return Err(LeanCodegenError::Unsupported(format!(
                    "function `{}` calls `print(...)`; the Lean lane has no IO in pure `def`s",
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
        Expr::LitInt(_) | Expr::LitBool(_) | Expr::LitFloat(_) | Expr::Unit => {}
        // PMAT-502dt: block-expr — recurse into the trailing value. (Lean
        // refuses block-exprs at emit; this is only reached by ident scans.)
        Expr::Block(b) => collect_idents(&b.trailing_return, out),
        // PMAT-477 (R8): float arithmetic — recurse into operands.
        Expr::FloatBinOp { lhs, rhs, .. } => {
            collect_idents(lhs, out);
            collect_idents(rhs, out);
        }
        Expr::BinOp { lhs, rhs, .. }
        | Expr::Concat { lhs, rhs }
        | Expr::ListConcat { lhs, rhs } => {
            collect_idents(lhs, out);
            collect_idents(rhs, out);
        }
        // PMAT-745: int/float exact comparison — recurse into both operands.
        Expr::MixedIntFloatCmp { int, float, .. } => {
            collect_idents(int, out);
            collect_idents(float, out);
        }
        // PMAT-502bh: str.format — recurse into each formatted arg.
        Expr::StrFormat { args, .. } => {
            for a in args {
                collect_idents(a, out);
            }
        }
        // PMAT-502am: formatted f-string field — recurse into the value.
        Expr::FormatSpec { value, .. } => collect_idents(value, out),
        // PMAT-502cd: `s[i]` over a string — recurse into both operands.
        Expr::StrCharAt { string, index } => {
            collect_idents(string, out);
            collect_idents(index, out);
        }
        // PMAT-502cl: string chars — recurse into the string expr.
        Expr::StrChars { string } => collect_idents(string, out),
        // PMAT-502cm: ord/chr — recurse into the value expr.
        Expr::Ord { value } | Expr::Chr { value } => collect_idents(value, out),
        // PMAT-502cv: hex/oct/bin — recurse into the value expr.
        // PMAT-939: thousands-grouping `f"{n:,}"` — recurse into the value expr.
        // PMAT-940: grouped-float `f"{x:,.Nf}"` — recurse into the value expr.
        // PMAT-941: scientific-float `f"{x:e}"` — recurse into the value expr.
        Expr::IntRadixStr { value, .. }
        | Expr::IntGroupedStr { value, .. }
        | Expr::FloatGroupedStr { value, .. }
        | Expr::FloatSciStr { value, .. }
        // PMAT-965: general-float `f"{x:g}"` — recurse into the value expr.
        | Expr::FloatGeneralStr { value, .. }
        | Expr::SpaceSignStr { value, .. } => collect_idents(value, out),
        // PMAT-502da: int(s, base) — recurse into the value expr.
        Expr::IntFromStrRadix { value, .. } => collect_idents(value, out),
        // PMAT-492: string method — recurse into the receiver + args.
        Expr::StrMethod { recv, args, .. } => {
            collect_idents(recv, out);
            for a in args {
                collect_idents(a, out);
            }
        }
        // PMAT-494: tuple literal — recurse into each element.
        Expr::TupleLit(elems) => {
            for e in elems {
                collect_idents(e, out);
            }
        }
        // PMAT-502q: tuple constant-index — recurse into the tuple expr.
        Expr::TupleIndex { tuple, .. } => collect_idents(tuple, out),
        // PMAT-496: slice — recurse into collection + bound expressions.
        // PMAT-502r: bounds are optional (open-ended slices).
        Expr::Slice {
            collection, lo, hi, ..
        } => {
            collect_idents(collection, out);
            if let Some(lo) = lo {
                collect_idents(lo, out);
            }
            if let Some(hi) = hi {
                collect_idents(hi, out);
            }
        }
        // PMAT-498: numeric builtin — recurse into each arg.
        Expr::NumBuiltin { args, .. } => {
            for a in args {
                collect_idents(a, out);
            }
        }
        // PMAT-498b: sum — recurse into the list expression.
        // PMAT-502cx: also recurse into the optional `start`.
        Expr::Sum { list, start, .. } => {
            collect_idents(list, out);
            if let Some(start) = start {
                collect_idents(start, out);
            }
        }
        // PMAT-502j: all/any — recurse into the bool list expression.
        Expr::BoolReduce { list, .. } => collect_idents(list, out),
        // PMAT-502k: seq * n — recurse into both sequence and count.
        Expr::Repeat { seq, n, .. } => {
            collect_idents(seq, out);
            collect_idents(n, out);
        }
        // PMAT-502m: int(x)/float(x) — recurse into the converted value.
        Expr::NumCast { value, .. } => collect_idents(value, out),
        // PMAT-502ad: str(x) — recurse into the converted value.
        Expr::ToStr { value, .. } | Expr::ReprStr { value } => collect_idents(value, out),
        // PMAT-502ak: round(x) — recurse into the rounded value.
        Expr::RoundToInt { value } => collect_idents(value, out),
        // PMAT-502al/PMAT-612: round(x, n) — recurse into value + ndigits.
        Expr::RoundToDigits { value, ndigits } | Expr::RoundIntToDigits { value, ndigits } => {
            collect_idents(value, out);
            collect_idents(ndigits, out);
        }
        // PMAT-502c: sorted — recurse into the list expression.
        Expr::Sorted { list, .. } => collect_idents(list, out),
        // PMAT-502d: reversed — recurse into the list expression.
        Expr::Reversed { list } => collect_idents(list, out),
        // PMAT-549: gcd — recurse into both operands.
        Expr::Gcd { a, b } | Expr::Lcm { a, b } => {
            collect_idents(a, out);
            collect_idents(b, out);
        }
        Expr::Comb { n, k } | Expr::Perm { n, k } => {
            collect_idents(n, out);
            collect_idents(k, out);
        }
        Expr::PowMod { base, exp, modulus } => {
            collect_idents(base, out);
            collect_idents(exp, out);
            collect_idents(modulus, out);
        }
        Expr::Factorial { n } | Expr::Isqrt { n } => collect_idents(n, out),
        // PMAT-502cj: list(range(...)) — recurse into the bound exprs.
        Expr::RangeList { start, stop, .. } => {
            collect_idents(start, out);
            collect_idents(stop, out);
        }
        // PMAT-502cw: set(xs) — recurse into the list expr.
        Expr::SetFromList { list } => collect_idents(list, out),
        Expr::SetToList { set } => collect_idents(set, out),
        // PMAT-502dk: dict(pairs) — recurse into the pairs list expr.
        Expr::DictFromPairs { pairs } => collect_idents(pairs, out),
        Expr::DictMerge { entries } => {
            for (k, v) in entries {
                if let Some(key) = k {
                    collect_idents(key, out);
                }
                collect_idents(v, out);
            }
        }
        // PMAT-502ab: filter — recurse into the list and predicate body.
        Expr::Filter { list, lambda } => {
            collect_idents(list, out);
            collect_idents(&lambda.body, out);
        }
        // PMAT-502ac: map — recurse into the list and transform body.
        Expr::Map { list, lambda } => {
            collect_idents(list, out);
            collect_idents(&lambda.body, out);
        }
        // PMAT-502ai: enumerate/zip — recurse into the source list(s).
        Expr::Enumerate { list, .. } => collect_idents(list, out),
        Expr::Zip { left, right } => {
            collect_idents(left, out);
            collect_idents(right, out);
        }
        // PMAT-502e: min/max reduction — recurse into the list expression.
        // PMAT-502dh: also recurse into the optional `default`.
        Expr::ListMinMax { list, default, .. } => {
            collect_idents(list, out);
            if let Some(d) = default {
                collect_idents(d, out);
            }
        }
        // PMAT-502u: list query — recurse into the list and the arg.
        Expr::ListQuery { list, arg, .. } => {
            collect_idents(list, out);
            collect_idents(arg, out);
        }
        // PMAT-502as: list pop — recurse into the list and optional index.
        Expr::ListPop { list, index } => {
            collect_idents(list, out);
            if let Some(i) = index {
                collect_idents(i, out);
            }
        }
        // PMAT-502au: dict pop — recurse into dict, key, optional default.
        Expr::DictPop { dict, key, default } => {
            collect_idents(dict, out);
            collect_idents(key, out);
            if let Some(d) = default {
                collect_idents(d, out);
            }
        }
        // PMAT-502ax: dict setdefault — recurse into dict, key, default.
        Expr::DictSetDefault { dict, key, default } => {
            collect_idents(dict, out);
            collect_idents(key, out);
            collect_idents(default, out);
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
        // PMAT-500: set literal / membership — recurse into sub-exprs.
        Expr::SetLit(elems) => {
            for e in elems {
                collect_idents(e, out);
            }
        }
        Expr::SetContains { set, elem } => {
            collect_idents(set, out);
            collect_idents(elem, out);
        }
        // PMAT-502an: list membership — recurse into both sides.
        Expr::ListContains { list, elem } => {
            collect_idents(list, out);
            collect_idents(elem, out);
        }
        // PMAT-502o: str substring containment — recurse into both sides.
        Expr::StrContains { haystack, needle } => {
            collect_idents(haystack, out);
            collect_idents(needle, out);
        }
        // PMAT-502g/ep: set algebra / predicates — recurse into both operands.
        Expr::SetOp { lhs, rhs, .. } | Expr::SetPred { lhs, rhs, .. } => {
            collect_idents(lhs, out);
            collect_idents(rhs, out);
        }
        // PMAT-502eq: shallow copy — recurse into the cloned value.
        Expr::Clone(inner) => collect_idents(inner, out),
        // PMAT-502ew: Option wrapper — recurse into the `Some(e)` payload.
        Expr::OptionExpr(inner) => {
            if let Some(e) = inner {
                collect_idents(e, out);
            }
        }
        // PMAT-721: Optional truthiness — recurse into the tested value (the body
        // is synthetic `__v`/literals). Lean refuses Optional at emit anyway.
        Expr::OptionTruthy { value, .. } => collect_idents(value, out),
        // PMAT-724: `x or default` over Optional — recurse into value + default.
        Expr::OptionOrDefault { value, default, .. } => {
            collect_idents(value, out);
            collect_idents(default, out);
        }
        // PMAT-502ex: `is None` test — recurse into the tested value.
        Expr::IsNone { value, .. } => collect_idents(value, out),
        // PMAT-502ez: flow-narrowed unwrap — recurse into the operand.
        Expr::OptionUnwrap(inner) => collect_idents(inner, out),
        // PMAT-503b: try/except — recurse into both body and handler.
        Expr::TryCatch { body, handler, .. } => {
            collect_idents(body, out);
            collect_idents(handler, out);
        }
        // PMAT-506b: struct literal / field access — recurse into the values.
        Expr::StructLit { fields, .. } => {
            for (_, v) in fields {
                collect_idents(v, out);
            }
        }
        Expr::FieldAccess { obj, .. } => collect_idents(obj, out),
        // PMAT-506d: method call — recurse into receiver + args.
        Expr::MethodCall { obj, args, .. } => {
            collect_idents(obj, out);
            for a in args {
                collect_idents(a, out);
            }
        }
        // PMAT-513: an enum member access references no local idents.
        Expr::EnumVariant { .. } => {}
        // PMAT-457: indexed access — recurse into both sides.
        Expr::Index { collection, index } => {
            collect_idents(collection, out);
            collect_idents(index, out);
        }
        // PMAT-466: dict ops — recurse into all sub-expressions so
        // loop-state ident discovery stays correct even though Lean
        // emit refuses these constructs downstream.
        Expr::DictGet { dict, key }
        | Expr::DictContains { dict, key }
        | Expr::DictGetOpt { dict, key } => {
            collect_idents(dict, out);
            collect_idents(key, out);
        }
        // PMAT-502v: dict view — recurse into the dict expression.
        Expr::DictView { dict, .. } => collect_idents(dict, out),
        Expr::DictGetOr { dict, key, default } => {
            collect_idents(dict, out);
            collect_idents(key, out);
            collect_idents(default, out);
        }
        // PMAT-459: len(x) — recurse into inner.
        Expr::Len(inner) => collect_idents(inner, out),
        Expr::FileReadAll(inner) => collect_idents(inner, out),
        Expr::FileReadLines(inner) => collect_idents(inner, out),
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
        // PMAT-909: a C `long`/`int64_t` (distinct 64-bit-ABI width). Lean's
        // `Int` is unbounded — same total-function shape as I64.
        Type::CLong => out.push_str("Int"),
        // PMAT-918: a C `unsigned`/`uint32_t` (distinct 32-bit UNSIGNED width)
        // is non-negative, so its faithful Lean shape is `Nat` — the
        // value-restricted analogue of `Int` for the unsigned width (the C ABI
        // distinction `c_uint` lives in xpile-ffi-manifest's `c_abi_type`).
        Type::CUInt => out.push_str("Nat"),
        // PMAT-921: a C `unsigned long`/`uint64_t` (distinct 64-bit UNSIGNED
        // width) is likewise non-negative → `Nat` (the unbounded non-negative
        // shape; the 64-bit C ABI distinction `c_ulonglong` lives in
        // xpile-ffi-manifest's `c_abi_type`).
        Type::CULong => out.push_str("Nat"),
        // PMAT-477 (R8): Python `float` → Lean `Float`.
        Type::F64 => out.push_str("Float"),
        // PMAT-911: a C `float` (32-bit). Lean's `Float` is its only float
        // shape (64-bit IEEE) — same total-function encoding as F64.
        Type::F32 => out.push_str("Float"),
        Type::Bool => out.push_str("Bool"),
        // Lean's Int is already unbounded — same shape as BigInt.
        Type::BigInt => out.push_str("Int"),
        // PMAT-502bl: a void (`None`-returning) function is side-effecting
        // — no total-function encoding in the Lean lane.
        Type::Unit => {
            return Err(LeanCodegenError::Unsupported(
                "Python `None`-returning (void) functions are not supported in the Lean lane — \
                 use `--target rust` or `--target ruchy`"
                    .into(),
            ))
        }
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
        // PMAT-500: Python sets deferred in the Lean lane at first cut.
        Type::Set(_) => {
            return Err(LeanCodegenError::Unsupported(
                "Python sets (set[T]) are not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-494: Python tuples deferred in the Lean lane at first cut
        // (Prod encoding + multi-return shape follow) — refuse with a
        // pointer, like the other capability-ahead-of-Lean refusals.
        Type::Tuple(_) => {
            return Err(LeanCodegenError::Unsupported(
                "Python tuples (tuple[...]) are not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-046: bashrs-domain types refused.
        Type::ShellString | Type::ExitCode => {
            return Err(LeanCodegenError::Unsupported(format!(
                "Lean code backend does not lower {t:?} — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs the bashrs type domain"
            )));
        }
        // PMAT-502ew: `Optional[T]` deferred in the Lean lane at first cut
        // (the wrapping-returns shape composes with early-return support that
        // Lean doesn't yet have) — refuse with a pointer.
        Type::Optional(_) => {
            return Err(LeanCodegenError::Unsupported(
                "Python `Optional[T]` is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-506b: struct values have no first-cut Lean encoding.
        Type::Struct(name) => {
            return Err(LeanCodegenError::Unsupported(format!(
                "struct type `{name}` (class/dataclass) is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
            )));
        }
        // PMAT-924: a raw C pointer / C `char` has no total-function Lean model
        // — a pointer is an address (mutable, aliasable runtime state), not a
        // pure value. Refuse, like the other capability-ahead-of-Lean types.
        Type::Ptr { .. } | Type::CChar => {
            return Err(LeanCodegenError::Unsupported(format!(
                "C pointer / `char` type {t:?} has no total-function Lean encoding \
                 (a pointer is mutable aliasable runtime state) — use `--target rust` \
                 or `--target ruchy`"
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
/// Shape on `safe_div`, formerly the asserted.py fixture. PMAT-1394 NOTE: this
/// exact program no longer lowers — its `a // b` has a runtime divisor, and an
/// `assert b != 0` does NOT discharge that obligation on this lane, because a
/// Lean `panic!` returns the type's default instead of aborting (`lean` still
/// exits 0). The shape below is retained because it documents the assert
/// CHAIN, which is unchanged for divisor-free bodies.
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
        Stmt::Assert { cond, .. } => {
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
        // PMAT-502bk: loop control has no encoding in the Lean lane.
        Stmt::Continue | Stmt::Break => Err(LeanCodegenError::Unsupported(
            "`continue`/`break` (loop control) is not lowered by the Lean backend — \
             use `--target rust` or `--target ruchy`"
                .into(),
        )),
        // PMAT-502bw: `print(...)` is an IO effect; pure Lean `def`s have no
        // IO, so the Lean backend refuses it.
        Stmt::Print { .. } => Err(LeanCodegenError::Unsupported(
            "`print(...)` is not lowered by the Lean backend (no IO in pure `def`s) — \
             use `--target rust` or `--target ruchy`"
                .into(),
        )),
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
        // PMAT-494b: tuple unpacking — the Lean lane does not support
        // tuples at v0.2.0 (refuse with a pointer, like Stmt::If/Return).
        Stmt::LetTuple { .. } => Err(LeanCodegenError::Unsupported(
            "Stmt::LetTuple (tuple unpacking) is not lowered by the Lean backend — \
             tuples are unsupported at v0.2.0; use `--target rust` or `--target ruchy`"
                .into(),
        )),
        // PMAT-504: first-class closures are a v0.3.0 Lean sub-track.
        Stmt::ClosureLet { .. } => Err(LeanCodegenError::Unsupported(
            "Stmt::ClosureLet (first-class closure) is not lowered by the Lean backend — \
             use `--target rust` or `--target ruchy`"
                .into(),
        )),
        // PMAT-736: named inner fns (nested functions) are a v0.3.0 Lean sub-track.
        Stmt::NestedFn { .. } => Err(LeanCodegenError::Unsupported(
            "Stmt::NestedFn (named inner function) is not lowered by the Lean backend — \
             use `--target rust` or `--target ruchy`"
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
        // PMAT-495: paired for-loop (enumerate / zip) — same monadic gap.
        Stmt::ForEachPair { .. } => Err(LeanCodegenError::Unsupported(
            "`for a, b in enumerate(xs)`/`zip(...)` (Stmt::ForEachPair) is not supported in \
             the Lean lane — use `--target rust` or `--target ruchy`"
                .into(),
        )),
        // PMAT-562: three-way zip — same monadic gap.
        Stmt::ForEachZip3 { .. } => Err(LeanCodegenError::Unsupported(
            "`for a, b, c in zip(x, y, z)` (Stmt::ForEachZip3) is not supported in \
             the Lean lane — use `--target rust` or `--target ruchy`"
                .into(),
        )),
        // PMAT-1016A: a statement-position side-effect call — a mutating
        // user-class method (`c.bump()`) or a void fn call. Lean is pure;
        // same state-monad gap as ListAppend.
        Stmt::SideEffectCall { .. } => Err(LeanCodegenError::Unsupported(
            "a statement-position side-effect call (Stmt::SideEffectCall — mutating method or \
             void function call) requires state-monad encoding in Lean — \
             use `--target rust` or `--target ruchy`"
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
        // PMAT-500b: set mutation — same monadic-encoding gap.
        Stmt::SetAdd { set_name, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{set_name}.add(...)` (Stmt::SetAdd) requires state-monad encoding in Lean — \
             use `--target rust` or `--target ruchy`"
        ))),
        // PMAT-502av: set remove/discard — same monadic-encoding gap.
        Stmt::SetRemove { set_name, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{set_name}.remove(x)`/`.discard(x)` (Stmt::SetRemove) requires state-monad \
             encoding in Lean — use `--target rust` or `--target ruchy`"
        ))),
        // PMAT-502ap: in-place list mutators — same monadic-encoding gap.
        Stmt::ListMutate { list_name, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{list_name}.sort()/.reverse()/.clear()` (Stmt::ListMutate) requires state-monad \
             encoding in Lean — use `--target rust` or `--target ruchy` for in-place mutation"
        ))),
        // PMAT-502aq: list.extend() — same monadic-encoding gap.
        Stmt::ListExtend { list_name, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{list_name}.extend(...)` (Stmt::ListExtend) requires state-monad encoding in Lean — \
             use `--target rust` or `--target ruchy` for in-place mutation"
        ))),
        // PMAT-502bb: dict.update() — same monadic-encoding gap.
        Stmt::DictUpdate { dict_name, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{dict_name}.update(...)` (Stmt::DictUpdate) requires state-monad encoding in Lean — \
             use `--target rust` or `--target ruchy` for in-place mutation"
        ))),
        // PMAT-502ar: list.insert() — same monadic-encoding gap.
        Stmt::ListInsert { list_name, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{list_name}.insert(i, x)` (Stmt::ListInsert) requires state-monad encoding in Lean — \
             use `--target rust` or `--target ruchy` for in-place mutation"
        ))),
        // PMAT-502eg: list.remove(value) — same monadic-encoding gap.
        Stmt::ListRemoveValue { list_name, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{list_name}.remove(x)` (Stmt::ListRemoveValue) requires state-monad encoding in Lean — \
             use `--target rust` or `--target ruchy` for in-place mutation"
        ))),
        // PMAT-461 (v0.2.0 Track 1.B): indexed assignment — same
        // monadic-encoding gap as ListAppend / ForEach.
        Stmt::IndexAssign { list_name, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{list_name}[i] = v` (Stmt::IndexAssign) requires state-monad encoding in Lean — \
             not yet implemented at v0.2.0 first cut (PMAT-461 follow-up); \
             use `--target rust` or `--target ruchy` for in-place mutation"
        ))),
        // PMAT-730: nested subscript assign — same in-place-mutation gap.
        Stmt::NestedSubscriptAssign { base, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{base}[a][b] = v` (Stmt::NestedSubscriptAssign) requires state-monad encoding in \
             Lean — not yet implemented; use `--target rust` or `--target ruchy`"
        ))),
        // PMAT-1037: field subscript store — same in-place-mutation gap.
        Stmt::FieldIndexAssign { obj, field, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{obj}.{field}[k] = v` (Stmt::FieldIndexAssign) requires state-monad encoding in \
             Lean — not yet implemented; use `--target rust` or `--target ruchy`"
        ))),
        // PMAT-466 (v0.2.0 Track 1.C): dict keyed assignment — same
        // state-monad encoding gap as IndexAssign / ListAppend.
        Stmt::DictSet { dict_name, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{dict_name}[k] = v` (Stmt::DictSet) requires state-monad encoding in Lean — \
             not yet implemented at v0.2.0 first cut (PMAT-466 follow-up); \
             use `--target rust` or `--target ruchy` for in-place mutation"
        ))),
        // PMAT-533: subscript-receiver append — same in-place-mutation gap.
        Stmt::IndexAppend { base, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{base}[i].append(e)` (Stmt::IndexAppend) requires state-monad encoding in Lean — \
             not yet implemented at v0.2.0 first cut; \
             use `--target rust` or `--target ruchy` for in-place mutation"
        ))),
        // PMAT-727: setdefault-append — same in-place-mutation gap.
        Stmt::DictSetdefaultAppend { dict, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{dict}.setdefault(k, d).append(e)` (Stmt::DictSetdefaultAppend) requires state-monad \
             encoding in Lean — not yet implemented at v0.2.0; use `--target rust`/`--target ruchy`"
        ))),
        // PMAT-506c: struct field assignment — struct values are deferred in
        // the Lean lane.
        Stmt::FieldAssign { obj, field, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`{obj}.{field} = v` (Stmt::FieldAssign) over a struct/dataclass is not supported \
             in the Lean lane — use `--target rust` or `--target ruchy`"
        ))),
        // PMAT-502at: del coll[key] — same state-monad encoding gap.
        Stmt::DelItem { name, .. } => Err(LeanCodegenError::Unsupported(format!(
            "`del {name}[k]` (Stmt::DelItem) requires state-monad encoding in Lean — \
             use `--target rust` or `--target ruchy` for in-place mutation"
        ))),
        // Stmt::Assert is handled by emit_stmts_then_trailing — should
        // never reach this match arm. The unreachable here catches a
        // future refactor that bypasses the recursive emit.
        Stmt::Assert { .. } => unreachable!(
            "Stmt::Assert handled in emit_stmts_then_trailing — emit_stmt called directly"
        ),
        // PMAT-503a: Python exceptions have no total-function encoding in
        // the Lean lane — `raise` is refused (use `--target rust`/`ruchy`).
        Stmt::Raise { .. } => Err(LeanCodegenError::Unsupported(
            "Stmt::Raise (Python `raise`) is not lowered by the Lean backend — \
             exceptions have no total-function encoding; use `--target rust` or `--target ruchy`"
                .into(),
        )),
        // PMAT-1058: statement-form try/except — no panic model in Lean.
        Stmt::TryCatch { .. } => Err(LeanCodegenError::Unsupported(
            "Stmt::TryCatch (Python `try`/`except`) is not lowered by the Lean backend — \
             exceptions have no total-function encoding; use `--target rust` or `--target ruchy`"
                .into(),
        )),
        // PMAT-039 / XPILE-BASHRS-MERGER-001 Layer B: Lean has no
        // notion of shell-command invocation. `C-BASHRS-POSIX-IDEMPOTENCE`
        // governs `Stmt::Cmd`; any cross-domain refinement would lower
        // via the bashrs domain, not the Lean one.
        Stmt::FileWrite { .. } => Err(LeanCodegenError::Unsupported(
            "Lean backend does not lower file I/O (`open(p, \"w\").write(...)`, Stmt::FileWrite) — it has no effectful file model".into(),
        )),
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
        Stmt::ShellIf { .. } => Err(LeanCodegenError::Unsupported(
            "Lean backend does not lower Stmt::ShellIf — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell conditionals; \
             use `--target shell`"
                .into(),
        )),
        Stmt::ShellCase { .. } => Err(LeanCodegenError::Unsupported(
            "Lean backend does not lower Stmt::ShellCase — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell case statements; \
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
        // PMAT-502eq: a shallow copy of an immutable Lean value is the value
        // itself — emit the inner expression directly.
        Expr::Clone(inner) => emit_expr(out, inner)?,
        // PMAT-502ew/ex: `Optional` values + `None` tests deferred in the Lean
        // lane (see emit_type).
        Expr::OptionExpr(_)
        | Expr::OptionTruthy { .. }
        | Expr::OptionOrDefault { .. }
        | Expr::IsNone { .. }
        | Expr::OptionUnwrap(_) => {
            return Err(LeanCodegenError::Unsupported(
                "Python `Optional`/`None` values + `is None` tests + flow-narrowed unwraps are not \
                 yet supported in the Lean lane — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-503b: try/except maps to `catch_unwind`, which has no Lean
        // (panic-free) model — refuse, like the other panic-based constructs.
        Expr::TryCatch { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python `try`/`except` lowers to Rust `catch_unwind` (a panic-recovery construct) \
                 with no Lean counterpart — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-506b: struct construction / field access — no first-cut Lean
        // encoding (struct values are deferred in the Lean lane).
        Expr::StructLit { .. }
        | Expr::FieldAccess { .. }
        | Expr::MethodCall { .. }
        | Expr::EnumVariant { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "struct construction / field access / method calls / enum members (class/dataclass/enum values) are not \
                 yet supported in the Lean lane — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502bl: void functions are refused at emit_type, so a unit
        // value should never reach here; refuse defensively.
        Expr::Unit => {
            return Err(LeanCodegenError::Unsupported(
                "Python `None` / unit value is not supported in the Lean lane".into(),
            ))
        }
        // PMAT-502dt: block-exprs (multi-statement closure bodies) are deferred
        // in the Lean lane.
        Expr::Block(_) => {
            return Err(LeanCodegenError::Unsupported(
                "block expressions (multi-statement closure bodies) are not supported in the \
                 Lean lane — use `--target rust` or `--target ruchy`"
                    .into(),
            ))
        }
        Expr::Ident(name) => write!(out, "{}", name)?,
        Expr::LitInt(v) => write!(out, "({}: Int)", v)?,
        // PMAT-477 (R8): Python `float` → Lean `Float` literal +
        // plain-infix arithmetic (Lean `Float` supports `+ - * /`).
        Expr::LitFloat(v) => write!(out, "({}: Float)", v)?,
        Expr::FloatBinOp { op, lhs, rhs } => match op {
            // PMAT-502br: Python float floor-division → `Float.floor (a / b)`.
            // PMAT-1394: divides, so the divisor must be provably nonzero.
            FloatOp::FloorDiv => {
                reject_zero_divisor(rhs, "//", "Float.floor (a / 0.0)` is `inf")?;
                out.push_str("(Float.floor (");
                emit_expr(out, lhs)?;
                out.push_str(" / ");
                emit_expr(out, rhs)?;
                out.push_str("))");
            }
            // PMAT-502br: Python float modulo → `a - b * Float.floor (a / b)`.
            // PMAT-1394: divides, so the divisor must be provably nonzero.
            FloatOp::Mod => {
                reject_zero_divisor(rhs, "%", "a - 0.0 * Float.floor (a / 0.0)` is `NaN")?;
                out.push('(');
                emit_expr(out, lhs)?;
                out.push_str(" - ");
                emit_expr(out, rhs)?;
                out.push_str(" * Float.floor (");
                emit_expr(out, lhs)?;
                out.push_str(" / ");
                emit_expr(out, rhs)?;
                out.push_str("))");
            }
            // PMAT-502bt: Python float power → `Float.pow a b`.
            FloatOp::Pow => {
                out.push_str("(Float.pow (");
                emit_expr(out, lhs)?;
                out.push_str(") (");
                emit_expr(out, rhs)?;
                out.push_str("))");
            }
            // PMAT-502en: the 2-arg math float methods are deferred in the Lean
            // lane (no clean `Float.hypot`/`atan2`/`log`-base mapping).
            FloatOp::Hypot | FloatOp::Atan2 | FloatOp::Log => {
                return Err(LeanCodegenError::Unsupported(
                    "`math.hypot`/`math.atan2`/`math.log(x, base)` are not supported in the \
                     Lean lane — use `--target rust` or `--target ruchy`"
                        .to_string(),
                ));
            }
            FloatOp::Add | FloatOp::Sub | FloatOp::Mul | FloatOp::Div => {
                // PMAT-1394: only `Div` divides; the other three are total.
                if matches!(op, FloatOp::Div) {
                    reject_zero_divisor(rhs, "/", "a / 0.0` is `inf")?;
                }
                let sym = match op {
                    FloatOp::Add => "+",
                    FloatOp::Sub => "-",
                    FloatOp::Mul => "*",
                    FloatOp::Div => "/",
                    FloatOp::FloorDiv
                    | FloatOp::Mod
                    | FloatOp::Pow
                    | FloatOp::Hypot
                    | FloatOp::Atan2
                    | FloatOp::Log => unreachable!(),
                };
                out.push('(');
                emit_expr(out, lhs)?;
                write!(out, " {sym} ")?;
                emit_expr(out, rhs)?;
                out.push(')');
            }
        },
        // PMAT-456 (v0.2.0 Track 1.B): bool literal — Lean
        // capitalises the constructors (`True` / `False`).
        Expr::LitBool(b) => write!(out, "{}", if *b { "True" } else { "False" })?,
        Expr::BinOp { op, lhs, rhs } => emit_binop(out, *op, lhs, rhs)?,
        // PMAT-745: the exact int/float comparison lowers to an `i128`-tiebreak
        // block in the Rust/Ruchy lanes; the Lean lane has no `f64`/`i128`
        // bit-level model, so refuse with a pointer (mirrors FloatBinOp's posture).
        Expr::MixedIntFloatCmp { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "exact int/float comparison is not supported in the Lean lane — \
                 use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
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
        // PMAT-973: Python list `xs + ys` → Lean `(xs ++ ys)`. `++` on
        // `List T` is `List.append`, a TOTAL core-Lean function (no
        // Mathlib, no panic) — the direct list-side companion of
        // `Expr::Concat` (string `++`, already emitted above). Python's
        // `+` builds a fresh list and mutates neither operand; Lean
        // values are immutable, so `++` matches that semantics exactly.
        // Nested `list[list[T]]` (`List (List Int)`) is equally total.
        Expr::ListConcat { lhs, rhs } => {
            out.push('(');
            emit_expr(out, lhs)?;
            out.push_str(" ++ ");
            emit_expr(out, rhs)?;
            out.push(')');
        }
        // PMAT-502bh: str.format deferred in the Lean lane.
        Expr::StrFormat { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python str.format(...) is not yet supported in the Lean lane — \
                 use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502cd: string indexing `s[i]` is deferred in the Lean lane
        // (no stable char-vec model at first cut) — refuse with a pointer.
        Expr::StrCharAt { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python string indexing `s[i]` is not yet supported in the Lean lane — \
                 use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502cl: string iteration `for c in s` deferred in the Lean lane.
        Expr::StrChars { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python string iteration `for c in s` is not yet supported in the Lean lane — \
                 use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502cm: ord(c) / chr(n) deferred in the Lean lane.
        Expr::Ord { .. } | Expr::Chr { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python ord(c) / chr(n) are not yet supported in the Lean lane — \
                 use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502cv: hex/oct/bin deferred in the Lean lane.
        Expr::IntRadixStr { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python hex(n) / oct(n) / bin(n) are not yet supported in the Lean lane — \
                 use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-939: thousands-grouping f-string field deferred in the Lean lane.
        Expr::IntGroupedStr { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python thousands-grouping `f\"{n:,}\"` / `f\"{n:_}\"` is not yet supported in \
                 the Lean lane — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-940: grouped-float f-string field deferred in the Lean lane.
        Expr::FloatGroupedStr { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python grouped-float `f\"{x:,.2f}\"` / `f\"{x:_.2f}\"` is not yet supported in \
                 the Lean lane — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-941: scientific-float f-string field deferred in the Lean lane.
        Expr::FloatSciStr { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python scientific-float `f\"{x:e}\"` / `f\"{x:.2E}\"` is not yet supported in \
                 the Lean lane — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-965: general-float f-string field deferred in the Lean lane.
        Expr::FloatGeneralStr { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python general-float `f\"{x:g}\"` / `f\"{x:.3G}\"` is not yet supported in \
                 the Lean lane — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-942: space-sign numeric f-string field deferred in the Lean lane.
        Expr::SpaceSignStr { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python space-sign `f\"{x: d}\"` / `f\"{x: .2f}\"` is not yet supported in \
                 the Lean lane — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502da: int(s, base) deferred in the Lean lane.
        Expr::IntFromStrRadix { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python int(s, base) is not yet supported in the Lean lane — \
                 use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-492: Python string transform methods are deferred in the
        // Lean lane (no stable String.toUpper / trim model at first cut)
        // — refuse with a pointer, like the other str-domain refusals.
        Expr::StrMethod { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python string methods (.upper()/.lower()/.strip()) are not yet \
                 supported in the Lean lane — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502am: formatted f-string fields deferred in the Lean lane.
        Expr::FormatSpec { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python f-string format specs (`{x:.2f}`) are not yet supported in the \
                 Lean lane — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-494: tuple literals deferred in the Lean lane at first cut.
        Expr::TupleLit(_) => {
            return Err(LeanCodegenError::Unsupported(
                "Python tuple literals are not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502q: tuple indexing deferred in the Lean lane (tuples
        // are unsupported there at first cut).
        Expr::TupleIndex { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python tuple indexing (t[N]) is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-496: slicing deferred in the Lean lane at first cut.
        Expr::Slice { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python slicing (xs[a:b]) is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-498: numeric builtins deferred in the Lean lane at first cut.
        Expr::NumBuiltin { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python numeric builtins (abs/min/max) are not yet supported in the \
                 Lean lane — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-498b: sum deferred in the Lean lane at first cut.
        Expr::Sum { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python sum(xs) is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502j: all/any deferred in the Lean lane at first cut.
        Expr::BoolReduce { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python all(xs)/any(xs) is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502k: seq * n repetition deferred in the Lean lane.
        Expr::Repeat { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python seq * n (string/list repetition) is not yet supported in the \
                 Lean lane — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502m: int(x)/float(x) conversion deferred in the Lean lane
        // (Int↔Float coercion isn't modeled in the v0.1.0 Int-only subset).
        Expr::NumCast { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python int(x)/float(x) numeric conversion is not yet supported in the \
                 Lean lane — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502ad: str(x) deferred in the Lean lane at first cut.
        // PMAT-582: repr(str) likewise deferred in the Lean lane.
        Expr::ToStr { .. } | Expr::ReprStr { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python str(x) is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502ak/al/PMAT-612: round(x)/round(x, n) deferred in the Lean lane.
        Expr::RoundToInt { .. } | Expr::RoundToDigits { .. } | Expr::RoundIntToDigits { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python round(x)/round(x, n) is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502c: sorted deferred in the Lean lane at first cut.
        Expr::Sorted { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python sorted(xs) is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502d: reversed deferred in the Lean lane at first cut.
        Expr::Reversed { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python reversed(xs) is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-549: math.gcd deferred in the Lean lane (imperative Euclid loop).
        Expr::Gcd { .. }
        | Expr::Lcm { .. }
        | Expr::Factorial { .. }
        | Expr::Isqrt { .. }
        | Expr::Comb { .. }
        | Expr::Perm { .. }
        | Expr::PowMod { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "math.gcd/lcm/factorial/isqrt/comb/perm/3-arg-pow is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502cj: list(range(...)) deferred in the Lean lane.
        Expr::RangeList { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python list(range(...)) is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502cw/520: set(xs) / list(set) deferred in the Lean lane.
        Expr::SetFromList { .. } | Expr::SetToList { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python set(xs) / list(set) is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502dk: dict(pairs) deferred in the Lean lane.
        Expr::DictFromPairs { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python dict(pairs) is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502dw: dict merge `{**a, **b}` deferred in the Lean lane.
        Expr::DictMerge { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python dict merge `{**a, **b}` is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502ab: filter deferred in the Lean lane at first cut.
        Expr::Filter { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python filter(pred, xs) is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502ac: map deferred in the Lean lane at first cut.
        Expr::Map { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python map(f, xs) is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502ai: enumerate/zip deferred in the Lean lane at first cut.
        Expr::Enumerate { .. } | Expr::Zip { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python enumerate(xs)/zip(xs, ys) is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502e: min/max reduction deferred in the Lean lane at first cut.
        Expr::ListMinMax { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python min(xs)/max(xs) over a list is not yet supported in \
                 the Lean lane — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502as: list pop deferred in the Lean lane (in-place mutation).
        Expr::ListPop { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python list.pop() is not yet supported in the Lean lane \
                 (in-place mutation) — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502au: dict pop deferred in the Lean lane (in-place mutation).
        Expr::DictPop { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python dict.pop() is not yet supported in the Lean lane \
                 (in-place mutation) — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502ax: dict setdefault deferred in the Lean lane (mutation).
        Expr::DictSetDefault { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python dict.setdefault() is not yet supported in the Lean lane \
                 (in-place mutation) — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502u: list query methods deferred in the Lean lane at first cut.
        Expr::ListQuery { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python list.count(x)/index(x) is not yet supported in \
                 the Lean lane — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
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
        // PMAT-457 (v0.2.0 Track 1.B): Lean's `xs[i]!` syntax — panics on
        // out-of-range with a clear error message.
        //
        // PMAT-1426: the index must be RESOLVED Python-style FIRST. The
        // original emission was a bare `xs[i.toNat]!`, and `Int.toNat`
        // CLAMPS a negative to `0` — so `a[-1]`, which CPython reads as the
        // LAST element, silently returned the FIRST one at exit 0. Measured:
        // `f [10,20,30]` answered 10 where CPython answers 30. This is the
        // same clamping shape PMAT-1425 removed from `>>` / `**`, one
        // vocabulary over.
        //
        // Python's rule is `xs[i] == xs[len(xs) + i]` for `i < 0`, and
        // IndexError when the resolved index falls outside `0 ..< len`. Both
        // halves are spellable in core Lean with no monad and no Mathlib, so
        // this LOWERS FAITHFULLY rather than refusing (PMAT-1425 lesson 4:
        // scope a refusal to what you cannot spell). The upper bound is
        // already reported by `[...]!`'s own `outOfBounds` panic; only the
        // still-negative case (`[10,20,30][-5]`) needs an explicit guard,
        // because `.toNat` would clamp it back into range.
        //
        // Collection and index are each bound once so a compound operand is
        // not emitted twice. `let` shadowing is lexically scoped, so a nested
        // `a[b[i]]` reusing these names is correct (witnessed).
        Expr::Index { collection, index } => {
            out.push_str("(let __xc := ");
            emit_expr(out, collection)?;
            out.push_str("; let __xi : Int := ");
            emit_expr(out, index)?;
            out.push_str(
                "; let __xj : Int := (if __xi < 0 then ((__xc).length : Int) + __xi else __xi); \
                 if __xj < 0 then panic! \"xpile: IndexError: list index out of range\" \
                 else __xc[__xj.toNat]!)",
            );
        }
        // PMAT-466 (v0.2.0 Track 1.C): dict read / get-with-default /
        // membership. The `List (K × V)` first-cut model has no
        // panic-on-absent lookup or O(1) keyed access; a faithful
        // encoding needs the `Std.HashMap` upgrade that also unblocks
        // Lean iteration/mutation. Deferred to v0.3.0; refuse clearly.
        // PMAT-500: sets deferred in the Lean lane at first cut.
        Expr::SetLit(_) | Expr::SetContains { .. } | Expr::SetOp { .. } | Expr::SetPred { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python sets ({a, b} / `x in s` / `a | b` / `a & b` / `a - b` / `a ^ b` / \
                 subset/superset/disjoint predicates) are not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502o: str substring containment deferred in the Lean lane.
        Expr::StrContains { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python `sub in s` (string substring containment) is not yet supported in \
                 the Lean lane — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        // PMAT-502an: list membership deferred in the Lean lane at first cut.
        Expr::ListContains { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "Python `x in xs` (list membership) is not yet supported in the Lean lane \
                 — use `--target rust` or `--target ruchy`"
                    .to_string(),
            ));
        }
        Expr::DictGet { .. }
        | Expr::DictGetOr { .. }
        | Expr::DictGetOpt { .. }
        | Expr::DictContains { .. }
        | Expr::DictView { .. } => {
            return Err(LeanCodegenError::Unsupported(
                "dict operations (`d[k]`, `d.get(k, default)`, `k in d`, `d.keys()`/`d.values()`) \
                 require the Std.HashMap Lean encoding — not yet implemented at v0.2.0 first cut \
                 (PMAT-466 follow-up, alongside Lean iteration/mutation); \
                 use `--target rust` or `--target ruchy`"
                    .into(),
            ));
        }
        // PMAT-459 (v0.2.0 Track 1.B): Lean's `.length` returns Nat;
        // PMAT-1074: the Lean proof lane has no file-I/O model — refuse.
        Expr::FileReadAll(_) => {
            return Err(LeanCodegenError::Unsupported(
                "Lean backend does not lower file I/O (`open(path).read()`, Expr::FileReadAll) — it has no effectful file model".into(),
            ));
        }
        Expr::FileReadLines(_) => {
            return Err(LeanCodegenError::Unsupported(
                "Lean backend does not lower file I/O (`for line in open(path)`, Expr::FileReadLines) — it has no effectful file model".into(),
            ));
        }
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
    match op {
        // PMAT-502fb: Lean's unbounded `Int` has no `~` operator, but Python's
        // `~x` is the exact identity `-(x + 1)`, which is total over `Int`.
        UnOp::BitNot => {
            write!(out, "(-(")?;
            emit_expr(out, operand)?;
            write!(out, " + 1))")?;
        }
        UnOp::Neg | UnOp::Not => {
            let sym = if matches!(op, UnOp::Neg) { "-" } else { "!" };
            write!(out, "({sym}")?;
            emit_expr(out, operand)?;
            write!(out, ")")?;
        }
    }
    Ok(())
}

/// PMAT-1394: is `e` a divisor Lean can be *trusted* with — i.e. one this
/// backend can see is nonzero without any runtime information?
///
/// Only literals qualify. A negation of a provably-nonzero literal is itself
/// provably nonzero (`-2` arrives as `UnOp::Neg` over `LitInt(2)`, not as
/// `LitInt(-2)`, so the recursion is load-bearing, not decoration). Everything
/// else — an `Ident`, a call, any arithmetic subtree — is *not* provably
/// nonzero, because this backend performs no range analysis. Being wrong in
/// the permissive direction here is precisely the bug PMAT-1394 fixes, so the
/// answer is `false` by default.
fn provably_nonzero_divisor(e: &Expr) -> bool {
    match e {
        Expr::LitInt(v) => *v != 0,
        Expr::LitFloat(v) => *v != 0.0,
        Expr::UnOp {
            op: UnOp::Neg,
            operand,
        } => provably_nonzero_divisor(operand),
        _ => false,
    }
}

/// PMAT-1394: refuse a division whose divisor is not provably nonzero.
///
/// Python raises `ZeroDivisionError` for `x // 0`, `x % 0` and `x / 0.0`. Lean
/// is a TOTAL language: it has no exception to raise, so its division
/// primitives return a value instead — and every one of them disagrees with
/// Python. Measured against lean 4.15.0:
///
/// ```text
///   def f : Int   := (Int.fdiv (7: Int) (0: Int))     #eval f  =>  0     (Python: raises)
///   def g : Int   := (Int.fmod (7: Int) (0: Int))     #eval g  =>  7     (Python: raises)
///   def h : Float := ((1: Float) / (0: Float))        #eval h  =>  inf   (Python: raises)
/// ```
///
/// `lean` exits 0 on all three, so the divergence is SILENT. That matters more
/// on this lane than on any other: the Lean backend is the repo's
/// SEMANTIC-stratum oracle, the thing other correctness claims are justified
/// *by*.
///
/// The fix is a refusal, not a guard, for two measured reasons:
///
///  1. A Lean `panic!` does NOT produce a non-zero exit — it prints to stderr
///     and returns the type's default (so `Int` division would still answer
///     `0`). Porting the Rust backend's `panic!` guard would buy a stderr line
///     and no exit-code parity, while making the emission opaque to the
///     `by decide` semantic witness the lane depends on.
///  2. The refusal boundary is not invented here. `contracts/lean/PyIntArith.lean`
///     already states its floor-div and mod equivalence theorems under the
///     hypothesis `(hb : b ≠ 0)`. Emission for a possibly-zero divisor was
///     therefore emission OUTSIDE the region the lane's own contract proves.
///     Refusing exactly `¬ provably-nonzero` realigns the codegen with the
///     contract, and changes nothing about the code that still lowers — so no
///     `contracts/*.yaml` / Diamond-theorem resync is required.
///
/// The Rust and Ruchy lanes are unaffected: they emit an explicit
/// `panic!("xpile: ZeroDivisionError: ...")` guard and abort with a non-zero
/// status, which is why the message points there.
fn reject_zero_divisor(rhs: &Expr, py_op: &str, lean_result: &str) -> Result<(), LeanCodegenError> {
    if provably_nonzero_divisor(rhs) {
        return Ok(());
    }
    // Split the diagnostic by KIND: a literal `0` is a definite miscompile,
    // an unknown divisor is an unprovable one. Same refusal, different truth.
    let why = if matches!(rhs, Expr::LitInt(0)) || matches!(rhs, Expr::LitFloat(v) if *v == 0.0) {
        "the divisor is the literal `0`"
    } else {
        "the divisor is not a provably-nonzero literal, so it may be zero at runtime"
    };
    Err(LeanCodegenError::Unsupported(format!(
        "Python `{py_op}` raises ZeroDivisionError on a zero divisor, and {why}. Lean is total \
         and has no exception to raise: `{lean_result}`, and `lean` exits 0, so the emission \
         would DISAGREE with Python silently. This is the same `b ≠ 0` hypothesis \
         contracts/lean/PyIntArith.lean already requires of its floor-div and mod equivalence \
         theorems. Use `--target rust` or `--target ruchy` (both emit an explicit \
         ZeroDivisionError panic guard), or make the divisor a nonzero literal."
    )))
}

/// PMAT-1425: refuse a bitwise/shift operator that has no Lean 4 core
/// spelling, rather than emit a constant that does not exist.
///
/// Through v0.1.617 four `emit_binop` arms named constants and instances
/// that are simply absent from Lean 4 core. MEASURED against lean 4.15.0 —
/// each of these is the toolchain's own answer, not a reading of the docs:
///
/// ```text
///   #check @Int.land            =>  unknown constant 'Int.land'
///   #check @Int.lor             =>  unknown constant 'Int.lor'
///   #check @Int.xor             =>  unknown constant 'Int.xor'
///   #check @Int.shiftLeft       =>  unknown constant 'Int.shiftLeft'
///   #check (1:Int) <<< (2:Nat)  =>  failed to synthesize HShiftLeft Int Nat
///   #check (1:Int) &&& (2:Int)  =>  failed to synthesize
///   -- the one that DOES exist, and is why `>>` survives:
///   #check @Int.shiftRight      =>  Int.shiftRight : Int → Nat → Int
/// ```
///
/// So `xpile transpile x.py --target lean` exited **0** for `a & b`,
/// `a | b`, `a ^ b` and `a << b`, writing a file that `lean` then rejects
/// with `unknown constant` — under a `/-- xpile-contract: C-PY-INT-ARITH -/`
/// docstring claiming the construct is carried by a machine-checked
/// contract. No witness ever covered these four arms, which is how a
/// vocabulary the emitter cannot spell shipped for the whole 0.1.x line.
///
/// This is a REFUSAL and not a fix because there is nothing to emit: the
/// operation is absent from the target language's core, so there is no
/// coercion available that would not be an invention (PMAT-1395 — making
/// uncompilable output compile by guessing installs a silent wrong answer).
/// A Mathlib-gated path is a future increment; the Lean lane deliberately
/// imports nothing (all 36 `contracts/lean/*.lean` modules are core-only).
///
/// The boundary is not invented here either — the repo had ALREADY WRITTEN IT
/// DOWN, in the contract these emissions cited. `contracts/lean/PyIntArith.lean`
/// lists under "Not yet covered": *"Bitwise (`&` / `|` / `^`) — core Lean lacks
/// `Int.land/lor/xor`, so this needs either a mathlib dep or a hand-rolled
/// cast-through-Nat encoding. Tracked as XPILE-REFINE-005."* and
/// `sub/provability-roadmap.md` adds *"core Lean 4.15 doesn't auto-synthesise
/// the `HShiftLeft Int Nat` instance"* — which is the `<<` arm exactly. The
/// PROOF lane knew all four were absent and modelled shifts as `a * 2^b` to
/// avoid them; the CODE lane emitted them anyway, under a docstring citing
/// C-PY-INT-ARITH. Refusing here makes the two agree.
fn no_lean_spelling(py_op: &str, what: &str, attempted: &str) -> LeanCodegenError {
    LeanCodegenError::Unsupported(format!(
        "Python `{py_op}` ({what}) has no Lean 4 core spelling — the emission attempted \
         `{attempted}`, which lean 4.15.0 rejects (`Int.land`/`Int.lor`/`Int.xor`/\
         `Int.shiftLeft` are not constants and there is no `HShiftLeft Int Nat` \
         instance; only `Int.shiftRight` exists, which is why `>>` still lowers). \
         The Lean lane is core-only by design, so there is nothing faithful to emit \
         and it refuses rather than write a file that `lean` cannot elaborate. \
         Every other integer backend HAS this operator — use `--target rust` or \
         `--target ruchy` (both emit `a {py_op} b` directly)."
    ))
}

/// PMAT-1425: refuse `>>` / `**` whose right operand is not a provably
/// non-negative literal.
///
/// The sibling of [`reject_zero_divisor`], for the same reason and with the
/// same boundary. Both operators lower through `.toNat`, and `Int.toNat`
/// CLAMPS a negative to `0` rather than failing — so the count silently
/// becomes `0`. MEASURED against lean 4.15.0 and CPython 3:
///
/// ```text
///   1024 >> -1   lean: 1024   CPython: ValueError: negative shift count
///      2 ** -1   lean:    1   CPython: 0.5   (a Float — not even this type)
///      5 ** -2   lean:    1   CPython: 0.04
/// ```
///
/// `lean` exits 0 on all three, so — exactly as for a zero divisor — the
/// divergence is SILENT, and it is silent in the one lane whose purpose is
/// machine-checked semantics. The other lanes all report it: the Rust and
/// Ruchy emissions carry an explicit
/// `panic!("xpile: negative shift amount (Python ValueError: negative shift
/// count; contract C-PY-INT-ARITH)")` and a `u32::try_from(exp).expect("…
/// Python returns Float for negative exponents …")`, and the WASM lane traps
/// in `$__wasm_shr_i64` (PMAT-1379, "shift-COUNT honest"). All three cite
/// C-PY-INT-ARITH while doing it — the same contract this lane cited while
/// answering `1024`.
///
/// A NON-NEGATIVE count is faithful and keeps lowering; this was measured
/// across the edges rather than assumed, including the cases most likely to
/// diverge (negative base, oversized count, `0 ** 0`):
///
/// ```text
///   -5 >> 1  =>  -3      -1 >> 3  =>  -1      1024 >> 70  =>  0
///   (-2) ** 3  =>  -8     0 ** 0  =>  1       2 ** 70  =>  1180591620717411303424
/// ```
///
/// all identical in lean and CPython — so this refusal is scoped to the
/// operand that can diverge, not to the operator (PMAT-1419: over-refusal is
/// the natural failure mode of a refusal fix).
///
/// And, as for the divisor, the surviving region is the one the contract
/// already proves: `shl_fast_path_eq_slow_path` / `shr_fast_path_eq_slow_path`
/// / `pow_fast_path_eq_slow_path` in `contracts/lean/PyIntArith.lean` all take
/// the count as `(b : Nat)`, so emission for an arbitrary `Int` count was
/// emission OUTSIDE the region C-PY-INT-ARITH proves. Refusing exactly
/// `¬ provably-non-negative` realigns the codegen with the contract and changes
/// nothing about the code that still lowers — so, as with PMAT-1394, no
/// `contracts/*.yaml` / Diamond-theorem resync is required.
fn reject_possibly_negative_count(
    rhs: &Expr,
    py_op: &str,
    what: &str,
    lean_result: &str,
    py_consequence: &str,
) -> Result<(), LeanCodegenError> {
    if provably_nonnegative_count(rhs) {
        return Ok(());
    }
    // Split the diagnostic by KIND, as `reject_zero_divisor` does: a negative
    // literal is a definite miscompile, an unknown operand is an unprovable
    // one. Same refusal, different truth. `py_consequence` is passed in per
    // operator rather than listing both: a message that describes `**`'s
    // behaviour while refusing a `>>` misdescribes its own input.
    let why = if provably_negative_count(rhs) {
        format!("the {what} is a negative literal, so this is a definite miscompile")
    } else {
        format!(
            "the {what} is not a provably-non-negative literal, so it may be negative at runtime"
        )
    };
    Err(LeanCodegenError::Unsupported(format!(
        "Python `{py_op}` requires a non-negative {what} — with a negative one it \
         {py_consequence} — and {why}. Lean is total and has no exception to raise: \
         the lowering goes through `Int.toNat`, which CLAMPS a negative to `0`, so \
         {lean_result} and `lean` exits 0 — the emission would DISAGREE with Python \
         silently. This is the same boundary `reject_zero_divisor` draws for `//` and \
         `%`. Use `--target rust` or `--target ruchy` (both emit an explicit panic \
         guard naming the Python error), or make the {what} a non-negative literal."
    )))
}

/// The `>>` / `**` analogue of [`provably_nonzero_divisor`]. Deliberately
/// NOT a recursion through `UnOp::Neg`: negating is what makes a count
/// unusable here, so only `-0` survives it.
fn provably_nonnegative_count(e: &Expr) -> bool {
    match e {
        Expr::LitInt(v) => *v >= 0,
        Expr::UnOp {
            op: UnOp::Neg,
            operand,
        } => matches!(operand.as_ref(), Expr::LitInt(0)),
        _ => false,
    }
}

/// True when the operand is a literal the lane can see is NEGATIVE. Used only
/// to pick the diagnostic wording — an operand that is neither provably
/// non-negative nor provably negative is the "unknown at runtime" case.
fn provably_negative_count(e: &Expr) -> bool {
    match e {
        Expr::LitInt(v) => *v < 0,
        Expr::UnOp {
            op: UnOp::Neg,
            operand,
        } => matches!(operand.as_ref(), Expr::LitInt(v) if *v > 0),
        _ => false,
    }
}

/// Binary ops:
///   - Arithmetic add/sub/mul: `+ - *`
///   - Comparisons: `== != < <= > >=`
///   - Logical: `&& ||`
///   - Python-floor division and modulo: `Int.fdiv` / `Int.fmod`
///     (NOT Lean's `/` and `%` on Int — those use T-division and
///     diverge from Python on negative operands.) PMAT-1394: only emitted
///     for a provably-nonzero divisor; see `reject_zero_divisor`.
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
        // PMAT-1394: `//` and `%` only lower when the divisor is PROVABLY
        // nonzero. `Int.fdiv a 0` is `0` and `Int.fmod a 0` is `a` — total,
        // silent, and wrong against Python's ZeroDivisionError. See
        // `reject_zero_divisor`.
        BinOp::FloorDiv => {
            reject_zero_divisor(rhs, "//", "Int.fdiv a 0` is `0")?;
            emit_prefix2(out, "Int.fdiv", lhs, rhs)
        }
        BinOp::Mod => {
            reject_zero_divisor(rhs, "%", "Int.fmod a 0` is `a")?;
            emit_prefix2(out, "Int.fmod", lhs, rhs)
        }
        // PMAT-1425: bitwise AND/OR/XOR and `<<` have NO Lean 4 core
        // spelling at all. The comment this replaces asserted they did
        // ("Lean 4 core provides Int.land / Int.lor / Int.xor … and Int
        // has HShiftLeft / HShiftRight instances taking Nat") and was
        // never checked against a toolchain; see `NO_LEAN_SPELLING`.
        BinOp::BitAnd => Err(no_lean_spelling("&", "bitwise AND", "Int.land")),
        BinOp::BitOr => Err(no_lean_spelling("|", "bitwise OR", "Int.lor")),
        BinOp::BitXor => Err(no_lean_spelling("^", "bitwise XOR", "Int.xor")),
        BinOp::Shl => Err(no_lean_spelling("<<", "left shift", "a <<< b.toNat")),
        // `>>` and `**` DO have faithful Lean spellings — but only for a
        // NON-NEGATIVE count. `Int.toNat` clamps a negative to `0`, so a
        // negative count silently answers `a` / `1` where Python RAISES
        // (`>>`) or returns a Float (`**`). Same rule as the divisor:
        // emit only where the lane can prove the count is in range.
        BinOp::Shr => {
            reject_possibly_negative_count(
                rhs,
                ">>",
                "shift count",
                "`a >>> 0` is `a`",
                "raises `ValueError: negative shift count`",
            )?;
            emit_shift(out, lhs, ">>>", rhs)
        }
        // Lean's `^` is `HPow.hPow`; for `Int` the standard library
        // resolves `(a : Int) ^ (n : Nat) : Int`.
        BinOp::Pow => {
            reject_possibly_negative_count(
                rhs,
                "**",
                "exponent",
                "`a ^ 0` is `1`",
                "returns a Float (`2 ** -1` is `0.5`), which the emitted `Int` \
                 signature cannot represent",
            )?;
            emit_shift(out, lhs, "^", rhs)
        }
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

    fn lower(&self, module: &Module, config: &BackendConfig) -> Result<Artifact, BackendError> {
        let primary = emit_module(module).map_err(|e| BackendError::Lower(e.to_string()))?;
        Ok(Artifact {
            primary,
            sidecars: Vec::new(),
            citations: Vec::new(),
            quorum_status: QuorumStatus::Single {
                emitter: "xpile-lean-codegen".to_string(),
            },
        }
        .with_citations(config.emit_contracts))
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

    /// Helper for the PMAT-1394 divisor tests: `a <op> <rhs>` over one `I64`
    /// param `a`.
    fn int_div_module(op: BinOp, rhs: Expr) -> Module {
        let f = Function {
            name: "fdiv".into(),
            params: vec![Param {
                name: "a".into(),
                ty: Type::I64,
                mutable: false,
            }],
            return_type: Type::I64,
            body: Expr::BinOp {
                op,
                lhs: Box::new(Expr::Ident("a".into())),
                rhs: Box::new(rhs),
            }
            .into(),
        };
        module_with("fixture", vec![Item::Function(f)])
    }

    /// Helper for the PMAT-1394 float divisor tests: `a <op> <rhs>` over one
    /// `F64` param `a`.
    fn float_div_module(op: FloatOp, rhs: Expr) -> Module {
        let f = Function {
            name: "fdiv".into(),
            params: vec![Param {
                name: "a".into(),
                ty: Type::F64,
                mutable: false,
            }],
            return_type: Type::F64,
            body: Expr::FloatBinOp {
                op,
                lhs: Box::new(Expr::Ident("a".into())),
                rhs: Box::new(rhs),
            }
            .into(),
        };
        module_with("fixture", vec![Item::Function(f)])
    }

    #[test]
    fn floordiv_uses_int_fdiv_not_division_operator() {
        // PMAT-1394: the divisor is now a nonzero LITERAL. The original form
        // of this test divided by an `Ident` and is the exact shape that is
        // now refused (see `floordiv_by_runtime_divisor_is_refused`), but the
        // property it pinned — Python `//` must lower to `Int.fdiv` (floor
        // semantics), never Lean's T-division `/` — is unchanged and still
        // needs pinning on whatever DOES lower.
        let m = int_div_module(BinOp::FloorDiv, Expr::LitInt(3));
        let lean = emit_module(&m).expect("emit ok");
        assert!(
            lean.contains("Int.fdiv"),
            "Python `//` must use Int.fdiv (floor semantics): got\n{lean}"
        );
        assert!(!lean.contains(" / "));
    }

    /// PMAT-1394: `a % <nonzero literal>` still lowers — the guard must not
    /// swallow the whole operator.
    #[test]
    fn mod_by_nonzero_literal_still_lowers() {
        let m = int_div_module(BinOp::Mod, Expr::LitInt(2));
        let lean = emit_module(&m).expect("emit ok");
        assert!(
            lean.contains("Int.fmod"),
            "`a % 2` must still lower to Int.fmod: got\n{lean}"
        );
    }

    /// PMAT-1394: a NEGATED nonzero literal is provably nonzero. `-2` arrives
    /// as `UnOp::Neg` over `LitInt(2)`, so this is the case that fails if the
    /// recursion in `provably_nonzero_divisor` is dropped.
    #[test]
    fn floordiv_by_negated_nonzero_literal_still_lowers() {
        let m = int_div_module(
            BinOp::FloorDiv,
            Expr::UnOp {
                op: UnOp::Neg,
                operand: Box::new(Expr::LitInt(2)),
            },
        );
        let lean = emit_module(&m).expect("emit ok");
        assert!(
            lean.contains("Int.fdiv"),
            "`a // -2` must still lower: got\n{lean}"
        );
    }

    /// PMAT-1394 (the reproduced bug, int floor-div leg): `7 // 0` used to
    /// emit `(Int.fdiv (7: Int) (0: Int))`, which `lean` elaborates to `0` at
    /// exit 0 while Python raises ZeroDivisionError.
    #[test]
    fn floordiv_by_literal_zero_is_refused() {
        let m = int_div_module(BinOp::FloorDiv, Expr::LitInt(0));
        let err = emit_module(&m).expect_err("`a // 0` must be refused");
        let msg = err.to_string();
        assert!(
            msg.contains("ZeroDivisionError"),
            "refusal must name ZeroDivisionError: {msg}"
        );
        assert!(
            msg.contains("the divisor is the literal `0`"),
            "a literal zero must be diagnosed as a literal zero, not as unprovable: {msg}"
        );
    }

    /// PMAT-1394 (int mod leg): `7 % 0` used to elaborate to `7`.
    #[test]
    fn mod_by_literal_zero_is_refused() {
        let m = int_div_module(BinOp::Mod, Expr::LitInt(0));
        let err = emit_module(&m).expect_err("`a % 0` must be refused");
        assert!(err.to_string().contains("ZeroDivisionError"));
    }

    /// PMAT-1394: the SEVERE half. A runtime divisor cannot be shown nonzero
    /// by this backend, so it must refuse too — a guard that only catches the
    /// literal `0` is a guard for one ARGUMENT, not for the operator.
    #[test]
    fn floordiv_by_runtime_divisor_is_refused() {
        let m = int_div_module(BinOp::FloorDiv, Expr::Ident("b".into()));
        let err = emit_module(&m).expect_err("`a // b` must be refused");
        let msg = err.to_string();
        assert!(msg.contains("ZeroDivisionError"), "{msg}");
        assert!(
            msg.contains("may be zero at runtime"),
            "an unknown divisor must be diagnosed as unprovable: {msg}"
        );
    }

    /// PMAT-1394: a nonzero-looking ARITHMETIC divisor is still refused —
    /// `provably_nonzero_divisor` does no range analysis and must not pretend
    /// to. (`b - b` is the witness that constant folding would be unsound.)
    #[test]
    fn floordiv_by_arithmetic_divisor_is_refused() {
        let m = int_div_module(
            BinOp::FloorDiv,
            Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(Expr::LitInt(1)),
                rhs: Box::new(Expr::LitInt(1)),
            },
        );
        emit_module(&m).expect_err("`a // (1 + 1)` must be refused (no range analysis)");
    }

    /// PMAT-1394 (float legs). Lean measured at 4.15.0: `a / 0.0` => `inf`,
    /// `Float.floor (a / 0.0)` => `inf`, the float-mod formula => `NaN`; Python
    /// raises ZeroDivisionError for all three.
    #[test]
    fn float_division_by_literal_zero_is_refused() {
        for op in [FloatOp::Div, FloatOp::FloorDiv, FloatOp::Mod] {
            let m = float_div_module(op, Expr::LitFloat(0.0));
            let msg = emit_module(&m)
                .map(|lean| panic!("{op:?} by 0.0 must be refused, got:\n{lean}"))
                .unwrap_err()
                .to_string();
            assert!(msg.contains("ZeroDivisionError"), "{op:?}: {msg}");
            assert!(
                msg.contains("the divisor is the literal `0`"),
                "{op:?}: literal 0.0 must be diagnosed as a literal zero: {msg}"
            );
        }
    }

    /// PMAT-1394: float division by a runtime divisor is refused, and by a
    /// nonzero literal still lowers.
    #[test]
    fn float_division_divisor_boundary() {
        for op in [FloatOp::Div, FloatOp::FloorDiv, FloatOp::Mod] {
            let refused = float_div_module(op, Expr::Ident("b".into()));
            let msg = emit_module(&refused)
                .expect_err("float division by a runtime divisor must be refused")
                .to_string();
            assert!(msg.contains("ZeroDivisionError"), "{op:?}: {msg}");

            let ok = float_div_module(op, Expr::LitFloat(2.0));
            emit_module(&ok).unwrap_or_else(|e| panic!("{op:?} by 2.0 must still lower: {e}"));
        }
    }

    #[test]
    fn list_concat_emits_lean_append_operator() {
        // PMAT-973: Python `xs + ys` over two `list[int]` lowers to Lean
        // `(xs ++ ys)` — `List.append`, total core-Lean. This is the
        // list-side companion of the string `++` (Expr::Concat) emission.
        let f = Function {
            name: "cat".into(),
            params: vec![
                Param {
                    name: "xs".into(),
                    ty: Type::List(Box::new(Type::I64)),
                    mutable: false,
                },
                Param {
                    name: "ys".into(),
                    ty: Type::List(Box::new(Type::I64)),
                    mutable: false,
                },
            ],
            return_type: Type::List(Box::new(Type::I64)),
            body: Expr::ListConcat {
                lhs: Box::new(Expr::Ident("xs".into())),
                rhs: Box::new(Expr::Ident("ys".into())),
            }
            .into(),
        };
        let m = module_with("fixture", vec![Item::Function(f)]);
        let lean = emit_module(&m).expect("emit ok");
        assert!(
            lean.contains("def cat (xs : List (Int)) (ys : List (Int)) : List (Int) :="),
            "expected List Int signature, got:\n{lean}"
        );
        assert!(
            lean.contains("(xs ++ ys)"),
            "Python list `+` must emit Lean `++` (List.append): got\n{lean}"
        );
        // Must NOT still be a refusal.
        assert!(
            !lean.contains("not yet supported"),
            "ListConcat should no longer be refused: got\n{lean}"
        );
    }

    #[test]
    fn list_concat_over_literals_emits_append() {
        // `[1, 2] + [3]` → `([(1: Int), (2: Int)] ++ [(3: Int)])`.
        let f = Function {
            name: "lit_cat".into(),
            params: vec![],
            return_type: Type::List(Box::new(Type::I64)),
            body: Expr::ListConcat {
                lhs: Box::new(Expr::ListLit(vec![Expr::LitInt(1), Expr::LitInt(2)])),
                rhs: Box::new(Expr::ListLit(vec![Expr::LitInt(3)])),
            }
            .into(),
        };
        let m = module_with("fixture", vec![Item::Function(f)]);
        let lean = emit_module(&m).expect("emit ok");
        assert!(
            lean.contains("([(1: Int), (2: Int)] ++ [(3: Int)])"),
            "expected literal list append, got:\n{lean}"
        );
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
