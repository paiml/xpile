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
    BinOp, Block, DictViewKind, Expr, FloatOp, Function, Item, ListMutateOp, ListQueryOp, Module,
    NumBuiltinOp, PairIterKind, Param, SetOp, SortKey, SourceLang, Stmt, StrMethodOp, Type, UnOp,
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
    /// PMAT-504: function-local closure bindings — maps a closure
    /// variable name (`f` in `f = lambda y: …`) to its inferred return
    /// type, so a call `f(x)` types correctly (the module signature
    /// table only covers top-level functions). Populated as
    /// [`Stmt::ClosureLet`] bindings are lowered.
    closure_returns: HashMap<String, Type>,
}

impl LoweringCtx {
    fn new(
        fn_name: &str,
        fn_return_type: Type,
        params: &[Param],
        body: &[ast::Stmt],
        signatures: Rc<HashMap<String, FnSig>>,
        consts: &HashMap<String, Type>,
    ) -> Self {
        // PMAT-502bj: module-level constants are visible (and immutably
        // bound) in every function body; a same-named param shadows the
        // constant (insert consts first, params override).
        let bound: HashSet<String> = params
            .iter()
            .map(|p| p.name.clone())
            .chain(consts.keys().cloned())
            .collect();
        let mut name_types: HashMap<String, Type> = consts.clone();
        for p in params {
            name_types.insert(p.name.clone(), p.ty.clone());
        }
        let mutable = compute_mutable_names(params, body);
        Self {
            fn_name: fn_name.to_string(),
            fn_return_type,
            bound,
            name_types,
            mutable,
            signatures,
            closure_returns: HashMap::new(),
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
        // PMAT-502as: `<name>.pop(...)` mutates its receiver even in
        // expression position (`x = xs.pop()`, `return xs.pop()`), so the
        // receiver's binding must be `mut`. Scan value expressions for pop
        // receivers — the param lift handles params, but a popped *local*
        // is only caught here.
        count_pop_receivers_in_stmt(stmt, &mut counts, if in_loop { 2 } else { 1 });
        match stmt {
            ast::Stmt::Assign(a) => {
                let bump = if in_loop { 2 } else { 1 };
                // PMAT-502bz: a chained assignment `x = y = …` binds every
                // Name target — count each so a later mutation lifts it to
                // `let mut`.
                if a.targets.len() > 1 {
                    for t in &a.targets {
                        if let ast::Expr::Name(n) = t {
                            *counts.entry(n.id.to_string()).or_insert(0) += bump;
                        }
                    }
                } else if let Some(name) = simple_assign_target_name(a) {
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
                            // PMAT-502ap: sort/reverse/clear also mutate
                            // the receiver in place.
                            if matches!(
                                attr.attr.as_str(),
                                "add"
                                    | "append"
                                    | "sort"
                                    | "reverse"
                                    | "clear"
                                    | "extend"
                                    | "insert"
                                    | "remove"
                                    | "discard"
                                    | "update"
                            ) {
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
            // PMAT-502at: `del coll[key]` mutates `coll` in place, so the
            // binding must be `mut` (mirrors the subscript-assign arm).
            ast::Stmt::Delete(d) => {
                let bump = if in_loop { 2 } else { 1 };
                for t in &d.targets {
                    if let ast::Expr::Subscript(sub) = t {
                        if let ast::Expr::Name(n) = sub.value.as_ref() {
                            *counts.entry(n.id.to_string()).or_insert(0) += bump;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    counts
}

/// PMAT-502as: extract the value expression(s) of a statement and scan
/// them for `<name>.pop(...)` receivers, bumping each by `bump`. Only the
/// statement positions that can hold a captured pop result are scanned
/// (assignment/return values); `If`/`While`/`For` bodies are handled by
/// the recursive `walk_counts` itself.
fn count_pop_receivers_in_stmt(stmt: &ast::Stmt, counts: &mut HashMap<String, usize>, bump: usize) {
    match stmt {
        ast::Stmt::Assign(a) => count_pop_receivers(&a.value, counts, bump),
        ast::Stmt::AugAssign(a) => count_pop_receivers(&a.value, counts, bump),
        ast::Stmt::AnnAssign(aa) => {
            if let Some(v) = &aa.value {
                count_pop_receivers(v, counts, bump);
            }
        }
        ast::Stmt::Return(r) => {
            if let Some(v) = &r.value {
                count_pop_receivers(v, counts, bump);
            }
        }
        _ => {}
    }
}

/// Recursively walk an expression, bumping the count of every simple-name
/// receiver of an expression-position mutator call — `.pop(...)`
/// (PMAT-502as/au) or `.setdefault(...)` (PMAT-502ax). Both mutate their
/// receiver while evaluating to a value, so a receiver popped/set-defaulted
/// in an `x = …` / `return …` position must be `mut`. Covers the common
/// nestings the value appears in (arithmetic, comparisons, call args,
/// collections); exotic shapes simply aren't counted (the receiver then
/// stays immutable, which at worst surfaces a compile error rather than
/// wrong behaviour).
fn count_pop_receivers(e: &ast::Expr, counts: &mut HashMap<String, usize>, bump: usize) {
    use ast::Expr as E;
    match e {
        E::Call(call) => {
            if let E::Attribute(attr) = call.func.as_ref() {
                if matches!(attr.attr.as_str(), "pop" | "setdefault") {
                    if let E::Name(n) = attr.value.as_ref() {
                        *counts.entry(n.id.to_string()).or_insert(0) += bump;
                    }
                }
                count_pop_receivers(attr.value.as_ref(), counts, bump);
            } else {
                count_pop_receivers(call.func.as_ref(), counts, bump);
            }
            for a in &call.args {
                count_pop_receivers(a, counts, bump);
            }
        }
        E::BinOp(b) => {
            count_pop_receivers(&b.left, counts, bump);
            count_pop_receivers(&b.right, counts, bump);
        }
        E::UnaryOp(u) => count_pop_receivers(&u.operand, counts, bump),
        E::BoolOp(b) => {
            for v in &b.values {
                count_pop_receivers(v, counts, bump);
            }
        }
        E::Compare(c) => {
            count_pop_receivers(&c.left, counts, bump);
            for c2 in &c.comparators {
                count_pop_receivers(c2, counts, bump);
            }
        }
        E::Subscript(s) => {
            count_pop_receivers(&s.value, counts, bump);
            count_pop_receivers(&s.slice, counts, bump);
        }
        E::Tuple(t) => {
            for el in &t.elts {
                count_pop_receivers(el, counts, bump);
            }
        }
        E::List(l) => {
            for el in &l.elts {
                count_pop_receivers(el, counts, bump);
            }
        }
        E::IfExp(i) => {
            count_pop_receivers(&i.test, counts, bump);
            count_pop_receivers(&i.body, counts, bump);
            count_pop_receivers(&i.orelse, counts, bump);
        }
        _ => {}
    }
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

        // PMAT-502bj: pre-pass — collect module-level constants
        // (`NAME = <int/bool/float-literal>`) and their types, so
        // references in function bodies type correctly.
        let mut const_map: HashMap<String, Type> = HashMap::new();
        for stmt in &suite {
            if let Some((name, ty, _)) = try_const_decl(stmt) {
                const_map.insert(name, ty);
            }
        }
        let consts = Rc::new(const_map);

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
            let item = lower_top_level_stmt(stmt, signatures.clone(), consts.clone())?;
            items.push(item);
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
    consts: Rc<HashMap<String, Type>>,
) -> Result<Item, FrontendError> {
    // PMAT-502bj: a module-level `NAME = <int/bool/float-literal>` is a
    // constant item (recognised before the `def`-only fallback).
    if let Some((name, ty, value)) = try_const_decl(&stmt) {
        return Ok(Item::Const { name, ty, value });
    }
    match stmt {
        ast::Stmt::FunctionDef(f) => {
            lower_function_def(f, signatures, consts).map(Item::Function)
        }
        // A top-level assignment that wasn't a recognised constant.
        ast::Stmt::Assign(_) | ast::Stmt::AnnAssign(_) => Err(FrontendError::Lower(
            "unsupported module-level assignment — v0.2.0 supports `NAME = <int/bool/float literal>` constants (str/collection constants deferred)".to_string(),
        )),
        other => Err(FrontendError::Lower(format!(
            "unsupported top-level statement: {:?} — only `def` and `NAME = <literal>` constants are supported at v0.2.0",
            std::mem::discriminant(&other)
        ))),
    }
}

/// PMAT-502bj: recognise a module-level constant declaration
/// `NAME = <value>` / `NAME: T = <value>` where the value lowers to an
/// `int` / `bool` / `float` literal (or a negated numeric literal) — the
/// forms that map to a Rust `const`. Returns `(name, type, value-expr)`,
/// or `None` for any other shape (str/collection values, computed
/// expressions, tuple targets) so the caller can report a precise error.
fn try_const_decl(stmt: &ast::Stmt) -> Option<(String, Type, Expr)> {
    let (name, value_ast) = match stmt {
        ast::Stmt::Assign(a) if a.targets.len() == 1 => match &a.targets[0] {
            ast::Expr::Name(n) => (n.id.to_string(), a.value.as_ref()),
            _ => return None,
        },
        ast::Stmt::AnnAssign(aa) => match (aa.target.as_ref(), aa.value.as_ref()) {
            (ast::Expr::Name(n), Some(v)) => (n.id.to_string(), v.as_ref()),
            _ => return None,
        },
        _ => return None,
    };
    let value = lower_expr(value_ast.clone()).ok()?;
    // Fold a negated numeric literal (`-5`, `-2.5`) into a single negative
    // literal so it emits as a plain const-safe `-5i64` / `-2.5f64` (the
    // generic `UnOp::Neg` emit uses `checked_neg().expect(…)`, which is not
    // a `const` expression). Only literal numeric / bool values are kept.
    let value = match value {
        Expr::UnOp {
            op: UnOp::Neg,
            operand,
        } => match *operand {
            Expr::LitInt(n) => Expr::LitInt(-n),
            Expr::LitFloat(f) => Expr::LitFloat(-f),
            _ => return None,
        },
        v @ (Expr::LitInt(_) | Expr::LitBool(_) | Expr::LitFloat(_)) => v,
        _ => return None,
    };
    let ty = match infer_type(&value) {
        t @ (Type::I64 | Type::Bool | Type::F64) => t,
        _ => return None,
    };
    Some((name, ty, value))
}

fn lower_function_def(
    f: ast::StmtFunctionDef,
    signatures: Rc<HashMap<String, FnSig>>,
    consts: Rc<HashMap<String, Type>>,
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
        &consts,
    );
    let mut stmts: Vec<Stmt> = Vec::with_capacity(leading.len());
    for stmt in leading {
        // A single Python statement may lower to multiple meta-HIR
        // statements — most notably a multi-assignment `if/else`, where
        // each assigned name gets its own `Let` with an `IfExpr` value
        // (PMAT-005), or a `while` whose body lowers to a nested vec.
        stmts.extend(lower_block_stmt(&mut ctx, stmt.clone())?);
    }

    // PMAT-502bl: a void (`-> None`) function has no trailing `return
    // expr` — its last statement is a regular (side-effecting) statement,
    // and the body evaluates to the unit value `()`. Lower the last
    // statement like the leading ones and use `Expr::Unit` as the trailing
    // return. (An explicit `return` inside a void function still flows
    // through the early-return error path — bare/early returns are a
    // separate deferred sub-slice.)
    if matches!(ctx_return_type, Type::Unit) {
        stmts.extend(lower_block_stmt(&mut ctx, last.clone())?);
        // Lift in-place-mutation receivers to `mut params` (same as the
        // value-returning path below).
        for p in &mut params {
            if ctx.mutable.contains(&p.name) {
                p.mutable = true;
            }
        }
        let body = Block {
            stmts,
            trailing_return: Expr::Unit,
        };
        return Ok(Function {
            name: f.name.to_string(),
            params,
            return_type: Type::Unit,
            body,
        });
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
        // PMAT-502bm: a terminal `if cond: return A else: return B`
        // (and `elif` chains) — where every branch is a single `return
        // <expr>` — becomes the function's trailing return via an
        // `Expr::IfExpr` (the same if-as-expression used for assignments).
        ast::Stmt::If(if_stmt) => match terminal_if_as_expr(&mut ctx, if_stmt)? {
            Some(expr) => expr,
            None => {
                return Err(FrontendError::Lower(format!(
                    "function `{}`'s final `if` is not an exhaustive `if/elif/else` whose every branch is a single `return <expr>` — v0.2.0 supports that shape (or a trailing `return`)",
                    f.name
                )));
            }
        },
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
        // PMAT-502bl: `-> None` is the void return type. Python's `None`
        // annotation parses as the `None` constant (and, defensively, a
        // bare `None` name).
        ast::Expr::Constant(c) if matches!(c.value, ast::Constant::None) => Ok(Type::Unit),
        ast::Expr::Name(n) => match n.id.as_str() {
            "int" => Ok(Type::I64),
            "bool" => Ok(Type::Bool),
            // PMAT-477 (R8): Python `float` → IEEE-754 `f64`.
            "float" => Ok(Type::F64),
            "BigInt" => Ok(Type::BigInt),
            "str" => Ok(Type::Str),
            "None" => Ok(Type::Unit),
            other => Err(FrontendError::Lower(format!(
                "function `{fn_name}` annotates `{site}` with unsupported type `{other}` — only `int`, `bool`, `BigInt`, `str`, `None`, `list[T]` at v0.2.0"
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
                // PMAT-504: `name = lambda param: body` → a closure binding.
                if let (ast::Expr::Name(n), ast::Expr::Lambda(lam)) =
                    (&asn.targets[0], asn.value.as_ref())
                {
                    let name = n.id.to_string();
                    return desugar_closure_assign(ctx, &name, lam).map(|s| vec![s]);
                }
            }
            // PMAT-502bz: chained assignment `x = y = z = <literal>`. Python
            // evaluates the value once and binds it to every target left to
            // right. First cut: all targets must be plain Names and the value
            // a scalar literal (int/float/bool/str), so re-lowering the value
            // per target is side-effect-free and each target gets an
            // independent copy (matches Python for scalars; list/dict
            // aliasing is out of scope under value semantics).
            if asn.targets.len() > 1 {
                return lower_chained_assign(ctx, asn);
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
        // PMAT-502bk: `continue` / `break` loop control. A `continue`
        // inside a `range(...)` for-loop is rejected in `lower_for_stmt`
        // (that desugars to a while whose tail counter-increment the
        // `continue` would skip); `break` and list/while `continue` are fine.
        ast::Stmt::Continue(_) => Ok(vec![Stmt::Continue]),
        ast::Stmt::Break(_) => Ok(vec![Stmt::Break]),
        // PMAT-502bn: `pass` is a no-op — it lowers to no statements. An
        // empty `if`/`for` body or a `pass`-only void function are the
        // common shapes (a `pass`-last in a value-returning function still
        // fails the trailing-`return` requirement, which is correct).
        ast::Stmt::Pass(_) => Ok(Vec::new()),
        // PMAT-502at: `del coll[key]` item deletion (list or dict).
        ast::Stmt::Delete(d) => lower_delete_stmt(ctx, d).map(|s| vec![s]),
        // PMAT-503a: `raise SomeException("msg")` → `Stmt::Raise`. Works
        // both at top level and inside a guard-clause `if` (this fn is the
        // recursion point for if-branch bodies).
        ast::Stmt::Raise(r) => lower_raise_stmt(ctx, r).map(|s| vec![s]),
        // PMAT-502bm: an early `return <expr>` (guard clause) → `Stmt::Return`.
        // The backends already emit `return <expr>;` (the C frontend produces
        // these).
        // PMAT-502bv: a bare `return` (no value) is Python's `return None`.
        // In a void function (`-> None`, `fn_return_type == Unit`) it lowers
        // to `Stmt::Return(Expr::Unit)` → `return ();` — the early-exit guard
        // clause shape (`if invalid: return`). In a value-returning function
        // a bare `return` would yield `None`, a type error, so it stays
        // rejected (with a clearer message).
        ast::Stmt::Return(ret) => match ret.value.as_ref() {
            Some(value) => {
                let lowered = lower_expr_in_ctx(ctx, (**value).clone())?;
                Ok(vec![Stmt::Return(lowered)])
            }
            None if matches!(ctx.fn_return_type, Type::Unit) => {
                Ok(vec![Stmt::Return(Expr::Unit)])
            }
            None => Err(FrontendError::Lower(format!(
                "function `{}` has a bare `return` (Python `return None`) but its return type is not `None` — add a return value or annotate `-> None`",
                ctx.fn_name
            ))),
        },
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
        // PMAT-502bw: `print(a, b, …)` builtin → `Stmt::Print`. Checked
        // before the list-method / subprocess.run paths.
        ast::Stmt::Expr(e) if is_print_call(&e) => {
            lower_print_stmt(ctx, &e).map(|s| vec![s])
        }
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
    // PMAT-502av: `s.remove(x)` / `s.discard(x)` — 1-arg set element
    // removal. `remove` raises KeyError on an absent element; `discard`
    // is a silent no-op. (List `.remove` has different semantics and is a
    // separate, unimplemented slice; the Set receiver type disambiguates.)
    if matches!(method, "remove" | "discard") && matches!(receiver_ty, Some(Type::Set(_))) {
        if !call.keywords.is_empty() || call.args.len() != 1 {
            return Some(Err(FrontendError::Lower(format!(
                "function `{}` calls `{receiver_name}.{method}(...)` with {} positional arg(s){}; \
                 set remove/discard take exactly 1",
                ctx.fn_name,
                call.args.len(),
                if call.keywords.is_empty() {
                    ""
                } else {
                    " plus keyword args"
                },
            ))));
        }
        let elem = match lower_expr_in_ctx(ctx, call.args[0].clone()) {
            Ok(e) => e,
            Err(err) => return Some(Err(err)),
        };
        ctx.mutable.insert(receiver_name.to_string());
        return Some(Ok(Stmt::SetRemove {
            set_name: receiver_name.to_string(),
            elem,
            error_if_absent: method == "remove",
        }));
    }
    // PMAT-502ap: no-arg in-place list mutators `xs.sort()/.reverse()/.clear()`.
    let list_mutate_op = match method {
        "sort" => Some(ListMutateOp::Sort),
        "reverse" => Some(ListMutateOp::Reverse),
        "clear" => Some(ListMutateOp::Clear),
        _ => None,
    };
    if let Some(op) = list_mutate_op {
        let Some(Type::List(inner)) = receiver_ty.as_ref() else {
            return None;
        };
        // 0-arg, no kwargs.
        if !call.args.is_empty() || !call.keywords.is_empty() {
            return Some(Err(FrontendError::Lower(format!(
                "function `{}` calls `{receiver_name}.{method}(...)` with arguments; \
                 the in-place list mutators sort/reverse/clear take none at v0.2.0",
                ctx.fn_name
            ))));
        }
        let of_float = matches!(inner.as_ref(), Type::F64);
        ctx.mutable.insert(receiver_name.to_string());
        return Some(Ok(Stmt::ListMutate {
            list_name: receiver_name.to_string(),
            op,
            of_float,
        }));
    }
    // PMAT-502aq: `xs.extend(ys)` — 1-arg in-place list concatenation.
    if method == "extend" {
        let Some(Type::List(_)) = receiver_ty.as_ref() else {
            return None;
        };
        if !call.keywords.is_empty() {
            return Some(Err(FrontendError::Lower(format!(
                "function `{}` calls `{receiver_name}.extend(...)` with keyword args; \
                 v0.2.0 takes a single positional list",
                ctx.fn_name
            ))));
        }
        if call.args.len() != 1 {
            return Some(Err(FrontendError::Lower(format!(
                "function `{}` calls `{receiver_name}.extend(...)` with {} positional arg(s); \
                 v0.2.0 requires exactly 1 (a list)",
                ctx.fn_name,
                call.args.len()
            ))));
        }
        let other = match lower_expr_in_ctx(ctx, call.args[0].clone()) {
            Ok(e) => e,
            Err(err) => return Some(Err(err)),
        };
        ctx.mutable.insert(receiver_name.to_string());
        return Some(Ok(Stmt::ListExtend {
            list_name: receiver_name.to_string(),
            other,
        }));
    }
    // PMAT-502bb: `d.update(other)` — 1-arg in-place dict merge.
    if method == "update" && matches!(receiver_ty, Some(Type::Dict(_, _))) {
        if !call.keywords.is_empty() {
            return Some(Err(FrontendError::Lower(format!(
                "function `{}` calls `{receiver_name}.update(...)` with keyword args; \
                 v0.2.0 takes a single positional dict",
                ctx.fn_name
            ))));
        }
        if call.args.len() != 1 {
            return Some(Err(FrontendError::Lower(format!(
                "function `{}` calls `{receiver_name}.update(...)` with {} positional arg(s); \
                 v0.2.0 requires exactly 1 (a dict)",
                ctx.fn_name,
                call.args.len()
            ))));
        }
        let other = match lower_expr_in_ctx(ctx, call.args[0].clone()) {
            Ok(e) => e,
            Err(err) => return Some(Err(err)),
        };
        ctx.mutable.insert(receiver_name.to_string());
        return Some(Ok(Stmt::DictUpdate {
            dict_name: receiver_name.to_string(),
            other,
        }));
    }
    // PMAT-502ar: `xs.insert(i, x)` — 2-arg positional list insertion.
    if method == "insert" {
        let Some(Type::List(_)) = receiver_ty.as_ref() else {
            return None;
        };
        if !call.keywords.is_empty() {
            return Some(Err(FrontendError::Lower(format!(
                "function `{}` calls `{receiver_name}.insert(...)` with keyword args; \
                 v0.2.0 takes two positional args (index, value)",
                ctx.fn_name
            ))));
        }
        if call.args.len() != 2 {
            return Some(Err(FrontendError::Lower(format!(
                "function `{}` calls `{receiver_name}.insert(...)` with {} positional arg(s); \
                 v0.2.0 requires exactly 2 (index, value)",
                ctx.fn_name,
                call.args.len()
            ))));
        }
        let index = match lower_expr_in_ctx(ctx, call.args[0].clone()) {
            Ok(e) => e,
            Err(err) => return Some(Err(err)),
        };
        if infer_type_in_ctx(ctx, &index) != Type::I64 {
            return Some(Err(FrontendError::Lower(format!(
                "function `{}` calls `{receiver_name}.insert(<index>, ...)` with a non-int index; \
                 v0.2.0 requires an int position",
                ctx.fn_name
            ))));
        }
        let elem = match lower_expr_in_ctx(ctx, call.args[1].clone()) {
            Ok(e) => e,
            Err(err) => return Some(Err(err)),
        };
        ctx.mutable.insert(receiver_name.to_string());
        return Some(Ok(Stmt::ListInsert {
            list_name: receiver_name.to_string(),
            index,
            elem,
        }));
    }
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
/// PMAT-502bw: does this expression statement call the `print` builtin?
fn is_print_call(e: &ast::StmtExpr) -> bool {
    if let ast::Expr::Call(call) = e.value.as_ref() {
        if let ast::Expr::Name(n) = call.func.as_ref() {
            return n.id.as_str() == "print";
        }
    }
    false
}

/// PMAT-502bw: lower `print(a, b, …)` → [`Stmt::Print`]. First cut admits
/// only positional `int`/`str` (incl. f-strings → `String`) arguments; the
/// `sep=`/`end=`/`file=` keyword args and `bool`/`float` arguments are
/// deferred with a precise error (Python's `True`/`2.0` formatting differs
/// from Rust's `Display`). An empty `print()` → `Stmt::Print(vec![])`.
fn lower_print_stmt(ctx: &mut LoweringCtx, e: &ast::StmtExpr) -> Result<Stmt, FrontendError> {
    let ast::Expr::Call(call) = e.value.as_ref() else {
        unreachable!("is_print_call gated this");
    };
    // PMAT-502by: `sep=`/`end=` keywords (string literals only). `file=`
    // and any other keyword are deferred. Defaults match Python.
    let mut sep = " ".to_string();
    let mut end = "\n".to_string();
    for kw in &call.keywords {
        let Some(name) = kw.arg.as_ref().map(|a| a.as_str()) else {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `print(**kwargs)` — `**` keyword unpacking is not supported",
                ctx.fn_name
            )));
        };
        match name {
            "sep" | "end" => {
                let ast::Expr::Constant(c) = &kw.value else {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` calls `print(..., {name}=<expr>)` with a non-literal — only a string literal is supported at v0.2.0",
                        ctx.fn_name
                    )));
                };
                let ast::Constant::Str(s) = &c.value else {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` calls `print(..., {name}=<non-str>)` — only a string literal is supported at v0.2.0",
                        ctx.fn_name
                    )));
                };
                if name == "sep" {
                    sep = s.to_string();
                } else {
                    end = s.to_string();
                }
            }
            other => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` calls `print(..., {other}=…)` — only `sep=`/`end=` string-literal kwargs are supported (`file=` deferred)",
                    ctx.fn_name
                )));
            }
        }
    }
    let mut args = Vec::with_capacity(call.args.len());
    for a in &call.args {
        let lowered = lower_expr_in_ctx(ctx, a.clone())?;
        match infer_type_in_ctx(ctx, &lowered) {
            // int / str print directly via `{}`.
            Type::I64 | Type::Str => args.push(lowered),
            // PMAT-502bx: a float prints with Python formatting (`2.0`, not
            // Rust's `2`) — reuse the `str(float)` block (`Expr::ToStr`).
            Type::F64 => args.push(Expr::ToStr {
                value: Box::new(lowered),
                of_float: true,
            }),
            // PMAT-502bx: a bool prints `True`/`False` (capitalised) — reuse
            // the `str(bool)` desugar to `"True" if b else "False"`.
            Type::Bool => args.push(Expr::IfExpr {
                cond: Box::new(lowered),
                then_expr: Box::new(Expr::LitStr("True".to_string())),
                else_expr: Box::new(Expr::LitStr("False".to_string())),
            }),
            other => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` calls `print(...)` with a `{other:?}` argument — only int/str/float/bool (incl. f-strings) are supported at v0.2.0 (list/dict/set repr deferred)",
                    ctx.fn_name
                )));
            }
        }
    }
    Ok(Stmt::Print { args, sep, end })
}

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
                        // PMAT-502ca: `enumerate(xs)` or `enumerate(xs, start)`
                        // (start = int literal); `zip(xs, ys)`.
                        let arity_ok = (fname == "enumerate"
                            && (call.args.len() == 1 || call.args.len() == 2))
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
                                // PMAT-502ca: the optional 2nd arg is the start
                                // index — an int literal at first cut.
                                let start = if call.args.len() == 2 {
                                    match &call.args[1] {
                                        ast::Expr::Constant(c) => match &c.value {
                                            ast::Constant::Int(i) => {
                                                i.to_string().parse::<i64>().map_err(|_| {
                                                    FrontendError::Lower(format!(
                                                        "function `{}` uses `enumerate(xs, <start>)` with an out-of-range integer start",
                                                        ctx.fn_name
                                                    ))
                                                })?
                                            }
                                            _ => {
                                                return Err(FrontendError::Lower(format!(
                                                    "function `{}` uses `enumerate(xs, <start>)` with a non-int start — only an integer literal is supported at v0.2.0",
                                                    ctx.fn_name
                                                )));
                                            }
                                        },
                                        _ => {
                                            return Err(FrontendError::Lower(format!(
                                                "function `{}` uses `enumerate(xs, <start>)` with a non-literal start — only an integer literal is supported at v0.2.0",
                                                ctx.fn_name
                                            )));
                                        }
                                    }
                                } else {
                                    0
                                };
                                ctx.name_types.insert(first.clone(), Type::I64);
                                ctx.name_types.insert(second.clone(), (*elem).clone());
                                PairIterKind::Enumerate { start }
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
    // PMAT-502bk: a `continue` belonging to this `range(...)` loop would
    // skip the tail counter-increment below (an infinite loop). Reject it
    // (a list iteration or a manual `while` loop is the workaround).
    // `break` is fine — it exits the loop before the increment.
    if body_has_top_level_continue(&body) {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses `continue` inside a `range(...)` for-loop; v0.2.0 \
             can't compose it with the loop's counter-increment — iterate a list or \
             use a `while` loop",
            ctx.fn_name
        )));
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

/// PMAT-502bk: does `stmts` contain a `continue` that belongs to *this*
/// loop — i.e. directly or inside an `if`, but NOT inside a nested loop
/// (where the `continue` belongs to that inner loop instead)? Used to
/// reject `continue` in a `range(...)` for-loop, whose desugaring appends
/// a tail counter-increment that a `continue` would skip.
fn body_has_top_level_continue(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::Continue => true,
        Stmt::If {
            then_body,
            else_body,
            ..
        } => body_has_top_level_continue(then_body) || body_has_top_level_continue(else_body),
        // Nested loops own their own `continue`/`break`; don't descend.
        _ => false,
    })
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

/// Lower `assert cond` / `assert cond, msg` to [`Stmt::Assert`]. PMAT-009;
/// the optional `msg` (must type as `Str`) is PMAT-502ao.
fn lower_assert_stmt(ctx: &mut LoweringCtx, a: ast::StmtAssert) -> Result<Stmt, FrontendError> {
    let cond = lower_expr_in_ctx(ctx, *a.test)?;
    if infer_type_in_ctx(ctx, &cond) != Type::Bool {
        return Err(FrontendError::Lower(format!(
            "function `{}` has an `assert` whose expression is not Bool (no int-truthiness at v0.1.0)",
            ctx.fn_name
        )));
    }
    // PMAT-502ao: `assert cond, msg` — the message must type as a `Str`.
    let msg = match a.msg {
        None => None,
        Some(m) => {
            let m = lower_expr_in_ctx(ctx, *m)?;
            if infer_type_in_ctx(ctx, &m) != Type::Str {
                return Err(FrontendError::Lower(format!(
                    "function `{}` has an `assert cond, msg` whose message is not a `Str`",
                    ctx.fn_name
                )));
            }
            Some(m)
        }
    };
    Ok(Stmt::Assert { cond, msg })
}

/// PMAT-502at: lower `del coll[key]` to [`Stmt::DelItem`]. First cut
/// supports exactly one subscript target whose receiver is a Name typing
/// as a list (int index) or a dict (any declared key type). Multiple
/// targets (`del a, b`), whole-name deletion (`del x`), and slice
/// deletion (`del xs[a:b]`) are rejected.
fn lower_delete_stmt(ctx: &mut LoweringCtx, d: ast::StmtDelete) -> Result<Stmt, FrontendError> {
    if d.targets.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "function `{}` has `del` with {} targets; v0.2.0 supports exactly one `del coll[key]`",
            ctx.fn_name,
            d.targets.len()
        )));
    }
    let ast::Expr::Subscript(sub) = &d.targets[0] else {
        return Err(FrontendError::Lower(format!(
            "function `{}` has a `del` of a non-subscript target — v0.2.0 supports `del coll[key]` only",
            ctx.fn_name
        )));
    };
    let ast::Expr::Name(recv) = sub.value.as_ref() else {
        return Err(FrontendError::Lower(format!(
            "function `{}` has a `del` whose collection isn't a simple Name — v0.2.0 supports `del <name>[key]` only",
            ctx.fn_name
        )));
    };
    let name = recv.id.to_string();
    let receiver_ty = ctx.name_types.get(&name).cloned();
    match receiver_ty {
        Some(Type::List(_)) => {
            let key = lower_expr_in_ctx(ctx, (*sub.slice).clone())?;
            let idx_ty = infer_type_in_ctx(ctx, &key);
            if !matches!(idx_ty, Type::I64) {
                return Err(FrontendError::Lower(format!(
                    "function `{}` deletes `{name}[<expr>]` where index types as {idx_ty:?}; only `int` indices are supported at v0.2.0",
                    ctx.fn_name
                )));
            }
            ctx.mutable.insert(name.clone());
            Ok(Stmt::DelItem {
                name,
                key,
                is_dict: false,
            })
        }
        Some(Type::Dict(_, _)) => {
            let key = lower_expr_in_ctx(ctx, (*sub.slice).clone())?;
            ctx.mutable.insert(name.clone());
            Ok(Stmt::DelItem {
                name,
                key,
                is_dict: true,
            })
        }
        _ => Err(FrontendError::Lower(format!(
            "function `{}` deletes from `{name}` which doesn't type as list[T] or dict[K, V] — v0.2.0 supports list/dict `del` only",
            ctx.fn_name
        ))),
    }
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
/// PMAT-502bm: convert a terminal `if cond: return A else: return B`
/// (including `elif` chains) into an `Expr::IfExpr`, so it can be the
/// function's trailing return. Returns `Ok(None)` if the shape isn't an
/// exhaustive if/elif/else whose every branch is exactly a single
/// `return <expr>` — the caller then reports a precise error. The
/// condition must type as `Bool`.
fn terminal_if_as_expr(
    ctx: &mut LoweringCtx,
    if_stmt: &ast::StmtIf,
) -> Result<Option<Expr>, FrontendError> {
    // The branch must be exactly one statement: a `return <value>`
    // (giving the branch expr), or a nested `if` (an `elif`/`else: if`).
    fn branch_expr(
        ctx: &mut LoweringCtx,
        body: &[ast::Stmt],
    ) -> Result<Option<Expr>, FrontendError> {
        match body {
            [ast::Stmt::Return(ret)] => match ret.value.as_ref() {
                Some(v) => Ok(Some(lower_expr_in_ctx(ctx, (**v).clone())?)),
                None => Ok(None),
            },
            [ast::Stmt::If(inner)] => terminal_if_as_expr(ctx, inner),
            _ => Ok(None),
        }
    }
    let cond = lower_expr_in_ctx(ctx, (*if_stmt.test).clone())?;
    if !matches!(infer_type_in_ctx(ctx, &cond), Type::Bool) {
        return Ok(None);
    }
    let Some(then_expr) = branch_expr(ctx, &if_stmt.body)? else {
        return Ok(None);
    };
    // An exhaustive if needs an `else` (or `elif … else`) so every path
    // returns a value.
    let Some(else_expr) = branch_expr(ctx, &if_stmt.orelse)? else {
        return Ok(None);
    };
    Ok(Some(Expr::IfExpr {
        cond: Box::new(cond),
        then_expr: Box::new(then_expr),
        else_expr: Box::new(else_expr),
    }))
}

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

