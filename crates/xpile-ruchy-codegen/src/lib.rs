//! Ruchy backend.
//!
//! Lowers meta-HIR to Ruchy source. v0.1.0 emits the same arithmetic
//! subset as `xpile-rust-codegen` — the surface difference is
//! `fun ... -> T { ... }` instead of Rust's `pub fn ... -> T { ... }`,
//! and floor-div / modulo still go through Euclidean semantics
//! (`div_euclid` / `rem_euclid`).
//!
//! Future scope (tracked by `Profile::RuchyOut`): reconstruct the
//! pipeline operator `|>` and DataFrame-flavored sugar from meta-HIR
//! patterns. See `docs/specifications/sub/bidirectional-ruchy.md`.

use std::fmt::Write;
use xpile_backend::{Artifact, Backend, BackendConfig, BackendError, QuorumStatus, Target};
use xpile_meta_hir::{
    BinOp, Block, DictViewKind, Expr, FloatOp, Function, Item, ListMutateOp, ListQueryOp, Module,
    NumBuiltinOp, Param, SetOp, Stmt, StrMethodOp, Type, UnOp,
};

/// PMAT-477 (R8): Ruchy → Rust infix symbol for a float arithmetic op.
fn float_op_sym(op: FloatOp) -> &'static str {
    match op {
        FloatOp::Add => "+",
        FloatOp::Sub => "-",
        FloatOp::Mul => "*",
        FloatOp::Div => "/",
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuchyCodegenError {
    #[error("unsupported item: {0}")]
    Unsupported(String),
    #[error("formatting error: {0}")]
    Format(#[from] std::fmt::Error),
}

pub fn emit_module(module: &Module) -> Result<String, RuchyCodegenError> {
    let mut out = String::new();
    writeln!(
        out,
        "// xpile-generated from {:?} module {}",
        module.source_lang, module.name
    )?;
    writeln!(out)?;
    for item in &module.items {
        match item {
            Item::Function(f) => emit_function(&mut out, f)?,
        }
    }
    Ok(out)
}

fn emit_function(out: &mut String, f: &Function) -> Result<(), RuchyCodegenError> {
    emit_contract_citations(out, f)?;
    // Ruchy: `fun name(params) -> ret { body }`. No `pub`.
    write!(out, "fun {}(", f.name)?;
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

/// PMAT-012-FOLLOWUP / PMAT-025: a function is in BigInt mode if any
/// param is BigInt, the return type is BigInt, OR any pre-bound Let
/// is BigInt. In BigInt mode, the Ruchy backend emits the same shape
/// as the Rust backend (since Ruchy compiles to Rust):
/// `xpile_bigint::BigInt::from(<n>i64)` literals + plain infix
/// arithmetic + `.clone()` on Ident references (BigInt isn't `Copy`).
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
            // PMAT-479 (R10): early return introduces no BigInt binding.
            // PMAT-494b: tuple unpacking introduces no BigInt binding.
            // PMAT-503a: a raise introduces no BigInt binding.
            Stmt::Assign { .. }
            | Stmt::Assert { .. }
            | Stmt::Return(_)
            | Stmt::LetTuple { .. }
            | Stmt::ClosureLet { .. }
            | Stmt::Raise { .. } => false,
            Stmt::While { body, .. }
            | Stmt::ForEach { body, .. }
            | Stmt::ForEachPair { body, .. } => body.iter().any(stmt_has_bigint),
            // PMAT-478 (R9): recurse both branches of an if/else.
            Stmt::If {
                then_body,
                else_body,
                ..
            } => then_body.iter().any(stmt_has_bigint) || else_body.iter().any(stmt_has_bigint),
            // PMAT-460: list.append() — same disposition. PMAT-502ap/aq/ar:
            // in-place list mutators / extend / insert likewise carry no binding.
            Stmt::ListAppend { .. }
            | Stmt::SetAdd { .. }
            | Stmt::SetRemove { .. }
            | Stmt::ListMutate { .. }
            | Stmt::ListExtend { .. }
            | Stmt::DictUpdate { .. }
            | Stmt::ListInsert { .. } => false,
            // PMAT-461: indexed assignment same disposition.
            Stmt::IndexAssign { .. } => false,
            // PMAT-466: dict keyed assignment same disposition.
            Stmt::DictSet { .. } => false,
            // PMAT-502at: del coll[key] introduces no binding.
            Stmt::DelItem { .. } => false,
            // PMAT-039: see rust-codegen's twin arm — shell commands
            // carry no BigInt operands.
            Stmt::Cmd { .. } => false,
            // PMAT-041: see rust-codegen's twin arm.
            Stmt::Pipeline { .. } => false,
            // PMAT-048: see rust-codegen's twin arm.
            Stmt::ShellLoop { .. } => false,
            // PMAT-051: see rust-codegen's twin arm.
            Stmt::ShellAssign { .. } => false,
        }
    }
    f.body.stmts.iter().any(stmt_has_bigint)
}

