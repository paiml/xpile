//! Python frontend for xpile.
//!
//! Parses `.py` source with `rustpython-parser` and lowers a constrained
//! subset into meta-HIR. Anything outside the subset returns
//! `FrontendError::Lower` with a message naming the unsupported construct.
//!
//! Subset supported at v0.1.0:
//!   - Top-level `def name(p1, p2, ...):` with a body of zero-or-more
//!     `name = expr` assignments followed by a final `return expr`.
//!   - Identifiers, integer literals.
//!   - Binary ops: `+ - * // %  ==  !=  <  <=  >  >=`.
//!   - Ternary `x if cond else y` (both branches must have the same type;
//!     `cond` must be Bool — no int-truthiness coercion).
//!   - Type inference: comparisons → `Bool`, otherwise `I64`.
//!
//! Extensions (later): type annotations, `if/else` statements, loops,
//! function calls, bigint promotion.

use std::path::Path;
use xpile_frontend::{Frontend, FrontendError};
use xpile_meta_hir::{
    BinOp, Block, Expr, Function, Item, Module, Param, SourceLang, Stmt, Type, UnOp,
};

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

    // Parse explicit param annotations (`a: int`). Default to I64 when
    // unannotated — Python lets that mean "any int" so it's safe.
    let mut params: Vec<Param> = Vec::with_capacity(f.args.args.len());
    for arg in f.args.args {
        let name = arg.def.arg.to_string();
        let ty = match arg.def.annotation.as_ref() {
            None => Type::I64,
            Some(ann) => parse_type_annotation(&f.name, &name, ann)?,
        };
        params.push(Param { name, ty });
    }

    // Body: zero or more leading `let`s, then a final `return expr`.
    if f.body.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{}` has an empty body",
            f.name
        )));
    }

    let body_stmts = f.body;
    let (last, leading) = body_stmts.split_last().expect("checked non-empty above");

    let mut stmts: Vec<Stmt> = Vec::with_capacity(leading.len());
    for stmt in leading {
        stmts.push(lower_block_stmt(&f.name, stmt.clone())?);
    }

    let trailing_return = match last {
        ast::Stmt::Return(ret) => {
            let value = ret.value.as_ref().ok_or_else(|| {
                FrontendError::Lower(format!("function `{}` returns nothing", f.name))
            })?;
            lower_expr((**value).clone())?
        }
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{}` does not end with `return expr` — required at v0.1.0",
                f.name
            )));
        }
    };

    let inferred_return = infer_type(&trailing_return);
    let return_type = match f.returns.as_ref() {
        None => inferred_return,
        Some(ann) => {
            let declared = parse_type_annotation(&f.name, "<return>", ann)?;
            if declared != inferred_return {
                return Err(FrontendError::Lower(format!(
                    "function `{}` declared return type {declared:?} but body produces {inferred_return:?}",
                    f.name
                )));
            }
            declared
        }
    };

    let body = Block {
        stmts,
        trailing_return,
    };

    Ok(Function {
        name: f.name.to_string(),
        params,
        return_type,
        body,
    })
}

/// Parse a Python type annotation expression to a meta-HIR [`Type`].
/// At v0.1.0 only `int` and `bool` are recognized.
fn parse_type_annotation(
    fn_name: &str,
    site: &str,
    ann: &ast::Expr,
) -> Result<Type, FrontendError> {
    match ann {
        ast::Expr::Name(n) => match n.id.as_str() {
            "int" => Ok(Type::I64),
            "bool" => Ok(Type::Bool),
            other => Err(FrontendError::Lower(format!(
                "function `{fn_name}` annotates `{site}` with unsupported type `{other}` — only `int` and `bool` at v0.1.0"
            ))),
        },
        _ => Err(FrontendError::Lower(format!(
            "function `{fn_name}` annotates `{site}` with a non-trivial type expression — not supported at v0.1.0"
        ))),
    }
}

