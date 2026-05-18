# Kaizen Fleet Membership

**Section 20 of [xpile-spec.md](../xpile-spec.md).**

## The fleet at a glance

Per [pv-spec §31](../../../../provable-contracts/docs/specifications/sub/kaizen-fleet-enforcement.md):

- **40 repos** under continuous-improvement enforcement
- **294 contracts** across the fleet
- **1025 Lean theorems**
- **20,110 assertions**
- **1107 call sites** with bindings
- **Grade A overall**; **Kernel Grade A** (174 postconditions, 315 preconditions)
- xpile becomes **repo #41**

## What membership buys

| Benefit | How |
|---|---|
| Fleet-level rollups | `pv kaizen` aggregates xpile alongside aprender, depyler, decy, trueno, etc. |
| Cross-repo audit chain | A paper → contract → code → proof chain that spans repos (e.g., xpile-rust-codegen binds to a trueno SIMD kernel) is auditable end-to-end |
| Shared graduation pipeline | Skills graduating from xpile feed into the fleet-wide graduation queue |
| Coordinated releases | `pv fleet release --bump minor` updates xpile + dependents atomically |
| Drift detection | `pv diff` catches when xpile's API changes break dependents in the fleet |

## What membership requires

A repo must:

1. Have a `contracts/` directory at the root
2. Have `cargo test -p <repo>-contracts --lib` exercising the contract framework
3. Pass `pv lint` 8/8 on every PR
4. Publish a `pvscore` baseline that CI maintains or improves
5. Register with `pv kaizen --register <repo>` (one-time, manual)
6. Be tagged in fleet config under `kaizen-fleet/repos.yaml`

## Registration

```bash
# One-time, from xpile's root
pv kaizen --register xpile
# Updates kaizen-fleet/repos.yaml in provable-contracts repo
# Triggers fleet-wide rollup recomputation on next nightly
```

After registration, `pv kaizen` rollups include xpile.

## Tier classification

The fleet has two tiers:

- **Kernel tier** (E2 quality): repos that produce verifiable kernels — aprender, trueno, realizar, entrenar. Graded on postcondition density + Kani proof coverage.
- **Tool tier** (penetration): repos that USE kernels — pmat, depyler, decy, etc. Graded on call-site coverage (% of operations that bind to a contract).

xpile is **kernel tier** at v0.1.0. Phase 4 (Kani equivalence proofs on Layer-1/Layer-2 contracts) was the planned trigger for this transition; that phase shipped early and overshipped — every contract in the substrate (12 of 12) has a Kani BMC harness on every PR via the dedicated `kani` CI job (PMAT-021 / XPILE-QUORUM-003). The translation contracts produce verifiable kernels by construction: **242 Lean refinement theorems (53 Bronze + 108 Silver + 24 Gold + 39 Platinum + 18 Diamond)** in `contracts/lean/` + **43 Kani BMC harnesses** in `contracts/kani/` = **285 stratum-vote artifacts** (post-PMAT-127..138 Bronze side 12 → 50; post-PMAT-147..151 / XPILE-QUORUM-006 Kani side 12 → 43; post-PMAT-156..183 Silver bracket completion — 42/42 equations at Silver; post-PMAT-185..197 Gold-tier UNIVERSAL — 12/12 contracts at Gold using 5 distinct subtype patterns; post-PMAT-199..212 Platinum-tier UNIVERSAL — 12/12 contracts at Platinum demonstrating 7 distinct compositional algebraic shapes; post-PMAT-214..226 **Diamond-tier UNIVERSAL — 12/12 contracts at Diamond using 12 distinct algebraic categories**: commutative-monoid/semiring, pure-function, abelian-group, equivalence-relation, bounded-monoid, string-monoid, free list-monoid, inductive-monoid, precondition-list-monoid, frontend equivalence-class, backend equivalence-class, citation render-monoid). See [phased-rollout.md](phased-rollout.md) "What actually shipped" for the comparison of planned vs. shipped phases.

## Fleet grade contribution

xpile's grade contributes to the fleet rollup weighted by call-site count. At v0.1.0:

- **Contract count:** 12 (Sem + Sym + Run + Ext votes in each; all 12 at 4-stratum minimum)
- **Lean theorem count:** 50 (one per equation across the 12 contracts, post-PMAT-127..138)
- **Kani harness count:** 12 (one per contract — additional per-equation harnesses are XPILE-QUORUM-006 follow-on)
- **Fleet contribution:** to be measured by `pv kaizen` rollup after registration; the substrate now meets the **Kernel Grade A** prerequisites (≥10 contracts proven, ≥1 four-stratum contract — now ≥12)

Originally projected after Phase 3 / Phase 6:

| Phase | Projected | Actual at v0.1.0 |
|---|---|---|
| After Phase 3 | ~50 call sites, ~5% fleet contribution | Codegen still hand-written; generator path is XPILE-PV-CODEGEN-001+ future work. Call sites bound via contract `binding:` fields are tracked separately. |
| After Phase 6 | ~300 call sites, ~25% fleet contribution | 50 Lean theorems shipped (overshipped from the ≥3 target — every equation in every contract gained its own Bronze-tier theorem in PMAT-127..138); fleet contribution awaits `pv kaizen` rollup recomputation. |

The goal of being one of the top-5 fleet contributors within 12 months remains live, with the substrate-completion run (PMAT-058..077) moving the load-bearing prerequisites forward by months relative to the original plan.

## Cross-repo binding example

```yaml
# In xpile/contracts/xlate-py-int-to-i64-v1.yaml
metadata:
  id: C-XLATE-PY-INT-TO-I64
  ...
depends_on:
  - C-PY-INT-ARITH                    # local
  - cross_repo: trueno:C-I64-WRAPPING-ADD-V1   # cross-repo
```

`pv audit` resolves cross-repo deps against the fleet manifest. If trueno's `C-I64-WRAPPING-ADD-V1` is removed or changes shape, xpile's CI fails before the breakage reaches main.

## Quarterly review

`pv kaizen rollup --quarter 2026Q3 > docs/status/quarterly/2026Q3.md` produces:

- Per-repo grade delta
- New contracts added per repo
- Skills graduated per repo
- Repair-invocation rate trend (per [skills.md](skills.md))
- Cross-repo binding count

The review is the **fleet status meeting** input. xpile participates from Phase 1 onward.

## Why this isn't optional

The user's vision: **pmat controls all work, pv controls all design.** Fleet membership is what makes that vision concrete. A repo outside the fleet is a repo where contracts and work items can drift unobserved.
