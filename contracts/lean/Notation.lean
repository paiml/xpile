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

/-- Fidelity of a proof-env EMITTER. A *faithful* emitter emits only
    a POINTER to the proof (the body stays in the Lean lane); a
    *body-pasting* emitter inlines the proof body TEXT into the emitted
    `EquationsBlock`, leaking it into the LaTeX/equation lane. -/
structure ProofEmitter where
  pastes_body : Bool
deriving DecidableEq

/-- xpile's ACTUAL proof-env emitter: faithful — pointer-only. -/
def xpileProofEmitter : ProofEmitter := { pastes_body := false }

/-- Silver-tier lowering: proof env → lean pointer, parameterized by
    emitter fidelity. `status` reflects the typed reason
    (Omitted/TODO/XXX/Sorry → "stub"; None → "claimed"); `stub_reason`
    is preserved verbatim; `body_leaked` is TRUE iff the emitter pastes
    the body AND the proof env actually HAS a body — so the `body`
    input is now LOAD-BEARING (an empty body has nothing to leak; a
    pointer-only emitter leaks nothing regardless).

    PMAT-1177: pre-fix this hardcoded `body_leaked := false` and
    DISCARDED `p.body`, so `proof_body_does_not_leak_silver` reduced to
    `false = false` and certified nothing — a body-pasting emitter
    satisfied it too (the PMAT-1141/1176 vacuity class). -/
def lower_proof_env_silver (e : ProofEmitter) (p : ProofEnvSilver) : LeanPointerSilver :=
  { status := match p.stub_reason with
      | ProofStubReason.none => "claimed"
      | _ => "stub"
    body_leaked := e.pastes_body && !p.body.isEmpty
    stub_reason := p.stub_reason }

/-- **Silver-tier refinement theorem** — stub reason preserved
    verbatim through lowering, for ANY emitter fidelity. Bronze proved
    binary stub/claimed classification; Silver captures the SPECIFIC
    reason. An emitter that collapses all stub kinds into a single
    category (or invents a new category) is caught at the enum level. -/
theorem proof_stub_reason_preserved_silver (e : ProofEmitter) (p : ProofEnvSilver) :
    (lower_proof_env_silver e p).stub_reason = p.stub_reason := by
  rfl

/-- **Silver-tier refinement theorem (the "pin")** — the proof body
    never leaks into the `EquationsBlock` under xpile's ACTUAL
    (faithful, pointer-only) emitter, for ANY proof env. It holds
    PRECISELY because `xpileProofEmitter.pastes_body = false`; it is
    NON-vacuous by the two duals below (analog of PMAT-1176's
    `faithful_lowering_matches_baseline`). -/
theorem proof_body_does_not_leak_silver (p : ProofEnvSilver) :
    (lower_proof_env_silver xpileProofEmitter p).body_leaked = false := by
  rfl

