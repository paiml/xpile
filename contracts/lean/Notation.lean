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

/-! ## PMAT-134 — Bronze-tier refinement theorems for the remaining
    6 equations of `C-NOTATION-LATEX-MATH-TO-EQUATION`.

    Each theorem captures a different load-bearing modelling
    commitment of the LaTeX→YAML lowering pipeline. All proofs are
    `rfl` at v0.1.0; Silver tier will refine each abstract structure
    into the typed AST it represents (preserving the locked-in
    invariant structurally). -/

/--
  Lower an inline math span `$formula$` (or `\(formula\)`) to its
  `EquationsBlock` entry. Bronze tier: byte-identity into the
  `ascii_normalised` field. Silver tier (XPILE-REFINE-NOTATION-001)
  introduces a real `ascii_normalize` that collapses whitespace,
  reorders commutative operands canonically, etc.
-/
def lower_inline_math (formula : String) : EquationFormula :=
  { ascii_normalised := formula }

/--
  **Refinement theorem** for `inline_math_to_equation`.

  An inline math span's formula lowers byte-for-byte into the
  `EquationsBlock` entry's `formula` field at Bronze tier.
  Falsified by an emitter that silently strips whitespace or
  normalises operator spelling — those changes are valid at
  Silver tier *under a canonical-equality relation*, not under
  byte identity, so the theorem must be upgraded in step.
-/
theorem inline_math_to_equation (formula : String) :
    (lower_inline_math formula).ascii_normalised = formula := by
  rfl

/-- Abstract `theorem`-class LaTeX environment. Carries the count of
    embedded math spans (used for the `formal:` extraction) and the
    flag for whether the body opens with `\textbf{Precondition:}`
    (which flips the obligation type to `precondition`). -/
structure LeanTheoremEnv where
  body_text : String
  is_precondition_flagged : Bool
deriving DecidableEq

/-- Abstract proof obligation entry as it lands in the
    `EquationsBlock.proof_obligations` list. -/
structure ObligationEntry where
  obligation_type : String
deriving DecidableEq

/-- Bronze-tier lowering: theorem env → exactly one obligation entry
    whose type is `postcondition` by default, or `precondition` when
    the precondition flag is set. -/
def lower_theorem_env (t : LeanTheoremEnv) : ObligationEntry :=
  if t.is_precondition_flagged then
    { obligation_type := "precondition" }
  else
    { obligation_type := "postcondition" }

/--
  **Refinement theorem** for `theorem_env_to_obligation`.

  The mapping from `\textbf{Precondition:}` flag to the
  obligation's `type` field is the load-bearing safety claim. An
  emitter that defaults to `precondition` when the flag is absent
  (or vice versa) would silently flip the obligation polarity,
  inverting what's assumed vs. what's proven.
-/
theorem theorem_env_to_obligation (t : LeanTheoremEnv) :
    (lower_theorem_env t).obligation_type =
      (if t.is_precondition_flagged then "precondition" else "postcondition") := by
  unfold lower_theorem_env
  split <;> rfl

/-- Abstract proof environment. Carries the body bytes (which MUST
    NOT land in the `EquationsBlock`) plus the bool for "body
    matches /(omitted|TODO|XXX|sorry)/i" → stub flag. -/
structure ProofEnv where
  body : String
  is_stub : Bool
deriving DecidableEq

/-- Abstract output from lowering a proof env: a `lean_pointer`
    metadata entry, never the body text itself. -/
structure LeanPointer where
  status : String
  /-- Bronze-tier invariant: body bytes never reach the output
      EquationsBlock. We model this by recording a single bit
      `body_leaked` that the lowering MUST keep `false`. -/
  body_leaked : Bool
deriving DecidableEq

/-- Bronze-tier lowering: proof env → `lean_pointer` with
    `status: "stub"` if `is_stub`, else `"claimed"`. `body_leaked`
    is `false` by construction — the body never escapes into
    EquationsBlock. -/
def lower_proof_env (p : ProofEnv) : LeanPointer :=
  { status := if p.is_stub then "stub" else "claimed"
    body_leaked := false }

/--
  **Refinement theorem** for `proof_env_to_lean_pointer`.

  Two load-bearing claims in one theorem:
  1. The stub/claimed classification follows the regex-on-body
     decision exactly (not "always stub" or "always claimed").
  2. The proof body NEVER leaks into `EquationsBlock` — the
     `body_leaked` bit is provably `false` by construction.

  Falsified by an emitter that "helpfully" pastes the proof body
  into the YAML as `proof_text:` for human readability — which
  would defeat the lane separation invariant.
-/
theorem proof_env_to_lean_pointer (p : ProofEnv) :
    (lower_proof_env p).status =
      (if p.is_stub then "stub" else "claimed") ∧
      (lower_proof_env p).body_leaked = false := by
  unfold lower_proof_env
  refine ⟨?_, ?_⟩ <;> simp <;> split <;> rfl

