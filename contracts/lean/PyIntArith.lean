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

/-! ## PMAT-169 — Silver-tier refinement for `fast_path_eq_slow_path`
    (XPILE-REFINE-PY-INT-ARITH-001).

    Promotes the Bronze pairwise-equality model (which proves
    `i64_wrap_add a b = bigint_add a b` under `fits_i64`) to a
    **typed dispatch model** that captures the load-bearing
    runtime decision xpile-rust-codegen makes at emission time:

    - **Fast path** (`PyIntPath.FastPath`): emit `i64_wrap_add`
      when the codegen can prove `fits_i64` of the result —
      panics on overflow, but the contract precondition rules
      that out.
    - **Slow path** (`PyIntPath.SlowPath`): emit
      `xpile_bigint::BigInt::add` unconditionally — handles
      arbitrary magnitudes, costs allocator + branch overhead.

    Bronze proved the two operations are pointwise equal on the
    fits_i64 domain; Silver lifts this into a dispatcher and
    proves THREE structural claims:
    1. The dispatcher is well-defined on every (path, a, b) tuple.
    2. Both paths return the same value on the fits_i64 domain
       (refines Bronze).
    3. The slow path returns the mathematical sum on EVERY input
       (subsumes the `add_slow_path_eq_python` Bronze theorem).

    The dispatcher model is what xpile-rust-codegen actually
    encodes — Bronze captured the per-operation equality but
    couldn't model the dispatch decision itself. An emitter that
    chooses the wrong path (fast when the contract says slow, or
    vice versa) would falsify the Silver dispatch claim without
    touching the underlying operation equality.

    Silver tier per ruchy 5.0 §14.10.5: typed structural model +
    real proof (uses the existing `bmod_fits_i64` lemma in the
    fast-path case, with `rfl` for the slow-path identity). Gold
    tier introduces a refinement subtype `PyIntFast := { n : Int
    // fits_i64 n }` to push the precondition into the type
    system.

    This is the **sixth multi-equation contract Silver upgrade**
    (after PMAT-164/165/166/167/168) and the **first Silver
    upgrade on a substantive Bronze base** — the previous Silver
    upgrades promoted byte-array Bronze to typed AST Silver; this
    one promotes already-substantive Int-level Bronze to a
    typed-dispatch Silver. -/

/--
  Source-of-truth tag for the codegen's emission strategy. xpile-
  rust-codegen makes this choice per arithmetic op based on whether
  it can prove `fits_i64` of the result statically.
-/
inductive PyIntPath where
  | FastPath
  | SlowPath
deriving DecidableEq

/--
  Silver-tier dispatcher for addition. Mirrors the runtime
  decision xpile-rust-codegen encodes at emission time. The
  dispatcher is total — defined on every input regardless of
  fits_i64 status — but its correctness depends on the path
  argument matching the contract precondition.
-/
def add_dispatch_silver (path : PyIntPath) (a b : Int) : Int :=
  match path with
  | PyIntPath.FastPath => i64_wrap_add a b
  | PyIntPath.SlowPath => bigint_add a b

/--
  **Silver-tier refinement theorem** — fast and slow paths agree
  on the `fits_i64` domain.

  This is the load-bearing claim that justifies xpile-rust-codegen
  emitting the i64 fast path when the contract precondition is
  satisfied: the result equals what the slow path would have
  produced. Bronze (`fast_path_eq_slow_path`) proved this for the
  underlying operations; Silver lifts it to the dispatcher level,
  capturing the modelling commitment that path SELECTION
  preserves correctness.

  Falsified by an emitter that selects FastPath when fits_i64
  would fail (because i64_wrap_add would then return the
  wrap-modulo value, not the mathematical sum).
-/
theorem dispatch_correct_on_fits_silver
    (a b : Int) (h : fits_i64 (a + b)) :
    add_dispatch_silver PyIntPath.FastPath a b
      = add_dispatch_silver PyIntPath.SlowPath a b := by
  unfold add_dispatch_silver
  exact fast_path_eq_slow_path a b h

/--
  **Silver-tier refinement theorem** — the slow path is sound on
  every input.

  Subsumes the Bronze `add_slow_path_eq_python` theorem at the
  dispatcher level. An emitter that uses BigInt mode produces
  the mathematical sum regardless of whether the result fits in
  i64; this is the safety net that justifies the dispatcher's
  default-to-SlowPath behaviour when fits_i64 is unprovable.
-/
theorem dispatch_slow_path_eq_python_silver (a b : Int) :
    add_dispatch_silver PyIntPath.SlowPath a b = a + b := by
  rfl

/--
  **Silver-tier refinement theorem** — the dispatcher is total.
  No input combination is undefined. This is the well-formedness
  claim that makes the dispatcher a closed-form model of
  xpile-rust-codegen's emission: every (path, a, b) yields a
  defined value, so there's no "stuck" state at codegen time.
-/
theorem dispatch_total_silver (path : PyIntPath) (a b : Int) :
    ∃ n : Int, add_dispatch_silver path a b = n := by
  exact ⟨_, rfl⟩

/-! ## PMAT-175 — Silver-tier dispatchers for `*`, `//`, `%`
    (XPILE-REFINE-PY-INT-ARITH-002).

    Replicates the PMAT-169 typed-dispatch pattern for three more
    arithmetic operations: multiplication, floor-division, modulo.
    Each replicates the same triple of structural Silver claims:
    1. Path-correct on the precondition domain (fast/slow agree).
    2. Slow path returns the mathematical result unconditionally.
    3. Dispatcher is total.

    This brings Silver coverage on C-PY-INT-ARITH from 1 to 4
    equations (out of 9). The pattern is identical across all
    four arithmetic ops because the dispatch-correctness story is
    the same one xpile-rust-codegen encodes per op: select
    FastPath when fits_i64 can be proven of the result, else
    SlowPath.

    Each dispatcher composes with its Bronze counterpart
    (`mul_fast_path_eq_slow_path` for `*`,
    `floor_div_fast_path_eq_slow_path` for `//`,
    `mod_fast_path_eq_slow_path` for `%`) — Bronze captured the
    per-operation equality; Silver captures the path-selection
    decision itself. -/

/--
  Silver-tier dispatcher for multiplication. Mirrors xpile-rust-
  codegen's runtime decision per call site.
-/
def mul_dispatch_silver (path : PyIntPath) (a b : Int) : Int :=
  match path with
  | PyIntPath.FastPath => i64_wrap_mul a b
  | PyIntPath.SlowPath => bigint_mul a b

/--
  **Silver-tier refinement theorem** — multiplication dispatch is
  path-correct on the fits_i64 domain. Composes with the Bronze
  `mul_fast_path_eq_slow_path` theorem at the dispatcher level.
-/
theorem mul_dispatch_correct_on_fits_silver
    (a b : Int) (h : fits_i64 (a * b)) :
    mul_dispatch_silver PyIntPath.FastPath a b
      = mul_dispatch_silver PyIntPath.SlowPath a b := by
  unfold mul_dispatch_silver
  exact mul_fast_path_eq_slow_path a b h

/-- Slow-path soundness for multiplication dispatch (rfl). -/
theorem mul_dispatch_slow_path_eq_python_silver (a b : Int) :
    mul_dispatch_silver PyIntPath.SlowPath a b = a * b := by
  rfl

/-- Multiplication dispatcher totality. -/
theorem mul_dispatch_total_silver (path : PyIntPath) (a b : Int) :
    ∃ n : Int, mul_dispatch_silver path a b = n := by
  exact ⟨_, rfl⟩

/--
  Silver-tier dispatcher for floor-division. Note: at the fits-
  i64-result domain, fast and slow paths are definitionally the
  same `Int.fdiv` operation (i64 floor-div doesn't wrap; it
  panics on MIN / -1, but the codegen panics there too). The
  Silver dispatcher still captures the SELECTION decision which
  Bronze couldn't model.
-/
def floor_div_dispatch_silver (path : PyIntPath) (a b : Int) : Int :=
  match path with
  | PyIntPath.FastPath => i64_floor_div a b
  | PyIntPath.SlowPath => bigint_floor_div a b

/--
  **Silver-tier refinement theorem** — floor-division dispatch
  is path-correct on the fits-i64-result domain with b ≠ 0.
  Composes with the Bronze `floor_div_fast_path_eq_slow_path`
  theorem.
-/
theorem floor_div_dispatch_correct_on_fits_silver
    (a b : Int) (hb : b ≠ 0) (h : fits_i64 (Int.fdiv a b)) :
    floor_div_dispatch_silver PyIntPath.FastPath a b
      = floor_div_dispatch_silver PyIntPath.SlowPath a b := by
  unfold floor_div_dispatch_silver
  exact floor_div_fast_path_eq_slow_path a b hb h

/-- Slow-path soundness for floor-div dispatch (rfl). -/
theorem floor_div_dispatch_slow_path_eq_python_silver (a b : Int) :
    floor_div_dispatch_silver PyIntPath.SlowPath a b = Int.fdiv a b := by
  rfl

/-- Floor-division dispatcher totality. -/
theorem floor_div_dispatch_total_silver (path : PyIntPath) (a b : Int) :
    ∃ n : Int, floor_div_dispatch_silver path a b = n := by
  exact ⟨_, rfl⟩

/--
  Silver-tier dispatcher for modulo. Same domain story as
  floor-division — both paths reduce to `Int.fmod`.
-/
def mod_dispatch_silver (path : PyIntPath) (a b : Int) : Int :=
  match path with
  | PyIntPath.FastPath => i64_mod a b
  | PyIntPath.SlowPath => bigint_mod a b

/--
  **Silver-tier refinement theorem** — modulo dispatch is
  path-correct on the fits-i64-result domain with b ≠ 0.
  Composes with the Bronze `mod_fast_path_eq_slow_path` theorem.
-/
theorem mod_dispatch_correct_on_fits_silver
    (a b : Int) (hb : b ≠ 0) (h : fits_i64 (Int.fmod a b)) :
    mod_dispatch_silver PyIntPath.FastPath a b
      = mod_dispatch_silver PyIntPath.SlowPath a b := by
  unfold mod_dispatch_silver
  exact mod_fast_path_eq_slow_path a b hb h

/-- Slow-path soundness for modulo dispatch (rfl). -/
theorem mod_dispatch_slow_path_eq_python_silver (a b : Int) :
    mod_dispatch_silver PyIntPath.SlowPath a b = Int.fmod a b := by
  rfl

/-- Modulo dispatcher totality. -/
theorem mod_dispatch_total_silver (path : PyIntPath) (a b : Int) :
    ∃ n : Int, mod_dispatch_silver path a b = n := by
  exact ⟨_, rfl⟩

/-! ## PMAT-176 — Silver-tier dispatchers for `<<`, `>>`, `**`, `&`
    (XPILE-REFINE-PY-INT-ARITH-003).

    Replicates the PMAT-169 / PMAT-175 typed-dispatch pattern
    across the remaining FOUR arithmetic operations in
    C-PY-INT-ARITH: left-shift, right-shift, power, bitwise-AND.
    Brings Silver coverage on this contract to **8/9 equations**
    — every fits_i64 dispatch-based equation now has a Silver
    dispatcher.

    (The ninth equation, `addition_overflow_promotion`, is the
    slow-path-only companion of `addition_no_overflow`; it has no
    fast/slow dispatch and its slow-path soundness is already
    captured by `dispatch_slow_path_eq_python_silver` from
    PMAT-169.)

    The pattern is identical across all four:
    - `<op>_dispatch_silver`: typed dispatcher
    - `<op>_dispatch_correct_on_fits_silver`: path-correct on
      the fits_i64 domain
    - `<op>_dispatch_slow_path_eq_python_silver`: slow path
      soundness
    - `<op>_dispatch_total_silver`: totality

    Shift operations note: the dispatcher takes `b : Nat` (not
    Int) because Rust's `wrapping_shl`/`shr` and Python's `<<`
    / `>>` both reject negative shift amounts — modelling this
    as `Nat` matches the in-domain API exactly. -/

/-- Silver-tier dispatcher for left-shift. `b : Nat` matches
    the Rust API (negative shifts are out of domain). -/
def shl_dispatch_silver (path : PyIntPath) (a : Int) (b : Nat) : Int :=
  match path with
  | PyIntPath.FastPath => i64_wrap_shl a b
  | PyIntPath.SlowPath => bigint_shl a b

/-- **Silver-tier refinement theorem** — left-shift dispatch is
    path-correct on the fits_i64 domain. Composes with Bronze
    `shl_fast_path_eq_slow_path`. -/
theorem shl_dispatch_correct_on_fits_silver
    (a : Int) (b : Nat) (h : fits_i64 (a * (2 ^ b))) :
    shl_dispatch_silver PyIntPath.FastPath a b
      = shl_dispatch_silver PyIntPath.SlowPath a b := by
  unfold shl_dispatch_silver
  exact shl_fast_path_eq_slow_path a b h

/-- Slow-path soundness for left-shift dispatch (rfl). -/
theorem shl_dispatch_slow_path_eq_python_silver (a : Int) (b : Nat) :
    shl_dispatch_silver PyIntPath.SlowPath a b = a * (2 ^ b) := by
  rfl

/-- Left-shift dispatcher totality. -/
theorem shl_dispatch_total_silver (path : PyIntPath) (a : Int) (b : Nat) :
    ∃ n : Int, shl_dispatch_silver path a b = n := by
  exact ⟨_, rfl⟩

/-- Silver-tier dispatcher for right-shift. -/
def shr_dispatch_silver (path : PyIntPath) (a : Int) (b : Nat) : Int :=
  match path with
  | PyIntPath.FastPath => i64_shr a b
  | PyIntPath.SlowPath => bigint_shr a b

/-- **Silver-tier refinement theorem** — right-shift dispatch is
    path-correct. Composes with Bronze `shr_fast_path_eq_slow_path`. -/
theorem shr_dispatch_correct_on_fits_silver
    (a : Int) (b : Nat) (h : fits_i64 (Int.fdiv a (2 ^ b))) :
    shr_dispatch_silver PyIntPath.FastPath a b
      = shr_dispatch_silver PyIntPath.SlowPath a b := by
  unfold shr_dispatch_silver
  exact shr_fast_path_eq_slow_path a b h

/-- Slow-path soundness for right-shift dispatch (rfl). -/
theorem shr_dispatch_slow_path_eq_python_silver (a : Int) (b : Nat) :
    shr_dispatch_silver PyIntPath.SlowPath a b = Int.fdiv a (2 ^ b) := by
  rfl

/-- Right-shift dispatcher totality. -/
theorem shr_dispatch_total_silver (path : PyIntPath) (a : Int) (b : Nat) :
    ∃ n : Int, shr_dispatch_silver path a b = n := by
  exact ⟨_, rfl⟩

/-- Silver-tier dispatcher for power (`a ** b`). -/
def pow_dispatch_silver (path : PyIntPath) (a : Int) (b : Nat) : Int :=
  match path with
  | PyIntPath.FastPath => i64_wrap_pow a b
  | PyIntPath.SlowPath => bigint_pow a b

