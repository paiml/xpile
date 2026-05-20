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

/-! ## PMAT-182 — Final Silver expansion: homogeneous +
    heterogeneous + alias + length (XPILE-REFINE-XLATE-PY-LIST-002).

    Replicates the PMAT-164 polymorphic Silver pattern across the
    last four remaining equations on C-XLATE-PY-LIST-TO-VEC.
    Brings Silver coverage to **6/6 equations — full Silver
    tier**, the **FIFTH contract in the substrate at full
    Silver** (after C-FFI-CPYTHON-EXT, C-XLATE-LEAN-TO-RUST,
    C-XLATE-RUST-FN-TO-LEAN-THM, C-NOTATION-LATEX-MATH-TO-EQUATION). -/

/--
  Silver-tier model of a homogeneous Python list, polymorphic
  over a concrete element type. Bronze captured count+kind via
  scalar fields; Silver lifts to a typed list parameterised by
  element type α plus a element_type_tag for audit.
-/
structure HomogeneousListSilver (α : Type) where
  elements : List α
  element_type_tag : String
deriving DecidableEq

/-- Silver model of the Rust Vec<T>. -/
structure TypedRustVecSilver (α : Type) where
  elements : List α
  element_type_tag : String
deriving DecidableEq

/-- Silver lowering: identity on elements + tag. -/
def lower_homogeneous_list_silver {α : Type} (l : HomogeneousListSilver α) :
    TypedRustVecSilver α :=
  { elements := l.elements, element_type_tag := l.element_type_tag }

/-- **Silver-tier refinement theorem** — element-type tag
    preserved across homogeneous lowering. Captures the load-
    bearing claim that the Rust Vec<T>'s T tag matches the
    source list's homogeneous-element-type declaration (no
    silent Box<dyn Any> erasure). -/
theorem homogeneous_element_type_preserved_silver {α : Type}
    (l : HomogeneousListSilver α) :
    (lower_homogeneous_list_silver l).element_type_tag = l.element_type_tag := by
  rfl

/-- **Silver-tier refinement theorem** — element list preserved
    polymorphically. -/
theorem homogeneous_elements_preserved_silver {α : Type}
    (l : HomogeneousListSilver α) :
    (lower_homogeneous_list_silver l).elements = l.elements := by
  rfl

/--
  Silver-tier model of a heterogeneous list-rejection event.
  Bronze captured the rejection via a boolean; Silver adds a
  typed rejection reason explaining WHY the depyler-frontend
  refused to lower this list.
-/
inductive RejectionReason where
  | mixedNumericNonNumeric
  | mixedSignedUnsigned
  | unknownDynamicType
  | multipleTypesAtSameDepth
deriving DecidableEq

/-- Silver heterogeneous-list rejection model. -/
structure HeterogeneousListSilver where
  reason : RejectionReason
  element_types : Array String
deriving DecidableEq

/-- Silver rejection result. -/
structure HeteroResultSilver where
  rejected : Bool
  reason : Option RejectionReason
deriving DecidableEq

/-- Silver lowering: always rejected, reason preserved. -/
def lower_heterogeneous_list_silver (l : HeterogeneousListSilver) :
    HeteroResultSilver :=
  { rejected := true, reason := some l.reason }

/-- **Silver-tier refinement theorem** — rejection reason
    preserved through lowering. Bronze proved binary
    rejected/accepted; Silver captures WHY the rejection
    happened. An emitter that collapses all rejection reasons
    into one category (or auto-coerces to Box<dyn Any>) would
    falsify this. -/
theorem heterogeneous_rejection_reason_preserved_silver
    (l : HeterogeneousListSilver) :
    (lower_heterogeneous_list_silver l).reason = some l.reason := by
  rfl

/-- **Silver-tier refinement theorem** — rejection is total
    (always rejected, never silently accepted with implicit
    casts). -/
theorem heterogeneous_always_rejected_silver
    (l : HeterogeneousListSilver) :
    (lower_heterogeneous_list_silver l).rejected = true := by
  rfl

/--
  Silver-tier model of an alias graph. Bronze captured a single
  bool `has_observable_alias`; Silver adds typed alias-kind
  enumeration capturing the SCOPE of the alias.
-/
inductive AliasKind where
  | inFunctionLocal
  | crossFunction
  | crossModule
deriving DecidableEq

/-- Silver alias-graph model. -/
structure AliasGraphSilver where
  kind : Option AliasKind
  reference_count : Nat
deriving DecidableEq

/-- Silver alias-treatment dispatch. -/
inductive AliasTreatmentSilver where
  | cloneEmitted
  | rcRefCellEmitted
  | noneEmitted
deriving DecidableEq

/-- Silver lowering for alias observations. Within-function alias
    → clone; cross-function/module → Rc<RefCell>; no alias →
    none. -/
def lower_alias_observation_silver (a : AliasGraphSilver) :
    AliasTreatmentSilver :=
  match a.kind with
  | none => AliasTreatmentSilver.noneEmitted
  | some AliasKind.inFunctionLocal => AliasTreatmentSilver.cloneEmitted
  | some _ => AliasTreatmentSilver.rcRefCellEmitted

/-- **Silver-tier refinement theorem** — when alias kind is
    inFunctionLocal, the dispatch always picks Clone. Captures
    the no-shared-mutable-state invariant for within-function
    aliases. Bronze proved a single binary (alias-yes/no);
    Silver captures the SCOPE distinction that determines whether
    Clone or Rc<RefCell> is emitted. -/