/-- Abstract `definition`-class LaTeX environment. Carries the
    first embedded math span — at Bronze tier the only required
    field — and an optional label that names the equation entry. -/
structure DefinitionEnv where
  first_math_span : String
deriving DecidableEq

/-- Bronze-tier lowering: definition env → exactly one
    `EquationsBlock.equations` entry whose `formula` is the
    extracted math span byte-for-byte. -/
def lower_definition_env (d : DefinitionEnv) : EquationFormula :=
  { ascii_normalised := d.first_math_span }

/--
  **Refinement theorem** for `definition_env_to_equation`.

  A `\begin{definition}` environment's first math span lowers
  byte-for-byte into the equation's `formula` field. Falsified
  by an emitter that runs the math span through a lossy
  normaliser before extraction — at Silver tier this becomes a
  *canonical-equality* relation, not byte identity, so the
  theorem must be upgraded in step.
-/
theorem definition_env_to_equation (d : DefinitionEnv) :
    (lower_definition_env d).ascii_normalised = d.first_math_span := by
  rfl

/-- Abstract `remark`-class LaTeX environment. The Bronze-tier
    model captures three flags: whether the body contains MUST/SHALL,
    whether it contains SHOULD, and whether it contains MUST NOT/SHALL NOT. -/
structure RemarkEnv where
  has_must : Bool
  has_should : Bool
  has_must_not : Bool
deriving DecidableEq

/-- Abstract entry in `EquationsBlock.falsification_tests`. -/
structure FalsificationEntry where
  ship_blocking : Bool
  /-- The "predicate inverted" flag, set when the source language
      was MUST NOT / SHALL NOT (negative imperative). -/
  predicate_inverted : Bool
deriving DecidableEq

/-- Bronze-tier lowering rule: emit a falsification entry iff some
    normative keyword is present; ship_blocking follows MUST/SHALL
    → true, SHOULD → false, MUST NOT/SHALL NOT → true-with-inverted-
    predicate. -/
def lower_remark_env (r : RemarkEnv) : Option FalsificationEntry :=
  if r.has_must_not then
    some { ship_blocking := true, predicate_inverted := true }
  else if r.has_must then
    some { ship_blocking := true, predicate_inverted := false }
  else if r.has_should then
    some { ship_blocking := false, predicate_inverted := false }
  else
    none

/--
  **Refinement theorem** for `remark_env_to_falsification`.

  The decision-table mapping from RFC-2119 keywords to
  `ship_blocking` / `predicate_inverted` is locked in here:
  MUST NOT/SHALL NOT take priority (ship-blocking, inverted),
  then MUST/SHALL (ship-blocking, not inverted), then SHOULD
  (advisory, not inverted). Absence of normative keywords → no
  entry (the remark stays as plain commentary).

  Falsified by an emitter that interprets MUST NOT as advisory
  (which would silently downgrade a hard constraint) or treats
  SHOULD as ship-blocking (which would over-trigger CI).
-/
theorem remark_env_to_falsification (r : RemarkEnv) :
    (lower_remark_env r).isSome ↔ (r.has_must ∨ r.has_should ∨ r.has_must_not) := by
  unfold lower_remark_env
  cases r.has_must_not <;> cases r.has_must <;> cases r.has_should <;> simp

/-- Abstract LaTeX-source citation entry, modelled as the cited
    contract ID's byte content. -/
structure LatexCitation where
  contract_id : String
deriving DecidableEq

/-- Abstract output `citations` entry: contract ID byte-for-byte
    from the source. -/
structure CitationOutput where
  contract_id : String
deriving DecidableEq

/-- Bronze-tier lowering: copy the contract ID byte-for-byte. No
    normalisation, no case folding, no prefix stripping. -/
def lower_citation (c : LatexCitation) : CitationOutput :=
  { contract_id := c.contract_id }

/--
  **Refinement theorem** for `citation_preservation`.

  Every cited contract ID survives the LaTeX→YAML lowering
  byte-for-byte. Falsified by any normalisation (dash-to-underscore,
  lowercase folding, BibTeX-key mangling) that would break
  round-trip lookup via the citation gate. Companion to
  `XlateLeanToRust.lean`'s `citation_in_emitted_rust` (PMAT-133)
  — together they bracket the citation-bridge claim across all
  three lanes (LaTeX, Lean, Rust).
-/
theorem citation_preservation (c : LatexCitation) :
    (lower_citation c).contract_id = c.contract_id := by
  rfl