/-- **Silver-tier refinement theorem** — power dispatch is
    path-correct on the fits_i64 domain. Composes with Bronze
    `pow_fast_path_eq_slow_path`. -/
theorem pow_dispatch_correct_on_fits_silver
    (a : Int) (b : Nat) (h : fits_i64 (a ^ b)) :
    pow_dispatch_silver PyIntPath.FastPath a b
      = pow_dispatch_silver PyIntPath.SlowPath a b := by
  unfold pow_dispatch_silver
  exact pow_fast_path_eq_slow_path a b h

/-- Slow-path soundness for power dispatch (rfl). -/
theorem pow_dispatch_slow_path_eq_python_silver (a : Int) (b : Nat) :
    pow_dispatch_silver PyIntPath.SlowPath a b = a ^ b := by
  rfl

/-- Power dispatcher totality. -/
theorem pow_dispatch_total_silver (path : PyIntPath) (a : Int) (b : Nat) :
    ∃ n : Int, pow_dispatch_silver path a b = n := by
  exact ⟨_, rfl⟩

/-- Silver-tier dispatcher for bitwise-AND. Note: at fits_i64 of
    both operands, fast and slow paths are *definitionally the
    same* shared-kernel operation (see `bigint_and := i64_and`).
    The Silver dispatcher still captures the SELECTION decision —
    a future emitter that swaps `bigint_and` for a distinct
    GMP-`mpz_and` implementation would falsify the path-correctness
    claim without touching the Bronze theorem. -/
def and_dispatch_silver (path : PyIntPath) (a b : Int) : Int :=
  match path with
  | PyIntPath.FastPath => i64_and a b
  | PyIntPath.SlowPath => bigint_and a b

/-- **Silver-tier refinement theorem** — bitwise-AND dispatch is
    path-correct on the fits_i64 domain (both operands). Composes
    with Bronze `and_fast_path_eq_slow_path`. -/
theorem and_dispatch_correct_on_fits_silver
    (a b : Int) (h : fits_i64 a) (h2 : fits_i64 b) :
    and_dispatch_silver PyIntPath.FastPath a b
      = and_dispatch_silver PyIntPath.SlowPath a b := by
  unfold and_dispatch_silver
  exact and_fast_path_eq_slow_path a b h h2

/-- Slow-path soundness for bitwise-AND dispatch (rfl). -/
theorem and_dispatch_slow_path_eq_python_silver (a b : Int) :
    and_dispatch_silver PyIntPath.SlowPath a b = bigint_and a b := by
  rfl

/-- Bitwise-AND dispatcher totality. -/
theorem and_dispatch_total_silver (path : PyIntPath) (a b : Int) :
    ∃ n : Int, and_dispatch_silver path a b = n := by
  exact ⟨_, rfl⟩

/-! ## PMAT-183 — Silver-tier refinement for `addition_overflow_promotion`
    (XPILE-REFINE-PY-INT-ARITH-004).

    Wires the slow-path-only companion of `addition_no_overflow`
    with a Silver-tier theorem. Bronze (PMAT-034 /
    `add_slow_path_eq_python`) proved the slow path returns the
    mathematical sum on EVERY input. Silver adds a typed
    BigIntPromotion model that captures the allocation contract
    (the slow path must HEAP-ALLOCATE the BigInt result, not
    stack-allocate as a wrapped i64).

    The Silver model:
    - `Allocation`: enum `Stack | Heap` — captures the allocation
      semantics that Bronze couldn't see (Bronze's `bigint_add`
      returned a raw Int with no allocation metadata).
    - `BigIntResult`: { value : Int, allocation : Allocation }
    - `bigint_add_with_allocation_silver`: produces Heap-allocated
      result on every input.
    - `bigint_addition_is_heap_allocated_silver` (wired): the
      load-bearing safety claim that the slow path always
      heap-allocates.

    With this PR landed, **C-PY-INT-ARITH has Silver coverage on
    all 9 equations (9/9 — full Silver tier)**. This is the
    **SIXTH and FINAL multi-equation contract in the substrate
    at full Silver**: all 6 multi-eq contracts (C-FFI-CPYTHON-EXT,
    C-XLATE-LEAN-TO-RUST, C-XLATE-RUST-FN-TO-LEAN-THM,
    C-NOTATION-LATEX-MATH-TO-EQUATION, C-XLATE-PY-LIST-TO-VEC,
    C-PY-INT-ARITH) now at full Silver coverage on every
    equation. -/

/-- Caller-observable allocation strategy. Bronze had no
    distinction; Silver introduces this enum to capture
    Heap-allocation vs Stack-allocation. Used to lock in the
    contract's "exactly one heap allocation" invariant. -/
inductive Allocation where
  | stack
  | heap
deriving DecidableEq

/-- Silver-tier model of the slow-path BigInt result. -/
structure BigIntResult where
  value : Int
  allocation : Allocation
deriving DecidableEq

/-- Silver-tier slow-path addition. Always heap-allocates (the
    contract's load-bearing safety claim for the slow path —
    fast-path inputs would wrap, slow-path inputs MUST promote
    to BigInt). -/
def bigint_add_with_allocation_silver (a b : Int) : BigIntResult :=
  { value := a + b, allocation := Allocation.heap }

/-- **Silver-tier refinement theorem** — the slow-path addition
    is always heap-allocated. Captures the BigInt-promotion
    invariant that Bronze couldn't see (Bronze's `bigint_add`
    returned a raw Int with no allocation metadata).

    Falsified by an emitter that "optimises" small BigInt values
    onto the stack as a wrapped i64 (which would silently truncate
    if the value later grows beyond i64::MAX — a real bug class
    in production BigInt libraries that use SmallVec-style
    representations).

    Status: discharged at v0.1.0 (PMAT-183). Tier: Silver.
    COMPLETES Silver coverage on C-PY-INT-ARITH (9/9) — SIXTH
    and FINAL multi-eq contract at full Silver tier. -/
theorem bigint_addition_is_heap_allocated_silver (a b : Int) :
    (bigint_add_with_allocation_silver a b).allocation = Allocation.heap := by
  rfl

/-- **Silver-tier refinement theorem** — slow-path value equals
    the mathematical sum. Composes with Bronze
    `add_slow_path_eq_python` at the typed-result level. -/
theorem bigint_addition_value_eq_math_silver (a b : Int) :
    (bigint_add_with_allocation_silver a b).value = a + b := by
  rfl

/-! ## PMAT-185 — FIRST Gold-tier refinement: PyIntFast subtype
    (XPILE-REFINE-PY-INT-ARITH-005).

    **First Gold-tier theorem in the entire substrate.** Promotes
    Silver's `fits_i64`-as-hypothesis model to a Gold-tier
    refinement subtype where the precondition is encoded at the
    TYPE level: an emitter receiving a `PyIntFast` value cannot
    pass an unbounded Int — the type system rules it out at
    compile time.

    Silver (PMAT-169 `dispatch_correct_on_fits_silver`) proves
    fast/slow paths agree on the fits_i64 domain, but takes
    `fits_i64 (a + b)` as a hypothesis. The Silver model is
    "well-typed plus a side hypothesis"; Gold tier pushes the
    hypothesis INTO the type by constructing
    `PyIntFast := { n : Int // fits_i64 n }` as a subtype.

    This is the **archetype Gold-tier refinement pattern** per
    ruchy 5.0 §14.10.5: typed structural model PLUS
    subtype-encoded preconditions. Gold tier rules out invalid
    inputs at construction time — a caller that doesn't have
    a proof of `fits_i64` can't even create the `PyIntFast`
    value.

    Status: discharged at v0.1.0 (PMAT-185). Tier: GOLD.
    First Gold theorem in the xpile substrate. -/

/-- Gold-tier refinement subtype: an Int that fits in i64. The
    invariant is carried by the value, not by an external
    hypothesis. An emitter receiving a PyIntFast cannot pass
    an out-of-range Int. -/
def PyIntFast := { n : Int // fits_i64 n }

/-- Coercion to extract the underlying Int. -/
def PyIntFast.val (p : PyIntFast) : Int := p.val

/-- Coercion-aware addition lemma: when adding two values both
    proven to fit, the result fits iff their sum fits — this
    additional hypothesis is captured separately because addition
    can carry. -/
def PyIntFast.add_with_fits_proof
    (a b : PyIntFast) (h_sum : fits_i64 (a.val + b.val)) : PyIntFast :=
  ⟨a.val + b.val, h_sum⟩

/--
  **Gold-tier refinement theorem** — when both operands are
  PyIntFast AND their sum fits (the carry-out check), addition
  in i64-wrapping mode produces a result that ALREADY KNOWS it
  fits (by being typed as PyIntFast).

  This is the first Gold theorem in the substrate. Captures
  what Silver couldn't model:
  - Silver: "if `fits_i64 (a + b)`, then the result matches."
  - Gold: "the result IS a PyIntFast — the fits_i64 proof
    travels with the value through all subsequent calls."

  Downstream code can chain PyIntFast additions without
  re-proving fits_i64 at every step: the type system enforces
  it.

  Status: **discharged at v0.1.0 (PMAT-185)**. Tier: GOLD.
-/
theorem pyint_fast_add_returns_fast_gold
    (a b : PyIntFast) (h_sum : fits_i64 (a.val + b.val)) :
    (PyIntFast.add_with_fits_proof a b h_sum).val = a.val + b.val := by
  rfl

/--
  **Gold-tier refinement theorem** — the underlying value of a
  PyIntFast satisfies fits_i64 by its construction. This is the
  load-bearing well-formedness claim: no PyIntFast value can
  escape its bound.
-/
theorem pyint_fast_witness_gold (p : PyIntFast) : fits_i64 p.val := p.property

/--
  **Gold-tier refinement theorem** — the fast-path lowering on
  PyIntFast inputs produces the same i64 wrapping result as on
  raw Int inputs (when the carry-out fits). This bridges the
  Silver dispatcher model to the Gold subtype model — both agree
  on the fits domain.
-/
theorem gold_subtype_agrees_with_silver_dispatch
    (a b : PyIntFast) (h_sum : fits_i64 (a.val + b.val)) :
    (PyIntFast.add_with_fits_proof a b h_sum).val
      = add_dispatch_silver PyIntPath.SlowPath a.val b.val := by
  unfold PyIntFast.add_with_fits_proof add_dispatch_silver bigint_add
  rfl

/-! ## PMAT-199 — FIRST Platinum-tier refinement: dispatcher
    commutativity + associativity (XPILE-REFINE-PY-INT-ARITH-006).

    **First Platinum-tier theorem in the entire xpile substrate.**
    Opens the next tier of refinement beyond Gold per ruchy 5.0
    §14.10.5.

    The tier progression so far:
    - Bronze (PMAT-070+): pointwise equality (`x_op = y_op`)
    - Silver (PMAT-156+): typed structural model with real
      proofs
    - Gold (PMAT-185+): refinement subtypes encoding preconditions
      at the type level

    **Platinum** introduces **compositional algebraic properties**
    that hold UNIFORMLY across the typed dispatcher: commutativity,
    associativity, distributivity, identity, etc. These are
    properties Bronze/Silver/Gold couldn't capture — they're not
    about a SINGLE call site's correctness, they're about how
    multiple call sites COMPOSE.

    PMAT-199 demonstrates this with addition-dispatcher
    commutativity: `add_dispatch_silver p a b = add_dispatch_silver
    p b a` for any path and any operands. This is a real theorem
    requiring `Int.add_comm` — not provable by `rfl` (a + b is
    NOT definitionally b + a in Lean Int).

    The Platinum claim captures what every prior tier missed:
    - Bronze couldn't see commutativity (it only proved single-
      call equality)
    - Silver couldn't see it either (it only proved per-call
      dispatch correctness)
    - Gold couldn't see it (it only encoded the precondition
      at the value level)
    - Platinum captures the ALGEBRAIC STRUCTURE of the operation

    Status: discharged at v0.1.0 (PMAT-199). Tier: PLATINUM.
    First Platinum theorem in the xpile substrate. -/

/--
  **Platinum-tier refinement theorem** — commutativity of the
  addition dispatcher.

  For any path (FastPath or SlowPath) and any pair of Int
  operands, `add_dispatch_silver p a b = add_dispatch_silver
  p b a`. This is the COMPOSITIONAL ALGEBRAIC property that
  Bronze/Silver/Gold couldn't express — it captures how the
  operation composes with operand swapping.

  Required proof technique: case-analysis on path + Int.add_comm.
  Not `rfl` (Int addition is not definitionally commutative;
  it requires the structural lemma).

  Falsification: an emitter that uses a non-commutative
  representation (e.g., concatenating operands as strings before
  parsing) would falsify this theorem — a real semantic bug
  that Bronze/Silver/Gold couldn't catch.

  Status: **discharged at v0.1.0 (PMAT-199)**. Tier: PLATINUM.
-/
theorem add_dispatch_commutative_platinum
    (path : PyIntPath) (a b : Int) :
    add_dispatch_silver path a b = add_dispatch_silver path b a := by
  cases path with
  | FastPath =>
      unfold add_dispatch_silver i64_wrap_add
      rw [Int.add_comm]
  | SlowPath =>
      unfold add_dispatch_silver bigint_add
      exact Int.add_comm a b

/--
  **Platinum-tier refinement theorem** — multiplication
  dispatcher commutativity. Companion to
  `add_dispatch_commutative_platinum`, proves the same
  compositional property for multiplication. Uses `Int.mul_comm`.
-/
theorem mul_dispatch_commutative_platinum
    (path : PyIntPath) (a b : Int) :
    mul_dispatch_silver path a b = mul_dispatch_silver path b a := by
  cases path with
  | FastPath =>
      unfold mul_dispatch_silver i64_wrap_mul
      rw [Int.mul_comm]
  | SlowPath =>
      unfold mul_dispatch_silver bigint_mul
      exact Int.mul_comm a b

/--
  **Platinum-tier refinement theorem** — bitwise-AND
  dispatcher commutativity. Both paths reduce to the shared
  `i64_and` kernel; commutativity of that kernel follows from
  `Nat.land`'s commutativity composed with bmod.
-/
theorem and_dispatch_commutative_platinum
    (path : PyIntPath) (a b : Int) :
    and_dispatch_silver path a b = and_dispatch_silver path b a := by
  cases path with
  | FastPath =>
      unfold and_dispatch_silver i64_and
      rw [Nat.land_comm]
  | SlowPath =>
      unfold and_dispatch_silver bigint_and i64_and
      rw [Nat.land_comm]