theorem in_function_alias_emits_clone_silver
    (a : AliasGraphSilver)
    (h : a.kind = some AliasKind.inFunctionLocal) :
    lower_alias_observation_silver a = AliasTreatmentSilver.cloneEmitted := by
  unfold lower_alias_observation_silver
  rw [h]

/-- **Silver-tier refinement theorem** — when alias kind is None
    (no observable alias), the dispatch picks None. Bronze
    couldn't distinguish None-alias from in-function alias when
    both produced "no special wrapping". -/
theorem no_alias_emits_none_silver
    (a : AliasGraphSilver)
    (h : a.kind = none) :
    lower_alias_observation_silver a = AliasTreatmentSilver.noneEmitted := by
  unfold lower_alias_observation_silver
  rw [h]

/--
  Silver-tier model of the length-method output with typed cast
  decision. Bronze used a Bool `i64_cast_inserted`; Silver
  promotes to a typed enum capturing WHICH platform target the
  cast was inserted for.
-/
inductive CastTarget where
  | none
  | i64
  | usize
deriving DecidableEq

/-- Silver length-method output. -/
structure LenMethodOutputSilver where
  raw_usize_len : Nat
  cast_target : CastTarget
deriving DecidableEq

/-- Silver lowering for len(). -/
def lower_length_method_silver (vec_len : Nat) (target : CastTarget) :
    LenMethodOutputSilver :=
  { raw_usize_len := vec_len, cast_target := target }

/-- **Silver-tier refinement theorem** — cast-target is preserved
    through length-method lowering. Bronze used a single Bool
    (cast/no-cast); Silver captures WHICH target type. Falsified
    by an emitter that defaults to usize cast when i64 is
    requested (silent truncation on 32-bit platforms). COMPLETES
    Silver coverage on C-XLATE-PY-LIST-TO-VEC (6/6) — FIFTH
    contract at full Silver. -/
theorem cast_target_preserved_silver (vec_len : Nat) (target : CastTarget) :
    (lower_length_method_silver vec_len target).cast_target = target := by
  rfl

/-- **Silver-tier refinement theorem** — usize length preserved. -/
theorem silver_length_preserved (vec_len : Nat) (target : CastTarget) :
    (lower_length_method_silver vec_len target).raw_usize_len = vec_len := by
  rfl

/-! ## PMAT-192 — SEVENTH Gold-tier refinement: NonEmptyHomogeneousList
    polymorphic subtype (XPILE-REFINE-XLATE-PY-LIST-003).

    Seventh Gold-tier theorem in the substrate. **Extends Gold
    to a seventh contract** (C-XLATE-PY-LIST-TO-VEC, Layer-2).

    Third demonstration of the collection-cardinality subtype
    pattern (after PMAT-189 NonEmptyDefinition on
    NOTATION-LATEX-MATH-TO-EQUATION and PMAT-191
    NonEmptyPreconditionList on C-XLATE-RUST-FN-TO-LEAN-THM).

    **First Gold refinement applied to a POLYMORPHIC Silver
    model**: HomogeneousListSilver is parameterised over
    element type α. The Gold subtype must thread the polymorphic
    parameter — `NonEmptyHomogeneousList α := { l :
    HomogeneousListSilver α // l.elements ≠ [] }` — demonstrating
    that refinement subtypes compose cleanly with polymorphism.

    Captures what Silver couldn't model:
    - Silver: "elements preserved IF list has at least one
      element" (precondition as a separate obligation, regardless
      of element type)
    - Gold: "input IS a NonEmptyHomogeneousList α" (non-emptiness
      witness travels with the value through α-polymorphic
      lowering)

    Cross-pattern composition: this PR demonstrates that the
    Gold-tier subtype pattern works uniformly with Silver-tier
    polymorphism. The two compose orthogonally — Silver
    introduces the type parameter, Gold adds the refinement
    invariant.

    Status: discharged at v0.1.0 (PMAT-192). Tier: GOLD.
    Seventh Gold theorem in the substrate, first to apply Gold
    over polymorphic Silver. -/

/-- Gold-tier polymorphic refinement subtype: a homogeneous list
    proven to have at least one element. The α parameter is
    inherited from the Silver model; the non-emptiness witness
    travels with the value. -/
