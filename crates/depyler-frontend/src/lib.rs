//! Python frontend for xpile.
//!
//! Parses `.py` source with `rustpython-parser` and lowers a constrained
//! subset into meta-HIR. Anything outside the subset returns
//! `FrontendError::Lower` with a message naming the unsupported
//! construct.
//!
//! **The canonical subset description lives in [`/CHANGELOG.md`].** Keep
//! it in sync there; this docstring intentionally does not duplicate the
//! list to avoid the staleness it accumulated through PRs #7 … #21
//! (each subset extension updated lowering but not this comment).
//!
//! Known limitations (future work, kept here only because they are
//! load-bearing rejections in the lowering code):
//!   - `for` over non-range iterables (lists, dicts, generators) — the
//!     `for target in range(...)` shape desugars via PMAT-007; other
//!     iterables wait on collection types.
//!   - Statically-inferred BigInt promotion (analyzing operand bounds
//!     to decide if i64 suffices) is still a follow-up. PMAT-013 shipped
//!     *return-type-driven* promotion: annotate only `-> BigInt` and
//!     `int` params lift automatically. Without an explicit annotation,
//!     the i64 fast path still panics with a contract-naming message
//!     instead of silently wrapping.
//!   - Type annotations beyond `int` / `bool` / `BigInt`.
//!   - Lean backend for `assert` — needs Decidable instances + a
//!     propositional formulation; the `while` encoding shipped in
//!     PMAT-010 via `partial def` threaded-state recursion.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use xpile_frontend::{Frontend, FrontendError};
use xpile_meta_hir::{
    BinOp, Block, DictViewKind, Expr, FloatOp, Function, Item, ListQueryOp, Module, NumBuiltinOp,
    PairIterKind, Param, SetOp, SortKey, SourceLang, Stmt, StrMethodOp, Type, UnOp,
};

use rustpython_parser::ast;
use rustpython_parser::Parse;

/// State threaded through function-body lowering so the frontend can
/// (a) decide whether `name = expr` is a first binding (`Let`) or a
/// reassignment (`Assign`), (b) know up-front which `Let`s must be
/// emitted as `let mut` so the loop body can rewrite them (PMAT-006),
/// and (c) propagate `BigInt` typing through identifier references so
/// the slow path of `C-PY-INT-ARITH` (PMAT-012) is taken when the user
/// annotates any param as `BigInt`.
/// PMAT-471 (R2) + PMAT-474 (R5): a top-level function's signature,
/// collected in a module pre-pass. `ret` types cross-function calls
/// (R2); `params` (declared parameter names, in order) lets `f(x=1,
/// y=2)` keyword calls be reordered to positional at lowering (R5).
#[derive(Clone)]
struct FnSig {
    ret: Type,
    params: Vec<String>,
}

struct LoweringCtx {
    fn_name: String,
    /// The declared (or inferred-at-construction) return type of the
    /// enclosing function. Used to type self-recursive calls without a
    /// cross-function signature table.
    fn_return_type: Type,
    /// Names already bound in this scope — params, plus every `Let`
    /// emitted so far during this function's lowering. New Assigns to a
    /// name already in this set lower to `Stmt::Assign`.
    bound: HashSet<String>,
    /// `name → Type` for every bound name. Drives type inference
    /// through `Ident` references so BigInt-mode functions correctly
    /// propagate the slow-path type.
    name_types: HashMap<String, Type>,
    /// Names that are reassigned somewhere in the function body (and so
    /// must be emitted as `let mut`). Computed once via a pre-walk
    /// before any statement is lowered. Names assigned inside a loop
    /// body count as mutable even if the source has only one assign,
    /// because the runtime executes that assign repeatedly.
    mutable: HashSet<String>,
    /// PMAT-471 (R2): module-level signature table — every top-level
    /// function's declared return type, built in a pre-pass before any
    /// function is lowered. Consulted when typing `Expr::Call` so a
    /// call to *another* function (e.g. `d = make_dict()`) gets its real
    /// return type instead of the old hardcoded `Type::I64` fallback
    /// (which silently emitted `let d: i64` and broke rustc). Shared
    /// across all functions in the module via `Rc`.
    signatures: Rc<HashMap<String, FnSig>>,
}

impl LoweringCtx {
    fn new(
        fn_name: &str,
        fn_return_type: Type,
        params: &[Param],
        body: &[ast::Stmt],
        signatures: Rc<HashMap<String, FnSig>>,
    ) -> Self {
        let bound: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
        let name_types: HashMap<String, Type> = params
            .iter()
            .map(|p| (p.name.clone(), p.ty.clone()))
            .collect();
        let mutable = compute_mutable_names(params, body);
        Self {
            fn_name: fn_name.to_string(),
            fn_return_type,
            bound,
            name_types,
            mutable,
            signatures,
        }
    }
}

/// Pre-walk: compute the per-name source-assignment count, treating
/// `if`-branches as alternatives (the max of the two branches' counts
/// for each name, not the sum — only one branch executes) and `while`
/// bodies as repeated (everything inside counts as 2+ since the body
/// executes more than once).
///
/// `mutable(name) = total_count(name) > 1`, after also counting the
/// param binding as 1 for any param.
fn compute_mutable_names(params: &[Param], body: &[ast::Stmt]) -> HashSet<String> {
    let mut counts: HashMap<String, usize> = walk_counts(body, /*in_loop=*/ false);
    for p in &params.iter().map(|p| p.name.clone()).collect::<Vec<_>>() {
        *counts.entry(p.clone()).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .filter_map(|(name, c)| if c > 1 { Some(name) } else { None })
        .collect()
}

/// Recursive count: returns a fresh map of `name → count` produced by
/// `stmts`. If-branches merge by taking the max per name (alternatives,
/// not sequential). While bodies count assignments as 2× (executed
/// repeatedly). Sequential statements add counts per name.
fn walk_counts(stmts: &[ast::Stmt], in_loop: bool) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for stmt in stmts {
        match stmt {
            ast::Stmt::Assign(a) => {
                let bump = if in_loop { 2 } else { 1 };
                if let Some(name) = simple_assign_target_name(a) {
                    *counts.entry(name).or_insert(0) += bump;
                } else if let Some(name) = subscript_assign_base_name(a) {
                    // PMAT-466 (also fixes latent list case): `d[k] = v`
                    // / `xs[i] = v` mutate the base collection in place,
                    // so the binding must be emitted `mut`. The pre-pass
                    // didn't previously see subscript targets.
                    *counts.entry(name).or_insert(0) += bump;
                }
            }
            // PMAT-470 (R1): `x <op>= e` is a read-modify-write
            // reassignment → mutates `x`, so count it like an Assign.
            ast::Stmt::AugAssign(a) => {
                if let ast::Expr::Name(n) = a.target.as_ref() {
                    let bump = if in_loop { 2 } else { 1 };
                    *counts.entry(n.id.to_string()).or_insert(0) += bump;
                }
            }
            // PMAT-466: an annotated local binding counts exactly ONCE,
            // even inside a loop — each iteration re-binds a fresh
            // (shadowing) local; it does NOT mutate a prior binding, so
            // the loop-doubling that is correct for `Assign` reassignment
            // must not apply here. A genuine mutation of an annotated
            // dict (e.g. `d[k] = v`) is counted by the subscript-assign
            // arm above and still crosses the `> 1` threshold. Doubling
            // here would emit a spurious `let mut` for a never-mutated
            // annotated loop-local, which `clippy -D warnings` rejects.
            ast::Stmt::AnnAssign(aa) => {
                if let ast::Expr::Name(n) = aa.target.as_ref() {
                    *counts.entry(n.id.to_string()).or_insert(0) += 1;
                }
            }
            // PMAT-500b: an in-place mutation method call `<name>.add(x)`
            // / `<name>.append(x)` mutates the receiver, so the binding
            // must be `mut`. The pre-pass didn't previously see method
            // mutations (it relied on the lower-time `ctx.mutable.insert`,
            // which is too late for the receiver's own `let`).
            ast::Stmt::Expr(e) => {
                if let ast::Expr::Call(call) = e.value.as_ref() {
                    if let ast::Expr::Attribute(attr) = call.func.as_ref() {
                        if let ast::Expr::Name(recv) = attr.value.as_ref() {
                            if matches!(attr.attr.as_str(), "add" | "append") {
                                let bump = if in_loop { 2 } else { 1 };
                                *counts.entry(recv.id.to_string()).or_insert(0) += bump;
                            }
                        }
                    }
                }
            }
            ast::Stmt::If(if_stmt) => {
                let then_counts = walk_counts(&if_stmt.body, in_loop);
                let else_counts = walk_counts(&if_stmt.orelse, in_loop);
                let merged = merge_branch_counts(then_counts, else_counts);
                for (name, c) in merged {
                    *counts.entry(name).or_insert(0) += c;
                }
            }
            ast::Stmt::While(w) => {
                let inner = walk_counts(&w.body, /*in_loop=*/ true);
                for (name, c) in inner {
                    // `c` is already bumped 2× by in_loop=true; add it
                    // to the enclosing sequential total. A name assigned
                    // *only* once inside a loop still becomes mutable
                    // via that 2× bump.
                    *counts.entry(name).or_insert(0) += c;
                }
            }
            ast::Stmt::For(f) => {
                // The for-target is bound at loop entry AND reassigned
                // each iteration — count it as 2 even before the body
                // is examined. Body counts use in_loop=true (same as while).
                if let ast::Expr::Name(n) = &*f.target {
                    *counts.entry(n.id.to_string()).or_insert(0) += 2;
                }
                let inner = walk_counts(&f.body, /*in_loop=*/ true);
                for (name, c) in inner {
                    *counts.entry(name).or_insert(0) += c;
                }
            }
            _ => {}
        }
    }
    counts
}

/// Merge two branch maps by taking the per-name max — only one branch
/// runs, so the effective "this statement contributed N assigns to X"
/// is the worse case of the two. A name in only one branch counts as
/// max(N, 0) = N.
fn merge_branch_counts(
    then_counts: HashMap<String, usize>,
    else_counts: HashMap<String, usize>,
) -> HashMap<String, usize> {
    let mut out = then_counts;
    for (name, c) in else_counts {
        let entry = out.entry(name).or_insert(0);
        if c > *entry {
            *entry = c;
        }
    }
    out
}

/// Extract a non-zero integer literal step from a `range(start, stop, step)`
/// argument. Python represents negative literals as `UnaryOp(USub,
/// Constant(N))` rather than `Constant(-N)`, so we look through that
/// case explicitly. Returns None for any other shape, or for step == 0
/// (which Python itself raises ValueError on).
fn extract_step_literal(e: &ast::Expr) -> Option<i64> {
    fn as_positive_int_literal(e: &ast::Expr) -> Option<i64> {
        match e {
            ast::Expr::Constant(c) => match &c.value {
                ast::Constant::Int(n) => n.to_string().parse::<i64>().ok(),
                _ => None,
            },
            _ => None,
        }
    }
    match e {
        ast::Expr::UnaryOp(u) if matches!(u.op, ast::UnaryOp::USub) => {
            let v = as_positive_int_literal(&u.operand)?;
            if v == 0 {
                None
            } else {
                Some(-v)
            }
        }
        _ => {
            let v = as_positive_int_literal(e)?;
            if v == 0 {
                None
            } else {
                Some(v)
            }
        }
    }
}

/// Best-effort extract of a simple `name = ...` target, for the mutable
/// pre-walk only. Returns None for tuple / attribute / subscript
/// targets; those will produce a proper error later in `lower_assign`.
fn simple_assign_target_name(a: &ast::StmtAssign) -> Option<String> {
    if a.targets.len() != 1 {
        return None;
    }
    if let ast::Expr::Name(n) = &a.targets[0] {
        Some(n.id.to_string())
    } else {
        None
    }
}

/// PMAT-466: the base name of a subscript-target assign (`name[k] = v`),
/// used by the mutability pre-pass — such an assignment mutates `name`
/// in place, so the binding must be `mut`.
fn subscript_assign_base_name(a: &ast::StmtAssign) -> Option<String> {
    if a.targets.len() != 1 {
        return None;
    }
    if let ast::Expr::Subscript(sub) = &a.targets[0] {
        if let ast::Expr::Name(n) = sub.value.as_ref() {
            return Some(n.id.to_string());
        }
    }
    None
}

pub struct PythonFrontend;

impl Frontend for PythonFrontend {
    fn name(&self) -> &'static str {
        "python"
    }

    fn extensions(&self) -> &[&'static str] {
        &["py", "pyi"]
    }

    fn parse_and_lower(&self, path: &Path, source: &str) -> Result<Module, FrontendError> {
        let module_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let source_name = path.to_string_lossy().to_string();
        let suite = ast::Suite::parse(source, &source_name)
            .map_err(|e| FrontendError::Parse(format!("{}: {}", source_name, e)))?;

        // PMAT-471 (R2) + PMAT-474 (R5): pre-pass — record every
        // top-level function's declared return type (so cross-function
        // calls type correctly) and its ordered parameter names (so
        // `f(x=1, y=2)` keyword calls reorder to positional). A
        // leniently-unparseable return annotation falls back to I64;
        // the per-function lowering below reports a precise error.
        let mut sig_map: HashMap<String, FnSig> = HashMap::new();
        for stmt in &suite {
            if let ast::Stmt::FunctionDef(f) = stmt {
                let ret = match f.returns.as_ref() {
                    None => Type::I64,
                    Some(ann) => {
                        parse_type_annotation(f.name.as_str(), "return", ann).unwrap_or(Type::I64)
                    }
                };
                let params = f.args.args.iter().map(|a| a.def.arg.to_string()).collect();
                sig_map.insert(f.name.to_string(), FnSig { ret, params });
            }
        }
        let signatures = Rc::new(sig_map);

        let mut items = Vec::new();
        for stmt in suite {
            // PMAT-036: `from __future__ import annotations` is the
            // canonical Python preamble that defers annotation
            // evaluation. xpile fixtures with `-> BigInt` (PMAT-013
            // implicit-promotion) need this so CPython can `exec` the
            // file without `NameError: BigInt`. The frontend skips it
            // (no Meta-HIR representation needed — annotations are
            // already treated as Type tokens at lower time).
            if is_future_annotations_import(&stmt) {
                continue;
            }
            let fn_item = lower_top_level_stmt(stmt, signatures.clone())?;
            items.push(Item::Function(fn_item));
        }

        Ok(Module {
            name: module_name,
            source_lang: SourceLang::Python,
            items,
            ffi_boundaries: Vec::new(),
        })
    }
}

/// True iff `stmt` is exactly `from __future__ import annotations`.
/// PMAT-036 — see the call site for the rationale on why this is the
/// only preamble form we currently tolerate.
fn is_future_annotations_import(stmt: &ast::Stmt) -> bool {
    let ast::Stmt::ImportFrom(imp) = stmt else {
        return false;
    };
    // `module` is `Option<Identifier>`. The form `from . import x`
    // has `module: None`; `from foo import x` has `module: Some("foo")`.
    let Some(mod_id) = imp.module.as_ref() else {
        return false;
    };
    if mod_id.as_str() != "__future__" {
        return false;
    }
    // The import list is `[Alias { name: Identifier, asname: ... }]`.
    // Accept any single-alias import where name == "annotations" and
    // there's no rename.
    imp.names
        .iter()
        .any(|alias| alias.name.as_str() == "annotations" && alias.asname.is_none())
}

fn lower_top_level_stmt(
    stmt: ast::Stmt,
    signatures: Rc<HashMap<String, FnSig>>,
) -> Result<Function, FrontendError> {
    match stmt {
        ast::Stmt::FunctionDef(f) => lower_function_def(f, signatures),
        other => Err(FrontendError::Lower(format!(
            "unsupported top-level statement: {:?} — only `def` is supported at v0.1.0",
            std::mem::discriminant(&other)
        ))),
    }
}

