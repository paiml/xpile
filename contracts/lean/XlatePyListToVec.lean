/-
  XlatePyListToVec.lean — Lean 4 refinement proofs for
  `C-XLATE-PY-LIST-TO-VEC`.

  This file is the proof-lane counterpart to
  `contracts/xlate-py-list-to-vec-v1.yaml` (PMAT-060). The YAML carries
  the *equations* describing how Python `list` lowers to Rust `Vec<T>`;
  this file carries the *theorem* that locks in the modelling
  commitment for the `iteration_order_preserved` equation.

  Cross-references:
    * Code lane:   crates/depyler-frontend/src/lib.rs
                   (currently scaffolded — list lowering arrives at
                   Layer 2 v0.2.0; this contract is the load-bearing
                   semantic anchor for that work).
    * Contract:    contracts/xlate-py-list-to-vec-v1.yaml
    * Citation:    every emitted Rust artifact for a list-shaped
                   meta-HIR input carries
                   `# xpile-contract: C-XLATE-PY-LIST-TO-VEC` above
                   its emitted Vec construction (PMAT-011 idiom).
    * Roadmap:     docs/specifications/xpile-spec.md §3 (translation
                   contracts), audit-design.md §6.

  Tier (per ruchy 5.0 §14.10.5): refinement target is Bronze at
  v0.1.0 — both the Python list and the Rust Vec are modelled as
  `Array UInt8`, and `lower_py_list_to_rust_vec` is the identity at
  the byte-array level. The theorem reduces to `rfl` by our
  modelling choice. Silver tier (v0.3.0+) refines `PyList` and
  `RustVec` to carry typed elements and an alias-graph annotation;
  the iteration-order claim then becomes a structural induction on
  list length.

  This is the *third contract Lean theorem* the project has
  (PMAT-044's Bashrs.lean was the first, PMAT-057's Notation.lean
  the second). Same scaffold posture — documentary modelling
  commitment locked in by `rfl`.
-/

namespace XpileContracts.CXlatePyListToVec

/--
  Abstract model of a Python `list` value as it lands in the
  meta-HIR Layer-1 representation. At v0.1.0 we model the
  contents as a `UInt8` array — enough to capture order and
  length, the two load-bearing properties of the
  `iteration_order_preserved` equation. Silver-tier refinement
  (XPILE-REFINE-XLATE-LIST-***+) replaces this with a typed
  `Array α` plus alias metadata.
-/
structure PyList where
  elems : Array UInt8
deriving DecidableEq

/--
  Abstract model of a Rust `Vec<T>` value as emitted by the
  Rust codegen. Same v0.1.0 shape as `PyList` — refined to carry
  Rust-side ownership semantics at Silver tier.
-/
structure RustVec where
  elems : Array UInt8
deriving DecidableEq

/--
  Lowering function: Python `list` → Rust `Vec`. v0.1.0 model:
  byte-array identity (length and order both trivially preserved
  by our representation choice).
-/
def lower_py_list_to_rust_vec (l : PyList) : RustVec :=
  { elems := l.elems }

/--
  **Refinement theorem** for `iteration_order_preserved` (the
  load-bearing claim from the equation block in the contract YAML).

  Iterating the lowered Rust Vec produces the same element sequence
  as iterating the source Python list. Proof is `rfl` by our
  modelling choice — Bronze tier per ruchy 5.0 §14.10.5.

  Documentary value: any future change to `lower_py_list_to_rust_vec`
  (e.g., adding reverse-order optimisation, or introducing a
  `SmallVec` fast path) must either preserve `rfl`-equivalence OR
  invalidate this theorem (and `refinement_proofs.rs`'s citation
  gate fires).

  Falsification: if a future PR ships a depyler-frontend whose
  list lowering reorders elements, *either* this theorem must be
  invalidated *or* the two paths stay artificially aligned and a
  runtime witness catches the divergence. Same Semantic + Runtime
  stratum cross-reinforcement as PMAT-044's bashrs theorem.

  Status: **discharged at v0.1.0 (PMAT-060)**. Tier: Bronze.
-/
theorem iteration_order_preserved (l : PyList) :
    (lower_py_list_to_rust_vec l).elems = l.elems := by
  rfl

/--
  **Length preservation** (auxiliary refinement claim, also from
  the equation block). Trivially `rfl` at v0.1.0 because we use
  the same underlying `Array UInt8` for both sides. Listed
  separately so the Silver-tier refinement (where `PyList` and
  `RustVec` get distinct element types) has a separate proof
  obligation rather than bundling order + length.
-/
theorem length_preserved (l : PyList) :
    (lower_py_list_to_rust_vec l).elems.size = l.elems.size := by
  rfl

