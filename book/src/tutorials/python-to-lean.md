# Tutorial: Python → Lean 4 (proof-lane shadow)

> **Governing contracts:** [`C-PY-INT-ARITH`](../reference/contracts.md#c-py-int-arith)
> (the Layer-1 semantics of Python int arithmetic) and
> [`C-XLATE-LEAN-TO-RUST`](../reference/contracts.md#c-xlate-lean-to-rust)
> (the inverse-direction Layer-2 lowering, used for the proof-lane
> round-trip check).

In the [Python → Rust tutorial](python-to-rust.md) the emitted Rust
*runs*. In this tutorial the emitted Lean 4 *proves*. The same source
file produces a Lean `def` carrying the same contract citation, and the
proof-lane version of "CI passed" is "the Lean file kernel-checks."

## 1. The same Python source

```python
# factorial.py
def factorial(n: int) -> int:
    return 1 if n <= 1 else n * factorial(n - 1)
```

## 2. Transpile to Lean

```bash
$ xpile transpile factorial.py --target lean
-- xpile-generated from Python module factorial

@[xpile_contract "C-PY-INT-ARITH"]
def factorial (n : Int) : Int :=
  if (n <= (1: Int)) then (1: Int) else (n * (factorial (n - (1: Int))))
```

A few things to notice:

1. **`@[xpile_contract "C-PY-INT-ARITH"]` is a real Lean attribute.**
   Not a comment. The
   [`C-XPILE-CONTRACT-BACKEND-TRAIT`](../reference/contracts.md#c-xpile-contract-backend-trait)
   contract forbids regex-over-body citations and requires
   format-native structured constructs. In Lean that's `@[...]`; in
   LaTeX it's `\xpileContract{...}{...}`.
2. **`Int`, not `Int64`.** Lean's `Int` is unbounded. The
   `C-PY-INT-ARITH` contract is satisfied **by construction** —
   nothing to discharge, no overflow checks emitted, no slow-path
   placeholder.
3. **Direct arithmetic, no `.checked_*()`.** Because nothing can
   overflow.

## 3. Why this matters

When you transpile Python to Rust, you get something that runs but
needs runtime checks. When you transpile the *same* Python to Lean, you
get something that *proves* the function is well-defined over the
unbounded integers. The two are **shadows of each other** through the
shared `C-PY-INT-ARITH` contract.

This is the proof-lane analogue of the code-lane round-trip:

| Code lane | Proof lane |
|---|---|
| `rustc -O factorial.rs` succeeds | `lean factorial.lean` kernel-checks |
| `factorial(10) == 3628800` | `theorem factorial_10 : factorial 10 = 3628800 := by decide` (writable on top) |
| `.checked_mul().expect("…C-PY-INT-ARITH slow path…")` discharges overflow at runtime | `Int` is unbounded; nothing to discharge |

## 4. Cross-lane citation graph

Because both emissions carry the same `C-PY-INT-ARITH` citation —
just in language-native form — a downstream tool can build a **citation
graph**:

```text
contracts/py-int-arith-v1.yaml
    │
    ├── (Semantic stratum)   contracts/lean/PyIntArith.lean
    │                          ├── theorem refinement_bronze
    │                          ├── theorem refinement_silver_int_to_i64
    │                          ├── ... 21 Diamond theorems ...
    │                          └── theorem round_trip_identity_diamond
    │
    ├── (Symbolic stratum)   contracts/kani/py_int_arith.rs
    │                          └── #[kani::proof] fn property_overflow_bound
    │
    ├── (Runtime stratum)    tests/fixtures/factorial.py
    │                          (mentions C-PY-INT-ARITH in a doctring)
    │
    └── (Extrinsic stratum)  roadmap.yaml :: PMAT-{017..156, 214..251, ...}
                               (human attestations across the project history)
```

— and a single tool, `xpile quorum`, reports the per-stratum vote tally
that determines whether `C-PY-INT-ARITH` is at QUORUM, PARTIAL, or
UNVERIFIED.

## 5. The full proof-lane round-trip

Beyond Python → Lean, the proof lane includes a second direction:
**Lean → Rust**, governed by
[`C-XLATE-LEAN-TO-RUST`](../reference/contracts.md#c-xlate-lean-to-rust).
This means a Lean `def` can be lowered back to a Rust function with
panic-on-overflow, preserving the contract citation across both
directions.

The bidirectional flow is what enables the "single contract, multiple
discharges" workflow described in
[Two lanes, one substrate](../concepts/two-lanes.md).

## What's next

- [Tutorial: POSIX shell round-trip](shell-roundtrip.md) — a different
  shape of "same-language round-trip" for the
  `C-BASHRS-POSIX-IDEMPOTENCE` contract.
- [The Diamond-tier substrate](../concepts/diamond-substrate.md) — the
  algebraic-invariants layer on top of every contract.
