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
    /// POSIX shell (bash / zsh / sh dialects + Makefile / Dockerfile) —
    /// the bashrs merger domain. PMAT-037 / XPILE-BASHRS-MERGER-001
    /// adds this variant as scaffold (`bashrs-frontend` produces it,
    /// `bashrs-backend` consumes it, all other backends return
    /// `Unsupported`). Per `sub/bashrs-merger.md` Layer B, meta-HIR
    /// will grow shell-specific `Stmt::Cmd` / `Stmt::Pipeline` etc.
    /// variants at v0.2.0 — at v0.1.0 a `SourceLang::Shell` `Module`
    /// is structurally empty (no items / no boundaries), validating
    /// only that the dispatch and SourceLang lane are wired.
    Shell,
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

impl Function {
    /// Returns the list of contract IDs that govern this function.
    /// Drives codegen citation emission (PMAT-011): each ID returned
    /// here will appear as `// xpile-contract: <ID>` (Rust/Ruchy) or
    /// `@[xpile_contract "<ID>"]` (Lean) next to the emitted function.
    ///
    /// Per v0.1.0's single Layer-1 contract, this returns
    /// `["C-PY-INT-ARITH"]` if and only if the function body uses any
    /// i64 arithmetic / bitwise / shift / power / unary-neg operator —
    /// the operations whose overflow / wrapping the contract bounds.
    /// Comparisons (`==`, `<`, etc.), logicals (`&&`, `!`), and
    /// constant-only / call-only bodies don't trigger the citation.
    pub fn applicable_contracts(&self) -> Vec<&'static str> {
        if self.uses_int_arithmetic() {
            vec!["C-PY-INT-ARITH"]
        } else {
            Vec::new()
        }
    }

    /// True if any expression in the function body uses an op that
    /// `C-PY-INT-ARITH` governs (overflow-prone arithmetic, bitwise,
    /// shifts, power, or unary negation). Walks all Stmt + Expr
    /// reachable from the body.
    pub fn uses_int_arithmetic(&self) -> bool {
        for stmt in &self.body.stmts {
            if stmt_has_int_arith(stmt) {
                return true;
            }
        }
        expr_has_int_arith(&self.body.trailing_return)
    }
}

fn stmt_has_int_arith(s: &Stmt) -> bool {
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } => expr_has_int_arith(value),
        Stmt::While { cond, body } => {
            if expr_has_int_arith(cond) {
                return true;
            }
            body.iter().any(stmt_has_int_arith)
        }
        Stmt::Assert { cond } => expr_has_int_arith(cond),
        // PMAT-039: shell commands are governed by `C-BASHRS-POSIX-IDEMPOTENCE`,
        // not `C-PY-INT-ARITH`. The args are `Vec<String>` (literal
        // tokens) — no arithmetic operands.
        Stmt::Cmd { .. } => false,
        // PMAT-041: pipelines compose Cmds — recurse into each stage
        // for completeness (currently every stage is a Cmd, so this is
        // always false in practice).
        Stmt::Pipeline { stages } => stages.iter().any(stmt_has_int_arith),
    }
}

fn expr_has_int_arith(e: &Expr) -> bool {
    match e {
        Expr::Ident(_) | Expr::LitInt(_) => false,
        Expr::BinOp { op, lhs, rhs } => {
            if binop_is_int_arith(*op) {
                return true;
            }
            expr_has_int_arith(lhs) || expr_has_int_arith(rhs)
        }
        Expr::UnOp { op, operand } => {
            if matches!(op, UnOp::Neg) {
                return true;
            }
            expr_has_int_arith(operand)
        }
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            expr_has_int_arith(cond)
                || expr_has_int_arith(then_expr)
                || expr_has_int_arith(else_expr)
        }
        Expr::Call { args, .. } => args.iter().any(expr_has_int_arith),
        // PMAT-042: string-literal Expr variants carry no arithmetic
        // operands; they're governed by C-BASHRS-POSIX-IDEMPOTENCE,
        // not C-PY-INT-ARITH.
        Expr::LitStr(_) | Expr::QuotedString { .. } => false,
        // PMAT-045: shell-variable references same disposition.
        Expr::ShellVar(_) => false,
    }
}

