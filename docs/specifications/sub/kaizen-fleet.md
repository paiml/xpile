# Kaizen Fleet Membership

**Section 20 of [xpile-spec.md](../xpile-spec.md).**

> ⚠️ **Six of the eight executable claims below were false against the installed
> `pv` when this page was measured on 2026-07-30 (PMAT-1497), and two of the six
> are the entire membership procedure.** This file was written on 2026-05-15
> (`cdcece9c`, the initial commit) against a `pv` CLI surface that either changed
> or never existed; the pinned toolchain today is **`pv 0.49.0`**. Nothing in this
> repository runs any of these commands — CI invokes `pv lint contracts/` and
> nothing else — so no gate could have caught the drift, and none did. Each claim
> below now carries the command that was run and the answer that came back;
> **re-run them before believing any of them**, because the subject is a
> third-party binary, not a file in this tree.
>
> | Published claim | Probe | Result at `pv 0.49.0` |
> |---|---|---|
> | `pv kaizen --register <repo>` (requirement 5 + "Registration" + `xpile-spec.md` §20) | `pv kaizen --register xpile` | `error: unexpected argument '--register' found` |
> | `pv kaizen rollup --quarter <Q>` ("Quarterly review") | `pv kaizen rollup --quarter 2026Q3` | `error: unexpected argument 'rollup' found` |
> | `pv fleet release --bump minor` ("What membership buys") | `pv fleet release --bump minor` | `error: unrecognized subcommand 'fleet'` |
> | "`pv kaizen` aggregates xpile alongside aprender, depyler, decy, trueno" | `pv kaizen --dry-run` in this repo root | `error: no repos found with binding.yaml and sibling directory` |
> | "`pv audit` resolves cross-repo deps against the fleet manifest" | `pv audit --help` | takes **one** `<CONTRACT>` file; flags are `--binding/--coq/--flux`; no fleet, no cross-repo resolution |
> | "`pv diff` catches when xpile's API changes break dependents in the fleet" | `pv diff --help` | takes `<OLD> <NEW>` **contract YAML files** and suggests a semver bump; no repo or fleet notion |
> | "Pass `pv lint` 8/8 on every PR" (requirement 3) | `pv lint contracts/` | 8 gates exist; **7 run, Gate 7 `reverse-coverage` is skipped** (`no --binding or --crate-dir provided`). Result `PASS`, 0 errors |
> | "Have a `contracts/` directory at the root" (requirement 1) | `ls contracts/` | ✅ true |
>
> ⛔ **And the one requirement that is entirely inside this repository — the only
> one a gate here could ever have checked — is discharged by a green command that
> runs nothing.** Requirement 2 asks for
> `cargo test -p <repo>-contracts --lib` *"exercising the contract framework"*.
> `cargo test -p xpile-contracts --lib` **exits 0 with `0 tests`**;
> `crates/xpile-contracts/src/lib.rs` is 118 lines carrying **zero `#[test]`**.
> A requirement written as *a command that passes* rather than *a property that
> holds* is satisfied by an empty crate, and this one has read as met for
> seventy-six days.
>
> **Nothing in this repository depends on fleet membership** — no CI job, no
> gate, no emitted artefact. This is a claim defect, not a capability gap. But
> §20 of the canonical spec tells a reader xpile *"becomes repo #41 once … `pv
> kaizen --register xpile` is run"*, i.e. that membership is one command away.
> It is not one command away; at `pv 0.49.0` that command does not exist.

## The fleet at a glance

Per pv-spec §31, **as transcribed on 2026-05-15 and never re-read since**. These
are figures about a document in another repository; this tree holds no copy of
pv-spec, so nothing here can confirm or refute them, and they should be read as
*what pv-spec said on the day this file was written*, not as the fleet's state
today (PMAT-1497):

- **40 repos** under continuous-improvement enforcement
- **294 contracts** across the fleet
- **1025 Lean theorems**
- **20,110 assertions**
- **1107 call sites** with bindings
- **Grade A overall**; **Kernel Grade A** (174 postconditions, 315 preconditions)
- xpile becomes **repo #41**

## What membership buys

**This table is the 2026-05-15 prospectus and three of its five mechanisms do not
exist at `pv 0.49.0`** — see the ⚠️ block at the top of this file for the probes.
Kept rather than deleted, because deleting it would erase the record that these
benefits were once published as available:

| Benefit | How (as published 2026-05-15) | Measured 2026-07-30 |
|---|---|---|
| Fleet-level rollups | `pv kaizen` aggregates xpile alongside aprender, depyler, decy, trueno, etc. | ⛔ `pv kaizen --dry-run` in this repo → `error: no repos found with binding.yaml and sibling directory`. It does not run here at all, let alone aggregate |
| Cross-repo audit chain | A paper → contract → code → proof chain that spans repos (e.g., xpile-rust-codegen binds to a trueno SIMD kernel) is auditable end-to-end | ⚠️ unverified — no tool in this tree resolves a cross-repo chain; see "Cross-repo binding example" below |
| Shared graduation pipeline | Skills graduating from xpile feed into the fleet-wide graduation queue | ⚠️ unverified — no local surface |
| Coordinated releases | `pv fleet release --bump minor` updates xpile + dependents atomically | ⛔ `error: unrecognized subcommand 'fleet'` — `pv` has no `fleet` command |
| Drift detection | `pv diff` catches when xpile's API changes break dependents in the fleet | ⛔ `pv diff <OLD> <NEW>` diffs two contract **YAML files** and suggests a semver bump. It has no repo, dependent, or fleet notion |

## What membership requires

A repo must:

1. Have a `contracts/` directory at the root — ✅ **met**, 35 contracts.
2. Have `cargo test -p <repo>-contracts --lib` exercising the contract framework
   — ⛔ **the command is green and exercises nothing.** `crates/xpile-contracts`
   exists, `cargo test -p xpile-contracts --lib` **exits 0 running `0 tests`**,
   and `crates/xpile-contracts/src/lib.rs` is 118 lines carrying **zero
   `#[test]`** (all targets, not just `--lib`: `0 tests`). This is the only
   requirement on the list that is entirely inside this repository and therefore
   the only one any gate here could have checked; it has read as met since
   2026-05-15 because the requirement is spelled as a **command that passes**
   rather than as a **property that holds**. What actually exercises the
   substrate is `cargo test --workspace` (`crates/xpile/tests/`) plus
   `pv lint contracts/` — neither of which this list names.