/// Lower a single non-trailing statement.
///
/// At v0.1.0 we recognize:
///   - `name = expr`          → [`Stmt::Let`]
///   - `if cond: name = a; else: name = b`  → [`Stmt::Let`] with an
///     [`Expr::IfExpr`] value. Both branches MUST be a single assignment
///     to the SAME name with the SAME inferred type. Everything else
///     (multi-statement branches, mismatched assignment targets, missing
///     else, type-mismatched values) errors with a clear message.
fn lower_block_stmt(fn_name: &str, stmt: ast::Stmt) -> Result<Stmt, FrontendError> {
    match stmt {
        ast::Stmt::Assign(asn) => lower_assign(fn_name, asn),
        ast::Stmt::If(if_stmt) => lower_if_stmt_as_let(fn_name, if_stmt),
        ast::Stmt::Return(_) => Err(FrontendError::Lower(format!(
            "function `{fn_name}` has an early `return` — only the last statement may be `return` at v0.1.0"
        ))),
        other => Err(FrontendError::Lower(format!(
            "function `{fn_name}` contains unsupported statement: {:?} — supported: assignment, then a final `return`",
            std::mem::discriminant(&other)
        ))),
    }
}

/// Lower a Python `if/elif*/else` statement whose every branch is a
/// single assignment to the same variable. Lifts the whole chain into
/// a meta-HIR `Stmt::Let { value: Expr::IfExpr { ... } }`. `elif`
/// chains nest as `else_expr` of each outer `IfExpr`.
fn lower_if_stmt_as_let(fn_name: &str, if_stmt: ast::StmtIf) -> Result<Stmt, FrontendError> {
    // Determine the target name from the then-branch first; the recursive
    // walk validates every other branch assigns to the same name.
    if if_stmt.body.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "function `{fn_name}` has an if-statement whose then-branch has multiple statements — v0.1.0 requires exactly one assignment per branch"
        )));
    }
    let target_name = match &if_stmt.body[0] {
        ast::Stmt::Assign(a) => single_name_target(fn_name, a)?,
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{fn_name}` has an if-statement whose then-branch is not `name = expr`"
            )));
        }
    };

    let if_expr = lower_if_chain_to_expr(fn_name, &if_stmt, &target_name)?;
    let ty = match &if_expr {
        Expr::IfExpr { then_expr, .. } => infer_type(then_expr),
        // The recursive lowering always produces an IfExpr — defensive
        // fallback in case the shape changes.
        other => infer_type(other),
    };

    Ok(Stmt::Let {
        name: target_name,
        ty,
        value: if_expr,
    })
}

/// Recursively lower a chain of `if/elif*/else` into a single
/// [`Expr::IfExpr`] expression. Used by `lower_if_stmt_as_let` to
/// support elif:
///
/// ```text
/// if a: x = 1                            if a { 1 }
/// elif b: x = 2          lowers to       else if b { 2 }
/// else:    x = 3                         else { 3 }
/// ```
///
/// Internally this becomes nested IfExpr nodes; the codegen pretty-print
/// is `if a { 1 } else { if b { 2 } else { 3 } }` (semantically equivalent
/// to `else if` — a future pretty-printer can flatten).
fn lower_if_chain_to_expr(
    fn_name: &str,
    if_stmt: &ast::StmtIf,
    target_name: &str,
) -> Result<Expr, FrontendError> {
    if if_stmt.orelse.is_empty() {
        return Err(FrontendError::Lower(format!(
            "function `{fn_name}` has `if` without `else` — at v0.1.0 every branch must assign `{target_name}` (no use-before-init)"
        )));
    }
    if if_stmt.body.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "function `{fn_name}` has an if-branch with multiple statements — v0.1.0 requires exactly one assignment per branch"
        )));
    }
    let then_asn = match &if_stmt.body[0] {
        ast::Stmt::Assign(a) => a,
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{fn_name}` has an if-branch that is not `name = expr`"
            )));
        }
    };
    let then_name = single_name_target(fn_name, then_asn)?;
    if then_name != target_name {
        return Err(FrontendError::Lower(format!(
            "function `{fn_name}` has if-branch assigning `{then_name}` but earlier branch assigns `{target_name}` — every branch must assign the same name"
        )));
    }

    let cond = lower_expr((*if_stmt.test).clone())?;
    if infer_type(&cond) != Type::Bool {
        return Err(FrontendError::Lower(format!(
            "function `{fn_name}` has an if-condition that is not Bool (no int-truthiness at v0.1.0)"
        )));
    }

    let then_expr = lower_expr((*then_asn.value).clone())?;
    let then_ty = infer_type(&then_expr);

    // Else branch is one of:
    //   [Assign(target_name, expr)]   — terminal else
    //   [StmtIf(nested)]              — elif (recurse)
    if if_stmt.orelse.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "function `{fn_name}` has an else-branch with multiple statements — v0.1.0 requires exactly one assignment or a single nested `if`"
        )));
    }
    let else_expr = match &if_stmt.orelse[0] {
        ast::Stmt::Assign(else_asn) => {
            let else_name = single_name_target(fn_name, else_asn)?;
            if else_name != target_name {
                return Err(FrontendError::Lower(format!(
                    "function `{fn_name}` has else-branch assigning `{else_name}` but earlier branches assign `{target_name}`"
                )));
            }
            lower_expr((*else_asn.value).clone())?
        }
        ast::Stmt::If(nested) => lower_if_chain_to_expr(fn_name, nested, target_name)?,
        _ => {
            return Err(FrontendError::Lower(format!(
                "function `{fn_name}` has else-branch that is neither `name = expr` nor a nested `if` (elif) — v0.1.0 supports only those shapes"
            )));
        }
    };
    let else_ty = infer_type(&else_expr);
    if then_ty != else_ty {
        return Err(FrontendError::Lower(format!(
            "function `{fn_name}` if-branches assign mismatched types ({then_ty:?} vs {else_ty:?})"
        )));
    }

    Ok(Expr::IfExpr {
        cond: Box::new(cond),
        then_expr: Box::new(then_expr),
        else_expr: Box::new(else_expr),
    })
}

/// Extract the single Name target of an `Assign` statement, rejecting
/// chained / tuple / attribute / subscript targets with messages
/// consistent with [`lower_assign`].
fn single_name_target(fn_name: &str, asn: &ast::StmtAssign) -> Result<String, FrontendError> {
    if asn.targets.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "function `{fn_name}` has chained assignment inside if-branch — not supported at v0.1.0"
        )));
    }
    match &asn.targets[0] {
        ast::Expr::Name(n) => Ok(n.id.to_string()),
        ast::Expr::Tuple(_) => Err(FrontendError::Lower(format!(
            "function `{fn_name}` uses tuple unpacking inside if-branch — not supported at v0.1.0"
        ))),
        _ => Err(FrontendError::Lower(format!(
            "function `{fn_name}` has unsupported assignment target inside if-branch"
        ))),
    }
}

fn lower_assign(fn_name: &str, asn: ast::StmtAssign) -> Result<Stmt, FrontendError> {
    if asn.targets.len() != 1 {
        return Err(FrontendError::Lower(format!(
            "function `{fn_name}` has chained assignment `a = b = ...` — not supported at v0.1.0"
        )));
    }
    let target = asn.targets.into_iter().next().expect("len checked");
    let name = match target {
        ast::Expr::Name(n) => n.id.to_string(),
        ast::Expr::Tuple(_) => {
            return Err(FrontendError::Lower(format!(
                "function `{fn_name}` uses tuple unpacking `a, b = ...` — not supported at v0.1.0"
            )));
        }
        ast::Expr::Attribute(_) | ast::Expr::Subscript(_) => {
            return Err(FrontendError::Lower(format!(
                "function `{fn_name}` assigns to an attribute/subscript — not supported at v0.1.0"
            )));
        }
        other => {
            return Err(FrontendError::Lower(format!(
                "function `{fn_name}` has unsupported assignment target: {:?}",
                std::mem::discriminant(&other)
            )));
        }
    };
    let value = lower_expr(*asn.value)?;
    let ty = infer_type(&value);
    Ok(Stmt::Let { name, ty, value })
}

/// Trivial type inference for the v0.1.0 subset. Comparisons yield Bool,
/// everything else yields I64. Conditional expressions inherit the type
/// of their `then` branch (the frontend validates that both branches
/// agree). Will move into meta-HIR once a second frontend needs the
/// same logic.
fn infer_type(e: &Expr) -> Type {
    match e {
        Expr::Ident(_) | Expr::LitInt(_) => Type::I64,
        Expr::BinOp { op, .. } => match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::FloorDiv | BinOp::Mod => Type::I64,
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => {
                Type::Bool
            }
            BinOp::And | BinOp::Or => Type::Bool,
        },
        Expr::IfExpr { then_expr, .. } => infer_type(then_expr),
        // Without a cross-function signature table, assume calls return I64
        // (matches the v0.1.0 invariant that every transpiled fn returns
        // I64 or Bool, and only one of those is possible for an arithmetic
        // computation).
        Expr::Call { .. } => Type::I64,
        Expr::UnOp { op, .. } => match op {
            UnOp::Neg => Type::I64,
            UnOp::Not => Type::Bool,
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
        ast::Expr::IfExp(ie) => lower_if_exp(ie),
        ast::Expr::Call(c) => lower_call(c),
        ast::Expr::BoolOp(b) => lower_bool_op(b),
        ast::Expr::UnaryOp(u) => lower_unary_op(u),
        other => Err(FrontendError::Lower(format!(
            "unsupported expression: {:?}",
            std::mem::discriminant(&other)
        ))),
    }
}

/// Lower Python `a and b and c` / `a or b or c` to a left-folded chain
/// of binary [`Expr::BinOp`] with [`BinOp::And`] / [`BinOp::Or`].
fn lower_bool_op(b: ast::ExprBoolOp) -> Result<Expr, FrontendError> {
    if b.values.len() < 2 {
        return Err(FrontendError::Lower(
            "boolean operator with fewer than 2 operands — unreachable Python AST".into(),
        ));
    }
    let op = match b.op {
        ast::BoolOp::And => BinOp::And,
        ast::BoolOp::Or => BinOp::Or,
    };
    let mut iter = b.values.into_iter();
    let first = lower_expr(iter.next().expect("len ≥ 2"))?;
    if infer_type(&first) != Type::Bool {
        return Err(FrontendError::Lower(
            "operands of `and`/`or` must be Bool (no int-truthiness at v0.1.0)".into(),
        ));
    }
    let mut acc = first;
    for next in iter {
        let rhs = lower_expr(next)?;
        if infer_type(&rhs) != Type::Bool {
            return Err(FrontendError::Lower(
                "operands of `and`/`or` must be Bool (no int-truthiness at v0.1.0)".into(),
            ));
        }
        acc = Expr::BinOp {
            op,
            lhs: Box::new(acc),
            rhs: Box::new(rhs),
        };
    }
    Ok(acc)
}

fn lower_unary_op(u: ast::ExprUnaryOp) -> Result<Expr, FrontendError> {
    let operand = lower_expr(*u.operand)?;
    let op = match u.op {
        ast::UnaryOp::USub => {
            if infer_type(&operand) != Type::I64 {
                return Err(FrontendError::Lower(
                    "unary `-` requires I64 operand".into(),
                ));
            }
            UnOp::Neg
        }
        ast::UnaryOp::Not => {
            if infer_type(&operand) != Type::Bool {
                return Err(FrontendError::Lower(
                    "`not` requires Bool operand (no int-truthiness at v0.1.0)".into(),
                ));
            }
            UnOp::Not
        }
        ast::UnaryOp::UAdd => {
            return Err(FrontendError::Lower(
                "unary `+` not supported at v0.1.0 (it's a Python no-op; just remove it)".into(),
            ));
        }
        ast::UnaryOp::Invert => {
            return Err(FrontendError::Lower(
                "bitwise `~` not supported at v0.1.0".into(),
            ));
        }
    };
    Ok(Expr::UnOp {
        op,
        operand: Box::new(operand),
    })
}

fn lower_call(c: ast::ExprCall) -> Result<Expr, FrontendError> {
    if !c.keywords.is_empty() {
        return Err(FrontendError::Lower(
            "keyword arguments in calls (`f(x=...)`) are not supported at v0.1.0".into(),
        ));
    }
    let callee = match *c.func {
        ast::Expr::Name(n) => n.id.to_string(),
        ast::Expr::Attribute(_) => {
            return Err(FrontendError::Lower(
                "method calls (`obj.method(...)`) are not supported at v0.1.0".into(),
            ));
        }
        _ => {
            return Err(FrontendError::Lower(
                "indirect calls (callable-valued expressions) are not supported at v0.1.0".into(),
            ));
        }
    };
    let args: Result<Vec<Expr>, _> = c.args.into_iter().map(lower_expr).collect();
    Ok(Expr::Call {
        callee,
        args: args?,
    })
}

fn lower_if_exp(ie: ast::ExprIfExp) -> Result<Expr, FrontendError> {
    // Python: `then if cond else else_` lowers to meta-HIR's IfExpr.
    // Note Python AST order is (test, body, orelse) = (cond, then, else).
    let cond = lower_expr(*ie.test)?;
    let then_expr = lower_expr(*ie.body)?;
    let else_expr = lower_expr(*ie.orelse)?;

    let then_ty = infer_type(&then_expr);
    let else_ty = infer_type(&else_expr);
    if then_ty != else_ty {
        return Err(FrontendError::Lower(format!(
            "ternary branches have mismatched types ({then_ty:?} vs {else_ty:?}); both must agree at v0.1.0"
        )));
    }

    // The condition must be Bool. v0.1.0 doesn't auto-coerce int truthiness.
    if infer_type(&cond) != Type::Bool {
        return Err(FrontendError::Lower(
            "ternary condition must be a comparison (Bool); int-truthiness coercion not supported at v0.1.0".into(),
        ));
    }

    Ok(Expr::IfExpr {
        cond: Box::new(cond),
        then_expr: Box::new(then_expr),
        else_expr: Box::new(else_expr),
    })
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
        assert!(f.body.stmts.is_empty());
        assert!(matches!(
            f.body.trailing_return,
            Expr::BinOp { op: BinOp::Add, .. }
        ));
    }

    #[test]
    fn lowers_constant_in_body() {
        let m = parse("def f(a):\n    return a + 1\n");
        let f = function(&m, 0);
        let Expr::BinOp { lhs, rhs, op } = &f.body.trailing_return else {
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
            f.body.trailing_return,
            Expr::BinOp {
                op: BinOp::LtEq,
                ..
            }
        ));
    }

    #[test]
    fn lowers_assignment_then_return() {
        let m = parse("def f(a, b):\n    s = a + b\n    return s\n");
        let f = function(&m, 0);
        assert_eq!(f.body.stmts.len(), 1);
        let Stmt::Let { name, ty, value } = &f.body.stmts[0];
        assert_eq!(name, "s");
        assert_eq!(*ty, Type::I64);
        assert!(matches!(value, Expr::BinOp { op: BinOp::Add, .. }));
        assert!(matches!(f.body.trailing_return, Expr::Ident(ref n) if n == "s"));
    }

    #[test]
    fn lowers_multiple_lets_then_return() {
        let m = parse("def f(a, b):\n    x = a + 1\n    y = b * 2\n    return x + y\n");
        let f = function(&m, 0);
        assert_eq!(f.body.stmts.len(), 2);
    }

    #[test]
    fn rejects_function_without_trailing_return() {
        let err = PythonFrontend
            .parse_and_lower(&PathBuf::from("fixture.py"), "def f(a):\n    x = a + 1\n")
            .expect_err("missing trailing return should fail");
        match err {
            FrontendError::Lower(msg) => {
                assert!(msg.contains("end with `return"), "unexpected msg: {}", msg);
            }
            _ => panic!("expected Lower error"),
        }
    }

    #[test]
    fn rejects_chained_assignment() {
        let err = PythonFrontend
            .parse_and_lower(
                &PathBuf::from("fixture.py"),
                "def f(a):\n    x = y = a\n    return x\n",
            )
            .expect_err("chained assignment should fail");
        match err {
            FrontendError::Lower(msg) => {
                assert!(msg.contains("chained"), "unexpected msg: {}", msg);
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
    fn lowers_ternary() {
        let m = parse("def pick(a, b):\n    return a if a <= b else b\n");
        let f = function(&m, 0);
        assert_eq!(f.return_type, Type::I64);
        let Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } = &f.body.trailing_return
        else {
            panic!("expected IfExpr, got {:?}", f.body.trailing_return);
        };
        assert!(matches!(
            **cond,
            Expr::BinOp {
                op: BinOp::LtEq,
                ..
            }
        ));
        assert!(matches!(**then_expr, Expr::Ident(_)));
        assert!(matches!(**else_expr, Expr::Ident(_)));
    }

    #[test]
    fn rejects_ternary_with_mismatched_branch_types() {
        // then-branch is i64, else-branch is bool — should fail.
        let err = PythonFrontend
            .parse_and_lower(
                &PathBuf::from("fixture.py"),
                "def f(a, b):\n    return a if a < b else (a < b)\n",
            )
            .expect_err("mismatched ternary branch types should fail");
        match err {
            FrontendError::Lower(msg) => assert!(
                msg.contains("mismatched") || msg.contains("agree"),
                "unexpected msg: {msg}"
            ),
            _ => panic!("expected Lower error"),
        }
    }

    #[test]
    fn rejects_ternary_with_non_bool_condition() {
        let err = PythonFrontend
            .parse_and_lower(
                &PathBuf::from("fixture.py"),
                "def f(a, b):\n    return a if a else b\n",
            )
            .expect_err("non-bool ternary cond should fail (no int-truthiness at v0.1.0)");
        match err {
            FrontendError::Lower(msg) => assert!(
                msg.contains("Bool") || msg.contains("truthiness"),
                "unexpected msg: {msg}"
            ),
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