fn lower_function_def(
    f: ast::StmtFunctionDef,
    signatures: Rc<HashMap<String, FnSig>>,
) -> Result<Function, FrontendError> {
    if !f.decorator_list.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` has decorators — not supported at v0.1.0",
            f.name
        )));
    }
    if !f.args.kwonlyargs.is_empty() || f.args.vararg.is_some() || f.args.kwarg.is_some() {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses keyword-only / *args / **kwargs — not supported at v0.1.0",
            f.name
        )));
    }

    // Parse explicit param annotations (`a: int`). Default to I64 when
    // unannotated — Python lets that mean "any int" so it's safe.
    let mut params: Vec<Param> = Vec::with_capacity(f.args.args.len());
    for arg in f.args.args {
        let name = arg.def.arg.to_string();
        let ty = match arg.def.annotation.as_ref() {
            None => Type::I64,
            Some(ann) => parse_type_annotation(&f.name, &name, ann)?,
        };
        params.push(Param {
            name,
            ty,
            mutable: false,
        });
    }

    // Body: zero or more leading `let`s, then a final `return expr`.
    if f.body.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` has an empty body",
            f.name
        )));
    }

    let body_stmts = f.body;
    let (last, leading) = body_stmts.split_last().expect("checked non-empty above");

    // Pre-parse the declared return type (if any) so the LoweringCtx can
    // type self-recursive calls correctly. Defaults to I64 — same as
    // unannotated functions.
    let declared_return_type: Option<Type> = match f.returns.as_ref() {
        None => None,
        Some(ann) => Some(parse_type_annotation(&f.name, "<return>", ann)?),
    };
    let ctx_return_type = declared_return_type.clone().unwrap_or(Type::I64);

    // PMAT-013: implicit BigInt promotion. When the user declares the
    // return type as BigInt, every `int`-typed param is promoted to
    // BigInt automatically — the user opts in once at the return site
    // instead of annotating every param. This is the most ergonomic
    // form of the slow path: `def factorial(n: int) -> BigInt:` reads
    // naturally and produces a BigInt-mode function end-to-end.
    //
    // Explicitly-annotated Bool params are left alone (Bool doesn't
    // overflow). Explicit BigInt annotations are already BigInt.
    if matches!(ctx_return_type, Type::BigInt) {
        for p in &mut params {
            if matches!(p.ty, Type::I64) {
                p.ty = Type::BigInt;
            }
        }
    }

    let mut ctx = LoweringCtx::new(
        &f.name,
        ctx_return_type.clone(),
        &params,
        &body_stmts,
        signatures,
    );
    let mut stmts: Vec<Stmt> = Vec::with_capacity(leading.len());
    for stmt in leading {
        // A single Python statement may lower to multiple meta-HIR
        // statements — most notably a multi-assignment `if/else`, where
        // each assigned name gets its own `Let` with an `IfExpr` value
        // (PMAT-005), or a `while` whose body lowers to a nested vec.
        stmts.extend(lower_block_stmt(&mut ctx, stmt.clone())?);
    }

    let trailing_return = match last {
        ast::Stmt::Return(ret) => {
            let value = ret.value.as_ref().ok_or_else(|| {
                FrontendError::Lower(format!("function `{}` returns nothing", f.name))
            })?;
            // PMAT-473 (R4): `return [elem for x in xs]` — hoist the
            // comprehension into the body (build a temp accumulator),
            // then return the temp.
            if let ast::Expr::ListComp(comp) = value.as_ref() {
                let tmp = "__xpile_comp";
                let comp_stmts = desugar_list_comp(&mut ctx, tmp, comp)?;
                stmts.extend(comp_stmts);
                Expr::Ident(tmp.to_string())
            } else if let ast::Expr::DictComp(comp) = value.as_ref() {
                // PMAT-501: `return {k: v for x in xs}` — same hoist.
                let tmp = "__xpile_comp";
                let comp_stmts = desugar_dict_comp(&mut ctx, tmp, comp)?;
                stmts.extend(comp_stmts);
                Expr::Ident(tmp.to_string())
            } else if let ast::Expr::SetComp(comp) = value.as_ref() {
                // PMAT-501b: `return {e for x in xs}` — same hoist.
                let tmp = "__xpile_comp";
                let comp_stmts = desugar_set_comp(&mut ctx, tmp, comp)?;
                stmts.extend(comp_stmts);
                Expr::Ident(tmp.to_string())
            } else {
                // PMAT-466: context-aware so `return table[key]`,
                // `return table.get(k, 0)`, and `return key in table`
                // lower to the dict variants.
                lower_expr_in_ctx(&ctx, (**value).clone())?
            }
        }
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{}` does not end with `return expr` — required at v0.1.0",
                f.name
            )));
        }
    };

    let inferred_return = infer_type_in_ctx(&ctx, &trailing_return);
    let return_type = match declared_return_type {
        None => inferred_return,
        Some(declared) => {
            if declared != inferred_return {
                return Err(FrontendError::Lower(format!(
                    "function `{}` declared return type {declared:?} but body produces {inferred_return:?}",
                    f.name
                )));
            }
            declared
        }
    };

    let body = Block {
        stmts,
        trailing_return,
    };

    // PMAT-466 (review #8): BigInt overflow-slow-path mode wraps integer
    // literals as `BigInt` and treats arithmetic as unbounded, but dict
    // keys/values are concrete fixed-width `i64`/`bool`/`String`. Mixing
    // them emits type-incoherent Rust (e.g. `unwrap_or(BigInt::from(0i64))`
    // on an `Option<i64>`), so reject the combination with a clear error.
    let bigint_mode = matches!(return_type, Type::BigInt)
        || params.iter().any(|p| matches!(p.ty, Type::BigInt))
        || body.stmts.iter().any(|s| {
            matches!(
                s,
                Stmt::Let {
                    ty: Type::BigInt,
                    ..
                }
            )
        });
    if bigint_mode && body_uses_dict(&body) {
        return Err(FrontendError::Lower(format!(
            "function `{}` combines BigInt (overflow slow-path) arithmetic with dict operations — unsupported at v0.2.0: dict keys/values are fixed-width while BigInt is unbounded, so the emission is type-incoherent. Move the dict work into a non-BigInt helper.",
            f.name
        )));
    }

    // PMAT-460: thread post-body mutability into the param list.
    // `try_lower_list_method_call` marks names in `ctx.mutable` when
    // it sees `xs.append(v)`; lift that to the corresponding Param's
    // mutable flag so the Rust/Ruchy emitter wraps the param as
    // `mut name: T`. Idempotent — names already mut for reassignment
    // are unchanged.
    for p in &mut params {
        if ctx.mutable.contains(&p.name) {
            p.mutable = true;
        }
    }

    Ok(Function {
        name: f.name.to_string(),
        params,
        return_type,
        body,
    })
}

/// Parse a Python type annotation expression to a meta-HIR [`Type`].
/// Recognized annotations:
/// * `int` → `Type::I64` — the fast path (overflow-checked at runtime).
/// * `bool` → `Type::Bool`.
/// * `BigInt` → `Type::BigInt` — the slow path of `C-PY-INT-ARITH`
///   (PMAT-012). User-supplied annotation that opts a function into
///   the unbounded-int Rust/Ruchy emission.
/// * `str` → `Type::Str` — PMAT-449, v0.2.0 Track 1.A foundation.
///   Python `str` annotation lowers to owned-`String` in Rust/Ruchy
///   and Lean `String` in the proof lane.
///
/// `BigInt` is intentionally not a real Python type — this is xpile-specific
/// nomenclature for the slow path. A future PR will infer it from `int`
/// when the function can overflow.
fn parse_type_annotation(
    fn_name: &str,
    site: &str,
    ann: &ast::Expr,
) -> Result<Type, FrontendError> {
    match ann {
        ast::Expr::Name(n) => match n.id.as_str() {
            "int" => Ok(Type::I64),
            "bool" => Ok(Type::Bool),
            // PMAT-477 (R8): Python `float` → IEEE-754 `f64`.
            "float" => Ok(Type::F64),
            "BigInt" => Ok(Type::BigInt),
            "str" => Ok(Type::Str),
            other => Err(FrontendError::Lower(format!(
                "function `{fn_name}` annotates `{site}` with unsupported type `{other}` — only `int`, `bool`, `BigInt`, `str`, `list[T]` at v0.2.0"
            ))),
        },
        // PMAT-455/PMAT-462 (v0.2.0 Track 1.B/1.C): list[T] / dict[K, V]
        // annotations parse as Python Subscript expressions. The
        // outer name selects the collection kind; the slice is either
        // a single type (list) or a tuple of two types (dict).
        ast::Expr::Subscript(sub) => {
            let ast::Expr::Name(outer) = sub.value.as_ref() else {
                return Err(FrontendError::Lower(format!(
                    "function `{fn_name}` annotates `{site}` with non-Name subscripted type — only `list[T]` / `dict[K, V]` at v0.2.0"
                )));
            };
            match outer.id.as_str() {
                "list" => {
                    let elem_ty = parse_type_annotation(
                        fn_name,
                        &format!("{site} element"),
                        &sub.slice,
                    )?;
                    Ok(Type::List(Box::new(elem_ty)))
                }
                // PMAT-500: `set[T]` annotation.
                "set" => {
                    let elem_ty = parse_type_annotation(
                        fn_name,
                        &format!("{site} element"),
                        &sub.slice,
                    )?;
                    Ok(Type::Set(Box::new(elem_ty)))
                }
                "dict" => {
                    let ast::Expr::Tuple(t) = sub.slice.as_ref() else {
                        return Err(FrontendError::Lower(format!(
                            "function `{fn_name}` annotates `{site}` with `dict[...]` lacking a key/value pair — expected `dict[K, V]`"
                        )));
                    };
                    if t.elts.len() != 2 {
                        return Err(FrontendError::Lower(format!(
                            "function `{fn_name}` annotates `{site}` with `dict[...]` containing {} type(s); expected exactly 2 (K, V)",
                            t.elts.len()
                        )));
                    }
                    let k_ty = parse_type_annotation(
                        fn_name,
                        &format!("{site} key"),
                        &t.elts[0],
                    )?;
                    let v_ty = parse_type_annotation(
                        fn_name,
                        &format!("{site} value"),
                        &t.elts[1],
                    )?;
                    Ok(Type::Dict(Box::new(k_ty), Box::new(v_ty)))
                }
                // PMAT-494: `tuple[T0, T1, ...]` (or single `tuple[T]`).
                "tuple" => {
                    let elem_tys = match sub.slice.as_ref() {
                        ast::Expr::Tuple(t) => t
                            .elts
                            .iter()
                            .map(|e| {
                                parse_type_annotation(fn_name, &format!("{site} element"), e)
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                        single => vec![parse_type_annotation(
                            fn_name,
                            &format!("{site} element"),
                            single,
                        )?],
                    };
                    Ok(Type::Tuple(elem_tys))
                }
                other => Err(FrontendError::Lower(format!(
                    "function `{fn_name}` annotates `{site}` with subscripted `{other}[...]` — only `list[T]` / `dict[K, V]` / `tuple[...]` at v0.2.0"
                ))),
            }
        }
        _ => Err(FrontendError::Lower(format!(
            "function `{fn_name}` annotates `{site}` with a non-trivial type expression — not supported at v0.2.0"
        ))),
    }
}

/// Lower a single non-trailing statement.
///
/// At v0.1.0 we recognize:
///   - `name = expr`          → one [`Stmt::Let`]
///   - `if cond: ...; else: ...`  → one [`Stmt::Let`] per assigned name,
///     each carrying an [`Expr::IfExpr`] over the same condition chain.
///     Both branches MUST assign the *same set* of names with matching
///     types per name. PMAT-005 lifted the previous single-target
///     restriction; mismatched targets / missing-else / non-assignment
///     statements still error with a clear message.
fn lower_block_stmt(ctx: &mut LoweringCtx, stmt: ast::Stmt) -> Result<Vec<Stmt>, FrontendError> {
    match stmt {
        ast::Stmt::Assign(asn) => {
            // PMAT-473 (R4): `name = [elem for var in iter]` materialises
            // to `name = []` + a for-append loop (a comprehension is an
            // expression but the meta-HIR has no block-expression).
            if asn.targets.len() == 1 {
                if let (ast::Expr::Name(n), ast::Expr::ListComp(comp)) =
                    (&asn.targets[0], asn.value.as_ref())
                {
                    let name = n.id.to_string();
                    return desugar_list_comp(ctx, &name, comp);
                }
                // PMAT-501: `name = {k: v for x in xs}` dict comprehension.
                if let (ast::Expr::Name(n), ast::Expr::DictComp(comp)) =
                    (&asn.targets[0], asn.value.as_ref())
                {
                    let name = n.id.to_string();
                    return desugar_dict_comp(ctx, &name, comp);
                }
                // PMAT-501b: `name = {e for x in xs}` set comprehension.
                if let (ast::Expr::Name(n), ast::Expr::SetComp(comp)) =
                    (&asn.targets[0], asn.value.as_ref())
                {
                    let name = n.id.to_string();
                    return desugar_set_comp(ctx, &name, comp);
                }
            }
            lower_assign(ctx, asn).map(|s| vec![s])
        }
        // PMAT-470 (R1): augmented assignment `x += e` → `x = x <op> e`.
        ast::Stmt::AugAssign(aug) => lower_aug_assign(ctx, aug).map(|s| vec![s]),
        // PMAT-466 (v0.2.0 Track 1.C): annotated local `name: T = value`.
        ast::Stmt::AnnAssign(aa) => lower_ann_assign(ctx, aa).map(|s| vec![s]),
        ast::Stmt::If(if_stmt) => lower_if_stmt(ctx, if_stmt),
        ast::Stmt::While(w) => lower_while_stmt(ctx, w).map(|s| vec![s]),
        ast::Stmt::For(f) => lower_for_stmt(ctx, f),
        ast::Stmt::Assert(a) => lower_assert_stmt(ctx, a).map(|s| vec![s]),
        // PMAT-503a: `raise SomeException("msg")` → `Stmt::Raise`. Works
        // both at top level and inside a guard-clause `if` (this fn is the
        // recursion point for if-branch bodies).
        ast::Stmt::Raise(r) => lower_raise_stmt(ctx, r).map(|s| vec![s]),
        ast::Stmt::Return(_) => Err(FrontendError::Lower(format!(
            "function `{}` has an early `return` — only the last statement may be `return` at v0.1.0",
            ctx.fn_name
        ))),
        // PMAT-040 / XPILE-BASHRS-MERGER-001 v0.3.0 falsifier evidence:
        // `subprocess.run([...])` is the first cross-domain producer
        // of `Stmt::Cmd`. Recognising it in depyler-frontend means
        // Python sources can be lowered into the bashrs domain (via
        // `xpile transpile foo.py --target shell`). This satisfies
        // the `sub/bashrs-merger.md` v0.3.0 check-back's "at least
        // one cross-domain consumer of shell variants must ship by
        // v0.3.0" precondition — and ships it at v0.1.0.
        //
        // PMAT-460 (v0.2.0 Track 1.B): an additional pre-check for
        // list method calls (`xs.append(v)`); if neither shape
        // matches, fall through to the subprocess.run path's error
        // messages.
        ast::Stmt::Expr(e) => match try_lower_list_method_call(ctx, &e) {
            Some(result) => result.map(|s| vec![s]),
            None => lower_expr_stmt_as_cmd(ctx, e).map(|s| vec![s]),
        },
        other => Err(FrontendError::Lower(format!(
            "function `{}` contains unsupported statement: {:?} — supported: assignment, if/elif/else, while, for-in-range, assert, subprocess.run([...]), then a final `return`",
            ctx.fn_name,
            std::mem::discriminant(&other)
        ))),
    }
}

/// PMAT-460 (v0.2.0 Track 1.B): pattern-match `<name>.append(<expr>)`
/// where `<name>` types as `Type::List(_)` and lower to
/// `Stmt::ListAppend`. Returns `None` if the expression-statement
/// doesn't match the shape (so the caller can try other dispatch
/// paths like `subprocess.run([...])`). Returns `Some(Err(...))` for
/// shape matches that fail later checks (e.g. wrong arity, non-list
/// receiver type).
fn try_lower_list_method_call(
    ctx: &mut LoweringCtx,
    e: &ast::StmtExpr,
) -> Option<Result<Stmt, FrontendError>> {
    let ast::Expr::Call(call) = e.value.as_ref() else {
        return None;
    };
    let ast::Expr::Attribute(attr) = call.func.as_ref() else {
        return None;
    };
    let ast::Expr::Name(receiver) = attr.value.as_ref() else {
        return None;
    };
    let method = attr.attr.as_str();
    let receiver_name = receiver.id.as_str();
    let receiver_ty = ctx.name_types.get(receiver_name).cloned();
    // `.append` on a list → ListAppend; `.add` on a set → SetAdd
    // (PMAT-500b). Other methods (`.extend`/`.insert`/`.pop`/`.remove`)
    // are explicit v0.3.0+ sub-tracks; non-matching receiver types fall
    // through to the next dispatch path's error surface.
    let is_append = method == "append" && matches!(receiver_ty, Some(Type::List(_)));
    let is_add = method == "add" && matches!(receiver_ty, Some(Type::Set(_)));
    if !is_append && !is_add {
        return None;
    }
    // Arity / kwargs check.
    if !call.keywords.is_empty() {
        return Some(Err(FrontendError::Lower(format!(
            "function `{}` calls `{receiver_name}.{method}(...)` with keyword args; \
             v0.2.0 first cut takes a single positional value",
            ctx.fn_name
        ))));
    }
    if call.args.len() != 1 {
        return Some(Err(FrontendError::Lower(format!(
            "function `{}` calls `{receiver_name}.{method}(...)` with {} positional arg(s); v0.2.0 requires exactly 1",
            ctx.fn_name,
            call.args.len()
        ))));
    }
    // PMAT-466: ctx-aware so `xs.append(d[k])` lowers the dict read to
    // DictGet, not a list index.
    let elem = match lower_expr_in_ctx(ctx, call.args[0].clone()) {
        Ok(e) => e,
        Err(err) => return Some(Err(err)),
    };
    // Mark the receiver as mutable (idempotent — the pre-pass also flags
    // it via the `.add`/`.append` walk_counts arm).
    ctx.mutable.insert(receiver_name.to_string());
    Some(Ok(if is_add {
        Stmt::SetAdd {
            set_name: receiver_name.to_string(),
            elem,
        }
    } else {
        Stmt::ListAppend {
            list_name: receiver_name.to_string(),
            elem,
        }
    }))
}

/// PMAT-040: pattern-match `subprocess.run([str-literal, ...])` and
/// lower to `Stmt::Cmd`. Returns a precise error for any other
/// expression-statement shape — we don't generalise to all
/// expression statements at v0.1.0 because the only such shape we
/// currently understand is this exact call. The narrow match keeps
/// the dispatch boundary explicit so future widening (e.g.
/// `subprocess.check_call`, `os.system(...)`) is an additive
/// pattern-match rather than a refactor of a generic-expr handler.
///
/// Accepted shapes:
///   subprocess.run(["echo", "hi"])
///   subprocess.run(["echo", "hi"], check=True)    # keywords ignored
///   subprocess.run(["pwd"])                       # 1-element list
///
/// Rejected:
///   subprocess.run(cmd)                           # non-list arg
///   subprocess.run(["echo", 42])                  # non-string element
///   subprocess.run([])                            # empty list (no program)
///   subprocess.call([...])                        # different function name
///   os.system("echo hi")                          # not subprocess.run
///   foo()                                         # not subprocess.run
fn lower_expr_stmt_as_cmd(ctx: &LoweringCtx, e: ast::StmtExpr) -> Result<Stmt, FrontendError> {
    let ast::Expr::Call(call) = *e.value else {
        return Err(FrontendError::Lower(format!(
            "function `{}` has a non-call expression statement — only `subprocess.run([...])` is recognised as an expression statement at v0.1.0",
            ctx.fn_name
        )));
    };
    // Callee must be `subprocess.run` (Attribute(Name("subprocess"), "run")).
    let ast::Expr::Attribute(attr) = call.func.as_ref() else {
        return Err(FrontendError::Lower(format!(
            "function `{}` has a non-`subprocess.run` call as expression statement — only `subprocess.run([...])` is recognised at v0.1.0",
            ctx.fn_name
        )));
    };
    let ast::Expr::Name(receiver) = attr.value.as_ref() else {
        return Err(FrontendError::Lower(format!(
            "function `{}`'s expression-statement call's receiver isn't a simple `subprocess` Name — only `subprocess.run([...])` shape is recognised",
            ctx.fn_name
        )));
    };
    if receiver.id.as_str() != "subprocess" || attr.attr.as_str() != "run" {
        return Err(FrontendError::Lower(format!(
            "function `{}` calls `{}.{}` as expression statement — only `subprocess.run([...])` is recognised at v0.1.0",
            ctx.fn_name,
            receiver.id.as_str(),
            attr.attr.as_str()
        )));
    }
    // Exactly one positional argument: a list literal of string literals.
    // Keyword args (e.g. `check=True`) are accepted but currently ignored
    // — semantically permissive, and consistent with how Python's
    // subprocess module treats them as runtime modifiers rather than
    // command-content modifiers.
    let positional: Vec<&ast::Expr> = call.args.iter().collect();
    if positional.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "function `{}` calls `subprocess.run` with {} positional arg(s); v0.1.0 requires exactly 1 (a list literal of string literals)",
            ctx.fn_name,
            positional.len()
        )));
    }
    let ast::Expr::List(list) = positional[0] else {
        return Err(FrontendError::Lower(format!(
            "function `{}` calls `subprocess.run(<expr>)` with a non-list argument — v0.1.0 supports only a list literal of string literals (e.g. `subprocess.run([\"echo\", \"hi\"])`)",
            ctx.fn_name
        )));
    };
    if list.elts.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` calls `subprocess.run([])` with an empty list — at least one element is required (the program to run)",
            ctx.fn_name
        )));
    }
    let mut tokens: Vec<String> = Vec::with_capacity(list.elts.len());
    for elt in &list.elts {
        let ast::Expr::Constant(c) = elt else {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `subprocess.run([..., <expr>, ...])` with a non-literal element — v0.1.0 supports only string literals in the list",
                ctx.fn_name
            )));
        };
        let ast::Constant::Str(s) = &c.value else {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `subprocess.run([..., <non-string>, ...])` — v0.1.0 supports only string-literal elements",
                ctx.fn_name
            )));
        };
        tokens.push(s.to_string());
    }
    // tokens.len() >= 1 by the empty-check above; safe to split.
    let program = tokens.remove(0);
    // PMAT-042: args are `Vec<Expr>` of `Expr::LitStr`. Python
    // string literals don't carry shell-quoting metadata, so we
    // emit the unquoted form. A future enhancement could detect
    // arg-containing-whitespace and promote to `Expr::QuotedString`
    // with the right strategy, but that's outside the v0.1.0 scope.
    let args: Vec<Expr> = tokens.into_iter().map(Expr::LitStr).collect();
    Ok(Stmt::Cmd { program, args })
}

/// Lower `for target in range(...)` by desugaring to a `Stmt::Let`
/// (the init binding for `target`) followed by a `Stmt::While` whose
/// body is the for-body plus a `target = target + step;` tail.
/// PMAT-007.
///
/// Supports `range(stop)`, `range(start, stop)`, and `range(start, stop, step)`.
/// The step must be a positive integer literal at v0.1.0 — negative or
/// zero steps would need `target > stop` and a different cond, which
/// is deferred. Other iterables (lists, generators, dict.items, etc.)
/// also error: v0.1.0 has no collection types yet.
fn lower_for_stmt(ctx: &mut LoweringCtx, f: ast::StmtFor) -> Result<Vec<Stmt>, FrontendError> {
    if !f.orelse.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` has a `for ... else:` clause — Python's `else` on loops is not supported at v0.1.0",
            ctx.fn_name
        )));
    }

    // PMAT-495: paired for-loop `for a, b in enumerate(xs)` /
    // `for a, b in zip(xs, ys)` → Stmt::ForEachPair.
    if let ast::Expr::Tuple(tgt) = &*f.target {
        if tgt.elts.len() == 2 {
            if let (ast::Expr::Name(a), ast::Expr::Name(b)) = (&tgt.elts[0], &tgt.elts[1]) {
                if let ast::Expr::Call(call) = &*f.iter {
                    if let ast::Expr::Name(fname) = call.func.as_ref() {
                        let fname = fname.id.to_string();
                        let arity_ok = (fname == "enumerate" && call.args.len() == 1)
                            || (fname == "zip" && call.args.len() == 2);
                        if arity_ok {
                            let first = a.id.to_string();
                            let second = b.id.to_string();
                            let iter_expr = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                            let Type::List(elem) = infer_type_in_ctx(ctx, &iter_expr) else {
                                return Err(FrontendError::Lower(format!(
                                    "function `{}` uses `{fname}(...)` over a non-list — only list iteration is supported at v0.2.0 first cut",
                                    ctx.fn_name
                                )));
                            };
                            let kind = if fname == "enumerate" {
                                ctx.name_types.insert(first.clone(), Type::I64);
                                ctx.name_types.insert(second.clone(), (*elem).clone());
                                PairIterKind::Enumerate
                            } else {
                                let other = lower_expr_in_ctx(ctx, call.args[1].clone())?;
                                let Type::List(elem2) = infer_type_in_ctx(ctx, &other) else {
                                    return Err(FrontendError::Lower(format!(
                                        "function `{}` uses `zip(...)` with a non-list second argument",
                                        ctx.fn_name
                                    )));
                                };
                                ctx.name_types.insert(first.clone(), (*elem).clone());
                                ctx.name_types.insert(second.clone(), (*elem2).clone());
                                PairIterKind::Zip(Box::new(other))
                            };
                            ctx.bound.insert(first.clone());
                            ctx.bound.insert(second.clone());
                            let mut body: Vec<Stmt> = Vec::new();
                            for s in f.body {
                                body.extend(lower_block_stmt(ctx, s)?);
                            }
                            return Ok(vec![Stmt::ForEachPair {
                                first,
                                second,
                                iter: iter_expr,
                                kind,
                                body,
                            }]);
                        }
                    }
                }
                // PMAT-502y: `for k, v in <list of 2-tuples>` (e.g.
                // `for k, v in d.items()`) — iterate a `List(Tuple[A, B])`
                // and destructure each element into (k, v). Reached only when
                // the iter is not enumerate/zip (those returned above).
                let iter_expr = lower_expr_in_ctx(ctx, (*f.iter).clone())?;
                if let Type::List(elem) = infer_type_in_ctx(ctx, &iter_expr) {
                    if let Type::Tuple(tys) = &*elem {
                        if tys.len() == 2 {
                            let first = a.id.to_string();
                            let second = b.id.to_string();
                            ctx.name_types.insert(first.clone(), tys[0].clone());
                            ctx.name_types.insert(second.clone(), tys[1].clone());
                            ctx.bound.insert(first.clone());
                            ctx.bound.insert(second.clone());
                            let mut body: Vec<Stmt> = Vec::new();
                            for s in f.body {
                                body.extend(lower_block_stmt(ctx, s)?);
                            }
                            return Ok(vec![Stmt::ForEachPair {
                                first,
                                second,
                                iter: iter_expr,
                                kind: PairIterKind::Pairs,
                                body,
                            }]);
                        }
                    }
                }
            }
        }
    }

    let target_name = match &*f.target {
        ast::Expr::Name(n) => n.id.to_string(),
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{}` uses a non-Name `for` target (tuple unpacking, attribute, subscript) — not supported at v0.1.0",
                ctx.fn_name
            )));
        }
    };

    // PMAT-458 (v0.2.0 Track 1.B): dispatch on iter shape.
    //   - `range(...)` call → existing Let+While desugar (below).
    //   - Otherwise: lower iter as an expression and emit
    //     `Stmt::ForEach` if it types as Type::List.
    // The match-on-Call below handles the range case; the early-
    // return here handles the non-Call (= collection-iter) case.
    if !matches!(&*f.iter, ast::Expr::Call(_)) {
        let iter_expr = lower_expr_in_ctx(ctx, (*f.iter).clone())?;
        let iter_ty = infer_type_in_ctx(ctx, &iter_expr);
        // PMAT-472 (R3): a dict iterates its keys (`for k in d:`), so
        // bind `target` to the key type and flag `over_keys`.
        let (elem_ty, over_keys) = match iter_ty {
            Type::List(elem) => (*elem, false),
            Type::Dict(key_ty, _) => (*key_ty, true),
            other => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` iterates a non-collection expression typing as {other:?} — \
                     v0.2.0 supports `for target in range(...)`, `for target in <list[T]>`, \
                     or `for key in <dict[K, V]>`; other iterables are deferred",
                    ctx.fn_name
                )));
            }
        };

        // Bind the loop variable in ctx so the body's typed accesses
        // resolve.
        ctx.bound.insert(target_name.clone());
        ctx.name_types.insert(target_name.clone(), elem_ty.clone());

        let mut body: Vec<Stmt> = Vec::new();
        for s in f.body {
            let lowered = lower_block_stmt(ctx, s)?;
            body.extend(lowered);
        }
        return Ok(vec![Stmt::ForEach {
            var: target_name,
            iter: iter_expr,
            elem_ty,
            body,
            over_keys,
        }]);
    }

    // Match range(...) call. Anything else (list/tuple/dict iteration)
    // requires collection types and is out of scope at v0.1.0.
    let call = match &*f.iter {
        ast::Expr::Call(c) => c,
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{}` iterates a non-call expression — v0.1.0 supports only `for target in range(...)`",
                ctx.fn_name
            )));
        }
    };
    let callee = match &*call.func {
        ast::Expr::Name(n) if n.id.as_str() == "range" => "range",
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{}` iterates a non-`range(...)` call — v0.1.0 supports only `for target in range(...)`",
                ctx.fn_name
            )));
        }
    };
    if !call.keywords.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` passes keyword args to `{callee}(...)` — v0.1.0 supports only positional args",
            ctx.fn_name
        )));
    }
    // PMAT-466: route range bounds through the context-aware path so a
    // dict read used as a bound (`for i in range(d[k])`) lowers to a
    // DictGet, not a list `Expr::Index` (`d[k as usize]`, uncompilable
    // against a HashMap). The step stays an integer literal.
    let (start_expr, stop_expr, step_int) = match call.args.as_slice() {
        [stop] => (Expr::LitInt(0), lower_expr_in_ctx(ctx, stop.clone())?, 1i64),
        [start, stop] => (
            lower_expr_in_ctx(ctx, start.clone())?,
            lower_expr_in_ctx(ctx, stop.clone())?,
            1i64,
        ),
        [start, stop, step] => {
            // v0.1.0 requires a non-zero *integer literal* step so the
            // loop direction (`i < stop` vs `i > stop`) is known at lower
            // time. PMAT-008 added negative-literal support; non-literal
            // / zero step still errors. Python's parser represents `-3`
            // as UnaryOp(USub, Constant(3)) rather than Constant(-3),
            // so we look through that.
            let step = extract_step_literal(step).ok_or_else(|| {
                FrontendError::Lower(format!(
                    "function `{}` uses `range(..., step)` with a non-literal-int or zero step — v0.1.0 requires a non-zero integer literal here",
                    ctx.fn_name
                ))
            })?;
            (
                lower_expr_in_ctx(ctx, start.clone())?,
                lower_expr_in_ctx(ctx, stop.clone())?,
                step,
            )
        }
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `range(...)` with {} args — Python supports 1-3, v0.1.0 too",
                ctx.fn_name,
                call.args.len()
            )));
        }
    };

    let step_expr = Expr::LitInt(step_int);

    // Emit:
    //   let mut target: i64 = <start>;
    //   while (target <cmp> <stop>) {        // cmp = `<` (pos step) or `>` (neg step)
    //       <body...>
    //       target = (target).checked_add(<step>);
    //   }
    // PMAT-036: when the enclosing function is BigInt-mode (return
    // type is BigInt → all `int` params auto-promoted, all int literals
    // lifted in the body), the for-target's binding type must also be
    // BigInt — otherwise the emitted `let mut i: i64 = n.clone()` is a
    // type error against the BigInt `n` and the BigInt step literal in
    // the tail. The choice of I64 vs BigInt is purely determined by the
    // function's return type; no other inference is needed because the
    // for-range desugaring rebinds `i` each iteration from the step
    // expression which already carries the right type.
    let target_ty = match ctx.fn_return_type {
        Type::BigInt => Type::BigInt,
        _ => Type::I64,
    }
    .clone();
    let init_stmt = if ctx.bound.contains(&target_name) {
        Stmt::Assign {
            name: target_name.clone(),
            value: start_expr,
        }
    } else {
        ctx.bound.insert(target_name.clone());
        ctx.name_types
            .insert(target_name.clone(), target_ty.clone());
        Stmt::Let {
            name: target_name.clone(),
            ty: target_ty,
            value: start_expr,
            // for-target is by definition reassigned each iteration —
            // mutable. The pre-walk also flags it, but we set explicitly
            // for clarity.
            mutable: true,
        }
    };

    let cond_op = if step_int > 0 { BinOp::Lt } else { BinOp::Gt };
    let cond = Expr::BinOp {
        op: cond_op,
        lhs: Box::new(Expr::Ident(target_name.clone())),
        rhs: Box::new(stop_expr),
    };

    let mut body = Vec::with_capacity(f.body.len() + 1);
    for stmt in f.body {
        body.extend(lower_block_stmt(ctx, stmt)?);
    }
    // Tail: target = target + step
    body.push(Stmt::Assign {
        name: target_name.clone(),
        value: Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::Ident(target_name)),
            rhs: Box::new(step_expr),
        },
    });

    let while_stmt = Stmt::While { cond, body };
    Ok(vec![init_stmt, while_stmt])
}

/// Lower a Python `while cond: body` into [`Stmt::While`]. Body
/// statements are lowered via [`lower_block_stmt`] in order, so the
/// inner Lets / Assigns share the same `bound` / `mutable` tracking
/// as the enclosing function. Variables introduced inside a loop
/// remain "bound" for subsequent loop iterations (that's how Python
/// works); we don't pop them when leaving the body.
fn lower_while_stmt(ctx: &mut LoweringCtx, w: ast::StmtWhile) -> Result<Stmt, FrontendError> {
    if !w.orelse.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` has a `while ... else:` clause — Python's `else` on loops is not supported at v0.1.0",
            ctx.fn_name
        )));
    }
    let cond = lower_expr_in_ctx(ctx, *w.test)?;
    if infer_type(&cond) != Type::Bool {
        return Err(FrontendError::Lower(format!(
            "function `{}` has a while-condition that is not Bool (no int-truthiness at v0.1.0)",
            ctx.fn_name
        )));
    }
    let mut body = Vec::with_capacity(w.body.len());
    for stmt in w.body {
        body.extend(lower_block_stmt(ctx, stmt)?);
    }
    Ok(Stmt::While { cond, body })
}

