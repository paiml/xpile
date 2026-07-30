# Status Index

This directory tracks where xpile is and what's next. The **single source of truth** is [`CURRENT.md`](CURRENT.md) — that's what a future session reads first.

> ⚠️ **Two of the four registers below have no writer, and this warning is the
> only thing that says so (PMAT-1495).** The `When written` column is measured,
> not aspirational: each cell states what actually keeps that row alive. A
> register with no writer does not announce its own age — an empty one reads as
> a clean bill of health, and a frozen table reads as a complete one. Derive the
> live picture from the commands below rather than from this page's contents.

## Conventions

| File | Purpose | Cadence as declared | **When written (measured)** |
|---|---|---|---|
| [`CURRENT.md`](CURRENT.md) | Live status: what's done, in-progress, next, blocked | Every session that changes the state | ✅ **LIVE.** Kept honest by `claims_drift.rs` (PMAT-1348 pins it as a POINTER file: no bare derived counts, no retired claims). The gate is why this one did not rot. |
| [`INDEX.md`](INDEX.md) (this file) | Map of all status documents | When a new session log lands | ⚠️ **NO WRITER.** Its stated trigger is a session log landing, and none has (see below), so the trigger has never fired. Edited only when some *other* slice happened to sweep it. |
| `YYYY-MM-DD-<topic>.md` | Per-session change log | At session end | ⛔ **ABANDONED 2026-05-18.** Superseded in practice by `CHANGELOG.md` + `docs/roadmaps/{queue,roadmap}.yaml`, which every session does write. Never struck from this table. |
| `quarterly/YYYY-Q?.md` | Quarterly rollup (from `pv kaizen`) | At quarter close | ⛔ **NEVER WRITTEN, AND UNWRITABLE.** The directory has never existed in this repository's history, and the writer this cell names does not exist: `pv kaizen rollup --quarter 2026Q3` → `error: unexpected argument 'rollup' found` at the pinned `pv 0.49.0` (PMAT-1497). |

Re-derive all four, rather than trusting the cells:

```sh
git log -1 --date=short --format='%ad %h' -- docs/status/CURRENT.md   # live?
git ls-files 'docs/status/20*.md'                                     # session logs that exist
git log --all --diff-filter=A -- 'docs/status/quarterly*' | wc -l      # 0 ⇒ never written
```

**Why no gate catches this.** `docs/status/INDEX.md` *is* in the strictest claim
corpus this repo has — `claim_pages()` in `crates/xpile/tests/claims_drift.rs`
names it explicitly, alongside `README.md` and `CLAUDE.md`, as "every prose page
whose subject is the system AS IT IS NOW". But every assertion over that corpus
hunts a **false or stale numeral**, and the two dead registers below contain no
numeral to falsify: one is empty and one is a frozen table whose figures are all
correct *about their own date*. **A drift gate keyed on counts is structurally
blind to drift in completeness** — and the page it is blind on is the one whose
entire subject is completeness. Filed as `XPILE-STATUSREG-001` in
`docs/roadmaps/queue.yaml` `next_lane` (the 2026-07-29 freeze bars a new
`tests/` file); it must derive each register's cadence from the tree, not assert
that a table is non-empty, since an honestly quiet register has no row either.

## Session log (historical record) — FROZEN 2026-05-15 .. 2026-05-18; **not** a record of this project's sessions

> The parenthetical above is **not decoration and must stay verbatim**: it is
> `HISTORICAL_MARKER` in `crates/xpile/tests/claims_drift.rs:331`, the
> heading-scoped token that exempts a dated record from the live-claim gates.
> The rows below assert a UNIVERSAL Diamond depth that the substrate no longer
> holds, and the marker is the only thing making that legal. This slice found
> out by rewriting the heading honestly and watching
> `docs_claim_no_universal_depth_the_substrate_does_not_hold` go red —
> **the phrase whose misleadingness is this section's defect is also the token
> that keeps its contents legal**, so correcting the prose revoked the
> exemption. Qualify around the marker; never drop it.

Both rows below were written in this project's first four days and the table has
not been appended to since. **It is a dated record of two sessions, not a record
of this project's sessions** — and for the whole interval in which that reading
became false, the bare heading `Session log (historical record)` said otherwise.
Every figure inside the rows is
preserved exactly as written and is true *of its own date*; none of them is a
statement about the tree today (the crate, contract and theorem counts in the
2026-05-18 row have all since moved). The real per-session record lives in
`CHANGELOG.md` and the two ledgers under `docs/roadmaps/`:

```sh
git log --format='%ad %h %s' --date=short          # every session, in order
git log --format='%ad' --date=short -1 -- docs/status/2026-05-18-substrate-completion.md
```

