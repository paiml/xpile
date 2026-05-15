# Vision and Architecture

**Section 1 of [xpile-spec.md](../xpile-spec.md).**

## Why xpile exists

Three things drive the need for a polyglot transpile workbench rather than N per-language transpilers:

1. **Architectural duplication.** depyler (Python→Rust) and decy (C→Rust) have parallel, near-identical architectures: HIR, oracle, agent, MCP, verify, llm, contracts. Every new language replicates the same ~15-crate tree. That doesn't scale.
2. **Hybrid transpilation.** A NumPy-using Python module contains both Python code (depyler's job) and C extension code (decy's job). Neither transpiler can produce a correct end-to-end Rust translation alone — the FFI boundary needs to be reasoned about, not assumed.
3. **Quality leverage.** Contracts, oracles, agent loops, and verification belong in one place where 14 frontends can share them, not 14 places where they drift.

xpile makes adding a new source language a *plug-in* operation: write one type implementing `Frontend`, register it, write semantics + translation contracts, done.

## Architectural layers

```
┌──────────────────────────────────────────────────────────────────────┐
│  xpile (CLI binary)                                                  │
├──────────────────────────────────────────────────────────────────────┤
│  xpile-core   — session orchestration                                │
├──────────────────────────────────────────────────────────────────────┤
│  xpile-agent  — bounded LLM repair loop (from alchemize)             │
│  xpile-mcp    — MCP server                                           │
├──────────────────────────────────────────────────────────────────────┤
│  xpile-oracle         xpile-rust-codegen        xpile-ffi-manifest   │
│  xpile-llm + cache    xpile-contracts (→ pv)                         │
├──────────────────────────────────────────────────────────────────────┤
│  xpile-frontend (trait)         xpile-meta-hir (canonical IR)        │
├──────────────────────────────────────────────────────────────────────┤
│  depyler-frontend   decy-frontend   ruchy-frontend   <future>        │
└──────────────────────────────────────────────────────────────────────┘
```

Dependency direction is strictly downward — no cycles. New frontends are added at the bottom; nothing above them changes.

## Design pillars

| Pillar | What it means | Where it lives |
|---|---|---|
| **One `Frontend` trait** | Every language plugs in identically | `xpile-frontend` |
| **One canonical IR** | Frontends lower to it; everything else consumes it | `xpile-meta-hir` |
| **One oracle protocol** | Capture original-language execution; compare to Rust | `xpile-oracle` |
| **One agent loop** | Adapted from alchemize; bounded; opt-in; deterministic via cache | `xpile-agent` |
| **One contract framework** | Delegated to `pv` (provable-contracts); design is YAML | `xpile-contracts` |
| **One quality regime** | `pmat tdg`, `pv lint`, `pv score`, `cargo llvm-cov`, `cargo mutants` | CI |

## What xpile is NOT

- Not a replacement for `depyler` or `decy` as user-facing names. They continue to exist; xpile is the shared substrate.
- Not a fully-unified IR. Each frontend keeps its own internal HIR; meta-HIR is the *coordination point*, not the universal type system.
- Not a build system. xpile produces `.rs` files; `cargo` builds them.
- Not a chatbot. The agent has a fixed tool surface and a single exit condition.

## Foundations

- **alchemize** — the four-tool agent loop pattern (`read_file`, `write_*`, `cargo_build`, `validate_*`)
- **aprender / provable-contracts** — papers→math→contracts→proofs pipeline
- **depyler / decy** — parallel per-language transpilers being consolidated
- **pmat** — work + quality controller across the fleet
