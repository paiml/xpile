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
        // PMAT-051: shell variable assignment carries a value Expr;
        // recurse for completeness (currently the value is always
        // a shell-domain Expr, so this is always false in practice).
        Stmt::ShellAssign { value, .. } => expr_has_int_arith(value),
        // PMAT-048: shell loops compose statements; recurse into
        // body + (where applicable) cond / items.
        Stmt::ShellLoop { kind, body } => {
            let kind_has = match kind {
                LoopKind::For { items, .. } => items.iter().any(expr_has_int_arith),
                LoopKind::While { cond } | LoopKind::Until { cond } => expr_has_int_arith(cond),
            };
            kind_has || body.iter().any(stmt_has_int_arith)
        }
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
        // operands; they're governed by C-BASHRS-POSIX-IDEMPOTENCE
        // (or, for Python `str` literals at v0.2.0, by
        // C-XLATE-PY-STR-TO-RUST-STRING), not C-PY-INT-ARITH.
        Expr::LitStr(_) | Expr::QuotedString { .. } => false,
        // PMAT-451: string concatenation is a str-domain operation;
        // operands type as `Type::Str` and never contribute to
        // C-PY-INT-ARITH's overflow analysis. Recurse defensively in
        // case future lowering ever nests an int-arith expression
        // inside a string-typed position (unlikely but cheap).
        Expr::Concat { lhs, rhs } => expr_has_int_arith(lhs) || expr_has_int_arith(rhs),
        // PMAT-455 (v0.2.0 Track 1.B): list literal — recurse into
        // each element. An int-typed element (`[1, 2, 3]`) doesn't
        // by itself involve overflow-prone arithmetic, but a list of
        // computed values (`[a + b, c * d]`) does.
        Expr::ListLit(elems) => elems.iter().any(expr_has_int_arith),
        // PMAT-045: shell-variable references same disposition.
        Expr::ShellVar(_) => false,
        // PMAT-055: shell special parameters same disposition.
        Expr::ShellSpecial(_) => false,
        // PMAT-047: command substitution composes a Stmt; recurse
        // into the inner Stmt for completeness (currently every
        // such Stmt is a shell-domain Cmd, so this is always
        // false in practice).
        Expr::CommandSubstitution(inner) => stmt_has_int_arith(inner),
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

// PMAT-047: PartialEq needed so `Expr::CommandSubstitution(Box<Stmt>)`
// participates in Expr's existing `PartialEq` derive. Every field
// of every Stmt variant is itself PartialEq (String, Type, Expr,
// Vec<Stmt>), so the derive is mechanical.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// `NAME=value` — POSIX shell variable assignment. PMAT-051 /
    /// XPILE-BASHRS-MERGER-001 Layer B (assignment idiom).
    ///
    /// The `name` is a POSIX-legal identifier (alphanumeric +
    /// underscore, no leading digit); the `value` is an `Expr` so
    /// future Layer B Expr variants (`Expr::CommandSubstitution`
    /// from PMAT-047/050, etc.) compose naturally — e.g.,
    /// `TODAY=$(date)` → `Stmt::ShellAssign { name: "TODAY", value:
    /// Expr::CommandSubstitution(Box::new(Stmt::Cmd { … })) }`.
    ///
    /// Same cross-domain disposition as the other bashrs variants:
    /// produced only by bashrs-frontend, consumed only by
    /// bashrs-backend, refused by other backends with
    /// `Unsupported(...)` naming `C-BASHRS-POSIX-IDEMPOTENCE`.
    ShellAssign { name: String, value: Expr },
    /// POSIX shell control-flow loop (`for x in …; do … done` /
    /// `while [ … ]; do … done` / `until [ … ]; do … done`). PMAT-048
    /// / XPILE-BASHRS-MERGER-001 Layer B (last variant from the
    /// spec table).
    ///
    /// IR-shape only at v0.1.0 — same scaffold posture as PMAT-046
    /// (Type variants) and PMAT-047 (`Expr::CommandSubstitution`):
    /// the variant is reachable through the IR, the bashrs-backend
    /// can render it, but bashrs-frontend's hand-rolled parser
    /// doesn't produce it yet. The v0.2.0 source fold's real bashrs
    /// parser produces it from real shell input.
    ///
    /// Same cross-domain disposition: bashrs-only; other backends
    /// refuse via `Unsupported(...)` naming `C-BASHRS-POSIX-IDEMPOTENCE`.
    ShellLoop { kind: LoopKind, body: Vec<Stmt> },
}

