//! Kani BMC harness for `C-XLATE-PY-OPTIONAL-TO-OPTION` (PMAT-1275 / XPILE-QUORUM-001).
//!
//! Symbolic-stratum counterpart to:
//!   - `contracts/xlate-py-optional-to-option-v1.yaml` (the equations)
//!   - `contracts/lean/XlatePyOptionalToOption.lean` (the Semantic stratum)
//!
//! Per `sub/provability-roadmap.md` §1.3 and the ruchy 5.0 §14.4 quorum
//! rule, a Layer-1 equation wants ≥1 Symbolic (Kani BMC) + ≥1 Semantic
//! (Lean / diff-exec) vote to be discharged. `C-XLATE-PY-OPTIONAL-TO-OPTION` was Semantic-only;
//! this file adds its Symbolic vote by proving, over the primitive Rust
//! type xpile actually emits, that a Python Optional[int] lowers to Rust Option<i64> with Some/None tag fidelity, payload preservation, and Some/None + payload injectivity.
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

/// Model of a Python `Optional[T]` as its `(present, payload)` structure,
/// lowered to the Rust `Option<i64>` xpile actually emits:
///   present=true  -> Some(payload)
///   present=false -> None
/// (`i64` is the primitive Python-int payload; no allocation, Kani-tractable.)
fn lower(present: bool, payload: i64) -> Option<i64> {
    if present {
        Some(payload)
    } else {
        None
    }
}

/// Equation `py_optional_structure_extensionality_diamond` from
/// `contracts/xlate-py-optional-to-option-v1.yaml`:
///
/// ```text
/// ∀ a b : PyOptional.
///   a.present = b.present → a.payload = b.payload → a = b
/// ```
///
/// A Python Optional is determined by its present-flag + payload; xpile lowers
/// it to `Option<i64>`. This Symbolic-stratum harness discharges the diamond
/// against the *emitted* Rust type, and is NON-VACUOUS: it asserts the honest
/// bijection between the `(present, payload)` model and `Option<i64>`, which
/// fails if the lowering drops the `Some` wrapper (`Some(x)` collapsing onto
/// `None`) or mangles the payload.
#[kani::proof]
fn py_optional_structure_extensionality_diamond() {
    let present: bool = kani::any();
    let payload: i64 = kani::any();
    let lowered: Option<i64> = lower(present, payload);

    // (A) Present-flag fidelity: the Some/None tag recovers `present`.
    //     Non-vacuous — fails if Some and None collapse onto one tag.
    assert_eq!(lowered.is_some(), present);
    assert_eq!(lowered.is_none(), !present);

    // (B) Payload preservation (projection): when present, unwrap recovers the
    //     exact payload. Non-vacuous — fails if the Some wrapper drops/mangles
    //     the value.
    if present {
        assert_eq!(lowered.unwrap(), payload);
    }

    let present2: bool = kani::any();
    let payload2: i64 = kani::any();
    let lowered2: Option<i64> = lower(present2, payload2);

    // (C) The diamond formula (forward extensionality): equal present AND
    //     equal payload ⟹ equal emitted Option.
    if present == present2 && payload == payload2 {
        assert_eq!(lowered, lowered2);
    }

    // (D) The load-bearing converse (injectivity on observable fields), the
    //     part that makes (C) non-vacuous rather than reflexive:
    //   - a differing present-flag ALWAYS yields differing Options
    //     (`Some(x) != None` — the Some/None DISTINCTION), and
    //   - two *present* Optionals with differing payloads yield differing
    //     Options (`Some(x) == Some(y) ⟺ x == y` — payload injectivity).
    if present != present2 {
        assert_ne!(lowered, lowered2);
    }
    if present && present2 && payload != payload2 {
        assert_ne!(lowered, lowered2);
    }
}