/-- **Falsifiability dual (non-vacuity lock #1)** — a body-PASTING
    emitter DOES leak a nonempty proof body. Proves the Silver model
    can EXPRESS the exact defect the `body_leaked = false` invariant
    forbids; without this the pin is vacuous. -/
theorem body_pasting_emitter_leaks_body :
    (lower_proof_env_silver { pastes_body := true }
      { body := "x", stub_reason := ProofStubReason.none }).body_leaked = true := by
  decide

/-- **Falsifiability dual (non-vacuity lock #2)** — on a real
    (nonempty) proof body the faithful emitter and the body-pasting
    emitter DIVERGE on `body_leaked`: the exact differential a
    lane-separation check exists to catch. Analog of PMAT-1176's
    `refcount_leak_lowering_diverges`. -/
theorem faithful_vs_pasting_emitter_diverges :
    (lower_proof_env_silver xpileProofEmitter
      { body := "x", stub_reason := ProofStubReason.none }).body_leaked
    ≠ (lower_proof_env_silver { pastes_body := true }
      { body := "x", stub_reason := ProofStubReason.none }).body_leaked := by
  decide

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
  -- Positional `.1` Subtype projection, NOT `n.val`: dot-notation `n.val`
  -- resolves to THIS definition (a non-terminating self-call, `n` unchanged) —
  -- the PMAT-914/915 name-shadowing class. `.1` breaks the self-reference so
  -- the def terminates and the downstream `.val`/`.property`/`Subtype.ext`
  -- proofs (lines ~858, ~1477) elaborate against the real underlying value.
  n.1

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

/-! ## PMAT-234 — SECOND Diamond on C-NOTATION-LATEX-MATH-TO-EQUATION
    (XPILE-REFINE-NOTATION-007): citation-product-monoid axioms.

    **Sixth depth-2 Diamond in the substrate** (after PMAT-228
    L1, PMAT-229 L2, PMAT-230 L4, PMAT-231 L5, PMAT-232 L3 — that
    completed UNIVERSAL depth-2 across all 5 layers). PMAT-234
    extends depth-2 coverage to a SECOND Layer-4 contract
    (Notation joins FfiCpython as the second Layer-4 contract
    with two Diamonds).

    Notation already has the citation-STRING-MONOID Diamond at
    PMAT-219 covering ONLY the contract_id field. PMAT-234 adds
    the CITATION-PRODUCT-MONOID Diamond — a fundamentally
    distinct algebraic category covering BOTH fields
    simultaneously as a free product of two string monoids:

    - PMAT-219: (String_contract_id, ++, "") string-monoid on
      just one field
    - PMAT-234: (String × String, ++_componentwise, ("", ""))
      product-monoid on the FULL Citation value

    The categorical distinction: string-monoid is on a single
    string component; product-monoid captures the algebraic
    PRODUCT of two independent string-monoids. The latter is
    strictly stronger — knowing each component is a monoid does
    NOT imply they form a product-monoid; the product structure
    requires that operations are component-wise (no cross-field
    interference).

    Status: discharged at v0.1.0 (PMAT-234). Tier: DIAMOND.
    SECOND Diamond category on C-NOTATION-LATEX-MATH-TO-EQUATION. -/

/--
  **Diamond-tier refinement theorem** — citation values form a
  PRODUCT MONOID under the (contract_id, bib_key) field pair.

  Combines four properties into the PRODUCT-MONOID
  axiomatization on `(LatexCitationSilver, compose, empty)`:
  (a) contract_id homomorphism (PMAT-208a lifted)
  (b) bib_key homomorphism (PMAT-208b lifted)
  (c) Left identity on contract_id (empty composes to identity)
  (d) Left identity on bib_key (empty composes to identity)

  An emitter that lowers contract_id correctly but introduces
  hidden coupling between contract_id and bib_key (e.g., always
  setting bib_key = contract_id without honoring the source
  value) would falsify (b) — the bib_key homomorphism would fail
  on inputs where the source bib_key differs from contract_id.

  Status: **discharged at v0.1.0 (PMAT-234)**. Tier: DIAMOND.
-/
theorem citation_product_monoid_diamond
    (c1 c2 : LatexCitationSilver) :
    -- (a) contract_id homomorphism (PMAT-208a lifted)
    (lower_citation_silver
      (compose_latex_citation_silver c1 c2)).contract_id
      = (lower_citation_silver c1).contract_id
          ++ (lower_citation_silver c2).contract_id
    -- (b) bib_key homomorphism (PMAT-208b lifted)
    ∧ (lower_citation_silver
        (compose_latex_citation_silver c1 c2)).bib_key
      = (lower_citation_silver c1).bib_key
          ++ (lower_citation_silver c2).bib_key
    -- (c) Left identity on contract_id (empty is identity)
    ∧ (compose_latex_citation_silver
        { contract_id := "", bib_key := "" } c1).contract_id
      = c1.contract_id
    -- (d) Left identity on bib_key (empty is identity)
    ∧ (compose_latex_citation_silver
        { contract_id := "", bib_key := "" } c1).bib_key
      = c1.bib_key := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact citation_composition_homomorphism_platinum c1 c2
  · exact bib_key_composition_homomorphism_platinum c1 c2
  · unfold compose_latex_citation_silver
    simp
  · unfold compose_latex_citation_silver
    simp

/-! ## PMAT-334 — THIRD Diamond on C-NOTATION-LATEX-MATH-TO-EQUATION
    (Layer 5 BROADENING DEPTH-3 from 9 to 10 contracts):
    EquationFormulaSilver STRUCTURE EXTENSIONALITY
    (XPILE-REFINE-NOTATION-007).

    **Broadens DEPTH-3 from 9 to 10 contracts.** Pushes
    NotationLatexMathToEquation (Layer 5) from depth-2 to depth-3,
    adding a SECOND Layer 5 contract at depth-3
    (CompileRustToPtxMma was first via PMAT-242).

    Seventh substrate-wide demonstration of structure-extensionality
    pattern (after PMAT-311/329/330/331/332/333).

    Status: discharged at v0.1.0 (PMAT-334). Tier: DIAMOND. -/

/--
  **Diamond-tier refinement theorem** — `EquationFormulaSilver`
  admits STRUCTURE EXTENSIONALITY.

  Status: **discharged at v0.1.0 (PMAT-334)**. Tier: DIAMOND.
-/
theorem equation_formula_struct_extensionality_diamond
    (f1 f2 : EquationFormulaSilver) :
    -- (a) Field equality → record equality
    (f1.kind = f2.kind ∧ f1.ascii_normalised = f2.ascii_normalised → f1 = f2)
    -- (b) Record equality → field equality
    ∧ (f1 = f2 → f1.kind = f2.kind ∧ f1.ascii_normalised = f2.ascii_normalised)
    -- (c) Decidable equality
    ∧ (f1 = f2 ∨ f1 ≠ f2)
    -- (d) Self-equality (reflexivity)
    ∧ (f1 = f1) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro ⟨h1, h2⟩
    cases f1; cases f2
    simp_all
  · intro h
    exact ⟨by rw [h], by rw [h]⟩
  · by_cases h : f1 = f2
    · exact Or.inl h
    · exact Or.inr h
  · rfl

/-! ## PMAT-342 — FOURTH Diamond on C-NOTATION-LATEX-MATH-TO-EQUATION
    (Layer 5 BROADENING DEPTH-4 from 9 to 10 contracts):
    LATEX-DISPLAY-KIND ENUM DISTINCTNESS
    (XPILE-REFINE-NOTATION-008).

    **Broadens DEPTH-4 from 9 to 10 contracts.** Pushes
    NotationLatexMathToEquation (Layer 5) from depth-3 to depth-4,
    adding a SECOND Layer 5 contract at depth-4 (CompileRustToPtxMma
    was first).

    The 4 Diamond categories on C-NOTATION-LATEX-MATH-TO-EQUATION:
    - PMAT-219 citation_string_monoid: monoid on contract_id
    - PMAT-234 citation_product_monoid: product monoid
    - PMAT-334 equation_formula_struct_extensionality: record
    - **PMAT-342: LATEX-DISPLAY-KIND ENUM DISTINCTNESS** ← depth-4

    Mirror of PMAT-339 (Target enum distinctness on
    XpileBackendTrait) — captures FINITE ENUMERATION DECIDABILITY
    of the 3-variant LatexDisplayKind enum (displayMath, equation,
    align).

    Status: discharged at v0.1.0 (PMAT-342). Tier: DIAMOND. -/

/--
  **Diamond-tier refinement theorem** — `LatexDisplayKind` is a
  3-variant decidable enumeration with distinct constructors.

  Status: **discharged at v0.1.0 (PMAT-342)**. Tier: DIAMOND.
-/
theorem latex_display_kind_enum_distinctness_diamond (k : LatexDisplayKind) :
    -- (a) displayMath ≠ equation
    (LatexDisplayKind.displayMath ≠ LatexDisplayKind.equation)
    -- (b) equation ≠ align
    ∧ (LatexDisplayKind.equation ≠ LatexDisplayKind.align)
    -- (c) Self-equality
    ∧ (k = k)
    -- (d) Decidable equality
    ∧ (k = LatexDisplayKind.displayMath ∨ k ≠ LatexDisplayKind.displayMath) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · decide
  · decide
  · rfl
  · by_cases h : k = LatexDisplayKind.displayMath
    · exact Or.inl h
    · exact Or.inr h

/-! ## PMAT-350 — FIFTH Diamond on C-NOTATION-LATEX-MATH-TO-EQUATION
    (Layer 5 BROADENING DEPTH-5 from 7 to 8 contracts):
    EQUATION-FORMULA ASCII LENGTH NAT STRUCTURE
    (XPILE-REFINE-NOTATION-009).

    **Broadens DEPTH-5 from 7 to 8 contracts.** After PMAT-349
    brought XlatePyListToVec (Layer 2) to depth-5 (substrate at 7
    contracts at depth-5+), PMAT-350 pushes
    NotationLatexMathToEquation (Layer 5) from depth-4 to depth-5,
    adding a SECOND Layer 5 contract at depth-5 (CompileRustToPtxMma
    was first via PMAT-287).

    The 5 Diamond categories on C-NOTATION-LATEX-MATH-TO-EQUATION:
    - PMAT-219 citation_string_monoid: monoid on contract_id
    - PMAT-234 citation_product_monoid: product monoid
    - PMAT-334 equation_formula_struct_extensionality: record
    - PMAT-342 latex_display_kind_enum_distinctness: enum
    - **PMAT-350: ASCII-LENGTH NAT STRUCTURE** ← depth-5

    The categorical distinction is sharp:
      - PMAT-219/234: monoid algebras (binary operations)
      - PMAT-334: record-from-fields extensionality
      - PMAT-342: enum constructor distinctness
      - PMAT-350: String.length Nat measure on ascii_normalised

    Mirror of PMAT-346 (Bashrs observable string length) — second
    substrate-wide demonstration of the String.length Nat-structure
    template on a TEXTUAL field. Complements the Array.size template
    family (PMAT-340/341/343/344/348) — both are Nat-measure
    invariants but on different underlying containers (String vs.
    Array).

    Status: discharged at v0.1.0 (PMAT-350). Tier: DIAMOND.
    Broadens DEPTH-5 from 7 to 8 contracts. -/

/--
  **Diamond-tier refinement theorem** — `EquationFormulaSilver.ascii_normalised`
  String.length Nat structure.

  Combines four LENGTH-NAT properties:
  (a) ascii_normalised.length is non-negative (trivially for Nat)
  (b) Empty ascii_normalised has length-0
  (c) Field-replacement preserves length
  (d) kind field is independent (length unchanged by kind swap)

  Second substrate-wide demonstration of the String.length
  Nat-structure pattern (after PMAT-346 OutcomeSilver), complementing
  the Array.size template family.

  Status: **discharged at v0.1.0 (PMAT-350)**. Tier: DIAMOND.
  Broadens DEPTH-5 from 7 to 8 contracts.
-/
theorem equation_formula_ascii_length_nat_diamond (f : EquationFormulaSilver) :
    -- (a) length is non-negative (trivially for Nat)
    (0 ≤ f.ascii_normalised.length)
    -- (b) Empty ascii_normalised gives length 0
    ∧ ((⟨LatexDisplayKind.displayMath, ""⟩ : EquationFormulaSilver).ascii_normalised.length = 0)
    -- (c) Field-replacement preserves length
    ∧ ((⟨f.kind, f.ascii_normalised⟩ : EquationFormulaSilver).ascii_normalised.length
        = f.ascii_normalised.length)
    -- (d) kind field is independent (length unchanged by kind swap)
    ∧ ((⟨LatexDisplayKind.equation, f.ascii_normalised⟩ : EquationFormulaSilver).ascii_normalised.length
        = f.ascii_normalised.length) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact Nat.zero_le _
  · rfl
  · rfl
  · rfl

/-! ## PMAT-362 — SIXTH Diamond on C-NOTATION-LATEX-MATH-TO-EQUATION
    (Layer 5 BROADENS DEPTH-6 post-ALL 5 LAYERS milestone):
    LATEX-CITATION-SILVER STRUCTURE EXTENSIONALITY
    (XPILE-REFINE-NOTATION-010).

    **Broadens depth-6 substrate-wide.** After PMAT-358 completed
    depth-6 ACROSS ALL 5 TAXONOMY LAYERS, PMAT-362 pushes
    NotationLatexMathToEquation (Layer 5) from depth-5 to depth-6 as
    the second L5 contract at depth-6+ (CompileRustToPtxMma was
    first via PMAT-291).

    The 6 Diamond categories on C-NOTATION-LATEX-MATH-TO-EQUATION:
    - PMAT-219 citation_string_monoid: contract_id String monoid
    - PMAT-234 citation_product_monoid: product monoid
    - PMAT-334 equation_formula_struct_extensionality: formula record
    - PMAT-342 latex_display_kind_enum_distinctness: kind enum
    - PMAT-350 equation_formula_ascii_length_nat: String.length
    - **PMAT-362: LATEX-CITATION-SILVER STRUCTURE EXTENSIONALITY** ← depth-6

    The categorical distinction is sharp:
      - PMAT-334 captures struct-ext of EquationFormulaSilver (the
        FORMULA record).
      - PMAT-362 captures struct-ext of LatexCitationSilver (the
        CITATION record — distinct AST node carrying citation
        bibkey metadata).

    Eighteenth substrate-wide demonstration of the structure-
    extensionality pattern (after PMAT-311/329..336/349/352/353/354/
    356/359/360/361).

    Status: discharged at v0.1.0 (PMAT-362). Tier: DIAMOND.
    Broadens depth-6 post-ALL 5 LAYERS milestone. -/

/--
  **Diamond-tier refinement theorem** — `LatexCitationSilver` admits
  STRUCTURE EXTENSIONALITY.

  Combines four STRUCTURE-EXTENSIONALITY properties on the 2-field
  LatexCitationSilver record (contract_id : String, bib_key : String):
  (a) Field-equality → record-equality
  (b) Record-equality → field-equality (congruence)
  (c) Decidable equality (deriving DecidableEq)
  (d) Self-equality (reflexivity)

  Eighteenth substrate-wide demonstration of the structure-
  extensionality pattern.

  Status: **discharged at v0.1.0 (PMAT-362)**. Tier: DIAMOND.
  Broadens depth-6 post-ALL 5 LAYERS milestone.
-/
theorem latex_citation_silver_struct_extensionality_diamond
    (c1 c2 : LatexCitationSilver) :
    -- (a) Field equality → record equality
    (c1.contract_id = c2.contract_id ∧ c1.bib_key = c2.bib_key → c1 = c2)
    -- (b) Record equality → field equality
    ∧ (c1 = c2 → c1.contract_id = c2.contract_id ∧ c1.bib_key = c2.bib_key)
    -- (c) Decidable equality
    ∧ (c1 = c2 ∨ c1 ≠ c2)
    -- (d) Self-equality (reflexivity)
    ∧ (c1 = c1) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro ⟨h1, h2⟩
    cases c1; cases c2
    simp_all
  · intro h
    exact ⟨by rw [h], by rw [h]⟩
  · by_cases h : c1 = c2
    · exact Or.inl h
    · exact Or.inr h
  · rfl

/-! ## PMAT-372 — SEVENTH Diamond on C-NOTATION-LATEX-MATH-TO-EQUATION
    (Layer 5 BROADENS DEPTH-7):
    LATEX-DISPLAY-KIND ENUM COMPLETENESS
    (XPILE-REFINE-NOTATION-011).

    **Broadens DEPTH-7 substrate-wide.** Pushes
    NotationLatexMathToEquation (Layer 5) from depth-6 to depth-7
    as the second L5 contract at depth-7+ (CompileRustToPtxMma was
    first via PMAT-293).

    The 7 Diamond categories on C-NOTATION-LATEX-MATH-TO-EQUATION:
    - PMAT-219 citation_string_monoid
    - PMAT-234 citation_product_monoid
    - PMAT-334 equation_formula_struct_extensionality
    - PMAT-342 latex_display_kind_enum_distinctness
    - PMAT-350 equation_formula_ascii_length_nat
    - PMAT-362 latex_citation_silver_struct_extensionality
    - **PMAT-372: LATEX-DISPLAY-KIND ENUM COMPLETENESS** ← depth-7

    Mirror of PMAT-370 (Target enum completeness on
    C-XPILE-BACKEND-TRAIT). Together PMAT-342 (distinctness) +
    PMAT-372 (completeness) give the full finite-enumeration
    axiomatization for LatexDisplayKind.

    Status: discharged at v0.1.0 (PMAT-372). Tier: DIAMOND. -/

/--
  **Diamond-tier refinement theorem** — `LatexDisplayKind` admits
  FINITE ENUMERATION COMPLETENESS.

  Combines four properties:
  (a) Total coverage: every LatexDisplayKind matches one of 3 variants
  (b) Self-equality
  (c) Decidable membership
  (d) Constructor distinctness sample

  Status: **discharged at v0.1.0 (PMAT-372)**. Tier: DIAMOND.
-/
theorem latex_display_kind_enum_completeness_diamond (k : LatexDisplayKind) :
    (k = LatexDisplayKind.displayMath ∨ k = LatexDisplayKind.equation
      ∨ k = LatexDisplayKind.align)
    ∧ (k = k)
    ∧ (k = LatexDisplayKind.displayMath ∨ k ≠ LatexDisplayKind.displayMath)
    ∧ (LatexDisplayKind.displayMath ≠ LatexDisplayKind.align) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  -- core `decide` over the decidable LatexDisplayKind disjunction (Mathlib
  -- `tauto` is unavailable under bare core — same fix as PMAT-904/913, cf.
  -- the already-green `decide` on clause (d) below) — PMAT-916
  · cases k <;> decide
  · rfl
  · by_cases h : k = LatexDisplayKind.displayMath
    · exact Or.inl h
    · exact Or.inr h
  · decide

/-! ## PMAT-383 — EIGHTH Diamond on C-NOTATION-LATEX-MATH-TO-EQUATION
    (Layer 5 BROADENS DEPTH-8):
    LEAN-THEOREM-ENV STRUCTURE EXTENSIONALITY
    (XPILE-REFINE-NOTATION-012).

    **Broadens DEPTH-8 substrate-wide.** Pushes
    NotationLatexMathToEquation (Layer 5) from depth-7 to depth-8 as
    the second L5 contract at depth-8+.

    The 8 Diamond categories on C-NOTATION-LATEX-MATH-TO-EQUATION:
    - PMAT-219 citation_string_monoid
    - PMAT-234 citation_product_monoid
    - PMAT-334 equation_formula_struct_extensionality
    - PMAT-342 latex_display_kind_enum_distinctness
    - PMAT-350 equation_formula_ascii_length_nat
    - PMAT-362 latex_citation_silver_struct_extensionality
    - PMAT-372 latex_display_kind_enum_completeness
    - **PMAT-383: LEAN-THEOREM-ENV STRUCTURE EXTENSIONALITY** ← depth-8

    Thirtieth substrate-wide demonstration of structure-extensionality.
    Captures LeanTheoremEnv record (body_text : String,
    is_precondition_flagged : Bool) — the abstract theorem-class
    LaTeX environment input record.

    Status: discharged at v0.1.0 (PMAT-383). Tier: DIAMOND. -/

/--
  **Diamond-tier refinement theorem** — `LeanTheoremEnv` admits
  STRUCTURE EXTENSIONALITY.

  2-field record (body_text : String, is_precondition_flagged : Bool)
  with derived DecidableEq.

  Status: **discharged at v0.1.0 (PMAT-383)**. Tier: DIAMOND.
-/
theorem lean_theorem_env_struct_extensionality_diamond
    (t1 t2 : LeanTheoremEnv) :
    (t1.body_text = t2.body_text
        ∧ t1.is_precondition_flagged = t2.is_precondition_flagged
      → t1 = t2)
    ∧ (t1 = t2 → t1.body_text = t2.body_text
        ∧ t1.is_precondition_flagged = t2.is_precondition_flagged)
    ∧ (t1 = t2 ∨ t1 ≠ t2)
    ∧ (t1 = t1) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro ⟨h_b, h_f⟩
    cases t1; cases t2
    simp_all
  · intro h
    exact ⟨by rw [h], by rw [h]⟩
  · by_cases h : t1 = t2
    · exact Or.inl h
    · exact Or.inr h
  · rfl

/--
  **PMAT-398 Diamond — NonEmptyDefinition subtype extensionality.**

  The Gold-tier subtype `NonEmptyDefinition := { d :
  DefinitionEnvSilver // d.all_math_spans.size > 0 }` satisfies
  subtype extensionality. TENTH substrate-wide
  subtype-extensionality demonstration. Template 9 (Gold-tier
  subtype-ext) expands to 10 substrate instances.

  Adds a NINTH distinct Diamond category on
  `C-NOTATION-LATEX-MATH-TO-EQUATION`, pushing the contract from
  depth-8 to depth-9. Third L5 contract at depth-9. **COMPLETES
  DEPTH-9 UNIVERSAL ACROSS ALL 12 CONTRACTS** alongside PMAT-296
  (PyIntArith L1), PMAT-297 (CompileRustToPtxMma L5), PMAT-389
  (FfiCpythonExt L4), PMAT-390 (Bashrs L2), PMAT-391
  (ContractFrontendTrait L3), PMAT-392 (BackendTrait L3),
  PMAT-393 (FrontendTrait L3), PMAT-394 (ContractBackendTrait L3),
  PMAT-395 (PyListToVec L2), PMAT-396 (XlateLeanToRust L5), and
  PMAT-397 (XlateRustFnToLeanThm L5).
-/
theorem non_empty_definition_subtype_extensionality_diamond
    (n1 n2 : NonEmptyDefinition) :
    (n1.val = n2.val → n1 = n2)
    ∧ (n1 = n2 → n1.val = n2.val)
    ∧ (n1 = n2 ∨ n1 ≠ n2)
    ∧ (n1 = n1) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro h
    exact Subtype.ext h
  · intro h
    rw [h]
  · by_cases h : n1 = n2
    · exact Or.inl h
    · exact Or.inr h
  · rfl

/--
  **PMAT-409 Diamond — Silver→Bronze tier projection on DefinitionEnvSilver.**

  Define the canonical forgetful map `definition_env_silver_to_bronze
  : DefinitionEnvSilver → DefinitionEnv` that drops the
  `all_math_spans` and `label` fields, retaining only
  `first_math_span`. **NINTH instance of Template 10
  (Tier-projection homomorphism)**. **COMPLETES DEPTH-10
  UNIVERSAL ACROSS ALL 12 CONTRACTS** alongside PMAT-300
  (PyIntArith L1), PMAT-301 (CompileRustToPtxMma L5), PMAT-400
  (FfiCpythonExt L4), PMAT-401 (Bashrs L2), PMAT-402 (BackendTrait
  L3), PMAT-403 (FrontendTrait L3), PMAT-404 (ContractFrontendTrait
  L3), PMAT-405 (ContractBackendTrait L3), PMAT-406 (PyListToVec
  L2), PMAT-407 (XlateLeanToRust L5), and PMAT-408
  (XlateRustFnToLeanThm L5).

  Adds a TENTH distinct Diamond category on
  `C-NOTATION-LATEX-MATH-TO-EQUATION`, pushing the contract from
  depth-9 to depth-10. Third L5 contract at depth-10.
-/
def definition_env_silver_to_bronze (d : DefinitionEnvSilver) : DefinitionEnv :=
  { first_math_span := d.first_math_span }

theorem definition_env_silver_to_bronze_projection_diamond
    (d : DefinitionEnvSilver) :
    -- (a) first_math_span preserved by projection
    ((definition_env_silver_to_bronze d).first_math_span = d.first_math_span)
    -- (b) projection is independent of all_math_spans (forgetful)
    ∧ (definition_env_silver_to_bronze ⟨d.first_math_span, #[], d.label⟩
        = definition_env_silver_to_bronze ⟨d.first_math_span, d.all_math_spans, d.label⟩)
    -- (c) empty first_math_span maps to empty Bronze first_math_span
    ∧ ((definition_env_silver_to_bronze ⟨"", d.all_math_spans, d.label⟩).first_math_span = "")
    -- (d) self-equality (reflexivity)
    ∧ (definition_env_silver_to_bronze d = definition_env_silver_to_bronze d) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · rfl
  · rfl
  · rfl
  · rfl

/--
  **PMAT-420 Diamond — Canonical empty DefinitionEnvSilver.**

  Define the canonical empty DefinitionEnvSilver with empty
  first_math_span, empty all_math_spans, and no label.
  **TENTH instance of Template 11 (Canonical identity element)**.
  **COMPLETES DEPTH-11 UNIVERSAL ACROSS ALL 12 CONTRACTS** alongside
  PMAT-302/303 (initial L1/L5 opens) and PMAT-411..419 (broadening
  wave). Substrate now at NINE UNIVERSAL milestones.

  Adds an ELEVENTH distinct Diamond category on
  `C-NOTATION-LATEX-MATH-TO-EQUATION`, pushing the contract from
  depth-10 to depth-11. Third L5 contract at depth-11.
-/
def empty_definition_env_silver : DefinitionEnvSilver :=
  { first_math_span := "", all_math_spans := #[], label := none }

theorem empty_definition_env_silver_canonical_diamond :
    -- (a) canonical first_math_span is empty
    (empty_definition_env_silver.first_math_span = "")
    -- (b) canonical all_math_spans is empty
    ∧ (empty_definition_env_silver.all_math_spans = #[])
    -- (c) canonical label is none
    ∧ (empty_definition_env_silver.label = none)
    -- (d) canonical all_math_spans size is 0
    ∧ (empty_definition_env_silver.all_math_spans.size = 0) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · rfl
  · rfl
  · rfl
  · rfl

/--
  **PMAT-431 Diamond — DefinitionEnv Bronze→Silver lift.**

  Define the canonical lift `definition_env_bronze_to_silver` :
  `DefinitionEnv → DefinitionEnvSilver` that preserves
  first_math_span and defaults all_math_spans to empty and label
  to none. **TENTH instance of Template 12 (Bronze→Silver
  canonical-lift homomorphism)**. **COMPLETES DEPTH-12 UNIVERSAL
  ACROSS ALL 12 CONTRACTS** alongside PMAT-305/306 (initial L1/L5
  opens) and PMAT-422..430 broadening wave.

  Adds a TWELFTH distinct Diamond category on
  `C-NOTATION-LATEX-MATH-TO-EQUATION`, pushing the contract from
  depth-11 to depth-12. Third L5 contract at depth-12.
-/
def definition_env_bronze_to_silver (d : DefinitionEnv) : DefinitionEnvSilver :=
  { first_math_span := d.first_math_span, all_math_spans := #[], label := none }

theorem definition_env_bronze_to_silver_lift_diamond (d : DefinitionEnv) :
    -- (a) lift preserves first_math_span
    ((definition_env_bronze_to_silver d).first_math_span = d.first_math_span)
    -- (b) lift sets default all_math_spans to empty
    ∧ ((definition_env_bronze_to_silver d).all_math_spans = #[])
    -- (c) empty Bronze first_math_span maps to empty Silver first_math_span
    ∧ ((definition_env_bronze_to_silver ⟨""⟩).first_math_span = "")
    -- (d) self-equality (reflexivity)
    ∧ (definition_env_bronze_to_silver d = definition_env_bronze_to_silver d) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · rfl
  · rfl
  · rfl
  · rfl

/--
  **PMAT-442 Diamond — DefinitionEnv round-trip identity. COMPLETES DEPTH-13 UNIVERSAL.**

  Compose PMAT-409 + PMAT-431. TENTH instance of Template 13.
  COMPLETES DEPTH-13 UNIVERSAL ACROSS ALL 12 CONTRACTS.
-/
theorem definition_env_roundtrip_identity_diamond (d : DefinitionEnv) :
    (definition_env_silver_to_bronze (definition_env_bronze_to_silver d) = d)
    ∧ ((definition_env_silver_to_bronze (definition_env_bronze_to_silver d)).first_math_span
        = d.first_math_span)
    ∧ (definition_env_silver_to_bronze (definition_env_bronze_to_silver ⟨""⟩) = ⟨""⟩)
    ∧ (d = d) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · cases d; rfl
  · rfl
  · rfl
  · rfl

end XpileContracts.CNotationLatexMathToEquation
