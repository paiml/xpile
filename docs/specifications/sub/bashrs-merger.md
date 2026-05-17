# bashrs Merger Plan

**Section 19 of [xpile-spec.md](../xpile-spec.md). Sibling: [migration.md](migration.md).**

**History note**: this document previously argued for *federation* between xpile and bashrs as separate transpilers. That decision was reversed on 2026-05-17 in favour of full merger — both IR-level and repo-level. The git history of this file (under its previous name `bashrs-federation.md`) preserves the federation-era reasoning for anyone wanting to read the path that was rejected and why.

## Decision

bashrs ([github.com/paiml/bashrs](https://github.com/paiml/bashrs)) joins xpile on the *same terms* as depyler and decy: **extract-then-merge**, with bashrs becoming a workspace member in `crates/bashrs-frontend/` and `crates/bashrs-backend/`. After the merge, the `paiml/bashrs` GitHub repo becomes a thin shim (~50 LoC) re-exporting from xpile, preserving `cargo install bashrs` invocations.

This is symmetric with §19 of the canonical spec — there is no longer a "native vs federated" distinction. All four Sovereign AI Stack transpilers (`depyler`, `decy`, `ruchy`, `bashrs`) live in one workspace, share one IR (meta-HIR), share one quality regime, and ship on one cadence.

## Two layers of merger

### Layer A — Repo merge

Same plan as §19 for depyler/decy/ruchy:

1. **Extract (weeks 1–6)** — bashrs's reusable internals (lexer, parser, ShellIR, quoting machinery, ShellCheck-compatible verifier, 17,882-pattern corpus) become workspace crates that the existing bashrs GitHub repo depends on. Per-domain repos shrink as functionality moves into xpile. Both coexist during extraction.
2. **Merge (weeks 7–8)** — `git filter-repo` + `git subtree add` folds the bashrs history into `crates/bashrs-frontend/` and `crates/bashrs-backend/`, preserving authorship and commit history.
3. **Post-merge** — `paiml/bashrs` becomes a re-export shim. `cargo install bashrs` continues to work; the binary is now xpile-internal.

### Layer B — IR merge (the new decision, 2026-05-17)

This is what changed from the previous federation plan. Meta-HIR grows to natively represent shell semantics, so the shell domain composes with C / Python / Rust at the type level. Cross-domain refinement (e.g., a Python `subprocess.run(...)` refining into a typed `Stmt::Pipeline` in one IR pass) becomes possible.

New meta-HIR variants planned (see [meta-hir.md](meta-hir.md) for the full update once it lands):

| Surface | Variant | Purpose |
|---|---|---|
| `Stmt` | `Cmd { program, args, env, redirections }` | Single command invocation |
| `Stmt` | `Pipeline { stages: Vec<Stmt> }` | `cmd1 \| cmd2 \| cmd3` |
| `Stmt` | `ShellLoop { kind: LoopKind, body }` | `for`/`while` over shell items |
| `Expr` | `ShellVar(String)` | `$NAME` / `${NAME}` reference |
| `Expr` | `QuotedString { content, quoting: QuotingStrategy }` | Tracks single vs double vs backslash quoting at the type level |
| `Expr` | `CommandSubstitution(Box<Stmt>)` | `$(cmd)` |
| `Type` | `ShellString` | Quoted-aware string type |
| `Type` | `ExitCode` | i32 status return |
| (enum) | `QuotingStrategy { Single, Double, Backslash, None }` | |
| (enum) | `LoopKind` | Shell-loop dialects (`for x in $list`, `while [ ... ]`, `until`, ...) |

Backends that don't consume shell variants (Rust, Ruchy, Lean, PTX, WGSL, SPIR-V) return `Unsupported` for them at first, the same way Lean returned `Unsupported` for `Stmt::While` between PMAT-006 and PMAT-010. As real cross-domain cases land, those backends grow handling.

### Why both layers, not just one

| Layer | Without it... |
|---|---|
| Repo merge only | bashrs's IR stays private; cross-domain refinement is impossible; the "everything above meta-HIR is shared" claim has a federation seam |
| IR merge only | bashrs continues to release independently from a separate repo; release-skew makes shared infrastructure (Oracle, agent loop, MCP, contracts) hard to keep aligned |
| Both | One workspace, one IR, one quality regime — the only architecture where the provability roadmap in §27 applies *uniformly* across every transpile target |

## What stops being true

The previous (federation-era) version of this document made claims that are now invalid. Recording them explicitly so future readers don't recover the wrong mental model:

- ~~"Shell semantics don't compose with C / Python / Rust types at the type level"~~ — they *will*, once meta-HIR grows the variants listed above. Whether that composition is *useful* is the load-bearing bet.
- ~~"Federation keeps each transpiler's IR clean"~~ — federation was discarded.
- ~~"bashrs publishes to crates.io independently"~~ — xpile will own the publish post-merge.
- ~~"Forcing shell through meta-HIR would either bloat the IR or lose fidelity"~~ — the IR-bloat concern is real and is now the load-bearing risk; see "What's now load-bearing" below.

## What's now load-bearing (and how we check)

The merge is worth doing only if real cross-domain consumers materialise. If shell variants in meta-HIR turn out to only ever be produced by `bashrs-frontend` and consumed by `bashrs-backend`, we've built a "shell ghetto" in the shared IR for no benefit.

**Check-back at v0.3.0** (status as of 2026-05-17): the acceptance set required at least one of the following to ship before v0.3.0. **(1) shipped at v0.1.0 (PMAT-040)**, ahead of schedule:

1. ✅ A Python frontend lowering that *produces* a shell-variant `Stmt::Cmd` or `Stmt::Pipeline` (e.g., recognising `subprocess.run([...])` calls and refining them). **Shipped 2026-05-17 (PMAT-040)** — depyler-frontend's `lower_expr_stmt_as_cmd` recognises `subprocess.run([str-literal, ...])` and lowers to `Stmt::Cmd`. Fixture `tests/fixtures/subprocess_demo.py` + integration test `transpile_python_subprocess_run_to_shell_via_bashrs_backend` lock it in. **The IR merge is no longer load-bearing on a future hypothesis — it has shipped evidence.**
2. A Rust frontend lowering that produces a shell variant (e.g., recognising `std::process::Command::new(...).args(...)`). Still future.
3. A Lean theorem in `contracts/lean/` that depends on a meta-HIR shell variant (e.g., proving `subprocess invariants` for transpiled Python). Still future.

The reversal mechanism (`XPILE-UNMERGE-001`) is now dormant — its precondition was "*none* of the three has shipped by v0.3.0", and (1) is in tree. The architectural cost of the merger is now demonstrated to buy a concrete capability rather than a speculative one.

This check-back is the falsifier for the IR merge decision. It is added to the §27 provability roadmap as a new falsifier metric (planned PMAT prefix: `XPILE-FALSIFY-SHELL-GHETTO`).

## Routing contract (unchanged surface, unchanged extensions)

The dispatch table the federation-era version established stays intact — what changed is what happens *behind* the dispatch.

| Extension(s) | Frontend |
|---|---|
| `.py`, `.pyi` | `depyler-frontend` (lowers to meta-HIR) |
| `.c`, `.h`, `.cpp`, `.hpp`, `.cu` | `decy-frontend` (lowers to meta-HIR) |
| `.rs` | `rust-frontend` (planned) (lowers to meta-HIR) |
| `.ruchy` | `ruchy-frontend` (lowers to meta-HIR) |
| `.lean` | `lean-frontend` (lowers to meta-HIR) |
| `.sh`, `.bash`, `.zsh` | `bashrs-frontend` (lowers to meta-HIR, using shell variants) |
| `Makefile`, `*.mk` | `bashrs-frontend` (Makefile dialect) |
| `Dockerfile` | `bashrs-frontend` (Dockerfile dialect) |

All frontends now sit on the same trait; `TranspileSession` has one lane.

## What gets reused across all four transpilers

Same list as the federation version — these were *always* shared infrastructure; the change is that now they no longer cross a federation boundary:

- Oracle (`xpile-oracle`)
- Bounded agent repair loop (`xpile-agent`)
- Contract substrate (`aprender-contracts` / `pv` + the `contracts/` directory)
- MCP server (`xpile-mcp`)
- Citation bridge (per §27 PMAT-011)
- Budget discipline
- Skills system
- CI gates (`fmt` / `clippy` / `deny` / `pv lint` / workspace tests)

## Implementation status

- **v0.1.0 (current — Layer A scaffold + Layer B IR variants COMPLETE; substrate at QUORUM, 2026-05-17/18 across PMAT-037..058)**: `crates/bashrs-frontend/` and `crates/bashrs-backend/` exist as workspace members alongside `depyler / decy / ruchy` siblings. `SourceLang::Shell` + `Target::Shell` enum variants land (PMAT-037). `Frontend::matches_path` enables canonical-filename routing for `Makefile` / `Dockerfile` (PMAT-038). **All Layer B meta-HIR shell variants from the table above shipped (PMAT-039..056):** `Stmt::Cmd` (PMAT-039), `Stmt::Pipeline` (PMAT-041), `Stmt::ShellLoop` + `LoopKind` (PMAT-048), `Stmt::ShellAssign` (PMAT-051, beyond the original spec table), `Expr::QuotedString` + `QuotingStrategy` (PMAT-042), `Expr::ShellVar` (PMAT-045), `Expr::CommandSubstitution(Box<Stmt>)` (PMAT-047), `Expr::ShellSpecial` (PMAT-055), `Type::ShellString` + `Type::ExitCode` (PMAT-046). bashrs-frontend's tokenizer handles realistic shell idioms (single+double+escape quoting, `$NAME`/`${NAME}` variable expansion, `$(cmd)` + backtick command substitution, `NAME=value` assignment, pipelines, inline `#` comments, POSIX special parameters $1-9/$@/$#/etc., escape sequences). bashrs-backend emits real POSIX sh for every variant. Other backends refuse shell Expr/Stmt/Type variants via explicit `Unsupported` arms naming `C-BASHRS-POSIX-IDEMPOTENCE`. **`C-BASHRS-POSIX-IDEMPOTENCE` reaches full four-stratum §14.4 QUORUM** (Sem=1 via PMAT-044's `contracts/lean/Bashrs.lean`, Sym=1 via PMAT-058's `contracts/kani/bashrs.rs`, Run=1 via PMAT-043's `shell_diff_exec.rs`, Ext≥6 via PMAT-037..058 roadmap entries (Extrinsic count continues to accumulate as new tickets reference the contract — Ext=8 after PMAT-085..092 bashrs polish)) — second contract after `C-PY-INT-ARITH` to clear that bar. **Cross-domain Python → shell shipped (PMAT-040):** depyler-frontend lowers `subprocess.run([str-literal, ...])` to `Stmt::Cmd`. **The v0.3.0 check-back is already satisfied** (one cross-domain consumer in tree); `XPILE-UNMERGE-001` is dormant. **Realistic e2e demo (PMAT-052):** `crates/xpile/tests/fixtures/bashrs_realistic_demo.sh` exercises every Layer B construct end-to-end and round-trips byte-identically through bashrs-frontend → meta-HIR → bashrs-backend. **No real bashrs source folded yet** — the full ShellIR + 17,882-pattern corpus + ShellCheck-compatible verifier come at v0.2.0.
- **v0.2.0 (target — Q3 2026)**: Layer A complete (bashrs source folded into the workspace via `git filter-repo` + `git subtree add`). The scaffold-stage `parse_and_lower` / `lower` bodies grow the full 17,882-pattern corpus' coverage (today's bashrs-frontend handles a realistic POSIX subset; v0.2.0 absorbs the long tail of bash/zsh/Makefile/Dockerfile idioms). The bashrs binary becomes xpile-internal; `paiml/bashrs` repo is a re-export shim. **Layer B IR shape is essentially complete at v0.1.0** — v0.2.0 work focuses on *coverage* (parsing more shell input, emitting more sophisticated outputs) rather than *new variants*. **Special-file matching (`Makefile`, `Dockerfile`) shipped at v0.1.0 (PMAT-038)** via `Frontend::matches_path` — the audit walker (`collect_source_files`) will be extended to walk those canonical filenames once the audit pipeline grows shell-target support.
- **v0.3.0 (target — Q1 2027)**: ✅ **already satisfied at v0.1.0 (PMAT-040)** — at least one cross-domain consumer of the shell variants has shipped (depyler-frontend recognizes `subprocess.run([...])` and lowers to `Stmt::Cmd`). The `XPILE-UNMERGE-001` reversal mechanism is dormant. Further cross-domain consumers (Rust `Command::new` recognition, Lean theorems about shell composition) remain on the v0.3.0+ wishlist but are no longer load-bearing for the merge survival.