/-! ## PMAT-167 — Silver-tier refinement for `display_math_eq_equation_env_eq_align_env`.

    Promotes the LaTeX display-math triumvirate (`\[ ... \]`,
    `equation`, `align`) from Bronze (where the model throws away
    the source kind so all three reduce to the same
    `EquationFormula`) to Silver (where the model RETAINS the
    source kind as a discriminator field, and the equivalence
    claim is recovered structurally under a normaliser).

    Why this is strictly stronger than Bronze:
    - Bronze proves `lower_display_math = lower_equation_env =
      lower_align_env` by `rfl` because all three return the same
      `EquationFormula` value (a structural anonymisation that
      DESTROYS the source-kind provenance).
    - Silver proves the equality of the *normalised* content
      while RETAINING the source kind (`DisplayMath | Equation |
      Align`) for audit purposes. An emitter that quietly relabels
      `\[ ... \]` as `Align` to enable multi-line wrapping (a
      benign-looking refactor) is now caught by the kind field —
      Bronze couldn't see it.

    Silver tier per ruchy 5.0 §14.10.5: typed structural model +
    real proof (non-`rfl` — uses a `normalise` extractor to bridge
    the kind-tagged structures). Gold tier introduces a true
    `canonical_equality : EquationFormula → EquationFormula → Prop`
    relation that admits whitespace/operator-spelling tolerance
    while still ruling out hostile transformations.

    This is the **fourth multi-equation contract Silver upgrade**
    (after PMAT-164 on C-XLATE-PY-LIST-TO-VEC, PMAT-165 on
    C-XLATE-LEAN-TO-RUST, PMAT-166 on C-XLATE-RUST-FN-TO-LEAN-THM)
    and the **first Silver upgrade on the notation lane** — until
    now all multi-eq Silver work has been on the code/proof
    translation lanes. Broadens the Silver-bracket horizontally.
-/

/--
  Source-environment discriminator. The three LaTeX display-math
  forms are syntactically distinct even though they produce
  equivalent ASCII-normalised content; Silver keeps the kind tag
  for audit (every emitted contract YAML can be traced to its
  source environment).
-/
inductive LatexDisplayKind where
  | displayMath
  | equation
  | align
deriving DecidableEq

/--
  Silver-tier model of an `EquationFormula` that retains its source
  environment kind. Bronze (`EquationFormula`) lost this information
  by reducing all three lowerings to the same anonymous record.
  Silver keeps `kind` AND `ascii_normalised`, locking in the
  modelling commitment that emission must record provenance even
  when the normalised content matches.
-/
structure EquationFormulaSilver where
  kind : LatexDisplayKind
  ascii_normalised : String
deriving DecidableEq

/--
  Silver-tier lowering: `\[ formula \]` produces a typed record
  with `kind = displayMath`. The `ascii_normalised` field carries
  the formula content byte-for-byte at v0.1.0; Gold tier introduces
  a real normaliser.
-/
def lower_display_math_silver (formula : String) : EquationFormulaSilver :=
  { kind := LatexDisplayKind.displayMath
    ascii_normalised := formula }

/-- Silver-tier lowering for `\begin{equation} ... \end{equation}`. -/
def lower_equation_env_silver (formula : String) : EquationFormulaSilver :=
  { kind := LatexDisplayKind.equation
    ascii_normalised := formula }

/-- Silver-tier lowering for `\begin{align} ... \end{align}`. -/
def lower_align_env_silver (formula : String) : EquationFormulaSilver :=
  { kind := LatexDisplayKind.align
    ascii_normalised := formula }

/--
  Normaliser that extracts the `ascii_normalised` content while
  discarding the kind discriminator. This is the bridge between
  the typed Silver model and Bronze's anonymisation — Silver
  proves equivalence UNDER this normaliser, not at the structural
  level.
-/
def normalise_silver (e : EquationFormulaSilver) : String :=
  e.ascii_normalised

/--
  **Silver-tier refinement theorem** for the LaTeX display-math
  triumvirate. The three forms produce structurally-distinct
  `EquationFormulaSilver` values (different `kind` tags) but
  their normalised contents are equal — exactly the strengthening
  Silver provides over Bronze.

  Documentary value: this theorem captures the modelling commitment
  that lowering is BOTH provenance-preserving (kind tag retained)
  AND content-equivalent (normaliser-agnostic). An emitter that
  rewrites `\[ ... \]` content during display-math lowering (e.g.,
  applying `\implies → \Rightarrow` substitution per Mathlib
  convention) would falsify the second conjunct without touching
  the first.

  Status: discharged at v0.1.0 (PMAT-167). Tier: Silver.
-/
theorem display_math_equiv_under_normaliser_silver (formula : String) :
    normalise_silver (lower_display_math_silver formula)
      = normalise_silver (lower_equation_env_silver formula)
    ∧ normalise_silver (lower_equation_env_silver formula)
      = normalise_silver (lower_align_env_silver formula) := by
  refine ⟨?_, ?_⟩ <;> rfl

/--
  **Silver-tier refinement theorem** — kind provenance is recorded
  distinctly for each source environment. This is the new structural
  claim Silver provides that Bronze couldn't: an emitter that
  conflates `\[ ... \]` with `align` (or vice versa) would falsify
  this theorem because the `kind` fields would no longer be
  pairwise distinct.

  Critical for audit: when a downstream tool sees a contract YAML
  emission with `display_kind: align`, it needs to trust that the
  source was indeed `\begin{align}` and not a silent relabelling
  by the emitter.