/// POSIX shell loop dialects. PMAT-048 / XPILE-BASHRS-MERGER-001
/// Layer B. Each variant carries the loop's control predicate /
/// item list; the body lives in `Stmt::ShellLoop::body`.
///
/// Future variants the spec hints at: `Select { var, items }` for
/// `select x in …;` interactive menus (rare; deferred).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LoopKind {
    /// `for VAR in item1 item2 …; do … done`. Each item is an
    /// `Expr` so future Layer B variants
    /// (`Expr::ShellVar` / `Expr::CommandSubstitution` / etc.)
    /// compose here without IR churn.
    For { var: String, items: Vec<Expr> },
    /// `while [ cond ]; do … done`. The condition is an `Expr` —
    /// typically a `Stmt::Cmd`-equivalent test expression, modelled
    /// as `Expr` for uniformity with the rest of the IR. A future
    /// `Expr::ShellTest` variant could carry POSIX `[ … ]` semantics
    /// explicitly; at v0.1.0 the condition is opaque.
    While { cond: Expr },
    /// `until [ cond ]; do … done` — POSIX's inverted while
    /// (continue while cond is *false*).
    Until { cond: Expr },
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
/// PMAT-455 — `Type` is no longer `Copy` because `Type::List(Box<Type>)`
/// is heap-allocated. Callers that previously relied on `*ty` to
/// dereference / copy now use `.clone()` (cheap — most Type values
/// are leaf variants or 1-deep boxes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Owned UTF-8 string — Python `str`'s native shape. PMAT-449,
    /// the v0.2.0 Track 1.A foundation
    /// ([`sub/v0.2.0-depyler-merger.md`](../../docs/specifications/sub/v0.2.0-depyler-merger.md)).
    ///
    /// Distinct from [`Type::ShellString`] (which carries an implicit
    /// quoting strategy and bashrs-domain semantics). `Type::Str`
    /// values are UTF-8 owned strings — Rust/Ruchy emit `String`,
    /// Lean emits `String`. v0.2.0 starts with owned-only; `&str`
    /// borrowing is the stretch sub-track 1.D.
    ///
    /// Currently produced by `depyler-frontend` for Python `str`
    /// annotations and `"..."` literals. Other frontends are free to
    /// produce it when they wire up real string parsing.
    ///
    /// Governing contract (Layer 2 translation, code lane): the v0.2.0
    /// `C-XLATE-PY-STR-TO-RUST-STRING` contract once authored.
    Str,
    /// Homogeneous list / vector. PMAT-455, v0.2.0 Track 1.B
    /// foundation (per [`sub/v0.2.0-depyler-merger.md`](../../docs/specifications/sub/v0.2.0-depyler-merger.md)).
    ///
    /// `Type::List(Box<Type>)` represents Python `list[T]` and lowers
    /// to Rust/Ruchy `Vec<T>` (owned, length-stable) and Lean `List T`.
    /// Heterogeneous lists are not supported at v0.2.0 — they'd
    /// require either `Box<dyn Any>`-style boxing (rejected by
    /// `C-XLATE-PY-LIST-TO-VEC`'s `heterogeneous_list_rejected`
    /// equation) or a Silver-tier sum type which is post-v0.2.0.
    ///
    /// At v0.2.0 first cut the element type is restricted to
    /// `Type::I64`; subsequent sub-tracks widen to `Type::Str`,
    /// `Type::Bool`, and nested `Type::List`. Other element types
    /// are explicitly rejected by the frontend with a clear error.
    ///
    /// Governing contract: `C-XLATE-PY-LIST-TO-VEC` (already QUORUM
    /// at depth-13).
    List(Box<Type>),
    /// POSIX shell string — quoted-aware string type for the bashrs
    /// domain. PMAT-046 / XPILE-BASHRS-MERGER-001 Layer B.
    ///
    /// Distinct from a generic `String` (which xpile doesn't have at
    /// v0.1.0): a `ShellString` value semantically carries an
    /// implicit `QuotingStrategy` and a shellcheck-equivalent
    /// validity claim. The v0.1.0 type is *load-bearing for future
    /// signatures* — when bashrs-frontend gains a real parser that
    /// types shell variables, it'll annotate them as
    /// `Type::ShellString`. Until then this variant is unused at the
    /// surface but present in the IR for the Bronze→Silver
    /// refinement of `C-BASHRS-POSIX-IDEMPOTENCE` (the typed POSIX
    /// state needed in `contracts/lean/Bashrs.lean` to model
    /// concrete shell semantics, not just abstract Outcomes).
    ///
    /// Other backends refuse `Type::ShellString` via `Unsupported`
    /// arms naming the bashrs contract.
    ShellString,
    /// POSIX exit code (i32 range, conventionally 0..=255). PMAT-046
    /// / XPILE-BASHRS-MERGER-001 Layer B.
    ///
    /// Same posture as `ShellString` — present for future signatures
    /// where shell-domain functions need a typed exit status (the
    /// Silver-tier Lean model's `Outcome` will carry an `ExitCode`
    /// field rather than just an opaque `observable: String`).
    ///
    /// Other backends refuse `Type::ExitCode` via `Unsupported` arms.
    ExitCode,
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
    /// Homogeneous list literal — Python `[1, 2, 3]`. PMAT-455,
    /// v0.2.0 Track 1.B foundation.
    ///
    /// All elements must type identically; heterogeneous literals are
    /// rejected at frontend lowering time per
    /// `C-XLATE-PY-LIST-TO-VEC::heterogeneous_list_rejected`. Empty
    /// list `[]` requires a type annotation upstream (since the
    /// frontend can't infer the element type from zero elements);
    /// v0.2.0 first cut requires non-empty list literals.
    ///
    /// Backends:
    ///   * Rust / Ruchy emit `vec![<elem>, <elem>, ...]`
    ///   * Lean emits `[<elem>, <elem>, ...]` (Lean's built-in
    ///     `List` literal syntax)
    ///   * Shell refuses (lists aren't a POSIX construct)
    ListLit(Vec<Expr>),
    /// String concatenation — Python `str + str` semantics. PMAT-451,
    /// v0.2.0 Track 1.A. Distinct from `BinOp::Add` because:
    ///   * No overflow concept (strings never overflow).
    ///   * Backend host varies: Rust/Ruchy emit `format!("{}{}", l, r)`;
    ///     Lean emits `l ++ r`.
    ///   * The contract substrate's associativity equation
    ///     (`C-XLATE-PY-STR-TO-RUST-STRING::concatenation_associativity`)
    ///     attaches to this variant, not to `BinOp::Add`.
    ///
    /// Produced by depyler-frontend when both operands of `+` are
    /// `Type::Str`-typed. f-string lowering (subsequent sub-track)
    /// generalises to `Vec<Expr>` parts via a `Concat { parts }`
    /// rewrite; the binary form is sufficient for v0.2.0 first cut.
    Concat { lhs: Box<Expr>, rhs: Box<Expr> },
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
    /// POSIX shell special parameter (`$1`..`$9`, `$0`, `$@`, `$*`,
    /// `$#`, `$?`, `$$`, `$!`, `$-`). PMAT-055.
    ///
    /// The carried `String` is the single-character name *without*
    /// the leading `$`. Distinct from `Expr::ShellVar` because
    /// special parameters are positional / runtime values set by
    /// the shell, not user-named variables. The bashrs-frontend
    /// parser tags them via this variant rather than ShellVar so
    /// future Lean refinement (Silver tier) can model them
    /// separately.
    ///
    /// bashrs-backend renders as `$<name>` (no braces — they're
    /// single-char so braces add nothing). Other backends refuse
    /// via `Unsupported(...)` naming `C-BASHRS-POSIX-IDEMPOTENCE`.
    ShellSpecial(String),
    /// Shell command substitution (`$(cmd)`). PMAT-047 /
    /// XPILE-BASHRS-MERGER-001 Layer B (Expr-side, composes Stmt
    /// into Expr).
    ///
    /// The nested `Box<Stmt>` is typically a `Stmt::Cmd` (or future
    /// `Stmt::Pipeline`). bashrs-backend renders by recursing into
    /// the inner statement and wrapping with `$(...)`.
    ///
    /// IR-shape only at v0.1.0: bashrs-frontend's hand-rolled parser
    /// doesn't recognise `$(...)` syntax yet (it'd require a real
    /// tokenizer that respects nested grouping and quoting). The
    /// v0.2.0 source fold's bashrs parser produces this variant
    /// from real shell input. Until then this is a *capability
    /// declaration* — the IR can carry it, the backend can render
    /// it, only the parser side is missing.
    ///
    /// Same cross-domain disposition: bashrs-only; other backends
    /// refuse with `Unsupported(...)` naming `C-BASHRS-POSIX-IDEMPOTENCE`.
    CommandSubstitution(Box<Stmt>),
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
