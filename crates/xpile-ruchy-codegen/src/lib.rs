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
    match stmt {
        Stmt::Let { name, ty, value } => {
            write!(out, "    let {name}: ")?;
            emit_type(out, *ty)?;
            write!(out, " = ")?;
            emit_expr(out, value)?;
            writeln!(out, ";")?;
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
    let sym = match op {
        UnOp::Neg => "-",
        UnOp::Not => "!",
    };
    write!(out, "({sym}")?;
    emit_expr(out, operand)?;
    write!(out, ")")?;
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

/// Same Euclidean semantics as the Rust backend — Python `//` and `%`
/// must NOT lower to plain `/` and `%` in any target that compiles via
/// Rust semantics (which Ruchy does).
fn emit_binop(
    out: &mut String,
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
) -> Result<(), RuchyCodegenError> {
    match op {
        BinOp::Add => emit_infix(out, lhs, " + ", rhs),
        BinOp::Sub => emit_infix(out, lhs, " - ", rhs),
        BinOp::Mul => emit_infix(out, lhs, " * ", rhs),
        BinOp::Eq => emit_infix(out, lhs, " == ", rhs),
        BinOp::NotEq => emit_infix(out, lhs, " != ", rhs),
        BinOp::Lt => emit_infix(out, lhs, " < ", rhs),
        BinOp::LtEq => emit_infix(out, lhs, " <= ", rhs),
        BinOp::Gt => emit_infix(out, lhs, " > ", rhs),
        BinOp::GtEq => emit_infix(out, lhs, " >= ", rhs),
        BinOp::And => emit_infix(out, lhs, " && ", rhs),
        BinOp::Or => emit_infix(out, lhs, " || ", rhs),
        BinOp::FloorDiv => {
            write!(out, "(")?;
            emit_expr(out, lhs)?;
            write!(out, ").div_euclid(")?;
            emit_expr(out, rhs)?;
            write!(out, ")")?;
            Ok(())
        }
        BinOp::Mod => {
            write!(out, "(")?;
            emit_expr(out, lhs)?;
            write!(out, ").rem_euclid(")?;
            emit_expr(out, rhs)?;
            write!(out, ")")?;
            Ok(())
        }
    }
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
        assert!(ruchy.contains("(a + b)"));
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
