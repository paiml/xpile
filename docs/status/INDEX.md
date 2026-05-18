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
| 2026-05-18 | Substrate completion + Silver/Gold/Platinum/Diamond-universal + Diamond depth-2 UNIVERSAL + **Diamond depth-3 UNIVERSAL across 5 layers** | [2026-05-18-substrate-completion.md](2026-05-18-substrate-completion.md) — PMAT-058..245: 12/12 contracts to §14.4 QUORUM + UNIVERSAL 5-tier refinement + UNIVERSAL Diamond depth-2 across all 12 contracts + UNIVERSAL Diamond depth-3 across all 5 layers (258 Lean theorems = 53 Bronze + 108 Silver + 24 Gold + 39 Platinum + 34 Diamond + 43 Kani harnesses = 301 stratum-vote artifacts), PMAT-214..226 Diamond-UNIVERSAL depth-1, PMAT-228..239 UNIVERSAL depth-2, **PMAT-241..245 UNIVERSAL Diamond depth-3 across all 5 layers** (shift-monoid L1, length-homomorphism L2, function-axiom L3, zero-copy-functor L4, meet-semilattice L5) |

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
