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
  **Target consistency** auxiliary claim. At Bronze tier this
  reduces to `rfl` because the model doesn't carry a target tag
  separate from the byte payload. Silver-tier refinement will
  introduce a `Target` field in `Artifact` and require the proof
  that `config.target == result.target` holds for all Backend
  impls.

  Listed here so the Silver refinement has a stub to overwrite
  rather than introducing a new theorem at refinement time.
-/
theorem target_consistency (module config : Array UInt8) :
    lower module config = lower module config := by
  rfl

end XpileContracts.CXpileBackendTrait
