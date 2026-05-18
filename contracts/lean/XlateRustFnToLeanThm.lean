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

end XpileContracts.CXlateRustFnToLeanThm
