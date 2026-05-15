# Contract Taxonomy

**Section 13 of [xpile-spec.md](../xpile-spec.md).**

## Lanes vs. layers

xpile contracts sit at the intersection of two **lanes** (code vs. proof) and five **layers** (semantics, translation, architectural, hybrid, compile-time). Lanes and layers are orthogonal — a contract has exactly one lane and exactly one layer.

- **Code lane**: produced/consumed by `Frontend` and `Backend` impls. Models executable code: meta-HIR, FFI manifest, emitted Rust / PTX / WGSL / etc.
- **Proof lane**: produced/consumed by `ContractFrontend` and `ContractBackend` impls. Models notation and proofs: LaTeX math, Lean 4 theorems, mdBook.

Both lanes share the same contract YAML substrate (`contracts/*.yaml`) — this taxonomy doc covers both. A contract's optional `xpile.lane` metadata field (`code` or `proof`) tags which lane it belongs to. Most existing contracts are code-lane; the proof-lane ones added 2026-05-15 (e.g., `notation-latex-math-to-equation-v1.yaml`, `xlate-rust-fn-to-lean-thm-v1.yaml`) carry `xpile.lane: proof`.

## Two real kinds + five xpile layers

`pv`'s schema accepts only `kernel`, `registry`, `model-family`, `model-family-variant`, `tokenizer`, `training-loop`, `pretraining-corpus`, `pattern`, `schema` as `metadata.kind` values. xpile uses two of these:

| `pv` `kind` | When | Validation requirements |
|---|---|---|
| `kernel` | Computational contracts with formal equations and proof obligations | MUST have non-empty `proof_obligations`, `falsification_tests`, AND `kani_harnesses`; `falsification_tests.len() ≥ proof_obligations.len()` |
| `pattern` | Cross-cutting / process / architectural invariants | Lighter; pattern contracts can omit kani harnesses |

On top of that, xpile organizes its contracts by **layer** — a metadata tag for the team's mental model, not enforced by `pv`:

| xpile layer | `pv` kind | Examples |
|---|---|---|
| **Layer 1: Language semantics** | `kernel` | `py-int-arith-v1.yaml`, `c-pointer-arith-v1.yaml`, `ruchy-pipeline-op-v1.yaml` |
| **Layer 2: Translation** | `kernel` | `xlate-py-list-to-vec-v1.yaml`, `xlate-c-struct-to-rust-v1.yaml` |
| **Layer 3: Architectural** | `pattern` | `xpile-frontend-trait-v1.yaml`, `xpile-backend-trait-v1.yaml`, `xpile-oracle-v1.yaml` |
| **Layer 4: Hybrid pipeline** | `pattern` | `ffi-cpython-ext-v1.yaml`, `ffi-pybind11-v1.yaml`, `ffi-cuda-kernel-v1.yaml` |
| **Layer 5: Compile-time / IR** | `pattern` | `compile-rust-to-ptx-mma-v1.yaml`, `compile-rust-to-wgsl-buffer-v1.yaml`, `compile-rust-to-spirv-v1.yaml` |

## Layer 1 — Language semantics

Encodes operational semantics of source-language constructs. One contract per construct family. The "spec" of the source language, made machine-checkable.

Example: `py-int-arith-v1.yaml`

- Equations: `addition_no_overflow`, `addition_overflow_promotion`, `multiplication_quadratic_promotion`, `division_floor_semantics`, `bitwise_and_signed_semantics`
- Generates: Rust trait stubs + Kani harnesses at i8 bit width (lifted to i64 via Lean)

## Layer 2 — Translation

Encodes how a Layer-1 construct lowers to Rust. Multiple translations may exist per construct (naïve vs. optimized).

Example: `xlate-py-list-to-vec-v1.yaml`

- Equations: `homogeneous_list_to_vec`, `heterogeneous_list_rejected`, `alias_observation_inserts_clone`, `iteration_order_preserved`, `length_method`
- Generates: the actual emission function in `xpile-rust-codegen` + a Kani-checked equivalence harness

## Layer 3 — Architectural

Encodes invariants the transpiler itself preserves. Pure xpile-internal.

Example: `xpile-frontend-trait-v1.yaml`

- Equations: `extension_ownership`, `parse_idempotency`, `source_lang_consistency`, `ffi_boundaries_are_outgoing_only`
- Generates: failing tests for each invariant + audit-chain entries

## Layer 4 — Hybrid pipeline

End-to-end contracts spanning multiple frontends. The load-bearing reason for xpile to exist.

Example: `ffi-cpython-ext-v1.yaml`

- Equations: `manifest_completeness`, `refcount_balance_on_success`, `refcount_balance_on_error`, `gil_invariant`, `buffer_protocol_zero_copy`, `oracle_endtoend_equivalence`
- Generates: scaffold for the FFI shim codegen + end-to-end oracle test harnesses

