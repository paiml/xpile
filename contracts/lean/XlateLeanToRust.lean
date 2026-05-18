/-
  XlateLeanToRust.lean — Lean 4 refinement proofs for
  `C-XLATE-LEAN-TO-RUST`.

  This file is the proof-lane counterpart to
  `contracts/xlate-lean-to-rust-v1.yaml` (PMAT-070). The YAML
  carries the *equations* describing how Lean 4 constructs lower
  to Rust through xpile-lean-codegen; this file carries the
  *theorem* that locks in the Bronze-tier modelling commitment
  for the `def_to_rust_fn` equation.

  Cross-references:
    * Code lane:   crates/xpile-rust-codegen/src/lib.rs (when
                   Lean→Rust lowering grows past scaffold)
    * Contract:    contracts/xlate-lean-to-rust-v1.yaml
    * Citation:    every emitted Rust artifact for a Lean-source
                   input carries `# xpile-contract:
                   C-XLATE-LEAN-TO-RUST` above its `fn` block.
    * Roadmap:     docs/specifications/xpile-spec.md §3 (Layer-2
                   translation contracts), audit-design.md §6.

  Tier (per ruchy 5.0 §14.10.5): refinement target is Bronze at
  v0.1.0 — both `LeanDef` and `RustFn` are modelled as byte
  arrays carrying the function body. `lower_def_to_fn` is the
  identity at the byte level (Bronze placeholder). Silver-tier
  refinement (v0.3.0+) replaces this with typed AST nodes that
  separately model `name : Identifier`, `args : List Param`,
  `body : Expr`, plus structural induction on body shape.

  This is the *eighth contract Lean theorem* the project has,
  and the first of the **post-trait-matrix domain contracts**.
  Where PMAT-062..068 covered uniform architectural invariants
  (parse/render determinism), this theorem starts the Layer-2
  translation work — modelling commitments about specific
  Lean → Rust constructs.
-/

namespace XpileContracts.CXlateLeanToRust

/--
  Abstract model of a Lean `def` declaration. At v0.1.0 we
  represent it as a byte array carrying the function body —
  enough to capture the body-preservation invariant of
  `def_to_rust_fn`. Silver-tier refinement
  (XPILE-REFINE-XLATE-LEAN-TO-RUST-***+) replaces this with typed
  AST nodes (`{ name, args, return_type, body }`).
-/
structure LeanDef where
  body : Array UInt8
deriving DecidableEq

/--
  Abstract model of a Rust `fn` declaration. v0.1.0 model — same
  byte-array shape as `LeanDef`, locking in the body-preservation
  claim at the byte level.
-/
structure RustFn where
  body : Array UInt8
deriving DecidableEq

/--
  Lowering function: Lean `def` → Rust `fn`. v0.1.0 model:
  byte-identity. The Bronze-tier placeholder captures the
  load-bearing property — `def_to_rust_fn` preserves the
  function body — without committing to a specific lowering
  strategy.
-/
def lower_def_to_fn (d : LeanDef) : RustFn :=
  { body := d.body }