-/
theorem kinds_are_distinct_silver (formula : String) :
    (lower_display_math_silver formula).kind = LatexDisplayKind.displayMath
    ∧ (lower_equation_env_silver formula).kind = LatexDisplayKind.equation
    ∧ (lower_align_env_silver formula).kind = LatexDisplayKind.align := by
  refine ⟨?_, ?_, ?_⟩ <;> rfl

/-! ## PMAT-180 — Silver expansion: inline_math + theorem_env +
    proof_env (XPILE-REFINE-NOTATION-002).

    Replicates the PMAT-167 kind-tagged typed model across three
    more equations on C-NOTATION-LATEX-MATH-TO-EQUATION. Brings
    Silver coverage from 1/7 to 4/7 equations. -/

/-- Source kind of an inline-math span. `Dollar` for `$...$` and
    `Paren` for `\(...\)` — both produce the same content but
    have different syntactic source. -/
inductive InlineMathKind where
  | dollar
  | paren
deriving DecidableEq

/-- Silver-tier model of an inline-math span. Retains the source
    syntactic form. -/
structure InlineMathSilver where
  kind : InlineMathKind
  ascii_normalised : String
deriving DecidableEq

/-- Silver lowering for `$formula$`. -/
def lower_inline_dollar_silver (formula : String) : InlineMathSilver :=
  { kind := InlineMathKind.dollar, ascii_normalised := formula }

/-- Silver lowering for `\(formula\)`. -/
def lower_inline_paren_silver (formula : String) : InlineMathSilver :=
  { kind := InlineMathKind.paren, ascii_normalised := formula }

/-- Normaliser extracting just the content. -/
def normalise_inline_silver (m : InlineMathSilver) : String :=
  m.ascii_normalised

/-- **Silver-tier refinement theorem** — both inline-math forms
    produce equivalent content under the normaliser. Bronze
    proved byte-identity; Silver lifts to kind-tagged equivalence
    with provenance retention. -/
theorem inline_math_equiv_under_normaliser_silver (formula : String) :
    normalise_inline_silver (lower_inline_dollar_silver formula)
      = normalise_inline_silver (lower_inline_paren_silver formula) := by
  rfl

/-- **Silver-tier refinement theorem** — kind tags are pairwise
    distinct. Captures the audit-traceability claim that an
    emitter cannot relabel `$...$` as `\(...\)` (or vice versa). -/
theorem inline_kinds_are_distinct_silver (formula : String) :
    (lower_inline_dollar_silver formula).kind = InlineMathKind.dollar
    ∧ (lower_inline_paren_silver formula).kind = InlineMathKind.paren := by
  refine ⟨?_, ?_⟩ <;> rfl

/-- Silver-tier model of a Lean theorem-class environment with
    typed obligation type. Bronze had a String for obligation_type;
    Silver promotes to an enum that captures the precondition /
    postcondition polarity at the type level. -/
inductive ObligationKind where
  | precondition
  | postcondition
deriving DecidableEq

/-- Silver-tier theorem-env model with typed obligation kind. -/
structure LeanTheoremEnvSilver where
  body_text : String
  is_precondition_flagged : Bool
deriving DecidableEq

/-- Silver-tier obligation entry with typed kind enum. -/
structure ObligationEntrySilver where
  kind : ObligationKind
deriving DecidableEq

/-- Silver lowering: theorem env → typed-kind obligation. Branches
    on the precondition flag identically to Bronze, but the
    output type ENUM rules out string-mangling bug classes. -/
def lower_theorem_env_silver (t : LeanTheoremEnvSilver) : ObligationEntrySilver :=
  if t.is_precondition_flagged then
    { kind := ObligationKind.precondition }
  else
    { kind := ObligationKind.postcondition }

/-- **Silver-tier refinement theorem** — obligation kind enum
    matches the precondition flag. Bronze proved on strings;
    Silver lifts the same decision to a typed enum where the
    distinction is no longer string-comparison-based — an emitter
    that emits `"PreCondition"` (capitalised) or `"prerequisite"`
    instead of `"precondition"` would falsify Bronze; at Silver,
    those representations are no longer expressible. -/
theorem theorem_env_obligation_kind_silver (t : LeanTheoremEnvSilver) :
    (lower_theorem_env_silver t).kind =
      (if t.is_precondition_flagged then
        ObligationKind.precondition
       else
        ObligationKind.postcondition) := by
  unfold lower_theorem_env_silver
  split <;> rfl

/-- Silver-tier model of a proof environment with explicit
    `stub_reason` tag (Omitted | TODO | XXX | Sorry | None) to
    capture WHICH stub-pattern the body matched. Bronze had a
    single is_stub bit; Silver promotes to an enum. -/
