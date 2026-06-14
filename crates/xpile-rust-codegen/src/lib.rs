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
    NumBuiltinOp, Param, Radix, SetOp, SetPredOp, SourceLang, Stmt, StrMethodOp, Type, UnOp,
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
        // FloorDiv/Mod/Pow + the method-style math ops are emitted via
        // dedicated formulas, never via this helper — keep the match exhaustive.
        FloatOp::FloorDiv => "//",
        FloatOp::Mod => "%",
        FloatOp::Pow => "**",
        FloatOp::Hypot => "hypot",
        FloatOp::Atan2 => "atan2",
        FloatOp::Log => "log",
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
    // PMAT-573: escape Rust-keyword identifiers (`type`/`match`/`loop`/…) on
    // a cloned IR before emission, so a Python local/param/function named
    // after a Rust keyword produces valid Rust. Rewriting the data once (at
    // every binding AND reference together) keeps the two from drifting.
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
            // PMAT-505a (classes epic, first cut): dataclass → derived struct.
            Item::Struct {
                name,
                fields,
                methods,
                frozen,
            } => {
                // PMAT-592: a frozen dataclass is hashable in Python, so it may
                // be a dict key / set element — derive `Eq, Hash` (else E0277/
                // E0599). Only when every field type is itself `Eq + Hash`
                // (`i64`/`bool`/`String`); a float field disqualifies it (`f64`
                // is neither `Eq` nor `Hash`).
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
                // PMAT-506d: instance methods → an `impl` block.
                if !methods.is_empty() {
                    writeln!(out, "impl {name} {{")?;
                    for m in methods {
                        emit_function(&mut out, m)?;
                    }
                    out.push_str("}\n");
                }
            }
            // PMAT-513: a Python `Enum` class → a Rust enum. The discriminants
            // are tracked in the IR but `C.NAME.value` lowers to its literal at
            // the frontend, so the emitted enum needs no explicit `= disc`.
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
            | Stmt::ForEachPair { body, .. }
            | Stmt::ForEachZip3 { body, .. } => body.iter().any(stmt_has_bigint),
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
            | Stmt::ListInsert { .. }
            | Stmt::ListRemoveValue { .. } => false,
            // PMAT-461: indexed assignment same disposition.
            Stmt::IndexAssign { .. } => false,
            // PMAT-533: subscript-receiver append carries no Type::Let.
            Stmt::IndexAppend { .. } => false,
            // PMAT-466: dict keyed assignment carries no Type::Let;
            // dict values are int/bool/str at v0.2.0, never BigInt.
            Stmt::DictSet { .. } => false,
            // PMAT-506c: field assignment introduces no binding (no Type::Let).
            Stmt::FieldAssign { .. } => false,
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
            // PMAT-598: a mutable empty `set()` binding must NOT pin its element
            // type to the guessed-default `HashSet<i64>` — when the set is later
            // `.add()`ed a non-int element (a struct, str, …) the annotation is
            // a lie (E0308). Suppress the explicit annotation so rustc infers
            // the element type from the subsequent `.insert(...)`. Sound only
            // for an empty `SetLit` (its value is a bare `HashSet::new()`, no
            // turbofish) that is mutable (⟹ a later insert/reassign rustc can
            // infer from) and still typed at the guessed `Set(I64)` default
            // (an explicit `set[str]`/`set[T]` annotation is already correct).
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
                    // PMAT-595: the offset add honors C-PY-INT-ARITH (a bare
                    // `+ start` silently wraps for a start near i64::MAX).
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
        // PMAT-562: three-way `zip` → left-nested `.zip()` chain with a nested
        // `((a, b), c)` destructure. `.iter().cloned()` on each (non-consuming,
        // like the 2-way `Zip`); stops at the shortest, matching Python `zip`.
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
        // PMAT-616: a NaN element makes `partial_cmp` return `None`; Python's
        // `sort` does NOT raise on NaN (it produces an undefined-but-non-crashing
        // order), so fall back to `Equal` instead of `.unwrap()` panicking.
        // Identical to `.unwrap()` for all finite floats.
        Stmt::ListMutate {
            list_name,
            op,
            of_float,
        } => {
            match op {
                ListMutateOp::Sort if *of_float => writeln!(
                    out,
                    "{indent}{list_name}.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));"
                )?,
                ListMutateOp::Sort => writeln!(out, "{indent}{list_name}.sort();")?,
                // PMAT-555: descending in-place sort (`sort(reverse=True)`) — a
                // reversed comparator.
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
        // PMAT-502ar / PMAT-590: `xs.insert(i, x)` clamps the index to
        // CPython `list.insert` semantics (listobject.c `ins1`) instead of
        // emitting a bare `as usize` cast. Python clamps any `i > len` to
        // `len` (append) and normalizes a negative `i` to `len + i`, clamping
        // to `0` if still negative — whereas Rust's `Vec::insert` panics for
        // `i > len` and a negative `i` casts to a huge `usize` that also
        // panics. The clamp block restores parity.
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
        // PMAT-502eg: `xs.remove(x)` → find the first equal element and
        // remove it, panicking (≈ Python `ValueError`) if it isn't present.
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
        // PMAT-461 (v0.2.0 Track 1.B): Python `xs[i] = v` → Rust
        // `xs[i as usize] = v;`. Same `as usize` coercion as
        // Expr::Index; same param-mut threading as ListAppend.
        Stmt::IndexAssign {
            list_name,
            indices,
            value,
        } => {
            // PMAT-502dy: a multi-element path is nested list indexing
            // (`grid[i][j] = v`) — each index is `usize`-coerced.
            // PMAT-560: when an index references the receiver (e.g. the
            // negative-index desugar `xs[len(xs) - k] = v`), the index's
            // immutable borrow of `xs` conflicts with `index_mut`'s mutable
            // borrow (E0502: `xs[xs.len() - 1] = v` doesn't compile). Bind each
            // index to a temp FIRST, then assign — only when needed, so the
            // common plain-index shape (`xs[i as usize] = v`) is unchanged.
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
        // PMAT-533: append on a subscript receiver. List base indexes a
        // mutable place directly (`base[(i) as usize].push(e)`); dict base
        // reaches the value via `get_mut(&(k)).unwrap()` (panic on absent
        // key = Python KeyError).
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
        // PMAT-502at: Python `del coll[key]`. list → `coll.remove((k) as
        // usize);` (shift tail left; panics past end = Python IndexError);
        // dict → `coll.remove(&(k));` (discards the value).
        Stmt::DelItem { name, key, is_dict } => {
            if *is_dict {
                write!(out, "{indent}{name}.remove(&(")?;
                emit_expr(out, key, mode)?;
                writeln!(out, "));")?;
            } else if expr_mentions_ident(key, name) {
                // PMAT-570: `del xs[-k]` → `xs.remove(len(xs) - k)`; the index
                // references `xs`, so bind it before the mutable `remove`.
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
    // PMAT-506d: a method's `self` receiver emits as `&self` (read-only first
    // cut) — never `self: StructName`.
    if p.name == "self" {
        out.push_str("&self");
        return Ok(());
    }
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
        // PMAT-502ew: Python `Optional[T]` → Rust `Option<T>`.
        Type::Optional(inner) => {
            out.push_str("Option<");
            emit_type(out, inner)?;
            out.push('>');
        }
        // PMAT-506b: a struct-typed value emits the bare struct name.
        Type::Struct(name) => out.push_str(name),
    }
    Ok(())
}

/// PMAT-560: does `e` reference the identifier `name`? Used by `IndexAssign` to
/// detect a self-referential index (e.g. the `xs[len(xs) - k]` negative-index
/// desugar), whose immutable borrow of the receiver conflicts with the
/// `index_mut` mutable borrow — such an index is bound to a temp first.
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
        // PMAT-609: recurse into conditional + block forms so a normalized pop
        // index that references the receiver (`{ let __pidx = i; if __pidx < 0 {
        // recv.len() + __pidx } else { __pidx } }`) is detected as
        // self-referential (must be bound before the mutable `remove`).
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            expr_mentions_ident(cond, name)
                || expr_mentions_ident(then_expr, name)
                || expr_mentions_ident(else_expr, name)
        }
        Expr::Block(b) => {
            b.stmts
                .iter()
                .any(|s| matches!(s, Stmt::Let { value, .. } if expr_mentions_ident(value, name)))
                || expr_mentions_ident(&b.trailing_return, name)
        }
        _ => false,
    }
}