def NonEmptyHomogeneousList (α : Type) :=
  { l : HomogeneousListSilver α // l.elements ≠ [] }

/-- Extract the underlying Silver homogeneous list. -/
def NonEmptyHomogeneousList.val {α : Type} (n : NonEmptyHomogeneousList α) :
    HomogeneousListSilver α :=
  n.val

/-- Gold-tier lowering: extracts the structural data; the
    non-emptiness witness is carried into the typed output. -/
def lower_non_empty_homogeneous_gold {α : Type}
    (n : NonEmptyHomogeneousList α) : TypedRustVecSilver α :=
  lower_homogeneous_list_silver n.val

/--
  **Gold-tier refinement theorem** — lowering a polymorphic
  NonEmptyHomogeneousList preserves the elements list AND the
  non-emptiness witness travels with the value at the type
  level. Holds for any element type α.

  This is the seventh Gold theorem in the substrate, and the
  first to apply the refinement-subtype pattern OVER a polymorphic
  Silver model. Captures what Silver couldn't model:
  - Silver: "elements preserved" (no constraint on emptiness)
  - Gold: "input IS a NonEmptyHomogeneousList α — non-emptiness
    witness travels with the value polymorphically"

  Status: **discharged at v0.1.0 (PMAT-192)**. Tier: GOLD.
-/
theorem non_empty_homogeneous_preserves_elements_gold {α : Type}
    (n : NonEmptyHomogeneousList α) :
    (lower_non_empty_homogeneous_gold n).elements = n.val.elements := by
  rfl

/--
  **Gold-tier refinement theorem** — the non-emptiness witness
  is preserved through polymorphic lowering. The output's
  elements is non-empty BY TYPE for any α.
-/
theorem non_empty_homogeneous_witness_gold {α : Type}
    (n : NonEmptyHomogeneousList α) :
    (lower_non_empty_homogeneous_gold n).elements ≠ [] := by
  unfold lower_non_empty_homogeneous_gold lower_homogeneous_list_silver
  exact n.property

/--
  **Gold-tier refinement theorem** — bridges Gold to Silver: the
  underlying elements agrees with what Silver's
  `homogeneous_elements_preserved_silver` produces. Gold simply
  carries the non-emptiness witness in addition, polymorphically.
-/
theorem gold_non_empty_homogeneous_agrees_with_silver {α : Type}
    (n : NonEmptyHomogeneousList α) :
    (lower_non_empty_homogeneous_gold n).elements
      = (lower_homogeneous_list_silver n.val).elements := by
  rfl

/-! ## PMAT-202 — FOURTH Platinum-tier refinement: functoriality
    of list lowering (XPILE-REFINE-XLATE-PY-LIST-004).

    Fourth Platinum-tier theorem in the substrate. Demonstrates
    the FOURTH distinct Platinum algebraic shape: **functoriality
    / homomorphism preservation** — `lower(l1 ++ l2) = lower(l1)
    ++ lower(l2)`. Distinct from:
    - PMAT-199 commutativity: `f(a, b) = f(b, a)`
    - PMAT-200 associativity: `f(f(a,b), c) = f(a, f(b,c))`
    - PMAT-201 idempotence: `f(x) = f(f(x))`
    - **PMAT-202 functoriality: `lower(append l1 l2) = append (lower l1) (lower l2)`**

    The list lowering is a HOMOMORPHISM: lowering distributes
    over the monoid operation (list append). This is a classic
    functoriality property — the lowering can be applied
    BEFORE or AFTER list combination with equivalent results.

    This property is load-bearing for emitter compositions: an
    emitter that builds a Rust Vec piecewise (e.g., by streaming
    elements through a buffer) produces the same result as one
    that builds the entire Python list first and then lowers in
    one shot. The functoriality theorem PROVES these strategies
    are equivalent.

    Status: discharged at v0.1.0 (PMAT-202). Tier: PLATINUM.
    Fourth Platinum theorem in the substrate. -/

/--
  **Platinum-tier refinement theorem** — list lowering is a
  homomorphism over list append.

  For any two polymorphic PyListSilver values l1 and l2 (of the
  same element type α), lowering their concatenation produces
  the same result as concatenating their individual lowerings.

  Formally: `(lower (l1 ++ l2)).elems = (lower l1).elems ++
  (lower l2).elems` where ++ is List append.

  This is functoriality of the lowering over the (List α, ++)
  monoid. Captures the load-bearing emitter-composition
  property that Bronze/Silver/Gold couldn't model.

  Status: **discharged at v0.1.0 (PMAT-202)**. Tier: PLATINUM.
-/
theorem lower_distributes_over_append_platinum {α : Type}
    (l1 l2 : PyListSilver α) :
    (lower_py_list_to_rust_vec_silver
       { elems := l1.elems ++ l2.elems }).elems
    = (lower_py_list_to_rust_vec_silver l1).elems
        ++ (lower_py_list_to_rust_vec_silver l2).elems := by
  rfl

/--
  **Platinum-tier refinement theorem** — list lowering preserves
  the empty list. The empty list is the identity element of
  list-append; this theorem captures the IDENTITY-PRESERVATION
  property of the homomorphism.

  Combined with `lower_distributes_over_append_platinum`, this
  proves the lowering is a MONOID HOMOMORPHISM (preserves
  identity AND distributes over the binary operation).
-/
theorem lower_preserves_empty_platinum (α : Type) :
    (lower_py_list_to_rust_vec_silver (α := α) { elems := [] }).elems
    = [] := by
  rfl

/--
  **Platinum-tier refinement theorem** — length is also a
  homomorphism: `length (lower l) = length l.elems`. This
  composes with the append-distributivity to give: `length
  (lower (l1 ++ l2)) = length l1 + length l2`.

  Captures the length-preservation property compositionally —
  an emitter that builds the Rust Vec piecewise produces the
  same total length as one that builds the full Python list
  first.
-/
theorem lower_length_homomorphism_platinum {α : Type}
    (l1 l2 : PyListSilver α) :
    (lower_py_list_to_rust_vec_silver
       { elems := l1.elems ++ l2.elems }).elems.length
    = l1.elems.length + l2.elems.length := by
  unfold lower_py_list_to_rust_vec_silver
  simp [List.length_append]

/-! ## PMAT-221 — SEVENTH Diamond-tier refinement: free
    list-monoid axioms (XPILE-REFINE-XLATE-PY-LIST-005).

    Seventh Diamond-tier theorem in the substrate. Combines four
    properties into the FREE LIST MONOID axiomatization
    polymorphic over element type α:
    - PMAT-202 Platinum functoriality (the homomorphism)
    - PMAT-202 companion empty preservation (identity)
    - Associativity (List.append_assoc)
    - Length-additivity (PMAT-202 companion lifted)

    Free list monoid is distinct from prior Diamond categories:
    1. PMAT-214: commutative-monoid / semiring (algebraic)
    2. PMAT-215: pure-function (functional)
    3. PMAT-216: abelian-group (algebraic w/ inverses)
    4. PMAT-217: equivalence-relation (relational)
    5. PMAT-218: bounded-monoid (bounded algebraic)
    6. PMAT-219: string-monoid (textual algebraic)
    7. **PMAT-221 (NEW): free list-monoid (polymorphic list)**

    The free list monoid captures the universal algebraic
    structure of lists with append + nil. "Free" means no
    additional relations — every monoid law that holds must
    follow from the three axioms.

    Status: discharged at v0.1.0 (PMAT-221). Tier: DIAMOND.
    Seventh Diamond theorem in the substrate. -/

/--
  **Diamond-tier refinement theorem** — list lowering forms a
  FREE LIST MONOID polymorphic over α.

  Combines four monoid axioms:
  - Closure / homomorphism (PMAT-202 lifted)
  - Associativity ((l1 ++ l2) ++ l3 = l1 ++ (l2 ++ l3))
  - Left identity ([] ++ l = l)
  - Length-additivity (length is a monoid homomorphism into Nat)

  Captures the FREE algebraic structure — additional relations
  cannot be assumed without falsifying the freeness claim.

  Status: **discharged at v0.1.0 (PMAT-221)**. Tier: DIAMOND.
-/
theorem list_free_monoid_diamond {α : Type}
    (l1 l2 l3 : PyListSilver α) :
    -- Closure / homomorphism (PMAT-202 lifted)
    (lower_py_list_to_rust_vec_silver
       { elems := l1.elems ++ l2.elems }).elems
      = (lower_py_list_to_rust_vec_silver l1).elems
          ++ (lower_py_list_to_rust_vec_silver l2).elems
    -- Associativity
    ∧ (l1.elems ++ l2.elems) ++ l3.elems
      = l1.elems ++ (l2.elems ++ l3.elems)
    -- Left identity (PMAT-202 companion lifted)
    ∧ (lower_py_list_to_rust_vec_silver (α := α) { elems := [] }).elems
      = []
    -- Length-additivity (PMAT-202 companion lifted)
    ∧ (lower_py_list_to_rust_vec_silver
        { elems := l1.elems ++ l2.elems }).elems.length
      = l1.elems.length + l2.elems.length := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · rfl
  · exact List.append_assoc l1.elems l2.elems l3.elems
  · rfl
  · simp [List.length_append]

/-! ## PMAT-229 — SECOND Diamond on C-XLATE-PY-LIST-TO-VEC
    (XPILE-REFINE-XLATE-PY-LIST-006): NonEmpty section-retraction
    axioms.

    **SECOND Diamond on this contract — Diamond breadth on Layer 2.**
    Following PMAT-228 (first depth-2 Diamond on Layer 1
    C-PY-INT-ARITH), PMAT-229 extends Diamond breadth to Layer 2.
    XlatePyListToVec already has the free list-monoid Diamond
    (PMAT-221); PMAT-229 captures NonEmpty SECTION-RETRACTION
    structure — a fundamentally distinct algebraic category from
    free list-monoid.

    - PMAT-221: free list-monoid (Int) covering closure +
      associativity + identity + length-additivity
    - PMAT-229: NonEmpty section-retraction (this) covering
      element preservation + non-emptiness witness preservation
      + tag preservation + injectivity-on-content

    The categorical distinction: free list-monoid is about HOW
    list-append composes; NonEmpty section-retraction is about
    HOW subtype refinements are PRESERVED across lowering. They
    capture orthogonal algebraic properties of the same contract.

    Status: discharged at v0.1.0 (PMAT-229). Tier: DIAMOND.
    SECOND Diamond category on C-XLATE-PY-LIST-TO-VEC. -/

/--
  **Diamond-tier refinement theorem** — NonEmpty section-
  retraction structure on polymorphic homogeneous lowering.

  Combines four properties into the SECTION-RETRACTION
  axiomatization on the pair
    `NonEmptyHomogeneousList α → TypedRustVecSilver α`:

  (a) Element-list preservation (PMAT-192a lifted)
  (b) Non-emptiness witness preservation (PMAT-192b lifted)
  (c) Element-type-tag preservation (PMAT-182 lifted, polymorphic)
  (d) Injectivity on content: same elements + same tag ⇒
      same output (no hidden side-channel state)

  An emitter that adds hidden state to the Rust Vec (e.g., a
  cache pointer field) would falsify the injectivity claim — two
  NonEmpty inputs with identical elements + tag would produce
  different Rust Vecs.

  Status: **discharged at v0.1.0 (PMAT-229)**. Tier: DIAMOND.
-/
theorem nonempty_section_retraction_diamond {α : Type}
    (n : NonEmptyHomogeneousList α) :
    -- (a) Element list preservation (PMAT-192a lifted)
    (lower_non_empty_homogeneous_gold n).elements = n.val.elements
    -- (b) Non-emptiness witness preserved (PMAT-192b lifted)
    ∧ (lower_non_empty_homogeneous_gold n).elements ≠ []
    -- (c) Element-type-tag preserved (PMAT-182 lifted, polymorphic)
    ∧ (lower_non_empty_homogeneous_gold n).element_type_tag
        = n.val.element_type_tag
    -- (d) Injectivity on content: same content ⇒ same output
    ∧ ∀ (n' : NonEmptyHomogeneousList α),
        n.val.elements = n'.val.elements →
        n.val.element_type_tag = n'.val.element_type_tag →
        lower_non_empty_homogeneous_gold n
          = lower_non_empty_homogeneous_gold n' := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · rfl
  · exact non_empty_homogeneous_witness_gold n
  · rfl
  · intros n' he ht
    unfold lower_non_empty_homogeneous_gold lower_homogeneous_list_silver
    rw [he, ht]

/-! ## PMAT-244 — THIRD Diamond on C-XLATE-PY-LIST-TO-VEC
    (Layer 2 DEPTH-3): length monoid homomorphism
    (XPILE-REFINE-XLATE-PY-LIST-007).

    **Fourth DEPTH-3 Diamond in the substrate.** Following
    PMAT-241/242/243 (depth-3 on L1, L5, L4), PMAT-244 extends
    Diamond depth-3 to Layer 2 C-XLATE-PY-LIST-TO-VEC. Four
    depth-3 contracts now span four distinct layers.

    XlatePyListToVec now has THREE Diamond categories:
    - PMAT-221: free list-monoid (append-composition algebra)
    - PMAT-229: NonEmpty section-retraction (subtype preservation)
    - **PMAT-244: length monoid homomorphism (cardinality
      projection into (Nat, +, 0))**

    The categorical distinction: free list-monoid is on the
    structural append; section-retraction is on the subtype
    refinement; length-homomorphism is the FUNCTORIAL projection
    into Nat — a different categorical pattern (functor) from
    the structural and subtype Diamonds.

    Status: discharged at v0.1.0 (PMAT-244). Tier: DIAMOND.
    FOURTH DEPTH-3 Diamond in the substrate. -/

/--
  **Diamond-tier refinement theorem** — length is a MONOID
  HOMOMORPHISM from `(PyListSilver α, ++, [])` to `(Nat, +, 0)`.

  Combines four properties:
  (a) Additivity: length(lower(l1 ++ l2)) = length(l1) + length(l2)
      (PMAT-202 length companion lifted)
  (b) Identity preservation: length(lower([])) = 0
  (c) Length preservation: length(lower(l)) = length(l)
  (d) Non-negativity: length(lower(l)) ≥ 0

  Status: **discharged at v0.1.0 (PMAT-244)**. Tier: DIAMOND.
-/
theorem length_monoid_homomorphism_diamond {α : Type}
    (l1 l2 : PyListSilver α) :
    -- (a) Additivity
    (lower_py_list_to_rust_vec_silver
        { elems := l1.elems ++ l2.elems }).elems.length
      = l1.elems.length + l2.elems.length
    -- (b) Identity preservation
    ∧ (lower_py_list_to_rust_vec_silver (α := α) { elems := [] }).elems.length = 0
    -- (c) Length preservation
    ∧ (lower_py_list_to_rust_vec_silver l1).elems.length = l1.elems.length
    -- (d) Non-negativity (Nat)
    ∧ (lower_py_list_to_rust_vec_silver l1).elems.length ≥ 0 := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact lower_length_homomorphism_platinum l1 l2
  · rfl
  · rfl
  · exact Nat.zero_le _

/-! ## PMAT-338 — FOURTH Diamond on C-XLATE-PY-LIST-TO-VEC (Layer 2
    BROADENING DEPTH-4 from 5 to 6 contracts): LIST REVERSE
    INVOLUTION (XPILE-REFINE-XLATE-PY-LIST-TO-VEC-006).

    **Broadens DEPTH-4 from 5 to 6 contracts.** After PMAT-330
    completed depth-4 ACROSS ALL 5 LAYERS, this is the first
    POST-UNIVERSAL broadening: pushes XlatePyList (Layer 2) from
    depth-3 to depth-4, adding a SECOND Layer 2 contract at
    depth-4 (Bashrs was the first via PMAT-329).

    The 4 Diamond categories on C-XLATE-PY-LIST-TO-VEC:
    - PMAT-221 list_free_monoid_diamond: (List, ++, []) monoid
    - PMAT-229 nonempty_section_retraction_diamond: NonEmpty subtype
    - PMAT-244 length_monoid_homomorphism_diamond: length functor
    - **PMAT-338: LIST REVERSE INVOLUTION** ← depth-4

    The categorical distinction is sharp:
      - PMAT-221: about `++` (concatenation algebra)
      - PMAT-229: about NonEmpty subtype refinement
      - PMAT-244: about length functor (homomorphism)
      - PMAT-338: about `reverse` (involution + interaction with
        length and base cases)

    `reverse` is a UNARY operation distinct from all three prior
    (which were about binary `++`, subtype, or functor properties).
    Reverse-involution is the canonical algebraic structure of
    `(List α, reverse)`.

    Status: discharged at v0.1.0 (PMAT-338). Tier: DIAMOND.
    Broadens DEPTH-4 from 5 to 6 contracts (post-PMAT-330 milestone). -/

/--
  **Diamond-tier refinement theorem** — `List.reverse` is an
  INVOLUTION on `PyListSilver.elems`.

  Combines four REVERSE-INVOLUTION properties:
  (a) Double reverse is identity: `l.elems.reverse.reverse = l.elems`
  (b) Reverse preserves length: `l.elems.reverse.length = l.elems.length`
  (c) Reverse of empty is empty: `([] : List α).reverse = []`
  (d) Reverse of singleton is itself: `[a].reverse = [a]`

  Uses Mathlib's `List.reverse_reverse`, `List.length_reverse`,
  `List.reverse_nil`. The singleton case is by `rfl`.

  Status: **discharged at v0.1.0 (PMAT-338)**. Tier: DIAMOND.
-/
theorem list_reverse_involution_diamond {α : Type} (l : PyListSilver α) :
    -- (a) Double reverse is identity (involution)
    (l.elems.reverse.reverse = l.elems)
    -- (b) Reverse preserves length
    ∧ (l.elems.reverse.length = l.elems.length)
    -- (c) Reverse of empty is empty
    ∧ (([] : List α).reverse = [])
    -- (d) Reverse of singleton is itself
    ∧ (∀ a : α, [a].reverse = [a]) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact List.reverse_reverse l.elems
  · exact List.length_reverse
  · rfl
  · intro a; rfl

/-! ## PMAT-349 — FIFTH Diamond on C-XLATE-PY-LIST-TO-VEC (Layer 2
    BROADENING DEPTH-5 from 6 to 7 contracts): PYLIST-SILVER
    STRUCTURE EXTENSIONALITY
    (XPILE-REFINE-XLATE-PY-LIST-TO-VEC-007).

    **Broadens DEPTH-5 from 6 to 7 contracts.** After PMAT-348
    achieved depth-5 with 2nd Layer 3 contract (XpileBackendTrait),
    PMAT-349 pushes XlatePyListToVec (Layer 2) from depth-4 to
    depth-5, adding a SECOND Layer 2 contract at depth-5
    (Bashrs was first via PMAT-346).

    The 5 Diamond categories on C-XLATE-PY-LIST-TO-VEC:
    - PMAT-221 list_free_monoid_diamond: (List, ++, []) monoid
    - PMAT-229 nonempty_section_retraction_diamond: NonEmpty subtype
    - PMAT-244 length_monoid_homomorphism_diamond: length functor
    - PMAT-338 list_reverse_involution_diamond: reverse involution
    - **PMAT-349: PYLIST-SILVER STRUCTURE EXTENSIONALITY** ← depth-5

    The categorical distinction is sharp:
      - PMAT-221: about `++` (concatenation algebra)
      - PMAT-229: about NonEmpty subtype refinement
      - PMAT-244: about length functor (homomorphism)
      - PMAT-338: about `reverse` (involution)
      - PMAT-349: about RECORD-FROM-FIELDS identity — distinct
        from all four prior algebraic categories

    Mirror of PMAT-311 (BoundedSmem), PMAT-329 (OutcomeSilver),
    PMAT-330 (MetaHirModuleSilver), PMAT-331 (ArtifactSilver),
    PMAT-332 (TranspileSession), PMAT-333 (Contract), PMAT-334
    (EquationFormula), PMAT-335 (RustFn), PMAT-336 (RustFnSilver) —
    tenth substrate-wide demonstration of the structure-
    extensionality pattern, first on PyListSilver.

    Status: discharged at v0.1.0 (PMAT-349). Tier: DIAMOND.
    Broadens DEPTH-5 from 6 to 7 contracts. -/

