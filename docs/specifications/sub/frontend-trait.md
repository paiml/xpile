# Polyglot Frontend Trait

**Section 2 of [xpile-spec.md](../xpile-spec.md).**

## Definition

```rust
pub trait Frontend: Send + Sync {
    fn name(&self) -> &'static str;
    fn extensions(&self) -> &[&'static str];
    fn parse_and_lower(&self, path: &Path, source: &str) -> Result<Module, FrontendError>;
}
```

Three methods. The trait is intentionally narrow because everything else (agent, oracle, codegen, MCP, contracts) is shared.

## Invariants

Encoded in [`contracts/xpile-frontend-trait-v1.yaml`](../../../contracts/xpile-frontend-trait-v1.yaml):

| Invariant | What it asserts |
|---|---|
| `extension_ownership` | No two frontends declare the same extension |
| `parse_idempotency` | `hash(parse(p, s)) == hash(parse(p, s))` — no mutable state, canonical serialization |
| `source_lang_consistency` | Module's `source_lang` matches the producing frontend's declared language |
| `ffi_boundaries_are_outgoing_only` | Frontends record outgoing calls only; incoming reconciliation is the FFI manifest's job |

## Implementations at v0.1.0

| Crate | Type | Extensions | Status |
|---|---|---|---|
| `depyler-frontend` | `PythonFrontend` | `py`, `pyi` | Scaffold (empty module) |
| `decy-frontend` | `CFrontend` | `c`, `h` | Scaffold (empty module) |
| `ruchy-frontend` | `RuchyFrontend` | `ruchy` | Scaffold (empty module) |

Real parser integration is Phase 2 of the rollout:

- `depyler-frontend` adopts rustpython-parser + the existing depyler-hir lowering
- `decy-frontend` adopts the existing decy-parser (clang/tree-sitter based)
- `ruchy-frontend` depends on the `ruchy` crate from crates.io and reuses its parser + AST

## Why object-safe

The trait uses `&dyn Frontend` in `xpile-core::TranspileSession::frontends` to allow dynamic dispatch by file extension. That requires:

- No associated types (so `parse_and_lower` returns the concrete `Module` type, not `Self::Hir`)
- All methods take `&self` (so the trait can be a trait object)
- `Send + Sync` (so sessions are usable across threads)

## Adding a new frontend

See [frontend-onboarding.md](frontend-onboarding.md). The seven steps are:

1. Add a variant to `xpile_meta_hir::SourceLang`
2. Create `crates/<lang>-frontend/`
3. Implement `Frontend`
4. Wire its parse/lower
5. Author a Layer-1 contract for one construct
6. Author a Layer-2 contract for that construct → Rust
7. Add a corpus regression test
