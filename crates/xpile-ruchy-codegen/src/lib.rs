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
use xpile_backend::{Artifact, Backend, BackendConfig, BackendError, Target};
use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, Param, Stmt, Type, UnOp};

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
    // Ruchy: `fun name(params) -> ret { body }`. No `pub`.
    write!(out, "fun {}(", f.name)?;
    for (i, p) in f.params.iter().enumerate() {
        if i > 0 {
            write!(out, ", ")?;
        }
        emit_param(out, p)?;
    }
    write!(out, ") -> ")?;
    emit_type(out, f.return_type)?;
    writeln!(out, " {{")?;
    emit_block(out, &f.body)?;
    writeln!(out, "}}")?;
    Ok(())
}

fn emit_block(out: &mut String, block: &Block) -> Result<(), RuchyCodegenError> {
    for stmt in &block.stmts {
        emit_stmt(out, stmt)?;
    }
    write!(out, "    ")?;
    emit_expr(out, &block.trailing_return)?;
    writeln!(out)?;
    Ok(())
}

fn emit_stmt(out: &mut String, stmt: &Stmt) -> Result<(), RuchyCodegenError> {
    emit_stmt_indented(out, stmt, "    ")
}

fn emit_stmt_indented(
    out: &mut String,
    stmt: &Stmt,
    indent: &str,
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
            emit_type(out, *ty)?;
            write!(out, " = ")?;
            emit_expr(out, value)?;
            writeln!(out, ";")?;
            Ok(())
        }
        Stmt::Assign { name, value } => {
            write!(out, "{indent}{name} = ")?;
            emit_expr(out, value)?;
            writeln!(out, ";")?;
            Ok(())
        }
        Stmt::While { cond, body } => {
            write!(out, "{indent}while ")?;
            emit_expr(out, cond)?;
            writeln!(out, " {{")?;
            let inner = format!("{indent}    ");
            for s in body {
                emit_stmt_indented(out, s, &inner)?;
            }
            writeln!(out, "{indent}}}")?;
            Ok(())
        }
    }
}

fn emit_param(out: &mut String, p: &Param) -> Result<(), RuchyCodegenError> {
    write!(out, "{}: ", p.name)?;
    emit_type(out, p.ty)?;
    Ok(())
}

fn emit_type(out: &mut String, t: Type) -> Result<(), RuchyCodegenError> {
    out.push_str(match t {
        Type::I64 => "i64",
        Type::Bool => "bool",
    });
    Ok(())
}

fn emit_expr(out: &mut String, e: &Expr) -> Result<(), RuchyCodegenError> {
    match e {
        Expr::Ident(name) => write!(out, "{}", name)?,
        Expr::LitInt(v) => write!(out, "{}i64", v)?,
        Expr::BinOp { op, lhs, rhs } => emit_binop(out, *op, lhs, rhs)?,
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => emit_if_expr(out, cond, then_expr, else_expr)?,
        Expr::Call { callee, args } => emit_call(out, callee, args)?,
        Expr::UnOp { op, operand } => emit_unop(out, *op, operand)?,
    }
    Ok(())
}

fn emit_unop(out: &mut String, op: UnOp, operand: &Expr) -> Result<(), RuchyCodegenError> {
    match op {
        // Python: `-x` on int never overflows. Ruchy compiles to Rust;
        // `i64::MIN.checked_neg() == None`. Use checked_neg + panic
        // pointing at contract C-PY-INT-ARITH slow path (matches Rust
        // backend semantics).
        UnOp::Neg => {
            write!(out, "(")?;
            emit_expr(out, operand)?;
            write!(
                out,
                ").checked_neg().expect(\"xpile: i64 negation overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\")"
            )?;
        }
        UnOp::Not => {
            write!(out, "(!")?;
            emit_expr(out, operand)?;
            write!(out, ")")?;
        }
    }
    Ok(())
}