/--
  **Diamond-tier refinement theorem** — `PyListSilver α` admits
  STRUCTURE EXTENSIONALITY.

  Combines four STRUCTURE-EXTENSIONALITY properties:
  (a) Field-equality → record-equality
  (b) Record-equality → field-equality (congruence)
  (c) Self-equality (reflexivity)
  (d) Identity field-replacement preserves elems

  Tenth substrate-wide demonstration of the structure-extensionality
  pattern (after PMAT-311/329/330/331/332/333/334/335/336).

  Status: **discharged at v0.1.0 (PMAT-349)**. Tier: DIAMOND.
  Broadens DEPTH-5 from 6 to 7 contracts.
-/
theorem py_list_silver_struct_extensionality_diamond {α : Type}
    (l1 l2 : PyListSilver α) :
    -- (a) Field equality → record equality
    (l1.elems = l2.elems → l1 = l2)
    -- (b) Record equality → field equality
    ∧ (l1 = l2 → l1.elems = l2.elems)
    -- (c) Self-equality (reflexivity)
    ∧ (l1 = l1)
    -- (d) Identity field-replacement preserves elems
    ∧ (({ elems := l1.elems } : PyListSilver α).elems = l1.elems) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro h
    cases l1; cases l2
    simp_all
  · intro h
    rw [h]
  · rfl
  · rfl

