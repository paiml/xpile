# xpile — project-local Claude instructions

These instructions override the global `~/.claude/CLAUDE.md` defaults for **this repository only**. They reflect a 2026-05-15 decision: xpile is a prototype and the user prioritizes iteration speed over per-commit approval ceremony.

## Autonomy

- **Commit autonomously and frequently.** After a coherent unit of work (a contract, a sub-spec, a crate scaffold, a binary wiring), commit it without asking.
- **Push to the current branch frequently** when a remote exists.
- **Group related changes** into one commit — "scaffold proof-lane traits + impls" is one commit, not nine.
- **Tight, accurate commit messages.** What changed and why. No marketing copy. Always include the `Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>` trailer.
- **Confirmation still required for destructive operations:** `git reset --hard`, deleting branches, force-push, rewriting published history. Autonomy applies to additive forward motion, not to undoing.

## Branch workflow

- **Main is protected** once a remote exists. Use feature branches for all new work.
- **Bootstrap initial commit** lands on main directly (one-time, no remote yet).
- **Branch naming:** descriptive — `feat/lean-bidirectional`, `fix/contract-citation`, `chore/scaffold-codegens`.
- **Open one PR per coherent feature**, not per commit. Long-running prototype branches are fine.
- **Never push --force to main.**
- **Never skip hooks** (`--no-verify`, `--no-gpg-sign`). Investigate hook failures; don't bypass.

## Pre-push checklist (xpile-specific)

Run before pushing any branch:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
pv lint contracts/
cargo deny check advisories
```

These are tailored to xpile. `certeza` (mentioned in the global CLAUDE.md) is aprender-specific and not required here.

## What still requires explicit user approval

- Creating or deleting a GitHub remote
- Force-pushing
- Deleting branches that have unmerged work
- Operations on `main` beyond fast-forward of PR merges
- Anything that affects state outside this repo (Slack, email, deploy)

## Working style

- **Speed over ceremony.** Don't draft three RFCs when the design conversation already converged.
- **Verify forward motion compiles** — every commit should leave `cargo check --workspace` green. If you must commit broken state (rare), tag the commit message clearly: `[WIP]`.
- **Quality gates are advisory during prototype**, blocking once we declare v1. Right now: pv lint must pass with zero ERRORS; advisory WARNINGS are acceptable.
- **Don't refactor pre-emptively.** Get to a working prototype first; the abstractions will harden as real implementations expose what's load-bearing.

## Persistence pointers

- Memory: `/home/noah/.claude/projects/-home-noah-src-xpile/memory/`
- Canonical design: `docs/specifications/xpile-spec.md` + `sub/*.md`
- Audit (adversarial record): `docs/specifications/audit-design.md`
- Contracts: `contracts/*.yaml` (validated via `pv lint contracts/`)

## When in doubt

Move forward. Commit what works. The user prefers an extra commit over a paused session.
