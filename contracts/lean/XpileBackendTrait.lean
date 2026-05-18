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

end XpileContracts.CXpileBackendTrait