/-! ## PMAT-164 — Silver-tier refinement for `iteration_order_preserved`
    (XPILE-REFINE-XLATE-PY-LIST-001).

    The Bronze model above uses `Array UInt8`, which captures the
    iteration-order claim at the byte level but doesn't generalise
    to element types `α`. The Silver model below introduces typed
    polymorphic `PyListSilver α` and `RustVecSilver α`, proves
    iteration order is preserved for ANY element type α, AND
    proves a stronger structural claim: every element at index `i`
    is preserved by the lowering.

    Eighth Silver refinement in the substrate (PMAT-156..162 +
    this one) — first to upgrade a multi-equation contract beyond
    its Bronze baseline. -/

/-- Silver-tier typed Python list: polymorphic over element type α
    rather than fixed at byte level. -/
structure PyListSilver (α : Type) where
  elems : List α

/-- Silver-tier typed Rust Vec: same element type as the source. -/
structure RustVecSilver (α : Type) where
  elems : List α

/-- Silver-tier lowering: polymorphic identity on the typed list. -/
def lower_py_list_to_rust_vec_silver {α : Type} (l : PyListSilver α) :
    RustVecSilver α :=
  { elems := l.elems }

/--
  **Silver-tier refinement theorem** for `iteration_order_preserved`
  (XPILE-REFINE-XLATE-PY-LIST-001 / PMAT-164).

  Iteration order is preserved for any element type `α`, not just
  bytes — the lowering is generic. This is the Bronze claim
  generalised: the Bronze theorem used `Array UInt8` (concrete);
  the Silver theorem uses `List α` (polymorphic).

  Falsification: a lowering specialised for byte-elements (e.g.,
  using SIMD intrinsics on u8 lanes) but breaking on other types
  would falsify the Silver claim while passing the Bronze one.

  Status: **discharged at v0.1.0 Silver tier (PMAT-164)** —
  eighth Silver refinement, first to upgrade a multi-equation
  contract.
-/
theorem iteration_order_preserved_silver {α : Type} (l : PyListSilver α) :
    (lower_py_list_to_rust_vec_silver l).elems = l.elems := by
  rfl

/--
  **Length preservation Silver** — companion claim, also generic
  over `α`. Bronze used `.size` on Array UInt8; Silver uses
  `.length` on `List α`.
-/
theorem length_preserved_silver {α : Type} (l : PyListSilver α) :
    (lower_py_list_to_rust_vec_silver l).elems.length = l.elems.length := by
  rfl

/-! ## PMAT-135 — Bronze-tier refinement theorems for the remaining
    4 equations of `C-XLATE-PY-LIST-TO-VEC`.

    Each theorem captures a different load-bearing modelling
    commitment of the Python-list → Rust-Vec lowering pipeline.
    All proofs are `rfl` (or near-`rfl`) at v0.1.0; Silver tier
    replaces the abstract structures with typed AST nodes carrying
    real element types, type-inference results, and the alias
    graph annotation. -/

/-- Tag for one of the canonical homogeneous element types
    {int, float, str, bool, bytes} that the Python frontend can
    infer for a list. Silver-tier refinement replaces this with
    the typed AST node's `element_type` field. -/
inductive PyElementType
  | int_type
  | float_type
  | str_type
  | bool_type
  | bytes_type
deriving DecidableEq

/-- Abstract homogeneous Python list with a tagged element type. -/
structure HomogeneousList where
  elems : Array UInt8
  element_type : PyElementType
deriving DecidableEq

/-- Abstract Rust Vec with a tagged element type. The lowering
    MUST preserve both the element bytes AND the tag (no implicit
    coercion at element boundaries). -/
structure TypedRustVec where
  elems : Array UInt8
  element_type : PyElementType
deriving DecidableEq

/-- Bronze-tier lowering: tag-preserving identity on the byte
    contents. -/
def lower_homogeneous_list (l : HomogeneousList) : TypedRustVec :=
  { elems := l.elems, element_type := l.element_type }

/--
  **Refinement theorem** for `homogeneous_list_to_vec`.

  When a list is inferred as homogeneous with element type T, the
  emitted Vec carries the same element bytes AND the same element-
  type tag. Falsified by any lowering that silently coerces (e.g.,
  promoting an int-tagged list to a float-tagged Vec on the
  presence of a single 1.0-valued element) — such a change would
  break the "no implicit type coercion at element boundaries"
  invariant declared in the contract.
-/
theorem homogeneous_list_to_vec (l : HomogeneousList) :
    (lower_homogeneous_list l).elems = l.elems ∧
      (lower_homogeneous_list l).element_type = l.element_type := by
  exact ⟨rfl, rfl⟩

/-- Abstract heterogeneous-list lowering result: either a
    successful Vec (forbidden by the contract on heterogeneous
    input) or an explicit error carrying the set of conflicting
    types. -/
inductive HeteroResult
  | ok (vec : TypedRustVec)
  | error (found_types : List PyElementType)
deriving DecidableEq

