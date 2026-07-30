# MCP Server

**Section 15 of [xpile-spec.md](../xpile-spec.md).**

> ⛔ **READ THIS FIRST — NOTHING ON THIS PAGE RUNS. `xpile mcp` IS NOT A SUBCOMMAND.**
>
> The whole MCP surface is design intent. Measured **2026-07-30** against
> **`xpile 0.1.618`** and the tree at tag `v0.1.618`:
> `crates/xpile-mcp/src/lib.rs` is **19 lines** — one `pub struct McpServer` with one
> `bind_addr: String` field and one constructor. There are **no tool functions, no
> transport, no argument validation, and no path handling of any kind.** No crate in
> the workspace depends on `xpile-mcp`, and `McpServer` is never constructed outside
> its own definition.
>
> Until 2026-07-30 this page published seven tools, a transport, a server lifecycle, a
> telemetry guarantee and a **security guarantee** in the present indicative, and its
> one true sentence — *"`McpServer::new(bind_addr)` exists as a stub"* — was the
> **last line of the file**, 80 lines below the claims it corrects (PMAT-1499).
>
> **The most dangerous line was the security one.** The page stated as fact that the
> server *"only operates on files within the project root"* and that *"arbitrary path
> arguments are rejected"*. A reader cannot falsify a sandbox property of a server
> they cannot start. See
> [Security posture — NOT IMPLEMENTED](#security-posture--not-implemented).
>
> `crates/xpile/tests/mcp_surface_disclosure_witness.rs` (`XPILE-MCPDOC-001`) holds
> the load-bearing half of this block against the tree on every run.

---

## Purpose

`xpile-mcp` is **intended** to expose xpile's tools via the
[Model Context Protocol](https://modelcontextprotocol.io/) so IDE assistants (Claude
Code, Claude Desktop, VS Code, JetBrains) can call them without spawning subprocesses
or maintaining their own state.

The design mirrors the pattern in `depyler-mcp` and `decy-mcp`. ⚠️ Neither crate is in
this workspace, so that comparison is **not verifiable from this repository** — treat
it as a pointer to a sibling project, not as a property of this tree. Through
2026-07-29 this page additionally described `depyler-mcp` as "PMCP-powered"; that is a
claim about a codebase no gate here can read, and it has been dropped.

## Shipped surface

Probed 2026-07-30, tag `v0.1.618`. **One item.**

| Item | Status |
|---|---|
| `McpServer::new(bind_addr) -> McpServer` | Exists. Stores the string. Does nothing with it. |

`crates/xpile-mcp/Cargo.toml` declares exactly two dependencies, `xpile-agent` and
`anyhow`, and `lib.rs` uses neither.

---

## Planned surface — does not run today

Everything in this section is design intent. Nothing below is implemented.

### Planned `xpile mcp`

`xpile` ships **seven** subcommands — `info`, `transpile`, `audit`, `attestations`,
`quorum`, `diamond`, `hybrid` — and **none of them is `mcp`**. Probed 2026-07-30, all
three published invocations fail identically at argument parsing:

| Published invocation | Result, `xpile 0.1.618` |
|---|---|
| `xpile mcp` | `error: unrecognized subcommand 'mcp'`, exit 2 |
| `xpile mcp --port 3000` | `error: unrecognized subcommand 'mcp'`, exit 2 |
| `xpile mcp --port 3000 --background` | `error: unrecognized subcommand 'mcp'`, exit 2 |

Exit **2** is clap's parse-error code. Re-derive the shipped list in one command:
`xpile --help`.

### Planned tools

**Seven** planned tools. This table is the single source of that number; §15 of
[xpile-spec.md](../xpile-spec.md) does not restate it. Through 2026-07-29 the parent
section published *"Six initial tools"* against these seven rows — two normative pages,
two counts, in the corpus every claim gate walks (PMAT-1499).

| Planned tool | Intended behaviour |
|---|---|
| `transpile_file(path, options)` | Transpile a single file; return generated Rust or error |
| `transpile_hybrid(dir, options)` | Hybrid transpile; return Rust files + FFI manifest |
| `inspect_meta_hir(path)` | Return the meta-HIR Module as JSON |
| `inspect_ffi_manifest(dir)` | Return the FFI manifest for a hybrid session |
| `lint_contracts(dir)` | Delegate to `pv lint`; return structured findings |
| `score_contracts(dir)` | Delegate to `pv score`; return per-contract scores |
| `query_contracts(query, kind?)` | Delegate to `pv query`; return matching contracts |

None of the seven exists as a function, a handler, or a name anywhere in
`crates/xpile-mcp/`.

### Why MCP and not a raw HTTP API

The design rationale, unchanged and unaffected by implementation status:

- **Auth inheritance.** MCP servers run under the user's IDE auth — no separate key
  management for xpile.
- **Tool-use-native.** LLMs already know how to use MCP tools; they would not have to
  learn xpile-specific HTTP conventions.
- **Cancellation.** MCP carries graceful cancellation through the LLM's tool-use
  lifecycle.

### Planned transport

The intended transport layer is the PMCP SDK, with stdio as the default and TCP for
daemon mode. ⛔ **Not wired.** `pmcp` occurs in exactly **one** line of this
repository — the `TODO` in `crates/xpile-mcp/src/lib.rs`:

```rust
//! TODO: wire up actual MCP transport (likely PMCP SDK).
```

Note the hedge. Through 2026-07-29 this page said `xpile-mcp` *"uses the PMCP SDK"*
while the source it describes said *"likely"* — the code author qualified the choice and
the spec author did not. `xpile-mcp` declares no `pmcp` dependency.

### Planned server lifecycle

```
1. IDE launches `xpile mcp` (via Claude Desktop's MCP config or VS Code extension)
2. xpile-mcp announces tool surface via MCP handshake
3. LLM calls tools as needed (e.g., during a hybrid-transpile task)
4. Each tool call:
   a. Validates arguments
   b. Delegates to xpile-core (or pv/pmat for proxied tools)
   c. Returns structured result
5. On IDE close, the server exits cleanly
```

Step 1 fails today (`unrecognized subcommand 'mcp'`), so steps 2–5 are unreachable.

### Planned tool argument schemas

The intent is that MCP tool arguments are JSON-Schema-validated. ⛔ **No validation
exists**: the string `schema` occurs in **zero** lines of `crates/xpile-mcp/`, and the
crate exposes no argument type to validate. The schema below is a sketch of the intended
shape for `transpile_file`, not a schema any code serves:

```json
{
  "name": "transpile_file",
  "description": "Transpile a single source file to Rust.",
  "input_schema": {
    "type": "object",
    "properties": {
      "path": { "type": "string", "description": "Absolute path to source file" },
      "repair": {
        "type": "string",
        "enum": ["off", "on", "cached", "force"],
        "default": "off"
      },
      "max_iterations": { "type": "integer", "default": 8 }
    },
    "required": ["path"]
  }
}
```

The `repair` values and `max_iterations` belong to the planned `--repair` lane, which
does not ship either — see [cli.md](cli.md) under "Planned surface".

### Planned telemetry passthrough

The intent is that MCP tool invocations are logged with the same telemetry shape as
direct CLI invocations, with a session identifier propagating from MCP through
`xpile-core` to `xpile-agent`.

⛔ **There is no telemetry and there is no session identifier.** Measured 2026-07-30:
the string `telemetry` occurs in **zero** lines under `crates/`. The session types that
do exist carry no id — `xpile_core::TranspileSession` holds registries, an FFI manifest
and an `Option<xpile_agent::Session>`; `xpile_agent::Session` has exactly two fields,
`model_id: String` and `budget: Budget`. There is no id to propagate, so the end-to-end
auditability this section promised has no mechanism behind it. Through 2026-07-29 both
sentences were stated as fact.

### Security posture — NOT IMPLEMENTED

⛔ **This was the sharpest falsehood on the page, and it is a SECURITY claim.**

Through 2026-07-29 this section read, in the present indicative:

> `xpile-mcp` only operates on files within the project root (resolved via
> `xpile.toml` location or CWD). Arbitrary path arguments are rejected — preventing
> accidental exfiltration when a misbehaving agent asks the server to read
> `/etc/passwd`.

**Every clause is false.** There is no path handling, no project-root resolution and no
rejection logic in the 19 lines of `crates/xpile-mcp/src/lib.rs`; the crate accepts no
paths at all because it exposes no tools. The stated root-resolution mechanism is
`xpile.toml`, which **has no reader anywhere in the workspace** — the string occurs in
two tracked files, both docs (this one and [cli.md](cli.md)), and in zero lines of Rust
(PMAT-1498).

**Nothing is exposed today**, so nothing is currently at risk: there is no server to
send a path to. The defect was the published *guarantee*, which a reader had no way to
falsify — a sandbox property of a server you cannot start is unfalsifiable from the
outside, unlike a subcommand a reader disproves in one keystroke.

**Requirement on the implementation.** Path confinement is a precondition for wiring
any tool that takes a path, not a follow-up: the first implemented tool must resolve and
confine paths, and reject arguments that escape the project root, before it is reachable
over any transport. Until that lands with its own falsification test, this section
states a requirement and not a property.

---

## Status at v0.1.0 — and the deferral that expired

`McpServer::new(bind_addr)` exists as a stub. That is the entire implementation.

⚠️ **The old deferral pointed at a milestone that has already closed.** Through
2026-07-29 this page ended `Real MCP wiring lands in Phase 4.` Phase 4 of the rollout
is the **Kani** phase — see [phased-rollout.md](phased-rollout.md) under "Phase 4 detail
— Kani" — and it **shipped**: Kani runs on every PR as a required gate
(`XPILE-QUORUM-003` / PMAT-021). Phase 4 came and went without MCP wiring, which left
the page deferring to a date in its own past.

**MCP wiring is unscheduled.** It is not in the v0.1.618 window and no phase or
milestone currently owns it. A deferral to a named milestone is a claim with an expiry
date; this page will not restate one until an owner and a milestone exist together.
