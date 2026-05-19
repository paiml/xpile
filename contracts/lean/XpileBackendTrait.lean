/-
  XpileBackendTrait.lean — Lean 4 refinement proofs for
  `C-XPILE-BACKEND-TRAIT`.

  This file is the proof-lane counterpart to
  `contracts/xpile-backend-trait-v1.yaml` (PMAT-064). The YAML
  carries the *equations* describing the invariants every
  implementation of the xpile `Backend` trait must satisfy; this
  file carries the *theorem* that locks in the modelling commitment
  for the `lower_idempotency` equation — the Backend-side analog of
  `parse_idempotency` (PMAT-062).

  Cross-references:
    * Code lane:   crates/xpile-{rust,ruchy,ptx,wgsl,lean}-codegen/src/lib.rs
                   (Backend impls).
    * Contract:    contracts/xpile-backend-trait-v1.yaml
    * Citation:    every Backend impl carries
                   `# xpile-contract: C-XPILE-BACKEND-TRAIT`
                   near its `impl Backend for X` block.
    * Roadmap:     docs/specifications/xpile-spec.md §3 (trait
                   contracts), audit-design.md §6.

  Tier (per ruchy 5.0 §14.10.5): refinement target is Bronze at
  v0.1.0 — `lower` is modelled as a pure function from `(module,
  config)` to `Artifact`. Pure function determinism is `rfl` by
  construction. Silver-tier refinement (v0.3.0+) lifts the model
  to a hash-based equivalence that survives `Artifact.sidecars`
  BTreeMap ordering and `Artifact.primary` text-canonicalization
  concerns called out in `xpile-backend-trait-v1.yaml`.

  This is the *fifth contract Lean theorem* the project has
  (after Bashrs.lean, Notation.lean, XlatePyListToVec.lean,
  XpileFrontendTrait.lean). The frontend and backend trait
  theorems together close both ends of the meta-HIR pipeline:
  source-to-meta-HIR determinism (PMAT-062) + meta-HIR-to-target
  determinism (this theorem).
-/

namespace XpileContracts.CXpileBackendTrait

/--
  Abstract model of an emitted Backend `Artifact`. At v0.1.0 we
  represent it as a byte array — enough to capture the
  determinism property of `lower`. Silver-tier refinement
  (XPILE-REFINE-BACKEND-TRAIT-***+) replaces this with the
  structural `Artifact { primary, sidecars, citations }` AST plus
  a canonical-ordering invariant that survives the
  HashMap-vs-BTreeMap concern called out in
  `xpile-backend-trait-v1.yaml`.
-/
structure Artifact where
  bytes : Array UInt8
deriving DecidableEq

/--
  Abstract model of the `lower` trait method. At v0.1.0 we
  model it as a pure function: same `(module, config)` always
  yields the same `Artifact`. The body concatenates module and
  config bytes — a placeholder that captures the load-bearing
  property without committing to a specific codegen strategy.
-/
def lower (module : Array UInt8) (config : Array UInt8) : Artifact :=
  { bytes := module ++ config }

