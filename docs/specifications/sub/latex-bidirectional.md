# LaTeX Bidirectional Integration

**Section 25 of [xpile-spec.md](../xpile-spec.md).** Lane-level overview of LaTeX's role. For trait-level detail see [contract-frontend-trait.md](contract-frontend-trait.md) and [contract-backend-trait.md](contract-backend-trait.md).

## Proof lane only

LaTeX is **proof-lane-only**. Unlike Lean 4, LaTeX has no executable semantics — it is a typesetting and notation language. A `.tex` source describes mathematical statements, theorem environments, proof prose, and citations; none of those have runtime behavior. So LaTeX appears in xpile through `ContractFrontend` and `ContractBackend` exclusively.

Meta-HIR is not extended to model LaTeX math expressions. The proof-lane substrate (`EquationsBlock`) is.

## Scope: math + theorem environments

Per the 2026-05-15 design decision (#2), LaTeX support covers **both** math mode AND theorem-class environments from the beginning. Specifically:

**Math mode lowers to `equations:` entries.**
| LaTeX construct | Lowering |
|---|---|
| `$x + y$` (inline) | one entry in `equations:`, auto-named |
| `\(x + y\)` (alternative inline) | equivalent to `$...$` |
| `\[ x + y \]` (display) | one entry |
| `\begin{equation} x + y \end{equation}` | one entry, labeled if `\label{}` present |
| `\begin{align} x + y \\ z + w \end{align}` | **N entries** for N rows |
| `\begin{gather} ... \end{gather}` | one entry per row |

**Theorem-class environments lower to `proof_obligations:` entries.**
| LaTeX construct | Lowering |
|---|---|
| `\begin{theorem}` | `proof_obligations:` entry, type `postcondition` |
| `\begin{lemma}` | same shape |
| `\begin{corollary}` | same shape |
| `\begin{proposition}` | same shape |
| `\begin{claim}` | same shape |
| `\begin{definition}` | lowered to `equations:` (definitions ARE equations) |
| `\begin{remark}` with normative language | `falsification_tests:` entry |
| `\begin{proof}` | **NOT** serialized to YAML — pointer to Lean sidecar instead |

The proof body is the boundary: `\begin{proof} ... \end{proof}` content stays in the Lean lane, never in YAML. The contract YAML carries a pointer (`metadata.lean_pointer`) referencing the corresponding Lean theorem.

Layer 2 contract: [`contracts/notation-latex-math-to-equation-v1.yaml`](../../../contracts/notation-latex-math-to-equation-v1.yaml).

## Citation bridge: `\xpileContract` macro

xpile ships an `xpile-contracts.sty` package that `xpile-latex-contract-backend` vendors as a sidecar artifact. The package defines:

```latex
\newcommand{\xpileContract}[2]{%
  \label{xpile:#1:#2}%
  \edef\@currentlabelname{xpile:#1:#2}%
}
```

Usage in a rendered `.tex` file:

```latex
\xpileContract{C-XLATE-PY-LIST-TO-VEC}{homogeneous_list_to_vec}
\begin{theorem}
For any homogeneous list $xs$ of type $T$ with a canonical Rust counterpart,
the translation produces a $\mathtt{Vec}$ preserving order and length.
\end{theorem}
```

The `\label{xpile:C-XLATE-PY-LIST-TO-VEC:homogeneous_list_to_vec}` is structured. LaTeX's labeling infrastructure (`latexmk`, `biblatex`) indexes it natively. Refactoring the theorem environment body does NOT lose the citation, because the label precedes the environment and references the contract by ID — not by the theorem's content.

Decision recorded 2026-05-15, revised post-audit (`docs/specifications/audit-design.md` §4 "Citation Bridge Fragility"). The earlier approach embedded the contract ID in the theorem name; it was found regex-fragile. The current approach uses LaTeX's first-class label machinery — parsing is via `latexmk` / `pylatexenc`, not regex over body text.

## Directions

### LaTeX → contract (`latex-contract-frontend`)

`latex-contract-frontend` (planned crate) parses `.tex` source into `EquationsBlock`. Use cases:

- **Bootstrap a contract from a paper.** Researcher hands xpile an arXiv paper; the frontend extracts equations and theorems into a draft contract YAML. The team reviews, fills in `formal:` fields where needed, and commits.
- **Import equations from an existing publication.** A new transpilation kernel cites a published reference; xpile can ingest the LaTeX directly so the contract's equations match the paper verbatim.

The parser **must** use a real LaTeX parser (pulldown-latex, lalrpop-based, or pylatexenc-rs) — never regex over `.tex` text. Citation preservation (`\cite{}`, `\href{}`, `\xpileContract{}{}`, `% xpile-cite:` comments) is checked via the structured parser's output.

### Contract → LaTeX (`xpile-latex-contract-backend`)

`xpile-latex-contract-backend` (planned crate) renders any parsed `Contract` as publication-quality LaTeX. Use cases:

- **Generate the formal section of a paper directly from contracts.** Every equation becomes a numbered `\begin{equation}`; every proof obligation becomes a `\begin{theorem}` with `\xpileContract` cite.
- **Render an mdBook / paper companion side-by-side.** The same contract corpus renders to mdBook (via `xpile-mdbook-contract-backend`) for the manual and to LaTeX for the paper.

Falsification tests render as `\begin{remark}` blocks when `include_falsification: true`, and are silently omitted when false.

## What about `pdflatex` / `xelatex` / `lualatex`?

The contract backend emits LaTeX **source**, not compiled PDFs. Compilation is downstream — the user runs `latexmk` against the emitted `.tex`. xpile only certifies that the `.tex` is syntactically well-formed and that the `\xpileContract{}{}` macros are present where required; PDF appearance is the LaTeX toolchain's concern.

## Round-trip and CI

For the proof lane, round-trip is the canonical test:

1. Parse `paper.tex` via `latex-contract-frontend` → `EquationsBlock`
2. Construct a full `Contract` from the block
3. Render via `xpile-latex-contract-backend` → `paper-roundtrip.tex`
4. Diff `paper.tex` and `paper-roundtrip.tex` modulo whitespace

A clean diff means lossless round-trip. CI runs this on a corpus of fixture papers (one short, one medium, one long); contract violations break the build.

## Open issues

1. **Mathematical notation outside KaTeX subset.** Some LaTeX math features (commutative diagrams, `tikz-cd`, custom macros from `\usepackage`) don't lower cleanly to xpile equation YAML. The parser flags these with `EquationsBlock.unhandled_macros: Vec<String>` and the contract author decides whether to expand them, transcribe them as `formal: TBD`, or carry them as opaque text in `references:`.
2. **BibTeX/biblatex integration.** A contract's `references:` field may include BibTeX entries lifted from the paper's `.bib` file. The backend re-emits them as sidecars when rendering; the frontend extracts them via a real `.bib` parser, not regex.
3. **Coq / Agda symmetry.** LaTeX's notation role overlaps with what Coq's notation system and Agda's mixfix operators provide. Future-`ContractFormat::Coq` / `::Agda` may share the LaTeX parser stack — design TBD.

## See also

- [contract-frontend-trait.md](contract-frontend-trait.md) — the `ContractFrontend` trait LaTeX implements
- [contract-backend-trait.md](contract-backend-trait.md) — the `ContractBackend` trait, including the citation bridge (`\xpileContract` macro)
- [`contracts/notation-latex-math-to-equation-v1.yaml`](../../../contracts/notation-latex-math-to-equation-v1.yaml) — Layer 2 proof-lane contract for LaTeX parsing
- [lean-bidirectional.md](lean-bidirectional.md) — sibling sub-spec for the proof lane's other supported notation language
