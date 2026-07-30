# CLI Reference (`xpile`)

**Section 14 of [xpile-spec.md](../xpile-spec.md).**

> ⛔ **READ THIS FIRST — this page carries TWO surfaces and only one of them runs.**
>
> Everything under **[Shipped surface](#shipped-surface)** was executed against the
> binary and works. Everything under **[Planned surface — does not run
> today](#planned-surface--does-not-run-today)** is design intent: on **2026-07-30**
> every command and flag in that half was run against **`xpile 0.1.618`** and
> **every one exited non-zero at argument parsing**, with the stderr transcribed
> beside it.
>
> Until 2026-07-30 the planned half was published FIRST and without qualification,
> and the file's own "Status at v0.1.0" correction — 100 lines below it — was itself
> stale in the opposite direction, claiming five shipped subcommands against a binary
> that ships seven (PMAT-1498).
>
> **The probe table below is a MEASUREMENT DATED 2026-07-30, NOT AN INVARIANT.** No
> gate re-derives it: `crates/xpile/tests/cli_docs_drift.rs` is named for exactly this
> job but reads `book/src/reference/cli.md`, a **different file**. Re-derive the
> shipped list in one command: `xpile --help`.

---

## Shipped surface

Probed 2026-07-30, `xpile 0.1.618`. Seven subcommands, each `--help` exit 0:

| Command | Purpose |
|---|---|
| `xpile info` | Show registered frontends and backends (default when no subcommand is given) |
| `xpile transpile <file>` | Transpile a source file. The extension selects the frontend; `--target` selects the backend |
| `xpile audit <path>` | Falsifier F1 — Layer-1 contract-citation coverage over a corpus. Drives the `XPILE-FALSIFY-001` metric (PMAT-029; denominator narrowed by `XPILE-FALSIFY-002`) |
| `xpile attestations` | Extrinsic-stratum per-contract attestation counts (`XPILE-QUORUM-005`). Scans `contracts/*.yaml` for IDs, counts mentions in `roadmap.yaml` (PMAT-032) |
| `xpile quorum` | Unified §14.4 N-of-M oracle quorum reporter — per-stratum votes (Semantic / Symbolic / Runtime / Extrinsic) (PMAT-033) |
| `xpile diamond` | Diamond-tier coverage reporter — counts `_diamond` `lean_theorem` refs per contract and classifies `none` / `depth-N`, exact and never bucketed (PMAT-249) |
| `xpile hybrid <path>` | Hybrid transpile, §16 Phase 1 + 2 — walks a module directory, dispatches each file to its frontend, reconciles cross-language FFI boundaries into a manifest |

`diamond` and `hybrid` are the two this page omitted while claiming a complete list;
`docs/specifications/audit-design.md` has published both throughout.

The per-command sections below exist so that the property
`cli_docs_drift::every_registered_subcommand_has_a_cli_md_section` — already written,
already green, and pointed at the *other* `cli.md` — passes on this page the day its
corpus is widened to include it. See [Provenance](#provenance).

## `xpile info`

```bash
xpile info
```

Prints the registered frontends and backends of the live session. The default action
when no subcommand is given. This is the command that re-derives the shipped surface;
the authoritative transcript is pinned in
[`book/src/reference/cli.md`](../../../book/src/reference/cli.md), which a gate holds
against the binary.

## `xpile transpile`

```bash
xpile transpile [OPTIONS] <INPUT>
```

The file extension selects the frontend; `--target` selects the backend.

| Flag | Default | Meaning |
|---|---|---|
| `<INPUT>` | required | path to the source file |
| `--target <T>` | `rust` | one of `rust`, `ruchy`, `ptx`, `wgsl`, `spirv`, `wasm`, `lean`, `shell`, `forjar`, or one of the aliases `wat`, `sh`, `bash`, `forjar-yaml` |
| `--out <P>` | stdout | output path. **Spelled `--out`, not `--output`** |
| `--emit-crate <D>` | — | write a buildable Cargo crate (`Cargo.toml` + `src/main.rs`) instead of printing; if the program defines `main()`, `cargo build --target wasm32-wasip1` then yields the portable "universal `.wasm`". `--target rust` only |
| `--contracts <on\|off>` | `on` | emit / suppress the `xpile-contract:` citations across the L1–L5 taxonomy. Library counterpart: `xpile_backend::strip_contract_citations` |
| `--hardware <H>` | — | hardware profile (`ptx`, `ptx:sm_89`); **required** to reach `--target ptx`, which refuses without a compute capability |

```bash
xpile transpile foo.py                                  # → stdout, --target rust
xpile transpile foo.py --target wasm --out foo.wat      # explicit backend + output path
xpile transpile foo.py --emit-crate ./out               # buildable Cargo crate
xpile transpile foo.py --contracts off                  # suppress citation comments
xpile transpile kernel.py --target ptx --hardware ptx:sm_89
```

Those five flags are the entire set. Before 2026-07-30 **none of them appeared on this
page** — the CLI reference documented nine flags that do not parse and zero of the
five that do, including `--target`, which is what selects among the nine backends.

## `xpile audit`

```bash
xpile audit [OPTIONS] [PATH]
```

Falsifier F1 — Layer-1 contract-citation coverage over a corpus. Walks the path,
transpiles every source file xpile recognises, and reports what percentage of the
functions that *require* a `// xpile-contract:` citation carry one. The denominator is
the functions whose `applicable_contracts()` is non-empty, **not** every emitted
function (`XPILE-FALSIFY-002` narrowed it there, because a comparison-only or
logical-only body correctly emits none). Both counts are printed.

## `xpile attestations`

```bash
xpile attestations [OPTIONS]
```

Extrinsic-stratum per-contract attestation counts (`XPILE-QUORUM-005`). Walks
`contracts/*.yaml` to discover the contract-ID universe, then scans the `roadmap.yaml`
work-item log for mentions of each ID; each occurrence is one human attestation.

## `xpile quorum`

```bash
xpile quorum [OPTIONS]
```

Unified §14.4 N-of-M oracle quorum reporter. Walks the contract corpus and tallies
per-stratum votes contract by contract — Semantic (`lean_theorem:` refs), Symbolic
(`kani_harness:` refs), Runtime (fixtures naming the ID, plus witness files naming it
*and* carrying a non-comment runtime-availability probe call, PMAT-1367), Extrinsic
(`roadmap.yaml` mentions) — classifying each as `QUORUM` (≥1 vote in ≥3 strata),
`PARTIAL` (1–2 strata) or `UNVERIFIED` (0 strata), and printing a totals line.

This page states no discharge total. Run the command for the live split; it is the
source `claims_drift.rs` derives from.

## `xpile diamond`

```bash
xpile diamond [OPTIONS]
```

Diamond-tier coverage reporter. Counts `_diamond` `lean_theorem` references per
contract and classifies from the count: 0 is `none`, N is `depth-N`. The
classification is EXACT and never bucketed, so which labels appear is a function of
the corpus rather than of a list written here.

## `xpile hybrid`

```bash
xpile hybrid [OPTIONS] <PATH>
```

Hybrid transpile — §16 Phase 1 + Phase 2. Walks a module directory, dispatches each
source file to its frontend, and reconciles their cross-language FFI boundaries
(`FfiManifest::reconcile`) into a manifest. Prints one line per resolved boundary
(symbol, from→to, shim_id), or prints the unresolved boundaries and exits non-zero —
the `manifest_completeness` gate of `C-FFI-CPYTHON-EXT`.

## `xpile help`

```bash
xpile help [COMMAND]
```

clap's generated help. `xpile --help` and `xpile -h` are equivalent for the top level;
`xpile --version` prints the version (there is no `xpile version` subcommand).

## Exit codes

Measured 2026-07-30 against `xpile 0.1.618`:

| Code | Meaning | Provenance |
|---|---|---|
| 0 | Success | measured — `transpile`, `hybrid` on a resolvable module |
| 1 | Runtime failure — unreadable input, backend refusal, unmet report threshold | measured — `transpile missing.py` → `Error: reading missing.py`; `quorum` on a corpus below quorum |
| 2 | **Argument-parse failure (clap)** | measured — every invocation in the planned half below returns this |

Code `2` is emitted by the argument parser for *any* malformed invocation. Through
2026-07-29 this table instead assigned `2` the meaning *"static-pass failure + repair
budget exhausted"*, and defined `3` (oracle divergence), `4` (manifest incompleteness)
and `5` (*"contract lint failure (`xpile lint`)"*) — all four keyed to `--repair`,
`xpile manifest` or `xpile lint`, none of which parse, and `5` keyed to a subcommand
that has never existed. A reader hitting a real `2` would have read it as a budget
exhaustion.

---

## Planned surface — does not run today

Design intent for the post-v0.1.0 CLI. **Nothing in this section runs.** Each row was
executed 2026-07-30 against `xpile 0.1.618`; the stderr is verbatim.

### Top-level commands (planned)

| Planned command | Result 2026-07-30 |
|---|---|
| `xpile lint` — delegate to `pv lint contracts/` | `error: unrecognized subcommand 'lint'` |
| `xpile score` — delegate to `pv score contracts/` | `error: unrecognized subcommand 'score'` |
| `xpile inspect <path>` — show meta-HIR for a parsed file | `error: unrecognized subcommand 'inspect'` |
| `xpile manifest` — display the FFI manifest for a hybrid session | `error: unrecognized subcommand 'manifest'` |
| `xpile mcp` — launch MCP server (see [mcp.md](mcp.md)) | `error: unrecognized subcommand 'mcp'` |
| `xpile cache <subcmd>` — cache management | `error: unrecognized subcommand 'cache'` |
| `xpile version` — print xpile + pv + pmat versions | `error: unrecognized subcommand 'version'` — use `xpile --version` |
| `xpile contract` (create / list / lint as a thin wrapper over `pv`) | `error: unrecognized subcommand 'contract'` |
| `xpile fleet` (cross-repo coordination) | `error: unrecognized subcommand 'fleet'` |
| `xpile agent --repair` (LLM-mediated repair; `xpile-agent` is scaffold-stage) | `error: unrecognized subcommand 'agent'` |

`xpile mcp serve` is the MCP server mode; the `xpile-mcp` crate is scaffold-stage.

### Planned `transpile` flags

Every one returns `error: unexpected argument '<flag>' found`, exit 2:

`--repair` · `--repair=cached` · `--repair=force` · `--hybrid` · `--output` (the
shipped spelling is `--out`) · `--dry-run` · `--repair-max-iterations` ·
`--repair-max-tokens` · `--repair-max-seconds`

`--hybrid` is superseded: hybrid mode shipped as the **`xpile hybrid`
subcommand**, not as a `transpile` flag.

#### Repair-mode flag matrix (planned)

Governs `--repair`, which does not parse. Retained as the design of the repair lane.

| Flag | LLM invoked? | Cache consulted? | Cache written? |
|---|---|---|---|
| (no flag) | never | never | never |
| `--repair` | only on static failure, cache miss | yes | yes |
| `--repair=cached` | never | yes (required — fails closed on miss) | no |
| `--repair=force` | always | no | yes |

Planned budget defaults: 8 iterations / 200,000 tokens / 300 s. See [budget.md](budget.md).

### Planned `xpile inspect`

`xpile inspect foo.py`, `--format json`, `--hir-only` — all
`error: unrecognized subcommand 'inspect'`. Would print the meta-HIR `Module` produced
by the dispatched frontend.

### Planned `xpile manifest`

`xpile manifest foo_module/`, `--reconcile`, `--validate` — all
`error: unrecognized subcommand 'manifest'`. The reconciliation itself ships today
inside `xpile hybrid`, which prints one line per resolved boundary and exits non-zero
on unresolved ones (the `manifest_completeness` gate of `C-FFI-CPYTHON-EXT`).

### Planned `xpile cache`

`cache stats`, `cache prune --older-than 90d`, `cache prune --orphaned`,
`cache verify`, `cache where` — all `error: unrecognized subcommand 'cache'`.

### Planned configuration file

⛔ **`xpile.toml` HAS NO READER.** Measured 2026-07-30: the string `xpile.toml` occurs
in exactly two tracked files, this one and [mcp.md](mcp.md), and in **zero** lines of
Rust; no tracked file is named `xpile.toml`; the keys below (`default_model`,
`max_wall_clock_seconds`, …) appear in no `.rs` file in the workspace. A reader who
creates this file and sets a value gets **silence** — not an error, not a warning, no
effect. Every key belongs to the planned `--repair` / `cache` / hybrid-oracle lanes
above.

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

The planned precedence is: command-line flags override `xpile.toml`; `xpile.toml`
overrides defaults. Stated here as *design*, because no code implements it — through
2026-07-29 it was stated as fact.

---

## Provenance

The originally-planned *"full CLI lands in Phase 2-3"* framing was superseded by what
the substrate-completion run (PMAT-058..077) exposed: per-contract introspection via
`quorum` / `attestations` / `diamond`, and the real `transpile` / `audit` / `hybrid`
paths over the shipped backends and frontends. PMAT-1498 separated the two surfaces
and dated the measurement.

**Why this page went unchecked for so long.** `crates/xpile/tests/cli_docs_drift.rs`
(`XPILE-CLIDOCS-001`, PMAT-1429) was written to lock out this exact defect class, and
its own header records the earlier instance: *"`xpile hybrid` — a registered
subcommand — had no section at all."* It reads `book/src/reference/cli.md`. There are
**two** files named `cli.md`, and all thirteen gate references in `crates/` resolve to
the book copy; **none** reads this one, the normative §14 the book page is derived
from. Measured 2026-07-30, its property
`every_registered_subcommand_has_a_cli_md_section` reds on **7 of the 8** registered
subcommands against the pre-PMAT-1498 text of this page, and passes 8 of 8 against the
current text. `claims_drift::claim_pages()` *does* walk `docs/specifications/`, so this
page was in the strictest claim corpus the whole time — but every assertion there hunts
a false or stale NUMERAL, and a phantom subcommand offers none. Being in scope is not
being covered. Widening the `cli_docs_drift.rs` corpus to both files is filed as
`XPILE-SELFCLI-001` in `docs/roadmaps/queue.yaml` `next_lane`; the Wed 2026-07-29
freeze bars the gate edit itself.
