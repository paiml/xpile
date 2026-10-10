# xpile release plan: v0.1.619 (core) and v0.1.620 (look-ahead)

**Status:** plan, drafted 2026-10-10. Epics: [#2124](https://github.com/paiml/xpile/issues/2124)
(v0.1.619) and [#2125](https://github.com/paiml/xpile/issues/2125) (v0.1.620). This file does not
create, move or delete a tag. A tag is cut only at release, after the clean-room build of the
tagged commit is green.

**Cadence:** one tagged release a week, cut by one steady worker. Each release has **CORE** rows
(they must be done, or re-deferred with a reason, before the tag) and **LOOK-AHEAD** rows (work
for the release after it, started only when CORE is clear and dropped first when time is short).

**CI is unchanged by this plan.** Every job keeps running on GitHub-hosted `ubuntu-latest`. A
worker's local compiles and test runs happen on dedicated build hosts, not on a workstation.

This file is superseded when v0.1.619 ships. The v0.1.620 plan replaces it and moves this table
into its "previous release" note.

---

## 1. Measured state (2026-10-10, `origin/main` = `d7b86815`)

Every figure below is a dated measurement. It comes with the command that reproduces it.

| fact | measured | command |
|---|---|---|
| newest tag | `v0.1.618`, tagged 2026-07-30 | `git tag -l 'v*' --sort=-v:refname \| head -1` |
| commits on main since that tag | 59 | `git rev-list --count v0.1.618..origin/main` |
| workspace version | `0.1.618` | `grep -m1 '^version' Cargo.toml` |
| `queue.yaml` `sprint.version` | `0.1.618` (stale: it names the shipped release) | `grep -A3 '^sprint:' docs/roadmaps/queue.yaml` |
| main CI at `d7b86815` | `ci` success, `book` success (2026-09-25) | `gh run list -R paiml/xpile -b main -L 2` |
| fmt, check, release witnesses at `d7b86815` | `cargo fmt --check` clean; `cargo check --workspace --all-targets` ok; `release_plan_freshness_witness`, `release_preflight_witness`, `release_story_derivation_witness`, `released_binary_crux` ok | run on a build host |
| open PRs | #2167 (nightly run of the full `ci` workflow on main, PMAT-2166) | `gh pr list -R paiml/xpile` |

**What this means.** Main is green, but nothing has been tagged in ten weeks. The rows of
**both** epics #2124 and #2125 landed on main from 2026-09-23 to 2026-09-25. So v0.1.619 will ship
the v0.1.620 proof work too. The ledger has not caught up: rows whose PRs merged still read
`inprogress`.

---

## 2. v0.1.619 — CORE: "Ledgers the tools can read, proofs that can block"

Target: tag cut Thursday 2026-10-15, publish Friday 2026-10-16.

| row | done_when (its own falsifier) |
|---|---|
| **C1 — ledger reconciliation.** PMAT-2131, PMAT-2133, PMAT-2136 and PMAT-2141 (epic #2124) and PMAT-2151, PMAT-2154, PMAT-2157, PMAT-2159, PMAT-2161 and PMAT-2163 (epic #2125) are `inprogress` with merged PRs. | Each row reads `done` with the merged sha in `notes`, and that sha is an ancestor of `origin/main`. `pmat work status <id>` exits 0 for each. Its issue is closed. |
| **C2 — night runs exist.** Land #2167, the nightly `schedule:` run of the full `ci` workflow on main (#2166). | The first scheduled run on main is recorded on epic #2124 by its run id. |
| **C3 — two-state CI (the remainder of #2119). This row adds no blocking check and removes no required one.** The live required set on `main` is `gate` and `workspace-test` (measured 2026-10-10 with `gh api repos/paiml/xpile/rules/branches/main`), and `gate` has no `needs:`. So seven `ci.yml` jobs run on every PR without being able to block it: `kani`, `lake-build`, `docs`, `wasi`, `lean-models`, `shader-validate` and `license-scan`. Under the owner's two-state rule, a check that cannot block is a night check. All seven therefore leave the pull-request path and run on the nightly schedule from C2. A red there opens or updates one issue with one owner, and that issue never holds a merge or a tag. `gate` and `workspace-test` do not change. | On epic #2124, record the `ci.yml` diff and the first PR run after it. That run shows only the required jobs running, no `continue-on-error` step left on the pull-request path, and the required set unchanged (the same `gh api` read). The first night run shows all seven running. Only the owner can make any of them blocking (L4). Depends on C2. |
| **C4 — repo hygiene (#2124 row; does not hold the tag).** Turn on `delete_branch_on_merge` and prune the merged branches. | The setting needs the repo owner, so this row is "waiting on a named owner decision", which epic #2124's acceptance allows. It is recorded on the epic and does not hold the tag. |
| **C5 — release mechanics.** Bump the version at every assignment site (it is several lines, see `docs/RELEASE.md`). Add the CHANGELOG section. Move `queue.yaml` `sprint` to `0.1.619`. | `docs/RELEASE.md` §2a tripwires exit 0. The full `cargo test --workspace` is green on a build host. The tag is cut only after C6 steps 1 and 2 are green. |
| **C6 — release gates, all before publish.** The order is fixed: (1) the clean-room build of the release commit is green; (2) the **pre-publish gate**: a binary that clean-room build produced from that commit is run through `released_binary_crux` (`XPILE_BIN` pointed at it) and the dogfood run, and both positive controls go RED. If (1) or (2) is red, nothing is tagged or published, and the release slips. (3) The tag is cut on that commit. (4) The crates are published. After the publish, the same checks are run once against `cargo install xpile --version 0.1.619 --locked`. That run is a record, not a gate. A red there opens an issue and is fixed by a new patch release. The tag is never moved. | Steps 1 and 2 are recorded on epic #2124 with their run ids, and the post-publish run is recorded beside them. The positive controls (`XPILE_CRUX_SELF_TEST=flip`, and a stub binary printing `0.1.618`) go RED at step 2. `docs/RELEASE.md` §5 steps 1–9 are recorded on the epic. |

## 3. v0.1.620 — LOOK-AHEAD: "The proofs are about xpile"

Target: the week after v0.1.619 (tag cut Thursday 2026-10-22). These rows start only once v0.1.619's CORE is clear.

| row | done_when |
|---|---|
| **L1 — re-verify #2125 on the released binary.** v0.1.619 ships #2125's rows (the Kani seam floor, the Lean↔shipped pilot, the PMAT-468 re-derive, PMAT-469 and the quorum pilot), because they landed before it. L1 adds no new proof work. It only re-checks them. | Each row's done_when is re-run against the shipped v0.1.619 tree and recorded with its sha. The release-specific CRUX row runs `xpile quorum` on a contract path shipped in the crate package, and its self-test (flip one stratum count) goes RED. |
| **L2 — a binary newer than the last tag (#2149).** | A nightly build publishes an installable `xpile` artifact from main. The dogfood run installs it, and `xpile --version` names main's sha. |
| **L3 — `xpile update` (#2153).** | The `update` subcommand and the daily update check exist, each with a RED arm. |
| **L4 — promotion requests.** | After three green nights in a row on main (C2 and C3), each of the seven night checks is put, one at a time, to the owner's decision process: should it block? Each yes names the check it retires. Until a yes is recorded, the check stays a night check. When the yes lands, #2119's "proofs that can block" is met for that check, and not before. |

## 4. Rules this plan keeps

- No tag is created, moved or deleted outside the release procedure. A published tag is never re-pointed.
- No `continue-on-error`, advisory or report-only state is added on the pull-request path; C3 moves every non-blocking PR job to the night run. No check becomes blocking without the owner's yes, three green nights on main and one check retired.
- A row is DONE only when its merged sha is an ancestor of `origin/main`.
- Epics #2124 and #2125 close **last**: after C1 is done, after the C6 records are on the epic, and after the tag. CORE rows record their evidence on the open epic.
- C3 narrows #2119. Its "the three proof jobs join the required set" is not done by this release. It moves to L4, and #2119 records that.