fn emit_call(out: &mut String, callee: &str, args: &[Expr]) -> Result<(), RuchyCodegenError> {
    write!(out, "{}(", callee)?;
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            write!(out, ", ")?;
        }
        emit_expr(out, a)?;
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
) -> Result<(), RuchyCodegenError> {
    write!(out, "if ")?;
    emit_expr(out, cond)?;
    write!(out, " {{ ")?;
    emit_expr(out, then_expr)?;
    write!(out, " }} else ")?;
    match else_expr {
        Expr::IfExpr {
            cond: c2,
            then_expr: t2,
            else_expr: e2,
        } => emit_if_expr(out, c2, t2, e2),
        _ => {
            write!(out, "{{ ")?;
            emit_expr(out, else_expr)?;
            write!(out, " }}")?;
            Ok(())
        }
    }
}

/// Arithmetic uses `checked_*` + `.expect(...)` to enforce the
/// Layer-1 contract C-PY-INT-ARITH. Ruchy compiles to Rust, so the
/// overflow semantics are identical. FloorDiv / Mod also preserve
/// Python-floor semantics via the Euclidean variants. Comparisons
/// and logical ops remain infix (no overflow risk).
fn emit_binop(
    out: &mut String,
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
) -> Result<(), RuchyCodegenError> {
    match op {
        BinOp::Add => emit_checked(out, lhs, "checked_add", rhs, "addition"),
        BinOp::Sub => emit_checked(out, lhs, "checked_sub", rhs, "subtraction"),
        BinOp::Mul => emit_checked(out, lhs, "checked_mul", rhs, "multiplication"),
        BinOp::FloorDiv => emit_checked(out, lhs, "checked_div_euclid", rhs, "floor-div"),
        BinOp::Mod => emit_checked(out, lhs, "checked_rem_euclid", rhs, "modulo"),
        BinOp::Eq => emit_infix(out, lhs, " == ", rhs),
        BinOp::NotEq => emit_infix(out, lhs, " != ", rhs),
        BinOp::Lt => emit_infix(out, lhs, " < ", rhs),
        BinOp::LtEq => emit_infix(out, lhs, " <= ", rhs),
        BinOp::Gt => emit_infix(out, lhs, " > ", rhs),
        BinOp::GtEq => emit_infix(out, lhs, " >= ", rhs),
        BinOp::And => emit_infix(out, lhs, " && ", rhs),
        BinOp::Or => emit_infix(out, lhs, " || ", rhs),
        BinOp::BitAnd => emit_infix(out, lhs, " & ", rhs),
        BinOp::BitOr => emit_infix(out, lhs, " | ", rhs),
        BinOp::BitXor => emit_infix(out, lhs, " ^ ", rhs),
        BinOp::Shl => emit_checked_shift(out, lhs, "checked_shl", rhs, "left-shift"),
        BinOp::Shr => emit_checked_shift(out, lhs, "checked_shr", rhs, "right-shift"),
        BinOp::Pow => emit_checked_pow(out, lhs, rhs),
    }
}

fn emit_checked_pow(out: &mut String, lhs: &Expr, rhs: &Expr) -> Result<(), RuchyCodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs)?;
    write!(out, ").checked_pow(u32::try_from(")?;
    emit_expr(out, rhs)?;
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
) -> Result<(), RuchyCodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs)?;
    write!(out, ").{method}(u32::try_from(")?;
    emit_expr(out, rhs)?;
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
) -> Result<(), RuchyCodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs)?;
    write!(out, ").{method}(")?;
    emit_expr(out, rhs)?;
    write!(
        out,
        ").expect(\"xpile: i64 {op_name} overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\")"
    )?;
    Ok(())
}

fn emit_infix(out: &mut String, lhs: &Expr, op: &str, rhs: &Expr) -> Result<(), RuchyCodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs)?;
    out.push_str(op);
    emit_expr(out, rhs)?;
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
        let ruchy = emit_module(&m).expect("emit ok");
        assert!(ruchy.contains("div_euclid"));
        assert!(!ruchy.contains(" / "));
    }
}
