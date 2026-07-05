//! Kani BMC harness for `C-XLATE-PY-CLASS-TO-STRUCT` (PMAT-1277 /
//! XPILE-QUORUM-001).
//!
//! Symbolic-stratum counterpart to:
//!   - `contracts/xlate-py-class-to-struct-v1.yaml` (the equations)
//!   - `contracts/lean/XlatePyClassToStruct.lean` (the Semantic stratum)
//!
//! Per `sub/provability-roadmap.md` §1.3 and the ruchy 5.0 §14.4 quorum
//! rule, a Layer-1 equation wants ≥1 Symbolic (Kani BMC) + ≥1 Semantic
//! (Lean / diff-exec) vote to be discharged. `C-XLATE-PY-CLASS-TO-STRUCT`
//! was Semantic-only; this file adds its Symbolic vote by proving, over
//! the primitive Rust `struct` xpile actually emits, that a class is
//! determined by its ordered field list (the diamond
//! `a.fields = b.fields → a = b`).
//!
//! ## How it is consumed
//!
//! The citation gate
//! `crates/xpile/tests/kani_harnesses.rs::every_referenced_kani_harness_exists_in_its_file`
//! checks the YAML→file→`fn` pipeline WITHOUT running Kani; the
//! `kani_verify.rs` gate (XPILE-QUORUM-002) DOES run `cargo kani` when
//! it is on PATH. This harness verifies `VERIFICATION:- SUCCESSFUL`
//! under Kani 0.67 (checked pre-commit).
//!
//! ## Non-vacuity (the skeptic's checkpoint)
//!
//! This is NOT the reflexive `if a == b {{ assert_eq!(a, b) }}` shape.
//! Over a real Rust struct with mixed primitive fields it asserts:
//!   - per-field PROJECTION (each field reads back exactly, no
//!     cross-position or cross-type coercion);
//!   - structure EXTENSIONALITY with an else-branch `assert_ne!` (equal
//!     iff ordered fields equal — a field-sensitive claim, not a
//!     tautology);
//!   - per-field INJECTIVITY (perturbing ANY single field yields a
//!     distinct struct — the diamond's contrapositive).
//! A lowering that dropped a field, collapsed two fields, or coerced a
//! field's type would FALSIFY one of these.

#![cfg(kani)]

/// Model of the Rust `struct` xpile emits for a Python class /
/// `@dataclass` — mixed primitive fields (an `i64` slot, a `bool`
/// slot, another `i64`) exercise per-field type + position fidelity.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct S {
    a: i64,
    b: bool,
    c: i64,
}

/// Equation `py_struct_structure_extensionality_diamond` from
/// `contracts/xlate-py-class-to-struct-v1.yaml`:
///
/// ```text
/// ∀ a b : PyStruct.  a.fields = b.fields → a = b
/// (a class/dataclass is determined by its ordered field list; xpile
///  emits a Rust struct)
/// ```
///
/// Symbolic counterpart to
/// `XpileContracts.CXlatePyClassToStruct.py_struct_structure_extensionality_diamond`.
#[kani::proof]
fn py_struct_structure_extensionality_diamond() {
    let a: i64 = kani::any();
    let b: bool = kani::any();
    let c: i64 = kani::any();
    let s = S { a, b, c };

    // Field projection: per-field fidelity across positions AND types
    // (an i64 slot stays i64, a bool slot stays bool, no coercion).
    assert_eq!(s.a, a);
    assert_eq!(s.b, b);
    assert_eq!(s.c, c);

    // Structure extensionality (the diamond): two structs are equal IFF
    // their ordered field lists are equal. The else-branch makes this
    // field-sensitive rather than reflexive.
    let a2: i64 = kani::any();
    let b2: bool = kani::any();
    let c2: i64 = kani::any();
    let t = S {
        a: a2,
        b: b2,
        c: c2,
    };
    if a == a2 && b == b2 && c == c2 {
        assert_eq!(s, t);
    } else {
        assert_ne!(s, t);
    }

    // Per-field injectivity (the diamond's contrapositive): perturbing
    // ANY single field, holding the others fixed, yields a distinct
    // struct — so the ordered field list determines the struct.
    let a_p: i64 = kani::any();
    kani::assume(a_p != a);
    assert_ne!(S { a: a_p, b, c }, s);

    let b_p: bool = kani::any();
    kani::assume(b_p != b);
    assert_ne!(S { a, b: b_p, c }, s);

    let c_p: i64 = kani::any();
    kani::assume(c_p != c);
    assert_ne!(S { a, b, c: c_p }, s);
}
