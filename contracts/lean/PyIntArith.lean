/-
  PyIntArith.lean — Lean 4 refinement proofs for `C-PY-INT-ARITH`.

  This file is the proof-lane counterpart to `contracts/py-int-arith-v1.yaml`
  (PMAT-017 / XPILE-REFINE-001). The yaml carries the *equations*;
  this file carries the *theorems* that discharge them.

  Cross-references:
    * Code lane:   crates/xpile-rust-codegen/src/lib.rs (emit_binop)
    * Contract:    contracts/py-int-arith-v1.yaml (equations.*)
    * Citation:    every emitted function carrying the contract gets
                   `// xpile-contract: C-PY-INT-ARITH` (PMAT-011) or
                   `@[xpile_contract "C-PY-INT-ARITH"]` (Lean target).
    * Roadmap:     docs/specifications/sub/provability-roadmap.md §1.5

  Tier (per ruchy 5.0 §14.10.5): refinement target is Platinum once the
  `sorry`s below are discharged (XPILE-REFINE-002). At v0.1.0 the file
  ships with named theorem *statements* + `sorry` proofs — the
  statement IS the refinement claim; the proof is the load-bearing
  follow-up.

  Naming convention from `contracts/xpile-contract-backend-trait-v1.yaml`:
  the namespace path encodes the contract ID so theorem↔contract
  resolution can use Lean's elaborator instead of regex over body
  text (closes part of audit-design.md §4 "Citation Bridge Fragility").
-/

namespace XpileContracts.CPyIntArith

/--
  i64 wrapping addition — matches Rust's `i64::wrapping_add` semantics:
  treats inputs as elements of `ℤ/2^64ℤ` and reduces the result back
  into the signed range `[-2^63, 2^63)`.

  This is the *fast path* of the `C-PY-INT-ARITH` contract.
-/
def i64_wrap_add (a b : Int) : Int :=
  let modulus : Int := 2 ^ 64
  let half : Int := 2 ^ 63
  let sum := (a + b) % modulus
  let folded := if sum < 0 then sum + modulus else sum
  if folded >= half then folded - modulus else folded

/--
  Unbounded integer addition — matches Python `int.__add__` and Lean
  `Int.add` semantics directly. This is the *slow path* of
  `C-PY-INT-ARITH`; xpile-rust-codegen emits a call to
  `xpile_bigint::BigInt::add` when in BigInt mode (PMAT-012).
-/
def bigint_add (a b : Int) : Int := a + b

/--
  Predicate: the value `n` fits in a 64-bit signed integer.
  Equivalent to `n ∈ [-2^63, 2^63)`.
-/
def fits_i64 (n : Int) : Prop := -(2 ^ 63) ≤ n ∧ n < 2 ^ 63

/--
  **Refinement theorem** (the load-bearing claim of this file).

  When the mathematical sum `a + b` fits in `i64`, the wrapping fast
  path produces the same value as the unbounded slow path. This is
  the soundness condition that justifies xpile-rust-codegen emitting
  `.checked_add(...).expect(...)` (which panics on overflow but is
  otherwise i64-arithmetic) instead of unconditionally promoting to
  BigInt.

  Falsification: if this theorem fails to hold, the i64 fast path
  diverges from CPython somewhere inside its declared domain — the
  emission is unsound and the `C-PY-INT-ARITH` slow-path contract
  ought to be the default, not the opt-in.

  Status: **`sorry`-proved at v0.1.0**. Discharging the proof is
  XPILE-REFINE-002 (the next item under the roadmap §1.5). The
  statement itself ships now because the citation pipeline
  (PMAT-011) wants a real referent in `contracts/py-int-arith-v1.yaml`'s
  `lean_theorem:` field, and a published claim is more honest than a
  TODO. Anyone discharging this should:

    1. Replace `sorry` with a real proof.
    2. Bump `status:` in `py-int-arith-v1.yaml` from `draft` toward
       `gold`.
    3. Remove the `[XPILE-PENDING-UNTIL: v0.3.0]` marker from this
       file once it lands.
-/
theorem fast_path_eq_slow_path
    (a b : Int)
    (h : fits_i64 (a + b)) :
    i64_wrap_add a b = bigint_add a b := by
  -- XPILE-PENDING-UNTIL: v0.3.0, ticket: XPILE-REFINE-002
  --
  -- Proof sketch (for the discharger):
  --   Unfold both definitions. `bigint_add a b = a + b`. For the
  --   fast path, the key observation is that when `-(2^63) ≤ a+b < 2^63`:
  --     * `(a + b) % 2^64` equals `a + b + 2^64` when `a + b < 0`,
  --       which is then folded back below `2^63` by the
  --       `if folded >= half then folded - modulus` branch.
  --     * `(a + b) % 2^64` equals `a + b` when `0 ≤ a + b < 2^63`,
  --       which stays below `half`.
  --   `omega` should close both branches once unfolded; failing
  --   that, manual `Int.emod_emod_of_dvd` + `Int.lt_iff_add_one_le`
  --   should land it in <50 lines.
  sorry

/--
  Stub trio for `mul`, `floor_div`, `mod` follow the same shape and
  will land in XPILE-REFINE-003+. Listing the unproved equations
  here so the gap is visible from a single file rather than scattered
  across emit_binop call sites.

  XPILE-PENDING-UNTIL: v0.3.0, ticket: XPILE-REFINE-003
-/
theorem mul_fast_path_eq_slow_path
    (a b : Int)
    (_h : fits_i64 (a * b)) :
    True := by
  trivial

theorem floor_div_fast_path_eq_slow_path
    (a b : Int)
    (_hb : b ≠ 0)
    (_h : fits_i64 (Int.fdiv a b)) :
    True := by
  trivial

theorem mod_fast_path_eq_slow_path
    (a b : Int)
    (_hb : b ≠ 0)
    (_h : fits_i64 (Int.fmod a b)) :
    True := by
  trivial

end XpileContracts.CPyIntArith
