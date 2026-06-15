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
    NumBuiltinOp, Param, Radix, SetOp, SetPredOp, Stmt, StrMethodOp, Type, UnOp,
};

/// PMAT-477 (R8): Ruchy → Rust infix symbol for a float arithmetic op.
/// PMAT-502by: escape a string for embedding inside a `println!`/`print!`
/// format-string literal (see the Rust backend's twin). Used by `print`'s
/// `sep=`/`end=` kwargs.
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

fn float_op_sym(op: FloatOp) -> &'static str {
    match op {
        FloatOp::Add => "+",
        FloatOp::Sub => "-",
        FloatOp::Mul => "*",
        FloatOp::Div => "/",
        // FloorDiv/Mod/Pow + math method-ops use dedicated formulas — keep the
        // match exhaustive.
        FloatOp::FloorDiv => "//",
        FloatOp::Mod => "%",
        FloatOp::Pow => "**",
        FloatOp::Hypot => "hypot",
        FloatOp::Atan2 => "atan2",
        FloatOp::Log => "log",
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
    // PMAT-573: escape Rust-keyword identifiers on a cloned IR before
    // emission (Ruchy shares Rust's keyword set + raw-identifier `r#`
    // syntax). See the Rust backend's twin and `escape_rust_reserved_idents`.
    let mut module = module.clone();
    xpile_meta_hir::escape_rust_reserved_idents(&mut module);
    let module = &module;
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
            // PMAT-502bj: module-level constant → `const NAME: TY = VALUE;`.
            Item::Const { name, ty, value } => {
                write!(out, "const {name}: ")?;
                emit_type(&mut out, ty)?;
                out.push_str(" = ");
                emit_expr(&mut out, value, /*mode=*/ false)?;
                out.push_str(";\n");
            }
            // PMAT-505a (classes epic, first cut): dataclass → derived struct
            // (Ruchy compiles to Rust — same shape).
            Item::Struct {
                name,
                fields,
                methods,
                frozen,
            } => {
                // PMAT-592: frozen dataclass is hashable → derive Eq, Hash when
                // all fields are Eq+Hash-capable. Matches the Rust backend.
                let derive_eq_hash = *frozen
                    && fields
                        .iter()
                        .all(|(_, ty)| matches!(ty, Type::I64 | Type::Bool | Type::Str));
                if derive_eq_hash {
                    out.push_str("#[derive(Clone, Debug, PartialEq, Eq, Hash)]\n");
                } else {
                    out.push_str("#[derive(Clone, Debug, PartialEq)]\n");
                }
                writeln!(out, "pub struct {name} {{")?;
                for (field, ty) in fields {
                    write!(out, "    pub {field}: ")?;
                    emit_type(&mut out, ty)?;
                    out.push_str(",\n");
                }
                out.push_str("}\n");
                // PMAT-506d: instance methods → an `impl` block (Ruchy → Rust).
                if !methods.is_empty() {
                    writeln!(out, "impl {name} {{")?;
                    for m in methods {
                        emit_function(&mut out, m)?;
                    }
                    out.push_str("}\n");
                }
            }
            // PMAT-513: a Python `Enum` class → a Rust enum (Ruchy → Rust).
            Item::Enum { name, variants } => {
                out.push_str("#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n");
                writeln!(out, "pub enum {name} {{")?;
                for (variant, _disc) in variants {
                    writeln!(out, "    {variant},")?;
                }
                out.push_str("}\n");
            }
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
            | Stmt::Continue
            | Stmt::Break
            // PMAT-502bw: print() introduces no binding.
            | Stmt::Print { .. }
            | Stmt::Raise { .. } => false,
            Stmt::While { body, .. }
            | Stmt::ForEach { body, .. }
            | Stmt::ForEachPair { body, .. }
            | Stmt::ForEachZip3 { body, .. } => body.iter().any(stmt_has_bigint),
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
            | Stmt::ListInsert { .. }
            | Stmt::ListRemoveValue { .. } => false,
            // PMAT-461: indexed assignment same disposition.
            Stmt::IndexAssign { .. } => false,
            // PMAT-533: subscript-receiver append carries no Type::Let.
            Stmt::IndexAppend { .. } => false,
            // PMAT-466: dict keyed assignment same disposition.
            Stmt::DictSet { .. } => false,
            // PMAT-506c: field assignment introduces no binding.
            Stmt::FieldAssign { .. } => false,
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
            // PMAT-598: suppress the element-type annotation on a mutable empty
            // `set()` so rustc infers it from the later `.insert(...)` (matches
            // the Rust backend).
            let infer_set_elem = *mutable
                && matches!(value, Expr::SetLit(elems) if elems.is_empty())
                && matches!(ty, Type::Set(inner) if **inner == Type::I64);
            let kw = if *mutable { "let mut" } else { "let" };
            if infer_set_elem {
                write!(out, "{indent}{kw} {name} = ")?;
            } else {
                write!(out, "{indent}{kw} {name}: ")?;
                emit_type(out, ty)?;
                write!(out, " = ")?;
            }
            emit_expr(out, value, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-494b: tuple unpacking → `let (a, b, ...) = <value>;`.
        Stmt::LetTuple {
            names,
            mutable,
            value,
        } => {
            // PMAT-547: mark each unpacked name `mut` per its `mutable` flag.
            let pat = names
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    if mutable.get(i).copied().unwrap_or(false) {
                        format!("mut {n}")
                    } else {
                        n.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            write!(out, "{indent}let ({pat}) = ")?;
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
        // PMAT-502bk: loop-control statements, matching the Rust backend.
        Stmt::Continue => {
            writeln!(out, "{indent}continue;")?;
            Ok(())
        }
        Stmt::Break => {
            writeln!(out, "{indent}break;")?;
            Ok(())
        }
        // PMAT-502bw/by: `print(a, b, …, sep=…, end=…)` (see the Rust
        // backend for the join/end logic).
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
                xpile_meta_hir::PairIterKind::Enumerate { start } => {
                    // PMAT-502ca / PMAT-595: `enumerate(xs, start)` offsets the
                    // index; the offset add honors C-PY-INT-ARITH.
                    if *start == 0 {
                        out.push_str(
                            ".iter().cloned().enumerate().map(|(__i, __e)| (__i as i64, __e))",
                        );
                    } else {
                        write!(
                            out,
                            ".iter().cloned().enumerate().map(|(__i, __e)| ((__i as i64).checked_add({start}i64).expect(\"xpile: i64 addition overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"), __e))"
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
        // PMAT-562: three-way `zip` → nested `.zip()` chain + `((a, b), c)`.
        Stmt::ForEachZip3 {
            first,
            second,
            third,
            iter1,
            iter2,
            iter3,
            body,
        } => {
            write!(out, "{indent}for (({first}, {second}), {third}) in ")?;
            emit_expr(out, iter1, mode)?;
            out.push_str(".iter().cloned().zip(");
            emit_expr(out, iter2, mode)?;
            out.push_str(".iter().cloned()).zip(");
            emit_expr(out, iter3, mode)?;
            out.push_str(".iter().cloned())");
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
                // PMAT-616: NaN-safe float sort (Python doesn't raise on NaN).
                ListMutateOp::Sort if *of_float => writeln!(
                    out,
                    "{indent}{list_name}.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));"
                )?,
                ListMutateOp::Sort => writeln!(out, "{indent}{list_name}.sort();")?,
                // PMAT-555: descending in-place sort (`sort(reverse=True)`).
                ListMutateOp::SortDesc if *of_float => writeln!(
                    out,
                    "{indent}{list_name}.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));"
                )?,
                ListMutateOp::SortDesc => {
                    writeln!(out, "{indent}{list_name}.sort_by(|a, b| b.cmp(a));")?
                }
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
        // PMAT-502ar / PMAT-590: `xs.insert(i, x)` clamps the index to
        // CPython `list.insert` semantics, matching the Rust backend.
        Stmt::ListInsert {
            list_name,
            index,
            elem,
        } => {
            write!(
                out,
                "{indent}{{ let __n = {list_name}.len() as i64; let mut __i = ("
            )?;
            emit_expr(out, index, mode)?;
            out.push_str("); if __i < 0 { __i += __n; if __i < 0 { __i = 0; } } if __i > __n { __i = __n; } ");
            write!(out, "{list_name}.insert(__i as usize, ")?;
            emit_expr(out, elem, mode)?;
            writeln!(out, "); }}")?;
            Ok(())
        }
        // PMAT-502eg: `xs.remove(x)` → position-find + remove, matching the
        // Rust backend (panics ≈ Python `ValueError` when absent).
        Stmt::ListRemoveValue { list_name, value } => {
            write!(out, "{indent}{{ let __v = ")?;
            emit_expr(out, value, mode)?;
            write!(
                out,
                "; let __p = {list_name}.iter().position(|__e| *__e == __v)\
                 .expect(\"xpile: ValueError: list.remove(x): x not in list\"); \
                 {list_name}.remove(__p); }}"
            )?;
            out.push('\n');
            Ok(())
        }
        // PMAT-461 (v0.2.0 Track 1.B): Ruchy → Rust →
        // `xs[i as usize] = v;`, matching the Rust backend.
        Stmt::IndexAssign {
            list_name,
            indices,
            value,
        } => {
            // PMAT-502dy: nested list indexing (`grid[i][j] = v`).
            // PMAT-560: a self-referential index (`xs[len(xs) - k] = v`, the
            // negative-index desugar) is bound to a temp first to avoid the
            // index_mut borrow conflict (E0502) — mirrors the Rust backend.
            let needs_temps = indices.iter().any(|i| expr_mentions_ident(i, list_name));
            if needs_temps {
                out.push_str(indent);
                out.push_str("{ ");
                for (n, index) in indices.iter().enumerate() {
                    write!(out, "let __ix{n} = (")?;
                    emit_expr(out, index, mode)?;
                    out.push_str(") as usize; ");
                }
                write!(out, "{list_name}")?;
                for n in 0..indices.len() {
                    write!(out, "[__ix{n}]")?;
                }
                out.push_str(" = ");
                emit_expr(out, value, mode)?;
                out.push_str("; }");
                writeln!(out)?;
                return Ok(());
            }
            write!(out, "{indent}{list_name}")?;
            for index in indices {
                out.push('[');
                emit_expr(out, index, mode)?;
                out.push_str(" as usize]");
            }
            out.push_str(" = ");
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
        // PMAT-533: append on a subscript receiver (mirrors the Rust twin).
        Stmt::IndexAppend {
            base,
            index,
            elem,
            base_is_dict,
        } => {
            if *base_is_dict {
                write!(out, "{indent}{base}.get_mut(&(")?;
                emit_expr(out, index, mode)?;
                out.push_str(")).unwrap().push(");
            } else {
                write!(out, "{indent}{base}[(")?;
                emit_expr(out, index, mode)?;
                out.push_str(") as usize].push(");
            }
            emit_expr(out, elem, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-506c: struct field assignment `(obj).field = value;`.
        Stmt::FieldAssign { obj, field, value } => {
            write!(out, "{indent}({obj}).{field} = ")?;
            emit_expr(out, value, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-502at: `del coll[key]`, matching the Rust backend.
        Stmt::DelItem { name, key, is_dict } => {
            if *is_dict {
                write!(out, "{indent}{name}.remove(&(")?;
                emit_expr(out, key, mode)?;
                writeln!(out, "));")?;
            } else if expr_mentions_ident(key, name) {
                // PMAT-570: `del xs[-k]` index references `xs` — bind before remove.
                write!(out, "{indent}{{ let __di = (")?;
                emit_expr(out, key, mode)?;
                writeln!(out, ") as usize; {name}.remove(__di); }}")?;
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
    // PMAT-506d: a method's `self` receiver emits as `&self`.
    if p.name == "self" {
        out.push_str("&self");
        return Ok(());
    }
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
        // PMAT-502bl: Python `None` return → unit `()`.
        Type::Unit => out.push_str("()"),
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
            // PMAT-625: 1-element tuple needs `(T,)` (matches the Rust backend).
            if elems.len() == 1 {
                out.push(',');
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
        // PMAT-502ew: Python `Optional[T]` → `Option<T>`, matching Rust.
        Type::Optional(inner) => {
            out.push_str("Option<");
            emit_type(out, inner)?;
            out.push('>');
        }
        // PMAT-506b: struct-typed value emits the bare struct name.
        Type::Struct(name) => out.push_str(name),
    }
    Ok(())
}

/// PMAT-560: does `e` reference the identifier `name`? (See the Rust backend's
/// twin for the `IndexAssign` self-referential-index rationale.)
fn expr_mentions_ident(e: &Expr, name: &str) -> bool {
    match e {
        Expr::Ident(n) => n == name,
        Expr::Len(inner) => expr_mentions_ident(inner, name),
        Expr::BinOp { lhs, rhs, .. } | Expr::FloatBinOp { lhs, rhs, .. } => {
            expr_mentions_ident(lhs, name) || expr_mentions_ident(rhs, name)
        }
        Expr::UnOp { operand, .. } => expr_mentions_ident(operand, name),
        Expr::NumCast { value, .. } => expr_mentions_ident(value, name),
        Expr::Index { collection, index } => {
            expr_mentions_ident(collection, name) || expr_mentions_ident(index, name)
        }
        _ => false,
    }
}

fn emit_expr(out: &mut String, e: &Expr, mode: bool) -> Result<(), RuchyCodegenError> {
    match e {
        // PMAT-502bl: the unit value (void function trailing return).
        Expr::Unit => out.push_str("()"),
        // PMAT-502dt: a block-expr — `{ <stmts> <trailing> }`.
        Expr::Block(b) => {
            out.push_str("{ ");
            for stmt in &b.stmts {
                emit_stmt(out, stmt, mode)?;
            }
            emit_expr(out, &b.trailing_return, mode)?;
            out.push_str(" }");
        }
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
        Expr::FloatBinOp { op, lhs, rhs } => match op {
            // PMAT-614: Python float floor-division is CPython `float_divmod`,
            // not `(a / b).floor()` (the naive floor over-rounds `1.0 // 0.1` to
            // 10.0 vs Python's 9.0, and mishandles infinite operands). Matches
            // the Rust backend: fmod-based div with sign-adjust + round-up.
            // PMAT-581: guard the zero divisor (Python raises ZeroDivisionError).
            FloatOp::FloorDiv => {
                out.push_str("{ let __fa: f64 = ");
                emit_expr(out, lhs, mode)?;
                out.push_str("; let __fz: f64 = ");
                emit_expr(out, rhs, mode)?;
                out.push_str("; if __fz == 0.0 { panic!(\"xpile: ZeroDivisionError: float floor division by zero\"); } let __fm = __fa % __fz; let mut __fd = (__fa - __fm) / __fz; if __fm != 0.0 && ((__fz < 0.0) != (__fm < 0.0)) { __fd -= 1.0; } let __ffl = __fd.floor(); if __fd - __ffl > 0.5 { __ffl + 1.0 } else { __ffl } }");
            }
            // PMAT-591: Python float modulo is CPython `float_rem` —
            // `fmod(a,b)` (Rust `%`) + sign-adjust toward the divisor, else
            // `copysign(0.0,b)`. Matches the Rust backend (the prior floor
            // formula diverged in the last ULP and lost the signed zero).
            // PMAT-581: guard the zero divisor; bind operands (evaluate-once).
            FloatOp::Mod => {
                out.push_str("{ let __fz: f64 = ");
                emit_expr(out, rhs, mode)?;
                out.push_str("; if __fz == 0.0 { panic!(\"xpile: ZeroDivisionError: float modulo\"); } let __fn: f64 = ");
                emit_expr(out, lhs, mode)?;
                out.push_str("; let __r = __fn % __fz; if __r != 0.0 { if (__fz < 0.0) != (__r < 0.0) { __r + __fz } else { __r } } else { 0.0_f64.copysign(__fz) } }");
            }
            // PMAT-502bt/em/en: method-style float ops — `(a).<method>(b)`,
            // matching the Rust backend.
            FloatOp::Pow | FloatOp::Hypot | FloatOp::Atan2 | FloatOp::Log => {
                let method = match op {
                    FloatOp::Pow => "powf",
                    FloatOp::Hypot => "hypot",
                    FloatOp::Atan2 => "atan2",
                    FloatOp::Log => "log",
                    _ => unreachable!(),
                };
                out.push('(');
                emit_expr(out, lhs, mode)?;
                write!(out, ").{method}(")?;
                emit_expr(out, rhs, mode)?;
                out.push(')');
            }
            // PMAT-581: float `/` (and int true-division) raises ZeroDivisionError.
            FloatOp::Div => {
                out.push_str("{ let __fz: f64 = ");
                emit_expr(out, rhs, mode)?;
                out.push_str("; if __fz == 0.0 { panic!(\"xpile: ZeroDivisionError: float division by zero\"); } (");
                emit_expr(out, lhs, mode)?;
                out.push_str(") / __fz }");
            }
            FloatOp::Add | FloatOp::Sub | FloatOp::Mul => {
                out.push('(');
                emit_expr(out, lhs, mode)?;
                write!(out, " {} ", float_op_sym(*op))?;
                emit_expr(out, rhs, mode)?;
                out.push(')');
            }
        },
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
        // PMAT-502bg: `xs + ys` (lists), matching the Rust backend.
        Expr::ListConcat { lhs, rhs } => {
            out.push('(');
            emit_expr(out, lhs, mode)?;
            out.push_str(").iter().chain((");
            emit_expr(out, rhs, mode)?;
            out.push_str(").iter()).cloned().collect::<Vec<_>>()");
        }
        // PMAT-502bh: `"<fmt>".format(args…)`, matching the Rust backend.
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
        // PMAT-502cd: `s[i]` over a string (see the Rust backend's twin) —
        // materialise the chars and index them (negative counts from the end).
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
        // PMAT-502cv: hex/oct/bin → radix string (see the Rust backend).
        Expr::IntRadixStr {
            value,
            radix,
            prefixed,
            upper,
        } => {
            out.push_str("{ let __n = (");
            emit_expr(out, value, mode)?;
            out.push_str(
                "); let __m = __n.unsigned_abs(); let __sign = if __n < 0 { \"-\" } else { \"\" }; format!(\"{}",
            );
            let (prefix, spec) = match radix {
                Radix::Hex if *upper => ("0x", "{:X}"),
                Radix::Hex => ("0x", "{:x}"),
                Radix::Oct => ("0o", "{:o}"),
                Radix::Bin => ("0b", "{:b}"),
            };
            if *prefixed {
                out.push_str(prefix);
            }
            out.push_str(spec);
            out.push_str("\", __sign, __m) }");
        }
        // PMAT-502da: `int(s, base)` → `i64::from_str_radix((s).trim(), base)`.
        Expr::IntFromStrRadix { value, radix } => {
            out.push_str("i64::from_str_radix((");
            emit_expr(out, value, mode)?;
            out.push_str(&format!(
                ").trim(), {radix}).expect(\"xpile: ValueError: invalid literal for int() with base {radix}\")"
            ));
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
                StrMethodOp::IsDigit
                    | StrMethodOp::IsAlpha
                    | StrMethodOp::IsSpace
                    | StrMethodOp::IsAlnum
            ) {
                out.push_str("(!(");
                emit_expr(out, recv, mode)?;
                out.push_str(").is_empty() && (");
                emit_expr(out, recv, mode)?;
                out.push_str(").chars().all(|__c| ");
                out.push_str(match op {
                    StrMethodOp::IsDigit => "__c.is_ascii_digit()",
                    StrMethodOp::IsAlpha => "__c.is_alphabetic()",
                    StrMethodOp::IsAlnum => "__c.is_alphanumeric()",
                    // PMAT-600: include the C0 separators U+001C..U+001F (Python
                    // isspace whitespace set; matches the Rust backend).
                    _ => "(__c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}'))",
                });
                out.push_str("))");
                return Ok(());
            }
            // PMAT-502di: `.isupper()`/`.islower()` → cased-char predicate.
            if matches!(op, StrMethodOp::IsUpper | StrMethodOp::IsLower) {
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
            // PMAT-502cq: `.removeprefix(p)`/`.removesuffix(p)` (block form,
            // matching the Rust backend).
            if matches!(op, StrMethodOp::RemovePrefix | StrMethodOp::RemoveSuffix) {
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str(if matches!(op, StrMethodOp::RemovePrefix) {
                    "); match __s.strip_prefix(&("
                } else {
                    "); match __s.strip_suffix(&("
                });
                emit_expr(out, &args[0], mode)?;
                out.push_str(")[..]) { Some(__r) => __r.to_string(), None => __s } }");
                return Ok(());
            }
            // PMAT-502cs: `.zfill(w)` (block form, matching the Rust backend).
            if matches!(op, StrMethodOp::ZFill) {
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str("); let __w = (");
                emit_expr(out, &args[0], mode)?;
                out.push_str(") as usize; let __n = __s.chars().count(); if __n >= __w { __s } else { let __pad = \"0\".repeat(__w - __n); if __s.starts_with('-') || __s.starts_with('+') { format!(\"{}{}{}\", &__s[..1], __pad, &__s[1..]) } else { format!(\"{}{}\", __pad, __s) } } }");
                return Ok(());
            }
            // PMAT-502cu: `.center(w)` (block form, matching the Rust backend).
            if matches!(op, StrMethodOp::Center) {
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str("); let __w = (");
                emit_expr(out, &args[0], mode)?;
                out.push_str(") as usize; let __n = __s.chars().count(); if __n >= __w { __s } else { let __marg = __w - __n; let __left = __marg / 2 + (__marg & __w & 1); format!(\"{}{}{}\", \" \".repeat(__left), __s, \" \".repeat(__marg - __left)) } }");
                return Ok(());
            }
            // PMAT-502dj: `.partition(sep)` / `.rpartition(sep)` → 3-tuple.
            if matches!(op, StrMethodOp::Partition | StrMethodOp::RPartition) {
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
                return Ok(());
            }
            // PMAT-502dl: `.splitlines()` → char-walk over Python's full line
            // boundary set (Rust `str::lines()` only covers LF/CRLF).
            if matches!(op, StrMethodOp::SplitLines) {
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str("); let mut __lines: Vec<String> = Vec::new(); let mut __cur = String::new(); let mut __it = __s.chars().peekable(); while let Some(__c) = __it.next() { match __c { '\\r' => { if __it.peek() == Some(&'\\n') { __it.next(); } __lines.push(std::mem::take(&mut __cur)); } '\\n' | '\\u{0b}' | '\\u{0c}' | '\\u{1c}' | '\\u{1d}' | '\\u{1e}' | '\\u{85}' | '\\u{2028}' | '\\u{2029}' => { __lines.push(std::mem::take(&mut __cur)); } _ => __cur.push(__c), } } if !__cur.is_empty() { __lines.push(__cur); } __lines }");
                return Ok(());
            }
            // PMAT-566: find/rfind/index/rindex return a Python CHAR index, not a
            // byte offset — bind recv to a temp and count chars before the match.
            if matches!(
                op,
                StrMethodOp::Find
                    | StrMethodOp::Rfind
                    | StrMethodOp::StrIndex
                    | StrMethodOp::RIndex
            ) {
                let finder = if matches!(op, StrMethodOp::Rfind | StrMethodOp::RIndex) {
                    "rfind"
                } else {
                    "find"
                };
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                write!(out, "); __s.{finder}(&(")?;
                emit_expr(out, &args[0], mode)?;
                out.push_str(")[..]).map(|__b| __s[..__b].chars().count() as i64)");
                if matches!(op, StrMethodOp::StrIndex | StrMethodOp::RIndex) {
                    out.push_str(".expect(\"xpile: ValueError: substring not found\") }");
                } else {
                    out.push_str(".unwrap_or(-1) }");
                }
                return Ok(());
            }
            emit_expr(out, recv, mode)?;
            match op {
                StrMethodOp::Upper => out.push_str(".to_uppercase()"),
                StrMethodOp::Lower => out.push_str(".to_lowercase()"),
                // PMAT-600: strip the Python whitespace set incl. U+001C..U+001F.
                StrMethodOp::Strip => out.push_str(".trim_matches(|__c: char| __c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}')).to_string()"),
                // PMAT-564: `len(str)` → Unicode char count (not byte len).
                StrMethodOp::CharCount => out.push_str(".chars().count() as i64"),
                // PMAT-530: `s[::-1]` → reverse by Unicode scalar value.
                StrMethodOp::Reverse => out.push_str(".chars().rev().collect::<String>()"),
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
                // PMAT-518: `.split(sep, maxsplit)` → `.splitn(maxsplit + 1, sep)`.
                // PMAT-621: negative maxsplit = "no limit" — `saturating_add(1)`
                // (not `+ 1`, which wraps to 0 for a negative value). Matches Rust.
                StrMethodOp::SplitN => {
                    out.push_str(".splitn(((");
                    emit_expr(out, &args[1], mode)?;
                    out.push_str(") as usize).saturating_add(1), &(");
                    emit_expr(out, &args[0], mode)?;
                    out.push_str(")[..]).map(|__c| __c.to_string()).collect::<Vec<String>>()");
                }
                // PMAT-502co: no-arg `.split()` → whitespace split.
                StrMethodOp::SplitWhitespace => {
                    out.push_str(
                        ".split_whitespace().map(|__c| __c.to_string()).collect::<Vec<String>>()",
                    );
                }
                // PMAT-502b: `.replace(old, new)`.
                StrMethodOp::Replace => {
                    out.push_str(".replace(&(");
                    emit_expr(out, &args[0], mode)?;
                    out.push_str(")[..], &(");
                    emit_expr(out, &args[1], mode)?;
                    out.push_str(")[..])");
                }
                // PMAT-517: `.replace(old, new, count)` → `.replacen(...)`.
                StrMethodOp::ReplaceN => {
                    out.push_str(".replacen(&(");
                    emit_expr(out, &args[0], mode)?;
                    out.push_str(")[..], &(");
                    emit_expr(out, &args[1], mode)?;
                    out.push_str(")[..], (");
                    emit_expr(out, &args[2], mode)?;
                    out.push_str(") as usize)");
                }
                // PMAT-502l: lstrip/rstrip → trim_start/trim_end; find/count → i64.
                // PMAT-600: against the Python whitespace set (incl. U+001C..U+001F).
                StrMethodOp::LStrip => out.push_str(".trim_start_matches(|__c: char| __c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}')).to_string()"),
                StrMethodOp::RStrip => out.push_str(".trim_end_matches(|__c: char| __c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}')).to_string()"),
                StrMethodOp::Count => {
                    out.push_str(".matches(&(");
                    emit_expr(out, &args[0], mode)?;
                    out.push_str(")[..]).count() as i64");
                }
                // PMAT-566: find/rfind/index/rindex return a CHAR index — block
                // form handled above (byte offset → chars().count()).
                StrMethodOp::Find
                | StrMethodOp::StrIndex
                | StrMethodOp::Rfind
                | StrMethodOp::RIndex => {
                    unreachable!("find/rfind/index/rindex handled above")
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
            // PMAT-625: 1-element tuple literal needs `(x,)` (matches Rust backend).
            if elems.len() == 1 {
                out.push(',');
            }
            out.push(')');
        }
        // PMAT-502q: Python `t[N]` (tuple) → `(<tuple>).N.clone()`.
        Expr::TupleIndex { tuple, index } => {
            out.push('(');
            emit_expr(out, tuple, mode)?;
            write!(out, ").{index}.clone()")?;
        }
        // PMAT-496/539: Python slice with full bound semantics (negative
        // bounds count from the end, clamp to `[0, len]`, `lo > hi` → empty).
        // Mirrors the Rust backend.
        Expr::Slice {
            collection,
            lo,
            hi,
            of_str,
            step,
        } => {
            let resolve = |out: &mut String,
                           bound: &Option<Box<Expr>>,
                           default: &str,
                           mode: bool|
             -> Result<(), RuchyCodegenError> {
                match bound {
                    Some(b) => {
                        out.push_str("{ let __b = (");
                        emit_expr(out, b, mode)?;
                        out.push_str(
                            ") as i64; if __b < 0 { (__n + __b).max(0) } else { __b.min(__n) } }",
                        );
                    }
                    None => out.push_str(default),
                }
                Ok(())
            };
            // PMAT-567: str slices index by Unicode chars (collect to Vec<char>);
            // list slices keep the by-reference element-indexed &Vec. Mirrors the
            // Rust backend.
            if *of_str {
                out.push_str("{ let __sl: Vec<char> = (");
                emit_expr(out, collection, mode)?;
                out.push_str(").chars().collect(); let __n = __sl.len() as i64; let __lo_i = ");
            } else {
                out.push_str("{ let __sl = &(");
                emit_expr(out, collection, mode)?;
                out.push_str("); let __n = __sl.len() as i64; let __lo_i = ");
            }
            resolve(out, lo, "0", mode)?;
            out.push_str("; let __hi_i = ");
            resolve(out, hi, "__n", mode)?;
            out.push_str("; let __lo = __lo_i as usize; let __hi = __hi_i.max(__lo_i) as usize; ");
            match step {
                // PMAT-548: negative list step `xs[::-k]` reverses then steps.
                Some(s) if *s < 0 => {
                    let k = (-s) as usize;
                    write!(
                        out,
                        "__sl[__lo..__hi].iter().rev().step_by({k}).cloned().collect::<Vec<_>>() }}"
                    )?;
                }
                Some(s) => {
                    write!(
                        out,
                        "__sl[__lo..__hi].iter().step_by({s}).cloned().collect::<Vec<_>>() }}"
                    )?;
                }
                None => out.push_str(if *of_str {
                    // PMAT-567: `__sl` is `Vec<char>` for str.
                    "__sl[__lo..__hi].iter().collect::<String>() }"
                } else {
                    "__sl[__lo..__hi].to_vec() }"
                }),
            }
        }
        // PMAT-498: scalar numeric builtins → receiver-method form.
        Expr::NumBuiltin { op, args, of_float } => {
            // PMAT-601: float max/min use Python first-arg-wins semantics
            // (matches the Rust backend); integer min/max keep `.min`/`.max`.
            if *of_float && matches!(op, NumBuiltinOp::Min | NumBuiltinOp::Max) {
                let cmp = if matches!(op, NumBuiltinOp::Min) {
                    "<"
                } else {
                    ">"
                };
                out.push_str("{ let mut __m: f64 = ");
                emit_expr(out, &args[0], mode)?;
                out.push(';');
                for arg in &args[1..] {
                    out.push_str(" { let __x: f64 = ");
                    emit_expr(out, arg, mode)?;
                    write!(out, "; if __x {cmp} __m {{ __m = __x; }} }}")?;
                }
                out.push_str(" __m }");
                return Ok(());
            }
            // PMAT-606: math.floor/ceil/trunc guard the rounded value (finite +
            // i64 range) and fail loud, like the int(float) guard (Rust twin).
            if matches!(
                op,
                NumBuiltinOp::Floor | NumBuiltinOp::Ceil | NumBuiltinOp::Trunc
            ) {
                let round = match op {
                    NumBuiltinOp::Floor => "floor",
                    NumBuiltinOp::Ceil => "ceil",
                    _ => "trunc",
                };
                out.push_str("{ let __mf = (");
                emit_expr(out, &args[0], mode)?;
                write!(
                    out,
                    ").{round}(); if !__mf.is_finite() {{ panic!(\"xpile: math.{round}() of a non-finite float (Python OverflowError/ValueError)\"); }} if __mf < (i64::MIN as f64) || __mf >= (i64::MAX as f64) {{ panic!(\"xpile: math.{round}() out of i64 range; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"); }} __mf as i64 }}"
                )?;
                return Ok(());
            }
            out.push('(');
            emit_expr(out, &args[0], mode)?;
            out.push(')');
            match op {
                // PMAT-579: checked i64 abs (see Rust twin); f64 abs is exact.
                NumBuiltinOp::Abs if *of_float => out.push_str(".abs()"),
                NumBuiltinOp::Abs => out.push_str(
                    ".checked_abs().expect(\"xpile: i64 abs overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\")",
                ),
                // PMAT-502ek: math functions, matching the Rust backend.
                NumBuiltinOp::Sqrt => out.push_str(".sqrt()"),
                NumBuiltinOp::Floor => out.push_str(".floor() as i64"),
                NumBuiltinOp::Ceil => out.push_str(".ceil() as i64"),
                // PMAT-502em: `math.trunc`, matching the Rust backend.
                NumBuiltinOp::Trunc => out.push_str(".trunc() as i64"),
                // PMAT-502el: trig / exp / log — matching the Rust backend.
                NumBuiltinOp::Sin => out.push_str(".sin()"),
                NumBuiltinOp::Cos => out.push_str(".cos()"),
                NumBuiltinOp::Tan => out.push_str(".tan()"),
                NumBuiltinOp::Exp => out.push_str(".exp()"),
                NumBuiltinOp::Ln => out.push_str(".ln()"),
                NumBuiltinOp::Log10 => out.push_str(".log10()"),
                NumBuiltinOp::Log2 => out.push_str(".log2()"),
                NumBuiltinOp::Min | NumBuiltinOp::Max => {
                    // PMAT-502cz: variadic — chain over every remaining arg.
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
            // PMAT-584: CPython float sum() is Neumaier-compensated (see Rust
            // twin); int stays exact `.iter().sum::<i64>()`.
            if *of_float {
                out.push_str("{ let mut __ss: f64 = ");
                if let Some(start) = start {
                    out.push('(');
                    emit_expr(out, start, mode)?;
                    out.push(')');
                } else {
                    out.push_str("0.0f64");
                }
                out.push_str("; let mut __sc = 0.0f64; for &__sx in (");
                emit_expr(out, list, mode)?;
                out.push_str(").iter() { let __st = __ss + __sx; if __ss.abs() >= __sx.abs() { __sc += (__ss - __st) + __sx; } else { __sc += (__sx - __st) + __ss; } __ss = __st; } __ss + __sc }");
            } else {
                // PMAT-595: integer `sum` honors C-PY-INT-ARITH via a checked
                // fold seeded with `start` (matches the Rust backend).
                out.push('(');
                emit_expr(out, list, mode)?;
                out.push_str(").iter().fold(");
                if let Some(start) = start {
                    out.push('(');
                    emit_expr(out, start, mode)?;
                    out.push(')');
                } else {
                    out.push_str("0i64");
                }
                out.push_str(", |__a, &__x| __a.checked_add(__x).expect(\"xpile: i64 addition overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"))");
            }
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
        Expr::Repeat { seq, n, of_str } => {
            if *of_str {
                out.push('(');
                emit_expr(out, seq, mode)?;
                out.push_str(").repeat(((");
                emit_expr(out, n, mode)?;
                out.push_str(").max(0)) as usize)");
            } else {
                // PMAT-569: list repeat clones elements (see Rust twin).
                out.push_str("{ let __rep = ");
                emit_expr(out, seq, mode)?;
                out.push_str("; (0..(((");
                emit_expr(out, n, mode)?;
                out.push_str(").max(0)) as usize)).flat_map(|_| __rep.iter().cloned()).collect::<Vec<_>>() }");
            }
        }
        // PMAT-502m: `int(x)`/`float(x)` → `((x) as i64)` / `((x) as f64)`.
        Expr::NumCast {
            value,
            to_float,
            from_str,
            from_float,
        } => {
            // PMAT-502bf: string parse, matching the Rust backend.
            if *from_str && *to_float {
                // PMAT-611: float(s) accepts PEP 515 underscores between digits
                // (matches the Rust backend). Bind a reference so a temporary
                // operand survives the block via lifetime extension (E0716).
                out.push_str("{ let __pf = &(");
                emit_expr(out, value, mode)?;
                out.push_str("); let __ps = __pf.trim(); let __pe = __ps.as_bytes(); if !__ps.bytes().enumerate().all(|(__k, __c)| __c != b'_' || (__k > 0 && __pe[__k - 1].is_ascii_digit() && __k + 1 < __pe.len() && __pe[__k + 1].is_ascii_digit())) { panic!(\"xpile: ValueError: could not convert string to float\"); } __ps.replace('_', \"\").parse::<f64>().expect(\"xpile: ValueError: could not convert string to float\") }");
            } else if *from_str {
                // PMAT-610: int(s) accepts PEP 515 underscores between digits
                // (matches the Rust backend). Bind a reference so a temporary
                // operand survives the block via lifetime extension (E0716).
                out.push_str("{ let __pf = &(");
                emit_expr(out, value, mode)?;
                out.push_str("); let __ps = __pf.trim(); let __pb = __ps.strip_prefix('-').or_else(|| __ps.strip_prefix('+')).unwrap_or(__ps); if __pb.starts_with('_') || __pb.ends_with('_') || __pb.contains(\"__\") { panic!(\"xpile: ValueError: invalid literal for int()\"); } __ps.replace('_', \"\").parse::<i64>().expect(\"xpile: ValueError: invalid literal for int()\") }");
            } else if !*to_float && *from_float {
                // PMAT-586: `int(float_x)` guards a non-finite source (see Rust twin).
                out.push_str("{ let __ic = ");
                emit_expr(out, value, mode)?;
                out.push_str("; if !__ic.is_finite() { panic!(\"xpile: int() of a non-finite float (Python OverflowError/ValueError)\"); } if __ic < (i64::MIN as f64) || __ic >= (i64::MAX as f64) { panic!(\"xpile: int() out of i64 range; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"); } __ic as i64 }");
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
                // PMAT-583: match CPython's float repr (sci notation when exp
                // `< -4` or `>= 16`) — see the Rust backend's twin.
                out.push_str("{ let __sf = ");
                emit_expr(out, value, mode)?;
                out.push_str(
                    r#"; if __sf.is_nan() { String::from("nan") } else if __sf.is_infinite() { String::from(if __sf < 0.0 { "-inf" } else { "inf" }) } else { let __se = format!("{:e}", __sf); let __ep = __se.find('e').unwrap(); let __ex: i32 = __se[__ep + 1..].parse().unwrap(); if __ex < -4 || __ex >= 16 { format!("{}e{}{:02}", &__se[..__ep], if __ex < 0 { "-" } else { "+" }, __ex.abs()) } else if __sf.fract() == 0.0 { format!("{}.0", __sf) } else { format!("{}", __sf) } } }"#,
                );
            } else {
                out.push_str("format!(\"{}\", ");
                emit_expr(out, value, mode)?;
                out.push(')');
            }
        }
        // PMAT-582: `repr(str)` — CPython-style quoted form (see Rust twin).
        Expr::ReprStr { value } => {
            out.push_str("{ let __rs = &(");
            emit_expr(out, value, mode)?;
            out.push_str(
                r#"); let __q = if __rs.contains('\'') && !__rs.contains('"') { '"' } else { '\'' }; let mut __ro = String::new(); __ro.push(__q); for __rc in __rs.chars() { match __rc { '\\' => { __ro.push('\\'); __ro.push('\\'); } '\n' => { __ro.push('\\'); __ro.push('n'); } '\r' => { __ro.push('\\'); __ro.push('r'); } '\t' => { __ro.push('\\'); __ro.push('t'); } __ec if __ec == __q => { __ro.push('\\'); __ro.push(__ec); } __ec => __ro.push(__ec) } } __ro.push(__q); __ro }"#,
            );
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
        // PMAT-612: `round(int, n)` → int (banker's rounding for n < 0, identity
        // for n >= 0; i128 arithmetic, fails loud out of i64 range). Mirrors the
        // Rust backend.
        Expr::RoundIntToDigits { value, ndigits } => {
            out.push_str("{ let __rv = (");
            emit_expr(out, value, mode)?;
            out.push_str(") as i128; let __rn = (");
            emit_expr(out, ndigits, mode)?;
            out.push_str("); if __rn >= 0 { __rv as i64 } else { let __rp = 10i128.checked_pow((-__rn) as u32).expect(\"xpile: OverflowError: round() scale out of range\"); let __rd = __rv.div_euclid(__rp); let __rm = __rv.rem_euclid(__rp); let __r2 = 2i128 * __rm; let __res = if __r2 < __rp { __rd * __rp } else if __r2 > __rp { (__rd + 1) * __rp } else if __rd % 2 == 0 { __rd * __rp } else { (__rd + 1) * __rp }; if __res < (i64::MIN as i128) || __res > (i64::MAX as i128) { panic!(\"xpile: OverflowError: round() result out of i64 range\"); } __res as i64 } }");
        }
        // PMAT-502e/h/aa: 1-arg `min(xs)`/`max(xs)`; `key=lambda` →
        // `min_by_key`/`max_by_key`.
        Expr::ListMinMax {
            list,
            is_max,
            of_float,
            key,
            default,
        } => {
            // PMAT-502dh: an optional `default` → `.unwrap_or(<default>)` on
            // the empty case; the float branch uses `.reduce(..)` with default.
            emit_expr(out, list, mode)?;
            match key {
                Some(k) => {
                    // PMAT-568: Python max(key=) returns the FIRST maximal element
                    // (Rust max_by_key returns the last) — reverse first. min ok.
                    if *is_max {
                        write!(
                            out,
                            ".iter().cloned().rev().max_by_key(|__k| {{ let {} = __k.clone(); ",
                            k.param
                        )?;
                    } else {
                        write!(
                            out,
                            ".iter().cloned().min_by_key(|__k| {{ let {} = __k.clone(); ",
                            k.param
                        )?;
                    }
                    emit_expr(out, &k.body, mode)?;
                    out.push_str(" })");
                }
                None => match *of_float {
                    // PMAT-502er: `.cloned()` (not `.copied()`) so non-Copy
                    // `String` min/max works too; i64/bool are `Clone`.
                    false => out.push_str(if *is_max {
                        ".iter().cloned().max()"
                    } else {
                        ".iter().cloned().min()"
                    }),
                    // PMAT-608: float min/max = first-arg-wins reduce (matches
                    // the Rust backend); empty → Option (ValueError, not ±∞).
                    true => {
                        let cmp = if *is_max { ">" } else { "<" };
                        write!(
                            out,
                            ".iter().copied().reduce(|__a, __b| if __b {cmp} __a {{ __b }} else {{ __a }})"
                        )?;
                    }
                },
            }
            match default {
                Some(d) => {
                    out.push_str(".unwrap_or(");
                    emit_expr(out, d, mode)?;
                    out.push(')');
                }
                None if *of_float => out.push_str(
                    ".expect(\"xpile: max()/min() of an empty sequence (Python ValueError)\")",
                ),
                None => out.push_str(".unwrap()"),
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
        // PMAT-570: a negative-resolved index (`len-k`) references the receiver —
        // bind it before remove() (E0502). Positive indices keep the inline form.
        Expr::ListPop { list, index } => match index {
            None => {
                out.push('(');
                emit_expr(out, list, mode)?;
                out.push_str(").pop().unwrap()");
            }
            Some(i) => {
                let refs_self =
                    matches!(list.as_ref(), Expr::Ident(n) if expr_mentions_ident(i, n));
                if refs_self {
                    out.push_str("{ let __pi = (");
                    emit_expr(out, i, mode)?;
                    out.push_str(") as usize; (");
                    emit_expr(out, list, mode)?;
                    out.push_str(").remove(__pi) }");
                } else {
                    out.push('(');
                    emit_expr(out, list, mode)?;
                    out.push_str(").remove((");
                    emit_expr(out, i, mode)?;
                    out.push_str(") as usize)");
                }
            }
        },
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
        Expr::Sorted {
            list,
            reverse,
            key,
            of_float,
        } => {
            out.push_str("{ let mut __xv = ");
            emit_expr(out, list, mode)?;
            out.push_str(".clone(); __xv.");
            // PMAT-568: reverse=True + key must be STABLE descending (see Rust twin).
            // PMAT-578: keyless float sort uses `sort_by(partial_cmp)` (no `Ord`).
            // PMAT-616: NaN-safe — fall back to `Equal` (Python doesn't raise on NaN).
            match (key, *reverse) {
                (None, false) if *of_float => {
                    out.push_str("sort_by(|__a, __b| __a.partial_cmp(__b).unwrap_or(std::cmp::Ordering::Equal));");
                }
                (None, true) if *of_float => {
                    out.push_str("sort_by(|__a, __b| __b.partial_cmp(__a).unwrap_or(std::cmp::Ordering::Equal));");
                }
                (None, false) => out.push_str("sort();"),
                (None, true) => out.push_str("sort(); __xv.reverse();"),
                // PMAT-603: float key → partial_cmp (no Ord); matches Rust twin.
                (Some(k), false) if *of_float => {
                    write!(
                        out,
                        "sort_by(|__a, __b| {{ let {p} = __a.clone(); ",
                        p = k.param
                    )?;
                    emit_expr(out, &k.body, mode)?;
                    write!(
                        out,
                        " }}.partial_cmp(&{{ let {p} = __b.clone(); ",
                        p = k.param
                    )?;
                    emit_expr(out, &k.body, mode)?;
                    out.push_str(" }).unwrap_or(std::cmp::Ordering::Equal));");
                }
                (Some(k), false) => {
                    write!(out, "sort_by_key(|__k| {{ let {} = __k.clone(); ", k.param)?;
                    emit_expr(out, &k.body, mode)?;
                    out.push_str(" });");
                }
                (Some(k), true) => {
                    write!(
                        out,
                        "sort_by(|__a, __b| {{ let __ka = {{ let {p} = __a.clone(); ",
                        p = k.param
                    )?;
                    emit_expr(out, &k.body, mode)?;
                    write!(
                        out,
                        " }}; let __kb = {{ let {p} = __b.clone(); ",
                        p = k.param
                    )?;
                    emit_expr(out, &k.body, mode)?;
                    if *of_float {
                        out.push_str(
                            " }; __kb.partial_cmp(&__ka).unwrap_or(std::cmp::Ordering::Equal) });",
                        );
                    } else {
                        out.push_str(" }; __kb.cmp(&__ka) });");
                    }
                }
            }
            out.push_str(" __xv }");
        }
        // PMAT-502d: `reversed(xs)` → a new reversed Vec.
        Expr::Reversed { list } => {
            out.push_str("{ let mut __xv = ");
            emit_expr(out, list, mode)?;
            out.push_str(".clone(); __xv.reverse(); __xv }");
        }
        // PMAT-549: `math.gcd(a, b)` → inline Euclidean algorithm (abs values).
        Expr::Gcd { a, b } => {
            out.push_str("{ let mut __ga = (");
            emit_expr(out, a, mode)?;
            out.push_str(").abs(); let mut __gb = (");
            emit_expr(out, b, mode)?;
            out.push_str(").abs(); while __gb != 0 { let __gt = __gb; __gb = __ga % __gb; __ga = __gt; } __ga }");
        }
        // PMAT-550: `math.lcm(a, b)` → `(abs(a)/gcd) * abs(b)` (0 if either is 0).
        Expr::Lcm { a, b } => {
            out.push_str("{ let __la = (");
            emit_expr(out, a, mode)?;
            out.push_str(").abs(); let __lb = (");
            emit_expr(out, b, mode)?;
            out.push_str(").abs(); if __la == 0 || __lb == 0 { 0 } else { let mut __ga = __la; let mut __gb = __lb; while __gb != 0 { let __gt = __gb; __gb = __ga % __gb; __ga = __gt; } (__la / __ga) * __lb } }");
        }
        // PMAT-551: `math.factorial(n)` → inline product loop (checked, n>=0).
        Expr::Factorial { n } => {
            out.push_str("{ let __nf = (");
            emit_expr(out, n, mode)?;
            out.push_str("); if __nf < 0 { panic!(\"xpile: ValueError: factorial() not defined for negative values\"); } let mut __f = 1i64; let mut __fi = 2i64; while __fi <= __nf { __f = __f.checked_mul(__fi).expect(\"xpile: i64 multiplication overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"); __fi += 1; } __f }");
        }
        // PMAT-552: `math.isqrt(n)` → exact integer Newton (no float).
        Expr::Isqrt { n } => {
            out.push_str("{ let __sn = (");
            emit_expr(out, n, mode)?;
            out.push_str("); if __sn < 0 { panic!(\"xpile: ValueError: isqrt() argument must be nonnegative\"); } if __sn == 0 { 0 } else { let mut __sx = 1i64 << ((64 - __sn.leading_zeros() + 1) / 2); loop { let __sy = (__sx + __sn / __sx) / 2; if __sy >= __sx { break; } __sx = __sy; } __sx } }");
        }
        // PMAT-553: `math.comb(n, k)` → incremental binomial product (k>n → 0).
        Expr::Comb { n, k } => {
            out.push_str("{ let __cn = (");
            emit_expr(out, n, mode)?;
            out.push_str("); let __ck = (");
            emit_expr(out, k, mode)?;
            out.push_str("); if __cn < 0 || __ck < 0 { panic!(\"xpile: ValueError: comb() arguments must be non-negative\"); } if __ck > __cn { 0 } else { let __ck2 = if __ck < __cn - __ck { __ck } else { __cn - __ck }; let mut __cr = 1i64; let mut __ci = 0i64; while __ci < __ck2 { __cr = __cr.checked_mul(__cn - __ci).expect(\"xpile: i64 multiplication overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\") / (__ci + 1); __ci += 1; } __cr } }");
        }
        // PMAT-554: `math.perm(n, k)` → descending product of k factors (k>n → 0).
        Expr::Perm { n, k } => {
            out.push_str("{ let __pn = (");
            emit_expr(out, n, mode)?;
            out.push_str("); let __pk = (");
            emit_expr(out, k, mode)?;
            out.push_str("); if __pn < 0 || __pk < 0 { panic!(\"xpile: ValueError: perm() arguments must be non-negative\"); } if __pk > __pn { 0 } else { let mut __pr = 1i64; let mut __pi = 0i64; while __pi < __pk { __pr = __pr.checked_mul(__pn - __pi).expect(\"xpile: i64 multiplication overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"); __pi += 1; } __pr } }");
        }
        // PMAT-571: `pow(base, exp, mod)` → modular exponentiation (see Rust twin).
        Expr::PowMod { base, exp, modulus } => {
            out.push_str("{ let __pmm = (");
            emit_expr(out, modulus, mode)?;
            out.push_str("); if __pmm == 0 { panic!(\"xpile: ValueError: pow() 3rd argument cannot be 0\"); } let __pme = (");
            emit_expr(out, exp, mode)?;
            out.push_str("); if __pme < 0 { panic!(\"xpile: ValueError: pow() 2nd argument cannot be negative when 3rd argument specified\"); } let __pmb0 = (");
            emit_expr(out, base, mode)?;
            // PMAT-619: modexp on the magnitude |m| (i128), sign-correct at the
            // end — matches the Rust backend (a negative modulus, esp. with a
            // negative base, previously gave the wrong sign/value).
            out.push_str("); let __pma = (__pmm as i128).abs(); let mut __pmb = { let __t = (__pmb0 as i128) % __pma; if __t < 0 { __t + __pma } else { __t } }; let mut __pmr = 1i128 % __pma; let mut __pmk = __pme; while __pmk > 0 { if __pmk & 1 == 1 { __pmr = (__pmr * __pmb) % __pma; } __pmk >>= 1; __pmb = (__pmb * __pmb) % __pma; } if __pmm < 0 && __pmr != 0 { __pmr -= __pma; } __pmr as i64 }");
        }
        // PMAT-502cj: `list(range(start, stop, step))` → a collected i64 range.
        Expr::RangeList { start, stop, step } => {
            if *step > 0 {
                out.push('(');
                emit_expr(out, start, mode)?;
                out.push_str("..");
                emit_expr(out, stop, mode)?;
                out.push(')');
                if *step != 1 {
                    write!(out, ".step_by({step}usize)")?;
                }
            } else {
                // PMAT-523: negative-step range (Ruchy → Rust).
                out.push_str("(((");
                emit_expr(out, stop, mode)?;
                out.push_str(") + 1)..=(");
                emit_expr(out, start, mode)?;
                out.push_str(")).rev()");
                let abs = -*step;
                if abs != 1 {
                    write!(out, ".step_by({abs}usize)")?;
                }
            }
            out.push_str(".collect::<Vec<i64>>()");
        }
        // PMAT-502cw: `set(xs)` → collect the list into a HashSet.
        Expr::SetFromList { list } => {
            emit_expr(out, list, mode)?;
            out.push_str(".iter().cloned().collect::<std::collections::HashSet<_>>()");
        }
        // PMAT-520: `list(<set>)` / `sorted(<set>)` → unique elements as a Vec.
        Expr::SetToList { set } => {
            emit_expr(out, set, mode)?;
            out.push_str(".iter().cloned().collect::<Vec<_>>()");
        }
        // PMAT-502dk: `dict(pairs)` → a HashMap from the list of 2-tuples.
        Expr::DictFromPairs { pairs } => {
            emit_expr(out, pairs, mode)?;
            out.push_str(".iter().cloned().collect::<std::collections::HashMap<_, _>>()");
        }
        // PMAT-502dw/dx: `{k: v, **d, …}` → chain each fragment's iterator into
        // a fresh HashMap (a later entry wins, matching Python).
        Expr::DictMerge { entries } => {
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(".chain(");
                }
                match k {
                    Some(key) => {
                        out.push_str("std::iter::once((");
                        emit_expr(out, key, mode)?;
                        out.push_str(", ");
                        emit_expr(out, v, mode)?;
                        out.push_str("))");
                    }
                    None => {
                        out.push('(');
                        emit_expr(out, v, mode)?;
                        out.push_str(").iter().map(|(__k, __v)| (__k.clone(), __v.clone()))");
                    }
                }
                if i > 0 {
                    out.push(')');
                }
            }
            out.push_str(".collect::<std::collections::HashMap<_, _>>()");
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
            // PMAT-502ej: parenthesize a block-producing collection
            // (`sorted(...)`/`reversed(...)`/block-expr) so `{block}[i]` doesn't
            // mis-parse — matching the Rust backend.
            let mut coll = String::new();
            emit_expr(&mut coll, collection, mode)?;
            if coll.trim_start().starts_with('{') {
                write!(out, "({coll})")?;
            } else {
                out.push_str(&coll);
            }
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
        // PMAT-502ey: 1-arg `d.get(k)` → `(d).get(&(k)).cloned()` : Option<V>.
        Expr::DictGetOpt { dict, key } => {
            emit_expr(out, dict, mode)?;
            out.push_str(".get(&(");
            emit_expr(out, key, mode)?;
            out.push_str(")).cloned()");
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
        // PMAT-502ep: set predicate — matching the Rust backend.
        Expr::SetPred { lhs, op, rhs } => {
            out.push_str("({ let __l = ");
            emit_expr(out, lhs, mode)?;
            out.push_str("; let __r = ");
            emit_expr(out, rhs, mode)?;
            out.push_str("; ");
            out.push_str(match op {
                SetPredOp::Subset => "__l.is_subset(&__r)",
                SetPredOp::Superset => "__l.is_superset(&__r)",
                SetPredOp::Disjoint => "__l.is_disjoint(&__r)",
                SetPredOp::ProperSubset => "__l.is_subset(&__r) && __l != __r",
                SetPredOp::ProperSuperset => "__l.is_superset(&__r) && __l != __r",
            });
            out.push_str(" })");
        }
        // PMAT-502eq: `.copy()` → `(<inner>).clone()`, matching the Rust backend.
        Expr::Clone(inner) => {
            out.push('(');
            emit_expr(out, inner, mode)?;
            out.push_str(").clone()");
        }
        // PMAT-502ew: `Option` value — `None` / `Some(<e>)`, matching Rust.
        Expr::OptionExpr(inner) => match inner {
            None => out.push_str("None"),
            Some(e) => {
                out.push_str("Some(");
                emit_expr(out, e, mode)?;
                out.push(')');
            }
        },
        // PMAT-502ex: `x is None`/`is not None` → `.is_none()`/`.is_some()`.
        Expr::IsNone { value, negated } => {
            out.push('(');
            emit_expr(out, value, mode)?;
            out.push_str(if *negated {
                ").is_some()"
            } else {
                ").is_none()"
            });
        }
        // PMAT-502ez: a flow-narrowed Optional read → `(<inner>).unwrap()`.
        Expr::OptionUnwrap(inner) => {
            out.push('(');
            emit_expr(out, inner, mode)?;
            out.push_str(").unwrap()");
        }
        // PMAT-506b: struct construction `Name { f0: v0, … }` (Ruchy → Rust).
        Expr::StructLit { name, fields } => {
            out.push_str(name);
            out.push_str(" { ");
            for (i, (field, value)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write!(out, "{field}: ")?;
                emit_expr(out, value, mode)?;
            }
            out.push_str(" }");
        }
        // PMAT-506b: struct field read `(obj).field`.
        Expr::FieldAccess { obj, field } => {
            out.push('(');
            emit_expr(out, obj, mode)?;
            write!(out, ").{field}")?;
        }
        // PMAT-506d: struct method call `(obj).method(args)`.
        Expr::MethodCall { obj, method, args } => {
            out.push('(');
            emit_expr(out, obj, mode)?;
            write!(out, ").{method}(")?;
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit_expr(out, a, mode)?;
            }
            out.push(')');
        }
        // PMAT-513: an enum member access `C::NAME`.
        Expr::EnumVariant { enum_name, variant } => write!(out, "{enum_name}::{variant}")?,
        // PMAT-503b: try/except → catch_unwind match (Ruchy compiles to Rust).
        Expr::TryCatch { body, handler } => {
            out.push_str("match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| ");
            emit_expr(out, body, mode)?;
            out.push_str(")) { Ok(__xpile_try) => __xpile_try, Err(_) => ");
            emit_expr(out, handler, mode)?;
            out.push_str(" }");
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
        // PMAT-502fb: Python `~x` == Rust `!x` on a signed integer.
        UnOp::BitNot => {
            write!(out, "(!(")?;
            emit_expr(out, operand, mode)?;
            write!(out, "))")?;
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
        // PMAT-538: euclidean div/rem only match Python `//`/`%` for a positive
        // divisor; emit the truncating quotient/remainder with a floor
        // correction (mirrors the Rust backend).
        BinOp::FloorDiv => emit_floor_div(out, lhs, rhs, mode),
        BinOp::Mod => emit_floor_mod(out, lhs, rhs, mode),
        // PMAT-618: `d.get(k) == v` / `!= v` — wrap the bare-value side in
        // `Some(...)` so a no-default `d.get` (`Option<T>`) compares as
        // `Option<T> == Some(v)`, matching Python (`None == v` is False).
        // Matches the Rust backend.
        BinOp::Eq if is_dict_get_opt(lhs) ^ is_dict_get_opt(rhs) => {
            emit_opt_eq(out, lhs, " == ", rhs, mode)
        }
        BinOp::NotEq if is_dict_get_opt(lhs) ^ is_dict_get_opt(rhs) => {
            emit_opt_eq(out, lhs, " != ", rhs, mode)
        }
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
    // PMAT-575: see the Rust backend's twin — `checked_shl` only guards the
    // shift amount, not value overflow (`1 << 63` wraps to i64::MIN silently),
    // falsifying C-PY-INT-ARITH. Emit a reversibility check for left-shift.
    if method == "checked_shl" && !mode {
        write!(out, "{{ let __shl_v: i64 = ")?;
        emit_expr(out, lhs, mode)?;
        write!(out, "; let __shl_n: u32 = u32::try_from(")?;
        emit_expr(out, rhs, mode)?;
        write!(
            out,
            ").expect(\"xpile: shift amount out of range for u32 (contract C-PY-INT-ARITH)\"); let __shl_r = __shl_v.checked_shl(__shl_n).expect(\"xpile: i64 left-shift overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"); if (__shl_r >> __shl_n) != __shl_v {{ panic!(\"xpile: i64 left-shift overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"); }} __shl_r }}"
        )?;
        return Ok(());
    }
    // PMAT-577: see the Rust backend's twin — Python `x >> n` saturates to the
    // sign fill for n >= 64 (0 / -1), but `checked_shr` panics; clamp to 63.
    if method == "checked_shr" && !mode {
        write!(out, "{{ let __shr_v: i64 = ")?;
        emit_expr(out, lhs, mode)?;
        write!(out, "; let __shr_n: i64 = ")?;
        emit_expr(out, rhs, mode)?;
        write!(
            out,
            "; if __shr_n < 0 {{ panic!(\"xpile: negative shift amount (Python ValueError: negative shift count; contract C-PY-INT-ARITH)\"); }} let __shr_amt: u32 = if __shr_n >= 64 {{ 63 }} else {{ __shr_n as u32 }}; __shr_v >> __shr_amt }}"
        )?;
        return Ok(());
    }
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

/// PMAT-538: Python floor-division `a // b` for i64 — truncating quotient with a
/// floor correction (Python floors toward −∞; `div_euclid` diverges for a
/// negative divisor). Mirrors the Rust backend.
fn emit_floor_div(
    out: &mut String,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
    let panic_msg = "xpile: i64 floor-div overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented";
    write!(out, "{{ let __fa = ")?;
    emit_expr(out, lhs, mode)?;
    write!(out, "; let __fb = ")?;
    emit_expr(out, rhs, mode)?;
    write!(
        out,
        "; let __q = __fa.checked_div(__fb).expect(\"{panic_msg}\"); \
         let __r = __fa.checked_rem(__fb).expect(\"{panic_msg}\"); \
         if __r != 0 && (__r < 0) != (__fb < 0) {{ __q - 1 }} else {{ __q }} }}"
    )?;
    Ok(())
}

/// PMAT-538: Python modulo `a % b` for i64 — truncating remainder with a floor
/// correction (Python's `%` takes the sign of the divisor). Mirrors the Rust
/// backend.
fn emit_floor_mod(
    out: &mut String,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
    let panic_msg = "xpile: i64 modulo overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented";
    write!(out, "{{ let __fa = ")?;
    emit_expr(out, lhs, mode)?;
    write!(out, "; let __fb = ")?;
    emit_expr(out, rhs, mode)?;
    write!(
        out,
        "; let __r = __fa.checked_rem(__fb).expect(\"{panic_msg}\"); \
         if __r != 0 && (__r < 0) != (__fb < 0) {{ __r + __fb }} else {{ __r }} }}"
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

/// PMAT-618: is this a no-default `d.get(k)` (an `Option<T>`)?
fn is_dict_get_opt(e: &Expr) -> bool {
    matches!(e, Expr::DictGetOpt { .. })
}

/// PMAT-618: `==`/`!=` where exactly one operand is a no-default `d.get(k)`;
/// the bare-value side is wrapped in `Some(...)` (matches the Rust backend).
fn emit_opt_eq(
    out: &mut String,
    lhs: &Expr,
    op: &str,
    rhs: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
    write!(out, "(")?;
    emit_opt_eq_operand(out, lhs, mode)?;
    out.push_str(op);
    emit_opt_eq_operand(out, rhs, mode)?;
    write!(out, ")")?;
    Ok(())
}

fn emit_opt_eq_operand(out: &mut String, e: &Expr, mode: bool) -> Result<(), RuchyCodegenError> {
    if is_dict_get_opt(e) {
        emit_expr(out, e, mode)
    } else {
        out.push_str("Some(");
        emit_expr(out, e, mode)?;
        out.push(')');
        Ok(())
    }
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
    fn ruchy_floordiv_uses_floor_correction() {
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
        // PMAT-538: floor correction, not div_euclid (wrong for a neg divisor).
        assert!(ruchy.contains("checked_div") && ruchy.contains("__q - 1"));
        assert!(!ruchy.contains("div_euclid"));
    }
}