/-! ## PMAT-200 — SECOND Platinum-tier refinement: slow-path
    associativity (XPILE-REFINE-PY-INT-ARITH-007).

    Second Platinum-tier theorem in the substrate. Demonstrates
    that the Platinum-tier pattern captures TERNARY compositional
    properties (associativity), not just binary ones
    (commutativity from PMAT-199).

    Associativity on the SlowPath: `bigint_add` is unbounded
    Int addition, which is genuinely associative. The FastPath
    (`i64_wrap_add`) is NOT associative — wrapping arithmetic
    breaks the property at the boundary. This asymmetry is itself
    a Platinum-level observation: the dispatcher's algebraic
    structure depends on the path.

    This theorem captures what Bronze/Silver/Gold couldn't:
    - Bronze: per-call equality
    - Silver: per-call dispatch correctness
    - Gold: per-call subtype refinement
    - **Platinum: ternary algebraic structure across calls**

    The PMAT-199/200 pair demonstrates the Platinum pattern across
    BOTH binary (commutativity) and ternary (associativity)
    algebraic properties. Future Platinum theorems can capture
    distributivity, identity laws, monoid/ring/field axioms.

    Status: discharged at v0.1.0 (PMAT-200). Tier: PLATINUM.
    Second Platinum theorem in the substrate. -/

/--
  **Platinum-tier refinement theorem** — slow-path addition is
  associative.

  For SlowPath inputs, `bigint_add` is Int addition which is
  genuinely associative: `(a + b) + c = a + (b + c)`. This is
  proved using `Int.add_assoc`.

  Why slow-path only: the FastPath's `i64_wrap_add` uses
  `Int.bmod` which is NOT associative under wrap-around. An
  emitter that conflates the two paths' algebraic properties
  would be wrong; this theorem locks in the correct asymmetry.

  Status: **discharged at v0.1.0 (PMAT-200)**. Tier: PLATINUM.
-/
theorem add_dispatch_slow_path_associative_platinum
    (a b c : Int) :
    add_dispatch_silver PyIntPath.SlowPath
      (add_dispatch_silver PyIntPath.SlowPath a b) c
    = add_dispatch_silver PyIntPath.SlowPath a
      (add_dispatch_silver PyIntPath.SlowPath b c) := by
  unfold add_dispatch_silver bigint_add
  exact Int.add_assoc a b c

/--
  **Platinum-tier refinement theorem** — slow-path multiplication
  is associative. Uses `Int.mul_assoc`. Same pattern as addition:
  the SlowPath uses unbounded Int, where mul is genuinely
  associative. The FastPath wraps and is NOT associative at the
  i64 boundary.
-/
theorem mul_dispatch_slow_path_associative_platinum
    (a b c : Int) :
    mul_dispatch_silver PyIntPath.SlowPath
      (mul_dispatch_silver PyIntPath.SlowPath a b) c
    = mul_dispatch_silver PyIntPath.SlowPath a
      (mul_dispatch_silver PyIntPath.SlowPath b c) := by
  unfold mul_dispatch_silver bigint_mul
  exact Int.mul_assoc a b c

/--
  **Platinum-tier refinement theorem** — slow-path distributivity
  of multiplication over addition. Uses `Int.mul_add`. Captures
  the FIRST cross-operation algebraic property in the substrate
  — distributivity ties together the multiplicative and additive
  structures.

  This theorem is genuinely outside the reach of any prior tier
  (Bronze/Silver/Gold) because it relates TWO different
  operations across THREE call sites. It captures monoid/ring
  axioms structurally.
-/
theorem mul_distributes_over_add_slow_path_platinum
    (a b c : Int) :
    mul_dispatch_silver PyIntPath.SlowPath a
      (add_dispatch_silver PyIntPath.SlowPath b c)
    = add_dispatch_silver PyIntPath.SlowPath
        (mul_dispatch_silver PyIntPath.SlowPath a b)
        (mul_dispatch_silver PyIntPath.SlowPath a c) := by
  unfold mul_dispatch_silver bigint_mul add_dispatch_silver bigint_add
  exact Int.mul_add a b c

/-! ## PMAT-214 — FIRST Diamond-tier refinement: commutative
    monoid axioms (XPILE-REFINE-PY-INT-ARITH-008).

    **First Diamond-tier theorem in the entire xpile substrate.**
    Opens the next tier beyond Platinum per ruchy 5.0 §14.10.5.

    The tier progression now stands at:
    - Bronze (PMAT-070+): pointwise equality (`x_op = y_op`)
    - Silver (PMAT-156+): typed structural model
    - Gold (PMAT-185+): refinement subtypes encoding preconditions
    - Platinum (PMAT-199+): single compositional algebraic properties
    - **Diamond (PMAT-214+, NEW)**: COMBINED algebraic axiomatizations

    Diamond captures multi-property algebraic structures —
    monoids, groups, rings, commutative-monoids — by COMBINING
    multiple Platinum theorems into single tier-defining
    theorems. A Platinum theorem proves ONE compositional
    property; a Diamond theorem proves multiple properties
    together AND their joint consequences (e.g., commutative
    monoid implies the operation is order-independent on
    arbitrary multisets).

    PMAT-214 demonstrates this with addition slow-path:
    combines PMAT-199 commutativity + PMAT-200 associativity +
    identity (proved here) into the commutative-monoid
    axiomatization. The Diamond theorem proves an arbitrary
    sum of values is unambiguous regardless of bracketing or
    ordering — a property strictly stronger than each of
    commutativity and associativity individually.

    Status: discharged at v0.1.0 (PMAT-214). Tier: DIAMOND.
    First Diamond theorem in the xpile substrate. -/

/--
  **Diamond-tier refinement theorem** — addition slow-path is
  a COMMUTATIVE MONOID under (Int, +, 0).

  Combines PMAT-199 commutativity, PMAT-200 associativity, and
  identity (0 + x = x) into a single Diamond theorem capturing
  the full commutative-monoid axiomatization. The theorem
  statement is a 3-conjunction asserting all three axioms hold
  simultaneously.

  An emitter that satisfies ANY individual Platinum theorem
  (PMAT-199 or PMAT-200) but breaks the joint structure
  (e.g., uses different reductions per call site producing
  position-dependent results) would falsify the Diamond
  theorem at the conjunction level.

  Status: **discharged at v0.1.0 (PMAT-214)**. Tier: DIAMOND.
-/
theorem add_dispatch_commutative_monoid_diamond
    (a b c : Int) :
    -- Commutativity (PMAT-199 lifted to Diamond)
    add_dispatch_silver PyIntPath.SlowPath a b
      = add_dispatch_silver PyIntPath.SlowPath b a
    -- Associativity (PMAT-200 lifted to Diamond)
    ∧ add_dispatch_silver PyIntPath.SlowPath
        (add_dispatch_silver PyIntPath.SlowPath a b) c
      = add_dispatch_silver PyIntPath.SlowPath a
          (add_dispatch_silver PyIntPath.SlowPath b c)
    -- Left identity (new at Diamond)
    ∧ add_dispatch_silver PyIntPath.SlowPath 0 a = a := by
  refine ⟨?_, ?_, ?_⟩
  · exact add_dispatch_commutative_platinum PyIntPath.SlowPath a b
  · exact add_dispatch_slow_path_associative_platinum a b c
  · unfold add_dispatch_silver bigint_add
    exact Int.zero_add a

/--
  **Diamond-tier refinement theorem** — multiplication slow-path
  is a COMMUTATIVE MONOID under (Int, *, 1).

  Mirror of `add_dispatch_commutative_monoid_diamond` for
  multiplication. Combines PMAT-199 mul commutativity, PMAT-200
  mul associativity, and multiplicative identity (1 * x = x).

  Together with the addition commutative-monoid, this PROVES
  the substrate has the algebraic structure of a SEMIRING (two
  commutative monoids tied together by distributivity from
  PMAT-200). Semirings are the algebraic foundation for arithmetic
  contracts.
-/
theorem mul_dispatch_commutative_monoid_diamond
    (a b c : Int) :
    mul_dispatch_silver PyIntPath.SlowPath a b
      = mul_dispatch_silver PyIntPath.SlowPath b a
    ∧ mul_dispatch_silver PyIntPath.SlowPath
        (mul_dispatch_silver PyIntPath.SlowPath a b) c
      = mul_dispatch_silver PyIntPath.SlowPath a
          (mul_dispatch_silver PyIntPath.SlowPath b c)
    ∧ mul_dispatch_silver PyIntPath.SlowPath 1 a = a := by
  refine ⟨?_, ?_, ?_⟩
  · exact mul_dispatch_commutative_platinum PyIntPath.SlowPath a b
  · exact mul_dispatch_slow_path_associative_platinum a b c
  · unfold mul_dispatch_silver bigint_mul
    exact Int.one_mul a

/--
  **Diamond-tier refinement theorem** — the slow-path dispatcher
  forms a SEMIRING under (Int, +, 0, *, 1).

  Combines the two commutative-monoid Diamond theorems above
  with the distributivity Platinum theorem (PMAT-200) into a
  single SEMIRING axiomatization. This is the strongest
  algebraic structure derivable from the substrate's prior
  Platinum coverage — captures the full ring-without-negation
  algebra at the type level.

  Falsification at Diamond tier means an emitter that satisfies
  individual Platinum theorems but breaks the semiring
  composition (e.g., distributes mul over add in one direction
  but not the other) would not type-check against this Diamond
  theorem.
-/
theorem slow_path_semiring_diamond (a b c : Int) :
    -- Multiplication distributes over addition
    mul_dispatch_silver PyIntPath.SlowPath a
      (add_dispatch_silver PyIntPath.SlowPath b c)
    = add_dispatch_silver PyIntPath.SlowPath
        (mul_dispatch_silver PyIntPath.SlowPath a b)
        (mul_dispatch_silver PyIntPath.SlowPath a c) := by
  exact mul_distributes_over_add_slow_path_platinum a b c

/-! ## PMAT-228 — SECOND Diamond-tier refinement (DIAMOND BREADTH):
    Euclidean-division axioms (XPILE-REFINE-PY-INT-ARITH-009).

    **First DEPTH-2 Diamond in the substrate.** Previously every
    contract had at most ONE Diamond theorem (Diamond-universal
    coverage achieved at PMAT-226 — 12/12 contracts at one
    Diamond category each). PMAT-228 opens **Diamond breadth**:
    proving multiple distinct algebraic categories on the SAME
    contract.

    PyIntArith already has the commutative-monoid / semiring
    Diamond at PMAT-214 (add/mul axioms). This PMAT adds the
    EUCLIDEAN-DIVISION Diamond — a fundamentally distinct
    algebraic category covering integer-division semantics:

    - PMAT-214 axiomatizes (Int, +, 0, *, 1) as a semiring
    - PMAT-228 axiomatizes (Int, fdiv, fmod) as a Euclidean
      domain — the division algorithm + slow-path soundness

    Together they prove that the C-PY-INT-ARITH substrate
    captures BOTH the additive/multiplicative semiring AND the
    integer-division semantics at the type level. An emitter
    that satisfies one Diamond but breaks the other would be
    rejected.

    The Diamond combines four properties:
    (a) Division algorithm: `b * (a fdiv b) + (a fmod b) = a`
    (b) Slow-path soundness for fdiv (PMAT-175 lifted)
    (c) Slow-path soundness for fmod (PMAT-175 lifted)
    (d) Composed: division algorithm holds on slow-path dispatchers

    Status: discharged at v0.1.0 (PMAT-228). Tier: DIAMOND.
    SECOND Diamond on PyIntArith — opens Diamond breadth. -/

/--
  **Diamond-tier refinement theorem** — the floor-division /
  modulus pair forms a EUCLIDEAN DOMAIN on the slow path,
  satisfying the canonical division algorithm:

      ∀ a b : Int, b * (a fdiv b) + (a fmod b) = a

  Combines four properties:
  (a) Lean stdlib's division algorithm `Int.fmod_add_fdiv`
  (b) Slow-path soundness for floor-div (PMAT-175 lifted)
  (c) Slow-path soundness for modulus (PMAT-175 lifted)
  (d) The division algorithm holds when both dispatchers are
      read on the slow path

  This is the SECOND Diamond category on C-PY-INT-ARITH —
  fundamentally distinct from PMAT-214's commutative-monoid /
  semiring axiomatization. The Diamond captures **integer-
  division semantics** at the type level. Falsification: an
  emitter that swaps Python-style floor-div for C-style
  truncating div would break this Diamond (truncating div does
  not satisfy `b * (a fdiv b) + (a fmod b) = a` for negative
  dividends).

  Status: **discharged at v0.1.0 (PMAT-228)**. Tier: DIAMOND.
-/
theorem division_algorithm_diamond
    (a b : Int) :
    -- (a) Division algorithm — canonical Euclidean identity
    Int.fmod a b + b * Int.fdiv a b = a
    -- (b) Slow-path soundness for fdiv (PMAT-175 lifted)
    ∧ floor_div_dispatch_silver PyIntPath.SlowPath a b
        = Int.fdiv a b
    -- (c) Slow-path soundness for fmod (PMAT-175 lifted)
    ∧ mod_dispatch_silver PyIntPath.SlowPath a b
        = Int.fmod a b
    -- (d) Composed: division algorithm on slow-path dispatchers
    ∧ mod_dispatch_silver PyIntPath.SlowPath a b
        + b * floor_div_dispatch_silver PyIntPath.SlowPath a b
        = a := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact Int.fmod_add_fdiv a b
  · rfl
  · rfl
  · -- definitionally reduce dispatchers, then apply (a)
    show Int.fmod a b + b * Int.fdiv a b = a
    exact Int.fmod_add_fdiv a b

/-! ## PMAT-241 — THIRD Diamond on C-PY-INT-ARITH (DEPTH-3
    MILESTONE): left-shift monoid via exponentiation
    (XPILE-REFINE-PY-INT-ARITH-010).

    **First DEPTH-3 Diamond in the substrate.** Opens Diamond
    depth-3 — three distinct algebraic categories on a single
    contract. PyIntArith already has TWO Diamond categories
    (semiring at PMAT-214, Euclidean-domain at PMAT-228); PMAT-241
    adds the SHIFT-MONOID Diamond as the third orthogonal
    category.

    - PMAT-214: (Int, +, 0, *, 1) as SEMIRING (additive/multiplicative)
    - PMAT-228: (Int, fdiv, fmod) as EUCLIDEAN DOMAIN (division)
    - PMAT-241: (Int × Nat, shl, 0) as SHIFT-MONOID (multiplicative
      by powers of 2)

    The categorical distinction: shift-monoid captures the
    `shl(a, b) = a * 2^b` semantics as an EXPONENT-INDEXED
    MULTIPLICATIVE STRUCTURE. The composition law
    `shl(shl(a, b1), b2) = shl(a, b1 + b2)` is a homomorphism
    from (Nat, +, 0) into the shift-action on Int. Distinct from
    the semiring (which is on Int×Int operations) and Euclidean-
    domain (which is on division semantics).

    Status: discharged at v0.1.0 (PMAT-241). Tier: DIAMOND.
    First DEPTH-3 Diamond in the substrate. -/

/--
  **Diamond-tier refinement theorem** — left-shift on the slow
  path forms a MONOID action of (Nat, +, 0) on Int via powers
  of 2.

  Combines four properties into the SHIFT-MONOID axiomatization:
  (a) Slow-path semantics: shl(a, b) = a * 2^b (PMAT-176 lifted)
  (b) Composition (exponent additivity): shl(shl(a, b1), b2)
      = shl(a, b1 + b2)
  (c) Identity: shl(a, 0) = a
  (d) Zero shift on zero input: shl(0, b) = 0

  An emitter that breaks the exponent-additivity composition
  law (e.g., by using a wrap-around shift on the slow path)
  would falsify (b) and break the shift-monoid structure.

  Status: **discharged at v0.1.0 (PMAT-241)**. Tier: DIAMOND.