/// PMAT-502bz: lower a chained assignment `x = y = z = <literal>` to one
/// `Stmt` per target. Restricted to plain-Name targets and a scalar-literal
/// value (int/float/bool/str) so re-lowering the value for each target is
/// side-effect-free and each target gets an independent value (Python's list/
/// dict aliasing for `a = b = []` is intentionally out of scope here).
fn lower_chained_assign(
    ctx: &mut LoweringCtx,
    asn: ast::StmtAssign,
) -> Result<Vec<Stmt>, FrontendError> {
    let names = asn
        .targets
        .iter()
        .map(|t| match t {
            ast::Expr::Name(n) => Ok(n.id.to_string()),
            _ => Err(FrontendError::Lower(format!(
                "function `{}` has a chained assignment with a non-Name target — only `a = b = … = <literal>` (plain names) is supported at v0.2.0",
                ctx.fn_name
            ))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let is_scalar_literal = matches!(
        asn.value.as_ref(),
        ast::Expr::Constant(c) if matches!(
            c.value,
            ast::Constant::Int(_)
                | ast::Constant::Float(_)
                | ast::Constant::Bool(_)
                | ast::Constant::Str(_)
        )
    );
    if !is_scalar_literal {
        return Err(FrontendError::Lower(format!(
            "function `{}` has a chained assignment `a = b = …` with a non-literal value — only a scalar literal (int/float/bool/str) is supported at v0.2.0 (avoids aliasing/move issues)",
            ctx.fn_name
        )));
    }
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let value = lower_expr_in_ctx(ctx, (*asn.value).clone())?;
        if ctx.bound.contains(&name) {
            out.push(Stmt::Assign { name, value });
        } else {
            let ty = infer_type_in_ctx(ctx, &value);
            let mutable = ctx.mutable.contains(&name);
            ctx.bound.insert(name.clone());
            ctx.name_types.insert(name.clone(), ty.clone());
            out.push(Stmt::Let {
                name,
                ty,
                value,
                mutable,
            });
        }
    }
    Ok(out)
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
/// Combine the current value `lhs` with `rhs` under the (AST) operator
/// `ast_op` for an augmented assignment, mirroring `lower_expr_in_ctx`'s
/// detections so `+=` on strings lowers to `Concat` (format!), not a
/// `checked_add`, and `+= -= *= /=` on a *float* lowers to `FloatBinOp`
/// (plain infix) rather than the i64-only `checked_*` path (PMAT-502bq).
/// Takes the AST operator (not a pre-lowered `BinOp`) so the float branch
/// can run *before* `lower_binop`, which rejects `/` — exactly as the
/// regular `ast::Expr::BinOp` lowering does.
fn combine_aug(
    ctx: &LoweringCtx,
    ast_op: &ast::Operator,
    lhs: Expr,
    rhs: Expr,
) -> Result<Expr, FrontendError> {
    // PMAT-502bq: float augmented arithmetic → FloatBinOp. Detected before
    // `lower_binop` (which rejects `/`), so `x /= y` over floats works too.
    // PMAT-502bu: cast BOTH operands to f64 (via `to_f64_operand`) so a
    // non-float rhs — e.g. `x += 1`, `x /= 2`, `x **= 2` on a float `x` —
    // doesn't emit a mismatched `f64 <op> i64`. `float_op_from_ast` maps
    // `**` to `FloatOp::Pow` so `**=` lowers to `(x).powf(..)`.
    if infer_type_in_ctx(ctx, &lhs) == Type::F64 || infer_type_in_ctx(ctx, &rhs) == Type::F64 {
        if let Some(fop) = float_op_from_ast(ast_op) {
            return Ok(Expr::FloatBinOp {
                op: fop,
                lhs: Box::new(to_f64_operand(ctx, lhs)),
                rhs: Box::new(to_f64_operand(ctx, rhs)),
            });
        }
    }
    let op = lower_binop(ast_op)?;
    if matches!(op, BinOp::Add)
        && (infer_type_in_ctx(ctx, &lhs) == Type::Str || infer_type_in_ctx(ctx, &rhs) == Type::Str)
    {
        Ok(Expr::Concat {
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    } else {
        Ok(Expr::BinOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    }
}

fn lower_aug_assign(ctx: &mut LoweringCtx, aug: ast::StmtAugAssign) -> Result<Stmt, FrontendError> {
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
            let value = combine_aug(ctx, &aug.op, Expr::Ident(name.clone()), rhs)?;
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
                    let value = combine_aug(ctx, &aug.op, current, rhs)?;
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
                    let value = combine_aug(ctx, &aug.op, current, rhs)?;
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
/// PMAT-502ba: extract `(start, stop, step_int)` from a `range(...)` call
/// for a comprehension's `for x in range(...)` generator. Mirrors the
/// range-bound handling in [`lower_for_stmt`]: 1–3 args, the step (when
/// present) a non-zero integer literal (so the loop direction is known at
/// lower time). Returns `Ok(None)` if `iter` isn't a `range(...)` call (the
/// caller then falls back to the list-iterable path).
fn comp_range_bounds(
    ctx: &mut LoweringCtx,
    iter: &ast::Expr,
) -> Result<Option<(Expr, Expr, i64)>, FrontendError> {
    let ast::Expr::Call(call) = iter else {
        return Ok(None);
    };
    match call.func.as_ref() {
        ast::Expr::Name(n) if n.id.as_str() == "range" => {}
        _ => return Ok(None),
    }
    if !call.keywords.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` passes keyword args to `range(...)` in a comprehension — only positional args are supported",
            ctx.fn_name
        )));
    }
    let bounds = match call.args.as_slice() {
        [stop] => (Expr::LitInt(0), lower_expr_in_ctx(ctx, stop.clone())?, 1i64),
        [start, stop] => (
            lower_expr_in_ctx(ctx, start.clone())?,
            lower_expr_in_ctx(ctx, stop.clone())?,
            1i64,
        ),
        [start, stop, step] => {
            let step = extract_step_literal(step).ok_or_else(|| {
                FrontendError::Lower(format!(
                    "function `{}` uses `range(..., step)` in a comprehension with a non-literal-int or zero step — a non-zero integer literal is required",
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
                "function `{}` calls `range(...)` with {} args in a comprehension — Python supports 1-3",
                ctx.fn_name,
                call.args.len()
            )));
        }
    };
    Ok(Some(bounds))
}

/// v0.2.0 slice: single generator; the iterable is either a `list[T]` or a
/// `range(...)` (PMAT-502ba). A single `if` filter is supported
/// (PMAT-502ay): `[elem for var in iter if cond]` wraps the append in an
/// `if cond { … }`. Multiple `if` clauses (`… if a if b`) are deferred —
/// use `… if a and b`. Other iterables (dict, etc.) remain deferred.
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
    if gen.ifs.len() > 1 {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses a list comprehension with multiple `if` clauses — v0.2.0 supports one (combine with `and`)",
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
    // PMAT-502ba: `[elem for x in range(...)]` desugars to a counter
    // `let mut x = start; while (x <cmp> stop) { <…append…>; x = x + step; }`
    // around the accumulator — mirroring the for-over-range desugaring,
    // rather than the list-iterable ForEach below.
    if let Some((start, stop, step_int)) = comp_range_bounds(ctx, &gen.iter)? {
        // The range counter is an `i64`; bind it before lowering elem/filter.
        ctx.bound.insert(var.clone());
        ctx.name_types.insert(var.clone(), Type::I64);
        let filter = match gen.ifs.first() {
            None => None,
            Some(cond) => {
                let c = lower_expr_in_ctx(ctx, cond.clone())?;
                if infer_type_in_ctx(ctx, &c) != Type::Bool {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` has a list-comprehension filter that is not Bool (no int-truthiness at v0.2.0)",
                        ctx.fn_name
                    )));
                }
                Some(c)
            }
        };
        let elem = lower_expr_in_ctx(ctx, (*comp.elt).clone())?;
        let out_ty = infer_type_in_ctx(ctx, &elem);
        let list_ty = Type::List(Box::new(out_ty));
        ctx.bound.insert(target.to_string());
        ctx.name_types.insert(target.to_string(), list_ty.clone());
        let append = Stmt::ListAppend {
            list_name: target.to_string(),
            elem,
        };
        let mut body = match filter {
            None => vec![append],
            Some(cond) => vec![Stmt::If {
                cond,
                then_body: vec![append],
                else_body: Vec::new(),
            }],
        };
        // Tail: x = x + step.
        body.push(Stmt::Assign {
            name: var.clone(),
            value: Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(Expr::Ident(var.clone())),
                rhs: Box::new(Expr::LitInt(step_int)),
            },
        });
        let cond = Expr::BinOp {
            op: if step_int > 0 { BinOp::Lt } else { BinOp::Gt },
            lhs: Box::new(Expr::Ident(var.clone())),
            rhs: Box::new(stop),
        };
        return Ok(vec![
            Stmt::Let {
                name: target.to_string(),
                ty: list_ty,
                value: Expr::ListLit(Vec::new()),
                mutable: true,
            },
            Stmt::Let {
                name: var,
                ty: Type::I64,
                value: start,
                mutable: true,
            },
            Stmt::While { cond, body },
        ]);
    }
    let iter_expr = lower_expr_in_ctx(ctx, gen.iter.clone())?;
    let elem_in_ty = match infer_type_in_ctx(ctx, &iter_expr) {
        Type::List(e) => *e,
        other => {
            return Err(FrontendError::Lower(format!(
                "function `{}` comprehends over an iterable typing as {other:?}; v0.2.0 supports `[… for x in <list[T]>]` or `[… for x in range(...)]` (dict iterables deferred)",
                ctx.fn_name
            )));
        }
    };
    // Bind the loop var so the element + filter expressions type correctly.
    ctx.bound.insert(var.clone());
    ctx.name_types.insert(var.clone(), elem_in_ty.clone());
    // PMAT-502ay: lower the optional `if` filter (must type as Bool).
    let filter = match gen.ifs.first() {
        None => None,
        Some(cond) => {
            let c = lower_expr_in_ctx(ctx, cond.clone())?;
            if infer_type_in_ctx(ctx, &c) != Type::Bool {
                return Err(FrontendError::Lower(format!(
                    "function `{}` has a list-comprehension filter that is not Bool (no int-truthiness at v0.2.0)",
                    ctx.fn_name
                )));
            }
            Some(c)
        }
    };
    let elem = lower_expr_in_ctx(ctx, (*comp.elt).clone())?;
    let out_ty = infer_type_in_ctx(ctx, &elem);
    let list_ty = Type::List(Box::new(out_ty));
    // Register the accumulator so later references type as the list.
    ctx.bound.insert(target.to_string());
    ctx.name_types.insert(target.to_string(), list_ty.clone());
    let append = Stmt::ListAppend {
        list_name: target.to_string(),
        elem,
    };
    let body = match filter {
        None => vec![append],
        Some(cond) => vec![Stmt::If {
            cond,
            then_body: vec![append],
            else_body: Vec::new(),
        }],
    };
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
            body,
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
    if gen.ifs.len() > 1 {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses a dict comprehension with multiple `if` clauses — v0.2.0 supports one (combine with `and`)",
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
    // PMAT-502bd: `{k: v for x in range(...)}` desugars to a counter loop
    // around a dict accumulator (same shape as the list-comp range branch).
    if let Some((start, stop, step_int)) = comp_range_bounds(ctx, &gen.iter)? {
        ctx.bound.insert(var.clone());
        ctx.name_types.insert(var.clone(), Type::I64);
        let filter = comp_filter(ctx, gen, "dict")?;
        let key = lower_expr_in_ctx(ctx, (*comp.key).clone())?;
        let value = lower_expr_in_ctx(ctx, (*comp.value).clone())?;
        let dict_ty = Type::Dict(
            Box::new(infer_type_in_ctx(ctx, &key)),
            Box::new(infer_type_in_ctx(ctx, &value)),
        );
        ctx.bound.insert(target.to_string());
        ctx.name_types.insert(target.to_string(), dict_ty.clone());
        let insert = Stmt::DictSet {
            dict_name: target.to_string(),
            key,
            value,
        };
        return Ok(comp_range_stmts(
            target,
            dict_ty,
            Expr::DictLit(Vec::new()),
            var,
            start,
            stop,
            step_int,
            filter,
            insert,
        ));
    }
    let iter_expr = lower_expr_in_ctx(ctx, gen.iter.clone())?;
    let elem_in_ty = match infer_type_in_ctx(ctx, &iter_expr) {
        Type::List(e) => *e,
        other => {
            return Err(FrontendError::Lower(format!(
                "function `{}` dict-comprehends over an iterable typing as {other:?}; v0.2.0 supports `{{… for x in <list[T]>}}` or `{{… for x in range(...)}}`",
                ctx.fn_name
            )));
        }
    };
    ctx.bound.insert(var.clone());
    ctx.name_types.insert(var.clone(), elem_in_ty.clone());
    // PMAT-502az: lower the optional `if` filter (must type as Bool).
    let filter = comp_filter(ctx, gen, "dict")?;
    let key = lower_expr_in_ctx(ctx, (*comp.key).clone())?;
    let value = lower_expr_in_ctx(ctx, (*comp.value).clone())?;
    let k_ty = infer_type_in_ctx(ctx, &key);
    let v_ty = infer_type_in_ctx(ctx, &value);
    let dict_ty = Type::Dict(Box::new(k_ty), Box::new(v_ty));
    ctx.bound.insert(target.to_string());
    ctx.name_types.insert(target.to_string(), dict_ty.clone());
    let insert = Stmt::DictSet {
        dict_name: target.to_string(),
        key,
        value,
    };
    let body = match filter {
        None => vec![insert],
        Some(cond) => vec![Stmt::If {
            cond,
            then_body: vec![insert],
            else_body: Vec::new(),
        }],
    };
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
            body,
            over_keys: false,
        },
    ])
}

