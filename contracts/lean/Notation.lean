/-
  Notation.lean — Lean 4 refinement proofs for
  `C-NOTATION-LATEX-MATH-TO-EQUATION`.

  This file is the proof-lane counterpart to
  `contracts/notation-latex-math-to-equation-v1.yaml` (PMAT-057).
  The yaml carries the *equations* describing how LaTeX math/theorem
  environments lower to xpile contract YAML; this file carries the
  *theorem* that locks in the modelling commitment for the
  `display_math_to_equation` equivalence claim.

  Cross-references:
    * Code lane:   crates/latex-contract-frontend/src/lib.rs
                   (parses .tex; produces contract YAML AST)
    * Contract:    contracts/notation-latex-math-to-equation-v1.yaml
    * Citation:    every emitted contract YAML carries
                   `# xpile-contract: C-NOTATION-LATEX-MATH-TO-EQUATION`
                   above its `metadata:` block (PMAT-011 idiom for
                   YAML hosts).
    * Roadmap:     docs/specifications/xpile-spec.md §3 (notation
                   contracts), audit-design.md §6.

  Tier (per ruchy 5.0 §14.10.5): refinement target is Bronze at
  v0.1.0 — `display_math_eq_equation_env` lowers all three LaTeX
  display-math forms (\\[ ... \\], \\begin{equation}, \\begin{align}) to
  the same abstract `EquationFormula` value, and the theorem
  reduces to `rfl` by our modelling choice. Silver tier (v0.3.0+)
  would refine `EquationFormula` to carry typed AST nodes that
  distinguish the three forms while preserving observable
  equivalence under the normaliser.

  This is the *second contract Lean theorem* the project has
  (PMAT-044's Bashrs.lean was the first). Same scaffold posture —
  documentary modelling commitment locked in by `rfl`.
-/

namespace XpileContracts.CNotationLatexMathToEquation

/--
  Abstract model of a `formula` value as it would land in an
  `equations:` block after LaTeX-frontend lowering. At v0.1.0 we
  represent it as the ASCII-normalised string content; Silver-tier
  refinement (XPILE-REFINE-NOTATION-***+) replaces this with a
  typed AST that distinguishes the three LaTeX display-math
  environments (`\[ ... \]`, `equation`, `align`).
-/
structure EquationFormula where
  ascii_normalised : String
deriving DecidableEq

/--
  Lower `\\[ formula \\]` (display-math span) to its `equations:`
  entry. v0.1.0 model: returns the formula's ASCII normalisation;
  the three display-math environments below all reduce to this.
-/
def lower_display_math (formula : String) : EquationFormula :=
  { ascii_normalised := formula }

/--
  Lower `\\begin{equation} formula \\end{equation}` to its
  `equations:` entry. Same model as above — `equation` is
  semantically identical to `\\[ ... \\]` per LaTeX conventions.
-/
def lower_equation_env (formula : String) : EquationFormula :=
  { ascii_normalised := formula }

/--
  Lower `\\begin{align} formula \\end{align}` to its `equations:`
  entry. `align` is multi-line capable but for a single-formula
  case it's also semantically identical.
-/
def lower_align_env (formula : String) : EquationFormula :=
  { ascii_normalised := formula }

/--
  **Refinement theorem** (the load-bearing claim of this file).

  All three LaTeX display-math forms lower to the same xpile
  `equations:` entry on the same `formula` input. Proof is `rfl`
  by our modelling choice — Bronze tier per ruchy 5.0 §14.10.5.

  Documentary value: any future change to one of the three
  lowering functions (e.g., adding multi-line normalisation to
  `lower_align_env` for proper align semantics) must either
  preserve `rfl`-equivalence with the other two OR invalidate
  this theorem (and `refinement_proofs.rs`'s citation gate fires).

  Falsification: if a future PR ships a latex-contract-frontend
  that lowers \\[...\\] and \\begin{equation}...\\end{equation} to
  syntactically distinct contract-YAML entries, *either* this
  theorem must be invalidated (gate fires) *or* the two paths
  stay artificially aligned and a runtime witness catches the
  divergence. Same Semantic + Runtime stratum cross-reinforcement
  as PMAT-044's bashrs theorem.

  Status: **discharged at v0.1.0 (PMAT-057)**. Tier: Bronze.
-/
theorem display_math_eq_equation_env_eq_align_env
    (formula : String) :
    lower_display_math formula
    = lower_equation_env formula
    ∧ lower_equation_env formula = lower_align_env formula := by
  refine ⟨?_, ?_⟩ <;> rfl

end XpileContracts.CNotationLatexMathToEquation
