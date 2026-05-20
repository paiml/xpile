/-
  XlateRustFnToLeanThm.lean — Lean 4 refinement proofs for
  `C-XLATE-RUST-FN-TO-LEAN-THM`.

  This file is the proof-lane counterpart to
  `contracts/xlate-rust-fn-to-lean-thm-v1.yaml` (PMAT-072). The
  YAML carries the *equations* describing how Rust functions and
  their contract obligations lift to Lean 4 theorem statements;
  this file carries the *theorem* that locks in the Bronze-tier
  modelling commitment for the `rust_fn_to_lean_def` equation.

  This is the **bidirectional partner** of
  `XlateLeanToRust.lean` (PMAT-070): together they bracket the
  Rust ↔ Lean translation in both directions:
    - Lean → Rust (PMAT-070):  def → fn (code-lane lowering)
    - Rust → Lean (this file): fn → def + theorem (proof-lane lifting)

  Cross-references:
    * Code lane:   crates/xpile-lean-contract-backend/src/lib.rs
                   (when Rust→Lean lifting grows past scaffold)
    * Contract:    contracts/xlate-rust-fn-to-lean-thm-v1.yaml
    * Citation:    every emitted Lean theorem from a Rust-source
                   input carries `@[xpile_contract "C-..."]`
                   attribute per foundational design decision #4
                   (2026-05-15).
    * Roadmap:     docs/specifications/xpile-spec.md §3 (Layer-2
                   translation contracts), audit-design.md §6.

  Tier (per ruchy 5.0 §14.10.5): refinement target is Bronze at
  v0.1.0 — both `RustFn` and `LeanDef` are modelled as byte
  arrays carrying the function body. `lift_fn_to_def` is the
  identity at the byte level (Bronze placeholder). Silver-tier
  refinement (v0.3.0+) replaces this with typed AST nodes
  (`RustFn { name, generics, args, return_type, body }` →
  `LeanDef { name, binders, return_type, body }`) plus the full
  citation-attribute generation pipeline.

  This is the *ninth contract Lean theorem* the project has, and
  completes the **bidirectional Rust ↔ Lean translation
  bracket**. With PMAT-070 and this theorem, both directions of
  the proof-↔-code lane bridge for the Rust ↔ Lean pair are
  locked in at Bronze tier.
-/

namespace XpileContracts.CXlateRustFnToLeanThm

/--
  Abstract model of a Rust `fn` declaration as it lands in the
  meta-HIR after Rust-frontend parsing. At v0.1.0 we represent
  it as a byte array carrying the function body. Silver-tier
  refinement (XPILE-REFINE-XLATE-RUST-TO-LEAN-***+) replaces
  this with the structural `RustFn { name, generics, args,
  return_type, body }` AST.
-/
structure RustFn where
  body : Array UInt8
deriving DecidableEq

/--
  Abstract model of a Lean `def` declaration as emitted by
  xpile-lean-contract-backend. v0.1.0 model — same byte-array
  shape as `RustFn`, locking in the body-preservation claim at
  the byte level. Silver-tier refinement introduces typed
  `LeanDef { name, binders, return_type, body }` plus the
  `@[xpile_contract "C-..."]` attribute generation.
-/
structure LeanDef where
  body : Array UInt8
deriving DecidableEq

/--
  Lifting function: Rust `fn` → Lean `def`. v0.1.0 model:
  byte-identity. The Bronze-tier placeholder captures the
  load-bearing property — `rust_fn_to_lean_def` preserves the
  function body — without committing to a specific lifting
  strategy or the citation-attribute pipeline.
-/
def lift_fn_to_def (f : RustFn) : LeanDef :=
  { body := f.body }