## Layer 5 — Compile-time / IR

Encodes invariants of the **emitted artifact** — not the source language, not the translation step, but the compiled output (Rust → PTX / WGSL / SPIR-V / binary). Layer 5 contracts assert properties that can only be checked *after* a backend has lowered meta-HIR to a target IR.

Adopted from aprender's `PerformanceContract` pattern (`crates/aprender-compute/contracts/cgp/cgp-gpu-mma-64x128-pipeline-v1.yaml`), but kept under `kind: pattern` for `pv` compatibility — the team-only `xpile.layer: compile` marker carries the distinction.

Example: `compile-rust-to-ptx-mma-v1.yaml`

- Equations: `mma_emission_for_gemm_kernel`, `cp_async_pipeline_overlap`, `shared_memory_budget`, `output_equivalence_to_cublas`
- Generates: a backend selection test in `xpile-ptx-codegen` + a PTX-text inspector + an `nvcc`-link smoke test

Layer 5 contracts introduce a top-level **`compile_targets:`** block carrying hardware/IR scope:

```yaml
compile_targets:
  - target: ptx
    hardware:
      compute_capability_min: "sm_80"
      compute_capability_max: "sm_90"
    via:
      - rustc_codegen_nvvm
```

Falsification tests at Layer 5 inspect the emitted IR text or binary directly:

```yaml
falsification_tests:
  - id: FALSIFY-COMPILE-PTX-001
    rule: emitted PTX must contain mma.sync, not scalar mad fallback
    test: grep -c 'mma\.sync\.aligned' target/ptx/gemm.ptx ; expect ≥ 1
```

Why not a new `kind`? Aprender's `kind: PerformanceContract` is custom to their validator. xpile stays compatible with vanilla `pv` by using `kind: pattern` + `xpile.layer: compile`. If `pv` upstream later adopts a compile/performance kind, xpile contracts migrate by changing one field.

### Layer 5 limitations — necessary, not sufficient

The 2026-05-15 audit (`docs/specifications/audit-design.md` §4 "Oracle Hardware Blind Spots Re-emerge" and Hypothesis 4) flagged a real composition risk: per-construct Layer 5 contracts validated individually do not prove safe *interactions* across constructs. Examples of what a single Layer 5 contract CANNOT guarantee:

- **Cross-thread memory ordering**: each `mma.sync` and `cp.async` validates against its own contract, but their joint emission may still race without correctly placed memory barriers.
- **Resource composition**: SMEM, registers, and warps each have per-construct bounds, but the *sum* over a whole kernel can exceed launch limits even when no single construct violates its bound.
- **Effect interleaving**: a sequence of contracted instructions can establish an invariant individually that the composition violates (e.g., aliasing assumptions that hold pairwise but not transitively).

The xpile design accepts this as a **known limit** rather than papering over it with composition contracts. Real-hardware Oracle runs on representative fixtures remain the final gate for execution-level correctness, and the test suite for any Layer 5 backend MUST include a "composition fixture" that exercises the cross-construct interaction surface. A future Layer 5b (composition contracts taking multiple Layer 5 IDs as `depends_on`) is reserved as an option if real-world incidents reveal patterns worth formalizing.

**Guidance for new Layer 5 contracts (Option Y of the audit response):** prefer **kernel-scoped** contracts (asserting properties of the whole emitted function, e.g., `mma_emission_for_gemm_kernel`) over **instruction-scoped** ones (asserting a single `mma.sync` site in isolation). Kernel scope collapses the composition problem inside the contract's purview rather than scattering it across many contracts. The existing `compile-rust-to-ptx-mma-v1.yaml` follows this convention.

## Layer determines lifecycle, not validation

Layers help humans navigate. Validation is `pv`'s — driven by `kind`. The same contract file passes `pv lint` whether you call it a Layer-3 or a Layer-4 (since both are `kind: pattern`); the difference is only how the team groups it in `pv query` results.

## Tagging via XpileContractLayer

```rust
// In xpile-contracts
pub enum XpileContractLayer {
    LanguageSemantics,
    Translation,
    Architectural,
    HybridPipeline,
    CompileTime,
}
```

Contract YAML can carry optional `xpile.layer` and `xpile.lane` fields in `metadata`:

```yaml
metadata:
  id: C-PY-INT-ARITH
  kind: kernel              # pv-validated
  xpile:
    layer: language_semantics   # team-only metadata
    lane: code                  # code | proof
```

Valid `xpile.layer` values: `language_semantics`, `translation`, `architectural`, `hybrid_pipeline`, `compile`.
Valid `xpile.lane` values: `code`, `proof`.

`pv` ignores unknown subfields; xpile tooling reads `xpile.layer` and `xpile.lane` for navigation, dispatch, and audit-chain reporting.