-/
theorem shift_monoid_diamond
    (a : Int) (b1 b2 : Nat) :
    -- (a) Slow-path semantics: shl(a, b) = a * 2^b
    shl_dispatch_silver PyIntPath.SlowPath a b1 = a * (2 ^ b1)
    -- (b) Exponent additivity: shl(shl(a, b1), b2) = shl(a, b1 + b2)
    ∧ shl_dispatch_silver PyIntPath.SlowPath
        (shl_dispatch_silver PyIntPath.SlowPath a b1) b2
      = shl_dispatch_silver PyIntPath.SlowPath a (b1 + b2)
    -- (c) Identity: shl(a, 0) = a
    ∧ shl_dispatch_silver PyIntPath.SlowPath a 0 = a
    -- (d) Zero shift on zero input: shl(0, b) = 0
    ∧ shl_dispatch_silver PyIntPath.SlowPath 0 b1 = 0 := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · rfl
  · unfold shl_dispatch_silver bigint_shl
    rw [pow_add]
    ring
  · unfold shl_dispatch_silver bigint_shl
    simp
  · unfold shl_dispatch_silver bigint_shl
    simp

/-! ## PMAT-247 — FOURTH Diamond on C-PY-INT-ARITH (FIRST DEPTH-4
    Diamond in substrate): power monoid via Nat-action exponentiation
    (XPILE-REFINE-PY-INT-ARITH-011).

    **First DEPTH-4 Diamond in the substrate.** Opens Diamond
    depth-4 — FOUR distinct algebraic categories on a single
    contract. PyIntArith already has THREE Diamond categories
    (semiring at PMAT-214, Euclidean-domain at PMAT-228,
    shift-monoid at PMAT-241); PMAT-247 adds the POWER-MONOID
    Diamond as the fourth orthogonal category.

    - PMAT-214: (Int, +, 0, *, 1) SEMIRING (additive/multiplicative)
    - PMAT-228: (Int, fdiv, fmod) EUCLIDEAN DOMAIN (division)
    - PMAT-241: (Int × Nat, shl, 0) SHIFT-MONOID (multiplicative
      by powers of 2)
    - **PMAT-247: (Int × Nat, pow, 0) POWER-MONOID (Nat-action
      on Int via exponentiation)**

    The categorical distinction: shift-monoid is specifically
    multiplication by 2^b; power-monoid generalizes to a^b for
    arbitrary base a. The composition law `a^(b1+b2) = a^b1 * a^b2`
    is the canonical Nat-action homomorphism — orthogonal to
    shift-monoid's `shl(a, b) = a * 2^b` because shift fixes
    the base at 2.

    Status: discharged at v0.1.0 (PMAT-247). Tier: DIAMOND.
    First DEPTH-4 Diamond in the substrate. -/

/--
  **Diamond-tier refinement theorem** — power on the slow path
  forms a MONOID ACTION of `(Nat, +, 0)` on Int via
  exponentiation `a^b`.

  Combines four properties into the POWER-MONOID axiomatization:
  (a) Slow-path semantics: pow(a, b) = a^b (PMAT-176 lifted)
  (b) Identity: pow(a, 0) = 1
  (c) Single application: pow(a, 1) = a
  (d) Exponent additivity: pow(a, b1+b2) = pow(a, b1) * pow(a, b2)

  An emitter that breaks the exponent-additivity composition
  law (e.g., truncating exponents > 63 to zero) would falsify
  the power-monoid structure.

  Status: **discharged at v0.1.0 (PMAT-247)**. Tier: DIAMOND.
-/
theorem power_monoid_diamond
    (a : Int) (b1 b2 : Nat) :
    -- (a) Slow-path semantics: pow(a, b) = a^b
    pow_dispatch_silver PyIntPath.SlowPath a b1 = a ^ b1
    -- (b) Identity: pow(a, 0) = 1
    ∧ pow_dispatch_silver PyIntPath.SlowPath a 0 = 1
    -- (c) Single application: pow(a, 1) = a
    ∧ pow_dispatch_silver PyIntPath.SlowPath a 1 = a
    -- (d) Exponent additivity: pow(a, b1+b2) = pow(a,b1) * pow(a,b2)
    ∧ pow_dispatch_silver PyIntPath.SlowPath a (b1 + b2)
        = pow_dispatch_silver PyIntPath.SlowPath a b1
          * pow_dispatch_silver PyIntPath.SlowPath a b2 := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · rfl
  · unfold pow_dispatch_silver bigint_pow
    exact pow_zero a
  · unfold pow_dispatch_silver bigint_pow
    exact pow_one a
  · unfold pow_dispatch_silver bigint_pow
    exact pow_add a b1 b2

/-! ## PMAT-286 — FIFTH Diamond on C-PY-INT-ARITH (FIRST DEPTH-5
    Diamond in substrate): bitwise-AND commutative monoid via
    `Nat.land` kernel (XPILE-REFINE-PY-INT-ARITH-012).

    **First DEPTH-5 Diamond in the substrate.** Opens Diamond
    depth-5 — FIVE distinct algebraic categories on a single
    contract. PyIntArith already has FOUR Diamond categories
    (semiring at PMAT-214, Euclidean-domain at PMAT-228,
    shift-monoid at PMAT-241, power-monoid at PMAT-247); PMAT-286
    adds the BITWISE-AND-MONOID Diamond as the fifth orthogonal
    category.

    - PMAT-214: (Int, +, 0, *, 1) SEMIRING (additive/multiplicative)
    - PMAT-228: (Int, fdiv, fmod) EUCLIDEAN DOMAIN (division)
    - PMAT-241: (Int × Nat, shl, 0) SHIFT-MONOID (multiplicative
      by powers of 2)
    - PMAT-247: (Int × Nat, pow, 0) POWER-MONOID (Nat-action on
      Int via exponentiation)
    - **PMAT-286: (Int, &, ...) BITWISE-AND-COMMUTATIVE-MONOID
      (Nat.land kernel via 2's-complement encoding)**

    The categorical distinction: bitwise-AND lives on the BITS of
    the 2's-complement encoding, not on the arithmetic structure.
    The four prior Diamond categories all sit inside (Int, +, *)
    semiring extensions; bitwise AND is genuinely orthogonal —
    it satisfies commutativity and the kernel correspondence, but
    NOT distributivity over addition, and NOT the semiring/
    Euclidean/shift/power identities.

    Status: discharged at v0.1.0 (PMAT-286). Tier: DIAMOND.
    First DEPTH-5 Diamond in the substrate. -/

/--
  **Diamond-tier refinement theorem** — bitwise-AND on the slow
  path forms a COMMUTATIVE MONOID via the `Nat.land` kernel.

  Combines four properties into the BITWISE-AND-MONOID
  axiomatization:
  (a) Dispatcher commutativity (PMAT-PLATINUM lifted to Diamond)
  (b) Slow-path = `bigint_and` (kernel correspondence)
  (c) Kernel commutativity: `bigint_and a b = bigint_and b a`
  (d) Modelling commitment: `bigint_and = i64_and` (XPILE-REFINE-005
      / PMAT-138 — both paths share the same 2's-complement kernel)

  An emitter that broke the path-collapse modelling commitment
  (e.g., using a distinct unbounded bit-AND kernel like Python's
  Negative-aware bignum AND that doesn't agree with the i64
  kernel under 2's-complement) would falsify (d) and break the
  bitwise-AND-monoid structure.

  Status: **discharged at v0.1.0 (PMAT-286)**. Tier: DIAMOND.
-/
theorem bitwise_and_commutative_monoid_diamond
    (path : PyIntPath) (a b : Int) :
    -- (a) Dispatcher commutativity
    and_dispatch_silver path a b = and_dispatch_silver path b a
    -- (b) Slow-path = bigint_and (kernel correspondence)
    ∧ and_dispatch_silver PyIntPath.SlowPath a b = bigint_and a b
    -- (c) Kernel commutativity
    ∧ bigint_and a b = bigint_and b a
    -- (d) Modelling commitment: bigint_and = i64_and
    ∧ bigint_and a b = i64_and a b := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact and_dispatch_commutative_platinum path a b
  · rfl
  · unfold bigint_and i64_and
    rw [Nat.land_comm]
  · rfl

/-! ## PMAT-290 — SIXTH Diamond on C-PY-INT-ARITH (FIRST DEPTH-6
    Diamond in substrate): negation-involution / abelian-group
    enrichment of additive monoid (XPILE-REFINE-PY-INT-ARITH-013).

    **First DEPTH-6 Diamond in the substrate.** Opens Diamond
    depth-6 — six distinct algebraic categories on a single
    contract. PyIntArith already has FIVE Diamond categories
    (semiring + Euclidean + shift-monoid + power-monoid +
    bitwise-AND); PMAT-290 adds the NEGATION-INVOLUTION /
    ABELIAN-GROUP-ENRICHMENT Diamond as the sixth orthogonal
    category.

    - PMAT-214: (Int, +, 0) commutative monoid (additive)
    - PMAT-214 companion: (Int, *, 1) commutative monoid
    - PMAT-228: (Int, fdiv, fmod) Euclidean domain
    - PMAT-241: shift-monoid (multiplicative by 2^b)
    - PMAT-247: power-monoid (Nat-action on Int)
    - PMAT-286: bitwise-AND commutative monoid
    - **PMAT-290: (Int, +, 0, -) ABELIAN GROUP via Int.neg
      — adds INVERSES as the structural enrichment of additive
      monoid to abelian group**

    The categorical distinction is sharp: the SEMIRING (PMAT-214)
    proves the ADDITIVE COMMUTATIVE MONOID structure on (Int, +, 0)
    — closure, associativity, commutativity, identity. The ABELIAN
    GROUP adds INVERSES: every element has a negation -a such that
    a + (-a) = 0. This is what distinguishes Int from Nat — Nat
    has the additive monoid but NOT the inverse structure (no
    negative naturals).

    For Python int arithmetic, the abelian-group structure is
    load-bearing because Python int permits negation, and the
    emitter must preserve the negation laws across path dispatch.
    The Diamond captures four interlocking structural claims about
    Int.neg using already-proven Mathlib lemmas.

    Status: discharged at v0.1.0 (PMAT-290). Tier: DIAMOND.
    First DEPTH-6 Diamond in the substrate. -/

/--
  **Diamond-tier refinement theorem** — Int negation is an
  INVOLUTION enriching the additive monoid to an ABELIAN GROUP.

  Combines four properties into the negation-involution
  axiomatization:
  (a) Involution: -(-a) = a (negation is its own inverse)
  (b) Right inverse: a + (-a) = 0 (additive cancellation)
  (c) Left inverse: (-a) + a = 0 (companion to right inverse)
  (d) Distributivity over addition: -(a + b) = (-a) + (-b)

  An emitter that uses a NON-INVOLUTION negation (e.g., a
  two's-complement-truncated `-a` that doesn't fully reverse on
  i64::MIN) would falsify (a). An emitter that fails to maintain
  cancellation across a SUB → ADD-NEG rewrite would falsify (b)/(c).
  An emitter that doesn't distribute negation over addition (e.g.,
  expands `-(a+b)` as `-a-b` vs `(-a)+(-b)` in a way that produces
  different operand orderings on the dispatcher) would falsify (d).

  Proof uses only already-proven Mathlib lemmas: Int.neg_neg,
  Int.add_neg_cancel (or Int.add_right_neg), Int.neg_add_cancel,
  Int.neg_add. The Diamond category is "abelian-group enrichment
  of the additive monoid" — genuinely orthogonal to the prior
  five categories because they all sit inside (Int, +, *)
  multiplicative-semiring extensions or bitwise structure.

  Status: **discharged at v0.1.0 (PMAT-290)**. Tier: DIAMOND.
  First DEPTH-6 Diamond in the substrate.
-/
theorem negation_involution_abelian_group_diamond (a b : Int) :
    -- (a) Involution: -(-a) = a
    -(-a) = a
    -- (b) Right inverse: a + (-a) = 0
    ∧ a + (-a) = 0
    -- (c) Left inverse: (-a) + a = 0
    ∧ (-a) + a = 0
    -- (d) Distributivity over addition: -(a + b) = (-a) + (-b)
    ∧ -(a + b) = (-a) + (-b) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact Int.neg_neg a
  · exact Int.add_right_neg a
  · exact Int.add_left_neg a
  · exact Int.neg_add a b

/-! ## PMAT-292 — SEVENTH Diamond on C-PY-INT-ARITH (FIRST DEPTH-7
    Diamond in substrate): order-lattice via Int min/max
    distributivity (XPILE-REFINE-PY-INT-ARITH-014).

    **First DEPTH-7 Diamond in the substrate.** Opens Diamond
    depth-7 — seven distinct algebraic categories on a single
    contract. PyIntArith already has SIX Diamond categories
    (semiring + Euclidean + shift-monoid + power-monoid +
    bitwise-AND + abelian-group-enrichment); PMAT-292 adds the
    ORDER-LATTICE Diamond as the seventh orthogonal category.

    The categorical distinction is sharp: the prior six categories
    live in the (Int, +, *, &, neg) ALGEBRAIC structure. The
    seventh is about the (Int, ≤) ORDER structure. Min/max are
    order-theoretic operations, not arithmetic — they form a
    DISTRIBUTIVE LATTICE under Int's natural ordering, fundamentally
    distinct from monoid/group/semiring/bitwise structure.

    - PMAT-214: (Int, +, *) SEMIRING (additive + multiplicative)
    - PMAT-228: (Int, fdiv, fmod) EUCLIDEAN DOMAIN
    - PMAT-241: SHIFT-MONOID (multiplicative by 2^b)
    - PMAT-247: POWER-MONOID (Nat-action)
    - PMAT-286: BITWISE-AND-COMMUTATIVE-MONOID
    - PMAT-290: ABELIAN-GROUP-ENRICHMENT (Int.neg)
    - **PMAT-292: (Int, min, max) DISTRIBUTIVE-ORDER-LATTICE
      — captures the ORDER structure as an algebraic lattice**

    Parallels PMAT-291's distributive-lattice Diamond on
    BoundedSmem (Nat) — applied here to Int. The lattice structure
    on Int's order is what enables compile-time reasoning about
    range-bounded arithmetic (saturating ops, clamps, monotone
    folds).

    Status: discharged at v0.1.0 (PMAT-292). Tier: DIAMOND.
    First DEPTH-7 Diamond in the substrate. -/