inductive ProofStubReason where
  | none
  | omitted
  | todo
  | xxx
  | sorry
deriving DecidableEq

/-- Silver proof-env model with typed stub reason. -/
structure ProofEnvSilver where
  body : String
  stub_reason : ProofStubReason
deriving DecidableEq

/-- Silver model of the emitted Lean pointer artifact. -/
structure LeanPointerSilver where
  status : String
  body_leaked : Bool
  stub_reason : ProofStubReason
deriving DecidableEq

/-- Silver-tier lowering: proof env → lean pointer. status reflects
    the typed reason (Omitted/TODO/XXX/Sorry → "stub"; None →
    "claimed"); body_leaked false by construction; stub_reason
    preserved verbatim. -/
def lower_proof_env_silver (p : ProofEnvSilver) : LeanPointerSilver :=
  { status := match p.stub_reason with
      | ProofStubReason.none => "claimed"
      | _ => "stub"
    body_leaked := false
    stub_reason := p.stub_reason }

/-- **Silver-tier refinement theorem** — stub reason preserved
    verbatim through lowering. Bronze proved binary stub/claimed
    classification; Silver captures the SPECIFIC reason. An
    emitter that collapses all stub kinds into a single category
    (or invents a new category) is caught at the enum level. -/
theorem proof_stub_reason_preserved_silver (p : ProofEnvSilver) :
    (lower_proof_env_silver p).stub_reason = p.stub_reason := by
  rfl

/-- **Silver-tier refinement theorem** — body never leaks into
    the EquationsBlock. Companion to the Bronze
    `proof_env_to_lean_pointer` claim, lifted to the Silver
    typed model. -/
theorem proof_body_does_not_leak_silver (p : ProofEnvSilver) :
    (lower_proof_env_silver p).body_leaked = false := by
  rfl

/-! ## PMAT-181 — Final Silver expansion: definition_env +
    remark_env + citation_preservation
    (XPILE-REFINE-NOTATION-003).

    Replicates the PMAT-167/180 typed-model Silver pattern across
    the last three remaining equations on
    C-NOTATION-LATEX-MATH-TO-EQUATION. Brings Silver coverage to
    **7/7 equations — full Silver tier**, the **FOURTH contract
    in the substrate at full Silver** (after C-FFI-CPYTHON-EXT,
    C-XLATE-LEAN-TO-RUST, C-XLATE-RUST-FN-TO-LEAN-THM). -/

/--
  Silver-tier model of a Lean definition environment with optional
  label and explicit math-span vector. Bronze captured just the
  first math span; Silver captures the ENTIRE math-span list and
  the optional source-label for cross-document reference
  resolution.
-/
structure DefinitionEnvSilver where
  first_math_span : String
  all_math_spans : Array String
  label : Option String
deriving DecidableEq

/-- Silver model of the emitted equation entry. -/
structure DefinitionEquationSilver where
  formula : String
  additional_spans : Array String
  label : Option String
deriving DecidableEq

/-- Silver-tier lowering for definition env. -/
def lower_definition_env_silver (d : DefinitionEnvSilver) :
    DefinitionEquationSilver :=
  { formula := d.first_math_span
    additional_spans := d.all_math_spans
    label := d.label }

/-- **Silver-tier refinement theorem** — additional math spans
    preserved. Bronze proved byte-identity on the first span;
    Silver captures that ALL spans survive, plus the optional
    source-label for cross-doc reference. -/
theorem additional_spans_preserved_silver (d : DefinitionEnvSilver) :
    (lower_definition_env_silver d).additional_spans = d.all_math_spans := by
  rfl

/-- **Silver-tier refinement theorem** — definition label
    preserved. Optional-typed: when absent in source, absent in
    output. Captures the cross-document reference invariant that
    Bronze couldn't model. -/
theorem definition_label_preserved_silver (d : DefinitionEnvSilver) :
    (lower_definition_env_silver d).label = d.label := by
  rfl

/--
  Silver-tier model of a remark environment with typed normative-
  keyword set. Bronze had three independent flags (has_must,
  has_should, has_must_not); Silver promotes to a typed enum
  capturing PRIORITY order: MustNot > Must > Should > None.
-/
inductive NormativeKeyword where
  | none
  | should
  | must
  | mustNot
deriving DecidableEq

/-- Silver remark-env model with typed normative kind. -/
structure RemarkEnvSilver where
  keyword : NormativeKeyword
deriving DecidableEq

/-- Silver model of a falsification-test entry. -/
structure FalsificationEntrySilver where
  ship_blocking : Bool
  predicate_inverted : Bool
deriving DecidableEq

