//! Python frontend for xpile.
//!
//! Parses `.py` source with `rustpython-parser` and lowers a tightly
//! constrained subset (top-level `def` with a single `return expr` body,
//! i64-typed params and return) into meta-HIR. Anything outside the
//! subset returns `FrontendError::Lower` with a message naming the
//! unsupported construct.
//!
//! Subset supported at v0.1.0:
//!   - Top-level `def name(p1, p2, ...): return expr`
//!   - Identifiers, integer literals
//!   - Binary ops: + - * // %  ==  !=  <  <=  >  >=
//!   - Everything is `i64` (typed as such on the Rust side).
//!
//! Extensions (later): type annotations, multi-statement bodies,
//! conditionals, calls, the rest of [`xpile_meta_hir::BinOp`].

use std::path::Path;
use xpile_frontend::{Frontend, FrontendError};
use xpile_meta_hir::{BinOp, Expr, Function, Item, Module, Param, SourceLang, Type};

use rustpython_parser::ast;
use rustpython_parser::Parse;

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

    let params: Vec<Param> = f
        .args
        .args
        .into_iter()
        .map(|arg| Param {
            name: arg.def.arg.to_string(),
            ty: Type::I64,
        })
        .collect();

    // Body must be exactly one `return expr`.
    let mut body_iter = f.body.into_iter();
    let first = body_iter
        .next()
        .ok_or_else(|| FrontendError::Lower(format!("function `{}` has an empty body", f.name)))?;
    if body_iter.next().is_some() {
        return Err(FrontendError::Lower(format!(
            "function `{}` has multiple statements — only `return expr` is supported at v0.1.0",
            f.name
        )));
    }

    let return_expr = match first {
        ast::Stmt::Return(ret) => ret.value.ok_or_else(|| {
            FrontendError::Lower(format!("function `{}` returns nothing", f.name))
        })?,
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{}` body is not `return expr`",
                f.name
            )));
        }
    };

    let body = lower_expr(*return_expr)?;
    let return_type = infer_type(&body);

    Ok(Function {
        name: f.name.to_string(),
        params,
        return_type,
        body,
    })
}

/// Trivial type inference for the v0.1.0 subset. Comparisons yield Bool,
/// everything else yields I64. Will move into meta-HIR once a second
/// frontend needs the same logic.
fn infer_type(e: &Expr) -> Type {
    match e {
        Expr::Ident(_) | Expr::LitInt(_) => Type::I64,
        Expr::BinOp { op, .. } => match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::FloorDiv | BinOp::Mod => Type::I64,
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => {
                Type::Bool
            }
        },
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
        other => Err(FrontendError::Lower(format!(
            "unsupported expression: {:?}",
            std::mem::discriminant(&other)
        ))),
    }
}

fn lower_binop(op: &ast::Operator) -> Result<BinOp, FrontendError> {
    Ok(match op {
        ast::Operator::Add => BinOp::Add,
        ast::Operator::Sub => BinOp::Sub,
        ast::Operator::Mult => BinOp::Mul,
        ast::Operator::FloorDiv => BinOp::FloorDiv,
        ast::Operator::Mod => BinOp::Mod,
        other => {
            return Err(FrontendError::Lower(format!(
                "unsupported binary operator: {:?} — supported: + - * // %",
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
        assert!(matches!(f.body, Expr::BinOp { op: BinOp::Add, .. }));
    }

    #[test]
    fn lowers_constant_in_body() {
        let m = parse("def f(a):\n    return a + 1\n");
        let f = function(&m, 0);
        let Expr::BinOp { lhs, rhs, op } = &f.body else {
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
            f.body,
            Expr::BinOp {
                op: BinOp::LtEq,
                ..
            }
        ));
    }

    #[test]
    fn rejects_multi_statement_body() {
        let err = PythonFrontend
            .parse_and_lower(
                &PathBuf::from("fixture.py"),
                "def f():\n    x = 1\n    return x\n",
            )
            .expect_err("multi-statement body should fail at v0.1.0");
        match err {
            FrontendError::Lower(msg) => {
                assert!(
                    msg.contains("multiple statements"),
                    "unexpected msg: {}",
                    msg
                );
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
    fn rejects_unsupported_operator() {
        let err = PythonFrontend
            .parse_and_lower(
                &PathBuf::from("fixture.py"),
                "def f(a, b):\n    return a ** b\n",
            )
            .expect_err("** should fail");
        match err {
            FrontendError::Lower(msg) => {
                assert!(msg.contains("supported"), "unexpected msg: {}", msg);
            }
            _ => panic!("expected Lower error"),
        }
    }
}
