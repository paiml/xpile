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
    NumBuiltinOp, PairIterKind, Param, Radix, SetOp, SetPredOp, SortKey, SourceLang, Stmt,
    StrMethodOp, Type, UnOp,
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
    /// PMAT-502ct: per-parameter default value (the Python AST expression),
    /// aligned with `params` (`None` for a parameter without a default).
    /// Used to fill omitted trailing arguments at call sites.
    defaults: Vec<Option<ast::Expr>>,
    /// PMAT-502dq: the `*args` vararg parameter (name + element type), when
    /// the function is variadic. `params` holds only the fixed parameters, so
    /// the vararg starts at index `params.len()`. At a call site the trailing
    /// positional args are collected into a single `list[elem]` argument.
    variadic: Option<(String, Type)>,
}

#[derive(Clone)]
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
    /// PMAT-588 (ownership cluster): per-name source READ count (number of
    /// `Name`-load occurrences in the function body). A non-Copy value passed
    /// by value to a function call is MOVED; if the same variable is read more
    /// than once, the second use fails to compile (rustc E0382 "use of moved
    /// value", e.g. `helper(xs) + helper(xs)`). Such a call argument is cloned
    /// so the original survives. Computed once in a pre-walk, like `mutable`.
    read_counts: HashMap<String, usize>,
    /// PMAT-471 (R2): module-level signature table — every top-level
    /// function's declared return type, built in a pre-pass before any
    /// function is lowered. Consulted when typing `Expr::Call` so a
    /// call to *another* function (e.g. `d = make_dict()`) gets its real
    /// return type instead of the old hardcoded `Type::I64` fallback
    /// (which silently emitted `let d: i64` and broke rustc). Shared
    /// across all functions in the module via `Rc`.
    signatures: Rc<HashMap<String, FnSig>>,
    /// PMAT-506b (classes epic): module-level struct table — every
    /// `@dataclass`/class's ordered `(field_name, field_type)` list, built in a
    /// pre-pass. Consulted to lower struct construction (`Name(a, b)` →
    /// `StructLit`, mapping positional args to field names) and to type field
    /// access (`obj.field`). Shared across the module via `Rc`.
    structs: Rc<HashMap<String, Vec<(String, Type)>>>,
    /// PMAT-506d (classes epic): per-struct method return types — `struct name
    /// → [(method_name, return_type)]`. Built in the same pre-pass as `structs`.
    /// Consulted to type `obj.method(args)` (`Expr::MethodCall`).
    struct_methods: Rc<HashMap<String, Vec<(String, Type)>>>,
    /// PMAT-506f (classes epic): per-struct field defaults — `struct name →
    /// [(field, lowered default Expr)]`, only for fields with a default
    /// (`x: int = 30`). Built in the same pre-pass; consulted at construction to
    /// fill omitted fields.
    struct_field_defaults: Rc<HashMap<String, Vec<(String, Expr)>>>,
    /// PMAT-506j (classes epic): per-struct `@property` names — `struct name →
    /// [property_name]`. Built in the same pre-pass as `structs`. A bare
    /// attribute read `obj.prop` whose name is a registered property lowers to a
    /// no-arg method call `(obj).prop()` (`Expr::MethodCall`) rather than a
    /// field access. The property's return type lives in `struct_methods`.
    struct_properties: Rc<HashMap<String, Vec<String>>>,
    /// PMAT-513 (Tranche 2): module-level enum table — `enum name →
    /// [(variant, discriminant)]`, built in the pre-pass. Consulted to lower a
    /// member access `C.NAME` → [`Expr::EnumVariant`] and `C.NAME.value` → the
    /// discriminant literal.
    enums: Rc<HashMap<String, Vec<(String, i64)>>>,
    /// PMAT-504: function-local closure bindings — maps a closure
    /// variable name (`f` in `f = lambda y: …`) to its inferred return
    /// type, so a call `f(x)` types correctly (the module signature
    /// table only covers top-level functions). Populated as
    /// [`Stmt::ClosureLet`] bindings are lowered.
    closure_returns: HashMap<String, Type>,
    /// PMAT-502dz: monotone counter minting fresh, unique Rust names for
    /// `_` loop/comprehension targets. Rust forbids `_` as a readable
    /// `let mut` binding, so `for _ in range(n)` / `[… for _ in range(n)]`
    /// can't emit `let mut _: i64`. Each `_` target claims `__xpile_idx{N}`.
    /// Nested `for _` need distinct names (the outer's tail increment would
    /// otherwise hit the inner shadow), hence a counter rather than a
    /// constant.
    underscore_counter: usize,
    /// PMAT-502dz: the fresh name the innermost enclosing `for _`/`… for _ …`
    /// minted for its `_` target, so a body read of `_` (legal Python — `_`
    /// is an ordinary, if conventionally-unused, binding) lowers to that
    /// same name instead of an uncompilable bare `_`. Saved/restored around
    /// each construct's body so `_` shadows correctly across nesting.
    underscore_rename: Option<String>,
    /// PMAT-502ez (Optional epic cut 4): names a preceding provably-exiting
    /// `if <name> is None: return …` guard has proven `Some`. A later read of
    /// such a name lowers to `Expr::OptionUnwrap` (`(<name>).unwrap()` : `T`)
    /// rather than the raw `Option<T>` ident. Populated as the function body's
    /// leading statements are lowered in order (see `lower_function_def`); only
    /// non-reassigned (`!mutable`) `Optional`-typed names are eligible, so the
    /// unwrap is sound (the name cannot become `None` after the guard).
    narrowed_some: HashSet<String>,
    /// PMAT-506h (classes epic): when lowering a `@classmethod` body, the name of
    /// the enclosing class. A `cls(...)` construction or `cls.method(...)` call in
    /// the body resolves `cls` to this class name (so it reuses the existing
    /// struct-construction / static-call dispatch). `None` everywhere else.
    cls_name: Option<String>,
}

impl LoweringCtx {
    /// PMAT-506h: resolve a receiver/callee name, mapping the classmethod
    /// pseudo-receiver `cls` to the enclosing class name when set. Any other
    /// name passes through unchanged.
    fn resolve_class_name<'a>(&'a self, name: &'a str) -> &'a str {
        if name == "cls" {
            self.cls_name.as_deref().unwrap_or(name)
        } else {
            name
        }
    }
}

impl LoweringCtx {
    /// PMAT-502dz: claim a Rust counter name for a `for`/comprehension
    /// target. A named target passes through unchanged (and leaves any
    /// enclosing `_`-rename in force, so a body read of an *outer* `_`
    /// still resolves). A `_` target mints a fresh `__xpile_idx{N}` and
    /// installs it as the active `_`-rename, returning the previous rename
    /// to restore via [`exit_loop_var`] once the body is lowered.
    fn enter_loop_var(&mut self, py_name: &str) -> (String, Option<String>) {
        if py_name == "_" {
            let fresh = format!("__xpile_idx{}", self.underscore_counter);
            self.underscore_counter += 1;
            let saved = self.underscore_rename.clone();
            self.underscore_rename = Some(fresh.clone());
            (fresh, saved)
        } else {
            (py_name.to_string(), self.underscore_rename.clone())
        }
    }

    /// PMAT-502dz: restore the `_`-rename saved by [`enter_loop_var`].
    fn exit_loop_var(&mut self, saved: Option<String>) {
        self.underscore_rename = saved;
    }
}

impl LoweringCtx {
    #[allow(clippy::too_many_arguments)]
    fn new(
        fn_name: &str,
        fn_return_type: Type,
        params: &[Param],
        body: &[ast::Stmt],
        signatures: Rc<HashMap<String, FnSig>>,
        consts: &HashMap<String, Type>,
        structs: Rc<HashMap<String, Vec<(String, Type)>>>,
        struct_methods: Rc<HashMap<String, Vec<(String, Type)>>>,
        struct_field_defaults: Rc<HashMap<String, Vec<(String, Expr)>>>,
        struct_properties: Rc<HashMap<String, Vec<String>>>,
        enums: Rc<HashMap<String, Vec<(String, i64)>>>,
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
        let read_counts = count_name_reads(body);
        Self {
            fn_name: fn_name.to_string(),
            fn_return_type,
            bound,
            name_types,
            mutable,
            read_counts,
            signatures,
            structs,
            struct_methods,
            struct_field_defaults,
            struct_properties,
            enums,
            closure_returns: HashMap::new(),
            underscore_counter: 0,
            underscore_rename: None,
            narrowed_some: HashSet::new(),
            cls_name: None,
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

/// PMAT-588 (ownership cluster): count how many times each name is *read*
/// (`Name`-load occurrences) across the function body. Used to decide whether a
/// non-Copy variable passed by value to a call must be cloned: if it is read
/// more than once, moving it into the call would make the other use a
/// use-after-move (rustc E0382). Conservative — exotic expression shapes
/// (comprehensions, lambdas, …) simply aren't recursed, so a name is at most
/// under-counted, which only ever skips a clone (never inserts a spurious one,
/// so single-use code is byte-identical). Modeled on `count_pop_receivers`.
fn count_name_reads(body: &[ast::Stmt]) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for s in body {
        count_reads_stmt(s, &mut counts);
    }
    counts
}

fn count_reads_stmt(s: &ast::Stmt, counts: &mut HashMap<String, usize>) {
    use ast::Stmt as S;
    match s {
        // Assignment targets are *stores*, not reads — only the value is read.
        S::Assign(a) => count_reads_expr(&a.value, counts),
        S::AnnAssign(aa) => {
            if let Some(v) = &aa.value {
                count_reads_expr(v, counts);
            }
        }
        // `x <op>= e` reads `x` (and `e`) before writing back.
        S::AugAssign(a) => {
            count_reads_expr(&a.target, counts);
            count_reads_expr(&a.value, counts);
        }
        S::Return(r) => {
            if let Some(v) = &r.value {
                count_reads_expr(v, counts);
            }
        }
        S::Expr(e) => count_reads_expr(&e.value, counts),
        S::If(i) => {
            count_reads_expr(&i.test, counts);
            for st in &i.body {
                count_reads_stmt(st, counts);
            }
            for st in &i.orelse {
                count_reads_stmt(st, counts);
            }
        }
        S::While(w) => {
            count_reads_expr(&w.test, counts);
            for st in &w.body {
                count_reads_stmt(st, counts);
            }
        }
        S::For(f) => {
            count_reads_expr(&f.iter, counts);
            for st in &f.body {
                count_reads_stmt(st, counts);
            }
        }
        S::Assert(a) => {
            count_reads_expr(&a.test, counts);
            if let Some(m) = &a.msg {
                count_reads_expr(m, counts);
            }
        }
        S::Delete(d) => {
            for t in &d.targets {
                count_reads_expr(t, counts);
            }
        }
        _ => {}
    }
}

fn count_reads_expr(e: &ast::Expr, counts: &mut HashMap<String, usize>) {
    use ast::Expr as E;
    match e {
        E::Name(n) => {
            if matches!(n.ctx, ast::ExprContext::Load) {
                *counts.entry(n.id.to_string()).or_insert(0) += 1;
            }
        }
        E::Call(c) => {
            count_reads_expr(&c.func, counts);
            for a in &c.args {
                count_reads_expr(a, counts);
            }
            for kw in &c.keywords {
                count_reads_expr(&kw.value, counts);
            }
        }
        E::BinOp(b) => {
            count_reads_expr(&b.left, counts);
            count_reads_expr(&b.right, counts);
        }
        E::UnaryOp(u) => count_reads_expr(&u.operand, counts),
        E::BoolOp(b) => {
            for v in &b.values {
                count_reads_expr(v, counts);
            }
        }
        E::Compare(c) => {
            count_reads_expr(&c.left, counts);
            for c2 in &c.comparators {
                count_reads_expr(c2, counts);
            }
        }
        E::Subscript(s) => {
            count_reads_expr(&s.value, counts);
            count_reads_expr(&s.slice, counts);
        }
        E::Attribute(a) => count_reads_expr(&a.value, counts),
        E::IfExp(i) => {
            count_reads_expr(&i.test, counts);
            count_reads_expr(&i.body, counts);
            count_reads_expr(&i.orelse, counts);
        }
        E::List(l) => {
            for el in &l.elts {
                count_reads_expr(el, counts);
            }
        }
        E::Tuple(t) => {
            for el in &t.elts {
                count_reads_expr(el, counts);
            }
        }
        E::Set(s) => {
            for el in &s.elts {
                count_reads_expr(el, counts);
            }
        }
        E::Dict(d) => {
            for k in d.keys.iter().flatten() {
                count_reads_expr(k, counts);
            }
            for v in &d.values {
                count_reads_expr(v, counts);
            }
        }
        E::Starred(s) => count_reads_expr(&s.value, counts),
        E::Slice(s) => {
            if let Some(l) = &s.lower {
                count_reads_expr(l, counts);
            }
            if let Some(u) = &s.upper {
                count_reads_expr(u, counts);
            }
            if let Some(st) = &s.step {
                count_reads_expr(st, counts);
            }
        }
        _ => {}
    }
}

/// PMAT-588 (ownership cluster): clone a non-Copy variable passed *by value* to
/// a function call when it is read more than once in the body. Without this, the
/// move into the call leaves any other use a use-after-move (rustc E0382 — e.g.
/// `helper(xs) + helper(xs)`, or `helper(xs)` followed by `len(xs)`). The clone
/// keeps the caller's binding alive. Gated on `read_count > 1`, so a single-use
/// call argument (the entire existing corpus) is byte-identical — no clone, no
/// churn, no perf cost — and the clone fires only on code that would otherwise
/// fail to compile. Copy operands (int/float/bool) are passed by value as before.
/// PMAT-588/628: if `expr` is a bare `Ident` for a non-Copy variable that is
/// read more than once in the function, wrap it in `Expr::Clone` so passing /
/// storing it doesn't move-then-use-after-move (E0382). A no-op for Copy types
/// (int/float/bool) and single-use bindings, so it never adds a clone to
/// previously-correct code.
fn clone_if_reused_non_copy(ctx: &LoweringCtx, expr: Expr) -> Expr {
    if let Expr::Ident(name) = &expr {
        let reused = ctx.read_counts.get(name).copied().unwrap_or(0) > 1;
        let non_copy = ctx
            .name_types
            .get(name)
            .is_some_and(|t| !matches!(t, Type::I64 | Type::F64 | Type::Bool));
        if reused && non_copy {
            return Expr::Clone(Box::new(expr));
        }
    }
    expr
}

fn clone_reused_call_args(ctx: &LoweringCtx, expr: Expr) -> Expr {
    let Expr::Call { callee, args } = expr else {
        return expr;
    };
    let args = args
        .into_iter()
        .map(|a| clone_if_reused_non_copy(ctx, a))
        .collect();
    Expr::Call { callee, args }
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
                } else if a.targets.len() == 1 && matches!(&a.targets[0], ast::Expr::Tuple(_)) {
                    // PMAT-547: tuple-unpack `a, b = …` binds every Name target —
                    // count each so a later reassignment/augment lifts it to
                    // `let (mut a, …)` (mirrors the chained-assign arm above).
                    if let ast::Expr::Tuple(t) = &a.targets[0] {
                        for e in &t.elts {
                            if let ast::Expr::Name(n) = e {
                                *counts.entry(n.id.to_string()).or_insert(0) += bump;
                            } else if let Some(base) = subscript_chain_base_name(e) {
                                // PMAT-559: `xs[i], xs[j] = …` (subscript-target
                                // swap / parallel assign) mutates the base
                                // collection in place → count it so it binds `mut`.
                                *counts.entry(base).or_insert(0) += bump;
                            }
                        }
                    }
                } else if a.targets.len() == 1 {
                    // PMAT-506c: `obj.field = v` mutates the struct binding in
                    // place → mark `obj` mutable.
                    if let ast::Expr::Attribute(attr) = &a.targets[0] {
                        if let ast::Expr::Name(n) = attr.value.as_ref() {
                            *counts.entry(n.id.to_string()).or_insert(0) += bump;
                        }
                    }
                }
            }
            // PMAT-470 (R1): `x <op>= e` is a read-modify-write
            // reassignment → mutates `x`, so count it like an Assign.
            // PMAT-502ea: a subscript target (`xs[i] += v`, `grid[i][j] += v`)
            // mutates the base collection in place — count the base name too,
            // at any depth, or a literal-initialised receiver is never `mut`.
            ast::Stmt::AugAssign(a) => {
                let bump = if in_loop { 2 } else { 1 };
                if let ast::Expr::Name(n) = a.target.as_ref() {
                    *counts.entry(n.id.to_string()).or_insert(0) += bump;
                } else if let Some(name) = subscript_chain_base_name(a.target.as_ref()) {
                    *counts.entry(name).or_insert(0) += bump;
                } else if let ast::Expr::Attribute(attr) = a.target.as_ref() {
                    // PMAT-506i: `obj.field <op>= v` mutates `obj` in place →
                    // mark `obj` mutable (mirrors the `obj.field = v` Assign arm).
                    if let ast::Expr::Name(n) = attr.value.as_ref() {
                        *counts.entry(n.id.to_string()).or_insert(0) += bump;
                    }
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
                        // PMAT-533: `base[i].append(e)` mutates `base` in place
                        // (the receiver is a subscript of a Name), so the base
                        // binding must be `mut`.
                        if attr.attr.as_str() == "append" {
                            if let ast::Expr::Subscript(sub) = attr.value.as_ref() {
                                if let ast::Expr::Name(base) = sub.value.as_ref() {
                                    let bump = if in_loop { 2 } else { 1 };
                                    *counts.entry(base.id.to_string()).or_insert(0) += bump;
                                }
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
            // PMAT-510: a `match` desugars to an if/elif/else chain — only one
            // case runs, so merge the case-body counts by max (like if-branches)
            // so a name assigned inside the cases is marked `mut` when needed.
            ast::Stmt::Match(m) => {
                let mut merged: HashMap<String, usize> = HashMap::new();
                for case in &m.cases {
                    merged = merge_branch_counts(merged, walk_counts(&case.body, in_loop));
                }
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
            // PMAT-503c: count assignments inside `try`/`except` arms so an
            // assignment-form try that reassigns an already-bound name marks it
            // `mut`. The body + each handler are alternatives (only one runs),
            // so merge them by max — which marks `mut` exactly when needed and
            // never spuriously (a name assigned only in the try arms still
            // counts once, so a fresh `let v = <try>` stays non-`mut`).
            ast::Stmt::Try(try_stmt) => {
                let mut merged = walk_counts(&try_stmt.body, in_loop);
                for handler in &try_stmt.handlers {
                    let ast::ExceptHandler::ExceptHandler(eh) = handler;
                    merged = merge_branch_counts(merged, walk_counts(&eh.body, in_loop));
                }
                let else_counts = walk_counts(&try_stmt.orelse, in_loop);
                let finally_counts = walk_counts(&try_stmt.finalbody, in_loop);
                for (name, c) in merged.into_iter().chain(else_counts).chain(finally_counts) {
                    *counts.entry(name).or_insert(0) += c;
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
        // PMAT-502eh: a bare `d.setdefault(k, v)` expression-statement also
        // mutates its receiver (it gets-or-inserts), so the receiver must be
        // `mut`. Scan the statement's value expression too.
        ast::Stmt::Expr(e) => count_pop_receivers(&e.value, counts, bump),
        // PMAT-574: an expression-position mutator (`.pop(...)` / `.setdefault(...)`)
        // in a *controlling condition* — `while xs.pop() >= 0:`, `if d.setdefault(k, v):`,
        // `assert xs.pop()` — also mutates its receiver, so the receiver must be `mut`.
        // The statement BODIES are walked by `walk_counts`'s recursion, but the
        // controlling expression itself was previously unscanned → the receiver
        // stayed immutable → rustc E0596. A `while` test runs every iteration, so
        // it carries loop bump semantics (`>= 2`) regardless of the enclosing level.
        ast::Stmt::While(w) => count_pop_receivers(&w.test, counts, bump.max(2)),
        ast::Stmt::If(s) => count_pop_receivers(&s.test, counts, bump),
        ast::Stmt::For(f) => count_pop_receivers(&f.iter, counts, bump),
        ast::Stmt::Assert(a) => {
            count_pop_receivers(&a.test, counts, bump);
            if let Some(m) = &a.msg {
                count_pop_receivers(m, counts, bump);
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
/// PMAT-502ea: peel a (possibly nested) subscript expression
/// `base[i]…[k]` to its base Name. Used by the mutability pre-walk so a
/// subscript assignment / augmented assignment marks the base collection
/// `let mut` — at any nesting depth (`xs[i] = v`, `grid[i][j] += v`).
/// Returns None unless the expression is a subscript bottoming at a Name.
fn subscript_chain_base_name(expr: &ast::Expr) -> Option<String> {
    let ast::Expr::Subscript(sub) = expr else {
        return None;
    };
    let mut cur = sub.value.as_ref();
    loop {
        match cur {
            ast::Expr::Name(n) => return Some(n.id.to_string()),
            ast::Expr::Subscript(inner) => cur = inner.value.as_ref(),
            _ => return None,
        }
    }
}

fn subscript_assign_base_name(a: &ast::StmtAssign) -> Option<String> {
    if a.targets.len() != 1 {
        return None;
    }
    // PMAT-502ea: peel nested chains too (`grid[i][j] = v` → `grid`), not
    // just single-level `xs[i] = v` — otherwise a literal-initialised nested
    // grid is never marked `let mut`.
    subscript_chain_base_name(&a.targets[0])
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
                // PMAT-502ct: capture each param's default value (ruff bundles
                // it per-arg in `ArgWithDefault.default`).
                let defaults = f
                    .args
                    .args
                    .iter()
                    .map(|a| a.default.as_deref().cloned())
                    .collect();
                // PMAT-502dq: record a `*args` vararg (name + element type from
                // its annotation, defaulting to `int`).
                let variadic = f.args.vararg.as_ref().map(|v| {
                    let elem = v
                        .annotation
                        .as_ref()
                        .and_then(|ann| {
                            parse_type_annotation(f.name.as_str(), v.arg.as_str(), ann).ok()
                        })
                        .unwrap_or(Type::I64);
                    (v.arg.to_string(), elem)
                });
                sig_map.insert(
                    f.name.to_string(),
                    FnSig {
                        ret,
                        params,
                        defaults,
                        variadic,
                    },
                );
            }
        }
        // PMAT-506g/506h (classes epic): register each class `@staticmethod` /
        // `@classmethod` under a qualified key `Class::method`, so a
        // `Class.method(args)` call reuses `Expr::Call` (callee = `Class::method`)
        // and types via the signature table — no instance receiver, no new IR.
        // Presence of the qualified key is itself the "this is a static/class
        // method" signal at the call site. A classmethod's `cls` first parameter
        // is implicit at the call site, so it is excluded from the recorded
        // parameter list (the caller supplies only the explicit args).
        for stmt in &suite {
            if let ast::Stmt::ClassDef(c) = stmt {
                for member in &c.body {
                    let ast::Stmt::FunctionDef(m) = member else {
                        continue;
                    };
                    let is_cm = is_classmethod(m);
                    if !is_staticmethod(m) && !is_cm {
                        continue;
                    }
                    let ret = match m.returns.as_ref() {
                        None => Type::Unit,
                        Some(ann) => parse_type_annotation(c.name.as_str(), "return", ann)
                            .unwrap_or(Type::I64),
                    };
                    // Skip the implicit `cls` receiver for a classmethod.
                    let args_iter = m.args.args.iter().skip(usize::from(is_cm));
                    let params = args_iter.clone().map(|a| a.def.arg.to_string()).collect();
                    let defaults = args_iter.map(|a| a.default.as_deref().cloned()).collect();
                    sig_map.insert(
                        format!("{}::{}", c.name, m.name),
                        FnSig {
                            ret,
                            params,
                            defaults,
                            variadic: None,
                        },
                    );
                }
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

        // PMAT-506b (classes epic): pre-pass — collect every class/dataclass's
        // ordered fields into the struct registry, so construction + field
        // access in function bodies (which may precede the class textually)
        // type correctly.
        let mut struct_map: HashMap<String, Vec<(String, Type)>> = HashMap::new();
        let mut struct_method_map: HashMap<String, Vec<(String, Type)>> = HashMap::new();
        let mut struct_default_map: HashMap<String, Vec<(String, Expr)>> = HashMap::new();
        // PMAT-506j: per-struct `@property` names, so a bare `obj.prop` read
        // lowers to a no-arg method call. (The property's return type is in
        // `struct_method_map` — a property is a `self` method.)
        let mut struct_property_map: HashMap<String, Vec<String>> = HashMap::new();
        // PMAT-513: per-enum `(variant, discriminant)` list, so member access
        // `C.NAME` / `C.NAME.value` in function bodies type/lower correctly.
        let mut enum_map: HashMap<String, Vec<(String, i64)>> = HashMap::new();
        for stmt in &suite {
            if let ast::Stmt::ClassDef(c) = stmt {
                // PMAT-513: an `Enum` class goes in the enum registry, NOT the
                // struct registry (it has variants, not fields).
                if is_enum_class(c) {
                    if let Ok(variants) = enum_variants(c) {
                        enum_map.insert(c.name.to_string(), variants);
                    }
                    continue;
                }
                if let Ok((name, fields, method_returns, field_defaults)) = class_def_signature(c) {
                    let props: Vec<String> = c
                        .body
                        .iter()
                        .filter_map(|m| match m {
                            ast::Stmt::FunctionDef(f) if is_property(f) => Some(f.name.to_string()),
                            _ => None,
                        })
                        .collect();
                    struct_map.insert(name.clone(), fields);
                    struct_method_map.insert(name.clone(), method_returns);
                    struct_default_map.insert(name.clone(), field_defaults);
                    struct_property_map.insert(name, props);
                }
            }
        }
        let structs = Rc::new(struct_map);
        let struct_methods = Rc::new(struct_method_map);
        let struct_field_defaults = Rc::new(struct_default_map);
        let struct_properties = Rc::new(struct_property_map);
        let enums = Rc::new(enum_map);

        let mut items = Vec::new();
        for stmt in suite {
            // PMAT-036: `from __future__ import annotations` is the
            // canonical Python preamble that defers annotation
            // evaluation. xpile fixtures with `-> BigInt` (PMAT-013
            // implicit-promotion) need this so CPython can `exec` the
            // file without `NameError: BigInt`. The frontend skips it
            // (no Meta-HIR representation needed — annotations are
            // already treated as Type tokens at lower time).
            if is_future_annotations_import(&stmt) || is_skippable_import(&stmt) {
                continue;
            }
            let item = lower_top_level_stmt(
                stmt,
                signatures.clone(),
                consts.clone(),
                structs.clone(),
                struct_methods.clone(),
                struct_field_defaults.clone(),
                struct_properties.clone(),
                enums.clone(),
            )?;
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

/// PMAT-502ek/ew: a plain `import <module>` (e.g. `import math`) or a
/// `from <module> import <names>` (e.g. `from typing import Optional`). An
/// import has no runtime effect we model — it just enables a namespace / names
/// — so we skip it; whether a given module's *uses* are supported is decided at
/// the call/attribute/annotation site (`math.sqrt(...)` and `Optional[int]` are
/// recognised; an unsupported `os.getcwd()` errors there with a clear message).
/// (`from __future__ import annotations` is handled separately upstream, but a
/// blanket `ImportFrom` skip subsumes it harmlessly.)
fn is_skippable_import(stmt: &ast::Stmt) -> bool {
    matches!(stmt, ast::Stmt::Import(_) | ast::Stmt::ImportFrom(_))
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

#[allow(clippy::too_many_arguments)]
fn lower_top_level_stmt(
    stmt: ast::Stmt,
    signatures: Rc<HashMap<String, FnSig>>,
    consts: Rc<HashMap<String, Type>>,
    structs: Rc<HashMap<String, Vec<(String, Type)>>>,
    struct_methods: Rc<HashMap<String, Vec<(String, Type)>>>,
    struct_field_defaults: Rc<HashMap<String, Vec<(String, Expr)>>>,
    struct_properties: Rc<HashMap<String, Vec<String>>>,
    enums: Rc<HashMap<String, Vec<(String, i64)>>>,
) -> Result<Item, FrontendError> {
    // PMAT-502bj: a module-level `NAME = <int/bool/float-literal>` is a
    // constant item (recognised before the `def`-only fallback).
    if let Some((name, ty, value)) = try_const_decl(&stmt) {
        return Ok(Item::Const { name, ty, value });
    }
    match stmt {
        ast::Stmt::FunctionDef(f) => lower_function_def(
            f,
            signatures,
            consts,
            structs,
            struct_methods,
            struct_field_defaults,
            struct_properties,
            enums,
            None,
            None,
        )
        .map(Item::Function),
        // PMAT-513: a `class C(Enum):` → an `Item::Enum` (handled before the
        // struct path, which rejects base classes).
        ast::Stmt::ClassDef(c) if is_enum_class(&c) => lower_enum_def(&c),
        // PMAT-505a/506d (classes epic): a field-only / `@dataclass` class → an
        // `Item::Struct` (fields + instance methods).
        ast::Stmt::ClassDef(c) => lower_class_def(
            c,
            signatures,
            consts,
            structs,
            struct_methods,
            struct_field_defaults,
            struct_properties,
            enums,
        ),
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

/// PMAT-587: a user `class`/`@dataclass`/enum whose name is a Rust *prelude
/// type that xpile actually emits* — `Vec` (lists), `String` (str), `HashMap`
/// (dict), `HashSet` (set), `Option`/`Some`/`None` (optionals) — emits a
/// `struct <Name>` that collides with the prelude. A bare unit struct merely
/// shadows it, but once the module also uses the generic form (e.g. a
/// `list[int]` → `Vec<i64>`), rustc rejects it (E0107: "struct takes 0 generic
/// arguments but 1 was supplied") — a transpile-success → invalid-Rust invariant
/// break. Reject with a clear message instead. (Auto-escaping the type name,
/// like the `r#` keyword escape, is a possible follow-up.) The set is limited to
/// what xpile emits, so prelude names it does NOT generate (`Result`/`Box`/…)
/// still work by shadowing.
fn rust_prelude_type_collision(name: &str) -> Option<String> {
    let collides = matches!(
        name,
        "Vec" | "String" | "Option" | "Some" | "None" | "HashMap" | "HashSet"
    );
    collides.then(|| {
        format!(
            "class/enum `{name}` collides with a Rust prelude type that xpile emits — \
             rename it (e.g. `{name}_`) so the generated code compiles"
        )
    })
}

/// PMAT-505a (classes epic, first cut): lower a Python class into an
/// `Item::Struct`. Supported shape: a class whose body is only annotated fields
/// (`x: int`) and/or `pass`, optionally decorated `@dataclass`. Each `x: T`
/// becomes a `(name, Type)` pair in declaration order. Methods, base classes,
/// class-vars with values (defaults), and non-field statements are rejected with
/// a clear error (follow-up sub-slices). This first cut emits the struct
/// *definition* only — construction/field-access are deferred.
/// PMAT-505a/506d: lightweight class signature — the field `(name, Type)` list
/// and each method's `(name, return_type)` — WITHOUT lowering method bodies.
/// Used in the module pre-pass to build the `structs` + `struct_methods`
/// registries before any function/method body is lowered (so construction,
/// field access, and method calls type correctly regardless of textual order).
#[allow(clippy::type_complexity)]
fn class_def_signature(
    c: &ast::StmtClassDef,
) -> Result<
    (
        String,
        Vec<(String, Type)>,
        Vec<(String, Type)>,
        Vec<(String, Expr)>,
    ),
    FrontendError,
> {
    let name = c.name.to_string();
    if let Some(msg) = rust_prelude_type_collision(&name) {
        return Err(FrontendError::Lower(msg));
    }
    if !c.bases.is_empty() || !c.keywords.is_empty() {
        return Err(FrontendError::Lower(format!(
            "class `{name}` has base classes / keyword bases — v0.2.0 first cut supports only a field-only / `@dataclass` class (no inheritance)"
        )));
    }
    let mut fields: Vec<(String, Type)> = Vec::new();
    let mut method_returns: Vec<(String, Type)> = Vec::new();
    let mut field_defaults: Vec<(String, Expr)> = Vec::new();
    for stmt in &c.body {
        match stmt {
            // `x: T` (field) — optionally with a default `x: T = <literal>`.
            ast::Stmt::AnnAssign(aa) => {
                let ast::Expr::Name(field) = aa.target.as_ref() else {
                    return Err(FrontendError::Lower(format!(
                        "class `{name}` has a non-Name annotated field target — v0.2.0 first cut supports plain `field: Type` members"
                    )));
                };
                let ty = parse_type_annotation(&name, field.id.as_str(), &aa.annotation)?;
                // PMAT-506f: a field default `x: T = <expr>`. First cut: the
                // default must be a literal (lowered context-free) — `field(...)`
                // / computed defaults are rejected.
                if let Some(default_ast) = aa.value.as_ref() {
                    if !is_literal_default(default_ast) {
                        return Err(FrontendError::Lower(format!(
                            "class `{name}` field `{}` has a non-literal default — v0.2.0 first cut supports only literal field defaults (int/float/str/bool, optionally negated)",
                            field.id
                        )));
                    }
                    let default = lower_expr((**default_ast).clone())?;
                    field_defaults.push((field.id.to_string(), default));
                }
                fields.push((field.id.to_string(), ty));
            }
            // PMAT-506d: an instance method `def m(self, …) -> R: …`. Record its
            // return type for call typing; the body is lowered later.
            // PMAT-506g/506h: a `@staticmethod` / `@classmethod` is NOT an
            // instance method — it is registered separately under a qualified
            // `Class::method` signature key (see the module pre-pass), so skip it
            // here.
            ast::Stmt::FunctionDef(m) => {
                if is_staticmethod(m) || is_classmethod(m) {
                    continue;
                }
                let ret = match m.returns.as_ref() {
                    None => Type::Unit,
                    Some(ann) => parse_type_annotation(&name, "<method return>", ann)?,
                };
                method_returns.push((m.name.to_string(), ret));
            }
            // A bare docstring (string-literal expression statement) or `pass`.
            ast::Stmt::Pass(_) => {}
            ast::Stmt::Expr(e) if matches!(e.value.as_ref(), ast::Expr::Constant(_)) => {}
            _ => {
                return Err(FrontendError::Lower(format!(
                    "class `{name}` has an unsupported member (nested statement) — v0.2.0 first cut supports annotated fields `field: Type [= literal]` and instance methods `def m(self, …)`"
                )));
            }
        }
    }
    Ok((name, fields, method_returns, field_defaults))
}

/// PMAT-506g (classes epic): true if a class method carries a bare
/// `@staticmethod` decorator. Such a method has no `self` receiver and is
/// called as `Class.method(args)` → `Class::method(args)` (an associated
/// function), rather than `obj.method(args)`.
fn is_staticmethod(m: &ast::StmtFunctionDef) -> bool {
    m.decorator_list
        .iter()
        .any(|d| matches!(d, ast::Expr::Name(n) if n.id.as_str() == "staticmethod"))
}

/// PMAT-506h (classes epic): true if a class method carries a bare
/// `@classmethod` decorator. Such a method has a `cls` first parameter (the
/// class itself) instead of `self`; it is called as `Class.method(args)` →
/// `Class::method(args)` (an associated function), and any `cls(...)` /
/// `cls.method(...)` in its body resolves to the enclosing class.
fn is_classmethod(m: &ast::StmtFunctionDef) -> bool {
    m.decorator_list
        .iter()
        .any(|d| matches!(d, ast::Expr::Name(n) if n.id.as_str() == "classmethod"))
}

/// PMAT-506j (classes epic): true if a class method carries a bare `@property`
/// decorator. A property is a read-only `self` method accessed as a bare
/// attribute (`obj.area`, no parens) — it lowers to `(obj).area()` (an
/// `Expr::MethodCall` with no args). Setters (`@area.setter`) are not supported.
fn is_property(m: &ast::StmtFunctionDef) -> bool {
    m.decorator_list
        .iter()
        .any(|d| matches!(d, ast::Expr::Name(n) if n.id.as_str() == "property"))
}

/// PMAT-592 (classes epic): true if a class carries `@dataclass(frozen=True)`.
/// A frozen dataclass is hashable in Python (it may be a dict key / set
/// element), so the backend derives `Eq, Hash` for it (when all field types
/// are Eq+Hash-capable). The decorator parses as a `Call` to `dataclass`
/// with a `frozen=True` keyword; bare `@dataclass` / `frozen=False` are not
/// frozen.
fn class_is_frozen(c: &ast::StmtClassDef) -> bool {
    c.decorator_list.iter().any(|d| {
        let ast::Expr::Call(call) = d else { return false };
        if !matches!(call.func.as_ref(), ast::Expr::Name(n) if n.id.as_str() == "dataclass") {
            return false;
        }
        call.keywords.iter().any(|kw| {
            kw.arg.as_deref() == Some("frozen")
                && matches!(&kw.value, ast::Expr::Constant(c) if matches!(c.value, ast::Constant::Bool(true)))
        })
    })
}

/// PMAT-513 (Tranche 2): true if `c` is an `Enum` class — exactly one base, the
/// bare name `Enum`, and no keyword bases. (`IntEnum`/`StrEnum`/`Flag` are not
/// recognised at the first cut.)
fn is_enum_class(c: &ast::StmtClassDef) -> bool {
    c.keywords.is_empty()
        && c.bases.len() == 1
        && matches!(&c.bases[0], ast::Expr::Name(n) if n.id.as_str() == "Enum")
}

/// PMAT-513: collect an `Enum` class's `NAME = <int literal>` members into
/// `(name, discriminant)` pairs (declaration order). Members must be plain
/// integer-literal assignments (optionally negated); `pass`/docstrings are
/// allowed. Auto-numbering (`auto()`), methods, and non-int values are rejected.
fn enum_variants(c: &ast::StmtClassDef) -> Result<Vec<(String, i64)>, FrontendError> {
    let name = c.name.to_string();
    let mut variants: Vec<(String, i64)> = Vec::new();
    for stmt in &c.body {
        match stmt {
            ast::Stmt::Assign(a) if a.targets.len() == 1 => {
                let ast::Expr::Name(member) = &a.targets[0] else {
                    return Err(FrontendError::Lower(format!(
                        "enum `{name}` has a non-Name member target — v0.2.0 supports `NAME = <int literal>` members"
                    )));
                };
                let disc = int_literal_value(a.value.as_ref()).ok_or_else(|| {
                    FrontendError::Lower(format!(
                        "enum `{name}` member `{}` is not an integer literal — v0.2.0 supports only `NAME = <int>` (no `auto()`/computed/str values)",
                        member.id
                    ))
                })?;
                variants.push((member.id.to_string(), disc));
            }
            ast::Stmt::Pass(_) => {}
            ast::Stmt::Expr(e) if matches!(e.value.as_ref(), ast::Expr::Constant(_)) => {}
            _ => {
                return Err(FrontendError::Lower(format!(
                    "enum `{name}` has an unsupported member — v0.2.0 supports only `NAME = <int literal>` members (no methods/`auto()`)"
                )));
            }
        }
    }
    if variants.is_empty() {
        return Err(FrontendError::Lower(format!(
            "enum `{name}` has no members — v0.2.0 requires at least one `NAME = <int literal>`"
        )));
    }
    Ok(variants)
}

/// PMAT-513: the value of an integer literal (`5`) or a negated one (`-1`).
fn int_literal_value(e: &ast::Expr) -> Option<i64> {
    match e {
        ast::Expr::Constant(c) => match &c.value {
            ast::Constant::Int(i) => i.to_string().parse::<i64>().ok(),
            _ => None,
        },
        ast::Expr::UnaryOp(u) => match u.op {
            ast::UnaryOp::USub => int_literal_value(u.operand.as_ref()).map(|v| -v),
            ast::UnaryOp::UAdd => int_literal_value(u.operand.as_ref()),
            _ => None,
        },
        _ => None,
    }
}

/// PMAT-506f: true if `e` is a literal usable as a field default — a constant
/// (`30`, `"x"`, `True`, `1.5`) or a negated numeric literal (`-1`). Computed
/// defaults / `field(...)` factories are rejected (the default is lowered
/// context-free, so it must not reference function-scope bindings).
fn is_literal_default(e: &ast::Expr) -> bool {
    match e {
        ast::Expr::Constant(_) => true,
        ast::Expr::UnaryOp(u) => {
            matches!(u.op, ast::UnaryOp::USub | ast::UnaryOp::UAdd)
                && matches!(u.operand.as_ref(), ast::Expr::Constant(_))
        }
        _ => false,
    }
}

/// PMAT-513 (Tranche 2): lower a Python `class C(Enum):` into an `Item::Enum`.
fn lower_enum_def(c: &ast::StmtClassDef) -> Result<Item, FrontendError> {
    let name = c.name.to_string();
    // PMAT-587: an enum named after a Rust prelude type would collide.
    if let Some(msg) = rust_prelude_type_collision(&name) {
        return Err(FrontendError::Lower(msg));
    }
    Ok(Item::Enum {
        name,
        variants: enum_variants(c)?,
    })
}

/// PMAT-505a/506d: lower a Python class into an `Item::Struct` — its fields plus
/// any instance methods (lowered as [`Function`]s with a `self` receiver typed
/// as the struct). Read-only methods only: a method that assigns to `self.field`
/// is rejected (a `&mut self` receiver would need caller-side mutability
/// inference — deferred). `@dataclass` construction is positional over the
/// fields; an explicit `__init__` is not supported yet.
#[allow(clippy::too_many_arguments)]
fn lower_class_def(
    c: ast::StmtClassDef,
    signatures: Rc<HashMap<String, FnSig>>,
    consts: Rc<HashMap<String, Type>>,
    structs: Rc<HashMap<String, Vec<(String, Type)>>>,
    struct_methods: Rc<HashMap<String, Vec<(String, Type)>>>,
    struct_field_defaults: Rc<HashMap<String, Vec<(String, Expr)>>>,
    struct_properties: Rc<HashMap<String, Vec<String>>>,
    enums: Rc<HashMap<String, Vec<(String, i64)>>>,
) -> Result<Item, FrontendError> {
    let (name, fields, _, _) = class_def_signature(&c)?;
    // PMAT-592: record `@dataclass(frozen=True)` before `c.body` is consumed.
    let frozen = class_is_frozen(&c);
    let self_ty = Type::Struct(name.clone());
    let mut methods: Vec<Function> = Vec::new();
    for stmt in c.body {
        if let ast::Stmt::FunctionDef(mut m) = stmt {
            // PMAT-506g: a `@staticmethod` lowers as a plain associated function
            // (no `self` receiver). Strip the decorator (lower_function_def
            // rejects decorators) and lower with `self_type = None`; the emitted
            // `Function` has no `self` param, so it renders as `pub fn m(args)`
            // inside the `impl` block. Call sites use `Class.method(args)` →
            // `Class::method(args)` (an `Expr::Call`, registered in the
            // pre-pass).
            if is_staticmethod(&m) {
                m.decorator_list.clear();
                let method = lower_function_def(
                    m,
                    signatures.clone(),
                    consts.clone(),
                    structs.clone(),
                    struct_methods.clone(),
                    struct_field_defaults.clone(),
                    struct_properties.clone(),
                    enums.clone(),
                    None,
                    None,
                )?;
                methods.push(method);
                continue;
            }
            // PMAT-506j: a `@property` is a read-only `self` method accessed as a
            // bare attribute (`obj.prop`). Strip the decorator and lower it as a
            // normal instance method; the bare-attribute read site turns
            // `obj.prop` into `(obj).prop()` (the property name is registered in
            // the pre-pass). Setters / self-mutation are rejected (read-only).
            if is_property(&m) {
                m.decorator_list.clear();
                let method = lower_function_def(
                    m,
                    signatures.clone(),
                    consts.clone(),
                    structs.clone(),
                    struct_methods.clone(),
                    struct_field_defaults.clone(),
                    struct_properties.clone(),
                    enums.clone(),
                    Some(self_ty.clone()),
                    None,
                )?;
                if body_assigns_self(&method.body.stmts) {
                    return Err(FrontendError::Lower(format!(
                        "class `{name}` `@property` `{}` assigns to `self` — properties are read-only at v0.2.0",
                        method.name
                    )));
                }
                methods.push(method);
                continue;
            }
            // PMAT-506h: a `@classmethod` lowers like a static method but its
            // `cls` first parameter is dropped (it carries no runtime value — it
            // is the class itself) and `cls(...)` / `cls.method(...)` in the body
            // resolve to the enclosing class via `ctx.cls_name`. Require the `cls`
            // receiver, remove it, then lower with `cls_name = Some(class)`.
            if is_classmethod(&m) {
                let first_is_cls = m
                    .args
                    .args
                    .first()
                    .is_some_and(|a| a.def.arg.as_str() == "cls");
                if !first_is_cls {
                    return Err(FrontendError::Lower(format!(
                        "class `{name}` `@classmethod` `{}` has no `cls` first parameter",
                        m.name
                    )));
                }
                m.args.args.remove(0);
                m.decorator_list.clear();
                let method = lower_function_def(
                    m,
                    signatures.clone(),
                    consts.clone(),
                    structs.clone(),
                    struct_methods.clone(),
                    struct_field_defaults.clone(),
                    struct_properties.clone(),
                    enums.clone(),
                    None,
                    Some(name.clone()),
                )?;
                methods.push(method);
                continue;
            }
            // The first param must be `self`.
            let first_is_self = m
                .args
                .args
                .first()
                .is_some_and(|a| a.def.arg.as_str() == "self");
            if !first_is_self {
                return Err(FrontendError::Lower(format!(
                    "class `{name}` method `{}` has no `self` first parameter — use `@staticmethod` (no receiver) or `@classmethod` (`cls` receiver)",
                    m.name
                )));
            }
            let method = lower_function_def(
                m,
                signatures.clone(),
                consts.clone(),
                structs.clone(),
                struct_methods.clone(),
                struct_field_defaults.clone(),
                struct_properties.clone(),
                enums.clone(),
                Some(self_ty.clone()),
                None,
            )?;
            // First cut: read-only methods. A `self.field = v` (FieldAssign on
            // `self`) would need a `&mut self` receiver + caller mutability —
            // deferred. Reject so we never emit code that fails to compile.
            if body_assigns_self(&method.body.stmts) {
                return Err(FrontendError::Lower(format!(
                    "class `{name}` method `{}` assigns to `self` (mutating method) — v0.2.0 first cut supports read-only `&self` methods only",
                    method.name
                )));
            }
            methods.push(method);
        }
    }
    Ok(Item::Struct {
        name,
        fields,
        methods,
        frozen,
    })
}

/// PMAT-506d: true if any statement assigns a field of `self` (`self.f = v`).
/// Used to reject self-mutating methods in the read-only first cut.
fn body_assigns_self(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::FieldAssign { obj, .. } => obj == "self",
        Stmt::If {
            then_body,
            else_body,
            ..
        } => body_assigns_self(then_body) || body_assigns_self(else_body),
        Stmt::While { body, .. }
        | Stmt::ForEach { body, .. }
        | Stmt::ForEachPair { body, .. }
        | Stmt::ForEachZip3 { body, .. } => body_assigns_self(body),
        _ => false,
    })
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

/// PMAT-502ez (Optional epic cut 4): if `stmt` is a provably-exiting None-guard
/// `if <name> is None: <body ending in return/raise>` (no `else`), register
/// `<name>` as flow-narrowed to `Some` for the rest of the function body — later
/// reads then lower to [`Expr::OptionUnwrap`]. Only non-reassigned (`!mutable`)
/// `Optional`-typed names are eligible, which keeps the emitted `.unwrap()`
/// sound: the guard exits on `None`, and the name can't be rebound afterwards,
/// so every later read is provably `Some`. Any shape outside this narrow,
/// obviously-sound pattern is simply not narrowed (no regression — the same
/// code transpiled before, it just rustc-errored if the value was used as `T`).
fn register_none_guard_narrowing(ctx: &mut LoweringCtx, stmt: &ast::Stmt) {
    let ast::Stmt::If(if_stmt) = stmt else {
        return;
    };
    // An `else`/`elif` complicates the post-guard fact; defer those shapes.
    if !if_stmt.orelse.is_empty() {
        return;
    }
    // Condition must be exactly `<name> is None`.
    let ast::Expr::Compare(cmp) = if_stmt.test.as_ref() else {
        return;
    };
    if cmp.ops.len() != 1
        || !matches!(cmp.ops[0], ast::CmpOp::Is)
        || cmp.comparators.len() != 1
        || !matches!(&cmp.comparators[0], ast::Expr::Constant(k) if matches!(k.value, ast::Constant::None))
    {
        return;
    }
    let ast::Expr::Name(name) = cmp.left.as_ref() else {
        return;
    };
    let name = name.id.to_string();
    // The guard body must unconditionally exit (return / raise) so the
    // fall-through is reached only when the value is `Some`.
    if !matches!(
        if_stmt.body.last(),
        Some(ast::Stmt::Return(_) | ast::Stmt::Raise(_))
    ) {
        return;
    }
    // Eligible only for a non-reassigned `Optional`-typed name.
    if ctx.mutable.contains(&name) {
        return;
    }
    if matches!(ctx.name_types.get(&name), Some(Type::Optional(_))) {
        ctx.narrowed_some.insert(name);
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_function_def(
    f: ast::StmtFunctionDef,
    signatures: Rc<HashMap<String, FnSig>>,
    consts: Rc<HashMap<String, Type>>,
    structs: Rc<HashMap<String, Vec<(String, Type)>>>,
    struct_methods: Rc<HashMap<String, Vec<(String, Type)>>>,
    struct_field_defaults: Rc<HashMap<String, Vec<(String, Expr)>>>,
    struct_properties: Rc<HashMap<String, Vec<String>>>,
    enums: Rc<HashMap<String, Vec<(String, i64)>>>,
    // PMAT-506d: when lowering a method, the type of the `self` receiver (the
    // enclosing struct). `None` for a top-level function. Decorators are
    // tolerated when set (a `@dataclass` method may be plain, but the class
    // itself carried the decorator, not the method).
    self_type: Option<Type>,
    // PMAT-506h: when lowering a `@classmethod` body, the enclosing class name
    // (so `cls(...)` / `cls.method(...)` resolve to it). `None` otherwise.
    cls_name: Option<String>,
) -> Result<Function, FrontendError> {
    if !f.decorator_list.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` has decorators — not supported at v0.1.0",
            f.name
        )));
    }
    if !f.args.kwonlyargs.is_empty() || f.args.kwarg.is_some() {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses keyword-only args / **kwargs — not supported at v0.1.0",
            f.name
        )));
    }

    // Parse explicit param annotations (`a: int`). Default to I64 when
    // unannotated — Python lets that mean "any int" so it's safe.
    let mut params: Vec<Param> = Vec::with_capacity(f.args.args.len());
    for arg in f.args.args {
        let name = arg.def.arg.to_string();
        // PMAT-506d: a method's `self` receiver takes the enclosing struct type
        // (it carries no annotation in Python). A `self` outside a method
        // context is an error.
        let ty = if name == "self" {
            self_type.clone().ok_or_else(|| {
                FrontendError::Lower(format!(
                    "function `{}` has a `self` parameter outside a method (class) context",
                    f.name
                ))
            })?
        } else {
            match arg.def.annotation.as_ref() {
                None => Type::I64,
                Some(ann) => parse_type_annotation(&f.name, &name, ann)?,
            }
        };
        params.push(Param {
            name,
            ty,
            mutable: false,
        });
    }
    // PMAT-502dq: a `*args` vararg becomes a `list[elem]` parameter (each
    // collected positional arg is an `elem`). The annotation gives the element
    // type (default `int`); call sites collect the trailing positional args
    // into this list.
    if let Some(v) = f.args.vararg.as_ref() {
        let elem = match v.annotation.as_ref() {
            None => Type::I64,
            Some(ann) => parse_type_annotation(&f.name, v.arg.as_str(), ann)?,
        };
        params.push(Param {
            name: v.arg.to_string(),
            ty: Type::List(Box::new(elem)),
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
        structs,
        struct_methods,
        struct_field_defaults,
        struct_properties,
        enums,
    );
    ctx.cls_name = cls_name;
    let mut stmts: Vec<Stmt> = Vec::with_capacity(leading.len());
    for stmt in leading {
        // A single Python statement may lower to multiple meta-HIR
        // statements — most notably a multi-assignment `if/else`, where
        // each assigned name gets its own `Let` with an `IfExpr` value
        // (PMAT-005), or a `while` whose body lowers to a nested vec.
        stmts.extend(lower_block_stmt(&mut ctx, stmt.clone())?);
        // PMAT-502ez: after a provably-exiting `if x is None: return …` guard,
        // narrow `x` to `Some` for the remaining (and trailing) statements.
        register_none_guard_narrowing(&mut ctx, stmt);
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
                // PMAT-502ec: `return []` / `return {}` take their element /
                // K-V types from the declared return type. PMAT-502ew: an
                // `Optional[T]` return wraps `return None`/`return x` in
                // `OptionExpr`.
                lower_return_value(&ctx, value)?
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
        // PMAT-503b: a terminal `try: return <expr> except [E]: return <expr>`
        // → an `Expr::TryCatch` (catch_unwind over xpile's panic-based
        // exception model).
        ast::Stmt::Try(try_stmt) => match terminal_try_as_expr(&ctx, try_stmt)? {
            Some(expr) => expr,
            None => {
                return Err(FrontendError::Lower(format!(
                    "function `{}`'s final `try` is not the supported `try: return <expr> except [E]: return <expr>` shape — v0.2.0 first cut requires a single `except` (no bound name), no `else`/`finally`, and a single `return` in each arm",
                    f.name
                )));
            }
        },
        // PMAT-510 (Tranche 2): a terminal `match` whose every case is a single
        // `return <expr>` — desugar to an `if`/`elif`/`else` chain, then reuse
        // the terminal-if-as-expression path.
        ast::Stmt::Match(match_stmt) => {
            let if_stmt = desugar_match_to_if(match_stmt)?;
            match terminal_if_as_expr(&mut ctx, &if_stmt)? {
                Some(expr) => expr,
                None => {
                    return Err(FrontendError::Lower(format!(
                        "function `{}`'s final `match` does not have a single `return <expr>` in every case — v0.2.0 supports literal `case` patterns + a trailing `case _:`, each returning",
                        f.name
                    )));
                }
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
            // PMAT-502ec: an empty `[]` / `{}` trailing return has no element
            // type to infer (`infer_type` defaults `[]` to `list[int]`), so it
            // is compatible with ANY matching collection return type — trust
            // the declared type rather than the defaulted inference.
            let empty_literal_ok = match (&trailing_return, &declared) {
                (Expr::ListLit(v), Type::List(_)) => v.is_empty(),
                (Expr::DictLit(p), Type::Dict(_, _)) => p.is_empty(),
                // PMAT-502ew: a bare `None` return (`OptionExpr(None)`) has no
                // payload type to infer — accept it against any `Optional`.
                (Expr::OptionExpr(None), Type::Optional(_)) => true,
                _ => false,
            };
            if !empty_literal_ok && declared != inferred_return {
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
            // PMAT-506b (classes epic): an unknown *capitalized* name is taken
            // as a struct type (Python class-name convention) → `Type::Struct`.
            // A struct value emits the bare name; if no such class exists the
            // emitted Rust fails to compile (clean enough). Lowercase unknowns
            // stay an error (likely a typo or an unsupported builtin).
            other if other.starts_with(|ch: char| ch.is_ascii_uppercase()) => {
                Ok(Type::Struct(other.to_string()))
            }
            other => Err(FrontendError::Lower(format!(
                "function `{fn_name}` annotates `{site}` with unsupported type `{other}` — only `int`, `bool`, `BigInt`, `str`, `None`, `list[T]`, or a class/dataclass name at v0.2.0"
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
                // PMAT-502ew: `Optional[T]` → `Type::Optional(T)`. First cut
                // supports it as a function return type only (see the
                // return-wrapping in the function-body lowering).
                "Optional" => {
                    let inner = parse_type_annotation(
                        fn_name,
                        &format!("{site} Optional element"),
                        &sub.slice,
                    )?;
                    Ok(Type::Optional(Box::new(inner)))
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
            // PMAT-559: tuple-unpack with a subscript target —
            // `xs[i], xs[j] = xs[j], xs[i]` (the in-place swap idiom) and
            // general parallel assignment with `base[idx]` targets.
            // PMAT-572 (CORRECTNESS): a tuple-unpack that REASSIGNS already-bound
            // names from a tuple literal (`a, b = b, a + b` / `a, b = b, a % b`)
            // must reassign — not emit a fresh `let (mut a, mut b)`, which only
            // SHADOWS inside a nested block (while/for/if body), so the outer
            // variables never change (→ Euclid GCD infinite loop, iterative
            // Fibonacci all-zeros). The shared helper evaluates all RHS into temps
            // first (swap-safe) then `Assign`s each already-bound name. A *fresh*
            // all-Name unpack keeps the `Stmt::LetTuple` path. A non-tuple-literal
            // RHS reassign (`a, b = f()`) stays on `LetTuple` (deferred edge).
            if let ast::Expr::Tuple(targets) = &asn.targets[0] {
                let has_subscript = targets
                    .elts
                    .iter()
                    .any(|e| matches!(e, ast::Expr::Subscript(_)));
                let reassigns_bound = targets.elts.iter().any(
                    |e| matches!(e, ast::Expr::Name(n) if ctx.bound.contains(n.id.as_str())),
                );
                let rhs_is_tuple_lit = matches!(asn.value.as_ref(), ast::Expr::Tuple(_));
                if has_subscript || (reassigns_bound && rhs_is_tuple_lit) {
                    return lower_tuple_unpack_with_subscript(ctx, asn);
                }
            }
            lower_assign(ctx, asn).map(|s| vec![s])
        }
        // PMAT-470 (R1): augmented assignment `x += e` → `x = x <op> e`.
        ast::Stmt::AugAssign(aug) => lower_aug_assign(ctx, aug).map(|s| vec![s]),
        // PMAT-466 (v0.2.0 Track 1.C): annotated local `name: T = value`.
        ast::Stmt::AnnAssign(aa) => lower_ann_assign(ctx, aa).map(|s| vec![s]),
        ast::Stmt::If(if_stmt) => lower_if_stmt(ctx, if_stmt),
        // PMAT-510 (Tranche 2): a `match` statement → desugar to an
        // `if`/`elif`/`else` chain and lower that (no new IR).
        ast::Stmt::Match(match_stmt) => lower_if_stmt(ctx, desugar_match_to_if(&match_stmt)?),
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
                // PMAT-502ec: an early `return []` / `return {}` takes its
                // element / K-V types from the declared return type. PMAT-502ew:
                // an `Optional[T]` return wraps the value in `OptionExpr`.
                let lowered = lower_return_value(ctx, value)?;
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
        // PMAT-502dr: a nested `def inner(...): return <expr>` → a closure
        // binding (`Stmt::ClosureLet`), reusing the lambda machinery.
        ast::Stmt::FunctionDef(f) => desugar_nested_fn(ctx, &f).map(|s| vec![s]),
        // PMAT-503c (exceptions epic): statement-position assignment-form
        // try/except — `try: x = <expr> except [E]: x = <expr>` → a `let`/assign
        // whose value is `Expr::TryCatch` (catch_unwind). The trailing
        // return-form try is handled separately in `lower_function_def`.
        ast::Stmt::Try(try_stmt) => lower_assignment_try(ctx, try_stmt),
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
    // PMAT-533: `xs[i].append(e)` (list-of-list) / `d[k].append(e)`
    // (dict-of-list) — `append` on a *subscript* receiver (the receiver is
    // itself a list reached through one subscript). The plain `<name>.append(e)`
    // form is handled below; here the receiver is `<name>[<index>]`.
    if attr.attr.as_str() == "append" {
        if let ast::Expr::Subscript(sub) = attr.value.as_ref() {
            if let ast::Expr::Name(base) = sub.value.as_ref() {
                // A slice receiver (`xs[a:b].append`) is not a place — skip.
                if !matches!(sub.slice.as_ref(), ast::Expr::Slice(_)) {
                    let base_name = base.id.to_string();
                    let base_is_dict = match ctx.name_types.get(&base_name) {
                        Some(Type::List(inner)) if matches!(inner.as_ref(), Type::List(_)) => {
                            Some(false)
                        }
                        Some(Type::Dict(_, val)) if matches!(val.as_ref(), Type::List(_)) => {
                            Some(true)
                        }
                        _ => None,
                    };
                    if let Some(base_is_dict) = base_is_dict {
                        if call.args.len() != 1 || !call.keywords.is_empty() {
                            return Some(Err(FrontendError::Lower(format!(
                                "function `{}` calls `{base_name}[...].append(...)` with {} \
                                 positional arg(s); append takes exactly 1",
                                ctx.fn_name,
                                call.args.len()
                            ))));
                        }
                        let index = match lower_expr_in_ctx(ctx, (*sub.slice).clone()) {
                            Ok(e) => e,
                            Err(err) => return Some(Err(err)),
                        };
                        let elem = match lower_expr_in_ctx(ctx, call.args[0].clone()) {
                            Ok(e) => e,
                            Err(err) => return Some(Err(err)),
                        };
                        ctx.mutable.insert(base_name.clone());
                        return Some(Ok(Stmt::IndexAppend {
                            base: base_name,
                            index,
                            elem,
                            base_is_dict,
                        }));
                    }
                }
            }
        }
    }
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
    // PMAT-532: `s.update(other)` — 1-arg in-place set union (Python
    // `set.update`). Reuses `Stmt::ListExtend` (`s.extend((other).iter()
    // .cloned())`, valid for `HashSet` as well as `Vec`), mirroring the
    // dict.update branch below. (Placed before the list-clear block, which
    // early-returns `None` for a non-list receiver.)
    if method == "update" && matches!(receiver_ty, Some(Type::Set(_))) {
        if !call.keywords.is_empty() {
            return Some(Err(FrontendError::Lower(format!(
                "function `{}` calls `{receiver_name}.update(...)` with keyword args; \
                 v0.2.0 takes a single positional set",
                ctx.fn_name
            ))));
        }
        if call.args.len() != 1 {
            return Some(Err(FrontendError::Lower(format!(
                "function `{}` calls `{receiver_name}.update(...)` with {} positional arg(s); \
                 v0.2.0 requires exactly 1 (a set)",
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
    // PMAT-532: `s.clear()` / `d.clear()` — 0-arg in-place container clear.
    // Reuses `Stmt::ListMutate { Clear }` (emits `<name>.clear();`, valid for
    // `HashSet`/`HashMap` as well as `Vec`), mirroring the list-clear block
    // below (which early-returns `None` for a non-list receiver, so the set/
    // dict forms must be handled here first).
    if method == "clear" && matches!(receiver_ty, Some(Type::Set(_) | Type::Dict(_, _))) {
        if !call.args.is_empty() || !call.keywords.is_empty() {
            return Some(Err(FrontendError::Lower(format!(
                "function `{}` calls `{receiver_name}.clear(...)` with arguments; \
                 the in-place container clear takes none at v0.2.0",
                ctx.fn_name
            ))));
        }
        ctx.mutable.insert(receiver_name.to_string());
        return Some(Ok(Stmt::ListMutate {
            list_name: receiver_name.to_string(),
            op: ListMutateOp::Clear,
            of_float: false,
        }));
    }
    // PMAT-561: in-place keyed sort `xs.sort(key=lambda v: e [, reverse=True])`
    // desugars to `xs = sorted(xs, key=…, reverse=…)`, reusing the whole
    // `Expr::Sorted` / `SortKey` machinery (the non-mutating `sorted(...)` form).
    // The receiver is already marked mutable by the pre-walk (it keys on the
    // `sort` method name). Only fires when a `key` kwarg is present; the bare
    // `sort()` / `sort(reverse=…)` forms keep the `ListMutate` path below.
    if method == "sort"
        && matches!(receiver_ty, Some(Type::List(_)))
        && call.args.is_empty()
        && call
            .keywords
            .iter()
            .any(|k| k.arg.as_deref() == Some("key"))
    {
        let mut reverse = false;
        let mut key: Option<SortKey> = None;
        let elem_ty = match receiver_ty.as_ref() {
            Some(Type::List(inner)) => Some((**inner).clone()),
            _ => None,
        };
        for kw in &call.keywords {
            match kw.arg.as_deref() {
                Some("key") => match lower_sort_key(ctx, &kw.value, elem_ty.clone()) {
                    Ok(Some(k)) => key = Some(k),
                    Ok(None) => {
                        return Some(Err(FrontendError::Lower(format!(
                            "function `{}` calls `{receiver_name}.sort(key=…)` with an unsupported key — only `lambda p: e` and a bare callable name are supported",
                            ctx.fn_name
                        ))))
                    }
                    Err(e) => return Some(Err(e)),
                },
                Some("reverse") => match &kw.value {
                    ast::Expr::Constant(c) => match &c.value {
                        ast::Constant::Bool(b) => reverse = *b,
                        _ => {
                            return Some(Err(FrontendError::Lower(format!(
                                "function `{}` calls `{receiver_name}.sort(reverse=…)` with a non-bool value",
                                ctx.fn_name
                            ))))
                        }
                    },
                    _ => {
                        return Some(Err(FrontendError::Lower(format!(
                            "function `{}` calls `{receiver_name}.sort(reverse=…)` with a non-literal value",
                            ctx.fn_name
                        ))))
                    }
                },
                _ => {
                    return Some(Err(FrontendError::Lower(format!(
                        "function `{}` calls `{receiver_name}.sort(...)` with an unsupported keyword argument",
                        ctx.fn_name
                    ))))
                }
            }
        }
        ctx.mutable.insert(receiver_name.to_string());
        // PMAT-578: a keyless float sort needs `partial_cmp` (f64 has no `Ord`).
        // PMAT-603: with a `key=`, the comparison values are the KEY results, so
        // `of_float` tracks whether the key returns float (not the element type).
        let of_float = match &key {
            Some(k) => sort_key_is_float(ctx, k, elem_ty.clone()),
            // PMAT-622: float anywhere in the element (tuple/nested) → partial_cmp.
            None => matches!(
                infer_type_in_ctx(ctx, &Expr::Ident(receiver_name.to_string())),
                Type::List(elem) if type_contains_float(&elem)
            ),
        };
        return Some(Ok(Stmt::Assign {
            name: receiver_name.to_string(),
            value: Expr::Sorted {
                list: Box::new(Expr::Ident(receiver_name.to_string())),
                reverse,
                key,
                of_float,
            },
        }));
    }
    // PMAT-502ap: no-arg in-place list mutators `xs.sort()/.reverse()/.clear()`.
    let list_mutate_op = match method {
        "sort" => Some(ListMutateOp::Sort),
        "reverse" => Some(ListMutateOp::Reverse),
        "clear" => Some(ListMutateOp::Clear),
        _ => None,
    };
    if let Some(mut op) = list_mutate_op {
        let Some(Type::List(inner)) = receiver_ty.as_ref() else {
            return None;
        };
        // PMAT-555: the only accepted argument on an in-place mutator is
        // `reverse=<bool literal>` on `sort` — `reverse=True` selects a
        // descending sort (`SortDesc`), `reverse=False` a plain ascending one.
        // `key=…` and every other arg/kwarg are rejected (no closure support
        // for the in-place form yet).
        let is_sort_reverse_kwarg = matches!(op, ListMutateOp::Sort)
            && call.args.is_empty()
            && call.keywords.len() == 1
            && call.keywords[0].arg.as_ref().map(|a| a.as_str()) == Some("reverse");
        if is_sort_reverse_kwarg {
            match &call.keywords[0].value {
                ast::Expr::Constant(c) => match &c.value {
                    ast::Constant::Bool(true) => op = ListMutateOp::SortDesc,
                    ast::Constant::Bool(false) => {}
                    _ => {
                        return Some(Err(FrontendError::Lower(format!(
                            "function `{}` calls `{receiver_name}.sort(reverse=...)` with a non-bool value",
                            ctx.fn_name
                        ))))
                    }
                },
                _ => {
                    return Some(Err(FrontendError::Lower(format!(
                        "function `{}` calls `{receiver_name}.sort(reverse=...)` with a non-literal value — only `True`/`False` are supported",
                        ctx.fn_name
                    ))))
                }
            }
        } else if !call.args.is_empty() || !call.keywords.is_empty() {
            return Some(Err(FrontendError::Lower(format!(
                "function `{}` calls `{receiver_name}.{method}(...)` with arguments; \
                 the in-place list mutators take none, except `sort(reverse=<bool>)`, at v0.2.0",
                ctx.fn_name
            ))));
        }
        // PMAT-622: a float ANYWHERE in the element (bare, tuple, nested list)
        // needs the `partial_cmp` sort path, not `Vec::sort` (f64 not `Ord`).
        let of_float = type_contains_float(inner.as_ref());
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
    // PMAT-502eg: `xs.remove(x)` — remove the first element equal to `x`
    // (raises `ValueError` if absent). The Set receiver case was handled
    // above; here the receiver must be a list. 1 positional arg, no kwargs.
    if method == "remove" && matches!(receiver_ty, Some(Type::List(_))) {
        if !call.keywords.is_empty() || call.args.len() != 1 {
            return Some(Err(FrontendError::Lower(format!(
                "function `{}` calls `{receiver_name}.remove(...)` with {} positional arg(s){}; \
                 list remove takes exactly 1 (a value)",
                ctx.fn_name,
                call.args.len(),
                if call.keywords.is_empty() {
                    ""
                } else {
                    " plus keyword args"
                },
            ))));
        }
        let value = match lower_expr_in_ctx(ctx, call.args[0].clone()) {
            Ok(e) => e,
            Err(err) => return Some(Err(err)),
        };
        ctx.mutable.insert(receiver_name.to_string());
        return Some(Ok(Stmt::ListRemoveValue {
            list_name: receiver_name.to_string(),
            value,
        }));
    }
    // PMAT-502eh: `d.setdefault(k, v)` as a bare statement (the value-position
    // form `x = d.setdefault(...)` already works). Reuse the same
    // `DictSetDefault` lowering — which validates arity and types — then
    // discard the result via `let _ = …;` (the get-or-insert side effect is
    // what the statement is for). The receiver must be a dict.
    if method == "setdefault" && matches!(receiver_ty, Some(Type::Dict(_, _))) {
        let expr = match lower_expr_in_ctx(ctx, (*e.value).clone()) {
            Ok(x) => x,
            Err(err) => return Some(Err(err)),
        };
        let ty = infer_type_in_ctx(ctx, &expr);
        ctx.mutable.insert(receiver_name.to_string());
        return Some(Ok(Stmt::Let {
            name: "_".to_string(),
            ty,
            value: expr,
            mutable: false,
        }));
    }
    // PMAT-528/529: `xs.pop()` / `xs.pop(i)` (list) or `d.pop(k)` / `d.pop(k,
    // default)` (dict) as a bare statement (the value-position `x = …pop(…)`
    // already works). Reuse the value-position lowering — which validates the
    // receiver + args — then discard the popped element via `let _ = …;` (the
    // statement is used for the removal side effect, e.g. `while xs: xs.pop()`
    // or `d.pop(stale_key)`).
    if method == "pop" && matches!(receiver_ty, Some(Type::List(_) | Type::Dict(_, _))) {
        let expr = match lower_expr_in_ctx(ctx, (*e.value).clone()) {
            Ok(x) => x,
            Err(err) => return Some(Err(err)),
        };
        let ty = infer_type_in_ctx(ctx, &expr);
        ctx.mutable.insert(receiver_name.to_string());
        return Some(Ok(Stmt::Let {
            name: "_".to_string(),
            ty,
            value: expr,
            mutable: false,
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
    // PMAT-628: clone a reused non-Copy variable element so `g.append(row);
    // g.append(row)` (or `row` used after) doesn't move-then-use (E0382).
    let elem = clone_if_reused_non_copy(ctx, elem);
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
            // PMAT-626: `print(list)` / `print(tuple)` render the Python repr,
            // reusing the same `build_list_repr`/`build_tuple_repr` desugar as
            // `str()` / f-string interpolation (PMAT-623/624).
            Type::List(elem) => args.push(build_list_repr(lowered, elem.as_ref())?),
            Type::Tuple(elems) => args.push(build_tuple_repr(lowered, &elems)?),
            other => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` calls `print(...)` with a `{other:?}` argument — only int/str/float/bool/list/tuple (incl. f-strings) are supported at v0.2.0 (dict/set repr deferred)",
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
/// PMAT-502ck: is this for-loop iterable a `range(...)` or
/// `reversed(range(...))` call? Such calls drive the counter-`while` desugar;
/// any *other* call (e.g. `reversed(xs)`, `sorted(xs)`, `list(range(n))`)
/// lowers to a `List`-typed value and goes through the collection-iteration
/// path instead.
fn is_range_like_call(iter: &ast::Expr) -> bool {
    let ast::Expr::Call(c) = iter else {
        return false;
    };
    let ast::Expr::Name(n) = &*c.func else {
        return false;
    };
    if n.id.as_str() == "range" {
        return true;
    }
    if n.id.as_str() == "reversed" && c.args.len() == 1 {
        if let ast::Expr::Call(inner) = &c.args[0] {
            if let ast::Expr::Name(m) = &*inner.func {
                return m.id.as_str() == "range";
            }
        }
    }
    false
}

/// PMAT-502cj: lower `list(range(...))` to [`Expr::RangeList`]. Accepts the
/// 1/2/3-arg `range` forms; the optional step must be a **positive** integer
/// literal at first cut (negative-step materialisation is deferred). Bounds
/// are lowered context-aware (so `range(d[k])` works).
/// PMAT-521: lower a builtin's iterable argument into a *list-typed* `Expr`,
/// materialising forms that aren't first-class lists: `range(...)` → a `Vec`
/// (`lower_range_list`), and any set-typed value (`set(...)`/`frozenset(...)`/a
/// set local) → `Expr::SetToList`. A list-typed value passes through. Returns
/// `Ok(None)` for anything else, so the caller falls through to its own handling.
///
/// Without this, reduction builtins like `sum(range(n))` / `max(set(xs))` fell
/// through to context-free lowering (which doesn't recognise `range`/`set`) and
/// emitted undefined `range(...)`/`set(...)` Rust calls — a silent miscompile.
fn materialize_iterable_arg(
    ctx: &LoweringCtx,
    arg: &ast::Expr,
) -> Result<Option<Expr>, FrontendError> {
    if let ast::Expr::Call(inner) = arg {
        if matches!(&*inner.func, ast::Expr::Name(n) if n.id.as_str() == "range")
            && inner.keywords.is_empty()
        {
            return Ok(Some(lower_range_list(ctx, inner)?));
        }
    }
    let lowered = lower_expr_in_ctx(ctx, arg.clone())?;
    match infer_type_in_ctx(ctx, &lowered) {
        Type::List(_) => Ok(Some(lowered)),
        Type::Set(_) => Ok(Some(Expr::SetToList {
            set: Box::new(lowered),
        })),
        _ => Ok(None),
    }
}

/// PMAT-522: lower a builtin argument, materialising a `range(...)` into a `Vec`
/// (`lower_range_list`) since `range` isn't a first-class value. Any other
/// argument lowers normally. Used by `len`/`sorted`/`reversed` so
/// `len(range(n))` / `sorted(range(n))` / `reversed(range(n))` don't fall through
/// to context-free lowering (which emitted an undefined `range(...)` call).
fn lower_arg_materializing_range(
    ctx: &LoweringCtx,
    arg: &ast::Expr,
) -> Result<Expr, FrontendError> {
    if let ast::Expr::Call(inner) = arg {
        if matches!(&*inner.func, ast::Expr::Name(n) if n.id.as_str() == "range")
            && inner.keywords.is_empty()
        {
            return lower_range_list(ctx, inner);
        }
    }
    lower_expr_in_ctx(ctx, arg.clone())
}

/// PMAT-534: lower `x in range(...)` to a bounds check (the range is NOT
/// materialized — `x in range(10**9)` must not allocate a Vec). `x` must type
/// as `int`. Builds the equivalent meta-HIR boolean expression directly:
///   - `range(stop)`:                  `0 <= x && x < stop`
///   - `range(start, stop)`:           `start <= x && x < stop`
///   - `range(start, stop, step>0)`:   `start <= x && x < stop && (x - start) % step == 0`
///   - `range(start, stop, step<0)`:   `start >= x && x > stop && (start - x) % -step == 0`
///
/// `x` is reused across the comparisons; v0.2.0 operands are pure, so this
/// matches Python's evaluate-once semantics observationally, like the chained
/// comparison `a < x < b` desugar.
fn lower_in_range(
    ctx: &LoweringCtx,
    left: &ast::Expr,
    call: &ast::ExprCall,
) -> Result<Expr, FrontendError> {
    if !call.keywords.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` tests `x in range(..., kw=...)` with keyword args — only positional are supported",
            ctx.fn_name
        )));
    }
    let x = lower_expr_in_ctx(ctx, left.clone())?;
    if !matches!(infer_type_in_ctx(ctx, &x), Type::I64) {
        return Err(FrontendError::Lower(format!(
            "function `{}` tests `x in range(...)` with a non-int `x` — only int membership is supported at v0.2.0",
            ctx.fn_name
        )));
    }
    let and = |a: Expr, b: Expr| Expr::BinOp {
        op: BinOp::And,
        lhs: Box::new(a),
        rhs: Box::new(b),
    };
    let cmp = |op: BinOp, a: Expr, b: Expr| Expr::BinOp {
        op,
        lhs: Box::new(a),
        rhs: Box::new(b),
    };
    let (start, stop, step) = match call.args.as_slice() {
        [stop] => (Expr::LitInt(0), lower_expr_in_ctx(ctx, stop.clone())?, 1i64),
        [start, stop] => (
            lower_expr_in_ctx(ctx, start.clone())?,
            lower_expr_in_ctx(ctx, stop.clone())?,
            1i64,
        ),
        [start, stop, step] => {
            let s = extract_step_literal(step).ok_or_else(|| {
                FrontendError::Lower(format!(
                    "function `{}` tests `x in range(start, stop, step)` with a non-literal-int or zero step — a non-zero integer literal is required",
                    ctx.fn_name
                ))
            })?;
            (
                lower_expr_in_ctx(ctx, start.clone())?,
                lower_expr_in_ctx(ctx, stop.clone())?,
                s,
            )
        }
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{}` tests `x in range(...)` with {} args — Python supports 1-3",
                ctx.fn_name,
                call.args.len()
            )))
        }
    };
    let bounds = if step > 0 {
        and(
            cmp(BinOp::LtEq, start.clone(), x.clone()),
            cmp(BinOp::Lt, x.clone(), stop),
        )
    } else {
        and(
            cmp(BinOp::GtEq, start.clone(), x.clone()),
            cmp(BinOp::Gt, x.clone(), stop),
        )
    };
    if step == 1 || step == -1 {
        return Ok(bounds);
    }
    // Step reachability: `(x - start) % |step| == 0` (the subtraction is
    // non-negative under the already-asserted bounds).
    let (diff, modulus) = if step > 0 {
        (cmp(BinOp::Sub, x.clone(), start), Expr::LitInt(step))
    } else {
        (cmp(BinOp::Sub, start, x.clone()), Expr::LitInt(-step))
    };
    let reachable = cmp(BinOp::Eq, cmp(BinOp::Mod, diff, modulus), Expr::LitInt(0));
    Ok(and(bounds, reachable))
}

/// PMAT-546: a comprehension/loop iterable that types as `str` iterates its
/// characters (each a 1-char string) — materialize via `Expr::StrChars`
/// (→ `List(Str)`), the same conversion the `for c in s` loop and the
/// `enumerate`/`zip` paired loops use. A no-op for any non-`str` iterable, so it
/// is safe to apply at every comprehension iterable site.
fn str_iter_to_chars(ctx: &LoweringCtx, iter: Expr) -> Expr {
    if matches!(infer_type_in_ctx(ctx, &iter), Type::Str) {
        Expr::StrChars {
            string: Box::new(iter),
        }
    } else {
        iter
    }
}

fn lower_range_list(ctx: &LoweringCtx, call: &ast::ExprCall) -> Result<Expr, FrontendError> {
    let (start, stop, step) = match call.args.as_slice() {
        [stop] => (Expr::LitInt(0), lower_expr_in_ctx(ctx, stop.clone())?, 1i64),
        [start, stop] => (
            lower_expr_in_ctx(ctx, start.clone())?,
            lower_expr_in_ctx(ctx, stop.clone())?,
            1i64,
        ),
        [start, stop, step] => {
            // PMAT-523: a non-zero integer literal step — positive OR negative.
            // `extract_step_literal` already rejects a zero / non-literal step.
            let s = extract_step_literal(step).ok_or_else(|| {
                FrontendError::Lower(format!(
                    "function `{}` uses `list(range(..., step))` with a non-literal-int or zero step — v0.2.0 requires a non-zero integer literal",
                    ctx.fn_name
                ))
            })?;
            (
                lower_expr_in_ctx(ctx, start.clone())?,
                lower_expr_in_ctx(ctx, stop.clone())?,
                s,
            )
        }
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `range(...)` with {} args — Python supports 1-3",
                ctx.fn_name,
                call.args.len()
            )));
        }
    };
    Ok(Expr::RangeList {
        start: Box::new(start),
        stop: Box::new(stop),
        step,
    })
}

fn lower_for_stmt(ctx: &mut LoweringCtx, f: ast::StmtFor) -> Result<Vec<Stmt>, FrontendError> {
    if !f.orelse.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` has a `for ... else:` clause — Python's `else` on loops is not supported at v0.1.0",
            ctx.fn_name
        )));
    }

    // PMAT-562: three-way `for a, b, c in zip(x, y, z)` → Stmt::ForEachZip3.
    // Checked before the 2-tuple branch below (a 3-name target wouldn't match it).
    if let ast::Expr::Tuple(tgt) = &*f.target {
        if let [ast::Expr::Name(a), ast::Expr::Name(b), ast::Expr::Name(c)] = tgt.elts.as_slice() {
            if let ast::Expr::Call(call) = &*f.iter {
                if let ast::Expr::Name(fname) = call.func.as_ref() {
                    if fname.id.as_str() == "zip"
                        && call.keywords.is_empty()
                        && call.args.len() == 3
                    {
                        let names = [a.id.to_string(), b.id.to_string(), c.id.to_string()];
                        let mut iters = Vec::with_capacity(3);
                        for arg in &call.args {
                            // PMAT-544: a `str` arg iterates its chars (1-char strings).
                            let mut it = lower_expr_in_ctx(ctx, arg.clone())?;
                            if matches!(infer_type_in_ctx(ctx, &it), Type::Str) {
                                it = Expr::StrChars {
                                    string: Box::new(it),
                                };
                            }
                            let Type::List(elem) = infer_type_in_ctx(ctx, &it) else {
                                return Err(FrontendError::Lower(format!(
                                    "function `{}` uses `zip(...)` with a non-list/str argument — only list and str iteration is supported at v0.2.0",
                                    ctx.fn_name
                                )));
                            };
                            iters.push((it, *elem));
                        }
                        for (name, (_, elem)) in names.iter().zip(iters.iter()) {
                            ctx.bound.insert(name.clone());
                            ctx.name_types.insert(name.clone(), elem.clone());
                        }
                        let mut body: Vec<Stmt> = Vec::new();
                        for s in f.body {
                            body.extend(lower_block_stmt(ctx, s)?);
                        }
                        let [name1, name2, name3] = names;
                        let mut iters = iters.into_iter();
                        let (iter1, _) = iters.next().expect("3 args");
                        let (iter2, _) = iters.next().expect("3 args");
                        let (iter3, _) = iters.next().expect("3 args");
                        return Ok(vec![Stmt::ForEachZip3 {
                            first: name1,
                            second: name2,
                            third: name3,
                            iter1,
                            iter2,
                            iter3,
                            body,
                        }]);
                    }
                }
            }
        }
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
                            let mut iter_expr = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                            // PMAT-544: `enumerate(s)` / `zip(s, …)` over a string
                            // iterate its characters (each a 1-char string) —
                            // materialize via `Expr::StrChars` (→ `List(Str)`),
                            // the same conversion the single-var `for c in s` loop
                            // uses.
                            if matches!(infer_type_in_ctx(ctx, &iter_expr), Type::Str) {
                                iter_expr = Expr::StrChars {
                                    string: Box::new(iter_expr),
                                };
                            }
                            let Type::List(elem) = infer_type_in_ctx(ctx, &iter_expr) else {
                                return Err(FrontendError::Lower(format!(
                                    "function `{}` uses `{fname}(...)` over a non-list/str — only list and str iteration is supported at v0.2.0 first cut",
                                    ctx.fn_name
                                )));
                            };
                            let kind = if fname == "enumerate" {
                                // PMAT-502ca / PMAT-594: the start index may be
                                // the 2nd positional arg OR a `start=` keyword
                                // (an int literal at first cut). Reject unknown
                                // keywords and a positional+keyword conflict so
                                // other keyword forms produce a clean error
                                // rather than silently dropping the start.
                                if let Some(bad) = call
                                    .keywords
                                    .iter()
                                    .find(|k| k.arg.as_deref() != Some("start"))
                                {
                                    let kw = bad.arg.as_deref().unwrap_or("**kwargs");
                                    return Err(FrontendError::Lower(format!(
                                        "function `{}` uses `enumerate(...)` with an unsupported keyword argument `{kw}` — only `start=` is supported",
                                        ctx.fn_name
                                    )));
                                }
                                let kw_start = call
                                    .keywords
                                    .iter()
                                    .find(|k| k.arg.as_deref() == Some("start"));
                                let start_src: Option<&ast::Expr> = if call.args.len() == 2 {
                                    if kw_start.is_some() {
                                        return Err(FrontendError::Lower(format!(
                                            "function `{}` uses `enumerate(xs, <start>, start=…)` giving the start both positionally and by keyword",
                                            ctx.fn_name
                                        )));
                                    }
                                    Some(&call.args[1])
                                } else {
                                    kw_start.map(|k| &k.value)
                                };
                                let start = match start_src {
                                    Some(ast::Expr::Constant(c)) => match &c.value {
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
                                    Some(_) => {
                                        return Err(FrontendError::Lower(format!(
                                            "function `{}` uses `enumerate(xs, <start>)` with a non-literal start — only an integer literal is supported at v0.2.0",
                                            ctx.fn_name
                                        )));
                                    }
                                    None => 0,
                                };
                                ctx.name_types.insert(first.clone(), Type::I64);
                                ctx.name_types.insert(second.clone(), (*elem).clone());
                                PairIterKind::Enumerate { start }
                            } else {
                                // PMAT-594: `zip` takes no keyword arguments —
                                // reject rather than silently ignore them.
                                if !call.keywords.is_empty() {
                                    return Err(FrontendError::Lower(format!(
                                        "function `{}` uses `zip(...)` with keyword arguments — `zip` takes only positional iterables",
                                        ctx.fn_name
                                    )));
                                }
                                let mut other = lower_expr_in_ctx(ctx, call.args[1].clone())?;
                                // PMAT-544: `zip(xs, s)` over a string second arg.
                                if matches!(infer_type_in_ctx(ctx, &other), Type::Str) {
                                    other = Expr::StrChars {
                                        string: Box::new(other),
                                    };
                                }
                                let Type::List(elem2) = infer_type_in_ctx(ctx, &other) else {
                                    return Err(FrontendError::Lower(format!(
                                        "function `{}` uses `zip(...)` with a non-list/str second argument",
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
    //   - `range(...)` / `reversed(range(...))` call → the Let+While desugar
    //     below (counter loop).
    //   - Otherwise (non-call, OR any other call that lowers to a `List` —
    //     `reversed(xs)`, `sorted(xs)`, `list(range(n))`, `d.items()` …):
    //     lower the iter and emit a `Stmt::ForEach`. PMAT-502ck generalised
    //     this from "non-call only" to "non-range-like".
    if !is_range_like_call(&f.iter) {
        let iter_expr = lower_expr_in_ctx(ctx, (*f.iter).clone())?;
        let iter_ty = infer_type_in_ctx(ctx, &iter_expr);
        // PMAT-472 (R3): a dict iterates its keys (`for k in d:`), so
        // bind `target` to the key type and flag `over_keys`.
        // PMAT-502cl: `for c in s` iterates a string's characters, each a
        // 1-char string. Wrap the string in `Expr::StrChars` (→ a
        // `list[str]`) so the `ForEach` `.iter().cloned()` yields `String`s.
        let (iter_expr, elem_ty, over_keys) = match iter_ty {
            Type::List(elem) => (iter_expr, *elem, false),
            Type::Dict(key_ty, _) => (iter_expr, *key_ty, true),
            Type::Str => (
                Expr::StrChars {
                    string: Box::new(iter_expr),
                },
                Type::Str,
                false,
            ),
            other => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` iterates a non-collection expression typing as {other:?} — \
                     v0.2.0 supports `for target in range(...)`, `for target in <list[T]>`, \
                     `for key in <dict[K, V]>`, or `for char in <str>`; other iterables are deferred",
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

    // PMAT-502ci: `for i in reversed(range(...))` iterates the range
    // descending. Unwrap a `reversed(<range call>)` wrapper here; the bounds
    // are flipped to a step -1 range below. (`reversed` over a non-range is
    // left to the range-call error path, unchanged.)
    let (range_call_expr, reverse_range) = match &*f.iter {
        ast::Expr::Call(c)
            if matches!(&*c.func, ast::Expr::Name(n) if n.id.as_str() == "reversed")
                && c.keywords.is_empty()
                && c.args.len() == 1
                && matches!(&c.args[0], ast::Expr::Call(inner)
                    if matches!(&*inner.func, ast::Expr::Name(n) if n.id.as_str() == "range")) =>
        {
            (&c.args[0], true)
        }
        other => (other, false),
    };

    // Match range(...) call. Anything else (list/tuple/dict iteration)
    // requires collection types and is out of scope at v0.1.0.
    let call = match range_call_expr {
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

    // PMAT-502ci: flip a `reversed(range(...))` to a descending range. For a
    // step-1 range `a..b` the reverse is `b-1, b-2, …, a`, i.e. a range with
    // start `b-1`, stop `a-1`, step `-1`. Reusing `BinOp::Sub` keeps the
    // bounds under C-PY-INT-ARITH. A non-default step, or a BigInt-mode
    // function, is deferred (the general reversed-stride / BigInt bound math
    // is more involved).
    let (start_expr, stop_expr, step_int) = if reverse_range {
        if step_int != 1 {
            return Err(FrontendError::Lower(format!(
                "function `{}` uses `reversed(range(..., step))` with a non-default step — deferred at v0.2.0",
                ctx.fn_name
            )));
        }
        if matches!(ctx.fn_return_type, Type::BigInt) {
            return Err(FrontendError::Lower(format!(
                "function `{}` uses `reversed(range(...))` in a BigInt-mode function — deferred at v0.2.0",
                ctx.fn_name
            )));
        }
        let sub1 = |e: Expr| Expr::BinOp {
            op: BinOp::Sub,
            lhs: Box::new(e),
            rhs: Box::new(Expr::LitInt(1)),
        };
        (sub1(stop_expr), sub1(start_expr), -1)
    } else {
        (start_expr, stop_expr, step_int)
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
    //
    // PMAT-502dz: `for _ in range(n)` desugars to a counter `let mut _`,
    // which Rust rejects (`_` is not a binding). Mint a fresh `__xpile_idx{N}`
    // for the counter and register it so a body read of `_` resolves to the
    // same name. Bounds were lowered above (so a bound reading an *outer* `_`
    // still saw the outer rename); the rename installed here covers only this
    // loop's body. `saved_rename` is restored once the body is lowered.
    let (target_name, saved_rename) = ctx.enter_loop_var(&target_name);
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
    // PMAT-502dz: body lowered — pop this loop's `_`-rename (restoring any
    // outer one). Nothing below lowers Python expressions, so it is safe here.
    ctx.exit_loop_var(saved_rename);
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
/// PMAT-527: container truthiness in a boolean condition. Python treats a
/// non-empty `list`/`dict`/`set`/`str` as truthy; xpile otherwise requires a
/// Bool condition. Converts a container-typed condition to `len(c) != 0`
/// (reusing `Expr::Len` + `BinOp::Ne` — no new IR). A Bool (or anything else)
/// passes through unchanged, so the caller's Bool check still rejects
/// int/float-truthiness. `if xs:` / `while q:` / `x if xs else y`.
fn truthy_condition(ctx: &LoweringCtx, cond: Expr) -> Expr {
    match infer_type_in_ctx(ctx, &cond) {
        Type::List(_) | Type::Dict(_, _) | Type::Set(_) | Type::Str => Expr::BinOp {
            op: BinOp::NotEq,
            lhs: Box::new(Expr::Len(Box::new(cond))),
            rhs: Box::new(Expr::LitInt(0)),
        },
        _ => cond,
    }
}

fn lower_while_stmt(ctx: &mut LoweringCtx, w: ast::StmtWhile) -> Result<Stmt, FrontendError> {
    if !w.orelse.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` has a `while ... else:` clause — Python's `else` on loops is not supported at v0.1.0",
            ctx.fn_name
        )));
    }
    let cond = truthy_condition(ctx, lower_expr_in_ctx(ctx, *w.test)?);
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
            // PMAT-570: `del xs[-k]` deletes from the end — resolve the negative
            // literal to `len(xs) - k` (else `(-k) as usize` → usize::MAX → panic).
            let key = if let Some(k) = neg_literal_int(sub.slice.as_ref()) {
                Expr::BinOp {
                    op: BinOp::Sub,
                    lhs: Box::new(Expr::Len(Box::new(Expr::Ident(name.clone())))),
                    rhs: Box::new(Expr::LitInt(k)),
                }
            } else {
                let key = lower_expr_in_ctx(ctx, (*sub.slice).clone())?;
                let idx_ty = infer_type_in_ctx(ctx, &key);
                if !matches!(idx_ty, Type::I64) {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` deletes `{name}[<expr>]` where index types as {idx_ty:?}; only `int` indices are supported at v0.2.0",
                        ctx.fn_name
                    )));
                }
                key
            };
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
/// PMAT-510 (Tranche 2): desugar a structural-pattern `match` into an
/// equivalent `if`/`elif`/`else` chain, reusing all existing `if` lowering (no
/// new IR). First cut — the common literal-dispatch form:
///
/// ```python
/// match cmd:
///     case 0: ...
///     case 1: ...
///     case _: ...        # required trailing wildcard
/// ```
/// becomes `if cmd == 0: … elif cmd == 1: … else: …`.
///
/// Constraints (else a clean error): the subject must be a plain **Name** (so
/// repeating it across comparisons is side-effect-free); each non-wildcard case
/// is a literal **value** pattern (`case 0`/`case "x"`/`case -1` — int/float/str,
/// optionally negated); the **last** case must be the wildcard `case _:` (so the
/// chain is exhaustive); no guards, captures, singletons (`True`/`False`/`None`),
/// or class/sequence/mapping/or-patterns yet.
fn desugar_match_to_if(m: &ast::StmtMatch) -> Result<ast::StmtIf, FrontendError> {
    let ast::Expr::Name(subject) = m.subject.as_ref() else {
        return Err(FrontendError::Lower(
            "`match` subject must be a plain variable at v0.2.0 — bind the value to a name first"
                .to_string(),
        ));
    };
    if m.cases.len() < 2 {
        return Err(FrontendError::Lower(
            "`match` must have at least one `case <literal>:` and a trailing `case _:` at v0.2.0"
                .to_string(),
        ));
    }
    // The wildcard `case _:` must be the last case (and the only wildcard).
    let (wildcard, value_cases) = m.cases.split_last().expect("len >= 2 checked above");
    let is_wildcard = |c: &ast::MatchCase| {
        c.guard.is_none()
            && matches!(&c.pattern, ast::Pattern::MatchAs(a) if a.pattern.is_none() && a.name.is_none())
    };
    if !is_wildcard(wildcard) {
        return Err(FrontendError::Lower(
            "`match` must end with a wildcard `case _:` at v0.2.0 (exhaustiveness)".to_string(),
        ));
    }
    // `subject == <literal>` for one alternative (range/subject captured).
    let eq = |value: &ast::Expr| {
        ast::Expr::Compare(ast::ExprCompare {
            range: m.range,
            left: Box::new(ast::Expr::Name(subject.clone())),
            ops: vec![ast::CmpOp::Eq],
            comparators: vec![value.clone()],
        })
    };
    // A value pattern → its comparator expr (else a clean error). Accepts a
    // literal (`case 0:`) or a dotted value pattern `Name.attr` — notably a
    // PMAT-514 enum member (`case Color.RED:`), which lowers to `Expr::EnumVariant`
    // downstream so `subject == Color::RED` type-checks.
    let literal_value = |pat: &ast::Pattern| -> Result<ast::Expr, FrontendError> {
        match pat {
            ast::Pattern::MatchValue(pv)
                if is_literal_default(pv.value.as_ref())
                    || matches!(pv.value.as_ref(), ast::Expr::Attribute(a) if matches!(a.value.as_ref(), ast::Expr::Name(_))) =>
            {
                Ok((*pv.value).clone())
            }
            _ => Err(FrontendError::Lower(
                "`match` supports literal value patterns (`case 0:`/`case \"x\":`), dotted value patterns (`case Color.RED:`), `|`-patterns of those, and a trailing `case _:` at v0.2.0 — captures/guards/class/sequence/mapping/`True`/`False`/`None` patterns are unsupported".to_string(),
            )),
        }
    };
    // Fold the value cases (in reverse) over the wildcard body, building a nested
    // `if subject == <lit>: <body> else: <rest>`.
    let mut orelse: Vec<ast::Stmt> = wildcard.body.clone();
    for case in value_cases.iter().rev() {
        if case.guard.is_some() {
            return Err(FrontendError::Lower(
                "`match` case guards (`case … if …:`) are not supported at v0.2.0".to_string(),
            ));
        }
        // PMAT-512: an `|`-pattern (`case 0 | 1 | 2:`) → an OR of equality tests;
        // a plain value pattern → a single equality test.
        let test = match &case.pattern {
            ast::Pattern::MatchOr(po) => {
                let mut values = Vec::with_capacity(po.patterns.len());
                for alt in &po.patterns {
                    values.push(eq(&literal_value(alt)?));
                }
                ast::Expr::BoolOp(ast::ExprBoolOp {
                    range: m.range,
                    op: ast::BoolOp::Or,
                    values,
                })
            }
            other => eq(&literal_value(other)?),
        };
        let if_stmt = ast::StmtIf {
            range: m.range,
            test: Box::new(test),
            body: case.body.clone(),
            orelse,
        };
        orelse = vec![ast::Stmt::If(if_stmt)];
    }
    match orelse.pop() {
        Some(ast::Stmt::If(top)) => Ok(top),
        _ => Err(FrontendError::Lower(
            "`match` must have at least one `case <literal>:` before the `case _:` at v0.2.0"
                .to_string(),
        )),
    }
}

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
    let cond = truthy_condition(ctx, lower_expr_in_ctx(ctx, (*if_stmt.test).clone())?);
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

/// PMAT-503b (exceptions epic): if `try_stmt` is the supported value-producing
/// shape `try: return <body> except [Type]: return <handler>` — a single
/// `except` (catch-all; a named exception type is *accepted but not matched*,
/// since Rust panics are untyped), no bound exception name, no `else`/`finally`,
/// and both the try-body and the handler are a single `return <expr>` — return
/// the [`Expr::TryCatch`]. Any other shape returns `None` (the caller emits a
/// clean "unsupported try shape" error). xpile models Python exceptions as Rust
/// panics, so the `except` catches them via `catch_unwind` in codegen.
fn terminal_try_as_expr(
    ctx: &LoweringCtx,
    try_stmt: &ast::StmtTry,
) -> Result<Option<Expr>, FrontendError> {
    // First cut: no `else`/`finally`, exactly one `except`.
    if !try_stmt.orelse.is_empty() || !try_stmt.finalbody.is_empty() {
        return Ok(None);
    }
    if try_stmt.handlers.len() != 1 {
        return Ok(None);
    }
    // The try-body must be a single `return <expr>`.
    let [ast::Stmt::Return(body_ret)] = try_stmt.body.as_slice() else {
        return Ok(None);
    };
    let Some(body_val) = body_ret.value.as_deref() else {
        return Ok(None);
    };
    // The handler: catch-all (the type, if any, is not matched), no bound name,
    // body a single `return <expr>`.
    let ast::ExceptHandler::ExceptHandler(h) = &try_stmt.handlers[0];
    if h.name.is_some() {
        return Ok(None);
    }
    let [ast::Stmt::Return(h_ret)] = h.body.as_slice() else {
        return Ok(None);
    };
    let Some(h_val) = h_ret.value.as_deref() else {
        return Ok(None);
    };
    // Both arms are the function's return value — lower them the same way the
    // trailing `return` is lowered (so `Optional` return wrapping etc. apply).
    let body = lower_return_value(ctx, body_val)?;
    let handler = lower_return_value(ctx, h_val)?;
    Ok(Some(Expr::TryCatch {
        body: Box::new(body),
        handler: Box::new(handler),
    }))
}

/// PMAT-502fa (Optional epic): if `test` is `<name> is not None` over a
/// non-reassigned `Optional`-typed name, return that name. Drives intra-branch
/// narrowing in [`lower_if_stmt`]: inside the `if x is not None:` then-body, a
/// read of `x` unwraps to `T`. The complementary `is None` else-branch and the
/// `is not None … else: return` fall-through both route through other lowering
/// paths (the trailing if-expression / if-as-let) and are a separate sub-slice;
/// any shape other than a bare `<name> is not None` returns `None` (no narrowing
/// — conservative, so non-narrowed shapes behave exactly as before).
fn is_not_none_narrow_target(ctx: &LoweringCtx, test: &ast::Expr) -> Option<String> {
    let ast::Expr::Compare(cmp) = test else {
        return None;
    };
    if cmp.ops.len() != 1
        || cmp.comparators.len() != 1
        || !matches!(cmp.ops[0], ast::CmpOp::IsNot)
        || !matches!(&cmp.comparators[0], ast::Expr::Constant(k) if matches!(k.value, ast::Constant::None))
    {
        return None;
    }
    let ast::Expr::Name(name) = cmp.left.as_ref() else {
        return None;
    };
    let name = name.id.to_string();
    if ctx.mutable.contains(&name) {
        return None;
    }
    if !matches!(ctx.name_types.get(&name), Some(Type::Optional(_))) {
        return None;
    }
    Some(name)
}

fn lower_if_stmt(ctx: &mut LoweringCtx, if_stmt: ast::StmtIf) -> Result<Vec<Stmt>, FrontendError> {
    if is_if_as_let_shape(&if_stmt) {
        return lower_if_stmt_as_lets(ctx, if_stmt);
    }
    let cond = truthy_condition(ctx, lower_expr_in_ctx(ctx, (*if_stmt.test).clone())?);
    if !matches!(infer_type_in_ctx(ctx, &cond), Type::Bool) {
        return Err(FrontendError::Lower(format!(
            "function `{}` has an `if` condition that does not type as bool — v0.2.0 requires a boolean condition",
            ctx.fn_name
        )));
    }
    // PMAT-502fa: intra-branch Optional narrowing for `if x is not None:`. The
    // condition is already lowered above (so its own `x` is NOT narrowed); for
    // the then-body we temporarily register `x` as narrowed so reads unwrap to
    // `T`. Only add the entry if it wasn't already narrowed (by an outer guard),
    // so the restore afterwards doesn't clobber that outer fact.
    let narrow = is_not_none_narrow_target(ctx, &if_stmt.test);
    let added = matches!(&narrow, Some(n) if ctx.narrowed_some.insert(n.clone()));
    let mut then_body = Vec::new();
    for s in if_stmt.body {
        then_body.extend(lower_block_stmt(ctx, s)?);
    }
    if added {
        if let Some(n) = &narrow {
            ctx.narrowed_some.remove(n);
        }
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

    let cond = truthy_condition(ctx, lower_expr_in_ctx(ctx, (*if_stmt.test).clone())?);
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
                        // PMAT-547: mark each unpacked name bound, so a later
                        // augmented assignment (`total += i`) recognises it as
                        // initialised (the plain `Let` path does the same).
                        ctx.bound.insert(n.clone());
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
            // PMAT-547: per-name mutability — an unpacked name later
            // reassigned/augmented must bind `mut` (the mutability pre-walk
            // recorded it in `ctx.mutable`).
            let mutable = names.iter().map(|n| ctx.mutable.contains(n)).collect();
            return Ok(Stmt::LetTuple {
                names,
                mutable,
                value,
            });
        }
        // PMAT-461 (v0.2.0 Track 1.B): `xs[i] = v` indexed assignment
        // for lists. PMAT-466 (v0.2.0 Track 1.C): `d[k] = v` keyed
        // assignment for dicts. The Subscript target's value must be a
        // Name; the receiver's inferred type selects the variant
        // (`Type::List` → `Stmt::IndexAssign`, `Type::Dict` →
        // `Stmt::DictSet`). Either way the receiver is marked mutable.
        ast::Expr::Subscript(sub) => {
            // PMAT-559: delegate to the shared subscript-target lowering (also
            // used by the tuple-unpack/swap path). It handles nested chains
            // (`grid[i][j] = v` → `IndexAssign`), single list / dict targets,
            // and PMAT-560 negative-literal indices (`xs[-k] = v`).
            let value = lower_expr_in_ctx(ctx, *asn.value)?;
            return lower_subscript_assign_target(ctx, &sub, value);
        }
        // PMAT-506c (classes epic): struct field assignment `obj.field = value`.
        // `obj` must be a plain bound name typing as a struct, and `field` a
        // known member; the value lowers context-aware and `obj` is marked
        // mutable by the pre-walk.
        ast::Expr::Attribute(attr) => {
            let ast::Expr::Name(obj) = attr.value.as_ref() else {
                return Err(FrontendError::Lower(format!(
                    "function `{}` assigns to a non-Name attribute receiver — only `obj.field = v` over a struct local/param is supported at v0.2.0",
                    ctx.fn_name
                )));
            };
            let obj_name = obj.id.to_string();
            let field = attr.attr.to_string();
            let obj_ty = ctx.name_types.get(&obj_name).cloned().unwrap_or(Type::I64);
            let Type::Struct(sname) = obj_ty else {
                return Err(FrontendError::Lower(format!(
                    "function `{}` assigns to `.{field}` of `{obj_name}`, which is not a struct value",
                    ctx.fn_name
                )));
            };
            let known = ctx
                .structs
                .get(&sname)
                .is_some_and(|fs| fs.iter().any(|(f, _)| *f == field));
            if !known {
                return Err(FrontendError::Lower(format!(
                    "function `{}` assigns field `{field}` of `{sname}`, which has no such field",
                    ctx.fn_name
                )));
            }
            let value = lower_expr_in_ctx(ctx, *asn.value)?;
            return Ok(Stmt::FieldAssign {
                obj: obj_name,
                field,
                value,
            });
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

/// PMAT-559: tuple-unpack assignment where at least one target is a subscript —
/// `xs[i], xs[j] = xs[j], xs[i]` (the in-place swap idiom) and general parallel
/// assignment with `base[idx]` / `d[k]` targets. The right-hand side must be a
/// tuple literal of matching arity. All RHS elements are lowered into temporaries
/// FIRST (so a swap reads both old values before writing either), then each temp
/// is assigned to its target — a plain Name (`Assign`/`Let`) or a list/dict
/// subscript (`IndexAssign`/`DictSet`). The all-Name tuple form keeps the
/// `Stmt::LetTuple` path in [`lower_assign`].
fn lower_tuple_unpack_with_subscript(
    ctx: &mut LoweringCtx,
    asn: ast::StmtAssign,
) -> Result<Vec<Stmt>, FrontendError> {
    let ast::Expr::Tuple(target_tuple) = &asn.targets[0] else {
        unreachable!("caller checked a Tuple target");
    };
    let targets = target_tuple.elts.clone();
    // The RHS must be a tuple literal of equal arity (covers the swap idiom and
    // parallel assignment). A non-literal tuple RHS with subscript targets is
    // deferred — it'd need destructuring an arbitrary value into temps.
    let ast::Expr::Tuple(rhs) = asn.value.as_ref() else {
        return Err(FrontendError::Lower(format!(
            "function `{}` unpacks into subscript targets from a non-tuple-literal right-hand side — only `a[i], b[j] = x, y` is supported at v0.2.0",
            ctx.fn_name
        )));
    };
    if rhs.elts.len() != targets.len() {
        return Err(FrontendError::Lower(format!(
            "function `{}` unpacks {} targets from a {}-element tuple",
            ctx.fn_name,
            targets.len(),
            rhs.elts.len()
        )));
    }
    let rhs_elts = rhs.elts.clone();
    let mut stmts: Vec<Stmt> = Vec::new();
    // 1. Evaluate every RHS element into a temp BEFORE any assignment.
    let mut temps: Vec<String> = Vec::with_capacity(rhs_elts.len());
    for (i, rv) in rhs_elts.into_iter().enumerate() {
        let value = lower_expr_in_ctx(ctx, rv)?;
        let ty = infer_type_in_ctx(ctx, &value);
        let tmp = format!("__unpack{i}");
        ctx.name_types.insert(tmp.clone(), ty.clone());
        ctx.bound.insert(tmp.clone());
        stmts.push(Stmt::Let {
            name: tmp.clone(),
            ty,
            value,
            mutable: false,
        });
        temps.push(tmp);
    }
    // 2. Assign each temp to its target.
    for (target, tmp) in targets.into_iter().zip(temps) {
        let value = Expr::Ident(tmp);
        match target {
            ast::Expr::Name(n) => {
                let name = n.id.to_string();
                if ctx.bound.contains(&name) {
                    stmts.push(Stmt::Assign { name, value });
                } else {
                    let ty = infer_type_in_ctx(ctx, &value);
                    let mutable = ctx.mutable.contains(&name);
                    ctx.bound.insert(name.clone());
                    ctx.name_types.insert(name.clone(), ty.clone());
                    stmts.push(Stmt::Let {
                        name,
                        ty,
                        value,
                        mutable,
                    });
                }
            }
            ast::Expr::Subscript(sub) => {
                stmts.push(lower_subscript_assign_target(ctx, &sub, value)?);
            }
            other => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` has an unsupported tuple-unpack target {:?} — only names and single-level subscripts are supported at v0.2.0",
                    ctx.fn_name,
                    std::mem::discriminant(&other)
                )));
            }
        }
    }
    Ok(stmts)
}

/// PMAT-559: build the assignment statement for a subscript target `base[idx]`
/// (or nested `g[i][j]`) given an already-lowered value. Mirrors the `Subscript`
/// branch of [`lower_assign`]; shared with the tuple-unpack path.
/// PMAT-560: if `e` is a negative integer *literal* (`-k`, parsed as
/// `UnaryOp(USub, Int(k))`), return `k` (the positive magnitude). Used to
/// desugar `xs[-k]` from-the-end indexing on the assignment side.
fn neg_literal_int(e: &ast::Expr) -> Option<i64> {
    if let ast::Expr::UnaryOp(u) = e {
        if matches!(u.op, ast::UnaryOp::USub) {
            if let ast::Expr::Constant(c) = u.operand.as_ref() {
                if let ast::Constant::Int(k) = &c.value {
                    return k.to_string().parse::<i64>().ok();
                }
            }
        }
    }
    None
}

fn lower_subscript_assign_target(
    ctx: &mut LoweringCtx,
    sub: &ast::ExprSubscript,
    value: Expr,
) -> Result<Stmt, FrontendError> {
    if let Some((receiver, indices)) = peel_nested_subscript_assign(ctx, sub)? {
        ctx.mutable.insert(receiver.clone());
        return Ok(Stmt::IndexAssign {
            list_name: receiver,
            indices,
            value,
        });
    }
    let receiver = match sub.value.as_ref() {
        ast::Expr::Name(n) => n.id.to_string(),
        _ => unreachable!("peel_nested_subscript_assign validated a Name base"),
    };
    let single = (*sub.slice).clone();
    match ctx.name_types.get(&receiver).cloned() {
        Some(Type::List(_)) => {
            // PMAT-560: negative-literal index `xs[-k] = v` → `xs[len(xs) - k] = v`
            // (Python from-the-end assignment), mirroring the read-side desugar
            // (PMAT-502s). Without this the index emits `(-k) as usize` →
            // `usize::MAX` → an out-of-bounds panic. A negative literal parses as
            // `UnaryOp(USub, Int(k))`.
            let index = if let Some(k) = neg_literal_int(&single) {
                Expr::BinOp {
                    op: BinOp::Sub,
                    lhs: Box::new(Expr::Len(Box::new(Expr::Ident(receiver.clone())))),
                    rhs: Box::new(Expr::LitInt(k)),
                }
            } else {
                // PMAT-466: ctx-aware so a dict read used as a list index
                // (`xs[d[k]] = v`) lowers to `DictGet`, not a nested list index.
                let index = lower_expr_in_ctx(ctx, single)?;
                let idx_ty = infer_type_in_ctx(ctx, &index);
                if !matches!(idx_ty, Type::I64) {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` indexed-assigns `{receiver}[<expr>]` where index types as {idx_ty:?}; only `int` indices are supported at v0.2.0",
                        ctx.fn_name
                    )));
                }
                index
            };
            ctx.mutable.insert(receiver.clone());
            Ok(Stmt::IndexAssign {
                list_name: receiver,
                indices: vec![index],
                value,
            })
        }
        Some(Type::Dict(_, _)) => {
            let key = lower_expr_in_ctx(ctx, single)?;
            ctx.mutable.insert(receiver.clone());
            Ok(Stmt::DictSet {
                dict_name: receiver,
                key,
                value,
            })
        }
        _ => Err(FrontendError::Lower(format!(
            "function `{}` keyed-assigns to `{receiver}` which doesn't type as list[T] or dict[K, V] — v0.2.0 supports list/dict subscript assignment only",
            ctx.fn_name
        ))),
    }
}

/// PMAT-503c (exceptions epic): statement-position assignment-form try/except —
/// `try: <name> = <body> except [E]: <name> = <handler>` (same target in both
/// arms) → `let <name> = <Expr::TryCatch>` (or `<name> = …` if already bound).
/// Reuses the PMAT-503b `TryCatch` machinery: the `catch_unwind` closure
/// *produces the value*, so there's no closure-mutation hazard. First cut: a
/// single `except` (catch-all; a named exception type is accepted but not
/// matched, since Rust panics are untyped) with no bound exception name, no
/// `else`/`finally`, and exactly one `<name> = <expr>` in each arm.
fn lower_assignment_try(
    ctx: &mut LoweringCtx,
    try_stmt: ast::StmtTry,
) -> Result<Vec<Stmt>, FrontendError> {
    let unsupported = |ctx: &LoweringCtx| {
        FrontendError::Lower(format!(
            "function `{}`'s `try` is not the supported `try: x = <expr> except [E]: x = <expr>` shape (same target, single `except` without a bound name, no `else`/`finally`, one assignment per arm) — v0.2.0 first cut",
            ctx.fn_name
        ))
    };
    if !try_stmt.orelse.is_empty() || !try_stmt.finalbody.is_empty() || try_stmt.handlers.len() != 1
    {
        return Err(unsupported(ctx));
    }
    // Extract `<name> = <expr>` from a single-statement body.
    fn single_name_assign(body: &[ast::Stmt]) -> Option<(String, &ast::Expr)> {
        let [ast::Stmt::Assign(a)] = body else {
            return None;
        };
        if a.targets.len() != 1 {
            return None;
        }
        let ast::Expr::Name(n) = &a.targets[0] else {
            return None;
        };
        Some((n.id.to_string(), a.value.as_ref()))
    }
    let Some((body_name, body_val)) = single_name_assign(&try_stmt.body) else {
        return Err(unsupported(ctx));
    };
    let ast::ExceptHandler::ExceptHandler(h) = &try_stmt.handlers[0];
    if h.name.is_some() {
        return Err(unsupported(ctx));
    }
    let Some((handler_name, handler_val)) = single_name_assign(&h.body) else {
        return Err(unsupported(ctx));
    };
    if body_name != handler_name {
        return Err(FrontendError::Lower(format!(
            "function `{}`'s try/except assigns different names (`{body_name}` vs `{handler_name}`) — both arms must assign the same target",
            ctx.fn_name
        )));
    }
    let body = lower_expr_in_ctx(ctx, body_val.clone())?;
    let handler = lower_expr_in_ctx(ctx, handler_val.clone())?;
    let value = Expr::TryCatch {
        body: Box::new(body),
        handler: Box::new(handler),
    };
    let ty = infer_type_in_ctx(ctx, &value);
    let name = body_name;
    if ctx.bound.contains(&name) {
        Ok(vec![Stmt::Assign { name, value }])
    } else {
        let mutable = ctx.mutable.contains(&name);
        ctx.bound.insert(name.clone());
        ctx.name_types.insert(name.clone(), ty.clone());
        Ok(vec![Stmt::Let {
            name,
            ty,
            value,
            mutable,
        }])
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
    // PMAT-629: `s *= n` (str) / `xs *= n` (list) is REPETITION, not numeric
    // multiplication — route to `Expr::Repeat` (same as `s * n` / `xs * n`),
    // else the backend emits `String`/`Vec::checked_mul` (E0599). `try_repeat`
    // returns `None` for int*int, so `x *= 2` still lowers to `BinOp::Mul`.
    if matches!(ast_op, ast::Operator::Mult) {
        let lhs_ty = infer_type_in_ctx(ctx, &lhs);
        let rhs_ty = infer_type_in_ctx(ctx, &rhs);
        if let Some(rep) = try_repeat(&lhs_ty, &rhs_ty, &lhs, &rhs) {
            return Ok(rep);
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
    } else if matches!(op, BinOp::Add)
        && matches!(infer_type_in_ctx(ctx, &lhs), Type::List(_))
        && matches!(infer_type_in_ctx(ctx, &rhs), Type::List(_))
    {
        // PMAT-604: `+` over two lists is concatenation, not integer addition.
        // The flat `xs += [..]` case is special-cased to `ListExtend` before
        // `combine_aug`, but the SUBSCRIPT aug-assign (`grid[i] += [..]`,
        // `grid[i][j] += [..]`) routes through here — without this it fell to a
        // `BinOp::Add` that the backend emits as `Vec::checked_add` (E0599).
        Ok(Expr::ListConcat {
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    } else if matches!(infer_type_in_ctx(ctx, &lhs), Type::Set(_))
        && matches!(infer_type_in_ctx(ctx, &rhs), Type::Set(_))
        && set_op_from_ast(ast_op).is_some()
    {
        // PMAT-615: augmented set algebra `s -= / |= / &= / ^= other` reuses the
        // binop `SetOp` path (difference / union / intersection /
        // symmetric_difference), exactly like the non-augmented `s - other`.
        // Without this it fell through to a `BinOp`, which the backend emits as
        // `HashSet::checked_sub` (E0599) for `-=` and owned-value `|`/`&`/`^` on
        // `HashSet` (E0369) for the others — transpile-success → invalid Rust.
        Ok(Expr::SetOp {
            op: set_op_from_ast(ast_op).expect("is_some checked above"),
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

/// PMAT-502ea: peel a subscript-assignment target `base[i]…[k]` to its
/// Name receiver + lowered index path (base→leaf), shared by plain
/// (`grid[i][j] = v`, PMAT-502dy) and augmented (`grid[i][j] += v`)
/// nested assignment. Returns `Ok(None)` for a single-level target
/// (`xs[i]` / `d[k]`) so the caller applies its own list/dict handling;
/// returns `Ok(Some((receiver, indices)))` for a genuinely-nested (≥2)
/// list target after validating the base types as `list[list[…]]` of
/// matching depth with every index `int`. A non-Name base or a
/// too-shallow / non-list base / non-int index is a clear error.
fn peel_nested_subscript_assign(
    ctx: &mut LoweringCtx,
    sub: &ast::ExprSubscript,
) -> Result<Option<(String, Vec<Expr>)>, FrontendError> {
    let mut slices: Vec<ast::Expr> = vec![(*sub.slice).clone()];
    let mut base = (*sub.value).clone();
    let receiver = loop {
        match base {
            ast::Expr::Name(n) => break n.id.to_string(),
            ast::Expr::Subscript(inner) => {
                slices.push((*inner.slice).clone());
                base = (*inner.value).clone();
            }
            _ => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` has a non-Name subscript-assignment target — v0.2.0 supports `<name>[k]…[k] = v`",
                    ctx.fn_name
                )));
            }
        }
    };
    slices.reverse();
    // Single subscript: the caller handles the list/dict single-level form.
    if slices.len() == 1 {
        return Ok(None);
    }
    // Nested list indexing — the base must type as `list[list[…]]` nested at
    // least as deep as the index path, and every index must be `int`.
    let mut depth_ty = ctx.name_types.get(&receiver).cloned();
    for _ in 0..slices.len() {
        match depth_ty {
            Some(Type::List(elem)) => depth_ty = Some(*elem),
            _ => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` nested-subscript-assigns `{receiver}[…][…]` but it is not a nested `list[list[…]]` of matching depth — only nested-list assignment is supported at v0.2.0",
                    ctx.fn_name
                )));
            }
        }
    }
    let mut indices = Vec::with_capacity(slices.len());
    for s in slices {
        let idx = lower_expr_in_ctx(ctx, s)?;
        if !matches!(infer_type_in_ctx(ctx, &idx), Type::I64) {
            return Err(FrontendError::Lower(format!(
                "function `{}` nested-indexed-assigns `{receiver}[…]` with a non-int index — only `int` indices are supported",
                ctx.fn_name
            )));
        }
        indices.push(idx);
    }
    Ok(Some((receiver, indices)))
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
            // PMAT-502eb: `xs += ys` over a list is Python's in-place list
            // extend, NOT numeric addition — emit `Stmt::ListExtend` (same as
            // `xs.extend(ys)`). Otherwise `combine_aug` routes `+` through
            // `checked_add`, which doesn't exist on `Vec` (silent miscompile).
            // Any other augmented operator on a list (`*=`, …) is rejected
            // cleanly rather than miscompiled.
            if matches!(ctx.name_types.get(&name), Some(Type::List(_))) {
                // PMAT-629: `xs *= n` is list repetition (`xs = xs * n`) — route
                // through `combine_aug` (which returns `Expr::Repeat`) and reassign,
                // mirroring `s *= n` for strings. (Was rejected as "only +=".)
                if matches!(aug.op, ast::Operator::Mult) {
                    if !matches!(infer_type_in_ctx(ctx, &rhs), Type::I64) {
                        return Err(FrontendError::Lower(format!(
                            "function `{}` uses `{name} *= <non-int>` on a list — repetition needs an int count",
                            ctx.fn_name
                        )));
                    }
                    ctx.mutable.insert(name.clone());
                    let value = combine_aug(ctx, &aug.op, Expr::Ident(name.clone()), rhs)?;
                    return Ok(Stmt::Assign { name, value });
                }
                if !matches!(aug.op, ast::Operator::Add) {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` uses `{name} <op>= …` on a list with an operator other than `+=` (extend) or `*=` (repeat) — v0.2.0 supports those two",
                        ctx.fn_name
                    )));
                }
                let other_ty = infer_type_in_ctx(ctx, &rhs);
                if !matches!(other_ty, Type::List(_)) {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` augments list `{name}` with `+= <{other_ty:?}>`; v0.2.0 supports `list += list` (in-place extend)",
                        ctx.fn_name
                    )));
                }
                ctx.mutable.insert(name.clone());
                return Ok(Stmt::ListExtend {
                    list_name: name,
                    other: rhs,
                });
            }
            // PMAT-593: `a |= b` over two dicts is PEP 584 in-place union —
            // emit `Stmt::DictUpdate` (identical to `a.update(b)`; b wins on
            // key conflicts). Other augmented operators on a dict are rejected
            // cleanly rather than routed through `combine_aug` (which would
            // emit an invalid `a = (a | b)` over `HashMap`).
            if matches!(ctx.name_types.get(&name), Some(Type::Dict(_, _))) {
                if !matches!(aug.op, ast::Operator::BitOr) {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` uses `{name} <op>= …` on a dict with an operator other than `|=` (PEP 584 union) — v0.2.0 supports only dict `|=`",
                        ctx.fn_name
                    )));
                }
                let other_ty = infer_type_in_ctx(ctx, &rhs);
                if !matches!(other_ty, Type::Dict(_, _)) {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` augments dict `{name}` with `|= <{other_ty:?}>`; v0.2.0 supports `dict |= dict` (in-place union)",
                        ctx.fn_name
                    )));
                }
                ctx.mutable.insert(name.clone());
                return Ok(Stmt::DictUpdate {
                    dict_name: name,
                    other: rhs,
                });
            }
            let value = combine_aug(ctx, &aug.op, Expr::Ident(name.clone()), rhs)?;
            Ok(Stmt::Assign { name, value })
        }
        // PMAT-497: augmented subscript assignment `d[k] += v` /
        // `xs[i] += v` — desugar to `d[k] = d[k] <op> v`, reusing the
        // shipped DictGet/Index reads + DictSet/IndexAssign writes.
        ast::Expr::Subscript(sub) => {
            // PMAT-502ea: nested augmented subscript `grid[i][j] += v` →
            // `grid[i][j] = grid[i][j] <op> v`. Peel + validate the index
            // path (shared with plain `= v`), fold the indices into a nested
            // `Expr::Index` read for the current value, combine, then emit a
            // multi-index `IndexAssign`. `None` ⇒ single-level (below).
            if let Some((receiver, indices)) = peel_nested_subscript_assign(ctx, sub)? {
                let mut current = Expr::Ident(receiver.clone());
                for idx in &indices {
                    current = Expr::Index {
                        collection: Box::new(current),
                        index: Box::new(idx.clone()),
                    };
                }
                let value = combine_aug(ctx, &aug.op, current, rhs)?;
                ctx.mutable.insert(receiver.clone());
                return Ok(Stmt::IndexAssign {
                    list_name: receiver,
                    indices,
                    value,
                });
            }
            let receiver = match sub.value.as_ref() {
                ast::Expr::Name(n) => n.id.to_string(),
                _ => unreachable!("peel_nested_subscript_assign validated a Name base"),
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
                    // PMAT-560: negative-literal index `xs[-k] += v` resolves to
                    // `xs[len(xs) - k]` on both the read and write side (same
                    // desugar as plain `xs[-k] = v`).
                    let index = if let Some(k) = neg_literal_int(sub.slice.as_ref()) {
                        Expr::BinOp {
                            op: BinOp::Sub,
                            lhs: Box::new(Expr::Len(Box::new(Expr::Ident(receiver.clone())))),
                            rhs: Box::new(Expr::LitInt(k)),
                        }
                    } else {
                        let index = lower_expr_in_ctx(ctx, (*sub.slice).clone())?;
                        if !matches!(infer_type_in_ctx(ctx, &index), Type::I64) {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` augments `{receiver}[<expr>]` with a non-int index",
                                ctx.fn_name
                            )));
                        }
                        index
                    };
                    let current = Expr::Index {
                        collection: Box::new(Expr::Ident(receiver.clone())),
                        index: Box::new(index.clone()),
                    };
                    let value = combine_aug(ctx, &aug.op, current, rhs)?;
                    ctx.mutable.insert(receiver.clone());
                    Ok(Stmt::IndexAssign {
                        list_name: receiver,
                        indices: vec![index],
                        value,
                    })
                }
                _ => Err(FrontendError::Lower(format!(
                    "function `{}` augments `{receiver}[...]` which doesn't type as list[T] or dict[K, V]",
                    ctx.fn_name
                ))),
            }
        }
        // PMAT-506i (classes epic): augmented struct field assignment
        // `obj.field <op>= v` — desugar to `obj.field = obj.field <op> v`,
        // reusing the shipped `FieldAccess` read + `FieldAssign` write (PMAT-506c).
        // `obj` must be a plain bound name typing as a struct and `field` a known
        // member; `obj` is marked mutable by the pre-walk (walk_counts counts an
        // Attribute aug-target). A `self.field <op>= v` lowers to a
        // `FieldAssign { obj: "self", … }` and is then rejected by
        // `body_assigns_self` (read-only methods), consistent with `self.f = v`.
        ast::Expr::Attribute(attr) => {
            let ast::Expr::Name(obj) = attr.value.as_ref() else {
                return Err(FrontendError::Lower(format!(
                    "function `{}` augments a non-Name attribute receiver — only `obj.field <op>= v` over a struct local/param is supported at v0.2.0",
                    ctx.fn_name
                )));
            };
            let obj_name = obj.id.to_string();
            let field = attr.attr.to_string();
            let obj_ty = ctx.name_types.get(&obj_name).cloned().unwrap_or(Type::I64);
            let Type::Struct(sname) = obj_ty else {
                return Err(FrontendError::Lower(format!(
                    "function `{}` augments `.{field}` of `{obj_name}`, which is not a struct value",
                    ctx.fn_name
                )));
            };
            let known = ctx
                .structs
                .get(&sname)
                .is_some_and(|fs| fs.iter().any(|(f, _)| *f == field));
            if !known {
                return Err(FrontendError::Lower(format!(
                    "function `{}` augments field `{field}` of `{sname}`, which has no such field",
                    ctx.fn_name
                )));
            }
            let current = Expr::FieldAccess {
                obj: Box::new(Expr::Ident(obj_name.clone())),
                field: field.clone(),
            };
            let value = combine_aug(ctx, &aug.op, current, rhs)?;
            ctx.mutable.insert(obj_name.clone());
            Ok(Stmt::FieldAssign {
                obj: obj_name,
                field,
                value,
            })
        }
        _ => Err(FrontendError::Lower(format!(
            "function `{}` uses augmented assignment on an unsupported target — supported: `name <op>= e`, `d[k] <op>= e`, `xs[i] <op>= e`, `obj.field <op>= e`",
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
/// PMAT-502fc/fd: shared two-generator comprehension desugaring. Validates the
/// two generators (plain-Name targets over `list[T]` iterables, ≤1 `if` each),
/// binds both loop vars (the inner iterable is lowered with the outer var in
/// scope, so it may reference it), and lowers each generator's optional filter.
/// It then calls `build` — with both loop vars bound — to produce the
/// per-element insert statement plus the accumulator's type and empty-literal
/// initializer, and assembles `let mut target = init` + nested `for`/`if` loops.
/// Shared by the list/dict/set 2-generator paths. Range / tuple-target /
/// 3+-generator comps remain deferred (clean error). The genexpr/`any`/`all`/
/// `sum` map-path is a separate lowering and still rejects multiple generators.
fn desugar_comp_2gen(
    ctx: &mut LoweringCtx,
    target: &str,
    generators: &[ast::Comprehension],
    kind: &str,
    build: impl FnOnce(&mut LoweringCtx) -> Result<(Stmt, Type, Expr), FrontendError>,
) -> Result<Vec<Stmt>, FrontendError> {
    // Capture the name (not `ctx`) so these helpers don't hold a borrow that
    // would conflict with the `&mut ctx` lowering calls below.
    let fn_name = ctx.fn_name.clone();
    let plain_name = |g: &ast::Comprehension| -> Result<String, FrontendError> {
        match &g.target {
            ast::Expr::Name(n) => Ok(n.id.to_string()),
            _ => Err(FrontendError::Lower(format!(
                "function `{fn_name}` has a multi-generator {kind} comprehension with a tuple/non-Name target — deferred (use plain `for x in … for y in …`)"
            ))),
        }
    };
    let list_elem = |ty: Type| -> Result<Type, FrontendError> {
        match ty {
            Type::List(e) => Ok(*e),
            other => Err(FrontendError::Lower(format!(
                "function `{fn_name}` has a multi-generator {kind} comprehension over an iterable typing as {other:?}; v0.2.0 supports two `for` clauses over `list[T]` / `range(...)` iterables (dict iterables deferred)"
            ))),
        }
    };
    // PMAT-543: materialize a bare `range(...)` generator iterable to a `Vec`
    // (the 2-generator path lowers to nested `ForEach` over list-typed iters, so
    // a range must become a first-class list — exactly like the 1-generator
    // path's range handling, just via `lower_range_list`).
    let materialize_iter =
        |ctx: &mut LoweringCtx, iter: &ast::Expr| -> Result<Expr, FrontendError> {
            if let ast::Expr::Call(call) = iter {
                if matches!(call.func.as_ref(), ast::Expr::Name(n) if n.id.as_str() == "range")
                    && call.keywords.is_empty()
                {
                    return lower_range_list(ctx, call);
                }
            }
            lower_expr_in_ctx(ctx, iter.clone())
        };
    let outer = &generators[0];
    let inner = &generators[1];
    // PMAT-563: multiple `if` clauses per generator are ANDed by `comp_filter`.
    let outer_var = plain_name(outer)?;
    let inner_var = plain_name(inner)?;

    // Outer generator: iterable + element type + bound var, then its filter.
    let outer_iter = materialize_iter(ctx, &outer.iter)?;
    let outer_elem_ty = list_elem(infer_type_in_ctx(ctx, &outer_iter))?;
    ctx.bound.insert(outer_var.clone());
    ctx.name_types
        .insert(outer_var.clone(), outer_elem_ty.clone());
    let outer_filter = comp_filter(ctx, outer, kind)?;

    // Inner generator (lowered with the outer var in scope, so its iterable may
    // reference the outer binding).
    let inner_iter = materialize_iter(ctx, &inner.iter)?;
    let inner_elem_ty = list_elem(infer_type_in_ctx(ctx, &inner_iter))?;
    ctx.bound.insert(inner_var.clone());
    ctx.name_types
        .insert(inner_var.clone(), inner_elem_ty.clone());
    let inner_filter = comp_filter(ctx, inner, kind)?;

    // Flavour-specific: lower the payload (with both vars bound) into the insert
    // statement + accumulator type + empty-literal initializer.
    let (insert, acc_ty, acc_init) = build(ctx)?;
    ctx.bound.insert(target.to_string());
    ctx.name_types.insert(target.to_string(), acc_ty.clone());

    // Wrap a body in its generator's optional `if` filter.
    let wrap = |filter: Option<Expr>, body: Vec<Stmt>| -> Vec<Stmt> {
        match filter {
            None => body,
            Some(cond) => vec![Stmt::If {
                cond,
                then_body: body,
                else_body: Vec::new(),
            }],
        }
    };

    let inner_body = wrap(inner_filter, vec![insert]);
    let inner_loop = Stmt::ForEach {
        var: inner_var,
        iter: inner_iter,
        elem_ty: inner_elem_ty,
        body: inner_body,
        over_keys: false,
    };
    let outer_body = wrap(outer_filter, vec![inner_loop]);
    let outer_loop = Stmt::ForEach {
        var: outer_var,
        iter: outer_iter,
        elem_ty: outer_elem_ty,
        body: outer_body,
        over_keys: false,
    };
    Ok(vec![
        Stmt::Let {
            name: target.to_string(),
            ty: acc_ty,
            value: acc_init,
            mutable: true,
        },
        outer_loop,
    ])
}

/// PMAT-502fc: two-generator list comprehension `[expr for x in a for y in b]`
/// → nested loops appending to the accumulator. See [`desugar_comp_2gen`].
fn desugar_list_comp_2gen(
    ctx: &mut LoweringCtx,
    target: &str,
    comp: &ast::ExprListComp,
) -> Result<Vec<Stmt>, FrontendError> {
    desugar_comp_2gen(ctx, target, &comp.generators, "list", |ctx| {
        let elem = lower_expr_in_ctx(ctx, (*comp.elt).clone())?;
        let acc_ty = Type::List(Box::new(infer_type_in_ctx(ctx, &elem)));
        let insert = Stmt::ListAppend {
            list_name: target.to_string(),
            elem,
        };
        Ok((insert, acc_ty, Expr::ListLit(Vec::new())))
    })
}

/// PMAT-502fd: two-generator dict comprehension `{k: v for x in a for y in b}`
/// → nested loops inserting into the accumulator. See [`desugar_comp_2gen`].
fn desugar_dict_comp_2gen(
    ctx: &mut LoweringCtx,
    target: &str,
    comp: &ast::ExprDictComp,
) -> Result<Vec<Stmt>, FrontendError> {
    desugar_comp_2gen(ctx, target, &comp.generators, "dict", |ctx| {
        let key = lower_expr_in_ctx(ctx, (*comp.key).clone())?;
        let value = lower_expr_in_ctx(ctx, (*comp.value).clone())?;
        let acc_ty = Type::Dict(
            Box::new(infer_type_in_ctx(ctx, &key)),
            Box::new(infer_type_in_ctx(ctx, &value)),
        );
        let insert = Stmt::DictSet {
            dict_name: target.to_string(),
            key,
            value,
        };
        Ok((insert, acc_ty, Expr::DictLit(Vec::new())))
    })
}

/// PMAT-502fd: two-generator set comprehension `{expr for x in a for y in b}`
/// → nested loops adding to the accumulator. See [`desugar_comp_2gen`].
fn desugar_set_comp_2gen(
    ctx: &mut LoweringCtx,
    target: &str,
    comp: &ast::ExprSetComp,
) -> Result<Vec<Stmt>, FrontendError> {
    desugar_comp_2gen(ctx, target, &comp.generators, "set", |ctx| {
        let elem = lower_expr_in_ctx(ctx, (*comp.elt).clone())?;
        let acc_ty = Type::Set(Box::new(infer_type_in_ctx(ctx, &elem)));
        let insert = Stmt::SetAdd {
            set_name: target.to_string(),
            elem,
        };
        Ok((insert, acc_ty, Expr::SetLit(Vec::new())))
    })
}

fn desugar_list_comp(
    ctx: &mut LoweringCtx,
    target: &str,
    comp: &ast::ExprListComp,
) -> Result<Vec<Stmt>, FrontendError> {
    // PMAT-502fc: two-generator list comprehension `[expr for x in a for y in b]`
    // → nested `for` loops appending to the accumulator. Both generators must
    // have plain-Name targets over list-typed iterables (range / tuple-target
    // multi-gen are deferred). Handled by a dedicated path so the single-gen
    // lowering below is untouched.
    if comp.generators.len() == 2 {
        return desugar_list_comp_2gen(ctx, target, comp);
    }
    if comp.generators.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses a list comprehension with {} `for` clauses — v0.2.0 supports one or two",
            ctx.fn_name,
            comp.generators.len()
        )));
    }
    let gen = &comp.generators[0];
    // PMAT-563: multiple `if` clauses are ANDed (see `combine_comp_filters`).
    // PMAT-502cg: tuple-target list comp `[f(k, v) for k, v in d.items()]`
    // → a `ForEachPair { Pairs }` loop appending to the accumulator (mirrors
    // the dict-comp tuple branch, PMAT-502cf). Iterable must type as
    // `list[tuple[K, V]]` (which `d.items()` yields).
    if let ast::Expr::Tuple(t) = &gen.target {
        let (first, second) = match (t.elts.first(), t.elts.get(1), t.elts.len()) {
            (Some(ast::Expr::Name(a)), Some(ast::Expr::Name(b)), 2) => {
                (a.id.to_string(), b.id.to_string())
            }
            _ => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` has a list-comprehension tuple target that isn't exactly two plain names — only `for k, v in …` is supported at v0.2.0",
                    ctx.fn_name
                )));
            }
        };
        let iter_expr = str_iter_to_chars(ctx, lower_expr_in_ctx(ctx, gen.iter.clone())?);
        let (k_in, v_in) = match infer_type_in_ctx(ctx, &iter_expr) {
            Type::List(elem) => match *elem {
                Type::Tuple(tys) if tys.len() == 2 => (tys[0].clone(), tys[1].clone()),
                other => {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` list-comprehends `for k, v in …` over a list whose element types as {other:?}; expected a list of 2-tuples (e.g. `d.items()`)",
                        ctx.fn_name
                    )));
                }
            },
            other => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` list-comprehends `for k, v in …` over a {other:?}; expected an iterable of 2-tuples (e.g. `d.items()`)",
                    ctx.fn_name
                )));
            }
        };
        ctx.bound.insert(first.clone());
        ctx.bound.insert(second.clone());
        ctx.name_types.insert(first.clone(), k_in);
        ctx.name_types.insert(second.clone(), v_in);
        let filter = comp_filter(ctx, gen, "list")?;
        let elem = lower_expr_in_ctx(ctx, (*comp.elt).clone())?;
        let list_ty = Type::List(Box::new(infer_type_in_ctx(ctx, &elem)));
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
        return Ok(vec![
            Stmt::Let {
                name: target.to_string(),
                ty: list_ty,
                value: Expr::ListLit(Vec::new()),
                mutable: true,
            },
            Stmt::ForEachPair {
                first,
                second,
                iter: iter_expr,
                kind: PairIterKind::Pairs,
                body,
            },
        ]);
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
        // PMAT-502dz: `[elem for _ in range(n)]` would desugar to `let mut _`
        // — mint a fresh counter name and register it for body reads of `_`.
        let (var, saved_rename) = ctx.enter_loop_var(&var);
        // The range counter is an `i64`; bind it before lowering elem/filter.
        ctx.bound.insert(var.clone());
        ctx.name_types.insert(var.clone(), Type::I64);
        // PMAT-563: fold all `if` clauses into one Bool filter (ANDed).
        let filter = combine_comp_filters(ctx, &gen.ifs, "list")?;
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
        ctx.exit_loop_var(saved_rename);
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
    let iter_expr = str_iter_to_chars(ctx, lower_expr_in_ctx(ctx, gen.iter.clone())?);
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
    // PMAT-502ay / PMAT-563: lower the `if` filter(s), ANDed into one Bool.
    let filter = combine_comp_filters(ctx, &gen.ifs, "list")?;
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
    // PMAT-502fd: two-generator dict comprehension → nested `for` loops.
    if comp.generators.len() == 2 {
        return desugar_dict_comp_2gen(ctx, target, comp);
    }
    if comp.generators.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses a dict comprehension with {} `for` clauses — v0.2.0 supports one or two",
            ctx.fn_name,
            comp.generators.len()
        )));
    }
    let gen = &comp.generators[0];
    // PMAT-563: multiple `if` clauses are ANDed (see `combine_comp_filters`).
    // PMAT-502cf: tuple-target dict comp `{k: f(v) for k, v in d.items()}`
    // → a `ForEachPair { Pairs }` loop building the dict (mirrors the
    // `for k, v in d.items()` statement form, PMAT-502y). The iterable must
    // type as a `list[tuple[K, V]]` (which `d.items()` yields).
    if let ast::Expr::Tuple(t) = &gen.target {
        let (first, second) = match (t.elts.first(), t.elts.get(1), t.elts.len()) {
            (Some(ast::Expr::Name(a)), Some(ast::Expr::Name(b)), 2) => {
                (a.id.to_string(), b.id.to_string())
            }
            _ => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` has a dict-comprehension tuple target that isn't exactly two plain names — only `for k, v in …` is supported at v0.2.0",
                    ctx.fn_name
                )));
            }
        };
        let iter_expr = str_iter_to_chars(ctx, lower_expr_in_ctx(ctx, gen.iter.clone())?);
        let (k_in, v_in) = match infer_type_in_ctx(ctx, &iter_expr) {
            Type::List(elem) => match *elem {
                Type::Tuple(tys) if tys.len() == 2 => (tys[0].clone(), tys[1].clone()),
                other => {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` dict-comprehends `for k, v in …` over a list whose element types as {other:?}; expected a list of 2-tuples (e.g. `d.items()`)",
                        ctx.fn_name
                    )));
                }
            },
            other => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` dict-comprehends `for k, v in …` over a {other:?}; expected an iterable of 2-tuples (e.g. `d.items()`)",
                    ctx.fn_name
                )));
            }
        };
        ctx.bound.insert(first.clone());
        ctx.bound.insert(second.clone());
        ctx.name_types.insert(first.clone(), k_in);
        ctx.name_types.insert(second.clone(), v_in);
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
        let body = match filter {
            None => vec![insert],
            Some(cond) => vec![Stmt::If {
                cond,
                then_body: vec![insert],
                else_body: Vec::new(),
            }],
        };
        return Ok(vec![
            Stmt::Let {
                name: target.to_string(),
                ty: dict_ty,
                value: Expr::DictLit(Vec::new()),
                mutable: true,
            },
            Stmt::ForEachPair {
                first,
                second,
                iter: iter_expr,
                kind: PairIterKind::Pairs,
                body,
            },
        ]);
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
        // PMAT-502dz: `{k: v for _ in range(n)}` — mint a fresh counter name
        // for the `_` target and register it for body reads of `_`.
        let (var, saved_rename) = ctx.enter_loop_var(&var);
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
        ctx.exit_loop_var(saved_rename);
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
    let iter_expr = str_iter_to_chars(ctx, lower_expr_in_ctx(ctx, gen.iter.clone())?);
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
    combine_comp_filters(ctx, &gen.ifs, kind)
}

/// PMAT-563: combine a comprehension generator's `if` clauses into a single Bool
/// filter. Python ANDs multiple `if`s (`[x for x in xs if a if b]` ==
/// `… if a and b`). `None` when there are no clauses; each must type as Bool.
/// The loop var(s) must already be bound in `ctx` so each clause types correctly.
fn combine_comp_filters(
    ctx: &LoweringCtx,
    ifs: &[ast::Expr],
    kind: &str,
) -> Result<Option<Expr>, FrontendError> {
    let mut acc: Option<Expr> = None;
    for cond_ast in ifs {
        let cond = lower_expr_in_ctx(ctx, cond_ast.clone())?;
        if infer_type_in_ctx(ctx, &cond) != Type::Bool {
            return Err(FrontendError::Lower(format!(
                "function `{}` has a {kind}-comprehension filter that is not Bool (no int-truthiness at v0.2.0)",
                ctx.fn_name
            )));
        }
        acc = Some(match acc {
            None => cond,
            Some(prev) => Expr::BinOp {
                op: BinOp::And,
                lhs: Box::new(prev),
                rhs: Box::new(cond),
            },
        });
    }
    Ok(acc)
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
    // PMAT-502fd: two-generator set comprehension → nested `for` loops.
    if comp.generators.len() == 2 {
        return desugar_set_comp_2gen(ctx, target, comp);
    }
    if comp.generators.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses a set comprehension with {} `for` clauses — v0.2.0 supports one or two",
            ctx.fn_name,
            comp.generators.len()
        )));
    }
    let gen = &comp.generators[0];
    // PMAT-563: multiple `if` clauses are ANDed (see `combine_comp_filters`).
    // PMAT-502cg: tuple-target set comp `{f(k, v) for k, v in d.items()}`
    // → a `ForEachPair { Pairs }` loop adding to the accumulator (mirrors the
    // dict/list-comp tuple branches). Iterable must type `list[tuple[K, V]]`.
    if let ast::Expr::Tuple(t) = &gen.target {
        let (first, second) = match (t.elts.first(), t.elts.get(1), t.elts.len()) {
            (Some(ast::Expr::Name(a)), Some(ast::Expr::Name(b)), 2) => {
                (a.id.to_string(), b.id.to_string())
            }
            _ => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` has a set-comprehension tuple target that isn't exactly two plain names — only `for k, v in …` is supported at v0.2.0",
                    ctx.fn_name
                )));
            }
        };
        let iter_expr = str_iter_to_chars(ctx, lower_expr_in_ctx(ctx, gen.iter.clone())?);
        let (k_in, v_in) = match infer_type_in_ctx(ctx, &iter_expr) {
            Type::List(elem) => match *elem {
                Type::Tuple(tys) if tys.len() == 2 => (tys[0].clone(), tys[1].clone()),
                other => {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` set-comprehends `for k, v in …` over a list whose element types as {other:?}; expected a list of 2-tuples (e.g. `d.items()`)",
                        ctx.fn_name
                    )));
                }
            },
            other => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` set-comprehends `for k, v in …` over a {other:?}; expected an iterable of 2-tuples (e.g. `d.items()`)",
                    ctx.fn_name
                )));
            }
        };
        ctx.bound.insert(first.clone());
        ctx.bound.insert(second.clone());
        ctx.name_types.insert(first.clone(), k_in);
        ctx.name_types.insert(second.clone(), v_in);
        let filter = comp_filter(ctx, gen, "set")?;
        let elem = lower_expr_in_ctx(ctx, (*comp.elt).clone())?;
        let set_ty = Type::Set(Box::new(infer_type_in_ctx(ctx, &elem)));
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
        return Ok(vec![
            Stmt::Let {
                name: target.to_string(),
                ty: set_ty,
                value: Expr::SetLit(Vec::new()),
                mutable: true,
            },
            Stmt::ForEachPair {
                first,
                second,
                iter: iter_expr,
                kind: PairIterKind::Pairs,
                body,
            },
        ]);
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
        // PMAT-502dz: `{e for _ in range(n)}` — mint a fresh counter name for
        // the `_` target and register it for body reads of `_`.
        let (var, saved_rename) = ctx.enter_loop_var(&var);
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
        ctx.exit_loop_var(saved_rename);
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
    let iter_expr = str_iter_to_chars(ctx, lower_expr_in_ctx(ctx, gen.iter.clone())?);
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
/// PMAT-502dr: lower a nested `def inner(p: T, …) -> R: return <expr>` to a
/// [`Stmt::ClosureLet`] (a Rust closure `let inner = |p: T, …| { <expr> };`),
/// reusing the closure machinery. Unlike the lambda path, the parameters carry
/// their *annotated* types (default `int`), and the return type comes from the
/// `-> R` annotation (else inferred). First cut: the body must be a single
/// `return <expr>` (multi-statement bodies need a block-expression and are
/// deferred); `*args`/`**kwargs`/keyword-only/pos-only params and decorators
/// are rejected. The closure captures enclosing locals (Rust closures capture
/// by default). Lean refuses `ClosureLet`.
fn desugar_nested_fn(
    ctx: &mut LoweringCtx,
    f: &ast::StmtFunctionDef,
) -> Result<Stmt, FrontendError> {
    if !f.decorator_list.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` has a nested function `{}` with decorators — not supported",
            ctx.fn_name, f.name
        )));
    }
    if !f.args.posonlyargs.is_empty()
        || !f.args.kwonlyargs.is_empty()
        || f.args.vararg.is_some()
        || f.args.kwarg.is_some()
    {
        return Err(FrontendError::Lower(format!(
            "nested function `{}` uses pos-only / keyword-only / *args / **kwargs — only plain positional parameters are supported",
            f.name
        )));
    }
    // First cut: the body must be exactly one `return <expr>` (a closure body
    // is a single expression — multi-statement nested functions are deferred).
    // PMAT-502dt: the body is zero or more leading statements followed by a
    // trailing `return <expr>` (the closure's value). A single-`return` body
    // stays a bare expression; a multi-statement body becomes an `Expr::Block`.
    if f.body.is_empty() {
        return Err(FrontendError::Lower(format!(
            "nested function `{}` has an empty body",
            f.name
        )));
    }
    let (last, leading) = f.body.split_last().expect("non-empty checked above");
    let trailing_ast = match last {
        ast::Stmt::Return(ret) => match ret.value.as_ref() {
            Some(v) => (**v).clone(),
            None => {
                return Err(FrontendError::Lower(format!(
                    "nested function `{}` ends with a bare `return` — only `return <expr>` is supported",
                    f.name
                )));
            }
        },
        _ => {
            return Err(FrontendError::Lower(format!(
                "nested function `{}` must end with `return <expr>`",
                f.name
            )));
        }
    };
    let fname = ctx.fn_name.clone();
    let mut params: Vec<(String, Type)> = Vec::with_capacity(f.args.args.len());
    for arg in &f.args.args {
        let pname = arg.def.arg.to_string();
        let ty = match arg.def.annotation.as_ref() {
            None => Type::I64,
            Some(ann) => parse_type_annotation(&fname, &pname, ann)?,
        };
        params.push((pname, ty));
    }
    // Snapshot the enclosing scope so the closure's params + body-locals don't
    // leak into the enclosing function's type environment (Rust scopes them in
    // the closure block; the leak would only mislead later type inference).
    let saved_bound = ctx.bound.clone();
    let saved_types = ctx.name_types.clone();
    for (p, t) in &params {
        ctx.bound.insert(p.clone());
        ctx.name_types.insert(p.clone(), t.clone());
    }
    // Lower the leading statements, then the trailing return expression.
    let mut stmts: Vec<Stmt> = Vec::new();
    for s in leading {
        stmts.extend(lower_block_stmt(ctx, s.clone())?);
    }
    let trailing = lower_expr_in_ctx(ctx, trailing_ast)?;
    // Return type: the `-> R` annotation, else inferred from the trailing expr.
    let ret_ty = match f.returns.as_ref() {
        Some(ann) => parse_type_annotation(&fname, "<return>", ann)?,
        None => infer_type_in_ctx(ctx, &trailing),
    };
    ctx.bound = saved_bound;
    ctx.name_types = saved_types;
    let body = if stmts.is_empty() {
        trailing
    } else {
        Expr::Block(Box::new(Block {
            stmts,
            trailing_return: trailing,
        }))
    };
    let name = f.name.to_string();
    ctx.closure_returns.insert(name.clone(), ret_ty);
    ctx.bound.insert(name.clone());
    Ok(Stmt::ClosureLet { name, params, body })
}

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
    if call.keywords.iter().any(|k| k.arg.is_none()) {
        return Err(FrontendError::Lower(format!(
            "function `{}` uses `**kwargs` unpacking in a call — not supported",
            ctx.fn_name
        )));
    }
    // PMAT-502ct: a call to a known top-level function may omit trailing
    // arguments that have defaults; such a call must still be normalised even
    // with no keywords. A call with no keywords to an unknown callee (builtin,
    // method, …) or one already at full arity is left untouched.
    let callee_name = match call.func.as_ref() {
        ast::Expr::Name(n) => Some(n.id.to_string()),
        _ => None,
    };
    let sig = callee_name.as_ref().and_then(|c| ctx.signatures.get(c));
    if call.keywords.is_empty() {
        match sig {
            Some(s) if call.args.len() < s.params.len() => { /* fill defaults below */ }
            _ => return Ok(call),
        }
    }
    let callee = callee_name.ok_or_else(|| {
        FrontendError::Lower(format!(
            "function `{}` passes keyword args to a non-Name callee — only `f(x=…)` to a top-level function is supported",
            ctx.fn_name
        ))
    })?;
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
    // Reject any keyword that doesn't name a parameter in the still-unfilled
    // tail (unknown name, or one already supplied positionally).
    for k in &call.keywords {
        let name = k.arg.as_ref().map(|a| a.as_str()).unwrap_or("");
        if !sig.params[n_pos..].iter().any(|p| p == name) {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `{callee}` with keyword `{name}` naming an unknown parameter or one already filled positionally",
                ctx.fn_name
            )));
        }
    }
    // Positional args fill params[0..n_pos]; each remaining param is filled
    // from its matching keyword, else its default value (PMAT-502ct), else
    // it is a genuine missing-argument error.
    let mut new_args = call.args.clone();
    for (offset, pname) in sig.params[n_pos..].iter().enumerate() {
        let param_idx = n_pos + offset;
        if let Some(k) = call
            .keywords
            .iter()
            .find(|k| k.arg.as_ref().map(|a| a.as_str()) == Some(pname.as_str()))
        {
            new_args.push(k.value.clone());
        } else if let Some(default) = sig.defaults.get(param_idx).and_then(|d| d.clone()) {
            new_args.push(default);
        } else {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `{callee}` missing argument `{pname}` (no value and no default)",
                ctx.fn_name
            )));
        }
    }
    Ok(ast::ExprCall {
        range: call.range,
        func: call.func,
        args: new_args,
        keywords: Vec::new(),
    })
}

/// PMAT-627: fill default arguments in NESTED user-function calls that appear
/// directly in argument position (`f(g(x))` where `g` has a default). The
/// outer call is reordered by [`reorder_kwargs_to_positional`], but it then
/// lowers via the context-free `lower_call` (whose args use `lower_expr`, not
/// `lower_expr_in_ctx`), so a nested call's defaults were never filled → E0061.
/// Recurse through any `Call`-shaped argument, reordering + filling each.
fn reorder_nested_call_args(
    ctx: &LoweringCtx,
    mut call: ast::ExprCall,
) -> Result<ast::ExprCall, FrontendError> {
    let mut new_args = Vec::with_capacity(call.args.len());
    for arg in call.args.into_iter() {
        match arg {
            ast::Expr::Call(inner) => {
                let inner = reorder_kwargs_to_positional(ctx, inner)?;
                let inner = reorder_nested_call_args(ctx, inner)?;
                new_args.push(ast::Expr::Call(inner));
            }
            other => new_args.push(other),
        }
    }
    call.args = new_args;
    Ok(call)
}

/// PMAT-466 (v0.2.0 Track 1.C): lower an annotated local assignment
/// `name: T = value`. The annotation is authoritative for the
/// binding's type — notably, an annotated empty dict
/// `counts: dict[K, V] = {}` lowers to `DictLit(vec![])` typed by the
/// annotation, the only way to introduce an empty dict (the value
/// alone can't infer K/V). Non-empty / non-dict values are lowered
/// through the context-aware path and must agree with the annotation.
/// PMAT-502ec: lower a value that may be an empty `[]` / `{}` literal needing
/// an `expected` type to fix its element / key-value types. An empty literal
/// can't self-infer (`infer_type` defaults `[]` to `list[int]` and `{}` is
/// ambiguous), so when `value` is an empty list/dict and `expected` is the
/// matching collection type we emit the empty literal and let the binding's /
/// return's declared type carry the element types. Any other value (including
/// a non-empty literal, or an empty literal whose `expected` type doesn't
/// match) falls through to the normal context-aware lowering.
fn lower_value_expecting(
    ctx: &LoweringCtx,
    value: &ast::Expr,
    expected: &Type,
) -> Result<Expr, FrontendError> {
    match value {
        ast::Expr::List(l) if l.elts.is_empty() && matches!(expected, Type::List(_)) => {
            Ok(Expr::ListLit(Vec::new()))
        }
        ast::Expr::Dict(d) if d.keys.is_empty() && matches!(expected, Type::Dict(_, _)) => {
            Ok(Expr::DictLit(Vec::new()))
        }
        _ => lower_expr_in_ctx(ctx, value.clone()),
    }
}

/// PMAT-502ew: lower a returned value, wrapping it for an `Optional[T]` return
/// type — `return None` → `OptionExpr(None)`, `return <x>` → `OptionExpr(Some(x))`
/// (the body produces concrete `T` values; only the return site wraps). For a
/// non-Optional return type, falls back to [`lower_value_expecting`] (which also
/// threads empty `[]`/`{}` against the declared type).
fn lower_return_value(ctx: &LoweringCtx, value: &ast::Expr) -> Result<Expr, FrontendError> {
    if matches!(ctx.fn_return_type, Type::Optional(_)) {
        // A bare `None` return must NOT be lowered as a value (that errors —
        // `None` has no value-position support yet); produce `OptionExpr(None)`.
        if matches!(value, ast::Expr::Constant(c) if matches!(c.value, ast::Constant::None)) {
            return Ok(Expr::OptionExpr(None));
        }
        let inner = lower_expr_in_ctx(ctx, value.clone())?;
        // PMAT-502ey: a value that already types as `Optional` (an `Optional`
        // param, or `d.get(k)`) is passed through verbatim — wrapping it in
        // `Some(...)` would double-wrap into `Option<Option<T>>`.
        if matches!(infer_type_in_ctx(ctx, &inner), Type::Optional(_)) {
            return Ok(inner);
        }
        return Ok(Expr::OptionExpr(Some(Box::new(inner))));
    }
    lower_value_expecting(ctx, value, &ctx.fn_return_type)
}

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
    let value = match value_expr.as_ref() {
        ast::Expr::Dict(d) if d.keys.is_empty() => {
            if !matches!(declared_ty, Type::Dict(_, _)) {
                return Err(FrontendError::Lower(format!(
                    "function `{}` assigns empty `{{}}` to `{name}` annotated as {declared_ty:?}; an empty literal requires a `dict[K, V]` annotation",
                    ctx.fn_name
                )));
            }
            Expr::DictLit(Vec::new())
        }
        // PMAT-502ec: empty list literal — the annotation supplies the element
        // type the bare `[]` can't (mirrors the empty-dict case). Without this
        // the context-aware lowering rejects `[]` outright.
        ast::Expr::List(l) if l.elts.is_empty() => {
            if !matches!(declared_ty, Type::List(_)) {
                return Err(FrontendError::Lower(format!(
                    "function `{}` assigns empty `[]` to `{name}` annotated as {declared_ty:?}; an empty literal requires a `list[T]` annotation",
                    ctx.fn_name
                )));
            }
            Expr::ListLit(Vec::new())
        }
        _ => lower_expr_in_ctx(ctx, (*value_expr).clone())?,
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
    // PMAT-602: reject an annotation/Optional mismatch — a non-Optional
    // annotation (`x: int`) over an Optional-typed initializer (1-arg
    // `d.get(k)`, an Optional param) would emit `Option<T>` into a `T`
    // binding (rustc E0308). Python doesn't enforce annotations
    // (`x: int = d.get("z")` binds `None`), so unwrapping would diverge on
    // the None case — reject so transpile fails fast rather than emitting
    // invalid Rust. Use `Optional[T]` / `d.get(k, default)` instead.
    if !matches!(declared_ty, Type::Optional(_))
        && matches!(infer_type_in_ctx(ctx, &value), Type::Optional(_))
    {
        return Err(FrontendError::Lower(format!(
            "function `{}` annotates `{name}` as {declared_ty:?} but its initializer is Optional (e.g. 1-arg `d.get(k)`); use an `Optional[...]` annotation or `d.get(k, default)`",
            ctx.fn_name
        )));
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
        Stmt::IndexAssign { indices, value, .. } => {
            indices.iter().any(expr_uses_dict) || expr_uses_dict(value)
        }
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
        | Expr::DictGetOpt { .. }
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
        // PMAT-506b: a struct literal types as its named struct. Field access
        // can't resolve a field type without the struct registry, so the
        // context-free path falls back to I64 (the ctx path resolves it).
        Expr::StructLit { name, .. } => Type::Struct(name.clone()),
        Expr::FieldAccess { .. } => Type::I64,
        // PMAT-513: an enum member types as the enum (a named type).
        Expr::EnumVariant { enum_name, .. } => Type::Struct(enum_name.clone()),
        // PMAT-506d: context-free can't resolve a method's return type (no
        // registry) — fall back to I64; the ctx path resolves it.
        Expr::MethodCall { .. } => Type::I64,
        // PMAT-502dt: a block-expr types as its trailing expression.
        // PMAT-556: when the trailing expr is a block-local `Ident`, recover its
        // type from the block's own `Let` (the enclosing scope can't see it).
        Expr::Block(b) => block_result_type(b, infer_type),
        // PMAT-477 (R8): float literal + float arithmetic are Type::F64.
        Expr::LitFloat(_) | Expr::FloatBinOp { .. } => Type::F64,
        // PMAT-456 (v0.2.0 Track 1.B): bool literal is Type::Bool.
        Expr::LitBool(_) => Type::Bool,
        // PMAT-459 (v0.2.0 Track 1.B): len(x) always returns Type::I64
        // (Python int).
        Expr::Len(_) => Type::I64,
        Expr::BinOp { op, lhs, rhs } => match op {
            BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::FloorDiv
            | BinOp::Mod
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::Pow => Type::I64,
            // PMAT-580: `&`/`|`/`^` over two bools is a bool (Python); otherwise
            // an int. (Context-free counterpart of the `infer_type_in_ctx` arm.)
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                if infer_type(lhs) == Type::Bool && infer_type(rhs) == Type::Bool {
                    Type::Bool
                } else {
                    Type::I64
                }
            }
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
            // PMAT-502fb: bitwise invert yields I64.
            UnOp::BitNot => Type::I64,
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
        // PMAT-502cl: string chars as a list[str].
        Expr::StrChars { .. } => Type::List(Box::new(Type::Str)),
        // PMAT-502cm: ord → int code point; chr → 1-char str.
        Expr::Ord { .. } => Type::I64,
        Expr::Chr { .. } => Type::Str,
        // PMAT-502cv: hex/oct/bin → str.
        Expr::IntRadixStr { .. } => Type::Str,
        // PMAT-502da: int(s, base) → int.
        Expr::IntFromStrRadix { .. } => Type::I64,
        // PMAT-502am: a formatted f-string field produces a Str.
        Expr::FormatSpec { .. } => Type::Str,
        // PMAT-492: string transform methods (upper/lower/strip) → Str.
        Expr::StrMethod { op, .. } => match op {
            StrMethodOp::Upper | StrMethodOp::Lower | StrMethodOp::Strip => Type::Str,
            StrMethodOp::StartsWith | StrMethodOp::EndsWith => Type::Bool,
            StrMethodOp::Split | StrMethodOp::SplitN | StrMethodOp::SplitWhitespace => {
                Type::List(Box::new(Type::Str))
            }
            StrMethodOp::Join | StrMethodOp::Replace | StrMethodOp::ReplaceN => Type::Str,
            // PMAT-502l: lstrip/rstrip → Str; find/count → Int.
            StrMethodOp::LStrip | StrMethodOp::RStrip => Type::Str,
            StrMethodOp::Find
            | StrMethodOp::Rfind
            | StrMethodOp::RIndex
            | StrMethodOp::Count
            | StrMethodOp::CharCount
            | StrMethodOp::StrIndex => Type::I64,
            // PMAT-502ag/502di: isdigit/isalpha/isspace/isalnum/isupper/islower → Bool.
            StrMethodOp::IsDigit
            | StrMethodOp::IsAlpha
            | StrMethodOp::IsSpace
            | StrMethodOp::IsAlnum
            | StrMethodOp::IsUpper
            | StrMethodOp::IsLower => Type::Bool,
            // PMAT-502ah: capitalize → Str. PMAT-502aj: title → Str.
            StrMethodOp::Capitalize | StrMethodOp::Title => Type::Str,
            // PMAT-502aw: rjust/ljust → Str.
            StrMethodOp::RJust | StrMethodOp::LJust => Type::Str,
            // PMAT-502cq: removeprefix/removesuffix → Str.
            StrMethodOp::RemovePrefix | StrMethodOp::RemoveSuffix => Type::Str,
            // PMAT-502cr: swapcase → Str.
            StrMethodOp::SwapCase => Type::Str,
            // PMAT-502cs: zfill → Str.
            StrMethodOp::ZFill => Type::Str,
            // PMAT-502cu: center → Str.
            StrMethodOp::Center => Type::Str,
            // PMAT-502dj: partition/rpartition → (str, str, str).
            StrMethodOp::Partition | StrMethodOp::RPartition => {
                Type::Tuple(vec![Type::Str, Type::Str, Type::Str])
            }
            // PMAT-502dl: splitlines → list[str].
            StrMethodOp::SplitLines => Type::List(Box::new(Type::Str)),
            // PMAT-530: s[::-1] reverse-slice → Str.
            StrMethodOp::Reverse => Type::Str,
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
        // PMAT-502ep: set predicates yield Bool.
        Expr::SetPred { .. } => Type::Bool,
        // PMAT-502eq: a copy has the same type as the value it clones.
        Expr::Clone(inner) => infer_type(inner),
        // PMAT-502ew: `Some(e)` types as `Optional(typeof e)`; a bare `None`
        // has no payload to infer (defaults `I64`; the return-type check
        // tolerates this against any declared `Optional`).
        Expr::OptionExpr(inner) => Type::Optional(Box::new(
            inner.as_deref().map(infer_type).unwrap_or(Type::I64),
        )),
        // PMAT-502ex: a `None` test yields Bool.
        Expr::IsNone { .. } => Type::Bool,
        // PMAT-502ez: unwrap yields the inner type of the operand's Optional.
        Expr::OptionUnwrap(inner) => match infer_type(inner) {
            Type::Optional(t) => *t,
            other => other,
        },
        // PMAT-503b: try/except types as the body (handler matches it).
        Expr::TryCatch { body, .. } => infer_type(body),
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
        // PMAT-502ek: `sqrt` is always `float`; `floor`/`ceil` are `int`
        // (Python `math.floor`/`ceil` return an int); `abs`/`min`/`max` take
        // the first argument's type.
        Expr::NumBuiltin { op, args, .. } => match op {
            NumBuiltinOp::Sqrt
            | NumBuiltinOp::Sin
            | NumBuiltinOp::Cos
            | NumBuiltinOp::Tan
            | NumBuiltinOp::Exp
            | NumBuiltinOp::Ln
            | NumBuiltinOp::Log10
            | NumBuiltinOp::Log2 => Type::F64,
            NumBuiltinOp::Floor | NumBuiltinOp::Ceil | NumBuiltinOp::Trunc => Type::I64,
            _ => args.first().map(infer_type).unwrap_or(Type::I64),
        },
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
        Expr::ToStr { .. } | Expr::ReprStr { .. } => Type::Str,
        // PMAT-502ak: round(x) → Int.
        Expr::RoundToInt { .. } => Type::I64,
        // PMAT-502al: round(x, n) → Float.
        Expr::RoundToDigits { .. } => Type::F64,
        // PMAT-612: round(int, n) → Int.
        Expr::RoundIntToDigits { .. } => Type::I64,
        // PMAT-502k: seq * n has the same type as the sequence.
        Expr::Repeat { seq, .. } => infer_type(seq),
        // PMAT-502c: sorted(xs) has the same type as its list.
        Expr::Sorted { list, .. } => infer_type(list),
        // PMAT-502d: reversed(xs) has the same type as its list.
        Expr::Reversed { list } => infer_type(list),
        // PMAT-549: gcd of two ints -> int.
        Expr::Gcd { .. }
        | Expr::Lcm { .. }
        | Expr::Factorial { .. }
        | Expr::Isqrt { .. }
        | Expr::Comb { .. }
        | Expr::Perm { .. }
        | Expr::PowMod { .. } => Type::I64,
        // PMAT-502cj: list(range(...)) materialises a list[int].
        Expr::RangeList { .. } => Type::List(Box::new(Type::I64)),
        // PMAT-502cw: set(xs) → set over the list's element type.
        Expr::SetFromList { list } => match infer_type(list) {
            Type::List(elem) => Type::Set(elem),
            _ => Type::Set(Box::new(Type::I64)),
        },
        // PMAT-520: list(set) / sorted(set) → List over the set's element type.
        Expr::SetToList { set } => match infer_type(set) {
            Type::Set(elem) => Type::List(elem),
            _ => Type::List(Box::new(Type::I64)),
        },
        // PMAT-502dk: dict(pairs) → Dict(K, V) over the list's tuple[K, V].
        Expr::DictFromPairs { pairs } => match infer_type(pairs) {
            Type::List(elem) => match *elem {
                Type::Tuple(tys) if tys.len() == 2 => {
                    Type::Dict(Box::new(tys[0].clone()), Box::new(tys[1].clone()))
                }
                _ => Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
            },
            _ => Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
        },
        // PMAT-502dw/dx: dict merge types as the first entry's dict type
        // (a splat's dict, or `dict[typeof k, typeof v]` for an explicit pair).
        Expr::DictMerge { entries } => match entries.first() {
            Some((Some(k), v)) => Type::Dict(Box::new(infer_type(k)), Box::new(infer_type(v))),
            Some((None, d)) => infer_type(d),
            None => Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
        },
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
        // PMAT-502ey: 1-arg `d.get(k)` → `Optional[V]`.
        Expr::DictGetOpt { dict, .. } => match infer_type(dict) {
            Type::Dict(_, value_ty) => Type::Optional(value_ty),
            _ => Type::Optional(Box::new(Type::I64)),
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
        // PMAT-506b: a struct literal types as its named struct; a field read
        // resolves the field's type from the struct registry (fallback I64).
        Expr::StructLit { name, .. } => Type::Struct(name.clone()),
        // PMAT-513: an enum member types as the enum (a named type).
        Expr::EnumVariant { enum_name, .. } => Type::Struct(enum_name.clone()),
        Expr::FieldAccess { obj, field } => match infer_type_in_ctx(ctx, obj) {
            Type::Struct(name) => ctx
                .structs
                .get(&name)
                .and_then(|fs| fs.iter().find(|(f, _)| f == field))
                .map(|(_, ty)| ty.clone())
                .unwrap_or(Type::I64),
            _ => Type::I64,
        },
        // PMAT-506d: a method call types as the method's declared return type.
        Expr::MethodCall { obj, method, .. } => match infer_type_in_ctx(ctx, obj) {
            Type::Struct(name) => ctx
                .struct_methods
                .get(&name)
                .and_then(|ms| ms.iter().find(|(m, _)| m == method))
                .map(|(_, ty)| ty.clone())
                .unwrap_or(Type::I64),
            _ => Type::I64,
        },
        // PMAT-502dt: a block-expr types as its trailing expression.
        // PMAT-556: a block-local `Ident` trailing recovers its type from the
        // block's own `Let` (the enclosing ctx can't see the temp).
        Expr::Block(b) => block_result_type(b, |e| infer_type_in_ctx(ctx, e)),
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
                // PMAT-580: `&`/`|`/`^` over two bools is a bool in Python
                // (`True & False` is `bool`, not `int`); Rust's `bool: BitAnd`
                // matches. Without this the result inferred as I64 and a
                // `-> bool` function was rejected ("body produces I64").
                if matches!(op, BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor)
                    && lt == Type::Bool
                    && rt == Type::Bool
                {
                    Type::Bool
                } else if matches!(lt, Type::BigInt) || matches!(rt, Type::BigInt) {
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
            // PMAT-502fb: bitwise invert yields I64.
            UnOp::BitNot => Type::I64,
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
        // PMAT-502cl: string chars as a list[str].
        Expr::StrChars { .. } => Type::List(Box::new(Type::Str)),
        // PMAT-502cm: ord → int code point; chr → 1-char str.
        Expr::Ord { .. } => Type::I64,
        Expr::Chr { .. } => Type::Str,
        // PMAT-502cv: hex/oct/bin → str.
        Expr::IntRadixStr { .. } => Type::Str,
        // PMAT-502da: int(s, base) → int.
        Expr::IntFromStrRadix { .. } => Type::I64,
        // PMAT-502am: a formatted f-string field produces a Str.
        Expr::FormatSpec { .. } => Type::Str,
        // PMAT-492: string transform methods (upper/lower/strip) → Str.
        Expr::StrMethod { op, .. } => match op {
            StrMethodOp::Upper | StrMethodOp::Lower | StrMethodOp::Strip => Type::Str,
            StrMethodOp::StartsWith | StrMethodOp::EndsWith => Type::Bool,
            StrMethodOp::Split | StrMethodOp::SplitN | StrMethodOp::SplitWhitespace => {
                Type::List(Box::new(Type::Str))
            }
            StrMethodOp::Join | StrMethodOp::Replace | StrMethodOp::ReplaceN => Type::Str,
            // PMAT-502l: lstrip/rstrip → Str; find/count → Int.
            StrMethodOp::LStrip | StrMethodOp::RStrip => Type::Str,
            StrMethodOp::Find
            | StrMethodOp::Rfind
            | StrMethodOp::RIndex
            | StrMethodOp::Count
            | StrMethodOp::CharCount
            | StrMethodOp::StrIndex => Type::I64,
            // PMAT-502ag/502di: isdigit/isalpha/isspace/isalnum/isupper/islower → Bool.
            StrMethodOp::IsDigit
            | StrMethodOp::IsAlpha
            | StrMethodOp::IsSpace
            | StrMethodOp::IsAlnum
            | StrMethodOp::IsUpper
            | StrMethodOp::IsLower => Type::Bool,
            // PMAT-502ah: capitalize → Str. PMAT-502aj: title → Str.
            StrMethodOp::Capitalize | StrMethodOp::Title => Type::Str,
            // PMAT-502aw: rjust/ljust → Str.
            StrMethodOp::RJust | StrMethodOp::LJust => Type::Str,
            // PMAT-502cq: removeprefix/removesuffix → Str.
            StrMethodOp::RemovePrefix | StrMethodOp::RemoveSuffix => Type::Str,
            // PMAT-502cr: swapcase → Str.
            StrMethodOp::SwapCase => Type::Str,
            // PMAT-502cs: zfill → Str.
            StrMethodOp::ZFill => Type::Str,
            // PMAT-502cu: center → Str.
            StrMethodOp::Center => Type::Str,
            // PMAT-502dj: partition/rpartition → (str, str, str).
            StrMethodOp::Partition | StrMethodOp::RPartition => {
                Type::Tuple(vec![Type::Str, Type::Str, Type::Str])
            }
            // PMAT-502dl: splitlines → list[str].
            StrMethodOp::SplitLines => Type::List(Box::new(Type::Str)),
            // PMAT-530: s[::-1] reverse-slice → Str.
            StrMethodOp::Reverse => Type::Str,
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
        // PMAT-502ep: set predicates yield Bool.
        Expr::SetPred { .. } => Type::Bool,
        // PMAT-502eq: a copy has the same type as the value it clones.
        Expr::Clone(inner) => infer_type_in_ctx(ctx, inner),
        // PMAT-502ew: `Some(e)` → `Optional(typeof e)`; bare `None` defaults
        // I64 (return-type check tolerates it against any declared Optional).
        Expr::OptionExpr(inner) => Type::Optional(Box::new(
            inner
                .as_deref()
                .map(|e| infer_type_in_ctx(ctx, e))
                .unwrap_or(Type::I64),
        )),
        // PMAT-502ex: a `None` test yields Bool.
        Expr::IsNone { .. } => Type::Bool,
        // PMAT-502ez: unwrap yields the inner type of the operand's Optional.
        Expr::OptionUnwrap(inner) => match infer_type_in_ctx(ctx, inner) {
            Type::Optional(t) => *t,
            other => other,
        },
        // PMAT-503b: try/except types as the body (handler matches it).
        Expr::TryCatch { body, .. } => infer_type_in_ctx(ctx, body),
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
        // PMAT-502ek: see the context-free twin — op-specific return type.
        Expr::NumBuiltin { op, args, .. } => match op {
            NumBuiltinOp::Sqrt
            | NumBuiltinOp::Sin
            | NumBuiltinOp::Cos
            | NumBuiltinOp::Tan
            | NumBuiltinOp::Exp
            | NumBuiltinOp::Ln
            | NumBuiltinOp::Log10
            | NumBuiltinOp::Log2 => Type::F64,
            NumBuiltinOp::Floor | NumBuiltinOp::Ceil | NumBuiltinOp::Trunc => Type::I64,
            _ => args
                .first()
                .map(|a| infer_type_in_ctx(ctx, a))
                .unwrap_or(Type::I64),
        },
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
        Expr::ToStr { .. } | Expr::ReprStr { .. } => Type::Str,
        // PMAT-502ak: round(x) → Int.
        Expr::RoundToInt { .. } => Type::I64,
        // PMAT-502al: round(x, n) → Float.
        Expr::RoundToDigits { .. } => Type::F64,
        // PMAT-612: round(int, n) → Int.
        Expr::RoundIntToDigits { .. } => Type::I64,
        // PMAT-502k: seq * n has the same type as the sequence.
        Expr::Repeat { seq, .. } => infer_type_in_ctx(ctx, seq),
        // PMAT-502c: sorted(xs) has the same type as its list.
        Expr::Sorted { list, .. } => infer_type_in_ctx(ctx, list),
        // PMAT-502d: reversed(xs) has the same type as its list.
        Expr::Reversed { list } => infer_type_in_ctx(ctx, list),
        // PMAT-549: gcd of two ints -> int.
        Expr::Gcd { .. }
        | Expr::Lcm { .. }
        | Expr::Factorial { .. }
        | Expr::Isqrt { .. }
        | Expr::Comb { .. }
        | Expr::Perm { .. }
        | Expr::PowMod { .. } => Type::I64,
        // PMAT-502cj: list(range(...)) materialises a list[int].
        Expr::RangeList { .. } => Type::List(Box::new(Type::I64)),
        // PMAT-502cw: set(xs) → set over the list's element type.
        Expr::SetFromList { list } => match infer_type_in_ctx(ctx, list) {
            Type::List(elem) => Type::Set(elem),
            _ => Type::Set(Box::new(Type::I64)),
        },
        // PMAT-520: list(set) / sorted(set) → List over the set's element type.
        Expr::SetToList { set } => match infer_type_in_ctx(ctx, set) {
            Type::Set(elem) => Type::List(elem),
            _ => Type::List(Box::new(Type::I64)),
        },
        // PMAT-502dk: dict(pairs) → Dict(K, V) over the list's tuple[K, V].
        Expr::DictFromPairs { pairs } => match infer_type_in_ctx(ctx, pairs) {
            Type::List(elem) => match *elem {
                Type::Tuple(tys) if tys.len() == 2 => {
                    Type::Dict(Box::new(tys[0].clone()), Box::new(tys[1].clone()))
                }
                _ => Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
            },
            _ => Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
        },
        // PMAT-502dw/dx: dict merge types as the first entry's dict type.
        Expr::DictMerge { entries } => match entries.first() {
            Some((Some(k), v)) => Type::Dict(
                Box::new(infer_type_in_ctx(ctx, k)),
                Box::new(infer_type_in_ctx(ctx, v)),
            ),
            Some((None, d)) => infer_type_in_ctx(ctx, d),
            None => Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
        },
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
        // PMAT-502ey: 1-arg `d.get(k)` → `Optional[V]`.
        Expr::DictGetOpt { dict, .. } => match infer_type_in_ctx(ctx, dict) {
            Type::Dict(_, value_ty) => Type::Optional(value_ty),
            _ => Type::Optional(Box::new(Type::I64)),
        },
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
        Expr::DictGetOpt { dict, key } => Expr::DictGetOpt {
            dict: rwb(dict),
            key: rwb(key),
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
            // PMAT-502de: general list index — lower the index context-aware
            // so a builtin index (`xs[abs(i)]`, `xs[max(0, i)]`) is recognized;
            // the context-free `lower_expr` would emit an undefined `abs(...)`.
            // (Dict / str / tuple / negative-literal indices returned above.)
            let index = lower_expr_in_ctx(ctx, (*sub.slice).clone())?;
            let idx_ty = infer_type_in_ctx(ctx, &index);
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
        // `d.get(k, default)` → `Expr::DictGetOr` when `d` is a dict.
        ast::Expr::Call(call) => {
            if let ast::Expr::Attribute(attr) = call.func.as_ref() {
                // PMAT-502ek: `math.<fn>(...)` module functions (`import math`
                // is accepted + skipped at the top level). The receiver is the
                // bare module name `math`, not a value.
                if let ast::Expr::Name(recv) = attr.value.as_ref() {
                    if recv.id.as_str() == "math" {
                        return lower_math_call(ctx, attr.attr.as_str(), &call);
                    }
                    // PMAT-506g/506h (classes epic): a `@staticmethod` /
                    // `@classmethod` call `Class.method(args)` — the receiver is
                    // a class *name* (a known struct), not an instance value
                    // (PMAT-506h: `cls.method(args)` inside a classmethod body
                    // resolves `cls` to the enclosing class). Lower to
                    // `Expr::Call { callee: "Class::method", args }` (registered
                    // in the module pre-pass under the qualified key). Must run
                    // BEFORE the instance-method block below, which would
                    // otherwise try to lower the bare class name as a value.
                    let class = ctx.resolve_class_name(recv.id.as_str());
                    if ctx.structs.contains_key(class) {
                        let method = attr.attr.as_str();
                        let key = format!("{class}::{method}");
                        if ctx.signatures.contains_key(&key) {
                            if !call.keywords.is_empty() {
                                return Err(FrontendError::Lower(format!(
                                    "function `{}` calls static method `{key}` with keyword args — v0.2.0 first cut supports positional calls only",
                                    ctx.fn_name
                                )));
                            }
                            let mut args = Vec::with_capacity(call.args.len());
                            for a in &call.args {
                                args.push(lower_expr_in_ctx(ctx, a.clone())?);
                            }
                            return Ok(Expr::Call { callee: key, args });
                        }
                        return Err(FrontendError::Lower(format!(
                            "function `{}` calls `{class}.{method}()`, which is not a `@staticmethod`/`@classmethod` of `{class}` — call an instance method on an instance, not the class",
                            ctx.fn_name
                        )));
                    }
                }
                // PMAT-506d (classes epic): struct method call `obj.method(args)`
                // over a struct-typed receiver → `Expr::MethodCall`. The type
                // check disambiguates from the list/dict/str/set method paths
                // below (those receivers are not `Type::Struct`).
                {
                    let recv = lower_expr_in_ctx(ctx, (*attr.value).clone())?;
                    if let Type::Struct(sname) = infer_type_in_ctx(ctx, &recv) {
                        let method = attr.attr.to_string();
                        let known = ctx
                            .struct_methods
                            .get(&sname)
                            .is_some_and(|ms| ms.iter().any(|(m, _)| *m == method));
                        if !known {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` calls `.{method}()` on `{sname}`, which has no such method",
                                ctx.fn_name
                            )));
                        }
                        if !call.keywords.is_empty() {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` calls method `{sname}.{method}` with keyword args — v0.2.0 first cut supports positional method calls only",
                                ctx.fn_name
                            )));
                        }
                        let mut args = Vec::with_capacity(call.args.len());
                        for a in &call.args {
                            args.push(lower_expr_in_ctx(ctx, a.clone())?);
                        }
                        return Ok(Expr::MethodCall {
                            obj: Box::new(recv),
                            method,
                            args,
                        });
                    }
                }
                // PMAT-502eo: set-algebra methods — `a.union(b)` /
                // `a.intersection(b)` / `a.difference(b)` /
                // `a.symmetric_difference(b)` are the method forms of
                // `|`/`&`/`-`/`^`, reusing `Expr::SetOp`. Both receiver and
                // argument must be sets; otherwise fall through.
                if let Some(sop) = set_method_op(attr.attr.as_str()) {
                    let recv = lower_expr_in_ctx(ctx, (*attr.value).clone())?;
                    if matches!(infer_type_in_ctx(ctx, &recv), Type::Set(_)) {
                        if !call.keywords.is_empty() || call.args.len() != 1 {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` calls set `.{}(...)` with {} positional arg(s){}; v0.2.0 takes exactly 1 (a set)",
                                ctx.fn_name,
                                attr.attr.as_str(),
                                call.args.len(),
                                if call.keywords.is_empty() { "" } else { " plus keyword args" },
                            )));
                        }
                        let other = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                        if !matches!(infer_type_in_ctx(ctx, &other), Type::Set(_)) {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` calls set `.{}(...)` with a non-set argument",
                                ctx.fn_name,
                                attr.attr.as_str(),
                            )));
                        }
                        return Ok(Expr::SetOp {
                            lhs: Box::new(recv),
                            op: sop,
                            rhs: Box::new(other),
                        });
                    }
                }
                // PMAT-502ep: set predicate methods — `a.issubset(b)` /
                // `a.issuperset(b)` / `a.isdisjoint(b)` → `Expr::SetPred`
                // (bool). Both receiver and argument must be sets.
                if let Some(pop) = set_pred_method(attr.attr.as_str()) {
                    let recv = lower_expr_in_ctx(ctx, (*attr.value).clone())?;
                    if matches!(infer_type_in_ctx(ctx, &recv), Type::Set(_)) {
                        if !call.keywords.is_empty() || call.args.len() != 1 {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` calls set `.{}(...)` with {} positional arg(s){}; v0.2.0 takes exactly 1 (a set)",
                                ctx.fn_name,
                                attr.attr.as_str(),
                                call.args.len(),
                                if call.keywords.is_empty() { "" } else { " plus keyword args" },
                            )));
                        }
                        let other = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                        if !matches!(infer_type_in_ctx(ctx, &other), Type::Set(_)) {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` calls set `.{}(...)` with a non-set argument",
                                ctx.fn_name,
                                attr.attr.as_str(),
                            )));
                        }
                        return Ok(Expr::SetPred {
                            lhs: Box::new(recv),
                            op: pop,
                            rhs: Box::new(other),
                        });
                    }
                }
                // PMAT-502eq: `xs.copy()` / `d.copy()` / `s.copy()` — a shallow
                // copy of a list / dict / set (0 args) → `Expr::Clone`.
                if attr.attr.as_str() == "copy" && call.args.is_empty() && call.keywords.is_empty()
                {
                    let recv = lower_expr_in_ctx(ctx, (*attr.value).clone())?;
                    if matches!(
                        infer_type_in_ctx(ctx, &recv),
                        Type::List(_) | Type::Dict(_, _) | Type::Set(_)
                    ) {
                        return Ok(Expr::Clone(Box::new(recv)));
                    }
                }
                if attr.attr.as_str() == "get" {
                    let recv = lower_expr_in_ctx(ctx, (*attr.value).clone())?;
                    if matches!(infer_type_in_ctx(ctx, &recv), Type::Dict(_, _)) {
                        if !call.keywords.is_empty()
                            || (call.args.len() != 1 && call.args.len() != 2)
                        {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` calls dict `.get(...)` with {} positional arg(s){} \
                                 — supports `.get(key)` → Optional[V] or `.get(key, default)`",
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
                        // PMAT-502ey: 1-arg `d.get(k)` → `Option<V>` (no default).
                        if call.args.len() == 1 {
                            return Ok(Expr::DictGetOpt {
                                dict: Box::new(recv),
                                key: Box::new(key),
                            });
                        }
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
                            // PMAT-502bh/cb/ch: automatic `{}` / positional `{N}`
                            // fields, each with an optional spec `{:.2f}`.
                            return lower_str_format(ctx, fmt, &call.args);
                        }
                    }
                }
                // PMAT-536: `"<fmt>".format(name=val, …)` — keyword (named) field
                // form. Named `{name}` placeholders are rewritten to positional
                // `{N}` (in first-occurrence order, repeats allowed) and the
                // matching kwarg values passed positionally to `lower_str_format`,
                // reusing all its spec translation / validation. Pure-keyword form
                // only (mixed positional+keyword and `**kwargs` are deferred).
                if attr.attr.as_str() == "format"
                    && call.args.is_empty()
                    && !call.keywords.is_empty()
                {
                    if let ast::Expr::Constant(c) = attr.value.as_ref() {
                        if let ast::Constant::Str(fmt) = &c.value {
                            return lower_str_format_kwargs(ctx, fmt, &call.keywords);
                        }
                    }
                }
                // PMAT-502co: `s.split()` (no arg) → whitespace split. Checked
                // before the generic dispatch (which maps "split" → the 1-arg
                // `Split`); the 1-arg `s.split(sep)` form is handled there.
                if attr.attr.as_str() == "split" && call.args.is_empty() && call.keywords.is_empty()
                {
                    let recv = lower_expr_in_ctx(ctx, (*attr.value).clone())?;
                    if matches!(infer_type_in_ctx(ctx, &recv), Type::Str) {
                        return Ok(Expr::StrMethod {
                            recv: Box::new(recv),
                            op: StrMethodOp::SplitWhitespace,
                            args: Vec::new(),
                        });
                    }
                }
                // PMAT-516 (correctness): `s.startswith((a, b, …))` /
                // `.endswith((…))` — Python accepts a TUPLE of prefixes/suffixes
                // (true if ANY matches). Rust's `str::starts_with` takes a single
                // pattern, so a tuple arg previously emitted `…starts_with(&(a,
                // b)[..])` — transpile-success-but-invalid-Rust. Expand to an OR
                // of per-prefix checks. (A single non-tuple arg falls through to
                // the generic 1-arg path below.)
                if matches!(attr.attr.as_str(), "startswith" | "endswith") {
                    if let Some(ast::Expr::Tuple(tup)) = call.args.first() {
                        if call.args.len() == 1 && call.keywords.is_empty() {
                            let recv = lower_expr_in_ctx(ctx, (*attr.value).clone())?;
                            if matches!(infer_type_in_ctx(ctx, &recv), Type::Str) {
                                let op = str_method_op(attr.attr.as_str())
                                    .expect("startswith/endswith map to a StrMethodOp");
                                // Python: an empty tuple of prefixes is always False.
                                if tup.elts.is_empty() {
                                    return Ok(Expr::LitBool(false));
                                }
                                let mut acc: Option<Expr> = None;
                                for elt in &tup.elts {
                                    let prefix = lower_expr_in_ctx(ctx, elt.clone())?;
                                    let check = Expr::StrMethod {
                                        recv: Box::new(recv.clone()),
                                        op,
                                        args: vec![prefix],
                                    };
                                    acc = Some(match acc {
                                        None => check,
                                        Some(prev) => Expr::BinOp {
                                            op: BinOp::Or,
                                            lhs: Box::new(prev),
                                            rhs: Box::new(check),
                                        },
                                    });
                                }
                                return Ok(acc.expect("tuple is non-empty (checked above)"));
                            }
                        }
                    }
                }
                // PMAT-517: `s.replace(old, new, count)` (3-arg) → Rust
                // `s.replacen(...)` (replace the first `count` occurrences). The
                // 2-arg form falls through to `Replace` in the generic path.
                if attr.attr.as_str() == "replace"
                    && call.args.len() == 3
                    && call.keywords.is_empty()
                {
                    let recv = lower_expr_in_ctx(ctx, (*attr.value).clone())?;
                    if matches!(infer_type_in_ctx(ctx, &recv), Type::Str) {
                        let mut args = Vec::with_capacity(3);
                        for a in &call.args {
                            args.push(lower_expr_in_ctx(ctx, a.clone())?);
                        }
                        if !matches!(infer_type_in_ctx(ctx, &args[2]), Type::I64) {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` calls str `.replace(old, new, count)` with a non-int count",
                                ctx.fn_name
                            )));
                        }
                        return Ok(Expr::StrMethod {
                            recv: Box::new(recv),
                            op: StrMethodOp::ReplaceN,
                            args,
                        });
                    }
                }
                // PMAT-518: `s.split(sep, maxsplit)` (2-arg) → Rust
                // `s.splitn(maxsplit + 1, sep)`. The 1-arg form falls through to
                // `Split` in the generic path.
                if attr.attr.as_str() == "split" && call.args.len() == 2 && call.keywords.is_empty()
                {
                    let recv = lower_expr_in_ctx(ctx, (*attr.value).clone())?;
                    if matches!(infer_type_in_ctx(ctx, &recv), Type::Str) {
                        let sep = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                        let maxsplit = lower_expr_in_ctx(ctx, call.args[1].clone())?;
                        if !matches!(infer_type_in_ctx(ctx, &maxsplit), Type::I64) {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` calls str `.split(sep, maxsplit)` with a non-int maxsplit",
                                ctx.fn_name
                            )));
                        }
                        return Ok(Expr::StrMethod {
                            recv: Box::new(recv),
                            op: StrMethodOp::SplitN,
                            args: vec![sep, maxsplit],
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
                            // PMAT-570: `xs.pop(-k)` removes from the end — resolve
                            // the negative literal to `len(xs) - k` (else it emits
                            // `(-k) as usize` → usize::MAX → panic).
                            Some(a) if neg_literal_int(a).is_some() => {
                                let k = neg_literal_int(a).unwrap();
                                Some(Box::new(Expr::BinOp {
                                    op: BinOp::Sub,
                                    lhs: Box::new(Expr::Len(Box::new(recv.clone()))),
                                    rhs: Box::new(Expr::LitInt(k)),
                                }))
                            }
                            Some(a) => {
                                let i = lower_expr_in_ctx(ctx, a.clone())?;
                                if infer_type_in_ctx(ctx, &i) != Type::I64 {
                                    return Err(FrontendError::Lower(format!(
                                        "function `{}` calls list `.pop(<index>)` with a \
                                         non-int index; v0.2.0 requires an int position",
                                        ctx.fn_name
                                    )));
                                }
                                // PMAT-609: a non-literal (runtime) index may be
                                // negative at runtime — Python `pop(i)` with i<0
                                // removes from the end (`i + len`). A bare
                                // `(i) as usize` wraps a negative i to usize::MAX
                                // → Vec::remove panics. Normalize: bind once, then
                                // `if __pidx < 0 { len + __pidx } else { __pidx }`.
                                // A non-negative literal needs no guard; negative
                                // literals are resolved to `len - k` above.
                                if matches!(i, Expr::LitInt(_)) {
                                    Some(Box::new(i))
                                } else {
                                    Some(Box::new(Expr::Block(Box::new(Block {
                                        stmts: vec![Stmt::Let {
                                            name: "__pidx".to_string(),
                                            ty: Type::I64,
                                            value: i,
                                            mutable: false,
                                        }],
                                        trailing_return: Expr::IfExpr {
                                            cond: Box::new(Expr::BinOp {
                                                op: BinOp::Lt,
                                                lhs: Box::new(Expr::Ident("__pidx".to_string())),
                                                rhs: Box::new(Expr::LitInt(0)),
                                            }),
                                            then_expr: Box::new(Expr::BinOp {
                                                op: BinOp::Add,
                                                lhs: Box::new(Expr::Len(Box::new(recv.clone()))),
                                                rhs: Box::new(Expr::Ident("__pidx".to_string())),
                                            }),
                                            else_expr: Box::new(Expr::Ident("__pidx".to_string())),
                                        },
                                    }))))
                                }
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
            // types appropriately, lower to `Expr::NumBuiltin`. Otherwise
            // fall through (e.g. a user fn named `min`).
            // PMAT-502cn: 2-arg `min`/`max` also accept `str`/`bool` operands
            // (all `Ord`; the codegen `(a).min(b)` resolves for each), so
            // `min("a", "b")` no longer silently emits an undefined `min(...)`.
            // `abs` stays numeric-only.
            if let ast::Expr::Name(fname) = call.func.as_ref() {
                // PMAT-506b/506e (classes epic): `Name(...)` over a known class →
                // struct construction. Positional args fill fields in declaration
                // order; keyword args (PMAT-506e) fill the rest by name (Python's
                // rule: positionals first, then keywords). Each field exactly
                // once; unknown keywords / duplicates / arity mismatch error.
                // PMAT-506h: inside a `@classmethod` body, `cls(...)` constructs
                // the enclosing class (resolved via `ctx.cls_name`).
                let ctor_name = ctx.resolve_class_name(fname.id.as_str());
                if let Some(field_names) = ctx
                    .structs
                    .get(ctor_name)
                    .map(|fs| fs.iter().map(|(f, _)| f.clone()).collect::<Vec<_>>())
                {
                    if call.args.len() > field_names.len() {
                        return Err(FrontendError::Lower(format!(
                            "function `{}` constructs `{}` with {} positional arg(s) but the class has {} field(s)",
                            ctx.fn_name,
                            fname.id,
                            call.args.len(),
                            field_names.len()
                        )));
                    }
                    let mut values: HashMap<String, Expr> = HashMap::new();
                    // Positional args fill the leading fields.
                    for (field, arg) in field_names.iter().zip(call.args.iter()) {
                        values.insert(field.clone(), lower_expr_in_ctx(ctx, arg.clone())?);
                    }
                    // Keyword args fill the rest by name.
                    for kw in &call.keywords {
                        let Some(kw_name) = kw.arg.as_ref().map(|id| id.to_string()) else {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` constructs `{}` with a `**`-splat — not supported at v0.2.0",
                                ctx.fn_name, fname.id
                            )));
                        };
                        if !field_names.contains(&kw_name) {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` constructs `{}` with unknown field `{kw_name}`",
                                ctx.fn_name, fname.id
                            )));
                        }
                        if values.contains_key(&kw_name) {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` constructs `{}` giving field `{kw_name}` both positionally and by keyword",
                                ctx.fn_name, fname.id
                            )));
                        }
                        values.insert(kw_name, lower_expr_in_ctx(ctx, kw.value.clone())?);
                    }
                    // PMAT-506f: fill any still-omitted field from its declared
                    // default (a literal lowered in the pre-pass).
                    if values.len() != field_names.len() {
                        if let Some(defaults) = ctx.struct_field_defaults.get(ctor_name) {
                            for (field, default) in defaults {
                                values
                                    .entry(field.clone())
                                    .or_insert_with(|| default.clone());
                            }
                        }
                    }
                    if values.len() != field_names.len() {
                        let missing: Vec<&str> = field_names
                            .iter()
                            .filter(|f| !values.contains_key(*f))
                            .map(String::as_str)
                            .collect();
                        return Err(FrontendError::Lower(format!(
                            "function `{}` constructs `{}` missing field(s) with no default: {missing:?}",
                            ctx.fn_name, fname.id
                        )));
                    }
                    // Emit fields in declaration order (deterministic).
                    let mut fields = Vec::with_capacity(field_names.len());
                    for f in &field_names {
                        fields.push((f.clone(), values.remove(f).expect("field covered above")));
                    }
                    return Ok(Expr::StructLit {
                        name: ctor_name.to_string(),
                        fields,
                    });
                }
                if let Some((op, arity)) = num_builtin_op(fname.id.as_str()) {
                    // PMAT-502cz: `min`/`max` are VARIADIC — accept any arity
                    // `>= 2` (`max(a, b, c)` chains `.max(b).max(c)`). `abs`
                    // stays exactly 1-arg. Previously `max(a, b, c)` fell to a
                    // generic call (undefined Rust `max(...)`); now it lowers.
                    let variadic = matches!(op, NumBuiltinOp::Min | NumBuiltinOp::Max);
                    let arity_ok = if variadic {
                        call.args.len() >= arity
                    } else {
                        call.args.len() == arity
                    };
                    if call.keywords.is_empty() && arity_ok {
                        let args = call
                            .args
                            .iter()
                            .map(|a| lower_expr_in_ctx(ctx, a.clone()))
                            .collect::<Result<Vec<_>, _>>()?;
                        let arg0_ty = infer_type_in_ctx(ctx, &args[0]);
                        let ok = match op {
                            NumBuiltinOp::Abs => matches!(arg0_ty, Type::I64 | Type::F64),
                            NumBuiltinOp::Min | NumBuiltinOp::Max => {
                                matches!(arg0_ty, Type::I64 | Type::F64 | Type::Str | Type::Bool)
                            }
                            // PMAT-502ek/el: the `math.*` ops come only from
                            // `math.<fn>` (lower_math_call), never the bare-name
                            // `num_builtin_op` dispatch — unreachable here.
                            NumBuiltinOp::Sqrt
                            | NumBuiltinOp::Floor
                            | NumBuiltinOp::Ceil
                            | NumBuiltinOp::Trunc
                            | NumBuiltinOp::Sin
                            | NumBuiltinOp::Cos
                            | NumBuiltinOp::Tan
                            | NumBuiltinOp::Exp
                            | NumBuiltinOp::Ln
                            | NumBuiltinOp::Log10
                            | NumBuiltinOp::Log2 => false,
                        };
                        if ok {
                            // PMAT-541: a mixed-numeric `min`/`max` (e.g.
                            // `min(x, n)` with `x: float`, `n: int`) must
                            // promote every operand to f64 — Rust's
                            // `f64::min` / `i64::min` can't mix types. Only when
                            // at least one operand is float; homogeneous
                            // int / str / bool min-max is left untouched.
                            let args = if matches!(op, NumBuiltinOp::Min | NumBuiltinOp::Max)
                                && args.iter().any(|a| infer_type_in_ctx(ctx, a) == Type::F64)
                            {
                                args.into_iter().map(|a| to_f64_operand(ctx, a)).collect()
                            } else {
                                args
                            };
                            // PMAT-579: record whether the operand is float so
                            // codegen picks `.abs()` (f64) vs `.checked_abs()`
                            // (i64). Consulted only for `Abs`; harmless for min/max.
                            let of_float = args
                                .first()
                                .is_some_and(|a| infer_type_in_ctx(ctx, a) == Type::F64);
                            return Ok(Expr::NumBuiltin { op, args, of_float });
                        }
                    }
                }
                // PMAT-502cy: `pow(a, b)` 2-arg == `a ** b` — reuse the `**`
                // machinery (float `powf` when either operand is `f64`, else
                // integer `checked_pow`). Previously `pow` fell through to a
                // generic call, emitting an undefined Rust `pow(...)` fn.
                // 3-arg `pow(a, b, mod)` (modular exponentiation) is deferred.
                if fname.id.as_str() == "pow" && call.keywords.is_empty() && call.args.len() == 2 {
                    // PMAT-607: a bool operand is an int in Python (`pow(True, n)`
                    // == `pow(1, n)`); coerce bool → i64 (no-op for int/float) so
                    // it expands to checked_pow/powf instead of a bare `pow(...)`.
                    let lhs = to_i64_operand(ctx, lower_expr_in_ctx(ctx, call.args[0].clone())?);
                    let rhs = to_i64_operand(ctx, lower_expr_in_ctx(ctx, call.args[1].clone())?);
                    let lty = infer_type_in_ctx(ctx, &lhs);
                    let rty = infer_type_in_ctx(ctx, &rhs);
                    if matches!(lty, Type::I64 | Type::F64) && matches!(rty, Type::I64 | Type::F64)
                    {
                        if lty == Type::F64 || rty == Type::F64 {
                            return Ok(Expr::FloatBinOp {
                                op: FloatOp::Pow,
                                lhs: Box::new(to_f64_operand(ctx, lhs)),
                                rhs: Box::new(to_f64_operand(ctx, rhs)),
                            });
                        }
                        return Ok(Expr::BinOp {
                            op: BinOp::Pow,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        });
                    }
                }
                // PMAT-571: 3-arg `pow(base, exp, mod)` — modular exponentiation
                // (all int). Emits an inline square-and-multiply (the previous
                // bare `pow(a,b,c)` call referenced an undefined Rust fn → E0425).
                if fname.id.as_str() == "pow" && call.keywords.is_empty() && call.args.len() == 3 {
                    // PMAT-607: coerce a bool base/exp/mod to i64 (Python int).
                    let base = to_i64_operand(ctx, lower_expr_in_ctx(ctx, call.args[0].clone())?);
                    let exp = to_i64_operand(ctx, lower_expr_in_ctx(ctx, call.args[1].clone())?);
                    let modulus =
                        to_i64_operand(ctx, lower_expr_in_ctx(ctx, call.args[2].clone())?);
                    if infer_type_in_ctx(ctx, &base) == Type::I64
                        && infer_type_in_ctx(ctx, &exp) == Type::I64
                        && infer_type_in_ctx(ctx, &modulus) == Type::I64
                    {
                        return Ok(Expr::PowMod {
                            base: Box::new(base),
                            exp: Box::new(exp),
                            modulus: Box::new(modulus),
                        });
                    }
                }
                // PMAT-502cm: `ord(c)` (str → int code point) and `chr(n)`
                // (int → 1-char str). 1-arg builtins. (Previously `ord`/`chr`
                // fell through to a generic call, emitting an undefined
                // `ord(...)`/`chr(...)` Rust fn.)
                if fname.id.as_str() == "ord" && call.keywords.is_empty() && call.args.len() == 1 {
                    let value = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    if matches!(infer_type_in_ctx(ctx, &value), Type::Str) {
                        return Ok(Expr::Ord {
                            value: Box::new(value),
                        });
                    }
                    return Err(FrontendError::Lower(format!(
                        "function `{}` calls `ord(...)` on a non-str argument",
                        ctx.fn_name
                    )));
                }
                if fname.id.as_str() == "chr" && call.keywords.is_empty() && call.args.len() == 1 {
                    let value = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    if matches!(infer_type_in_ctx(ctx, &value), Type::I64) {
                        return Ok(Expr::Chr {
                            value: Box::new(value),
                        });
                    }
                    return Err(FrontendError::Lower(format!(
                        "function `{}` calls `chr(...)` on a non-int argument",
                        ctx.fn_name
                    )));
                }
                // PMAT-502cv: `hex(n)` / `oct(n)` / `bin(n)` (int → radix str).
                if let Some(radix) = match fname.id.as_str() {
                    "hex" => Some(Radix::Hex),
                    "oct" => Some(Radix::Oct),
                    "bin" => Some(Radix::Bin),
                    _ => None,
                } {
                    if call.keywords.is_empty() && call.args.len() == 1 {
                        let value = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                        if matches!(infer_type_in_ctx(ctx, &value), Type::I64) {
                            return Ok(Expr::IntRadixStr {
                                value: Box::new(value),
                                radix,
                                prefixed: true,
                                upper: false,
                            });
                        }
                        return Err(FrontendError::Lower(format!(
                            "function `{}` calls `{}(...)` on a non-int argument",
                            ctx.fn_name,
                            fname.id.as_str()
                        )));
                    }
                }
                // PMAT-502w: ctx-aware `len(x)` — lower the argument through
                // the context path so a context-dependent collection (e.g.
                // `len(d.keys())`, `len(sorted(xs))`) is recognized. The
                // context-free `lower_call` path also handles bare `len(xs)`,
                // but loses ctx (method calls there error). Same `Expr::Len`.
                if fname.id.as_str() == "len" && call.keywords.is_empty() && call.args.len() == 1 {
                    // PMAT-522: `len(range(n))` materialises the range to a Vec.
                    let inner = lower_arg_materializing_range(ctx, &call.args[0])?;
                    // PMAT-564: `len(str)` counts Unicode code points, not UTF-8
                    // bytes — route to `.chars().count()` (NOT `Expr::Len`, which
                    // emits `.len()` = byte length and is wrong for non-ASCII).
                    if infer_type_in_ctx(ctx, &inner) == Type::Str {
                        return Ok(Expr::StrMethod {
                            recv: Box::new(inner),
                            op: StrMethodOp::CharCount,
                            args: vec![],
                        });
                    }
                    return Ok(Expr::Len(Box::new(inner)));
                }
                // PMAT-498b: `sum(xs)` over a numeric list. PMAT-502cx:
                // `sum(xs, start)` 2-arg — `start` must match the element
                // type (`int` for an int list, `float` for a float list);
                // emitted as `(start) + sum(xs)` (no cast).
                if fname.id.as_str() == "sum"
                    && call.keywords.is_empty()
                    && (1..=2).contains(&call.args.len())
                {
                    // PMAT-521: materialise `range(...)` / a set arg into a list.
                    if let Some(list) = materialize_iterable_arg(ctx, &call.args[0])? {
                        if let Type::List(elem) = infer_type_in_ctx(ctx, &list) {
                            // PMAT-565: `sum(list[bool])` — Python counts True as
                            // 1 (bool is an int subtype). Map each bool → i64,
                            // then sum as ints. Fixes both `sum(bs)` over a bool
                            // list (bare-`sum()` rustc error) and the very common
                            // `sum(x > 0 for x in xs)` counting genexpr (reject).
                            if matches!(*elem, Type::Bool) {
                                let mapped = Expr::Map {
                                    list: Box::new(list),
                                    lambda: SortKey {
                                        param: "__b".to_string(),
                                        body: Box::new(Expr::NumCast {
                                            value: Box::new(Expr::Ident("__b".to_string())),
                                            to_float: false,
                                            from_str: false,
                                            from_float: false,
                                        }),
                                    },
                                };
                                let start = if call.args.len() == 2 {
                                    Some(Box::new(lower_expr_in_ctx(ctx, call.args[1].clone())?))
                                } else {
                                    None
                                };
                                return Ok(Expr::Sum {
                                    list: Box::new(mapped),
                                    of_float: false,
                                    start,
                                });
                            }
                            if matches!(*elem, Type::I64 | Type::F64) {
                                let of_float = matches!(*elem, Type::F64);
                                let start = if call.args.len() == 2 {
                                    let s = lower_expr_in_ctx(ctx, call.args[1].clone())?;
                                    let sty = infer_type_in_ctx(ctx, &s);
                                    let matches_elem = if of_float {
                                        matches!(sty, Type::F64)
                                    } else {
                                        matches!(sty, Type::I64)
                                    };
                                    if !matches_elem {
                                        return Err(FrontendError::Lower(format!(
                                            "sum(xs, start): start type {sty:?} must match the \
                                         list element type ({} expected)",
                                            if of_float { "float" } else { "int" }
                                        )));
                                    }
                                    Some(Box::new(s))
                                } else {
                                    None
                                };
                                return Ok(Expr::Sum {
                                    list: Box::new(list),
                                    of_float,
                                    start,
                                });
                            }
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
                // PMAT-502da: `int(s, base)` — parse a string in the given
                // radix → `i64::from_str_radix((s).trim(), base)`. `base` must
                // be an int literal `2..=36` (variable / auto-detect `base=0`
                // deferred). Previously `int(s, 16)` fell to a generic call,
                // emitting an undefined Rust `int(s, 16)` fn (silent miscompile).
                if fname.id.as_str() == "int" && call.keywords.is_empty() && call.args.len() == 2 {
                    let value = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    if matches!(infer_type_in_ctx(ctx, &value), Type::Str) {
                        let radix_expr = lower_expr_in_ctx(ctx, call.args[1].clone())?;
                        if let Expr::LitInt(base) = radix_expr {
                            if (2..=36).contains(&base) {
                                return Ok(Expr::IntFromStrRadix {
                                    value: Box::new(value),
                                    radix: base as u32,
                                });
                            }
                            return Err(FrontendError::Lower(format!(
                                "int(s, base): base {base} out of range (must be 2..=36)"
                            )));
                        }
                        return Err(FrontendError::Lower(
                            "int(s, base): base must be an integer literal (2..=36)".to_string(),
                        ));
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
                            // PMAT-586: `int(float_x)` guards a non-finite source.
                            from_float: matches!(vty, Type::F64),
                        });
                    }
                    // PMAT-535: `int(b)` / `float(b)` over a `bool` — Python
                    // `True`/`False` → `1`/`0` (`1.0`/`0.0`). Rust allows
                    // `bool as i64` (false=0, true=1) but NOT `bool as f64`, so
                    // `float(bool)` casts through `i64` first.
                    if matches!(vty, Type::Bool) {
                        let as_int = Expr::NumCast {
                            value: Box::new(value),
                            to_float: false,
                            from_str: false,
                            from_float: false,
                        };
                        return Ok(if fname.id.as_str() == "float" {
                            Expr::NumCast {
                                value: Box::new(as_int),
                                to_float: true,
                                from_str: false,
                                from_float: false,
                            }
                        } else {
                            as_int
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
                            return Ok(bool_to_python_str(value));
                        }
                        // PMAT-626: `str(list)` / `str(tuple)` → the Python repr,
                        // reusing the same `build_list_repr`/`build_tuple_repr`
                        // desugar as f-string interpolation (PMAT-623/624).
                        Type::List(elem) => return build_list_repr(value, elem.as_ref()),
                        Type::Tuple(elems) => return build_tuple_repr(value, &elems),
                        _ => {}
                    }
                }
                // PMAT-597: the standalone `format(value[, spec])` builtin
                // (distinct from `str.format` / `%`-formatting). `format(x)` and
                // `format(x, "")` == `str(x)`; `format(x, "<literal spec>")`
                // applies the Python format mini-language (shared with f-string
                // fields). Without this, `format(...)` fell through to a generic
                // call that inferred I64 and emitted a bare `format(...)` — but
                // Rust's `format` is a *macro*, so rustc rejected it (E0423).
                if fname.id.as_str() == "format"
                    && call.keywords.is_empty()
                    && (call.args.len() == 1 || call.args.len() == 2)
                {
                    let value = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    let spec = if call.args.len() == 2 {
                        match &call.args[1] {
                            ast::Expr::Constant(c) => match &c.value {
                                ast::Constant::Str(s) => s.clone(),
                                _ => {
                                    return Err(FrontendError::Lower(format!(
                                        "function `{}` calls `format(x, spec)` with a non-string spec",
                                        ctx.fn_name
                                    )));
                                }
                            },
                            _ => {
                                return Err(FrontendError::Lower(format!(
                                    "function `{}` calls `format(x, spec)` with a non-literal spec — only a string-literal spec is supported at v0.2.0",
                                    ctx.fn_name
                                )));
                            }
                        }
                    } else {
                        String::new()
                    };
                    if !spec.is_empty() {
                        return apply_nonempty_format_spec(ctx, value, &spec);
                    }
                    // No spec / empty spec → `str(value)`.
                    return match infer_type_in_ctx(ctx, &value) {
                        Type::I64 => Ok(Expr::ToStr {
                            value: Box::new(value),
                            of_float: false,
                        }),
                        Type::F64 => Ok(Expr::ToStr {
                            value: Box::new(value),
                            of_float: true,
                        }),
                        Type::Bool => Ok(bool_to_python_str(value)),
                        Type::Str => Ok(value), // `format(s)` == `s`
                        other => Err(FrontendError::Lower(format!(
                            "function `{}` calls `format(...)` on a {other:?}; v0.2.0 supports format over int/float/bool/str",
                            ctx.fn_name
                        ))),
                    };
                }
                // PMAT-582: `repr(x)`. For an int/float/bool, `repr == str`
                // (reuse `ToStr` / the `str(bool)` desugar). For a string,
                // `repr` adds Python-style quotes + escapes → `Expr::ReprStr`.
                // (Container `repr` + f-string `{x!r}` are deferred.)
                if fname.id.as_str() == "repr" && call.keywords.is_empty() && call.args.len() == 1 {
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
                            return Ok(bool_to_python_str(value));
                        }
                        Type::Str => {
                            return Ok(Expr::ReprStr {
                                value: Box::new(value),
                            });
                        }
                        other => {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` calls `repr(...)` on a {other:?}; v0.2.0 supports repr over int/float/bool/str (container repr deferred)",
                                ctx.fn_name
                            )));
                        }
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
                    // PMAT-612: `round(int, n)` → int (was a bare `round(x, n)`
                    // call → E0425). A non-negative literal `n` is the identity
                    // (an int has no fractional part to round); a negative or
                    // non-literal `n` rounds to the nearest `10^(-n)` with
                    // banker's rounding at runtime (`Expr::RoundIntToDigits`).
                    if infer_type_in_ctx(ctx, &value) == Type::I64
                        && infer_type_in_ctx(ctx, &ndigits) == Type::I64
                    {
                        if let Expr::LitInt(k) = &ndigits {
                            if *k >= 0 {
                                return Ok(value);
                            }
                        }
                        return Ok(Expr::RoundIntToDigits {
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
                    // PMAT-502dh: optional `default=<expr>` — returned when the
                    // list is empty (instead of panicking).
                    let mut default: Option<Box<Expr>> = None;
                    let mut kwargs_ok = true;
                    for kw in &call.keywords {
                        match kw.arg.as_ref().map(|a| a.as_str()) {
                            // PMAT-502aa/ei: `key=lambda p: e` or `key=<fn>`.
                            Some("key") => {
                                if let Some(k) = lower_sort_key(
                                    ctx,
                                    &kw.value,
                                    sort_target_elem_type(ctx, &call.args[0]),
                                )? {
                                    key = Some(k);
                                    continue;
                                }
                            }
                            Some("default") => {
                                default = Some(Box::new(lower_expr_in_ctx(ctx, kw.value.clone())?));
                                continue;
                            }
                            _ => {}
                        }
                        kwargs_ok = false;
                    }
                    if kwargs_ok {
                        // PMAT-521: materialise `range(...)` / a set arg into a list.
                        if let Some(list) = materialize_iterable_arg(ctx, &call.args[0])? {
                            if let Type::List(elem) = infer_type_in_ctx(ctx, &list) {
                                // With a key, any element type works (the key
                                // supplies the ordering); without, the element must
                                // be `Ord` (or `f64`, via the fold) — PMAT-502er
                                // adds `str`/`bool` (both `Ord`) to the int/float
                                // first cut, so `min(words)`/`max(words)` work.
                                if key.is_some()
                                    || matches!(
                                        *elem,
                                        Type::I64 | Type::F64 | Type::Str | Type::Bool
                                    )
                                {
                                    return Ok(Expr::ListMinMax {
                                        list: Box::new(list),
                                        is_max: fname.id.as_str() == "max",
                                        of_float: matches!(*elem, Type::F64),
                                        key,
                                        default,
                                    });
                                }
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
                            // PMAT-502z/ei: `key=lambda p: e` or `key=<fn>`.
                            Some("key") => {
                                if let Some(k) = lower_sort_key(
                                    ctx,
                                    &kw.value,
                                    sort_target_elem_type(ctx, &call.args[0]),
                                )? {
                                    key = Some(k);
                                    continue;
                                }
                                kwargs_ok = false;
                            }
                            _ => kwargs_ok = false,
                        }
                    }
                    if kwargs_ok {
                        // PMAT-522: `sorted(range(n))` materialises the range.
                        let arg = lower_arg_materializing_range(ctx, &call.args[0])?;
                        // PMAT-502eu: `sorted(d)` over a dict sorts its KEYS
                        // (Python iterates a dict as its keys) — materialize the
                        // keys list first. PMAT-502ev: `sorted(s)` over a str
                        // sorts its characters → a list of 1-char strings
                        // (`Expr::StrChars`). `sorted(xs)` over a list unchanged.
                        let list = match infer_type_in_ctx(ctx, &arg) {
                            Type::List(_) => Some(arg),
                            Type::Dict(_, _) => Some(Expr::DictView {
                                dict: Box::new(arg),
                                kind: DictViewKind::Keys,
                            }),
                            Type::Str => Some(Expr::StrChars {
                                string: Box::new(arg),
                            }),
                            // PMAT-520: `sorted(set(...))` / `sorted(<set>)` →
                            // sort the unique elements (materialise the set to a
                            // Vec first). Previously fell through to a miscompile.
                            Type::Set(_) => Some(Expr::SetToList { set: Box::new(arg) }),
                            _ => None,
                        };
                        if let Some(list) = list {
                            // PMAT-578: a keyless float sort needs `partial_cmp`
                            // (no `Ord` for f64); record the element type.
                            // PMAT-603: with a `key=`, the compared values are the
                            // key results, so track the KEY's float-ness instead.
                            let of_float = match &key {
                                Some(k) => sort_key_is_float(
                                    ctx,
                                    k,
                                    sort_target_elem_type(ctx, &call.args[0]),
                                ),
                                // PMAT-622: float anywhere in the element
                                // (tuple/nested list) → partial_cmp, not Vec::sort.
                                None => {
                                    matches!(infer_type_in_ctx(ctx, &list), Type::List(elem) if type_contains_float(&elem))
                                }
                            };
                            return Ok(Expr::Sorted {
                                list: Box::new(list),
                                reverse,
                                key,
                                of_float,
                            });
                        }
                    }
                }
                // PMAT-502d: `reversed(xs)` over a list → a new reversed
                // list. The supported subset materializes Python's lazy
                // `reversed` iterator as a `Vec`, so `reversed(xs)` and the
                // idiomatic `list(reversed(xs))` both produce `Expr::Reversed`.
                // PMAT-596: `reversed(s)` over a `str` reverses the *characters*
                // (Python yields an iterator of 1-char strings, materialized as
                // a `list[str]`). Lower to `Reversed(StrChars(s))` — reusing
                // both existing nodes — so it types as `List(Str)` and composes
                // with `"".join(reversed(s))`, `list(reversed(s))`, and
                // `for c in reversed(s)` exactly like Python (the `s[::-1]`
                // slice form, which yields a `str`, is a separate lowering).
                if fname.id.as_str() == "reversed"
                    && call.keywords.is_empty()
                    && call.args.len() == 1
                {
                    // PMAT-522: `reversed(range(n))` materialises the range.
                    let list = lower_arg_materializing_range(ctx, &call.args[0])?;
                    match infer_type_in_ctx(ctx, &list) {
                        Type::List(_) => {
                            return Ok(Expr::Reversed {
                                list: Box::new(list),
                            });
                        }
                        Type::Str => {
                            return Ok(Expr::Reversed {
                                list: Box::new(Expr::StrChars {
                                    string: Box::new(list),
                                }),
                            });
                        }
                        _ => {}
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
                            if let Type::List(elem) = infer_type_in_ctx(ctx, &list) {
                                let param = lam.args.args[0].def.arg.to_string();
                                // PMAT-526: bind the lambda param to the element
                                // type so e.g. `p[1]` over a tuple element lowers
                                // to `.1` (was lowered with `p` unbound → I64).
                                let mut sub = ctx.clone();
                                sub.bound.insert(param.clone());
                                sub.name_types.insert(param.clone(), *elem);
                                let body = lower_expr_in_ctx(&sub, (*lam.body).clone())?;
                                if infer_type_in_ctx(&sub, &body) == Type::Bool {
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
                            if let Type::List(elem) = infer_type_in_ctx(ctx, &list) {
                                let param = lam.args.args[0].def.arg.to_string();
                                // PMAT-526: bind the lambda param to the element
                                // type (e.g. `p[0] + p[1]` over a tuple element).
                                let mut sub = ctx.clone();
                                sub.bound.insert(param.clone());
                                sub.name_types.insert(param.clone(), *elem);
                                let body = lower_expr_in_ctx(&sub, (*lam.body).clone())?;
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
                    // PMAT-502cj: `list(range(...))` materialises a range into a
                    // Vec (`Expr::RangeList`). Detected on the AST before
                    // lowering (a bare `range(...)` isn't a first-class value).
                    if let ast::Expr::Call(inner) = &call.args[0] {
                        if matches!(&*inner.func, ast::Expr::Name(n) if n.id.as_str() == "range")
                            && inner.keywords.is_empty()
                        {
                            return lower_range_list(ctx, inner);
                        }
                    }
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
                    // PMAT-502cj: `list(xs)` over an existing list is a copy —
                    // value semantics already clones, so return it as-is.
                    if matches!(infer_type_in_ctx(ctx, &inner), Type::List(_)) {
                        return Ok(inner);
                    }
                    // PMAT-520: `list(set(...))` / `list(<set>)` → the unique
                    // elements as a Vec. Previously fell through to a miscompile
                    // (`list(set(...))` with both calls emitted as undefined fns).
                    if matches!(infer_type_in_ctx(ctx, &inner), Type::Set(_)) {
                        return Ok(Expr::SetToList {
                            set: Box::new(inner),
                        });
                    }
                    // PMAT-522: `list(d)` over a dict → its keys (Python iterates
                    // a dict as its keys). Previously a miscompile (`list(...)`).
                    if matches!(infer_type_in_ctx(ctx, &inner), Type::Dict(_, _)) {
                        return Ok(Expr::DictView {
                            dict: Box::new(inner),
                            kind: DictViewKind::Keys,
                        });
                    }
                }
                // PMAT-502cw: `set(xs)` materialises a list into a HashSet
                // (de-duplicating). 1-arg over a list-typed value.
                // PMAT-519: `frozenset(xs)` — Rust has no frozen set; an immutable
                // set is just a `HashSet` that is never mutated, so route it
                // through the same `SetFromList` path (previously a silent
                // miscompile that emitted an undefined `frozenset(...)` call).
                if matches!(fname.id.as_str(), "set" | "frozenset")
                    && call.keywords.is_empty()
                    && call.args.len() == 1
                {
                    let inner = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    if matches!(infer_type_in_ctx(ctx, &inner), Type::List(_)) {
                        return Ok(Expr::SetFromList {
                            list: Box::new(inner),
                        });
                    }
                    return Err(FrontendError::Lower(format!(
                        "function `{}` calls `{}(<expr>)` over a non-list — v0.2.0 supports `set()`/`frozenset()` (empty) or over a `<list>`",
                        ctx.fn_name, fname.id
                    )));
                }
                // PMAT-502dk: `dict(pairs)` materialises a list of 2-tuples
                // into a HashMap. 1-arg over a `list[tuple[K, V]]` value (so
                // `dict([(k, v), …])`, `dict(zip(a, b))`, `dict(enumerate(xs))`
                // all work). The empty 0-arg `dict()` is handled below.
                if fname.id.as_str() == "dict" && call.keywords.is_empty() && call.args.len() == 1 {
                    let inner = lower_expr_in_ctx(ctx, call.args[0].clone())?;
                    if let Type::List(elem) = infer_type_in_ctx(ctx, &inner) {
                        if matches!(*elem, Type::Tuple(ref tys) if tys.len() == 2) {
                            return Ok(Expr::DictFromPairs {
                                pairs: Box::new(inner),
                            });
                        }
                    }
                    return Err(FrontendError::Lower(format!(
                        "function `{}` calls `dict(<expr>)` over a non-(list of 2-tuples) — v0.2.0 supports `dict()` (empty) or `dict(<list of (key, value) pairs>)`",
                        ctx.fn_name
                    )));
                }
                // PMAT-502i: empty collection constructors. `set()`/`dict()`/
                // `list()` (0 args) → the corresponding empty literal. Like
                // the empty `{}` dict, the element type comes from a binding
                // annotation (`s: set[int] = set()`) or a subsequent
                // `.add()`/`.append()` that lets rustc infer it.
                if call.keywords.is_empty() && call.args.is_empty() {
                    match fname.id.as_str() {
                        // PMAT-519: `frozenset()` maps to an (immutable) HashSet.
                        "set" | "frozenset" => return Ok(Expr::SetLit(Vec::new())),
                        "dict" => return Ok(Expr::DictLit(Vec::new())),
                        "list" => return Ok(Expr::ListLit(Vec::new())),
                        _ => {}
                    }
                }
                // PMAT-502fe: `tuple(<iterable>)` has no Rust target — Rust
                // tuples are fixed-arity, so a variable-length `tuple(xs)`
                // cannot be represented as a Rust tuple type. Reject cleanly
                // instead of silently emitting an undefined `tuple(...)` call
                // that fails rustc — this upholds the central "transpile-success
                // ⟹ valid Rust" guarantee (a silent miscompile is a thesis
                // violation). Fixed-arity tuples are written as `(a, b)` literals
                // (Type::Tuple); a growable sequence stays a `list`.
                if fname.id.as_str() == "tuple" {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` calls `tuple(...)` — Rust tuples are fixed-arity, so a \
                         variable-length `tuple(<iterable>)` has no Rust counterpart at v0.2.0; \
                         write a fixed tuple as a `(a, b)` literal, or keep the value as a `list`",
                        ctx.fn_name
                    )));
                }
            }
            // PMAT-474 (R5): reorder keyword args to positional using
            // the module signature table, then lower as a plain call.
            // PMAT-627: also fill defaults in nested user-calls in argument
            // position — `lower_call` (used below) lowers args context-free, so a
            // nested `f(g(x))` would otherwise emit `g(x)` bare (E0061).
            let call = reorder_kwargs_to_positional(ctx, call)?;
            let call = reorder_nested_call_args(ctx, call)?;
            // PMAT-502dq: a call to a variadic (`*args`) function collects the
            // trailing positional args (those past the fixed params) into a
            // single `list` argument, matching the `list[elem]` vararg param.
            if let ast::Expr::Name(n) = call.func.as_ref() {
                let is_variadic = ctx
                    .signatures
                    .get(n.id.as_str())
                    .is_some_and(|s| s.variadic.is_some());
                if is_variadic && call.keywords.is_empty() {
                    let callee = n.id.to_string();
                    let fixed = ctx.signatures.get(&callee).map_or(0, |s| s.params.len());
                    if call.args.len() >= fixed {
                        // PMAT-502ds: `f(fixed…, *xs)` — a `*`-splat covering the
                        // whole vararg tail passes the list directly (`f(…, xs)`)
                        // instead of collecting into a fresh `vec![]`.
                        let tail_is_splat = call.args.len() == fixed + 1
                            && matches!(call.args.get(fixed), Some(ast::Expr::Starred(_)));
                        let no_fixed_splat = call.args[..fixed]
                            .iter()
                            .all(|a| !matches!(a, ast::Expr::Starred(_)));
                        if tail_is_splat && no_fixed_splat {
                            let mut lowered: Vec<Expr> = Vec::with_capacity(fixed + 1);
                            for a in &call.args[..fixed] {
                                lowered.push(lower_expr_in_ctx(ctx, a.clone())?);
                            }
                            let ast::Expr::Starred(s) = &call.args[fixed] else {
                                unreachable!("checked Starred above")
                            };
                            let list = lower_expr_in_ctx(ctx, (*s.value).clone())?;
                            if !matches!(infer_type_in_ctx(ctx, &list), Type::List(_)) {
                                return Err(FrontendError::Lower(format!(
                                    "function `{}` splats `*<expr>` into variadic `{callee}`, but the expr is not a list",
                                    ctx.fn_name
                                )));
                            }
                            lowered.push(list);
                            return Ok(Expr::Call {
                                callee,
                                args: lowered,
                            });
                        }
                        // Any other `*`-splat shape (mixed with positionals, or
                        // in a fixed slot) is deferred.
                        if call.args.iter().any(|a| matches!(a, ast::Expr::Starred(_))) {
                            return Err(FrontendError::Lower(format!(
                                "function `{}` uses an unsupported `*`-splat shape calling variadic `{callee}` — only `{callee}(fixed…, *iterable)` is supported",
                                ctx.fn_name
                            )));
                        }
                        let mut lowered: Vec<Expr> = Vec::with_capacity(fixed + 1);
                        for a in &call.args {
                            lowered.push(lower_expr_in_ctx(ctx, a.clone())?);
                        }
                        let tail = lowered.split_off(fixed);
                        lowered.push(Expr::ListLit(tail));
                        return Ok(Expr::Call {
                            callee,
                            args: lowered,
                        });
                    }
                }
            }
            // PMAT-630: lower a user-function call's args CONTEXT-AWARE. `lower_call`
            // lowers args via the context-free `lower_expr`, which loses parameter
            // type bindings — so a context-dependent argument (a `bool` `and`/`or`/
            // `not`/ternary like `g(5, c and d)`, a nested call, a dict read) was
            // mis-typed and rejected ("operands of `and`/`or` must be Bool"). The
            // callee is a Name here (builtins were handled earlier in this match;
            // `len` is too, but exclude it defensively and let `lower_call` own it).
            if let ast::Expr::Name(n) = call.func.as_ref() {
                let callee = n.id.to_string();
                if callee != "len" {
                    let args = call
                        .args
                        .iter()
                        .map(|a| lower_expr_in_ctx(ctx, a.clone()))
                        .collect::<Result<Vec<_>, _>>()?;
                    return Ok(clone_reused_call_args(ctx, Expr::Call { callee, args }));
                }
            }
            // PMAT-588: clone reused non-Copy call args (E0382 fix).
            lower_call(call).map(|e| clone_reused_call_args(ctx, e))
        }
        // `k in d` / `k not in d` → `Expr::DictContains` (wrapped in
        // `not` for the negated form) when the RHS is a dict.
        ast::Expr::Compare(c) => {
            if c.ops.len() == 1
                && c.comparators.len() == 1
                && matches!(c.ops[0], ast::CmpOp::In | ast::CmpOp::NotIn)
            {
                // PMAT-534: `x in range(...)` / `x not in range(...)` → a bounds
                // check (the range is NOT materialized — `x in range(10**9)`
                // must not allocate a Vec). Detected syntactically before the
                // rhs is lowered as a value.
                if let ast::Expr::Call(call) = &c.comparators[0] {
                    if matches!(call.func.as_ref(), ast::Expr::Name(n) if n.id.as_str() == "range")
                    {
                        let in_range = lower_in_range(ctx, c.left.as_ref(), call)?;
                        return Ok(if matches!(c.ops[0], ast::CmpOp::NotIn) {
                            Expr::UnOp {
                                op: UnOp::Not,
                                operand: Box::new(in_range),
                            }
                        } else {
                            in_range
                        });
                    }
                }
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
                if let Type::List(list_elem) = infer_type_in_ctx(ctx, &rhs) {
                    let elem = lower_expr_in_ctx(ctx, (*c.left).clone())?;
                    // PMAT-565: `True in xs` over a list[int] — coerce the bool
                    // needle to i64 (bool is an int subtype) so `contains` gets a
                    // matching element type (else `contains(&true)` on Vec<i64>).
                    let elem = if *list_elem == Type::I64 {
                        to_i64_operand(ctx, elem)
                    } else {
                        elem
                    };
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
            // PMAT-502dc: regular (non-membership) comparisons lower their
            // operands context-aware so a builtin operand (`abs(n) > 0`,
            // `len(s) > 3`, `max(a, b) <= c`) is recognized; the context-free
            // `lower_compare` would emit an undefined Rust `abs(...)` etc.
            lower_compare_in_ctx(ctx, c)
        }
        // Recurse through `+`/etc. so a dict op on either side (e.g.
        // `counts.get(x, 0) + 1`) is lowered correctly. Mirror the
        // str-Concat detection from `lower_expr`, using the
        // context-aware inference.
        ast::Expr::BinOp(b) => {
            // PMAT-502dm: printf-style `"<template>" % args` — the `%`
            // operator with a string *literal* LHS. Detected before the
            // numeric `%` (Mod) path; the template is parsed and translated
            // into a Rust `format!` string (an `Expr::StrFormat`).
            if matches!(b.op, ast::Operator::Mod) {
                if let ast::Expr::Constant(c) = b.left.as_ref() {
                    if let ast::Constant::Str(tmpl) = &c.value {
                        return lower_percent_format(ctx, tmpl, &b.right);
                    }
                }
            }
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
                    // PMAT-540: a mixed `float <op> int` must promote the int
                    // operand to f64 — Rust rejects `f64 + i64` (E0277). Python
                    // promotes the int. `to_f64_operand` is a no-op when the
                    // operand is already f64.
                    return Ok(Expr::FloatBinOp {
                        op: fop,
                        lhs: Box::new(to_f64_operand(ctx, lhs)),
                        rhs: Box::new(to_f64_operand(ctx, rhs)),
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
            // PMAT-593: dict operators. `a | b` over two dicts is PEP 584
            // union — semantically `{**a, **b}` (b wins on key conflicts), so
            // reuse `Expr::DictMerge` (chains both iterators into a fresh
            // HashMap; the later entry wins via `collect`). Python defines no
            // other binary operator on dicts, so `&`/`-`/`^`/`+`/… over two
            // dicts are rejected cleanly rather than emitting invalid
            // `HashMap <op> HashMap` (E0369).
            if matches!(infer_type_in_ctx(ctx, &lhs), Type::Dict(_, _))
                && matches!(infer_type_in_ctx(ctx, &rhs), Type::Dict(_, _))
            {
                if matches!(b.op, ast::Operator::BitOr) {
                    return Ok(Expr::DictMerge {
                        entries: vec![(None, lhs), (None, rhs)],
                    });
                }
                return Err(FrontendError::Lower(format!(
                    "function `{}` applies `{:?}` to two dicts; Python defines only `|` (PEP 584 union) as a binary dict operator",
                    ctx.fn_name, b.op
                )));
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
            // PMAT-565: Python's `bool` is an `int` subtype (True==1), so a bool
            // operand in integer arithmetic is coerced to i64 — without this the
            // i64-arith lowering emits e.g. `(a).checked_add(b)` on a `bool`
            // (invalid Rust). No-op for non-bool operands and non-arith ops.
            // PMAT-580: but `&`/`|`/`^` over TWO bools stays a bool op (Python
            // returns bool; Rust's `bool: BitAnd` matches), so don't coerce —
            // the result keeps `Type::Bool`.
            let both_bool = matches!(op, BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor)
                && infer_type_in_ctx(ctx, &lhs) == Type::Bool
                && infer_type_in_ctx(ctx, &rhs) == Type::Bool;
            let (lhs, rhs) = if is_int_arith_binop(op) && !both_bool {
                (to_i64_operand(ctx, lhs), to_i64_operand(ctx, rhs))
            } else {
                (lhs, rhs)
            };
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
            } else if let Expr::LitFloat(f) = operand {
                // PMAT-502bo: negative-float-literal fold.
                Ok(Expr::LitFloat(-f))
            } else if matches!(infer_type_in_ctx(ctx, &operand), Type::I64) {
                // PMAT-502de: i64 negation built from the *context-aware*
                // operand so a builtin operand (`-abs(n)`, `-max(a, b)`) is
                // recognized; the context-free `lower_unary_op` would re-lower
                // it and emit an undefined `abs(...)`.
                Ok(Expr::UnOp {
                    op: UnOp::Neg,
                    operand: Box::new(operand),
                })
            } else {
                Err(FrontendError::Lower(
                    "unary `-` requires an I64 operand or a float literal (float-variable negation is deferred)".into(),
                ))
            }
        }
        // PMAT-502cc: context-aware `not <bool var>`. The context-free
        // `lower_unary_op` infers a bare Ident as I64 and so rejects
        // `not b` for a `bool` parameter/local; using `infer_type_in_ctx`
        // sees the real type. Non-Bool operands still error (no
        // int-truthiness), via the context-free fallback.
        ast::Expr::UnaryOp(u) if matches!(u.op, ast::UnaryOp::Not) => {
            let operand = lower_expr_in_ctx(ctx, (*u.operand).clone())?;
            match infer_type_in_ctx(ctx, &operand) {
                Type::Bool => Ok(Expr::UnOp {
                    op: UnOp::Not,
                    operand: Box::new(operand),
                }),
                // PMAT-527: `not <container>` → `len(c) == 0` (empty is falsy).
                Type::List(_) | Type::Dict(_, _) | Type::Set(_) | Type::Str => Ok(Expr::BinOp {
                    op: BinOp::Eq,
                    lhs: Box::new(Expr::Len(Box::new(operand))),
                    rhs: Box::new(Expr::LitInt(0)),
                }),
                _ => lower_unary_op(u),
            }
        }
        // PMAT-502fb: context-aware bitwise invert `~x` over an I64. Lowered
        // context-aware so a builtin/typed operand (`~max(a, b)`, `~n`) is
        // recognized; the context-free path would re-lower and mis-infer it.
        ast::Expr::UnaryOp(u) if matches!(u.op, ast::UnaryOp::Invert) => {
            let operand = lower_expr_in_ctx(ctx, (*u.operand).clone())?;
            if matches!(infer_type_in_ctx(ctx, &operand), Type::I64) {
                Ok(Expr::UnOp {
                    op: UnOp::BitNot,
                    operand: Box::new(operand),
                })
            } else {
                Err(FrontendError::Lower(
                    "bitwise `~` requires an I64 operand".into(),
                ))
            }
        }
        // PMAT-502ce: context-aware `a and b` / `a or b`. The context-free
        // path mis-infers a bare Ident as I64 and rejects bool variables.
        ast::Expr::BoolOp(b) => lower_bool_op_in_ctx(ctx, b),
        // PMAT-502db: context-aware ternary. The context-free `lower_if_exp`
        // lowers each branch with `lower_expr`, so a builtin in a branch
        // (`abs(n) if … else …`, `max(a, b) if …`, `pow(n, 2) if …`) is not
        // recognized and SILENTLY emits an undefined Rust fn (`abs(...)`).
        // Lowering the branches context-aware fixes the miscompile.
        ast::Expr::IfExp(ie) => lower_if_exp_in_ctx(ctx, ie),
        // PMAT-502dd: context-aware collection literals. The context-free
        // handlers lower each element with `lower_expr`, so a builtin element
        // (`[abs(a), abs(b)]`, `{"k": abs(v)}`, `{abs(a), abs(b)}`) silently
        // emits an undefined Rust `abs(...)`. Lower elements context-aware.
        ast::Expr::List(list_expr) => lower_list_literal_in_ctx(ctx, list_expr),
        ast::Expr::Dict(dict_expr) => lower_dict_literal_in_ctx(ctx, dict_expr),
        ast::Expr::Set(set_expr) => lower_set_literal_in_ctx(ctx, set_expr),
        // PMAT-502df: a generator expression `<elt> for x in <iter>` desugars
        // to `Expr::Map` (the List-producing `map(lambda x: elt, iter)` form),
        // so `sum(...)` / `max(...)` / `min(...)` / `list(...)` accept it.
        ast::Expr::GeneratorExp(ge) => lower_generator_exp_in_ctx(ctx, ge),
        // PMAT-502du: an expression-position list comprehension (`sum([x for x
        // in xs])`, `return [x*2 for x in xs]`) lowers through the same
        // `Map`/`Filter` machinery (the statement form `name = [comp]` still
        // uses the dedicated for-append desugar, intercepted earlier).
        ast::Expr::ListComp(comp) => lower_list_comp_in_ctx(ctx, comp),
        // PMAT-502dv: set / dict comprehensions in expr position lower via the
        // same `Map`/`Filter` form, wrapped in `SetFromList` / `DictFromPairs`
        // (the statement + return forms keep their own desugars).
        ast::Expr::SetComp(comp) => lower_set_comp_in_ctx(ctx, comp),
        ast::Expr::DictComp(comp) => lower_dict_comp_in_ctx(ctx, comp),
        // PMAT-502dz: a body read of a `_` loop/comprehension target. Rust
        // forbids a bare `_` read, so resolve it to the fresh `__xpile_idx{N}`
        // name the enclosing `for _`/`… for _ …` minted (see `enter_loop_var`).
        // Only fires while such a rename is active; an unbound stray `_`
        // elsewhere still falls through to the context-free path unchanged.
        // PMAT-502ez (Optional epic cut 4): a name a preceding provably-exiting
        // `if x is None: return …` guard has proven `Some` reads as its unwrapped
        // value — `(<name>).unwrap()` : `T` — so it can be used where `T` is
        // expected. Sound because narrowing only registers for non-reassigned
        // `Optional` names guarded by an always-exiting None-check.
        ast::Expr::Name(n) if ctx.narrowed_some.contains(n.id.as_str()) => {
            Ok(Expr::OptionUnwrap(Box::new(Expr::Ident(n.id.to_string()))))
        }
        ast::Expr::Name(n) if n.id.as_str() == "_" && ctx.underscore_rename.is_some() => Ok(
            Expr::Ident(ctx.underscore_rename.clone().expect("rename is Some")),
        ),
        // PMAT-502el: `math.<const>` attribute read (`math.pi`/`math.e`/
        // `math.tau`) → a float literal. Non-`math` attribute reads fall
        // through to the context-free path (which errors — attributes aren't
        // otherwise supported).
        ast::Expr::Attribute(attr) if matches!(attr.value.as_ref(), ast::Expr::Name(n) if n.id.as_str() == "math") => {
            lower_math_const(ctx, attr.attr.as_str())
        }
        // PMAT-506b (classes epic): `obj.field` over a struct-typed receiver →
        // field access. (Method calls are `Call(Attribute(...))`, handled in the
        // Call arm; this fires only on a bare attribute read.)
        ast::Expr::Attribute(attr) => {
            // PMAT-513: `C.NAME.value` → the variant's discriminant literal;
            // PMAT-515: `C.NAME.name` → the variant name string. Both are
            // compile-time known. The receiver is itself `Enum.Variant`.
            if matches!(attr.attr.as_str(), "value" | "name") {
                if let ast::Expr::Attribute(inner) = attr.value.as_ref() {
                    if let ast::Expr::Name(en) = inner.value.as_ref() {
                        if let Some(variants) = ctx.enums.get(en.id.as_str()) {
                            if let Some((vname, disc)) =
                                variants.iter().find(|(v, _)| *v == inner.attr.as_str())
                            {
                                return Ok(if attr.attr.as_str() == "value" {
                                    Expr::LitInt(*disc)
                                } else {
                                    Expr::LitStr(vname.clone())
                                });
                            }
                        }
                    }
                }
            }
            // PMAT-513: `C.NAME` where `C` is an enum → `Expr::EnumVariant`.
            if let ast::Expr::Name(en) = attr.value.as_ref() {
                if let Some(variants) = ctx.enums.get(en.id.as_str()) {
                    let variant = attr.attr.to_string();
                    if variants.iter().any(|(v, _)| *v == variant) {
                        return Ok(Expr::EnumVariant {
                            enum_name: en.id.to_string(),
                            variant,
                        });
                    }
                    return Err(FrontendError::Lower(format!(
                        "function `{}` reads `{}.{variant}`, but enum `{}` has no such variant",
                        ctx.fn_name, en.id, en.id
                    )));
                }
            }
            let obj = lower_expr_in_ctx(ctx, (*attr.value).clone())?;
            let field = attr.attr.to_string();
            if let Type::Struct(sname) = infer_type_in_ctx(ctx, &obj) {
                // PMAT-506j: a bare read of a registered `@property` lowers to a
                // no-arg method call `(obj).prop()` (the property is a read-only
                // `self` method; its return type is in `struct_methods`).
                let is_prop = ctx
                    .struct_properties
                    .get(&sname)
                    .is_some_and(|ps| ps.contains(&field));
                if is_prop {
                    return Ok(Expr::MethodCall {
                        obj: Box::new(obj),
                        method: field,
                        args: Vec::new(),
                    });
                }
                let field_ty = ctx
                    .structs
                    .get(&sname)
                    .and_then(|fs| fs.iter().find(|(f, _)| *f == field).map(|(_, t)| t.clone()));
                if field_ty.is_none() {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` reads field `{field}` of `{sname}`, which has no such field",
                        ctx.fn_name
                    )));
                }
                let access = Expr::FieldAccess {
                    obj: Box::new(obj),
                    field,
                };
                // PMAT-585: reading a NON-Copy field by value out of a (borrowed)
                // receiver — `return self.name` over a `String`/list/dict/set/
                // struct field — moves out of a shared reference (rustc E0507).
                // Clone it. Copy fields (int/float/bool) read by value. Safe to
                // clone unconditionally: a field is never a mutation receiver
                // (`self.items.append(x)` is rejected upstream), so a field only
                // ever appears in a read/value position.
                return Ok(match field_ty {
                    Some(ty) if !matches!(ty, Type::I64 | Type::F64 | Type::Bool) => {
                        Expr::Clone(Box::new(access))
                    }
                    _ => access,
                });
            }
            Err(FrontendError::Lower(format!(
                "function `{}` reads attribute `.{field}` of a non-struct value — only struct/dataclass field access is supported at v0.2.0",
                ctx.fn_name
            )))
        }
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
    let ty = infer_type_in_ctx(ctx, &acc);
    Ok(stringify_lone_fstring_field(acc, ty))
}

/// PMAT-502ed: a single plain `{x}` field (no literal text, no format spec)
/// lowers to the bare value — for an `int` that leaves the whole f-string typed
/// `i64` instead of `Str` (`f"{n}"` returned `n`, failing the `-> str` check).
/// Wrap a lone `int` field in a `format!("{:}", …)` (an empty `FormatSpec`) so
/// the f-string is always `Str`. Multi-part chains already stringify via
/// `Concat`'s `format!`, and a `Str` value is already a string. `float`/`bool`
/// lone fields stay unwrapped (and so still error) because Rust and Python
/// disagree on their `Display` repr (`3.0`→`3`, `true`→`True`).
fn stringify_lone_fstring_field(acc: Expr, ty: Type) -> Expr {
    if ty == Type::I64 {
        Expr::FormatSpec {
            value: Box::new(acc),
            rust_spec: String::new(),
        }
    } else {
        acc
    }
}

/// Lower a single f-string part (a literal `Constant` or a `FormattedValue`)
/// context-aware. A `FormattedValue` with a static, supported format spec
/// becomes [`Expr::FormatSpec`]; a plain `{expr}` lowers its value; conversion
/// flags (`!r`/`!s`/`!a`) and unsupported / dynamic specs error.
/// PMAT-502el: lower a `math.<const>` attribute read (`math.pi`, `math.e`,
/// `math.tau`) to an `Expr::LitFloat`. The f64 constant emits as a
/// round-trip-precise literal (the same value CPython's `math.pi` holds).
/// `math.inf`/`math.nan` are deferred (they need `f64::INFINITY`/`NAN`, not a
/// finite literal). Other `math.<name>` reads error clearly.
fn lower_math_const(ctx: &LoweringCtx, name: &str) -> Result<Expr, FrontendError> {
    let v = match name {
        "pi" => std::f64::consts::PI,
        "e" => std::f64::consts::E,
        "tau" => std::f64::consts::TAU,
        other => {
            return Err(FrontendError::Lower(format!(
                "function `{}` reads `math.{other}` — v0.2.0 supports the constants `math.pi`/`math.e`/`math.tau` (and the `math.<fn>(...)` functions); `math.inf`/`math.nan` are a follow-up",
                ctx.fn_name
            )));
        }
    };
    Ok(Expr::LitFloat(v))
}

/// PMAT-502ek: lower a `math.<fn>(...)` module-function call. First cut covers
/// the common single-argument float functions `sqrt` / `floor` / `ceil`,
/// reusing [`Expr::NumBuiltin`] (so all the existing inference / codegen
/// machinery applies). `sqrt` returns `float`; `floor` / `ceil` return `int`
/// (matching Python). Other `math.*` names error clearly.
fn lower_math_call(
    ctx: &LoweringCtx,
    fn_name: &str,
    call: &ast::ExprCall,
) -> Result<Expr, FrontendError> {
    // PMAT-549/550/553: `math.gcd` / `math.lcm` / `math.comb` — 2-arg int → int
    // (inline blocks). Both args must type as `int`.
    if fn_name == "gcd" || fn_name == "lcm" || fn_name == "comb" {
        if !call.keywords.is_empty() || call.args.len() != 2 {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `math.{fn_name}(...)` with {} positional arg(s){}; v0.2.0 takes exactly 2 ints",
                ctx.fn_name,
                call.args.len(),
                if call.keywords.is_empty() { "" } else { " plus keyword args" },
            )));
        }
        let a = lower_expr_in_ctx(ctx, call.args[0].clone())?;
        let b = lower_expr_in_ctx(ctx, call.args[1].clone())?;
        if infer_type_in_ctx(ctx, &a) != Type::I64 || infer_type_in_ctx(ctx, &b) != Type::I64 {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `math.{fn_name}(...)` with a non-int argument — only `int` is supported",
                ctx.fn_name
            )));
        }
        let (a, b) = (Box::new(a), Box::new(b));
        return Ok(match fn_name {
            "gcd" => Expr::Gcd { a, b },
            "lcm" => Expr::Lcm { a, b },
            _ => Expr::Comb { n: a, k: b },
        });
    }
    // PMAT-554: `math.perm(n, k)` — k-permutations P(n,k) = n!/(n-k)! (int).
    // Two-arg form → inline product block (`Expr::Perm`); the one-arg form
    // `math.perm(n)` equals `n!`, so it lowers to `Expr::Factorial`. Args int.
    if fn_name == "perm" {
        if !call.keywords.is_empty() || call.args.is_empty() || call.args.len() > 2 {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `math.perm(...)` with {} positional arg(s){}; takes 1 or 2 ints",
                ctx.fn_name,
                call.args.len(),
                if call.keywords.is_empty() { "" } else { " plus keyword args" },
            )));
        }
        let n = lower_expr_in_ctx(ctx, call.args[0].clone())?;
        if infer_type_in_ctx(ctx, &n) != Type::I64 {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `math.perm(...)` with a non-int argument — only `int` is supported",
                ctx.fn_name
            )));
        }
        if call.args.len() == 1 {
            return Ok(Expr::Factorial { n: Box::new(n) });
        }
        let k = lower_expr_in_ctx(ctx, call.args[1].clone())?;
        if infer_type_in_ctx(ctx, &k) != Type::I64 {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `math.perm(...)` with a non-int argument — only `int` is supported",
                ctx.fn_name
            )));
        }
        return Ok(Expr::Perm {
            n: Box::new(n),
            k: Box::new(k),
        });
    }
    // PMAT-551/552: `math.factorial(n)` / `math.isqrt(n)` — 1-arg int → int
    // (inline loop blocks). The arg must type as `int`.
    if fn_name == "factorial" || fn_name == "isqrt" {
        if !call.keywords.is_empty() || call.args.len() != 1 {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `math.{fn_name}(...)` with {} positional arg(s){}; v0.2.0 takes exactly 1 int",
                ctx.fn_name,
                call.args.len(),
                if call.keywords.is_empty() { "" } else { " plus keyword args" },
            )));
        }
        let n = lower_expr_in_ctx(ctx, call.args[0].clone())?;
        if infer_type_in_ctx(ctx, &n) != Type::I64 {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `math.{fn_name}(...)` with a non-int argument — only `int` is supported",
                ctx.fn_name
            )));
        }
        let n = Box::new(n);
        return Ok(if fn_name == "factorial" {
            Expr::Factorial { n }
        } else {
            Expr::Isqrt { n }
        });
    }
    // PMAT-502em/en: 2-arg float methods — `math.pow(x, y)` → `(x).powf(y)`,
    // `math.hypot(x, y)` → `(x).hypot(y)`, `math.atan2(y, x)` → `(y).atan2(x)`,
    // `math.log(x, base)` → `(x).log(base)`. All return float (Python's
    // `math.pow` is float even for int args), so both operands are coerced to
    // f64 and reuse `Expr::FloatBinOp`. `math.log` with ONE arg is natural log
    // (`Ln`, handled in the 1-arg table below); only the 2-arg form is here.
    let two_arg_op = match (fn_name, call.args.len()) {
        ("pow", _) => Some(FloatOp::Pow),
        ("hypot", _) => Some(FloatOp::Hypot),
        ("atan2", _) => Some(FloatOp::Atan2),
        ("log", 2) => Some(FloatOp::Log),
        _ => None,
    };
    if let Some(fop) = two_arg_op {
        if !call.keywords.is_empty() || call.args.len() != 2 {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `math.{fn_name}(...)` with {} positional arg(s){}; it takes exactly 2",
                ctx.fn_name,
                call.args.len(),
                if call.keywords.is_empty() { "" } else { " plus keyword args" },
            )));
        }
        let lhs = lower_expr_in_ctx(ctx, call.args[0].clone())?;
        let rhs = lower_expr_in_ctx(ctx, call.args[1].clone())?;
        let lty = infer_type_in_ctx(ctx, &lhs);
        let rty = infer_type_in_ctx(ctx, &rhs);
        if !matches!(lty, Type::I64 | Type::F64) || !matches!(rty, Type::I64 | Type::F64) {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `math.{fn_name}(...)` with a non-numeric argument ({lty:?}, {rty:?})",
                ctx.fn_name
            )));
        }
        return Ok(Expr::FloatBinOp {
            op: fop,
            lhs: Box::new(to_f64_operand(ctx, lhs)),
            rhs: Box::new(to_f64_operand(ctx, rhs)),
        });
    }
    let op = match fn_name {
        "sqrt" => NumBuiltinOp::Sqrt,
        "floor" => NumBuiltinOp::Floor,
        "ceil" => NumBuiltinOp::Ceil,
        // PMAT-502el: trig / exp / log — all 1-arg `f64 → f64`.
        "sin" => NumBuiltinOp::Sin,
        "cos" => NumBuiltinOp::Cos,
        "tan" => NumBuiltinOp::Tan,
        "exp" => NumBuiltinOp::Exp,
        "log" => NumBuiltinOp::Ln,
        "log10" => NumBuiltinOp::Log10,
        "log2" => NumBuiltinOp::Log2,
        // PMAT-502em: `math.trunc(x)` → `(x).trunc() as i64` (returns int).
        "trunc" => NumBuiltinOp::Trunc,
        other => {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `math.{other}(...)` — v0.2.0 supports `math.sqrt`/`floor`/`ceil`/`trunc`/`sin`/`cos`/`tan`/`exp`/`log`/`log10`/`log2`/`pow`/`hypot`/`atan2` (other `math` functions are a follow-up)",
                ctx.fn_name
            )));
        }
    };
    if !call.keywords.is_empty() || call.args.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "function `{}` calls `math.{fn_name}(...)` with {} positional arg(s){}; it takes exactly 1",
            ctx.fn_name,
            call.args.len(),
            if call.keywords.is_empty() {
                ""
            } else {
                " plus keyword args"
            },
        )));
    }
    let arg = lower_expr_in_ctx(ctx, call.args[0].clone())?;
    // PMAT-579: `math.*` builtins are float-domain; `of_float` is unused by
    // codegen for these ops (only `Abs` consults it), but set it from the arg
    // type for consistency.
    let of_float = infer_type_in_ctx(ctx, &arg) == Type::F64;
    Ok(Expr::NumBuiltin {
        op,
        args: vec![arg],
        of_float,
    })
}

/// PMAT-502ei: parse a `key=` argument for `sorted`/`min`/`max` into a
/// [`SortKey`]. Two forms: a simple single-param `lambda p: e`, or a bare
/// callable name (`key=abs`, `key=len`, `key=my_fn`) — the latter is
/// synthesized into the equivalent `lambda __xpile_k: <name>(__xpile_k)` and
/// lowered through the same path (the body is lowered with the param left
/// unbound, matching the lambda case). Returns `Ok(None)` for an unrecognized
/// key shape (the caller then rejects the whole call's kwargs).
/// PMAT-603: true if a sort `key=` lambda returns a `float`. A float key makes
/// the comparison values `f64`, which has no `Ord` — the codegen must then use
/// `sort_by(partial_cmp)` instead of `sort_by_key`/`cmp` (E0277 otherwise). The
/// key body is inferred with its param bound to the collection's element type
/// (same binding `lower_sort_key` uses), so e.g. `key=lambda x: x / 2.0` over a
/// `list[int]` is detected as a float key.
/// PMAT-622: does this type contain a `float` anywhere a keyless sort would
/// compare? A keyless `sorted`/`.sort()` over an element type that embeds an
/// `f64` (a bare float, a tuple with a float, or a nested list of floats) cannot
/// use `Vec::sort` (`f64` is not `Ord` → E0277) and must use the `partial_cmp`
/// path (PMAT-578/616). Recurse through `Tuple` and `List`.
fn type_contains_float(ty: &Type) -> bool {
    match ty {
        Type::F64 => true,
        Type::Tuple(elems) => elems.iter().any(type_contains_float),
        Type::List(inner) => type_contains_float(inner),
        _ => false,
    }
}

fn sort_key_is_float(ctx: &LoweringCtx, key: &SortKey, elem_type: Option<Type>) -> bool {
    let mut sub = ctx.clone();
    if let Some(ty) = elem_type {
        sub.name_types.insert(key.param.clone(), ty);
        sub.bound.insert(key.param.clone());
    }
    infer_type_in_ctx(&sub, &key.body) == Type::F64
}

fn lower_sort_key(
    ctx: &LoweringCtx,
    value: &ast::Expr,
    // PMAT-524: the collection's element type, so the key param types correctly
    // (e.g. `key=lambda p: p[1]` over a `list[tuple[..]]` — `p[1]` must lower to
    // a tuple field access `.1`, not generic `[1]` indexing). `None` leaves the
    // param untyped (defaults to I64, the pre-PMAT-524 behaviour).
    elem_type: Option<Type>,
) -> Result<Option<SortKey>, FrontendError> {
    // Lower the key body with `param` bound to the element type (when known).
    let lower_body = |param: &str, body: ast::Expr| -> Result<Expr, FrontendError> {
        match &elem_type {
            Some(ty) => {
                let mut sub = ctx.clone();
                sub.name_types.insert(param.to_string(), ty.clone());
                sub.bound.insert(param.to_string());
                lower_expr_in_ctx(&sub, body)
            }
            None => lower_expr_in_ctx(ctx, body),
        }
    };
    match value {
        ast::Expr::Lambda(lam)
            if lam.args.args.len() == 1
                && lam.args.posonlyargs.is_empty()
                && lam.args.kwonlyargs.is_empty()
                && lam.args.vararg.is_none()
                && lam.args.kwarg.is_none() =>
        {
            let param = lam.args.args[0].def.arg.to_string();
            let body = lower_body(&param, (*lam.body).clone())?;
            Ok(Some(SortKey {
                param,
                body: Box::new(body),
            }))
        }
        // PMAT-502ei: a bare callable name → `<name>(__xpile_k)`.
        ast::Expr::Name(name) => {
            let param = "__xpile_k".to_string();
            let arg = ast::Expr::Name(ast::ExprName {
                range: name.range,
                id: ast::Identifier::new(param.clone()),
                ctx: ast::ExprContext::Load,
            });
            let synth = ast::Expr::Call(ast::ExprCall {
                range: name.range,
                func: Box::new(value.clone()),
                args: vec![arg],
                keywords: vec![],
            });
            let body = lower_body(&param, synth)?;
            Ok(Some(SortKey {
                param,
                body: Box::new(body),
            }))
        }
        _ => Ok(None),
    }
}

/// PMAT-524: infer the element type of a `sorted`/`min`/`max` collection
/// argument, so a `key=` lambda's parameter types correctly. Mirrors how those
/// handlers materialise the target (range→list, set→list, dict→its keys,
/// str→1-char str). Returns `None` if the type isn't a recognised iterable.
fn sort_target_elem_type(ctx: &LoweringCtx, arg: &ast::Expr) -> Option<Type> {
    let lowered = lower_arg_materializing_range(ctx, arg).ok()?;
    match infer_type_in_ctx(ctx, &lowered) {
        Type::List(e) | Type::Set(e) => Some(*e),
        Type::Dict(k, _) => Some(*k),
        Type::Str => Some(Type::Str),
        _ => None,
    }
}

/// PMAT-502ae / PMAT-502ee: Python's `bool` stringifies to capitalized
/// `"True"`/`"False"`, unlike Rust's lowercase `Display`. `str(b)`, a bool in an
/// f-string, `print(b)`, and `%s` over a bool all desugar to the same
/// `"True" if b else "False"` (`Expr::IfExpr`).
fn bool_to_python_str(value: Expr) -> Expr {
    Expr::IfExpr {
        cond: Box::new(value),
        then_expr: Box::new(Expr::LitStr("True".to_string())),
        else_expr: Box::new(Expr::LitStr("False".to_string())),
    }
}

/// PMAT-623: Python-style `repr` of a single value of type `ty`, used to render
/// list elements. int/float reuse `ToStr`, bool → `True`/`False`, str → quoted
/// `ReprStr`, nested list → recursive list repr. Unsupported element types
/// (dict/set/tuple inside a list) are declined.
fn pyrepr_of(value: Expr, ty: &Type) -> Result<Expr, FrontendError> {
    Ok(match ty {
        Type::I64 => Expr::ToStr {
            value: Box::new(value),
            of_float: false,
        },
        Type::F64 => Expr::ToStr {
            value: Box::new(value),
            of_float: true,
        },
        Type::Bool => bool_to_python_str(value),
        Type::Str => Expr::ReprStr {
            value: Box::new(value),
        },
        Type::List(inner) => build_list_repr(value, inner)?,
        Type::Tuple(elems) => build_tuple_repr(value, elems)?,
        other => {
            return Err(FrontendError::Lower(format!(
                "f-string interpolation of a container with {other:?} elements is not supported \
                 — container repr covers int/float/bool/str/nested list & tuple at v0.2.0"
            )))
        }
    })
}

/// PMAT-624: build Python's tuple `repr` — `"(" + repr(e0) + ", " + repr(e1) +
/// … + ")"`, with the single-element trailing comma (`(42,)`) and `()` for the
/// empty tuple. Heterogeneous element types (so per-position, not `Map`). The
/// tuple is bound once in a `Block` so a side-effecting operand isn't
/// re-evaluated per position. Recursive (a nested tuple/list element reuses
/// `pyrepr_of`). NO new IR.
fn build_tuple_repr(tuple_expr: Expr, elems: &[Type]) -> Result<Expr, FrontendError> {
    let concat = |lhs: Expr, rhs: Expr| Expr::Concat {
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    };
    let tmp = "__tp";
    let let_stmt = Stmt::Let {
        name: tmp.to_string(),
        ty: Type::Tuple(elems.to_vec()),
        value: tuple_expr,
        mutable: false,
    };
    let mut acc = Expr::LitStr("(".to_string());
    for (i, ty) in elems.iter().enumerate() {
        if i > 0 {
            acc = concat(acc, Expr::LitStr(", ".to_string()));
        }
        let elem = pyrepr_of(
            Expr::TupleIndex {
                tuple: Box::new(Expr::Ident(tmp.to_string())),
                index: i,
            },
            ty,
        )?;
        acc = concat(acc, elem);
    }
    // A 1-tuple keeps Python's trailing comma: `(x,)`. Empty tuple → `()`.
    let close = if elems.len() == 1 { ",)" } else { ")" };
    acc = concat(acc, Expr::LitStr(close.to_string()));
    Ok(Expr::Block(Box::new(Block {
        stmts: vec![let_stmt],
        trailing_return: acc,
    })))
}

/// PMAT-623: build `"[" + ", ".join([repr(e) for e in xs]) + "]"` for a
/// `list[elem_ty]` value — Python's list `str`/`repr`. Desugar reusing
/// `Map` + `str.join` + `Concat` + the per-element `pyrepr_of`; recursive for
/// nested lists. NO new IR.
fn build_list_repr(list_expr: Expr, elem_ty: &Type) -> Result<Expr, FrontendError> {
    let body = pyrepr_of(Expr::Ident("__re".to_string()), elem_ty)?;
    let mapped = Expr::Map {
        list: Box::new(list_expr),
        lambda: SortKey {
            param: "__re".to_string(),
            body: Box::new(body),
        },
    };
    // `", ".join(<Vec<String>>)` → `StrMethod{recv: ", ", Join, args: [list]}`.
    let joined = Expr::StrMethod {
        recv: Box::new(Expr::LitStr(", ".to_string())),
        op: StrMethodOp::Join,
        args: vec![mapped],
    };
    Ok(Expr::Concat {
        lhs: Box::new(Expr::LitStr("[".to_string())),
        rhs: Box::new(Expr::Concat {
            lhs: Box::new(joined),
            rhs: Box::new(Expr::LitStr("]".to_string())),
        }),
    })
}

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
    // PMAT-620: a no-default `d.get(k)` in an f-string field is `Option<T>`,
    // which has no `Display` — `f"{d.get(k)}"` emitted `format!("{}", Option)`
    // (E0308: transpile-success → invalid Rust). `str(d.get(k))` and
    // `print(d.get(k))` already reject a bare Optional, so reject the f-string
    // case too (fail-loud, consistent) instead of emitting uncompilable Rust.
    // Rendering a bare Optional to "None"/value is a deferred Optional sub-track.
    if matches!(value, Expr::DictGetOpt { .. }) {
        return Err(FrontendError::Lower(format!(
            "function `{}` interpolates a bare `d.get(k)` (an Optional) in an f-string; \
             rendering an Optional is not supported — use `d.get(k, <default>)`, or guard \
             with `k in d` and index `d[k]`",
            ctx.fn_name
        )));
    }
    let Some(spec_expr) = fv.format_spec.as_ref() else {
        // Plain `{expr}` — no spec; Display-coerced by the surrounding format!.
        // PMAT-502ee/ef: `bool` and `float` fields must render Python-style,
        // not Rust's `Display`: `bool` → `True`/`False` (lowercase in Rust),
        // `float` → Python repr (`3.0`, where Rust's `{}` prints `3`). Both
        // reuse the same conversions `str(bool)` / `str(float)` use, which also
        // makes a lone `f"{x}"` a `Str` (un-deferring it from PMAT-502ed).
        match infer_type_in_ctx(ctx, &value) {
            Type::Bool => return Ok(bool_to_python_str(value)),
            Type::F64 => {
                return Ok(Expr::ToStr {
                    value: Box::new(value),
                    of_float: true,
                })
            }
            // PMAT-623: a list interpolates as its Python repr (`[1, 2, 3]`),
            // not Rust's `Display` (Vec has none → E0277). Desugar to
            // `"[" + ", ".join([repr(e) for e in xs]) + "]"`.
            Type::List(elem) => return build_list_repr(value, elem.as_ref()),
            // PMAT-624: a tuple interpolates as its Python repr (`(1, 2)`, with
            // the `(x,)` single-element comma); tuples have no `Display` (E0277).
            Type::Tuple(elems) => return build_tuple_repr(value, &elems),
            _ => return Ok(value),
        }
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
    apply_nonempty_format_spec(ctx, value, &spec)
}

/// PMAT-597: apply a non-empty Python format spec to an already-lowered value,
/// producing the `Expr::FormatSpec` (or the percent-special). Shared by
/// f-string fields (`f"{x:spec}"`) and the standalone `format(x, "spec")`
/// builtin — the spec mini-language is identical.
fn apply_nonempty_format_spec(
    ctx: &LoweringCtx,
    value: Expr,
    spec: &str,
) -> Result<Expr, FrontendError> {
    let ty = infer_type_in_ctx(ctx, &value);
    // PMAT-558: percent format `:.N%` / `:%` (float). Python scales the value by
    // 100, formats it with N decimals (bare `%` → Python's default 6), and
    // appends a literal `%`. Lowered to `Concat(FormatSpec((x)*100.0, ".N"),
    // "%")` — no new IR. Only sound for a float (whole-int promotion deferred).
    if let Some(prec) = spec.strip_suffix('%') {
        if ty == Type::F64 {
            let rust_spec = if prec.is_empty() {
                Some(".6".to_string())
            } else if let Some(n) = prec.strip_prefix('.') {
                digits_only(n).then(|| format!(".{n}"))
            } else {
                None
            };
            if let Some(rust_spec) = rust_spec {
                let scaled = Expr::FloatBinOp {
                    op: FloatOp::Mul,
                    lhs: Box::new(value),
                    rhs: Box::new(Expr::LitFloat(100.0)),
                };
                return Ok(Expr::Concat {
                    lhs: Box::new(Expr::FormatSpec {
                        value: Box::new(scaled),
                        rust_spec,
                    }),
                    rhs: Box::new(Expr::LitStr("%".to_string())),
                });
            }
        }
        // Non-float receiver or malformed precision → fall through to a reject.
    }
    // PMAT-613: a BARE radix spec (`x`/`X`/`b`/`o`, no width/fill/precision)
    // over an int. Python formats negatives **sign-magnitude** (`f"{-255:x}"`
    // == "-ff"), but Rust's `format!("{:x}", n)` is two's-complement
    // (`ffffffffffffff01`) → silent wrong output. Reuse `IntRadixStr`
    // (`prefixed: false`), which emits `sign + format(unsigned_abs)`, matching
    // Python (and the `hex`/`bin`/`oct` builtins). Radix-WITH-width keeps the
    // `FormatSpec` path (correct for non-negatives; sign-aware zero-padding of a
    // negative is a deferred follow-up).
    if ty == Type::I64 {
        let radix_upper = match spec {
            "x" => Some((Radix::Hex, false)),
            "X" => Some((Radix::Hex, true)),
            "b" => Some((Radix::Bin, false)),
            "o" => Some((Radix::Oct, false)),
            _ => None,
        };
        if let Some((radix, upper)) = radix_upper {
            return Ok(Expr::IntRadixStr {
                value: Box::new(value),
                radix,
                prefixed: false,
                upper,
            });
        }
    }
    match translate_format_spec(spec, &ty) {
        Some(rust_spec) => Ok(Expr::FormatSpec {
            value: Box::new(value),
            rust_spec,
        }),
        None => Err(FrontendError::Lower(format!(
            "unsupported format spec `:{spec}` (for a {ty:?} value) — supported: \
             `.Nf` (float), `.N%` (float percent), `0Nd`/`Nd` (int), `>N`/`<N`/`^N` (align), \
             `+`/`-` (sign) at v0.2.0"
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
        if step_is_neg_one && slice.lower.is_none() && slice.upper.is_none() {
            if of_str {
                // PMAT-530: `s[::-1]` over a `str` → a new reversed string.
                // Reuses the `StrMethod` pipeline (`Reverse` op → `.chars()
                // .rev().collect::<String>()`), mirroring the list reverse below.
                return Ok(Expr::StrMethod {
                    recv: Box::new(collection),
                    op: StrMethodOp::Reverse,
                    args: vec![],
                });
            }
            return Ok(Expr::Reversed {
                list: Box::new(collection),
            });
        }
        // PMAT-502bc / PMAT-548: an integer-literal step over a *list*. A
        // positive step keeps `xs[a:b:c]` (1 is the default, dropped). A
        // **negative** step `xs[::-k]` (k ≥ 2) over a list with NO bounds
        // reverses then steps (codegen emits `.iter().rev().step_by(|k|)`); the
        // `xs[::-1]` reverse is the special case handled above. Bounded
        // negative-step slices (different start/stop defaults) and stepped
        // string slices remain deferred.
        match extract_step_literal(step) {
            Some(s) if s < 0 => {
                if of_str || slice.lower.is_some() || slice.upper.is_some() {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` uses a negative-step slice with bounds or over a `str`; \
                         v0.2.0 supports only the unbounded list form `xs[::-k]` (and `xs[::-1]`/`s[::-1]`)",
                        ctx.fn_name
                    )));
                }
                step_lit = Some(s); // negative → codegen reverses + steps
            }
            Some(s) if s >= 1 => {
                if of_str {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` uses a stepped string slice — deferred at v0.2.0",
                        ctx.fn_name
                    )));
                }
                step_lit = if s == 1 { None } else { Some(s) };
            }
            // s == 0 or non-literal step.
            _ => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` uses a zero or non-literal slice step; v0.2.0 requires a non-zero integer literal step",
                    ctx.fn_name
                )));
            }
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
            // PMAT-565: bool operand in int arithmetic → coerce to i64 (context-
            // free counterpart; recognises bool *literals*).
            // PMAT-580: `&`/`|`/`^` over two bools stays a bool op (no coercion).
            let both_bool = matches!(op, BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor)
                && infer_type(&lhs) == Type::Bool
                && infer_type(&rhs) == Type::Bool;
            let (lhs, rhs) = if is_int_arith_binop(op) && !both_bool {
                (to_i64_operand_cf(lhs), to_i64_operand_cf(rhs))
            } else {
                (lhs, rhs)
            };
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
        // PMAT-502cp: tuple literal in context-free position (e.g. a list
        // element `[(1, 2), (3, 4)]`) → `Expr::TupleLit`. The ctx-aware path
        // has its own Tuple arm; this mirror lets tuple literals appear as
        // list elements (which lower context-free).
        ast::Expr::Tuple(t) => {
            let elems = t
                .elts
                .into_iter()
                .map(lower_expr)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Expr::TupleLit(elems))
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
    // PMAT-502ed: stringify a lone `int` field (see the ctx-aware twin).
    let ty = infer_type(&acc);
    Ok(stringify_lone_fstring_field(acc, ty))
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

/// PMAT-502am / PMAT-502ed: translate a static Python format spec (the part
/// after `:` in `{x:...}`) to the equivalent Rust spec (the part after `:` in
/// `{:...}`), or `None` if unsupported. Python and Rust share nearly identical
/// fill/align/zero/width/precision/type mini-languages, so most forms pass
/// through; the exceptions handled here are Python's `.Nf` (Rust `.N`) and `d`
/// (decimal — Rust has no `d` type letter, so it is dropped). Thousands
/// separators (comma / underscore), sign flags, and `#` alternate forms are
/// deferred.
fn translate_format_spec(spec: &str, ty: &Type) -> Option<String> {
    // PMAT-557: Python sign flag. `+` (always show a sign) maps 1:1 to Rust's
    // `+` flag, which composes with precision / width / zero-pad / radix exactly
    // like Python (`{:+}`, `{:+.2}`, `{:+05}`, `{:+x}`). `-` is the default in
    // both (show only for negatives) → drop it and translate the remainder. A
    // space flag (` `) has no Rust equivalent → fall through to a reject.
    //
    // A *bare* sign (no precision/width following) is only safe for an int: a
    // bare-`+`/`-` on a float hits the whole-float repr divergence (`+3` vs
    // Python `+3.0`), the same reason bare float widths are deferred; only an
    // explicit `.Nf` precision (which forces the decimals) is sound there.
    if let Some(rest) = spec.strip_prefix('+') {
        if rest.is_empty() {
            return (*ty == Type::I64).then(|| "+".to_string());
        }
        let inner = translate_format_spec(rest, ty)?;
        return Some(format!("+{inner}"));
    }
    if let Some(rest) = spec.strip_prefix('-') {
        if rest.is_empty() {
            return (*ty == Type::I64).then(String::new);
        }
        return translate_format_spec(rest, ty);
    }
    // `.Nf` — fixed-point float (float only). A `.`-prefixed spec is
    // float-specific; never fall through to the integer/width branches (Python
    // `.2` without `f` means *significant figures*, not Rust's decimal places).
    if let Some(rest) = spec.strip_prefix('.') {
        if let Some(n) = rest.strip_suffix('f') {
            if *ty == Type::F64 && digits_only(n) {
                return Some(format!(".{n}"));
            }
        }
        return None;
    }
    // Explicit alignment `[<>^]N` (any Display type) — Rust uses the same
    // `[align][width]` syntax, so pass it through verbatim.
    if let Some(align) = spec.chars().next() {
        if matches!(align, '<' | '>' | '^') {
            let width = &spec[align.len_utf8()..];
            return digits_only(width).then(|| spec.to_string());
        }
    }
    // Integer specs (int only — Rust's syntax AND default right-alignment match
    // Python for ints, and int repr is identical). A radix char `x`/`X`/`b`/`o`
    // maps 1:1 to Rust, `d` (decimal) is dropped; each takes an optional
    // `[0]width` pad. A bare `[0]width` (`5`, `05`) is width / zero-pad. Float
    // bare-width and bool are deferred — Rust and Python disagree on whole-float
    // repr (`3.0` vs `3`) and bool repr (`true` vs `True`); only `.Nf` (float)
    // and explicit alignment (above) are safe there.
    if *ty == Type::I64 {
        if let Some(last) = spec.chars().last() {
            if matches!(last, 'x' | 'X' | 'b' | 'o') {
                let prefix = &spec[..spec.len() - last.len_utf8()];
                return pad_width(prefix).map(|pad| format!("{pad}{last}"));
            }
            if last == 'd' {
                // PMAT-557: `d` (decimal) is the int default in Rust, so it's
                // dropped; a bare `:d` (or sign-stripped `:+d`) → an empty spec
                // (plain `{}`), not a reject.
                return pad_width(&spec[..spec.len() - 1]);
            }
        }
        return pad_width(spec).filter(|p| !p.is_empty());
    }
    None
}

/// `[0][digits]` → the matching Rust pad/width string. Empty input yields
/// `Some("")` (the "no width" case, valid after a radix char like `:x`); a lone
/// `0` (zero flag, no width) or any non-digit width yields `None`.
fn pad_width(s: &str) -> Option<String> {
    if s.is_empty() {
        return Some(String::new());
    }
    let (zero, width) = match s.strip_prefix('0') {
        Some(w) => ("0", w),
        None => ("", s),
    };
    digits_only(width).then(|| format!("{zero}{width}"))
}

fn digits_only(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
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
            // PMAT-502fb: Python `~x` == `-(x+1)` == Rust `!x` on a signed int.
            if infer_type(&operand) != Type::I64 {
                return Err(FrontendError::Lower(
                    "bitwise `~` requires an I64 operand".into(),
                ));
            }
            UnOp::BitNot
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
        let inner = args.pop().unwrap();
        // PMAT-564: context-free `len(str-literal)` also counts chars not bytes.
        if infer_type(&inner) == Type::Str {
            return Ok(Expr::StrMethod {
                recv: Box::new(inner),
                op: StrMethodOp::CharCount,
                args: vec![],
            });
        }
        return Ok(Expr::Len(Box::new(inner)));
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
    // PMAT-542: mixed float/int ternary promotes the int branch to f64 (mirrors
    // the context-aware variant). Here `infer_type` only sees float *literals*.
    let (then_expr, else_expr) = if then_ty == Type::F64 && else_ty == Type::I64 {
        (then_expr, to_f64_operand_cf(else_expr))
    } else if then_ty == Type::I64 && else_ty == Type::F64 {
        (to_f64_operand_cf(then_expr), else_expr)
    } else {
        (then_expr, else_expr)
    };
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

/// PMAT-502db: context-aware variant of [`lower_if_exp`]. Lowers the
/// condition + both branches with `lower_expr_in_ctx` so a builtin or a
/// typed-variable expression in a ternary branch sees its real type — the
/// context-free version silently miscompiles `abs(n) if … else …` to an
/// undefined Rust `abs(...)` because it can't recognize the builtin without
/// the type context.
fn lower_if_exp_in_ctx(ctx: &LoweringCtx, ie: ast::ExprIfExp) -> Result<Expr, FrontendError> {
    // Python AST order is (test, body, orelse) = (cond, then, else).
    let cond = truthy_condition(ctx, lower_expr_in_ctx(ctx, *ie.test)?);
    let then_expr = lower_expr_in_ctx(ctx, *ie.body)?;
    let else_expr = lower_expr_in_ctx(ctx, *ie.orelse)?;

    let then_ty = infer_type_in_ctx(ctx, &then_expr);
    let else_ty = infer_type_in_ctx(ctx, &else_expr);
    // PMAT-542: a mixed `float`/`int` ternary promotes the int branch to f64 —
    // Python yields a float when either branch is float (`x if b else 0` over a
    // float `x`), and Rust requires both arms of an `if`-expression to share a
    // type. `to_f64_operand` is a no-op when already f64.
    let (then_expr, else_expr) = if then_ty == Type::F64 && else_ty == Type::I64 {
        (then_expr, to_f64_operand(ctx, else_expr))
    } else if then_ty == Type::I64 && else_ty == Type::F64 {
        (to_f64_operand(ctx, then_expr), else_expr)
    } else {
        (then_expr, else_expr)
    };
    let then_ty = infer_type_in_ctx(ctx, &then_expr);
    let else_ty = infer_type_in_ctx(ctx, &else_expr);
    if then_ty != else_ty {
        return Err(FrontendError::Lower(format!(
            "ternary branches have mismatched types ({then_ty:?} vs {else_ty:?}); both must agree at v0.1.0"
        )));
    }

    if infer_type_in_ctx(ctx, &cond) != Type::Bool {
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
            from_float: false,
        }
    }
}

/// PMAT-565: is `op` an integer-arithmetic / bitwise binary operator (one whose
/// i64 lowering needs both operands to be `i64`)? Comparisons (`==`,`<`,…) and
/// the boolean `and`/`or` are excluded — those accept/produce `bool` directly.
fn is_int_arith_binop(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::FloorDiv
            | BinOp::Mod
            | BinOp::Pow
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
    )
}

/// PMAT-565: coerce a `bool` operand to `i64` in an integer context — Python's
/// `bool` is an `int` subtype (`True == 1`), so `True + True == 2`. Without this
/// the i64-arith lowering emits `(a).checked_add(b)` on a `bool` (invalid Rust).
/// A no-op for any non-bool operand. `bool as i64` is a valid Rust cast.
fn to_i64_operand(ctx: &LoweringCtx, e: Expr) -> Expr {
    if infer_type_in_ctx(ctx, &e) == Type::Bool {
        Expr::NumCast {
            value: Box::new(e),
            to_float: false,
            from_str: false,
            from_float: false,
        }
    } else {
        e
    }
}

/// PMAT-617: unconditionally wrap an expression KNOWN to be `bool` in a
/// `bool -> i64` cast (`(b) as i64`). Unlike [`to_i64_operand`] this does not
/// re-infer the type, so it is safe to apply to a `__cmpN` chained-comparison
/// temp that is not registered in the lowering context.
fn bool_to_i64_cast(e: Expr) -> Expr {
    Expr::NumCast {
        value: Box::new(e),
        to_float: false,
        from_str: false,
        from_float: false,
    }
}

/// Context-free counterpart of [`to_i64_operand`] (recognises bool *literals*).
fn to_i64_operand_cf(e: Expr) -> Expr {
    if infer_type(&e) == Type::Bool {
        Expr::NumCast {
            value: Box::new(e),
            to_float: false,
            from_str: false,
            from_float: false,
        }
    } else {
        e
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
            from_float: false,
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

/// PMAT-536: lower the keyword (named-field) form `"<fmt>".format(name=val, …)`.
/// Named `{name}` / `{name:spec}` placeholders are rewritten to positional
/// `{N}` / `{N:spec}` — `N` is the field's index in first-occurrence order, so a
/// repeated `{name}` reuses the same index (which Rust's positional `{N}`
/// supports but auto `{}` does not). Only the fields actually referenced by the
/// template become positional args (Rust's `format!` rejects an unused arg,
/// while Python tolerates an unused kwarg). The rewritten template + ordered
/// values are handed to [`lower_str_format`], reusing all its spec translation
/// and per-type validation. `**kwargs`, auto `{}` / positional `{N}` fields
/// (no positional args are passed here), and unknown field names are rejected.
fn lower_str_format_kwargs(
    ctx: &LoweringCtx,
    fmt: &str,
    keywords: &[ast::Keyword],
) -> Result<Expr, FrontendError> {
    let mut kw: std::collections::HashMap<String, ast::Expr> = std::collections::HashMap::new();
    for k in keywords {
        let Some(name) = k.arg.as_ref().map(|a| a.to_string()) else {
            return Err(FrontendError::Lower(format!(
                "function `{}` calls `.format(**kwargs)` — `**` keyword unpacking is not supported",
                ctx.fn_name
            )));
        };
        kw.insert(name, k.value.clone());
    }
    let mut used: Vec<String> = Vec::new();
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push_str("{{");
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push_str("}}");
            }
            '}' => {
                return Err(FrontendError::Lower(format!(
                    "function `{}` has an unmatched `}}` in a `.format(...)` template",
                    ctx.fn_name
                )));
            }
            '{' => {
                let mut field = String::new();
                while let Some(&nc) = chars.peek() {
                    if nc == '}' {
                        break;
                    }
                    field.push(nc);
                    chars.next();
                }
                if chars.next() != Some('}') {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` has an unterminated `{{` in a `.format(...)` template",
                        ctx.fn_name
                    )));
                }
                let (name, spec) = match field.split_once(':') {
                    Some((n, s)) => (n, Some(s)),
                    None => (field.as_str(), None),
                };
                if name.is_empty() || name.chars().all(|ch| ch.is_ascii_digit()) {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` mixes auto/positional `{{}}` fields with `.format(name=...)` keyword args — use named `{{field}}` placeholders",
                        ctx.fn_name
                    )));
                }
                if !kw.contains_key(name) {
                    return Err(FrontendError::Lower(format!(
                        "function `{}` `.format(...)` template references `{{{name}}}` with no matching keyword arg",
                        ctx.fn_name
                    )));
                }
                let idx = match used.iter().position(|u| u == name) {
                    Some(i) => i,
                    None => {
                        used.push(name.to_string());
                        used.len() - 1
                    }
                };
                match spec {
                    Some(s) => out.push_str(&format!("{{{idx}:{s}}}")),
                    None => out.push_str(&format!("{{{idx}}}")),
                }
            }
            other => out.push(other),
        }
    }
    let ordered_values: Vec<ast::Expr> = used.iter().map(|n| kw[n].clone()).collect();
    lower_str_format(ctx, &out, &ordered_values)
}

/// PMAT-502bh/cb/ch: lower a Python `"<fmt>".format(args…)` to an
/// [`Expr::StrFormat`]. Supports automatic `{}` and positional `{N}` fields,
/// each with an optional format spec `{:.2f}` / `{N:>5}` (PMAT-502ch). The
/// template is re-built into a Rust format string (`{{`/`}}` escapes and
/// literal text preserved; each Python spec translated to its Rust form by
/// the argument's type via [`translate_format_spec`]). Mixing automatic and
/// manual numbering is rejected (per Python); `{name}` fields are deferred
/// (they need keyword args). A spec-less field requires an `I64`/`Str` arg
/// (a `bool`/`float` `Display`s differently than Python); a float arg is
/// admitted when it carries a `.Nf` spec. Every argument must be referenced
/// (Rust's `format!` rejects an unused one). Braces are ASCII, so the
/// byte-walk with string-slice copies is UTF-8-safe.
/// PMAT-502dm: lower printf-style `"<template>" % args` to an
/// [`Expr::StrFormat`]. The template's `%`-conversions are translated to a
/// Rust `format!` string. First cut supports `%s` (over `int`/`str` — `bool`
/// and `float` diverge under Rust's `{}` so they're deferred), `%d`/`%i`
/// (int), `%f` (float → `{:.6}`, Python's default precision), and `%%`. Width,
/// precision, flags, and `%x`/`%X`/`%o` (Rust `{:x}` is two's-complement for
/// negatives, unlike Python's sign-first) are rejected with a clear error. The
/// RHS is a single value or a tuple of values, matched left-to-right.
fn lower_percent_format(
    ctx: &LoweringCtx,
    tmpl: &str,
    rhs: &ast::Expr,
) -> Result<Expr, FrontendError> {
    // The RHS is either a tuple of values or a single value.
    let raw_args: Vec<ast::Expr> = match rhs {
        ast::Expr::Tuple(t) => t.elts.clone(),
        other => vec![other.clone()],
    };
    let mut args = raw_args
        .into_iter()
        .map(|a| lower_expr_in_ctx(ctx, a))
        .collect::<Result<Vec<_>, _>>()?;

    let mut fmt = String::new();
    let mut arg_idx = 0usize;
    let mut chars = tmpl.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // Escape Rust `format!`'s own metacharacters.
            '{' => fmt.push_str("{{"),
            '}' => fmt.push_str("}}"),
            '%' => {
                // Parse the optional `[flags][width][.precision]` mini-language
                // (PMAT-502dn). Flags `-` (left), `0` (zero-pad), `+` (sign).
                // ` ` and `#` are deferred (rejected).
                let (mut flag_left, mut flag_zero, mut flag_plus) = (false, false, false);
                loop {
                    match chars.peek() {
                        Some('-') => flag_left = true,
                        Some('0') => flag_zero = true,
                        Some('+') => flag_plus = true,
                        Some(' ') | Some('#') => {
                            return Err(FrontendError::Lower(
                                "unsupported `%`-format flag (' '/'#') — not yet supported".into(),
                            ));
                        }
                        _ => break,
                    }
                    chars.next();
                }
                let mut width = String::new();
                while matches!(chars.peek(), Some(d) if d.is_ascii_digit()) {
                    width.push(chars.next().unwrap());
                }
                let mut precision: Option<String> = None;
                if chars.peek() == Some(&'.') {
                    chars.next();
                    let mut p = String::new();
                    while matches!(chars.peek(), Some(d) if d.is_ascii_digit()) {
                        p.push(chars.next().unwrap());
                    }
                    precision = Some(p);
                }
                let conv = chars
                    .next()
                    .ok_or_else(|| FrontendError::Lower("trailing `%` in format string".into()))?;
                if conv == '%' {
                    if flag_left
                        || flag_zero
                        || flag_plus
                        || !width.is_empty()
                        || precision.is_some()
                    {
                        return Err(FrontendError::Lower(
                            "`%%` does not take flags/width/precision".into(),
                        ));
                    }
                    fmt.push('%');
                    continue;
                }
                if arg_idx >= args.len() {
                    return Err(FrontendError::Lower(
                        "not enough arguments for `%` format string".into(),
                    ));
                }
                let ty = infer_type_in_ctx(ctx, &args[arg_idx]);
                // Validate conversion ↔ argument type, and which conversions
                // accept a precision (Rust ignores precision on integers, so
                // `%.Nd` and `%.Ns`-over-int would diverge → rejected).
                match conv {
                    's' if matches!(ty, Type::Str) => {}
                    's' if matches!(ty, Type::I64) => {
                        if precision.is_some() {
                            return Err(FrontendError::Lower(
                                "`%.Ns` over an int is not supported (precision truncation \
                                 differs from Rust)"
                                    .into(),
                            ));
                        }
                    }
                    // PMAT-502do: `%s` over bool/float — str()-convert the arg
                    // first (bool → "True"/"False", float → Python repr) so the
                    // `{}` placeholder yields Python's `str(x)`. Width/precision
                    // then apply to the resulting `String` (matching Python).
                    's' if matches!(ty, Type::Bool) => {
                        let v = args[arg_idx].clone();
                        args[arg_idx] = Expr::IfExpr {
                            cond: Box::new(v),
                            then_expr: Box::new(Expr::LitStr("True".to_string())),
                            else_expr: Box::new(Expr::LitStr("False".to_string())),
                        };
                    }
                    's' if matches!(ty, Type::F64) => {
                        let v = args[arg_idx].clone();
                        args[arg_idx] = Expr::ToStr {
                            value: Box::new(v),
                            of_float: true,
                        };
                    }
                    'd' | 'i' if matches!(ty, Type::I64) => {
                        if precision.is_some() {
                            return Err(FrontendError::Lower(
                                "`%.Nd` (integer precision) is not yet supported".into(),
                            ));
                        }
                    }
                    'f' if matches!(ty, Type::F64) => {}
                    // PMAT-502dp: `%x`/`%X`/`%o` over an int — wrap the arg as a
                    // *no-prefix* sign-first radix string (Rust's `{:x}` is
                    // two's-complement for negatives; Python is sign-first), then
                    // render it via `{}`. Only an optional width is allowed —
                    // `0`/`+`/precision on the resulting `String` would diverge.
                    'x' | 'X' | 'o' if matches!(ty, Type::I64) => {
                        if precision.is_some() || flag_zero || flag_plus {
                            return Err(FrontendError::Lower(
                                "`%x`/`%X`/`%o` support only an optional width (precision, `0`, \
                                 and `+` are not yet supported)"
                                    .into(),
                            ));
                        }
                        let radix = if conv == 'o' { Radix::Oct } else { Radix::Hex };
                        let v = args[arg_idx].clone();
                        args[arg_idx] = Expr::IntRadixStr {
                            value: Box::new(v),
                            radix,
                            prefixed: false,
                            upper: conv == 'X',
                        };
                    }
                    's' | 'd' | 'i' | 'f' | 'x' | 'X' | 'o' => {
                        return Err(FrontendError::Lower(format!(
                            "`%{conv}` format expects a different argument type than {ty:?} \
                             (`%s` str/bool/float, `%d`/`%x`/`%X`/`%o` int, `%f` float)"
                        )));
                    }
                    _ => {
                        return Err(FrontendError::Lower(format!(
                            "unsupported `%{conv}` conversion"
                        )));
                    }
                }
                // `%f` defaults to 6 decimals when no precision is given.
                let prec = if conv == 'f' {
                    Some(precision.unwrap_or_else(|| "6".to_string()))
                } else {
                    precision
                };
                let bare =
                    width.is_empty() && prec.is_none() && !flag_left && !flag_zero && !flag_plus;
                if bare {
                    fmt.push_str("{}");
                } else {
                    // Assemble a Rust spec `{:[align][sign][0][width][.prec]}`.
                    // Python right-aligns by default (incl. `%Ns`, where Rust
                    // would left-align) → emit an explicit `>` unless `-`/`0`.
                    let mut spec = String::from("{:");
                    if flag_zero && !flag_left {
                        if flag_plus {
                            spec.push('+');
                        }
                        spec.push('0');
                    } else if flag_left {
                        spec.push('<');
                        if flag_plus {
                            spec.push('+');
                        }
                    } else {
                        if !width.is_empty() {
                            spec.push('>');
                        }
                        if flag_plus {
                            spec.push('+');
                        }
                    }
                    spec.push_str(&width);
                    if let Some(p) = &prec {
                        spec.push('.');
                        spec.push_str(p);
                    }
                    spec.push('}');
                    fmt.push_str(&spec);
                }
                arg_idx += 1;
            }
            _ => fmt.push(c),
        }
    }
    if arg_idx != args.len() {
        return Err(FrontendError::Lower(
            "not all arguments converted during `%` string formatting".into(),
        ));
    }
    Ok(Expr::StrFormat { fmt, args })
}

fn lower_str_format(
    ctx: &LoweringCtx,
    fmt: &str,
    raw_args: &[ast::Expr],
) -> Result<Expr, FrontendError> {
    let fname = ctx.fn_name.clone();
    // Lower the args first so specs can be translated against their types.
    let mut args = Vec::with_capacity(raw_args.len());
    let mut arg_tys = Vec::with_capacity(raw_args.len());
    for a in raw_args {
        let lo = lower_expr_in_ctx(ctx, a.clone())?;
        arg_tys.push(infer_type_in_ctx(ctx, &lo));
        args.push(lo);
    }
    let nargs = args.len();
    let b = fmt.as_bytes();
    let mut rust_fmt = String::new();
    let mut i = 0;
    let mut lit_start = 0;
    let mut auto_ctr = 0usize;
    let mut used = vec![false; nargs];
    let mut saw_auto = false;
    let mut saw_pos = false;
    while i < b.len() {
        let c = b[i];
        if c != b'{' && c != b'}' {
            i += 1;
            continue;
        }
        rust_fmt.push_str(&fmt[lit_start..i]);
        // `{{` / `}}` brace escapes.
        if c == b'{' && b.get(i + 1) == Some(&b'{') {
            rust_fmt.push_str("{{");
            i += 2;
            lit_start = i;
            continue;
        }
        if c == b'}' && b.get(i + 1) == Some(&b'}') {
            rust_fmt.push_str("}}");
            i += 2;
            lit_start = i;
            continue;
        }
        if c == b'}' {
            return Err(FrontendError::Lower(format!(
                "function `{fname}` has a lone `}}` in a str.format template — use `}}}}` to emit a literal brace"
            )));
        }
        // A real `{…}` placeholder — find its closing `}`.
        let close = fmt[i + 1..].find('}').map(|p| i + 1 + p).ok_or_else(|| {
            FrontendError::Lower(format!(
                "function `{fname}` has an unmatched `{{` in a str.format template"
            ))
        })?;
        let inner = &fmt[i + 1..close];
        let (field_str, spec) = match inner.split_once(':') {
            Some((f, s)) => (f, Some(s)),
            None => (inner, None),
        };
        let arg_idx = if field_str.is_empty() {
            saw_auto = true;
            let idx = auto_ctr;
            auto_ctr += 1;
            idx
        } else if let Ok(n) = field_str.parse::<usize>() {
            saw_pos = true;
            n
        } else {
            return Err(FrontendError::Lower(format!(
                "function `{fname}` uses a named str.format field `{{{field_str}}}` — keyword `.format(name=…)` is deferred at v0.2.0"
            )));
        };
        if arg_idx >= nargs {
            return Err(FrontendError::Lower(format!(
                "function `{fname}` references str.format field `{{{arg_idx}}}` but only {nargs} arg(s) were given"
            )));
        }
        used[arg_idx] = true;
        rust_fmt.push('{');
        rust_fmt.push_str(field_str);
        match spec {
            None => {
                if !matches!(arg_tys[arg_idx], Type::I64 | Type::Str) {
                    return Err(FrontendError::Lower(format!(
                        "function `{fname}` formats a {:?} value via str.format without a spec; v0.2.0 supports int/str (a bool/float needs a spec, e.g. `{{:.2f}}`)",
                        arg_tys[arg_idx]
                    )));
                }
            }
            Some(s) => {
                let rust_spec = translate_format_spec(s, &arg_tys[arg_idx]).ok_or_else(|| {
                    FrontendError::Lower(format!(
                        "function `{fname}` uses an unsupported str.format spec `{{:{s}}}` for a {:?} value",
                        arg_tys[arg_idx]
                    ))
                })?;
                rust_fmt.push(':');
                rust_fmt.push_str(&rust_spec);
            }
        }
        rust_fmt.push('}');
        i = close + 1;
        lit_start = i;
    }
    rust_fmt.push_str(&fmt[lit_start..]);
    if saw_auto && saw_pos {
        return Err(FrontendError::Lower(format!(
            "function `{fname}` mixes automatic `{{}}` and manual `{{0}}` str.format fields — Python forbids switching numbering"
        )));
    }
    for (k, was_used) in used.iter().enumerate() {
        if !was_used {
            return Err(FrontendError::Lower(format!(
                "function `{fname}` calls str.format with {nargs} arg(s) but never references arg {k} — Rust's format! requires every argument be used"
            )));
        }
    }
    Ok(Expr::StrFormat {
        fmt: rust_fmt,
        args,
    })
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
        "rfind" => Some(StrMethodOp::Rfind),
        "rindex" => Some(StrMethodOp::RIndex),
        "count" => Some(StrMethodOp::Count),
        // PMAT-502bi: str.index (1-arg, → Int; panics if absent = ValueError).
        "index" => Some(StrMethodOp::StrIndex),
        // PMAT-502ag: classification predicates (0-arg).
        "isdigit" => Some(StrMethodOp::IsDigit),
        "isalpha" => Some(StrMethodOp::IsAlpha),
        "isspace" => Some(StrMethodOp::IsSpace),
        // PMAT-502di: more classification predicates (0-arg).
        "isalnum" => Some(StrMethodOp::IsAlnum),
        "isupper" => Some(StrMethodOp::IsUpper),
        "islower" => Some(StrMethodOp::IsLower),
        // PMAT-502dj: partition/rpartition (1-arg → 3-tuple).
        "partition" => Some(StrMethodOp::Partition),
        "rpartition" => Some(StrMethodOp::RPartition),
        // PMAT-502dl: splitlines (0-arg → list[str]).
        "splitlines" => Some(StrMethodOp::SplitLines),
        // PMAT-502ah: capitalize (0-arg).
        "capitalize" => Some(StrMethodOp::Capitalize),
        "title" => Some(StrMethodOp::Title),
        // PMAT-502aw: rjust/ljust (1-arg width).
        "rjust" => Some(StrMethodOp::RJust),
        "ljust" => Some(StrMethodOp::LJust),
        // PMAT-502cq: removeprefix/removesuffix (1-arg).
        "removeprefix" => Some(StrMethodOp::RemovePrefix),
        "removesuffix" => Some(StrMethodOp::RemoveSuffix),
        // PMAT-502cr: swapcase (0-arg).
        "swapcase" => Some(StrMethodOp::SwapCase),
        // PMAT-502cs: zfill (1-arg width).
        "zfill" => Some(StrMethodOp::ZFill),
        // PMAT-502cu: center (1-arg width).
        "center" => Some(StrMethodOp::Center),
        _ => None,
    }
}

/// Number of arguments a [`StrMethodOp`] expects: 0 for the transforms,
/// 1 for the predicates / `split` / `join`, 2 for `replace(old, new)`.
fn str_method_arity(op: StrMethodOp) -> usize {
    match op {
        StrMethodOp::Upper | StrMethodOp::Lower | StrMethodOp::Strip => 0,
        // PMAT-564: `len(str)` char count takes no args.
        StrMethodOp::CharCount => 0,
        // PMAT-502co: no-arg whitespace split.
        StrMethodOp::SplitWhitespace => 0,
        StrMethodOp::StartsWith
        | StrMethodOp::EndsWith
        | StrMethodOp::Split
        | StrMethodOp::Join => 1,
        StrMethodOp::Replace => 2,
        // PMAT-517/518: `replace(old, new, count)` / `split(sep, maxsplit)` —
        // routed via dedicated branches, but kept here for arity completeness.
        StrMethodOp::ReplaceN => 3,
        StrMethodOp::SplitN => 2,
        // PMAT-502l: lstrip/rstrip take no args; find/count take one.
        StrMethodOp::LStrip | StrMethodOp::RStrip => 0,
        StrMethodOp::Find
        | StrMethodOp::Rfind
        | StrMethodOp::RIndex
        | StrMethodOp::Count
        | StrMethodOp::StrIndex => 1,
        // PMAT-502ag/502di: classification predicates take no args.
        StrMethodOp::IsDigit
        | StrMethodOp::IsAlpha
        | StrMethodOp::IsSpace
        | StrMethodOp::IsAlnum
        | StrMethodOp::IsUpper
        | StrMethodOp::IsLower => 0,
        // PMAT-502ah: capitalize takes no args.
        StrMethodOp::Capitalize | StrMethodOp::Title => 0,
        // PMAT-502aw: rjust/ljust take one width arg.
        StrMethodOp::RJust | StrMethodOp::LJust => 1,
        // PMAT-502cq: removeprefix/removesuffix take one arg.
        StrMethodOp::RemovePrefix | StrMethodOp::RemoveSuffix => 1,
        // PMAT-502cr: swapcase takes no args.
        StrMethodOp::SwapCase => 0,
        // PMAT-502cs: zfill takes one width arg.
        StrMethodOp::ZFill => 1,
        // PMAT-502cu: center takes one width arg.
        StrMethodOp::Center => 1,
        // PMAT-502dj: partition/rpartition take one separator arg.
        StrMethodOp::Partition | StrMethodOp::RPartition => 1,
        // PMAT-502dl: splitlines takes no args (keepends deferred).
        StrMethodOp::SplitLines => 0,
        // PMAT-530: s[::-1] reverse — synthesized (no surface method), 0 args.
        StrMethodOp::Reverse => 0,
    }
}

/// PMAT-502k: detect Python sequence repetition `seq * n` / `n * seq`
/// (one operand a `Str`/`List`, the other an `Int`). Returns the
/// `Expr::Repeat`, trying both operand orders, or `None` when the pair
/// isn't (sequence, int). Caller only invokes this for the `*` operator.
/// PMAT-556: a block's result type. When the trailing expression is an `Ident`
/// bound by a `Let` *inside* the block (e.g. the accumulator built by a
/// two-generator comprehension), the enclosing scope can't see that local, so
/// inferring the bare `Ident` would default to `I64`. Recover the type from the
/// block's own `Let` binding; otherwise infer the trailing expression directly.
fn block_result_type(b: &Block, infer: impl Fn(&Expr) -> Type) -> Type {
    if let Expr::Ident(name) = &b.trailing_return {
        for s in &b.stmts {
            if let Stmt::Let { name: n, ty, .. } = s {
                if n == name {
                    return ty.clone();
                }
            }
        }
    }
    infer(&b.trailing_return)
}

fn try_repeat(lhs_ty: &Type, rhs_ty: &Type, lhs: &Expr, rhs: &Expr) -> Option<Expr> {
    let is_seq = |t: &Type| matches!(t, Type::Str | Type::List(_));
    if is_seq(lhs_ty) && *rhs_ty == Type::I64 {
        Some(Expr::Repeat {
            seq: Box::new(lhs.clone()),
            n: Box::new(rhs.clone()),
            of_str: *lhs_ty == Type::Str,
        })
    } else if *lhs_ty == Type::I64 && is_seq(rhs_ty) {
        Some(Expr::Repeat {
            seq: Box::new(rhs.clone()),
            n: Box::new(lhs.clone()),
            of_str: *rhs_ty == Type::Str,
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

/// PMAT-502eo: map a set-algebra *method* name to its [`SetOp`] — the method
/// forms (`a.union(b)`, …) of the `|`/`&`/`-`/`^` operators. `None` for any
/// other method name.
fn set_method_op(name: &str) -> Option<SetOp> {
    match name {
        "union" => Some(SetOp::Union),
        "intersection" => Some(SetOp::Intersection),
        "difference" => Some(SetOp::Difference),
        "symmetric_difference" => Some(SetOp::SymmetricDifference),
        _ => None,
    }
}

/// PMAT-502ep: map a comparison operator to a set predicate (subset / proper /
/// superset). `==`/`!=` return `None` (handled by the plain `BinOp`, which
/// `HashSet` supports via `PartialEq`).
fn set_pred_from_cmp(op: &ast::CmpOp) -> Option<SetPredOp> {
    match op {
        ast::CmpOp::LtE => Some(SetPredOp::Subset),
        ast::CmpOp::Lt => Some(SetPredOp::ProperSubset),
        ast::CmpOp::GtE => Some(SetPredOp::Superset),
        ast::CmpOp::Gt => Some(SetPredOp::ProperSuperset),
        _ => None,
    }
}

/// PMAT-502ep: map a set-predicate *method* name to its [`SetPredOp`]
/// (`a.issubset(b)` / `a.issuperset(b)` / `a.isdisjoint(b)`). The methods are
/// non-strict (no proper-subset method in Python). `None` otherwise.
fn set_pred_method(name: &str) -> Option<SetPredOp> {
    match name {
        "issubset" => Some(SetPredOp::Subset),
        "issuperset" => Some(SetPredOp::Superset),
        "isdisjoint" => Some(SetPredOp::Disjoint),
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

/// PMAT-502dc: context-aware variant of [`lower_compare`]. Lowers each
/// operand with `lower_expr_in_ctx` so a builtin in a comparison operand
/// (`abs(n) > 0`, `len(s) > 3`, `max(a, b) <= c`) is recognized — the
/// context-free `lower_compare` emits an undefined Rust `abs(...)` etc.
/// Membership (`in`/`not in`) is handled by the caller before this point.
fn lower_compare_in_ctx(ctx: &LoweringCtx, c: ast::ExprCompare) -> Result<Expr, FrontendError> {
    if c.ops.is_empty() || c.ops.len() != c.comparators.len() {
        return Err(FrontendError::Lower(
            "malformed comparison (ops/comparators mismatch) — unreachable Python AST".into(),
        ));
    }
    // PMAT-502ex: `x is None` / `x is not None` over an `Optional`-typed value
    // → `Expr::IsNone` (`.is_none()` / `.is_some()`). A single `is`/`is not`
    // comparison against the `None` constant; the operand must type as
    // `Optional` (a `None` test on a non-Optional value is degenerate — Python
    // always-False — and deferred). Intercepted before the operand loop because
    // a bare `None` constant has no value-position lowering.
    if c.ops.len() == 1
        && matches!(c.ops[0], ast::CmpOp::Is | ast::CmpOp::IsNot)
        && matches!(&c.comparators[0], ast::Expr::Constant(k) if matches!(k.value, ast::Constant::None))
    {
        let value = lower_expr_in_ctx(ctx, (*c.left).clone())?;
        if matches!(infer_type_in_ctx(ctx, &value), Type::Optional(_)) {
            return Ok(Expr::IsNone {
                value: Box::new(value),
                negated: matches!(c.ops[0], ast::CmpOp::IsNot),
            });
        }
        return Err(FrontendError::Lower(format!(
            "function `{}` uses `is None` / `is not None` on a non-`Optional` value — v0.2.0 supports the `None` test only on `Optional[T]` values",
            ctx.fn_name
        )));
    }
    let mut operands: Vec<Expr> = Vec::with_capacity(c.ops.len() + 1);
    operands.push(lower_expr_in_ctx(ctx, *c.left)?);
    for cmp in c.comparators {
        operands.push(lower_expr_in_ctx(ctx, cmp)?);
    }
    // Each operand was lowered exactly once into `operands`. Precompute each
    // operand's type so the chained path (which references temps, not the
    // originals) doesn't need to re-infer for the Set / float-promotion logic.
    let op_types: Vec<Type> = operands.iter().map(|o| infer_type_in_ctx(ctx, o)).collect();

    // A single comparison shares no operand between sub-comparisons, so emit it
    // directly (the overwhelmingly common path — keep it a plain `BinOp`).
    if c.ops.len() == 1 {
        let rhs = operands.pop().expect("two operands for one op");
        let lhs = operands.pop().expect("two operands for one op");
        return build_chain_cmp(ctx, &c.ops[0], lhs, &op_types[0], rhs, &op_types[1]);
    }

    // PMAT-576: a CHAINED comparison (`a < b < c`, `a == f() == b`) desugars to
    // `(a OP b) && (b OP c) && …`, where each *interior* operand is SHARED by
    // two adjacent sub-comparisons. Emitting the lowered operand into both
    // (the previous `operands[i].clone()`) evaluated it TWICE — diverging from
    // Python, which evaluates every operand exactly once, left to right: a
    // side-effecting middle (`0 < xs.pop() < 10`) popped twice, an expensive
    // one ran twice. Bind every operand to a fresh temp ONCE, then fold the
    // sub-comparisons over the temps inside an `Expr::Block`. (Short-circuit
    // order is preserved: `&&` still stops at the first false sub-comparison;
    // only the one-time *binding* of operands is hoisted, which is observable
    // only when an operand has a side effect — exactly the bug.)
    let names: Vec<String> = (0..operands.len()).map(|i| format!("__cmp{i}")).collect();
    let mut stmts: Vec<Stmt> = Vec::with_capacity(operands.len());
    for (i, value) in operands.into_iter().enumerate() {
        stmts.push(Stmt::Let {
            name: names[i].clone(),
            ty: op_types[i].clone(),
            value,
            mutable: false,
        });
    }
    let mut acc: Option<Expr> = None;
    for (i, op) in c.ops.iter().enumerate() {
        let cmp = build_chain_cmp(
            ctx,
            op,
            Expr::Ident(names[i].clone()),
            &op_types[i],
            Expr::Ident(names[i + 1].clone()),
            &op_types[i + 1],
        )?;
        acc = Some(match acc {
            None => cmp,
            Some(prev) => Expr::BinOp {
                op: BinOp::And,
                lhs: Box::new(prev),
                rhs: Box::new(cmp),
            },
        });
    }
    Ok(Expr::Block(Box::new(Block {
        stmts,
        trailing_return: acc.expect("ops non-empty (checked above)"),
    })))
}

/// PMAT-576: build one sub-comparison of a (possibly chained) `Compare`, from
/// its already-lowered operands and their precomputed types. Two sets become a
/// subset/superset `SetPred` (HashSet has no `<`; PMAT-502ep); a mixed
/// `float`/`int` comparison promotes the int side to f64 (PMAT-540, which Rust
/// requires and Python does numerically); everything else is a plain
/// ordering/equality `BinOp`.
fn build_chain_cmp(
    ctx: &LoweringCtx,
    op: &ast::CmpOp,
    lhs: Expr,
    lt: &Type,
    rhs: Expr,
    rt: &Type,
) -> Result<Expr, FrontendError> {
    if matches!(lt, Type::Set(_)) && matches!(rt, Type::Set(_)) {
        if let Some(sp) = set_pred_from_cmp(op) {
            return Ok(Expr::SetPred {
                lhs: Box::new(lhs),
                op: sp,
                rhs: Box::new(rhs),
            });
        }
    }
    let mut lhs = lhs;
    let mut rhs = rhs;
    if *lt == Type::F64 && *rt == Type::I64 {
        rhs = to_f64_operand(ctx, rhs);
    } else if *lt == Type::I64 && *rt == Type::F64 {
        lhs = to_f64_operand(ctx, lhs);
    } else if *lt == Type::Bool && *rt == Type::I64 {
        // PMAT-617: Python bool is an int subtype, so `flag == 1` / `flag < 2`
        // are valid (`True == 1`). xpile emitted a bare `bool OP i64`, which
        // rustc rejects (E0308). Coerce the bool side to `i64` (`(b) as i64`),
        // the deferred comparison half of the bool-as-int story (PMAT-565). Use
        // the authoritative `lt`/`rt` to build the cast directly rather than
        // re-inferring via `to_i64_operand`: in a CHAINED comparison the operand
        // here is a `__cmpN` temp not registered in `ctx`, so re-inference would
        // miss it (the float path's `to_f64_operand` survives that only because a
        // redundant `f64 as f64` is harmless). (bool/bool needs no coercion —
        // Rust `bool: Ord`; bool/float is a separate rarer follow-up.)
        lhs = bool_to_i64_cast(lhs);
    } else if *lt == Type::I64 && *rt == Type::Bool {
        rhs = bool_to_i64_cast(rhs);
    }
    Ok(Expr::BinOp {
        op: cmp_binop(op)?,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    })
}

/// PMAT-502dd: context-aware variant of the context-free list-literal handler.
/// Lowers each element with `lower_expr_in_ctx` so a builtin element
/// (`[abs(a), max(a, b)]`) is recognized; the homogeneity check uses
/// `infer_type_in_ctx`.
fn lower_list_literal_in_ctx(
    ctx: &LoweringCtx,
    list_expr: ast::ExprList,
) -> Result<Expr, FrontendError> {
    if list_expr.elts.is_empty() {
        return Err(FrontendError::Lower(
            "empty list literal `[]` requires a type annotation to infer the element type \
             — pass via `def f() -> list[int]: return []` once empty-literal annotation \
             threading lands in a subsequent v0.2.0 Track 1.B sub-track"
                .into(),
        ));
    }
    // PMAT-502es: a list literal with `*`-splat elements (`[*a, *b]`,
    // `[x, *a, y]`) is a concatenation. Fold the elements into a chain of
    // `Expr::ListConcat`: a `*e` contributes the list `e` (which must type as a
    // list), a plain `x` contributes a singleton `[x]`. The fold produces a
    // fresh `Vec`; a lone `[*a]` (no concat) is wrapped in `Expr::Clone` so it
    // copies (Python `[*a]` is a shallow copy) rather than moving `a`.
    if list_expr
        .elts
        .iter()
        .any(|e| matches!(e, ast::Expr::Starred(_)))
    {
        let mut acc: Option<Expr> = None;
        for elt in list_expr.elts {
            let part = match elt {
                ast::Expr::Starred(s) => {
                    let inner = lower_expr_in_ctx(ctx, (*s.value).clone())?;
                    if !matches!(infer_type_in_ctx(ctx, &inner), Type::List(_)) {
                        return Err(FrontendError::Lower(format!(
                            "function `{}` splats a non-list (`[*x]` where `x` is not a list) — only list splats are supported at v0.2.0",
                            ctx.fn_name
                        )));
                    }
                    inner
                }
                other => Expr::ListLit(vec![lower_expr_in_ctx(ctx, other)?]),
            };
            acc = Some(match acc {
                None => part,
                Some(prev) => Expr::ListConcat {
                    lhs: Box::new(prev),
                    rhs: Box::new(part),
                },
            });
        }
        let result = acc.expect("non-empty (a Starred element guarantees >= 1)");
        // A `ListConcat` chain already produces a fresh `Vec`; a lone bare
        // splat (`[*a]`) would otherwise move `a`, so clone it for a copy.
        return Ok(match result {
            Expr::ListConcat { .. } => result,
            other => Expr::Clone(Box::new(other)),
        });
    }
    let mut elems = Vec::with_capacity(list_expr.elts.len());
    let mut elem_ty: Option<Type> = None;
    for e in list_expr.elts {
        let lowered = lower_expr_in_ctx(ctx, e)?;
        let ty = infer_type_in_ctx(ctx, &lowered);
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
        // PMAT-628: clone a reused non-Copy variable element so `[inner, inner]`
        // (or `inner` also used after the literal) doesn't move-then-use (E0382).
        // (Python aliases the same inner object; the clone gives independent
        // copies — the documented PMAT-569 value-semantics divergence, but it now
        // compiles instead of emitting invalid Rust.)
        elems.push(clone_if_reused_non_copy(ctx, lowered));
    }
    Ok(Expr::ListLit(elems))
}

/// PMAT-502dd: context-aware variant of the context-free dict-literal handler.
/// Lowers each key/value with `lower_expr_in_ctx` so a builtin value
/// (`{"k": abs(v)}`) is recognized.
fn lower_dict_literal_in_ctx(
    ctx: &LoweringCtx,
    dict_expr: ast::ExprDict,
) -> Result<Expr, FrontendError> {
    if dict_expr.keys.is_empty() {
        return Err(FrontendError::Lower(
            "empty dict literal `{}` requires a type annotation to infer K/V — \
             pass via `def f() -> dict[str, int]: return {}` once empty-literal annotation \
             threading lands in a subsequent v0.2.0 Track 1.C sub-track"
                .into(),
        ));
    }
    // PMAT-502dw/dx: a dict literal containing any `**`-splat (key == None) —
    // possibly mixed with explicit `k: v` entries — lowers to `Expr::DictMerge`
    // (a left-to-right chain so a later entry wins on a key collision, matching
    // Python). Each splatted value must itself be a dict.
    if dict_expr.keys.iter().any(|k| k.is_none()) {
        let mut entries: Vec<(Option<Expr>, Expr)> = Vec::with_capacity(dict_expr.keys.len());
        for (k_opt, v) in dict_expr.keys.iter().zip(dict_expr.values.iter()) {
            let lv = lower_expr_in_ctx(ctx, v.clone())?;
            match k_opt {
                Some(k) => {
                    let lk = lower_expr_in_ctx(ctx, k.clone())?;
                    entries.push((Some(lk), lv));
                }
                None => {
                    if !matches!(infer_type_in_ctx(ctx, &lv), Type::Dict(_, _)) {
                        return Err(FrontendError::Lower(
                            "`{**x}` dict-splat requires each spread value to be a dict".into(),
                        ));
                    }
                    entries.push((None, lv));
                }
            }
        }
        return Ok(Expr::DictMerge { entries });
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
        let lk = lower_expr_in_ctx(ctx, k)?;
        let lv = lower_expr_in_ctx(ctx, v)?;
        let kt = infer_type_in_ctx(ctx, &lk);
        let vt = infer_type_in_ctx(ctx, &lv);
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

/// PMAT-502dd: context-aware variant of the context-free set-literal handler.
/// Lowers each element with `lower_expr_in_ctx` so a builtin element
/// (`{abs(a), abs(b)}`) is recognized.
fn lower_set_literal_in_ctx(
    ctx: &LoweringCtx,
    set_expr: ast::ExprSet,
) -> Result<Expr, FrontendError> {
    if set_expr.elts.is_empty() {
        return Err(FrontendError::Lower(
            "empty set literal requires `set()` or an annotation — deferred".into(),
        ));
    }
    // PMAT-502et: a set literal with `*`-splat elements (`{*a, *b}`, `{*a, x}`)
    // is a union. Fold the elements into a chain of `Expr::SetOp{Union}`: a `*e`
    // contributes the set `e` (which must type as a set), a plain `x` a
    // singleton `{x}`. The union chain produces a fresh `HashSet`; a lone `{*a}`
    // is wrapped in `Expr::Clone` so it copies rather than moving `a`. (Parallels
    // the list-splat handling, PMAT-502es.)
    if set_expr
        .elts
        .iter()
        .any(|e| matches!(e, ast::Expr::Starred(_)))
    {
        let mut acc: Option<Expr> = None;
        for elt in set_expr.elts {
            let part = match elt {
                ast::Expr::Starred(s) => {
                    let inner = lower_expr_in_ctx(ctx, (*s.value).clone())?;
                    if !matches!(infer_type_in_ctx(ctx, &inner), Type::Set(_)) {
                        return Err(FrontendError::Lower(format!(
                            "function `{}` splats a non-set (`{{*x}}` where `x` is not a set) — only set splats are supported at v0.2.0",
                            ctx.fn_name
                        )));
                    }
                    inner
                }
                other => Expr::SetLit(vec![lower_expr_in_ctx(ctx, other)?]),
            };
            acc = Some(match acc {
                None => part,
                Some(prev) => Expr::SetOp {
                    lhs: Box::new(prev),
                    op: SetOp::Union,
                    rhs: Box::new(part),
                },
            });
        }
        let result = acc.expect("non-empty (a Starred element guarantees >= 1)");
        return Ok(match result {
            Expr::SetOp { .. } => result,
            other => Expr::Clone(Box::new(other)),
        });
    }
    let mut elems = Vec::with_capacity(set_expr.elts.len());
    let mut elem_ty: Option<Type> = None;
    for e in set_expr.elts {
        let lowered = lower_expr_in_ctx(ctx, e)?;
        let ty = infer_type_in_ctx(ctx, &lowered);
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

/// PMAT-502df: lower a generator expression `<elt> for <var> in <iter>`
/// (single generator, no `if` filter, first cut) to `Expr::Map` — the same
/// List-producing form as `map(lambda <var>: <elt>, <iter>)`. This lets the
/// common consumers (`sum(...)`, `max(...)`, `min(...)`, `list(...)`) accept a
/// generator expression. The iterable may be a `range(...)` (materialised via
/// `lower_range_list`) or any list-typed expression; the body is lowered with
/// the loop var unbound (matching `map`'s element-type inference). An `if`
/// filter, multiple generators, and a tuple target are deferred (use a
/// filtered list comprehension assigned to a variable instead).
fn lower_generator_exp_in_ctx(
    ctx: &LoweringCtx,
    ge: ast::ExprGeneratorExp,
) -> Result<Expr, FrontendError> {
    // PMAT-556: a two-generator genexpr (`sum(i*j for i in a for j in b)`)
    // builds its flattened list via nested loops in an `Expr::Block`.
    if ge.generators.len() == 2 {
        return lower_comp_2gen_to_block(ctx, &ge.generators, "generator expression", |sub| {
            lower_expr_in_ctx(sub, (*ge.elt).clone())
        });
    }
    lower_comp_to_map(ctx, &ge.generators, "generator expression", |sub| {
        lower_expr_in_ctx(sub, (*ge.elt).clone())
    })
}

/// PMAT-502du: an expression-position list comprehension (`sum([x for x in
/// xs])`, `return [x*2 for x in xs]`) lowers through the same `Map`/`Filter`
/// machinery as a generator expression (it produces the same List). The
/// statement form `name = [comp]` still uses the dedicated for-append desugar.
/// Shares the loop-var-unbound limitation with `map`/genexpr (str-method
/// element bodies need the statement form).
fn lower_list_comp_in_ctx(
    ctx: &LoweringCtx,
    comp: ast::ExprListComp,
) -> Result<Expr, FrontendError> {
    // PMAT-556: an expr-position two-generator list comp builds via nested loops.
    if comp.generators.len() == 2 {
        return lower_comp_2gen_to_block(ctx, &comp.generators, "list comprehension", |sub| {
            lower_expr_in_ctx(sub, (*comp.elt).clone())
        });
    }
    lower_comp_to_map(ctx, &comp.generators, "list comprehension", |sub| {
        lower_expr_in_ctx(sub, (*comp.elt).clone())
    })
}

/// PMAT-502dv: an expression-position set comprehension (`len({x for x in
/// xs})`) lowers to `set(<list-comp>)` — i.e. `SetFromList` over the same
/// `Map`/`Filter` form. The statement / return forms keep their own desugars.
fn lower_set_comp_in_ctx(ctx: &LoweringCtx, comp: ast::ExprSetComp) -> Result<Expr, FrontendError> {
    // PMAT-556: a two-generator set comp builds its underlying list via nested
    // loops (an `Expr::Block`), then collects into a set.
    let list = if comp.generators.len() == 2 {
        lower_comp_2gen_to_block(ctx, &comp.generators, "set comprehension", |sub| {
            lower_expr_in_ctx(sub, (*comp.elt).clone())
        })?
    } else {
        lower_comp_to_map(ctx, &comp.generators, "set comprehension", |sub| {
            lower_expr_in_ctx(sub, (*comp.elt).clone())
        })?
    };
    Ok(Expr::SetFromList {
        list: Box::new(list),
    })
}

/// PMAT-502dv: an expression-position dict comprehension (`len({k: v for x in
/// xs})`) lowers to `dict(<list of (k, v) tuples>)` — i.e. `DictFromPairs`
/// over a `Map` whose body is the `(key, value)` tuple.
fn lower_dict_comp_in_ctx(
    ctx: &LoweringCtx,
    comp: ast::ExprDictComp,
) -> Result<Expr, FrontendError> {
    // PMAT-556: a two-generator dict comp builds a list of `(k, v)` tuples via
    // nested loops (an `Expr::Block`), then collects into a map.
    let pairs = if comp.generators.len() == 2 {
        lower_comp_2gen_to_block(ctx, &comp.generators, "dict comprehension", |sub| {
            let key = lower_expr_in_ctx(sub, (*comp.key).clone())?;
            let value = lower_expr_in_ctx(sub, (*comp.value).clone())?;
            Ok(Expr::TupleLit(vec![key, value]))
        })?
    } else {
        lower_comp_to_map(ctx, &comp.generators, "dict comprehension", |sub| {
            let key = lower_expr_in_ctx(sub, (*comp.key).clone())?;
            let value = lower_expr_in_ctx(sub, (*comp.value).clone())?;
            let key = clone_comp_key_if_binder_reused(sub, &comp, key);
            Ok(Expr::TupleLit(vec![key, value]))
        })?
    };
    Ok(Expr::DictFromPairs {
        pairs: Box::new(pairs),
    })
}

/// PMAT-599 (ownership): in a single-generator dict comprehension
/// `{K: V for x in xs}`, if the binder `x` is non-Copy and referenced in BOTH
/// `K` and `V` (read >1× across the pair), the bare-binder key moves it into
/// the `(key, value)` tuple before `V` can use it (rustc E0382 — `{w: w …}`,
/// `{w: w + "!" …}`, `{k: len(k) …}`). Clone the key so the value keeps a live
/// value. Gated on read-count>1 + non-Copy → existing comprehensions with a
/// Copy binder (e.g. `{x: x*2 for x in xs}`) or a single-use binder are
/// byte-identical (zero churn; the clone fires only on previously-failing code).
fn clone_comp_key_if_binder_reused(sub: &LoweringCtx, comp: &ast::ExprDictComp, key: Expr) -> Expr {
    let Some(gen) = comp.generators.first() else {
        return key;
    };
    let ast::Expr::Name(n) = &gen.target else {
        return key;
    };
    let binder = n.id.as_str();
    let mut counts: HashMap<String, usize> = HashMap::new();
    count_reads_expr(&comp.key, &mut counts);
    count_reads_expr(&comp.value, &mut counts);
    let reused = counts.get(binder).copied().unwrap_or(0) > 1;
    let non_copy = sub
        .name_types
        .get(binder)
        .is_some_and(|t| !matches!(t, Type::I64 | Type::F64 | Type::Bool));
    if reused && non_copy {
        Expr::Clone(Box::new(key))
    } else {
        key
    }
}

/// Shared core for generator-expression / expr-position comprehension lowering
/// (PMAT-502df/dg/du/dv): a single generator `for <var> in <iter> [if <cond>]`
/// over a pre-lowered `body` (with the loop var unbound) → `Expr::Map` (over an
/// optional `Expr::Filter`). Single generator, ≤1 `if`, plain-Name target.
fn lower_comp_to_map(
    ctx: &LoweringCtx,
    generators: &[ast::Comprehension],
    kind: &str,
    // PMAT-525: lowers the comprehension body given a ctx with the loop var bound
    // to the iterable's element type (so e.g. a tuple-index `p[1]` lowers to the
    // `.1` field access, and `p.field` over a struct element resolves). The body
    // was previously pre-lowered with the var UNBOUND (→ default I64 → tuple/
    // struct element bodies miscompiled / were rejected).
    lower_body: impl FnOnce(&LoweringCtx) -> Result<Expr, FrontendError>,
) -> Result<Expr, FrontendError> {
    if generators.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "{kind} with multiple `for` clauses is not supported at v0.2.0"
        )));
    }
    let gen = &generators[0];
    // PMAT-563: multiple `if` clauses are ANDed (folded below).
    // PMAT-531: the loop target is either a plain Name (`for x in …`) or a
    // 2-name tuple (`for k, v in d.items()`). The tuple form binds both names
    // via a Rust tuple-destructure closure param (`|__k| { let (k, v) =
    // __k.clone(); … }`), mirroring the list-comp tuple branch (PMAT-502cg)
    // for expression-position comprehensions / generator expressions.
    let targets: Vec<String> = match &gen.target {
        ast::Expr::Name(var) => vec![var.id.to_string()],
        ast::Expr::Tuple(t) => {
            if let [ast::Expr::Name(a), ast::Expr::Name(b)] = t.elts.as_slice() {
                vec![a.id.to_string(), b.id.to_string()]
            } else {
                return Err(FrontendError::Lower(format!(
                    "{kind} tuple target must be exactly two plain names at v0.2.0"
                )));
            }
        }
        _ => {
            return Err(FrontendError::Lower(format!(
                "{kind} with a non-Name target is not supported at v0.2.0"
            )));
        }
    };
    // Materialise the iterable into a list: a bare `range(...)` (not a
    // first-class value) lowers via `lower_range_list`; anything else must
    // already be list-typed.
    let iter_list = if let ast::Expr::Call(inner) = &gen.iter {
        if matches!(&*inner.func, ast::Expr::Name(n) if n.id.as_str() == "range")
            && inner.keywords.is_empty()
        {
            lower_range_list(ctx, inner)?
        } else {
            str_iter_to_chars(ctx, lower_expr_in_ctx(ctx, gen.iter.clone())?)
        }
    } else {
        // PMAT-546: a `str` iterable comprehends over its chars (1-char strings).
        str_iter_to_chars(ctx, lower_expr_in_ctx(ctx, gen.iter.clone())?)
    };
    let elem_ty = match infer_type_in_ctx(ctx, &iter_list) {
        Type::List(e) => *e,
        _ => {
            return Err(FrontendError::Lower(format!(
                "{kind} iterates over a non-list — only `range(...)` and list-typed iterables are \
                 supported at v0.2.0"
            )));
        }
    };
    // PMAT-525: bind the loop var to the element type so the filter + body type
    // correctly (e.g. `p[1]` over a `tuple` element → `.1`). PMAT-531: a 2-name
    // tuple target splits the element 2-tuple type and binds both names; the
    // closure param becomes a Rust destructure pattern `(k, v)`.
    let mut sub = ctx.clone();
    let param = if targets.len() == 1 {
        sub.bound.insert(targets[0].clone());
        sub.name_types.insert(targets[0].clone(), elem_ty);
        targets[0].clone()
    } else {
        let (ta, tb) = match elem_ty {
            Type::Tuple(tys) if tys.len() == 2 => (tys[0].clone(), tys[1].clone()),
            other => {
                return Err(FrontendError::Lower(format!(
                    "{kind} `for k, v in …` iterates a {other:?}; expected an iterable of 2-tuples (e.g. `d.items()`)"
                )))
            }
        };
        sub.bound.insert(targets[0].clone());
        sub.bound.insert(targets[1].clone());
        sub.name_types.insert(targets[0].clone(), ta);
        sub.name_types.insert(targets[1].clone(), tb);
        format!("({}, {})", targets[0], targets[1])
    };
    // PMAT-563: the `if <cond>` clauses (ANDed) wrap the iterable in an
    // `Expr::Filter` (also List-typed, so `Map` composes); each must type as Bool.
    let list = if let Some(cond) = combine_comp_filters(&sub, &gen.ifs, kind)? {
        Expr::Filter {
            list: Box::new(iter_list),
            lambda: SortKey {
                param: param.clone(),
                body: Box::new(cond),
            },
        }
    } else {
        iter_list
    };
    let body = lower_body(&sub)?;
    Ok(Expr::Map {
        list: Box::new(list),
        lambda: SortKey {
            param,
            body: Box::new(body),
        },
    })
}

/// PMAT-556: expression-position **two-generator** comprehension / generator
/// expression → an `Expr::Block` that builds a `Vec` via the statement-position
/// nested-loop machinery ([`desugar_comp_2gen`], run on a *cloned* ctx so the
/// loop-var bindings don't leak) and returns the accumulator as its trailing
/// expression. This is what lets `sum(i*j for i in range(n) for j in range(m))`
/// and the expr-position `[…]` / `{…}` 2-generator forms lower; the
/// single-generator path stays [`lower_comp_to_map`]. The block-local
/// accumulator's type is recovered at use sites by [`block_result_type`].
fn lower_comp_2gen_to_block(
    ctx: &LoweringCtx,
    generators: &[ast::Comprehension],
    kind: &str,
    lower_elem: impl FnOnce(&LoweringCtx) -> Result<Expr, FrontendError>,
) -> Result<Expr, FrontendError> {
    let target = "__xcomp2";
    let mut sub = ctx.clone();
    let stmts = desugar_comp_2gen(&mut sub, target, generators, kind, |c| {
        let elem = lower_elem(c)?;
        let acc_ty = Type::List(Box::new(infer_type_in_ctx(c, &elem)));
        let insert = Stmt::ListAppend {
            list_name: target.to_string(),
            elem,
        };
        Ok((insert, acc_ty, Expr::ListLit(Vec::new())))
    })?;
    Ok(Expr::Block(Box::new(Block {
        stmts,
        trailing_return: Expr::Ident(target.to_string()),
    })))
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
            Item::Struct { .. } => panic!("expected a function item, found a struct"),
            Item::Enum { .. } => panic!("expected a function item, found an enum"),
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
