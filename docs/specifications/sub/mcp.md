# MCP Server

**Section 15 of [xpile-spec.md](../xpile-spec.md).**

## Purpose

`xpile-mcp` exposes xpile's tools via the [Model Context Protocol](https://modelcontextprotocol.io/) so IDE assistants (Claude Code, Claude Desktop, VS Code, JetBrains) can call them without spawning subprocesses or maintaining their own state.

Mirrors the pattern in `depyler-mcp` (PMCP-powered) and `decy-mcp`.

## Tools exposed (v1)

| Tool | What it does |
|---|---|
| `transpile_file(path, options)` | Transpile a single file; return generated Rust or error |
| `transpile_hybrid(dir, options)` | Hybrid transpile; return Rust files + FFI manifest |
| `inspect_meta_hir(path)` | Return the meta-HIR Module as JSON |
| `inspect_ffi_manifest(dir)` | Return the FFI manifest for a hybrid session |
| `lint_contracts(dir)` | Delegate to `pv lint`; return structured findings |
| `score_contracts(dir)` | Delegate to `pv score`; return per-contract scores |
| `query_contracts(query, kind?)` | Delegate to `pv query`; return matching contracts |

## Why MCP and not a raw HTTP API

- **Auth inheritance.** MCP servers run under the user's IDE auth — no separate key management for xpile.
- **Tool-use-native.** LLMs already know how to use MCP tools; they don't have to learn xpile-specific HTTP conventions.
- **Cancellation.** MCP carries graceful cancellation through the LLM's tool-use lifecycle.

## Transport

`xpile-mcp` uses the PMCP SDK (same as `depyler-mcp`). Default transport: stdio. Optional transport: TCP for daemon mode.

```bash
xpile mcp                          # foreground, stdio
xpile mcp --port 3000              # daemon, TCP
xpile mcp --port 3000 --background # daemon, background
```

## Server lifecycle

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

## Tool argument schemas

All MCP tool arguments are JSON-Schema-validated. Example (`transpile_file`):

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

## Telemetry passthrough

MCP tool invocations are logged with the same telemetry shape as direct CLI invocations. The session ID propagates from MCP → xpile-core → xpile-agent, so a hybrid-transpile session triggered from an IDE is auditable end-to-end.

## Security posture

`xpile-mcp` only operates on files within the project root (resolved via `xpile.toml` location or CWD). Arbitrary path arguments are rejected — preventing accidental exfiltration when a misbehaving agent asks the server to read `/etc/passwd`.

## Status at v0.1.0

`McpServer::new(bind_addr)` exists as a stub. Real MCP wiring lands in Phase 4.