fn binop_is_int_arith(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::FloorDiv
            | BinOp::Mod
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::Pow
    )
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
    /// `let [mut] name: ty = value;` — first binding of `name` in this
    /// scope. `mutable` is set by the frontend when the same name is
    /// reassigned later in the function (including inside a [`While`]
    /// loop body). Rust/Ruchy emission honors it (`let mut` vs `let`)
    /// to keep `clippy -D unused_mut` happy. PMAT-006.
    Let {
        name: String,
        ty: Type,
        value: Expr,
        #[serde(default)]
        mutable: bool,
    },
    /// `name = value;` — reassignment of a name previously introduced
    /// by [`Stmt::Let`]. PMAT-006.
    Assign { name: String, value: Expr },
    /// `while cond { body }` — Python `while cond: body`. The body is
    /// a list of statements (no trailing return; the loop body is not
    /// an expression). PMAT-006.
    While { cond: Expr, body: Vec<Stmt> },
    /// `assert cond` — Python `assert cond` (no message form at v0.1.0).
    /// Lowers to `assert!(cond);` in Rust/Ruchy. Lean is skipped (Lean's
    /// assertion machinery requires `Decidable` instances; deferred). PMAT-009.
    Assert { cond: Expr },
    /// `program arg1 arg2 ...` — a single shell-command invocation.
    /// PMAT-039 / XPILE-BASHRS-MERGER-001 Layer B: the first shell
    /// variant to land in meta-HIR.
    ///
    /// Produced exclusively by `bashrs-frontend`; consumed exclusively
    /// by `bashrs-backend` at v0.1.0. Other backends (Rust / Ruchy /
    /// Lean / PTX / WGSL) return `CodegenError::Unsupported` when they
    /// encounter it — the explicit-arm dispatch makes the cross-domain
    /// boundary load-bearing rather than implicit.
    ///
    /// PMAT-042: args are `Vec<Expr>` — every arg is an `Expr::LitStr`
    /// (the unquoted / raw form) at the bashrs-frontend output by
    /// default. `Expr::QuotedString` carries an explicit
    /// `QuotingStrategy` for args that need shell-level quoting.
    /// Future Layer B variants (`Expr::ShellVar`, `Expr::CommandSubstitution`)
    /// plug in here without further IR churn.
    ///
    /// `Stmt::Pipeline { stages: Vec<Stmt::Cmd> }` shipped in PMAT-041.
    Cmd { program: String, args: Vec<Expr> },
    /// `cmd1 | cmd2 | cmd3 …` — POSIX pipeline composition. PMAT-041 /
    /// XPILE-BASHRS-MERGER-001 Layer B (second variant). Each stage
    /// is a `Stmt` so the variant composes (in principle) with the
    /// future `Stmt::ShellLoop` etc. — at v0.1.0 every stage is a
    /// `Stmt::Cmd` in practice; the bashrs-frontend parser rejects
    /// nested-pipeline / control-flow stages with an explicit
    /// diagnostic.
    ///
    /// Same cross-cutting posture as `Stmt::Cmd`: produced only by
    /// `bashrs-frontend`, consumed only by `bashrs-backend`, refused
    /// by every other backend via `Unsupported(...)` arms naming
    /// `C-BASHRS-POSIX-IDEMPOTENCE`.
    Pipeline { stages: Vec<Stmt> },
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
    /// 64-bit signed integer. The fast path for Python `int` — covers
    /// every case where the frontend can prove the value fits.
    I64,
    /// Boolean — produced by comparison ops in [`Expr::BinOp`].
    Bool,
    /// Unbounded integer — Python `int`'s native shape. The slow path
    /// of contract `C-PY-INT-ARITH`. Rust/Ruchy emit
    /// `xpile_bigint::BigInt`; Lean emits `Int` (which is already
    /// unbounded). PMAT-012.
    BigInt,
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
    /// Unary operation — `not x` (logical, Bool → Bool) or `-x`
    /// (numeric negate, I64 → I64).
    UnOp { op: UnOp, operand: Box<Expr> },
    /// String literal — the unquoted / "raw token" form. PMAT-042 /
    /// XPILE-BASHRS-MERGER-001 Layer B (Expr-side).
    ///
    /// Used exclusively in `Stmt::Cmd::args` and pipeline stages
    /// (bashrs domain); the Python / C / Rust frontends don't
    /// produce strings at v0.1.0 because their meta-HIR subset is
    /// integer-arithmetic-only (`C-PY-INT-ARITH` territory). Other
    /// backends (rust / ruchy / lean) refuse `Expr::LitStr` via
    /// `Unsupported` arms naming `C-BASHRS-POSIX-IDEMPOTENCE`.
    ///
    /// `LitStr` renders as a bareword in POSIX sh (no quoting). For
    /// args that contain whitespace / special chars, use
    /// `Expr::QuotedString` instead so the rendered shell carries
    /// the right quoting.
    LitStr(String),
    /// Quoted string literal carrying an explicit `QuotingStrategy`.
    /// PMAT-042 — the typed counterpart to `LitStr` when shell-level
    /// quoting matters (whitespace-containing args, vars-disabled
    /// args, etc.).
    ///
    /// The `content` is the *unescaped* string; the backend is
    /// responsible for emitting any escape sequences the chosen
    /// quoting style requires.
    QuotedString {
        content: String,
        quoting: QuotingStrategy,
    },
    /// Shell variable reference (`$NAME` or `${NAME}`). PMAT-045 /
    /// XPILE-BASHRS-MERGER-001 Layer B (Expr-side).
    ///
    /// The carried `String` is the variable name *without* the
    /// leading `$` and *without* the optional braces. bashrs-backend
    /// renders as `$NAME` by default. POSIX-legal name predicate
    /// (alphanumeric + underscore, no leading digit) is enforced
    /// at parse time, so a reachable `ShellVar` is always
    /// renderable as bareword.
    ///
    /// Same cross-domain disposition as the other Layer B Expr
    /// variants: produced only by `bashrs-frontend`, consumed only
    /// by `bashrs-backend`; rust/ruchy/lean refuse via
    /// `Unsupported(...)` naming `C-BASHRS-POSIX-IDEMPOTENCE`.
    ShellVar(String),
}

