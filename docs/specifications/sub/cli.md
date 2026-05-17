# CLI Reference (`xpile`)

**Section 14 of [xpile-spec.md](../xpile-spec.md).**

## Top-level commands

```bash
xpile <command> [options]
```

| Command | Purpose |
|---|---|
| `xpile transpile <path>` | Transpile a single file or directory |
| `xpile lint` | Delegate to `pv lint contracts/` |
| `xpile score` | Delegate to `pv score contracts/` |
| `xpile inspect <path>` | Show meta-HIR for a parsed file |
| `xpile manifest` | Display the FFI manifest for a hybrid session |
| `xpile mcp` | Launch MCP server (see [mcp.md](mcp.md)) |
| `xpile cache <subcmd>` | Cache management (`prune`, `verify`, `stats`) |
| `xpile version` | Print xpile + pv + pmat versions |

## `xpile transpile`

```bash
xpile transpile foo.py                        # static path
xpile transpile foo.py --repair               # static → if fail, agent loop
xpile transpile foo.py --repair=cached        # cache hit required; never call model
xpile transpile foo.py --repair=force         # bypass cache; always re-run agent
xpile transpile --hybrid foo_module/          # multi-language session
xpile transpile foo.py --output bar.rs        # explicit output path
xpile transpile foo.py --dry-run              # show plan; don't write
```

## Repair-mode flag matrix

| Flag | LLM invoked? | Cache consulted? | Cache written? |
|---|---|---|---|
| (no flag) | never | never | never |
| `--repair` | only on static failure, cache miss | yes | yes |
| `--repair=cached` | never | yes (required — fails closed on miss) | no |
| `--repair=force` | always | no | yes |

## Budget overrides

```bash
xpile transpile foo.py --repair \
    --repair-max-iterations=12 \
    --repair-max-tokens=400000 \
    --repair-max-seconds=600
```

Defaults: 8 / 200,000 / 300. See [budget.md](budget.md).

## `xpile inspect`

```bash
xpile inspect foo.py                  # human-readable
xpile inspect foo.py --format json    # machine-readable
xpile inspect foo.py --hir-only       # skip FFI boundaries
```

Outputs the meta-HIR `Module` produced by the dispatched frontend. Useful for debugging frontend lowering.

## `xpile manifest`

```bash
xpile manifest foo_module/            # show current manifest
xpile manifest foo_module/ --reconcile   # run reconciliation; print resulting entries
xpile manifest foo_module/ --validate    # check completeness against the FFI contract
```

## `xpile cache`

```bash
xpile cache stats                          # cache size, hit rate, oldest entry
xpile cache prune --older-than 90d         # evict entries unused in 90 days
xpile cache prune --orphaned               # evict entries whose source files no longer exist
xpile cache verify                         # rehash all entries; quarantine corrupted ones
xpile cache where                          # print cache directory path
```

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success (transpile, lint, etc.) |
| 1 | Static-pass failure (no `--repair` flag) |
| 2 | Static-pass failure + repair budget exhausted |
| 3 | Oracle divergence (built clean, semantics wrong) |
| 4 | Manifest incompleteness (hybrid mode) |
| 5 | Contract lint failure (`xpile lint`) |
| 10 | Internal error (bug) — please file an issue |

## Configuration

`xpile.toml` in the project root (optional):

```toml
[repair]
default_model = "claude-sonnet-4-6"
max_iterations = 8
max_tokens = 200_000
max_wall_clock_seconds = 300

[cache]
location = "~/.cache/xpile/repair"

[hybrid]
oracle_python = "python3.12"
oracle_gcc = "gcc-14"
```

Command-line flags override `xpile.toml`; `xpile.toml` overrides defaults.

## Status at v0.1.0

The `xpile` binary ships with **5 implemented subcommands** as of v0.1.0:

| Subcommand | Purpose | Status |
|---|---|---|
| `xpile info` | Show registered frontends and backends (default action when no subcommand given) | **shipped** |
| `xpile transpile <file>` | Transpile a source file. Extension selects the frontend; `--target` selects the backend. Real emission for `--target {rust,ruchy,lean,shell}`. | **shipped** |
| `xpile audit <path>` | Report falsifier F1 (Layer-1 contract citation coverage) over a corpus. Drives the XPILE-FALSIFY-001 metric. | **shipped** (PMAT-029) |
| `xpile attestations` | Report Extrinsic-stratum per-contract attestation counts (XPILE-QUORUM-005). Scans `contracts/*.yaml` for IDs, counts mentions in `roadmap.yaml`. | **shipped** (PMAT-032) |
| `xpile quorum` | Unified §14.4 N-of-M oracle quorum reporter. Walks every contract and tallies per-stratum votes (Semantic / Symbolic / Runtime / Extrinsic). | **shipped** (PMAT-033) |

Run `xpile --help` to see the live list. Subcommands not yet shipped (post-v0.1.0):

- `xpile contract` family (create / list / lint as a thin wrapper over `pv`)
- `xpile fleet` (cross-repo coordination)
- `xpile mcp serve` (MCP server mode — `xpile-mcp` crate is scaffold-stage)
- `xpile agent --repair` (LLM-mediated repair — `xpile-agent` crate is scaffold-stage)
- `xpile cache` (content-addressed-cache management)

The originally-planned "Full CLI lands in Phase 2-3" framing has been superseded — the actual shipped CLI surface is anchored to what the substrate-completion run (PMAT-058..077) exposed: per-contract introspection via `quorum` / `attestations`, and the real `transpile` / `audit` paths over the 4 real backends + 2 real frontends + their cross-domain consumers.