/// Lower `assert cond` (no message form at v0.1.0) to [`Stmt::Assert`].
/// Python `assert cond, msg` requires the message at runtime and is
/// deferred. PMAT-009.
fn lower_assert_stmt(ctx: &mut LoweringCtx, a: ast::StmtAssert) -> Result<Stmt, FrontendError> {
    if a.msg.is_some() {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses `assert cond, msg` — message form not supported at v0.1.0",
            ctx.fn_name
        )));
    }
    let cond = lower_expr_in_ctx(ctx, *a.test)?;
    if infer_type(&cond) != Type::Bool {
        return Err(FrontendError::Lower(format!(
            "function `{}` has an `assert` whose expression is not Bool (no int-truthiness at v0.1.0)",
            ctx.fn_name
        )));
    }
    Ok(Stmt::Assert { cond })
}

/// PMAT-503a (first sub-slice of PMAT-503 exceptions): lower
/// `raise SomeException("message")` to [`Stmt::Raise`]. The supported
/// forms are `raise Exc("msg")` / `raise Exc(<str-expr>)` (1 string arg),
/// `raise Exc()` (no args → the class name is the message), and a bare
/// `raise Exc` class name. A re-raising bare `raise` and the
/// `raise ... from ...` cause form are rejected at this first cut.
fn lower_raise_stmt(ctx: &mut LoweringCtx, r: ast::StmtRaise) -> Result<Stmt, FrontendError> {
    if r.cause.is_some() {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses `raise ... from ...` — the cause form is not supported at v0.1.0",
            ctx.fn_name
        )));
    }
    let Some(exc) = r.exc else {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses a bare `raise` (re-raise) — only `raise Exc(\"msg\")` is supported at v0.1.0",
            ctx.fn_name
        )));
    };
    let message = match *exc {
        // `raise ValueError("msg")` / `raise Exception(<str expr>)`.
        ast::Expr::Call(call) if call.keywords.is_empty() && call.args.len() == 1 => {
            let msg = lower_expr_in_ctx(ctx, call.args[0].clone())?;
            if infer_type_in_ctx(ctx, &msg) != Type::Str {
                return Err(FrontendError::Lower(format!(
                    "function `{}` raises with a non-string message — only a `Str` \
                     message (`raise Exc(\"...\")`) is supported at v0.1.0",
                    ctx.fn_name
                )));
            }
            msg
        }
        // `raise ValueError()` — no message → use the exception class name.
        ast::Expr::Call(call) if call.keywords.is_empty() && call.args.is_empty() => {
            Expr::LitStr(exc_class_name(&call.func))
        }
        // `raise StopIteration` — a bare exception class name.
        ast::Expr::Name(name) => Expr::LitStr(name.id.to_string()),
        other => {
            return Err(FrontendError::Lower(format!(
                "function `{}` uses an unsupported `raise` form: {:?}",
                ctx.fn_name,
                std::mem::discriminant(&other)
            )));
        }
    };
    Ok(Stmt::Raise { message })
}

/// Best-effort name of the exception class in a `raise Exc(...)` callee —
/// used as the panic message when the constructor has no string argument.
fn exc_class_name(func: &ast::Expr) -> String {
    match func {
        ast::Expr::Name(n) => n.id.to_string(),
        ast::Expr::Attribute(a) => a.attr.to_string(),
        _ => "exception".to_string(),
    }
}

