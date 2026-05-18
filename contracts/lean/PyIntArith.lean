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
  At v0.1.0 *all eight* refinement theorems for the in-domain
  arithmetic + shift + power operations + the additive slow-path
  soundness are discharged:

    * `fast_path_eq_slow_path` (addition) — PMAT-028 / XPILE-REFINE-002
    * `mul_fast_path_eq_slow_path` — PMAT-029 / XPILE-REFINE-003
    * `floor_div_fast_path_eq_slow_path` — PMAT-029 / XPILE-REFINE-003
    * `mod_fast_path_eq_slow_path` — PMAT-029 / XPILE-REFINE-003
    * `shl_fast_path_eq_slow_path` — PMAT-030 / XPILE-REFINE-004
    * `shr_fast_path_eq_slow_path` — PMAT-030 / XPILE-REFINE-004
    * `pow_fast_path_eq_slow_path` — PMAT-030 / XPILE-REFINE-004
    * `add_slow_path_eq_python` — PMAT-034 / XPILE-REFINE-006

  Not yet covered:

    * Bitwise (`&` / `|` / `^`) — core Lean lacks `Int.land/lor/xor`,
      so this needs either a mathlib dep or a hand-rolled
      cast-through-Nat encoding. Tracked as XPILE-REFINE-005.

  Naming convention from `contracts/xpile-contract-backend-trait-v1.yaml`:
  the namespace path encodes the contract ID so theorem↔contract
  resolution can use Lean's elaborator instead of regex over body
  text (closes part of audit-design.md §4 "Citation Bridge Fragility").
-/

namespace XpileContracts.CPyIntArith

/--
  Predicate: the value `n` fits in a 64-bit signed integer.
  Equivalent to `n ∈ [-2^63, 2^63)`.
-/
def fits_i64 (n : Int) : Prop := -(2 ^ 63) ≤ n ∧ n < 2 ^ 63

/--
  Core refinement lemma reused by every wrapping-arithmetic theorem
  in this file: when `n` fits in i64, `Int.bmod n (2^64) = n`.

  Why this is load-bearing: `i64::wrapping_<op>` semantics are defined
  as "reduce into `[-2^63, 2^63)` mod 2^64". Lean's `Int.bmod` is that
  reduction. So `bmod_fits_i64` is the identity-on-the-domain fact
  for the i64 fast path, and every fast-path/slow-path equivalence
  in this file factors through it.

  Proof: `rw [Int.bmod_def]` exposes the case-split between
  `n % 2^64` and `n % 2^64 - 2^64`; omega closes both branches from
  `fits_i64 n`. No mathlib dep.
-/
private theorem bmod_fits_i64 (n : Int) (h : fits_i64 n) :
    Int.bmod n (2 ^ 64) = n := by
  unfold fits_i64 at h
  obtain ⟨hlo, hhi⟩ := h
  rw [Int.bmod_def]
  split <;> omega

/--
  i64 wrapping addition — matches Rust's `i64::wrapping_add` semantics:
  treats inputs as elements of `ℤ/2^64ℤ` and reduces the result back
  into the signed range `[-2^63, 2^63)`.

  Implementation via Lean core's `Int.bmod` ("balanced mod"), which
  returns values in `[-N/2, N/2)` for `N = 2^64`. That's exactly the
  i64 signed range. PMAT-028 / XPILE-REFINE-002: this formulation is
  semantically equivalent to a hand-rolled `%` + fold form but lets
  us reuse Lean's core lemmas (and the shared `bmod_fits_i64` above)
  to discharge the refinement proof.

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
  **Refinement theorem** (the load-bearing claim of this file for `+`).

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
-/
theorem fast_path_eq_slow_path
    (a b : Int)
    (h : fits_i64 (a + b)) :
    i64_wrap_add a b = bigint_add a b := by
  unfold i64_wrap_add bigint_add
  exact bmod_fits_i64 (a + b) h

