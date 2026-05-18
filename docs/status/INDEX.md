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
| 2026-05-18 | Substrate completion + Silver-completion + Gold-universal milestone | [2026-05-18-substrate-completion.md](2026-05-18-substrate-completion.md) — PMAT-058..198: 12/12 contracts to §14.4 QUORUM at 4-stratum (185 Lean theorems = 53 Bronze + 108 Silver + 24 Gold + 43 Kani harnesses = 228 stratum-vote artifacts), 9-PR bashrs round-trip lock-in series, comprehensive doc sweep across 24+ files, PMAT-127..145 quality sweep (warnings 79 → 0), PMAT-147..151 XPILE-QUORUM-006 per-equation Kani fan-out, PMAT-156..183 Silver bracket completion (42/42 equations at Silver), **PMAT-185..197 brought Gold-tier coverage to UNIVERSAL — every one of the 12 contracts has at least one Gold theorem** (PyIntFast, BoundedRefcountDelta, BoundedSmem, WarningLineCount, NonEmptyDefinition, NonEmptyPreconditionList, NonEmptyHomogeneousList α, SuccessfulOutcome, ConsistentFrontendOutput, ConsistentBackendInput, FrameSafeTransition, CitationCompleteContract) demonstrating 5 distinct subtype patterns (bounded-numeric, collection-cardinality, equality-to-constant, cross-field equality, frame-safety) across all 5 contract layers |

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
