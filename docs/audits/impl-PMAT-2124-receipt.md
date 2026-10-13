# impl receipt — PMAT-2124, v0.1.619 plan row C1 (ledger reconciliation)

## The row (verbatim, `docs/specifications/sub/release-v0.1.619-plan.md` §2)

> **C1 — ledger reconciliation.** PMAT-2131, PMAT-2133, PMAT-2136 and PMAT-2141 (epic #2124) and PMAT-2151, PMAT-2154, PMAT-2157, PMAT-2159, PMAT-2161 and PMAT-2163 (epic #2125) are `inprogress` with merged PRs.
> done_when: Each row reads `done` with the merged sha in `notes`, and that sha is an ancestor of `origin/main`. `pmat work status <id>` exits 0 for each. Its issue is closed.

## What the diff does

Only `docs/roadmaps/roadmap.yaml` changes, and only those ten rows: `status: inprogress` becomes `status: done`, `updated:` is set to the edit time, and `notes` gains one sentence naming the full merged sha and the PR. No other row, no code, no test, no workflow.

| id | merged sha | PR |
|---|---|---|
| PMAT-2131 | 5c0739ca | #2132 |
| PMAT-2133 | dffc91b5 | #2134 |
| PMAT-2136 | 4b1b9b44 | #2137 |
| PMAT-2141 | dc6ea146 | #2143 |
| PMAT-2151 | 19b9d68b | #2152 |
| PMAT-2154 | dbbe8a51 | #2156 |
| PMAT-2157 | cf52f3bb | #2158 |
| PMAT-2159 | 29ccf87a | #2160 |
| PMAT-2161 | 3eafb209 | #2162 |
| PMAT-2163 | d7b86815 | #2164 |

## Evidence (command output)

- Each sha: `git merge-base --is-ancestor <sha> origin/main` exit 0 (origin/main = b374a35b).
- `pmat work status <id>` exit 0 for all ten; PMAT-2131 reads `Status: Completed`.
- `gh issue list` state: all ten issues CLOSED.
- On a build host: `cargo test -p xpile --no-fail-fast` over the ten test files that read the roadmap (roadmap_ledger_parses, roadmap_pmat_schema, ledger_integrity_witness, release_plan_freshness_witness, release_story_derivation_witness, quorum_fixture_evidence_witness, packaged_contracts, derived_corpus_vacuity_witness, cli_attestation_tally_witness, cli_reporter_vacuity_witness): 10 of 10 binaries `ok`, 53 tests passed, 0 failed. `cargo fmt --all -- --check` clean.

## Refute it

A row whose sha is not an ancestor of origin/main, whose sha is not the PR named, or whose issue is open fails the row. So does any change outside the ten rows.