/// Lower a Python `if/elif*/else` statement whose every branch is a
/// list of single-name assignments. The set of assigned names must be
/// the *same* across all branches (no use-before-init in subsequent
/// stmts). Lifts to one `Stmt::Let { name, value: Expr::IfExpr { ... } }`
/// per assigned name, all sharing the same condition chain. PMAT-005
/// extended this from "exactly one assignment per branch" to "any
/// number of single-name assignments, same set per branch".
///
/// Multiple Lets means the condition is evaluated once per assigned
/// name in the generated Rust. v0.1.0 has no observable side effects
/// (no mutation, no I/O, function calls are pure-from-codegen's-pov),
/// so this is semantically equivalent to evaluating once.
fn lower_if_stmt_as_lets(
    ctx: &mut LoweringCtx,
    if_stmt: ast::StmtIf,
) -> Result<Vec<Stmt>, FrontendError> {
    let target_names = collect_branch_assignment_names(&ctx.fn_name, &if_stmt.body)?;
    if target_names.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` has an if-branch with no assignments — v0.1.0 if-as-let requires at least one assignment per branch",
            ctx.fn_name
        )));
    }
    validate_branch_name_sets(&ctx.fn_name, &if_stmt, &target_names)?;

    let mut stmts = Vec::with_capacity(target_names.len());
    for name in &target_names {
        let if_expr = lower_if_chain_to_expr(ctx, &ctx.fn_name, &if_stmt, name)?;
        let ty = match &if_expr {
            Expr::IfExpr { then_expr, .. } => infer_type_in_ctx(ctx, then_expr),
            other => infer_type_in_ctx(ctx, other),
        };
        // If the name is already in scope, this if-as-let is actually
        // a multi-branch reassignment — emit `Stmt::Assign`. Otherwise
        // a fresh `Let` (with `mutable` computed up front).
        if ctx.bound.contains(name) {
            stmts.push(Stmt::Assign {
                name: name.clone(),
                value: if_expr,
            });
        } else {
            stmts.push(Stmt::Let {
                name: name.clone(),
                ty: ty.clone(),
                value: if_expr,
                mutable: ctx.mutable.contains(name),
            });
            ctx.bound.insert(name.clone());
            ctx.name_types.insert(name.clone(), ty);
        }
    }
    Ok(stmts)
}

/// True for a simple `name = expr` statement (the if-as-let branch shape).
fn is_simple_name_assign(s: &ast::Stmt) -> bool {
    matches!(s, ast::Stmt::Assign(a)
        if a.targets.len() == 1 && matches!(a.targets[0], ast::Expr::Name(_)))
}

/// Whether `if_stmt` fits the value-producing **if-as-let** shape: every
/// statement in every branch is `name = expr`, with a final `else`.
/// Otherwise (side-effecting branches — subscript assigns, `.append`,
/// dict mutation, …) the if/else lowers to a general [`Stmt::If`].
fn is_if_as_let_shape(if_stmt: &ast::StmtIf) -> bool {
    if if_stmt.body.is_empty() || !if_stmt.body.iter().all(is_simple_name_assign) {
        return false;
    }
    match if_stmt.orelse.as_slice() {
        [] => false,
        [ast::Stmt::If(nested)] => is_if_as_let_shape(nested),
        rest => rest.iter().all(is_simple_name_assign),
    }
}

/// PMAT-502: lower a Python `if/elif/else`. Dispatches between the
/// value-producing if-as-let form (all branches `name = expr` → `let x =
/// if c { … } else { … }`) and a general [`Stmt::If`] for side-effecting
/// branches. In the general form, names assigned inside a branch do NOT
/// escape it (Rust block scoping) — use the `name = expr` form for a
/// value needed after the `if`. `elif` nests as a `Stmt::If` in `else_body`
/// via `lower_block_stmt` recursion.
fn lower_if_stmt(ctx: &mut LoweringCtx, if_stmt: ast::StmtIf) -> Result<Vec<Stmt>, FrontendError> {
    if is_if_as_let_shape(&if_stmt) {
        return lower_if_stmt_as_lets(ctx, if_stmt);
    }
    let cond = lower_expr_in_ctx(ctx, (*if_stmt.test).clone())?;
    if !matches!(infer_type_in_ctx(ctx, &cond), Type::Bool) {
        return Err(FrontendError::Lower(format!(
            "function `{}` has an `if` condition that does not type as bool — v0.2.0 requires a boolean condition",
            ctx.fn_name
        )));
    }
    let mut then_body = Vec::new();
    for s in if_stmt.body {
        then_body.extend(lower_block_stmt(ctx, s)?);
    }
    let mut else_body = Vec::new();
    for s in if_stmt.orelse {
        else_body.extend(lower_block_stmt(ctx, s)?);
    }
    Ok(vec![Stmt::If {
        cond,
        then_body,
        else_body,
    }])
}

/// Inspect a branch body (a Vec<ast::Stmt>) and return the list of
/// single-name targets in source order. Errors if any statement is not
/// `name = expr` (the v0.1.0 shape for if-branches).
fn collect_branch_assignment_names(
    fn_name: &str,
    body: &[ast::Stmt],
) -> Result<Vec<String>, FrontendError> {
    let mut names = Vec::with_capacity(body.len());
    for stmt in body {
        match stmt {
            ast::Stmt::Assign(a) => names.push(single_name_target(fn_name, a)?),
            _ => {
                return Err(FrontendError::Lower(format!(
                    "function `{fn_name}` has an if-branch statement that is not `name = expr` — v0.1.0 if-as-let requires every statement in every branch to be a simple assignment"
                )));
            }
        }
    }
    Ok(names)
}

/// Walk the `if/elif*/else` chain and verify every branch's
/// assignment-name *set* matches `expected`. Order within a branch
/// doesn't need to match — we sort+compare.
fn validate_branch_name_sets(
    fn_name: &str,
    if_stmt: &ast::StmtIf,
    expected: &[String],
) -> Result<(), FrontendError> {
    let mut expected_sorted: Vec<&str> = expected.iter().map(String::as_str).collect();
    expected_sorted.sort_unstable();

    // The then-branch is the source of truth (already covered by
    // `expected`); validate the orelse chain.
    if if_stmt.orelse.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{fn_name}` has `if` without `else` — at v0.1.0 every branch must assign the same names (no use-before-init)"
        )));
    }
    let mut current: &[ast::Stmt] = &if_stmt.orelse;
    loop {
        if current.len() == 1 {
            if let ast::Stmt::If(nested) = &current[0] {
                let nested_names = collect_branch_assignment_names(fn_name, &nested.body)?;
                let mut nested_sorted: Vec<&str> =
                    nested_names.iter().map(String::as_str).collect();
                nested_sorted.sort_unstable();
                if nested_sorted != expected_sorted {
                    return Err(FrontendError::Lower(format!(
                        "function `{fn_name}` has an elif-branch assigning {nested_sorted:?} but the then-branch assigns {expected_sorted:?} — every branch must assign the same names"
                    )));
                }
                if nested.orelse.is_empty() {
                    return Err(FrontendError::Lower(format!(
                        "function `{fn_name}` has elif without final else — every branch must assign the same names"
                    )));
                }
                current = &nested.orelse;
                continue;
            }
        }
        // Terminal else: must be a list of assignments matching `expected`.
        let else_names = collect_branch_assignment_names(fn_name, current)?;
        let mut else_sorted: Vec<&str> = else_names.iter().map(String::as_str).collect();
        else_sorted.sort_unstable();
        if else_sorted != expected_sorted {
            return Err(FrontendError::Lower(format!(
                "function `{fn_name}` has an else-branch assigning {else_sorted:?} but the then-branch assigns {expected_sorted:?} — every branch must assign the same names"
            )));
        }
        return Ok(());
    }
}

/// Recursively lower a chain of `if/elif*/else` into a single
/// [`Expr::IfExpr`] expression. Used by `lower_if_stmt_as_let` to
/// support elif:
///
/// ```text
/// if a: x = 1                            if a { 1 }
/// elif b: x = 2          lowers to       else if b { 2 }
/// else:    x = 3                         else { 3 }
/// ```
///
/// Internally this becomes nested IfExpr nodes; the codegen pretty-print
/// is `if a { 1 } else { if b { 2 } else { 3 } }` (semantically equivalent
/// to `else if` — a future pretty-printer can flatten).
fn lower_if_chain_to_expr(
    ctx: &LoweringCtx,
    fn_name: &str,
    if_stmt: &ast::StmtIf,
    target_name: &str,
) -> Result<Expr, FrontendError> {
    // Set-equality of branch names is already enforced upstream by
    // `validate_branch_name_sets`. This function focuses on extracting
    // *this* target's value from each branch and building the IfExpr.
    if if_stmt.orelse.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{fn_name}` has `if` without `else` — at v0.1.0 every branch must assign `{target_name}` (no use-before-init)"
        )));
    }

    let cond = lower_expr_in_ctx(ctx, (*if_stmt.test).clone())?;
    if infer_type_in_ctx(ctx, &cond) != Type::Bool {
        return Err(FrontendError::Lower(format!(
            "function `{fn_name}` has an if-condition that is not Bool (no int-truthiness at v0.1.0)"
        )));
    }

    let then_expr = find_assignment_value(ctx, fn_name, &if_stmt.body, target_name)?;
    let then_ty = infer_type_in_ctx(ctx, &then_expr);

    // Else branch is one of:
    //   nested StmtIf → recurse (handles elif)
    //   any list of assignments → terminal else: find `target_name` here
    let else_expr = if if_stmt.orelse.len() == 1 {
        if let ast::Stmt::If(nested) = &if_stmt.orelse[0] {
            lower_if_chain_to_expr(ctx, fn_name, nested, target_name)?
        } else {
            find_assignment_value(ctx, fn_name, &if_stmt.orelse, target_name)?
        }
    } else {
        find_assignment_value(ctx, fn_name, &if_stmt.orelse, target_name)?
    };
    let else_ty = infer_type_in_ctx(ctx, &else_expr);
    if then_ty != else_ty {
        return Err(FrontendError::Lower(format!(
            "function `{fn_name}` if-branches assign `{target_name}` with mismatched types ({then_ty:?} vs {else_ty:?})"
        )));
    }

    Ok(Expr::IfExpr {
        cond: Box::new(cond),
        then_expr: Box::new(then_expr),
        else_expr: Box::new(else_expr),
    })
}

/// Walk a branch body looking for `target_name = expr` and return the
/// lowered RHS. Errors if `target_name` is not assigned in this branch
/// (set-equality is checked upstream; this is the "extract" step).
fn find_assignment_value(
    ctx: &LoweringCtx,
    fn_name: &str,
    body: &[ast::Stmt],
    target_name: &str,
) -> Result<Expr, FrontendError> {
    for stmt in body {
        if let ast::Stmt::Assign(a) = stmt {
            let name = single_name_target(fn_name, a)?;
            if name == target_name {
                // PMAT-466: context-aware so a dict read `y = d[k]` in an
                // if/else branch lowers to `DictGet`, not a list index.
                return lower_expr_in_ctx(ctx, (*a.value).clone());
            }
        }
    }
    Err(FrontendError::Lower(format!(
        "function `{fn_name}` has a branch that does not assign `{target_name}` — every branch must assign every name (set-equality)"
    )))
}

/// Extract the single Name target of an `Assign` statement, rejecting
/// chained / tuple / attribute / subscript targets with messages
/// consistent with [`lower_assign`].
fn single_name_target(fn_name: &str, asn: &ast::StmtAssign) -> Result<String, FrontendError> {
    if asn.targets.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "function `{fn_name}` has chained assignment inside if-branch — not supported at v0.1.0"
        )));
    }
    match &asn.targets[0] {
        ast::Expr::Name(n) => Ok(n.id.to_string()),
        ast::Expr::Tuple(_) => Err(FrontendError::Lower(format!(
            "function `{fn_name}` uses tuple unpacking inside if-branch — not supported at v0.1.0"
        ))),
        _ => Err(FrontendError::Lower(format!(
            "function `{fn_name}` has unsupported assignment target inside if-branch"
        ))),
    }
}

fn lower_assign(ctx: &mut LoweringCtx, asn: ast::StmtAssign) -> Result<Stmt, FrontendError> {
    if asn.targets.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "function `{}` has chained assignment `a = b = ...` — not supported at v0.1.0",
            ctx.fn_name
        )));
    }
    let target = asn.targets.into_iter().next().expect("len checked");
    let name = match target {
        ast::Expr::Name(n) => n.id.to_string(),
        // PMAT-494b: tuple unpacking `a, b = <expr>` → Stmt::LetTuple.
        // All targets must be plain names (no nested / starred / subscript
        // patterns at first cut). Each name's type comes from the value's
        // tuple type so later references infer correctly.
        ast::Expr::Tuple(t) => {
            let names = t
                .elts
                .iter()
                .map(|e| match e {
                    ast::Expr::Name(n) => Ok(n.id.to_string()),
                    _ => Err(FrontendError::Lower(format!(
                        "function `{}` uses a non-Name tuple-unpacking target (nested / starred / subscript) — not supported at v0.2.0 first cut",
                        ctx.fn_name
                    ))),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let value = lower_expr_in_ctx(ctx, *asn.value)?;
            match infer_type_in_ctx(ctx, &value) {
                Type::Tuple(elem_tys) if elem_tys.len() == names.len() => {
                    for (n, ty) in names.iter().zip(elem_tys.into_iter()) {
                        ctx.name_types.insert(n.clone(), ty);
                    }
                }
                other => {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` unpacks {} names but the right-hand side types as {other:?} — expected a tuple of {} elements",
                        ctx.fn_name,
                        names.len(),
                        names.len()
                    )));
                }
            }
            return Ok(Stmt::LetTuple { names, value });
        }
        // PMAT-461 (v0.2.0 Track 1.B): `xs[i] = v` indexed assignment
        // for lists. PMAT-466 (v0.2.0 Track 1.C): `d[k] = v` keyed
        // assignment for dicts. The Subscript target's value must be a
        // Name; the receiver's inferred type selects the variant
        // (`Type::List` → `Stmt::IndexAssign`, `Type::Dict` →
        // `Stmt::DictSet`). Either way the receiver is marked mutable.
        ast::Expr::Subscript(sub) => {
            let receiver = match sub.value.as_ref() {
                ast::Expr::Name(n) => n.id.to_string(),
                _ => {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` has a non-Name subscript-assignment target — v0.2.0 supports `<name>[k] = v` only",
                        ctx.fn_name
                    )));
                }
            };
            let receiver_ty = ctx.name_types.get(&receiver).cloned();
            match receiver_ty {
                Some(Type::List(_)) => {
                    // PMAT-466: ctx-aware so a dict read used as a list
                    // index (`xs[d[k]] = v`) lowers to DictGet, not a
                    // nested list index.
                    let index = lower_expr_in_ctx(ctx, (*sub.slice).clone())?;
                    let idx_ty = infer_type_in_ctx(ctx, &index);
                    if !matches!(idx_ty, Type::I64) {
                        return Err(FrontendError::Lower(format!(
                            "function `{}` indexed-assigns `{receiver}[<expr>]` where index types as {idx_ty:?}; only `int` indices are supported at v0.2.0",
                            ctx.fn_name
                        )));
                    }
                    let value = lower_expr_in_ctx(ctx, *asn.value)?;
                    ctx.mutable.insert(receiver.clone());
                    return Ok(Stmt::IndexAssign {
                        list_name: receiver,
                        index,
                        value,
                    });
                }
                // PMAT-466: dict keyed assignment. Keys may be any
                // hashable scalar the dict was declared over (str / int
                // / bool) — no `int`-index constraint as for lists.
                Some(Type::Dict(_, _)) => {
                    let key = lower_expr_in_ctx(ctx, (*sub.slice).clone())?;
                    let value = lower_expr_in_ctx(ctx, *asn.value)?;
                    ctx.mutable.insert(receiver.clone());
                    return Ok(Stmt::DictSet {
                        dict_name: receiver,
                        key,
                        value,
                    });
                }
                _ => {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` keyed-assigns to `{receiver}` which doesn't type as list[T] or dict[K, V] — v0.2.0 supports list/dict subscript assignment only",
                        ctx.fn_name
                    )));
                }
            }
        }
        ast::Expr::Attribute(_) => {
            return Err(FrontendError::Lower(format!(
                "function `{}` assigns to an attribute — not supported at v0.2.0",
                ctx.fn_name
            )));
        }
        other => {
            return Err(FrontendError::Lower(format!(
                "function `{}` has unsupported assignment target: {:?}",
                ctx.fn_name,
                std::mem::discriminant(&other)
            )));
        }
    };
    // PMAT-466: route the RHS through the context-aware path so a
    // dict op on the right of a plain `name = ...` (e.g.
    // `result = table.get(k, 0)`) lowers correctly.
    let value = lower_expr_in_ctx(ctx, *asn.value)?;
    let ty = infer_type_in_ctx(ctx, &value);
    // If the name is already bound, this is a reassignment — emit
    // `Stmt::Assign` (the backend will write `name = value;` and the
    // earlier `Let` will be `let mut`). Otherwise, fresh `Let`.
    if ctx.bound.contains(&name) {
        Ok(Stmt::Assign { name, value })
    } else {
        let mutable = ctx.mutable.contains(&name);
        ctx.bound.insert(name.clone());
        ctx.name_types.insert(name.clone(), ty.clone());
        Ok(Stmt::Let {
            name,
            ty,
            value,
            mutable,
        })
    }
}

/// PMAT-470 (R1): lower an augmented assignment `x <op>= e` to the
/// reassignment `x = x <op> e`, reusing the existing `BinOp` machinery
/// (so overflow checking, str-concat detection, etc. apply uniformly).
/// No meta-HIR or backend change. Subscript targets (`d[k] += e`) are
/// not handled here — use the explicit `d[k] = d[k] + e` form.
/// Combine the current value `lhs` with `rhs` under `op` for an augmented
/// assignment, mirroring `lower_expr_in_ctx`'s str-concat detection so
/// `+=` on strings lowers to `Concat` (format!), not a `checked_add`.
fn combine_aug(ctx: &LoweringCtx, op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    if matches!(op, BinOp::Add)
        && (infer_type_in_ctx(ctx, &lhs) == Type::Str || infer_type_in_ctx(ctx, &rhs) == Type::Str)
    {
        Expr::Concat {
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }
    } else {
        Expr::BinOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }
    }
}

fn lower_aug_assign(ctx: &mut LoweringCtx, aug: ast::StmtAugAssign) -> Result<Stmt, FrontendError> {
    let op = lower_binop(&aug.op)?;
    let rhs = lower_expr_in_ctx(ctx, (*aug.value).clone())?;
    match aug.target.as_ref() {
        ast::Expr::Name(n) => {
            let name = n.id.to_string();
            if !ctx.bound.contains(&name) {
                return Err(FrontendError::Lower(format!(
                    "function `{}` augments `{name}` (`{name} <op>= …`) before it is assigned — initialise `{name}` first",
                    ctx.fn_name
                )));
            }
            let value = combine_aug(ctx, op, Expr::Ident(name.clone()), rhs);
            Ok(Stmt::Assign { name, value })
        }
        // PMAT-497: augmented subscript assignment `d[k] += v` /
        // `xs[i] += v` — desugar to `d[k] = d[k] <op> v`, reusing the
        // shipped DictGet/Index reads + DictSet/IndexAssign writes.
        ast::Expr::Subscript(sub) => {
            let receiver = match sub.value.as_ref() {
                ast::Expr::Name(n) => n.id.to_string(),
                _ => {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` augments a non-Name subscript target — v0.2.0 supports `<name>[k] <op>= v`",
                        ctx.fn_name
                    )));
                }
            };
            match ctx.name_types.get(&receiver).cloned() {
                Some(Type::Dict(_, _)) => {
                    let key = lower_expr_in_ctx(ctx, (*sub.slice).clone())?;
                    let current = Expr::DictGet {
                        dict: Box::new(Expr::Ident(receiver.clone())),
                        key: Box::new(key.clone()),
                    };
                    let value = combine_aug(ctx, op, current, rhs);
                    ctx.mutable.insert(receiver.clone());
                    Ok(Stmt::DictSet {
                        dict_name: receiver,
                        key,
                        value,
                    })
                }
                Some(Type::List(_)) => {
                    let index = lower_expr_in_ctx(ctx, (*sub.slice).clone())?;
                    if !matches!(infer_type_in_ctx(ctx, &index), Type::I64) {
                        return Err(FrontendError::Lower(format!(
                            "function `{}` augments `{receiver}[<expr>]` with a non-int index",
                            ctx.fn_name
                        )));
                    }
                    let current = Expr::Index {
                        collection: Box::new(Expr::Ident(receiver.clone())),
                        index: Box::new(index.clone()),
                    };
                    let value = combine_aug(ctx, op, current, rhs);
                    ctx.mutable.insert(receiver.clone());
                    Ok(Stmt::IndexAssign {
                        list_name: receiver,
                        index,
                        value,
                    })
                }
                _ => Err(FrontendError::Lower(format!(
                    "function `{}` augments `{receiver}[...]` which doesn't type as list[T] or dict[K, V]",
                    ctx.fn_name
                ))),
            }
        }
        _ => Err(FrontendError::Lower(format!(
            "function `{}` uses augmented assignment on an unsupported target — supported: `name <op>= e`, `d[k] <op>= e`, `xs[i] <op>= e`",
            ctx.fn_name
        ))),
    }
}

/// PMAT-473 (R4): desugar a list comprehension `[elem for var in iter]`
/// into the statements that build it: a fresh `let mut <target>: list[T]
/// = []` followed by `for var in iter { target.append(elem) }`. A
/// comprehension is an *expression* but the meta-HIR has no
/// block-expression, so it is materialised at statement level (in
/// assignment position, or hoisted to a temp in return position).
///
/// v0.2.0 slice: single generator, no `if` filter, iterable typing as a
/// `list[T]` (range/dict iterables and filters are deferred).
fn desugar_list_comp(
    ctx: &mut LoweringCtx,
    target: &str,
    comp: &ast::ExprListComp,
) -> Result<Vec<Stmt>, FrontendError> {
    if comp.generators.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses a multi-generator list comprehension — v0.2.0 supports a single `for` clause",
            ctx.fn_name
        )));
    }
    let gen = &comp.generators[0];
    if !gen.ifs.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses a filtered list comprehension (`[… for … if …]`) — deferred",
            ctx.fn_name
        )));
    }
    let var = match &gen.target {
        ast::Expr::Name(n) => n.id.to_string(),
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{}` has a non-Name comprehension target (tuple unpacking) — deferred",
                ctx.fn_name
            )));
        }
    };
    let iter_expr = lower_expr_in_ctx(ctx, gen.iter.clone())?;
    let elem_in_ty = match infer_type_in_ctx(ctx, &iter_expr) {
        Type::List(e) => *e,
        other => {
            return Err(FrontendError::Lower(format!(
                "function `{}` comprehends over an iterable typing as {other:?}; v0.2.0 supports `[… for x in <list[T]>]` (range/dict iterables deferred)",
                ctx.fn_name
            )));
        }
    };
    // Bind the loop var so the element expression types correctly.
    ctx.bound.insert(var.clone());
    ctx.name_types.insert(var.clone(), elem_in_ty.clone());
    let elem = lower_expr_in_ctx(ctx, (*comp.elt).clone())?;
    let out_ty = infer_type_in_ctx(ctx, &elem);
    let list_ty = Type::List(Box::new(out_ty));
    // Register the accumulator so later references type as the list.
    ctx.bound.insert(target.to_string());
    ctx.name_types.insert(target.to_string(), list_ty.clone());
    Ok(vec![
        Stmt::Let {
            name: target.to_string(),
            ty: list_ty,
            value: Expr::ListLit(Vec::new()),
            mutable: true,
        },
        Stmt::ForEach {
            var,
            iter: iter_expr,
            elem_ty: elem_in_ty,
            body: vec![Stmt::ListAppend {
                list_name: target.to_string(),
                elem,
            }],
            over_keys: false,
        },
    ])
}

/// PMAT-501: desugar a dict comprehension `{k: v for x in iter}` into
/// `let mut <target>: dict[K, V] = {}` + `for x in iter { <target>[k] = v }`
/// — the same materialisation as [`desugar_list_comp`] but with a
/// `Stmt::DictSet` insert instead of an append. Single generator, no
/// filter, list-typed iterable (the list-comp slice's restrictions).
fn desugar_dict_comp(
    ctx: &mut LoweringCtx,
    target: &str,
    comp: &ast::ExprDictComp,
) -> Result<Vec<Stmt>, FrontendError> {
    if comp.generators.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses a multi-generator dict comprehension — v0.2.0 supports a single `for` clause",
            ctx.fn_name
        )));
    }
    let gen = &comp.generators[0];
    if !gen.ifs.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses a filtered dict comprehension (`{{… if …}}`) — deferred",
            ctx.fn_name
        )));
    }
    let var = match &gen.target {
        ast::Expr::Name(n) => n.id.to_string(),
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{}` has a non-Name dict-comprehension target (tuple unpacking) — deferred",
                ctx.fn_name
            )));
        }
    };
    let iter_expr = lower_expr_in_ctx(ctx, gen.iter.clone())?;
    let elem_in_ty = match infer_type_in_ctx(ctx, &iter_expr) {
        Type::List(e) => *e,
        other => {
            return Err(FrontendError::Lower(format!(
                "function `{}` dict-comprehends over an iterable typing as {other:?}; v0.2.0 supports `{{… for x in <list[T]>}}`",
                ctx.fn_name
            )));
        }
    };
    ctx.bound.insert(var.clone());
    ctx.name_types.insert(var.clone(), elem_in_ty.clone());
    let key = lower_expr_in_ctx(ctx, (*comp.key).clone())?;
    let value = lower_expr_in_ctx(ctx, (*comp.value).clone())?;
    let k_ty = infer_type_in_ctx(ctx, &key);
    let v_ty = infer_type_in_ctx(ctx, &value);
    let dict_ty = Type::Dict(Box::new(k_ty), Box::new(v_ty));
    ctx.bound.insert(target.to_string());
    ctx.name_types.insert(target.to_string(), dict_ty.clone());
    Ok(vec![
        Stmt::Let {
            name: target.to_string(),
            ty: dict_ty,
            value: Expr::DictLit(Vec::new()),
            mutable: true,
        },
        Stmt::ForEach {
            var,
            iter: iter_expr,
            elem_ty: elem_in_ty,
            body: vec![Stmt::DictSet {
                dict_name: target.to_string(),
                key,
                value,
            }],
            over_keys: false,
        },
    ])
}