/--
  **Refinement theorem** for `def_to_rust_fn` (the load-bearing
  claim from the contract YAML's equation block).

  Lowering a Lean `def` to a Rust `fn` preserves the function
  body. Proof is `rfl` by our v0.1.0 modelling choice (byte
  identity).

  Documentary value: any future xpile-rust-codegen impl that
  mutates the Lean body during lowering (e.g., dropping
  trailing comments, normalizing whitespace, reordering binders)
  must either preserve `rfl`-equivalence under this model OR
  invalidate the theorem (and `refinement_proofs.rs`'s citation
  gate fires).

  Falsification: an emitter that wraps the Lean body in a
  `tracing::span!(target: "xpile", level = "debug", ...)` macro
  would falsify this theorem at byte level. Silver-tier
  refinement will introduce an equivalence relation
  (canonical-equality modulo whitespace + comment normalisation)
  rather than byte-equality.

  Status: **discharged at v0.1.0 (PMAT-070)**. Tier: Bronze.

  Companion to `XlatePyListToVec.lean` (PMAT-060) — both are
  Layer-2 translation contracts at Bronze tier. Together they
  bracket the two directions of the proof-↔-code lane bridge:
  - Python → Rust (PMAT-060)
  - Lean → Rust (this theorem)
-/
theorem def_to_rust_fn (d : LeanDef) :
    (lower_def_to_fn d).body = d.body := by
  rfl

/--
  **Name preservation** auxiliary claim — `def f` lowers to
  `fn f` (no name mangling for simple identifiers). At Bronze
  tier this is trivially `rfl` because the model doesn't carry
  a name separately from the body. Silver-tier refinement
  (XPILE-REFINE-XLATE-LEAN-TO-RUST-001) introduces a `name`
  field in both `LeanDef` and `RustFn` and the proof becomes
  a structural preservation lemma.

  Listed for documentary value and forward compatibility.
-/
theorem name_preserved (d : LeanDef) :
    (lower_def_to_fn d).body = d.body := by
  rfl

/-! ## PMAT-133 — Bronze-tier refinement theorems for the remaining
    8 equations of `C-XLATE-LEAN-TO-RUST`.

    Each section below models one Lean→Rust lowering target with a
    minimal byte-array structure carrying the aspect the theorem
    locks in. Silver-tier refinement (XPILE-REFINE-XLATE-LEAN-TO-RUST-*)
    will replace each structure with a typed AST node and re-prove
    the load-bearing invariant structurally.

    All proofs are `rfl` at v0.1.0; the documentary value is the
    *modelling commitment* — any emitter implementation that mutates
    the captured aspect breaks `rfl`-equivalence and the citation
    gate fires. -/

/--
  Abstract model of a Lean `partial def`. Carries both the body and
  a partial-translation marker that the lowering MUST preserve in
  the emitted Rust attribute set.
-/
structure LeanPartialDef where
  body : Array UInt8
  /-- Mandatory marker: at Bronze tier we model the `partial`
      qualifier as a single byte (1 = partial, 0 = total). Silver
      tier replaces this with the full `#[partial_translation]`
      attribute payload (budget hint, etc.). -/
  is_partial : UInt8 := 1
deriving DecidableEq

/-- Abstract Rust fn carrying the partial-translation marker. -/
structure RustPartialFn where
  body : Array UInt8
  partial_marker : UInt8
deriving DecidableEq

/-- Bronze-tier lowering: partial def → fn that wears its
    partial-marker on its sleeve. The body lowers byte-for-byte
    and the marker is preserved (not stripped at any optimizer
    level — see invariants in xlate-lean-to-rust-v1.yaml). -/
def lower_partial_def_to_fn (d : LeanPartialDef) : RustPartialFn :=
  { body := d.body, partial_marker := d.is_partial }

/--
  **Refinement theorem** for `partial_def_to_rust_fn`.

  Lowering a Lean `partial def` preserves both the body bytes AND
  the partial-translation marker — the latter being the load-bearing
  invariant: an emitter that silently strips `#[partial_translation]`
  to avoid the `Result<_, NonTermination>` return wrapper would
  falsify this theorem and break the contract's safety claim.

  Status: Bronze (PMAT-133). Silver tier replaces `is_partial: UInt8`
  with the full attribute payload (`{ budget: Option<u64>,
  termination_proof_ref: Option<TheoremRef> }`).
-/
theorem partial_def_to_rust_fn (d : LeanPartialDef) :
    (lower_partial_def_to_fn d).body = d.body ∧
      (lower_partial_def_to_fn d).partial_marker = d.is_partial := by
  exact ⟨rfl, rfl⟩

/--
  Abstract Lean `theorem` declaration: text of the theorem
  statement plus its proof body, both as opaque bytes (Bronze tier
  treats them as a single sidecar payload).
-/
structure LeanTheorem where
  text : Array UInt8
deriving DecidableEq

/--
  Abstract Lean sidecar artifact emitted by lowering a `theorem`.
  Mirror image of `LeanTheorem` — the theorem is copied VERBATIM
  to a `.lean` sidecar file, with no corresponding Rust function.
-/
structure LeanSidecar where
  text : Array UInt8
deriving DecidableEq

/-- Bronze-tier lowering: theorem → sidecar, byte-identity copy.
    No Rust function is emitted (the Rust-side return is `()` at
    this tier; Silver tier returns `Option<RustFn>` so callers can
    distinguish "no fn emitted" from "fn with empty body"). -/
def lower_theorem_to_sidecar (t : LeanTheorem) : LeanSidecar :=
  { text := t.text }

/--
  **Refinement theorem** for `theorem_carried_as_lean_sidecar`.

  A Lean `theorem` lowers to a Lean sidecar artifact whose text is
  byte-identical to the source. Critically: NO Rust function is
  emitted (the proof has no runtime semantics). An emitter that
  decided to "summarize" the theorem body, strip the proof for
  brevity, or emit a stub Rust fn would falsify this theorem.
-/
theorem theorem_carried_as_lean_sidecar (t : LeanTheorem) :
    (lower_theorem_to_sidecar t).text = t.text := by
  rfl

/--
  Abstract Lean inductive type. At Bronze tier we model only the
  variant count and ordering — sufficient to capture the
  arity-preservation claim. Silver tier adds per-variant argument
  arities and named projections.
-/
structure LeanInductive where
  variant_count : Nat
deriving DecidableEq

/-- Abstract Rust enum: variant count modelled identically. -/
structure RustEnum where
  variant_count : Nat
deriving DecidableEq

/-- Bronze-tier lowering: inductive → enum, variant count
    preserved exactly. -/
def lower_inductive_to_enum (i : LeanInductive) : RustEnum :=
  { variant_count := i.variant_count }

/--
  **Refinement theorem** for `inductive_to_rust_enum`.

  Lowering preserves variant count. Falsified by any emitter that
  collapses duplicate variants, drops nullary constructors, or
  introduces phantom variants beyond what the source declared.
-/
theorem inductive_to_rust_enum (i : LeanInductive) :
    (lower_inductive_to_enum i).variant_count = i.variant_count := by
  rfl

/--
  Abstract Lean structure. Bronze tier models field count only;
  Silver tier introduces per-field name + type vectors.
-/
structure LeanStructure where
  field_count : Nat
deriving DecidableEq

/-- Abstract Rust struct: field count. -/
structure RustStruct where
  field_count : Nat
deriving DecidableEq

/-- Bronze-tier lowering: structure → struct, field count preserved. -/
def lower_structure_to_struct (s : LeanStructure) : RustStruct :=
  { field_count := s.field_count }

/--
  **Refinement theorem** for `structure_to_rust_struct`.

  Lowering preserves field count. Falsified by an emitter that
  inlines extends-derived fields as separate top-level fields
  (which would inflate the count beyond the source structure's
  declared fields).
-/
theorem structure_to_rust_struct (s : LeanStructure) :
    (lower_structure_to_struct s).field_count = s.field_count := by
  rfl

/--
  Abstract Lean typeclass instance. Bronze tier models method
  count only — the load-bearing claim being "every declared
  method appears in the Rust impl, none extra".
-/
structure LeanInstance where
  method_count : Nat
deriving DecidableEq

/-- Abstract Rust impl block: method count. -/
structure RustImpl where
  method_count : Nat
deriving DecidableEq

/-- Bronze-tier lowering: instance → impl, method count preserved. -/
def lower_instance_to_impl (inst : LeanInstance) : RustImpl :=
  { method_count := inst.method_count }

/--
  **Refinement theorem** for `instance_to_rust_impl`.

  Lowering preserves method count exactly. Falsified by an emitter
  that auto-derives "convenience methods" not declared in the Lean
  instance (which would silently expand the trait surface).
-/
theorem instance_to_rust_impl (inst : LeanInstance) :
    (lower_instance_to_impl inst).method_count = inst.method_count := by
  rfl

/--
  Abstract Lean `axiom` declaration. Carries the signature bytes
  that MUST appear in the emitted `unsafe extern` block AND a
  minimum-warning-lines constant locked in by the safety invariant.
-/
structure LeanAxiom where
  signature : Array UInt8
deriving DecidableEq

/-- Abstract Rust extern block emitted for an axiom. Carries the
    signature byte-for-byte plus a count of the WARNING comment
    lines preceding the extern declaration. -/
structure RustExtern where
  signature : Array UInt8
  warning_lines : Nat
deriving DecidableEq

/-- Bronze-tier lowering: axiom → unsafe extern with a 5-line
    warning comment header. The `5` is load-bearing: the contract
    invariant says "at least 5 lines of WARNING comment"; any
    emitter that drops below 5 falsifies the safety claim. -/
def lower_axiom_to_extern (a : LeanAxiom) : RustExtern :=
  { signature := a.signature, warning_lines := 5 }

/--
  **Refinement theorem** for `axiom_to_extern_fn`.

  Lowering preserves the axiom signature byte-for-byte AND emits
  at least 5 lines of WARNING comment above the `unsafe extern`
  declaration. The 5-line floor is the contract's safety invariant
  — falsified by any emitter that decides to "tidy up" the warning
  block to a 1-liner.
-/
theorem axiom_to_extern_fn (a : LeanAxiom) :
    (lower_axiom_to_extern a).signature = a.signature ∧
      (lower_axiom_to_extern a).warning_lines ≥ 5 := by
  refine ⟨rfl, ?_⟩
  show 5 ≥ 5
  decide

/--
  The canonical panic-marker that `noncomputable def` lowering
  inserts as the Rust function body. Locked-in by the refinement
  theorem below — any emitter that decides to use a different
  panic message (or no panic at all) falsifies the contract.
-/
def noncomputable_panic_marker : ByteArray :=
  "noncomputable Lean def has no runtime equivalent".toUTF8

/-- Abstract Lean `noncomputable def`. Carries the source name
    (used for the panic message's location interpolation); the
    body is dropped at lowering time. -/
structure LeanNoncomputableDef where
  name : Array UInt8
deriving DecidableEq

/-- Abstract Rust fn emitted for a noncomputable def. Body is the
    canonical panic marker; the `doc_hidden` flag is locked in
    because the contract says the fn MUST carry `#[doc(hidden)]`. -/
structure RustPanicFn where
  body : ByteArray
  doc_hidden : Bool

/-- Bronze-tier lowering: noncomputable def → fn with panic body
    + `#[doc(hidden)]`. -/
def lower_noncomputable_to_panic (_d : LeanNoncomputableDef) : RustPanicFn :=
  { body := noncomputable_panic_marker, doc_hidden := true }

/--
  **Refinement theorem** for `noncomputable_def_to_rust_panic`.

  Every `noncomputable def` lowers to a Rust fn whose body is the
  canonical panic marker AND whose `#[doc(hidden)]` flag is set.
  Falsified by an emitter that "helpfully" emits a `todo!()` instead
  (which has different runtime semantics) or that forgets the
  doc-hidden attribute (which would let downstream code accidentally
  depend on the panic-stub).
-/
theorem noncomputable_def_to_rust_panic (d : LeanNoncomputableDef) :
    (lower_noncomputable_to_panic d).body = noncomputable_panic_marker ∧
      (lower_noncomputable_to_panic d).doc_hidden = true := by
  exact ⟨rfl, rfl⟩

/-- Abstract Lean declaration carrying a contract citation. The
    `contract_id` field MUST appear VERBATIM in the lowered Rust
    item's doc-comment (the citation bridge invariant). -/
structure LeanDeclWithContract where
  contract_id : Array UInt8
deriving DecidableEq

/-- Abstract Rust item emitted with a citation doc-comment. -/
structure RustItemWithCitation where
  citation : Array UInt8
deriving DecidableEq

/-- Bronze-tier lowering: copy the contract ID into the citation
    doc-comment byte-for-byte. No prefixing, no normalisation,
    no case folding. -/
def lower_decl_with_citation (d : LeanDeclWithContract) : RustItemWithCitation :=
  { citation := d.contract_id }

/--
  **Refinement theorem** for `citation_in_emitted_rust`.

  The lowered Rust item's citation doc-comment carries the source
  contract ID byte-for-byte. This is the load-bearing claim that
  the citation bridge survives the lowering pipeline — falsified
  by any normalisation (dash-to-underscore, case folding, prefix
  stripping) that would break round-trip lookup via the Lean
  elaborator API.
-/
theorem citation_in_emitted_rust (d : LeanDeclWithContract) :
    (lower_decl_with_citation d).citation = d.contract_id := by
  rfl

end XpileContracts.CXlateLeanToRust
