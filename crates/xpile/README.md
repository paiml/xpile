# xpile

**Polyglot transpile workbench with provable contracts at every layer.**

Status: **v0.0.1 — crates.io name reservation.** The real CLI lands in v0.1.0+. The full workspace, design specs, and contracts are being built in the open at <https://github.com/paiml/xpile>.

## Vision

xpile is a contract-driven polyglot transpile workbench. One CLI binary, one umbrella library, a five-layer × two-lane contract taxonomy, and a pluggable Frontend / Backend / ContractFrontend / ContractBackend trait surface.

**Code lane (executable code):**

```
Frontends                                 Backends
─────────────────                         ─────────────────
Python   →┐                            ┌→ Rust
C        →┤                            ├→ Ruchy
C++      →┼→  meta-HIR  →─────────────→┼→ PTX
Rust     →┤                            ├→ WGSL
Ruchy    →┤                            ├→ SPIR-V
Lean 4   →┘                            └→ Lean 4
```

**Proof lane (notation and proofs):**

```
ContractFrontends                         ContractBackends
─────────────────                         ─────────────────
LaTeX       →┐                          ┌→ LaTeX
Lean 4 thm  →┼→ contract equations →───┼→ Lean 4 theorems
mdBook      →┘                          └→ mdBook
```

Lean 4 is the only language that spans both lanes. LaTeX is proof-lane-only.

## Provable contracts

Every translation, IR construct, and emitted artifact cites a contract under `contracts/*.yaml`. Five layers:

1. Language semantics (Python int arith, C pointer arith)
2. Translation (Python list → Rust `Vec<T>`, Lean inductive → Rust enum)
3. Architectural (trait invariants — `extension_ownership`, `parse_idempotency`, ...)
4. Hybrid FFI (CPython refcount balance, pybind11, CUDA kernel boundaries)
5. Compile-time / IR (PTX `mma.sync` emission, WGSL buffer alignment, SPIR-V version gating)

Validated via `pv lint` (the [aprender-contracts](https://crates.io/crates/aprender-contracts) provable-contracts framework).

## Install

```sh
cargo install xpile
```

This installs the v0.0.1 placeholder binary. The real CLI follows.

## License

MIT OR Apache-2.0.