/// PMAT-502bd: lower a comprehension's optional single `if` filter to a
/// `Bool` expression (the loop var must already be bound). `kind` names
/// the comprehension flavour for the error message.
fn comp_filter(
    ctx: &mut LoweringCtx,
    gen: &ast::Comprehension,
    kind: &str,
) -> Result<Option<Expr>, FrontendError> {
    match gen.ifs.first() {
        None => Ok(None),
        Some(cond) => {
            let c = lower_expr_in_ctx(ctx, cond.clone())?;
            if infer_type_in_ctx(ctx, &c) != Type::Bool {
                return Err(FrontendError::Lower(format!(
                    "function `{}` has a {kind}-comprehension filter that is not Bool (no int-truthiness at v0.2.0)",
                    ctx.fn_name
                )));
            }
            Ok(Some(c))
        }
    }
}

/// PMAT-502bd: assemble the statements for a comprehension over `range(...)`:
/// the accumulator `let`, a counter `let mut var = start`, and a `while
/// (var <cmp> stop) { <filter-wrapped insert>; var = var + step; }`.
#[allow(clippy::too_many_arguments)]
fn comp_range_stmts(
    target: &str,
    acc_ty: Type,
    acc_init: Expr,
    var: String,
    start: Expr,
    stop: Expr,
    step_int: i64,
    filter: Option<Expr>,
    insert: Stmt,
) -> Vec<Stmt> {
    let mut body = match filter {
        None => vec![insert],
        Some(cond) => vec![Stmt::If {
            cond,
            then_body: vec![insert],
            else_body: Vec::new(),
        }],
    };
    body.push(Stmt::Assign {
        name: var.clone(),
        value: Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::Ident(var.clone())),
            rhs: Box::new(Expr::LitInt(step_int)),
        },
    });
    let cond = Expr::BinOp {
        op: if step_int > 0 { BinOp::Lt } else { BinOp::Gt },
        lhs: Box::new(Expr::Ident(var.clone())),
        rhs: Box::new(stop),
    };
    vec![
        Stmt::Let {
            name: target.to_string(),
            ty: acc_ty,
            value: acc_init,
            mutable: true,
        },
        Stmt::Let {
            name: var,
            ty: Type::I64,
            value: start,
            mutable: true,
        },
        Stmt::While { cond, body },
    ]
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
    if gen.ifs.len() > 1 {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses a set comprehension with multiple `if` clauses — v0.2.0 supports one (combine with `and`)",
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
    // PMAT-502bd: `{e for x in range(...)}` desugars to a counter loop
    // around a set accumulator (same shape as the list-comp range branch).
    if let Some((start, stop, step_int)) = comp_range_bounds(ctx, &gen.iter)? {
        ctx.bound.insert(var.clone());
        ctx.name_types.insert(var.clone(), Type::I64);
        let filter = comp_filter(ctx, gen, "set")?;
        let elem = lower_expr_in_ctx(ctx, (*comp.elt).clone())?;
        let set_ty = Type::Set(Box::new(infer_type_in_ctx(ctx, &elem)));
        ctx.bound.insert(target.to_string());
        ctx.name_types.insert(target.to_string(), set_ty.clone());
        let insert = Stmt::SetAdd {
            set_name: target.to_string(),
            elem,
        };
        return Ok(comp_range_stmts(
            target,
            set_ty,
            Expr::SetLit(Vec::new()),
            var,
            start,
            stop,
            step_int,
            filter,
            insert,
        ));
    }
    let iter_expr = lower_expr_in_ctx(ctx, gen.iter.clone())?;
    let elem_in_ty = match infer_type_in_ctx(ctx, &iter_expr) {
        Type::List(e) => *e,
        other => {
            return Err(FrontendError::Lower(format!(
                "function `{}` set-comprehends over an iterable typing as {other:?}; v0.2.0 supports `{{… for x in <list[T]>}}` or `{{… for x in range(...)}}`",
                ctx.fn_name
            )));
        }
    };
    ctx.bound.insert(var.clone());
    ctx.name_types.insert(var.clone(), elem_in_ty.clone());
    // PMAT-502az: lower the optional `if` filter (must type as Bool).
    let filter = comp_filter(ctx, gen, "set")?;
    let elem = lower_expr_in_ctx(ctx, (*comp.elt).clone())?;
    let out_ty = infer_type_in_ctx(ctx, &elem);
    let set_ty = Type::Set(Box::new(out_ty));
    ctx.bound.insert(target.to_string());
    ctx.name_types.insert(target.to_string(), set_ty.clone());
    let insert = Stmt::SetAdd {
        set_name: target.to_string(),
        elem,
    };
    let body = match filter {
        None => vec![insert],
        Some(cond) => vec![Stmt::If {
            cond,
            then_body: vec![insert],
            else_body: Vec::new(),
        }],
    };
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
            body,
            over_keys: false,
        },
    ])
}

