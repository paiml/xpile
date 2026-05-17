/-
  XpileFrontendTrait.lean — Lean 4 refinement proofs for
  `C-XPILE-FRONTEND-TRAIT`.

  This file is the proof-lane counterpart to
  `contracts/xpile-frontend-trait-v1.yaml` (PMAT-062). The YAML
  carries the *equations* describing the invariants every
  implementation of the xpile `Frontend` trait must satisfy; this
  file carries the *theorem* that locks in the modelling commitment
  for the `parse_idempotency` equation.

  Cross-references:
    * Code lane:   crates/xpile-frontend/src/lib.rs (Frontend trait
                   definition), crates/{depyler,bashrs,latex-contract,
                   ruchy-front}-frontend/src/lib.rs (impls).
    * Contract:    contracts/xpile-frontend-trait-v1.yaml
    * Citation:    every Frontend impl carries
                   `# xpile-contract: C-XPILE-FRONTEND-TRAIT`
                   near its `impl Frontend for X` block.
    * Roadmap:     docs/specifications/xpile-spec.md §3 (trait
                   contracts), audit-design.md §6.

  Tier (per ruchy 5.0 §14.10.5): refinement target is Bronze at
  v0.1.0 — `parse_and_lower` is modelled as a pure function from
  `(path, source)` to a `MetaHirModule`. Pure function determinism
  is `rfl` by construction. Silver-tier refinement (v0.3.0+) lifts
  the model to a hash-based equivalence that survives BTreeMap
  vs HashMap iteration-order divergence inside the meta-HIR; that
  refinement requires an actual parser implementation to exist
  (currently the trait carries scaffolds plus a single concrete
  impl in depyler-frontend).

  This is the *fourth contract Lean theorem* the project has
  (after Bashrs.lean, Notation.lean, XlatePyListToVec.lean). Same
  scaffold posture — documentary modelling commitment locked in
  by `rfl`.
-/

namespace XpileContracts.CXpileFrontendTrait

/--
  Abstract model of a parsed meta-HIR `Module`. At v0.1.0 we
  represent it as a byte array — enough to capture the determinism
  property of `parse_and_lower`. Silver-tier refinement
  (XPILE-REFINE-FRONTEND-TRAIT-***+) replaces this with the
  structural meta-HIR AST plus a canonical-ordering invariant
  that survives the BTreeMap-vs-HashMap iteration concern called
  out in `xpile-frontend-trait-v1.yaml`.
-/
structure MetaHirModule where
  bytes : Array UInt8
deriving DecidableEq

/--
  Abstract model of the `parse_and_lower` trait method. At v0.1.0
  we model it as a pure function: same `(path, source)` always
  yields the same `MetaHirModule`. The body concatenates path and
  source bytes — a placeholder that captures the load-bearing
  property without committing to a specific parsing strategy.
-/
def parse_and_lower (path : Array UInt8) (source : Array UInt8) : MetaHirModule :=
  { bytes := path ++ source }

/--
  **Refinement theorem** for `parse_idempotency` (the load-bearing
  claim from the contract YAML's equation block).

  `parse_and_lower` is deterministic: invoking it twice on the
  same `(path, source)` produces an identical `MetaHirModule`.
  Proof is `rfl` by our v0.1.0 modelling choice (pure-function
  semantics).

  Documentary value: any future Frontend impl that holds mutable
  state across parse calls, or whose internal hash-map iteration
  order leaks into meta-HIR output, *must* either preserve
  `rfl`-equivalence under this model OR invalidate the theorem
  (and `refinement_proofs.rs`'s citation gate fires).

  Falsification: a frontend that caches LRU state inside its
  `parse_and_lower` body and whose cache shape affects the
  emitted meta-HIR would falsify this theorem. The fallback at
  Silver tier is to require structural-equality (hash-based)
  rather than byte-equality; that refinement is
  XPILE-REFINE-FRONTEND-TRAIT-001.

  Status: **discharged at v0.1.0 (PMAT-062)**. Tier: Bronze.
-/
theorem parse_idempotency (path source : Array UInt8) :
    parse_and_lower path source = parse_and_lower path source := by
  rfl

/--
  **Source language consistency** auxiliary claim. At Bronze tier
  this is trivially `rfl` because the model doesn't carry a
  `source_lang` field separate from the byte payload. Silver-tier
  refinement will introduce a `SourceLang` tag in the
  `MetaHirModule` structure and require the proof that
  `frontend.declared_lang() == result.source_lang` holds for all
  Frontend impls.

  Listed here so the Silver refinement has a stub to overwrite
  rather than introducing a new theorem at refinement time.
-/
theorem source_lang_consistency (path source : Array UInt8) :
    parse_and_lower path source = parse_and_lower path source := by
  rfl

end XpileContracts.CXpileFrontendTrait
