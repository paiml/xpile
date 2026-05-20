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
use xpile_frontend::{Frontend, FrontendError};
use xpile_meta_hir::{
    BinOp, Block, Expr, Function, Item, Module, Param, SourceLang, Stmt, Type, UnOp,
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
}

impl LoweringCtx {
    fn new(fn_name: &str, fn_return_type: Type, params: &[Param], body: &[ast::Stmt]) -> Self {
        let bound: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
        let name_types: HashMap<String, Type> =
            params.iter().map(|p| (p.name.clone(), p.ty)).collect();
        let mutable = compute_mutable_names(params, body);
        Self {
            fn_name: fn_name.to_string(),
            fn_return_type,
            bound,
            name_types,
            mutable,
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
                if let Some(name) = simple_assign_target_name(a) {
                    let bump = if in_loop { 2 } else { 1 };
                    *counts.entry(name).or_insert(0) += bump;
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
            let fn_item = lower_top_level_stmt(stmt)?;
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

fn lower_top_level_stmt(stmt: ast::Stmt) -> Result<Function, FrontendError> {
    match stmt {
        ast::Stmt::FunctionDef(f) => lower_function_def(f),
        other => Err(FrontendError::Lower(format!(
            "unsupported top-level statement: {:?} — only `def` is supported at v0.1.0",
            std::mem::discriminant(&other)
        ))),
    }
}

fn lower_function_def(f: ast::StmtFunctionDef) -> Result<Function, FrontendError> {
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
        params.push(Param { name, ty });
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
    let ctx_return_type = declared_return_type.unwrap_or(Type::I64);

    // PMAT-013: implicit BigInt promotion. When the user declares the
    // return type as BigInt, every `int`-typed param is promoted to
    // BigInt automatically — the user opts in once at the return site
    // instead of annotating every param. This is the most ergonomic
    // form of the slow path: `def factorial(n: int) -> BigInt:` reads
    // naturally and produces a BigInt-mode function end-to-end.
    //
    // Explicitly-annotated Bool params are left alone (Bool doesn't
    // overflow). Explicit BigInt annotations are already BigInt.
    if ctx_return_type == Type::BigInt {
        for p in &mut params {
            if p.ty == Type::I64 {
                p.ty = Type::BigInt;
            }
        }
    }

    let mut ctx = LoweringCtx::new(&f.name, ctx_return_type, &params, &body_stmts);
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
            lower_expr((**value).clone())?
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
            "BigInt" => Ok(Type::BigInt),
            "str" => Ok(Type::Str),
            other => Err(FrontendError::Lower(format!(
                "function `{fn_name}` annotates `{site}` with unsupported type `{other}` — only `int`, `bool`, `BigInt`, `str` at v0.2.0 Track 1.A"
            ))),
        },
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
        ast::Stmt::Assign(asn) => lower_assign(ctx, asn).map(|s| vec![s]),
        ast::Stmt::If(if_stmt) => lower_if_stmt_as_lets(ctx, if_stmt),
        ast::Stmt::While(w) => lower_while_stmt(ctx, w).map(|s| vec![s]),
        ast::Stmt::For(f) => lower_for_stmt(ctx, f),
        ast::Stmt::Assert(a) => lower_assert_stmt(ctx, a).map(|s| vec![s]),
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
        ast::Stmt::Expr(e) => lower_expr_stmt_as_cmd(ctx, e).map(|s| vec![s]),
        other => Err(FrontendError::Lower(format!(
            "function `{}` contains unsupported statement: {:?} — supported: assignment, if/elif/else, while, for-in-range, assert, subprocess.run([...]), then a final `return`",
            ctx.fn_name,
            std::mem::discriminant(&other)
        ))),
    }
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

    let target_name = match &*f.target {
        ast::Expr::Name(n) => n.id.to_string(),
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{}` uses a non-Name `for` target (tuple unpacking, attribute, subscript) — not supported at v0.1.0",
                ctx.fn_name
            )));
        }
    };

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
    let (start_expr, stop_expr, step_int) = match call.args.as_slice() {
        [stop] => (Expr::LitInt(0), lower_expr(stop.clone())?, 1i64),
        [start, stop] => (lower_expr(start.clone())?, lower_expr(stop.clone())?, 1i64),
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
            (lower_expr(start.clone())?, lower_expr(stop.clone())?, step)
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
    };
    let init_stmt = if ctx.bound.contains(&target_name) {
        Stmt::Assign {
            name: target_name.clone(),
            value: start_expr,
        }
    } else {
        ctx.bound.insert(target_name.clone());
        ctx.name_types.insert(target_name.clone(), target_ty);
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
    let cond = lower_expr(*w.test)?;
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
    let cond = lower_expr(*a.test)?;
    if infer_type(&cond) != Type::Bool {
        return Err(FrontendError::Lower(format!(
            "function `{}` has an `assert` whose expression is not Bool (no int-truthiness at v0.1.0)",
            ctx.fn_name
        )));
    }
    Ok(Stmt::Assert { cond })
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
        let if_expr = lower_if_chain_to_expr(&ctx.fn_name, &if_stmt, name)?;
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
                ty,
                value: if_expr,
                mutable: ctx.mutable.contains(name),
            });
            ctx.bound.insert(name.clone());
            ctx.name_types.insert(name.clone(), ty);
        }
    }
    Ok(stmts)
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

    let cond = lower_expr((*if_stmt.test).clone())?;
    if infer_type(&cond) != Type::Bool {
        return Err(FrontendError::Lower(format!(
            "function `{fn_name}` has an if-condition that is not Bool (no int-truthiness at v0.1.0)"
        )));
    }

    let then_expr = find_assignment_value(fn_name, &if_stmt.body, target_name)?;
    let then_ty = infer_type(&then_expr);

    // Else branch is one of:
    //   nested StmtIf → recurse (handles elif)
    //   any list of assignments → terminal else: find `target_name` here
    let else_expr = if if_stmt.orelse.len() == 1 {
        if let ast::Stmt::If(nested) = &if_stmt.orelse[0] {
            lower_if_chain_to_expr(fn_name, nested, target_name)?
        } else {
            find_assignment_value(fn_name, &if_stmt.orelse, target_name)?
        }
    } else {
        find_assignment_value(fn_name, &if_stmt.orelse, target_name)?
    };
    let else_ty = infer_type(&else_expr);
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
    fn_name: &str,
    body: &[ast::Stmt],
    target_name: &str,
) -> Result<Expr, FrontendError> {
    for stmt in body {
        if let ast::Stmt::Assign(a) = stmt {
            let name = single_name_target(fn_name, a)?;
            if name == target_name {
                return lower_expr((*a.value).clone());
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
        ast::Expr::Tuple(_) => {
            return Err(FrontendError::Lower(format!(
                "function `{}` uses tuple unpacking `a, b = ...` — not supported at v0.1.0",
                ctx.fn_name
            )));
        }
        ast::Expr::Attribute(_) | ast::Expr::Subscript(_) => {
            return Err(FrontendError::Lower(format!(
                "function `{}` assigns to an attribute/subscript — not supported at v0.1.0",
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
    let value = lower_expr(*asn.value)?;
    let ty = infer_type_in_ctx(ctx, &value);
    // If the name is already bound, this is a reassignment — emit
    // `Stmt::Assign` (the backend will write `name = value;` and the
    // earlier `Let` will be `let mut`). Otherwise, fresh `Let`.
    if ctx.bound.contains(&name) {
        Ok(Stmt::Assign { name, value })
    } else {
        let mutable = ctx.mutable.contains(&name);
        ctx.bound.insert(name.clone());
        ctx.name_types.insert(name.clone(), ty);
        Ok(Stmt::Let {
            name,
            ty,
            value,
            mutable,
        })
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
        // PMAT-042 + PMAT-045 + PMAT-047 + PMAT-055: shell-domain
        // Expr variants don't appear inside Python-frontend lowering.
        // Default to I64 for safety so any future accidental call
        // doesn't panic.
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
        Expr::Ident(n) => ctx.name_types.get(n).copied().unwrap_or(Type::I64),
        Expr::LitInt(_) => {
            if ctx.fn_return_type == Type::BigInt {
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
                if lt == Type::BigInt || rt == Type::BigInt {
                    Type::BigInt
                } else {
                    Type::I64
                }
            }
        },
        Expr::IfExpr { then_expr, .. } => infer_type_in_ctx(ctx, then_expr),
        Expr::Call { callee, .. } => {
            if callee == &ctx.fn_name {
                ctx.fn_return_type
            } else {
                Type::I64
            }
        }
        Expr::UnOp { op, operand } => match op {
            UnOp::Neg => infer_type_in_ctx(ctx, operand),
            UnOp::Not => Type::Bool,
        },
        // PMAT-449 (v0.2.0 Track 1.A): Python `"..."` literal is
        // typed as `Type::Str`.
        Expr::LitStr(_) => Type::Str,
        // PMAT-042 + PMAT-045 + PMAT-047 + PMAT-055: see twin arm
        // in `infer_type` above.
        Expr::QuotedString { .. }
        | Expr::ShellVar(_)
        | Expr::CommandSubstitution(_)
        | Expr::ShellSpecial(_) => Type::I64,
    }
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
            // PMAT-449 (v0.2.0 Track 1.A): Python `"..."` literal →
            // `Expr::LitStr`, downstream-typed as `Type::Str`. The
            // raw Python source text is carried through to the
            // backend (which escapes for its target language).
            ast::Constant::Str(s) => Ok(Expr::LitStr(s.to_string())),
            other => Err(FrontendError::Lower(format!(
                "unsupported constant: {:?}",
                std::mem::discriminant(&other)
            ))),
        },
        ast::Expr::BinOp(b) => {
            let op = lower_binop(&b.op)?;
            Ok(Expr::BinOp {
                op,
                lhs: Box::new(lower_expr(*b.left)?),
                rhs: Box::new(lower_expr(*b.right)?),
            })
        }
        ast::Expr::Compare(c) => lower_compare(c),
        ast::Expr::IfExp(ie) => lower_if_exp(ie),
        ast::Expr::Call(c) => lower_call(c),
        ast::Expr::BoolOp(b) => lower_bool_op(b),
        ast::Expr::UnaryOp(u) => lower_unary_op(u),
        other => Err(FrontendError::Lower(format!(
            "unsupported expression: {:?}",
            std::mem::discriminant(&other)
        ))),
    }
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
    let args: Result<Vec<Expr>, _> = c.args.into_iter().map(lower_expr).collect();
    Ok(Expr::Call {
        callee,
        args: args?,
    })
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

fn lower_compare(c: ast::ExprCompare) -> Result<Expr, FrontendError> {
    // Python allows chained compares (`a < b < c`); v0.1.0 supports only a single
    // comparison.
    if c.ops.len() != 1 || c.comparators.len() != 1 {
        return Err(FrontendError::Lower(
            "chained comparisons (e.g., `a < b < c`) are not supported at v0.1.0".into(),
        ));
    }
    let op = match c.ops[0] {
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
    };
    let mut comparators = c.comparators;
    let rhs = comparators.pop().expect("len checked above");
    Ok(Expr::BinOp {
        op,
        lhs: Box::new(lower_expr(*c.left)?),
        rhs: Box::new(lower_expr(rhs)?),
    })
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