3. Pass `pv lint` 8/8 on every PR — ⚠️ **7/8, not 8/8.** `pv lint contracts/`
   defines eight gates and reports `PASS` with 0 errors, but **Gate 7
   (`reverse-coverage`) is skipped** — `no --binding or --crate-dir provided` —
   because this repo passes neither. The CI step (`.github/workflows/ci.yml`,
   the `gate` job) invokes exactly `pv lint contracts/`, so the skip is
   permanent until a `--binding` registry exists. "8/8" was never measured.
4. Publish a `pvscore` baseline that CI maintains or improves — ⛔ **not met,
   and this repo already says so 250 lines away.** No baseline is committed and
   no CI step computes one; [`ci-gates.md`](ci-gates.md) lists
   `pv score --no-regression-vs main` under **"Gates planned but not yet
   wired (post-v0.1.0)"** as `XPILE-CI-SCORE-001`, reason *"requires `pvscore`
   baseline maintenance"*. Two pages of this same spec tree, each locally
   consistent, disagreeing about whether a membership prerequisite is met — the
   only tell is reading them in one pass (PMAT-1482's shape).
5. Register with `pv kaizen --register <repo>` (one-time, manual) — ⛔
   **impossible at `pv 0.49.0`**: `error: unexpected argument '--register' found`.
6. Be tagged in fleet config under `kaizen-fleet/repos.yaml` — ⚠️ external to
   this repo; unverifiable from here.

**So the requirement list is 1 met, 1 not met, 1 impossible, and 3 either
mis-stated or unverifiable.** Whether xpile is in fact "repo #41" is not
knowable from this tree and is not asserted anywhere that this tree can check.

## Registration

⛔ **The block below does not run.** Preserved verbatim as the procedure that was
published from 2026-05-15 to 2026-07-30; see the ⚠️ table at the top of this file.

```bash
# One-time, from xpile's root
pv kaizen --register xpile          # ⛔ error: unexpected argument '--register' found
# Updates kaizen-fleet/repos.yaml in provable-contracts repo
# Triggers fleet-wide rollup recomputation on next nightly
```

`pv kaizen`'s actual surface at 0.49.0 is a **local** enforcement loop —
`--contract-dir`, `--src-root`, `--repo`, `--dry-run`, `--codegen`, `--fix`,
`--json`, `--min-score` — with no registration verb and no fleet argument.
Invoked here it exits on `error: no repos found with binding.yaml and sibling
directory`. There is therefore no known command that registers this repo, and
consequently no basis for the sentence this paragraph replaces ("After
registration, `pv kaizen` rollups include xpile"). Restoring the claim requires
finding the real command in `pv` and **running it**, not re-wording this page.

## Tier classification

The fleet has two tiers:

- **Kernel tier** (E2 quality): repos that produce verifiable kernels — aprender, trueno, realizar, entrenar. Graded on postcondition density + Kani proof coverage.
- **Tool tier** (penetration): repos that USE kernels — pmat, depyler, decy, etc. Graded on call-site coverage (% of operations that bind to a contract).

xpile is **kernel tier** at v0.1.0. Phase 4 (Kani equivalence proofs on Layer-1/Layer-2 contracts) was the planned trigger for this transition; that phase shipped early and overshipped — **24 of the 35** contracts carry a Kani BMC harness in `contracts/kani/`, run on every PR by the dedicated `kani` CI job (PMAT-021 / XPILE-QUORUM-003); the other eleven have none. The translation contracts produce verifiable kernels by construction: **489 Lean refinement theorems** in `contracts/lean/` + **95 Kani BMC harnesses** in `contracts/kani/` = **584 stratum-vote artifacts** (Bronze + Silver + Gold + Platinum + Diamond depth-1 UNIVERSAL; **Diamond depth-2 across all 12 contracts of the v0.1.0 substrate** PMAT-228..250 with CI-enforcement via PMAT-251 `diamond_coverage.rs` gate — a floor over that named cohort ever since R6/PMAT-475, not a universal one; **Diamond depth-3 across all 5 layers** PMAT-241..245; **Diamond depth-4 opened** on PyIntArith and CompileRustToPtxMma via PMAT-247..248; `xpile diamond` reports the live Diamond tally, PMAT-249). See [phased-rollout.md](phased-rollout.md) "What actually shipped" for the comparison of planned vs. shipped phases.

Through v0.1.617 the paragraph above published the 2026-05-18 snapshot as present-tense fact: `260 Lean refinement theorems (53 Bronze + 108 Silver + 24 Gold + 39 Platinum + 36 Diamond)`, `43 Kani BMC harnesses`, `303 stratum-vote artifacts`, and `every contract in the substrate (12 of 12) has a Kani BMC harness`. The tally understated the substrate roughly two-fold while the COVERAGE clause overstated it, asserting a harness for eleven contracts that have none. `crates/xpile/tests/claims_drift.rs` derives all four from `contracts/` (PMAT-1455).

## Fleet grade contribution

xpile's grade contributes to the fleet rollup weighted by call-site count. At v0.1.0:

- **Contract count:** 12 (Sem + Sym + Run + Ext votes in each; all 12 at 4-stratum minimum)
- **Lean theorem count:** 50 (one per equation across the 12 contracts, post-PMAT-127..138)
- **Kani harness count:** 12 (one per contract — additional per-equation harnesses are XPILE-QUORUM-006 follow-on)
- **Fleet contribution:** to be measured by `pv kaizen` rollup after registration; the substrate now meets the **Kernel Grade A** prerequisites (≥10 contracts proven, ≥1 four-stratum contract — now ≥12). ⛔ **Unreachable as written (PMAT-1497)** — there is no registration command and no rollup subcommand at `pv 0.49.0`, so this figure has never been produced and there is no known path to producing it. The same caveat applies to *"fleet contribution awaits `pv kaizen` rollup recomputation"* in the table below.

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

⛔ **The example above is illustrative and the enforcement sentence under it was
false three ways (PMAT-1497).** The paragraph read: *"`pv audit` resolves
cross-repo deps against the fleet manifest. If trueno's `C-I64-WRAPPING-ADD-V1`
is removed or changes shape, xpile's CI fails before the breakage reaches
main."* Measured 2026-07-30:

1. **The file does not exist.** `contracts/xlate-py-int-to-i64-v1.yaml` is not in
   this tree; the whole YAML block is a sketch, not a transcript.
2. **The command has no such capability.** `pv audit` takes exactly one
   `<CONTRACT>` path; its flags are `--binding`, `--coq`, `--flux`. There is no
   fleet-manifest argument and no cross-repo resolution. The single real
   `cross_repo:` in this tree is `cross_repo: aprender` in
   `contracts/compile-rust-to-ptx-mma-v1.yaml` — a bare repo name, not the
   `repo:CONTRACT-ID` shape the example teaches.
3. **The CI consequence cannot occur, and this spec tree already said so.**
   `pv audit` is invoked **nowhere** in this repository — CI runs `pv lint
   contracts/` and nothing else — and [`ci-gates.md`](ci-gates.md) lists
   `pv audit` (paper→proof traversal) under *"Gates planned but not yet wired
   (post-v0.1.0)"* as `XPILE-CI-PV-AUDIT-001`. So the sentence promising a CI
   failure is contradicted by a sibling page in the same directory, for the same
   reason requirement 4 is.

Kept as the *intended* design of a cross-repo binding. Nothing enforces it.

## Quarterly review

⛔ **This cadence has never run, and the command that would run it does not
exist (PMAT-1497).** The line published here from 2026-05-15 to 2026-07-30 was:

```bash
pv kaizen rollup --quarter 2026Q3 > docs/status/quarterly/2026Q3.md
#   ⛔ error: unexpected argument 'rollup' found
```

`pv kaizen` at 0.49.0 has no `rollup` subcommand and no `--quarter` flag, and it
does not run in this repo at all (`error: no repos found with binding.yaml and
sibling directory`). The destination is the register PMAT-1495 measured one day
earlier: **`docs/status/quarterly/` has never existed in this repository's
history** (`git log --all --diff-filter=A -- 'docs/status/quarterly*'` returns
zero commits). PMAT-1495 correctly reported that the register had no writer and
filed *"is the quarterly rollup a real commitment?"* as the owner decision
`quarterly-rollup-cadence`. **The mechanism half was one `--help` away: the
writer it names was never a command.** That does not settle the owner's
question — whether to promise quarterly rollups is still a choice about what
this project wants to commit to — but it removes one of the three options that
decision was written against, because "keep the row and write the rollup at the
2026-09-30 close" is not available with the tooling as it stands.

Had it run, it was specified to produce:

- Per-repo grade delta
- New contracts added per repo
- Skills graduated per repo
- Repair-invocation rate trend (per [skills.md](skills.md))
- Cross-repo binding count

The review was to be the **fleet status meeting** input, with xpile
participating from Phase 1 onward. No such review has been held or recorded in
this repository.

## Why this isn't optional

The user's vision: **pmat controls all work, pv controls all design.** Fleet membership is what makes that vision concrete. A repo outside the fleet is a repo where contracts and work items can drift unobserved.
