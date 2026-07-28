# Tutorial: Python → Rust (with overflow checks)

> **Governing contract:** [`C-PY-INT-ARITH`](../reference/contracts.md#c-py-int-arith)
> — Layer 1 (semantics), code lane, kind: kernel.
> The invariants it pins: `addition_no_overflow`,
> `addition_overflow_promotion`,
> `multiplication_quadratic_promotion`, `division_floor_semantics`,
> `modulo_floor_semantics` — the mismatch between Python's unbounded
> `int` and Rust's fixed-width `i64`, which is why the emit carries
> `.checked_*().expect(...)` at every arithmetic site until the bigint
> slow path is implemented.

This tutorial walks through what happens, step by step, when you
transpile a Python arithmetic function to Rust. By the end you'll know
exactly which contract is doing what work, and where to look in the
codebase for each step.

## 1. The Python source

```python
# factorial.py
def factorial(n: int) -> int:
    return 1 if n <= 1 else n * factorial(n - 1)
```

This is the same example from the [quick start](../quickstart.md). The
function is deliberately small — typed parameter, typed return, one
ternary, one self-recursive call, two arithmetic ops.

## 2. The transpile

```bash
$ xpile transpile factorial.py --target rust
// xpile-generated from Python module factorial

// xpile-contract: C-PY-INT-ARITH
pub fn factorial(n: i64) -> i64 {
    if (n <= 1i64) { 1i64 } else {
        (n).checked_mul(factorial(
            (n).checked_sub(1i64).expect("xpile: i64 subtraction overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented")
        )).expect("xpile: i64 multiplication overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented")
    }
}
```

Three things to notice:

1. **`// xpile-contract: C-PY-INT-ARITH`** at the top of the emitted
   function. This is the citation that links the emitted Rust back to
   the contract YAML. The
   [`C-XPILE-BACKEND-TRAIT`](../reference/contracts.md#c-xpile-backend-trait)
   contract's `compile_contract_citation` equation quantifies over the
   *constructs* of the emitted artifact, so it requires a citation
   exactly where a construct has a governing contract. `factorial` does
   arithmetic, so it gets one; a comparison-only function emits none, at
   exit 0, by design. Through v0.1.617 this step said the contract
   "requires every emitted function to carry such a citation".
2. **`checked_sub` / `checked_mul`** wrap every arithmetic op. Python
   ints are unbounded, Rust `i64` isn't. The contract requires that we
   *do not silently wrap*; instead we panic and point the user at the
   bigint slow path.
3. **The panic message itself names the contract.** Anyone debugging
   an overflow gets a literal pointer to `C-PY-INT-ARITH` in the panic
   text — no detective work required.

## 3. Verifying the round-trip in CI

xpile's CI doesn't just check that the emitted Rust compiles — it
checks that it *computes the right values*. Two tests do it, over two
different programs, and the difference matters:

```text
// crates/xpile/tests/readme_quickstart_witness.rs — the i64 program
// shown above. Source and expected transcript are parsed out of
// README.md, so the published example cannot rot.
the_readme_output_compiles_under_rustc_o_and_computes_3628800
the_readme_overflow_claim_panics_naming_the_contract   // factorial(21)

// crates/xpile/tests/transpile_e2e.rs — a DIFFERENT factorial.py,
// annotated `-> BigInt`. Emits no `checked_` call at all; compiled
// against an inline BigInt shim.
factorial_emitted_rust_computes_correct_values
```

Every PR runs both. Through v0.1.617 only the second existed, while
this page and `README.md` both described it as covering the `i64`
emit — which it never did: a shim whose `Mul` is a plain `*` cannot
observe an overflow panic. PMAT-1415 added the first test rather than
softening the claim, because the claim turned out to be true.

## 4. The proof-lane shadow

Same source, proof lane:

```bash
$ xpile transpile factorial.py --target lean
-- xpile-generated from Python module factorial

/-- xpile-contract: C-PY-INT-ARITH -/
def factorial (n : Int) : Int :=
  if (n <= (1: Int)) then (1: Int) else (n * (factorial (n - (1: Int))))
```

Notice the **`/-- xpile-contract: C-PY-INT-ARITH -/` docstring** — this
is the proof-lane analogue of the `//` citation in the Rust output.
Both sides of the dual emission carry the same contract ID, so any
downstream analyzer (a doc generator, a citation graph, the audit
falsifier) can join the two — and on the Lean side it joins through
`Lean.findDocString?` on the elaborated environment rather than a regex
over the file. (Through v0.1.617 this was an `@[xpile_contract "…"]`
attribute; see [Python → Lean](python-to-lean.md) for why it changed.)

Lean's `Int` is unbounded, so the contract is satisfied **by
construction** — no `.checked_*()` calls needed. The same contract,
discharged two different ways depending on the host language.

## 5. Where this lives in the codebase

| Concept | File |
|---|---|
| Contract YAML | [`contracts/py-int-arith-v1.yaml`](https://github.com/paiml/xpile/blob/main/contracts/py-int-arith-v1.yaml) |
| Lean theorems | [`contracts/lean/PyIntArith.lean`](https://github.com/paiml/xpile/blob/main/contracts/lean/PyIntArith.lean) |
| Kani harness | [`contracts/kani/py_int_arith.rs`](https://github.com/paiml/xpile/blob/main/contracts/kani/py_int_arith.rs) |
| Frontend lowering | `crates/depyler-frontend/src/` |
| Rust backend emit | `crates/xpile-rust-codegen/src/` |
| Lean backend emit | `crates/xpile-lean-codegen/src/` |
| E2E test | `crates/xpile/tests/transpile_e2e.rs` |

## What you've learnt

- A xpile transpile is **dual-emitted**: code lane and proof lane both
  carry the same contract citation.
- The contract YAML is the single source of truth; everything else
  cites it.
- Discharge happens differently per target — `checked_*()` in Rust,
  free-by-construction in Lean.
- CI doesn't just check compilation, it checks **semantic round-trip**
  (`assert_eq!(factorial(10), 3628800)`).

## What's next

- [Tutorial: Python → Lean 4](python-to-lean.md) — going deeper on the
  proof-lane side.
- [Tutorial: POSIX shell round-trip](shell-roundtrip.md) — a
  qualitatively different shape (same-language round-trip).
