# xpile — Contract-Driven Quality Design v1

**Spec ID:** XPILE-CONTRACT-DRIVEN-V1
**Status:** Draft
**Created:** 2026-05-15
**Supersedes scope of:** XPILE-ARCH-V1 (quality regime only — architecture spec remains canonical for crate layout)
**Depends on:** [`aprender-contracts`](https://github.com/paiml/aprender/tree/main/crates/aprender-contracts) (the "Papers to Math to Contracts in Code" framework)

---

## Thesis

**Contracts are the canonical artifact in xpile.** Spec markdown, Rust trait stubs, property tests, Kani proof harnesses, and Lean 4 theorems are *generated from* the YAML contracts under `contracts/`. The architecture doc you read first is now downstream of the architectural contracts, not upstream of them.

This is a deliberate inversion from the lightweight "spec.md + contracts/ as falsification checks" pattern that depyler bootstrapped (see depyler #240/#255). For xpile we adopt aprender's heavier model because **transpilation correctness is exactly the kind of property formal methods are good at**, and because hybrid transpilation (Python+C, Python+CUDA) is only tractable when the boundary is a verifiable contract.

## Why xpile in particular benefits

| Property | Why contracts win |
|---|---|
| **Semantic equivalence** (source program ≡ Rust output) | Decidable on bounded inputs via Kani; provable via Lean for arithmetic kernels. Hand-written tests are samples; proofs are universal quantifiers. |
| **FFI boundary correctness** | The boundary is the contract — no other place in the code is the source of truth. Two transpilers (depyler-frontend + decy-frontend) can't disagree if they bind to the same equation. |
| **Drift between spec and code** | Structurally impossible — the spec is generated from the contract, the code stubs are generated from the contract. There is no "spec says X, code does Y" failure mode. |
| **Long-tail edge cases** | `kani_gen` finds counterexamples that randomized tests miss (integer overflow on specific bit patterns, alias witness pairs). |
| **Cross-language reasoning** | A canonical equation form (e.g., `python_int_add ≡ wrapping_add WHEN no overflow`) lets the FFI manifest reconcile boundary semantics symbolically, not by example. |

## Contract taxonomy for xpile

Four layers, ordered by abstraction:

### Layer 1 — Language semantics contracts (per source language)

Encode operational semantics of source-language constructs. One contract per construct family.

Examples:
- `C-PY-INT-ARITH-V1` — Python `int` arithmetic (with bigint promotion)
- `C-PY-LIST-INDEX-V1` — list indexing, including negative indices
- `C-C-POINTER-ARITH-V1` — C pointer arithmetic, defined behavior bounds
- `C-C-INTEGER-OVERFLOW-V1` — signed overflow is UB, unsigned wraps
- `C-RUCHY-PIPELINE-OP-V1` — Ruchy `|>` pipeline semantics

**Generated:** Rust trait stub for each operation, property tests on the operational equation, Kani harness for the equivalence claim.

### Layer 2 — Translation contracts (source → Rust mapping)

Encode how a Layer-1 construct lowers to Rust. Multiple translations may exist per construct (e.g., naïve vs. optimized).

Examples:
- `C-XLATE-PY-INT-TO-I64-V1` — Python int → Rust i64 in the no-overflow domain
- `C-XLATE-PY-INT-TO-BIGINT-V1` — Python int → `num_bigint::BigInt` in the overflow domain
- `C-XLATE-PY-LIST-TO-VEC-V1` — Python list → `Vec<T>`, with reference-semantics caveat
- `C-XLATE-C-STRUCT-TO-RUST-V1` — C struct → Rust `#[repr(C)]` struct

**Generated:** the actual codegen function in `xpile-rust-codegen`, plus a Kani-checked equivalence harness.

### Layer 3 — Architectural contracts (xpile-internal invariants)

Encode invariants the transpiler itself preserves. Most of these were drafted in the depyler-repair work and ported here.

Examples:
- `C-XPILE-FRONTEND-TRAIT-V1` — every registered frontend owns its extensions; `parse_and_lower` is idempotent
- `C-XPILE-ORACLE-V1` — exit requires both `cargo build` and oracle pass
- `C-XPILE-AGENT-BUDGET-V1` — per-file caps, fails closed
- `C-XPILE-DETERMINISM-V1` — cache key uniqueness, byte-identical replay
- `C-XPILE-PROVENANCE-V1` — every repaired `.rs` carries a marker
- `C-XPILE-FFI-MANIFEST-V1` — every cross-language call in a session is registered

**Generated:** failing tests for each invariant, audit-chain entries, the `xpile-architecture-v1.md` doc itself.

### Layer 4 — Hybrid pipeline contracts (end-to-end)

Encode end-to-end behavior of multi-language transpiles. The load-bearing reason for xpile to exist.

Examples:
- `C-FFI-CPYTHON-EXT-V1` — CPython C extension boundary (Python + C)
- `C-FFI-PYBIND11-V1` — Python + C++ via pybind11
- `C-FFI-CUDA-KERNEL-LAUNCH-V1` — Python host + CUDA device kernel
- `C-FFI-NUMPY-ARRAY-PASSTHROUGH-V1` — NumPy ndarray semantics preserved across the boundary

**Generated:** scaffold for the FFI shim codegen, end-to-end oracle test harnesses, the `--hybrid` CLI mode's test corpus.

## What gets generated from a single contract

A contract YAML under `contracts/foo-v1.yaml` produces:

```
contracts/foo-v1.yaml
   │
   ├─→ target/contracts/scaffold/foo.rs      (failing Rust stubs)
   ├─→ target/contracts/probar/foo_test.rs   (property tests)
   ├─→ target/contracts/kani/foo_harness.rs  (#[kani::proof] harnesses)
   ├─→ target/contracts/lean/Foo.lean        (theorem stubs, only if math-dense)
   ├─→ target/contracts/coq/foo.v            (only if Lean too restrictive)
   ├─→ docs/book/contracts/foo.md            (generated mdBook page)
   └─→ README.md numeric claims              (via readme_gen drift detection)
```

`target/contracts/` is gitignored. Regeneration is idempotent (`make contracts`). The contract is the only thing checked into git for the "claim"; everything else is recomputable.

## The pipeline

```
┌───────────────────────────────────────────────────────────────────┐
│                                                                   │
│  1. Author writes contracts/foo-v1.yaml                           │
│                                                                   │
│  2. CI runs `cargo run -p xpile-contracts-cli -- lint`            │
│      → schema validation                                          │
│      → audit chain check                                          │
│      → drift detection vs. previous version                       │
│                                                                   │
│  3. `make contracts` generates:                                   │
│      → Rust scaffold (initially failing tests)                    │
│      → Kani harnesses                                             │
│      → Lean theorem stubs                                         │
│      → Book pages                                                 │
│                                                                   │
│  4. Engineer implements the bound function                        │
│      → tests pass                                                 │
│      → Kani verifies                                              │
│      → Lean theorem closed (if applicable)                        │
│                                                                   │
│  5. CI on every PR:                                               │
│      → cargo test -p xpile-contracts --lib (validation)           │
│      → cargo test --features kani (bounded proofs)                │
│      → coverage report (every equation must bind to code)         │
│      → audit chain unbroken                                       │
│      → PMAT grade ≥ A-                                            │
│                                                                   │
│  6. Contract status: draft → enforced                             │
│      Once enforced, drift fails CI hard.                          │
│                                                                   │
└───────────────────────────────────────────────────────────────────┘
```

## Quality gates (CI-enforced)

| Gate | Threshold | Source |
|---|---|---|
| **PMAT TDG grade** | ≥ A- | `pmat tdg` |
| **Line coverage** | ≥ 95% | `cargo llvm-cov` (NOT tarpaulin) |
| **Mutation coverage** | ≥ 80% | `cargo mutants` |
| **Contract obligation coverage** | 100% (every equation has a binding) | `aprender-contracts::coverage` |
| **Audit chain integrity** | unbroken (paper/spec → contract → code → proof) | `aprender-contracts::audit` |
| **Drift detection** | no breaking changes without version bump | `aprender-contracts::diff` |
| **Kani proofs (enforced contracts)** | all pass | `cargo kani` |
| **Clippy** | zero warnings (`-D warnings`) | `cargo clippy` |
| **Cargo deny advisories** | zero unyanked advisories | `cargo deny check` |
| **Provable-contracts lint** | zero violations | `cargo run -p xpile-contracts-cli -- lint` |

These are not aspirations — they're CI gates. A PR that drops coverage by 0.5% fails. A contract whose Kani proof regresses fails. A README claim that no longer matches a contract fails.

## Scaffold migration

The hand-written scaffold from XPILE-ARCH-V1 is treated as **draft stubs**. Each crate's central trait or struct gets a corresponding architectural contract:

| Crate | Current (hand-written) | Future (generated from contract) |
|---|---|---|
| `xpile-frontend` | `Frontend` trait by hand | Generated from `C-XPILE-FRONTEND-TRAIT-V1` |
| `xpile-oracle` | `Oracle` trait by hand | Generated from `C-XPILE-ORACLE-V1` |
| `xpile-agent` | `Session`, `Budget` by hand | Generated from `C-XPILE-AGENT-BUDGET-V1` |
| `xpile-ffi-manifest` | `FfiManifest`, `FfiEntry` by hand | Generated from `C-XPILE-FFI-MANIFEST-V1` |
| `xpile-rust-codegen` | `emit_module` stub | Per-translation contracts (Layer 2) generate the actual emission functions |
| `docs/specifications/xpile-architecture-v1.md` | Hand-written | Generated via `book_gen` from the Layer-3 contracts |
| `README.md` (quality claims table) | Hand-written numbers | Generated via `readme_gen` |

**Rule:** any hand-written code that survives past Phase 3 (below) must have an explicit `# manual: <justification>` annotation in the contract that *would* govern it. No silent hand-writing.

## Phased rollout

| Phase | Scope | Exit criterion |
|---|---|---|
| **0. Dependency wiring** | Add `aprender-contracts` as a workspace dep; `xpile-contracts` re-exports its schema types; lint runs in CI | `cargo run -p xpile-contracts-cli -- lint` succeeds on an empty contract set |
| **1. Architectural contracts** | Author all 6 Layer-3 contracts (frontend, oracle, agent, manifest, determinism, provenance) | All 6 contracts in `enforced` status; existing hand-written stubs validated against them |
| **2. Python semantics starter set** | 5 Layer-1 contracts: int arith, list indexing, dict get, function call, attribute access | 5 contracts in `enforced` status with Kani harnesses passing |
| **3. Codegen replacement** | Generate `xpile-rust-codegen` emission functions from Layer-2 translation contracts | At least 3 Python constructs end-to-end via generated codegen, hand-written code deleted |
| **4. Kani equivalence proofs** | Wire `kani_gen` for Layer-1 + Layer-2; CI enforces Kani for arithmetic contracts | All arithmetic contracts have passing Kani proofs at default unwind depth |
| **5. Hybrid pipeline contract** | First Layer-4 contract: `C-FFI-CPYTHON-EXT-V1`; one demo (NumPy-using `.py` + companion `.c`) | End-to-end oracle pass on the demo; FFI shim generated from manifest |
| **6. Lean theorems** | Layer-1 arithmetic contracts get Lean 4 theorems for unbounded properties | At least 3 theorems closed; CI rejects PRs that break theorem statements |

## Concrete example contracts

See alongside this doc:
- `contracts/xpile-frontend-trait-v1.yaml` — Layer 3 (architectural)
- `contracts/py-int-arith-v1.yaml` — Layer 1 (Python semantics)
- `contracts/xlate-py-list-to-vec-v1.yaml` — Layer 2 (translation)
- `contracts/ffi-cpython-ext-v1.yaml` — Layer 4 (hybrid)

These are *full* contracts in aprender-contracts schema — metadata, equations with domain/codomain/invariants/preconditions, proof_obligations, kani harnesses, Lean theorem references. Use them as templates for new contracts.

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| `aprender-contracts` is in active development (v0.33.0); version churn could break xpile | Path-dep for now; pin to a tagged commit before first crates.io release |
| Kani has size limits — not every semantic equivalence is decidable in finite time | Bounded-unwind config per contract; full Lean theorem only for the contracts that need it |
| Writing rich contracts is slow — initial velocity loss is real | Phase 0-1 is intentionally small (6 contracts) before scaling up |
| Team has to learn YAML schema, Kani, probar | Pair-write the first 3 contracts; promote aprender's docs as required reading |
| Generated code obscures bugs (you can't `Edit` what's regenerated each build) | All generation is reproducible from contract + framework version; the contract is the readable source |
| Two layers of generation (xpile generates Rust; aprender-contracts generates everything else) creates a hairy build graph | `make contracts` is the single entry point; build graph is `contracts/*.yaml → target/contracts/* → cargo build` |

## Why this is worth it (the hard sell)

Transpilers are a domain where:

1. The "spec" is a natural-language language standard (PEP-8 + Python docs, C99/C11/C17, etc.) that's already too informal to be trusted as canonical.
2. The "correctness criterion" is semantic equivalence between source and target, which is exactly what formal methods are good at expressing.
3. The "long tail" — the cases where naïve transpilation gets it wrong — is exactly where property-based tests + Kani find bugs that hand-written tests miss.
4. The "hybrid transpile" problem only makes sense if the FFI boundary is a *verifiable* artifact, not a comment.

Aprender uses provable-contracts because every ML kernel has a paper with formal equations. xpile uses it because every transpile rule has an *implicit* paper — the language standard — and our job is to make that implicit math explicit, machine-checkable, and impossible to drift from.

The depyler "spec.md + falsifying contracts" pattern from #255 is fine for behavioral process invariants. But the moment xpile starts transpiling Python list comprehensions to Rust iterators, we need to *prove* the semantics match, not just test a few cases and hope. That requires contracts as the source of truth — generating tests, proofs, stubs, and docs from one well-typed YAML.

## Open questions

1. **Dep model.** Path-dep on `~/src/aprender/crates/aprender-contracts` (chosen for v0.1), or a published crates.io version pin? Path-dep blocks publishing xpile to crates.io until aprender-contracts has a stable release.
2. **Generated-artifact location.** `target/contracts/{kani,probar,lean,book}/`? Or `crates/xpile-contracts-generated/`? Former is gitignored & cleaner; latter is more discoverable.
3. **Lean threshold.** Which contracts warrant a Lean theorem vs. Kani-only? Default rule: math-dense (arithmetic, geometry, linear algebra in kernels) → Lean; behavioral (process, ordering, resource bounds) → Kani only.
4. **Contract review workflow.** Should every new contract require a second-reviewer sign-off (like aprender does for paper-grounded kernels)? Recommendation: yes for Layer 1, 2, 4; optional for Layer 3 (we wrote them).
5. **What about decy's existing contracts?** decy has 4 contracts already. Plan: port them as Layer 1 (C semantics) and Layer 2 (C → Rust translation) contracts in Phase 1, retiring decy's standalone `contracts/` directory.

## References

- aprender-contracts crate root: `~/src/aprender/crates/aprender-contracts/src/lib.rs` (the `provable_contracts` library)
- aprender example: `~/src/aprender/contracts/adamw-kernel-v1.yaml` (paradigmatic full contract)
- aprender schema types: `~/src/aprender/crates/aprender-contracts/src/schema/{types,parser,validator,composition,kind}.rs`
- depyler #255 — repair-mode contracts (the lighter falsification pattern)
- This spec's sibling: `docs/specifications/xpile-architecture-v1.md` (the crate layout)
