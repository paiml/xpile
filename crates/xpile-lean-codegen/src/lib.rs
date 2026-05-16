//! Lean 4 (executable) backend.
//!
//! Emits the executable subset of Lean 4 from meta-HIR. Theorem
//! statements are NOT emitted here — those go through the proof lane
//! (`xpile-lean-contract-backend`).
//!
//! Surface mapping (Python → Lean):
//!   I64                  → Int
//!   Bool                 → Bool
//!   `pub fn name(...)`   → `def name (...) : T := body`
//!   `let x: T = v;`      → `let x := v;`  (Lean infers `T` from `v`)
//!   `if c { a } else { b }` (expr) → `if c then a else b`
//!   `a // b` (FloorDiv)  → `Int.fdiv a b`  (Python floor semantics)
//!   `a %  b` (Mod)       → `Int.fmod a b`
//!   logical `and`/`or`   → `&&` / `||`
//!   unary `-x` / `not x` → `(-x)` / `(!x)`
//!   call `f(args)`       → `(f arg1 arg2)` (Lean juxtaposition, parenthesized)
//!
//! Layer 2 contract for the Lean→Rust direction is
//! `contracts/xlate-lean-to-rust-v1.yaml`. The companion contract for
//! Rust/meta-HIR → Lean is reserved for a later authoring pass.

use std::fmt::Write;
use xpile_backend::{Artifact, Backend, BackendConfig, BackendError, Target};
use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, Param, Stmt, Type, UnOp};