## What absorbed bashrs users should expect

- **Release cadence**: was bashrs-driven (multi-release-per-month). Becomes xpile-driven. Security fixes (SEC001–SEC008) ride xpile's release train.
- **Versioning**: bashrs is at v6.65.0. Post-merge, the `bashrs` shim re-exports xpile's `bashrs-frontend` under the historic name. The shim's published version may temporarily pin xpile's; communication of this transition lives in CHANGELOG.md.
- **Existing CI integrations** depending on bashrs continue to work via the shim. There is no breaking change at the binary surface.

## Why this asymmetry-elimination is worth the cost

The previous (federation) plan had a clean architectural argument — keep IRs separate when they don't compose. The new (merger) plan accepts the cost (potential meta-HIR pollution; absorbed release cadence; one-time community disruption) in exchange for:

1. **One quality regime, no escape**. audit-design.md §4's "ecosystem isolation" and "citation-bridge fragility" caveats both narrow significantly.
2. **Atomic cross-language refactors**. `Type::BigInt` would have been one PR instead of four if bashrs had been in-tree at the time.
3. **Cross-domain contracts become first-class**. A contract about "POSIX shell idempotency" could constrain how Python `subprocess.run` lowers to shell. Today that's impossible because Python and shell live in disjoint IRs.
4. **Single install story**. `cargo install xpile` → every transpiler. The Sovereign AI Stack narrative becomes literal, not federated.
5. **One bus factor problem, not four**. Federation multiplies governance overhead by sibling count.

## Cross-references

- xpile-spec.md §1 — Vision (no longer split into native vs federated tiers).
- xpile-spec.md §3 — Meta-HIR (will grow shell variants per Layer B).
- xpile-spec.md §19 — Migration (now symmetric across all four transpilers).
- xpile-spec.md §27 — Provability roadmap (gains `XPILE-FALSIFY-SHELL-GHETTO` check-back).
- audit-design.md §4 — Caveats (ecosystem-isolation and citation-bridge fragility both narrow post-merge).
- bashrs README — domain coverage that becomes xpile-owned post-merge.
