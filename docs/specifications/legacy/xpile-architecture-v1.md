# xpile — Architecture Specification v1

**Spec ID:** XPILE-ARCH-V1
**Status:** Draft
**Created:** 2026-05-15

---

## Purpose

xpile is a **polyglot transpile workbench**. It consolidates the parallel architectures of [depyler](https://github.com/paiml/depyler) (Python→Rust) and [decy](https://github.com/paiml/decy) (C→Rust) into a single workspace, factors out the shared concerns (oracle, agent loop, LLM cache, MCP server, contracts framework, Rust codegen), and adds the **FFI manifest** that makes *hybrid* transpilation possible — single artifacts that cross language boundaries.

Adding a new source language is "plug a new frontend in," not "fork the architecture."

## Goals

1. One workspace where the agent can see all source languages at once
2. One Rust codegen backend, fed by a canonical meta-HIR
3. One oracle abstraction, with per-language implementations
4. One agent loop, adapted from [alchemize](https://github.com/pymc-labs/alchemize)'s four-tool pattern
5. One MCP server, exposing tools to IDE assistants
6. One contracts framework, falsifiable in CI
7. **Hybrid transpiles** as first-class: Python+C, Python+CUDA, etc.

## Non-goals (v1)

- **Not** a replacement for `depyler` or `decy` as user-facing names. They continue to exist as published crates; xpile is the shared substrate. (Decision: revisit at v0.5 when actual hybrid demos motivate consolidation.)
- **Not** a "meta-IR with full type inference across languages." We start federated and lossy; grow as hybrid demand justifies.
- **Not** a build system. xpile produces `.rs` files; `cargo` builds them.

## Crate layout

```
xpile/
├── crates/
│   ├── xpile/                # CLI binary
│   ├── xpile-core/           # session orchestration
│   ├── xpile-agent/          # bounded agent loop
│   ├── xpile-oracle/         # Oracle trait
│   ├── xpile-llm/            # model invocation + content-addressed cache
│   ├── xpile-mcp/            # MCP server
│   ├── xpile-contracts/      # provable-contracts framework
│   ├── xpile-rust-codegen/   # shared Rust emission
│   ├── xpile-meta-hir/       # canonical IR
│   ├── xpile-ffi-manifest/   # cross-language boundary registry
│   ├── xpile-frontend/       # Frontend trait
│   │
│   ├── depyler-frontend/     # Python
│   ├── decy-frontend/        # C
│   └── ruchy-frontend/       # Ruchy
```

### Dependency direction (acyclic)

```
                    xpile (bin)
                        │
                    xpile-core
              ┌─────────┼─────────┬──────────────┐
              ▼         ▼         ▼              ▼
       xpile-agent  xpile-oracle  xpile-ffi-manifest
        │   │   │
        │   │   └──→ xpile-llm
        │   └──────→ xpile-rust-codegen ──→ xpile-meta-hir
        └─────────→ xpile-frontend ──→ xpile-meta-hir
                          ▲
                          │
        ┌─────────────────┼─────────────────┐
        │                 │                 │
  depyler-frontend  decy-frontend  ruchy-frontend
```

`xpile-mcp` and `xpile-contracts` are independent leaves used directly by `xpile-core`.

## The Frontend trait (load-bearing)

```rust
pub trait Frontend: Send + Sync {
    fn name(&self) -> &'static str;
    fn extensions(&self) -> &[&'static str];
    fn parse_and_lower(&self, path: &Path, source: &str) -> Result<Module, FrontendError>;
}
```

A frontend is the ONLY language-specific piece. Everything below it — meta-HIR, codegen, oracle protocol, agent loop, MCP — is language-neutral.

## Federated HIR (not unified, by design)

The meta-HIR is intentionally **lossy and minimal**. Each frontend keeps its own internal HIR (e.g., `depyler-hir`, `decy-hir`) and lowers to meta-HIR only when crossing the boundary into shared infrastructure (codegen, FFI manifest).

This is the **federated** option from the brainstorm — chosen over a full meta-IR because:

- We don't yet have hybrid demos to validate the right shape
- Each frontend can evolve language-specific optimizations in its own HIR
- The meta-HIR can grow as needed; over-designing now would lock in mistakes

Migration path to a richer meta-HIR: when a third hybrid case demands cross-language type inference, expand `xpile-meta-hir` to carry types, and add a `Frontend::infer_types` method.

## The hybrid-transpile flow

```text
$ xpile transpile --hybrid foo_module/
   ├── foo.py        (depyler-frontend)
   ├── _foo_core.c   (decy-frontend)
   └── setup.py

1. Each frontend lowers its file → meta-HIR module
2. xpile-ffi-manifest reconciles boundaries:
   - foo.py imports _foo_core.sum()
   - _foo_core.c exports PyObject* sum(PyObject* args)
   - manifest registers: sum(arr: ndarray<f64>) -> f64
3. xpile-rust-codegen emits Rust on both sides + FFI shim
4. xpile-oracle captures end-to-end CPython behavior on a fixture
5. Validates Rust matches CPython on every fixture input
6. If fails → xpile-agent loops with cargo + oracle errors as input
```

This is what neither `depyler` nor `decy` can do alone, and is the load-bearing motivation for the monorepo.

## Agent loop (adapted from alchemize)

Tools exposed to the agent:

| Tool | Purpose |
|---|---|
| `read_file(path)` | Inspect any workspace file |
| `write_file_in_lang(lang, path, content)` | Overwrite a generated .rs |
| `cargo_build()` | Compile with structured diagnostics |
| `cargo_test()` | Run quickcheck + oracle tests |
| `run_hybrid_oracle()` | Capture original Python/C/etc. behavior and compare |
| `apply_skill(name)` | Pull a markdown skill into agent context |

Exit condition: `cargo_build && run_hybrid_oracle` pass. Failure mode: budget exhaustion (8 iterations / 200K tokens / 300s default), fails closed to the original static error.

Provenance marker on every repaired file:
```rust
// xpile-repaired: <cache_key_hex> via <model_id> at <utc_iso8601>
```

## Provable contracts

The `contracts/` directory will mirror what depyler bootstrapped (see [pending: depyler #255 umbrella](https://github.com/paiml/depyler/issues/255)):

| Contract | What it asserts |
|---|---|
| `xpile-determinism-v1.yaml` | Default never runs LLM; cache key uniqueness; byte-identical output on hit |
| `xpile-budget-v1.yaml` | Per-file caps enforced; budget exhaustion fails closed |
| `xpile-provenance-v1.yaml` | Every repaired `.rs` carries marker |
| `xpile-oracle-v1.yaml` | Exit requires oracle pass |
| `xpile-frontend-trait-v1.yaml` | Every registered frontend handles its declared extensions |
| `xpile-ffi-manifest-v1.yaml` | Every cross-language call in a session is registered |

CI runs every contract's falsifiable formula; drift fails the build.

## Migration path from depyler/decy

Per the brainstorm, **extract first, merge second**:

1. **Phase 1 (weeks 1-6):** Publish `xpile-*` crates to crates.io. depyler and decy depend on them. Existing per-language repos shrink as functionality moves into xpile. **xpile and the per-language repos coexist.**
2. **Phase 2 (weeks 7-8):** `git filter-repo` + `git subtree add` to fold depyler's and decy's source into xpile, preserving history. Per-language repos become thin shims that re-export from xpile.
3. **Phase 3 (weeks 9-16):** First hybrid demo (NumPy-using Python script). Validates that the federated HIR + FFI manifest design holds.
4. **Phase 4 (months 5-6):** Add CUDA and C++ frontends. Each is a 2-4 week effort instead of "fork the architecture."

## Open questions

1. **Where does the model call live?** In-process via the Anthropic SDK, or out-of-process via `xpile-mcp` talking to Claude Code? Probably both — SDK for CI/scripted use; MCP for interactive IDE use.
2. **Cache location.** `~/.cache/xpile/` per user, or project-local? Per-user is simpler; project-local enables reproducibility across machines via committed caches.
3. **Should we publish a single `xpile` binary or keep `depyler` and `decy` as separate user-facing CLIs?** Strong case for the latter — preserves brand and discoverability, monorepo is an implementation detail.
4. **Licensing of generated Rust.** When the agent repairs code, what license does the output carry? (Probably matches the source, with the provenance marker noting the LLM's role.)

## References

- depyler repair mode spec: `~/src/depyler/docs/specifications/depyler-repair-mode.md` (branch `feat/repair-mode-spec`)
- alchemize compiler.py: agent loop pattern
- aprender contracts/: provable-contracts pattern
- ruchy: https://github.com/paiml/ruchy