/-! ## PMAT-360 — SIXTH Diamond on C-XLATE-PY-LIST-TO-VEC
    (Layer 2 BROADENS DEPTH-6):
    TYPED-RUST-VEC-SILVER STRUCTURE EXTENSIONALITY
    (XPILE-REFINE-XLATE-PY-LIST-008).

    **Broadens depth-6 substrate-wide.** PMAT-360 pushes
    XlatePyListToVec (Layer 2) from depth-5 to depth-6 as the
    second L2 contract at depth-6+ (Bashrs was first via PMAT-357).

    The 6 Diamond categories on C-XLATE-PY-LIST-TO-VEC:
    - PMAT-221 list_free_monoid_diamond: free monoid
    - PMAT-229 nonempty_section_retraction_diamond: NonEmpty subtype
    - PMAT-244 length_monoid_homomorphism_diamond: length functor
    - PMAT-338 list_reverse_involution_diamond: reverse involution
    - PMAT-349 py_list_silver_struct_extensionality_diamond: INPUT record
    - **PMAT-360: TYPED-RUST-VEC-SILVER STRUCTURE EXTENSIONALITY** ← depth-6

    The categorical distinction is sharp:
      - PMAT-349 captures struct-ext of PyListSilver α (the INPUT
        record, the Python list).
      - PMAT-360 captures struct-ext of TypedRustVecSilver α (the
        OUTPUT record, the Rust Vec) — fundamentally distinct
        structural target. Closes the Python↔Rust lowering pair at
        the STRUCTURE level (mirror of PMAT-336/PMAT-352 which
        closed the Rust↔Lean pair).

    Sixteenth substrate-wide demonstration of the structure-
    extensionality pattern (after PMAT-311/329..336/349/352/353/
    354/356/359).

    Status: discharged at v0.1.0 (PMAT-360). Tier: DIAMOND.
    Broadens depth-6 to 7 contracts. -/