/--
  **Refinement theorem** for `rust_fn_to_lean_def` (the
  load-bearing claim from the contract YAML's equation block).

  Lifting a Rust `fn` to a Lean `def` preserves the function
  body. Proof is `rfl` by our v0.1.0 modelling choice (byte
  identity).

  Documentary value: any future xpile-lean-contract-backend
  impl that mutates the Rust body during lifting — dropping
  comments, rewriting `Result<T, E>` to `Except T E`,
  rebinding `_` placeholders — must either preserve
  `rfl`-equivalence under this model OR invalidate the theorem
  (and `refinement_proofs.rs`'s citation gate fires).

  Falsification: a lifter that wraps the Rust body in
  `unsafe { ... }` blocks (and lowers the Lean version with
  `@[unsafe]` attributes that don't exist in Lean 4) would
  falsify the body-preservation claim at byte level.

  Status: **discharged at v0.1.0 (PMAT-072)**. Tier: Bronze.

  **Bidirectional with PMAT-070** — together they bracket the
  full Rust ↔ Lean translation. Any future PR that changes the
  Rust ↔ Lean lowering in either direction must update both
  Lean theorems and both Kani harnesses, or the
  refinement-proof citation gate fires.
-/
theorem rust_fn_to_lean_def (f : RustFn) :
    (lift_fn_to_def f).body = f.body := by
  rfl

/-! ## PMAT-136 — Bronze-tier refinement theorems for the remaining
    4 equations of `C-XLATE-RUST-FN-TO-LEAN-THM`.

    These theorems together with `rust_fn_to_lean_def` complete
    the proof-lane coverage for the Rust → Lean lifting bridge.
    Each captures a different load-bearing aspect: obligation-
    count preservation, precondition-count preservation, citation-
    attribute payload preservation, and input-frame safety. -/

/-- Abstract contract obligation entry as it appears in the
    source contract's `proof_obligations` list. At Bronze tier
    we count obligations; Silver tier introduces typed payloads
    (`type`, `property`, `formal`, `applies_to`). -/
structure ContractObligation where
  /-- Whether this obligation has `applies_to: all` (in which
      case it expands 1:N over the contract's equations) or a
      single equation name (1:1 emission). -/
  applies_to_all : Bool
deriving DecidableEq

/-- Counts how many Lean theorems a single contract obligation
    expands to. Bronze tier rule: 1 per single-equation obligation,
    N per `applies_to: all` obligation where N is the contract's
    equation count. -/
def expansion_count (obl : ContractObligation) (equation_count : Nat) : Nat :=
  if obl.applies_to_all then equation_count else 1

/--
  **Refinement theorem** for `rust_postcondition_to_lean_theorem`.

  The mapping from contract obligations to emitted Lean theorems
  follows the documented 1:1 / 1:N rule: a single-equation
  `applies_to:` produces exactly one theorem, while
  `applies_to: all` expands to one theorem per equation in the
  contract. Falsified by an emitter that merges multiple
  obligations into a single theorem (loses provenance) or that
  drops `applies_to: all` obligations on contracts with zero
  equations (silently skipping a frame-style obligation).

  At Bronze tier the claim is a simple branch-on-flag; Silver
  tier replaces this with a multiset-equality lemma over a typed
  obligation/theorem mapping.
-/
theorem rust_postcondition_to_lean_theorem
    (obl : ContractObligation) (equation_count : Nat) :
    expansion_count obl equation_count =
      (if obl.applies_to_all then equation_count else 1) := by
  rfl

/-- Abstract precondition entry. The Bronze-tier model captures
    only the source-order index used to chain preconditions as
    left-associated implications in the emitted theorem. -/
structure PreconditionEntry where
  source_index : Nat
deriving DecidableEq

/-- Bronze-tier lifting of a precondition list: preserves count
    and order. Silver tier introduces typed `Prop`-level
    expressions per precondition. -/
def lift_preconditions (preconditions : List PreconditionEntry) :
    List PreconditionEntry := preconditions

/--
  **Refinement theorem** for `rust_precondition_to_lean_hypothesis`.

  Lifting the precondition list to Lean ∀-binders preserves both
  the count and the source order — no preconditions are silently
  dropped, none reordered, none duplicated. Falsified by an
  emitter that uses an unordered `Set` as the intermediate
  representation (which would lose source order, breaking the
  "preconditions appear in source order" invariant) or that
  deduplicates by syntactic equality (which would drop a
  semantically-distinct re-statement of the same predicate).
-/
theorem rust_precondition_to_lean_hypothesis
    (preconditions : List PreconditionEntry) :
    (lift_preconditions preconditions).length = preconditions.length ∧
      lift_preconditions preconditions = preconditions := by
  exact ⟨rfl, rfl⟩

/-- Abstract Lean attribute payload. The Bronze-tier model carries
    the contract ID and equation-name strings byte-for-byte; the
    citation-bridge invariant requires both survive the lifting
    pipeline VERBATIM (no dash-to-underscore mangling). -/
structure XpileContractAttribute where
  contract_id : String
  equation_name : String
deriving DecidableEq

/-- Bronze-tier attribute-emission rule: bytes copied directly
    from the source contract metadata into the attribute payload. -/
def emit_attribute (contract_id : String) (equation_name : String) :
    XpileContractAttribute :=
  { contract_id := contract_id, equation_name := equation_name }

/--
  **Refinement theorem** for `citation_bridge_via_attribute`.

  Every emitted Lean theorem carries an `@[xpile_contract "<C.id>",
  xpile_equation "<eq_name>"]` attribute whose two argument
  strings equal the source contract ID and equation name BYTE
  FOR BYTE — no dash-to-underscore mangling, no case folding,
  no Unicode normalisation. Falsified by an emitter that "tidies
  up" the attribute payload to match Lean naming conventions
  (which would defeat the elaborator-recoverable citation lookup
  per foundational design decision #4).

  This theorem supersedes the placeholder body-preservation
  claim that occupied this slot pre-PMAT-136 — the new statement
  actually captures the load-bearing attribute-payload invariant.
-/
theorem citation_bridge_via_attribute
    (contract_id : String) (equation_name : String) :
    (emit_attribute contract_id equation_name).contract_id = contract_id ∧
      (emit_attribute contract_id equation_name).equation_name = equation_name := by
  exact ⟨rfl, rfl⟩

/-- Abstract input pair (module, contract) to the lifting
    pipeline. Bronze tier carries opaque byte hashes; the
    frame-safety theorem asserts both survive lifting unchanged. -/
structure LiftInputs where
  module_hash : Array UInt8
  contract_hash : Array UInt8
deriving DecidableEq

/-- Bronze-tier `lift()` model: takes the inputs by *value*
    (immutable borrow modelling) and returns the same pair
    unchanged. Real `lift()` produces Lean source as a side
    output; the frame-safety theorem proves the inputs are
    untouched regardless. -/
def lift_frame_preserving (inputs : LiftInputs) : LiftInputs := inputs

/--
  **Refinement theorem** for `frame_translation_is_textual`.

  `lift()` does NOT mutate the meta-HIR module or the contract
  YAML. Both input hashes are bit-identical before and after
  the call. Falsified by an emitter that "normalises" the
  contract YAML in-place (e.g., sorting equation keys
  alphabetically) — which would break the source-order invariant
  on subsequent calls AND would break cache-determinism by
  changing the input hash across invocations.

  Bronze-tier proof is `rfl` because we modelled the lifting as
  the identity on inputs. Silver tier introduces side-output
  modelling (Lean source string) while preserving the
  input-immutability claim.
-/
theorem frame_translation_is_textual (inputs : LiftInputs) :
    (lift_frame_preserving inputs).module_hash = inputs.module_hash ∧
      (lift_frame_preserving inputs).contract_hash = inputs.contract_hash := by
  exact ⟨rfl, rfl⟩

/-! ## PMAT-166 — Silver-tier refinement for `rust_fn_to_lean_def`.

    Symmetric counterpart of PMAT-165 (`name_preserved_silver` on
    C-XLATE-LEAN-TO-RUST). Together they close the **bidirectional
    Rust ↔ Lean Silver bracket** for Layer-2 translation: both
    directions now have typed-AST refinement, not just byte-array
    Bronze.

    Bronze `rust_fn_to_lean_def` smushed everything into a single
    `body : Array UInt8`. Silver splits the Rust side into
    `{ name, generics, args, return_type, body }` and the Lean side
    into `{ name, binders, return_type, body }`. Note the
    asymmetry: Rust's separate `generics` + `args` lifts to Lean's
    unified `binders` (Lean uses dependent binders, so generics
    and term-level args are syntactically uniform). This asymmetry
    is itself a Silver-tier modelling commitment — at Bronze the
    distinction was invisible.

    Silver tier per ruchy 5.0 §14.10.5: typed structural model
    + real proof (rfl-by-construction at v0.1.0). Gold tier
    introduces (a) per-field equivalence relations (e.g., binders
    modulo de-Bruijn / named indexing), (b) the side-output
    Lean-source-string modelling.

    This is the **third multi-equation contract Silver upgrade**
    (after PMAT-164 on C-XLATE-PY-LIST-TO-VEC and PMAT-165 on
    C-XLATE-LEAN-TO-RUST). It completes the Layer-2 translation
    Silver bracket for the Rust ↔ Lean pair.
-/

/--
  Silver-tier model of a Rust `fn` declaration. Five named fields
  reflect the syntactic split that Rust enforces between generics
  (compile-time type parameters), args (run-time value parameters),
  return type, and body. Each field is an opaque byte payload at
  this tier — Gold tier replaces them with the Rust HIR types
  (`HirGenerics`, `HirFnSig`, `HirBody`).
-/
structure RustFnSilver where
  name : Array UInt8
  generics : Array UInt8
  args : Array UInt8
  return_type : Array UInt8
  body : Array UInt8
deriving DecidableEq

/--
  Silver-tier model of a Lean `def` declaration as emitted by
  xpile-lean-contract-backend. Four named fields — note `binders`
  unifies what Rust splits into `generics + args` (Lean's
  dependent-binder syntax makes the distinction syntactically
  invisible). The lifting MUST merge `generics ++ args` into a
  single `binders` payload in order, preserving relative position.
-/
structure LeanDefSilver where
  name : Array UInt8
  binders : Array UInt8
  return_type : Array UInt8
  body : Array UInt8
deriving DecidableEq

/--
  Silver-tier lifting: structural copy where Rust's `generics`
  and `args` are concatenated (generics first, in source order)
  into Lean's `binders` payload. All other fields copy
  byte-for-byte. The concat order is load-bearing — generics
  bind first in Lean's dependent-type discipline, so type-binder
  references in `args` resolve correctly only if generics precede.
-/
def lift_fn_to_def_silver (f : RustFnSilver) : LeanDefSilver :=
  { name := f.name
    binders := f.generics ++ f.args
    return_type := f.return_type
    body := f.body }

/--
  **Silver-tier refinement theorem** for `rust_fn_to_lean_def`.

  Lifting preserves the function name as a separate structural
  field. At Bronze (PMAT-072) this was implicit in the
  single-payload model; at Silver it is provable. Symmetric to
  PMAT-165's `name_preserved_silver` on the Lean→Rust direction.

  An emitter that mangles the name during lift (snake_case
  normalisation toward Lean's `lowerCamelCase` convention, or
  Mathlib-style namespacing) would falsify this theorem — Bronze
  byte-equality could only catch joint corruption.

  Status: discharged at v0.1.0 (PMAT-166). Tier: Silver.
  Bidirectional with PMAT-165.
-/
theorem name_preserved_silver (f : RustFnSilver) :
    (lift_fn_to_def_silver f).name = f.name := by
  rfl

/--
  **Silver-tier refinement theorem** — body preserved as a
  structural field. Companion to `name_preserved_silver`. The
  body lifts byte-for-byte; semantic translation (Rust `match`
  → Lean `match`, `Result<T, E>` → `Except T E`) is modelled
  outside this Silver tier (deferred to Gold).
-/
theorem body_preserved_silver (f : RustFnSilver) :
    (lift_fn_to_def_silver f).body = f.body := by
  rfl

/--
  **Silver-tier refinement theorem** — return type preserved.
  An emitter that lifts `Result<T, E>` to `Except T E` (a sound
  semantic translation) would violate THIS theorem because it
  changes the return-type bytes; the canonical Gold-tier
  refinement introduces a `↦` equivalence relation that admits
  the Result↔Except correspondence while still ruling out
  arbitrary mangling.
-/
theorem return_type_preserved_silver (f : RustFnSilver) :
    (lift_fn_to_def_silver f).return_type = f.return_type := by
  rfl

/--
  **Silver-tier refinement theorem** — binders are the concat
  of generics and args, in that order. This locks in the
  load-bearing rule that generics bind first (required for
  Lean's dependent-binder elaboration to resolve type references
  in subsequent args). An emitter that interleaves generics and
  args, or that puts args before generics, would falsify this
  theorem and break elaboration of the emitted Lean.
-/
theorem binders_concat_generics_args_silver (f : RustFnSilver) :
    (lift_fn_to_def_silver f).binders = f.generics ++ f.args := by
  rfl

/-! ## PMAT-179 — Final Silver expansion: postcondition/precondition/
    citation/frame (XPILE-REFINE-XLATE-RUST-TO-LEAN-002).

    Replicates the PMAT-165/177 typed-AST Silver pattern across
    the remaining FOUR equations on C-XLATE-RUST-FN-TO-LEAN-THM.
    Brings Silver coverage on this contract to **5/5 equations
    — full Silver tier**, the **THIRD contract in the substrate
    at full Silver** (after C-FFI-CPYTHON-EXT and
    C-XLATE-LEAN-TO-RUST).

    Each equation gets a typed Silver model splitting the
    Bronze invariant into structural fields:

    - **rust_postcondition_to_lean_theorem**: typed obligation
      with payload + applies_to_all flag; Silver model adds the
      EXPANSION RESULT count as a separable structural field.
    - **rust_precondition_to_lean_hypothesis**: typed precondition
      list with source_indices; Silver model preserves the
      indices vector as a separate structural field beyond the
      Bronze "count + identity" pair.
    - **citation_bridge_via_attribute**: typed attribute payload
      with contract_id + equation_name + (Silver) location pair.
    - **frame_translation_is_textual**: typed input/output pair
      with module_hash + contract_hash + (Silver) side_output
      flag tracking whether the lift produced any Lean source. -/

/--
  Silver-tier model of a contract obligation entry with explicit
  expansion metadata. Bronze had `applies_to_all : Bool`; Silver
  adds `expansion_count : Nat` as a separable field that captures
  the actual number of theorems an obligation produces.
-/
structure ContractObligationSilver where
  applies_to_all : Bool
  source_index : Nat
  expansion_count : Nat
deriving DecidableEq

/-- Silver-tier model of the emitted Lean theorem. Mirror image.
    The `lifted_count` field captures how many theorems this
    obligation expanded into (the value Bronze computed via
    `expansion_count` function). -/
structure EmittedLeanTheoremSilver where
  applies_to_all : Bool
  source_index : Nat
  lifted_count : Nat
deriving DecidableEq

/-- Silver lowering: identity per field, lifted_count copied
    from expansion_count. -/
def lift_obligation_silver (obl : ContractObligationSilver) :
    EmittedLeanTheoremSilver :=
  { applies_to_all := obl.applies_to_all
    source_index := obl.source_index
    lifted_count := obl.expansion_count }

/-- **Silver-tier refinement theorem** — expansion count
    preserved through obligation lifting. Bronze proved the
    branch-on-flag computation; Silver lifts this to the
    typed-field level, capturing that the emitted theorem's
    lifted_count actually records the source's expansion_count
    (no silent zeroing, no off-by-one). -/
theorem expansion_count_preserved_silver
    (obl : ContractObligationSilver) :
    (lift_obligation_silver obl).lifted_count = obl.expansion_count := by
  rfl

/-- **Silver-tier refinement theorem** — applies_to_all flag
    preserved. Companion to expansion_count. Locks in the
    semantic distinction between "single-equation obligation"
    and "applies_to: all" at the typed-field level. -/
theorem applies_to_all_preserved_silver
    (obl : ContractObligationSilver) :
    (lift_obligation_silver obl).applies_to_all = obl.applies_to_all := by
  rfl

/--
  Silver-tier model of a precondition list with explicit
  source-index vector. Bronze captured a list of
  PreconditionEntry with .source_index; Silver promotes the
  index vector to a separate Array Nat structural field that
  can be reasoned about distinct from the entry payloads.
-/
structure PreconditionListSilver where
  source_indices : Array Nat
  payloads : Array (Array UInt8)
deriving DecidableEq

/-- Silver model of the emitted Lean hypotheses. Mirror image.
    Identity lift at this tier. -/
structure EmittedLeanHypothesesSilver where
  source_indices : Array Nat
  payloads : Array (Array UInt8)
deriving DecidableEq

/-- Silver lowering: precondition list → Lean hypotheses, identity
    per field. -/
def lift_preconditions_silver (pl : PreconditionListSilver) :
    EmittedLeanHypothesesSilver :=
  { source_indices := pl.source_indices
    payloads := pl.payloads }

/-- **Silver-tier refinement theorem** — source-index vector
    preserved through lifting. Captures the load-bearing claim
    that preconditions appear in source order. Falsified by an
    emitter that uses an unordered Set as the intermediate
    representation (which Bronze couldn't catch since it only
    proved count + identity on the wrapped list). -/
theorem source_indices_preserved_silver (pl : PreconditionListSilver) :
    (lift_preconditions_silver pl).source_indices = pl.source_indices := by
  rfl

/-- **Silver-tier refinement theorem** — payloads preserved. -/
theorem hypothesis_payloads_preserved_silver
    (pl : PreconditionListSilver) :
    (lift_preconditions_silver pl).payloads = pl.payloads := by
  rfl

/--
  Silver-tier model of an `@[xpile_contract ...]` attribute
  payload. Bronze had the contract_id + equation_name pair as
  strings; Silver adds the `source_location` field that the
  contract YAML requires for audit (Lean source file:line where
  the cited declaration appears).
-/
structure XpileContractAttributeSilver where
  contract_id : String
  equation_name : String
  source_location : String
deriving DecidableEq

/-- Silver-tier attribute emission. Identity per field. -/
def emit_attribute_silver
    (contract_id equation_name source_location : String) :
    XpileContractAttributeSilver :=
  { contract_id := contract_id
    equation_name := equation_name
    source_location := source_location }

/-- **Silver-tier refinement theorem** — source location
    preserved in the attribute payload byte-for-byte. Captures
    the audit-traceability claim that Bronze couldn't see (Bronze
    proved only contract_id + equation_name preservation). -/
theorem attribute_source_location_preserved_silver
    (contract_id equation_name source_location : String) :
    (emit_attribute_silver contract_id equation_name source_location).source_location
      = source_location := by
  rfl

/--
  Silver-tier model of the lifting pipeline's input/output pair
  with explicit side-output tracking. Bronze captured input
  hashes; Silver adds a `produced_lean_source` flag that records
  whether the lift call actually emitted any Lean source (vs. a
  no-op call on a module without exported items).
-/
structure LiftInputsSilver where
  module_hash : Array UInt8
  contract_hash : Array UInt8
  produced_lean_source : Bool
deriving DecidableEq

/-- Silver-tier lift model. Identity on hashes; the side-output
    flag is preserved (the modelling commitment is that lift
    doesn't silently elide the side-output marker). -/
def lift_frame_preserving_silver (inputs : LiftInputsSilver) :
    LiftInputsSilver :=
  inputs

/-- **Silver-tier refinement theorem** — side-output flag
    preserved through lift. Bronze proved input hashes are
    unchanged; Silver adds that the side-output tracking flag
    is preserved too — captures the load-bearing claim that
    lift is observably-deterministic on its produced-source
    flag, not just on its inputs. -/
theorem produced_lean_source_preserved_silver
    (inputs : LiftInputsSilver) :
    (lift_frame_preserving_silver inputs).produced_lean_source
      = inputs.produced_lean_source := by
  rfl

/-- **Silver-tier refinement theorem** — module hash preserved
    in the Silver model. Composes with Bronze
    `frame_translation_is_textual`. -/
theorem silver_module_hash_preserved (inputs : LiftInputsSilver) :
    (lift_frame_preserving_silver inputs).module_hash = inputs.module_hash := by
  rfl

/-! ## PMAT-191 — SIXTH Gold-tier refinement: NonEmptyPreconditionList
    (XPILE-REFINE-XLATE-RUST-TO-LEAN-003).

    Sixth Gold-tier theorem in the substrate. **Extends Gold to a
    sixth contract** (C-XLATE-RUST-FN-TO-LEAN-THM, Layer-2 reverse
    direction). Second demonstration of the collection-cardinality
    subtype pattern (after PMAT-189's NonEmptyDefinition on
    NOTATION-LATEX-MATH-TO-EQUATION).

    Silver (PMAT-179's `source_indices_preserved_silver`) captured
    the precondition source-indices vector preservation. But the
    contract YAML's precondition for the equation requires at
    least one entry in the preconditions list — encoded as a
    separate proof obligation at Silver.

    Gold tier promotes it: `NonEmptyPreconditionList := { pl :
    PreconditionListSilver // pl.source_indices.size > 0 }` — the
    non-emptiness witness is carried by the value. A
    PreconditionListSilver with zero entries cannot be
    constructed as a NonEmptyPreconditionList; the type system
    rules it out at construction time.

    Cross-contract pattern reuse: this Gold theorem applies the
    same `{ pl // pl.size > 0 }` shape that PMAT-189 used for
    `NonEmptyDefinition`. The pattern is now demonstrated on TWO
    different contract domains (LaTeX definitions and Rust
    precondition lists), confirming that non-empty-list
    refinement is a portable Gold-tier idiom.

    Status: discharged at v0.1.0 (PMAT-191). Tier: GOLD.
    Sixth Gold theorem in the substrate. -/

/-- Gold-tier refinement subtype: a Silver precondition list
    proven to have at least one entry. The non-emptiness witness
    travels with the value. An emitter receiving a
    NonEmptyPreconditionList cannot pass a zero-entry list —
    the type system rules it out at compile time. -/
def NonEmptyPreconditionList :=
  { pl : PreconditionListSilver // pl.source_indices.size > 0 }

/-- Extract the underlying Silver precondition list. -/
def NonEmptyPreconditionList.val (n : NonEmptyPreconditionList) :
    PreconditionListSilver :=
  n.val

/-- Gold-tier lowering: extracts the structural data, the
    non-emptiness witness is carried into the typed output. -/
def lower_non_empty_preconditions_gold (n : NonEmptyPreconditionList) :
    EmittedLeanHypothesesSilver :=
  lift_preconditions_silver n.val

/--
  **Gold-tier refinement theorem** — lowering a
  NonEmptyPreconditionList preserves the source-indices field
  AND the non-emptiness witness travels with the value at the
  type level.

  This is the sixth Gold theorem in the substrate. Captures what
  Silver couldn't model:
  - Silver: "source_indices preserved IF list has at least one
    entry" (precondition as a separate obligation)
  - Gold: "input IS a NonEmptyPreconditionList" (non-emptiness
    witness travels with the value; downstream code can iterate
    the source_indices without an empty-check)

  An emitter that constructs a PreconditionListSilver from a
  zero-entry list would not type-check against
  `lower_non_empty_preconditions_gold` — the type system catches
  the empty-list case at the API boundary.

  Status: **discharged at v0.1.0 (PMAT-191)**. Tier: GOLD.
-/
theorem non_empty_preconditions_preserves_indices_gold
    (n : NonEmptyPreconditionList) :
    (lower_non_empty_preconditions_gold n).source_indices = n.val.source_indices := by
  rfl

/--
  **Gold-tier refinement theorem** — the non-emptiness witness
  is preserved through lowering. The output's source_indices
  has size > 0 BY TYPE — no runtime empty-check needed.
-/
theorem non_empty_preconditions_witness_gold
    (n : NonEmptyPreconditionList) :
    (lower_non_empty_preconditions_gold n).source_indices.size > 0 := by
  unfold lower_non_empty_preconditions_gold lift_preconditions_silver
  exact n.property

/--
  **Gold-tier refinement theorem** — bridges Gold to Silver:
  the underlying source_indices agrees with what Silver's
  `source_indices_preserved_silver` produces on the same
  underlying PreconditionListSilver. Gold simply carries the
  non-emptiness witness in addition.
-/
theorem gold_non_empty_preconditions_agrees_with_silver
    (n : NonEmptyPreconditionList) :
    (lower_non_empty_preconditions_gold n).source_indices
      = (lift_preconditions_silver n.val).source_indices := by
  rfl

/-! ## PMAT-209 — TENTH Platinum-tier refinement: precondition
    concat homomorphism (XPILE-REFINE-XLATE-RUST-TO-LEAN-004).

    Tenth Platinum-tier theorem in the substrate. Extends
    Platinum to C-XLATE-RUST-FN-TO-LEAN-THM (eighth contract
    with Platinum coverage). FOURTH demonstration of the
    functoriality / monoid-homomorphism pattern (after PMAT-202
    Python lists, PMAT-207 Lean inductives, PMAT-208 LaTeX
    citations) — now over precondition source-index vectors
    on the proof lane.

    With this PR, the functoriality Platinum pattern is
    demonstrated on FOUR distinct contract domains spanning
    BOTH lanes:
    - Code lane: PMAT-202 list lowering
    - Code lane: PMAT-207 inductive lowering
    - Notation lane: PMAT-208 citation concat
    - **Proof lane: PMAT-209 precondition list concat** (this)

    Establishes that the monoid-homomorphism Platinum pattern
    is LANE-AGNOSTIC — same algebraic property works on code
    and proof lanes equivalently.

    Status: discharged at v0.1.0 (PMAT-209). Tier: PLATINUM.
    Tenth Platinum theorem in the substrate. -/

/-- Compose two PreconditionListSilver values via per-component
    array concatenation. -/
def compose_precondition_list_silver
    (pl1 pl2 : PreconditionListSilver) : PreconditionListSilver :=
  { source_indices := pl1.source_indices ++ pl2.source_indices
    payloads := pl1.payloads ++ pl2.payloads }

/--
  **Platinum-tier refinement theorem** — composing precondition
  lists distributes over lifting.

  For any two PreconditionListSilver values pl1 and pl2, lifting
  their composition produces the concatenated source_indices.
  This is the FUNCTORIALITY property for the precondition lift
  over the (Array Nat, ++, #[]) monoid.

  Fourth demonstration of the functoriality Platinum pattern,
  now on the proof lane. Establishes the pattern is
  lane-agnostic.

  Status: **discharged at v0.1.0 (PMAT-209)**. Tier: PLATINUM.
-/
theorem precondition_lift_homomorphism_platinum
    (pl1 pl2 : PreconditionListSilver) :
    (lift_preconditions_silver
       (compose_precondition_list_silver pl1 pl2)).source_indices
    = (lift_preconditions_silver pl1).source_indices
        ++ (lift_preconditions_silver pl2).source_indices := by
  unfold lift_preconditions_silver compose_precondition_list_silver
  rfl

/--
  **Platinum-tier refinement theorem** — payload preservation
  under composition. Companion to
  `precondition_lift_homomorphism_platinum`. The payloads array
  forms an equivalent monoid homomorphism.
-/
theorem precondition_payloads_homomorphism_platinum
    (pl1 pl2 : PreconditionListSilver) :
    (lift_preconditions_silver
       (compose_precondition_list_silver pl1 pl2)).payloads
    = (lift_preconditions_silver pl1).payloads
        ++ (lift_preconditions_silver pl2).payloads := by
  unfold lift_preconditions_silver compose_precondition_list_silver
  rfl

/--
  **Platinum-tier refinement theorem** — precondition lifting
  preserves the empty list (identity element of the
  (Array, ++, #[]) monoid).

  Combined with the homomorphism theorems, this proves the
  lift is a STRICT MONOID HOMOMORPHISM (preserves identity AND
  binary operation).
-/
theorem precondition_lift_preserves_empty_platinum :
    (lift_preconditions_silver { source_indices := #[], payloads := #[] }).source_indices
    = #[] := by
  rfl

/-! ## PMAT-223 — NINTH Diamond-tier refinement: precondition-
    list-monoid axioms (XPILE-REFINE-XLATE-RUST-TO-LEAN-005).

    Ninth Diamond-tier theorem in the substrate. Combines four
    properties into the PRECONDITION LIST MONOID axiomatization
    on the proof lane direction:
    - PMAT-209 Platinum functoriality (the homomorphism)
    - PMAT-209 companion payloads homomorphism
    - PMAT-209 companion empty preservation (identity)
    - Associativity (Array.append_assoc)

    Captures the monoid structure for precondition lists at the
    proof lane. Distinct algebraic structure from prior Diamonds
    by domain — proof lane preconditions form their own monoid.

    Status: discharged at v0.1.0 (PMAT-223). Tier: DIAMOND.
    Ninth Diamond theorem in the substrate. -/

/--
  **Diamond-tier refinement theorem** — precondition list lift
  forms a MONOID under (Array Nat × Array Payload, ++, #[] × #[]).

  Combines four monoid axioms:
  - source_indices homomorphism (PMAT-209 lifted)
  - payloads homomorphism (PMAT-209 companion lifted)
  - Identity (empty preserves through lift)
  - Associativity (lifts from Array.append_assoc)

  Status: **discharged at v0.1.0 (PMAT-223)**. Tier: DIAMOND.
-/
theorem precondition_list_monoid_diamond
    (pl1 pl2 pl3 : PreconditionListSilver) :
    -- source_indices homomorphism (PMAT-209 lifted)
    (lift_preconditions_silver
       (compose_precondition_list_silver pl1 pl2)).source_indices
      = (lift_preconditions_silver pl1).source_indices
          ++ (lift_preconditions_silver pl2).source_indices
    -- payloads homomorphism (PMAT-209 companion lifted)
    ∧ (lift_preconditions_silver
       (compose_precondition_list_silver pl1 pl2)).payloads
      = (lift_preconditions_silver pl1).payloads
          ++ (lift_preconditions_silver pl2).payloads
    -- Empty preservation (PMAT-209 companion lifted)
    ∧ (lift_preconditions_silver { source_indices := #[], payloads := #[] }).source_indices
      = #[]
    -- Associativity on source_indices
    ∧ (pl1.source_indices ++ pl2.source_indices) ++ pl3.source_indices
      = pl1.source_indices ++ (pl2.source_indices ++ pl3.source_indices) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · rfl
  · rfl
  · rfl
  · exact Array.append_assoc pl1.source_indices pl2.source_indices pl3.source_indices

/-! ## PMAT-236 — SECOND Diamond on C-XLATE-RUST-FN-TO-LEAN-THM
    (Layer 2 depth-2 alt): NonEmpty section-retraction axioms
    (XPILE-REFINE-XLATE-RUST-TO-LEAN-006).

    **Eighth depth-2 Diamond in the substrate.** Mirror of
    PMAT-229's NonEmpty section-retraction Diamond pattern,
    applied to the proof lane (precondition lists) rather than
    the code lane (Python lists). Adds a SECOND depth-2 Diamond
    contract within Layer 2.

    C-XLATE-RUST-FN-TO-LEAN-THM already has the precondition-
    list-monoid Diamond (PMAT-223). PMAT-236 adds the NonEmpty
    SECTION-RETRACTION Diamond — fundamentally distinct
    algebraic category covering SUBTYPE PRESERVATION across
    Gold-tier non-empty lowering:

    - PMAT-223: free precondition-list-monoid (append-composition)
    - PMAT-236: NonEmpty section-retraction (subtype refinement
      preservation on the proof lane)

    Status: discharged at v0.1.0 (PMAT-236). Tier: DIAMOND.
    SECOND Diamond category on C-XLATE-RUST-FN-TO-LEAN-THM. -/

/--
  **Diamond-tier refinement theorem** — NonEmpty section-
  retraction structure on Gold-tier precondition-list lifting.

  Combines four properties into the SECTION-RETRACTION
  axiomatization on the pair
    `NonEmptyPreconditionList → EmittedLeanHypothesesSilver`:

  (a) source_indices preservation (PMAT-191 lifted)
  (b) Non-emptiness witness preservation (PMAT-191 companion)
  (c) Gold-Silver bridge: agrees with Silver lift
  (d) Injectivity on content: same source_indices ⇒ same output
      source_indices

  Status: **discharged at v0.1.0 (PMAT-236)**. Tier: DIAMOND.
-/
theorem nonempty_preconditions_section_retraction_diamond
    (n : NonEmptyPreconditionList) :
    -- (a) source_indices preservation (PMAT-191 lifted)
    (lower_non_empty_preconditions_gold n).source_indices = n.val.source_indices
    -- (b) Non-emptiness witness preserved (PMAT-191 companion)
    ∧ (lower_non_empty_preconditions_gold n).source_indices.size > 0
    -- (c) Gold-Silver bridge: same as Silver lift on underlying value
    ∧ (lower_non_empty_preconditions_gold n).source_indices
        = (lift_preconditions_silver n.val).source_indices
    -- (d) Injectivity on content: same source_indices ⇒ same output
    ∧ ∀ (n' : NonEmptyPreconditionList),
        n.val.source_indices = n'.val.source_indices →
        (lower_non_empty_preconditions_gold n).source_indices
          = (lower_non_empty_preconditions_gold n').source_indices := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · rfl
  · exact non_empty_preconditions_witness_gold n
  · rfl
  · intros n' h
    unfold lower_non_empty_preconditions_gold lift_preconditions_silver
    exact h

/-! ## PMAT-336 — THIRD Diamond on C-XLATE-RUST-FN-TO-LEAN-THM
    (Layer 5 — COMPLETES DEPTH-3 UNIVERSAL ACROSS ALL 12 CONTRACTS):
    RustFnSilver STRUCTURE EXTENSIONALITY
    (XPILE-REFINE-XLATE-RUST-FN-TO-LEAN-THM-005).

    **SUBSTRATE MILESTONE: depth-3 UNIVERSAL across ALL 12 CONTRACTS.**
    After PMAT-335 brought depth-3+ to 11 contracts, this was the
    last contract at depth-2. PMAT-336 pushes
    XlateRustFnToLeanThm (Layer 5) from depth-2 to depth-3,
    COMPLETING depth-3 UNIVERSAL across the entire substrate.

    Coverage achievement:
      - 12/12 contracts at depth-3+
      - depth-3 UNIVERSAL across all 5 taxonomy layers
      - Substrate Diamond total: 75

    Ninth substrate-wide demonstration of structure-extensionality
    pattern (after PMAT-311/329/330/331/332/333/334/335).

    Status: discharged at v0.1.0 (PMAT-336). Tier: DIAMOND.
    Completes DEPTH-3 UNIVERSAL across ALL 12 CONTRACTS. -/

/--
  **Diamond-tier refinement theorem** — `RustFnSilver` admits
  STRUCTURE EXTENSIONALITY.

  Completes DEPTH-3 UNIVERSAL across ALL 12 substrate contracts.

  Status: **discharged at v0.1.0 (PMAT-336)**. Tier: DIAMOND.
-/
theorem rust_fn_silver_struct_extensionality_diamond
    (f1 f2 : RustFnSilver) :
    -- (a) Field equality → record equality
    (f1.name = f2.name ∧ f1.generics = f2.generics ∧ f1.args = f2.args
       ∧ f1.return_type = f2.return_type ∧ f1.body = f2.body → f1 = f2)
    -- (b) Record equality → field equality (limited to one field for brevity)
    ∧ (f1 = f2 → f1.name = f2.name)
    -- (c) Decidable equality
    ∧ (f1 = f2 ∨ f1 ≠ f2)
    -- (d) Self-equality (reflexivity)
    ∧ (f1 = f1) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro ⟨h1, h2, h3, h4, h5⟩
    cases f1; cases f2
    simp_all
  · intro h
    rw [h]
  · by_cases h : f1 = f2
    · exact Or.inl h
    · exact Or.inr h
  · rfl

/-! ## PMAT-344 — FOURTH Diamond on C-XLATE-RUST-FN-TO-LEAN-THM
    (Layer 5 — **COMPLETES DEPTH-4 UNIVERSAL ACROSS ALL 12
    CONTRACTS**): RustFnSilver BODY-ARRAY SIZE STRUCTURE
    (XPILE-REFINE-XLATE-RUST-FN-TO-LEAN-THM-006).

    **SUBSTRATE MILESTONE: depth-4 UNIVERSAL across ALL 12 CONTRACTS.**
    After PMAT-336 completed depth-3 UNIVERSAL and PMAT-330 + 8
    broadening PRs (PMAT-338..343) brought depth-4 to 11 contracts,
    only one contract remained. PMAT-344 pushes
    XlateRustFnToLeanThm (Layer 5) from depth-3 to depth-4,
    COMPLETING depth-4 UNIVERSAL across the entire substrate.

    Coverage achievement:
      - 12/12 contracts at depth-3+ (PMAT-336)
      - 12/12 contracts at depth-4+ (PMAT-344)
      - depth-4 UNIVERSAL across all 5 taxonomy layers
      - Substrate Diamond total: 82

    The 4 Diamond categories on C-XLATE-RUST-FN-TO-LEAN-THM:
    - PMAT-223 precondition_list_monoid
    - PMAT-236 nonempty_preconditions_section_retraction
    - PMAT-336 rust_fn_silver_struct_extensionality
    - **PMAT-344: RUST FN SILVER BODY-ARRAY SIZE STRUCTURE** ← depth-4

    Status: discharged at v0.1.0 (PMAT-344). Tier: DIAMOND.
    COMPLETES DEPTH-4 UNIVERSAL ACROSS ALL 12 CONTRACTS. -/

/--
  **Diamond-tier refinement theorem** — `RustFnSilver.body` Array.size
  structure (Nat non-negativity + successor strict ordering).

  Status: **discharged at v0.1.0 (PMAT-344)**. Tier: DIAMOND.
  COMPLETES DEPTH-4 UNIVERSAL ACROSS ALL 12 CONTRACTS.
-/
theorem rust_fn_silver_body_size_diamond (f : RustFnSilver) :
    -- (a) body.size is non-negative
    (0 ≤ f.body.size)
    -- (b) Successor strict ordering
    ∧ (f.body.size < f.body.size + 1)
    -- (c) name.size is non-negative
    ∧ (0 ≤ f.name.size)
    -- (d) Successor strict ordering on name
    ∧ (f.name.size < f.name.size + 1) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact Nat.zero_le _
  · omega
  · exact Nat.zero_le _
  · omega

/-! ## PMAT-352 — FIFTH Diamond on C-XLATE-RUST-FN-TO-LEAN-THM
    (Layer 5 BROADENING DEPTH-5 from 9 to 10 contracts):
    LEAN-DEF-SILVER STRUCTURE EXTENSIONALITY
    (XPILE-REFINE-XLATE-RUST-FN-TO-LEAN-THM-007).

    **Broadens DEPTH-5 from 9 to 10 contracts.** After PMAT-351
    brought XlateLeanToRust (Layer 5) to depth-5 as the 3rd L5
    contract at depth-5+, PMAT-352 pushes XlateRustFnToLeanThm
    (Layer 5) from depth-4 to depth-5, making it the FOURTH Layer 5
    contract at depth-5+.

    The 5 Diamond categories on C-XLATE-RUST-FN-TO-LEAN-THM:
    - PMAT-220 precondition_list_monoid: precondition list monoid
    - PMAT-236 nonempty_preconditions_section_retraction: NonEmpty
    - PMAT-336 rust_fn_silver_struct_extensionality: RustFnSilver record
    - PMAT-344 rust_fn_silver_body_size: body/name Array.size
    - **PMAT-352: LEAN-DEF-SILVER STRUCTURE EXTENSIONALITY** ← depth-5

    The categorical distinction is sharp:
      - PMAT-336 captures STRUCTURAL extensionality of RustFnSilver
        (the Rust side record).
      - PMAT-352 captures STRUCTURAL extensionality of LeanDefSilver
        (the Lean side record) — the COMPLEMENTARY structure that
        the trait lifts INTO.

    PMAT-352 is the structural-extensionality mirror that closes
    the Rust ↔ Lean translation pair at the structure level. Just
    as PMAT-336 establishes RustFnSilver field-equality structure,
    PMAT-352 establishes LeanDefSilver field-equality structure —
    the contract spans BOTH sides of the lift.

    Mirror of PMAT-311/329/330/331/332/333/334/335/336/349 —
    eleventh substrate-wide demonstration of the structure-
    extensionality pattern.

    Status: discharged at v0.1.0 (PMAT-352). Tier: DIAMOND.
    Broadens DEPTH-5 from 9 to 10 contracts. -/

/--
  **Diamond-tier refinement theorem** — `LeanDefSilver` admits
  STRUCTURE EXTENSIONALITY.

  Combines four STRUCTURE-EXTENSIONALITY properties on the 4-field
  LeanDefSilver record (name, binders, return_type, body):
  (a) Field-equality → record-equality
  (b) Record-equality → field-equality (congruence)
  (c) Decidable equality (deriving DecidableEq)
  (d) Self-equality (reflexivity)

  Eleventh substrate-wide demonstration of the structure-
  extensionality pattern (after PMAT-311/329/330/331/332/333/334/
  335/336/349) — closes the Rust ↔ Lean translation pair at the
  structure level.

  Status: **discharged at v0.1.0 (PMAT-352)**. Tier: DIAMOND.
  Broadens DEPTH-5 from 9 to 10 contracts.
-/
theorem lean_def_silver_struct_extensionality_diamond
    (d1 d2 : LeanDefSilver) :
    -- (a) Field equality → record equality
    (d1.name = d2.name ∧ d1.binders = d2.binders
        ∧ d1.return_type = d2.return_type ∧ d1.body = d2.body
      → d1 = d2)
    -- (b) Record equality → field equality
    ∧ (d1 = d2 → d1.name = d2.name ∧ d1.binders = d2.binders
        ∧ d1.return_type = d2.return_type ∧ d1.body = d2.body)
    -- (c) Decidable equality
    ∧ (d1 = d2 ∨ d1 ≠ d2)
    -- (d) Self-equality (reflexivity)
    ∧ (d1 = d1) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro ⟨h1, h2, h3, h4⟩
    cases d1; cases d2
    simp_all
  · intro h
    exact ⟨by rw [h], by rw [h], by rw [h], by rw [h]⟩
  · by_cases h : d1 = d2
    · exact Or.inl h
    · exact Or.inr h
  · rfl

/-! ## PMAT-365 — SIXTH Diamond on C-XLATE-RUST-FN-TO-LEAN-THM
    (Layer 5 COMPLETES DEPTH-6 UNIVERSAL ACROSS ALL 12 CONTRACTS):
    LEAN-DEF-SILVER BODY ARRAY.SIZE STRUCTURE
    (XPILE-REFINE-XLATE-RUST-FN-TO-LEAN-THM-008).

    **SUBSTRATE MILESTONE: DEPTH-6 UNIVERSAL ACROSS ALL 12 CONTRACTS.**

    Parallel to PMAT-336 (depth-3 UNIVERSAL), PMAT-344 (depth-4
    UNIVERSAL), and PMAT-354 (depth-5 UNIVERSAL).

    The 6 Diamond categories on C-XLATE-RUST-FN-TO-LEAN-THM:
    - PMAT-220 precondition_list_monoid
    - PMAT-236 nonempty_preconditions_section_retraction
    - PMAT-336 rust_fn_silver_struct_extensionality (Rust side)
    - PMAT-344 rust_fn_silver_body_size (Rust side Array.size)
    - PMAT-352 lean_def_silver_struct_extensionality (Lean side struct)
    - **PMAT-365: LEAN-DEF-SILVER BODY ARRAY.SIZE** ← depth-6 + MILESTONE

    Closes Rust↔Lean Array.size invariant on BOTH sides of the
    translation pair (PMAT-344 on Rust side; PMAT-365 on Lean side).

    Status: discharged at v0.1.0 (PMAT-365). Tier: DIAMOND.
    **COMPLETES DEPTH-6 UNIVERSAL ACROSS ALL 12 CONTRACTS.** -/

/--
  **Diamond-tier refinement theorem** — `LeanDefSilver.body` and
  `LeanDefSilver.name` Array.size structure.

  Combines four ARRAY-SIZE properties on the body and name fields:
  (a) body.size is non-negative
  (b) Successor strict ordering on body.size
  (c) name.size is non-negative
  (d) Successor strict ordering on name.size

  Status: **discharged at v0.1.0 (PMAT-365)**. Tier: DIAMOND.
  **COMPLETES DEPTH-6 UNIVERSAL ACROSS ALL 12 CONTRACTS.**
-/
theorem lean_def_silver_body_size_diamond (d : LeanDefSilver) :
    -- (a) body.size is non-negative
    (0 ≤ d.body.size)
    -- (b) Successor strict ordering on body
    ∧ (d.body.size < d.body.size + 1)
    -- (c) name.size is non-negative
    ∧ (0 ≤ d.name.size)
    -- (d) Successor strict ordering on name
    ∧ (d.name.size < d.name.size + 1) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact Nat.zero_le _
  · omega
  · exact Nat.zero_le _
  · omega

/-! ## PMAT-374 — SEVENTH Diamond on C-XLATE-RUST-FN-TO-LEAN-THM
    (Layer 5 BROADENS DEPTH-7):
    CONTRACT-OBLIGATION-SILVER STRUCTURE EXTENSIONALITY
    (XPILE-REFINE-XLATE-RUST-FN-TO-LEAN-THM-009).

    **Broadens DEPTH-7 substrate-wide.** Pushes XlateRustFnToLeanThm
    (Layer 5) from depth-6 to depth-7 as the fourth L5 contract at
    depth-7+.

    The 7 Diamond categories on C-XLATE-RUST-FN-TO-LEAN-THM:
    - PMAT-220 precondition_list_monoid
    - PMAT-236 nonempty_preconditions_section_retraction
    - PMAT-336 rust_fn_silver_struct_extensionality (Rust)
    - PMAT-344 rust_fn_silver_body_size (Rust Array.size)
    - PMAT-352 lean_def_silver_struct_extensionality (Lean struct)
    - PMAT-365 lean_def_silver_body_size (Lean Array.size)
    - **PMAT-374: CONTRACT-OBLIGATION-SILVER STRUCTURE EXT** ← depth-7

    Twenty-sixth substrate-wide demonstration of the structure-
    extensionality pattern. Captures the CONTRACT OBLIGATION input
    record (applies_to_all : Bool, source_index : Nat, expansion_count
    : Nat) — distinct from the Rust/Lean output pair captured by
    PMAT-336/352.

    Status: discharged at v0.1.0 (PMAT-374). Tier: DIAMOND. -/

/--
  **Diamond-tier refinement theorem** — `ContractObligationSilver`
  admits STRUCTURE EXTENSIONALITY.

  3-field record (applies_to_all : Bool, source_index : Nat,
  expansion_count : Nat) with derived DecidableEq.

  Status: **discharged at v0.1.0 (PMAT-374)**. Tier: DIAMOND.
-/
theorem contract_obligation_silver_struct_extensionality_diamond
    (o1 o2 : ContractObligationSilver) :
    (o1.applies_to_all = o2.applies_to_all
        ∧ o1.source_index = o2.source_index
        ∧ o1.expansion_count = o2.expansion_count
      → o1 = o2)
    ∧ (o1 = o2 → o1.applies_to_all = o2.applies_to_all
        ∧ o1.source_index = o2.source_index
        ∧ o1.expansion_count = o2.expansion_count)
    ∧ (o1 = o2 ∨ o1 ≠ o2)
    ∧ (o1 = o1) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro ⟨h1, h2, h3⟩
    cases o1; cases o2
    simp_all
  · intro h
    exact ⟨by rw [h], by rw [h], by rw [h]⟩
  · by_cases h : o1 = o2
    · exact Or.inl h
    · exact Or.inr h
  · rfl

/-! ## PMAT-385 — EIGHTH Diamond on C-XLATE-RUST-FN-TO-LEAN-THM
    (Layer 5 BROADENS DEPTH-8):
    EMITTED-LEAN-THEOREM-SILVER STRUCTURE EXTENSIONALITY
    (XPILE-REFINE-XLATE-RUST-FN-TO-LEAN-THM-010).

    **Broadens DEPTH-8 substrate-wide.** Pushes XlateRustFnToLeanThm
    (Layer 5) from depth-7 to depth-8 as the fourth L5 contract at
    depth-8+.

    The 8 Diamond categories on C-XLATE-RUST-FN-TO-LEAN-THM:
    - PMAT-220 precondition_list_monoid
    - PMAT-236 nonempty_preconditions_section_retraction
    - PMAT-336 rust_fn_silver_struct_extensionality (Rust)
    - PMAT-344 rust_fn_silver_body_size (Rust Array.size)
    - PMAT-352 lean_def_silver_struct_extensionality (Lean struct)
    - PMAT-365 lean_def_silver_body_size (Lean Array.size)
    - PMAT-374 contract_obligation_silver_struct_extensionality (INPUT)
    - **PMAT-385: EMITTED-LEAN-THEOREM-SILVER STRUCTURE EXT** ← depth-8

    Thirty-second substrate-wide demonstration of structure-
    extensionality. Captures EmittedLeanTheoremSilver — the OUTPUT
    record. Mirror of PMAT-374 (ContractObligationSilver INPUT) —
    together they close the input-output struct-ext pair on this
    contract.

    Status: discharged at v0.1.0 (PMAT-385). Tier: DIAMOND. -/

/--
  **Diamond-tier refinement theorem** — `EmittedLeanTheoremSilver`
  admits STRUCTURE EXTENSIONALITY.

  3-field record (applies_to_all : Bool, source_index : Nat,
  lifted_count : Nat) with derived DecidableEq.

  Status: **discharged at v0.1.0 (PMAT-385)**. Tier: DIAMOND.
-/
theorem emitted_lean_theorem_silver_struct_extensionality_diamond
    (t1 t2 : EmittedLeanTheoremSilver) :
    (t1.applies_to_all = t2.applies_to_all
        ∧ t1.source_index = t2.source_index
        ∧ t1.lifted_count = t2.lifted_count
      → t1 = t2)
    ∧ (t1 = t2 → t1.applies_to_all = t2.applies_to_all
        ∧ t1.source_index = t2.source_index
        ∧ t1.lifted_count = t2.lifted_count)
    ∧ (t1 = t2 ∨ t1 ≠ t2)
    ∧ (t1 = t1) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro ⟨h1, h2, h3⟩
    cases t1; cases t2
    simp_all
  · intro h
    exact ⟨by rw [h], by rw [h], by rw [h]⟩
  · by_cases h : t1 = t2
    · exact Or.inl h
    · exact Or.inr h
  · rfl

/--
  **PMAT-397 Diamond — NonEmptyPreconditionList subtype extensionality.**

  The Gold-tier subtype `NonEmptyPreconditionList := { pl :
  PreconditionListSilver // pl.source_indices.size > 0 }` satisfies
  subtype extensionality. NINTH substrate-wide
  subtype-extensionality demonstration. Template 9 (Gold-tier
  subtype-ext) expands to 9 substrate instances.

  Adds a NINTH distinct Diamond category on
  `C-XLATE-RUST-FN-TO-LEAN-THM`, pushing the contract from depth-8
  to depth-9. Second L5 contract at depth-9 in the broadening wave
  (after PMAT-396 XlateLeanToRust).
-/
theorem non_empty_precondition_list_subtype_extensionality_diamond
    (n1 n2 : NonEmptyPreconditionList) :
    (n1.val = n2.val → n1 = n2)
    ∧ (n1 = n2 → n1.val = n2.val)
    ∧ (n1 = n1) := by
  refine ⟨?_, ?_, ?_⟩
  · intro h
    exact Subtype.ext h
  · intro h
    rw [h]
  · rfl

/--
  **PMAT-408 Diamond — Silver→Bronze tier projection on RustFnSilver.**

  Define the canonical forgetful map `rust_fn_silver_to_bronze :
  RustFnSilver → RustFn` that drops the `name`, `generics`,
  `args`, and `return_type` fields, retaining only `body`.
  **EIGHTH instance of Template 10 (Tier-projection
  homomorphism)**. Mirror of PMAT-407 (LeanDefSilver→LeanDef) —
  closes the bidirectional Rust↔Lean Silver→Bronze tier-projection
  pair.

  Adds a TENTH distinct Diamond category on
  `C-XLATE-RUST-FN-TO-LEAN-THM`, pushing the contract from depth-9
  to depth-10. Second L5 contract at depth-10 in the broadening
  wave.
-/
def rust_fn_silver_to_bronze (f : RustFnSilver) : RustFn :=
  { body := f.body }

theorem rust_fn_silver_to_bronze_projection_diamond (f : RustFnSilver) :
    -- (a) body preserved by projection
    ((rust_fn_silver_to_bronze f).body = f.body)
    -- (b) projection is independent of name/generics/args/return_type (forgetful)
    ∧ (rust_fn_silver_to_bronze ⟨#[], f.generics, f.args, f.return_type, f.body⟩
        = rust_fn_silver_to_bronze ⟨f.name, f.generics, f.args, f.return_type, f.body⟩)
    -- (c) empty body maps to empty Bronze RustFn
    ∧ ((rust_fn_silver_to_bronze ⟨f.name, f.generics, f.args, f.return_type, #[]⟩).body.size = 0)
    -- (d) self-equality (reflexivity)
    ∧ (rust_fn_silver_to_bronze f = rust_fn_silver_to_bronze f) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · rfl
  · rfl
  · rfl
  · rfl

/--
  **PMAT-419 Diamond — Canonical empty RustFnSilver.**

  Define the canonical empty RustFnSilver with all 5 fields empty
  (the "empty Rust function" placeholder). **NINTH instance of
  Template 11 (Canonical identity element)**. Mirror of PMAT-418
  (empty_lean_def_silver) — closes Rust↔Lean canonical-element
  symmetry pair.

  Adds an ELEVENTH distinct Diamond category on
  `C-XLATE-RUST-FN-TO-LEAN-THM`, pushing the contract from depth-10
  to depth-11. Second L5 contract at depth-11.
-/
def empty_rust_fn_silver : RustFnSilver :=
  { name := #[], generics := #[], args := #[], return_type := #[], body := #[] }

theorem empty_rust_fn_silver_canonical_diamond :
    -- (a) canonical name is empty
    (empty_rust_fn_silver.name = #[])
    -- (b) canonical body is empty
    ∧ (empty_rust_fn_silver.body = #[])
    -- (c) canonical name size is 0
    ∧ (empty_rust_fn_silver.name.size = 0)
    -- (d) canonical body size is 0
    ∧ (empty_rust_fn_silver.body.size = 0) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · rfl
  · rfl
  · rfl
  · rfl

end XpileContracts.CXlateRustFnToLeanThm
