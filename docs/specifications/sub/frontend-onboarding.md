# Frontend Onboarding

**Section 17 of [xpile-spec.md](../xpile-spec.md).**

## Seven-step checklist

Adding a new source language to xpile is bounded work, not architecture surgery:

1. **Add a `SourceLang` variant** in `xpile-meta-hir`
2. **Create the crate** at `crates/<lang>-frontend/`
3. **Implement `Frontend`** (3 methods: `name`, `extensions`, `parse_and_lower`)
4. **Wire the parser** — usually by adopting an existing parser crate as a dependency
5. **Author a Layer-1 semantics contract** for one core construct
6. **Author a Layer-2 translation contract** for that construct → Rust
7. **Add a corpus regression test** under `crates/<lang>-frontend/tests/corpus/`

Phase 2 of the rollout does this for Python (depyler-frontend ↔ rustpython-parser); Phase 6+ for new languages.

## Step-by-step (with a hypothetical Zig frontend)

### Step 1 — Meta-HIR variant

```rust
// crates/xpile-meta-hir/src/lib.rs
pub enum SourceLang {
    Python,
    C,
    Cpp,
    Cuda,
    Ruchy,
    Zig,    // NEW
}
```

This single addition is what every other crate needs to know about the new language existing.

### Step 2 — Crate scaffold

```bash
cargo new --lib crates/zig-frontend
```

Edit the `crates/zig-frontend/Cargo.toml`:

```toml
[package]
name = "zig-frontend"
version.workspace = true
edition.workspace = true
description = "Zig frontend for xpile. Parses .zig and lowers to meta-HIR."

[dependencies]
xpile-frontend = { workspace = true }
xpile-meta-hir = { workspace = true }
# tree-sitter-zig or similar parser dep here
```

Add to the root workspace `Cargo.toml`:

```toml
members = [..., "crates/zig-frontend"]
```

### Step 3 — Implement `Frontend`

```rust
// crates/zig-frontend/src/lib.rs
use std::path::Path;
use xpile_frontend::{Frontend, FrontendError};
use xpile_meta_hir::{Module, SourceLang};

pub struct ZigFrontend;

impl Frontend for ZigFrontend {
    fn name(&self) -> &'static str { "zig" }
    fn extensions(&self) -> &[&'static str] { &["zig"] }
    fn parse_and_lower(&self, path: &Path, source: &str) -> Result<Module, FrontendError> {
        // ... parse + lower
    }
}
```

### Step 4 — Wire the parser

Adopt an existing Zig parser. Tree-sitter-zig is the lowest-friction option. depyler does the same with rustpython-parser; decy with clang/tree-sitter-c.

The parse step produces a Zig-specific AST or HIR. The lowering step walks that AST and emits `xpile_meta_hir::Module`.

### Step 5 — Layer-1 semantics contract

Pick one core construct (e.g., Zig's `comptime` evaluation). Write:

```yaml
# contracts/zig-comptime-v1.yaml
metadata:
  id: C-ZIG-COMPTIME
  kind: kernel
  ...
equations:
  comptime_eval_pure:
    formula: |
      comptime_eval(e) yields a constant if e is pure
    ...
```

Run `pv lint contracts/zig-comptime-v1.yaml`. Iterate until 8/8 gates pass.

### Step 6 — Layer-2 translation contract

```yaml
# contracts/xlate-zig-comptime-to-rust-const-v1.yaml
metadata:
  id: C-XLATE-ZIG-COMPTIME-TO-RUST-CONST
  kind: kernel
  ...
```

This contract drives the emission function in `xpile-rust-codegen`.

### Step 7 — Corpus regression test

```
crates/zig-frontend/tests/corpus/
├── 01_pure_comptime.zig            # input
├── 01_pure_comptime.expected.rs    # what xpile should produce
└── ...
```

`cargo test -p zig-frontend --test corpus` parses each `.zig`, transpiles, and diffs against the expected `.rs`.

## What you do NOT have to do

You **do not** have to:

- Touch `xpile-core`, `xpile-agent`, `xpile-oracle`, `xpile-llm`, `xpile-mcp`, `xpile-rust-codegen`, `xpile-ffi-manifest`, `xpile-contracts`, or `xpile-frontend`
- Write a new agent loop
- Write a new oracle
- Write new MCP tools
- Touch CI

All shared infrastructure picks up the new frontend automatically once `TranspileSession::register_frontend(Arc::new(ZigFrontend))` is added in `xpile-core`.

## Effort estimate per language

Based on the alchemize / depyler / decy experience:

| Sub-task | Effort |
|---|---|
| Steps 1-3 (scaffold + trait impl) | 1-2 days |
| Step 4 (parser wiring) | 1-2 weeks (depending on parser maturity) |
| Steps 5-6 (one contract pair) | 3-5 days |
| Step 7 (corpus regression) | 1-3 days |
| Total to first end-to-end demo | **2-4 weeks** |

Subsequent constructs in the same language are an additional ~1 week per construct (one Layer-1 + one Layer-2 contract).

## Ruchy as the canary

`ruchy-frontend` exists at v0.1.0 specifically so the onboarding path is exercised from day one. If the path proves painful for Ruchy (which is a paiml-native language with good interop story), it's the signal that the trait needs refinement before C++ / CUDA / Zig onboard.