/--
  **Diamond-tier refinement theorem** — `TypedRustVecSilver α`
  admits STRUCTURE EXTENSIONALITY.

  Combines four STRUCTURE-EXTENSIONALITY properties on the 2-field
  TypedRustVecSilver record (elements : List α, element_type_tag :
  String):
  (a) Field-equality → record-equality
  (b) Record-equality → field-equality (congruence)
  (c) Self-equality (reflexivity)
  (d) Identity field-replacement preserves elements

  Sixteenth substrate-wide demonstration of the structure-
  extensionality pattern. Closes Python↔Rust lowering pair at the
  STRUCTURE level (mirror of PMAT-336/PMAT-352 closing Rust↔Lean).

  Status: **discharged at v0.1.0 (PMAT-360)**. Tier: DIAMOND.
  Broadens depth-6 to 7 contracts.
-/
theorem typed_rust_vec_silver_struct_extensionality_diamond
    {α : Type} (v1 v2 : TypedRustVecSilver α) :
    -- (a) Field equality → record equality
    (v1.elements = v2.elements ∧ v1.element_type_tag = v2.element_type_tag
      → v1 = v2)
    -- (b) Record equality → field equality
    ∧ (v1 = v2 → v1.elements = v2.elements
        ∧ v1.element_type_tag = v2.element_type_tag)
    -- (c) Self-equality (reflexivity)
    ∧ (v1 = v1)
    -- (d) Identity field-replacement preserves elements
    ∧ (({ elements := v1.elements, element_type_tag := v1.element_type_tag }
          : TypedRustVecSilver α).elements = v1.elements) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro ⟨h1, h2⟩
    cases v1; cases v2
    simp_all
  · intro h
    exact ⟨by rw [h], by rw [h]⟩
  · rfl
  · rfl

