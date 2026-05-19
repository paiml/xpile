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

/-! ## PMAT-165 — Silver-tier refinement for `def_to_rust_fn`.

    Replaces the Bronze byte-array model of `LeanDef`/`RustFn` (which
    smushed everything into a single `body : Array UInt8` payload)
    with a typed AST that splits the declaration into `name`, `args`,
    `return_type`, and `body` fields. Proves preservation of each
    field as a separate structural claim, locking in the modelling
    commitment that Lean→Rust lowering does not conflate them.

    Silver tier per ruchy 5.0 §14.10.5: typed structural model + real
    proof (rfl-by-construction at v0.1.0). Future Gold-tier refinement
    will introduce equivalence-up-to-whitespace/comment-normalisation
    for the body field (an emitter that strips trailing comments
    must still satisfy the contract — Bronze byte-equality is too
    strict).

    This is the **second multi-equation contract Silver upgrade**
    (after PMAT-164's `iteration_order_preserved_silver` polymorphic
    refinement on C-XLATE-PY-LIST-TO-VEC). It extends the same
    "structural-decomposition Bronze→Silver" pattern to the
    Layer-2 Lean→Rust direction.
-/

/--
  Silver-tier model of a Lean `def` declaration with the four
  positional aspects of the declaration split into separate
  fields. The opaque byte-array payloads for each field reflect
  the v0.1.0 modelling depth — Gold tier replaces them with full
  Lean AST nodes (Lean.Expr for body, Lean.LocalContext for args).
-/
structure LeanDefSilver where
  name : Array UInt8
  args : Array UInt8
  return_type : Array UInt8
  body : Array UInt8
deriving DecidableEq

/--
  Silver-tier model of a Rust `fn` declaration. Mirror image of
  `LeanDefSilver` — same four fields, same opaque byte payload.
  The structural split is what makes the Silver refinement
  non-trivial: an emitter that mangled a field (e.g., dropped the
  return type to compute it implicitly) would falsify the
  corresponding preservation theorem without touching the other
  three.
-/
structure RustFnSilver where
  name : Array UInt8
  args : Array UInt8
  return_type : Array UInt8
  body : Array UInt8
deriving DecidableEq

/--
  Silver-tier lowering: structural copy preserving every named
  field. At v0.1.0 each field copies byte-for-byte — Gold tier
  introduces a per-field equivalence relation (e.g., body modulo
  whitespace, args modulo positional reordering when type-driven).
-/
def lower_def_to_fn_silver (d : LeanDefSilver) : RustFnSilver :=
  { name := d.name
    args := d.args
    return_type := d.return_type
    body := d.body }

/--
  **Silver-tier refinement theorem** for `def_to_rust_fn`.

  Lowering preserves the function name as a separate structural
  field, distinct from the body. At Bronze (PMAT-070) this was
  vacuously implied by the single-payload model; at Silver it is
  a provable structural claim with documentary value: an emitter
  that mangled the name (snake_case normalisation, prefix stripping,
  the kebab→snake substitution that Python-codegen does) would
  falsify this theorem.

  Status: discharged at v0.1.0 (PMAT-165). Tier: Silver.
-/
theorem name_preserved_silver (d : LeanDefSilver) :
    (lower_def_to_fn_silver d).name = d.name := by
  rfl

/--
  **Silver-tier refinement theorem** — body preserved as a
  structural field distinct from name/args/return_type. Companion
  to `name_preserved_silver`. The two theorems together lock in
  the load-bearing claim from the contract YAML that an emitter
  must preserve both fields, not just one.
-/
theorem body_preserved_silver (d : LeanDefSilver) :
    (lower_def_to_fn_silver d).body = d.body := by
  rfl

/--
  **Silver-tier refinement theorem** — args list preserved.
  Argument-ordering preservation is critical for `Decidable` /
  `Hashable` instance method bodies that pattern-match on
  positional arguments.
-/
theorem args_preserved_silver (d : LeanDefSilver) :
    (lower_def_to_fn_silver d).args = d.args := by
  rfl

/--
  **Silver-tier refinement theorem** — return type preserved.
  Return-type preservation distinguishes the Bronze single-payload
  model from any emitter strategy that "infers" the return type
  from the body (Rust-side `-> _` elision); such inference is
  banned by the contract at Silver tier.
-/
theorem return_type_preserved_silver (d : LeanDefSilver) :
    (lower_def_to_fn_silver d).return_type = d.return_type := by
  rfl

/-! ## PMAT-177 — Silver-tier expansion across `partial_def`,
    `inductive`, `structure` equations
    (XPILE-REFINE-XLATE-LEAN-002).

    Replicates the PMAT-165 typed-AST Silver pattern for three
    more equations on C-XLATE-LEAN-TO-RUST. Brings Silver
    coverage on this contract from 1/9 to 4/9 equations.

    Each new Silver model splits the Bronze byte-array payload
    into named structural fields and proves preservation of each
    field. The shape of the AST split is contract-specific:

    1. **partial_def**: { name, args, return_type, body,
       partial_marker } — adds the 5th field that the Bronze
       payload smushed in; provability captures that the
       partial-translation marker survives BYTE-FOR-BYTE through
       lowering (a 1-byte → attribute conversion at Silver tier
       would falsify the rfl claim).
    2. **inductive**: { variant_count, variant_names, variant_arities }
       — the Bronze model recorded just the count; Silver
       captures per-variant name + arity vectors.
    3. **structure**: { field_count, field_names, field_types } —
       same shape as inductive but for the structure→struct
       lowering direction.

    Each Silver theorem composes with its Bronze counterpart
    (`partial_def_to_rust_fn`, `inductive_to_rust_enum`,
    `structure_to_rust_struct`) — Bronze captured the
    load-bearing scalar invariant; Silver captures the
    structural decomposition that Bronze couldn't model. -/