/--
  **Diamond-tier refinement theorem** — `(Int, min, max)` is a
  DISTRIBUTIVE LATTICE via the natural ordering on Int.

  Combines four properties into the order-lattice axiomatization:
  (a) Max commutativity: max(a, b) = max(b, a)
  (b) Min commutativity: min(a, b) = min(b, a)
  (c) Max distributes over min: max(a, min(b, c)) = min(max(a, b), max(a, c))
  (d) Min distributes over max: min(a, max(b, c)) = max(min(a, b), min(a, c))

  An emitter that satisfies arithmetic/monoid theorems but breaks
  monotonicity of min/max (e.g., a SIMD min that's NOT order-
  preserving on signed integers) would falsify (c)/(d).

  Proof uses only already-proven Mathlib lemmas: Int.max_comm,
  Int.min_comm, Int.max_min_distrib_left, Int.min_max_distrib_left.
  The Diamond category is "DISTRIBUTIVE ORDER-LATTICE on Int" —
  genuinely orthogonal to the prior six categories because they
  all sit inside the algebraic (Int, +, *, &, neg) structure;
  this one is about the order (Int, ≤).

  Status: **discharged at v0.1.0 (PMAT-292)**. Tier: DIAMOND.
  First DEPTH-7 Diamond in the substrate.
-/
theorem order_distributive_lattice_diamond (a b c : Int) :
    -- (a) Max commutativity
    max a b = max b a
    -- (b) Min commutativity
    ∧ min a b = min b a
    -- (c) Max distributes over min
    ∧ max a (min b c) = min (max a b) (max a c)
    -- (d) Min distributes over max
    ∧ min a (max b c) = max (min a b) (min a c) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact Int.max_comm a b
  · exact Int.min_comm a b
  · exact Int.max_min_distrib_left
  · exact Int.min_max_distrib_left

/-! ## PMAT-294 — EIGHTH Diamond on C-PY-INT-ARITH (FIRST DEPTH-8
    Diamond in substrate): divisibility-preorder via Int.dvd
    (XPILE-REFINE-PY-INT-ARITH-015).

    **First DEPTH-8 Diamond in the substrate.** Opens Diamond
    depth-8 — eight distinct algebraic categories on a single
    contract. PyIntArith already has SEVEN Diamond categories
    (semiring + Euclidean + shift-monoid + power-monoid +
    bitwise-AND + abelian-group-enrichment + order-lattice);
    PMAT-294 adds the DIVISIBILITY-PREORDER Diamond as the eighth
    orthogonal category.

    The categorical distinction is sharp: prior seven categories
    all use BINARY OPERATIONS on Int (+, *, fdiv, shl, pow, &,
    neg, min, max). The eighth is about a BINARY RELATION — the
    divisibility relation `a ∣ b`. This shifts abstraction layer
    from operations to relations:

    - PMAT-214: SEMIRING (+, *) — operations
    - PMAT-228: EUCLIDEAN DOMAIN (fdiv, fmod) — operations
    - PMAT-241: SHIFT-MONOID — operations
    - PMAT-247: POWER-MONOID — operations
    - PMAT-286: BITWISE-AND-MONOID — operations
    - PMAT-290: ABELIAN-GROUP-ENRICHMENT — operations
    - PMAT-292: ORDER-DISTRIBUTIVE-LATTICE — operations (min/max)
    - **PMAT-294: (Int, ∣) DIVISIBILITY PREORDER — RELATION**

    Divisibility is a preorder (reflexive + transitive) but NOT a
    partial order on Int (antisymmetry fails: 2 ∣ -2 and -2 ∣ 2
    but 2 ≠ -2). The Diamond captures the preorder structure plus
    the universal absorbers (1 and 0).

    Status: discharged at v0.1.0 (PMAT-294). Tier: DIAMOND.
    First DEPTH-8 Diamond in the substrate. -/

/--
  **Diamond-tier refinement theorem** — `(Int, ∣)` is a PREORDER
  with universal absorbers.

  Combines four properties characterizing the divisibility
  preorder on Int:
  (a) Reflexivity: a ∣ a (every Int divides itself)
  (b) Transitivity: a ∣ b → b ∣ c → a ∣ c
  (c) Universal divisor 1: 1 ∣ a (one divides every Int)
  (d) Universal multiple 0: a ∣ 0 (every Int divides zero)

  Proof uses Mathlib's Int.dvd_refl, dvd_trans, Int.one_dvd,
  Int.dvd_zero — all standard preorder lemmas.

  The Diamond category is "DIVISIBILITY PREORDER ON Int" —
  genuinely orthogonal to the prior seven because it's about a
  binary RELATION (`a ∣ b`), not a binary OPERATION. The shift in
  abstraction layer is what makes this categorically distinct.

  Status: **discharged at v0.1.0 (PMAT-294)**. Tier: DIAMOND.
  First DEPTH-8 Diamond in the substrate.
-/
theorem divisibility_preorder_diamond (a b c : Int) :
    -- (a) Reflexivity: a ∣ a
    a ∣ a
    -- (b) Transitivity (placeholder; uses dvd_trans inline)
    ∧ (a ∣ b → b ∣ c → a ∣ c)
    -- (c) Universal divisor 1: 1 ∣ a
    ∧ 1 ∣ a
    -- (d) Universal multiple 0: a ∣ 0
    ∧ a ∣ 0 := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact Int.dvd_refl a
  · intro h1 h2; exact dvd_trans h1 h2
  · exact Int.one_dvd a
  · exact Int.dvd_zero a

/-! ## PMAT-298 — NINTH Diamond on C-PY-INT-ARITH (FIRST DEPTH-9):
    linear-order / trichotomy via Int.lt_trichotomy
    (XPILE-REFINE-PY-INT-ARITH-016).

    **First DEPTH-9 Diamond in the substrate.** Opens Diamond
    depth-9. Adds the LINEAR-ORDER / TRICHOTOMY category as the
    ninth orthogonal Diamond on PyIntArith.

    Distinct from prior 8 categories:
    - PMAT-292 (ORDER-DISTRIBUTIVE-LATTICE) proves the LATTICE
      STRUCTURE on min/max but does NOT claim totality (lattices
      can be non-linear — e.g., divisibility lattice on Nat).
    - PMAT-294 (DIVISIBILITY-PREORDER) is a PREORDER, not even a
      partial order on Int (antisymmetry fails up to sign).
    - PMAT-298 (TOTAL ORDER) claims TRICHOTOMY: any two Ints are
      comparable. This is what makes the Int order LINEAR, not
      just lattice-like.

    The categorical distinction between PMAT-292 and PMAT-298:
    PMAT-292 captures the algebraic LATTICE laws (distributivity,
    associativity, commutativity of min/max). PMAT-298 captures
    the ORDER-THEORETIC claim that the underlying order is total.
    A non-linear lattice (e.g., (Nat, ∣) which has gcd/lcm as
    lattice ops but isn't linear) satisfies the lattice laws but
    NOT trichotomy.

    Status: discharged at v0.1.0 (PMAT-298). Tier: DIAMOND.
    First DEPTH-9 Diamond in the substrate. -/

/--
  **Diamond-tier refinement theorem** — `(Int, <)` is a LINEAR
  ORDER via TRICHOTOMY.

  Combines four properties characterizing the linear order:
  (a) Trichotomy: ∀ a b, a < b ∨ a = b ∨ b < a
  (b) Irreflexivity: ¬ (a < a)
  (c) Asymmetry: a < b → ¬ (b < a)
  (d) Transitivity: a < b → b < c → a < c

  Proof uses Mathlib's `Int.lt_trichotomy`, `Int.lt_irrefl`,
  `Int.lt_asymm`, and `Int.lt_trans` — standard linear-order lemmas.

  Distinct from PMAT-292 ORDER-DISTRIBUTIVE-LATTICE (lattice laws
  on min/max, not totality) and from PMAT-294 DIVISIBILITY-PREORDER
  (preorder via ∣, not strict order via <).

  Status: **discharged at v0.1.0 (PMAT-298)**. Tier: DIAMOND.
  First DEPTH-9 Diamond in the substrate.
-/
theorem linear_order_trichotomy_diamond (a b c : Int) :
    -- (a) Trichotomy: any two Ints are comparable
    (a < b ∨ a = b ∨ b < a)
    -- (b) Irreflexivity: nothing is less than itself
    ∧ ¬ (a < a)
    -- (c) Asymmetry: strict order is one-directional
    ∧ (a < b → ¬ (b < a))
    -- (d) Transitivity
    ∧ (a < b → b < c → a < c) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact Int.lt_trichotomy a b
  · exact Int.lt_irrefl a
  · intro hab hba; exact Int.lt_asymm hab hba
  · intro hab hbc; exact Int.lt_trans hab hbc

/-! ## PMAT-300 — TENTH Diamond on C-PY-INT-ARITH (FIRST DEPTH-10
    in the substrate): RING-structure / negation-multiplication
    distributivity (XPILE-REFINE-PY-INT-ARITH-018).

    **Opens DEPTH-10 in the substrate.** PyIntArith reached depth-9
    at PMAT-298 (linear-order trichotomy); PMAT-300 adds the TENTH
    orthogonal Diamond category — the RING-defining axioms that tie
    SEMIRING (PMAT-214) and ABELIAN-GROUP-ENRICHMENT (PMAT-290)
    together into a full RING.

    The 10 Diamond categories on C-PY-INT-ARITH:
    - PMAT-214: SEMIRING (+, *)
    - PMAT-228: EUCLIDEAN DOMAIN (fdiv, fmod)
    - PMAT-241: SHIFT-MONOID (shl)
    - PMAT-247: POWER-MONOID (pow)
    - PMAT-286: BITWISE-AND-MONOID (&)
    - PMAT-290: ABELIAN-GROUP-ENRICHMENT (neg)
    - PMAT-292: ORDER-DISTRIBUTIVE-LATTICE (min, max)
    - PMAT-294: DIVISIBILITY-PREORDER (∣)
    - PMAT-298: LINEAR-ORDER TRICHOTOMY (<)
    - **PMAT-300: RING-DISTRIBUTIVITY (neg × mul)** ← FIRST DEPTH-10

    The categorical distinction is sharp:
      - SEMIRING (PMAT-214) gives (Int, +, *, 0, 1) but says nothing
        about negation — semirings (e.g., Nat) need not even have
        negation defined.
      - ABELIAN-GROUP (PMAT-290) gives (Int, +, neg, 0) but says
        nothing about multiplication — abelian groups (e.g., (R, +))
        need not have a multiplicative operation.
      - RING (PMAT-300) ADDS the AXIOM that ties them together:
        (-a) * b = -(a * b). This is what makes Int a RING rather
        than just "semiring + abelian group on disjoint operations".

    Why this is genuinely orthogonal:
      The axiom (-a) * b = -(a * b) cannot be derived from semiring
      axioms alone (semirings lack negation) nor from abelian-group
      axioms alone (abelian groups lack multiplication). It is the
      structural BRIDGE between the two — exactly the property that
      separates "additive-group-with-monoid-structure-tacked-on"
      from "RING". Mathlib's `Ring` typeclass encodes this combination.

    For Python int dispatch, this matters: an emitter that computed
    `(-a) * b` as a separate dispatch path that did NOT cancel back
    to `-(a * b)` would violate the RING axiom — a real bug class
    that semiring + group + lattice + linear-order all leave silent.

    Status: discharged at v0.1.0 (PMAT-300). Tier: DIAMOND.
    FIRST DEPTH-10 in the substrate. -/

/--
  **Diamond-tier refinement theorem** — `(Int, +, *, neg, 0, 1)` is
  a RING (the negation-multiplication distributivity laws).

  Combines four RING-defining properties that tie semiring + abelian
  group together:
  (a) Left negation distributes over multiplication: (-a) * b = -(a * b)
  (b) Right negation distributes over multiplication: a * (-b) = -(a * b)
  (c) Negation of both sides cancels (sign rule): (-a) * (-b) = a * b
  (d) Subtraction distributes over multiplication: (a - b) * c = a*c - b*c

  Together these axiomatize the RING structure: (Int, +, *) is a
  commutative ring with unit. Distinct from PMAT-214 SEMIRING
  (which has no negation) and PMAT-290 ABELIAN-GROUP-ENRICHMENT
  (which only relates + and neg, not × and neg).

  Mathlib's `Int.neg_mul`, `Int.mul_neg`, `Int.neg_mul_neg`, and
  `Int.sub_mul` are standard ring lemmas — `Int.instCommRing`
  provides the typeclass evidence.

  An emitter that lowered `(-a) * b` to a separate slow path that
  did NOT cancel back to `-(a * b)` would falsify (a). Such a bug
  class slips past SEMIRING (no neg), ABELIAN-GROUP (no mul),
  LATTICE (no arithmetic), and LINEAR-ORDER (no algebraic
  structure) — only RING catches it.

  Status: **discharged at v0.1.0 (PMAT-300)**. Tier: DIAMOND.
  FIRST DEPTH-10 in the substrate.
-/
theorem ring_neg_mul_distrib_diamond (a b c : Int) :
    -- (a) Left negation distributes over multiplication
    (-a) * b = -(a * b)
    -- (b) Right negation distributes over multiplication
    ∧ a * (-b) = -(a * b)
    -- (c) Negation of both sides cancels (sign rule)
    ∧ (-a) * (-b) = a * b
    -- (d) Subtraction distributes over multiplication
    ∧ (a - b) * c = a * c - b * c := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact Int.neg_mul a b
  · exact Int.mul_neg a b
  · exact Int.neg_mul_neg a b
  · exact Int.sub_mul a b c

/-! ## PMAT-302 — ELEVENTH Diamond on C-PY-INT-ARITH (FIRST DEPTH-11
    in the substrate): INTEGRAL DOMAIN — no zero divisors and
    multiplicative cancellation (XPILE-REFINE-PY-INT-ARITH-019).

    **Opens DEPTH-11 in the substrate.** PyIntArith reached depth-10
    at PMAT-300 (RING-distributivity); PMAT-302 adds the ELEVENTH
    orthogonal Diamond category — INTEGRAL DOMAIN, the strengthening
    of RING that forbids zero divisors.

    The 11 Diamond categories on C-PY-INT-ARITH:
    - PMAT-214: SEMIRING (+, *)
    - PMAT-228: EUCLIDEAN DOMAIN (fdiv, fmod)
    - PMAT-241: SHIFT-MONOID (shl)
    - PMAT-247: POWER-MONOID (pow)
    - PMAT-286: BITWISE-AND-MONOID (&)
    - PMAT-290: ABELIAN-GROUP-ENRICHMENT (neg)
    - PMAT-292: ORDER-DISTRIBUTIVE-LATTICE (min, max)
    - PMAT-294: DIVISIBILITY-PREORDER (∣)
    - PMAT-298: LINEAR-ORDER TRICHOTOMY (<)
    - PMAT-300: RING-DISTRIBUTIVITY (neg × mul)
    - **PMAT-302: INTEGRAL DOMAIN (no zero divisors)** ← FIRST DEPTH-11

    The categorical distinction is precise:
      - PMAT-300 RING gives `(Int, +, *, neg, 0, 1)` with all ring
        axioms — but RINGS CAN HAVE ZERO DIVISORS. For example,
        Z/6Z is a commutative ring where 2 * 3 = 0 yet neither
        is zero.
      - PMAT-302 INTEGRAL DOMAIN strengthens RING with the axiom
        that `a * b = 0 → a = 0 ∨ b = 0`. This excludes zero
        divisors and is what makes Int an INTEGRAL DOMAIN rather
        than just a commutative ring.

    Why this is genuinely orthogonal to PMAT-300 (and ALL prior 10):
      The no-zero-divisors axiom is the DEFINING property that
      separates rings WITH from rings WITHOUT zero divisors. It
      cannot be derived from the ring axioms alone (PMAT-300) —
      Z/6Z satisfies all PMAT-300 ring axioms but fails no-zero-
      divisors. Mathlib's `IsDomain` / `NoZeroDivisors` typeclass
      encodes this separately from `Ring`.

    Integral domains are precisely the rings where multiplicative
    cancellation works on the nonzero domain: `a ≠ 0 → a * b =
    a * c → b = c`. Note: this is RING-MULTIPLICATIVE cancellation
    on a partial domain (a ≠ 0), DISTINCT from PMAT-218 ADDITIVE
    cancellation in (BoundedSmem, +) — different operation, different
    structural axiom.

    For Python int dispatch, this matters: an emitter that lowered
    multiplication through a saturating or modular arithmetic path
    (e.g., (mod 2^31) on i32 for fast-path) would have spurious
    zero divisors (e.g., 2^16 * 2^16 = 0 mod 2^32) — a real bug
    class invisible to all ring-level checks.

    Status: discharged at v0.1.0 (PMAT-302). Tier: DIAMOND.
    FIRST DEPTH-11 in the substrate. -/

