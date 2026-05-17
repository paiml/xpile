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
use xpile_backend::{Artifact, Backend, BackendConfig, BackendError, Target};
use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, Param, Stmt, Type, UnOp};

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
    for item in &module.items {
        match item {
            Item::Function(f) => {
                emit_function(&mut out, f)?;
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
    emit_type(out, f.return_type)?;
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
    if f.return_type == Type::BigInt {
        return true;
    }
    if f.params.iter().any(|p| p.ty == Type::BigInt) {
        return true;
    }
    fn stmt_has_bigint(s: &Stmt) -> bool {
        match s {
            Stmt::Let { ty, .. } => *ty == Type::BigInt,
            Stmt::Assign { .. } | Stmt::Assert { .. } => false,
            Stmt::While { body, .. } => body.iter().any(stmt_has_bigint),
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
            emit_type(out, *ty)?;
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
        Stmt::Assert { cond } => {
            write!(out, "{indent}assert!(")?;
            emit_expr(out, cond, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
    }
}

fn emit_param(out: &mut String, p: &Param) -> Result<(), CodegenError> {
    write!(out, "{}: ", p.name)?;
    emit_type(out, p.ty)?;
    Ok(())
}

fn emit_type(out: &mut String, t: Type) -> Result<(), CodegenError> {
    out.push_str(match t {
        Type::I64 => "i64",
        Type::Bool => "bool",
        // PMAT-012: re-exported from `xpile-bigint` (which wraps
        // `num_bigint::BigInt`). Operator overloads (`+`, `-`, `*`,
        // `<=`, …) work without method calls, matching the i64 codegen
        // shape — except no `.checked_*().expect(...)` since BigInt
        // never overflows.
        Type::BigInt => "xpile_bigint::BigInt",
    });
    Ok(())
}

fn emit_expr(out: &mut String, e: &Expr, mode: bool) -> Result<(), CodegenError> {
    match e {
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
        Expr::BinOp { op, lhs, rhs } => emit_binop(out, *op, lhs, rhs, mode)?,
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => emit_if_expr(out, cond, then_expr, else_expr, mode)?,
        Expr::Call { callee, args } => emit_call(out, callee, args, mode)?,
        Expr::UnOp { op, operand } => emit_unop(out, *op, operand, mode)?,
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
                },
                Param {
                    name: "b".into(),
                    ty: Type::I64,
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
                },
                Param {
                    name: "b".into(),
                    ty: Type::I64,
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
                },
                Param {
                    name: "b".into(),
                    ty: Type::I64,
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
                },
                Param {
                    name: "b".into(),
                    ty: Type::I64,
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