/// PMAT-501b: desugar a set comprehension `{e for x in iter}` into
/// `let mut <target>: set[T] = set()` + `for x in iter { <target>.add(e) }`
/// — the same materialisation as [`desugar_dict_comp`] but with a
/// `Stmt::SetAdd` insert into an empty-`SetLit` accumulator.
fn desugar_set_comp(
    ctx: &mut LoweringCtx,
    target: &str,
    comp: &ast::ExprSetComp,
) -> Result<Vec<Stmt>, FrontendError> {
    if comp.generators.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses a multi-generator set comprehension — v0.2.0 supports a single `for` clause",
            ctx.fn_name
        )));
    }
    let gen = &comp.generators[0];
    if !gen.ifs.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses a filtered set comprehension (`{{… if …}}`) — deferred",
            ctx.fn_name
        )));
    }
    let var = match &gen.target {
        ast::Expr::Name(n) => n.id.to_string(),
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{}` has a non-Name set-comprehension target (tuple unpacking) — deferred",
                ctx.fn_name
            )));
        }
    };
    let iter_expr = lower_expr_in_ctx(ctx, gen.iter.clone())?;
    let elem_in_ty = match infer_type_in_ctx(ctx, &iter_expr) {
        Type::List(e) => *e,
        other => {
            return Err(FrontendError::Lower(format!(
                "function `{}` set-comprehends over an iterable typing as {other:?}; v0.2.0 supports `{{… for x in <list[T]>}}`",
                ctx.fn_name
            )));
        }
    };
    ctx.bound.insert(var.clone());
    ctx.name_types.insert(var.clone(), elem_in_ty.clone());
    let elem = lower_expr_in_ctx(ctx, (*comp.elt).clone())?;
    let out_ty = infer_type_in_ctx(ctx, &elem);
    let set_ty = Type::Set(Box::new(out_ty));
    ctx.bound.insert(target.to_string());
    ctx.name_types.insert(target.to_string(), set_ty.clone());
    Ok(vec![
        Stmt::Let {
            name: target.to_string(),
            ty: set_ty,
            value: Expr::SetLit(Vec::new()),
            mutable: true,
        },
        Stmt::ForEach {
            var,
            iter: iter_expr,
            elem_ty: elem_in_ty,
            body: vec![Stmt::SetAdd {
                set_name: target.to_string(),
                elem,
            }],
            over_keys: false,
        },
    ])
}

/// PMAT-474 (R5): rewrite a call with keyword arguments `f(x=1, y=2)`
/// into a plain positional call using the callee's declared parameter
/// order (from the module signature table). Calls without keywords pass
/// through unchanged. Default arguments are not supported, so every
/// parameter must be supplied (positionally or by keyword).
fn reorder_kwargs_to_positional(
    ctx: &LoweringCtx,
    call: ast::ExprCall,
) -> Result<ast::ExprCall, FrontendError> {
    if call.keywords.is_empty() {
        return Ok(call);
    }
    if call.keywords.iter().any(|k| k.arg.is_none()) {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses `**kwargs` unpacking in a call — not supported",
            ctx.fn_name
        )));
    }
    let callee = match call.func.as_ref() {
        ast::Expr::Name(n) => n.id.to_string(),
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{}` passes keyword args to a non-Name callee — only `f(x=…)` to a top-level function is supported",
                ctx.fn_name
            )));
        }
    };
    let sig = ctx.signatures.get(&callee).ok_or_else(|| {
        FrontendError::Lower(format!(
            "function `{}` passes keyword args to unknown function `{callee}` — only top-level functions in this module support keyword calls",
            ctx.fn_name
        ))
    })?;
    let n_pos = call.args.len();
    if n_pos > sig.params.len() {
        return Err(FrontendError::Lower(format!(
            "function `{}` calls `{callee}` with {n_pos} positional args but it declares {} params",
            ctx.fn_name,
            sig.params.len()
        )));
    }
    // Positional args fill params[0..n_pos]; each remaining param is
    // filled from its matching keyword (in declared order).
    let mut new_args = call.args.clone();
    for pname in &sig.params[n_pos..] {
        match call
            .keywords
            .iter()
            .find(|k| k.arg.as_ref().map(|a| a.as_str()) == Some(pname.as_str()))
        {
            Some(k) => new_args.push(k.value.clone()),
            None => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` calls `{callee}` missing argument `{pname}` — default arguments are not supported at v0.2.0 (supply every argument)",
                    ctx.fn_name
                )));
            }
        }
    }
    if call.keywords.len() != sig.params.len() - n_pos {
        return Err(FrontendError::Lower(format!(
            "function `{}` calls `{callee}` with a keyword naming an unknown parameter or one already filled positionally",
            ctx.fn_name
        )));
    }
    Ok(ast::ExprCall {
        range: call.range,
        func: call.func,
        args: new_args,
        keywords: Vec::new(),
    })
}

/// PMAT-466 (v0.2.0 Track 1.C): lower an annotated local assignment
/// `name: T = value`. The annotation is authoritative for the
/// binding's type — notably, an annotated empty dict
/// `counts: dict[K, V] = {}` lowers to `DictLit(vec![])` typed by the
/// annotation, the only way to introduce an empty dict (the value
/// alone can't infer K/V). Non-empty / non-dict values are lowered
/// through the context-aware path and must agree with the annotation.
fn lower_ann_assign(ctx: &mut LoweringCtx, aa: ast::StmtAnnAssign) -> Result<Stmt, FrontendError> {
    let name = match aa.target.as_ref() {
        ast::Expr::Name(n) => n.id.to_string(),
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{}` has a non-Name annotated-assignment target — v0.2.0 supports `name: T = value` only",
                ctx.fn_name
            )));
        }
    };
    let declared_ty = parse_type_annotation(&ctx.fn_name, &name, &aa.annotation)?;
    let value_expr = aa.value.ok_or_else(|| {
        FrontendError::Lower(format!(
            "function `{}` declares `{name}: {declared_ty:?}` without an initializer — v0.2.0 requires `name: T = value`",
            ctx.fn_name
        ))
    })?;
    // Empty dict literal: the annotation supplies K/V that the value
    // can't. Any other empty-collection / annotation combination is
    // rejected here rather than silently mis-typing.
    let value = if let ast::Expr::Dict(d) = value_expr.as_ref() {
        if d.keys.is_empty() {
            if !matches!(declared_ty, Type::Dict(_, _)) {
                return Err(FrontendError::Lower(format!(
                    "function `{}` assigns empty `{{}}` to `{name}` annotated as {declared_ty:?}; an empty literal requires a `dict[K, V]` annotation",
                    ctx.fn_name
                )));
            }
            Expr::DictLit(Vec::new())
        } else {
            lower_expr_in_ctx(ctx, (*value_expr).clone())?
        }
    } else {
        lower_expr_in_ctx(ctx, (*value_expr).clone())?
    };
    // PMAT-466 (review #3): reject an obvious annotation/initializer
    // KIND mismatch when the value is a literal (kind known exactly) —
    // e.g. `x: dict[int,int] = 5`, `x: int = [1, 2]`. Non-literal values
    // keep trusting the annotation (v0.2.0 inference is too thin to
    // judge a call/ident without false positives).
    let value_lit_kind = match &value {
        Expr::ListLit(_) => Some("list[T]"),
        Expr::DictLit(pairs) if !pairs.is_empty() => Some("dict[K, V]"),
        Expr::LitInt(_) | Expr::LitBool(_) | Expr::LitStr(_) | Expr::Concat { .. } => {
            Some("a scalar")
        }
        _ => None,
    };
    let declared_kind = match declared_ty {
        Type::List(_) => "list[T]",
        Type::Dict(_, _) => "dict[K, V]",
        _ => "a scalar",
    };
    if let Some(vk) = value_lit_kind {
        if vk != declared_kind {
            return Err(FrontendError::Lower(format!(
                "function `{}` annotates `{name}` as {declared_ty:?} ({declared_kind}) but its initializer is {vk} — the annotation and value must agree",
                ctx.fn_name
            )));
        }
    }
    // Annotation is the source of truth for the binding type (an empty
    // DictLit would otherwise infer the wrong K/V). For non-empty
    // values we trust the annotation and let backend compilation catch
    // any genuine mismatch — the v0.2.0 inference is intentionally thin.
    let mutable = ctx.mutable.contains(&name);
    ctx.bound.insert(name.clone());
    ctx.name_types.insert(name.clone(), declared_ty.clone());
    Ok(Stmt::Let {
        name,
        ty: declared_ty,
        value,
        mutable,
    })
}

/// PMAT-466 (review #8): true if any expression in the lowered body
/// uses a dict construct (read, get-with-default, membership, keyed
/// assignment, or literal). Drives the BigInt-mode + dict rejection.
fn body_uses_dict(body: &Block) -> bool {
    body.stmts.iter().any(stmt_uses_dict) || expr_uses_dict(&body.trailing_return)
}

fn stmt_uses_dict(s: &Stmt) -> bool {
    match s {
        Stmt::DictSet { .. } => true,
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } => expr_uses_dict(value),
        Stmt::While { cond, body } => expr_uses_dict(cond) || body.iter().any(stmt_uses_dict),
        Stmt::ForEach { iter, body, .. } => expr_uses_dict(iter) || body.iter().any(stmt_uses_dict),
        Stmt::ListAppend { elem, .. } => expr_uses_dict(elem),
        Stmt::IndexAssign { index, value, .. } => expr_uses_dict(index) || expr_uses_dict(value),
        Stmt::Assert { cond } => expr_uses_dict(cond),
        _ => false,
    }
}