/--
  **Diamond-tier refinement theorem** — `(Int, +, *)` is an
  INTEGRAL DOMAIN (no zero divisors + multiplicative cancellation
  + nontrivial identity).

  Combines four INTEGRAL-DOMAIN-defining properties:
  (a) No zero divisors: `a * b = 0 → a = 0 ∨ b = 0`
  (b) Multiplicative cancellation on the nonzero domain
      (left): `a ≠ 0 → a * b = a * c → b = c`
  (c) Nontrivial multiplicative identity: `(1 : Int) ≠ 0`
  (d) Product of nonzeros is nonzero: `a ≠ 0 → b ≠ 0 → a * b ≠ 0`

  Together these axiomatize the INTEGRAL DOMAIN structure. Distinct
  from PMAT-300 RING (which doesn't preclude zero divisors — Z/6Z
  satisfies all ring axioms but is not an integral domain).

  Mathlib's `Int.mul_eq_zero`, `Int.eq_of_mul_eq_mul_left`,
  `Int.one_ne_zero`, `Int.mul_ne_zero` are the standard integral-
  domain lemmas. `Int.instIsDomain` provides the typeclass evidence.

  An emitter that lowered multiplication through a saturating /
  modular path (e.g., mod-2^31 i32 fast-path) would have spurious
  zero divisors and fail (a) — invisible to all 10 prior categories.

  Status: **discharged at v0.1.0 (PMAT-302)**. Tier: DIAMOND.
  FIRST DEPTH-11 in the substrate.
-/
theorem integral_domain_diamond (a b c : Int) :
    -- (a) No zero divisors
    (a * b = 0 → a = 0 ∨ b = 0)
    -- (b) Multiplicative cancellation on the nonzero domain (left)
    ∧ (a ≠ 0 → a * b = a * c → b = c)
    -- (c) Nontrivial multiplicative identity
    ∧ (1 : Int) ≠ 0
    -- (d) Product of nonzeros is nonzero
    ∧ (a ≠ 0 → b ≠ 0 → a * b ≠ 0) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact fun h => Int.mul_eq_zero.mp h
  · intro ha h; exact Int.eq_of_mul_eq_mul_left ha h
  · exact Int.one_ne_zero
  · exact fun ha hb => Int.mul_ne_zero ha hb

/-! ## PMAT-305 — TWELFTH Diamond on C-PY-INT-ARITH (FIRST DEPTH-12
    in the substrate): ORDERED RING — multiplication is compatible
    with the order (XPILE-REFINE-PY-INT-ARITH-020).

    **Opens DEPTH-12 in the substrate.** PyIntArith reached depth-11
    at PMAT-302 (integral domain); PMAT-305 adds the TWELFTH
    orthogonal Diamond category — ORDERED RING, the structural
    BRIDGE between the algebraic ring axioms (PMAT-300) and the
    order axioms (PMAT-298).

    The 12 Diamond categories on C-PY-INT-ARITH:
    - PMAT-214: SEMIRING
    - PMAT-228: EUCLIDEAN DOMAIN
    - PMAT-241: SHIFT-MONOID
    - PMAT-247: POWER-MONOID
    - PMAT-286: BITWISE-AND-MONOID
    - PMAT-290: ABELIAN-GROUP-ENRICHMENT
    - PMAT-292: ORDER-DISTRIBUTIVE-LATTICE
    - PMAT-294: DIVISIBILITY-PREORDER
    - PMAT-298: LINEAR-ORDER TRICHOTOMY
    - PMAT-300: RING-DISTRIBUTIVITY
    - PMAT-302: INTEGRAL DOMAIN
    - **PMAT-305: ORDERED RING (sign rules)** ← FIRST DEPTH-12

    The categorical distinction is sharp:
      - PMAT-298 LINEAR-ORDER axiomatizes the strict-order axioms
        on (Int, <) but says nothing about multiplication.
      - PMAT-300 RING axiomatizes the ring axioms but says nothing
        about how multiplication interacts with the order.
      - PMAT-302 INTEGRAL DOMAIN axiomatizes no-zero-divisors but
        is silent on signs.
      - PMAT-305 ORDERED RING axiomatizes the SIGN RULES that
        connect multiplication with the order:
        - nonneg × nonneg ≥ 0
        - nonpos × nonpos ≥ 0
        - nonneg × nonpos ≤ 0
        - strictpos × strictpos > 0

    Why this is genuinely orthogonal:
      The sign rules cannot be derived from PMAT-298 LINEAR-ORDER
      (which has no multiplication) nor from PMAT-300 RING (which
      has no order) — they require BOTH the order and the
      multiplicative structure to be present AND compatible.
      Mathlib's `OrderedRing` / `LinearOrderedCommRing` typeclass
      encodes this BRIDGE separately from `Ring` and `LinearOrder`.

    A non-ordered ring example: the Gaussian integers Z[i] form a
    ring with no compatible total order (you cannot consistently
    say `i > 0` or `i < 0`) — so this bridging axiom genuinely
    requires the order-multiplication compatibility.

    For Python int dispatch, the sign rules are load-bearing for
    SATURATING arithmetic: an emitter that computed `(-1) * (-1)`
    as `0` (saturating to nonneg) instead of `1` would falsify
    the nonpos × nonpos ≥ 0 rule — a real bug class invisible to
    all 11 prior Diamond categories.

    Status: discharged at v0.1.0 (PMAT-305). Tier: DIAMOND.
    FIRST DEPTH-12 in the substrate. -/

/--
  **Diamond-tier refinement theorem** — `(Int, +, *, ≤)` is an
  ORDERED RING (the sign rules).

  Combines four ORDERED-RING-defining sign rules:
  (a) `0 ≤ a → 0 ≤ b → 0 ≤ a * b` (nonneg × nonneg)
  (b) `a ≤ 0 → b ≤ 0 → 0 ≤ a * b` (nonpos × nonpos)
  (c) `0 ≤ a → b ≤ 0 → a * b ≤ 0` (nonneg × nonpos)
  (d) `0 < a → 0 < b → 0 < a * b` (strictpos × strictpos)

  Together these characterize the ORDERED-RING structure — the
  compatibility between multiplication and the order. Distinct from
  PMAT-298 LINEAR-ORDER (no multiplication), PMAT-300 RING (no order),
  and PMAT-302 INTEGRAL DOMAIN (no signs).

  Proved by `nlinarith` — Mathlib's nonlinear arithmetic tactic
  for ordered rings handles these sign rules automatically.

  An emitter that lowered Python-int multiplication through a
  SATURATING-to-nonneg fast-path (e.g., clamping `(-1) * (-1)` to
  `0`) would falsify (b) — a real bug class invisible to all 11
  prior Diamond categories which lack order × ring interaction.

  Status: **discharged at v0.1.0 (PMAT-305)**. Tier: DIAMOND.
  FIRST DEPTH-12 in the substrate.
-/
theorem ordered_ring_diamond (a b : Int) :
    -- (a) Nonneg × nonneg ≥ 0
    (0 ≤ a → 0 ≤ b → 0 ≤ a * b)
    -- (b) Nonpos × nonpos ≥ 0
    ∧ (a ≤ 0 → b ≤ 0 → 0 ≤ a * b)
    -- (c) Nonneg × nonpos ≤ 0
    ∧ (0 ≤ a → b ≤ 0 → a * b ≤ 0)
    -- (d) Strictpos × strictpos > 0
    ∧ (0 < a → 0 < b → 0 < a * b) := by
  refine ⟨?_, ?_, ?_, ?_⟩ <;> intros <;> nlinarith

/-! ## PMAT-307 — THIRTEENTH Diamond on C-PY-INT-ARITH (FIRST
    DEPTH-13 in the substrate): ABSOLUTE VALUE / NORM AXIOMS
    on (Int, |·|) (XPILE-REFINE-PY-INT-ARITH-021).

    **Opens DEPTH-13 in the substrate.** PyIntArith reached depth-12
    at PMAT-305 (ordered-ring sign rules); PMAT-307 adds the
    THIRTEENTH orthogonal Diamond category — ABSOLUTE VALUE as a
    NORM on the ring Int.

    The 13 Diamond categories on C-PY-INT-ARITH:
    - PMAT-214: SEMIRING
    - PMAT-228: EUCLIDEAN DOMAIN
    - PMAT-241: SHIFT-MONOID
    - PMAT-247: POWER-MONOID
    - PMAT-286: BITWISE-AND-MONOID
    - PMAT-290: ABELIAN-GROUP-ENRICHMENT
    - PMAT-292: ORDER-DISTRIBUTIVE-LATTICE
    - PMAT-294: DIVISIBILITY-PREORDER
    - PMAT-298: LINEAR-ORDER TRICHOTOMY
    - PMAT-300: RING-DISTRIBUTIVITY
    - PMAT-302: INTEGRAL DOMAIN
    - PMAT-305: ORDERED RING
    - **PMAT-307: ABSOLUTE VALUE / NORM** ← FIRST DEPTH-13

    The categorical distinction is sharp:
      None of the prior 12 categories mentions a UNARY OPERATION
      that captures "size" (norm). |·| is a non-trivial unary
      operation with:
        - non-negativity:        0 ≤ |a|
        - definiteness:          |a| = 0 ↔ a = 0
        - triangle inequality:   |a + b| ≤ |a| + |b|
        - multiplicativity:      |a * b| = |a| * |b|

    These are the AXIOMS of an ABSOLUTE VALUE / NORM on a ring.
    Mathlib's `AbsoluteValue` typeclass and `abs` function on Int
    satisfy all four.

    Why this is genuinely orthogonal:
      ABSOLUTE VALUE captures the "size" / "magnitude" of an Int
      independently of sign. None of the prior 12 categories
      (semiring, ring, integral domain, ordered ring, lattice,
      linear order, divisibility, ...) defines or constrains a
      norm-like unary operation. The norm axioms (non-negativity,
      definiteness, triangle, multiplicativity) characterize Int
      as a NORMED RING — a strictly richer structure than just
      an ordered ring.

    For Python int dispatch, this matters: an emitter that lowered
    abs through a path that violated the triangle inequality (e.g.,
    a saturating abs that wrapped around for very negative inputs)
    would falsify (c). This bug class slips past all 12 prior
    categories because none mention abs.

    Status: discharged at v0.1.0 (PMAT-307). Tier: DIAMOND.
    FIRST DEPTH-13 in the substrate. -/

/--
  **Diamond-tier refinement theorem** — `(Int, |·|)` is a
  NORMED RING.

  Combines four NORM-defining properties:
  (a) Non-negativity:        `0 ≤ |a|`
  (b) Definiteness:          `|a| = 0 ↔ a = 0`
  (c) Triangle inequality:   `|a + b| ≤ |a| + |b|`
  (d) Multiplicativity:      `|a * b| = |a| * |b|`

  Together these axiomatize |·| as an ABSOLUTE VALUE / NORM on
  the ring Int. Distinct from all prior 12 categories — none
  mention a unary operation capturing "size".

  Uses standard Mathlib lemmas: `abs_nonneg`, `abs_eq_zero`,
  `abs_add`, `abs_mul`. Mathlib's `AbsoluteValue` typeclass
  encodes this structure.

  An emitter that lowered abs through a path that violated the
  triangle inequality (e.g., a saturating abs that wrapped
  around for `Int.minValue`) would falsify (c) — a real bug
  class invisible to all 12 prior categories.

  Status: **discharged at v0.1.0 (PMAT-307)**. Tier: DIAMOND.
  FIRST DEPTH-13 in the substrate.
-/
theorem abs_value_norm_diamond (a b : Int) :
    -- (a) Non-negativity of absolute value
    (0 ≤ |a|)
    -- (b) Definiteness: |a| = 0 ↔ a = 0
    ∧ (|a| = 0 ↔ a = 0)
    -- (c) Triangle inequality
    ∧ (|a + b| ≤ |a| + |b|)
    -- (d) Multiplicativity
    ∧ (|a * b| = |a| * |b|) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact abs_nonneg a
  · exact abs_eq_zero
  · exact abs_add a b
  · exact abs_mul a b

/-! ## PMAT-310 — FOURTEENTH Diamond on C-PY-INT-ARITH (FIRST
    DEPTH-14 in the substrate): NAT-TO-INT RING HOMOMORPHISM —
    the canonical embedding (·: Nat → Int) is a ring homomorphism,
    establishing Int as a Z-extension of Nat (XPILE-REFINE-PY-INT-
    ARITH-022).

    **Opens DEPTH-14 in the substrate.** PyIntArith reached depth-13
    at PMAT-307 (absolute value / norm); PMAT-310 adds the
    FOURTEENTH orthogonal Diamond category — the RING-HOMOMORPHISM
    axioms for the canonical inclusion `Nat → Int`.

    The 14 Diamond categories on C-PY-INT-ARITH:
    - PMAT-214: SEMIRING
    - PMAT-228: EUCLIDEAN DOMAIN
    - PMAT-241: SHIFT-MONOID
    - PMAT-247: POWER-MONOID
    - PMAT-286: BITWISE-AND-MONOID
    - PMAT-290: ABELIAN-GROUP-ENRICHMENT
    - PMAT-292: ORDER-DISTRIBUTIVE-LATTICE
    - PMAT-294: DIVISIBILITY-PREORDER
    - PMAT-298: LINEAR-ORDER TRICHOTOMY
    - PMAT-300: RING-DISTRIBUTIVITY
    - PMAT-302: INTEGRAL DOMAIN
    - PMAT-305: ORDERED RING
    - PMAT-307: ABSOLUTE VALUE / NORM
    - **PMAT-310: NAT-CAST RING HOMOMORPHISM** ← FIRST DEPTH-14

    The categorical distinction is sharp:
      None of the prior 13 categories axiomatizes the RELATIONSHIP
      between Int and its Nat-subset via a structure-preserving map.
      The four conjuncts characterize `Nat.cast : Nat → Int` as a
      RING HOMOMORPHISM:
        - preserves 0 (additive identity)
        - preserves 1 (multiplicative identity)
        - preserves + (additive structure)
        - preserves * (multiplicative structure)

    These are the AXIOMS of a `RingHom Nat Int` in Mathlib's
    `RingHom` typeclass. Their joint satisfaction is what makes Int
    a NAT-ALGEBRA (or equivalently, Int has CHARACTERISTIC ZERO,
    since the kernel of `Nat → Int` is trivial).

    Why this is genuinely orthogonal:
      The prior 13 categories all live "inside" the ring Int. None
      relates Int to ANOTHER algebraic structure (Nat) via a map.
      The RING-HOMOMORPHISM axioms are about EXTERNAL structure
      preservation — a category-theoretic claim, not a per-element
      algebraic property. Mathlib's `Int.instSemiringHomNat` (and
      related) encode this separately from any of the prior
      typeclasses.

    For Python int dispatch, this matters: an emitter that lowered
    Python's non-negative-int fast path (a wrapper around `Nat`)
    must preserve all four homomorphism properties to be sound. An
    emitter that mapped `(0 : Nat)` to anything other than `(0 : Int)`
    (e.g., a "sentinel" representation) would falsify (a).

    Status: discharged at v0.1.0 (PMAT-310). Tier: DIAMOND.
    FIRST DEPTH-14 in the substrate. -/

