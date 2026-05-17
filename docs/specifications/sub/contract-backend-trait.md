# Contract Backend Trait

**Section 5c of [xpile-spec.md](../xpile-spec.md).** Sibling to [backend-trait.md](backend-trait.md) but operates on the **proof lane**.

## Definition

```rust
pub trait ContractBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn formats(&self) -> &[ContractFormat];
    fn render(
        &self,
        contract: &Contract,
        config: &ContractRenderConfig,
    ) -> Result<RenderedDoc, ContractBackendError>;
}

pub struct ContractRenderConfig {
    pub format: ContractFormat,
    pub embed_citation: bool,            // see citation bridge below
    pub include_falsification: bool,     // emit falsification tests as remarks/lemmas
    pub lean_version: Option<(u32, u32)>, // (4, _) only; reserved for future variants
}

pub struct RenderedDoc {
    pub primary: String,                  // .tex / .lean / .md text
    pub sidecars: Vec<(String, Vec<u8>)>, // figures, bibtex entries, oleantheorem files
    pub citations: Vec<ContractId>,       // every rendered statement cites its contract
}
```

Three methods — symmetric with `ContractFrontend`. `Contract` (defined in `xpile-contracts`) is the parsed YAML; `ContractBackend::render` produces deterministic notation.

## Invariants

To be encoded in `contracts/xpile-contract-backend-trait-v1.yaml` (Layer 3 architectural — to author):