/-- Abstract heterogeneous source: a list whose elements span
    ≥2 distinct element types. The `found_types` list MUST be
    non-empty (and in fact have at least 2 entries — Bronze tier
    accepts the list itself as the witness without a separate
    `nodup` proof). -/
structure HeterogeneousList where
  found_types : List PyElementType
deriving DecidableEq

/-- Bronze-tier lowering: heterogeneous input always produces an
    error result; the `found_types` list is preserved byte-for-byte
    so the user can repair the source. -/
def lower_heterogeneous_list (l : HeterogeneousList) : HeteroResult :=
  HeteroResult.error l.found_types

/--
  **Refinement theorem** for `heterogeneous_list_rejected`.

  The lowering of a heterogeneous list NEVER produces an `ok`
  result — it always errors with the full found-types list
  preserved. Falsified by any lowering that silently boxes into
  `Vec<Box<dyn Any>>` (which would defeat the type contract) or
  that drops some of the conflicting types from the error report
  (which would leave the user unable to repair the source).

  The proof is by case analysis on the result: the `error` arm
  carries the contract's load-bearing claim (rfl-preserved
  type list); the `ok` arm is impossible by construction.
-/
theorem heterogeneous_list_rejected (l : HeterogeneousList) :
    match lower_heterogeneous_list l with
    | HeteroResult.error found => found = l.found_types
    | HeteroResult.ok _ => False := by
  rfl

/-- Abstract alias-graph annotation: a pair (binder_index,
    observer_index) where mutation crosses the boundary. Bronze
    tier captures the load-bearing fact that ≥1 such pair exists. -/
structure AliasGraph where
  has_observable_alias : Bool
deriving DecidableEq

/-- Abstract Rust output annotated with what reference-semantics
    treatment the emitter applied: explicit `.clone()` insertion,
    `Rc<RefCell<...>>` wrap, or none (the last is a falsification
    target — emitting move-semantics where Python uses reference-
    semantics). -/
inductive AliasTreatment
  | clone_inserted
  | rc_refcell_wrap
  | none_emitted
deriving DecidableEq

/-- Bronze-tier lowering: if the alias graph flagged an observable
    alias, emit `.clone()` (the simpler of the two valid options);
    otherwise emit no special treatment. -/
def lower_alias_observation (a : AliasGraph) : AliasTreatment :=
  if a.has_observable_alias then
    AliasTreatment.clone_inserted
  else
    AliasTreatment.none_emitted

/--
  **Refinement theorem** for `alias_observation_inserts_clone`.

  If the alias graph carries an observable-alias annotation, the
  emitted Rust uses reference-semantics (clone-insertion at this
  tier; Silver tier extends to Rc<RefCell> when explicit
  shared-mutable semantics are needed). Crucially the result is
  NEVER `none_emitted` when the alias flag is set — that case is
  excluded by construction so move-semantics never silently
  replace reference-semantics.
-/
theorem alias_observation_inserts_clone (a : AliasGraph) :
    a.has_observable_alias = true →
      lower_alias_observation a ≠ AliasTreatment.none_emitted := by
  intro h
  unfold lower_alias_observation
  rw [h]
  intro h'
  cases h'

/-- Abstract output of the `len()` lowering: a `usize` value AND
    the flag indicating whether the consumer expects an i64
    (Python-int-compatible) value. Bronze tier models the cast
    decision as a bool. -/
structure LenMethodOutput where
  /-- The usize result of `rust_vec.len()`. -/
  raw_usize_len : Nat
  /-- True iff the consumer expected i64 and the codegen
      inserted an explicit cast. False when the consumer was
      happy with usize (no cast needed). -/
  i64_cast_inserted : Bool
deriving DecidableEq

/-- Bronze-tier lowering: usize length is the array size; the cast
    flag is set exactly when the consumer expects i64. -/
def lower_length_method (vec_len : Nat) (consumer_expects_i64 : Bool) : LenMethodOutput :=
  { raw_usize_len := vec_len, i64_cast_inserted := consumer_expects_i64 }

/--
  **Refinement theorem** for `length_method`.

  Two load-bearing claims in one theorem:
  1. The emitted usize result equals the source `vec.len()` call
     byte-identically (no off-by-one, no signed-int confusion).
  2. The explicit-cast flag follows the consumer-expectation
     decision exactly — never a silent `usize → i64` truncation,
     never a missing cast where one is needed.

  Falsified by an emitter that drops the cast for "performance"
  reasons (which would silently truncate on platforms where
  `usize > i64`) or that always inserts the cast (which would
  introduce a useless runtime check on the usize → usize path).
-/
theorem length_method (vec_len : Nat) (consumer_expects_i64 : Bool) :
    (lower_length_method vec_len consumer_expects_i64).raw_usize_len = vec_len ∧
      (lower_length_method vec_len consumer_expects_i64).i64_cast_inserted = consumer_expects_i64 := by
  exact ⟨rfl, rfl⟩

end XpileContracts.CXlatePyListToVec