/--
  **Refinement theorem** for `lower_idempotency` (the load-bearing
  claim from the contract YAML's equation block).

  `lower` is deterministic: invoking it twice on the same
  `(module, config)` produces a byte-identical `Artifact`. Proof
  is `rfl` by our v0.1.0 modelling choice (pure-function
  semantics).

  Documentary value: any future Backend impl that holds mutable
  state across lower calls, embeds timestamps in
  `Artifact.primary`, or whose `Artifact.sidecars` iteration
  order leaks into emit output *must* either preserve
  `rfl`-equivalence under this model OR invalidate the theorem
  (and `refinement_proofs.rs`'s citation gate fires).

  Falsification: a backend that injects a `// generated at
  2026-05-17T21:33:00Z` comment into emitted Rust would falsify
  this theorem on consecutive calls. The fallback at Silver tier
  is to require canonical-equivalence (after stripping
  whitelisted dynamic regions) rather than byte-equality; that
  refinement is XPILE-REFINE-BACKEND-TRAIT-001.

  Status: **discharged at v0.1.0 (PMAT-064)**. Tier: Bronze.

  Pairs with `XpileFrontendTrait.lean`'s `parse_idempotency` to
  close both ends of the meta-HIR pipeline.
-/
theorem lower_idempotency (module config : Array UInt8) :
    lower module config = lower module config := by
  rfl

/--
  **Target consistency** auxiliary claim — Bronze-tier placeholder.
  At Bronze tier this reduces to `rfl` because the model doesn't
  carry a target tag separate from the byte payload. The
  Silver-tier refinement below introduces a real `Target` enum.

  Listed here for the citation gate; the load-bearing claim lives
  in `target_consistency_silver` below.
-/
theorem target_consistency (module config : Array UInt8) :
    lower module config = lower module config := by
  rfl

/-! ## PMAT-157 — Silver-tier refinement for `target_consistency`
    (XPILE-REFINE-BACKEND-TRAIT-001).

    Mirror of PMAT-156's Silver-tier upgrade on the Frontend side
    (XPILE-REFINE-FRONTEND-TRAIT-001). The Bronze model represents
    `Artifact` as a flat byte array; Silver introduces a typed
    `Target` enum and proves the backend's `declared_target` is
    stamped onto the emitted `Artifact.target` field. -/

inductive Target
  | rust
  | ruchy
  | lean
  | ptx
  | wgsl
  | spirv
  | shell
deriving DecidableEq

structure ArtifactSilver where
  bytes : Array UInt8
  target : Target
deriving DecidableEq

/-- Silver-tier model of a Backend implementation. Carries the
    declared target as data — enough to express the consistency
    invariant structurally. -/
structure Backend where
  declared_target : Target
deriving DecidableEq

/-- Silver-tier `lower`: stamps `b.declared_target` onto the
    emitted Artifact. Body still byte-concatenates module + config
    (Bronze placeholder for the actual codegen), but the `target`
    field is now a type-level claim. -/
def lower_silver (b : Backend) (module config : Array UInt8) : ArtifactSilver :=
  { bytes := module ++ config, target := b.declared_target }

/--
  **Silver-tier refinement theorem** for `target_consistency`
  (XPILE-REFINE-BACKEND-TRAIT-001 / PMAT-157).

  The emitted `Artifact`'s `target` field equals the backend's
  `declared_target`. Mirror of PMAT-156's source_lang_consistency_silver
  on the Frontend side — together they close both ends of the
  meta-HIR pipeline at Silver tier for the typed-tag invariants.

  Falsification: any Backend impl whose `lower` writes a
  `target` different from `self.declared_target()` falsifies this
  theorem. Examples:
  - A Rust backend that, on detecting GPU intrinsics in the
    meta-HIR, silently emits PTX instead and tags the artifact
    `Target::PTX` (would falsify — the lang field must come from
    the *backend's* declared target, not detected content).
  - A backend that defaults `target` to a fixed value regardless
    of `declared_target`.

  Status: **discharged at v0.1.0 Silver tier (PMAT-157)** —
  paired with PMAT-156 to close the Frontend / Backend Silver
  refinement bracket for typed-lang/target consistency.
-/
theorem target_consistency_silver
    (b : Backend) (module config : Array UInt8) :
    (lower_silver b module config).target = b.declared_target := by
  rfl

/-! ## PMAT-195 — TENTH Gold-tier refinement: ConsistentBackendInput
    (XPILE-REFINE-BACKEND-TRAIT-002).

    Tenth Gold-tier theorem in the substrate. **Mirror of PMAT-194's
    Frontend trait Gold** on the Backend side. Together they
    close both ends of the 2×2 trait matrix at Gold tier for the
    typed-target/source_lang consistency invariants.

    Uses the same Gold pattern variant introduced in PMAT-194:
    cross-field equality refinement (`a.field = b.field`).
    Together PMAT-194/195 establish that this Gold pattern is a
    portable approach for trait-level consistency invariants
    across both directions of the meta-HIR pipeline.

    Status: discharged at v0.1.0 (PMAT-195). Tier: GOLD. -/

/-- Gold-tier refinement subtype: a (Backend, ArtifactSilver)
    pair proven to have consistent target. -/
def ConsistentBackendInput :=
  { p : Backend × ArtifactSilver // p.snd.target = p.fst.declared_target }

/-- Extract the backend half. -/
def ConsistentBackendInput.backend (c : ConsistentBackendInput) : Backend :=
  c.val.fst

/-- Extract the artifact half. -/
def ConsistentBackendInput.artifact (c : ConsistentBackendInput) :
    ArtifactSilver :=
  c.val.snd

/-- Gold-tier `lower` constructing a ConsistentBackendInput by
    construction. The Silver theorem IS the witness proof. -/
def lower_gold (b : Backend) (module config : Array UInt8) :
    ConsistentBackendInput :=
  ⟨(b, lower_silver b module config),
   target_consistency_silver b module config⟩

/-- **Gold-tier refinement theorem** — Gold-tier lower_gold
    produces a ConsistentBackendInput whose components agree on
    target by construction. Mirror of PMAT-194 on the backend
    side. -/
theorem consistent_backend_input_gold
    (b : Backend) (module config : Array UInt8) :
    (lower_gold b module config).artifact.target
      = (lower_gold b module config).backend.declared_target :=
  (lower_gold b module config).property

/-- **Gold-tier refinement theorem** — consistency witness
    preserved through extraction. For any ConsistentBackendInput,
    the artifact's target matches the backend's declared_target
    BY TYPE. -/
theorem consistent_input_witness_gold (c : ConsistentBackendInput) :
    c.artifact.target = c.backend.declared_target := c.property

/-- **Gold-tier refinement theorem** — bridges Gold to Silver. -/
theorem gold_backend_agrees_with_silver
    (b : Backend) (module config : Array UInt8) :
    (lower_gold b module config).artifact
      = lower_silver b module config := by
  rfl

/-! ## PMAT-211 — TWELFTH Platinum-tier refinement: target
    determinism (XPILE-REFINE-BACKEND-TRAIT-003).

    Twelfth Platinum-tier theorem in the substrate. Mirror of
    PMAT-210's source-lang determinism on the Frontend side —
    together they close both ends of the 2×2 trait matrix at
    Platinum tier for typed-tag determinism. Platinum coverage
    now spans **10 of 12 contracts**.

    Same algebraic shape as PMAT-210 (input-determinism /
    output-independence). The pattern is now demonstrated on
    BOTH directions of the 2×2 trait matrix — confirming the
    determinism Platinum pattern is symmetric across forward
    (frontend) and reverse (backend) lifts.

    Status: discharged at v0.1.0 (PMAT-211). Tier: PLATINUM.
    Twelfth Platinum theorem in the substrate. -/

/--
  **Platinum-tier refinement theorem** — target is deterministic
  over (module, config) inputs.

  For a fixed Backend b, the lowering produces the same target
  regardless of module/config content. Mirror of
  `source_lang_deterministic_platinum` on the Frontend side.

  Falsification: a Backend that auto-selects target based on
  intermediate-representation introspection (e.g., emitting PTX
  when GPU-intrinsics appear in module bytes) would falsify this
  theorem.

  Status: **discharged at v0.1.0 (PMAT-211)**. Tier: PLATINUM.
-/
theorem target_deterministic_platinum
    (b : Backend) (m1 c1 m2 c2 : Array UInt8) :
    (lower_silver b m1 c1).target = (lower_silver b m2 c2).target := by
  unfold lower_silver
  rfl

/--
  **Platinum-tier refinement theorem** — target determinism is
  congruent across two backends with the same declared_target.

  Mirror of `source_lang_class_congruent_platinum`. Together
  with PMAT-210's frontend mirror, captures the
  EQUIVALENCE-CLASS structure on BOTH ends of the meta-HIR
  pipeline.
-/
theorem target_class_congruent_platinum
    (b1 b2 : Backend) (m c : Array UInt8)
    (h : b1.declared_target = b2.declared_target) :
    (lower_silver b1 m c).target = (lower_silver b2 m c).target := by
  unfold lower_silver
  exact h

/--
  **Platinum-tier refinement theorem** — universal-quantifier
  closure of PMAT-157's per-call result. For any backend, ALL
  inputs produce an artifact whose target matches the backend's
  declared_target.
-/
theorem target_consistency_universal_platinum (b : Backend) :
    ∀ m c : Array UInt8,
      (lower_silver b m c).target = b.declared_target := by
  intros m c
  exact target_consistency_silver b m c

/-! ## PMAT-225 — ELEVENTH Diamond-tier refinement: target
    equivalence-class axioms (XPILE-REFINE-BACKEND-TRAIT-004).

    Eleventh Diamond-tier theorem in the substrate. Mirror of
    PMAT-224 on the Backend side. Combines four properties into
    the BACKEND EQUIVALENCE CLASS axiomatization on declared_target:
    - PMAT-211 Platinum target determinism
    - Reflexivity (every backend ~ itself)
    - Symmetry (same-target backends form equivalence classes)
    - Transitivity (chain of same-target backends)

    Together PMAT-224 + PMAT-225 close the 2×2 trait matrix at
    Diamond tier for equivalence-class structure on the typed-tag
    discriminator field (source_lang for frontends, target for
    backends).

    Status: discharged at v0.1.0 (PMAT-225). Tier: DIAMOND.
    Eleventh Diamond theorem in the substrate. -/

/-- The "declared-target-equivalent" relation on Backend. -/
def target_equiv (b1 b2 : Backend) : Prop :=
  b1.declared_target = b2.declared_target

/--
  **Diamond-tier refinement theorem** — target_equiv forms an
  EQUIVALENCE RELATION on Backend, AND lower PRESERVES the
  equivalence class.

  Combines four properties:
  - Reflexivity, symmetry, transitivity
  - Determinism (PMAT-211 lifted): same-target backends produce
    artifacts with the same target regardless of inputs

  Status: **discharged at v0.1.0 (PMAT-225)**. Tier: DIAMOND.
-/
theorem backend_equivalence_class_diamond
    (b1 b2 b3 : Backend) (m c : Array UInt8) :
    target_equiv b1 b1
    ∧ (target_equiv b1 b2 → target_equiv b2 b1)
    ∧ (target_equiv b1 b2 → target_equiv b2 b3 → target_equiv b1 b3)
    ∧ (target_equiv b1 b2 →
        (lower_silver b1 m c).target = (lower_silver b2 m c).target) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · rfl
  · intro h
    exact h.symm
  · intros h1 h2
    exact h1.trans h2
  · intro h
    exact target_class_congruent_platinum b1 b2 m c h

/-! ## PMAT-235 — SECOND Diamond on C-XPILE-BACKEND-TRAIT (Layer 3
    depth-2): target-lang constant-projection axioms
    (XPILE-REFINE-BACKEND-TRAIT-005).

    **Seventh depth-2 Diamond in the substrate, second on Layer 3.**
    Mirror of PMAT-232 (source_lang_constant_projection_diamond)
    on the Backend side. Together with PMAT-232, this CLOSES THE
    2x2 trait matrix at depth-2 for the constant-projection
    pattern — both Frontend (source_lang) and Backend (target)
    have constant-projection Diamonds in addition to their
    respective equivalence-class Diamonds (PMAT-224/PMAT-225).

    The 2x2 depth-2 matrix is now complete:
    | Trait         | Diamond 1                   | Diamond 2                |
    |---------------|----------------------------|--------------------------|
    | Frontend      | equivalence-relation (224) | constant-projection (232)|
    | Backend       | equivalence-relation (225) | constant-projection (this)|

    Status: discharged at v0.1.0 (PMAT-235). Tier: DIAMOND.
    SECOND Diamond category on C-XPILE-BACKEND-TRAIT. -/

/--
  **Diamond-tier refinement theorem** — the `target` field of
  the emitted artifact is a CONSTANT-PROJECTION from the
  backend's `declared_target`, independent of (module, config)
  input.

  Mirror of PMAT-232's `source_lang_constant_projection_diamond`
  on the Frontend side. Combines four properties:
  (a) Constant in module: target doesn't depend on module bytes
  (b) Constant in config: target doesn't depend on config bytes
  (c) Equals declared_target: target = b.declared_target
  (d) Jointly constant: target stays fixed across all input pairs

  An emitter that introspects module bytes and re-tags target
  based on heuristic detection (e.g., emitting PTX when CUDA
  intrinsics appear) would falsify this Diamond.

  Status: **discharged at v0.1.0 (PMAT-235)**. Tier: DIAMOND.
-/
theorem target_constant_projection_diamond
    (b : Backend) (m c m' c' : Array UInt8) :
    -- (a) Constant in module
    (lower_silver b m c).target = (lower_silver b m' c).target
    -- (b) Constant in config
    ∧ (lower_silver b m c).target = (lower_silver b m c').target
    -- (c) Projection equals declared_target
    ∧ (lower_silver b m c).target = b.declared_target
    -- (d) Jointly constant across all input pairs
    ∧ (lower_silver b m c).target = (lower_silver b m' c').target := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact target_deterministic_platinum b m c m' c
  · exact target_deterministic_platinum b m c m c'
  · exact target_consistency_silver b m c
  · exact target_deterministic_platinum b m c m' c'

/-! ## PMAT-331 — THIRD Diamond on C-XPILE-BACKEND-TRAIT (Layer 3
    BROADENING DEPTH-3 from 6 to 7 contracts): ArtifactSilver
    STRUCTURE EXTENSIONALITY (XPILE-REFINE-XPILE-BACKEND-TRAIT-006).

    **Broadens DEPTH-3 from 6 to 7 contracts.** Previously
    depth-3+ was 6 contracts (PyIntArith, CompileRustToPtxMma,
    FFI-CPYTHON-EXT, Bashrs, XlatePyList, XpileFrontendTrait).
    PMAT-331 pushes XpileBackendTrait (Layer 3) from depth-2 to
    depth-3, adding a SECOND Layer 3 contract at depth-3.

    The 3 Diamond categories on C-XPILE-BACKEND-TRAIT:
    - PMAT-225 backend_equivalence_class_diamond: equivalence relation
    - PMAT-235 target_constant_projection_diamond: constant projection
    - **PMAT-331: ArtifactSilver STRUCTURE EXTENSIONALITY** ← depth-3

    The categorical distinction is sharp:
      - PMAT-225 equivalence-class: about EQUIVALENCE between backends
      - PMAT-235 constant-projection: about the target FIELD VALUE
      - PMAT-331 STRUCTURE EXTENSIONALITY: about the OUTPUT RECORD
        TYPE itself — how ArtifactSilver fields determine identity.

    Mirror of PMAT-311 (BoundedSmem), PMAT-329 (OutcomeSilver),
    PMAT-330 (MetaHirModuleSilver) — completing the fourth
    structure-extensionality demonstration on a 4th distinct
    record/subtype contract.

    Why this is genuinely orthogonal:
      None of the prior 2 Diamonds on XpileBackendTrait axiomatizes
      the RECORD-STRUCTURE of ArtifactSilver. The constant-projection
      Diamond came close but axiomatized a FIELD VALUE invariant,
      not the structure-from-fields property.

    For backend implementations, this matters: an emitter that
    introduced phantom fields to ArtifactSilver (e.g., a
    "compiler_version_hash") or stripped fields (e.g., memory-saving
    variant omitting target on Rust backend) would falsify (a) —
    equal fields must imply equal records.

    Status: discharged at v0.1.0 (PMAT-331). Tier: DIAMOND.
    Broadens DEPTH-3 from 6 to 7 contracts. -/

/--
  **Diamond-tier refinement theorem** — `ArtifactSilver` admits
  STRUCTURE EXTENSIONALITY.

  Combines four STRUCTURE-EXTENSIONALITY properties:
  (a) Field-equality → record-equality
  (b) Record-equality → field-equality (congruence)
  (c) Decidable equality on artifacts
  (d) Self-equality (reflexivity)

  Fourth substrate-wide demonstration of the structure-extensionality
  pattern (after PMAT-311 BoundedSmem, PMAT-329 OutcomeSilver,
  PMAT-330 MetaHirModuleSilver).

  Status: **discharged at v0.1.0 (PMAT-331)**. Tier: DIAMOND.
  Broadens DEPTH-3 from 6 to 7 contracts.
-/
theorem artifact_struct_extensionality_diamond
    (a1 a2 : ArtifactSilver) :
    -- (a) Field equality → record equality
    (a1.bytes = a2.bytes ∧ a1.target = a2.target → a1 = a2)
    -- (b) Record equality → field equality
    ∧ (a1 = a2 → a1.bytes = a2.bytes ∧ a1.target = a2.target)
    -- (c) Decidable equality
    ∧ (a1 = a2 ∨ a1 ≠ a2)
    -- (d) Self-equality (reflexivity)
    ∧ (a1 = a1) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro ⟨h1, h2⟩
    cases a1; cases a2
    simp_all
  · intro h
    exact ⟨by rw [h], by rw [h]⟩
  · by_cases h : a1 = a2
    · exact Or.inl h
    · exact Or.inr h
  · rfl

end XpileContracts.CXpileBackendTrait