/// PMAT-011: same `// xpile-contract: <ID>` form as the Rust backend.
/// Ruchy compiles to Rust, so it shares the comment-citation convention.
fn emit_contract_citations(out: &mut String, f: &Function) -> Result<(), RuchyCodegenError> {
    for id in f.applicable_contracts() {
        writeln!(out, "// xpile-contract: {id}")?;
    }
    Ok(())
}

fn emit_block(out: &mut String, block: &Block, mode: bool) -> Result<(), RuchyCodegenError> {
    for stmt in &block.stmts {
        emit_stmt(out, stmt, mode)?;
    }
    write!(out, "    ")?;
    emit_expr(out, &block.trailing_return, mode)?;
    writeln!(out)?;
    Ok(())
}

fn emit_stmt(out: &mut String, stmt: &Stmt, mode: bool) -> Result<(), RuchyCodegenError> {
    emit_stmt_indented(out, stmt, "    ", mode)
}

fn emit_stmt_indented(
    out: &mut String,
    stmt: &Stmt,
    indent: &str,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
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
        // PMAT-494b: tuple unpacking → `let (a, b, ...) = <value>;`.
        Stmt::LetTuple { names, value } => {
            write!(out, "{indent}let ({}) = ", names.join(", "))?;
            emit_expr(out, value, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-479 (R10): early `return <expr>;` (guard clause).
        Stmt::Return(e) => {
            write!(out, "{indent}return ")?;
            emit_expr(out, e, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-478 (R9): if/else statement → `if c { … } else { … }`.
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
        Stmt::Assign { name, value } => {
            write!(out, "{indent}{name} = ")?;
            emit_expr(out, value, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-504: closure binding (0+ params), matching the Rust backend.
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
        // PMAT-458 (v0.2.0 Track 1.B): Ruchy → Rust → for-each with
        // .iter().cloned() for owned-value bindings.
        Stmt::ForEach {
            var,
            iter,
            body,
            over_keys,
            ..
        } => {
            // PMAT-472 (R3): dict iterates keys via `.keys().cloned()`.
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
        // PMAT-495: paired for-loop (enumerate / zip), Ruchy → Rust.
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
                xpile_meta_hir::PairIterKind::Enumerate => {
                    out.push_str(
                        ".iter().cloned().enumerate().map(|(__i, __e)| (__i as i64, __e))",
                    );
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
        // PMAT-460 (v0.2.0 Track 1.B): Ruchy → Rust → `.push(...)`.
        Stmt::ListAppend { list_name, elem } => {
            write!(out, "{indent}{list_name}.push(")?;
            emit_expr(out, elem, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-500b: Ruchy → Rust `s.insert(x);`.
        Stmt::SetAdd { set_name, elem } => {
            write!(out, "{indent}{set_name}.insert(")?;
            emit_expr(out, elem, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-502av: `s.remove(x)` / `s.discard(x)`, matching the Rust backend.
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
        // PMAT-502ap: in-place list mutators, matching the Rust backend.
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
        // PMAT-502aq: `xs.extend(ys)`, matching the Rust backend.
        Stmt::ListExtend { list_name, other } => {
            write!(out, "{indent}{list_name}.extend((")?;
            emit_expr(out, other, mode)?;
            writeln!(out, ").iter().cloned());")?;
            Ok(())
        }
        // PMAT-502bb: `d.update(other)`, matching the Rust backend.
        Stmt::DictUpdate { dict_name, other } => {
            write!(out, "{indent}{dict_name}.extend((")?;
            emit_expr(out, other, mode)?;
            writeln!(
                out,
                ").iter().map(|(__k, __v)| (__k.clone(), __v.clone())));"
            )?;
            Ok(())
        }
        // PMAT-502ar: `xs.insert(i, x)`, matching the Rust backend.
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
        // PMAT-461 (v0.2.0 Track 1.B): Ruchy → Rust →
        // `xs[i as usize] = v;`, matching the Rust backend.
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
        // PMAT-466 (v0.2.0 Track 1.C): Ruchy → Rust
        // `{ let __v = v; d.insert(k.clone(), __v); }`, matching the Rust
        // backend — value bound to a temp before insert, and the key
        // cloned so a non-Copy str key survives a later read (see the
        // Rust twin arm for the full move-then-borrow rationale).
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
        // PMAT-502at: `del coll[key]`, matching the Rust backend.
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
        // PMAT-503a: `raise Exc("msg")` → `panic!("{}", <message>);` (Ruchy
        // compiles to Rust and inherits the diverging-panic disposition).
        Stmt::Raise { message } => {
            write!(out, "{indent}panic!(\"{{}}\", ")?;
            emit_expr(out, message, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-039 / XPILE-BASHRS-MERGER-001 Layer B: see rust-codegen's
        // matching arm. Ruchy compiles to Rust and inherits Rust's
        // disposition — no Ruchy-level translation of `Stmt::Cmd`
        // exists.
        Stmt::Cmd { program, args } => Err(RuchyCodegenError::Unsupported(format!(
            "Ruchy backend does not lower Stmt::Cmd (`{program}` with {} arg(s)) — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs this construct; \
             use `--target shell` to emit POSIX sh via bashrs-backend",
            args.len()
        ))),
        // PMAT-041: same disposition as Cmd.
        Stmt::Pipeline { stages } => Err(RuchyCodegenError::Unsupported(format!(
            "Ruchy backend does not lower Stmt::Pipeline ({} stages) — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell pipelines; \
             use `--target shell`",
            stages.len()
        ))),
        // PMAT-048: same disposition.
        Stmt::ShellLoop { .. } => Err(RuchyCodegenError::Unsupported(
            "Ruchy backend does not lower Stmt::ShellLoop — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell loops; \
             use `--target shell`"
                .into(),
        )),
        // PMAT-051: same disposition.
        Stmt::ShellAssign { name, .. } => Err(RuchyCodegenError::Unsupported(format!(
            "Ruchy backend does not lower Stmt::ShellAssign (`{name}=…`) — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell variable assignment; \
             use `--target shell`"
        ))),
    }
}

fn emit_param(out: &mut String, p: &Param) -> Result<(), RuchyCodegenError> {
    // PMAT-460: same posture as the Rust backend.
    if p.mutable {
        write!(out, "mut ")?;
    }
    write!(out, "{}: ", p.name)?;
    emit_type(out, &p.ty)?;
    Ok(())
}

/// Escape a string for emission inside a Ruchy `"..."` literal.
/// PMAT-449 — Ruchy compiles to Rust, so identical escape semantics.
fn escape_ruchy_str(s: &str) -> String {
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

fn emit_type(out: &mut String, t: &Type) -> Result<(), RuchyCodegenError> {
    match t {
        Type::I64 => out.push_str("i64"),
        // PMAT-477 (R8): Ruchy → Rust `f64`.
        Type::F64 => out.push_str("f64"),
        Type::Bool => out.push_str("bool"),
        // Ruchy compiles to Rust → same BigInt re-export. PMAT-012.
        Type::BigInt => out.push_str("xpile_bigint::BigInt"),
        // PMAT-449 (v0.2.0 Track 1.A): Ruchy → Rust → owned `String`,
        // mirrors xpile-rust-codegen's lowering.
        Type::Str => out.push_str("String"),
        // PMAT-455 (v0.2.0 Track 1.B): Ruchy → Rust Vec<T>.
        Type::List(elem_ty) => {
            out.push_str("Vec<");
            emit_type(out, elem_ty)?;
            out.push('>');
        }
        // PMAT-462 (v0.2.0 Track 1.C): Ruchy → Rust HashMap<K, V>.
        Type::Dict(k_ty, v_ty) => {
            out.push_str("std::collections::HashMap<");
            emit_type(out, k_ty)?;
            out.push_str(", ");
            emit_type(out, v_ty)?;
            out.push('>');
        }
        // PMAT-500: Ruchy → Rust `HashSet<T>`.
        Type::Set(elem_ty) => {
            out.push_str("std::collections::HashSet<");
            emit_type(out, elem_ty)?;
            out.push('>');
        }
        // PMAT-494: Ruchy → Rust `(T0, T1, ...)`.
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
        // PMAT-046: same disposition as the Rust backend.
        Type::ShellString | Type::ExitCode => {
            return Err(RuchyCodegenError::Unsupported(format!(
                "Ruchy backend does not lower {t:?} — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs the bashrs type domain; \
                 use `--target shell`"
            )));
        }
    }
    Ok(())
}

fn emit_expr(out: &mut String, e: &Expr, mode: bool) -> Result<(), RuchyCodegenError> {
    match e {
        Expr::Ident(name) => {
            // PMAT-025: in BigInt mode, append `.clone()` to every
            // Ident reference. BigInt isn't `Copy` (it's
            // heap-allocated), so a name referenced in cond +
            // branches + recursive call would move-on-first-use.
            // Mirrors the Rust backend's PMAT-013 emission.
            if mode {
                write!(out, "{}.clone()", name)?;
            } else {
                write!(out, "{}", name)?;
            }
        }
        Expr::LitInt(v) => {
            if mode {
                write!(out, "xpile_bigint::BigInt::from({}i64)", v)?;
            } else {
                write!(out, "{}i64", v)?;
            }
        }
        // PMAT-477 (R8): float literal + plain-infix float arithmetic.
        Expr::LitFloat(v) => write!(out, "{}f64", v)?,
        Expr::FloatBinOp { op, lhs, rhs } => {
            out.push('(');
            emit_expr(out, lhs, mode)?;
            write!(out, " {} ", float_op_sym(*op))?;
            emit_expr(out, rhs, mode)?;
            out.push(')');
        }
        // PMAT-456 (v0.2.0 Track 1.B): Ruchy → Rust → lowercase
        // `true` / `false`.
        Expr::LitBool(b) => write!(out, "{}", b)?,
        Expr::BinOp { op, lhs, rhs } => emit_binop(out, *op, lhs, rhs, mode)?,
        // PMAT-451 (v0.2.0 Track 1.A): same str-concat shape as the
        // Rust backend — Ruchy compiles to Rust, so `format!()` works
        // identically.
        Expr::Concat { lhs, rhs } => {
            out.push_str("format!(\"{}{}\", ");
            emit_expr(out, lhs, mode)?;
            out.push_str(", ");
            emit_expr(out, rhs, mode)?;
            out.push(')');
        }
        // PMAT-502am: a formatted f-string field → `format!("{:<spec>}", v)`.
        Expr::FormatSpec { value, rust_spec } => {
            write!(out, "format!(\"{{:{rust_spec}}}\", ")?;
            emit_expr(out, value, mode)?;
            out.push(')');
        }
        // PMAT-492/493b: Python string methods (Ruchy → Rust). No-arg
        // transforms emit a suffix; startswith/endswith emit
        // `.starts_with(&(<pat>)[..])` (the reslice yields `&str`).
        Expr::StrMethod { recv, op, args } => {
            // PMAT-492d: `join` inverts receiver/arg (sep.join(xs) → xs.join(sep)).
            if matches!(op, StrMethodOp::Join) {
                emit_expr(out, &args[0], mode)?;
                out.push_str(".join(&(");
                emit_expr(out, recv, mode)?;
                out.push_str(")[..])");
                return Ok(());
            }
            // PMAT-502ag: `.isdigit()`/`.isalpha()`/`.isspace()` →
            // `(!(s).is_empty() && (s).chars().all(|__c| __c.<pred>()))`.
            if matches!(
                op,
                StrMethodOp::IsDigit | StrMethodOp::IsAlpha | StrMethodOp::IsSpace
            ) {
                out.push_str("(!(");
                emit_expr(out, recv, mode)?;
                out.push_str(").is_empty() && (");
                emit_expr(out, recv, mode)?;
                out.push_str(").chars().all(|__c| __c.");
                out.push_str(match op {
                    StrMethodOp::IsDigit => "is_ascii_digit()",
                    StrMethodOp::IsAlpha => "is_alphabetic()",
                    _ => "is_whitespace()",
                });
                out.push_str("))");
                return Ok(());
            }
            // PMAT-502ah: `.capitalize()` → first char upper, rest lower.
            if matches!(op, StrMethodOp::Capitalize) {
                out.push_str("{ let __cs = &(");
                emit_expr(out, recv, mode)?;
                out.push_str("); let mut __ch = __cs.chars(); match __ch.next() { Some(__f) => __f.to_uppercase().collect::<String>() + &(__ch.as_str().to_lowercase()), None => String::new() } }");
                return Ok(());
            }
            // PMAT-502aj: `.title()` → title-case each word.
            if matches!(op, StrMethodOp::Title) {
                out.push_str("{ let mut __tr = String::new(); let mut __pa = false; for __c in (");
                emit_expr(out, recv, mode)?;
                out.push_str(").chars() { if __c.is_alphabetic() { if __pa { __tr.extend(__c.to_lowercase()); } else { __tr.extend(__c.to_uppercase()); } __pa = true; } else { __tr.push(__c); __pa = false; } } __tr }");
                return Ok(());
            }
            // PMAT-502aw: `.rjust(w)`/`.ljust(w)`, matching the Rust backend.
            if matches!(op, StrMethodOp::RJust | StrMethodOp::LJust) {
                out.push_str(if matches!(op, StrMethodOp::RJust) {
                    "format!(\"{:>1$}\", "
                } else {
                    "format!(\"{:<1$}\", "
                });
                emit_expr(out, recv, mode)?;
                out.push_str(", (");
                emit_expr(out, &args[0], mode)?;
                out.push_str(") as usize)");
                return Ok(());
            }
            emit_expr(out, recv, mode)?;
            match op {
                StrMethodOp::Upper => out.push_str(".to_uppercase()"),
                StrMethodOp::Lower => out.push_str(".to_lowercase()"),
                StrMethodOp::Strip => out.push_str(".trim().to_string()"),
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
                // PMAT-502b: `.replace(old, new)`.
                StrMethodOp::Replace => {
                    out.push_str(".replace(&(");
                    emit_expr(out, &args[0], mode)?;
                    out.push_str(")[..], &(");
                    emit_expr(out, &args[1], mode)?;
                    out.push_str(")[..])");
                }
                // PMAT-502l: lstrip/rstrip → trim_start/trim_end; find/count → i64.
                StrMethodOp::LStrip => out.push_str(".trim_start().to_string()"),
                StrMethodOp::RStrip => out.push_str(".trim_end().to_string()"),
                StrMethodOp::Find => {
                    out.push_str(".find(&(");
                    emit_expr(out, &args[0], mode)?;
                    out.push_str(")[..]).map(|__i| __i as i64).unwrap_or(-1)");
                }
                StrMethodOp::Count => {
                    out.push_str(".matches(&(");
                    emit_expr(out, &args[0], mode)?;
                    out.push_str(")[..]).count() as i64");
                }
                StrMethodOp::Join => unreachable!("Join handled above"),
                StrMethodOp::IsDigit | StrMethodOp::IsAlpha | StrMethodOp::IsSpace => {
                    unreachable!("classification predicates handled above")
                }
                StrMethodOp::Capitalize => unreachable!("capitalize handled above"),
                StrMethodOp::Title => unreachable!("title handled above"),
                StrMethodOp::RJust | StrMethodOp::LJust => {
                    unreachable!("rjust/ljust handled above")
                }
            }
        }
        // PMAT-455 (v0.2.0 Track 1.B): Ruchy → Rust → `vec![...]`.
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
        // PMAT-494: Python tuple literal → Ruchy → Rust `(e0, e1, ...)`.
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
        // PMAT-502q: Python `t[N]` (tuple) → `(<tuple>).N.clone()`.
        Expr::TupleIndex { tuple, index } => {
            out.push('(');
            emit_expr(out, tuple, mode)?;
            write!(out, ").{index}.clone()")?;
        }
        // PMAT-496: Ruchy → Rust slice (`.to_vec()` / `.to_string()`).
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
            // PMAT-502bc: positive list step, matching the Rust backend.
            match step {
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
                    out.push_str(if matches!(op, NumBuiltinOp::Min) {
                        ".min("
                    } else {
                        ".max("
                    });
                    emit_expr(out, &args[1], mode)?;
                    out.push(')');
                }
            }
        }
        // PMAT-498b: `sum(xs)` → `<list>.iter().sum::<T>()`.
        Expr::Sum { list, of_float } => {
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
        // PMAT-502k: `seq * n` → `(seq).repeat(((n).max(0)) as usize)`.
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
            // PMAT-502bf: string parse, matching the Rust backend.
            if *from_str {
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
        // Python-matching format block (float).
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
        // PMAT-502ak: `round(x)` (float) → `((x).round_ties_even() as i64)`.
        Expr::RoundToInt { value } => {
            out.push_str("((");
            emit_expr(out, value, mode)?;
            out.push_str(").round_ties_even() as i64)");
        }
        // PMAT-502al: `round(x, n)` (float) → Python's decimal rounding
        // (format-to-n-decimals for n >= 0, scale for n < 0).
        Expr::RoundToDigits { value, ndigits } => {
            out.push_str("{ let __rx = ");
            emit_expr(out, value, mode)?;
            out.push_str("; let __rn = ");
            emit_expr(out, ndigits, mode)?;
            out.push_str("; if __rn >= 0 { format!(\"{:.1$}\", __rx, __rn as usize).parse::<f64>().unwrap() } else { let __rp = 10f64.powi((-__rn) as i32); (__rx / __rp).round_ties_even() * __rp } }");
        }
        // PMAT-502e/h/aa: 1-arg `min(xs)`/`max(xs)`; `key=lambda` →
        // `min_by_key`/`max_by_key`.
        Expr::ListMinMax {
            list,
            is_max,
            of_float,
            key,
        } => {
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
                    out.push_str(" }).unwrap()");
                }
                None => out.push_str(match (*of_float, *is_max) {
                    (false, true) => ".iter().copied().max().unwrap()",
                    (false, false) => ".iter().copied().min().unwrap()",
                    (true, true) => ".iter().copied().fold(f64::NEG_INFINITY, f64::max)",
                    (true, false) => ".iter().copied().fold(f64::INFINITY, f64::min)",
                }),
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
        // PMAT-502as: `xs.pop()` / `xs.pop(i)`, matching the Rust backend.
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
        // PMAT-502au: `d.pop(k)` / `d.pop(k, def)`, matching the Rust backend.
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
        // PMAT-502ax: `d.setdefault(k, default)`, matching the Rust backend.
        Expr::DictSetDefault { dict, key, default } => {
            out.push('(');
            emit_expr(out, dict, mode)?;
            out.push_str(").entry((");
            emit_expr(out, key, mode)?;
            out.push_str(").clone()).or_insert(");
            emit_expr(out, default, mode)?;
            out.push_str(").clone()");
        }
        // PMAT-502c/f/z: clone+sort block; `reverse=True` appends
        // `__xv.reverse();`; `key=lambda p: e` → `sort_by_key`.
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
        // PMAT-502ab: `filter(pred, xs)` → `.iter().cloned().filter(...).collect()`.
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
        // PMAT-502ac: `map(f, xs)` → `.iter().cloned().map(...).collect()`.
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
        // PMAT-462 (v0.2.0 Track 1.C): Ruchy → Rust HashMap-init block.
        // PMAT-466: empty literal → bare `HashMap::new()` (see the Rust
        // backend's twin arm — avoids clippy `unused_mut`).
        Expr::DictLit(pairs) => {
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
        // PMAT-457 (v0.2.0 Track 1.B): Ruchy → Rust →
        // `xs[i as usize].clone()`, matching the Rust backend.
        Expr::Index { collection, index } => {
            emit_expr(out, collection, mode)?;
            out.push('[');
            emit_expr(out, index, mode)?;
            out.push_str(" as usize].clone()");
        }
        // PMAT-466 (v0.2.0 Track 1.C): dict ops → Rust, matching the
        // Rust backend exactly (Ruchy compiles to Rust).
        Expr::DictGet { dict, key } => {
            emit_expr(out, dict, mode)?;
            out.push_str("[&(");
            emit_expr(out, key, mode)?;
            out.push_str(")].clone()");
        }
        Expr::DictGetOr { dict, key, default } => {
            emit_expr(out, dict, mode)?;
            out.push_str(".get(&(");
            emit_expr(out, key, mode)?;
            out.push_str(")).cloned().unwrap_or(");
            emit_expr(out, default, mode)?;
            out.push(')');
        }
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
        // PMAT-500/501b: Ruchy → Rust set literal (empty → bare new()).
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
        // PMAT-502g: set algebra → fresh HashSet via `.cloned().collect()`.
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
        // PMAT-459 (v0.2.0 Track 1.B): Ruchy → Rust → `.len() as i64`.
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
        // PMAT-449 (v0.2.0 Track 1.A): Python `str` literal → Ruchy
        // owned `String::from("...")`. Same escape semantics as the
        // Rust backend.
        Expr::LitStr(s) => {
            write!(out, "String::from(\"{}\")", escape_ruchy_str(s))?;
        }
        // PMAT-042: `QuotedString` carries explicit shell quoting and
        // stays bashrs-only.
        Expr::QuotedString { .. } => {
            return Err(RuchyCodegenError::Unsupported(
                "Ruchy backend does not lower Expr::QuotedString — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs quoted shell strings; \
                 use `--target shell`"
                    .into(),
            ));
        }
        // PMAT-045: see rust-codegen's matching arm.
        Expr::ShellVar(name) => {
            return Err(RuchyCodegenError::Unsupported(format!(
                "Ruchy backend does not lower Expr::ShellVar (${name}) — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs shell variable refs; \
                 use `--target shell`"
            )));
        }
        // PMAT-047: see rust-codegen.
        Expr::CommandSubstitution(_) => {
            return Err(RuchyCodegenError::Unsupported(
                "Ruchy backend does not lower Expr::CommandSubstitution — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs shell substitution; \
                 use `--target shell`"
                    .into(),
            ));
        }
        // PMAT-055: see rust-codegen.
        Expr::ShellSpecial(name) => {
            return Err(RuchyCodegenError::Unsupported(format!(
                "Ruchy backend does not lower Expr::ShellSpecial (${name}) — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs shell special params; \
                 use `--target shell`"
            )));
        }
    }
    Ok(())
}

fn emit_unop(
    out: &mut String,
    op: UnOp,
    operand: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
    match op {
        UnOp::Neg => {
            if mode {
                // BigInt::neg is total — no overflow.
                write!(out, "(-")?;
                emit_expr(out, operand, mode)?;
                write!(out, ")")?;
            } else {
                // Python: `-x` on int never overflows mathematically.
                // Rust i64::MIN.checked_neg() == None — use checked_neg
                // + panic pointing at C-PY-INT-ARITH slow path.
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
) -> Result<(), RuchyCodegenError> {
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

/// Ruchy uses Rust-like `if cond { then } else { else_ }` as an expression.
/// Flattens nested `else if` for readability (same pattern as the Rust backend).
fn emit_if_expr(
    out: &mut String,
    cond: &Expr,
    then_expr: &Expr,
    else_expr: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
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
        } => emit_if_expr(out, c2, t2, e2, mode),
        _ => {
            write!(out, "{{ ")?;
            emit_expr(out, else_expr, mode)?;
            write!(out, " }}")?;
            Ok(())
        }
    }
}

/// Arithmetic emits two shapes per the C-PY-INT-ARITH contract:
///
/// * i64 fast path: `.checked_*().expect("...")` with the slow-path
///   panic message (no overflow → no panic).
/// * BigInt slow path (mode=true): plain infix on BigInt operands
///   (BigInt overloads `+ - * <= ...`); FloorDiv / Mod use
///   `xpile_bigint::div_floor / mod_floor`; bitwise / shift / pow
///   deferred (same scope as the Rust backend).
///
/// Mirrors the Rust backend's emission shape — Ruchy compiles to Rust
/// so they share semantics. PMAT-025.
fn emit_binop(
    out: &mut String,
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
    match op {
        BinOp::Add if mode => emit_infix(out, lhs, " + ", rhs, mode),
        BinOp::Sub if mode => emit_infix(out, lhs, " - ", rhs, mode),
        BinOp::Mul if mode => emit_infix(out, lhs, " * ", rhs, mode),
        BinOp::FloorDiv if mode => emit_bigint_floor_call(out, "div_floor", lhs, rhs, mode),
        BinOp::Mod if mode => emit_bigint_floor_call(out, "mod_floor", lhs, rhs, mode),
        // PMAT-026 / PMAT-013-FOLLOWUP — mirror of the Rust backend.
        // See `xpile-rust-codegen/src/lib.rs` for the design rationale.
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

/// BigInt-mode floor-div / mod via the helpers in xpile-bigint
/// (num-bigint requires `Integer` trait + reference operands).
/// PMAT-025; mirrors Rust backend.
fn emit_bigint_floor_call(
    out: &mut String,
    method: &str,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
    write!(out, "xpile_bigint::{method}(&")?;
    emit_expr(out, lhs, mode)?;
    write!(out, ", &")?;
    emit_expr(out, rhs, mode)?;
    write!(out, ")")?;
    Ok(())
}

fn emit_checked_pow(
    out: &mut String,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
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

fn emit_checked_shift(
    out: &mut String,
    lhs: &Expr,
    method: &str,
    rhs: &Expr,
    op_name: &str,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
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

fn emit_checked(
    out: &mut String,
    lhs: &Expr,
    method: &str,
    rhs: &Expr,
    op_name: &str,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
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
) -> Result<(), RuchyCodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs, mode)?;
    out.push_str(op);
    emit_expr(out, rhs, mode)?;
    write!(out, ")")?;
    Ok(())
}

pub struct RuchyBackend;

impl Backend for RuchyBackend {
    fn name(&self) -> &'static str {
        "ruchy"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Ruchy]
    }

    fn lower(&self, module: &Module, _config: &BackendConfig) -> Result<Artifact, BackendError> {
        let primary = emit_module(module).map_err(|e| BackendError::Lower(e.to_string()))?;
        Ok(Artifact {
            primary,
            sidecars: Vec::new(),
            citations: Vec::new(),
            quorum_status: QuorumStatus::Single {
                emitter: "xpile-ruchy-codegen".to_string(),
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
    fn emits_fun_keyword_not_pub_fn() {
        let m = module_with("fixture", vec![Item::Function(add_fn())]);
        let ruchy = emit_module(&m).expect("emit ok");
        assert!(
            ruchy.contains("fun add("),
            "Ruchy uses `fun`, not `fn` or `pub fn`: got\n{}",
            ruchy
        );
        assert!(
            !ruchy.contains("pub fn"),
            "Ruchy emission must not produce `pub fn` (that's Rust)"
        );
        // Post PMAT-002: addition lowers to checked_add (Ruchy compiles
        // to Rust, so it shares Rust's overflow semantics + contract
        // C-PY-INT-ARITH).
        assert!(
            ruchy.contains("checked_add"),
            "expected checked_add: {ruchy}"
        );
        assert!(ruchy.contains("C-PY-INT-ARITH"));
    }

    #[test]
    fn ruchy_floordiv_also_uses_div_euclid() {
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
        let ruchy = emit_module(&m).expect("emit ok");
        assert!(ruchy.contains("div_euclid"));
        assert!(!ruchy.contains(" / "));
    }
}