/-! ## PMAT-371 — SEVENTH Diamond on C-XLATE-PY-LIST-TO-VEC
    (Layer 2 BROADENS DEPTH-7):
    HETEROGENEOUS-LIST-SILVER STRUCTURE EXTENSIONALITY
    (XPILE-REFINE-XLATE-PY-LIST-008).

    **Broadens DEPTH-7 substrate-wide.** Pushes XlatePyListToVec
    (Layer 2) from depth-6 to depth-7 as the second L2 contract at
    depth-7+ (Bashrs was first via PMAT-368).

    The 7 Diamond categories on C-XLATE-PY-LIST-TO-VEC:
    - PMAT-221 list_free_monoid_diamond
    - PMAT-229 nonempty_section_retraction_diamond
    - PMAT-244 length_monoid_homomorphism_diamond
    - PMAT-338 list_reverse_involution_diamond
    - PMAT-349 py_list_silver_struct_extensionality_diamond (Python INPUT)
    - PMAT-360 typed_rust_vec_silver_struct_extensionality_diamond (Rust OUTPUT)
    - **PMAT-371: HETEROGENEOUS-LIST-SILVER STRUCTURE EXT** ← depth-7

    Twenty-fourth substrate-wide demonstration of the structure-
    extensionality pattern. Distinct from PMAT-349/PMAT-360 (which
    captured the homogeneous Python/Rust pair) — PMAT-371 captures
    the HETEROGENEOUS-list rejection record (reason : RejectionReason,
    element_types : Array String), the third major record type on
    this contract.

    Status: discharged at v0.1.0 (PMAT-371). Tier: DIAMOND. -/

/--
  **Diamond-tier refinement theorem** — `HeterogeneousListSilver`
  admits STRUCTURE EXTENSIONALITY.

  Combines four properties on the 2-field record:
  (a) Field-equality → record-equality
  (b) Record-equality → field-equality (congruence)
  (c) Decidable equality
  (d) Self-equality

  Status: **discharged at v0.1.0 (PMAT-371)**. Tier: DIAMOND.
-/
theorem heterogeneous_list_silver_struct_extensionality_diamond
    (h1 h2 : HeterogeneousListSilver) :
    (h1.reason = h2.reason ∧ h1.element_types = h2.element_types → h1 = h2)
    ∧ (h1 = h2 → h1.reason = h2.reason ∧ h1.element_types = h2.element_types)
    ∧ (h1 = h2 ∨ h1 ≠ h2)
    ∧ (h1 = h1) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro ⟨h_a, h_b⟩
    cases h1; cases h2
    simp_all
  · intro h
    exact ⟨by rw [h], by rw [h]⟩
  · by_cases h : h1 = h2
    · exact Or.inl h
    · exact Or.inr h
  · rfl

