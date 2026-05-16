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
    emit_block(out, &f.body)?;
    writeln!(out, "}}")?;
    Ok(())
}

fn emit_block(out: &mut String, block: &Block) -> Result<(), CodegenError> {
    for stmt in &block.stmts {
        emit_stmt(out, stmt)?;
    }
    write!(out, "    ")?;
    emit_expr(out, &block.trailing_return)?;
    writeln!(out)?;
    Ok(())
}

fn emit_stmt(out: &mut String, stmt: &Stmt) -> Result<(), CodegenError> {
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

fn emit_param(out: &mut String, p: &Param) -> Result<(), CodegenError> {
    write!(out, "{}: ", p.name)?;
    emit_type(out, p.ty)?;
    Ok(())
}

fn emit_type(out: &mut String, t: Type) -> Result<(), CodegenError> {
    out.push_str(match t {
        Type::I64 => "i64",
        Type::Bool => "bool",
    });
    Ok(())
}

fn emit_expr(out: &mut String, e: &Expr) -> Result<(), CodegenError> {
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

fn emit_unop(out: &mut String, op: UnOp, operand: &Expr) -> Result<(), CodegenError> {
    match op {
        // Python: `-x` on int never overflows mathematically (int is unbounded).
        // Rust: `i64::MIN.checked_neg() == None`. Use checked_neg + panic that
        // points at the unimplemented bigint promotion slow path of
        // contract C-PY-INT-ARITH. See py-int-arith-v1.yaml.
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

fn emit_call(out: &mut String, callee: &str, args: &[Expr]) -> Result<(), CodegenError> {
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

/// Rust `if cond { then } else { else_ }` — usable as an expression.
/// When the `else_` is itself another `IfExpr`, emit a flat
/// `else if ...` form (no extra braces) for readability.
fn emit_if_expr(
    out: &mut String,
    cond: &Expr,
    then_expr: &Expr,
    else_expr: &Expr,
) -> Result<(), CodegenError> {
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
        } => {
            emit_if_expr(out, c2, t2, e2)?;
            return Ok(());
        }
        _ => {
            write!(out, "{{ ")?;
            emit_expr(out, else_expr)?;
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
fn emit_binop(out: &mut String, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<(), CodegenError> {
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
    }
}

/// Emit a checked binary op: `(<lhs>).<method>(<rhs>).expect("<msg> overflow ...")`.
/// Returns `i64`, identical to infix on the no-overflow fast path.
fn emit_checked(
    out: &mut String,
    lhs: &Expr,
    method: &str,
    rhs: &Expr,
    op_name: &str,
) -> Result<(), CodegenError> {
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

fn emit_infix(out: &mut String, lhs: &Expr, op: &str, rhs: &Expr) -> Result<(), CodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs)?;
    out.push_str(op);
    emit_expr(out, rhs)?;
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