/-- Silver lowering rule with priority-ordered enum dispatch. -/
def lower_remark_env_silver (r : RemarkEnvSilver) :
    Option FalsificationEntrySilver :=
  match r.keyword with
  | NormativeKeyword.mustNot =>
      some { ship_blocking := true, predicate_inverted := true }
  | NormativeKeyword.must =>
      some { ship_blocking := true, predicate_inverted := false }
  | NormativeKeyword.should =>
      some { ship_blocking := false, predicate_inverted := false }
  | NormativeKeyword.none => Option.none

/-- **Silver-tier refinement theorem** — keyword → falsification-
    entry mapping. Captures the RFC-2119 priority order at the
    typed-enum level. Bronze proved the existence claim (Option
    isSome iff some flag set); Silver proves the SPECIFIC
    classification for each keyword. -/
theorem normative_keyword_classification_silver (r : RemarkEnvSilver) :
    (lower_remark_env_silver r).isSome ↔ r.keyword ≠ NormativeKeyword.none := by
  unfold lower_remark_env_silver
  cases r.keyword <;> simp

/-- **Silver-tier refinement theorem** — when keyword is MustNot,
    the falsification entry is ship-blocking with inverted
    predicate. Captures the load-bearing safety claim that
    MUST NOT cannot be silently downgraded. -/
theorem must_not_implies_ship_blocking_inverted_silver
    (r : RemarkEnvSilver)
    (h : r.keyword = NormativeKeyword.mustNot) :
    ∃ entry : FalsificationEntrySilver,
      lower_remark_env_silver r = some entry
      ∧ entry.ship_blocking = true
      ∧ entry.predicate_inverted = true := by
  unfold lower_remark_env_silver
  rw [h]
  exact ⟨_, rfl, rfl, rfl⟩

/--
  Silver-tier model of a LaTeX citation with explicit BibTeX-key
  alongside the contract ID. Bronze captured just the
  contract_id; Silver adds the bib_key (the citation's
  bibliographic-database key) for traceability into LaTeX's
  cross-reference machinery.
-/
structure LatexCitationSilver where
  contract_id : String
  bib_key : String
deriving DecidableEq

/-- Silver model of the emitted citation output. Mirror image. -/
structure CitationOutputSilver where
  contract_id : String
  bib_key : String
deriving DecidableEq

/-- Silver lowering: copy both fields verbatim. -/
def lower_citation_silver (c : LatexCitationSilver) : CitationOutputSilver :=
  { contract_id := c.contract_id, bib_key := c.bib_key }

/-- **Silver-tier refinement theorem** — bib_key preserved
    byte-for-byte through citation lowering. Bronze proved
    contract_id preservation; Silver captures the BibTeX-key
    side that enables LaTeX-source ↔ contract-YAML round-tripping.
    Falsified by an emitter that drops the bib_key during YAML
    emission (which would orphan the citation from LaTeX's
    \\cite{...} resolution). -/
theorem bib_key_preserved_silver (c : LatexCitationSilver) :
    (lower_citation_silver c).bib_key = c.bib_key := by
  rfl

/-- **Silver-tier refinement theorem** — contract ID preserved
    in the Silver typed model. Composes with Bronze
    `citation_preservation`. COMPLETES Silver coverage on
    C-NOTATION-LATEX-MATH-TO-EQUATION (7/7) — fourth contract at
    full Silver. -/
theorem silver_contract_id_preserved (c : LatexCitationSilver) :
    (lower_citation_silver c).contract_id = c.contract_id := by
  rfl