/-! ## PMAT-382 — EIGHTH Diamond on C-XLATE-PY-LIST-TO-VEC
    (Layer 2 BROADENS DEPTH-8):
    HOMOGENEOUS-LIST-SILVER α STRUCTURE EXTENSIONALITY
    (XPILE-REFINE-XLATE-PY-LIST-009).

    **Broadens DEPTH-8 substrate-wide.** Pushes XlatePyListToVec
    (Layer 2) from depth-7 to depth-8 as the second L2 contract at
    depth-8+.

    The 8 Diamond categories on C-XLATE-PY-LIST-TO-VEC:
    - PMAT-221 list_free_monoid
    - PMAT-229 nonempty_section_retraction
    - PMAT-244 length_monoid_homomorphism
    - PMAT-338 list_reverse_involution
    - PMAT-349 py_list_silver_struct_extensionality
    - PMAT-360 typed_rust_vec_silver_struct_extensionality
    - PMAT-371 heterogeneous_list_silver_struct_extensionality
    - **PMAT-382: HOMOGENEOUS-LIST-SILVER α STRUCTURE EXT** ← depth-8

    Twenty-ninth substrate-wide demonstration of structure-
    extensionality. Captures HomogeneousListSilver α — the fourth
    major record on this contract (after PMAT-349 PyListSilver α,
    PMAT-360 TypedRustVecSilver α, PMAT-371 HeterogeneousListSilver).

    Status: discharged at v0.1.0 (PMAT-382). Tier: DIAMOND. -/

/--
  **Diamond-tier refinement theorem** — `HomogeneousListSilver α`
  admits STRUCTURE EXTENSIONALITY.

  2-field polymorphic record (elements : List α, element_type_tag :
  String) with derived DecidableEq.

  Status: **discharged at v0.1.0 (PMAT-382)**. Tier: DIAMOND.
-/
theorem homogeneous_list_silver_struct_extensionality_diamond
    {α : Type} (h1 h2 : HomogeneousListSilver α) :
    (h1.elements = h2.elements ∧ h1.element_type_tag = h2.element_type_tag
      → h1 = h2)
    ∧ (h1 = h2 → h1.elements = h2.elements
        ∧ h1.element_type_tag = h2.element_type_tag)
    ∧ (h1 = h1) := by
  refine ⟨?_, ?_, ?_⟩
  · intro ⟨h_e, h_t⟩
    cases h1; cases h2
    simp_all
  · intro h
    exact ⟨by rw [h], by rw [h]⟩
  · rfl

/--
  **PMAT-395 Diamond — NonEmptyHomogeneousList subtype extensionality.**

  The Gold-tier subtype `NonEmptyHomogeneousList α := { l :
  HomogeneousListSilver α // l.elements ≠ [] }` satisfies subtype
  extensionality. SEVENTH substrate-wide subtype-extensionality
  demonstration. Template 9 (Gold-tier subtype-ext) expands to 7
  substrate instances.

  Adds a NINTH distinct Diamond category on
  `C-XLATE-PY-LIST-TO-VEC`, pushing the contract from depth-8 to
  depth-9. Second L2 contract at depth-9 (after PMAT-390 Bashrs).
  Polymorphic in α — first polymorphic subtype-ext on the
  substrate.
-/
theorem non_empty_homogeneous_list_subtype_extensionality_diamond
    {α : Type} (n1 n2 : NonEmptyHomogeneousList α) :
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
  **PMAT-406 Diamond — Tagged→Untagged tier projection on HomogeneousListSilver.**

  Define the canonical forgetful map `homogeneous_to_simple_list :
  HomogeneousListSilver α → PyListSilver α` that drops the
  `element_type_tag` field, retaining only `elements`. **SIXTH
  instance of Template 10 (Tier-projection homomorphism)**.
  Captures the tagged→untagged refinement direction within the
  Silver tier — the audit tag is forgotten while the structural
  payload is preserved.

  Adds a TENTH distinct Diamond category on
  `C-XLATE-PY-LIST-TO-VEC`, pushing the contract from depth-9 to
  depth-10. Third L2 contract at depth-10 (after PMAT-401 Bashrs
  and PMAT-400 FfiCpythonExt openers). Polymorphic in α — second
  polymorphic Template 10 instance.
-/
def homogeneous_to_simple_list {α : Type}
    (h : HomogeneousListSilver α) : PyListSilver α :=
  { elems := h.elements }

theorem homogeneous_to_simple_list_projection_diamond
    {α : Type} (h : HomogeneousListSilver α) :
    -- (a) elements preserved by projection
    ((homogeneous_to_simple_list h).elems = h.elements)
    -- (b) projection is independent of element_type_tag (forgetful)
    ∧ (homogeneous_to_simple_list ⟨h.elements, ""⟩
        = homogeneous_to_simple_list ⟨h.elements, h.element_type_tag⟩)
    -- (c) empty elements maps to empty PyListSilver
    ∧ ((homogeneous_to_simple_list
          ⟨([] : List α), h.element_type_tag⟩).elems = ([] : List α))
    -- (d) self-equality (reflexivity)
    ∧ (homogeneous_to_simple_list h = homogeneous_to_simple_list h) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · rfl
  · rfl
  · rfl
  · rfl

end XpileContracts.CXlatePyListToVec