/// Per-arg shell quoting choice. Carried by `Expr::QuotedString` so
/// the bashrs-backend renders each arg in the strategy the source
/// indicated. v0.1.0's bashrs-frontend doesn't have a real
/// quoting-aware parser yet — it produces `Expr::LitStr` for every
/// arg, and `QuotingStrategy::None` is implicit there. The full
/// strategy set is wired so the v0.2.0 source fold's real bashrs
/// parser can plug in without IR churn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum QuotingStrategy {
    /// `'literal'` — single quotes. No variable expansion, no
    /// command substitution. Most idempotence-friendly form.
    Single,
    /// `"literal"` — double quotes. Variable expansion + command
    /// substitution still occur inside; only globbing/word-splitting
    /// are suppressed.
    Double,
    /// `\literal` — backslash-escape individual characters. Useful
    /// for short fragments where surrounding quotes would be
    /// awkward.
    Backslash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnOp {
    /// Numeric negation: `-x`, I64 → I64. Note `i64::MIN`'s negation
    /// overflows; frontends should warn but emit anyway.
    Neg,
    /// Logical not: `not x`, Bool → Bool.
    Not,
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
    // Logical — both operands `Bool`, result `Bool`. Python's `and`/`or`
    // are short-circuiting; the Rust/Ruchy `&&`/`||` emissions match.
    And,
    Or,
    // Bitwise — both operands `I64`, result `I64`.
    // `&`, `|`, `^` lower to plain infix (no overflow risk on i64).
    // Shifts use `checked_shl` / `checked_shr` so an out-of-range shift
    // amount (>= 64) panics referencing the `C-PY-INT-ARITH` slow path,
    // matching the arithmetic ops' overflow contract. Note: bit-truncation
    // from a large left shift (e.g. `(1<<62) << 2`) is *not* detected by
    // `checked_shl`; that's also part of the bigint slow path.
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    /// Python `**`. Both operands `I64`, result `I64` for non-negative
    /// exponents that don't overflow. Lowers to `i64::checked_pow(u32)`
    /// in Rust/Ruchy; negative exponents panic (Python returns Float,
    /// which the v0.1.0 type system has no I64-compatible representation
    /// for — surfacing as a `C-PY-INT-ARITH` slow-path panic for now).
    Pow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfiBoundary {
    pub from_lang: SourceLang,
    pub to_lang: SourceLang,
    pub symbol: String,
    pub signature: String,
}
