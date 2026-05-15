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

The `xpile` binary prints a scaffold banner. None of these commands are implemented yet. Full CLI lands in Phase 2-3.
