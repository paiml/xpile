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

  Tier (per ruchy 5.0 §14.10.5): refinement target is Platinum.
  At v0.1.0 the primary refinement theorem `fast_path_eq_slow_path`
  is **discharged** (PMAT-028 / XPILE-REFINE-002) via
  `Int.bmod_def + split <;> omega`. The mul / floor_div / mod stub
  theorems remain at `trivial` placeholders pending XPILE-REFINE-003
  (different shape — needs `Int.bmod_mul_emod_self_left` and friends).

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

  Implementation via Lean core's `Int.bmod` ("balanced mod"), which
  returns values in `[-N/2, N/2)` for `N = 2^64`. That's exactly the
  i64 signed range. PMAT-028 / XPILE-REFINE-002: this formulation is
  semantically equivalent to the previous hand-rolled `%` + fold form
  but lets us reuse Lean's `Int.bmod_emod` / `Int.bmod_eq_iff` core
  lemmas to discharge the refinement proof below.

  This is the *fast path* of the `C-PY-INT-ARITH` contract.
-/
def i64_wrap_add (a b : Int) : Int := Int.bmod (a + b) (2 ^ 64)

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

  Status: **discharged at v0.1.0 (PMAT-028 / XPILE-REFINE-002)**.
  Proof: refactored `i64_wrap_add` to use Lean core's `Int.bmod`
  (balanced-mod returning values in `[-N/2, N/2)`); the proof then
  unfolds via `Int.bmod_def` + `split <;> omega`. Lean 4.15 closes
  it without any mathlib dep. See the tactic body below for the
  full discharge.
-/
theorem fast_path_eq_slow_path
    (a b : Int)
    (h : fits_i64 (a + b)) :
    i64_wrap_add a b = bigint_add a b := by
  -- PMAT-028 / XPILE-REFINE-002: discharged via `Int.bmod`'s
  -- characterising property — `bmod x N = x` when `x ∈ [-N/2, N/2)`.
  -- That's exactly the `fits_i64` precondition for `N = 2^64`,
  -- so the proof unfolds to a straightforward bmod-identity step
  -- plus `omega` to close the arithmetic.
  unfold i64_wrap_add bigint_add fits_i64 at *
  -- After unfolding: goal is `Int.bmod (a + b) (2 ^ 64) = a + b`,
  -- hypothesis is `-(2^63) ≤ a + b ∧ a + b < 2^63`.
  obtain ⟨hlo, hhi⟩ := h
  -- `Int.bmod_eq_of_neg_lt_lt_div_two` (informally — exact name may
  -- differ across Lean versions): `bmod x N = x` when
  -- `-N/2 ≤ x < N/2`. We have N = 2^64, so N/2 = 2^63.
  -- Try `omega` against `Int.bmod_def` unfolded.
  rw [Int.bmod_def]
  -- Goal now uses `%` and a conditional; omega + numerical facts
  -- about 2^64 and 2^63 close it.
  -- The split is on whether `(a + b) % 2^64 < (2^64 + 1) / 2`.
  -- (a + b) % 2^64 = a + b when 0 ≤ a + b < 2^64 (which fits_i64
  -- guarantees on the lower half); = a + b + 2^64 otherwise.
  split <;> omega

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
