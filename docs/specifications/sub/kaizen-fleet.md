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

xpile is **tool tier** at v0.1.0 (no kernels of its own). Once Phase 4 lands (Kani equivalence proofs on Layer-1/Layer-2 contracts), xpile transitions to **kernel tier** because translation contracts produce verifiable kernels (the codegen functions).

## Fleet grade contribution

xpile's grade contributes to the fleet rollup weighted by call-site count. At v0.1.0:

- Call sites: 0 (no real bindings yet)
- Fleet contribution: 0%

After Phase 3:

- Call sites: ~50 (one per generated codegen function)
- Fleet contribution: ~5%

After Phase 6:

- Call sites: ~300 (each contract's emit function + each skill's invocation site)
- Fleet contribution: ~25%

The goal is for xpile to be one of the top-5 fleet contributors within 12 months.

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
