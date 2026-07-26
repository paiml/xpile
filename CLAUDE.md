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
**Scope (be honest about it — PMAT-989 / PMAT-1268 / PMAT-1276 /
PMAT-1281 / PMAT-1283 / PMAT-1284 / PMAT-1285):** the bashrs frontend
handles the FLAT-command subset — quoting (`'...'` / `"..."`), `$VAR`,
`$(...)` / backtick command substitution, pipelines, and single-value
assignment (`VAR=value`) — PLUS the FULL shell CONTROL-FLOW surface for
v0.1.0: the three LOOP dialects `for … in <items>; do … done`
(PMAT-1268) and `while`/`until COND; do … done` (PMAT-1276); the
`if COND; then … [elif COND; then …]* [else …] fi` conditional
(PMAT-1283 for if/then/else, PMAT-1284 for `elif`); and the
`case WORD in PAT1|PAT2) BODY ;; … esac` statement (PMAT-1285) — in
single-line and multi-line forms, INCLUDING arbitrarily NESTED blocks
of any mix (loop-in-loop, if-in-loop, loop-in-if, if-in-if,
loop/if-in-case-arm). Loops lower to `Stmt::ShellLoop`, conditionals
to `Stmt::ShellIf { cond, then_body, else_body }` (an `elif` chain is
DESUGARED into a nested `ShellIf` in the parent's `else_body` and the
backend RE-SUGARS it back to `elif` — byte-identical round-trip;
explicit `else if … fi` canonicalises to `elif`, idempotent), and
`case` to `Stmt::ShellCase { word, arms: Vec<CaseArm{patterns, body}> }`.
All round-trip through bashrs-backend to executable POSIX (see the
`shell_diff_demo_{for,while_until,nested}_loop_round_trip`,
`shell_diff_demo_if_round_trip` (covers `elif`), and
`shell_diff_demo_case_round_trip` execution witnesses). Honesty
caveats: (a) a `while`/`until`/`if`/`elif` CONDITION and a `case`
pattern are captured VERBATIM (opaque `Expr::LitStr` / raw glob
strings) and printed back byte-for-byte — the `[ … ]` test and glob
metacharacters are NOT modelled structurally (v0.2.0); (b) `case` is
TOP-LEVEL only — a `case` nested inside a loop/if body refuses (the
`;`-segment split would mangle arm `;;`; arm bodies THEMSELVES may
contain nested loops/ifs); (c) `;&`/`;;&` (bash fall-through) is
REFUSED (PMAT-1371), not modelled — through v0.1.617 it was SHREDDED,
emitting a bare `&` that failed `bash -n`; (d) HERE-DOCUMENTS
(`<<EOF`, `<<-EOF`) are REFUSED (PMAT-1371). There is no here-doc
handling at all: the frontend trims every line and drops blank ones,
so a here-doc body was re-tokenized as commands and reflowed — through
v0.1.617 `cat <<EOF` over "  keep  me" exited 0, passed `bash -n`, and
executed DIFFERENTLY from its source (whitespace collapsed, blank line
dropped). Detection is TOKEN-level, so `echo "a << b"` still works.
Everything else refuses with a hard `FrontendError` rather than
shredding into barewords — a claim that only became true at PMAT-1371;
before it, the four shapes above plus a bare `&` in command position
all exited 0. The `C-BASHRS-POSIX-IDEMPOTENCE`
contract holds for the flat-command subset AND the (nested)
for/while/until loops, if/then/elif/else conditionals, and top-level
`case` under its §14.4 stratum coverage — it does NOT certify
case-in-loop, structural condition/pattern modelling, `;&`
fall-through, or here-documents (v0.2.0). Do not claim the frontend "handles ALL POSIX
shell" — it handles the flat-command subset plus (nested)
for/while/until + if/then/elif/else + top-level case (with opaque
conditions/patterns). See CHANGELOG PMAT-085..092 + PMAT-119 for the
flat-subset round-trip invariant lock-in series, PMAT-989 for the
control-flow refusal, PMAT-1268 for `for`, PMAT-1276 for
`while`/`until`, PMAT-1281 for recursive loop NESTING, PMAT-1283 for
`if`/`then`/`else`, PMAT-1284 for `elif`, PMAT-1285 for `case`, and
PMAT-1371 for the `;&`/`;;&`/here-doc/bare-`&` REFUSALS.
With `case`, the v0.1.0 shell CONTROL-FLOW surface is COMPLETE — only
structural condition/pattern modelling, case-in-loop, `;&`
fall-through, and here-documents remain for the v0.2.0 "real bashrs
parser" fold, and all four now REFUSE rather than shred.

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
