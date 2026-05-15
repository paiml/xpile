# FFI Manifest

**Section 4 of [xpile-spec.md](../xpile-spec.md).**

## Purpose

In a hybrid transpile session, the agent must know exactly which symbols cross language lines and what their Rust shim signatures should be. The FFI manifest is the **single source of truth** for that mapping.

Without an explicit manifest, both transpilers (depyler-frontend, decy-frontend) would have to guess at boundary semantics, and any guess is a chance for divergence. The manifest is what makes hybrid transpile *symbolically* tractable instead of best-effort.

## Shape

```rust
pub struct FfiManifest {
    pub entries: Vec<FfiEntry>,
}

pub struct FfiEntry {
    pub symbol: String,
    pub from_lang: SourceLang,
    pub to_lang: SourceLang,
    pub source_signature: String,
    pub rust_shim_signature: String,
    pub shim_id: String,
}
```

## Reconciliation flow

```
1. Each frontend records outgoing FFI boundaries in its Module.ffi_boundaries.
2. xpile-core::reconcile_manifest() walks all parsed Modules and:
   a. Matches outgoing symbols against incoming declarations in other Modules
   b. Synthesizes a rust_shim_signature from the source signatures
   c. Generates a shim_id (sha256 of normalized signature)
   d. Adds an FfiEntry to the manifest
3. xpile-rust-codegen consumes the manifest to emit FFI shims.
4. xpile-oracle uses the manifest to know which calls to instrument
   (refcount tracking, GIL state, etc.).
```

## Invariants

From [`contracts/ffi-cpython-ext-v1.yaml`](../../../contracts/ffi-cpython-ext-v1.yaml):

| Invariant | What it asserts |
|---|---|
| `manifest_completeness` | Every cross-language call in any parsed module has an entry |
| `refcount_balance_on_success` | PyObject* refcount unchanged on successful return |
| `refcount_balance_on_error` | Refcount unchanged on error path (the most common CPython bug) |
| `gil_invariant` | GIL is held wherever CPython API is touched |
| `buffer_protocol_zero_copy` | ndarray passthrough is `O(1)`, not `O(N)` |
| `oracle_endtoend_equivalence` | Transpiled Rust matches CPython on every fixture input |

## Hybrid-specific languages

| Source pair | Manifest annotation |
|---|---|
| Python ↔ C (CPython API) | `convention: cpython`, refcount semantics auto-tracked |
| Python ↔ C++ (pybind11) | `convention: pybind11`, RAII inferred |
| Python ↔ CUDA (`@cuda.jit`) | `convention: cuda_kernel`, device-side memory layout in `source_signature` |
| Ruchy ↔ Python (data interop) | `convention: pyo3`, type marshalling per `pyo3::FromPyObject` |

## Status at v0.1.0

`FfiManifest::new()` and `FfiManifest::register()` work. Reconciliation logic is a stub (`reconcile_manifest()` returns an empty manifest). Phase 5 (hybrid pipeline demo) implements reconciliation.
