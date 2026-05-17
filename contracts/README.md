# xpile contracts

This directory holds **provable contracts** — YAML files that bind quantitative claims about xpile to falsifiable shell-command formulas, paired with **Lean refinement theorems** and **Kani BMC harnesses** that discharge those claims under the §14.4 N-of-M evidence model from ruchy 5.0.

The YAML format is borrowed from [aprender's contracts](https://github.com/paiml/aprender/tree/main/contracts) and used identically by the depyler repair-mode work.

## Directory layout

```
contracts/
├── *.yaml                — contract substrate (12 contracts at v0.1.0)
├── lean/                 — Lean 4 refinement proofs (one .lean file per contract)
└── kani/                 — Kani BMC harnesses (one .rs file per contract)
```

## Substrate state (v0.1.0)

**12 contracts, 100% at §14.4 QUORUM.** Every contract has a paired Lean refinement theorem + Kani BMC harness at Bronze tier. Two contracts (`C-PY-INT-ARITH`, `C-BASHRS-POSIX-IDEMPOTENCE`) reach full four-stratum coverage (Semantic + Symbolic + Runtime + Extrinsic); the other ten reach three-stratum QUORUM (Semantic + Symbolic + Extrinsic — Runtime witnesses pending Bronze→Gold tier refinement per each contract's `XPILE-REFINE-*-001+` follow-on).

Run `xpile quorum` for the live per-contract stratum table.

### Contracts by layer

| Layer | Lane | Contract | Lean | Kani |
|---|---|---|---|---|
| 1 — per-language semantics | code | [`py-int-arith-v1.yaml`](py-int-arith-v1.yaml) | [`lean/PyIntArith.lean`](lean/PyIntArith.lean) | [`kani/py_int_arith.rs`](kani/py_int_arith.rs) |
| 1 | code | [`bashrs-posix-idempotence-v1.yaml`](bashrs-posix-idempotence-v1.yaml) | [`lean/Bashrs.lean`](lean/Bashrs.lean) | [`kani/bashrs.rs`](kani/bashrs.rs) |
| 2 — translation | code | [`xlate-py-list-to-vec-v1.yaml`](xlate-py-list-to-vec-v1.yaml) | [`lean/XlatePyListToVec.lean`](lean/XlatePyListToVec.lean) | [`kani/xlate_py_list_to_vec.rs`](kani/xlate_py_list_to_vec.rs) |
| 2 | code | [`xlate-lean-to-rust-v1.yaml`](xlate-lean-to-rust-v1.yaml) | [`lean/XlateLeanToRust.lean`](lean/XlateLeanToRust.lean) | [`kani/xlate_lean_to_rust.rs`](kani/xlate_lean_to_rust.rs) |
| 2 | proof | [`xlate-rust-fn-to-lean-thm-v1.yaml`](xlate-rust-fn-to-lean-thm-v1.yaml) | [`lean/XlateRustFnToLeanThm.lean`](lean/XlateRustFnToLeanThm.lean) | [`kani/xlate_rust_fn_to_lean_thm.rs`](kani/xlate_rust_fn_to_lean_thm.rs) |
| 2 | proof | [`notation-latex-math-to-equation-v1.yaml`](notation-latex-math-to-equation-v1.yaml) | [`lean/Notation.lean`](lean/Notation.lean) | [`kani/notation.rs`](kani/notation.rs) |
| 3 — architectural | code | [`xpile-frontend-trait-v1.yaml`](xpile-frontend-trait-v1.yaml) | [`lean/XpileFrontendTrait.lean`](lean/XpileFrontendTrait.lean) | [`kani/xpile_frontend_trait.rs`](kani/xpile_frontend_trait.rs) |
| 3 | code | [`xpile-backend-trait-v1.yaml`](xpile-backend-trait-v1.yaml) | [`lean/XpileBackendTrait.lean`](lean/XpileBackendTrait.lean) | [`kani/xpile_backend_trait.rs`](kani/xpile_backend_trait.rs) |
| 3 | proof | [`xpile-contract-frontend-trait-v1.yaml`](xpile-contract-frontend-trait-v1.yaml) | [`lean/XpileContractFrontendTrait.lean`](lean/XpileContractFrontendTrait.lean) | [`kani/xpile_contract_frontend_trait.rs`](kani/xpile_contract_frontend_trait.rs) |
| 3 | proof | [`xpile-contract-backend-trait-v1.yaml`](xpile-contract-backend-trait-v1.yaml) | [`lean/XpileContractBackendTrait.lean`](lean/XpileContractBackendTrait.lean) | [`kani/xpile_contract_backend_trait.rs`](kani/xpile_contract_backend_trait.rs) |
| 4 — hybrid pipeline | code | [`ffi-cpython-ext-v1.yaml`](ffi-cpython-ext-v1.yaml) | [`lean/FfiCpythonExt.lean`](lean/FfiCpythonExt.lean) | [`kani/ffi_cpython_ext.rs`](kani/ffi_cpython_ext.rs) |
| 5 — compile-time / IR | code | [`compile-rust-to-ptx-mma-v1.yaml`](compile-rust-to-ptx-mma-v1.yaml) | [`lean/CompileRustToPtxMma.lean`](lean/CompileRustToPtxMma.lean) | [`kani/compile_rust_to_ptx_mma.rs`](kani/compile_rust_to_ptx_mma.rs) |

## YAML format

```yaml
metadata:
  id: C-EXAMPLE-NAME
  version: "1.0.0"
  created: "2026-05-15"
  author: PAIML Engineering
  kind: kernel | pattern | behavioral | process
  status: draft | enforced | deprecated
  description: |
    Plain-English statement of what this contract pins down and why.
  references:
    - docs/specifications/...
  depends_on: []

equations:
  some_named_property:
    formula: |
      lhs == rhs
    domain: |
      Why this property must hold.
    invariants:
      - "concrete shell-falsifiable statement"
    preconditions:
      - "what must be true for the formula to be meaningful"
    # Discharge bindings (set when a refinement ships):
    lean_theorem: "XpileContracts.CExampleName.some_named_property"
    lean_file: "contracts/lean/ExampleName.lean"
    kani_harness: "some_named_property"
    kani_file: "contracts/kani/example_name.rs"
```

## CI integration (live)

The contract substrate is gated by `cargo test --workspace` on every PR. The relevant gates:

| Gate | What it asserts |
|---|---|
| `pv lint contracts/` | YAML structurally valid, 0 errors |
| `cargo test -p xpile --test refinement_proofs` | Every `lean_theorem:` field references a real theorem in a real `.lean` file; theorem files carry required landmarks (no `sorry`, no `by trivial` placeholders) |
| `cargo test -p xpile --test kani_harnesses` | Every `kani_harness:` field references a real `#[kani::proof]` function in a real `.rs` file (citation gate) |
| `cargo test -p xpile --test kani_verify` | `cargo kani` actually discharges every harness; asserts exit-0 AND stdout contains `VERIFICATION:- SUCCESSFUL` (skip-gracefully if `cargo-kani` is missing from PATH) |
| `cargo test -p xpile --test quorum` | `C-PY-INT-ARITH` has all four §14.4 strata represented |
| `cargo test -p xpile --test attestations` | `xpile attestations` counts Extrinsic stratum votes per contract |
| dedicated `kani` GitHub-Actions job | runs `cargo kani` over all 12 harnesses on every PR |

`xpile quorum` consolidates all four strata into a single per-contract reporter; `xpile attestations` lists Extrinsic votes; both reflect the live state of the substrate.

## Tier roadmap (per ruchy 5.0 §14.10.5)

- **Bronze** (current v0.1.0): byte-identity modelling commitments, `rfl` by construction. 12 of 12 contracts at Bronze.
- **Silver** (v0.3.0+): typed AST models replace byte arrays; theorems become structural inductions; Kani harnesses target property-specific assertions rather than byte identity. Tracked per contract under `XPILE-REFINE-*-001+`.
- **Gold** (v0.4.0+): Runtime witnesses (`*_diff_exec`-style fixtures + integration tests) added for the 10 contracts currently at 3-stratum QUORUM, bringing them to full four-stratum coverage.
- **Platinum** (v1.0.0+): each contract proven sound under a categorical interpretation; the substrate ships as a verified library, not just a tested one.
