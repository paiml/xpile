# Contract Frontend Trait

**Section 2b of [xpile-spec.md](../xpile-spec.md).** Sibling to [frontend-trait.md](frontend-trait.md) but operates on the **proof lane**, not the code lane.

## Two-lane recap

xpile has two parallel pipelines that meet at the contract substrate:

- **Code lane:** `Frontend` parses source → meta-HIR; `Backend` lowers meta-HIR → target code/IR
- **Proof lane:** `ContractFrontend` parses notation → contract equations; `ContractBackend` renders contracts → notation/proofs

`Frontend`/`Backend` model **executable code**. `ContractFrontend`/`ContractBackend` model **notation and proofs** (LaTeX math, Lean theorems, mdBook). The two lanes don't fight: a single source file (e.g., a `.lean` file) can be parsed by both — `lean-frontend` extracts executable code into meta-HIR, while `lean-contract-frontend` (future) extracts theorem statements into equation YAML.

## Definition

```rust
pub trait ContractFrontend: Send + Sync {
    fn name(&self) -> &'static str;
    fn formats(&self) -> &[ContractFormat];
    fn parse_to_equations(&self, source: &str) -> Result<EquationsBlock, ContractFrontendError>;
}

pub enum ContractFormat {
    LatexMath,         // LaTeX math mode + theorem/proof/lemma environments
    LeanTheorem,       // Lean 4 theorem text (read-only — Lean is also code-lane)
    MdBook,            // Markdown with embedded math (existing pv format)
    Coq,               // future
    Agda,              // future
    Isabelle,          // future
}

pub struct EquationsBlock {
    pub equations: BTreeMap<String, Equation>,
    pub proof_obligations: Vec<ProofObligation>,
    pub references: Vec<Reference>,
    pub citations: Vec<ContractId>,    // contract IDs cited by the source notation
}
```

Three methods — same shape as `Frontend`. The trait is narrow because everything downstream (`pv` validation, falsification harnesses, contract composition) is shared.

## Invariants

To be encoded in `contracts/xpile-contract-frontend-trait-v1.yaml` (Layer 3 architectural — to author):

| Invariant | What it asserts |
|---|---|
| `format_ownership` | No two contract frontends declare the same `ContractFormat` variant |
| `parse_idempotency` | `hash(parse(s)) == hash(parse(s))` — deterministic, canonical YAML serialization |
| `equations_only` | `ContractFrontend::parse_to_equations` MUST NOT produce or mutate any `xpile_meta_hir::Module`; that is `Frontend`'s job. Mixing the lanes pollutes meta-HIR with non-executable artifacts. |
| `citation_preservation` | Every contract ID present in the source via a **structured citation construct** — `\cite{C-PY-INT-ARITH}` and `\xpileContract{C-PY-INT-ARITH}{...}` in LaTeX, `@[xpile_contract "C-PY-INT-ARITH"]` attribute in Lean, `<!-- xpile-contract: C-PY-INT-ARITH -->` in mdBook — MUST appear in `EquationsBlock.citations`. Parsers MUST use the host format's structured parser (Lean elaborator, LaTeX label machinery, mdBook preprocessor), NOT regex over body text. Revised post-2026-05-15 audit (`docs/specifications/audit-design.md` §4 "Citation Bridge Fragility"). |

`citation_preservation` is load-bearing for the audit chain. The proof lane must round-trip cleanly: if a paper cites contract `C-X`, parsing → re-rendering must preserve that citation.

## Implementations at v0.1.0 and planned

| Crate | Struct | Format(s) | Status |
|---|---|---|---|
| `latex-contract-frontend` | `LatexContractFrontend` | `LatexMath` | Planned (Phase 4) — see [latex-bidirectional.md](latex-bidirectional.md) |
| `lean-contract-frontend` | `LeanContractFrontend` | `LeanTheorem` | Planned (Phase 4) — read-only Lean theorem extraction |
| `mdbook-contract-frontend` | `MdBookContractFrontend` | `MdBook` | Inherited from `pv`; vendored wrapper at Phase 3 |

No contract frontends ship at v0.1.0. The trait exists in `xpile-contract-frontend` (to scaffold) and is consumed by `xpile-core::TranspileSession` when a `.tex` or `.lean` file is supplied alongside a contract directory.

## LaTeX scope — math + theorem

Per the foundational design decision (locked 2026-05-15), `latex-contract-frontend` handles both:

- **Math mode:** `$...$`, `\[...\]`, `\begin{equation}...\end{equation}`, `\begin{align}...\end{align}`, `\begin{gather}...\end{gather}`
- **Theorem environments:** `\begin{theorem}`, `\begin{lemma}`, `\begin{corollary}`, `\begin{proposition}`, `\begin{definition}`, `\begin{remark}`, `\begin{proof}`

Math-mode content lowers to `equations:` entries; theorem-environment content lowers to `proof_obligations:` entries; `\begin{proof}` content lowers to a Lean theorem pointer (the proof lives in the Lean lane, not the contract YAML). See `contracts/notation-latex-math-to-equation-v1.yaml` for the full mapping.

## Lean read-only in the proof lane

Per decision (Lean 4 only, all constructs in scope), Lean has dual citizenship:

- **Code-lane:** `lean-frontend` (a regular `Frontend`) parses `.lean` files for executable constructs (`def`, `partial def`, `inductive`, `structure`, `instance`, etc.) → meta-HIR
- **Proof-lane:** `lean-contract-frontend` (a `ContractFrontend`) parses the same `.lean` files for **theorem statements only** — extracts `theorem`, `lemma`, `example`, `axiom` declarations → `proof_obligations:`

The proof lane is *read-only* for Lean: xpile does not generate Lean from notation; Lean theorem text is canonical when written. `lean-contract-frontend` exists for ingest (e.g., when a contract is bootstrapped from an existing Lean library).

## Why object-safe

Same reasoning as `Frontend`:

- No associated types (returns concrete `EquationsBlock`)
- All methods take `&self`
- `Send + Sync` for cross-thread sessions

Dispatched via `&dyn ContractFrontend` in `xpile-core::TranspileSession::contract_frontends`.

## Adding a new contract frontend

Mirrors the [frontend-onboarding.md](frontend-onboarding.md) checklist, abbreviated to the proof lane:

1. Add a variant to `ContractFormat`
2. Create `crates/<format>-contract-frontend/`
3. Implement `ContractFrontend`
4. Wire `parse_to_equations` over a fixture notation file
5. Author a Layer-2 notation contract (e.g., `notation-coq-to-equation-v1.yaml`)
6. Author an architectural obligation (idempotency, citation preservation)
7. Add a round-trip test: parse → render through the matching `ContractBackend` → diff
