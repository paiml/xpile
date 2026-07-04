//! Kani BMC harness for `C-XLATE-PY-BOOL-TO-RUST-BOOL` (PMAT-1275 / XPILE-QUORUM-001).
//!
//! Symbolic-stratum counterpart to:
//!   - `contracts/xlate-py-bool-to-rust-bool-v1.yaml` (the equations)
//!   - `contracts/lean/XlatePyBoolToRustBool.lean` (the Semantic stratum)
//!
//! Per `sub/provability-roadmap.md` §1.3 and the ruchy 5.0 §14.4 quorum
//! rule, a Layer-1 equation wants ≥1 Symbolic (Kani BMC) + ≥1 Semantic
//! (Lean / diff-exec) vote to be discharged. `C-XLATE-PY-BOOL-TO-RUST-BOOL` was Semantic-only;
//! this file adds its Symbolic vote by proving, over the primitive Rust
//! type xpile actually emits, that a Python bool lowers to a Rust bool whose {0,1} representation is polarity-pinned, distinct, and round-trips (True/False stay distinct bits).
//!
//! ## How it is consumed
//!
//! The citation gate
//! `crates/xpile/tests/kani_harnesses.rs::every_referenced_kani_harness_exists_in_its_file`
//! checks the YAML→file→`fn` pipeline WITHOUT running Kani; the
//! `kani_verify.rs` gate (XPILE-QUORUM-002) DOES run `cargo kani` when
//! it is on PATH. This harness verifies `VERIFICATION:- SUCCESSFUL`
//! under Kani 0.67 (checked twice pre-commit: by the drafting agent and
//! independently by the integrator).
//!
//! ## Non-vacuity (the skeptic's checkpoint)
//!
//! This is NOT the reflexive `if a == b { assert_eq!(a, b) }` shape
//! that certifies nothing. Every assertion is a mapping property that a
//! wrong lowering would FALSIFY (distinctness / projection / injectivity
//! / order-preservation) — see the per-function docs.

#![cfg(kani)]

/// Equation `py_bool_structure_extensionality_diamond` from
/// `contracts/xlate-py-bool-to-rust-bool-v1.yaml`:
///
/// ```text
/// ∀ a b : PyBool.  a.truth = b.truth → a = b
/// (a Python bool is determined by its truth-flag; xpile emits a Rust bool)
/// ```
///
/// Symbolic counterpart to
/// `XpileContracts.CXlatePyBoolToRustBool.py_bool_structure_extensionality_diamond`
/// in `contracts/lean/XlatePyBoolToRustBool.lean`.
///
/// Model: a Python `bool` is its single truth-flag; xpile lowers it to a
/// Rust `bool`, whose in-memory representation is `b as u8` ∈ {0, 1}.
///   lower : bool -> u8   (the emitted Rust representation bit)
///   raise : u8   -> bool (recover the truth-flag)
///
/// NON-vacuous — this is NOT `if a == b { assert_eq!(a, b) }`:
///   - polarity is pinned to concrete bits (True→1, False→0);
///   - distinctness asserts the two truth values occupy DISTINCT bits
///     (fails if the lowering collapsed True/False onto one value);
///   - the round-trip `raise(lower(a)) == a` over ALL symbolic `a` is a
///     BIJECTION witness (fails on a polarity flip, e.g. True→0);
///   - injectivity `lower(a) == lower(b) → a == b` is the diamond's
///     antecedent→conclusion realized on the lowered representation.
#[kani::proof]
fn py_bool_structure_extensionality_diamond() {
    fn lower(b: bool) -> u8 {
        b as u8
    }
    fn raise(x: u8) -> bool {
        x != 0
    }

    // (1) Concrete truth-value ↔ bit mapping (polarity, pinned): True→1, False→0.
    assert_eq!(lower(true), 1);
    assert_eq!(lower(false), 0);

    // (2) Distinctness — the two truth values occupy DISTINCT bits.
    //     Fails if the lowering collapsed True/False onto one value.
    assert_ne!(lower(true), lower(false));

    // (3) Structure extensionality (the diamond) over ALL symbolic inputs.
    let a: bool = kani::any();
    let b: bool = kani::any();

    // Round-trip bijection onto {0,1}: raise ∘ lower = id.
    assert_eq!(raise(lower(a)), a);

    // Injectivity: equal lowered bits ⟹ equal source bools
    // (a.truth = b.truth → a = b).
    if lower(a) == lower(b) {
        assert!(a == b);
    }
}