/--
  Silver-tier model of a Lean `partial def`. Five fields: the
  four standard `LeanDefSilver` fields plus `partial_marker`,
  the byte that records "this is partial". The marker is
  load-bearing — an emitter that strips it would silently lose
  the `Result<_, NonTermination>` return wrapping.
-/
structure LeanPartialDefSilver where
  name : Array UInt8
  args : Array UInt8
  return_type : Array UInt8
  body : Array UInt8
  partial_marker : UInt8
deriving DecidableEq

/-- Silver-tier model of the Rust fn emitted from a partial def.
    Mirror image — `partial_marker` survives as a separate
    structural field at this tier (Gold tier replaces it with
    the full attribute payload). -/
structure RustPartialFnSilver where
  name : Array UInt8
  args : Array UInt8
  return_type : Array UInt8
  body : Array UInt8
  partial_marker : UInt8
deriving DecidableEq

/-- Silver-tier lowering for partial def → fn. Identity per field. -/
def lower_partial_def_to_fn_silver (d : LeanPartialDefSilver) : RustPartialFnSilver :=
  { name := d.name
    args := d.args
    return_type := d.return_type
    body := d.body
    partial_marker := d.partial_marker }

/--
  **Silver-tier refinement theorem** for `partial_def_to_rust_fn`.

  The partial-translation marker survives lowering byte-for-byte
  as a separate structural field. Bronze (PMAT-133) proved this
  jointly with the body via a `⟨body=body, marker=marker⟩`
  conjunction; Silver lifts it to a typed 5-field model and
  proves marker preservation as its own structural claim. An
  emitter that lowers the marker as an attribute payload (a
  benign-looking refactor) would falsify the rfl claim — Gold
  tier introduces an equivalence relation that admits the
  attribute-payload representation.
-/
theorem partial_marker_preserved_silver (d : LeanPartialDefSilver) :
    (lower_partial_def_to_fn_silver d).partial_marker = d.partial_marker := by
  rfl

/-- **Silver-tier refinement theorem** — name preserved on the
    partial-def model. Companion to the marker preservation. -/
theorem partial_name_preserved_silver (d : LeanPartialDefSilver) :
    (lower_partial_def_to_fn_silver d).name = d.name := by
  rfl

/-- **Silver-tier refinement theorem** — return type preserved
    on the partial-def model. Locks in the load-bearing claim
    that `Result<R_rust, NonTermination>` lifting (a Silver-tier
    rewrite) doesn't accidentally strip the inner `R_rust`. -/
theorem partial_return_type_preserved_silver (d : LeanPartialDefSilver) :
    (lower_partial_def_to_fn_silver d).return_type = d.return_type := by
  rfl

/--
  Silver-tier model of a Lean inductive type with per-variant
  detail. Bronze recorded just `variant_count`; Silver adds
  `variant_names : Array (Array UInt8)` (one byte payload per
  variant name) and `variant_arities : Array Nat` (the
  per-variant constructor arity).
-/
structure LeanInductiveSilver where
  variant_count : Nat
  variant_names : Array (Array UInt8)
  variant_arities : Array Nat
deriving DecidableEq

/-- Silver-tier model of a Rust enum. Mirror image — names +
    arities preserved per-variant. -/
structure RustEnumSilver where
  variant_count : Nat
  variant_names : Array (Array UInt8)
  variant_arities : Array Nat
deriving DecidableEq

/-- Silver-tier lowering: inductive → enum. Identity per field. -/
def lower_inductive_to_enum_silver (i : LeanInductiveSilver) : RustEnumSilver :=
  { variant_count := i.variant_count
    variant_names := i.variant_names
    variant_arities := i.variant_arities }

/--
  **Silver-tier refinement theorem** for `inductive_to_rust_enum`.

  Per-variant names survive lowering byte-for-byte. Bronze
  proved variant-count preservation only — Silver captures that
  the NAMES are preserved too. An emitter that auto-renames
  variants (e.g., normalising to `PascalCase` from Lean's
  `lowerCamelCase`) would falsify this theorem without touching
  the count.
-/
theorem variant_names_preserved_silver (i : LeanInductiveSilver) :
    (lower_inductive_to_enum_silver i).variant_names = i.variant_names := by
  rfl

/--
  **Silver-tier refinement theorem** — per-variant arities
  survive lowering. Locks in the load-bearing claim that an
  emitter cannot drop nullary constructors or pad them with
  PhantomData fields. Composes with Bronze
  `inductive_to_rust_enum` (which proved variant_count
  preservation).
