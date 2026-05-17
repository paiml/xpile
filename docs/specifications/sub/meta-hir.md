# Canonical Meta-HIR

**Section 3 of [xpile-spec.md](../xpile-spec.md).**

## v0.1.0 shape (post-substrate-completion, PMAT-058..092)

```rust
pub struct Module {
    pub name: String,
    pub source_lang: SourceLang,
    pub items: Vec<Item>,
    pub ffi_boundaries: Vec<FfiBoundary>,
}

pub enum SourceLang {
    Python,
    C,
    Cpp,
    Cuda,
    Ruchy,
    Rust,    // keystone for bidirectional Rust↔Ruchy + Rust→GPU
    Lean,    // executable Lean 4 subset (def / inductive / structure / instance / ...)
    Shell,   // POSIX sh / bash / zsh / Makefile / Dockerfile — bashrs merger domain
}

pub enum Item {
    Function(Function),
}

pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Block,
}

pub struct Block {
    pub stmts: Vec<Stmt>,
}

pub enum Stmt {
    Let { name: String, ty: Option<Type>, value: Expr },
    Return(Expr),
    Expr(Expr),
    If { cond: Expr, then_branch: Block, else_branch: Option<Block> },
    While { cond: Expr, body: Block },
    // Layer B shell variants (PMAT-039..056) — produced only by bashrs-frontend,
    // consumed only by bashrs-backend; other backends return Unsupported.
    Cmd { program: String, args: Vec<Expr> },                    // PMAT-039
    Pipeline { stages: Vec<Stmt> },                              // PMAT-041
    ShellLoop { kind: LoopKind, var: String, body: Block },      // PMAT-048
    ShellAssign { name: String, value: Expr },                   // PMAT-051
}

pub enum Expr {
    LitInt(i64),
    LitBool(bool),
    Ident(String),
    BinOp { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    UnaryOp { op: UnaryOp, operand: Box<Expr> },
    Call { name: String, args: Vec<Expr> },
    IfExpr { cond: Box<Expr>, then_expr: Box<Expr>, else_expr: Box<Expr> },
    // Layer B shell-side expressions
    LitStr(String),                                              // PMAT-039
    QuotedString { content: String, quoting: QuotingStrategy },  // PMAT-042
    ShellVar(String),                                            // PMAT-045
    CommandSubstitution(Box<Stmt>),                              // PMAT-047
    ShellSpecial(String),                                        // PMAT-055
}

pub enum Type {
    I64,
    Bool,
    BigInt,
    Tuple(Vec<Type>),
    Unit,
    // Layer B shell types
    ShellString,                                                 // PMAT-046
    ExitCode,                                                    // PMAT-046
}

pub enum QuotingStrategy { Single, Double, Backslash }
pub enum LoopKind { For, While, Until }

pub struct FfiBoundary {
    pub from_lang: SourceLang,
    pub to_lang: SourceLang,
    pub symbol: String,
    pub signature: String,
}
```

The Layer B shell variants were added across PMAT-039..056 as part of the bashrs merger (see [bashrs-merger.md](bashrs-merger.md)). Other backends explicitly reject these variants via `Unsupported` arms naming `C-BASHRS-POSIX-IDEMPOTENCE` — the load-bearing cross-domain dispatch boundary. The IR carries shell semantics first-class, not as escape hatches.

## Federated → unified (the 2026-05-17 reversal)

Originally meta-HIR was designed as a **coordination layer** — each frontend kept its own internal HIR, and meta-HIR was the minimum shape needed for shared infrastructure to dispatch correctly. The bashrs merger reversed that — meta-HIR now grows native variants for each domain rather than carrying them as external HIR references. The bashrs Layer B variants (PMAT-039..056) demonstrate the new posture: shell semantics live in meta-HIR, not in a separate ShellIR.

Why the reversal:

1. **Cross-domain refinement requires it.** `depyler-frontend`'s `subprocess.run([...])` recognition (PMAT-040) lowers to `Stmt::Cmd` — that's a Python frontend producing a shell-domain meta-HIR variant. If shell variants lived outside meta-HIR, this cross-domain composition would be impossible.
2. **The "everything above meta-HIR is shared" claim only holds if meta-HIR is the true union.** A federated approach leaves a federation seam at every dispatch site.
3. **Contracts compose at the IR level.** `C-BASHRS-POSIX-IDEMPOTENCE` constrains shell semantics regardless of which frontend produces them. The contract substrate's quality regime ($14.4 N-of-M QUORUM, 12 contracts at 100%) only holds end-to-end because the IR is unified.

The federation-era reasoning is preserved in git history of `sub/bashrs-federation.md` (the file was renamed to `sub/bashrs-merger.md` post-reversal). Per Popperian falsification, the federation hypothesis was that cross-domain composition could be deferred indefinitely; the PMAT-040 cross-domain consumer falsified it within weeks of the bashrs merger landing.

## Determinism requirement

Meta-HIR must serialize canonically (BTreeMap-ordered, no HashMap iteration). Reason: the cache key in [cache-determinism-provenance.md](cache-determinism-provenance.md) hashes serialized meta-HIR; non-deterministic hash inputs would break the determinism contract.

The trait determinism for `Frontend::parse_and_lower` is covered by `C-XPILE-FRONTEND-TRAIT` at full §14.4 QUORUM (PMAT-062/063); same for `Backend::lower` via `C-XPILE-BACKEND-TRAIT` (PMAT-064/065). Both are byte-identity Bronze-tier modelling at v0.1.0; Silver-tier refinement introduces canonical-equivalence under whitelisted dynamic regions.

## Growth trajectory

| Trigger | Meta-HIR addition | Status |
|---|---|---|
| Bashrs merger lands | `SourceLang::Shell` + Layer B shell variants | **shipped** (PMAT-037..056) |
| First hybrid Python+shell demo | `subprocess.run` recognition + cross-domain dispatch | **shipped** (PMAT-040) |
| First hybrid Python+C demo | `FfiBoundary` type carrier | scaffold present; real wiring is post-v0.1.0 |
| Generators in scope | Coroutine-state representation | post-v0.1.0 |
| Async support | Future/Promise canonical form | post-v0.1.0 |
| CUDA frontend lands | Device-kernel-launch construct | post-v0.1.0; Layer-5 PTX compile contract already at QUORUM (PMAT-074/075) |
| Lean parser lands | `SourceLang::Lean` lowering path | post-v0.1.0; `SourceLang::Lean` variant present, parser is XPILE-LEAN-FRONTEND-001 future work |

Each addition is a contract: `xpile-meta-hir-vN.yaml` versions the IR shape, and `pv diff` detects breaking changes.
