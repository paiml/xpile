# Phased Rollout

**Section 21 of [xpile-spec.md](../xpile-spec.md).**

> **Historical context (2026-05-18 / PMAT-096):** This document
> was authored at v0.0.1 scaffold time and describes the
> *originally planned* phase sequence. The actual v0.1.0 path
> diverged substantially — see the "What actually shipped"
> section below for the live record. The original phases are
> preserved below as the falsification trace (per Popperian
> design discipline): comparing the planned shape to the shipped
> shape tells you where the original design was wrong and where
> it was right. **Canonical source for the live subset:**
> [`/CHANGELOG.md`](../../../CHANGELOG.md) and the live
> `xpile quorum` reporter.

## What actually shipped (v0.1.0)

The planned 7-phase sequence collapsed to one substantive run:
**substrate completion** (PMAT-058..077) and its companion
**bashrs polish** (PMAT-085..092) + **documentation sweep**
(PMAT-078..084, 093..095).

Live substrate state at v0.1.0:

- **27 workspace crates**, all green on `cargo check` /
  `clippy -D warnings` / `cargo deny advisories`
- **12 contracts at 100% §14.4 N-of-M QUORUM** — every contract
  has paired Lean refinement theorem + Kani BMC harness at
  Bronze tier. Two contracts (C-PY-INT-ARITH,
  C-BASHRS-POSIX-IDEMPOTENCE) at full four-stratum coverage;
  the other ten at three-stratum (Sem+Sym+Ext).
- **Four real backends**: Rust, Ruchy, Lean 4, Shell/bashrs.
  PTX / WGSL / SPIR-V scaffolded.
- **Two real frontends with substantive parsers**:
  depyler-frontend (Python — typed `def`, all binary/unary ops,
  if/elif/else, while loops, for-in-range, function calls
  including self-recursion, `subprocess.run` cross-domain to
  shell) and bashrs-frontend (POSIX shell — 54 tests across
  quoting, variable expansion, command substitution,
  pipelines, ShellLoop, ShellAssign, special parameters,
  escape sequences, line continuation, redirections, short-
  circuit operators, test brackets, arithmetic expansion,
  subshells).
- **CI gates**: `gate` + `kani` + `workspace-test` required on
  every PR. Branch protection on `main`.
- **Kani BMC**: 43 harnesses verify on every CI run (post-XPILE-QUORUM-006 / PMAT-147..151 per-equation fan-out).

How the actual path differs from the planned one:

- **Phase 1 (architectural contracts)**: Planned 6 contracts
  (oracle, agent-budget, determinism, provenance, ffi-manifest,
  frontend-trait). Actual: 4 trait contracts (xpile-frontend-trait,
  xpile-backend-trait, xpile-contract-frontend-trait,
  xpile-contract-backend-trait), forming the 2×2 trait-
  determinism matrix at QUORUM via PMAT-062..069.
- **Phase 2 (Python semantics)**: Planned 5 Layer-1 kernel
  contracts. Actual: 2 Layer-1 contracts (py-int-arith,
  bashrs-posix-idempotence), both at four-stratum QUORUM. The
  other planned items (py-list-index, py-dict-get, etc.) became
  Layer-2 contracts where appropriate (xlate-py-list-to-vec).
- **Phase 4 (Kani)**: Planned nightly Kani gate. Actual: Kani
  runs on every PR as a required gate (XPILE-QUORUM-003 / PMAT-021).
  The `nightly` plan was superseded by the §14.4 N-of-M model
  requiring symbolic-stratum votes per contract.
- **Phase 6 (Lean)**: Planned ≥3 theorems. Actual: 12 theorems
  (one per contract).
- **Phase 5 (hybrid demo)**: Planned numpy-using demo. Actual:
  shipped a bashrs-domain demo (PMAT-052
  `bashrs_realistic_demo.sh`) as the first hybrid end-to-end,
  plus a Python→shell cross-domain via subprocess.run
  recognition (PMAT-040). NumPy demo punted to v0.2.0.

The lesson: the original phased plan assumed Python+C as the
primary axis. The actual project pivoted to a substrate-first
strategy where the contract substrate matured before any single
frontend reached deep coverage. That pivot was the right call —
substrate quality is harder to add late, and the v0.1.0 QUORUM
guarantee is the load-bearing claim that distinguishes xpile
from "yet another transpiler."

## Seven phases (originally planned, preserved for falsification trace)

| Phase | Scope | Exit criterion | Status |
|---|---|---|---|
| **0. Scaffold + pv wiring** | Workspace, traits, deps, 4 example contracts | `cargo check` clean, `pv lint` 8/8 ✅ | **DONE** |
| **1. Architectural contracts** | 6 Layer-3 contracts enforced; hand-written stubs validated | All 6 in `enforced` status; existing stubs lint-clean and binding-resolved | superseded — see "What actually shipped" |
| **2. Python semantics starter set** | 5 Layer-1 kernel contracts (int, list, dict, function-call, attr-access) | All 5 `enforced` with Kani harnesses passing at i8 bit width | partially shipped (py-int-arith only) |
| **3. Codegen replacement** | Generate emission functions from Layer-2 translation contracts | ≥3 Python constructs end-to-end via generated codegen, hand-written code deleted with `# manual:` justification | not shipped — codegen remains hand-written; the equation→generator pipeline is XPILE-PV-CODEGEN-001+ future work |
| **4. Kani equivalence proofs** | Wire `pv kani` for Layer-1 + Layer-2; CI enforces Kani for arithmetic | All arith contracts Kani-green at default unwind depth | overshipped — Kani is on every PR, not just nightly, and covers all 12 contracts not just arithmetic |
| **5. Hybrid pipeline demo** | First Layer-4 contract: NumPy-using `.py` + companion `.c` | End-to-end oracle pass on the demo; FFI shim generated from manifest | partially shipped — bashrs-domain hybrid landed (PMAT-040/043/052); NumPy demo punted to v0.2.0 |
| **6. Lean theorems** | Layer-1 arithmetic contracts get Lean 4 theorems for unbounded properties | ≥3 theorems closed; CI rejects PRs that break theorem statements | overshipped — 12 theorems shipped, not 3, covering all 5 contract-taxonomy layers |

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
| `py-int-arith-v1.yaml` (fast path done, PR #23 + PR for PMAT-003) | int `+`, `-`, `*`, `//`, `%`, unary `-` (i64 with `checked_*().expect()`); bitwise `& \| ^ << >>` (infix for `& \| ^`, `checked_shl` / `checked_shr` for shifts); bigint slow path still TODO | i8 exhaustive |
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