/// PMAT-504: lower `name = lambda param: body` to a [`Stmt::ClosureLet`].
/// First cut: exactly one simple positional parameter, assumed to type as
/// `i64` (the common case — `lambda y: y + 1`). The body is lowered with
/// the parameter bound as `i64` (restored afterwards so it doesn't leak
/// into the enclosing scope), and the closure's inferred return type is
/// recorded in `ctx.closure_returns` so a later `name(arg)` types
/// correctly. The closure is then callable via the existing `Expr::Call`
/// machinery.
fn desugar_closure_assign(
    ctx: &mut LoweringCtx,
    name: &str,
    lam: &ast::ExprLambda,
) -> Result<Stmt, FrontendError> {
    if !lam.args.posonlyargs.is_empty()
        || !lam.args.kwonlyargs.is_empty()
        || lam.args.vararg.is_some()
        || lam.args.kwarg.is_some()
    {
        return Err(FrontendError::Lower(format!(
            "function `{}` binds a lambda with an unsupported parameter list (posonly/kwonly/*args/**kwargs); v0.2.0 supports plain positional parameters (`name = lambda x, y: …`)",
            ctx.fn_name
        )));
    }
    // First cut: every parameter types as `i64` (covers arithmetic /
    // comparison bodies; 0+ params). Bind them for the body's inference,
    // then restore the prior bindings so the closure-local names don't leak.
    let param_names: Vec<String> = lam
        .args
        .args
        .iter()
        .map(|a| a.def.arg.to_string())
        .collect();
    let saved: Vec<(bool, Option<Type>)> = param_names
        .iter()
        .map(|p| (ctx.bound.contains(p), ctx.name_types.get(p).cloned()))
        .collect();
    for p in &param_names {
        ctx.bound.insert(p.clone());
        ctx.name_types.insert(p.clone(), Type::I64);
    }
    let body = lower_expr_in_ctx(ctx, (*lam.body).clone())?;
    let ret_ty = infer_type_in_ctx(ctx, &body);
    for (p, (prev_bound, prev_ty)) in param_names.iter().zip(saved.into_iter()) {
        if prev_bound {
            if let Some(t) = prev_ty {
                ctx.name_types.insert(p.clone(), t);
            }
        } else {
            ctx.bound.remove(p);
            ctx.name_types.remove(p);
        }
    }
    // Record the closure binding so `name(args…)` types as `ret_ty`.
    ctx.closure_returns.insert(name.to_string(), ret_ty);
    ctx.bound.insert(name.to_string());
    let params: Vec<(String, Type)> = param_names.into_iter().map(|p| (p, Type::I64)).collect();
    Ok(Stmt::ClosureLet {
        name: name.to_string(),
        params,
        body,
    })
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
        Stmt::Assert { cond, msg } => {
            expr_uses_dict(cond) || msg.as_ref().is_some_and(expr_uses_dict)
        }
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
        // PMAT-502bl: the unit value types as Unit (void function return).
        Expr::Unit => Type::Unit,
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
        // PMAT-502bg: list concatenation types as the list (of `lhs`).
        Expr::ListConcat { lhs, .. } => infer_type(lhs),
        // PMAT-502bh: str.format produces a Str.
        Expr::StrFormat { .. } => Type::Str,
        // PMAT-502cd: `s[i]` over a string yields a 1-char string.
        Expr::StrCharAt { .. } => Type::Str,
        // PMAT-502am: a formatted f-string field produces a Str.
        Expr::FormatSpec { .. } => Type::Str,
        // PMAT-492: string transform methods (upper/lower/strip) → Str.
        Expr::StrMethod { op, .. } => match op {
            StrMethodOp::Upper | StrMethodOp::Lower | StrMethodOp::Strip => Type::Str,
            StrMethodOp::StartsWith | StrMethodOp::EndsWith => Type::Bool,
            StrMethodOp::Split => Type::List(Box::new(Type::Str)),
            StrMethodOp::Join | StrMethodOp::Replace => Type::Str,
            // PMAT-502l: lstrip/rstrip → Str; find/count → Int.
            StrMethodOp::LStrip | StrMethodOp::RStrip => Type::Str,
            StrMethodOp::Find | StrMethodOp::Count | StrMethodOp::StrIndex => Type::I64,
            // PMAT-502ag: isdigit/isalpha/isspace → Bool.
            StrMethodOp::IsDigit | StrMethodOp::IsAlpha | StrMethodOp::IsSpace => Type::Bool,
            // PMAT-502ah: capitalize → Str. PMAT-502aj: title → Str.
            StrMethodOp::Capitalize | StrMethodOp::Title => Type::Str,
            // PMAT-502aw: rjust/ljust → Str.
            StrMethodOp::RJust | StrMethodOp::LJust => Type::Str,
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
        // PMAT-502an: list membership -> Bool.
        Expr::ListContains { .. } => Type::Bool,
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
        // PMAT-502ad: str(x) → Str.
        Expr::ToStr { .. } => Type::Str,
        // PMAT-502ak: round(x) → Int.
        Expr::RoundToInt { .. } => Type::I64,
        // PMAT-502al: round(x, n) → Float.
        Expr::RoundToDigits { .. } => Type::F64,
        // PMAT-502k: seq * n has the same type as the sequence.
        Expr::Repeat { seq, .. } => infer_type(seq),
        // PMAT-502c: sorted(xs) has the same type as its list.
        Expr::Sorted { list, .. } => infer_type(list),
        // PMAT-502d: reversed(xs) has the same type as its list.
        Expr::Reversed { list } => infer_type(list),
        // PMAT-502ab: filter(pred, xs) keeps the input list type.
        Expr::Filter { list, .. } => infer_type(list),
        // PMAT-502ac: map(f, xs) → List of the body's transformed type.
        Expr::Map { lambda, .. } => Type::List(Box::new(infer_type(&lambda.body))),
        // PMAT-502ai: enumerate(xs) → List(Tuple[I64, elem]); zip(xs, ys) →
        // List(Tuple[elemL, elemR]).
        Expr::Enumerate { list } => {
            let elem = match infer_type(list) {
                Type::List(e) => *e,
                _ => Type::I64,
            };
            Type::List(Box::new(Type::Tuple(vec![Type::I64, elem])))
        }
        Expr::Zip { left, right } => {
            let el = match infer_type(left) {
                Type::List(e) => *e,
                _ => Type::I64,
            };
            let er = match infer_type(right) {
                Type::List(e) => *e,
                _ => Type::I64,
            };
            Type::List(Box::new(Type::Tuple(vec![el, er])))
        }
        // PMAT-502e: min(xs)/max(xs) reduce a list to its element type.
        Expr::ListMinMax { list, .. } => match infer_type(list) {
            Type::List(elem) => *elem,
            _ => Type::I64,
        },
        // PMAT-502u: list.count(x)/index(x) return Int.
        Expr::ListQuery { .. } => Type::I64,
        // PMAT-502as: list.pop() returns the list's element type.
        Expr::ListPop { list, .. } => match infer_type(list) {
            Type::List(elem) => *elem,
            _ => Type::I64,
        },
        // PMAT-502au: dict.pop() returns the dict's value type.
        Expr::DictPop { dict, .. } => match infer_type(dict) {
            Type::Dict(_, v) => *v,
            _ => Type::I64,
        },
        // PMAT-502ax: dict.setdefault() returns the dict's value type.
        Expr::DictSetDefault { dict, .. } => match infer_type(dict) {
            Type::Dict(_, v) => *v,
            _ => Type::I64,
        },
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
        // PMAT-502bl: the unit value types as Unit (void function return).
        Expr::Unit => Type::Unit,
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
        // PMAT-504: a call to a local closure binding gets its recorded
        // return type; otherwise consult the module signature table (R2).
        Expr::Call { callee, .. } => ctx
            .closure_returns
            .get(callee)
            .cloned()
            .or_else(|| ctx.signatures.get(callee).map(|s| s.ret.clone()))
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
        // PMAT-502bg: list concatenation types as the list (of `lhs`).
        Expr::ListConcat { lhs, .. } => infer_type_in_ctx(ctx, lhs),
        // PMAT-502bh: str.format produces a Str.
        Expr::StrFormat { .. } => Type::Str,
        // PMAT-502cd: `s[i]` over a string yields a 1-char string.
        Expr::StrCharAt { .. } => Type::Str,
        // PMAT-502am: a formatted f-string field produces a Str.
        Expr::FormatSpec { .. } => Type::Str,
        // PMAT-492: string transform methods (upper/lower/strip) → Str.
        Expr::StrMethod { op, .. } => match op {
            StrMethodOp::Upper | StrMethodOp::Lower | StrMethodOp::Strip => Type::Str,
            StrMethodOp::StartsWith | StrMethodOp::EndsWith => Type::Bool,
            StrMethodOp::Split => Type::List(Box::new(Type::Str)),
            StrMethodOp::Join | StrMethodOp::Replace => Type::Str,
            // PMAT-502l: lstrip/rstrip → Str; find/count → Int.
            StrMethodOp::LStrip | StrMethodOp::RStrip => Type::Str,
            StrMethodOp::Find | StrMethodOp::Count | StrMethodOp::StrIndex => Type::I64,
            // PMAT-502ag: isdigit/isalpha/isspace → Bool.
            StrMethodOp::IsDigit | StrMethodOp::IsAlpha | StrMethodOp::IsSpace => Type::Bool,
            // PMAT-502ah: capitalize → Str. PMAT-502aj: title → Str.
            StrMethodOp::Capitalize | StrMethodOp::Title => Type::Str,
            // PMAT-502aw: rjust/ljust → Str.
            StrMethodOp::RJust | StrMethodOp::LJust => Type::Str,
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
        // PMAT-502an: list membership -> Bool.
        Expr::ListContains { .. } => Type::Bool,
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
        // PMAT-502ad: str(x) → Str.
        Expr::ToStr { .. } => Type::Str,
        // PMAT-502ak: round(x) → Int.
        Expr::RoundToInt { .. } => Type::I64,
        // PMAT-502al: round(x, n) → Float.
        Expr::RoundToDigits { .. } => Type::F64,
        // PMAT-502k: seq * n has the same type as the sequence.
        Expr::Repeat { seq, .. } => infer_type_in_ctx(ctx, seq),
        // PMAT-502c: sorted(xs) has the same type as its list.
        Expr::Sorted { list, .. } => infer_type_in_ctx(ctx, list),
        // PMAT-502d: reversed(xs) has the same type as its list.
        Expr::Reversed { list } => infer_type_in_ctx(ctx, list),
        // PMAT-502ab: filter(pred, xs) keeps the input list type.
        Expr::Filter { list, .. } => infer_type_in_ctx(ctx, list),
        // PMAT-502ac: map(f, xs) → List of the body's transformed type.
        Expr::Map { lambda, .. } => Type::List(Box::new(infer_type_in_ctx(ctx, &lambda.body))),
        // PMAT-502ai: enumerate/zip → List(Tuple[...]).
        Expr::Enumerate { list } => {
            let elem = match infer_type_in_ctx(ctx, list) {
                Type::List(e) => *e,
                _ => Type::I64,
            };
            Type::List(Box::new(Type::Tuple(vec![Type::I64, elem])))
        }
        Expr::Zip { left, right } => {
            let el = match infer_type_in_ctx(ctx, left) {
                Type::List(e) => *e,
                _ => Type::I64,
            };
            let er = match infer_type_in_ctx(ctx, right) {
                Type::List(e) => *e,
                _ => Type::I64,
            };
            Type::List(Box::new(Type::Tuple(vec![el, er])))
        }
        // PMAT-502e: min(xs)/max(xs) reduce a list to its element type.
        Expr::ListMinMax { list, .. } => match infer_type_in_ctx(ctx, list) {
            Type::List(elem) => *elem,
            _ => Type::I64,
        },
        // PMAT-502u: list.count(x)/index(x) return Int.
        Expr::ListQuery { .. } => Type::I64,
        // PMAT-502as: list.pop() returns the list's element type.
        Expr::ListPop { list, .. } => match infer_type_in_ctx(ctx, list) {
            Type::List(elem) => *elem,
            _ => Type::I64,
        },
        // PMAT-502au: dict.pop() returns the dict's value type.
        Expr::DictPop { dict, .. } => match infer_type_in_ctx(ctx, dict) {
            Type::Dict(_, v) => *v,
            _ => Type::I64,
        },
        // PMAT-502ax: dict.setdefault() returns the dict's value type.
        Expr::DictSetDefault { dict, .. } => match infer_type_in_ctx(ctx, dict) {
            Type::Dict(_, v) => *v,
            _ => Type::I64,
        },
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
            // PMAT-502cd: `s[i]` over a string → `Expr::StrCharAt` (a 1-char
            // string). Handles positive, negative, and variable int indices;
            // the codegen materialises the chars and indexes them. Rejects a
            // non-int index.
            if matches!(infer_type_in_ctx(ctx, &collection), Type::Str) {
                let index = lower_expr_in_ctx(ctx, (*sub.slice).clone())?;
                let idx_ty = infer_type_in_ctx(ctx, &index);
                if !matches!(idx_ty, Type::I64) {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` indexes a string with a {idx_ty:?} index — only `int` is supported",
                        ctx.fn_name
                    )));
                }
                return Ok(Expr::StrCharAt {
                    string: Box::new(collection),
                    index: Box::new(index),
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
                // PMAT-502ax: `d.setdefault(k, default)` — get-or-insert over
                // a dict (2 args). Mutates on the absent path, so the receiver
                // is marked mut by the `count_pop_receivers` pre-pass (which
                // also scans `.setdefault`).
                // First cut requires the explicit default (1-arg setdefault,
                // which defaults to `None`, needs Optional support).
                if attr.attr.as_str() == "setdefault" {
                    let recv = lower_expr_in_ctx(ctx, (*attr.value).clone())?;
                    if matches!(infer_type_in_ctx(ctx, &recv), Type::Dict(_, _)) {
                        if !call.keywords.is_empty() || call.args.len() != 2 {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` calls dict `.setdefault(...)` with {} positional \
                                 arg(s){} — v0.2.0 supports exactly `.setdefault(key, default)`",
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
                        return Ok(Expr::DictSetDefault {
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
                // PMAT-502bh: `"<fmt>".format(args…)` — Python str.format
                // with sequential `{}` placeholders. The receiver must be a
                // string literal so the format string is validated at lower
                // time; the `{}` count must match the arg count; args are
                // int/str (bool/float Display differently than Python).
                if attr.attr.as_str() == "format" && call.keywords.is_empty() {
                    if let ast::Expr::Constant(c) = attr.value.as_ref() {
                        if let ast::Constant::Str(fmt) = &c.value {
                            // PMAT-502cb: accept automatic `{}` and positional
                            // `{N}` (both re-emitted verbatim — Rust shares the
                            // syntax). `{name}`/`{:spec}`/mixed are rejected.
                            let nargs = call.args.len();
                            match parse_format_placeholders(fmt).ok_or_else(|| {
                                FrontendError::Lower(format!(
                                    "function `{}` uses str.format with unsupported placeholders (`{{name}}`/`{{:spec}}`/mixed `{{}}`+`{{0}}`) — v0.2.0 supports `{{}}` or `{{N}}`",
                                    ctx.fn_name
                                ))
                            })? {
                                FmtPlaceholders::Sequential(n) => {
                                    if n != nargs {
                                        return Err(FrontendError::Lower(format!(
                                            "function `{}` calls str.format with {n} `{{}}` placeholder(s) but {nargs} arg(s)",
                                            ctx.fn_name
                                        )));
                                    }
                                }
                                FmtPlaceholders::Positional(indices) => {
                                    // Rust's `format!` requires every positional
                                    // arg be referenced and every index be in
                                    // range — validate both.
                                    if let Some(&max) = indices.iter().max() {
                                        if max >= nargs {
                                            return Err(FrontendError::Lower(format!(
                                                "function `{}` uses str.format placeholder `{{{max}}}` but only {nargs} arg(s) were given",
                                                ctx.fn_name
                                            )));
                                        }
                                    }
                                    for k in 0..nargs {
                                        if !indices.contains(&k) {
                                            return Err(FrontendError::Lower(format!(
                                                "function `{}` calls str.format with {nargs} arg(s) but never references `{{{k}}}` — Rust's format! requires every positional argument be used",
                                                ctx.fn_name
                                            )));
                                        }
                                    }
                                }
                            }
                            let mut args = Vec::with_capacity(call.args.len());
                            for a in &call.args {
                                let lowered = lower_expr_in_ctx(ctx, a.clone())?;
                                match infer_type_in_ctx(ctx, &lowered) {
                                    Type::I64 | Type::Str => args.push(lowered),
                                    other => {
                                        return Err(FrontendError::Lower(format!(
                                            "function `{}` formats a {other:?} value via str.format; v0.2.0 supports int/str args (bool/float deferred — they Display differently than Python)",
                                            ctx.fn_name
                                        )))
                                    }
                                }
                            }
                            return Ok(Expr::StrFormat {
                                fmt: fmt.clone(),
                                args,
                            });
                        }
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
                // PMAT-502as: `xs.pop()` / `xs.pop(i)` — an expression that
                // removes and returns an element (so the receiver mutates).
                // 0 args → remove last; 1 int arg → remove at that index.
                if attr.attr.as_str() == "pop" {
                    let recv = lower_expr_in_ctx(ctx, (*attr.value).clone())?;
                    let recv_ty = infer_type_in_ctx(ctx, &recv);
                    if matches!(recv_ty, Type::List(_))
                        && call.keywords.is_empty()
                        && call.args.len() <= 1
                    {
                        let index = match call.args.first() {
                            None => None,
                            Some(a) => {
                                let i = lower_expr_in_ctx(ctx, a.clone())?;
                                if infer_type_in_ctx(ctx, &i) != Type::I64 {
                                    return Err(FrontendError::Lower(format!(
                                        "function `{}` calls list `.pop(<index>)` with a \
                                         non-int index; v0.2.0 requires an int position",
                                        ctx.fn_name
                                    )));
                                }
                                Some(Box::new(i))
                            }
                        };
                        // Receiver mutability is handled entirely by the
                        // `count_pop_receivers` pre-pass (a popped param or
                        // local crosses the `> 1` count → `mut`); this
                        // expr-lowering path has only `&ctx`.
                        return Ok(Expr::ListPop {
                            list: Box::new(recv),
                            index,
                        });
                    }
                    // PMAT-502au: `d.pop(k)` / `d.pop(k, default)` over a
                    // dict — 1 or 2 positional args (a no-arg `.pop()` is a
                    // Python error for dicts). The receiver is marked mut by
                    // the same `count_pop_receivers` pre-pass as list pop.
                    if matches!(recv_ty, Type::Dict(_, _))
                        && call.keywords.is_empty()
                        && (call.args.len() == 1 || call.args.len() == 2)
                    {
                        let key = Box::new(lower_expr_in_ctx(ctx, call.args[0].clone())?);
                        let default = match call.args.get(1) {
                            None => None,
                            Some(a) => Some(Box::new(lower_expr_in_ctx(ctx, a.clone())?)),
                        };
                        return Ok(Expr::DictPop {
                            dict: Box::new(recv),
                            key,
                            default,
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
                // numeric arg → `(x) as i64` / `(x) as f64`. PMAT-502bf: over
                // a `str` arg → a trimmed `.parse()` (panics on bad input,
                // matching Python's `ValueError`).
                if matches!(fname.id.as_str(), "int" | "float")
                    && call.keywords.is_empty()
                    && call.args.len() == 1
                {
                    let value = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    let vty = infer_type_in_ctx(ctx, &value);
                    if matches!(vty, Type::I64 | Type::F64 | Type::Str) {
                        return Ok(Expr::NumCast {
                            value: Box::new(value),
                            to_float: fname.id.as_str() == "float",
                            from_str: matches!(vty, Type::Str),
                        });
                    }
                }
                // PMAT-502be: `bool(x)` truthiness cast — a pure desugar to a
                // `!= 0` comparison (no new Expr). int → `x != 0`; str / list /
                // dict / set → `len(x) != 0`; bool → identity. (float deferred.)
                if fname.id.as_str() == "bool" && call.keywords.is_empty() && call.args.len() == 1 {
                    let value = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    let ne_zero = |lhs: Expr| Expr::BinOp {
                        op: BinOp::NotEq,
                        lhs: Box::new(lhs),
                        rhs: Box::new(Expr::LitInt(0)),
                    };
                    return match infer_type_in_ctx(ctx, &value) {
                        Type::Bool => Ok(value),
                        Type::I64 => Ok(ne_zero(value)),
                        Type::Str | Type::List(_) | Type::Dict(_, _) | Type::Set(_) => {
                            Ok(ne_zero(Expr::Len(Box::new(value))))
                        }
                        other => Err(FrontendError::Lower(format!(
                            "function `{}` calls `bool(...)` on a {other:?}; v0.2.0 supports bool over int/bool/str/list/dict/set",
                            ctx.fn_name
                        ))),
                    };
                }
                // PMAT-502ad/af: `str(x)` over an `int`/`float` → `Expr::ToStr`
                // (int → `format!("{}", x)`; float → a Python-matching format
                // block). PMAT-502ae: `str(b)` over a `bool` desugars to
                // `"True" if b else "False"` (an `IfExpr`) — Python capitalizes
                // (`"True"`/`"False"`), unlike Rust's lowercase `format!`.
                if fname.id.as_str() == "str" && call.keywords.is_empty() && call.args.len() == 1 {
                    let value = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    match infer_type_in_ctx(ctx, &value) {
                        Type::I64 => {
                            return Ok(Expr::ToStr {
                                value: Box::new(value),
                                of_float: false,
                            });
                        }
                        Type::F64 => {
                            return Ok(Expr::ToStr {
                                value: Box::new(value),
                                of_float: true,
                            });
                        }
                        Type::Bool => {
                            return Ok(Expr::IfExpr {
                                cond: Box::new(value),
                                then_expr: Box::new(Expr::LitStr("True".to_string())),
                                else_expr: Box::new(Expr::LitStr("False".to_string())),
                            });
                        }
                        _ => {}
                    }
                }
                // PMAT-502ak: `round(x)` (1-arg). Over a `float` → the nearest
                // int via banker's rounding (`Expr::RoundToInt`); over an
                // `int` it's the identity (return the value as-is).
                if fname.id.as_str() == "round" && call.keywords.is_empty() && call.args.len() == 1
                {
                    let value = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    match infer_type_in_ctx(ctx, &value) {
                        Type::F64 => {
                            return Ok(Expr::RoundToInt {
                                value: Box::new(value),
                            });
                        }
                        Type::I64 => return Ok(value),
                        _ => {}
                    }
                }
                // PMAT-502al: `round(x, n)` (2-arg) over a `float` x and `int`
                // n → the float rounded to n decimals (`Expr::RoundToDigits`,
                // returns a `Float`, banker's rounding after `10^n` scaling).
                if fname.id.as_str() == "round" && call.keywords.is_empty() && call.args.len() == 2
                {
                    let value = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    let ndigits = lower_expr_in_ctx(ctx, call.args[1].clone())?;
                    if infer_type_in_ctx(ctx, &value) == Type::F64
                        && infer_type_in_ctx(ctx, &ndigits) == Type::I64
                    {
                        return Ok(Expr::RoundToDigits {
                            value: Box::new(value),
                            ndigits: Box::new(ndigits),
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
                // PMAT-502ab: `filter(lambda p: pred, xs)` over a list → a new
                // list of the elements where the Bool predicate holds
                // (materializing the lazy iterator). The body is lowered with
                // `p` unbound (same as sorted-key, v0.1.58); non-Bool bodies
                // (Python truthiness) fall through to error.
                if fname.id.as_str() == "filter" && call.keywords.is_empty() && call.args.len() == 2
                {
                    if let ast::Expr::Lambda(lam) = &call.args[0] {
                        if lam.args.args.len() == 1
                            && lam.args.posonlyargs.is_empty()
                            && lam.args.kwonlyargs.is_empty()
                            && lam.args.vararg.is_none()
                            && lam.args.kwarg.is_none()
                        {
                            let list = lower_expr_in_ctx(ctx, call.args[1].clone())?;
                            if matches!(infer_type_in_ctx(ctx, &list), Type::List(_)) {
                                let param = lam.args.args[0].def.arg.to_string();
                                let body = lower_expr_in_ctx(ctx, (*lam.body).clone())?;
                                if infer_type_in_ctx(ctx, &body) == Type::Bool {
                                    return Ok(Expr::Filter {
                                        list: Box::new(list),
                                        lambda: SortKey {
                                            param,
                                            body: Box::new(body),
                                        },
                                    });
                                }
                            }
                        }
                    }
                }
                // PMAT-502ac: `map(lambda p: e, xs)` over a list → a new list
                // of the transformed elements (materializing the lazy
                // iterator). Like filter, the body is lowered with `p`
                // unbound; the result element type is the body's type.
                if fname.id.as_str() == "map" && call.keywords.is_empty() && call.args.len() == 2 {
                    if let ast::Expr::Lambda(lam) = &call.args[0] {
                        if lam.args.args.len() == 1
                            && lam.args.posonlyargs.is_empty()
                            && lam.args.kwonlyargs.is_empty()
                            && lam.args.vararg.is_none()
                            && lam.args.kwarg.is_none()
                        {
                            let list = lower_expr_in_ctx(ctx, call.args[1].clone())?;
                            if matches!(infer_type_in_ctx(ctx, &list), Type::List(_)) {
                                let param = lam.args.args[0].def.arg.to_string();
                                let body = lower_expr_in_ctx(ctx, (*lam.body).clone())?;
                                return Ok(Expr::Map {
                                    list: Box::new(list),
                                    lambda: SortKey {
                                        param,
                                        body: Box::new(body),
                                    },
                                });
                            }
                        }
                    }
                }
                // PMAT-502ai: `enumerate(xs)` over a list → a Vec of
                // (index, element) tuples (materializing the lazy iterator).
                if fname.id.as_str() == "enumerate"
                    && call.keywords.is_empty()
                    && call.args.len() == 1
                {
                    let list = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    if matches!(infer_type_in_ctx(ctx, &list), Type::List(_)) {
                        return Ok(Expr::Enumerate {
                            list: Box::new(list),
                        });
                    }
                }
                // PMAT-502ai: `zip(xs, ys)` over two lists → a Vec of paired
                // tuples (truncated to the shorter).
                if fname.id.as_str() == "zip" && call.keywords.is_empty() && call.args.len() == 2 {
                    let left = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    let right = lower_expr_in_ctx(ctx, call.args[1].clone())?;
                    if matches!(infer_type_in_ctx(ctx, &left), Type::List(_))
                        && matches!(infer_type_in_ctx(ctx, &right), Type::List(_))
                    {
                        return Ok(Expr::Zip {
                            left: Box::new(left),
                            right: Box::new(right),
                        });
                    }
                }
                // PMAT-502d: `list(reversed(xs))` — the `list(...)` wrapper is
                // a no-op once the inner `reversed(xs)` already materializes
                // to a `Vec`. Unwrap a single already-list-typed argument.
                if fname.id.as_str() == "list" && call.keywords.is_empty() && call.args.len() == 1 {
                    let inner = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    if matches!(
                        inner,
                        Expr::Reversed { .. }
                            | Expr::Sorted { .. }
                            | Expr::Filter { .. }
                            | Expr::Map { .. }
                            | Expr::Enumerate { .. }
                            | Expr::Zip { .. }
                    ) {
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
                // PMAT-502an: `x in xs` / `x not in xs` over a list.
                if matches!(infer_type_in_ctx(ctx, &rhs), Type::List(_)) {
                    let elem = lower_expr_in_ctx(ctx, (*c.left).clone())?;
                    let contains = Expr::ListContains {
                        list: Box::new(rhs),
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
            // PMAT-502bs: Python 3 `/` is ALWAYS true division → f64, even
            // for two int operands (`7 / 2 == 3.5`). Cast non-float
            // operands to f64 and emit `FloatBinOp::Div`. This also fixes
            // mixed `float_var / int_literal` (the int side gets cast, so
            // no `f64 / i64` mismatch). Floor-division `//` stays integer.
            if matches!(b.op, ast::Operator::Div) {
                return Ok(Expr::FloatBinOp {
                    op: FloatOp::Div,
                    lhs: Box::new(to_f64_operand(ctx, lhs)),
                    rhs: Box::new(to_f64_operand(ctx, rhs)),
                });
            }
            // PMAT-502bt: Python `a ** b` with a float operand → float power
            // `(a).powf(b)`. Both operands cast to f64 (powf needs f64).
            // `int ** int` stays integer (`checked_pow`).
            if matches!(b.op, ast::Operator::Pow)
                && (infer_type_in_ctx(ctx, &lhs) == Type::F64
                    || infer_type_in_ctx(ctx, &rhs) == Type::F64)
            {
                return Ok(Expr::FloatBinOp {
                    op: FloatOp::Pow,
                    lhs: Box::new(to_f64_operand(ctx, lhs)),
                    rhs: Box::new(to_f64_operand(ctx, rhs)),
                });
            }
            // PMAT-477 (R8): float arithmetic → FloatBinOp (plain infix).
            // Detected before `lower_binop`. Float *comparisons* fall
            // through to BinOp (plain infix is already f64-correct, yields
            // Bool).
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
            // PMAT-502bg: `xs + ys` over two lists → list concatenation
            // (disambiguated from int `+` by operand type).
            if matches!(op, BinOp::Add)
                && matches!(infer_type_in_ctx(ctx, &lhs), Type::List(_))
                && matches!(infer_type_in_ctx(ctx, &rhs), Type::List(_))
            {
                return Ok(Expr::ListConcat {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                });
            }
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
        // PMAT-502am: f-strings lower context-aware so a `{x:.2f}` field can
        // see the value's type (the context-free path can't, and rejects
        // format specs). Plain `{name}` parts still work via Display.
        ast::Expr::JoinedStr(js) => lower_fstring_in_ctx(ctx, js.values),
        // PMAT-502bp: context-aware unary `-` over a *float* expression
        // (`-x` where `x: float`) → `0.0 - x` (a `FloatBinOp`, plain infix),
        // since the generic `UnOp::Neg` emits the i64-only `checked_neg`.
        // The context-free path can't see that `x` is a float; everything
        // else (i64 negation, `not`, the negative-float-literal fold) still
        // routes through `lower_unary_op`.
        ast::Expr::UnaryOp(u) if matches!(u.op, ast::UnaryOp::USub) => {
            let operand = lower_expr_in_ctx(ctx, (*u.operand).clone())?;
            // A float *literal* keeps the cleaner negative-literal fold
            // (PMAT-502bo); only a non-literal float *expression* needs the
            // `0.0 - x` form.
            if matches!(infer_type_in_ctx(ctx, &operand), Type::F64)
                && !matches!(operand, Expr::LitFloat(_))
            {
                Ok(Expr::FloatBinOp {
                    op: FloatOp::Sub,
                    lhs: Box::new(Expr::LitFloat(0.0)),
                    rhs: Box::new(operand),
                })
            } else {
                lower_unary_op(u)
            }
        }
        // PMAT-502cc: context-aware `not <bool var>`. The context-free
        // `lower_unary_op` infers a bare Ident as I64 and so rejects
        // `not b` for a `bool` parameter/local; using `infer_type_in_ctx`
        // sees the real type. Non-Bool operands still error (no
        // int-truthiness), via the context-free fallback.
        ast::Expr::UnaryOp(u) if matches!(u.op, ast::UnaryOp::Not) => {
            let operand = lower_expr_in_ctx(ctx, (*u.operand).clone())?;
            if matches!(infer_type_in_ctx(ctx, &operand), Type::Bool) {
                Ok(Expr::UnOp {
                    op: UnOp::Not,
                    operand: Box::new(operand),
                })
            } else {
                lower_unary_op(u)
            }
        }
        // PMAT-502ce: context-aware `a and b` / `a or b`. The context-free
        // path mis-infers a bare Ident as I64 and rejects bool variables.
        ast::Expr::BoolOp(b) => lower_bool_op_in_ctx(ctx, b),
        // No dict-specific shape: the context-free path is sufficient.
        other => lower_expr(other),
    }
}

/// PMAT-502am (ctx-aware f-string): fold the `values` parts into a
/// left-associative [`Expr::Concat`] chain, lowering each part with `ctx` so a
/// `FormattedValue` with a static format spec (`{x:.2f}`) can be type-checked
/// and translated. Mirrors [`lower_fstring`] but ctx-aware.
fn lower_fstring_in_ctx(ctx: &LoweringCtx, values: Vec<ast::Expr>) -> Result<Expr, FrontendError> {
    let mut parts = values.into_iter();
    let Some(first) = parts.next() else {
        return Ok(Expr::LitStr(String::new()));
    };
    let mut acc = lower_fstring_part_in_ctx(ctx, first)?;
    for v in parts {
        let rhs = lower_fstring_part_in_ctx(ctx, v)?;
        acc = Expr::Concat {
            lhs: Box::new(acc),
            rhs: Box::new(rhs),
        };
    }
    Ok(acc)
}

/// Lower a single f-string part (a literal `Constant` or a `FormattedValue`)
/// context-aware. A `FormattedValue` with a static, supported format spec
/// becomes [`Expr::FormatSpec`]; a plain `{expr}` lowers its value; conversion
/// flags (`!r`/`!s`/`!a`) and unsupported / dynamic specs error.
fn lower_fstring_part_in_ctx(ctx: &LoweringCtx, part: ast::Expr) -> Result<Expr, FrontendError> {
    let ast::Expr::FormattedValue(fv) = part else {
        return lower_expr_in_ctx(ctx, part);
    };
    if fv.conversion != ast::ConversionFlag::None {
        return Err(FrontendError::Lower(
            "f-string conversion flags (`!r` / `!s` / `!a`) are not supported at v0.2.0".into(),
        ));
    }
    let value = lower_expr_in_ctx(ctx, (*fv.value).clone())?;
    let Some(spec_expr) = fv.format_spec.as_ref() else {
        // Plain `{expr}` — no spec; Display-coerced by the surrounding format!.
        return Ok(value);
    };
    let Some(spec) = static_format_spec(spec_expr) else {
        return Err(FrontendError::Lower(
            "f-string format spec must be a static literal (dynamic widths like `{x:{w}}` \
             are not supported at v0.2.0)"
                .into(),
        ));
    };
    if spec.is_empty() {
        return Ok(value); // `{x:}` — empty spec, same as plain.
    }
    let ty = infer_type_in_ctx(ctx, &value);
    match translate_format_spec(&spec, &ty) {
        Some(rust_spec) => Ok(Expr::FormatSpec {
            value: Box::new(value),
            rust_spec,
        }),
        None => Err(FrontendError::Lower(format!(
            "unsupported f-string format spec `:{spec}` (for a {ty:?} value) — supported: \
             `.Nf` (float), `0Nd`/`Nd` (int), `>N`/`<N`/`^N` (align) at v0.2.0"
        ))),
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
    let mut step_lit: Option<i64> = None;
    if let Some(step) = slice.step.as_ref() {
        // PMAT-502t: the reverse idiom `xs[::-1]` (no bounds, step −1) over a
        // list → a new reversed list, reusing `Expr::Reversed` (PMAT-502d). A
        // negative literal parses as `UnaryOp(USub, Int(1))`.
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
        // PMAT-502bc: a **positive** integer-literal step over a *list*
        // (`xs[a:b:c]`, `xs[::2]`). A step of 1 is the default (drop it).
        // Negative steps (other than the `-1` reverse above) and stepped
        // string slices remain deferred.
        if let ast::Expr::Constant(c) = step.as_ref() {
            if let ast::Constant::Int(k) = &c.value {
                if let Ok(s) = k.to_string().parse::<i64>() {
                    if s >= 1 && !of_str {
                        step_lit = if s == 1 { None } else { Some(s) };
                    } else {
                        return Err(FrontendError::Lower(format!(
                            "function `{}` uses a slice step that is not a positive list step; \
                             v0.2.0 supports `xs[a:b:c]` over a list with a positive literal `c` \
                             (and the `xs[::-1]` reverse idiom); negative/string steps are deferred",
                            ctx.fn_name
                        )));
                    }
                } else {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` uses a non-`i64` slice step — unsupported",
                        ctx.fn_name
                    )));
                }
            } else {
                return Err(FrontendError::Lower(format!(
                    "function `{}` uses a non-integer slice step — unsupported",
                    ctx.fn_name
                )));
            }
        } else {
            return Err(FrontendError::Lower(format!(
                "function `{}` uses a non-literal slice step; v0.2.0 requires a positive integer literal step",
                ctx.fn_name
            )));
        }
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
        step: step_lit,
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
            // PMAT-502bs: Python 3 `/` is ALWAYS true division → f64 (see
            // the ctx-aware arm). Context-free, only float *literals* are
            // known floats; other operands are cast to f64.
            if matches!(b.op, ast::Operator::Div) {
                return Ok(Expr::FloatBinOp {
                    op: FloatOp::Div,
                    lhs: Box::new(to_f64_operand_cf(lhs)),
                    rhs: Box::new(to_f64_operand_cf(rhs)),
                });
            }
            // PMAT-502bt: float power `a ** b` (context-free detects float
            // *literals*; param-typed floats are caught by the ctx path).
            if matches!(b.op, ast::Operator::Pow)
                && (infer_type(&lhs) == Type::F64 || infer_type(&rhs) == Type::F64)
            {
                return Ok(Expr::FloatBinOp {
                    op: FloatOp::Pow,
                    lhs: Box::new(to_f64_operand_cf(lhs)),
                    rhs: Box::new(to_f64_operand_cf(rhs)),
                });
            }
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
            // PMAT-502bg: `xs + ys` over two lists → list concatenation.
            if matches!(op, BinOp::Add)
                && matches!(infer_type(&lhs), Type::List(_))
                && matches!(infer_type(&rhs), Type::List(_))
            {
                return Ok(Expr::ListConcat {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                });
            }
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

/// PMAT-502am: extract a **static** f-string format spec (the common case
/// where `{x:.2f}` has a literal spec, not a nested-expr dynamic width). The
/// `format_spec` is itself a `JoinedStr`; a static spec is a single
/// `Constant(Str)` part. Returns the raw Python spec string, or `None` for a
/// dynamic / non-literal spec (caller errors).
fn static_format_spec(spec: &ast::Expr) -> Option<String> {
    let ast::Expr::JoinedStr(js) = spec else {
        return None;
    };
    match js.values.as_slice() {
        // `{x:}` (empty spec) — an empty JoinedStr.
        [] => Some(String::new()),
        [ast::Expr::Constant(c)] => match &c.value {
            ast::Constant::Str(s) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// PMAT-502am: translate a static Python format spec to the equivalent Rust
/// format spec for the supported subset, given the formatted value's type.
/// Returns `None` for unsupported specs (the caller errors so they aren't
/// silently mis-formatted). Supported:
///   * `.Nf` — fixed-point float, N decimals (requires a `float` value) → `.N`
///   * `0Nd` / `Nd` — integer width / zero-pad (requires `int`) → `0N` / `N`
///   * `>N` / `<N` / `^N` — alignment within width N (any Display value) → same
fn translate_format_spec(spec: &str, ty: &Type) -> Option<String> {
    // `.Nf` — fixed-point float.
    if let Some(rest) = spec.strip_prefix('.') {
        if let Some(n) = rest.strip_suffix('f') {
            if *ty == Type::F64 && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) {
                return Some(format!(".{n}"));
            }
        }
        return None;
    }
    // `0Nd` / `Nd` — integer width / zero-pad.
    if let Some(n) = spec.strip_suffix('d') {
        if *ty == Type::I64 && !n.is_empty() {
            let (zero, width) = match n.strip_prefix('0') {
                Some(w) => ("0", w),
                None => ("", n),
            };
            if !width.is_empty() && width.bytes().all(|b| b.is_ascii_digit()) {
                return Some(format!("{zero}{width}"));
            }
        }
        return None;
    }
    // `>N` / `<N` / `^N` — alignment within width (any Display type).
    if let Some(align) = spec.chars().next() {
        if matches!(align, '<' | '>' | '^') {
            let width = &spec[align.len_utf8()..];
            if !width.is_empty() && width.bytes().all(|b| b.is_ascii_digit()) {
                // Rust uses the same `[align][width]` syntax.
                return Some(spec.to_string());
            }
        }
    }
    None
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

/// PMAT-502ce: context-aware `a and b` / `a or b`. The context-free
/// [`lower_bool_op`] infers a bare Ident as I64 and so rejects `a and b` for
/// `bool` parameters/locals; using `infer_type_in_ctx` sees the real Bool
/// type. Same recurring fix as `not` (PMAT-502cc) and float-var negation.
fn lower_bool_op_in_ctx(ctx: &LoweringCtx, b: ast::ExprBoolOp) -> Result<Expr, FrontendError> {
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
    let first = lower_expr_in_ctx(ctx, iter.next().expect("len ≥ 2"))?;
    if infer_type_in_ctx(ctx, &first) != Type::Bool {
        return Err(FrontendError::Lower(
            "operands of `and`/`or` must be Bool (no int-truthiness at v0.1.0)".into(),
        ));
    }
    let mut acc = first;
    for next in iter {
        let rhs = lower_expr_in_ctx(ctx, next)?;
        if infer_type_in_ctx(ctx, &rhs) != Type::Bool {
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
            // PMAT-502bo: a negated float literal (`-3.14`) folds to a
            // single negative `LitFloat` — `UnOp::Neg` emits `checked_neg`,
            // which is i64-only. (Float *variable* negation `-x` needs
            // context-aware typing and is deferred.)
            if let Expr::LitFloat(f) = operand {
                return Ok(Expr::LitFloat(-f));
            }
            if infer_type(&operand) != Type::I64 {
                return Err(FrontendError::Lower(
                    "unary `-` requires an I64 operand or a float literal (float-variable negation is deferred)".into(),
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
/// (comparisons stay on `BinOp`) and for bitwise/`**` on floats
/// (deferred). PMAT-502br: `//`/`%` now map to the floor-semantics
/// `FloorDiv`/`Mod` (Python float floor-division + sign-of-divisor
/// modulo); the codegen emits the `(a / b).floor()` formulas.
fn float_op_from_ast(op: &ast::Operator) -> Option<FloatOp> {
    match op {
        ast::Operator::Add => Some(FloatOp::Add),
        ast::Operator::Sub => Some(FloatOp::Sub),
        ast::Operator::Mult => Some(FloatOp::Mul),
        ast::Operator::Div => Some(FloatOp::Div),
        ast::Operator::FloorDiv => Some(FloatOp::FloorDiv),
        ast::Operator::Mod => Some(FloatOp::Mod),
        // PMAT-502bu: `**` over floats. In the expression-position BinOp
        // arms a dedicated branch handles Pow first (casting operands), so
        // this mapping is only reached via `combine_aug` (`x **= y`).
        ast::Operator::Pow => Some(FloatOp::Pow),
        _ => None,
    }
}

/// PMAT-502bs: wrap `e` in an `(e) as f64` cast unless it already types as
/// `F64`. Used by Python-3 true division `/`, which always yields a float
/// even for two int operands (`7 / 2 == 3.5`).
fn to_f64_operand(ctx: &LoweringCtx, e: Expr) -> Expr {
    if infer_type_in_ctx(ctx, &e) == Type::F64 {
        e
    } else {
        Expr::NumCast {
            value: Box::new(e),
            to_float: true,
            from_str: false,
        }
    }
}

/// Context-free counterpart of [`to_f64_operand`] (uses `infer_type`, so
/// only float *literals* are recognised as already-`F64`).
fn to_f64_operand_cf(e: Expr) -> Expr {
    if infer_type(&e) == Type::F64 {
        e
    } else {
        Expr::NumCast {
            value: Box::new(e),
            to_float: true,
            from_str: false,
        }
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

/// PMAT-502bh: count the sequential `{}` placeholders in a Python
/// `str.format` template, treating `{{` / `}}` as literal-brace escapes.
/// Returns `None` if the template uses an unsupported field — an indexed
/// (`{0}`), named (`{name}`), or spec'd (`{:.2f}`) placeholder, or an
/// unmatched `{` / `}`. Braces are ASCII, so byte-walking is UTF-8-safe.
/// PMAT-502cb: the placeholder shape of a `str.format` format string.
enum FmtPlaceholders {
    /// All-automatic `{}` placeholders — the `usize` is the count.
    Sequential(usize),
    /// All-positional `{N}` placeholders — the indices in order of appearance.
    Positional(Vec<usize>),
}

/// PMAT-502bh/cb: classify a `str.format` format string. Returns `None` for
/// `{name}` / `{:spec}` / lone braces / a mix of `{}` and `{N}` (Python
/// forbids switching between automatic and manual field numbering). `{{`/`}}`
/// are brace escapes. Rust's `format!` accepts the same `{}` and `{N}`
/// syntaxes verbatim, so the format string is re-emitted unchanged.
fn parse_format_placeholders(fmt: &str) -> Option<FmtPlaceholders> {
    let b = fmt.as_bytes();
    let mut i = 0;
    let mut seq_count = 0usize;
    let mut indices: Vec<usize> = Vec::new();
    let mut saw_seq = false;
    let mut saw_pos = false;
    while i < b.len() {
        match b[i] {
            b'{' => {
                if i + 1 < b.len() && b[i + 1] == b'{' {
                    i += 2; // `{{` escape
                } else if i + 1 < b.len() && b[i + 1] == b'}' {
                    saw_seq = true;
                    seq_count += 1;
                    i += 2; // automatic `{}`
                } else {
                    // Try to parse a positional `{N}`.
                    let start = i + 1;
                    let mut j = start;
                    while j < b.len() && b[j].is_ascii_digit() {
                        j += 1;
                    }
                    if j > start && j < b.len() && b[j] == b'}' {
                        let idx: usize = fmt[start..j].parse().ok()?;
                        saw_pos = true;
                        indices.push(idx);
                        i = j + 1;
                    } else {
                        return None; // `{name}` / `{:spec}` / lone `{`
                    }
                }
            }
            b'}' => {
                if i + 1 < b.len() && b[i + 1] == b'}' {
                    i += 2; // `}}` escape
                } else {
                    return None; // lone `}`
                }
            }
            _ => i += 1,
        }
    }
    if saw_seq && saw_pos {
        return None; // Python forbids mixing `{}` and `{N}`
    }
    if saw_pos {
        Some(FmtPlaceholders::Positional(indices))
    } else {
        Some(FmtPlaceholders::Sequential(seq_count))
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
        // PMAT-502bi: str.index (1-arg, → Int; panics if absent = ValueError).
        "index" => Some(StrMethodOp::StrIndex),
        // PMAT-502ag: classification predicates (0-arg).
        "isdigit" => Some(StrMethodOp::IsDigit),
        "isalpha" => Some(StrMethodOp::IsAlpha),
        "isspace" => Some(StrMethodOp::IsSpace),
        // PMAT-502ah: capitalize (0-arg).
        "capitalize" => Some(StrMethodOp::Capitalize),
        "title" => Some(StrMethodOp::Title),
        // PMAT-502aw: rjust/ljust (1-arg width).
        "rjust" => Some(StrMethodOp::RJust),
        "ljust" => Some(StrMethodOp::LJust),
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
        StrMethodOp::Find | StrMethodOp::Count | StrMethodOp::StrIndex => 1,
        // PMAT-502ag: classification predicates take no args.
        StrMethodOp::IsDigit | StrMethodOp::IsAlpha | StrMethodOp::IsSpace => 0,
        // PMAT-502ah: capitalize takes no args.
        StrMethodOp::Capitalize | StrMethodOp::Title => 0,
        // PMAT-502aw: rjust/ljust take one width arg.
        StrMethodOp::RJust | StrMethodOp::LJust => 1,
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
            Item::Const { .. } => panic!("expected a function item, found a constant"),
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