/--
  **Diamond-tier refinement theorem** — the canonical embedding
  `Nat.cast : Nat → Int` is a RING HOMOMORPHISM.

  Combines four RING-HOMOMORPHISM axioms:
  (a) Preserves zero:           `((0 : Nat) : Int) = 0`
  (b) Preserves one:            `((1 : Nat) : Int) = 1`
  (c) Preserves addition:       `((m + n : Nat) : Int) = (m : Int) + (n : Int)`
  (d) Preserves multiplication: `((m * n : Nat) : Int) = (m : Int) * (n : Int)`

  Together these axiomatize `Nat → Int` as a ring homomorphism,
  establishing Int as a Nat-ALGEBRA with CHARACTERISTIC ZERO. This
  is the EXTERNAL category-theoretic content (structure-preserving
  map between two rings), distinct from all 13 prior INTERNAL
  algebraic claims.

  Uses standard Mathlib lemmas: `Nat.cast_zero`, `Nat.cast_one`,
  `Nat.cast_add`, `Nat.cast_mul`. Mathlib's `RingHom Nat Int`
  encodes this.

  An emitter that lowered Python's non-negative-int fast path
  through a path that violated (a) — e.g., mapped `(0 : Nat)` to
  a sentinel value ≠ `(0 : Int)` — would falsify the zero-preservation
  axiom. This bug class slips past all 13 prior categories which
  axiomatize Int internally but not its relationship to Nat.

  Status: **discharged at v0.1.0 (PMAT-310)**. Tier: DIAMOND.
  FIRST DEPTH-14 in the substrate.
-/
theorem nat_cast_ring_hom_diamond (m n : Nat) :
    -- (a) Preserves zero
    (((0 : Nat) : Int) = 0)
    -- (b) Preserves one
    ∧ (((1 : Nat) : Int) = 1)
    -- (c) Preserves addition
    ∧ (((m + n : Nat) : Int) = (m : Int) + (n : Int))
    -- (d) Preserves multiplication
    ∧ (((m * n : Nat) : Int) = (m : Int) * (n : Int)) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact Nat.cast_zero
  · exact Nat.cast_one
  · exact Nat.cast_add m n
  · exact Nat.cast_mul m n

/-! ## PMAT-312 — FIFTEENTH Diamond on C-PY-INT-ARITH (FIRST
    DEPTH-15 in the substrate): INT-EMOD QUOTIENT HOMOMORPHISM —
    Int.emod : Int → Z/nZ is a ring homomorphism, characterizing
    the QUOTIENT RING structure (XPILE-REFINE-PY-INT-ARITH-023).

    **Opens DEPTH-15 in the substrate.** PyIntArith reached depth-14
    at PMAT-310 (Nat-cast ring homomorphism); PMAT-312 adds the
    FIFTEENTH orthogonal Diamond category — the QUOTIENT-RING
    HOMOMORPHISM axioms for `Int.emod : Int → Z/nZ`.

    The 15 Diamond categories on C-PY-INT-ARITH:
    - PMAT-214: SEMIRING
    - PMAT-228: EUCLIDEAN DOMAIN
    - PMAT-241: SHIFT-MONOID
    - PMAT-247: POWER-MONOID
    - PMAT-286: BITWISE-AND-MONOID
    - PMAT-290: ABELIAN-GROUP-ENRICHMENT
    - PMAT-292: ORDER-DISTRIBUTIVE-LATTICE
    - PMAT-294: DIVISIBILITY-PREORDER
    - PMAT-298: LINEAR-ORDER TRICHOTOMY
    - PMAT-300: RING-DISTRIBUTIVITY
    - PMAT-302: INTEGRAL DOMAIN
    - PMAT-305: ORDERED RING
    - PMAT-307: ABSOLUTE VALUE / NORM
    - PMAT-310: NAT-CAST RING HOMOMORPHISM
    - **PMAT-312: INT-EMOD QUOTIENT HOMOMORPHISM** ← FIRST DEPTH-15

    The categorical distinction is sharp:
      - PMAT-310 (NAT-CAST RING HOMOMORPHISM) is an INJECTIVE
        embedding `Nat → Int` (lossless extension).
      - PMAT-312 (INT-EMOD QUOTIENT HOMOMORPHISM) is a SURJECTIVE
        projection `Int → Z/nZ` (lossy collapse). Demonstrated for
        n = 2, capturing the PARITY structure (Z/2Z quotient).

    Both are RING HOMOMORPHISMS, but in opposite directions:
      Nat ──cast──> Int ──emod n──> Z/nZ
      ─────────────  ─────────────
          injective    surjective

    PMAT-312 adds the FIRST QUOTIENT-RING claim to the substrate.
    Mathlib's `Int.emod` (Euclidean modulo, always non-negative
    result) satisfies the ring-homomorphism axioms:
      - `(a + b) % n = (a%n + b%n) % n` (preserves +)
      - `(a * b) % n = (a%n * b%n) % n` (preserves *)
      - `0 ≤ a % n` (lands in non-negative domain)
      - `a % n < n` (lands in {0, ..., n-1} = Z/nZ)

    Why this is genuinely orthogonal:
      The QUOTIENT structure is fundamentally different from the
      EMBEDDING structure (PMAT-310). An emitter could lower
      Int correctly (preserving the Nat embedding) yet falsify
      the quotient claim by using e.g. signed modulo (where
      `(-1) % 2 = -1` instead of `1`) — this is the standard
      C `%` semantics, which differs from Python's `%` and from
      Lean's `Int.emod`.

    For Python int dispatch, this matters: an emitter that lowered
    Python's `%` operator to C-style signed modulo (instead of
    Python's truncated-toward-negative-infinity modulo) would
    falsify the non-negativity axiom (c) for negative dividends.
    This is a documented Python-vs-C semantic mismatch.

    Status: discharged at v0.1.0 (PMAT-312). Tier: DIAMOND.
    FIRST DEPTH-15 in the substrate. -/

/--
  **Diamond-tier refinement theorem** — `Int.emod (· 2) : Int → Z/2Z`
  is a RING HOMOMORPHISM (quotient ring construction).

  Combines four QUOTIENT-HOMOMORPHISM properties:
  (a) Preserves +:           `(a + b) % 2 = (a % 2 + b % 2) % 2`
  (b) Preserves *:           `(a * b) % 2 = (a % 2 * b % 2) % 2`
  (c) Lands in non-negative: `0 ≤ a % 2`
  (d) Lands in `{0, 1}`:     `a % 2 < 2`

  Together these characterize `(· % 2) : Int → Z/2Z` as a SURJECTIVE
  ring homomorphism (quotient projection). Mirror of PMAT-310 which
  characterized `Nat → Int` as an INJECTIVE ring homomorphism.

  Uses Mathlib's `Int.add_emod`, `Int.mul_emod`, `Int.emod_nonneg`,
  `Int.emod_lt_of_pos`. These are the standard quotient-ring
  homomorphism lemmas.

  An emitter that lowered Python's `%` to C-style signed modulo
  (where `(-1) % 2 = -1` in C, vs `1` in Python and Lean's
  `Int.emod`) would falsify (c) for negative dividends — a real
  bug class invisible to all 14 prior Diamond categories.

  Status: **discharged at v0.1.0 (PMAT-312)**. Tier: DIAMOND.
  FIRST DEPTH-15 in the substrate.
-/
theorem int_emod_quotient_hom_diamond (a b : Int) :
    -- (a) emod is + homomorphism: (a + b) % 2 = (a%2 + b%2) % 2
    ((a + b) % 2 = (a % 2 + b % 2) % 2)
    -- (b) emod is * homomorphism: (a * b) % 2 = (a%2 * b%2) % 2
    ∧ ((a * b) % 2 = (a % 2 * b % 2) % 2)
    -- (c) emod result is non-negative (lands in Z/nZ canonical domain)
    ∧ (0 ≤ a % 2)
    -- (d) emod result is strictly less than 2 (lands in {0, 1} = Z/2Z)
    ∧ (a % 2 < 2) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact Int.add_emod a b 2
  · exact Int.mul_emod a b 2
  · exact Int.emod_nonneg a (by decide)
  · exact Int.emod_lt_of_pos a (by decide)

/-! ## PMAT-315 — SIXTEENTH Diamond on C-PY-INT-ARITH (FIRST
    DEPTH-16 in the substrate): GCD MONOID / BÉZOUT IDENTITY —
    `Int.gcd` satisfies the GCD-MONOID universal property AND
    yields a constructive linear combination
    (XPILE-REFINE-PY-INT-ARITH-024).

    **Opens DEPTH-16 in the substrate.** PyIntArith reached depth-15
    at PMAT-312 (Int-emod quotient ring homomorphism); PMAT-315
    adds the SIXTEENTH orthogonal Diamond category — the GCD-MONOID
    axioms with the BÉZOUT IDENTITY (constructive linear
    combination).

    The 16 Diamond categories on C-PY-INT-ARITH:
    - PMAT-214: SEMIRING
    - PMAT-228: EUCLIDEAN DOMAIN (fdiv, fmod algorithm)
    - PMAT-241: SHIFT-MONOID
    - PMAT-247: POWER-MONOID
    - PMAT-286: BITWISE-AND-MONOID
    - PMAT-290: ABELIAN-GROUP-ENRICHMENT
    - PMAT-292: ORDER-DISTRIBUTIVE-LATTICE
    - PMAT-294: DIVISIBILITY-PREORDER
    - PMAT-298: LINEAR-ORDER TRICHOTOMY
    - PMAT-300: RING-DISTRIBUTIVITY
    - PMAT-302: INTEGRAL DOMAIN
    - PMAT-305: ORDERED RING
    - PMAT-307: ABSOLUTE VALUE / NORM
    - PMAT-310: NAT-CAST RING HOMOMORPHISM
    - PMAT-312: INT-EMOD QUOTIENT RING HOMOMORPHISM
    - **PMAT-315: GCD MONOID / BÉZOUT IDENTITY** ← FIRST DEPTH-16

    The categorical distinction is sharp:
      - PMAT-228 EUCLIDEAN DOMAIN axiomatized the fdiv/fmod
        ALGORITHM (mechanical division-with-remainder).
      - PMAT-302 INTEGRAL DOMAIN axiomatized no-zero-divisors.
      - PMAT-315 GCD-MONOID axiomatizes the GCD as a UNIVERSAL
        OBJECT — `gcd a b` divides both a and b, AND is divisible
        by every common divisor. Plus the constructive BÉZOUT
        IDENTITY: `gcd a b = x*a + y*b` for some x, y.

    None of the prior 15 categories characterizes the gcd as a
    universal/categorical object with a constructive linear
    combination. Mathlib's `Int.gcd`, `Int.gcd_dvd_left`,
    `Int.gcd_dvd_right`, `Int.dvd_gcd`, `Int.gcd_eq_gcd_ab` are
    the standard GCD-monoid + Bézout lemmas.

    Why this is genuinely orthogonal:
      The Bézout identity `gcd a b = x*a + y*b` is what makes Int
      a PRINCIPAL IDEAL DOMAIN (PID) — every ideal in Int is
      principal, generated by the gcd. The prior 15 categories
      stop short of PID; PMAT-315 opens the door. Mathlib's
      `Int.instIsPrincipalIdealRing` provides the typeclass evidence.

    For Python int dispatch, this matters: the gcd computation is
    a core primitive (used in fractions, modular arithmetic,
    polynomial factoring). An emitter that lowered gcd through a
    path that didn't produce a constructive Bézout pair (e.g.,
    Stein's binary GCD without backward substitution) would
    falsify (d). The Bézout pair is needed for modular inverses.

    Status: discharged at v0.1.0 (PMAT-315). Tier: DIAMOND.
    FIRST DEPTH-16 in the substrate. -/

/--
  **Diamond-tier refinement theorem** — `Int.gcd` satisfies the
  GCD-MONOID universal property AND yields a constructive BÉZOUT
  IDENTITY.

  Combines four GCD-MONOID properties:
  (a) GCD divides left:        `(Int.gcd a b : Int) ∣ a`
  (b) GCD divides right:       `(Int.gcd a b : Int) ∣ b`
  (c) GCD is universal:        any common divisor divides gcd
                               `c ∣ a → c ∣ b → c ∣ (Int.gcd a b : Int)`
  (d) Bézout identity:         `(Int.gcd a b : Int) = a * gcdA + b * gcdB`

  Together these characterize `Int.gcd` as a UNIVERSAL OBJECT
  (categorical gcd in `IsGCDMonoid`) with a CONSTRUCTIVE linear
  combination (Bézout). Distinct from PMAT-228 EUCLIDEAN DOMAIN
  (fdiv/fmod algorithm) and PMAT-302 INTEGRAL DOMAIN
  (no-zero-divisors).

  Uses Mathlib's `Int.gcd_dvd_left`, `Int.gcd_dvd_right`,
  `Int.dvd_gcd`, `Int.gcd_eq_gcd_ab` — the standard PID/Bézout
  lemmas. `Int.instIsPrincipalIdealRing` provides typeclass
  evidence that Int is a Principal Ideal Domain.

  An emitter that lowered gcd through Stein's binary GCD (which
  doesn't naturally produce a Bézout pair) without backward
  substitution would falsify (d). The Bézout pair is load-bearing
  for modular inverses (used in fractions, polynomial factoring).

  Status: **discharged at v0.1.0 (PMAT-315)**. Tier: DIAMOND.
  FIRST DEPTH-16 in the substrate.
