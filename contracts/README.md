# xpile contracts

This directory holds **provable contracts** — YAML files that bind quantitative claims about xpile to falsifiable shell-command formulas, paired with **Lean refinement theorems** and **Kani BMC harnesses** that discharge those claims under the §14.4 N-of-M evidence model from ruchy 5.0.

The YAML format is borrowed from [aprender's contracts](https://github.com/paiml/aprender/tree/main/contracts) and used identically by the depyler repair-mode work.

## Directory layout

```
contracts/
├── *.yaml                — contract substrate (one YAML per contract)
├── lean/                 — Lean 4 refinement proofs (one .lean file per contract)
└── kani/                 — Kani BMC harnesses (one .rs file per HARNESSED contract;
                          not every contract has one — see the table below)
```

## Substrate state

**Derive it, do not read it here.** The figures below are a *dated snapshot*, not a
live claim — this section previously asserted "12 contracts, 100% at QUORUM" for
long enough that every number in it was false. Regenerate with:

```console
$ xpile quorum          # per-contract stratum table + totals
$ ls contracts/*.yaml   # substrate size the totals range over
```

Snapshot @ 2026-07-27 (`34592ceb`) — **35 contracts: 26 at §14.4 QUORUM, 9 PARTIAL,
0 UNVERIFIED.** Every contract binds a Lean refinement theorem; **24 of 35** also bind
a Kani BMC harness, so the Symbolic stratum is genuinely empty for the other 11 rather
than merely unreported. By stratum shape:

| Shape | Count | Notes |
|---|---|---|
| All four strata | 15 | Semantic + Symbolic + Runtime + Extrinsic |
| Three-stratum QUORUM | 11 | 8 via Semantic + Symbolic + Extrinsic; **2 via Semantic + Runtime + Extrinsic** (`C-COMPILE-RUST-TO-WASM`, `C-WASM-HEAP` — no Kani harness, they reach QUORUM *through Runtime* witnesses); 1 via Semantic + Symbolic + Runtime |
| PARTIAL | 9 | all at Semantic + Extrinsic; Symbolic and Runtime both empty |

The native-WASM pair is the reason the old "three-stratum QUORUM = Semantic + Symbolic
+ Extrinsic" prose was structurally wrong and not merely stale: the lane carrying the
largest Runtime witness corpus in the repo has **zero** Symbolic votes.

### Every contract

All 35, generated from the `lean_file:` / `kani_file:` bindings in the YAML itself —
a `—` in the Kani column means the contract has no harness, which is exactly why its
Symbolic stratum reads 0 in `xpile quorum`. The 1–5 *layer* taxonomy lives in the
specification, which is authoritative for it; it is deliberately not restated here.

| Contract | YAML | Lean refinement | Kani harness |
|---|---|---|---|
| `C-BASHRS-POSIX-IDEMPOTENCE` | [`bashrs-posix-idempotence-v1.yaml`](bashrs-posix-idempotence-v1.yaml) | [`lean/Bashrs.lean`](lean/Bashrs.lean) | [`kani/bashrs.rs`](kani/bashrs.rs) |
| `C-C-FLOAT-ARITH` | [`c-c-float-arith-v1.yaml`](c-c-float-arith-v1.yaml) | [`lean/CFloatArith.lean`](lean/CFloatArith.lean) | [`kani/c_float_arith.rs`](kani/c_float_arith.rs) |
| `C-C-INT-ARITH` | [`c-int-arith-v1.yaml`](c-int-arith-v1.yaml) | [`lean/CIntArith.lean`](lean/CIntArith.lean) | [`kani/c_int_arith.rs`](kani/c_int_arith.rs) |
| `C-COMPILE-RUST-TO-PTX-MMA` | [`compile-rust-to-ptx-mma-v1.yaml`](compile-rust-to-ptx-mma-v1.yaml) | [`lean/CompileRustToPtxMma.lean`](lean/CompileRustToPtxMma.lean) | [`kani/compile_rust_to_ptx_mma.rs`](kani/compile_rust_to_ptx_mma.rs) |
| `C-COMPILE-RUST-TO-SPIRV` | [`compile-rust-to-spirv-v1.yaml`](compile-rust-to-spirv-v1.yaml) | [`lean/CompileRustToSpirv.lean`](lean/CompileRustToSpirv.lean) | — |
| `C-COMPILE-RUST-TO-WASM` | [`compile-rust-to-wasm-v1.yaml`](compile-rust-to-wasm-v1.yaml) | [`lean/XlateRustToWasm.lean`](lean/XlateRustToWasm.lean) | — |
| `C-COMPILE-RUST-TO-WGSL` | [`compile-rust-to-wgsl-v1.yaml`](compile-rust-to-wgsl-v1.yaml) | [`lean/CompileRustToWgsl.lean`](lean/CompileRustToWgsl.lean) | — |
| `C-COMPILE-SHELL-TO-FORJAR` | [`compile-shell-to-forjar-v1.yaml`](compile-shell-to-forjar-v1.yaml) | [`lean/XlateShellToForjar.lean`](lean/XlateShellToForjar.lean) | — |
| `C-CONST-TRANSLATION` | [`const-translation-v1.yaml`](const-translation-v1.yaml) | [`lean/ConstTranslation.lean`](lean/ConstTranslation.lean) | — |
| `C-ENUM-TRANSLATION` | [`enum-translation-v1.yaml`](enum-translation-v1.yaml) | [`lean/EnumTranslation.lean`](lean/EnumTranslation.lean) | [`kani/enum_translation.rs`](kani/enum_translation.rs) |
| `C-FFI-CPYTHON-EXT` | [`ffi-cpython-ext-v1.yaml`](ffi-cpython-ext-v1.yaml) | [`lean/FfiCpythonExt.lean`](lean/FfiCpythonExt.lean) | [`kani/ffi_cpython_ext.rs`](kani/ffi_cpython_ext.rs) |
| `C-FFI-SHELL-SUBPROCESS` | [`ffi-shell-subprocess-v1.yaml`](ffi-shell-subprocess-v1.yaml) | [`lean/FfiShellSubprocess.lean`](lean/FfiShellSubprocess.lean) | — |
| `C-NOTATION-LATEX-MATH-TO-EQUATION` | [`notation-latex-math-to-equation-v1.yaml`](notation-latex-math-to-equation-v1.yaml) | [`lean/Notation.lean`](lean/Notation.lean) | [`kani/notation.rs`](kani/notation.rs) |
| `C-OLS-MODEL-UNIQUENESS` | [`ols-model-uniqueness-v1.yaml`](ols-model-uniqueness-v1.yaml) | [`lean-models/Models/GeneralLinear.lean`](lean-models/Models/GeneralLinear.lean) · [`lean/OlsModelUniqueness.lean`](lean/OlsModelUniqueness.lean) | [`kani/ols_model_uniqueness.rs`](kani/ols_model_uniqueness.rs) |
| `C-PY-CONTEXT-MANAGER-EXIT` | [`py-context-manager-exit-v1.yaml`](py-context-manager-exit-v1.yaml) | [`lean/PyContextManagerExit.lean`](lean/PyContextManagerExit.lean) | — |
| `C-PY-EXCEPT-ALLOWLIST` | [`py-except-allowlist-v1.yaml`](py-except-allowlist-v1.yaml) | [`lean/PyExceptAllowlist.lean`](lean/PyExceptAllowlist.lean) | — |
| `C-PY-FILE-IO-ROUNDTRIP` | [`py-file-io-roundtrip-v1.yaml`](py-file-io-roundtrip-v1.yaml) | [`lean/PyFileIoRoundtrip.lean`](lean/PyFileIoRoundtrip.lean) | — |
| `C-PY-FLOAT-ARITH` | [`py-float-arith-v1.yaml`](py-float-arith-v1.yaml) | [`lean/PyFloatArith.lean`](lean/PyFloatArith.lean) | [`kani/py_float_arith.rs`](kani/py_float_arith.rs) |
| `C-PY-GENERATOR-EAGER` | [`py-generator-eager-v1.yaml`](py-generator-eager-v1.yaml) | [`lean/PyGeneratorEager.lean`](lean/PyGeneratorEager.lean) | — |
| `C-PY-INT-ARITH` | [`py-int-arith-v1.yaml`](py-int-arith-v1.yaml) | [`lean/PyIntArith.lean`](lean/PyIntArith.lean) | [`kani/py_int_arith.rs`](kani/py_int_arith.rs) |
| `C-WASM-HEAP` | [`c-wasm-heap-v1.yaml`](c-wasm-heap-v1.yaml) | [`lean/WasmHeap.lean`](lean/WasmHeap.lean) | — |
| `C-XLATE-LEAN-TO-RUST` | [`xlate-lean-to-rust-v1.yaml`](xlate-lean-to-rust-v1.yaml) | [`lean/XlateLeanToRust.lean`](lean/XlateLeanToRust.lean) | [`kani/xlate_lean_to_rust.rs`](kani/xlate_lean_to_rust.rs) |
| `C-XLATE-PY-BOOL-TO-RUST-BOOL` | [`xlate-py-bool-to-rust-bool-v1.yaml`](xlate-py-bool-to-rust-bool-v1.yaml) | [`lean/XlatePyBoolToRustBool.lean`](lean/XlatePyBoolToRustBool.lean) | [`kani/xlate_py_bool_to_rust_bool.rs`](kani/xlate_py_bool_to_rust_bool.rs) |
| `C-XLATE-PY-CLASS-TO-STRUCT` | [`xlate-py-class-to-struct-v1.yaml`](xlate-py-class-to-struct-v1.yaml) | [`lean/XlatePyClassToStruct.lean`](lean/XlatePyClassToStruct.lean) | [`kani/xlate_py_class_to_struct.rs`](kani/xlate_py_class_to_struct.rs) |
| `C-XLATE-PY-DICT-TO-HASHMAP` | [`xlate-py-dict-to-hashmap-v1.yaml`](xlate-py-dict-to-hashmap-v1.yaml) | [`lean/XlatePyDictToHashmap.lean`](lean/XlatePyDictToHashmap.lean) | [`kani/xlate_py_dict_to_hashmap.rs`](kani/xlate_py_dict_to_hashmap.rs) |
| `C-XLATE-PY-LIST-TO-VEC` | [`xlate-py-list-to-vec-v1.yaml`](xlate-py-list-to-vec-v1.yaml) | [`lean/XlatePyListToVec.lean`](lean/XlatePyListToVec.lean) | [`kani/xlate_py_list_to_vec.rs`](kani/xlate_py_list_to_vec.rs) |
| `C-XLATE-PY-OPTIONAL-TO-OPTION` | [`xlate-py-optional-to-option-v1.yaml`](xlate-py-optional-to-option-v1.yaml) | [`lean/XlatePyOptionalToOption.lean`](lean/XlatePyOptionalToOption.lean) | [`kani/xlate_py_optional_to_option.rs`](kani/xlate_py_optional_to_option.rs) |
| `C-XLATE-PY-SET-TO-HASHSET` | [`xlate-py-set-to-hashset-v1.yaml`](xlate-py-set-to-hashset-v1.yaml) | [`lean/XlatePySetToHashset.lean`](lean/XlatePySetToHashset.lean) | [`kani/xlate_py_set_to_hashset.rs`](kani/xlate_py_set_to_hashset.rs) |
| `C-XLATE-PY-STR-TO-RUST-STRING` | [`xlate-py-str-to-rust-string-v1.yaml`](xlate-py-str-to-rust-string-v1.yaml) | [`lean/XlatePyStrToRustString.lean`](lean/XlatePyStrToRustString.lean) | [`kani/xlate_py_str_to_rust_string.rs`](kani/xlate_py_str_to_rust_string.rs) |
| `C-XLATE-PY-TUPLE-TO-RUST-TUPLE` | [`xlate-py-tuple-to-rust-tuple-v1.yaml`](xlate-py-tuple-to-rust-tuple-v1.yaml) | [`lean/XlatePyTupleToRustTuple.lean`](lean/XlatePyTupleToRustTuple.lean) | [`kani/xlate_py_tuple_to_rust_tuple.rs`](kani/xlate_py_tuple_to_rust_tuple.rs) |
| `C-XLATE-RUST-FN-TO-LEAN-THM` | [`xlate-rust-fn-to-lean-thm-v1.yaml`](xlate-rust-fn-to-lean-thm-v1.yaml) | [`lean/XlateRustFnToLeanThm.lean`](lean/XlateRustFnToLeanThm.lean) | [`kani/xlate_rust_fn_to_lean_thm.rs`](kani/xlate_rust_fn_to_lean_thm.rs) |
| `C-XPILE-BACKEND-TRAIT` | [`xpile-backend-trait-v1.yaml`](xpile-backend-trait-v1.yaml) | [`lean/XpileBackendTrait.lean`](lean/XpileBackendTrait.lean) | [`kani/xpile_backend_trait.rs`](kani/xpile_backend_trait.rs) |
| `C-XPILE-CONTRACT-BACKEND-TRAIT` | [`xpile-contract-backend-trait-v1.yaml`](xpile-contract-backend-trait-v1.yaml) | [`lean/XpileContractBackendTrait.lean`](lean/XpileContractBackendTrait.lean) | [`kani/xpile_contract_backend_trait.rs`](kani/xpile_contract_backend_trait.rs) |
| `C-XPILE-CONTRACT-FRONTEND-TRAIT` | [`xpile-contract-frontend-trait-v1.yaml`](xpile-contract-frontend-trait-v1.yaml) | [`lean/XpileContractFrontendTrait.lean`](lean/XpileContractFrontendTrait.lean) | [`kani/xpile_contract_frontend_trait.rs`](kani/xpile_contract_frontend_trait.rs) |
| `C-XPILE-FRONTEND-TRAIT` | [`xpile-frontend-trait-v1.yaml`](xpile-frontend-trait-v1.yaml) | [`lean/XpileFrontendTrait.lean`](lean/XpileFrontendTrait.lean) | [`kani/xpile_frontend_trait.rs`](kani/xpile_frontend_trait.rs) |

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
| dedicated `kani` GitHub-Actions job | runs `cargo kani` over all 101 `#[kani::proof]` harnesses (24 files, covering 24 of 35 contracts) on every PR. **ADVISORY, not merge-blocking** — a red `kani` job does not stop a merge |

**Do not read the merge-blocking set here.** It is the union over every ruleset
protecting `main`, recorded as one receipt per ruleset under
[`docs/status/ruleset-*.json`](../docs/status/) and derived from them by
`crates/xpile/tests/ruleset_drift.rs`, which checks that union against the live
`gh api repos/paiml/xpile/rules/branches/main`. `kani` is advisory, which is the
only thing this table needs from it.

`xpile quorum` consolidates all four strata into a single per-contract reporter; `xpile attestations` lists Extrinsic votes; both reflect the live state of the substrate.

## Tier roadmap (per ruchy 5.0 §14.10.5)

- **Bronze** (current): byte-identity modelling commitments, `rfl` by construction. All 35 contracts sit at Bronze.
- **Silver** (v0.3.0+): typed AST models replace byte arrays; theorems become structural inductions; Kani harnesses target property-specific assertions rather than byte identity. Tracked per contract under `XPILE-REFINE-*-001+`.
- **Gold** (v0.4.0+): Runtime witnesses (`*_diff_exec`-style fixtures + integration tests) added for the contracts not yet at four-stratum coverage — as of the snapshot above, the 11 at 3-stratum QUORUM plus the 9 at PARTIAL. The two three-stratum native-WASM contracts need the *Symbolic* stratum instead, not Runtime.
- **Platinum** (v1.0.0+): each contract proven sound under a categorical interpretation; the substrate ships as a verified library, not just a tested one.