| Invariant | What it asserts |
|---|---|
| `format_ownership` | No two contract backends declare the same `ContractFormat` variant |
| `render_idempotency` | `hash(render(c, cfg)) == hash(render(c, cfg))` — deterministic output, no timestamps in default config |
| `citation_round_trip` | Every contract ID in `Contract.depends_on` and `Contract.references` MUST appear in `RenderedDoc.citations` (and, when `embed_citation: true`, also via a structured attribute in `RenderedDoc.primary`) |
| `citation_via_structured_attribute` | For every supported format, citations MUST be embedded via a **format-native structured construct** (Lean attribute, LaTeX macro, mdBook structured comment) — NOT via regex over text. See [Citation bridge](#citation-bridge-decision-4-revised-post-audit) below |
| `falsification_render_optional` | When `include_falsification: false`, no falsification tests appear in output; when `true`, every test surfaces as a `\begin{remark}` (LaTeX) or `-- FALSIFY-<ID>` comment block (Lean) |

## Citation bridge (decision #4 — revised post-audit)

The 2026-05-15 audit (`docs/specifications/audit-design.md` §4 "Citation Bridge Fragility") flagged that text/regex-based citation is structurally brittle: manual rename in the proof lane silently breaks `citation_round_trip`. The revised approach uses **format-native structured constructs** parsed by the host language's elaborator/parser, not by regex over body text.

### Lean 4: custom attribute

xpile ships a small library `XpileContracts.Attr` (lives in the `xpile-lean-contract-backend` crate as a vendored `.lean` preamble) that all xpile-generated Lean files import:

```lean
import XpileContracts.Attr

@[xpile_contract "C-XLATE-PY-LIST-TO-VEC", xpile_equation "homogeneous_list_to_vec"]
theorem homogeneous_list_to_vec
    {T : Type} [HasRustEquiv T]
    (xs : List T)
    : xlate xs = .ok (Vec.ofList (xs.map toRust)) := by
  ...
```

The attribute is parsed by Lean's elaborator. Malformed citations fail at *compile time*. Renaming the theorem does not break the citation — the contract ID lives in the attribute, not the theorem name. Tooling queries the attribute table via Lean's metaprogramming API (`Lean.Meta`), not by regex.

An optional `namespace XpileContracts.<id_underscored>` is still allowed as documentation grouping, but is **no longer load-bearing** for `citation_round_trip`. Auditors and tooling must read the attribute, not the namespace.

### LaTeX: structured macro + label

xpile ships an `xpile-contracts.sty` package (vendored by `xpile-latex-contract-backend`):

```latex
\newcommand{\xpileContract}[2]{%
  \label{xpile:#1:#2}%
  \edef\@currentlabelname{xpile:#1:#2}%
}
```

Usage:

```latex
\xpileContract{C-XLATE-PY-LIST-TO-VEC}{homogeneous_list_to_vec}
\begin{theorem}
For any homogeneous list $xs$ of type $T$ with a canonical Rust counterpart...
\end{theorem}
```

The `\label{xpile:C-XLATE-PY-LIST-TO-VEC:homogeneous_list_to_vec}` is structured. `latexmk`, `biblatex`, and standard LaTeX cross-reference tooling index it. Renaming the theorem environment doesn't lose the citation because the label precedes it.

### mdBook: structured HTML comment

mdBook lacks native attribute support, so xpile uses a strict comment grammar parsed by an `xpile-mdbook-preprocessor`:

```markdown
<!-- xpile-contract: C-XLATE-PY-LIST-TO-VEC -->
<!-- xpile-equation: homogeneous_list_to_vec -->

**Theorem.** For any homogeneous list $xs$ of type $T$...
```

Comments use a fixed `key: value` grammar (not free text). The preprocessor parses them in a single pass and registers them in a sidecar JSON index. Bare-regex misuse is rejected at preprocess time.

### Why all three are rename-robust

| Format | Citation lives in | Renaming the theorem/section keeps cite? |
|---|---|---|
| Lean 4 | `@[xpile_contract ...]` attribute on the decl | Yes — attribute outlives the decl name |
| LaTeX | `\xpileContract{...}` macro before env | Yes — label outlives env contents |
| mdBook | `<!-- xpile-contract: ... -->` comment block | Yes — comment outlives heading text |

`citation_round_trip` is now strengthened: every contract ID in `Contract.depends_on` and `Contract.references` MUST be parseable from the rendered document **via the host format's structured parser** (Lean elaborator, LaTeX label machinery, mdBook preprocessor) — NOT by regex over text body. A failing structural parse is a hard contract violation; missing-but-recoverable-by-regex is no longer acceptable.

## Implementations at v0.1.0 and planned

| Crate | Struct | Format(s) | Status |
|---|---|---|---|
| `xpile-latex-contract-backend` | `LatexContractBackend` | `LatexMath` | **shipped as a workspace crate** (scaffold-stage `render` body; real LaTeX rendering is post-v0.1.0). See [latex-bidirectional.md](latex-bidirectional.md). |
| `xpile-lean-contract-backend` | `LeanContractBackend` | `LeanTheorem` | **shipped as a workspace crate** (scaffold-stage `render` body; the citation-bridge attribute `@[xpile_contract "C-..."]` is the load-bearing spec'd shape, real emission is post-v0.1.0). |
| `xpile-mdbook-contract-backend` | `MdBookContractBackend` | `MdBook` | Inherited from `pv`; vendored wrapper post-v0.1.0. |

At v0.1.0 the `ContractBackend` trait lives in `xpile-contract-backend` (real trait, not a scaffold — the trait determinism invariant is covered by `C-XPILE-CONTRACT-BACKEND-TRAIT` at full §14.4 QUORUM via PMAT-068 / PMAT-069). Both `xpile-latex-contract-backend` and `xpile-lean-contract-backend` are workspace crates with the trait wired; the `render` method bodies are scaffold-stage at v0.1.0.

## Lean version (decision #1)

`lean_version: Some((4, _))` only. Lean 3 is end-of-life and Mathlib has fully migrated; xpile does not target Lean 3 even read-only. `LeanContractBackend::render` returns `Err(ContractBackendError::UnsupportedLeanVersion)` if the config requests Lean 3.

## Lean scope (decision #3)

All Lean 4 constructs are in scope as **render targets**:

- `def`, `partial def`, `theorem`, `lemma`, `example`
- `inductive`, `structure`, `instance`
- `axiom` (with explicit warning comment)
- `noncomputable def` (when contract equations involve classical reasoning)

`partial def` rendering injects a `partial` keyword automatically when the contract's `equations.<name>.invariants` does not include a termination claim. `axiom` rendering inserts a banner comment `-- ⚠ AXIOM: not proven, contract <ID> assumes this is sound` so reviewers see it.

## Proof lane vs code lane: a Lean file may appear in both

A `.lean` file produced by xpile may be the output of:

- **Code lane:** `xpile-lean-codegen` (a `Backend`) emitting executable Lean from meta-HIR — `def foo (x : Nat) : Nat := ...`
- **Proof lane:** `xpile-lean-contract-backend` (a `ContractBackend`) emitting theorem statements about that code — `theorem foo_correct : ∀ x, foo x = expected x := ...`

Both can land in the same `.lean` file, separated by section markers:

```lean
import XpileContracts.Attr

section CodeLane
-- emitted by xpile-lean-codegen from meta-HIR
def foo (x : Nat) : Nat := x + 1
end CodeLane

-- emitted by xpile-lean-contract-backend from contract YAML
@[xpile_contract "C-XLATE-PY-INT-ARITH", xpile_equation "addition_no_overflow"]
theorem addition_no_overflow : ∀ x, foo x = x + 1 := by
  intro x; rfl
```

`xpile-core::TranspileSession` orchestrates the merge.

## Why object-safe

Same reasoning as `Backend`:

- No associated types (returns concrete `RenderedDoc`)
- All methods take `&self`
- `Send + Sync`

Dispatched via `&dyn ContractBackend` in `xpile-core::TranspileSession::contract_backends`.

## Adding a new contract backend

Mirrors the [backend-trait.md](backend-trait.md) checklist:

1. Add a variant to `ContractFormat` (if not already present from a `ContractFrontend`)
2. Create `crates/xpile-<format>-contract-backend/`
3. Implement `ContractBackend`
4. Wire `render` over the existing contract corpus
5. Author a Layer-2 contract for the rendering rules (e.g., `xlate-contract-to-coq-v1.yaml`)
6. Author the citation-bridge obligation specific to the format
7. Round-trip test: render a contract → parse it back via the matching `ContractFrontend` → diff equation YAML
