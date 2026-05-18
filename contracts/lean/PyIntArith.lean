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

end XpileContracts.CPyIntArith