#[derive(Debug, thiserror::Error)]
pub enum LeanCodegenError {
    #[error("unsupported item: {0}")]
    Unsupported(String),
    #[error("formatting error: {0}")]
    Format(#[from] std::fmt::Error),
}

pub fn emit_module(module: &Module) -> Result<String, LeanCodegenError> {
    let mut out = String::new();
    writeln!(
        out,
        "-- xpile-generated from {:?} module {}",
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

fn emit_function(out: &mut String, f: &Function) -> Result<(), LeanCodegenError> {
    write!(out, "def {}", f.name)?;
    for p in &f.params {
        write!(out, " (")?;
        emit_param(out, p)?;
        write!(out, ")")?;
    }
    write!(out, " : ")?;
    emit_type(out, f.return_type)?;
    writeln!(out, " :=")?;
    emit_block(out, &f.body)?;
    Ok(())
}

fn emit_param(out: &mut String, p: &Param) -> Result<(), LeanCodegenError> {
    write!(out, "{} : ", p.name)?;
    emit_type(out, p.ty)?;
    Ok(())
}

fn emit_type(out: &mut String, t: Type) -> Result<(), LeanCodegenError> {
    out.push_str(match t {
        Type::I64 => "Int",
        Type::Bool => "Bool",
    });
    Ok(())
}

fn emit_block(out: &mut String, block: &Block) -> Result<(), LeanCodegenError> {
    for stmt in &block.stmts {
        emit_stmt(out, stmt)?;
    }
    write!(out, "  ")?;
    emit_expr(out, &block.trailing_return)?;
    writeln!(out)?;
    Ok(())
}

fn emit_stmt(out: &mut String, stmt: &Stmt) -> Result<(), LeanCodegenError> {
    match stmt {
        Stmt::Let { name, value, .. } => {
            write!(out, "  let {name} := ")?;
            emit_expr(out, value)?;
            writeln!(out)?;
            Ok(())
        }
    }
}

fn emit_expr(out: &mut String, e: &Expr) -> Result<(), LeanCodegenError> {
    match e {
        Expr::Ident(name) => write!(out, "{}", name)?,
        Expr::LitInt(v) => write!(out, "({}: Int)", v)?,
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

/// Lean 4 `if c then a else b` — an expression.
fn emit_if_expr(
    out: &mut String,
    cond: &Expr,
    then_expr: &Expr,
    else_expr: &Expr,
) -> Result<(), LeanCodegenError> {
    write!(out, "if ")?;
    emit_expr(out, cond)?;
    write!(out, " then ")?;
    emit_expr(out, then_expr)?;
    write!(out, " else ")?;
    emit_expr(out, else_expr)?;
    Ok(())
}

/// Lean calls use juxtaposition: `(f a b)`, not `f(a, b)`.
fn emit_call(out: &mut String, callee: &str, args: &[Expr]) -> Result<(), LeanCodegenError> {
    write!(out, "({callee}")?;
    for a in args {
        write!(out, " ")?;
        emit_expr(out, a)?;
    }
    write!(out, ")")?;
    Ok(())
}

fn emit_unop(out: &mut String, op: UnOp, operand: &Expr) -> Result<(), LeanCodegenError> {
    let sym = match op {
        UnOp::Neg => "-",
        UnOp::Not => "!",
    };
    write!(out, "({sym}")?;
    emit_expr(out, operand)?;
    write!(out, ")")?;
    Ok(())
}

/// Binary ops:
///   - Arithmetic add/sub/mul: `+ - *`
///   - Comparisons: `== != < <= > >=`
///   - Logical: `&& ||`
///   - Python-floor division and modulo: `Int.fdiv` / `Int.fmod`
///     (NOT Lean's `/` and `%` on Int — those use T-division and
///     diverge from Python on negative operands.)
fn emit_binop(out: &mut String, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<(), LeanCodegenError> {
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
        BinOp::FloorDiv => emit_prefix2(out, "Int.fdiv", lhs, rhs),
        BinOp::Mod => emit_prefix2(out, "Int.fmod", lhs, rhs),
        // Bitwise: Lean 4 core provides Int.land / Int.lor / Int.xor for
        // the bool-ops and Int has HShiftLeft / HShiftRight instances
        // taking Nat. We coerce rhs via `.toNat` for shifts (matches
        // Python's "shift amount must be non-negative" check; if rhs is
        // negative the resulting toNat is 0, which differs from Python's
        // ValueError — leaving as a known Lean fidelity gap, callable
        // from any equivalence theorem against the Rust emission via the
        // `C-PY-INT-ARITH` contract).
        BinOp::BitAnd => emit_prefix2(out, "Int.land", lhs, rhs),
        BinOp::BitOr => emit_prefix2(out, "Int.lor", lhs, rhs),
        BinOp::BitXor => emit_prefix2(out, "Int.xor", lhs, rhs),
        BinOp::Shl => emit_shift(out, lhs, "<<<", rhs),
        BinOp::Shr => emit_shift(out, lhs, ">>>", rhs),
    }
}

fn emit_shift(out: &mut String, lhs: &Expr, op: &str, rhs: &Expr) -> Result<(), LeanCodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs)?;
    write!(out, " {op} (")?;
    emit_expr(out, rhs)?;
    write!(out, ").toNat)")?;
    Ok(())
}

fn emit_infix(out: &mut String, lhs: &Expr, op: &str, rhs: &Expr) -> Result<(), LeanCodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs)?;
    out.push_str(op);
    emit_expr(out, rhs)?;
    write!(out, ")")?;
    Ok(())
}

fn emit_prefix2(
    out: &mut String,
    name: &str,
    lhs: &Expr,
    rhs: &Expr,
) -> Result<(), LeanCodegenError> {
    write!(out, "({name} ")?;
    emit_expr(out, lhs)?;
    write!(out, " ")?;
    emit_expr(out, rhs)?;
    write!(out, ")")?;
    Ok(())
}

pub struct LeanBackend;

impl Backend for LeanBackend {
    fn name(&self) -> &'static str {
        "lean"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Lean]
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
            body: Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(Expr::Ident("a".into())),
                rhs: Box::new(Expr::Ident("b".into())),
            }
            .into(),
        }
    }

    #[test]
    fn emits_def_with_int_signature() {
        let m = module_with("fixture", vec![Item::Function(add_fn())]);
        let lean = emit_module(&m).expect("emit ok");
        assert!(
            lean.contains("def add (a : Int) (b : Int) : Int :="),
            "expected Lean def signature in:\n{lean}"
        );
        assert!(!lean.contains("pub fn"));
        assert!(!lean.contains("fun "));
    }

    #[test]
    fn floordiv_uses_int_fdiv_not_division_operator() {
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
            body: Expr::BinOp {
                op: BinOp::FloorDiv,
                lhs: Box::new(Expr::Ident("a".into())),
                rhs: Box::new(Expr::Ident("b".into())),
            }
            .into(),
        };
        let m = module_with("fixture", vec![Item::Function(f)]);
        let lean = emit_module(&m).expect("emit ok");
        assert!(
            lean.contains("Int.fdiv"),
            "Python `//` must use Int.fdiv (floor semantics): got\n{lean}"
        );
        assert!(!lean.contains(" / "));
    }

    #[test]
    fn emits_call_via_juxtaposition_not_paren_form() {
        let f = Function {
            name: "caller".into(),
            params: vec![Param {
                name: "x".into(),
                ty: Type::I64,
            }],
            return_type: Type::I64,
            body: Expr::Call {
                callee: "double".into(),
                args: vec![Expr::Ident("x".into())],
            }
            .into(),
        };
        let m = module_with("fixture", vec![Item::Function(f)]);
        let lean = emit_module(&m).expect("emit ok");
        assert!(
            lean.contains("(double x)"),
            "expected `(double x)` juxtaposition form, got:\n{lean}"
        );
        assert!(!lean.contains("double(x)"));
    }
}
