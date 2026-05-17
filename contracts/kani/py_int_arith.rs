//! Kani BMC harnesses for `C-PY-INT-ARITH` (PMAT-019 / XPILE-QUORUM-001).
//!
//! This file is the **Symbolic stratum** counterpart to:
//!   - `contracts/py-int-arith-v1.yaml` (the equations)
//!   - `contracts/lean/PyIntArith.lean` (the Semantic stratum's
//!     refinement-proof side)
//!
//! Per `sub/provability-roadmap.md` §1.3 (XPILE-QUORUM-001), Layer-1
//! contracts require at least one Symbolic vote + one Semantic vote
//! to be considered discharged. This file provides the Symbolic
//! votes; PyIntArith.lean + the diff_exec runtime check provide the
//! Semantic votes.
//!
//! ## How this file is consumed
//!
//! It is NOT part of the workspace `cargo test --workspace` run —
//! `cargo kani` invokes `rustc` with the `kani` cfg flag and a
//! verifier backend that explores all inputs symbolically up to a
//! bound. The CI gate is
//! `crates/xpile/tests/kani_harnesses.rs::every_kani_harness_exists_in_its_file`,
//! which validates the citation pipeline (YAML → harness file → named
//! `#[kani::proof]` function) without actually running Kani. Running
//! Kani in CI is XPILE-QUORUM-002.
//!
//! ## Authoring conventions
//!
//! - One `#[kani::proof]` function per equation in the contract YAML.
//! - Function name matches the YAML equation name (snake_case).
//! - Function body: `kani::any()` for inputs, `kani::assume(...)` for
//!   the equation's `WHEN <precondition>`, `kani::assert(...)` for
//!   the claim itself.
//! - The proof is mechanical for `addition_no_overflow` (Kani can
//!   discharge it in seconds via bit-blasted i64 arithmetic). For
//!   `multiplication_quadratic_promotion` and the bigint-promotion
//!   equations the BMC bound has to be tuned — punted to XPILE-QUORUM-002.

#![cfg(kani)]

/// Equation `addition_no_overflow` from `contracts/py-int-arith-v1.yaml`:
///
///     python_int_add(a, b) == rust_i64::wrapping_add(a, b)  WHEN fits_i64(a+b)
///
/// In Rust the "Python semantics" side is the mathematically-correct
/// `a + b` extended to i128 (which always fits and never wraps); the
/// "Rust wrapping" side is `a.wrapping_add(b)` (i64 mod 2^64 wrapping).
/// When `(a as i128 + b as i128)` lands in i64 range, both sides
/// agree.
///
/// This is the symbolic counterpart to `XpileContracts.CPyIntArith.fast_path_eq_slow_path`
/// in `contracts/lean/PyIntArith.lean` — Kani BMC discharges this
/// automatically; the Lean version still carries `sorry`
/// (XPILE-REFINE-002).
#[kani::proof]
fn addition_no_overflow() {
    let a: i64 = kani::any();
    let b: i64 = kani::any();

    let sum_i128: i128 = a as i128 + b as i128;
    kani::assume(sum_i128 >= i64::MIN as i128);
    kani::assume(sum_i128 <= i64::MAX as i128);

    let fast: i64 = a.wrapping_add(b);
    let slow: i64 = sum_i128 as i64; // safe by the assumes above

    assert_eq!(fast, slow);
}

/// Equation `subtraction_no_overflow` — mirror of the addition case.
/// Listed alongside even though `py-int-arith-v1.yaml` doesn't (yet)
/// have an explicit `subtraction_no_overflow:` equation — that
/// would be XPILE-CONTRACT-EXTEND-001. The harness ships here so the
/// codegen's `checked_sub` emission has symbolic backing once the
/// equation lands.
#[kani::proof]
fn subtraction_no_overflow() {
    let a: i64 = kani::any();
    let b: i64 = kani::any();

    let diff_i128: i128 = a as i128 - b as i128;
    kani::assume(diff_i128 >= i64::MIN as i128);
    kani::assume(diff_i128 <= i64::MAX as i128);

    let fast: i64 = a.wrapping_sub(b);
    let slow: i64 = diff_i128 as i64;

    assert_eq!(fast, slow);
}