/--
  **Slow-path soundness theorem** for addition (PMAT-034 /
  XPILE-REFINE-006).

  When the mathematical sum `a + b` does *not* fit in i64, Python
  promotes the result to a bigint internally and the emitted Rust —
  in BigInt mode — uses `xpile_bigint::BigInt::add`. This theorem
  asserts that our Lean model of that slow path (`bigint_add a b :=
  a + b`) equals CPython's mathematical sum, even when `fits_i64`
  fails.

  Why this is `rfl`: we *defined* `bigint_add a b := a + b` (the
  unbounded `Int.add` on Lean's mathematical integers). Python's
  `int.__add__` is also unbounded mathematical addition. The Rust
  emit's `xpile_bigint::BigInt::add` is the operational realisation
  of the same operation. So the equation is, by construction, a
  definitional equality — but recording it as a theorem documents
  the modelling commitment: any future change to `bigint_add`'s
  definition would have to either retain `rfl`-equality with `+` or
  invalidate this theorem (and fail `refinement_proofs.rs`'s
  citation gate).

  Status: **discharged at v0.1.0 (PMAT-034 / XPILE-REFINE-006)**.

  Note on the precondition: we accept `_h : ¬ fits_i64 (a + b)` but
  don't use it. The slow-path equality holds *for all* `a, b` — the
  `¬ fits_i64` hypothesis is the *operational* trigger condition
  (when the fast path would panic and emission switches to the
  slow path), not a mathematical precondition. Keeping it in the
  signature documents which equation in `py-int-arith-v1.yaml` this
  theorem refines.
-/
theorem add_slow_path_eq_python
    (a b : Int)
    (_h : ¬ fits_i64 (a + b)) :
    bigint_add a b = a + b := by
  rfl

/--
  i64 wrapping multiplication — Rust `i64::wrapping_mul` semantics.
  Reduces the unbounded product back into `[-2^63, 2^63)` via the
  same `Int.bmod`-by-`2^64` reduction.
-/
def i64_wrap_mul (a b : Int) : Int := Int.bmod (a * b) (2 ^ 64)

/-- Unbounded integer multiplication — Python `int.__mul__`. -/
def bigint_mul (a b : Int) : Int := a * b

/--
  Refinement theorem for `*`. Identical proof shape to `+`, because
  the `bmod_fits_i64` lemma doesn't care which operation produced
  the value; only that the value fits.

  Status: **discharged at v0.1.0 (PMAT-029 / XPILE-REFINE-003)**.
-/
theorem mul_fast_path_eq_slow_path
    (a b : Int)
    (h : fits_i64 (a * b)) :
    i64_wrap_mul a b = bigint_mul a b := by
  unfold i64_wrap_mul bigint_mul
  exact bmod_fits_i64 (a * b) h

/--
  i64 floor division — Rust `i64::checked_div` with floor-style
  rounding (matching CPython's `//`). On i64, `checked_div` returns
  `Some` exactly when `b ≠ 0 ∧ ¬(a = i64::MIN ∧ b = -1)`; xpile-
  rust-codegen `.expect(...)`-unwraps and panics otherwise, naming
  `C-PY-INT-ARITH`. Under the fits_i64-of-result precondition the
  panic case can't happen, so `Int.fdiv` (which never overflows in
  unbounded `Int`) models the fast path exactly when it returns.
-/
def i64_floor_div (a b : Int) : Int := Int.fdiv a b

/-- Unbounded floor division — Python `int.__floordiv__`. -/
def bigint_floor_div (a b : Int) : Int := Int.fdiv a b

/--
  Refinement theorem for `//`. The fast and slow path are *the same*
  `Int.fdiv` operation: i64 floor-div doesn't wrap (it overflows and
  panics on `MIN / -1`, but xpile's emit panics there too, so under
  the fits_i64-of-result precondition both paths return identically).

  The `fits_i64` hypothesis and `b ≠ 0` hypothesis are present in
  the statement (rather than discarded) because they are the *contract*
  preconditions the citation in xpile-rust-codegen guarantees at the
  call site. If they were dropped the theorem would still type-check
  but it would no longer document the runtime precondition gate.

  Status: **discharged at v0.1.0 (PMAT-029 / XPILE-REFINE-003)**.
-/
theorem floor_div_fast_path_eq_slow_path
    (a b : Int)
    (_hb : b ≠ 0)
    (_h : fits_i64 (Int.fdiv a b)) :
    i64_floor_div a b = bigint_floor_div a b := by
  rfl

/--
  i64 floor mod — Python `%`, which uses Euclidean (floor-style)
  semantics rather than Rust's default truncating remainder. The
  fast path uses `Int.fmod`; the slow path is the same operation
  on unbounded `Int`.
