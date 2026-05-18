# Status Index

This directory tracks where xpile is and what's next. The **single source of truth** is [`CURRENT.md`](CURRENT.md) — that's what a future session reads first.

## Conventions

| File | Purpose | When updated |
|---|---|---|
| [`CURRENT.md`](CURRENT.md) | Live status: what's done, in-progress, next, blocked | Every session that changes the state |
| [`INDEX.md`](INDEX.md) (this file) | Map of all status documents | When a new session log lands |
| `YYYY-MM-DD-<topic>.md` | Per-session change log | At session end |
| `quarterly/YYYY-Q?.md` | Quarterly rollup (from `pv kaizen`) | At quarter close |

## Session log

| Date | Session | Topic |
|---|---|---|
| 2026-05-15 | Initial scaffold | [2026-05-15-scaffold.md](2026-05-15-scaffold.md) — workspace creation, 14 crates, `pv` wiring, 4 example contracts, full docs/specifications tree |
| 2026-05-18 | Substrate completion + UNIVERSAL 5-tier + UNIVERSAL Diamond depth-2 (CI-enforced) + depth-3 UNIVERSAL across layers + depth-4 opened | [2026-05-18-substrate-completion.md](2026-05-18-substrate-completion.md) — PMAT-058..251: 12/12 contracts to §14.4 QUORUM + UNIVERSAL 5-tier refinement + UNIVERSAL Diamond depth-2 across all 12 contracts (CI-enforced via PMAT-251 `diamond_coverage.rs` gate) + UNIVERSAL Diamond depth-3 across all 5 layers + Diamond depth-4 opened on 2 contracts (260 Lean theorems = 53 Bronze + 108 Silver + 24 Gold + 39 Platinum + 36 Diamond + 43 Kani harnesses = 303 stratum-vote artifacts), PMAT-214..226 Diamond depth-1, PMAT-228..250 depth-2, PMAT-241..245 depth-3 across layers, PMAT-247..248 depth-4 opened (PyIntArith L1, CompileRustToPtxMma L5), PMAT-249 `xpile diamond` reporter, PMAT-251 Diamond CI gate |

## Quarterly rollups

(none yet — first quarterly rollup expected at 2026-Q3 close. The original "after Phase 1 lands" gating was superseded by the substrate-first pivot — Phase 1 is now an over-shipped artifact per `sub/phased-rollout.md`.)

## How to pick this up in a future session

1. Read [`CURRENT.md`](CURRENT.md) top to bottom
2. Check the **Next Actions** section
3. Pick the highest-priority open pmat work item
4. Read its linked spec under [`../specifications/`](../specifications/)
5. Implement until [`§18 CI gates`](../specifications/sub/ci-gates.md) pass
6. Update `CURRENT.md` with what changed
7. Append a new `YYYY-MM-DD-<topic>.md` session log