fn expr_uses_dict(e: &Expr) -> bool {
    match e {
        Expr::DictGet { .. }
        | Expr::DictGetOr { .. }
        | Expr::DictContains { .. }
        | Expr::DictLit(_) => true,
        Expr::BinOp { lhs, rhs, .. } | Expr::Concat { lhs, rhs } => {
            expr_uses_dict(lhs) || expr_uses_dict(rhs)
        }
        Expr::UnOp { operand, .. } => expr_uses_dict(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => expr_uses_dict(cond) || expr_uses_dict(then_expr) || expr_uses_dict(else_expr),
        Expr::Call { args, .. } => args.iter().any(expr_uses_dict),
        Expr::Len(inner) => expr_uses_dict(inner),
        Expr::Index { collection, index } => expr_uses_dict(collection) || expr_uses_dict(index),
        Expr::ListLit(elems) => elems.iter().any(expr_uses_dict),
        _ => false,
    }
}

/// Context-free type inference. Used in spots where ctx isn't readily
/// available (e.g., cond-is-Bool checks where the Bool/I64 distinction
/// is what matters and BigInt-vs-I64 doesn't). Idents default to I64.
/// For BigInt-aware inference inside a function body, use
/// [`infer_type_in_ctx`].
fn infer_type(e: &Expr) -> Type {
    match e {
        Expr::Ident(_) | Expr::LitInt(_) => Type::I64,
        // PMAT-477 (R8): float literal + float arithmetic are Type::F64.
        Expr::LitFloat(_) | Expr::FloatBinOp { .. } => Type::F64,
        // PMAT-456 (v0.2.0 Track 1.B): bool literal is Type::Bool.
        Expr::LitBool(_) => Type::Bool,
        // PMAT-459 (v0.2.0 Track 1.B): len(x) always returns Type::I64
        // (Python int).
        Expr::Len(_) => Type::I64,
        Expr::BinOp { op, .. } => match op {
            BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::FloorDiv
            | BinOp::Mod
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::Pow => Type::I64,
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => {
                Type::Bool
            }
            BinOp::And | BinOp::Or => Type::Bool,
        },
        Expr::IfExpr { then_expr, .. } => infer_type(then_expr),
        // Without a cross-function signature table, assume calls return I64
        // (matches the v0.1.0 invariant that every transpiled fn returns
        // I64 or Bool, and only one of those is possible for an arithmetic
        // computation).
        Expr::Call { .. } => Type::I64,
        Expr::UnOp { op, .. } => match op {
            UnOp::Neg => Type::I64,
            UnOp::Not => Type::Bool,
        },
        // PMAT-449 (v0.2.0 Track 1.A): Python `"..."` literal now
        // produces `Expr::LitStr` and is typed as `Type::Str`.
        Expr::LitStr(_) => Type::Str,
        // PMAT-451: str concatenation is a Type::Str-producing op.
        Expr::Concat { .. } => Type::Str,
        // PMAT-492: string transform methods (upper/lower/strip) → Str.
        Expr::StrMethod { op, .. } => match op {
            StrMethodOp::Upper | StrMethodOp::Lower | StrMethodOp::Strip => Type::Str,
            StrMethodOp::StartsWith | StrMethodOp::EndsWith => Type::Bool,
            StrMethodOp::Split => Type::List(Box::new(Type::Str)),
            StrMethodOp::Join | StrMethodOp::Replace => Type::Str,
            // PMAT-502l: lstrip/rstrip → Str; find/count → Int.
            StrMethodOp::LStrip | StrMethodOp::RStrip => Type::Str,
            StrMethodOp::Find | StrMethodOp::Count => Type::I64,
        },
        // PMAT-455 (v0.2.0 Track 1.B): list literal infers element
        // type from the first element (frontend ensures homogeneity
        // at lowering time). Empty literal is conservatively typed as
        // List I64 — the frontend rejects empty literals without an
        // annotation, so this path is only reached for non-empty.
        Expr::ListLit(elems) => {
            let elem_ty = elems.first().map(infer_type).unwrap_or(Type::I64);
            Type::List(Box::new(elem_ty))
        }
        // PMAT-500: set literal / membership.
        Expr::SetLit(elems) => {
            Type::Set(Box::new(elems.first().map(infer_type).unwrap_or(Type::I64)))
        }
        Expr::SetContains { .. } => Type::Bool,
        // PMAT-502o: str substring containment -> Bool.
        Expr::StrContains { .. } => Type::Bool,
        // PMAT-502g: set algebra preserves the operand set type.
        Expr::SetOp { lhs, .. } => infer_type(lhs),
        // PMAT-494: tuple literal → Type::Tuple of each element's type.
        Expr::TupleLit(elems) => Type::Tuple(elems.iter().map(infer_type).collect()),
        // PMAT-502q: tuple constant-index → the N-th element type.
        Expr::TupleIndex { tuple, index } => match infer_type(tuple) {
            Type::Tuple(elems) => elems.get(*index).cloned().unwrap_or(Type::I64),
            _ => Type::I64,
        },
        // PMAT-496: a slice has the same type as its collection.
        Expr::Slice { collection, .. } => infer_type(collection),
        // PMAT-498: numeric builtin types as its first argument.
        Expr::NumBuiltin { args, .. } => args.first().map(infer_type).unwrap_or(Type::I64),
        // PMAT-498b: sum types as the list's element type.
        Expr::Sum { of_float, .. } => {
            if *of_float {
                Type::F64
            } else {
                Type::I64
            }
        }
        // PMAT-502j: all(xs)/any(xs) reduce a bool list to a Bool.
        Expr::BoolReduce { .. } => Type::Bool,
        // PMAT-502m: int(x)/float(x) type as I64/F64 respectively.
        Expr::NumCast { to_float, .. } => {
            if *to_float {
                Type::F64
            } else {
                Type::I64
            }
        }
        // PMAT-502k: seq * n has the same type as the sequence.
        Expr::Repeat { seq, .. } => infer_type(seq),
        // PMAT-502c: sorted(xs) has the same type as its list.
        Expr::Sorted { list, .. } => infer_type(list),
        // PMAT-502d: reversed(xs) has the same type as its list.
        Expr::Reversed { list } => infer_type(list),
        // PMAT-502e: min(xs)/max(xs) reduce a list to its element type.
        Expr::ListMinMax { list, .. } => match infer_type(list) {
            Type::List(elem) => *elem,
            _ => Type::I64,
        },
        // PMAT-502u: list.count(x)/index(x) return Int.
        Expr::ListQuery { .. } => Type::I64,
        // PMAT-502v/502x: d.keys()/d.values()/d.items() materialize to
        // List(K)/List(V)/List(Tuple[K, V]).
        Expr::DictView { dict, kind } => match infer_type(dict) {
            Type::Dict(k, v) => Type::List(match kind {
                DictViewKind::Keys => k,
                DictViewKind::Values => v,
                DictViewKind::Items => Box::new(Type::Tuple(vec![*k, *v])),
            }),
            _ => Type::List(Box::new(Type::I64)),
        },
        // PMAT-457 (v0.2.0 Track 1.B): indexed access returns the
        // collection's element type. If the collection types as
        // Type::List(T), the result is T; otherwise fall back to I64
        // (defensive — frontend only emits Index when typing succeeds).
        Expr::Index { collection, .. } => match infer_type(collection) {
            Type::List(elem_ty) => *elem_ty,
            Type::Dict(_, value_ty) => *value_ty,
            _ => Type::I64,
        },
        // PMAT-466 (v0.2.0 Track 1.C): dict read + get-with-default
        // return the dict's value type; membership is Bool.
        Expr::DictGet { dict, .. } | Expr::DictGetOr { dict, .. } => match infer_type(dict) {
            Type::Dict(_, value_ty) => *value_ty,
            _ => Type::I64,
        },
        Expr::DictContains { .. } => Type::Bool,
        // PMAT-462 (v0.2.0 Track 1.C): dict literal types as
        // Type::Dict over the inferred key + value types from the
        // first pair. Frontend enforces homogeneity at lowering time.
        Expr::DictLit(pairs) => {
            let (k_ty, v_ty) = pairs
                .first()
                .map(|(k, v)| (infer_type(k), infer_type(v)))
                .unwrap_or((Type::Str, Type::I64));
            Type::Dict(Box::new(k_ty), Box::new(v_ty))
        }
        // PMAT-042 + PMAT-045 + PMAT-047 + PMAT-055: shell-domain
        // Expr variants don't appear inside Python-frontend lowering.
        Expr::QuotedString { .. }
        | Expr::ShellVar(_)
        | Expr::CommandSubstitution(_)
        | Expr::ShellSpecial(_) => Type::I64,
    }
}

/// Context-aware type inference. Looks Idents up in `ctx.name_types`,
/// propagates BigInt through arithmetic operands (BigInt + anything
/// → BigInt), types self-recursive calls as the enclosing function's
/// return type, and (PMAT-013) lifts integer literals to BigInt when
/// the enclosing function is BigInt-typed — without this lift, a
/// trailing `return 1 if n <= 1 else ...` would infer I64 for the
/// `1` branch and fail the return-type check.
fn infer_type_in_ctx(ctx: &LoweringCtx, e: &Expr) -> Type {
    match e {
        Expr::Ident(n) => ctx.name_types.get(n).cloned().unwrap_or(Type::I64),
        // PMAT-477 (R8): float literal + float arithmetic are Type::F64.
        Expr::LitFloat(_) | Expr::FloatBinOp { .. } => Type::F64,
        // PMAT-456 (v0.2.0 Track 1.B): bool literal is Type::Bool.
        Expr::LitBool(_) => Type::Bool,
        // PMAT-459: len() always returns Type::I64.
        Expr::Len(_) => Type::I64,
        Expr::LitInt(_) => {
            if matches!(ctx.fn_return_type, Type::BigInt) {
                Type::BigInt
            } else {
                Type::I64
            }
        }
        Expr::BinOp { op, lhs, rhs } => match op {
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => {
                Type::Bool
            }
            BinOp::And | BinOp::Or => Type::Bool,
            _ => {
                let lt = infer_type_in_ctx(ctx, lhs);
                let rt = infer_type_in_ctx(ctx, rhs);
                if matches!(lt, Type::BigInt) || matches!(rt, Type::BigInt) {
                    Type::BigInt
                } else {
                    Type::I64
                }
            }
        },
        Expr::IfExpr { then_expr, .. } => infer_type_in_ctx(ctx, then_expr),
        // PMAT-471 (R2): consult the module signature table first so a
        // call to another function gets its real declared return type
        // (e.g. `make_dict()` → Type::Dict, not the old I64 fallback
        // that silently emitted `let d: i64`). The table includes the
        // enclosing function, so self-recursion is covered too.
        Expr::Call { callee, .. } => ctx
            .signatures
            .get(callee)
            .map(|s| s.ret.clone())
            .unwrap_or_else(|| {
                if callee == &ctx.fn_name {
                    ctx.fn_return_type.clone()
                } else {
                    Type::I64
                }
            }),
        Expr::UnOp { op, operand } => match op {
            UnOp::Neg => infer_type_in_ctx(ctx, operand),
            UnOp::Not => Type::Bool,
        },
        // PMAT-449 (v0.2.0 Track 1.A): Python `"..."` literal is
        // typed as `Type::Str`.
        Expr::LitStr(_) => Type::Str,
        // PMAT-451: str concatenation is Type::Str-producing.
        Expr::Concat { .. } => Type::Str,
        // PMAT-492: string transform methods (upper/lower/strip) → Str.
        Expr::StrMethod { op, .. } => match op {
            StrMethodOp::Upper | StrMethodOp::Lower | StrMethodOp::Strip => Type::Str,
            StrMethodOp::StartsWith | StrMethodOp::EndsWith => Type::Bool,
            StrMethodOp::Split => Type::List(Box::new(Type::Str)),
            StrMethodOp::Join | StrMethodOp::Replace => Type::Str,
            // PMAT-502l: lstrip/rstrip → Str; find/count → Int.
            StrMethodOp::LStrip | StrMethodOp::RStrip => Type::Str,
            StrMethodOp::Find | StrMethodOp::Count => Type::I64,
        },
        // PMAT-455 (v0.2.0 Track 1.B): list literal — same inference
        // shape as the context-free `infer_type` arm.
        Expr::ListLit(elems) => {
            let elem_ty = elems
                .first()
                .map(|e| infer_type_in_ctx(ctx, e))
                .unwrap_or(Type::I64);
            Type::List(Box::new(elem_ty))
        }
        // PMAT-500: set literal / membership.
        Expr::SetLit(elems) => Type::Set(Box::new(
            elems
                .first()
                .map(|e| infer_type_in_ctx(ctx, e))
                .unwrap_or(Type::I64),
        )),
        Expr::SetContains { .. } => Type::Bool,
        // PMAT-502o: str substring containment -> Bool.
        Expr::StrContains { .. } => Type::Bool,
        // PMAT-502g: set algebra preserves the operand set type.
        Expr::SetOp { lhs, .. } => infer_type_in_ctx(ctx, lhs),
        // PMAT-494: tuple literal → Type::Tuple of each element's type.
        Expr::TupleLit(elems) => {
            Type::Tuple(elems.iter().map(|e| infer_type_in_ctx(ctx, e)).collect())
        }
        // PMAT-502q: tuple constant-index → the N-th element type.
        Expr::TupleIndex { tuple, index } => match infer_type_in_ctx(ctx, tuple) {
            Type::Tuple(elems) => elems.get(*index).cloned().unwrap_or(Type::I64),
            _ => Type::I64,
        },
        // PMAT-496: a slice has the same type as its collection.
        Expr::Slice { collection, .. } => infer_type_in_ctx(ctx, collection),
        // PMAT-498: numeric builtin types as its first argument.
        Expr::NumBuiltin { args, .. } => args
            .first()
            .map(|a| infer_type_in_ctx(ctx, a))
            .unwrap_or(Type::I64),
        // PMAT-498b: sum types as the list's element type.
        Expr::Sum { of_float, .. } => {
            if *of_float {
                Type::F64
            } else {
                Type::I64
            }
        }
        // PMAT-502j: all(xs)/any(xs) reduce a bool list to a Bool.
        Expr::BoolReduce { .. } => Type::Bool,
        // PMAT-502m: int(x)/float(x) type as I64/F64 respectively.
        Expr::NumCast { to_float, .. } => {
            if *to_float {
                Type::F64
            } else {
                Type::I64
            }
        }
        // PMAT-502k: seq * n has the same type as the sequence.
        Expr::Repeat { seq, .. } => infer_type_in_ctx(ctx, seq),
        // PMAT-502c: sorted(xs) has the same type as its list.
        Expr::Sorted { list, .. } => infer_type_in_ctx(ctx, list),
        // PMAT-502d: reversed(xs) has the same type as its list.
        Expr::Reversed { list } => infer_type_in_ctx(ctx, list),
        // PMAT-502e: min(xs)/max(xs) reduce a list to its element type.
        Expr::ListMinMax { list, .. } => match infer_type_in_ctx(ctx, list) {
            Type::List(elem) => *elem,
            _ => Type::I64,
        },
        // PMAT-502u: list.count(x)/index(x) return Int.
        Expr::ListQuery { .. } => Type::I64,
        // PMAT-502v: d.keys()/d.values() materialize to List(K)/List(V).
        Expr::DictView { dict, kind } => match infer_type_in_ctx(ctx, dict) {
            Type::Dict(k, v) => Type::List(match kind {
                DictViewKind::Keys => k,
                DictViewKind::Values => v,
                DictViewKind::Items => Box::new(Type::Tuple(vec![*k, *v])),
            }),
            _ => Type::List(Box::new(Type::I64)),
        },
        // PMAT-457: indexed access returns the collection element type.
        Expr::Index { collection, .. } => match infer_type_in_ctx(ctx, collection) {
            Type::List(elem_ty) => *elem_ty,
            Type::Dict(_, value_ty) => *value_ty,
            _ => Type::I64,
        },
        // PMAT-466 (v0.2.0 Track 1.C): dict read + get-with-default
        // return the dict value type; membership is Bool.
        Expr::DictGet { dict, .. } | Expr::DictGetOr { dict, .. } => {
            match infer_type_in_ctx(ctx, dict) {
                Type::Dict(_, value_ty) => *value_ty,
                _ => Type::I64,
            }
        }
        Expr::DictContains { .. } => Type::Bool,
        // PMAT-462: dict literal — see twin arm in `infer_type` above.
        Expr::DictLit(pairs) => {
            let (k_ty, v_ty) = pairs
                .first()
                .map(|(k, v)| (infer_type_in_ctx(ctx, k), infer_type_in_ctx(ctx, v)))
                .unwrap_or((Type::Str, Type::I64));
            Type::Dict(Box::new(k_ty), Box::new(v_ty))
        }
        Expr::QuotedString { .. }
        | Expr::ShellVar(_)
        | Expr::CommandSubstitution(_)
        | Expr::ShellSpecial(_) => Type::I64,
    }
}

/// PMAT-466 (v0.2.0 Track 1.C operations): context-aware expression
/// lowering used for every function-body expression.
///
/// Two stages: (1) [`lower_expr_in_ctx_inner`] dispatches the dict-only
/// *constructing* shapes — `d.get(k, default)` and `k in d` membership
/// (which the context-free [`lower_expr`] would reject) plus the
/// directly-recursed `d[k]` and BinOp cases. (2) [`rewrite_dict_reads`]
/// then walks the whole lowered tree and repairs any *remaining*
/// `Expr::Index` whose collection types as a dict into an
/// `Expr::DictGet`. Stage 2 is what makes `d[k]` reads correct in EVERY
/// position (call args, ternary branches, `len(...)` args, relational
/// operands, …), not just the few the inner pass recurses through.
fn lower_expr_in_ctx(ctx: &LoweringCtx, e: ast::Expr) -> Result<Expr, FrontendError> {
    Ok(rewrite_dict_reads(ctx, lower_expr_in_ctx_inner(ctx, e)?))
}

/// PMAT-466: post-lowering rewrite — `Expr::Index` over a dict-typed
/// collection becomes `Expr::DictGet`. Recurses through every compound
/// `Expr`. Idempotent (a tree with no dict-typed `Index` is returned
/// unchanged).
fn rewrite_dict_reads(ctx: &LoweringCtx, e: Expr) -> Expr {
    let rw = |x: Expr| rewrite_dict_reads(ctx, x);
    let rwb = |x: Box<Expr>| Box::new(rewrite_dict_reads(ctx, *x));
    match e {
        Expr::Index { collection, index } => {
            let collection = rwb(collection);
            let index = rwb(index);
            if matches!(infer_type_in_ctx(ctx, &collection), Type::Dict(_, _)) {
                Expr::DictGet {
                    dict: collection,
                    key: index,
                }
            } else {
                Expr::Index { collection, index }
            }
        }
        Expr::DictGet { dict, key } => Expr::DictGet {
            dict: rwb(dict),
            key: rwb(key),
        },
        Expr::DictContains { dict, key } => Expr::DictContains {
            dict: rwb(dict),
            key: rwb(key),
        },
        Expr::DictGetOr { dict, key, default } => Expr::DictGetOr {
            dict: rwb(dict),
            key: rwb(key),
            default: rwb(default),
        },
        Expr::BinOp { op, lhs, rhs } => Expr::BinOp {
            op,
            lhs: rwb(lhs),
            rhs: rwb(rhs),
        },
        Expr::Concat { lhs, rhs } => Expr::Concat {
            lhs: rwb(lhs),
            rhs: rwb(rhs),
        },
        Expr::UnOp { op, operand } => Expr::UnOp {
            op,
            operand: rwb(operand),
        },
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => Expr::IfExpr {
            cond: rwb(cond),
            then_expr: rwb(then_expr),
            else_expr: rwb(else_expr),
        },
        Expr::Call { callee, args } => Expr::Call {
            callee,
            args: args.into_iter().map(rw).collect(),
        },
        Expr::Len(inner) => Expr::Len(rwb(inner)),
        Expr::ListLit(elems) => Expr::ListLit(elems.into_iter().map(rw).collect()),
        Expr::DictLit(pairs) => {
            Expr::DictLit(pairs.into_iter().map(|(k, v)| (rw(k), rw(v))).collect())
        }
        // Leaves + shell-domain variants carry no nested dict reads.
        other => other,
    }
}

fn lower_expr_in_ctx_inner(ctx: &LoweringCtx, e: ast::Expr) -> Result<Expr, FrontendError> {
    match e {
        // `d[k]` read → `Expr::DictGet` when the receiver types as a
        // dict; otherwise fall through to the context-free list path.
        ast::Expr::Subscript(sub) => {
            let collection = lower_expr_in_ctx(ctx, (*sub.value).clone())?;
            // PMAT-496: `xs[lo:hi]` slice (the subscript is a Slice node).
            if let ast::Expr::Slice(slice) = sub.slice.as_ref() {
                return lower_slice_in_ctx(ctx, collection, slice);
            }
            if matches!(infer_type_in_ctx(ctx, &collection), Type::Dict(_, _)) {
                let key = lower_expr_in_ctx(ctx, (*sub.slice).clone())?;
                return Ok(Expr::DictGet {
                    dict: Box::new(collection),
                    key: Box::new(key),
                });
            }
            // PMAT-502q: `t[N]` over a Tuple-typed `t` with a compile-time
            // non-negative literal N in range → field access `t.N` (Rust
            // tuples don't support `[]` indexing). Out-of-range / non-literal
            // / negative indices fall through to the list-index path's error.
            if let Type::Tuple(elem_tys) = infer_type_in_ctx(ctx, &collection) {
                if let ast::Expr::Constant(c) = sub.slice.as_ref() {
                    if let ast::Constant::Int(n) = &c.value {
                        if let Some(idx) = n.to_string().parse::<i64>().ok().filter(|i| *i >= 0) {
                            let idx = idx as usize;
                            if idx < elem_tys.len() {
                                return Ok(Expr::TupleIndex {
                                    tuple: Box::new(collection),
                                    index: idx,
                                });
                            }
                        }
                    }
                }
            }
            // PMAT-502s: negative list index `xs[-k]` → `xs[len(xs) - k]`
            // (Python's from-the-end indexing). Pure desugar reusing
            // `Expr::Len` + `BinOp::Sub` + `Expr::Index`, so the resulting
            // index inherits the C-PY-INT-ARITH checked subtraction. The
            // collection appears twice (in the length and the index target);
            // v0.1.0 collections are pure, so the reuse is sound. A negative
            // literal parses as `UnaryOp(USub, Int(k))`.
            if matches!(infer_type_in_ctx(ctx, &collection), Type::List(_)) {
                if let ast::Expr::UnaryOp(u) = sub.slice.as_ref() {
                    if matches!(u.op, ast::UnaryOp::USub) {
                        if let ast::Expr::Constant(c) = u.operand.as_ref() {
                            if let ast::Constant::Int(k) = &c.value {
                                if let Ok(k) = k.to_string().parse::<i64>() {
                                    let index = Expr::BinOp {
                                        op: BinOp::Sub,
                                        lhs: Box::new(Expr::Len(Box::new(collection.clone()))),
                                        rhs: Box::new(Expr::LitInt(k)),
                                    };
                                    return Ok(Expr::Index {
                                        collection: Box::new(collection),
                                        index: Box::new(index),
                                    });
                                }
                            }
                        }
                    }
                }
            }
            lower_expr(ast::Expr::Subscript(sub))
        }
        // `d.get(k, default)` → `Expr::DictGetOr` when `d` is a dict.
        ast::Expr::Call(call) => {
            if let ast::Expr::Attribute(attr) = call.func.as_ref() {
                if attr.attr.as_str() == "get" {
                    let recv = lower_expr_in_ctx(ctx, (*attr.value).clone())?;
                    if matches!(infer_type_in_ctx(ctx, &recv), Type::Dict(_, _)) {
                        if !call.keywords.is_empty() || call.args.len() != 2 {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` calls dict `.get(...)` with {} positional arg(s){} \
                                 — v0.2.0 Track 1.C supports exactly `.get(key, default)`",
                                ctx.fn_name,
                                call.args.len(),
                                if call.keywords.is_empty() {
                                    ""
                                } else {
                                    " plus keyword args"
                                },
                            )));
                        }
                        let key = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                        let default = lower_expr_in_ctx(ctx, call.args[1].clone())?;
                        return Ok(Expr::DictGetOr {
                            dict: Box::new(recv),
                            key: Box::new(key),
                            default: Box::new(default),
                        });
                    }
                }
                // PMAT-502v/502x: dict view methods `d.keys()` / `d.values()`
                // / `d.items()` (0 args) → a materialized `Vec` of keys /
                // values / (k, v) tuples, when the receiver types as a dict.
                if matches!(attr.attr.as_str(), "keys" | "values" | "items") {
                    let recv = lower_expr_in_ctx(ctx, (*attr.value).clone())?;
                    if matches!(infer_type_in_ctx(ctx, &recv), Type::Dict(_, _))
                        && call.keywords.is_empty()
                        && call.args.is_empty()
                    {
                        return Ok(Expr::DictView {
                            dict: Box::new(recv),
                            kind: match attr.attr.as_str() {
                                "keys" => DictViewKind::Keys,
                                "values" => DictViewKind::Values,
                                _ => DictViewKind::Items,
                            },
                        });
                    }
                }
                // PMAT-492/493b: string methods — `s.upper()/.lower()/
                // .strip()` (0 args, → Str) and `s.startswith(p)/
                // .endswith(p)` (1 pattern arg, → Bool) — when the
                // receiver types as `Type::Str`.
                if let Some(op) = str_method_op(attr.attr.as_str()) {
                    let recv = lower_expr_in_ctx(ctx, (*attr.value).clone())?;
                    if matches!(infer_type_in_ctx(ctx, &recv), Type::Str) {
                        let arity = str_method_arity(op);
                        if !call.keywords.is_empty() || call.args.len() != arity {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` calls str `.{}(...)` with {} positional arg(s){}; \
                                 expected exactly {arity}",
                                ctx.fn_name,
                                attr.attr.as_str(),
                                call.args.len(),
                                if call.keywords.is_empty() {
                                    ""
                                } else {
                                    " plus keyword args"
                                },
                            )));
                        }
                        let args = call
                            .args
                            .iter()
                            .map(|a| lower_expr_in_ctx(ctx, a.clone()))
                            .collect::<Result<Vec<_>, _>>()?;
                        return Ok(Expr::StrMethod {
                            recv: Box::new(recv),
                            op,
                            args,
                        });
                    }
                }
                // PMAT-502u: list query methods `xs.count(x)` / `xs.index(x)`
                // over a `list[int]` (1 arg, → Int). `.count` also names a str
                // method (handled above for a Str receiver); the receiver-type
                // check disambiguates.
                if matches!(attr.attr.as_str(), "count" | "index") {
                    let recv = lower_expr_in_ctx(ctx, (*attr.value).clone())?;
                    if matches!(infer_type_in_ctx(ctx, &recv), Type::List(elem) if *elem == Type::I64)
                        && call.keywords.is_empty()
                        && call.args.len() == 1
                    {
                        let arg = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                        return Ok(Expr::ListQuery {
                            list: Box::new(recv),
                            op: if attr.attr.as_str() == "count" {
                                ListQueryOp::Count
                            } else {
                                ListQueryOp::Index
                            },
                            arg: Box::new(arg),
                        });
                    }
                }
            }
            // PMAT-498: scalar numeric builtins abs/min/max — when the
            // callee is one of these by name + arity and the first arg
            // types as a number, lower to `Expr::NumBuiltin`. Otherwise
            // fall through (e.g. a user fn named `min`).
            if let ast::Expr::Name(fname) = call.func.as_ref() {
                if let Some((op, arity)) = num_builtin_op(fname.id.as_str()) {
                    if call.keywords.is_empty() && call.args.len() == arity {
                        let args = call
                            .args
                            .iter()
                            .map(|a| lower_expr_in_ctx(ctx, a.clone()))
                            .collect::<Result<Vec<_>, _>>()?;
                        if matches!(infer_type_in_ctx(ctx, &args[0]), Type::I64 | Type::F64) {
                            return Ok(Expr::NumBuiltin { op, args });
                        }
                    }
                }
                // PMAT-502w: ctx-aware `len(x)` — lower the argument through
                // the context path so a context-dependent collection (e.g.
                // `len(d.keys())`, `len(sorted(xs))`) is recognized. The
                // context-free `lower_call` path also handles bare `len(xs)`,
                // but loses ctx (method calls there error). Same `Expr::Len`.
                if fname.id.as_str() == "len" && call.keywords.is_empty() && call.args.len() == 1 {
                    let inner = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    return Ok(Expr::Len(Box::new(inner)));
                }
                // PMAT-498b: `sum(xs)` over a numeric list.
                if fname.id.as_str() == "sum" && call.keywords.is_empty() && call.args.len() == 1 {
                    let list = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    if let Type::List(elem) = infer_type_in_ctx(ctx, &list) {
                        if matches!(*elem, Type::I64 | Type::F64) {
                            return Ok(Expr::Sum {
                                list: Box::new(list),
                                of_float: matches!(*elem, Type::F64),
                            });
                        }
                    }
                }
                // PMAT-502j: `all(xs)`/`any(xs)` over a `list[bool]` →
                // `.iter().all/any(|&__b| __b)`. (Truthiness over non-bool
                // lists is deferred — v0.1.0 has no int/str truthiness.)
                if matches!(fname.id.as_str(), "all" | "any")
                    && call.keywords.is_empty()
                    && call.args.len() == 1
                {
                    let list = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    if let Type::List(elem) = infer_type_in_ctx(ctx, &list) {
                        if matches!(*elem, Type::Bool) {
                            return Ok(Expr::BoolReduce {
                                list: Box::new(list),
                                is_all: fname.id.as_str() == "all",
                            });
                        }
                    }
                }
                // PMAT-502m: `int(x)` / `float(x)` numeric conversion over a
                // numeric arg → `(x) as i64` / `(x) as f64`. (The `int("42")`
                // string-parse form is deferred — only numeric args here.)
                if matches!(fname.id.as_str(), "int" | "float")
                    && call.keywords.is_empty()
                    && call.args.len() == 1
                {
                    let value = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    if matches!(infer_type_in_ctx(ctx, &value), Type::I64 | Type::F64) {
                        return Ok(Expr::NumCast {
                            value: Box::new(value),
                            to_float: fname.id.as_str() == "float",
                        });
                    }
                }
                // PMAT-502n: `divmod(a, b)` over two ints → the tuple
                // `(a // b, a % b)`. Pure desugar reusing the existing
                // floor-div + mod ops, so it's consistent with `//`/`%` by
                // construction (both inherit the C-PY-INT-ARITH contract).
                // a/b are pure v0.1.0 exprs, so the double-eval is sound.
                if fname.id.as_str() == "divmod" && call.keywords.is_empty() && call.args.len() == 2
                {
                    let a = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    let b = lower_expr_in_ctx(ctx, call.args[1].clone())?;
                    if infer_type_in_ctx(ctx, &a) == Type::I64
                        && infer_type_in_ctx(ctx, &b) == Type::I64
                    {
                        return Ok(Expr::TupleLit(vec![
                            Expr::BinOp {
                                op: BinOp::FloorDiv,
                                lhs: Box::new(a.clone()),
                                rhs: Box::new(b.clone()),
                            },
                            Expr::BinOp {
                                op: BinOp::Mod,
                                lhs: Box::new(a),
                                rhs: Box::new(b),
                            },
                        ]));
                    }
                }
                // PMAT-502e/502h: 1-arg `min(xs)`/`max(xs)` over a numeric
                // list → a reduction. The 2-arg `min(a, b)` form is handled
                // above by the NumBuiltin intercept (arity 2), so this only
                // matches the single-list-argument reduction. `int` lists use
                // `.min()/.max()` (i64: Ord); `float` lists use a fold (f64
                // has no Ord) — see the codegen `of_float` branch.
                // PMAT-502aa: an optional `key=lambda p: e` reduces by the key
                // (`min_by_key`/`max_by_key`); with a key the element may be
                // any type (only the key needs `Ord`).
                if matches!(fname.id.as_str(), "min" | "max") && call.args.len() == 1 {
                    let mut key: Option<SortKey> = None;
                    let mut kwargs_ok = true;
                    for kw in &call.keywords {
                        if kw.arg.as_ref().map(|a| a.as_str()) == Some("key") {
                            if let ast::Expr::Lambda(lam) = &kw.value {
                                if lam.args.args.len() == 1
                                    && lam.args.posonlyargs.is_empty()
                                    && lam.args.kwonlyargs.is_empty()
                                    && lam.args.vararg.is_none()
                                    && lam.args.kwarg.is_none()
                                {
                                    let param = lam.args.args[0].def.arg.to_string();
                                    let body = lower_expr_in_ctx(ctx, (*lam.body).clone())?;
                                    key = Some(SortKey {
                                        param,
                                        body: Box::new(body),
                                    });
                                    continue;
                                }
                            }
                        }
                        kwargs_ok = false;
                    }
                    if kwargs_ok {
                        let list = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                        if let Type::List(elem) = infer_type_in_ctx(ctx, &list) {
                            // With a key, any element type works (the key
                            // supplies the ordering); without, restrict to
                            // numeric for the `.min()/.max()`/fold form.
                            if key.is_some() || matches!(*elem, Type::I64 | Type::F64) {
                                return Ok(Expr::ListMinMax {
                                    list: Box::new(list),
                                    is_max: fname.id.as_str() == "max",
                                    of_float: matches!(*elem, Type::F64),
                                    key,
                                });
                            }
                        }
                    }
                }
                // PMAT-502c: `sorted(xs)` over a list → a new sorted list.
                // PMAT-502f: optional `reverse=<bool literal>` (descending).
                // PMAT-502z: optional `key=lambda p: e` (sort_by_key). The
                // lambda must be a simple single-param form; its body is
                // lowered with `p` left unbound (works for arithmetic / `len`
                // / builtin bodies; str-method keys fall through to error).
                // Any other keyword leaves the intercept to fall through.
                if fname.id.as_str() == "sorted" && call.args.len() == 1 {
                    let mut reverse = false;
                    let mut key: Option<SortKey> = None;
                    let mut kwargs_ok = true;
                    for kw in &call.keywords {
                        match kw.arg.as_ref().map(|a| a.as_str()) {
                            Some("reverse") => {
                                if let ast::Expr::Constant(c) = &kw.value {
                                    if let ast::Constant::Bool(b) = &c.value {
                                        reverse = *b;
                                        continue;
                                    }
                                }
                                kwargs_ok = false;
                            }
                            Some("key") => {
                                if let ast::Expr::Lambda(lam) = &kw.value {
                                    if lam.args.args.len() == 1
                                        && lam.args.posonlyargs.is_empty()
                                        && lam.args.kwonlyargs.is_empty()
                                        && lam.args.vararg.is_none()
                                        && lam.args.kwarg.is_none()
                                    {
                                        let param = lam.args.args[0].def.arg.to_string();
                                        let body = lower_expr_in_ctx(ctx, (*lam.body).clone())?;
                                        key = Some(SortKey {
                                            param,
                                            body: Box::new(body),
                                        });
                                        continue;
                                    }
                                }
                                kwargs_ok = false;
                            }
                            _ => kwargs_ok = false,
                        }
                    }
                    if kwargs_ok {
                        let list = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                        if matches!(infer_type_in_ctx(ctx, &list), Type::List(_)) {
                            return Ok(Expr::Sorted {
                                list: Box::new(list),
                                reverse,
                                key,
                            });
                        }
                    }
                }
                // PMAT-502d: `reversed(xs)` over a list → a new reversed
                // list. The supported subset materializes Python's lazy
                // `reversed` iterator as a `Vec`, so `reversed(xs)` and the
                // idiomatic `list(reversed(xs))` both produce `Expr::Reversed`.
                if fname.id.as_str() == "reversed"
                    && call.keywords.is_empty()
                    && call.args.len() == 1
                {
                    let list = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    if matches!(infer_type_in_ctx(ctx, &list), Type::List(_)) {
                        return Ok(Expr::Reversed {
                            list: Box::new(list),
                        });
                    }
                }
                // PMAT-502d: `list(reversed(xs))` — the `list(...)` wrapper is
                // a no-op once the inner `reversed(xs)` already materializes
                // to a `Vec`. Unwrap a single already-list-typed argument.
                if fname.id.as_str() == "list" && call.keywords.is_empty() && call.args.len() == 1 {
                    let inner = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    if matches!(inner, Expr::Reversed { .. } | Expr::Sorted { .. }) {
                        return Ok(inner);
                    }
                }
                // PMAT-502i: empty collection constructors. `set()`/`dict()`/
                // `list()` (0 args) → the corresponding empty literal. Like
                // the empty `{}` dict, the element type comes from a binding
                // annotation (`s: set[int] = set()`) or a subsequent
                // `.add()`/`.append()` that lets rustc infer it.
                if call.keywords.is_empty() && call.args.is_empty() {
                    match fname.id.as_str() {
                        "set" => return Ok(Expr::SetLit(Vec::new())),
                        "dict" => return Ok(Expr::DictLit(Vec::new())),
                        "list" => return Ok(Expr::ListLit(Vec::new())),
                        _ => {}
                    }
                }
            }
            // PMAT-474 (R5): reorder keyword args to positional using
            // the module signature table, then lower as a plain call.
            let call = reorder_kwargs_to_positional(ctx, call)?;
            lower_call(call)
        }
        // `k in d` / `k not in d` → `Expr::DictContains` (wrapped in
        // `not` for the negated form) when the RHS is a dict.
        ast::Expr::Compare(c) => {
            if c.ops.len() == 1
                && c.comparators.len() == 1
                && matches!(c.ops[0], ast::CmpOp::In | ast::CmpOp::NotIn)
            {
                let rhs = lower_expr_in_ctx(ctx, c.comparators[0].clone())?;
                if matches!(infer_type_in_ctx(ctx, &rhs), Type::Dict(_, _)) {
                    let key = lower_expr_in_ctx(ctx, (*c.left).clone())?;
                    let contains = Expr::DictContains {
                        dict: Box::new(rhs),
                        key: Box::new(key),
                    };
                    return Ok(if matches!(c.ops[0], ast::CmpOp::NotIn) {
                        Expr::UnOp {
                            op: UnOp::Not,
                            operand: Box::new(contains),
                        }
                    } else {
                        contains
                    });
                }
                // PMAT-502o: `sub in s` / `sub not in s` over a Str →
                // substring containment.
                if infer_type_in_ctx(ctx, &rhs) == Type::Str {
                    let needle = lower_expr_in_ctx(ctx, (*c.left).clone())?;
                    let contains = Expr::StrContains {
                        haystack: Box::new(rhs),
                        needle: Box::new(needle),
                    };
                    return Ok(if matches!(c.ops[0], ast::CmpOp::NotIn) {
                        Expr::UnOp {
                            op: UnOp::Not,
                            operand: Box::new(contains),
                        }
                    } else {
                        contains
                    });
                }
                // PMAT-500: `x in s` / `x not in s` over a set.
                if matches!(infer_type_in_ctx(ctx, &rhs), Type::Set(_)) {
                    let elem = lower_expr_in_ctx(ctx, (*c.left).clone())?;
                    let contains = Expr::SetContains {
                        set: Box::new(rhs),
                        elem: Box::new(elem),
                    };
                    return Ok(if matches!(c.ops[0], ast::CmpOp::NotIn) {
                        Expr::UnOp {
                            op: UnOp::Not,
                            operand: Box::new(contains),
                        }
                    } else {
                        contains
                    });
                }
            }
            lower_compare(c)
        }
        // Recurse through `+`/etc. so a dict op on either side (e.g.
        // `counts.get(x, 0) + 1`) is lowered correctly. Mirror the
        // str-Concat detection from `lower_expr`, using the
        // context-aware inference.
        ast::Expr::BinOp(b) => {
            let lhs = lower_expr_in_ctx(ctx, *b.left)?;
            let rhs = lower_expr_in_ctx(ctx, *b.right)?;
            // PMAT-477 (R8): float arithmetic → FloatBinOp (plain infix,
            // `/` is true division). Detected before `lower_binop`,
            // which rejects `/`. Float *comparisons* fall through to
            // BinOp (plain infix is already f64-correct, yields Bool).
            if infer_type_in_ctx(ctx, &lhs) == Type::F64
                || infer_type_in_ctx(ctx, &rhs) == Type::F64
            {
                if let Some(fop) = float_op_from_ast(&b.op) {
                    return Ok(Expr::FloatBinOp {
                        op: fop,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    });
                }
            }
            // PMAT-502g: set algebra — when BOTH operands are sets, `|`/`&`/
            // `-`/`^` are union/intersection/difference/symmetric-difference
            // (disambiguated from the int bitwise/arith BinOp by operand type).
            if matches!(infer_type_in_ctx(ctx, &lhs), Type::Set(_))
                && matches!(infer_type_in_ctx(ctx, &rhs), Type::Set(_))
            {
                if let Some(sop) = set_op_from_ast(&b.op) {
                    return Ok(Expr::SetOp {
                        lhs: Box::new(lhs),
                        op: sop,
                        rhs: Box::new(rhs),
                    });
                }
            }
            let op = lower_binop(&b.op)?;
            if matches!(op, BinOp::Add)
                && (infer_type_in_ctx(ctx, &lhs) == Type::Str
                    || infer_type_in_ctx(ctx, &rhs) == Type::Str)
            {
                return Ok(Expr::Concat {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                });
            }
            // PMAT-502k: `seq * n` / `n * seq` sequence repetition.
            if matches!(op, BinOp::Mul) {
                if let Some(rep) = try_repeat(
                    &infer_type_in_ctx(ctx, &lhs),
                    &infer_type_in_ctx(ctx, &rhs),
                    &lhs,
                    &rhs,
                ) {
                    return Ok(rep);
                }
            }
            Ok(Expr::BinOp {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })
        }
        // PMAT-494: tuple literal / multiple-return `return a, b` →
        // `Expr::TupleLit`, lowering each element context-aware.
        ast::Expr::Tuple(t) => {
            let elems = t
                .elts
                .into_iter()
                .map(|e| lower_expr_in_ctx(ctx, e))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Expr::TupleLit(elems))
        }
        // No dict-specific shape: the context-free path is sufficient.
        other => lower_expr(other),
    }
}

/// PMAT-496: lower a Python `xs[lo:hi]` slice. First cut requires both
/// bounds present, `int`-typed, step 1; the collection must type as
/// `list[T]` or `str` (which selects the `of_str` emit). Open-ended /
/// stepped / negative slices are deferred.
fn lower_slice_in_ctx(
    ctx: &LoweringCtx,
    collection: Expr,
    slice: &ast::ExprSlice,
) -> Result<Expr, FrontendError> {
    let coll_ty = infer_type_in_ctx(ctx, &collection);
    let of_str = match coll_ty {
        Type::Str => true,
        Type::List(_) => false,
        other => {
            return Err(FrontendError::Lower(format!(
                "function `{}` slices a value typing as {other:?}; only `list[T]` and `str` are sliceable at v0.2.0 first cut",
                ctx.fn_name
            )));
        }
    };
    if let Some(step) = slice.step.as_ref() {
        // PMAT-502t: the reverse idiom `xs[::-1]` (no bounds, step −1) over a
        // list → a new reversed list, reusing `Expr::Reversed` (PMAT-502d). A
        // negative literal parses as `UnaryOp(USub, Int(1))`. Other steps (and
        // `str[::-1]`) are still deferred.
        let mut step_is_neg_one = false;
        if let ast::Expr::UnaryOp(u) = step.as_ref() {
            if matches!(u.op, ast::UnaryOp::USub) {
                if let ast::Expr::Constant(c) = u.operand.as_ref() {
                    if let ast::Constant::Int(k) = &c.value {
                        step_is_neg_one = k.to_string() == "1";
                    }
                }
            }
        }
        if step_is_neg_one && slice.lower.is_none() && slice.upper.is_none() && !of_str {
            return Ok(Expr::Reversed {
                list: Box::new(collection),
            });
        }
        return Err(FrontendError::Lower(format!(
            "function `{}` uses a slice step (`xs[a:b:c]`); only the reverse idiom `xs[::-1]` over a list is supported at v0.1.0",
            ctx.fn_name
        )));
    }
    // PMAT-502r: an absent bound is an open end (`xs[a:]`, `xs[:b]`, `xs[:]`).
    // Each present bound must type as `int`.
    let lower_bound = |which: &str,
                       b: Option<&Box<ast::Expr>>|
     -> Result<Option<Box<Expr>>, FrontendError> {
        match b {
            None => Ok(None),
            Some(e) => {
                let lowered = lower_expr_in_ctx(ctx, (**e).clone())?;
                let bt = infer_type_in_ctx(ctx, &lowered);
                if !matches!(bt, Type::I64) {
                    return Err(FrontendError::Lower(format!(
                            "function `{}` has a slice {which} bound typing as {bt:?}; only `int` bounds are supported",
                            ctx.fn_name
                        )));
                }
                Ok(Some(Box::new(lowered)))
            }
        }
    };
    let lo = lower_bound("lower", slice.lower.as_ref())?;
    let hi = lower_bound("upper", slice.upper.as_ref())?;
    Ok(Expr::Slice {
        collection: Box::new(collection),
        lo,
        hi,
        of_str,
    })
}

fn lower_expr(e: ast::Expr) -> Result<Expr, FrontendError> {
    match e {
        ast::Expr::Name(n) => Ok(Expr::Ident(n.id.to_string())),
        ast::Expr::Constant(c) => match c.value {
            ast::Constant::Int(big) => {
                let v: i64 = big.try_into().map_err(|_| {
                    FrontendError::Lower(
                        "integer literal does not fit in i64 — bigint promotion not implemented at v0.1.0".into(),
                    )
                })?;
                Ok(Expr::LitInt(v))
            }
            // PMAT-477 (R8): Python float literal `3.14` → LitFloat.
            ast::Constant::Float(f) => Ok(Expr::LitFloat(f)),
            // PMAT-449 (v0.2.0 Track 1.A): Python `"..."` literal →
            // `Expr::LitStr`, downstream-typed as `Type::Str`. The
            // raw Python source text is carried through to the
            // backend (which escapes for its target language).
            ast::Constant::Str(s) => Ok(Expr::LitStr(s.to_string())),
            // PMAT-456 (v0.2.0 Track 1.B): Python `True` / `False`
            // literals → `Expr::LitBool(bool)`. Aligns with the
            // existing `LitInt` / `LitStr` shape. Backends emit
            // `true` / `false` (Rust/Ruchy) and `True` / `False`
            // (Lean — capitalised).
            ast::Constant::Bool(b) => Ok(Expr::LitBool(b)),
            other => Err(FrontendError::Lower(format!(
                "unsupported constant: {:?}",
                std::mem::discriminant(&other)
            ))),
        },
        ast::Expr::BinOp(b) => {
            let lhs = lower_expr(*b.left)?;
            let rhs = lower_expr(*b.right)?;
            // PMAT-477 (R8): float arithmetic (context-free path detects
            // float *literals*; param-typed floats are caught by
            // lower_expr_in_ctx). Checked before `lower_binop` rejects
            // `/`.
            if infer_type(&lhs) == Type::F64 || infer_type(&rhs) == Type::F64 {
                if let Some(fop) = float_op_from_ast(&b.op) {
                    return Ok(Expr::FloatBinOp {
                        op: fop,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    });
                }
            }
            // PMAT-502g: set algebra (context-free path catches set
            // *literals*; set-typed params are caught by the ctx path).
            if matches!(infer_type(&lhs), Type::Set(_)) && matches!(infer_type(&rhs), Type::Set(_))
            {
                if let Some(sop) = set_op_from_ast(&b.op) {
                    return Ok(Expr::SetOp {
                        lhs: Box::new(lhs),
                        op: sop,
                        rhs: Box::new(rhs),
                    });
                }
            }
            let op = lower_binop(&b.op)?;
            // PMAT-451 (v0.2.0 Track 1.A): when `+` is applied to two
            // Type::Str operands, lower to Expr::Concat. The backends
            // emit `format!("{}{}", l, r)` (Rust/Ruchy) or `l ++ r`
            // (Lean), governed by the new
            // `C-XLATE-PY-STR-TO-RUST-STRING::concatenation_associativity`
            // equation.
            //
            // Type inference here is context-free (`infer_type`); a
            // future enhancement will use `infer_type_in_ctx` to also
            // recognize `name: str` parameters via the name table.
            // First cut handles `"a" + "b"` and `"prefix" + literal`
            // shapes — sufficient for the greet_concat fixture.
            if matches!(op, BinOp::Add)
                && (infer_type(&lhs) == Type::Str || infer_type(&rhs) == Type::Str)
            {
                return Ok(Expr::Concat {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                });
            }
            // PMAT-502k: `seq * n` / `n * seq` (context-free path catches
            // literal sequences; param-typed ones via the ctx path).
            if matches!(op, BinOp::Mul) {
                if let Some(rep) = try_repeat(&infer_type(&lhs), &infer_type(&rhs), &lhs, &rhs) {
                    return Ok(rep);
                }
            }
            Ok(Expr::BinOp {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })
        }
        ast::Expr::Compare(c) => lower_compare(c),
        ast::Expr::IfExp(ie) => lower_if_exp(ie),
        ast::Expr::Call(c) => lower_call(c),
        ast::Expr::BoolOp(b) => lower_bool_op(b),
        ast::Expr::UnaryOp(u) => lower_unary_op(u),
        // PMAT-452 (v0.2.0 Track 1.A): f-string lowering. Python
        // `f"hello, {name}!"` parses as JoinedStr { values: [
        //   Constant("hello, "), FormattedValue(name), Constant("!")
        // ] }. We fold the values into a left-associative chain of
        // Expr::Concat — each level emits format!("{}{}", l, r) in
        // Rust/Ruchy (which Display-coerces any operand), or
        // (l ++ r) in Lean. Empty f-string → empty LitStr.
        ast::Expr::JoinedStr(js) => lower_fstring(js.values),
        // PMAT-455 (v0.2.0 Track 1.B): list literal. The frontend
        // enforces non-emptiness (an empty `[]` requires upstream
        // annotation to type the elements, which v0.2.0 doesn't yet
        // thread through — handled in subsequent sub-tracks).
        // Homogeneity is enforced by the lowering check before the
        // ListLit is built.
        // PMAT-457 (v0.2.0 Track 1.B): list indexed access. Python
        // `xs[i]` parses as Subscript when in expression position.
        // (In type-annotation position, Subscript means `list[T]` /
        // generic-param-application — that path is handled by
        // parse_type_annotation, distinct call site.)
        ast::Expr::Subscript(sub) => {
            let collection = lower_expr(*sub.value)?;
            let index = lower_expr(*sub.slice)?;
            // v0.2.0 first cut: index must type as Type::I64. Float /
            // slice-object / advanced subscript shapes are deferred.
            let idx_ty = infer_type(&index);
            if !matches!(idx_ty, Type::I64) {
                return Err(FrontendError::Lower(format!(
                    "list-index expression types as {idx_ty:?} but only `int` indices are \
                     supported at v0.2.0 first cut — slicing, negative-step ranges, and \
                     non-integer keys are deferred to subsequent sub-tracks"
                )));
            }
            Ok(Expr::Index {
                collection: Box::new(collection),
                index: Box::new(index),
            })
        }
        // PMAT-462 (v0.2.0 Track 1.C): dict literal. Python `{...}`
        // parses as ast::Expr::Dict { keys: Vec<Option<Expr>>, values:
        // Vec<Expr> } — the key slot is Option to accommodate `**unpack`
        // splats (None for those). v0.2.0 first cut requires all keys
        // to be Some (no splat) and rejects empty literals (no upstream
        // annotation threading for empty dicts yet).
        ast::Expr::Dict(dict_expr) => {
            if dict_expr.keys.is_empty() {
                return Err(FrontendError::Lower(
                    "empty dict literal `{}` requires a type annotation to infer K/V — \
                     pass via `def f() -> dict[str, int]: return {}` once empty-literal annotation \
                     threading lands in a subsequent v0.2.0 Track 1.C sub-track"
                        .into(),
                ));
            }
            let mut pairs: Vec<(Expr, Expr)> = Vec::with_capacity(dict_expr.keys.len());
            let mut k_ty: Option<Type> = None;
            let mut v_ty: Option<Type> = None;
            for (k_opt, v) in dict_expr.keys.into_iter().zip(dict_expr.values.into_iter()) {
                let Some(k) = k_opt else {
                    return Err(FrontendError::Lower(
                        "dict-splat (`**other`) in literals not supported at v0.2.0".into(),
                    ));
                };
                let lk = lower_expr(k)?;
                let lv = lower_expr(v)?;
                let kt = infer_type(&lk);
                let vt = infer_type(&lv);
                if let Some(expected) = &k_ty {
                    if expected != &kt {
                        return Err(FrontendError::Lower(format!(
                            "heterogeneous dict literal — key types {expected:?} and {kt:?} \
                             mixed; C-XLATE-PY-DICT-TO-HASHMAP requires homogeneous keys"
                        )));
                    }
                } else {
                    k_ty = Some(kt);
                }
                if let Some(expected) = &v_ty {
                    if expected != &vt {
                        return Err(FrontendError::Lower(format!(
                            "heterogeneous dict literal — value types {expected:?} and {vt:?} \
                             mixed; C-XLATE-PY-DICT-TO-HASHMAP requires homogeneous values"
                        )));
                    }
                } else {
                    v_ty = Some(vt);
                }
                pairs.push((lk, lv));
            }
            Ok(Expr::DictLit(pairs))
        }
        ast::Expr::List(list_expr) => {
            if list_expr.elts.is_empty() {
                return Err(FrontendError::Lower(
                    "empty list literal `[]` requires a type annotation to infer the element type \
                     — pass via `def f() -> list[int]: return []` once empty-literal annotation \
                     threading lands in a subsequent v0.2.0 Track 1.B sub-track"
                        .into(),
                ));
            }
            let mut elems = Vec::with_capacity(list_expr.elts.len());
            let mut elem_ty: Option<Type> = None;
            for e in list_expr.elts {
                let lowered = lower_expr(e)?;
                let ty = infer_type(&lowered);
                if let Some(expected) = &elem_ty {
                    if expected != &ty {
                        return Err(FrontendError::Lower(format!(
                            "heterogeneous list literal — element types {expected:?} and {ty:?} \
                             mixed; C-XLATE-PY-LIST-TO-VEC requires homogeneous element types"
                        )));
                    }
                } else {
                    elem_ty = Some(ty);
                }
                elems.push(lowered);
            }
            Ok(Expr::ListLit(elems))
        }
        // PMAT-500: Python set literal `{a, b, c}` → `Expr::SetLit`.
        // `{}` parses as an empty Dict (handled elsewhere); a non-empty
        // `{...}` with no `:` parses as `ast::Expr::Set`.
        ast::Expr::Set(set_expr) => {
            if set_expr.elts.is_empty() {
                return Err(FrontendError::Lower(
                    "empty set literal requires `set()` or an annotation — deferred".into(),
                ));
            }
            let mut elems = Vec::with_capacity(set_expr.elts.len());
            let mut elem_ty: Option<Type> = None;
            for e in set_expr.elts {
                let lowered = lower_expr(e)?;
                let ty = infer_type(&lowered);
                if let Some(expected) = &elem_ty {
                    if expected != &ty {
                        return Err(FrontendError::Lower(format!(
                            "heterogeneous set literal — element types {expected:?} and {ty:?} mixed"
                        )));
                    }
                } else {
                    elem_ty = Some(ty);
                }
                elems.push(lowered);
            }
            Ok(Expr::SetLit(elems))
        }
        // PMAT-452: FormattedValue inside an f-string. We strip the
        // conversion + format_spec at v0.2.0 first cut (only `{expr}`
        // without `!r` / `:>10` / etc. is supported); the underlying
        // expression lowers as usual and gets Display-coerced by the
        // surrounding format!() call.
        ast::Expr::FormattedValue(fv) => {
            if fv.conversion != ast::ConversionFlag::None || fv.format_spec.is_some() {
                return Err(FrontendError::Lower(
                    "f-string conversion flags (`!r`/`!s`/`!a`) and format specs (`:>10`/`:.2f`/etc.) \
                     are not supported at v0.2.0 — use plain `{expr}` only"
                        .into(),
                ));
            }
            lower_expr(*fv.value)
        }
        other => Err(FrontendError::Lower(format!(
            "unsupported expression: {:?}",
            std::mem::discriminant(&other)
        ))),
    }
}

/// PMAT-452 — lower an f-string's `values: Vec<ast::Expr>` to a
/// left-associative chain of `Expr::Concat`.
///
/// - `values.len() == 0` → empty literal (`""`).
/// - `values.len() == 1` → unwrap to the single part (no Concat).
/// - `values.len() >= 2` → fold left: `Concat(Concat(v0, v1), v2)` ...
fn lower_fstring(values: Vec<ast::Expr>) -> Result<Expr, FrontendError> {
    let mut parts = values.into_iter();
    let Some(first) = parts.next() else {
        return Ok(Expr::LitStr(String::new()));
    };
    let mut acc = lower_expr(first)?;
    for v in parts {
        let rhs = lower_expr(v)?;
        acc = Expr::Concat {
            lhs: Box::new(acc),
            rhs: Box::new(rhs),
        };
    }
    Ok(acc)
}

/// Lower Python `a and b and c` / `a or b or c` to a left-folded chain
/// of binary [`Expr::BinOp`] with [`BinOp::And`] / [`BinOp::Or`].
fn lower_bool_op(b: ast::ExprBoolOp) -> Result<Expr, FrontendError> {
    if b.values.len() < 2 {
        return Err(FrontendError::Lower(
            "boolean operator with fewer than 2 operands — unreachable Python AST".into(),
        ));
    }
    let op = match b.op {
        ast::BoolOp::And => BinOp::And,
        ast::BoolOp::Or => BinOp::Or,
    };
    let mut iter = b.values.into_iter();
    let first = lower_expr(iter.next().expect("len ≥ 2"))?;
    if infer_type(&first) != Type::Bool {
        return Err(FrontendError::Lower(
            "operands of `and`/`or` must be Bool (no int-truthiness at v0.1.0)".into(),
        ));
    }
    let mut acc = first;
    for next in iter {
        let rhs = lower_expr(next)?;
        if infer_type(&rhs) != Type::Bool {
            return Err(FrontendError::Lower(
                "operands of `and`/`or` must be Bool (no int-truthiness at v0.1.0)".into(),
            ));
        }
        acc = Expr::BinOp {
            op,
            lhs: Box::new(acc),
            rhs: Box::new(rhs),
        };
    }
    Ok(acc)
}

fn lower_unary_op(u: ast::ExprUnaryOp) -> Result<Expr, FrontendError> {
    let operand = lower_expr(*u.operand)?;
    let op = match u.op {
        ast::UnaryOp::USub => {
            if infer_type(&operand) != Type::I64 {
                return Err(FrontendError::Lower(
                    "unary `-` requires I64 operand".into(),
                ));
            }
            UnOp::Neg
        }
        ast::UnaryOp::Not => {
            if infer_type(&operand) != Type::Bool {
                return Err(FrontendError::Lower(
                    "`not` requires Bool operand (no int-truthiness at v0.1.0)".into(),
                ));
            }
            UnOp::Not
        }
        ast::UnaryOp::UAdd => {
            return Err(FrontendError::Lower(
                "unary `+` not supported at v0.1.0 (it's a Python no-op; just remove it)".into(),
            ));
        }
        ast::UnaryOp::Invert => {
            return Err(FrontendError::Lower(
                "bitwise `~` not supported at v0.1.0".into(),
            ));
        }
    };
    Ok(Expr::UnOp {
        op,
        operand: Box::new(operand),
    })
}

fn lower_call(c: ast::ExprCall) -> Result<Expr, FrontendError> {
    if !c.keywords.is_empty() {
        return Err(FrontendError::Lower(
            "keyword arguments in calls (`f(x=...)`) are not supported at v0.1.0".into(),
        ));
    }
    let callee = match *c.func {
        ast::Expr::Name(n) => n.id.to_string(),
        ast::Expr::Attribute(_) => {
            return Err(FrontendError::Lower(
                "method calls (`obj.method(...)`) are not supported at v0.1.0".into(),
            ));
        }
        _ => {
            return Err(FrontendError::Lower(
                "indirect calls (callable-valued expressions) are not supported at v0.1.0".into(),
            ));
        }
    };
    let args: Vec<Expr> = c
        .args
        .into_iter()
        .map(lower_expr)
        .collect::<Result<_, _>>()?;
    // PMAT-459 (v0.2.0 Track 1.B): builtin `len(x)` recognized here
    // and lowered to Expr::Len. The frontend disambiguates `len`
    // from a user-defined function via fixed-arity + builtin-name
    // matching; a function named `len` in user code would shadow
    // the builtin, but v0.2.0 first cut takes the builtin path
    // unconditionally (consistent with Python's actual behavior
    // unless `len` is rebound).
    if callee == "len" {
        if args.len() != 1 {
            return Err(FrontendError::Lower(format!(
                "builtin `len(x)` takes exactly one argument; got {}",
                args.len()
            )));
        }
        let mut args = args;
        return Ok(Expr::Len(Box::new(args.pop().unwrap())));
    }
    Ok(Expr::Call { callee, args })
}

fn lower_if_exp(ie: ast::ExprIfExp) -> Result<Expr, FrontendError> {
    // Python: `then if cond else else_` lowers to meta-HIR's IfExpr.
    // Note Python AST order is (test, body, orelse) = (cond, then, else).
    let cond = lower_expr(*ie.test)?;
    let then_expr = lower_expr(*ie.body)?;
    let else_expr = lower_expr(*ie.orelse)?;

    let then_ty = infer_type(&then_expr);
    let else_ty = infer_type(&else_expr);
    if then_ty != else_ty {
        return Err(FrontendError::Lower(format!(
            "ternary branches have mismatched types ({then_ty:?} vs {else_ty:?}); both must agree at v0.1.0"
        )));
    }

    // The condition must be Bool. v0.1.0 doesn't auto-coerce int truthiness.
    if infer_type(&cond) != Type::Bool {
        return Err(FrontendError::Lower(
            "ternary condition must be a comparison (Bool); int-truthiness coercion not supported at v0.1.0".into(),
        ));
    }

    Ok(Expr::IfExpr {
        cond: Box::new(cond),
        then_expr: Box::new(then_expr),
        else_expr: Box::new(else_expr),
    })
}

/// PMAT-477 (R8): map a Python arithmetic operator to a [`FloatOp`]
/// for float operands. Returns `None` for non-arithmetic ops
/// (comparisons stay on `BinOp`) and for `//`/`%`/bitwise/`**` on
/// floats (deferred).
fn float_op_from_ast(op: &ast::Operator) -> Option<FloatOp> {
    match op {
        ast::Operator::Add => Some(FloatOp::Add),
        ast::Operator::Sub => Some(FloatOp::Sub),
        ast::Operator::Mult => Some(FloatOp::Mul),
        ast::Operator::Div => Some(FloatOp::Div),
        _ => None,
    }
}

/// PMAT-492/493b (sprint): map a Python string method name to its
/// [`StrMethodOp`]. Returns `None` for any other attribute name (which
/// then falls through to the normal call-lowering path). The whole
/// string-method family (upper/lower/strip/startswith/endswith/split/
/// join) is handled here as of PMAT-492a..d.
/// PMAT-498: map a Python scalar numeric builtin name to its
/// [`NumBuiltinOp`] and argument count. `None` for any other name.
fn num_builtin_op(name: &str) -> Option<(NumBuiltinOp, usize)> {
    match name {
        "abs" => Some((NumBuiltinOp::Abs, 1)),
        "min" => Some((NumBuiltinOp::Min, 2)),
        "max" => Some((NumBuiltinOp::Max, 2)),
        _ => None,
    }
}

fn str_method_op(name: &str) -> Option<StrMethodOp> {
    match name {
        "upper" => Some(StrMethodOp::Upper),
        "lower" => Some(StrMethodOp::Lower),
        "strip" => Some(StrMethodOp::Strip),
        "startswith" => Some(StrMethodOp::StartsWith),
        "endswith" => Some(StrMethodOp::EndsWith),
        "split" => Some(StrMethodOp::Split),
        "join" => Some(StrMethodOp::Join),
        "replace" => Some(StrMethodOp::Replace),
        // PMAT-502l: lstrip/rstrip (0-arg) + find/count (1-arg).
        "lstrip" => Some(StrMethodOp::LStrip),
        "rstrip" => Some(StrMethodOp::RStrip),
        "find" => Some(StrMethodOp::Find),
        "count" => Some(StrMethodOp::Count),
        _ => None,
    }
}

/// Number of arguments a [`StrMethodOp`] expects: 0 for the transforms,
/// 1 for the predicates / `split` / `join`, 2 for `replace(old, new)`.
fn str_method_arity(op: StrMethodOp) -> usize {
    match op {
        StrMethodOp::Upper | StrMethodOp::Lower | StrMethodOp::Strip => 0,
        StrMethodOp::StartsWith
        | StrMethodOp::EndsWith
        | StrMethodOp::Split
        | StrMethodOp::Join => 1,
        StrMethodOp::Replace => 2,
        // PMAT-502l: lstrip/rstrip take no args; find/count take one.
        StrMethodOp::LStrip | StrMethodOp::RStrip => 0,
        StrMethodOp::Find | StrMethodOp::Count => 1,
    }
}

/// PMAT-502k: detect Python sequence repetition `seq * n` / `n * seq`
/// (one operand a `Str`/`List`, the other an `Int`). Returns the
/// `Expr::Repeat`, trying both operand orders, or `None` when the pair
/// isn't (sequence, int). Caller only invokes this for the `*` operator.
fn try_repeat(lhs_ty: &Type, rhs_ty: &Type, lhs: &Expr, rhs: &Expr) -> Option<Expr> {
    let is_seq = |t: &Type| matches!(t, Type::Str | Type::List(_));
    if is_seq(lhs_ty) && *rhs_ty == Type::I64 {
        Some(Expr::Repeat {
            seq: Box::new(lhs.clone()),
            n: Box::new(rhs.clone()),
        })
    } else if *lhs_ty == Type::I64 && is_seq(rhs_ty) {
        Some(Expr::Repeat {
            seq: Box::new(rhs.clone()),
            n: Box::new(lhs.clone()),
        })
    } else {
        None
    }
}

/// PMAT-502g: map a Python binary operator to its set-algebra meaning.
/// `None` for operators with no set interpretation (the caller then treats
/// the expression as an int [`Expr::BinOp`]). Only consulted when both
/// operands already typed as [`Type::Set`].
fn set_op_from_ast(op: &ast::Operator) -> Option<SetOp> {
    match op {
        ast::Operator::BitOr => Some(SetOp::Union),
        ast::Operator::BitAnd => Some(SetOp::Intersection),
        ast::Operator::Sub => Some(SetOp::Difference),
        ast::Operator::BitXor => Some(SetOp::SymmetricDifference),
        _ => None,
    }
}

fn lower_binop(op: &ast::Operator) -> Result<BinOp, FrontendError> {
    Ok(match op {
        ast::Operator::Add => BinOp::Add,
        ast::Operator::Sub => BinOp::Sub,
        ast::Operator::Mult => BinOp::Mul,
        ast::Operator::FloorDiv => BinOp::FloorDiv,
        ast::Operator::Mod => BinOp::Mod,
        ast::Operator::BitAnd => BinOp::BitAnd,
        ast::Operator::BitOr => BinOp::BitOr,
        ast::Operator::BitXor => BinOp::BitXor,
        ast::Operator::LShift => BinOp::Shl,
        ast::Operator::RShift => BinOp::Shr,
        ast::Operator::Pow => BinOp::Pow,
        other => {
            return Err(FrontendError::Lower(format!(
                "unsupported binary operator: {:?} — supported: + - * // % & | ^ << >> **",
                other
            )));
        }
    })
}

/// Map a Python comparison operator to its meta-HIR [`BinOp`].
fn cmp_binop(op: &ast::CmpOp) -> Result<BinOp, FrontendError> {
    Ok(match op {
        ast::CmpOp::Eq => BinOp::Eq,
        ast::CmpOp::NotEq => BinOp::NotEq,
        ast::CmpOp::Lt => BinOp::Lt,
        ast::CmpOp::LtE => BinOp::LtEq,
        ast::CmpOp::Gt => BinOp::Gt,
        ast::CmpOp::GtE => BinOp::GtEq,
        other => {
            return Err(FrontendError::Lower(format!(
                "unsupported comparison operator: {:?}",
                other
            )));
        }
    })
}

fn lower_compare(c: ast::ExprCompare) -> Result<Expr, FrontendError> {
    if c.ops.is_empty() || c.ops.len() != c.comparators.len() {
        return Err(FrontendError::Lower(
            "malformed comparison (ops/comparators mismatch) — unreachable Python AST".into(),
        ));
    }
    // PMAT-502p: Python chained comparison `a OP1 b OP2 c` means
    // `(a OP1 b) and (b OP2 c)` — each adjacent operand pair is compared and
    // the booleans are `&&`-folded. Lower every operand once into `operands`
    // ([left, comparators…]); a middle operand is reused (cloned) across the
    // two comparisons it participates in. v0.1.0 operands are pure, so the
    // reuse matches Python's evaluate-once semantics observationally. A single
    // comparison (the common case) folds to exactly one `BinOp`, unchanged.
    let mut operands: Vec<Expr> = Vec::with_capacity(c.ops.len() + 1);
    operands.push(lower_expr(*c.left)?);
    for cmp in c.comparators {
        operands.push(lower_expr(cmp)?);
    }
    let mut acc: Option<Expr> = None;
    for (i, op) in c.ops.iter().enumerate() {
        let cmp = Expr::BinOp {
            op: cmp_binop(op)?,
            lhs: Box::new(operands[i].clone()),
            rhs: Box::new(operands[i + 1].clone()),
        };
        acc = Some(match acc {
            None => cmp,
            Some(prev) => Expr::BinOp {
                op: BinOp::And,
                lhs: Box::new(prev),
                rhs: Box::new(cmp),
            },
        });
    }
    Ok(acc.expect("ops non-empty (checked above)"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn parse(src: &str) -> Module {
        PythonFrontend
            .parse_and_lower(&PathBuf::from("fixture.py"), src)
            .expect("parse should succeed")
    }

    fn function(m: &Module, i: usize) -> &Function {
        match &m.items[i] {
            Item::Function(f) => f,
        }
    }

    #[test]
    fn lowers_add() {
        let m = parse("def add(a, b):\n    return a + b\n");
        assert_eq!(m.name, "fixture");
        assert_eq!(m.source_lang, SourceLang::Python);
        assert_eq!(m.items.len(), 1);
        let f = function(&m, 0);
        assert_eq!(f.name, "add");
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.params[0].name, "a");
        assert_eq!(f.params[1].name, "b");
        assert!(f.body.stmts.is_empty());
        assert!(matches!(
            f.body.trailing_return,
            Expr::BinOp { op: BinOp::Add, .. }
        ));
    }

    #[test]
    fn lowers_constant_in_body() {
        let m = parse("def f(a):\n    return a + 1\n");
        let f = function(&m, 0);
        let Expr::BinOp { lhs, rhs, op } = &f.body.trailing_return else {
            panic!("expected BinOp");
        };
        assert_eq!(*op, BinOp::Add);
        assert!(matches!(**lhs, Expr::Ident(_)));
        assert!(matches!(**rhs, Expr::LitInt(1)));
    }

    #[test]
    fn lowers_comparison() {
        let m = parse("def le(a, b):\n    return a <= b\n");
        let f = function(&m, 0);
        assert!(matches!(
            f.body.trailing_return,
            Expr::BinOp {
                op: BinOp::LtEq,
                ..
            }
        ));
    }

    #[test]
    fn lowers_assignment_then_return() {
        let m = parse("def f(a, b):\n    s = a + b\n    return s\n");
        let f = function(&m, 0);
        assert_eq!(f.body.stmts.len(), 1);
        let Stmt::Let {
            name, ty, value, ..
        } = &f.body.stmts[0]
        else {
            panic!("expected Let");
        };
        assert_eq!(name, "s");
        assert_eq!(*ty, Type::I64);
        assert!(matches!(value, Expr::BinOp { op: BinOp::Add, .. }));
        assert!(matches!(f.body.trailing_return, Expr::Ident(ref n) if n == "s"));
    }

    #[test]
    fn lowers_multiple_lets_then_return() {
        let m = parse("def f(a, b):\n    x = a + 1\n    y = b * 2\n    return x + y\n");
        let f = function(&m, 0);
        assert_eq!(f.body.stmts.len(), 2);
    }

    #[test]
    fn rejects_function_without_trailing_return() {
        let err = PythonFrontend
            .parse_and_lower(&PathBuf::from("fixture.py"), "def f(a):\n    x = a + 1\n")
            .expect_err("missing trailing return should fail");
        match err {
            FrontendError::Lower(msg) => {
                assert!(msg.contains("end with `return"), "unexpected msg: {}", msg);
            }
            _ => panic!("expected Lower error"),
        }
    }

    #[test]
    fn rejects_chained_assignment() {
        let err = PythonFrontend
            .parse_and_lower(
                &PathBuf::from("fixture.py"),
                "def f(a):\n    x = y = a\n    return x\n",
            )
            .expect_err("chained assignment should fail");
        match err {
            FrontendError::Lower(msg) => {
                assert!(msg.contains("chained"), "unexpected msg: {}", msg);
            }
            _ => panic!("expected Lower error"),
        }
    }

    #[test]
    fn rejects_decorator() {
        let err = PythonFrontend
            .parse_and_lower(
                &PathBuf::from("fixture.py"),
                "@staticmethod\ndef f(a):\n    return a\n",
            )
            .expect_err("decorator should fail at v0.1.0");
        match err {
            FrontendError::Lower(msg) => {
                assert!(msg.contains("decorator"), "unexpected msg: {}", msg);
            }
            _ => panic!("expected Lower error"),
        }
    }

    #[test]
    fn lowers_ternary() {
        let m = parse("def pick(a, b):\n    return a if a <= b else b\n");
        let f = function(&m, 0);
        assert_eq!(f.return_type, Type::I64);
        let Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } = &f.body.trailing_return
        else {
            panic!("expected IfExpr, got {:?}", f.body.trailing_return);
        };
        assert!(matches!(
            **cond,
            Expr::BinOp {
                op: BinOp::LtEq,
                ..
            }
        ));
        assert!(matches!(**then_expr, Expr::Ident(_)));
        assert!(matches!(**else_expr, Expr::Ident(_)));
    }

    #[test]
    fn rejects_ternary_with_mismatched_branch_types() {
        // then-branch is i64, else-branch is bool — should fail.
        let err = PythonFrontend
            .parse_and_lower(
                &PathBuf::from("fixture.py"),
                "def f(a, b):\n    return a if a < b else (a < b)\n",
            )
            .expect_err("mismatched ternary branch types should fail");
        match err {
            FrontendError::Lower(msg) => assert!(
                msg.contains("mismatched") || msg.contains("agree"),
                "unexpected msg: {msg}"
            ),
            _ => panic!("expected Lower error"),
        }
    }

    #[test]
    fn rejects_ternary_with_non_bool_condition() {
        let err = PythonFrontend
            .parse_and_lower(
                &PathBuf::from("fixture.py"),
                "def f(a, b):\n    return a if a else b\n",
            )
            .expect_err("non-bool ternary cond should fail (no int-truthiness at v0.1.0)");
        match err {
            FrontendError::Lower(msg) => assert!(
                msg.contains("Bool") || msg.contains("truthiness"),
                "unexpected msg: {msg}"
            ),
            _ => panic!("expected Lower error"),
        }
    }

    #[test]
    fn rejects_unsupported_operator() {
        // `@` (matrix multiplication, ast::Operator::MatMult) is still
        // outside the v0.1.0 subset — `**`, `<<`, `>>`, etc. are now
        // supported (PMAT-003 + PMAT-004), so this test moved to MatMult.
        let err = PythonFrontend
            .parse_and_lower(
                &PathBuf::from("fixture.py"),
                "def f(a, b):\n    return a @ b\n",
            )
            .expect_err("@ should fail");
        match err {
            FrontendError::Lower(msg) => {
                assert!(msg.contains("supported"), "unexpected msg: {}", msg);
            }
            _ => panic!("expected Lower error"),
        }
    }
}