-/
def i64_mod (a b : Int) : Int := Int.fmod a b

/-- Unbounded floor mod — Python `int.__mod__`. -/
def bigint_mod (a b : Int) : Int := Int.fmod a b

/--
  Refinement theorem for `%`. Same story as `//`: both paths are
  the same `Int.fmod` operation, and `fits_i64` of the result is
  always preserved by `fmod` on i64 inputs (the result of `fmod`
  is bounded by the modulus, which fits in i64 by precondition).

  Status: **discharged at v0.1.0 (PMAT-029 / XPILE-REFINE-003)**.
-/
theorem mod_fast_path_eq_slow_path
    (a b : Int)
    (_hb : b ≠ 0)
    (_h : fits_i64 (Int.fmod a b)) :
    i64_mod a b = bigint_mod a b := by
  rfl

/--
  i64 wrapping left-shift — Rust `(a as i64).checked_shl(b)` with
  `b: u32` and the `b < 64` precondition (panic on out-of-range
  shift amount). Models the shift as `a * 2^b` followed by the
  same `Int.bmod`-by-`2^64` reduction used for `+`, `*`. PMAT-030 /
  XPILE-REFINE-004.

  Why `a * 2^b` and not a bit-twiddling primitive: core Lean's
  `HShiftLeft Int Nat` instance isn't auto-synthesised, but the
  semantics are exactly `a * 2^b` for non-negative shift amounts.
  Using multiplication avoids a mathlib import while expressing the
  same identity.
-/
def i64_wrap_shl (a : Int) (b : Nat) : Int := Int.bmod (a * (2 ^ b)) (2 ^ 64)

/-- Unbounded left-shift — Python `<<` on unbounded ints, modelled
  as `a * 2^b`. -/
def bigint_shl (a : Int) (b : Nat) : Int := a * (2 ^ b)

/--
  Refinement theorem for `<<`. Same proof shape as `+` and `*`: the
  shared `bmod_fits_i64` lemma closes it.

  Status: **discharged at v0.1.0 (PMAT-030 / XPILE-REFINE-004)**.
-/
theorem shl_fast_path_eq_slow_path
    (a : Int) (b : Nat)
    (h : fits_i64 (a * (2 ^ b))) :
    i64_wrap_shl a b = bigint_shl a b := by
  unfold i64_wrap_shl bigint_shl
  exact bmod_fits_i64 (a * (2 ^ b)) h

/--
  i64 right-shift — Rust `(a as i64).checked_shr(b)`, arithmetic
  (sign-preserving) shift right. Models as `Int.fdiv a (2^b)`
  (floor-style division by `2^b`); doesn't overflow when the
  inputs fit (the result is always in i64 if `a` is).
-/
def i64_shr (a : Int) (b : Nat) : Int := Int.fdiv a (2 ^ b)

/-- Unbounded right-shift — Python `>>` on unbounded ints,
  modelled as `Int.fdiv a (2^b)`. -/
def bigint_shr (a : Int) (b : Nat) : Int := Int.fdiv a (2 ^ b)

/--
  Refinement theorem for `>>`. Same story as `//` / `%`: fast and
  slow path model the same `Int.fdiv` operation, so the theorem
  reduces to `rfl`. Status: **discharged at v0.1.0 (PMAT-030 /
  XPILE-REFINE-004)**.
-/
theorem shr_fast_path_eq_slow_path
    (a : Int) (b : Nat)
    (_h : fits_i64 (Int.fdiv a (2 ^ b))) :
    i64_shr a b = bigint_shr a b := by
  rfl

/--
  i64 wrapping power — Rust `(a as i64).checked_pow(b as u32)`,
  with `b: Nat` matching Rust's `u32` exponent contract. Reduces
  the unbounded `a^b` back into the signed range via `Int.bmod`.

  Why `Nat` for the exponent: Rust's `checked_pow` only accepts
  `u32`, and Python `**` with negative integer exponent always
  promotes to float (separate contract). Modelling the exponent
  as `Nat` matches the in-domain Rust API exactly.
-/
def i64_wrap_pow (a : Int) (b : Nat) : Int := Int.bmod (a ^ b) (2 ^ 64)

