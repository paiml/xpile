//! Kani BMC harness for `C-OLS-MODEL-UNIQUENESS` (PMAT-959).
//!
//! This file is the **Symbolic stratum** counterpart to:
//!   - `contracts/ols-model-uniqueness-v1.yaml` (the equations)
//!   - `contracts/lean/OlsModelUniqueness.lean` (the core-Lean
//!     structural Diamond — `linear_model_structure_extensionality_diamond`)
//!   - `contracts/lean-models/Models/GeneralLinear.lean` (the
//!     Mathlib-backed DEEP uniqueness certificate — `XpileModels.ols_unique`
//!     / `ols_strict`).
//!
//! ## Scope — READ THIS (acceptance-honesty)
//!
//! The DEEP theorem this contract certifies — that a full-column-rank
//! least-squares fit is THE unique minimiser of the sum of squared
//! errors — is a statement about REAL-valued coefficient vectors and
//! is NOT Kani-expressible (CBMC has no reals; the Mathlib
//! `ols_unique` proof in the walled-off `lean-models` lane carries
//! that content).
//!
//! What Kani CAN discharge is the tractable, integer, COMPUTABLE
//! NON-DEGENERACY that OLS uniqueness RESTS ON. For a simple
//! (1-feature) least-squares fit the slope is
//!
//!     β = Σᵢ (xᵢ − x̄)(yᵢ − ȳ) / Σᵢ (xᵢ − x̄)²
//!
//! and it is uniquely defined exactly when the denominator
//! `D = Σᵢ (xᵢ − x̄)²` is NONZERO — i.e. when the x-values are not all
//! identical (equivalently: the single feature column has full rank).
//! This harness proves `D > 0` under that non-degeneracy hypothesis.
//! It is a SURROGATE for the full uniqueness theorem, NOT the theorem
//! itself: it establishes that the normal equations have a unique
//! solution (the coefficient is well-defined) whenever at least two
//! x-values differ. Do not read this harness as proving `ols_unique`.
//!
//! ## Integer-scaling trick (avoids rationals so CBMC stays exact)
//!
//! `x̄ = (Σx)/N` is a rational, so `D = Σ(xᵢ − x̄)²` is not an integer.
//! Scale every term by `N²`:
//!
//!     N²·D = Σ (N·xᵢ − N·x̄)² = Σ (N·xᵢ − Σx)²  =:  S
//!
//! `S` is a pure-integer quantity, and since `N² > 0` we have
//! `S > 0  ⟺  D > 0`. The harness proves `S > 0`, which is equivalent
//! to (and avoids the fractions of) the real denominator claim.
//!
//! Why `S > 0` holds: `S` is a sum of squares, so `S ≥ 0`, and `S = 0`
//! iff every `N·xᵢ − Σx = 0`, i.e. `xᵢ = Σx/N = x̄` for ALL i, i.e. all
//! `xᵢ` are equal. The `kani::assume` that at least two differ excludes
//! exactly that case, so `S > 0` strictly.
//!
//! ## How this file is consumed
//!
//! Like `py_int_arith.rs`, this file is NOT part of `cargo test
//! --workspace`; `cargo kani` compiles it under the `kani` cfg and
//! explores all inputs symbolically. The workspace-side CI gate is
//! `crates/xpile/tests/kani_harnesses.rs::every_referenced_kani_harness_exists_in_its_file`,
//! which validates the citation pipeline (YAML → harness file → named
//! `#[kani::proof]`) WITHOUT running Kani. Running Kani in CI is the
//! `kani` job (XPILE-QUORUM-002).

#![cfg(kani)]

/// Equation `simple_ols_denominator_positive` from
/// `contracts/ols-model-uniqueness-v1.yaml`.
///
/// For a simple (1-feature) least-squares fit over a fixed-size array
/// of `i64` x-values, if at least two x-values differ then the scaled
/// denominator `S = Σ (N·xᵢ − Σx)² = N²·D` is strictly positive, so
/// the OLS slope `β = Σ(xᵢ−x̄)(yᵢ−ȳ) / D` is uniquely defined (the
/// normal equations have a unique solution).
///
/// This is the Symbolic-stratum SURROGATE for the deep real-valued
/// uniqueness theorem `XpileModels.ols_unique` in the Mathlib lane
/// (`contracts/lean-models/Models/GeneralLinear.lean`): it discharges
/// the COMPUTABLE non-degeneracy (denominator positivity ⇒ unique
/// slope), not the full uniqueness content.
///
/// `N = 3` and the operand range `[-100, 100]` are kept small so BMC
/// is tractable and — importantly — so the property is about the MATH,
/// not about i64 overflow: with `|xᵢ| ≤ 100` and `N = 3`,
/// `|N·xᵢ − Σx| ≤ 600`, each square `≤ 360_000`, and `S ≤ 1_080_000`,
/// far inside i64. The `assume` bounds guarantee no intermediate
/// overflow, so the assertion tests the denominator identity itself.
#[kani::proof]
fn simple_ols_denominator_positive() {
    const N: i64 = 3;

    let x0: i64 = kani::any();
    let x1: i64 = kani::any();
    let x2: i64 = kani::any();

    // Small range keeps BMC tractable AND guarantees every product /
    // sum below stays well within i64 — the property is about the
    // math, not about overflow (see the ≤ 1_080_000 bound above).
    kani::assume(x0 >= -100 && x0 <= 100);
    kani::assume(x1 >= -100 && x1 <= 100);
    kani::assume(x2 >= -100 && x2 <= 100);

    // Non-degeneracy hypothesis: the x-values are NOT all identical
    // (at least one pair differs). This is the full-column-rank /
    // identifiability condition for the single feature column.
    kani::assume(!(x0 == x1 && x1 == x2));

    let sum: i64 = x0 + x1 + x2; // = Σx

    // Scaled deviations N·xᵢ − Σx (integer; equals N·(xᵢ − x̄)).
    let t0: i64 = N * x0 - sum;
    let t1: i64 = N * x1 - sum;
    let t2: i64 = N * x2 - sum;

    // S = Σ (N·xᵢ − Σx)² = N²·D, the integer-scaled denominator.
    let s: i64 = t0 * t0 + t1 * t1 + t2 * t2;

    // The load-bearing non-degeneracy claim: with at least two x-values
    // differing, the (scaled) least-squares denominator is strictly
    // positive, so the OLS slope is uniquely defined.
    assert!(s > 0);
}
