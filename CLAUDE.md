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

## Shell / Makefile / Dockerfile artifacts

xpile currently has zero `.sh` / `.bash` / `.zsh` / `Makefile` /
`Dockerfile` files. **Don't introduce them without routing through
bashrs**. The bashrs merger shipped early — `crates/bashrs-frontend/`
and `crates/bashrs-backend/` exist as workspace members alongside
`crates/depyler-frontend/` etc. as of v0.1.0 (PMAT-037..058 +
PMAT-085..092 + PMAT-119 polish; see
[`docs/specifications/sub/bashrs-merger.md`](docs/specifications/sub/bashrs-merger.md)).
**Scope (be honest about it — PMAT-989):** the bashrs frontend
handles the FLAT-command subset only — quoting (`'...'` / `"..."`),
`$VAR`, `$(...)` / backtick command substitution, pipelines, and
single-value assignment (`VAR=value`). Shell CONTROL-FLOW (loops
`for`/`while`/`until`, conditionals `if`/`case`) is NOT handled: the
frontend now REFUSES it with a hard `FrontendError` rather than
silently shredding it into barewords (real `Stmt::ShellLoop`
production is the v0.2.0 "real bashrs parser" job). The
`C-BASHRS-POSIX-IDEMPOTENCE` contract holds for that flat-command
subset under its §14.4 stratum coverage — it does NOT certify
control-flow round-tripping (out of scope until the v0.2.0 fold).
Do not claim the frontend "handles realistic POSIX shell idioms"
in general; it handles the flat-command subset and refuses the
rest. See CHANGELOG PMAT-085..092 + PMAT-119 for the flat-subset
round-trip invariant lock-in series, and PMAT-989 for the
control-flow refusal. The "v0.2.0 merger" framing was superseded
for the flat subset — but control-flow parsing genuinely remains
v0.2.0 work.

Concrete workflow when shell artifacts become necessary:

1. **Prefer Rust → POSIX** — author the script as a small Rust file
   and run the in-tree transpile path. The Rust source goes in
   `scripts/` (new directory) and the emitted `.sh` is gitignored
   or committed as a build artifact, never hand-edited.
2. **Hand-written shell is round-tripped through bashrs-frontend
   before commit** — if a `.sh` *must* be authored directly
   (rare), run it through the bashrs-frontend → bashrs-backend
   round-trip (see `parse_and_lower_*` tests for the supported
   POSIX subset). Same for `Makefile` and `Dockerfile`.
3. **In-tree workflow (v0.1.0+)**: `xpile transpile foo.py
   --target shell` is the cross-domain path (PMAT-040 recognizes
   `subprocess.run([...])` and lowers via bashrs-backend).
   Bashrs-backend round-trips through `bashrs_realistic_demo.sh`
   (PMAT-052) on every CI cycle.
4. **No silent introduction** — adding shell-flavored CI logic,
   release scripts, dev-loop helpers, or Docker images outside
   the substrate-quality path should be discussed before
   landing. Shell is in scope at v0.1.0; ungated shell files
   are not.

The point: xpile's "quality regime" claim only holds if every
language in the repo is under the regime. After the merger, shell
domains are under the regime by construction.

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