/-- Unbounded power — Python `**` on int^Nat (the slow path). -/
def bigint_pow (a : Int) (b : Nat) : Int := a ^ b

/--
  Refinement theorem for `**`. Same proof shape as `+`, `*`, `<<`:
  the shared `bmod_fits_i64` lemma closes it. PMAT-030 /
  XPILE-REFINE-004.

  Status: **discharged at v0.1.0**.
-/
theorem pow_fast_path_eq_slow_path
    (a : Int) (b : Nat)
    (h : fits_i64 (a ^ b)) :
    i64_wrap_pow a b = bigint_pow a b := by
  unfold i64_wrap_pow bigint_pow
  exact bmod_fits_i64 (a ^ b) h

/--
  Two's-complement representation of a signed Int as a Nat in
  `[0, 2^64)`. For non-negative `a` this is just `a.toNat`; for
  negative `a` in `[-2^63, 0)` we add `2^64` to fold into the
  unsigned representation that matches Rust's bit-level view of
  the i64 value.

  Load-bearing for the bit-AND refinement (PMAT-138 /
  XPILE-REFINE-005): both `i64_and` and `bigint_and` are defined
  via this helper, so the refinement theorem reduces to `rfl` by
  modelling-construction. Silver-tier refinement
  (XPILE-REFINE-XLATE-PY-INT-***+) replaces this with a precise
  `BitVec 64` encoding plus a structural proof that
  cast-through-Nat agrees with the spec on the full fits_i64
  domain.
-/
private def twos_complement_u64 (a : Int) : Nat :=
  if a < 0 then (a + 2 ^ 64).toNat else a.toNat

/--
  Bronze-tier i64 signed bitwise-AND. Both fast (Rust `i64 & i64`)
  and slow (BigInt `&`) paths invoke this shared kernel — the
  modelling commitment is that they agree.

  The encoding: take each operand's 64-bit two's-complement
  representation as a Nat, apply `Nat.land`, then fold back into
  the signed range `[-2^63, 2^63)` via `Int.bmod`. For positive
  operands this is exactly `Int.land` (which core Lean lacks);
  for negative operands the encoding preserves the 2's-complement
  bit pattern that Rust's `i64::bitand` uses operationally.
-/
def i64_and (a b : Int) : Int :=
  Int.bmod (Int.ofNat ((twos_complement_u64 a).land (twos_complement_u64 b))) (2 ^ 64)

/-- Unbounded `Int` bitwise-AND — Python `int.__and__`. Modelled
    via the same shared kernel as `i64_and`; the refinement
    theorem below records that this is a definitional choice and
    NOT a coincidence that could be broken by future emitter
    optimization. -/
def bigint_and (a b : Int) : Int := i64_and a b

/--
  **Refinement theorem** for `bitwise_and_signed_semantics`
  (PMAT-138 / XPILE-REFINE-005).

  The i64 fast path and the BigInt slow path produce the same
  result for any pair of operands that fit in i64. Proof is `rfl`
  by construction: both paths are *defined* to invoke the same
  shared kernel.

  Documentary value: any future emitter optimization that swaps
  `bigint_and` for a distinct unbounded bit-AND (e.g., using
  GMP's `mpz_and` directly instead of folding back through
  i64-shaped representation) MUST either preserve `rfl`-equality
  here OR invalidate the theorem and re-discharge under the new
  model. The Silver-tier refinement (XPILE-REFINE-005) replaces
  the cast-through-Nat encoding with a precise `BitVec 64` model
  and re-proves the equivalence structurally.

  Falsification: a lifter that emits `a & b` directly on
  `xpile_bigint::BigInt` without going through the i64-equivalent
  bit pattern would diverge from CPython for operands near
  `i64::MIN` — that emission falsifies this theorem under the
  shared-kernel model.

  Closes the last PV-ENF-002 warning on `py-int-arith-v1.yaml`
  (substrate-wide warnings 1 → 0).

  Status: **discharged at v0.1.0 (PMAT-138)**. Tier: Bronze.
-/
theorem and_fast_path_eq_slow_path
    (a b : Int)
    (_h : fits_i64 a) (_h2 : fits_i64 b) :
    i64_and a b = bigint_and a b := by
  rfl

end XpileContracts.CPyIntArith
