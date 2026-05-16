# Phased Rollout

**Section 21 of [xpile-spec.md](../xpile-spec.md).**

## Seven phases

| Phase | Scope | Exit criterion | Status |
|---|---|---|---|
| **0. Scaffold + pv wiring** | Workspace, traits, deps, 4 example contracts | `cargo check` clean, `pv lint` 8/8 ✅ | **DONE** |
| **1. Architectural contracts** | 6 Layer-3 contracts enforced; hand-written stubs validated | All 6 in `enforced` status; existing stubs lint-clean and binding-resolved | NEXT |
| **2. Python semantics starter set** | 5 Layer-1 kernel contracts (int, list, dict, function-call, attr-access) | All 5 `enforced` with Kani harnesses passing at i8 bit width | |
| **3. Codegen replacement** | Generate emission functions from Layer-2 translation contracts | ≥3 Python constructs end-to-end via generated codegen, hand-written code deleted with `# manual:` justification | |
| **4. Kani equivalence proofs** | Wire `pv kani` for Layer-1 + Layer-2; CI enforces Kani for arithmetic | All arith contracts Kani-green at default unwind depth | |
| **5. Hybrid pipeline demo** | First Layer-4 contract: NumPy-using `.py` + companion `.c` | End-to-end oracle pass on the demo; FFI shim generated from manifest | |
| **6. Lean theorems** | Layer-1 arithmetic contracts get Lean 4 theorems for unbounded properties | ≥3 theorems closed; CI rejects PRs that break theorem statements | |

## Phase 1 detail — Architectural contracts

Six contracts to author and enforce, all `kind: pattern`:

1. `xpile-frontend-trait-v1.yaml` — done ✅
2. `xpile-oracle-v1.yaml` — port from depyler `repair-oracle-v1.yaml`
3. `xpile-agent-budget-v1.yaml` — port from depyler `repair-budget-v1.yaml`
4. `xpile-determinism-v1.yaml` — port from depyler `repair-determinism-v1.yaml`
5. `xpile-provenance-v1.yaml` — port from depyler `repair-provenance-v1.yaml`
6. `xpile-ffi-manifest-v1.yaml` — new (xpile-specific)

Each gets wired to actual code via `binding: <crate>::<symbol>` in the YAML; `pv coverage` then reports reverse-coverage and fails if any equation is unbound.

Effort: 1 week. PR sequence:

```
PR #1: Port depyler contracts (4 of them) → xpile/contracts/
PR #2: New xpile-ffi-manifest contract
PR #3: Wire bindings in each architectural crate
PR #4: Move contracts from draft → enforced
```

## Phase 2 detail — Python semantics

| Contract | Construct | Kani strategy |
|---|---|---|
| `py-int-arith-v1.yaml` (fast path done, PR #23) | int `+`, `-`, `*`, `//`, `%`, unary `-` (i64 with `checked_*().expect()`; bitwise `& \| ^ << >>` and bigint slow path still TODO) | i8 exhaustive |
| `py-list-index-v1.yaml` | list[i] including negative indices | bounded length 5 |
| `py-dict-get-v1.yaml` | dict[k] vs dict.get(k, default) | bounded size 4 |
| `py-function-call-v1.yaml` | positional / keyword / *args / **kwargs | bounded arity 4 |
| `py-attribute-access-v1.yaml` | obj.attr including @property | bounded chain depth 3 |

These are the highest-frequency Python constructs by corpus analysis (from depyler's Tier 1 stdlib results).

## Phase 3 detail — Codegen replacement

Pick three Python constructs covered by Phase 2 contracts. Implement:

1. The `pv scaffold` output is committed as the source of truth
2. The matching contract's `binding:` field points at the generated function
3. The corresponding hand-written code in `xpile-rust-codegen` is deleted
4. A corpus regression test ensures the generated function works on real inputs

If a generated function regresses, the fix is to update the contract, not to hand-edit the generated code.

## Phase 4 detail — Kani

```bash
cargo install --locked kani-verifier
cargo kani --workspace --enable-stubbing
```

Phase 4 adds `cargo kani` as a **nightly** CI gate (Kani is slow). On the first run:

- Generates harnesses from every `kani_harnesses:` block in `enforced` contracts
- Runs each at the contract's specified unwind depth
- Hard-fail nightly on any harness failure

Phase 4 doesn't add Kani to per-PR CI; that's reserved for after we have ≥10 contracts proven (Phase 5+).

## Phase 5 detail — Hybrid demo

First end-to-end demo:

```
demos/numpy_sum/
├── foo.py                 # imports numpy, calls _core.sum
├── _core.c                # CPython C extension, sums an ndarray
└── setup.py
```

Acceptance criteria:

- `xpile transpile --hybrid demos/numpy_sum/` produces a buildable Rust workspace
- `cargo build` clean on the output
- `cargo test --oracle` matches CPython on a fixture of (random ndarray) → (sum) pairs
- Refcount tracker reports zero leaks
- Performance: zero-copy ndarray passthrough verified by pointer-equality test

Once this demo passes, **the load-bearing thesis of xpile is proven**: hybrid transpile works, monorepo wins.

## Phase 6 detail — Lean

Lean 4 theorems for the math-dense subset of contracts:

```lean
-- target/contracts/lean/PyIntArith.lean
theorem add_promotes_correctly (a b : Int) :
    python_int_add a b = if (a + b).fits_in_i64 then
                              i64_wrap_add a b
                            else
                              bigint_add a b
```

These theorems extend Kani's bounded proofs to the unbounded domain. Discharged in Lean by the `aprender-contracts::lean_gen` codegen path.

Phase 6 brings xpile to **Kernel Grade A** in the fleet rollup.

## Beyond Phase 6 (steady state)

After Phase 6, xpile is feature-complete for Python+C and Ruchy. New work:

- More Python constructs (generators, classes, decorators, async)
- More C constructs (function pointers, varargs, inline asm)
- New frontends (C++, CUDA, Zig) via [frontend-onboarding.md](frontend-onboarding.md)
- Skill graduation: skills accumulated during Phases 2-5 promoted to static rules

The success signal in steady state is **repair-invocation rate per corpus trending down**, per [skills.md](skills.md).