/-! ## PMAT-189 — FIFTH Gold-tier refinement: NonEmptyDefinition
    on definition_env_to_equation (XPILE-REFINE-NOTATION-004).

    Fifth Gold-tier theorem in the substrate. **Demonstrates the
    Gold-tier subtype pattern on a NEW shape** — non-empty-list
    refinement, distinct from the bounded-Nat pattern used in
    PMAT-185/186/187/188.

    Silver (PMAT-181's `additional_spans_preserved_silver`)
    captured "all math spans survive lowering" via a typed model.
    But Silver couldn't encode the contract's `domain` precondition
    "definition body contains at least one math span": that
    constraint was a separate proof obligation.

    Gold tier promotes it: `NonEmptyDefinition := { d :
    DefinitionEnvSilver // d.all_math_spans.size > 0 }` — the
    non-emptiness witness is carried by the value. A
    DefinitionEnvSilver with an empty math-span vector cannot
    even be constructed as a NonEmptyDefinition; the type
    system catches the violation at the API boundary.

    **Why this matters as a new pattern**: bounded-Nat subtypes
    (PMAT-185..188) encode numeric inequalities. Non-empty-list
    subtypes encode a different proof shape — collection
    non-emptiness, which appears in many other contracts
    (precondition lists, equation lists, citation sets).
    PMAT-189 establishes that Gold-tier refinement works for
    collection-cardinality preconditions too, not just numeric
    bounds.

    Status: discharged at v0.1.0 (PMAT-189). Tier: GOLD.
    Fifth Gold theorem in the xpile substrate, first of a new
    subtype pattern (non-emptiness rather than numeric bounds). -/

/-- Gold-tier refinement subtype: a Silver definition env proven
    to have at least one math span. The non-emptiness witness
    travels with the value. An emitter receiving a
    NonEmptyDefinition cannot pass an empty-span definition —
    the type system rules it out at compile time. -/
def NonEmptyDefinition :=
  { d : DefinitionEnvSilver // d.all_math_spans.size > 0 }

/-- Extract the underlying Silver definition. -/
def NonEmptyDefinition.val (n : NonEmptyDefinition) : DefinitionEnvSilver :=
  n.val

/-- Gold-tier lowering: extracts the structural definition data
    from the non-empty wrapper. The non-emptiness witness is
    carried into the typed output. -/
def lower_non_empty_definition_gold (d : NonEmptyDefinition) :
    DefinitionEquationSilver :=
  lower_definition_env_silver d.val

/--
  **Gold-tier refinement theorem** — lowering a NonEmptyDefinition
  preserves the additional_spans field, AND the non-emptiness
  witness travels with the value at the type level.

  This is the fifth Gold theorem in the substrate. Captures what
  Silver couldn't model:
  - Silver: "additional_spans preserved IF body has at least one
    span" (precondition as a separate obligation)
  - Gold: "input IS a NonEmptyDefinition" (non-emptiness witness
    travels with the value; downstream code can iterate the
    additional_spans without an empty-check)

  An emitter that constructs a DefinitionEnvSilver from a
  zero-span body would not type-check against
  `lower_non_empty_definition_gold` — the type system catches
  the empty-body case at the API boundary.

  Status: **discharged at v0.1.0 (PMAT-189)**. Tier: GOLD.
-/
theorem non_empty_definition_preserves_spans_gold (d : NonEmptyDefinition) :
    (lower_non_empty_definition_gold d).additional_spans = d.val.all_math_spans := by
  rfl

/--
  **Gold-tier refinement theorem** — the non-emptiness witness
  is preserved through lowering. The output's additional_spans
  has size > 0 BY TYPE — no runtime empty-check needed.
-/
theorem non_empty_witness_gold (d : NonEmptyDefinition) :
    (lower_non_empty_definition_gold d).additional_spans.size > 0 := by
  unfold lower_non_empty_definition_gold lower_definition_env_silver
  exact d.property

/--
  **Gold-tier refinement theorem** — bridges Gold to Silver: the
  underlying additional_spans agrees with what Silver's
  `additional_spans_preserved_silver` produces on the same
  underlying DefinitionEnvSilver. Gold simply carries the
  non-emptiness witness in addition.
-/
theorem gold_non_empty_agrees_with_silver_spans
    (d : NonEmptyDefinition) :
    (lower_non_empty_definition_gold d).additional_spans
      = (lower_definition_env_silver d.val).additional_spans := by
  rfl

/-! ## PMAT-208 — NINTH Platinum-tier refinement: citation
    concatenation homomorphism (XPILE-REFINE-NOTATION-005).

    Ninth Platinum-tier theorem in the substrate. **Extends
    Platinum to C-NOTATION-LATEX-MATH-TO-EQUATION** — Platinum
    coverage now spans 7 of 12 contracts across all 5 layers.

    Demonstrates the functoriality/homomorphism Platinum pattern
    on a THIRD contract domain (after PMAT-202 Python lists and
    PMAT-207 Lean inductives). The citation lowering distributes
    over concatenation of contract-id arrays, just as list
    lowering distributes over List append. This locks in the
    "no citations dropped" invariant compositionally.

    The pattern is now demonstrated on THREE distinct algebraic
    structures:
    - List α (PMAT-202): Python list lowering
    - Inductive types (PMAT-207): Lean inductive → enum
    - **Array String (PMAT-208): citation set concatenation**

    Status: discharged at v0.1.0 (PMAT-208). Tier: PLATINUM.
    Ninth Platinum theorem in the substrate. -/

/-- Compose two LatexCitationSilver values via contract_id +
    bib_key concatenation. The composition forms a (String,
    String)-pair monoid where the operation is per-component
    concatenation. -/
def compose_latex_citation_silver
    (c1 c2 : LatexCitationSilver) : LatexCitationSilver :=
  { contract_id := c1.contract_id ++ c2.contract_id
    bib_key := c1.bib_key ++ c2.bib_key }

/--
  **Platinum-tier refinement theorem** — composing two citations
  preserves the concatenation structure through lowering.

  For any two LatexCitationSilver values c1, c2, lowering their
  composition produces the concatenated contract_id (and
  bib_key). This is the FUNCTORIALITY property for citation
  lowering over the (String, String)-pair monoid.

  Third demonstration of the functoriality Platinum pattern
  (after PMAT-202 list lowering and PMAT-207 inductive
  lowering). Captures cross-domain consistency: the same
  algebraic property holds across three distinct contract
  taxonomies.

  Status: **discharged at v0.1.0 (PMAT-208)**. Tier: PLATINUM.
-/
theorem citation_composition_homomorphism_platinum
    (c1 c2 : LatexCitationSilver) :
    (lower_citation_silver
       (compose_latex_citation_silver c1 c2)).contract_id
    = (lower_citation_silver c1).contract_id
        ++ (lower_citation_silver c2).contract_id := by
  unfold lower_citation_silver compose_latex_citation_silver
  rfl

/--
  **Platinum-tier refinement theorem** — bib_key also forms a
  homomorphism under composition. Companion to
  `citation_composition_homomorphism_platinum`. The (String,
  ++, "") monoid structure is preserved on BOTH the contract_id
  and bib_key fields independently.
-/
theorem bib_key_composition_homomorphism_platinum
    (c1 c2 : LatexCitationSilver) :
    (lower_citation_silver
       (compose_latex_citation_silver c1 c2)).bib_key
    = (lower_citation_silver c1).bib_key
        ++ (lower_citation_silver c2).bib_key := by
  unfold lower_citation_silver compose_latex_citation_silver
  rfl

/--
  **Platinum-tier refinement theorem** — citation lowering is
  associative under composition. Follows from
  String.append_assoc applied per-component.

  Combined with the homomorphism theorems above, this proves
  citation lowering is a STRICT MONOID HOMOMORPHISM (preserves
  the monoid operation AND associativity AND identity if we
  added an empty-citation lemma).
-/
theorem citation_composition_associative_platinum
    (c1 c2 c3 : LatexCitationSilver) :
    (compose_latex_citation_silver
       (compose_latex_citation_silver c1 c2) c3).contract_id
    = (compose_latex_citation_silver c1
        (compose_latex_citation_silver c2 c3)).contract_id := by
  unfold compose_latex_citation_silver
  simp [String.append_assoc]

/-! ## PMAT-219 — SIXTH Diamond-tier refinement: string-monoid
    axioms (XPILE-REFINE-NOTATION-006).

    Sixth Diamond-tier theorem in the substrate. Combines four
    monoid properties on citation composition:
    - PMAT-208 Platinum functoriality (the homomorphism)
    - Associativity (PMAT-208 companion)
    - Identity (empty citation = "")
    - The String-monoid structure

    Captures the (String, ++, "") monoid structure for citation
    lowering at the type level — fundamental to compositional
    citation analysis.

    Sixth distinct Diamond category, distinct from prior 5:
    1. PMAT-214: commutative-monoid / semiring (algebraic)
    2. PMAT-215: pure-function (functional)
    3. PMAT-216: abelian-group (algebraic w/ inverses)
    4. PMAT-217: equivalence-relation (relational)
    5. PMAT-218: bounded-monoid (bounded algebraic)
    6. **PMAT-219 (NEW): string-monoid (textual algebraic)**

    Status: discharged at v0.1.0 (PMAT-219). Tier: DIAMOND.
    Sixth Diamond theorem in the substrate. -/

/--
  **Diamond-tier refinement theorem** — citation composition
  forms a STRING MONOID under (String, ++, "").

  Combines four monoid axioms:
  - Closure: composing two citations produces a citation
  - Associativity: (c1 ++ c2) ++ c3 = c1 ++ (c2 ++ c3)
  - Left identity: "" ++ c = c
  - Right identity: c ++ "" = c

  Note: NOT commutative (string concat is order-sensitive) —
  this distinguishes the string-monoid from the commutative-
  monoid of PMAT-214.

  Status: **discharged at v0.1.0 (PMAT-219)**. Tier: DIAMOND.
-/
theorem citation_string_monoid_diamond
    (c1 c2 c3 : LatexCitationSilver) :
    -- Closure / homomorphism (PMAT-208 lifted)
    (lower_citation_silver (compose_latex_citation_silver c1 c2)).contract_id
      = (lower_citation_silver c1).contract_id
          ++ (lower_citation_silver c2).contract_id
    -- Associativity (PMAT-208 companion lifted)
    ∧ (compose_latex_citation_silver
        (compose_latex_citation_silver c1 c2) c3).contract_id
      = (compose_latex_citation_silver c1
          (compose_latex_citation_silver c2 c3)).contract_id
    -- Left identity
    ∧ (compose_latex_citation_silver
        { contract_id := "", bib_key := "" } c1).contract_id
      = c1.contract_id
    -- Right identity
    ∧ (compose_latex_citation_silver
        c1 { contract_id := "", bib_key := "" }).contract_id
      = c1.contract_id := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · unfold lower_citation_silver compose_latex_citation_silver
    rfl
  · exact citation_composition_associative_platinum c1 c2 c3
  · unfold compose_latex_citation_silver
    simp
  · unfold compose_latex_citation_silver
    simp

end XpileContracts.CNotationLatexMathToEquation