fn emit_expr(out: &mut String, e: &Expr, mode: bool) -> Result<(), CodegenError> {
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
            // PMAT-614: Python float floor-division `a // b` is CPython
            // `float_divmod` (Objects/floatobject.c), NOT `(a / b).floor()`.
            // The naive floor over-rounds whenever `a / b` lands just below an
            // integer in float (`1.0 // 0.1` is 9.0 in Python but
            // `(1.0/0.1).floor()` is 10.0), and gives the wrong result for
            // infinite operands (`inf // 2` is `nan`, `-5.0 // inf` is `-1.0`).
            // Replicate CPython exactly: `mod = fmod(a, b)` (Rust `%` IS C
            // `fmod`), `div = (a - mod) / b`, nudge `div` down by 1 when the
            // remainder's sign differs from the divisor's, then `floor(div)`
            // with CPython's `div - floor > 0.5` round-up correction.
            // PMAT-581: guard the zero divisor (Python raises ZeroDivisionError,
            // not `inf`); both operands bound to temps (evaluate-once).
            FloatOp::FloorDiv => {
                out.push_str("{ let __fa: f64 = ");
                emit_expr(out, lhs, mode)?;
                out.push_str("; let __fz: f64 = ");
                emit_expr(out, rhs, mode)?;
                out.push_str("; if __fz == 0.0 { panic!(\"xpile: ZeroDivisionError: float floor division by zero\"); } let __fm = __fa % __fz; let mut __fd = (__fa - __fm) / __fz; if __fm != 0.0 && ((__fz < 0.0) != (__fm < 0.0)) { __fd -= 1.0; } let __ffl = __fd.floor(); if __fd - __ffl > 0.5 { __ffl + 1.0 } else { __ffl } }");
            }
            // PMAT-591: Python float modulo `a % b` is CPython `float_rem`
            // (Objects/floatobject.c): `mod = fmod(a, b)` (Rust's `%` IS C
            // `fmod`), then if `mod != 0` adjust toward the divisor's sign
            // (`mod += b` when their signs differ), else `copysign(0.0, b)`.
            // The earlier floor formula `a - b*(a/b).floor()` (PMAT-502br)
            // introduced an extra rounding step → last-ULP divergence on
            // ~60% of non-power-of-two divisors, and always produced `+0.0`
            // for a zero remainder, losing CPython's divisor-signed zero.
            // PMAT-581: guard the zero divisor (Python raises ZeroDivisionError,
            // not `nan`); bind both operands to temps (evaluate-once).
            FloatOp::Mod => {
                out.push_str("{ let __fz: f64 = ");
                emit_expr(out, rhs, mode)?;
                out.push_str("; if __fz == 0.0 { panic!(\"xpile: ZeroDivisionError: float modulo\"); } let __fn: f64 = ");
                emit_expr(out, lhs, mode)?;
                out.push_str("; let __r = __fn % __fz; if __r != 0.0 { if (__fz < 0.0) != (__r < 0.0) { __r + __fz } else { __r } } else { 0.0_f64.copysign(__fz) } }");
            }
            // PMAT-502bt/em/en: method-style float ops — `(a).<method>(b)`.
            // Pow → powf; the 2-arg math functions hypot/atan2/log map 1:1.
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
            // PMAT-581: float `/` (and int true-division, which lowers to a
            // float Div) raises ZeroDivisionError in Python, not `inf` — guard
            // the divisor.
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
            // PMAT-502dp: prefix (`0x`/`0o`/`0b`) only when `prefixed`; the
            // hex spec is `{:X}` when `upper`.
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
                out.push_str(").chars().all(|__c| ");
                out.push_str(match op {
                    StrMethodOp::IsDigit => "__c.is_ascii_digit()",
                    StrMethodOp::IsAlpha => "__c.is_alphabetic()",
                    StrMethodOp::IsAlnum => "__c.is_alphanumeric()",
                    // PMAT-600: Python `str.isspace()` also treats the C0
                    // information separators FS/GS/RS/US (U+001C..U+001F) as
                    // whitespace, which Rust's `char::is_whitespace()` excludes.
                    _ => "(__c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}'))",
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
            } else if matches!(
                op,
                StrMethodOp::Find
                    | StrMethodOp::Rfind
                    | StrMethodOp::StrIndex
                    | StrMethodOp::RIndex
            ) {
                // PMAT-566: `.find/.rfind/.index/.rindex` must return a Python
                // CHARACTER index, not Rust's byte offset. Bind the receiver to a
                // temp (single eval), find the byte offset, then count the chars
                // before it (`__s[..__b].chars().count()`) — `__b` is always a
                // char boundary since it's a match start. `find`/`rfind` →
                // `unwrap_or(-1)`; `index`/`rindex` → `.expect(ValueError)`.
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
            } else {
                emit_expr(out, recv, mode)?;
                match op {
                    StrMethodOp::Upper => out.push_str(".to_uppercase()"),
                    StrMethodOp::Lower => out.push_str(".to_lowercase()"),
                    // PMAT-600: Python `strip()` removes the C0 separators
                    // U+001C..U+001F too (Rust `trim()` / `char::is_whitespace`
                    // does not) — trim against the Python whitespace predicate.
                    StrMethodOp::Strip => out.push_str(".trim_matches(|__c: char| __c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}')).to_string()"),
                    // PMAT-564: `len(str)` → Unicode char count (not byte len).
                    StrMethodOp::CharCount => out.push_str(".chars().count() as i64"),
                    // PMAT-530: `s[::-1]` → reverse by Unicode scalar value.
                    StrMethodOp::Reverse => {
                        out.push_str(".chars().rev().collect::<String>()")
                    }
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
                    StrMethodOp::SplitN => {
                        out.push_str(".splitn(((");
                        emit_expr(out, &args[1], mode)?;
                        out.push_str(") as usize) + 1, &(");
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
                    // PMAT-502l: lstrip/rstrip → trim_start/trim_end.
                    // PMAT-600: against the Python whitespace set (incl. the C0
                    // separators U+001C..U+001F).
                    StrMethodOp::LStrip => out.push_str(".trim_start_matches(|__c: char| __c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}')).to_string()"),
                    StrMethodOp::RStrip => out.push_str(".trim_end_matches(|__c: char| __c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}')).to_string()"),
                    // PMAT-502l: `.count(sub)` → non-overlapping match count (i64).
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
        // PMAT-496/539: Python `xs[lo:hi]` slice with full Python bound
        // semantics — a negative bound counts from the end (`+len`), every
        // bound clamps to `[0, len]`, and `lo > hi` yields empty. The naive
        // `(lo) as usize` panicked on a negative bound (wraps to a huge usize)
        // or an out-of-range bound. Emit a block that binds the collection,
        // resolves + clamps each bound, then slices.
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
             -> Result<(), CodegenError> {
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
            // PMAT-567: a str slice indexes by Unicode CHARACTERS, not bytes —
            // collect to `Vec<char>` so `__n` (char count), the bound clamping,
            // and `__sl[__lo..__hi]` are all char-based (a byte slice gives wrong
            // results AND panics on a char boundary for non-ASCII input). A list
            // slice keeps the by-reference `&Vec` (element-indexed, already right).
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
                // PMAT-548: a negative list step `xs[::-k]` reverses then steps
                // (the frontend only sets a negative `step` for the unbounded
                // form, so `__lo..__hi` spans the whole list).
                Some(s) if *s < 0 => {
                    let k = (-s) as usize;
                    write!(
                        out,
                        "__sl[__lo..__hi].iter().rev().step_by({k}).cloned().collect::<Vec<_>>() }}"
                    )?;
                }
                // PMAT-502bc: positive list step → `.iter().step_by(c)
                // .cloned().collect::<Vec<_>>()` (str steps are rejected
                // in the frontend, so `step` is only ever set for lists).
                Some(s) => {
                    write!(
                        out,
                        "__sl[__lo..__hi].iter().step_by({s}).cloned().collect::<Vec<_>>() }}"
                    )?;
                }
                None => out.push_str(if *of_str {
                    // PMAT-567: `__sl` is `Vec<char>` for str — collect the slice
                    // back into a String.
                    "__sl[__lo..__hi].iter().collect::<String>() }"
                } else {
                    "__sl[__lo..__hi].to_vec() }"
                }),
            }
        }
        // PMAT-498: scalar numeric builtins → receiver-method form.
        Expr::NumBuiltin { op, args, of_float } => {
            // PMAT-601: float `max`/`min` must follow Python's first-argument-
            // wins semantics (and NaN propagation), NOT `f64::max`/`f64::min`
            // (which treat `+0.0 > -0.0` and silently drop NaN). Emit a left
            // fold: the accumulator starts at args[0]; a later arg replaces it
            // only on a STRICT compare, so a tie (`-0.0`/`0.0`) or a NaN compare
            // (always false) keeps the earlier value, exactly like Python's
            // `result = a; if b > result: result = b`. Integer min/max keep the
            // total-order `.min`/`.max` chain (i64 has no signed-zero/NaN).
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
            // PMAT-606: `math.floor`/`ceil`/`trunc` return a Python int, so the
            // f64 result is cast to i64. A bare `as i64` SATURATES (since Rust
            // 1.45): `1e30.floor() as i64` → i64::MAX (silent), `inf` → i64::MAX,
            // `nan` → 0 — but Python returns an exact bignum for a huge float and
            // raises OverflowError(inf)/ValueError(nan). Guard the rounded value
            // (finite + i64 range) and fail loud, mirroring the `int(float)`
            // guard (PMAT-586/589). The suffix arms below stay for match
            // exhaustiveness but are superseded by this guarded fast-path.
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
                // PMAT-579: `abs` of an i64 must be checked — `i64::MIN.abs()`
                // wraps to `i64::MIN` silently (no overflow under `-O`), which
                // falsifies C-PY-INT-ARITH (Python's `abs` is exact). An f64
                // `abs` never overflows, so it keeps `.abs()`.
                NumBuiltinOp::Abs if *of_float => out.push_str(".abs()"),
                NumBuiltinOp::Abs => out.push_str(
                    ".checked_abs().expect(\"xpile: i64 abs overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\")",
                ),
                // PMAT-502ek: math functions. `floor`/`ceil` return Python
                // `int`, so cast the f64 result to i64.
                NumBuiltinOp::Sqrt => out.push_str(".sqrt()"),
                NumBuiltinOp::Floor => out.push_str(".floor() as i64"),
                NumBuiltinOp::Ceil => out.push_str(".ceil() as i64"),
                // PMAT-502em: `math.trunc` — truncate toward zero, return int.
                NumBuiltinOp::Trunc => out.push_str(".trunc() as i64"),
                // PMAT-502el: trig / exp / log — 1-arg f64 → f64.
                NumBuiltinOp::Sin => out.push_str(".sin()"),
                NumBuiltinOp::Cos => out.push_str(".cos()"),
                NumBuiltinOp::Tan => out.push_str(".tan()"),
                NumBuiltinOp::Exp => out.push_str(".exp()"),
                NumBuiltinOp::Ln => out.push_str(".ln()"),
                NumBuiltinOp::Log10 => out.push_str(".log10()"),
                NumBuiltinOp::Log2 => out.push_str(".log2()"),
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
            // PMAT-584: CPython's float `sum()` uses Neumaier compensated
            // summation (Py3.12+) — naive left-to-right `.iter().sum()` diverges
            // on catastrophic cancellation (`sum([1.0, 1e16, 1.0, -1e16])` is
            // 2.0, not 0.0; `sum([0.1]*10)` is 1.0, not 0.9999999999999999).
            // Emit the same compensated fold, seeded with `start` (or 0.0). The
            // int case stays exact `.iter().sum::<i64>()` (with `start`).
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
                // PMAT-595: integer `sum(xs[, start])` must honor the
                // C-PY-INT-ARITH overflow contract like every other int-arith
                // path (`+`, `*`, abs, the shift trio) — a bare
                // `.iter().sum::<i64>()` silently wraps under `-O` (and panics
                // with a generic message in debug), bypassing the contract.
                // Emit a checked fold seeded with `start` (or 0) that fails
                // loud, folding the start in so the seed is also contract-safe.
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
        // PMAT-502k: `seq * n` → `(seq).repeat(((n).max(0)) as usize)`
        // (str → String, slice → Vec; negative count clamps to empty).
        Expr::Repeat { seq, n, of_str } => {
            if *of_str {
                // `String::repeat` — no `Copy` bound, unchanged.
                out.push('(');
                emit_expr(out, seq, mode)?;
                out.push_str(").repeat(((");
                emit_expr(out, n, mode)?;
                out.push_str(").max(0)) as usize)");
            } else {
                // PMAT-569: a list repeat clones its elements (slice `repeat`
                // needs `T: Copy`, which fails for `[[0]] * n` etc.). Works for
                // any `Clone` element; `.max(0)` clamps a negative count.
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
            if *from_str && *to_float {
                // PMAT-502bf: `float(s)` → trimmed `.parse()` (panics on bad
                // input, matching Python's `ValueError`).
                // PMAT-611: Python `float()` also accepts PEP 515 underscores
                // BETWEEN digits (`float("1_000.5")` == 1000.5), which Rust's
                // `parse::<f64>()` rejects → panic. Validate that every `_` has an
                // ASCII digit on both sides (the exact Python rule, covering the
                // fractional/exponent parts), then strip + parse; invalid
                // placements (`1_.5`, `1.5_`, `1_e5`, `_1.0`) still raise.
                // Bind a *reference* (not the value) so a temporary-String operand
                // (`float("inf")`) survives the block via temporary lifetime
                // extension, and a reused variable operand is not moved (E0716).
                out.push_str("{ let __pf = &(");
                emit_expr(out, value, mode)?;
                out.push_str("); let __ps = __pf.trim(); let __pe = __ps.as_bytes(); if !__ps.bytes().enumerate().all(|(__k, __c)| __c != b'_' || (__k > 0 && __pe[__k - 1].is_ascii_digit() && __k + 1 < __pe.len() && __pe[__k + 1].is_ascii_digit())) { panic!(\"xpile: ValueError: could not convert string to float\"); } __ps.replace('_', \"\").parse::<f64>().expect(\"xpile: ValueError: could not convert string to float\") }");
            } else if *from_str {
                // PMAT-610: `int(s)` accepts PEP 515 underscore digit separators
                // (`int(\"1_000\") == 1000`), which Rust's `parse::<i64>()` rejects
                // → panic. Python allows a single `_` only BETWEEN digits, so for
                // an int (digits-only body after an optional sign) that is exactly
                // "no leading/trailing/doubled underscore"; validate that, then
                // strip the separators and parse. Invalid placements (or any
                // other bad literal) still panic ≈ Python `ValueError`.
                // Bind a *reference* (not the value) so a temporary-String operand
                // (`int("1_000")`) survives the block via temporary lifetime
                // extension, and a reused variable operand is not moved (E0716).
                out.push_str("{ let __pf = &(");
                emit_expr(out, value, mode)?;
                out.push_str("); let __ps = __pf.trim(); let __pb = __ps.strip_prefix('-').or_else(|| __ps.strip_prefix('+')).unwrap_or(__ps); if __pb.starts_with('_') || __pb.ends_with('_') || __pb.contains(\"__\") { panic!(\"xpile: ValueError: invalid literal for int()\"); } __ps.replace('_', \"\").parse::<i64>().expect(\"xpile: ValueError: invalid literal for int()\") }");
            } else if !*to_float && *from_float {
                // PMAT-586/589: `int(float_x)` — Python raises `OverflowError`
                // for `int(inf)` and `ValueError` for `int(nan)`, and returns an
                // exact (arbitrary-precision) integer for an out-of-i64-range
                // finite float like `int(1e30)`; Rust's `as i64` saturates
                // (`inf`/huge → `i64::MAX`) / zeroes (`nan` → 0) silently. Guard
                // both a non-finite source and an out-of-i64-range one and panic
                // (the contract's fail-loud posture until bigint promotion lands).
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
        // Python-matching format block (float: `nan` + `".0"` whole-number suffix).
        Expr::ToStr { value, of_float } => {
            if *of_float {
                // PMAT-583: match CPython's float repr — scientific notation when
                // the decimal exponent is `< -4` or `>= 16` (else fixed). The
                // exponent is read from Rust's `{:e}` (exact; avoids `log10`
                // rounding error), and reformatted to Python's `e±NN` style
                // (signed, ≥2-digit). Mantissa digits already match (both use
                // shortest round-trip). Fixed path keeps the `.0`-if-whole shape.
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
        // PMAT-582: `repr(str)` — CPython-style quoted form. Pick the quote
        // (single, or double if the string has a `'` but no `"`), then escape
        // `\`, the quote, `\n`, `\r`, `\t`. A raw codegen string keeps the
        // emitted Rust verbatim. (Other non-printables emit verbatim; full
        // `\xNN` escaping deferred.)
        Expr::ReprStr { value } => {
            out.push_str("{ let __rs = &(");
            emit_expr(out, value, mode)?;
            out.push_str(
                r#"); let __q = if __rs.contains('\'') && !__rs.contains('"') { '"' } else { '\'' }; let mut __ro = String::new(); __ro.push(__q); for __rc in __rs.chars() { match __rc { '\\' => { __ro.push('\\'); __ro.push('\\'); } '\n' => { __ro.push('\\'); __ro.push('n'); } '\r' => { __ro.push('\\'); __ro.push('r'); } '\t' => { __ro.push('\\'); __ro.push('t'); } __ec if __ec == __q => { __ro.push('\\'); __ro.push(__ec); } __ec => __ro.push(__ec) } } __ro.push(__q); __ro }"#,
            );
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
        // PMAT-612: `round(int, n)` → int. For n >= 0 the int is returned
        // unchanged; for n < 0 it is rounded to the nearest multiple of
        // `10^(-n)` using round-half-to-EVEN (banker's rounding, like Python).
        // The arithmetic is done in `i128` so the scale and products can't
        // overflow; the result fails loud if it leaves `i64` range.
        Expr::RoundIntToDigits { value, ndigits } => {
            out.push_str("{ let __rv = (");
            emit_expr(out, value, mode)?;
            out.push_str(") as i128; let __rn = (");
            emit_expr(out, ndigits, mode)?;
            out.push_str("); if __rn >= 0 { __rv as i64 } else { let __rp = 10i128.checked_pow((-__rn) as u32).expect(\"xpile: OverflowError: round() scale out of range\"); let __rd = __rv.div_euclid(__rp); let __rm = __rv.rem_euclid(__rp); let __r2 = 2i128 * __rm; let __res = if __r2 < __rp { __rd * __rp } else if __r2 > __rp { (__rd + 1) * __rp } else if __rd % 2 == 0 { __rd * __rp } else { (__rd + 1) * __rp }; if __res < (i64::MIN as i128) || __res > (i64::MAX as i128) { panic!(\"xpile: OverflowError: round() result out of i64 range\"); } __res as i64 } }");
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
                    // PMAT-568: Python `max(key=)` returns the FIRST element with
                    // the maximal key, but Rust's `max_by_key` returns the LAST.
                    // Reverse the iterator first so its last-wins picks the
                    // original first maximum. `min` is unaffected — both Python
                    // `min` and Rust `min_by_key` keep the first minimum.
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
                    // Ord element (i64 / str / bool): `.min()/.max()` returns
                    // Option. `.cloned()` (not `.copied()`) so non-Copy
                    // `String` works too (PMAT-502er); i64/bool are `Clone`.
                    false => out.push_str(if *is_max {
                        ".iter().cloned().max()"
                    } else {
                        ".iter().cloned().min()"
                    }),
                    // PMAT-608: float min/max follow Python's first-argument-wins
                    // semantics (and NaN propagation), NOT `f64::max`/`f64::min`,
                    // via a strict-compare `reduce` (→ Option). This also fixes
                    // the empty case: the old `fold(±∞, …)` returned ±∞ for an
                    // empty sequence; `reduce` yields `None`, unwrapped below to
                    // a Python-`ValueError`-style panic (or the default).
                    true => {
                        let cmp = if *is_max { ">" } else { "<" };
                        write!(
                            out,
                            ".iter().copied().reduce(|__a, __b| if __b {cmp} __a {{ __b }} else {{ __a }})"
                        )?;
                    }
                },
            }
            // Every branch now yields an `Option`; unwrap (empty → Python
            // ValueError) or substitute the default.
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
        // PMAT-502as: `xs.pop()` → `(<list>).pop().unwrap()` (last; panics
        // if empty, matching Python IndexError); `xs.pop(i)` →
        // `(<list>).remove((<i>) as usize)` (panics if out of range).
        Expr::ListPop { list, index } => match index {
            None => {
                out.push('(');
                emit_expr(out, list, mode)?;
                out.push_str(").pop().unwrap()");
            }
            // PMAT-570: a negative-resolved index (`len(xs) - k`) references the
            // receiver, conflicting with `remove`'s mutable borrow (E0502) — bind
            // it first. Positive indices keep the inline form.
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
        Expr::Sorted {
            list,
            reverse,
            key,
            of_float,
        } => {
            out.push_str("{ let mut __xv = ");
            emit_expr(out, list, mode)?;
            out.push_str(".clone(); __xv.");
            match (key, *reverse) {
                // PMAT-578: `Vec<f64>` has no `Ord`, so a keyless float sort uses
                // `sort_by(partial_cmp)`; an i64 list keeps `.sort()`. Mirrors
                // `ListMutateOp::Sort`.
                // PMAT-616: a NaN element makes `partial_cmp` return `None`;
                // Python's `sorted` does NOT raise on NaN, so fall back to `Equal`
                // rather than `.unwrap()` panicking (identical for finite floats).
                (None, false) if *of_float => {
                    out.push_str(
                        "sort_by(|__a, __b| __a.partial_cmp(__b).unwrap_or(std::cmp::Ordering::Equal));",
                    );
                }
                (None, true) if *of_float => {
                    out.push_str(
                        "sort_by(|__a, __b| __b.partial_cmp(__a).unwrap_or(std::cmp::Ordering::Equal));",
                    );
                }
                (None, false) => out.push_str("sort();"),
                // Equal elements are identical, so reverse() can't disturb order.
                (None, true) => out.push_str("sort(); __xv.reverse();"),
                // PMAT-603: a FLOAT-returning key makes the comparison values
                // `f64` (no `Ord`) — `sort_by_key` is E0277. Compare the
                // recomputed key with `partial_cmp`.
                // PMAT-616: a NaN key falls back to `Equal` (Python doesn't raise).
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
                // PMAT-568: Python `sorted(key=, reverse=True)` is STABLE — equal-
                // key elements keep their ORIGINAL order. `sort_by_key` + `.reverse()`
                // flips them (descending-stable, not original-order-preserving); use
                // a stable descending comparator on the key instead.
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
                    // PMAT-603: a float key compares with `partial_cmp` (no `Ord`);
                    // integer/str keys use `cmp`. Descending + stable either way.
                    if *of_float {
                        // PMAT-616: NaN key → `Equal` (Python doesn't raise on NaN).
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
        // PMAT-549: `math.gcd(a, b)` → inline Euclidean algorithm over abs values.
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
        // PMAT-571: `pow(base, exp, mod)` → modular exponentiation (square &
        // multiply, reduced mod m each step via i128 products → no overflow).
        // PMAT-605: Python's 3-arg `pow` returns a result with the SIGN of the
        // modulus (range `(m, 0]` for `m < 0`); the square-multiply loop yields
        // the non-negative Euclidean residue, so re-sign at the end when `m < 0`.
        Expr::PowMod { base, exp, modulus } => {
            out.push_str("{ let __pmm = (");
            emit_expr(out, modulus, mode)?;
            out.push_str("); if __pmm == 0 { panic!(\"xpile: ValueError: pow() 3rd argument cannot be 0\"); } let __pme = (");
            emit_expr(out, exp, mode)?;
            out.push_str("); if __pme < 0 { panic!(\"xpile: ValueError: pow() 2nd argument cannot be negative when 3rd argument specified\"); } let __pmb0 = (");
            emit_expr(out, base, mode)?;
            out.push_str("); let mut __pmb = { let __t = __pmb0 % __pmm; if __t < 0 { __t + __pmm } else { __t } }; let mut __pmr = 1i64 % __pmm; let mut __pmk = __pme; while __pmk > 0 { if __pmk & 1 == 1 { __pmr = (((__pmr as i128) * (__pmb as i128)) % (__pmm as i128)) as i64; } __pmk >>= 1; __pmb = (((__pmb as i128) * (__pmb as i128)) % (__pmm as i128)) as i64; } if __pmm < 0 && __pmr != 0 { __pmr += __pmm; } __pmr }");
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
                // PMAT-523: negative-step range — Python `range(start, stop,
                // step<0)` = `((stop)+1 ..= (start)).rev().step_by(|step|)`.
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
        // PMAT-520: `list(<set>)` / `sorted(<set>)` → the set's unique elements
        // as a Vec.
        Expr::SetToList { set } => {
            emit_expr(out, set, mode)?;
            out.push_str(".iter().cloned().collect::<Vec<_>>()");
        }
        // PMAT-502dk: `dict(pairs)` → a HashMap from the list of 2-tuples.
        Expr::DictFromPairs { pairs } => {
            emit_expr(out, pairs, mode)?;
            out.push_str(".iter().cloned().collect::<std::collections::HashMap<_, _>>()");
        }
        // PMAT-502dw/dx: `{k: v, **d, …}` → chain each fragment's iterator
        // (explicit pair → `once((k, v))`; splat → `(d).iter().map(clone)`)
        // into a fresh HashMap (a later entry wins, matching Python).
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
            // PMAT-502ej: a block-producing collection (`sorted(...)`,
            // `reversed(...)`, a block-expr — all emit `{ … }`) can't be
            // indexed directly: `{block}[i]` mis-parses as a block statement
            // followed by an array literal. Emit the collection to a temp and
            // wrap it in parens when it opens with `{` (parens are always safe;
            // a plain `xs` / `xs[i]` collection is left unparenthesized).
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
        // PMAT-502ey: 1-arg `d.get(k)` → `(d).get(&(k)).cloned()` : Option<V>.
        Expr::DictGetOpt { dict, key } => {
            emit_expr(out, dict, mode)?;
            out.push_str(".get(&(");
            emit_expr(out, key, mode)?;
            out.push_str(")).cloned()");
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
        // PMAT-502ep: set predicate → a parenthesized temp-bound block over
        // `HashSet::is_subset`/`is_superset`/`is_disjoint` (proper variants add
        // `&& __l != __r`). Temps avoid double-evaluating either operand.
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
        // PMAT-502eq: `xs.copy()` / `d.copy()` / `s.copy()` → `(<inner>).clone()`.
        Expr::Clone(inner) => {
            out.push('(');
            emit_expr(out, inner, mode)?;
            out.push_str(").clone()");
        }
        // PMAT-502ew: `Option` value — `None` / `Some(<e>)`.
        Expr::OptionExpr(inner) => match inner {
            None => out.push_str("None"),
            Some(e) => {
                out.push_str("Some(");
                emit_expr(out, e, mode)?;
                out.push(')');
            }
        },
        // PMAT-502ex: `x is None` → `(x).is_none()`; `x is not None` →
        // `(x).is_some()`.
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
        // PMAT-506b: struct construction `Name { f0: v0, … }`.
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
        // PMAT-503b: `try: return <body> except: return <handler>` → catch the
        // panics xpile raises for Python exceptions via `catch_unwind`.
        Expr::TryCatch { body, handler } => {
            out.push_str("match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| ");
            emit_expr(out, body, mode)?;
            out.push_str(")) { Ok(__xpile_try) => __xpile_try, Err(_) => ");
            emit_expr(out, handler, mode)?;
            out.push_str(" }");
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
        // PMAT-502fb: Python `~x` == `-(x+1)` == Rust `!x` on a signed integer.
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
        // PMAT-538: `div_euclid`/`rem_euclid` only match Python `//`/`%` for a
        // POSITIVE divisor. Python `//` floors toward −∞ and `%` takes the sign
        // of the divisor; for a negative divisor the euclidean ops diverge
        // (e.g. `-7 // -2` is 3 in Python but `div_euclid` gives 4). Emit the
        // truncating quotient/remainder with a floor correction instead.
        BinOp::FloorDiv => emit_floor_div(out, lhs, rhs, mode),
        BinOp::Mod => emit_floor_mod(out, lhs, rhs, mode),
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
    // PMAT-575: `checked_shl` only validates the shift *amount* (`None` iff the
    // amount is >= 64); it does NOT detect VALUE overflow. So `1i64 << 63`
    // returns `Some(i64::MIN)` and the `.expect(... overflow ...)` never fires —
    // a silent wrap that falsifies C-PY-INT-ARITH's overflow guarantee (Python's
    // `<<` is exact / arbitrary-precision, so the contract promises a panic until
    // bigint promotion lands). Emit a reversibility check: a left shift loses no
    // significant bits iff `(v << n) >> n == v` (arithmetic shift-back, valid for
    // both signs). Right-shift never value-overflows, and bigint mode is
    // arbitrary-precision, so both keep the plain checked form.
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
    // PMAT-577: Python defines `x >> n` for ANY non-negative `n` — once `n`
    // reaches the bit width the result saturates to the sign fill (`0` for
    // `x >= 0`, `-1` for `x < 0`, since `>>` is arithmetic on a signed int).
    // Rust's `checked_shr` returns `None` for `n >= 64`, so the `.expect`
    // panicked where Python returns a value. Clamp the amount to 63 (which
    // yields exactly that sign fill); a NEGATIVE amount still panics, matching
    // Python's `ValueError: negative shift count`.
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

/// PMAT-538: Python floor-division `a // b` for i64. Rust `/` truncates toward
/// zero and `div_euclid` keeps a non-negative remainder; neither matches
/// Python's floor (toward −∞) for a negative divisor. Emit the truncating
/// quotient plus a floor correction (subtract 1 when the remainder is non-zero
/// and its sign differs from the divisor's). `checked_div`/`checked_rem` keep
/// the `i64::MIN / -1` and divide-by-zero panics (same contract posture as the
/// former `checked_div_euclid`); the `__q - 1` correction is only reached when
/// the remainder is non-zero, where `__q` is never `i64::MIN`, so it cannot
/// overflow.
fn emit_floor_div(
    out: &mut String,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), CodegenError> {
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

/// PMAT-538: Python modulo `a % b` for i64. Python's result takes the sign of
/// the divisor; Rust `%` takes the sign of the dividend and `rem_euclid` is
/// always non-negative — both diverge for a negative divisor. Emit the
/// truncating remainder plus a floor correction (add the divisor when the
/// remainder is non-zero and its sign differs). The corrected value has
/// magnitude < |divisor|, so `__r + __fb` cannot overflow.
fn emit_floor_mod(
    out: &mut String,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), CodegenError> {
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
            // PMAT-502fb: bitwise invert — Rust `!` on a signed integer.
            UnOp::BitNot => {
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
    fn emits_floordiv_with_floor_correction() {
        // PMAT-538: Python `a // b` floors toward −∞ (so the result diverges
        // from `div_euclid` for a negative divisor). The emit must use the
        // truncating quotient (`checked_div`) plus a floor correction, NOT
        // `div_euclid` and NOT a bare Rust `/`.
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
            rust.contains("checked_div") && rust.contains("__q - 1"),
            "Python floor-div must lower to checked_div + floor correction (got: {rust})"
        );
        assert!(
            !rust.contains("div_euclid"),
            "must not use div_euclid (wrong for a negative divisor): {rust}"
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
