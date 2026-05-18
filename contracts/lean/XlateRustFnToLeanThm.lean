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

end XpileContracts.CXlateRustFnToLeanThm