| Date | Session | Topic |
|---|---|---|
| 2026-05-15 | Initial scaffold | [2026-05-15-scaffold.md](2026-05-15-scaffold.md) — workspace creation, 14 crates, `pv` wiring, 4 example contracts, full docs/specifications tree |
| 2026-05-18 | Substrate completion + UNIVERSAL 5-tier + UNIVERSAL Diamond depth-2 (CI-enforced) + depth-3 UNIVERSAL across layers + depth-4 opened | [2026-05-18-substrate-completion.md](2026-05-18-substrate-completion.md) — PMAT-058..251: 12/12 contracts to §14.4 QUORUM + UNIVERSAL 5-tier refinement + UNIVERSAL Diamond depth-2 across all 12 contracts (CI-enforced via PMAT-251 `diamond_coverage.rs` gate) + UNIVERSAL Diamond depth-3 across all 5 layers + Diamond depth-4 opened on 2 contracts (260 Lean theorems = 53 Bronze + 108 Silver + 24 Gold + 39 Platinum + 36 Diamond + 43 Kani harnesses = 303 stratum-vote artifacts), PMAT-214..226 Diamond depth-1, PMAT-228..250 depth-2, PMAT-241..245 depth-3 across layers, PMAT-247..248 depth-4 opened (PyIntArith L1, CompileRustToPtxMma L5), PMAT-249 `xpile diamond` reporter, PMAT-251 Diamond CI gate |

## Quarterly rollups — EMPTY REGISTER, born 2026-05-18, never written

`docs/status/quarterly/` has never existed in this repository's history
(`git log --all --diff-filter=A -- 'docs/status/quarterly*'` is empty), and no
`pv kaizen` rollup has ever been committed under it.

**Date the register before reading its contents as history.** The line this
section replaces — *"(none yet — first quarterly rollup expected at 2026-Q3
close…)"* — was authored on **2026-05-18** (`be2c4620`, 06:29 CEST), six weeks
**before** the 2026-Q2 close on 2026-06-30 that the conventions table above
commits this directory to. So the one quarter boundary that has actually elapsed
inside this project's life was not deferred and not slipped: it was
*unrepresentable*, because the text already promised the **first** rollup at the
**next** close. An empty register plus a forward-looking deferral is a claim
surface with nothing in it to inspect and doubt, and it gets more persuasive the
longer it goes unwritten.

Emptiness here is therefore evidence about this register's **age and missing
writer**, and about nothing else. Whether the quarterly rollup is a real
commitment or should be struck from the conventions table is an **owner
decision** — `quarterly-rollup-cadence`, filed under `owner_decisions` in
`docs/roadmaps/queue.yaml`. Until it is made, do not read this section as a
statement that no quarter has closed.

⭐ **The missing writer has a name now, and it was one `--help` away (PMAT-1497,
2026-07-30).** The conventions table above sources this register from
`pv kaizen`, and [`../specifications/sub/kaizen-fleet.md`](../specifications/sub/kaizen-fleet.md)
spells the exact invocation: `pv kaizen rollup --quarter 2026Q3 >
docs/status/quarterly/2026Q3.md`. At the pinned `pv 0.49.0` that is
`error: unexpected argument 'rollup' found` — **`pv kaizen` has no `rollup`
subcommand and no `--quarter` flag**, and it does not run in this repo at all
(`error: no repos found with binding.yaml and sibling directory`). So this
register was never merely unwritten; **it was never writable**, from the day the
cadence was published. That does not decide the owner's question — what to
promise is still a choice — but it strikes one of the three options the decision
was written against, and it is the difference between a commitment that was
dropped and one that was never executable. **The escalation was right; stopping
at it was not** — an owner-gated question is a reason to escalate, never a reason
to stop measuring.

## How to pick this up in a future session

⚠️ Step 7 below is the **writer that never ran**, and the reason is structural,
not sloth: this checklist lives *inside* the file it maintains, and nothing
outside points at it. `CLAUDE.md`'s "Persistence pointers" name the memory
directory, `docs/specifications/`, `audit-design.md` and `contracts/` — never
`docs/status/`; `README.md` links into this directory exactly once, and to a
different file. **A register whose only writer is an instruction inside itself
has no writer.** The steps a session actually follows now are `CLAUDE.md` →
`docs/roadmaps/queue.yaml` → `CHANGELOG.md`, and the list below is kept as the
historical intent rather than as live procedure.

1. Read [`CURRENT.md`](CURRENT.md) top to bottom
2. Check the **Next Actions** section
3. Pick the highest-priority open pmat work item — in practice, from [`../roadmaps/queue.yaml`](../roadmaps/queue.yaml)
4. Read its linked spec under [`../specifications/`](../specifications/)
5. Implement until [`§18 CI gates`](../specifications/sub/ci-gates.md) pass
6. Update `CURRENT.md` with what changed
7. ~~Append a new `YYYY-MM-DD-<topic>.md` session log~~ — **superseded**; the
   per-session record goes to `CHANGELOG.md` plus a `queue.yaml`/`roadmap.yaml`
   entry. Kept struck-through rather than deleted so the abandonment is visible
   in the file that declared the cadence.