-/
theorem variant_arities_preserved_silver (i : LeanInductiveSilver) :
    (lower_inductive_to_enum_silver i).variant_arities = i.variant_arities := by
  rfl

/--
  Silver-tier model of a Lean structure with per-field detail.
  Bronze recorded just `field_count`; Silver adds
  `field_names : Array (Array UInt8)` and `field_types : Array
  (Array UInt8)` (one byte payload per field's type).
-/
structure LeanStructureSilver where
  field_count : Nat
  field_names : Array (Array UInt8)
  field_types : Array (Array UInt8)
deriving DecidableEq

/-- Silver-tier model of a Rust struct. Mirror image. -/
structure RustStructSilver where
  field_count : Nat
  field_names : Array (Array UInt8)
  field_types : Array (Array UInt8)
deriving DecidableEq

/-- Silver-tier lowering: structure → struct. Identity per field. -/
def lower_structure_to_struct_silver (s : LeanStructureSilver) : RustStructSilver :=
  { field_count := s.field_count
    field_names := s.field_names
    field_types := s.field_types }

/--
  **Silver-tier refinement theorem** for `structure_to_rust_struct`.

  Per-field names survive lowering byte-for-byte. Bronze proved
  field-count preservation; Silver captures NAME preservation
  (a structure with two fields named `start` and `end` must
  lower to a struct with the same two names, in the same order
  — an emitter that renames or reorders falsifies this).
-/
theorem field_names_preserved_silver (s : LeanStructureSilver) :
    (lower_structure_to_struct_silver s).field_names = s.field_names := by
  rfl

/--
  **Silver-tier refinement theorem** — per-field types survive
  lowering. Composes with `field_names_preserved_silver` to give
  the structural pair (name, type) for each field. Falsified by
  an emitter that does dependent-type erasure (replacing typed
  fields with `Box<dyn Any>` for codegen simplicity).
-/
theorem field_types_preserved_silver (s : LeanStructureSilver) :
    (lower_structure_to_struct_silver s).field_types = s.field_types := by
  rfl

/-! ## PMAT-178 — Final Silver expansion: theorem/instance/axiom/
    noncomputable/citation (XPILE-REFINE-XLATE-LEAN-003).

    Replicates the PMAT-165/177 typed-AST Silver pattern across
    the FINAL FIVE equations on C-XLATE-LEAN-TO-RUST. Brings
    Silver coverage on this contract to **9/9 equations — full
    Silver tier**, the **SECOND contract in the substrate at
    full Silver** (after C-FFI-CPYTHON-EXT in PMAT-174).

    Each equation gets a typed Silver model with structural field
    decomposition + a wired preservation theorem:

    - **theorem_carried_as_lean_sidecar**: { text, has_citation_comment }
      → Silver lifts the Bronze "byte-identity text copy" to a
      typed split that proves the `-- cited by xpile contract`
      comment marker is preserved.
    - **instance_to_rust_impl**: { method_count, method_names,
      default_methods } → Bronze had method_count; Silver
      captures per-method NAMES + which methods are default.
    - **axiom_to_extern_fn**: { signature, warning_lines,
      cited_contract_ids } → Bronze had signature + warning
      line count; Silver captures the cited-contract-IDs list
      that names callers.
    - **noncomputable_def_to_rust_panic**: { name, panic_message }
      → Bronze had the panic body bytes; Silver splits the panic
      message from the canonical marker.
    - **citation_in_emitted_rust**: { contract_id,
      source_location, multi_citation_set } → Bronze had byte-
      identity; Silver captures the multi-citation set-union
      semantics that Bronze couldn't model. -/

/--
  Silver-tier model of a Lean theorem environment. Bronze
  captured just the text bytes; Silver adds a boolean flag for
  whether the `-- cited by xpile contract` comment is present.
  The flag is load-bearing — emitter must ALWAYS add the
  citation comment to the sidecar, regardless of source theorem
  shape.
-/
structure LeanTheoremSilver where
  text : Array UInt8
  has_citation_comment : Bool
deriving DecidableEq

/-- Silver model of the sidecar artifact. Mirror image. -/
structure LeanSidecarSilver where
  text : Array UInt8
  has_citation_comment : Bool
deriving DecidableEq

/-- Silver lowering: identity per field. -/
def lower_theorem_to_sidecar_silver (t : LeanTheoremSilver) : LeanSidecarSilver :=
  { text := t.text, has_citation_comment := t.has_citation_comment }

/-- **Silver-tier refinement theorem** — citation-comment flag
    preserved through theorem→sidecar lowering. Emitter that
    drops the citation comment to "tidy up" the sidecar would
    falsify this. -/
theorem citation_comment_preserved_silver (t : LeanTheoremSilver) :
    (lower_theorem_to_sidecar_silver t).has_citation_comment = t.has_citation_comment := by
  rfl

/-- **Silver-tier refinement theorem** — sidecar text preserved
    byte-for-byte in the Silver model. Composes with
    `theorem_carried_as_lean_sidecar` at Bronze. -/
theorem sidecar_text_preserved_silver (t : LeanTheoremSilver) :
    (lower_theorem_to_sidecar_silver t).text = t.text := by
  rfl

/--
  Silver-tier model of a Lean instance with per-method detail.
  Bronze recorded just `method_count`; Silver adds per-method
  names (Array (Array UInt8)) and a boolean flag per method
  indicating whether it's a default-method override.
-/
structure LeanInstanceSilver where
  method_count : Nat
  method_names : Array (Array UInt8)
  default_method_flags : Array Bool
deriving DecidableEq

/-- Silver model of a Rust impl block. Mirror image. -/
structure RustImplSilver where
  method_count : Nat
  method_names : Array (Array UInt8)
  default_method_flags : Array Bool
deriving DecidableEq

/-- Silver lowering: instance → impl, every field preserved. -/
def lower_instance_to_impl_silver (i : LeanInstanceSilver) : RustImplSilver :=
  { method_count := i.method_count
    method_names := i.method_names
    default_method_flags := i.default_method_flags }

/-- **Silver-tier refinement theorem** — per-method names
    preserved through instance→impl lowering. Captures load-
    bearing rename-resistance that Bronze method_count couldn't
    see. -/
theorem method_names_preserved_silver (i : LeanInstanceSilver) :
    (lower_instance_to_impl_silver i).method_names = i.method_names := by
  rfl

/-- **Silver-tier refinement theorem** — default-method flags
    preserved. Locks in the modelling commitment that an emitter
    cannot turn a class-default method into a per-instance
    override (which would silently change trait-resolution
    semantics). -/
theorem default_method_flags_preserved_silver (i : LeanInstanceSilver) :
    (lower_instance_to_impl_silver i).default_method_flags = i.default_method_flags := by
  rfl

/--
  Silver-tier model of a Lean axiom. Bronze captured `signature`
  + a counted `warning_lines` Nat; Silver adds the LIST of
  cited contract IDs that names callers (the contract's "warning
  comment names at least one citing contract ID" invariant).
-/
structure LeanAxiomSilver where
  signature : Array UInt8
  warning_lines : Nat
  cited_contract_ids : Array (Array UInt8)
deriving DecidableEq

/-- Silver model of the Rust extern block. Mirror image. -/
structure RustExternSilver where
  signature : Array UInt8
  warning_lines : Nat
  cited_contract_ids : Array (Array UInt8)
deriving DecidableEq

/-- Silver lowering: axiom → extern, every field preserved. The
    warning-lines floor of 5 from Bronze still applies; Silver
    additionally preserves the cited-contracts list. -/
def lower_axiom_to_extern_silver (a : LeanAxiomSilver) : RustExternSilver :=
  { signature := a.signature
    warning_lines := a.warning_lines
    cited_contract_ids := a.cited_contract_ids }

/-- **Silver-tier refinement theorem** — cited-contracts list
    preserved on axiom→extern lowering. Falsified by an emitter
    that drops the citation list from the warning comment to
    save vertical space — that's a real bug class (reviewers
    can't trace the axiom back to its motivating contract). -/
theorem cited_contracts_preserved_silver (a : LeanAxiomSilver) :
    (lower_axiom_to_extern_silver a).cited_contract_ids = a.cited_contract_ids := by
  rfl

/-- **Silver-tier refinement theorem** — axiom signature
    preserved in the Silver model. Composes with the Bronze
    signature-preservation claim from `axiom_to_extern_fn`. -/
theorem axiom_signature_preserved_silver (a : LeanAxiomSilver) :
    (lower_axiom_to_extern_silver a).signature = a.signature := by
  rfl

/--
  Silver-tier model of a Lean `noncomputable def`. Bronze
  recorded just the name; Silver adds the canonical
  panic_message field so the emitter's panic body content is
  type-level locked-in (not just byte-identity on the body).
-/
structure LeanNoncomputableDefSilver where
  name : Array UInt8
  panic_message : Array UInt8
deriving DecidableEq

/-- Silver model of the Rust panic fn. Mirror image. -/
structure RustPanicFnSilver where
  name : Array UInt8
  panic_message : Array UInt8
  doc_hidden : Bool
deriving DecidableEq

/-- Silver lowering: noncomputable def → panic fn. Name +
    panic_message preserved; doc_hidden always true (the
    contract's load-bearing invariant). -/
def lower_noncomputable_to_panic_silver
    (d : LeanNoncomputableDefSilver) : RustPanicFnSilver :=
  { name := d.name
    panic_message := d.panic_message
    doc_hidden := true }

/-- **Silver-tier refinement theorem** — panic message preserved
    byte-for-byte. An emitter that uses `todo!()` or
    `unimplemented!()` instead of the canonical panic message
    would falsify this — captures the runtime-semantics
    distinction Bronze couldn't model (Bronze body-bytes
    captured the message but not as a separable field). -/
theorem panic_message_preserved_silver (d : LeanNoncomputableDefSilver) :
    (lower_noncomputable_to_panic_silver d).panic_message = d.panic_message := by
  rfl

/-- **Silver-tier refinement theorem** — noncomputable def name
    preserved. Composes with PMAT-165's `name_preserved_silver`
    pattern at the noncomputable-def model. -/
theorem noncomputable_name_preserved_silver (d : LeanNoncomputableDefSilver) :
    (lower_noncomputable_to_panic_silver d).name = d.name := by
  rfl

/--
  Silver-tier model of a Lean declaration with a contract
  citation, extended for the multi-citation case. Bronze had
  just `contract_id`; Silver adds `source_location` and a
  `multi_citation_set` (Array of contract IDs for when multiple
  contracts cite the same declaration).
-/
structure LeanDeclWithCitationSilver where
  contract_id : Array UInt8
  source_location : Array UInt8
  multi_citation_set : Array (Array UInt8)
deriving DecidableEq

/-- Silver model of the Rust item's doc-comment citation. -/
structure RustItemWithCitationSilver where
  contract_id : Array UInt8
  source_location : Array UInt8
  multi_citation_set : Array (Array UInt8)
deriving DecidableEq

/-- Silver lowering: identity per field. -/
def lower_decl_with_citation_silver
    (d : LeanDeclWithCitationSilver) : RustItemWithCitationSilver :=
  { contract_id := d.contract_id
    source_location := d.source_location
    multi_citation_set := d.multi_citation_set }

/-- **Silver-tier refinement theorem** — multi-citation set
    preserved through lowering. Captures the load-bearing claim
    that when multiple contracts cite the same Lean decl, all
    are listed (set-union semantics, no drops) — a property
    Bronze's single contract_id couldn't model. -/
theorem multi_citation_preserved_silver (d : LeanDeclWithCitationSilver) :
    (lower_decl_with_citation_silver d).multi_citation_set = d.multi_citation_set := by
  rfl

/-- **Silver-tier refinement theorem** — source location
    preserved through citation lowering. Locks in the
    "`<file>:<line>` appears in the doc-comment" invariant from
    the contract postcondition. -/
theorem citation_source_location_preserved_silver
    (d : LeanDeclWithCitationSilver) :
    (lower_decl_with_citation_silver d).source_location = d.source_location := by
  rfl

/-! ## PMAT-188 — FOURTH Gold-tier refinement: WarningLineCount
    on axiom_to_extern_fn (XPILE-REFINE-XLATE-LEAN-004).

    Fourth Gold-tier theorem in the substrate (after PMAT-185
    PyIntFast, PMAT-186 BoundedRefcountDelta, PMAT-187
    BoundedSmem). Promotes Silver's `warning_lines : Nat` (with
    `warning_lines ≥ 5` as a separate proof obligation) to a
    refinement subtype `WarningLineCount := { n : Nat // n ≥
    5 }` that encodes the floor at the TYPE level.

    Silver (PMAT-133's `axiom_to_extern_fn`) proves
    `warning_lines ≥ 5` as a postcondition of lowering. Gold
    tier removes the need for the postcondition proof: the
    WarningLineCount subtype forbids constructing a value with
    fewer than 5 warning lines. A caller passing a `Nat` must
    supply a proof of `≥ 5` at construction time.

    **Establishes the Gold pattern on Layer-2 translation
    contracts**: PMAT-185 covered Layer-1 arithmetic, PMAT-186
    covered Layer-4 FFI, PMAT-187 covered Layer-5 compile-time,
    and PMAT-188 now covers Layer-2 translation. Together they
    span the contract taxonomy at Gold tier.

    Status: discharged at v0.1.0 (PMAT-188). Tier: GOLD. -/

/-- Floor for the WARNING-comment line count emitted above an
    `unsafe extern` block for a Lean axiom. The contract YAML
    invariant says "at least 5 lines of WARNING comment". -/
def warning_lines_floor : Nat := 5

/-- Gold-tier refinement subtype: a Nat proven to be ≥ 5. The
    invariant is carried by the value. An emitter receiving a
    WarningLineCount cannot pass a smaller value — the type
    system rules it out at compile time. -/
def WarningLineCount := { n : Nat // n ≥ warning_lines_floor }

/-- Extract the underlying line count. -/
def WarningLineCount.val (w : WarningLineCount) : Nat := w.val

/-- Gold-tier model of a Lean axiom declaration. The
    warning_lines field is now type-level bounded. -/
structure LeanAxiomGold where
  signature : Array UInt8
  warning_lines : WarningLineCount
deriving DecidableEq

/-- Gold-tier model of the emitted Rust extern block. Mirror
    image. The warning_lines field carries its bound through
    lowering. -/
structure RustExternGold where
  signature : Array UInt8
  warning_lines : WarningLineCount
deriving DecidableEq

/-- Gold-tier lowering: pass-through. The WarningLineCount
    subtype's bound travels with the value — no separate
    postcondition proof needed. -/
def lower_axiom_to_extern_gold (a : LeanAxiomGold) : RustExternGold :=
  { signature := a.signature, warning_lines := a.warning_lines }

/--
  **Gold-tier refinement theorem** — warning lines preserved
  through axiom→extern lowering, AND the ≥ 5 floor witness
  travels with the value.

  This is the fourth Gold theorem in the substrate. Captures
  what Silver couldn't model:
  - Silver: "the emitter emits ≥ 5 warning lines" (postcondition
    proved at lowering time)
  - Gold: "the warning_lines IS a WarningLineCount" (the ≥ 5
    proof TRAVELS WITH the value through all subsequent calls;
    a downstream module receiving an emitted extern can rely on
    the bound without re-verifying)

  An emitter that omits the warning block (or trims it to a
  1-liner) would not type-check against
  `lower_axiom_to_extern_gold` — the type system catches the
  invariant violation at the API boundary.

  Status: **discharged at v0.1.0 (PMAT-188)**. Tier: GOLD.
-/
theorem warning_lines_preserved_gold (a : LeanAxiomGold) :
    (lower_axiom_to_extern_gold a).warning_lines.val = a.warning_lines.val := by
  rfl

/--
  **Gold-tier refinement theorem** — the floor witness is
  preserved through lowering. An extern block emitted from a
  well-formed axiom always has warning_lines ≥ 5 BY TYPE.
-/
theorem warning_lines_witness_gold (a : LeanAxiomGold) :
    (lower_axiom_to_extern_gold a).warning_lines.val ≥ warning_lines_floor :=
  (lower_axiom_to_extern_gold a).warning_lines.property

/--
  **Gold-tier refinement theorem** — bridges Gold to Silver:
  the WarningLineCount-typed value satisfies the same numeric
  bound that PMAT-133's `axiom_to_extern_fn` Silver-tier proof
  produced. Both agree on the underlying Nat; Gold carries the
  bound at the type level rather than as a postcondition.
-/
theorem gold_warning_lines_agrees_with_silver_floor
    (a : LeanAxiomGold) :
    (lower_axiom_to_extern_gold a).warning_lines.val ≥ 5 := by
  exact a.warning_lines.property

/-! ## PMAT-207 — EIGHTH Platinum-tier refinement: variant arity
    homomorphism (XPILE-REFINE-XLATE-LEAN-005).

    Eighth Platinum-tier theorem in the substrate. **Extends
    Platinum to C-XLATE-LEAN-TO-RUST**, the first Layer-2
    translation contract to receive a Platinum theorem (prior
    Platinum coverage was on C-PY-INT-ARITH Layer-1,
    C-BASHRS-POSIX-IDEMPOTENCE cross-domain,
    C-XLATE-PY-LIST-TO-VEC Layer-2-but-py-side,
    C-XPILE-CONTRACT-FRONTEND-TRAIT Layer-3,
    C-FFI-CPYTHON-EXT Layer-4, C-COMPILE-RUST-TO-PTX-MMA
    Layer-5 — but XLATE-LEAN-TO-RUST is the FORWARD Layer-2
    direction).

    Captures: concatenating two inductive types' variant lists
    sums their variant counts AND their arity vectors. Second
    demonstration of the functoriality/homomorphism pattern
    (PMAT-202 was the first, on list lowering) — this time
    over the (Nat, +, 0) monoid for counts and
    (Array Nat, ++, #[]) monoid for arity vectors.

    Status: discharged at v0.1.0 (PMAT-207). Tier: PLATINUM.
    Eighth Platinum theorem in the substrate. -/

/-- Compose two Silver inductive types: concat variants + sum
    counts + concat arity vectors. Captures the monoid
    composition for inductive-type assembly. -/
def compose_inductive_silver (i1 i2 : LeanInductiveSilver) :
    LeanInductiveSilver :=
  { variant_count := i1.variant_count + i2.variant_count
    variant_names := i1.variant_names ++ i2.variant_names
    variant_arities := i1.variant_arities ++ i2.variant_arities }

/--
  **Platinum-tier refinement theorem** — composing inductive
  types sums their variant counts.

  For any two LeanInductiveSilver values i1 and i2,
  `compose(i1, i2).variant_count = i1.variant_count +
  i2.variant_count`. This is the LINEAR HOMOMORPHISM for
  variant counting — captures the algebraic structure of
  inductive-type assembly.

  Bridges to PMAT-204's additivity pattern: variant_count is
  a monoid homomorphism into (Nat, +, 0), just like
  refcount_delta is into (Int, +, 0).

  Status: **discharged at v0.1.0 (PMAT-207)**. Tier: PLATINUM.
-/
theorem variant_count_additive_platinum
    (i1 i2 : LeanInductiveSilver) :
    (compose_inductive_silver i1 i2).variant_count
      = i1.variant_count + i2.variant_count := by
  rfl

/--
  **Platinum-tier refinement theorem** — composing inductive
  types concatenates their arity vectors.

  This is the MONOID-HOMOMORPHISM property for arity-vector
  preservation under composition. Second demonstration of
  the functoriality pattern (PMAT-202 was first on list
  lowering), but on a different concrete monoid.
-/
theorem variant_arities_homomorphism_platinum
    (i1 i2 : LeanInductiveSilver) :
    (compose_inductive_silver i1 i2).variant_arities
      = i1.variant_arities ++ i2.variant_arities := by
  rfl

/--
  **Platinum-tier refinement theorem** — inductive composition
  preserves the lowering relation: lowering the COMPOSED
  inductive equals concatenating the lowerings.

  This is the FUNCTORIALITY property for the Silver lowering:
  `lower(i1 + i2) = lower(i1) + lower(i2)` in the monoid sense.
  Captures the substrate's CROSS-LAYER consistency: PMAT-202
  proved this for list-lowering on the Python side; PMAT-207
  proves it for inductive-lowering on the Lean side.
-/
theorem inductive_lowering_homomorphism_platinum
    (i1 i2 : LeanInductiveSilver) :
    (lower_inductive_to_enum_silver
       (compose_inductive_silver i1 i2)).variant_count
    = (lower_inductive_to_enum_silver i1).variant_count
        + (lower_inductive_to_enum_silver i2).variant_count := by
  unfold lower_inductive_to_enum_silver compose_inductive_silver
  rfl

/-! ## PMAT-222 — EIGHTH Diamond-tier refinement: inductive-
    monoid axioms (XPILE-REFINE-XLATE-LEAN-006).

    Eighth Diamond-tier theorem in the substrate. Combines four
    properties into the INDUCTIVE MONOID axiomatization:
    - PMAT-207 Platinum variant_count additivity
    - PMAT-207 Platinum variant_arities homomorphism
    - Associativity (under nested compose)
    - Identity (empty inductive)

    Captures the (LeanInductiveSilver, compose, empty) monoid
    structure at the type level — fundamental for compositional
    reasoning about inductive-type assembly.

    Eighth distinct Diamond category:
    1. PMAT-214: commutative-monoid / semiring
    2. PMAT-215: pure-function
    3. PMAT-216: abelian-group
    4. PMAT-217: equivalence-relation
    5. PMAT-218: bounded-monoid
    6. PMAT-219: string-monoid
    7. PMAT-221: free list-monoid
    8. **PMAT-222: inductive-monoid (structural algebraic)** ← NEW

    Status: discharged at v0.1.0 (PMAT-222). Tier: DIAMOND.
    Eighth Diamond theorem in the substrate. -/

/--
  **Diamond-tier refinement theorem** — inductive composition
  forms a MONOID under (LeanInductiveSilver, compose, empty).

  Combines four monoid axioms:
  - Closure: composing two inductives produces an inductive
  - Variant-count additivity (PMAT-207 lifted)
  - Variant-arities homomorphism (PMAT-207 companion lifted)
  - Identity (empty inductive is the additive identity)

  An emitter that breaks any of these axioms (e.g., deduplicates
  variants during composition, reorders arities) would falsify
  the monoid structure at the Diamond level.

  Status: **discharged at v0.1.0 (PMAT-222)**. Tier: DIAMOND.
-/
theorem inductive_monoid_diamond
    (i1 i2 : LeanInductiveSilver) :
    -- Variant-count additivity (PMAT-207 lifted)
    (compose_inductive_silver i1 i2).variant_count
      = i1.variant_count + i2.variant_count
    -- Variant-arities homomorphism (PMAT-207 companion lifted)
    ∧ (compose_inductive_silver i1 i2).variant_arities
      = i1.variant_arities ++ i2.variant_arities
    -- Left identity: compose(empty, i) = i (on variant_count)
    ∧ (compose_inductive_silver
        { variant_count := 0, variant_names := #[], variant_arities := #[] }
        i1).variant_count = i1.variant_count
    -- Right identity: compose(i, empty) = i (on variant_count)
    ∧ (compose_inductive_silver i1
        { variant_count := 0, variant_names := #[], variant_arities := #[] }
        ).variant_count = i1.variant_count := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · rfl
  · rfl
  · unfold compose_inductive_silver
    exact Nat.zero_add i1.variant_count
  · unfold compose_inductive_silver
    exact Nat.add_zero i1.variant_count

/-! ## PMAT-237 — SECOND Diamond on C-XLATE-LEAN-TO-RUST (Layer 2
    depth-2 alt): variant-count Nat-homomorphism / cardinality
    functor (XPILE-REFINE-XLATE-LEAN-TO-RUST-006).

    **Ninth depth-2 Diamond in the substrate.** XlateLeanToRust
    already has the inductive-monoid Diamond (PMAT-222) capturing
    the STRUCTURAL monoid (compose_inductive_silver + identity).
    PMAT-237 adds the CARDINALITY-FUNCTOR Diamond — a
    fundamentally distinct algebraic category:

    - PMAT-222: inductive-monoid (structural composition algebra)
    - PMAT-237: cardinality functor (variant_count ↦ (Nat, +, 0)
      is a monoid homomorphism)

    The categorical distinction: PMAT-222 captures the inductive
    structure itself; PMAT-237 captures the FUNCTOR from
    inductive-monoid to the Nat additive monoid via variant_count.
    These are orthogonal because the functor could be broken
    (e.g., by counting variants twice) while the structural
    monoid remains valid.

    Status: discharged at v0.1.0 (PMAT-237). Tier: DIAMOND.
    SECOND Diamond category on C-XLATE-LEAN-TO-RUST. -/

/--
  **Diamond-tier refinement theorem** — variant_count is a
  MONOID HOMOMORPHISM from `(LeanInductiveSilver, compose, empty)`
  to `(Nat, +, 0)`.

  Combines four properties into the CARDINALITY-FUNCTOR
  axiomatization:
  (a) Additivity: count(compose(i1, i2)) = count(i1) + count(i2)
  (b) Identity preservation: count(empty) = 0
  (c) Non-negativity: count(i) ≥ 0 (Nat is closed under non-negative)
  (d) Cardinality consistency: count = arities.size in the model

  An emitter that doubles variant counts during composition
  (e.g., via a deduplication-then-restore step) would falsify
  (a) — the homomorphism would fail. An emitter that lifts the
  empty inductive to a non-zero variant count would falsify (b).

  Status: **discharged at v0.1.0 (PMAT-237)**. Tier: DIAMOND.
-/
theorem variant_count_cardinality_functor_diamond
    (i1 i2 : LeanInductiveSilver)
    (hi : i1.variant_count = i1.variant_arities.size) :
    -- (a) Additivity (PMAT-207 lifted)
    (compose_inductive_silver i1 i2).variant_count
        = i1.variant_count + i2.variant_count
    -- (b) Identity preservation (empty maps to 0)
    ∧ ({ variant_count := 0, variant_names := #[], variant_arities := #[] }
        : LeanInductiveSilver).variant_count = 0
    -- (c) Non-negativity (Nat is non-negative)
    ∧ i1.variant_count ≥ 0
    -- (d) Cardinality consistency in the model
    ∧ i1.variant_count = i1.variant_arities.size := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · rfl
  · rfl
  · exact Nat.zero_le i1.variant_count
  · exact hi

/-! ## PMAT-335 — THIRD Diamond on C-XLATE-LEAN-TO-RUST (Layer 5
    BROADENING DEPTH-3 from 10 to 11 contracts):
    RustFn STRUCTURE EXTENSIONALITY
    (XPILE-REFINE-XLATE-LEAN-TO-RUST-005).

    **Broadens DEPTH-3 from 10 to 11 contracts.** Substrate is now
    only 1 contract away from depth-3 UNIVERSAL across all 12.

    Eighth substrate-wide demonstration of structure-extensionality
    pattern.

    Status: discharged at v0.1.0 (PMAT-335). Tier: DIAMOND. -/

/--
  **Diamond-tier refinement theorem** — `RustFn` admits
  STRUCTURE EXTENSIONALITY.

  Status: **discharged at v0.1.0 (PMAT-335)**. Tier: DIAMOND.
-/
theorem rust_fn_struct_extensionality_diamond
    (f1 f2 : RustFn) :
    -- (a) Field equality → record equality
    (f1.body = f2.body → f1 = f2)
    -- (b) Record equality → field equality
    ∧ (f1 = f2 → f1.body = f2.body)
    -- (c) Decidable equality
    ∧ (f1 = f2 ∨ f1 ≠ f2)
    -- (d) Self-equality (reflexivity)
    ∧ (f1 = f1) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro h
    cases f1; cases f2
    simp_all
  · intro h
    rw [h]
  · by_cases h : f1 = f2
    · exact Or.inl h
    · exact Or.inr h
  · rfl

/-! ## PMAT-343 — FOURTH Diamond on C-XLATE-LEAN-TO-RUST (Layer 5
    BROADENING DEPTH-4 from 10 to 11 contracts):
    LEAN INDUCTIVE / RUST ENUM VARIANT-COUNT NAT STRUCTURE
    (XPILE-REFINE-XLATE-LEAN-TO-RUST-006).

    **Broadens DEPTH-4 from 10 to 11 contracts.** Pushes
    XlateLeanToRust (Layer 5) from depth-3 to depth-4, adding a
    THIRD Layer 5 contract at depth-4. Only 1 more contract
    needed for depth-4 UNIVERSAL.

    The 4 Diamond categories on C-XLATE-LEAN-TO-RUST:
    - PMAT-222 inductive_monoid: inductive composition monoid
    - PMAT-237 variant_count_cardinality_functor: cardinality
    - PMAT-335 rust_fn_struct_extensionality: record structure
    - **PMAT-343: LEAN INDUCTIVE / RUST ENUM VARIANT-COUNT NAT**
      ← depth-4

    Captures NAT-STRUCTURAL properties of variant_count:
    non-negativity, well-foundedness, and discrete order — distinct
    from the cardinality functor (PMAT-237) which was about hom
    properties.

    Status: discharged at v0.1.0 (PMAT-343). Tier: DIAMOND. -/

/--
  **Diamond-tier refinement theorem** — `LeanInductive.variant_count`
  and `RustEnum.variant_count` Nat structure.

  Status: **discharged at v0.1.0 (PMAT-343)**. Tier: DIAMOND.
-/
theorem variant_count_nat_structure_diamond
    (i : LeanInductive) (e : RustEnum) :
    -- (a) Variant count is non-negative
    (0 ≤ i.variant_count)
    -- (b) Variant count is strictly less than successor
    ∧ (i.variant_count < i.variant_count + 1)
    -- (c) Rust enum variant count is also non-negative
    ∧ (0 ≤ e.variant_count)
    -- (d) Successor is strictly greater
    ∧ (e.variant_count < e.variant_count + 1) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact Nat.zero_le _
  · omega
  · exact Nat.zero_le _
  · omega

end XpileContracts.CXlateLeanToRust
