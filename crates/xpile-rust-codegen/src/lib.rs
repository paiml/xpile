//! Shared Rust emission.
//!
//! Takes meta-HIR as input, emits idiomatic Rust. Language-neutral by
//! design — language-specific quirks (Python's int promotion, C's
//! pointer arithmetic, Ruchy's pipeline operator) are normalized in
//! each frontend before reaching codegen.
//!
//! Exposes both:
//!   * [`emit_module`] — free function, kept stable for callers that
//!     don't want to go through the [`Backend`] trait.
//!   * [`RustBackend`] — a [`Backend`] impl that wraps [`emit_module`]
//!     so Rust dispatches through the same trait as PTX / WGSL / Lean.

use std::fmt::Write;
use xpile_backend::{Artifact, Backend, BackendConfig, BackendError, QuorumStatus, Target};
use xpile_meta_hir::{
    BinOp, Block, DictViewKind, Expr, FloatOp, Function, Item, ListMutateOp, ListQueryOp, Module,
    NumBuiltinOp, Param, Radix, SetOp, SourceLang, Stmt, StrMethodOp, Type, UnOp,
};

/// PMAT-502by: escape a string for embedding inside a `format!`/`println!`
/// format-string literal — `{`/`}` are doubled (format escapes), `"`/`\`
/// are backslash-escaped (Rust string-literal escapes), and the common
/// control chars are emitted as `\n`/`\t`/`\r`. Used for `print(sep=…, end=…)`.
fn escape_format_literal(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '{' => out.push_str("{{"),
            '}' => out.push_str("}}"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

/// PMAT-477 (R8): the Rust infix symbol for a float arithmetic op.
fn float_op_sym(op: FloatOp) -> &'static str {
    match op {
        FloatOp::Add => "+",
        FloatOp::Sub => "-",
        FloatOp::Mul => "*",
        FloatOp::Div => "/",
        // FloorDiv/Mod/Pow are emitted via dedicated formulas, never via
        // this helper — keep the match exhaustive.
        FloatOp::FloorDiv => "//",
        FloatOp::Mod => "%",
        FloatOp::Pow => "**",
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    #[error("unsupported item: {0}")]
    Unsupported(String),
    #[error("formatting error: {0}")]
    Format(#[from] std::fmt::Error),
}

pub fn emit_module(module: &Module) -> Result<String, CodegenError> {
    let mut out = String::new();
    writeln!(
        out,
        "// xpile-generated from {:?} module {}",
        module.source_lang, module.name
    )?;
    writeln!(out)?;
    // PMAT-467 (v0.2.0 Track 2.A): C sources lower with C arithmetic
    // semantics (fixed-width `i32`, wrapping overflow) via an isolated
    // emit path, keeping the Python/Ruchy codegen (i64 + checked /
    // bigint) untouched. Governed by `C-C-INT-ARITH` (substrate queued).
    let is_c = matches!(module.source_lang, SourceLang::C);
    for item in &module.items {
        match item {
            Item::Function(f) => {
                if is_c {
                    emit_c_function(&mut out, f)?;
                } else {
                    emit_function(&mut out, f)?;
                }
            }
            // PMAT-502bj: module-level constant → `const NAME: TY = VALUE;`.
            Item::Const { name, ty, value } => {
                write!(out, "const {name}: ")?;
                emit_type(&mut out, ty)?;
                out.push_str(" = ");
                emit_expr(&mut out, value, /*mode=*/ false)?;
                out.push_str(";\n");
            }
        }
    }
    Ok(out)
}

fn emit_function(out: &mut String, f: &Function) -> Result<(), CodegenError> {
    emit_contract_citations(out, f)?;
    write!(out, "pub fn {}(", f.name)?;
    for (i, p) in f.params.iter().enumerate() {
        if i > 0 {
            write!(out, ", ")?;
        }
        emit_param(out, p)?;
    }
    write!(out, ") -> ")?;
    emit_type(out, &f.return_type)?;
    writeln!(out, " {{")?;
    let mode = function_bigint_mode(f);
    emit_block(out, &f.body, mode)?;
    writeln!(out, "}}")?;
    Ok(())
}

/// PMAT-012: a function is in BigInt mode if any param is BigInt OR
/// any pre-bound Let has type BigInt OR the return type is BigInt. In
/// BigInt mode, the Rust backend emits `xpile_bigint::BigInt::from(...)`
/// for integer literals and plain infix `+ - * <= ...` for arithmetic
/// (BigInt never overflows, so no `.checked_*().expect(...)`).
fn function_bigint_mode(f: &Function) -> bool {
    if matches!(f.return_type, Type::BigInt) {
        return true;
    }
    if f.params.iter().any(|p| matches!(p.ty, Type::BigInt)) {
        return true;
    }
    fn stmt_has_bigint(s: &Stmt) -> bool {
        match s {
            Stmt::Let { ty, .. } => matches!(ty, Type::BigInt),
            // PMAT-494b: tuple unpacking introduces no BigInt binding
            // (tuples aren't BigInt-typed at first cut).
            Stmt::LetTuple { .. } => false,
            // PMAT-504: a closure binding is never BigInt-typed at v0.2.0.
            Stmt::ClosureLet { .. } => false,
            // PMAT-479 (R10): an early return introduces no BigInt
            // binding (bigint mode is set by params/lets/return type).
            // PMAT-503a: a raise introduces no BigInt binding.
            Stmt::Assign { .. } | Stmt::Assert { .. } | Stmt::Return(_) | Stmt::Raise { .. } => {
                false
            }
            // PMAT-502bk: loop-control statements carry no binding.
            Stmt::Continue | Stmt::Break => false,
            // PMAT-502bw: print() introduces no binding.
            Stmt::Print { .. } => false,
            Stmt::While { body, .. }
            | Stmt::ForEach { body, .. }
            | Stmt::ForEachPair { body, .. } => body.iter().any(stmt_has_bigint),
            // PMAT-478 (R9): recurse both branches of an if/else.
            Stmt::If {
                then_body,
                else_body,
                ..
            } => then_body.iter().any(stmt_has_bigint) || else_body.iter().any(stmt_has_bigint),
            // PMAT-460: list.append() carries no Type::Let, so no
            // BigInt-mode trigger of its own. PMAT-502ap/aq/ar: in-place
            // list mutators / extend / insert likewise carry no binding.
            Stmt::ListAppend { .. }
            | Stmt::SetAdd { .. }
            | Stmt::SetRemove { .. }
            | Stmt::ListMutate { .. }
            | Stmt::ListExtend { .. }
            | Stmt::DictUpdate { .. }
            | Stmt::ListInsert { .. } => false,
            // PMAT-461: indexed assignment same disposition.
            Stmt::IndexAssign { .. } => false,
            // PMAT-466: dict keyed assignment carries no Type::Let;
            // dict values are int/bool/str at v0.2.0, never BigInt.
            Stmt::DictSet { .. } => false,
            // PMAT-502at: del coll[key] introduces no binding.
            Stmt::DelItem { .. } => false,
            // PMAT-039: shell commands carry no BigInt operands. They
            // also never reach this Rust-codegen scan in practice
            // (bashrs-frontend produces Shell modules that the Rust
            // backend declines at emit_stmt), but exhaustive match
            // keeps the dispatch boundary explicit.
            Stmt::Cmd { .. } => false,
            // PMAT-041: same disposition as Cmd — Pipeline composes
            // Cmd stages; no BigInt operand reachable.
            Stmt::Pipeline { .. } => false,
            // PMAT-048: ShellLoop is bashrs-domain — no BigInt
            // operand reachable through it.
            Stmt::ShellLoop { .. } => false,
            // PMAT-051: ShellAssign same disposition.
            Stmt::ShellAssign { .. } => false,
        }
    }
    f.body.stmts.iter().any(stmt_has_bigint)
}

/// PMAT-011: emit one `// xpile-contract: <ID>` comment line per
/// contract that governs this function. Matches the mdBook convention
/// from `sub/contract-frontend-trait.md`'s citation grid — same prefix
/// across all text-comment hosts, so a single regex finds them all.
/// Lean uses `@[xpile_contract "<ID>"]` (proper structured attribute);
/// LaTeX uses `\xpileContract{<ID>}{...}`; mdBook + Rust + Ruchy share
/// the comment form.
fn emit_contract_citations(out: &mut String, f: &Function) -> Result<(), CodegenError> {
    for id in f.applicable_contracts() {
        writeln!(out, "// xpile-contract: {id}")?;
    }
    Ok(())
}

fn emit_block(out: &mut String, block: &Block, mode: bool) -> Result<(), CodegenError> {
    for stmt in &block.stmts {
        emit_stmt(out, stmt, mode)?;
    }
    write!(out, "    ")?;
    emit_expr(out, &block.trailing_return, mode)?;
    writeln!(out)?;
    Ok(())
}

fn emit_stmt(out: &mut String, stmt: &Stmt, mode: bool) -> Result<(), CodegenError> {
    emit_stmt_indented(out, stmt, "    ", mode)
}

fn emit_stmt_indented(
    out: &mut String,
    stmt: &Stmt,
    indent: &str,
    mode: bool,
) -> Result<(), CodegenError> {
    match stmt {
        Stmt::Let {
            name,
            ty,
            value,
            mutable,
        } => {
            let kw = if *mutable { "let mut" } else { "let" };
            write!(out, "{indent}{kw} {name}: ")?;
            emit_type(out, ty)?;
            write!(out, " = ")?;
            emit_expr(out, value, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        Stmt::Assign { name, value } => {
            write!(out, "{indent}{name} = ")?;
            emit_expr(out, value, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-504: `let <name> = |<params>| { <body> };` — a first-class
        // closure (0+ params). The return type is left to Rust inference.
        Stmt::ClosureLet { name, params, body } => {
            write!(out, "{indent}let {name} = |")?;
            for (i, (p, ty)) in params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write!(out, "{p}: ")?;
                emit_type(out, ty)?;
            }
            out.push_str("| { ");
            emit_expr(out, body, mode)?;
            writeln!(out, " }};")?;
            Ok(())
        }
        // PMAT-494b: tuple unpacking → `let (a, b, ...) = <value>;`.
        Stmt::LetTuple { names, value } => {
            write!(out, "{indent}let ({}) = ", names.join(", "))?;
            emit_expr(out, value, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-479 (R10): early `return <expr>;` (e.g. a guard clause).
        Stmt::Return(e) => {
            write!(out, "{indent}return ")?;
            emit_expr(out, e, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-502bk: loop-control statements.
        Stmt::Continue => {
            writeln!(out, "{indent}continue;")?;
            Ok(())
        }
        Stmt::Break => {
            writeln!(out, "{indent}break;")?;
            Ok(())
        }
        // PMAT-502bw/by: `print(a, b, …, sep=…, end=…)`. Args are joined by
        // `sep` in the format string; `end == "\n"` (Python default) uses
        // `println!` (which appends the newline), any other `end` uses
        // `print!` with `end` appended literally. Bare `print()` →
        // `println!();` (or `print!("…end…")`).
        Stmt::Print { args, sep, end } => {
            let macro_name = if end == "\n" { "println!" } else { "print!" };
            let sep_esc = escape_format_literal(sep);
            let mut fmt = (0..args.len())
                .map(|_| "{}")
                .collect::<Vec<_>>()
                .join(&sep_esc);
            if end != "\n" {
                fmt.push_str(&escape_format_literal(end));
            }
            if args.is_empty() && fmt.is_empty() {
                writeln!(out, "{indent}{macro_name}();")?;
                return Ok(());
            }
            write!(out, "{indent}{macro_name}(\"{fmt}\"")?;
            for a in args {
                out.push_str(", ");
                emit_expr(out, a, mode)?;
            }
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-478 (R9): if/else statement → Rust `if c { … } else { … }`.
        // The `else` block is omitted when `else_body` is empty.
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            write!(out, "{indent}if ")?;
            emit_expr(out, cond, mode)?;
            writeln!(out, " {{")?;
            let inner = format!("{indent}    ");
            for s in then_body {
                emit_stmt_indented(out, s, &inner, mode)?;
            }
            if else_body.is_empty() {
                writeln!(out, "{indent}}}")?;
            } else {
                writeln!(out, "{indent}}} else {{")?;
                for s in else_body {
                    emit_stmt_indented(out, s, &inner, mode)?;
                }
                writeln!(out, "{indent}}}")?;
            }
            Ok(())
        }
        Stmt::While { cond, body } => {
            write!(out, "{indent}while ")?;
            emit_expr(out, cond, mode)?;
            writeln!(out, " {{")?;
            let inner = format!("{indent}    ");
            for s in body {
                emit_stmt_indented(out, s, &inner, mode)?;
            }
            writeln!(out, "{indent}}}")?;
            Ok(())
        }
        // PMAT-458 (v0.2.0 Track 1.B): for-each over a collection.
        // Emit `for var in iter.iter().cloned() { body }` — the
        // .iter().cloned() produces owned elements matching the
        // v0.2.0 owned-value posture (Index already returns .clone(),
        // so the body sees owned values consistently).
        Stmt::ForEach {
            var,
            iter,
            body,
            over_keys,
            ..
        } => {
            // PMAT-472 (R3): a dict iterates keys (`for k in d:`) via
            // `.keys().cloned()`; a list iterates elements via
            // `.iter().cloned()`. Both yield owned values.
            let method = if *over_keys { "keys" } else { "iter" };
            write!(out, "{indent}for {var} in ")?;
            emit_expr(out, iter, mode)?;
            writeln!(out, ".{method}().cloned() {{")?;
            let inner = format!("{indent}    ");
            for s in body {
                emit_stmt_indented(out, s, &inner, mode)?;
            }
            writeln!(out, "{indent}}}")?;
            Ok(())
        }
        // PMAT-495: paired for-loop. enumerate → `(i as i64, e)`; zip →
        // both iterators `.iter().cloned()`.
        Stmt::ForEachPair {
            first,
            second,
            iter,
            kind,
            body,
        } => {
            write!(out, "{indent}for ({first}, {second}) in ")?;
            emit_expr(out, iter, mode)?;
            match kind {
                xpile_meta_hir::PairIterKind::Enumerate { start } => {
                    // PMAT-502ca: `enumerate(xs, start)` offsets the index.
                    if *start == 0 {
                        out.push_str(
                            ".iter().cloned().enumerate().map(|(__i, __e)| (__i as i64, __e))",
                        );
                    } else {
                        write!(
                            out,
                            ".iter().cloned().enumerate().map(|(__i, __e)| (__i as i64 + {start}i64, __e))"
                        )?;
                    }
                }
                xpile_meta_hir::PairIterKind::Zip(other) => {
                    out.push_str(".iter().cloned().zip(");
                    emit_expr(out, other, mode)?;
                    out.push_str(".iter().cloned())");
                }
                // PMAT-502y: iterate a list of 2-tuples, destructuring each.
                xpile_meta_hir::PairIterKind::Pairs => {
                    out.push_str(".iter().cloned()");
                }
            }
            writeln!(out, " {{")?;
            let inner = format!("{indent}    ");
            for s in body {
                emit_stmt_indented(out, s, &inner, mode)?;
            }
            writeln!(out, "{indent}}}")?;
            Ok(())
        }
        // PMAT-460 (v0.2.0 Track 1.B): Python `xs.append(v)` → Rust
        // `xs.push(v);`. The frontend has already marked `xs` as
        // mutable so the emission type-checks.
        Stmt::ListAppend { list_name, elem } => {
            write!(out, "{indent}{list_name}.push(")?;
            emit_expr(out, elem, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-500b: Python `s.add(x)` → Rust `s.insert(x);`.
        Stmt::SetAdd { set_name, elem } => {
            write!(out, "{indent}{set_name}.insert(")?;
            emit_expr(out, elem, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-502av: Python `s.remove(x)` panics if absent (KeyError) →
        // `assert!(s.remove(&(x)), "…");`; `s.discard(x)` is a silent no-op
        // → `s.remove(&(x));` (the returned bool is discarded).
        Stmt::SetRemove {
            set_name,
            elem,
            error_if_absent,
        } => {
            if *error_if_absent {
                write!(out, "{indent}assert!({set_name}.remove(&(")?;
                emit_expr(out, elem, mode)?;
                writeln!(
                    out,
                    ")), \"xpile: KeyError: set.remove(x): x not in set\");"
                )?;
            } else {
                write!(out, "{indent}{set_name}.remove(&(")?;
                emit_expr(out, elem, mode)?;
                writeln!(out, "));")?;
            }
            Ok(())
        }
        // PMAT-502ap: in-place list mutators `xs.sort()/.reverse()/.clear()`.
        // `Vec<f64>` has no `Ord`, so a float sort uses `sort_by(partial_cmp)`.
        Stmt::ListMutate {
            list_name,
            op,
            of_float,
        } => {
            match op {
                ListMutateOp::Sort if *of_float => writeln!(
                    out,
                    "{indent}{list_name}.sort_by(|a, b| a.partial_cmp(b).unwrap());"
                )?,
                ListMutateOp::Sort => writeln!(out, "{indent}{list_name}.sort();")?,
                ListMutateOp::Reverse => writeln!(out, "{indent}{list_name}.reverse();")?,
                ListMutateOp::Clear => writeln!(out, "{indent}{list_name}.clear();")?,
            }
            Ok(())
        }
        // PMAT-502aq: `xs.extend(ys)` → `xs.extend((<ys>).iter().cloned());`.
        Stmt::ListExtend { list_name, other } => {
            write!(out, "{indent}{list_name}.extend((")?;
            emit_expr(out, other, mode)?;
            writeln!(out, ").iter().cloned());")?;
            Ok(())
        }
        // PMAT-502bb: `d.update(other)` → merge entries, cloning each
        // (`other` is not consumed, matching Python).
        Stmt::DictUpdate { dict_name, other } => {
            write!(out, "{indent}{dict_name}.extend((")?;
            emit_expr(out, other, mode)?;
            writeln!(
                out,
                ").iter().map(|(__k, __v)| (__k.clone(), __v.clone())));"
            )?;
            Ok(())
        }
        // PMAT-502ar: `xs.insert(i, x)` → `xs.insert((i) as usize, x);`
        // (same `as usize` coercion as IndexAssign).
        Stmt::ListInsert {
            list_name,
            index,
            elem,
        } => {
            write!(out, "{indent}{list_name}.insert((")?;
            emit_expr(out, index, mode)?;
            out.push_str(") as usize, ");
            emit_expr(out, elem, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-461 (v0.2.0 Track 1.B): Python `xs[i] = v` → Rust
        // `xs[i as usize] = v;`. Same `as usize` coercion as
        // Expr::Index; same param-mut threading as ListAppend.
        Stmt::IndexAssign {
            list_name,
            index,
            value,
        } => {
            write!(out, "{indent}{list_name}[")?;
            emit_expr(out, index, mode)?;
            out.push_str(" as usize] = ");
            emit_expr(out, value, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-466 (v0.2.0 Track 1.C): Python `d[k] = v` → Rust
        // `{ let __v = v; d.insert(k.clone(), __v); }`. Present-key
        // overwrite / absent-key insert matches Python dict assignment.
        //
        // Two subtleties, both about the move-then-borrow hazard of a
        // non-Copy (`String`) key:
        //   1. The value is bound to a temp BEFORE `.insert`, so the
        //      canonical `d[k] = d.get(k, 0) + 1` idiom (value borrows
        //      the key) doesn't move the key out from under its own
        //      value expression (E0382). Binding the value first also
        //      ends the immutable `.get` borrow before the mutable
        //      `.insert` borrow (NLL).
        //   2. The key is `.clone()`d into `.insert` so the caller's key
        //      binding survives a *later* read of the same key (e.g.
        //      `d[k] = …; return d[k]`). For Copy keys (int/bool) the
        //      clone is a no-op move; `rustc` accepts it (the
        //      `clone_on_copy` lint is clippy-only and xpile does not
        //      clippy emitted output).
        Stmt::DictSet {
            dict_name,
            key,
            value,
        } => {
            write!(out, "{indent}{{ let __xpile_dict_val = ")?;
            emit_expr(out, value, mode)?;
            write!(out, "; {dict_name}.insert(")?;
            emit_expr(out, key, mode)?;
            writeln!(out, ".clone(), __xpile_dict_val); }}")?;
            Ok(())
        }
        // PMAT-502at: Python `del coll[key]`. list → `coll.remove((k) as
        // usize);` (shift tail left; panics past end = Python IndexError);
        // dict → `coll.remove(&(k));` (discards the value).
        Stmt::DelItem { name, key, is_dict } => {
            if *is_dict {
                write!(out, "{indent}{name}.remove(&(")?;
                emit_expr(out, key, mode)?;
                writeln!(out, "));")?;
            } else {
                write!(out, "{indent}{name}.remove((")?;
                emit_expr(out, key, mode)?;
                writeln!(out, ") as usize);")?;
            }
            Ok(())
        }
        // PMAT-502ao: `assert cond, msg` → `assert!(cond, "{}", <msg>);`.
        Stmt::Assert { cond, msg } => {
            write!(out, "{indent}assert!(")?;
            emit_expr(out, cond, mode)?;
            if let Some(msg) = msg {
                out.push_str(", \"{}\", ");
                emit_expr(out, msg, mode)?;
            }
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-503a: `raise Exc("msg")` → `panic!("{}", <message>);`. The
        // diverging `!` type unifies with any function return, so a `raise`
        // in a guard clause type-checks without a phantom value.
        Stmt::Raise { message } => {
            write!(out, "{indent}panic!(\"{{}}\", ")?;
            emit_expr(out, message, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-039 / XPILE-BASHRS-MERGER-001 Layer B: shell-command
        // statements are produced exclusively by bashrs-frontend and
        // consumed exclusively by bashrs-backend. The Rust backend
        // refuses them — there is no meaningful Rust translation of an
        // anonymous shell-line invocation that respects
        // `C-BASHRS-POSIX-IDEMPOTENCE`. (A future cross-domain
        // refinement of `subprocess.run([...])` into a typed
        // `Stmt::Cmd` would still be lowered via Rust's
        // `std::process::Command` API — that's separate machinery, not
        // a generic Cmd-to-Rust translation.)
        Stmt::Cmd { program, args } => Err(CodegenError::Unsupported(format!(
            "Rust backend does not lower Stmt::Cmd (`{program}` with {} arg(s)) — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs this construct; \
             use `--target shell` to emit POSIX sh via bashrs-backend",
            args.len()
        ))),
        // PMAT-041: see Cmd arm above. Pipelines have the same
        // cross-domain disposition.
        Stmt::Pipeline { stages } => Err(CodegenError::Unsupported(format!(
            "Rust backend does not lower Stmt::Pipeline ({} stages) — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell pipelines; \
             use `--target shell` to emit POSIX sh via bashrs-backend",
            stages.len()
        ))),
        // PMAT-048: same disposition as the rest of the shell domain.
        Stmt::ShellLoop { .. } => Err(CodegenError::Unsupported(
            "Rust backend does not lower Stmt::ShellLoop — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell loops; \
             use `--target shell`"
                .into(),
        )),
        // PMAT-051: same disposition.
        Stmt::ShellAssign { name, .. } => Err(CodegenError::Unsupported(format!(
            "Rust backend does not lower Stmt::ShellAssign (`{name}=…`) — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell variable assignment; \
             use `--target shell`"
        ))),
    }
}

fn emit_param(out: &mut String, p: &Param) -> Result<(), CodegenError> {
    // PMAT-460: `mut name: T` for params mutated in-place (currently
    // only via xs.append(v)). Required for Rust to type-check the
    // emitted `name.push(v)`.
    if p.mutable {
        write!(out, "mut ")?;
    }
    write!(out, "{}: ", p.name)?;
    emit_type(out, &p.ty)?;
    Ok(())
}

/// Escape a string for emission inside a Rust `"..."` literal.
/// PMAT-449 (v0.2.0 Track 1.A): minimal escape set for the first
/// `Type::Str` pass — `\` and `"`. Newlines / tabs / unicode escapes
/// land in subsequent sub-tracks alongside f-string lowering.
fn escape_rust_str(s: &str) -> String {
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

fn emit_type(out: &mut String, t: &Type) -> Result<(), CodegenError> {
    match t {
        Type::I64 => out.push_str("i64"),
        // PMAT-477 (R8): Python `float` → Rust `f64`.
        Type::F64 => out.push_str("f64"),
        Type::Bool => out.push_str("bool"),
        // PMAT-502bl: Python `None` return → Rust unit `()`.
        Type::Unit => out.push_str("()"),
        // PMAT-012: re-exported from `xpile-bigint` (which wraps
        // `num_bigint::BigInt`). Operator overloads (`+`, `-`, `*`,
        // `<=`, …) work without method calls, matching the i64 codegen
        // shape — except no `.checked_*().expect(...)` since BigInt
        // never overflows.
        Type::BigInt => out.push_str("xpile_bigint::BigInt"),
        // PMAT-449: v0.2.0 Track 1.A — Python `str` → Rust owned
        // `String`. First pass is owned-only; `&str` borrowing is the
        // 1.D stretch sub-track per sub/v0.2.0-depyler-merger.md.
        Type::Str => out.push_str("String"),
        // PMAT-455: v0.2.0 Track 1.B — Python `list[T]` → Rust
        // `Vec<T>`. Owned-first; lifetime-borrowing variants come
        // after Track 1.D `&str` work lands.
        Type::List(elem_ty) => {
            out.push_str("Vec<");
            emit_type(out, elem_ty)?;
            out.push('>');
        }
        // PMAT-462: v0.2.0 Track 1.C — Python `dict[K, V]` → Rust
        // `std::collections::HashMap<K, V>`. Owned-first. The
        // fully-qualified path avoids requiring callers to add a
        // `use` statement.
        Type::Dict(k_ty, v_ty) => {
            out.push_str("std::collections::HashMap<");
            emit_type(out, k_ty)?;
            out.push_str(", ");
            emit_type(out, v_ty)?;
            out.push('>');
        }
        // PMAT-500: Python `set[T]` → Rust `HashSet<T>`.
        Type::Set(elem_ty) => {
            out.push_str("std::collections::HashSet<");
            emit_type(out, elem_ty)?;
            out.push('>');
        }
        // PMAT-494: Python `tuple[T0, T1, ...]` → Rust `(T0, T1, ...)`.
        Type::Tuple(elems) => {
            out.push('(');
            for (i, t) in elems.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit_type(out, t)?;
            }
            out.push(')');
        }
        // PMAT-046: bashrs-domain types. Rust backend refuses — the
        // analogous Rust type for ShellString would be the bashrs
        // runtime's quoting-aware wrapper (not yet shipped); the
        // analogous type for ExitCode is `std::process::ExitStatus`
        // but lowering meta-HIR `Type::ExitCode` to that requires
        // touching the broader `std::process` integration which is
        // XPILE-BASHRS-MERGER-***+. Use `--target shell` instead.
        Type::ShellString | Type::ExitCode => {
            return Err(CodegenError::Unsupported(format!(
                "Rust backend does not lower {t:?} — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs the bashrs type domain; \
                 use `--target shell` for shell-typed signatures"
            )));
        }
    }
    Ok(())
}

fn emit_expr(out: &mut String, e: &Expr, mode: bool) -> Result<(), CodegenError> {
    match e {
        // PMAT-502bl: the unit value (void function trailing return).
        Expr::Unit => out.push_str("()"),
        Expr::Ident(name) => {
            // PMAT-013: in BigInt mode, append `.clone()` to every
            // Ident reference. BigInt isn't `Copy`, so an Ident used
            // more than once in a function body (cond + branches,
            // multiplication + recursive call, etc.) would move-on-
            // first-use otherwise. Cloning unconditionally is mechanical
            // and correct; LLVM elides unneeded clones at -O.
            if mode {
                write!(out, "{}.clone()", name)?;
            } else {
                write!(out, "{}", name)?;
            }
        }
        Expr::LitInt(v) => {
            if mode {
                // PMAT-012: literal `n` in a BigInt-mode function is
                // `BigInt::from(<n>i64)`. num-bigint accepts i64 directly.
                write!(out, "xpile_bigint::BigInt::from({}i64)", v)?;
            } else {
                write!(out, "{}i64", v)?;
            }
        }
        // PMAT-477 (R8): float literal → `<v>f64`; float arithmetic →
        // plain infix (IEEE-754 saturates, no checked path).
        Expr::LitFloat(v) => write!(out, "{}f64", v)?,
        Expr::FloatBinOp { op, lhs, rhs } => match op {
            // PMAT-502br: Python float floor-division `a // b` → `(a / b).floor()`.
            FloatOp::FloorDiv => {
                out.push_str("((");
                emit_expr(out, lhs, mode)?;
                out.push_str(" / ");
                emit_expr(out, rhs, mode)?;
                out.push_str(").floor())");
            }
            // PMAT-502br: Python float modulo `a % b` follows the divisor's
            // sign → `a - b * (a / b).floor()` (Rust's `%` follows the
            // dividend, which diverges for mixed signs).
            FloatOp::Mod => {
                out.push('(');
                emit_expr(out, lhs, mode)?;
                out.push_str(" - ");
                emit_expr(out, rhs, mode)?;
                out.push_str(" * (");
                emit_expr(out, lhs, mode)?;
                out.push_str(" / ");
                emit_expr(out, rhs, mode)?;
                out.push_str(").floor())");
            }
            // PMAT-502bt: Python float power `a ** b` → `(a).powf(b)`
            // (both operands are f64).
            FloatOp::Pow => {
                out.push('(');
                emit_expr(out, lhs, mode)?;
                out.push_str(").powf(");
                emit_expr(out, rhs, mode)?;
                out.push(')');
            }
            FloatOp::Add | FloatOp::Sub | FloatOp::Mul | FloatOp::Div => {
                out.push('(');
                emit_expr(out, lhs, mode)?;
                write!(out, " {} ", float_op_sym(*op))?;
                emit_expr(out, rhs, mode)?;
                out.push(')');
            }
        },
        // PMAT-456 (v0.2.0 Track 1.B): bool literal — Rust's
        // lowercase `true` / `false`.
        Expr::LitBool(b) => write!(out, "{}", b)?,
        Expr::BinOp { op, lhs, rhs } => emit_binop(out, *op, lhs, rhs, mode)?,
        // PMAT-451 (v0.2.0 Track 1.A): str concatenation. Rust's
        // `String + &str` is the idiomatic form but requires the lhs
        // to be owned and rhs to be borrowed — annoying to thread
        // through when both come from the same xpile lowering pipeline.
        // `format!("{}{}", l, r)` works uniformly for any `Display`
        // operands and produces an owned `String`, matching the v0.2.0
        // owned-only ownership posture (see C-XLATE-PY-STR-TO-RUST-STRING
        // `ownership_owned` equation).
        Expr::Concat { lhs, rhs } => {
            out.push_str("format!(\"{}{}\", ");
            emit_expr(out, lhs, mode)?;
            out.push_str(", ");
            emit_expr(out, rhs, mode)?;
            out.push(')');
        }
        // PMAT-502bg: `xs + ys` (lists) → a fresh `Vec` chaining both,
        // consuming neither operand (matching Python).
        Expr::ListConcat { lhs, rhs } => {
            out.push('(');
            emit_expr(out, lhs, mode)?;
            out.push_str(").iter().chain((");
            emit_expr(out, rhs, mode)?;
            out.push_str(").iter()).cloned().collect::<Vec<_>>()");
        }
        // PMAT-502bh: `"<fmt>".format(args…)` → `format!("<fmt>", args…)`.
        // `{fmt:?}` re-escapes the validated format string as a Rust string
        // literal (preserving `{}` placeholders + `{{`/`}}` escapes).
        Expr::StrFormat { fmt, args } => {
            write!(out, "format!({fmt:?}")?;
            for a in args {
                out.push_str(", ");
                emit_expr(out, a, mode)?;
            }
            out.push(')');
        }
        // PMAT-502am: a formatted f-string field → `format!("{:<spec>}", v)`.
        Expr::FormatSpec { value, rust_spec } => {
            write!(out, "format!(\"{{:{rust_spec}}}\", ")?;
            emit_expr(out, value, mode)?;
            out.push(')');
        }
        // PMAT-502cd: `s[i]` over a string — materialise the chars and index
        // them (Rust `String` has no positional `[]`). Negative `i` counts
        // from the end; out-of-range panics (≈ Python `IndexError`).
        Expr::StrCharAt { string, index } => {
            out.push_str("{ let __cs: Vec<char> = (");
            emit_expr(out, string, mode)?;
            out.push_str(").chars().collect(); let __i: i64 = (");
            emit_expr(out, index, mode)?;
            out.push_str("); let __idx = if __i < 0 { __cs.len() as i64 + __i } else { __i }; __cs[__idx as usize].to_string() }");
        }
        // PMAT-502cl: string chars as a Vec<String> (for `for c in s`).
        Expr::StrChars { string } => {
            out.push('(');
            emit_expr(out, string, mode)?;
            out.push_str(").chars().map(|__c| __c.to_string()).collect::<Vec<String>>()");
        }
        // PMAT-502cm: ord(c) → code point; chr(n) → 1-char string.
        Expr::Ord { value } => {
            out.push('(');
            emit_expr(out, value, mode)?;
            out.push_str(
                ".chars().next().expect(\"xpile: ord() expected a single character\") as i64)",
            );
        }
        Expr::Chr { value } => {
            out.push_str("char::from_u32((");
            emit_expr(out, value, mode)?;
            out.push_str(") as u32).expect(\"xpile: chr() arg not in range(0x110000) (ValueError)\").to_string()");
        }
        // PMAT-502cv: hex/oct/bin → radix string, sign-first (magnitude via
        // `unsigned_abs` so i64::MIN is safe).
        Expr::IntRadixStr { value, radix } => {
            out.push_str("{ let __n = (");
            emit_expr(out, value, mode)?;
            out.push_str(
                "); let __m = __n.unsigned_abs(); let __sign = if __n < 0 { \"-\" } else { \"\" }; format!(\"{}",
            );
            out.push_str(match radix {
                Radix::Hex => "0x{:x}",
                Radix::Oct => "0o{:o}",
                Radix::Bin => "0b{:b}",
            });
            out.push_str("\", __sign, __m) }");
        }
        // PMAT-502da: `int(s, base)` → `i64::from_str_radix((s).trim(), base)`
        // (a parse failure / out-of-range digit panics ≈ Python ValueError).
        Expr::IntFromStrRadix { value, radix } => {
            out.push_str("i64::from_str_radix((");
            emit_expr(out, value, mode)?;
            out.push_str(&format!(
                ").trim(), {radix}).expect(\"xpile: ValueError: invalid literal for int() with base {radix}\")"
            ));
        }
        // PMAT-492/493b: Python string methods. No-arg transforms emit a
        // suffix; the startswith/endswith predicates emit
        // `.starts_with(&(<pat>)[..])` — the `&(..)[..]` reslice yields
        // `&str` uniformly whether the pattern is a `String` or a literal.
        Expr::StrMethod { recv, op, args } => {
            // PMAT-492d: `join` inverts receiver/arg — Python `sep.join(xs)`
            // is Rust `xs.join(sep)` — so emit the list arg as the receiver.
            if matches!(op, StrMethodOp::Join) {
                emit_expr(out, &args[0], mode)?;
                out.push_str(".join(&(");
                emit_expr(out, recv, mode)?;
                out.push_str(")[..])");
            } else if matches!(
                op,
                StrMethodOp::IsDigit
                    | StrMethodOp::IsAlpha
                    | StrMethodOp::IsSpace
                    | StrMethodOp::IsAlnum
            ) {
                // PMAT-502ag/502di: `.isdigit()`/`.isalpha()`/`.isspace()`/
                // `.isalnum()` → `(!(s).is_empty() && (s).chars().all(|__c|
                // __c.<pred>()))`. The empty guard matches Python.
                out.push_str("(!(");
                emit_expr(out, recv, mode)?;
                out.push_str(").is_empty() && (");
                emit_expr(out, recv, mode)?;
                out.push_str(").chars().all(|__c| __c.");
                out.push_str(match op {
                    StrMethodOp::IsDigit => "is_ascii_digit()",
                    StrMethodOp::IsAlpha => "is_alphabetic()",
                    StrMethodOp::IsAlnum => "is_alphanumeric()",
                    _ => "is_whitespace()",
                });
                out.push_str("))");
            } else if matches!(op, StrMethodOp::IsUpper | StrMethodOp::IsLower) {
                // PMAT-502di: `.isupper()` → at least one cased char AND no
                // lowercase among them: `((s).chars().any(|__c|
                // __c.is_uppercase()) && !(s).chars().any(|__c|
                // __c.is_lowercase()))`. `.islower()` is the mirror.
                let (want, forbid) = if matches!(op, StrMethodOp::IsUpper) {
                    ("is_uppercase()", "is_lowercase()")
                } else {
                    ("is_lowercase()", "is_uppercase()")
                };
                out.push_str("((");
                emit_expr(out, recv, mode)?;
                write!(out, ").chars().any(|__c| __c.{want}) && !(")?;
                emit_expr(out, recv, mode)?;
                write!(out, ").chars().any(|__c| __c.{forbid}))")?;
            } else if matches!(op, StrMethodOp::Capitalize) {
                // PMAT-502ah: `.capitalize()` → first char upper, rest lower
                // (empty → ""), matching Python.
                out.push_str("{ let __cs = &(");
                emit_expr(out, recv, mode)?;
                out.push_str("); let mut __ch = __cs.chars(); match __ch.next() { Some(__f) => __f.to_uppercase().collect::<String>() + &(__ch.as_str().to_lowercase()), None => String::new() } }");
            } else if matches!(op, StrMethodOp::Title) {
                // PMAT-502aj: `.title()` → upper the first alpha of each word,
                // lower the rest; any non-alpha is a word boundary (matches
                // Python, incl. `"it's".title()` → `"It'S"`).
                out.push_str("{ let mut __tr = String::new(); let mut __pa = false; for __c in (");
                emit_expr(out, recv, mode)?;
                out.push_str(").chars() { if __c.is_alphabetic() { if __pa { __tr.extend(__c.to_lowercase()); } else { __tr.extend(__c.to_uppercase()); } __pa = true; } else { __tr.push(__c); __pa = false; } } __tr }");
            } else if matches!(op, StrMethodOp::RJust | StrMethodOp::LJust) {
                // PMAT-502aw: `.rjust(w)`/`.ljust(w)` → `format!("{:>1$}", s, w)`
                // / `format!("{:<1$}", s, w)`. Rust's format width is a minimum,
                // so a longer string is returned unchanged (matching Python).
                out.push_str(if matches!(op, StrMethodOp::RJust) {
                    "format!(\"{:>1$}\", "
                } else {
                    "format!(\"{:<1$}\", "
                });
                emit_expr(out, recv, mode)?;
                out.push_str(", (");
                emit_expr(out, &args[0], mode)?;
                out.push_str(") as usize)");
            } else if matches!(op, StrMethodOp::RemovePrefix | StrMethodOp::RemoveSuffix) {
                // PMAT-502cq: `.removeprefix(p)`/`.removesuffix(p)` →
                // `strip_prefix`/`strip_suffix`, returning the receiver
                // unchanged when the affix is absent (matching Python).
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str(if matches!(op, StrMethodOp::RemovePrefix) {
                    "); match __s.strip_prefix(&("
                } else {
                    "); match __s.strip_suffix(&("
                });
                emit_expr(out, &args[0], mode)?;
                out.push_str(")[..]) { Some(__r) => __r.to_string(), None => __s } }");
            } else if matches!(op, StrMethodOp::ZFill) {
                // PMAT-502cs: `.zfill(w)` → sign-aware zero-pad to `w` chars
                // (a leading -/+ stays first; already-wide strings unchanged).
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str("); let __w = (");
                emit_expr(out, &args[0], mode)?;
                out.push_str(") as usize; let __n = __s.chars().count(); if __n >= __w { __s } else { let __pad = \"0\".repeat(__w - __n); if __s.starts_with('-') || __s.starts_with('+') { format!(\"{}{}{}\", &__s[..1], __pad, &__s[1..]) } else { format!(\"{}{}\", __pad, __s) } } }");
            } else if matches!(op, StrMethodOp::Center) {
                // PMAT-502cu: `.center(w)` → space-pad centred, CPython bias
                // `left = marg/2 + (marg & w & 1)` (extra padding parity).
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str("); let __w = (");
                emit_expr(out, &args[0], mode)?;
                out.push_str(") as usize; let __n = __s.chars().count(); if __n >= __w { __s } else { let __marg = __w - __n; let __left = __marg / 2 + (__marg & __w & 1); format!(\"{}{}{}\", \" \".repeat(__left), __s, \" \".repeat(__marg - __left)) } }");
            } else if matches!(op, StrMethodOp::Partition | StrMethodOp::RPartition) {
                // PMAT-502dj: `.partition(sep)` / `.rpartition(sep)` → the
                // 3-tuple `(before, sep, after)` at the first / last `sep`. The
                // absent case differs: partition → `(s, "", "")`, rpartition →
                // `("", "", s)` (matching Python).
                let is_r = matches!(op, StrMethodOp::RPartition);
                out.push_str("match (");
                emit_expr(out, recv, mode)?;
                out.push_str(if is_r {
                    ").rsplit_once(&("
                } else {
                    ").split_once(&("
                });
                emit_expr(out, &args[0], mode)?;
                out.push_str(")[..]) { Some((__a, __b)) => (__a.to_string(), (");
                emit_expr(out, &args[0], mode)?;
                out.push_str(").to_string(), __b.to_string()), None => ");
                if is_r {
                    out.push_str("(String::new(), String::new(), (");
                    emit_expr(out, recv, mode)?;
                    out.push_str(").to_string()) }");
                } else {
                    out.push('(');
                    emit_expr(out, recv, mode)?;
                    out.push_str(".to_string(), String::new(), String::new()) }");
                }
            } else if matches!(op, StrMethodOp::SplitLines) {
                // PMAT-502dl: `.splitlines()` → split on Python's full line
                // boundary set (Rust's `str::lines()` only covers LF/CRLF), with
                // no trailing empty element for a trailing break. Char-walk.
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str("); let mut __lines: Vec<String> = Vec::new(); let mut __cur = String::new(); let mut __it = __s.chars().peekable(); while let Some(__c) = __it.next() { match __c { '\\r' => { if __it.peek() == Some(&'\\n') { __it.next(); } __lines.push(std::mem::take(&mut __cur)); } '\\n' | '\\u{0b}' | '\\u{0c}' | '\\u{1c}' | '\\u{1d}' | '\\u{1e}' | '\\u{85}' | '\\u{2028}' | '\\u{2029}' => { __lines.push(std::mem::take(&mut __cur)); } _ => __cur.push(__c), } } if !__cur.is_empty() { __lines.push(__cur); } __lines }");
            } else {
                emit_expr(out, recv, mode)?;
                match op {
                    StrMethodOp::Upper => out.push_str(".to_uppercase()"),
                    StrMethodOp::Lower => out.push_str(".to_lowercase()"),
                    StrMethodOp::Strip => out.push_str(".trim().to_string()"),
                    // PMAT-502cr: `.swapcase()` → per-char upper↔lower.
                    StrMethodOp::SwapCase => out.push_str(
                        ".chars().map(|__c| if __c.is_uppercase() { __c.to_lowercase().collect::<String>() } else if __c.is_lowercase() { __c.to_uppercase().collect::<String>() } else { __c.to_string() }).collect::<String>()",
                    ),
                    StrMethodOp::StartsWith | StrMethodOp::EndsWith => {
                        out.push_str(if matches!(op, StrMethodOp::StartsWith) {
                            ".starts_with(&("
                        } else {
                            ".ends_with(&("
                        });
                        emit_expr(out, &args[0], mode)?;
                        out.push_str(")[..])");
                    }
                    // PMAT-492c: `.split(sep)` → Vec<String>.
                    StrMethodOp::Split => {
                        out.push_str(".split(&(");
                        emit_expr(out, &args[0], mode)?;
                        out.push_str(")[..]).map(|__c| __c.to_string()).collect::<Vec<String>>()");
                    }
                    // PMAT-502co: no-arg `.split()` → whitespace split.
                    StrMethodOp::SplitWhitespace => {
                        out.push_str(
                            ".split_whitespace().map(|__c| __c.to_string()).collect::<Vec<String>>()",
                        );
                    }
                    // PMAT-502b: `.replace(old, new)` → `.replace(&(old)[..], &(new)[..])`.
                    StrMethodOp::Replace => {
                        out.push_str(".replace(&(");
                        emit_expr(out, &args[0], mode)?;
                        out.push_str(")[..], &(");
                        emit_expr(out, &args[1], mode)?;
                        out.push_str(")[..])");
                    }
                    // PMAT-502l: lstrip/rstrip → trim_start/trim_end.
                    StrMethodOp::LStrip => out.push_str(".trim_start().to_string()"),
                    StrMethodOp::RStrip => out.push_str(".trim_end().to_string()"),
                    // PMAT-502l: `.find(sub)` → byte index or -1 (i64).
                    StrMethodOp::Find => {
                        out.push_str(".find(&(");
                        emit_expr(out, &args[0], mode)?;
                        out.push_str(")[..]).map(|__i| __i as i64).unwrap_or(-1)");
                    }
                    // PMAT-502l: `.count(sub)` → non-overlapping match count (i64).
                    StrMethodOp::Count => {
                        out.push_str(".matches(&(");
                        emit_expr(out, &args[0], mode)?;
                        out.push_str(")[..]).count() as i64");
                    }
                    // PMAT-502bi: `.index(sub)` → byte index or panic (ValueError).
                    StrMethodOp::StrIndex => {
                        out.push_str(".find(&(");
                        emit_expr(out, &args[0], mode)?;
                        out.push_str(")[..]).map(|__i| __i as i64).expect(\"xpile: ValueError: substring not found\")");
                    }
                    StrMethodOp::Join => unreachable!("Join handled above"),
                    StrMethodOp::IsDigit
                    | StrMethodOp::IsAlpha
                    | StrMethodOp::IsSpace
                    | StrMethodOp::IsAlnum
                    | StrMethodOp::IsUpper
                    | StrMethodOp::IsLower => {
                        unreachable!("classification predicates handled above")
                    }
                    StrMethodOp::Capitalize => unreachable!("capitalize handled above"),
                    StrMethodOp::Title => unreachable!("title handled above"),
                    StrMethodOp::RJust | StrMethodOp::LJust => {
                        unreachable!("rjust/ljust handled above")
                    }
                    StrMethodOp::RemovePrefix | StrMethodOp::RemoveSuffix => {
                        unreachable!("removeprefix/removesuffix handled above")
                    }
                    StrMethodOp::ZFill => unreachable!("zfill handled above"),
                    StrMethodOp::Center => unreachable!("center handled above"),
                    StrMethodOp::Partition | StrMethodOp::RPartition => {
                        unreachable!("partition/rpartition handled above")
                    }
                    StrMethodOp::SplitLines => unreachable!("splitlines handled above"),
                }
            }
        }
        // PMAT-455 (v0.2.0 Track 1.B): Python list literal → Rust
        // `vec![...]` macro. The element types are guaranteed
        // homogeneous by the frontend's lowering check.
        Expr::ListLit(elems) => {
            out.push_str("vec![");
            for (i, e) in elems.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit_expr(out, e, mode)?;
            }
            out.push(']');
        }
        // PMAT-494: Python tuple literal → Rust `(e0, e1, ...)`.
        Expr::TupleLit(elems) => {
            out.push('(');
            for (i, e) in elems.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit_expr(out, e, mode)?;
            }
            out.push(')');
        }
        // PMAT-502q: Python `t[N]` (tuple) → `(<tuple>).N.clone()` — Rust
        // tuple field access, owned-value posture (matches list-index clone).
        Expr::TupleIndex { tuple, index } => {
            out.push('(');
            emit_expr(out, tuple, mode)?;
            write!(out, ").{index}.clone()")?;
        }
        // PMAT-496: Python `xs[lo:hi]` slice → `<c>[(lo) as usize..(hi)
        // as usize].to_vec()` (list) / `.to_string()` (str).
        Expr::Slice {
            collection,
            lo,
            hi,
            of_str,
            step,
        } => {
            // PMAT-502r: an absent bound is an open end (`a..`, `..b`, `..`).
            emit_expr(out, collection, mode)?;
            out.push('[');
            if let Some(lo) = lo {
                out.push('(');
                emit_expr(out, lo, mode)?;
                out.push_str(") as usize");
            }
            out.push_str("..");
            if let Some(hi) = hi {
                out.push('(');
                emit_expr(out, hi, mode)?;
                out.push_str(") as usize");
            }
            out.push(']');
            match step {
                // PMAT-502bc: positive list step → `.iter().step_by(c)
                // .cloned().collect::<Vec<_>>()` (str steps are rejected
                // in the frontend, so `step` is only ever set for lists).
                Some(s) => {
                    write!(out, ".iter().step_by({s}).cloned().collect::<Vec<_>>()")?;
                }
                None => out.push_str(if *of_str { ".to_string()" } else { ".to_vec()" }),
            }
        }
        // PMAT-498: scalar numeric builtins → receiver-method form.
        Expr::NumBuiltin { op, args } => {
            out.push('(');
            emit_expr(out, &args[0], mode)?;
            out.push(')');
            match op {
                NumBuiltinOp::Abs => out.push_str(".abs()"),
                NumBuiltinOp::Min | NumBuiltinOp::Max => {
                    // PMAT-502cz: variadic — chain `.min`/`.max` over every
                    // remaining arg (`max(a, b, c)` → `(a).max(b).max(c)`).
                    let method = if matches!(op, NumBuiltinOp::Min) {
                        ".min("
                    } else {
                        ".max("
                    };
                    for arg in &args[1..] {
                        out.push_str(method);
                        emit_expr(out, arg, mode)?;
                        out.push(')');
                    }
                }
            }
        }
        // PMAT-498b: `sum(xs)` → `<list>.iter().sum::<T>()`.
        Expr::Sum {
            list,
            of_float,
            start,
        } => {
            // PMAT-502cx: `sum(xs, start)` → `(start) + xs.iter().sum::<T>()`
            // (Python's `sum(xs, start) == start + sum(xs)`; start matches
            // the element type so no cast is needed).
            if let Some(start) = start {
                out.push('(');
                emit_expr(out, start, mode)?;
                out.push_str(") + ");
            }
            emit_expr(out, list, mode)?;
            out.push_str(if *of_float {
                ".iter().sum::<f64>()"
            } else {
                ".iter().sum::<i64>()"
            });
        }
        // PMAT-502j: `all(xs)`/`any(xs)` over a bool list.
        Expr::BoolReduce { list, is_all } => {
            emit_expr(out, list, mode)?;
            out.push_str(if *is_all {
                ".iter().all(|&__b| __b)"
            } else {
                ".iter().any(|&__b| __b)"
            });
        }
        // PMAT-502k: `seq * n` → `(seq).repeat(((n).max(0)) as usize)`
        // (str → String, slice → Vec; negative count clamps to empty).
        Expr::Repeat { seq, n } => {
            out.push('(');
            emit_expr(out, seq, mode)?;
            out.push_str(").repeat(((");
            emit_expr(out, n, mode)?;
            out.push_str(").max(0)) as usize)");
        }
        // PMAT-502m: `int(x)`/`float(x)` → `((x) as i64)` / `((x) as f64)`.
        Expr::NumCast {
            value,
            to_float,
            from_str,
        } => {
            if *from_str {
                // PMAT-502bf: `int(s)`/`float(s)` → trimmed `.parse()`
                // (panics on bad input, matching Python's `ValueError`).
                out.push('(');
                emit_expr(out, value, mode)?;
                out.push_str(if *to_float {
                    ").trim().parse::<f64>().expect(\"xpile: ValueError: could not convert string to float\")"
                } else {
                    ").trim().parse::<i64>().expect(\"xpile: ValueError: invalid literal for int()\")"
                });
            } else {
                out.push_str("((");
                emit_expr(out, value, mode)?;
                out.push_str(if *to_float { ") as f64)" } else { ") as i64)" });
            }
        }
        // PMAT-502ad/af: `str(x)` → `format!("{}", x)` (int) or a
        // Python-matching format block (float: `nan` + `".0"` whole-number suffix).
        Expr::ToStr { value, of_float } => {
            if *of_float {
                out.push_str("{ let __sf = ");
                emit_expr(out, value, mode)?;
                out.push_str("; if __sf.is_nan() { String::from(\"nan\") } else if __sf.is_finite() && __sf.fract() == 0.0 { format!(\"{}.0\", __sf) } else { format!(\"{}\", __sf) } }");
            } else {
                out.push_str("format!(\"{}\", ");
                emit_expr(out, value, mode)?;
                out.push(')');
            }
        }
        // PMAT-502ak: `round(x)` (float) → `((x).round_ties_even() as i64)`
        // — banker's rounding, matching Python's `round`.
        Expr::RoundToInt { value } => {
            out.push_str("((");
            emit_expr(out, value, mode)?;
            out.push_str(").round_ties_even() as i64)");
        }
        // PMAT-502al: `round(x, n)` (float) → Python's decimal rounding. For
        // n >= 0, format to n decimals (Rust's `{:.}` is round-half-to-even,
        // matching Python) and parse back; for n < 0, scale + round_ties_even.
        Expr::RoundToDigits { value, ndigits } => {
            out.push_str("{ let __rx = ");
            emit_expr(out, value, mode)?;
            out.push_str("; let __rn = ");
            emit_expr(out, ndigits, mode)?;
            out.push_str("; if __rn >= 0 { format!(\"{:.1$}\", __rx, __rn as usize).parse::<f64>().unwrap() } else { let __rp = 10f64.powi((-__rn) as i32); (__rx / __rp).round_ties_even() * __rp } }");
        }
        // PMAT-502e: 1-arg `min(xs)`/`max(xs)` reduction over an int list.
        // PMAT-502h: `list[float]` uses a fold (f64 has no `Ord`).
        // PMAT-502aa: `key=lambda p: e` → `min_by_key`/`max_by_key`.
        Expr::ListMinMax {
            list,
            is_max,
            of_float,
            key,
            default,
        } => {
            // PMAT-502dh: a `default` makes the empty case return it (via
            // `.unwrap_or(<default>)`) instead of panicking; the float branch
            // switches from the ±∞ fold to a `.reduce(..).unwrap_or(<default>)`.
            emit_expr(out, list, mode)?;
            match key {
                Some(k) => {
                    write!(
                        out,
                        ".iter().cloned().{}(|__k| {{ let {} = __k.clone(); ",
                        if *is_max { "max_by_key" } else { "min_by_key" },
                        k.param
                    )?;
                    emit_expr(out, &k.body, mode)?;
                    out.push_str(" })");
                }
                None => match (*of_float, default.is_some()) {
                    // i64: Ord → `.min()/.max()` returns Option.
                    (false, _) => out.push_str(if *is_max {
                        ".iter().copied().max()"
                    } else {
                        ".iter().copied().min()"
                    }),
                    // f64 with a default → `.reduce(..)` (Option) + unwrap_or.
                    (true, true) => out.push_str(if *is_max {
                        ".iter().copied().reduce(f64::max)"
                    } else {
                        ".iter().copied().reduce(f64::min)"
                    }),
                    // f64, no default → the ±∞ fold (empty → ±∞, first-cut wart).
                    (true, false) => out.push_str(if *is_max {
                        ".iter().copied().fold(f64::NEG_INFINITY, f64::max)"
                    } else {
                        ".iter().copied().fold(f64::INFINITY, f64::min)"
                    }),
                },
            }
            // The float-no-default fold already produced a bare `f64`; every
            // other branch produced an `Option`, which needs unwrapping.
            if !(*of_float && default.is_none()) {
                match default {
                    Some(d) => {
                        out.push_str(".unwrap_or(");
                        emit_expr(out, d, mode)?;
                        out.push(')');
                    }
                    None => out.push_str(".unwrap()"),
                }
            }
        }
        // PMAT-502u: list query — `xs.count(x)` / `xs.index(x)` (→ i64).
        Expr::ListQuery { list, op, arg } => {
            emit_expr(out, list, mode)?;
            match op {
                ListQueryOp::Count => {
                    out.push_str(".iter().filter(|&&__e| __e == ");
                    emit_expr(out, arg, mode)?;
                    out.push_str(").count() as i64");
                }
                ListQueryOp::Index => {
                    out.push_str(".iter().position(|&__e| __e == ");
                    emit_expr(out, arg, mode)?;
                    out.push_str(").map(|__i| __i as i64).expect(\"xpile: ValueError: list.index(x): x not in list\")");
                }
            }
        }
        // PMAT-502as: `xs.pop()` → `(<list>).pop().unwrap()` (last; panics
        // if empty, matching Python IndexError); `xs.pop(i)` →
        // `(<list>).remove((<i>) as usize)` (panics if out of range).
        Expr::ListPop { list, index } => {
            out.push('(');
            emit_expr(out, list, mode)?;
            match index {
                None => out.push_str(").pop().unwrap()"),
                Some(i) => {
                    out.push_str(").remove((");
                    emit_expr(out, i, mode)?;
                    out.push_str(") as usize)");
                }
            }
        }
        // PMAT-502au: `d.pop(k)` → `(<dict>).remove(&(<key>)).unwrap()`
        // (panics if absent, matching Python `KeyError`); `d.pop(k, def)`
        // → `(<dict>).remove(&(<key>)).unwrap_or(<default>)`.
        Expr::DictPop { dict, key, default } => {
            out.push('(');
            emit_expr(out, dict, mode)?;
            out.push_str(").remove(&(");
            emit_expr(out, key, mode)?;
            match default {
                None => out.push_str(")).unwrap()"),
                Some(d) => {
                    out.push_str(")).unwrap_or(");
                    emit_expr(out, d, mode)?;
                    out.push(')');
                }
            }
        }
        // PMAT-502ax: `d.setdefault(k, default)` →
        // `(<dict>).entry(<key>.clone()).or_insert(<default>).clone()`.
        Expr::DictSetDefault { dict, key, default } => {
            out.push('(');
            emit_expr(out, dict, mode)?;
            out.push_str(").entry((");
            emit_expr(out, key, mode)?;
            out.push_str(").clone()).or_insert(");
            emit_expr(out, default, mode)?;
            out.push_str(").clone()");
        }
        // PMAT-502c/f/z: `sorted(xs)` → `{ let mut __xv = <list>.clone();
        // __xv.sort(); __xv }`; `reverse=True` appends `__xv.reverse();`;
        // `key=lambda p: e` uses `__xv.sort_by_key(|__k| { let p = __k.clone(); e })`.
        Expr::Sorted { list, reverse, key } => {
            out.push_str("{ let mut __xv = ");
            emit_expr(out, list, mode)?;
            out.push_str(".clone(); __xv.");
            match key {
                None => out.push_str("sort();"),
                Some(k) => {
                    write!(out, "sort_by_key(|__k| {{ let {} = __k.clone(); ", k.param)?;
                    emit_expr(out, &k.body, mode)?;
                    out.push_str(" });");
                }
            }
            if *reverse {
                out.push_str(" __xv.reverse();");
            }
            out.push_str(" __xv }");
        }
        // PMAT-502d: `reversed(xs)` → a new reversed Vec.
        Expr::Reversed { list } => {
            out.push_str("{ let mut __xv = ");
            emit_expr(out, list, mode)?;
            out.push_str(".clone(); __xv.reverse(); __xv }");
        }
        // PMAT-502cj: `list(range(start, stop, step))` → a collected i64 range.
        Expr::RangeList { start, stop, step } => {
            out.push('(');
            emit_expr(out, start, mode)?;
            out.push_str("..");
            emit_expr(out, stop, mode)?;
            out.push(')');
            if *step != 1 {
                write!(out, ".step_by({step}usize)")?;
            }
            out.push_str(".collect::<Vec<i64>>()");
        }
        // PMAT-502cw: `set(xs)` → collect the list into a HashSet.
        Expr::SetFromList { list } => {
            emit_expr(out, list, mode)?;
            out.push_str(".iter().cloned().collect::<std::collections::HashSet<_>>()");
        }
        // PMAT-502dk: `dict(pairs)` → a HashMap from the list of 2-tuples.
        Expr::DictFromPairs { pairs } => {
            emit_expr(out, pairs, mode)?;
            out.push_str(".iter().cloned().collect::<std::collections::HashMap<_, _>>()");
        }
        // PMAT-502ab: `filter(pred, xs)` → `.iter().cloned().filter(|__k| {
        // let p = __k.clone(); pred }).collect::<Vec<_>>()`.
        Expr::Filter { list, lambda } => {
            emit_expr(out, list, mode)?;
            write!(
                out,
                ".iter().cloned().filter(|__k| {{ let {} = __k.clone(); ",
                lambda.param
            )?;
            emit_expr(out, &lambda.body, mode)?;
            out.push_str(" }).collect::<Vec<_>>()");
        }
        // PMAT-502ac: `map(f, xs)` → `.iter().cloned().map(|__k| { let p =
        // __k.clone(); e }).collect::<Vec<_>>()`.
        Expr::Map { list, lambda } => {
            emit_expr(out, list, mode)?;
            write!(
                out,
                ".iter().cloned().map(|__k| {{ let {} = __k.clone(); ",
                lambda.param
            )?;
            emit_expr(out, &lambda.body, mode)?;
            out.push_str(" }).collect::<Vec<_>>()");
        }
        // PMAT-502ai: `enumerate(xs)` → Vec of (i64, elem) tuples.
        Expr::Enumerate { list } => {
            emit_expr(out, list, mode)?;
            out.push_str(
                ".iter().cloned().enumerate().map(|(__i, __e)| (__i as i64, __e)).collect::<Vec<_>>()",
            );
        }
        // PMAT-502ai: `zip(xs, ys)` → Vec of paired tuples.
        Expr::Zip { left, right } => {
            emit_expr(out, left, mode)?;
            out.push_str(".iter().cloned().zip(");
            emit_expr(out, right, mode)?;
            out.push_str(".iter().cloned()).collect::<Vec<_>>()");
        }
        // PMAT-462 (v0.2.0 Track 1.C): Python dict literal →
        // Rust `{ let mut m = HashMap::new(); m.insert(k, v); ... m }`
        // block expression returning the owned HashMap.
        Expr::DictLit(pairs) => {
            // PMAT-466: the empty literal emits a bare `HashMap::new()`
            // (the surrounding `let`'s annotation supplies K/V). A
            // `{ let mut m = …; m }` block with no inserts would trip
            // clippy's `unused_mut` under `-D warnings`.
            if pairs.is_empty() {
                out.push_str("std::collections::HashMap::new()");
            } else {
                out.push_str("{ let mut m = std::collections::HashMap::new(); ");
                for (k, v) in pairs {
                    out.push_str("m.insert(");
                    emit_expr(out, k, mode)?;
                    out.push_str(", ");
                    emit_expr(out, v, mode)?;
                    out.push_str("); ");
                }
                out.push_str("m }");
            }
        }
        // PMAT-457 (v0.2.0 Track 1.B): Python `xs[i]` → Rust
        // `xs[i as usize].clone()`. The `.clone()` produces an
        // owned value matching the v0.2.0 owned-only ownership
        // posture (we don't yet emit `&xs[i]` borrowed refs). `i64`
        // indices coerce to `usize` via `as`; negative indices
        // would underflow and panic — that's the v0.2.0 first-cut
        // semantics (Python's negative-index wrap is a v0.3.0+
        // sub-track).
        Expr::Index { collection, index } => {
            emit_expr(out, collection, mode)?;
            out.push('[');
            emit_expr(out, index, mode)?;
            out.push_str(" as usize].clone()");
        }
        // PMAT-466 (v0.2.0 Track 1.C): Python `d[k]` → Rust
        // `d[&(k)].clone()`. HashMap's `Index` panics on an absent key
        // (matches Python `KeyError`); `.clone()` yields an owned value
        // (the v0.2.0 owned-only posture); `&(k)` borrows the key for
        // the `Index<&Q>` impl.
        Expr::DictGet { dict, key } => {
            emit_expr(out, dict, mode)?;
            out.push_str("[&(");
            emit_expr(out, key, mode)?;
            out.push_str(")].clone()");
        }
        // PMAT-466: Python `d.get(k, default)` → Rust
        // `d.get(&(k)).cloned().unwrap_or(default)`. Total: never
        // panics; returns `default` for an absent key.
        Expr::DictGetOr { dict, key, default } => {
            emit_expr(out, dict, mode)?;
            out.push_str(".get(&(");
            emit_expr(out, key, mode)?;
            out.push_str(")).cloned().unwrap_or(");
            emit_expr(out, default, mode)?;
            out.push(')');
        }
        // PMAT-466: Python `k in d` → Rust `d.contains_key(&(k))`.
        Expr::DictContains { dict, key } => {
            emit_expr(out, dict, mode)?;
            out.push_str(".contains_key(&(");
            emit_expr(out, key, mode)?;
            out.push_str("))");
        }
        // PMAT-502v/502x: `d.keys()`/`d.values()`/`d.items()` → materialized Vec.
        Expr::DictView { dict, kind } => {
            emit_expr(out, dict, mode)?;
            out.push_str(match kind {
                DictViewKind::Keys => ".keys().cloned().collect::<Vec<_>>()",
                DictViewKind::Values => ".values().cloned().collect::<Vec<_>>()",
                DictViewKind::Items => {
                    ".iter().map(|(__k, __v)| (__k.clone(), __v.clone())).collect::<Vec<_>>()"
                }
            });
        }
        // PMAT-500: Python set literal `{a, b, c}` → HashSet-init block.
        // PMAT-501b: an empty SetLit (the set-comprehension accumulator)
        // emits a bare `HashSet::new()` (the let annotation supplies T) —
        // a `{ … }` block with no inserts would trip clippy's unused_mut.
        Expr::SetLit(elems) => {
            if elems.is_empty() {
                out.push_str("std::collections::HashSet::new()");
            } else {
                out.push_str("{ let mut __xset = std::collections::HashSet::new(); ");
                for e in elems {
                    out.push_str("__xset.insert(");
                    emit_expr(out, e, mode)?;
                    out.push_str("); ");
                }
                out.push_str("__xset }");
            }
        }
        // PMAT-500: Python `x in s` → `<set>.contains(&(<elem>))`.
        Expr::SetContains { set, elem } => {
            emit_expr(out, set, mode)?;
            out.push_str(".contains(&(");
            emit_expr(out, elem, mode)?;
            out.push_str("))");
        }
        // PMAT-502an: Python `x in xs` (list) → `(xs).contains(&(x))`.
        Expr::ListContains { list, elem } => {
            out.push('(');
            emit_expr(out, list, mode)?;
            out.push_str(").contains(&(");
            emit_expr(out, elem, mode)?;
            out.push_str("))");
        }
        // PMAT-502o: Python `sub in s` (str) → `(s).contains(&(sub)[..])`.
        Expr::StrContains { haystack, needle } => {
            out.push('(');
            emit_expr(out, haystack, mode)?;
            out.push_str(").contains(&(");
            emit_expr(out, needle, mode)?;
            out.push_str(")[..])");
        }
        // PMAT-502g: set algebra → `(lhs).<method>(&(rhs)).cloned().collect()`
        // into a fresh `HashSet`.
        Expr::SetOp { lhs, op, rhs } => {
            let method = match op {
                SetOp::Union => "union",
                SetOp::Intersection => "intersection",
                SetOp::Difference => "difference",
                SetOp::SymmetricDifference => "symmetric_difference",
            };
            out.push('(');
            emit_expr(out, lhs, mode)?;
            out.push_str(").");
            out.push_str(method);
            out.push_str("(&(");
            emit_expr(out, rhs, mode)?;
            out.push_str(")).cloned().collect::<std::collections::HashSet<_>>()");
        }
        // PMAT-459 (v0.2.0 Track 1.B): Python `len(x)` → Rust
        // `x.len() as i64`. Vec/String both expose `.len()` returning
        // `usize`; the `as i64` cast brings the result back into
        // Python's signed-int domain.
        Expr::Len(inner) => {
            emit_expr(out, inner, mode)?;
            out.push_str(".len() as i64");
        }
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => emit_if_expr(out, cond, then_expr, else_expr, mode)?,
        Expr::Call { callee, args } => emit_call(out, callee, args, mode)?,
        Expr::UnOp { op, operand } => emit_unop(out, *op, operand, mode)?,
        // PMAT-449 (v0.2.0 Track 1.A): Python `str` literals lower
        // to owned `String::from("...")`. The character set is
        // escape-aware (`"` and `\` → `\"` / `\\`); v0.2.0 starts
        // with the minimal escape set, expanded in later sub-tracks.
        Expr::LitStr(s) => {
            write!(out, "String::from(\"{}\")", escape_rust_str(s))?;
        }
        // PMAT-042: `QuotedString` carries an explicit shell-domain
        // quoting strategy (bareword vs single-quote vs double-quote);
        // its semantics are bashrs-only. Rust backend refuses.
        Expr::QuotedString { .. } => {
            return Err(CodegenError::Unsupported(
                "Rust backend does not lower Expr::QuotedString — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs quoted shell strings; \
                 use `--target shell`"
                    .into(),
            ));
        }
        // PMAT-045: shell-variable references — same disposition.
        Expr::ShellVar(name) => {
            return Err(CodegenError::Unsupported(format!(
                "Rust backend does not lower Expr::ShellVar (${name}) — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs shell variable references; \
                 use `--target shell`"
            )));
        }
        // PMAT-047: command substitution — same disposition.
        Expr::CommandSubstitution(_) => {
            return Err(CodegenError::Unsupported(
                "Rust backend does not lower Expr::CommandSubstitution — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs shell substitution; \
                 use `--target shell`"
                    .into(),
            ));
        }
        // PMAT-055: shell special parameters — same disposition.
        Expr::ShellSpecial(name) => {
            return Err(CodegenError::Unsupported(format!(
                "Rust backend does not lower Expr::ShellSpecial (${name}) — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs shell special params; \
                 use `--target shell`"
            )));
        }
    }
    Ok(())
}

fn emit_unop(out: &mut String, op: UnOp, operand: &Expr, mode: bool) -> Result<(), CodegenError> {
    match op {
        UnOp::Neg => {
            if mode {
                // BigInt::neg returns BigInt without overflow risk.
                // PMAT-012 — slow-path side of C-PY-INT-ARITH.
                write!(out, "(-")?;
                emit_expr(out, operand, mode)?;
                write!(out, ")")?;
            } else {
                // Python: `-x` on int never overflows mathematically (int is unbounded).
                // Rust: `i64::MIN.checked_neg() == None`. Use checked_neg + panic that
                // points at the unimplemented bigint promotion slow path of
                // contract C-PY-INT-ARITH. See py-int-arith-v1.yaml.
                write!(out, "(")?;
                emit_expr(out, operand, mode)?;
                write!(
                    out,
                    ").checked_neg().expect(\"xpile: i64 negation overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\")"
                )?;
            }
        }
        UnOp::Not => {
            write!(out, "(!")?;
            emit_expr(out, operand, mode)?;
            write!(out, ")")?;
        }
    }
    Ok(())
}

fn emit_call(
    out: &mut String,
    callee: &str,
    args: &[Expr],
    mode: bool,
) -> Result<(), CodegenError> {
    write!(out, "{}(", callee)?;
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            write!(out, ", ")?;
        }
        emit_expr(out, a, mode)?;
    }
    write!(out, ")")?;
    Ok(())
}

/// Rust `if cond { then } else { else_ }` — usable as an expression.
/// When the `else_` is itself another `IfExpr`, emit a flat
/// `else if ...` form (no extra braces) for readability.
fn emit_if_expr(
    out: &mut String,
    cond: &Expr,
    then_expr: &Expr,
    else_expr: &Expr,
    mode: bool,
) -> Result<(), CodegenError> {
    write!(out, "if ")?;
    emit_expr(out, cond, mode)?;
    write!(out, " {{ ")?;
    emit_expr(out, then_expr, mode)?;
    write!(out, " }} else ")?;
    match else_expr {
        Expr::IfExpr {
            cond: c2,
            then_expr: t2,
            else_expr: e2,
        } => {
            emit_if_expr(out, c2, t2, e2, mode)?;
            return Ok(());
        }
        _ => {
            write!(out, "{{ ")?;
            emit_expr(out, else_expr, mode)?;
            write!(out, " }}")?;
        }
    }
    Ok(())
}

/// Emit a binary op.
///
/// Arithmetic (`+`, `-`, `*`, `//`, `%`) uses `checked_*` variants with
/// `.expect("…")` rather than wrapping/truncating. Python `int` is
/// mathematically unbounded — silently wrapping at i64 would violate
/// the Layer-1 contract `C-PY-INT-ARITH`. Until the contract's bigint
/// slow path is implemented, overflow panics with a message pointing
/// at the contract.
///
/// FloorDiv / Mod additionally preserve Python-floor semantics via
/// `checked_div_euclid` / `checked_rem_euclid` (plain `/` and `%` in
/// Rust truncate toward zero, which diverges from Python on negative
/// operands).
///
/// Comparisons and logical ops never overflow, so they remain infix.
fn emit_binop(
    out: &mut String,
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), CodegenError> {
    match op {
        // Arithmetic: in BigInt mode all of these are plain infix
        // (BigInt overloads `+ - * <= ...` via num-bigint) — no
        // overflow risk, so no `.checked_*().expect(...)`. The C-PY-INT-ARITH
        // slow path is satisfied directly. PMAT-012.
        BinOp::Add if mode => emit_infix(out, lhs, " + ", rhs, mode),
        BinOp::Sub if mode => emit_infix(out, lhs, " - ", rhs, mode),
        BinOp::Mul if mode => emit_infix(out, lhs, " * ", rhs, mode),
        BinOp::FloorDiv if mode => emit_bigint_floor_call(out, "div_floor", lhs, rhs, mode),
        BinOp::Mod if mode => emit_bigint_floor_call(out, "mod_floor", lhs, rhs, mode),
        // PMAT-026 / PMAT-013-FOLLOWUP: bitwise + shift + power on
        // BigInt. num-bigint's `BitAnd / BitOr / BitXor` are direct
        // infix operators on `BigInt`; `<< >>` and `**` take rhs as
        // `usize` / `u32` (not BigInt), so we route through helpers
        // in `xpile_bigint::{shl, shr, pow}` that handle the
        // BigInt → primitive conversion (with a contract-named panic
        // on out-of-range exponents — same posture as the i64 fast
        // path's shift / pow handling).
        BinOp::BitAnd if mode => emit_infix(out, lhs, " & ", rhs, mode),
        BinOp::BitOr if mode => emit_infix(out, lhs, " | ", rhs, mode),
        BinOp::BitXor if mode => emit_infix(out, lhs, " ^ ", rhs, mode),
        BinOp::Shl if mode => emit_bigint_floor_call(out, "shl", lhs, rhs, mode),
        BinOp::Shr if mode => emit_bigint_floor_call(out, "shr", lhs, rhs, mode),
        BinOp::Pow if mode => emit_bigint_floor_call(out, "pow", lhs, rhs, mode),
        BinOp::Add => emit_checked(out, lhs, "checked_add", rhs, "addition", mode),
        BinOp::Sub => emit_checked(out, lhs, "checked_sub", rhs, "subtraction", mode),
        BinOp::Mul => emit_checked(out, lhs, "checked_mul", rhs, "multiplication", mode),
        BinOp::FloorDiv => emit_checked(out, lhs, "checked_div_euclid", rhs, "floor-div", mode),
        BinOp::Mod => emit_checked(out, lhs, "checked_rem_euclid", rhs, "modulo", mode),
        BinOp::Eq => emit_infix(out, lhs, " == ", rhs, mode),
        BinOp::NotEq => emit_infix(out, lhs, " != ", rhs, mode),
        BinOp::Lt => emit_infix(out, lhs, " < ", rhs, mode),
        BinOp::LtEq => emit_infix(out, lhs, " <= ", rhs, mode),
        BinOp::Gt => emit_infix(out, lhs, " > ", rhs, mode),
        BinOp::GtEq => emit_infix(out, lhs, " >= ", rhs, mode),
        BinOp::And => emit_infix(out, lhs, " && ", rhs, mode),
        BinOp::Or => emit_infix(out, lhs, " || ", rhs, mode),
        BinOp::BitAnd => emit_infix(out, lhs, " & ", rhs, mode),
        BinOp::BitOr => emit_infix(out, lhs, " | ", rhs, mode),
        BinOp::BitXor => emit_infix(out, lhs, " ^ ", rhs, mode),
        BinOp::Shl => emit_checked_shift(out, lhs, "checked_shl", rhs, "left-shift", mode),
        BinOp::Shr => emit_checked_shift(out, lhs, "checked_shr", rhs, "right-shift", mode),
        BinOp::Pow => emit_checked_pow(out, lhs, rhs, mode),
    }
}

/// BigInt-mode floor-div / mod via the helpers exposed in xpile-bigint.
/// Takes references because num-bigint's `Integer::div_floor` consumes
/// `self`; the wrappers borrow. PMAT-012.
fn emit_bigint_floor_call(
    out: &mut String,
    method: &str,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), CodegenError> {
    write!(out, "xpile_bigint::{method}(&")?;
    emit_expr(out, lhs, mode)?;
    write!(out, ", &")?;
    emit_expr(out, rhs, mode)?;
    write!(out, ")")?;
    Ok(())
}

/// Emit `(lhs).checked_pow(u32::try_from(rhs).expect(...)).expect(...)`.
/// Same panic-naming pattern as shifts: the inner expect fires on a
/// negative exponent (Python would return Float, which v0.1.0's type
/// system has no I64-compatible representation for); the outer expect
/// fires on i64 overflow.
fn emit_checked_pow(
    out: &mut String,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), CodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs, mode)?;
    write!(out, ").checked_pow(u32::try_from(")?;
    emit_expr(out, rhs, mode)?;
    write!(
        out,
        ").expect(\"xpile: exponent out of range for u32 — Python returns Float for negative exponents which v0.1.0 cannot represent (contract C-PY-INT-ARITH)\")).expect(\"xpile: i64 power overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\")"
    )?;
    Ok(())
}

/// Emit a shift: `(lhs).checked_sh*(u32::try_from(rhs).expect(...)).expect(...)`.
/// Both panics name `C-PY-INT-ARITH` so the trail is still legible — the
/// inner one fires when Python's "shift by negative or huge" raises in
/// CPython; the outer one fires when the shift amount is >= 64 on i64.
fn emit_checked_shift(
    out: &mut String,
    lhs: &Expr,
    method: &str,
    rhs: &Expr,
    op_name: &str,
    mode: bool,
) -> Result<(), CodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs, mode)?;
    write!(out, ").{method}(u32::try_from(")?;
    emit_expr(out, rhs, mode)?;
    write!(
        out,
        ").expect(\"xpile: shift amount out of range for u32 (contract C-PY-INT-ARITH)\")).expect(\"xpile: i64 {op_name} overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\")"
    )?;
    Ok(())
}

/// Emit a checked binary op: `(<lhs>).<method>(<rhs>).expect("<msg> overflow ...")`.
/// Returns `i64`, identical to infix on the no-overflow fast path.
fn emit_checked(
    out: &mut String,
    lhs: &Expr,
    method: &str,
    rhs: &Expr,
    op_name: &str,
    mode: bool,
) -> Result<(), CodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs, mode)?;
    write!(out, ").{method}(")?;
    emit_expr(out, rhs, mode)?;
    write!(
        out,
        ").expect(\"xpile: i64 {op_name} overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\")"
    )?;
    Ok(())
}

fn emit_infix(
    out: &mut String,
    lhs: &Expr,
    op: &str,
    rhs: &Expr,
    mode: bool,
) -> Result<(), CodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs, mode)?;
    out.push_str(op);
    emit_expr(out, rhs, mode)?;
    write!(out, ")")?;
    Ok(())
}

pub struct RustBackend;

impl Backend for RustBackend {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Rust]
    }

    fn lower(&self, module: &Module, _config: &BackendConfig) -> Result<Artifact, BackendError> {
        let primary = emit_module(module).map_err(|e| BackendError::Lower(e.to_string()))?;
        Ok(Artifact {
            primary,
            sidecars: Vec::new(),
            citations: Vec::new(),
            quorum_status: QuorumStatus::Single {
                emitter: "xpile-rust-codegen".to_string(),
            },
        })
    }
}

// ── C emit path (PMAT-467, v0.2.0 Track 2.A) ────────────────────────
//
// Isolated from the Python/Ruchy emit above so C's semantics can't
// regress it. C `int` is fixed-width `i32`; signed overflow is UB, for
// which `wrapping_*` is the sound conservative discharge (it produces a
// deterministic two's-complement result rather than invoking Rust UB).
// This mirrors the standalone-decy → C-C-INT-ARITH plan in
// `sub/v0.2.0-decy-merger.md`; the contract substrate is queued.

fn emit_c_function(out: &mut String, f: &Function) -> Result<(), CodegenError> {
    // Forward-reference citation (substrate queued, same posture as the
    // dict lane citing C-XLATE-PY-DICT-TO-HASHMAP before it existed).
    writeln!(out, "// xpile-contract: C-C-INT-ARITH")?;
    write!(out, "pub fn {}(", f.name)?;
    for (i, p) in f.params.iter().enumerate() {
        if i > 0 {
            write!(out, ", ")?;
        }
        write!(out, "{}: i32", p.name)?;
    }
    writeln!(out, ") -> i32 {{")?;
    for stmt in &f.body.stmts {
        emit_c_stmt(out, stmt, "    ")?;
    }
    write!(out, "    ")?;
    emit_c_expr(out, &f.body.trailing_return)?;
    writeln!(out)?;
    writeln!(out, "}}")?;
    Ok(())
}

fn emit_c_stmt(out: &mut String, stmt: &Stmt, indent: &str) -> Result<(), CodegenError> {
    match stmt {
        Stmt::Let {
            name,
            value,
            mutable,
            ..
        } => {
            let kw = if *mutable { "let mut" } else { "let" };
            write!(out, "{indent}{kw} {name}: i32 = ")?;
            emit_c_expr(out, value)?;
            writeln!(out, ";")?;
            Ok(())
        }
        Stmt::Assign { name, value } => {
            write!(out, "{indent}{name} = ")?;
            emit_c_expr(out, value)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-479 (R10): C early `return <expr>;` (guard clause).
        Stmt::Return(e) => {
            write!(out, "{indent}return ")?;
            emit_c_expr(out, e)?;
            writeln!(out, ";")?;
            Ok(())
        }
        Stmt::While { cond, body } => {
            write!(out, "{indent}while ")?;
            emit_c_expr(out, cond)?;
            writeln!(out, " {{")?;
            let inner = format!("{indent}    ");
            for s in body {
                emit_c_stmt(out, s, &inner)?;
            }
            writeln!(out, "{indent}}}")?;
            Ok(())
        }
        // PMAT-478 (R9): C `if (c) { … } else { … }` → Rust if/else
        // statement (the `else` block omitted when empty).
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            write!(out, "{indent}if ")?;
            emit_c_expr(out, cond)?;
            writeln!(out, " {{")?;
            let inner = format!("{indent}    ");
            for s in then_body {
                emit_c_stmt(out, s, &inner)?;
            }
            if else_body.is_empty() {
                writeln!(out, "{indent}}}")?;
            } else {
                writeln!(out, "{indent}}} else {{")?;
                for s in else_body {
                    emit_c_stmt(out, s, &inner)?;
                }
                writeln!(out, "{indent}}}")?;
            }
            Ok(())
        }
        other => Err(CodegenError::Unsupported(format!(
            "C backend supports `int x = e;`, `x = e;`, `if (c) {{ … }} else {{ … }}`, and `while (c) {{ … }}`, got {other:?}"
        ))),
    }
}

fn emit_c_expr(out: &mut String, e: &Expr) -> Result<(), CodegenError> {
    match e {
        Expr::LitInt(v) => write!(out, "{v}i32")?,
        Expr::Ident(name) => write!(out, "{name}")?,
        Expr::BinOp { op, lhs, rhs } => emit_c_binop(out, *op, lhs, rhs)?,
        Expr::UnOp { op, operand } => match op {
            // C unary minus on `int` is wrapping (INT_MIN negation is UB
            // in C; `wrapping_neg` is the sound deterministic discharge).
            UnOp::Neg => {
                write!(out, "(")?;
                emit_c_expr(out, operand)?;
                write!(out, ").wrapping_neg()")?;
            }
            UnOp::Not => {
                write!(out, "!(")?;
                emit_c_expr(out, operand)?;
                write!(out, ")")?;
            }
        },
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            write!(out, "if ")?;
            emit_c_expr(out, cond)?;
            write!(out, " {{ ")?;
            emit_c_expr(out, then_expr)?;
            write!(out, " }} else {{ ")?;
            emit_c_expr(out, else_expr)?;
            write!(out, " }}")?;
        }
        Expr::Call { callee, args } => {
            write!(out, "{callee}(")?;
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    write!(out, ", ")?;
                }
                emit_c_expr(out, a)?;
            }
            write!(out, ")")?;
        }
        other => {
            return Err(CodegenError::Unsupported(format!(
                "C backend slice 1 does not lower {other:?} — supported: int literals, \
                 identifiers, calls, + - *, comparisons, && ||, unary - !, and the ternary"
            )));
        }
    }
    Ok(())
}

fn emit_c_binop(out: &mut String, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<(), CodegenError> {
    // Arithmetic: wrapping (C signed overflow is UB → deterministic
    // two's-complement). Comparisons / logicals: plain infix, producing
    // a Rust `bool` (correct for `if`/`&&`/`||` operand positions, which
    // is where the C frontend places them).
    let wrapping = |out: &mut String, method: &str| -> Result<(), CodegenError> {
        write!(out, "(")?;
        emit_c_expr(out, lhs)?;
        write!(out, ").{method}(")?;
        emit_c_expr(out, rhs)?;
        write!(out, ")")?;
        Ok(())
    };
    let infix = |out: &mut String, sym: &str| -> Result<(), CodegenError> {
        emit_c_expr(out, lhs)?;
        write!(out, " {sym} ")?;
        emit_c_expr(out, rhs)?;
        Ok(())
    };
    match op {
        BinOp::Add => wrapping(out, "wrapping_add")?,
        BinOp::Sub => wrapping(out, "wrapping_sub")?,
        BinOp::Mul => wrapping(out, "wrapping_mul")?,
        // C `/` truncates toward zero (Rust integer `/` does too);
        // `wrapping_div`/`wrapping_rem` add the INT_MIN/-1 UB guard.
        // The frontend carries these as FloorDiv/Mod (shared IR
        // variants); here they mean C truncating div/rem, not Python
        // floor.
        BinOp::FloorDiv => wrapping(out, "wrapping_div")?,
        BinOp::Mod => wrapping(out, "wrapping_rem")?,
        BinOp::Eq => infix(out, "==")?,
        BinOp::NotEq => infix(out, "!=")?,
        BinOp::Lt => infix(out, "<")?,
        BinOp::LtEq => infix(out, "<=")?,
        BinOp::Gt => infix(out, ">")?,
        BinOp::GtEq => infix(out, ">=")?,
        BinOp::And => infix(out, "&&")?,
        BinOp::Or => infix(out, "||")?,
        other => {
            return Err(CodegenError::Unsupported(format!(
                "C backend slice 1 does not lower BinOp::{other:?} — `/`, `%`, bitwise, \
                 shift, and power are deferred to a later decy slice"
            )));
        }
    }
    Ok(())
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
            body: Block {
                stmts: vec![],
                trailing_return: Expr::BinOp {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::Ident("a".into())),
                    rhs: Box::new(Expr::Ident("b".into())),
                },
            },
        }
    }

    #[test]
    fn emits_add_function() {
        let m = module_with("fixture", vec![Item::Function(add_fn())]);
        let rust = emit_module(&m).expect("emit ok");
        assert!(rust.contains("pub fn add(a: i64, b: i64) -> i64"));
        // After contract C-PY-INT-ARITH was wired in (PMAT-002),
        // addition emits `(a).checked_add(b).expect(...)`, not plain
        // `(a + b)`. Assert on the load-bearing invariants rather
        // than the exact shape.
        assert!(rust.contains("checked_add"), "expected checked_add: {rust}");
        assert!(
            rust.contains("C-PY-INT-ARITH"),
            "expected contract reference in panic msg: {rust}"
        );
    }

    #[test]
    fn emits_floordiv_as_div_euclid() {
        // Python `a // b` must NOT lower to Rust `/`.
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
            body: Block {
                stmts: vec![],
                trailing_return: Expr::BinOp {
                    op: BinOp::FloorDiv,
                    lhs: Box::new(Expr::Ident("a".into())),
                    rhs: Box::new(Expr::Ident("b".into())),
                },
            },
        };
        let m = module_with("fixture", vec![Item::Function(f)]);
        let rust = emit_module(&m).expect("emit ok");
        assert!(
            rust.contains("div_euclid"),
            "Python floor-div must lower to div_euclid (got: {})",
            rust
        );
        assert!(
            !rust.contains(" / "),
            "must not use plain Rust `/` for Python `//`"
        );
    }

    #[test]
    fn emits_comparison_returning_bool() {
        let f = Function {
            name: "le".into(),
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
            return_type: Type::Bool,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::BinOp {
                    op: BinOp::LtEq,
                    lhs: Box::new(Expr::Ident("a".into())),
                    rhs: Box::new(Expr::Ident("b".into())),
                },
            },
        };
        let m = module_with("fixture", vec![Item::Function(f)]);
        let rust = emit_module(&m).expect("emit ok");
        assert!(rust.contains("-> bool"));
        assert!(rust.contains("(a <= b)"));
    }

    #[test]
    fn emits_if_expression_for_ternary() {
        let f = Function {
            name: "pick".into(),
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
            body: Block {
                stmts: vec![],
                trailing_return: Expr::IfExpr {
                    cond: Box::new(Expr::BinOp {
                        op: BinOp::LtEq,
                        lhs: Box::new(Expr::Ident("a".into())),
                        rhs: Box::new(Expr::Ident("b".into())),
                    }),
                    then_expr: Box::new(Expr::Ident("a".into())),
                    else_expr: Box::new(Expr::Ident("b".into())),
                },
            },
        };
        let m = module_with("fixture", vec![Item::Function(f)]);
        let rust = emit_module(&m).expect("emit ok");
        assert!(rust.contains("if (a <= b) { a } else { b }"));
        assert!(rust.contains("pub fn pick(a: i64, b: i64) -> i64"));
    }

    #[test]
    fn emit_module_produces_rustc_parseable_output() {
        // Run the emitted source through `syn::parse_file` to ensure
        // syntactic well-formedness without spawning rustc. Doesn't
        // type-check (that's the workspace-test job).
        // (syn isn't a dep here; instead check basic shape.)
        let m = module_with("fixture", vec![Item::Function(add_fn())]);
        let rust = emit_module(&m).expect("emit ok");
        // sanity: balanced braces and trailing newline
        assert_eq!(rust.matches('{').count(), rust.matches('}').count());
        assert!(rust.ends_with('\n'));
    }
}