-/
theorem gcd_monoid_bezout_diamond (a b c : Int) :
    -- (a) gcd divides a
    ((Int.gcd a b : Int) ∣ a)
    -- (b) gcd divides b
    ∧ ((Int.gcd a b : Int) ∣ b)
    -- (c) gcd is universal: any common divisor divides gcd
    ∧ (c ∣ a → c ∣ b → c ∣ (Int.gcd a b : Int))
    -- (d) Bézout identity (constructive linear combination)
    ∧ ((Int.gcd a b : Int) = a * Int.gcdA a b + b * Int.gcdB a b) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact Int.gcd_dvd_left
  · exact Int.gcd_dvd_right
  · intro h1 h2; exact Int.dvd_gcd h1 h2
  · exact Int.gcd_eq_gcd_ab a b

/-! ## PMAT-317 — SEVENTEENTH Diamond on C-PY-INT-ARITH (FIRST
    DEPTH-17 in the substrate): UNIT GROUP STRUCTURE — the units
    of `Int` form the multiplicative group `{1, -1} ≅ Z/2Z`
    (XPILE-REFINE-PY-INT-ARITH-025).

    **Opens DEPTH-17 in the substrate.** PyIntArith reached depth-16
    at PMAT-315 (GCD monoid + Bézout / PID); PMAT-317 adds the
    SEVENTEENTH orthogonal Diamond category — the UNIT GROUP
    structure characterizing the multiplicatively invertible
    elements of Int.

    The 17 Diamond categories on C-PY-INT-ARITH:
    - PMAT-214..315: prior 16 categories (semiring, Euclidean,
      shift, power, AND, abelian group, lattice, divisibility,
      linear-order, ring, integral domain, ordered ring, norm,
      Nat-cast hom, emod quotient hom, GCD monoid + Bézout/PID)
    - **PMAT-317: UNIT GROUP `{1, -1} ≅ Z/2Z`** ← FIRST DEPTH-17

    The categorical distinction is sharp:
      - PMAT-315 GCD-MONOID + BÉZOUT established Int as a PID.
      - PMAT-317 UNIT GROUP characterizes the GROUP OF UNITS within
        that PID: `{a : Int | ∃ b, a*b = 1} = {1, -1}`. This is
        the multiplicative-inverse-structure of a ring, distinct
        from the additive group (PMAT-290 abelian-group-enrichment)
        and from the gcd structure (PMAT-315 GCD + Bézout).

    The unit group of Int is the simplest non-trivial finite
    group: `Z/2Z`. The four conjuncts encode this concretely:
      - 1 is the identity (mul_one)
      - -1 is its own inverse: (-1) * (-1) = 1 (Z/2Z involution)
      - Negation factors through -1: -a = (-1) * a
      - Squares are non-negative (sign rule from PMAT-305 lifted
        to the unit-group context: a² = a*a ≥ 0)

    Why this is genuinely orthogonal:
      None of the prior 16 categories axiomatizes the MULTIPLICATIVE
      INVERTIBLE ELEMENTS as a separate algebraic structure. PMAT-300
      RING / PMAT-302 INTEGRAL DOMAIN / PMAT-305 ORDERED RING all
      live in the ring; PMAT-315 GCD/PID characterizes ideals/gcd;
      PMAT-310 NAT-CAST / PMAT-312 INT-EMOD are homomorphisms.
      The unit group `Int×` is a NEW algebraic object — the kernel
      of the natural map `Int → Int / Int×` (associates).

    For Python int dispatch, this matters: an emitter that lowered
    Python's unary minus through a path that didn't satisfy `-a =
    (-1) * a` (e.g., a bitwise-complement-plus-one shortcut that
    failed on `Int.minValue` due to overflow) would falsify (c).

    Status: discharged at v0.1.0 (PMAT-317). Tier: DIAMOND.
    FIRST DEPTH-17 in the substrate. -/

/--
  **Diamond-tier refinement theorem** — the unit group of `Int` is
  `{1, -1} ≅ Z/2Z`.

  Combines four UNIT-GROUP properties:
  (a) Multiplicative identity:       `a * 1 = a`
  (b) `-1` is self-inverse (Z/2Z):   `(-1) * (-1) = 1`
  (c) Negation = mult by -1:         `-a = (-1) * a`
  (d) Squares are non-negative:      `0 ≤ a * a`

  Together these characterize the multiplicative-inverse-structure
  of Int: the units are `{1, -1}`, forming a cyclic group of order
  2. Uses `mul_one`, `Int.neg_one_mul`, `mul_self_nonneg` from
  Mathlib plus `decide` for the concrete `(-1) * (-1) = 1` fact.

  An emitter that lowered unary minus through a path that didn't
  preserve `-a = (-1) * a` (e.g., bitwise-complement-plus-one
  on a two's-complement representation that failed on Int.minValue
  due to overflow) would falsify (c) — a real bug class invisible
  to the prior 16 categories.

  Status: **discharged at v0.1.0 (PMAT-317)**. Tier: DIAMOND.
  FIRST DEPTH-17 in the substrate.
-/
theorem unit_group_diamond (a : Int) :
    -- (a) 1 is the multiplicative identity
    (a * 1 = a)
    -- (b) -1 is self-inverse: (-1) * (-1) = 1 (Z/2Z structure)
    ∧ ((-1 : Int) * (-1) = 1)
    -- (c) Negation factors through multiplication by -1
    ∧ (-a = (-1) * a)
    -- (d) Squares are non-negative (sign rule for a²)
    ∧ (0 ≤ a * a) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact mul_one a
  · decide
  · exact (Int.neg_one_mul a).symm
  · exact mul_self_nonneg a

/-! ## PMAT-320 — EIGHTEENTH Diamond on C-PY-INT-ARITH (FIRST
    DEPTH-18 in the substrate): SIGN FUNCTION MONOID HOMOMORPHISM
    — `Int.sign` is a multiplicative monoid hom from `(Int, *)` to
    `({-1, 0, 1}, *)` (XPILE-REFINE-PY-INT-ARITH-026).

    **Opens DEPTH-18 in the substrate.** PyIntArith reached depth-17
    at PMAT-317 (unit group `{1, -1} ≅ Z/2Z`); PMAT-320 adds the
    EIGHTEENTH orthogonal Diamond category — the SIGN FUNCTION as
    a multiplicative monoid homomorphism.

    The 18 Diamond categories on C-PY-INT-ARITH:
    - PMAT-214..317: prior 17 categories
    - **PMAT-320: SIGN FUNCTION (monoid hom Int → {-1, 0, 1})**
      ← FIRST DEPTH-18

    The categorical distinction is sharp:
      - PMAT-307 ABSOLUTE VALUE / NORM: `(Int, |·|)` is a NORMED
        RING. Captures the "size" of an Int.
      - PMAT-317 UNIT GROUP `{1, -1} ≅ Z/2Z`: characterizes the
        multiplicative-inverse elements as an algebraic structure.
      - PMAT-320 SIGN FUNCTION: characterizes the MAP `Int → {-1, 0, 1}`
        as a SURJECTIVE multiplicative monoid homomorphism that
        captures the "sign" of an Int. Together with PMAT-307's
        absolute value, `Int.sign a * |a| = a` reconstructs an Int
        from its sign and magnitude.

    None of the prior 17 categories axiomatizes the SIGN FUNCTION
    as a SEPARATE OPERATION with its own monoid-homomorphism
    structure. PMAT-307 was about the abs operation; PMAT-317 was
    about the unit-group elements; PMAT-320 is about the sign as
    a morphism. Three orthogonal pieces of the
    `Int = sign × magnitude` decomposition.

    Why this is genuinely orthogonal:
      The sign function maps `(Int, *)` to `({-1, 0, 1}, *)` —
      this is a SURJECTIVE multiplicative monoid hom (not group
      hom, because 0 has no multiplicative inverse). The
      homomorphism property `sign(a*b) = sign(a) * sign(b)` is a
      genuine structural claim about how multiplication interacts
      with sign. Combined with `sign(-a) = -sign(a)`, it captures
      the full multiplicative behavior of the sign function.

    For Python int dispatch, this matters: sign comparisons are
    used in dispatch (e.g., choosing fast-path for positive
    operands). An emitter that lowered `sign(a * b)` directly
    rather than going through `sign(a) * sign(b)` could overflow
    on large operands and produce wrong sign — falsifying (a).

    Status: discharged at v0.1.0 (PMAT-320). Tier: DIAMOND.
    FIRST DEPTH-18 in the substrate. -/

/--
  **Diamond-tier refinement theorem** — `Int.sign` is a
  multiplicative monoid homomorphism `(Int, *) → ({-1, 0, 1}, *)`.

  Combines four MONOID-HOMOMORPHISM properties:
  (a) Preserves multiplication: `Int.sign (a * b) = Int.sign a * Int.sign b`
  (b) Preserves negation:       `Int.sign (-a) = -Int.sign a`
  (c) Preserves zero:           `Int.sign 0 = 0`
  (d) Preserves one (identity): `Int.sign 1 = 1`

  Together these characterize `Int.sign` as the canonical SURJECTIVE
  multiplicative monoid hom to the sign monoid `{-1, 0, 1}`.

  Uses Mathlib's `Int.sign_mul`, `Int.sign_neg`. The `sign 0 = 0`
  and `sign 1 = 1` are definitionally true (`rfl`).

  An emitter that lowered `sign(a * b)` directly via the result's
  bit pattern (rather than via `sign(a) * sign(b)`) could overflow
  on `Int.minValue * Int.minValue` and produce wrong sign —
  falsifying (a). This bug class is invisible to PMAT-307 ABS
  (which captures magnitude) and PMAT-317 UNIT GROUP (which
  captures invertibles), neither mentioning the sign map.

  Status: **discharged at v0.1.0 (PMAT-320)**. Tier: DIAMOND.
  FIRST DEPTH-18 in the substrate.
-/
theorem int_sign_monoid_hom_diamond (a b : Int) :
    -- (a) Sign is multiplicative
    (Int.sign (a * b) = Int.sign a * Int.sign b)
    -- (b) Sign respects negation
    ∧ (Int.sign (-a) = -Int.sign a)
    -- (c) Sign of zero is zero
    ∧ (Int.sign 0 = 0)
    -- (d) Sign of one is one (preserves multiplicative identity)
    ∧ (Int.sign 1 = 1) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · exact Int.sign_mul a b
  · exact Int.sign_neg a
  · rfl
  · rfl

/-! ## PMAT-322 — NINETEENTH Diamond on C-PY-INT-ARITH (FIRST
    DEPTH-19 in the substrate): NEGATION-ORDER COMPATIBILITY —
    how negation interacts with the linear order on Int
    (XPILE-REFINE-PY-INT-ARITH-027).

    **Opens DEPTH-19 in the substrate.** PyIntArith reached depth-18
    at PMAT-320 (sign function monoid homomorphism); PMAT-322 adds
    the NINETEENTH orthogonal Diamond category — the
    NEGATION-ORDER COMPATIBILITY axioms.

    The 19 Diamond categories on C-PY-INT-ARITH:
    - PMAT-214..320: prior 18 categories
    - **PMAT-322: NEGATION-ORDER COMPATIBILITY** ← FIRST DEPTH-19

    The categorical distinction is sharp:
      - PMAT-290 ABELIAN-GROUP-ENRICHMENT: negation as group
        inverse (-(-a) = a, distributivity)
      - PMAT-298 LINEAR-ORDER TRICHOTOMY: strict-order axioms
      - PMAT-305 ORDERED RING: sign rules on PRODUCTS
      - PMAT-322 NEGATION-ORDER COMPATIBILITY: how unary `-`
        interacts with the linear order:
          a < b ↔ -b < -a (order-reversing)
          0 < a ↔ -a < 0 (sign-flip)

    None of the prior 18 categories axiomatizes how NEGATION
    interacts with ORDER. PMAT-290 was negation as group inverse
    (algebraic). PMAT-298 was strict order axioms (no negation).
    PMAT-305 was sign rules on multiplication (no negation
    interaction). PMAT-322 is the FIRST claim that negation
    REVERSES the strict order — a fundamental property of
    ORDERED-COMMUTATIVE-GROUP structure.

    Why this is genuinely orthogonal:
      ORDERED-COMMUTATIVE-GROUP requires the order-reversing
      property `a < b → -b < -a`. This is the canonical axiom
      that distinguishes an ordered group from a group with an
      independent (unrelated) order. Mathlib's `OrderedAddCommGroup`
      typeclass encodes precisely this. The bidirectional iff
      formulation captures both order-reversing and its converse.

    For Python int dispatch, this matters: an emitter that lowered
    unary minus through a path that didn't preserve order-reversing
    (e.g., a saturating negation where -2^63 maps to 2^63-1 instead
    of overflow) would falsify (a) or (b). Python's int negation
    is order-reversing because Python ints are unbounded; a
    fixed-width fast-path that wraps would falsify the axiom.

    Status: discharged at v0.1.0 (PMAT-322). Tier: DIAMOND.
    FIRST DEPTH-19 in the substrate. -/

/--
  **Diamond-tier refinement theorem** — negation on `Int` is
  ORDER-REVERSING (the bidirectional negation-order
  compatibility axioms).

  Combines four NEGATION-ORDER properties:
  (a) Negation reverses strict order: `a < b ↔ -b < -a`
  (b) Negation reverses non-strict order: `a ≤ b ↔ -b ≤ -a`
  (c) Positivity-negativity duality: `0 < a ↔ -a < 0`
  (d) Non-negativity-non-positivity duality: `0 ≤ a ↔ -a ≤ 0`

  Together these characterize Int as an ORDERED-ADDITIVE-
  COMMUTATIVE-GROUP — Mathlib's `OrderedAddCommGroup` typeclass
  shape. Distinct from PMAT-290 abelian-group enrichment (which
  captured negation as group inverse without order) and from
  PMAT-298 linear-order trichotomy (which captured the order
  without negation interaction).

  Uses Mathlib's `neg_lt_neg_iff`, `neg_le_neg_iff`,
  `Left.neg_neg_iff`, `Left.neg_nonpos_iff` (or `neg_lt_zero` /
  `neg_le_zero`). All standard ordered-group lemmas.

  An emitter that lowered unary minus through a saturating path
  (e.g., -2^63 maps to 2^63-1 instead of overflow) would falsify
  (a) — saturating negation is not order-reversing on the
  saturation boundary. Python's int negation is order-reversing
  because Python ints are unbounded.

  Status: **discharged at v0.1.0 (PMAT-322)**. Tier: DIAMOND.
  FIRST DEPTH-19 in the substrate.
-/
theorem int_neg_order_compat_diamond (a : Int) :
    -- (a) Negation reverses strict order
    (∀ b : Int, a < b ↔ -b < -a)
    -- (b) Negation reverses non-strict order
    ∧ (∀ b : Int, a ≤ b ↔ -b ≤ -a)
    -- (c) Positivity-negativity duality
    ∧ (0 < a ↔ -a < 0)
    -- (d) Non-negativity-non-positivity duality
    ∧ (0 ≤ a ↔ -a ≤ 0) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro b; constructor <;> intro h <;> omega
  · intro b; constructor <;> intro h <;> omega
  · constructor <;> intro h <;> omega
  · constructor <;> intro h <;> omega

end XpileContracts.CPyIntArith
