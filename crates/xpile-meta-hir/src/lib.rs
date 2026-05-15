//! Canonical meta-HIR.
//!
//! Every frontend in xpile lowers its language-specific AST to this
//! shared, intentionally minimal IR. The federated approach (see
//! `docs/specifications/xpile-architecture-v1.md`) means we start
//! lossy and grow as hybrid-transpile cases demand.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub name: String,
    pub source_lang: SourceLang,
    pub items: Vec<Item>,
    pub ffi_boundaries: Vec<FfiBoundary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SourceLang {
    Python,
    C,
    Cpp,
    Cuda,
    Ruchy,
    /// Rust as a source language — keystone for bidirectional Rust↔Ruchy
    /// and the Rust→GPU paths (Rust→PTX/WGSL/SPIR-V).
    Rust,
    /// Lean 4 as a source language — its executable subset (def, partial
    /// def, inductive, structure, instance, ...) lowers to Rust via the
    /// code lane. Theorem statements parse via the proof lane separately.
    Lean,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Item {
    Function(Function),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Type,
    /// Function body. A sequence of zero or more [`Stmt`] followed by a
    /// trailing return expression — the invariant that every function
    /// terminates by yielding exactly one value, kept explicit.
    pub body: Block,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// Zero or more `let` bindings (and, in the future, control-flow
    /// statements) executed before the trailing return.
    pub stmts: Vec<Stmt>,
    /// The expression whose value is the function's return value.
    pub trailing_return: Expr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Stmt {
    /// `let name: ty = value;` — Python `name = value` lowered with
    /// frontend-inferred `ty`. Shadowing is allowed (re-binding the same
    /// name re-emits `let`), matching Python's assignment semantics.
    Let { name: String, ty: Type, value: Expr },
}

/// Convenience: a single-expression body wraps as `Block { stmts: vec![], trailing_return: expr }`.
/// Useful in tests and for tiny frontends that don't synthesize `let`s.
impl From<Expr> for Block {
    fn from(expr: Expr) -> Self {
        Block {
            stmts: Vec::new(),
            trailing_return: expr,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub ty: Type,
}

/// MVP type lattice. Will grow as frontends demand it. Keep semantically
/// minimal; map source-language type idiosyncrasies (Python int bigint
/// promotion, C signed/unsigned, etc.) at the frontend boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Type {
    /// 64-bit signed integer. The default-and-only numeric type at v0.1.0.
    I64,
    /// Boolean — produced by comparison ops in [`Expr::BinOp`].
    Bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    /// Local identifier reference (function parameter or future `let`).
    Ident(String),
    /// Integer literal, lowered as i64 at the boundary.
    LitInt(i64),
    /// Binary operation. Type inference is intentionally absent at v0.1.0:
    /// each backend infers result type from operand types via [`BinOp`].
    BinOp {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// Conditional expression — Python's `then if cond else else_`, lowered
    /// from `ast::Expr::IfExp`. Both branches must produce the same type at
    /// v0.1.0 (the frontend rejects branch-type-mismatch; future versions
    /// may unify or pick a least upper bound).
    IfExpr {
        cond: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
    /// Direct call by name: `callee(args...)`. The callee must be a
    /// top-level identifier (no method calls, no first-class function
    /// values at v0.1.0). Result type is inferred as I64 — the frontend
    /// lacks a cross-function signature table, so promote to proper
    /// inference when one lands.
    Call { callee: String, args: Vec<Expr> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    // Arithmetic — both operands `I64`, result `I64`.
    Add,
    Sub,
    Mul,
    /// Python `//`. Rust counterpart for signed: `a.div_euclid(b)` (matches
    /// Python's floor semantics). Plain `/` truncates toward zero in Rust,
    /// which diverges from Python for negative operands.
    FloorDiv,
    /// Python `%`. Rust counterpart: `a.rem_euclid(b)`. Same reason as FloorDiv.
    Mod,
    // Comparison — both operands `I64`, result `Bool`.
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfiBoundary {
    pub from_lang: SourceLang,
    pub to_lang: SourceLang,
    pub symbol: String,
    pub signature: String,
}
