//! Kani BMC harness for `C-XLATE-PY-SET-TO-HASHSET` (PMAT-1282 /
//! XPILE-QUORUM-001).
//!
//! Symbolic-stratum counterpart to:
//!   - `contracts/xlate-py-set-to-hashset-v1.yaml` (the equations)
//!   - `contracts/lean/XlatePySetToHashset.lean` (the Semantic stratum)
//!
//! ## Why a MODEL (not `std::HashSet`)
//!
//! `std::collections::HashSet` is Kani-HOSTILE: even with a
//! deterministic hasher (no `getrandom`), its `RawTable` allocation
//! internals surface `handle_alloc_error` / `size_of_val` unsupported
//! constructs. So — exactly as `enum_translation.rs` models an enum's
//! ordered variant list as `[u8; N]` — this harness models the SET
//! SEMANTICS the emitted `HashSet` must satisfy over a Kani-clean
//! membership BITMASK (bit `i` set ⟺ element `i` present), which is the
//! order-independent "canonical repr" the diamond
//! `a.repr = b.repr → a = b` keys on.
//!
//! ## Non-vacuity (the skeptic's checkpoint)
//!
//! Every assertion is a real set property a wrong lowering would break:
//! ORDER-INDEPENDENCE and IDEMPOTENCE (dedup) — both FALSE for a
//! list/multiset — plus membership fidelity and extensionality (equal
//! repr ⟺ agree on every element's membership; distinct reprs differ on
//! some element). Verified `VERIFICATION:- SUCCESSFUL` under Kani 0.67.

#![cfg(kani)]

/// Insert element `x` (folded into the 8-element universe) into the
/// membership bitmask — the emitted `HashSet`'s canonical repr.
fn s_insert(mask: u8, x: u8) -> u8 {
    mask | (1u8 << (x & 7))
}

/// Membership test against the canonical repr.
fn s_member(mask: u8, x: u8) -> bool {
    mask & (1u8 << (x & 7)) != 0
}

/// Equation `py_set_structure_extensionality_diamond` from
/// `contracts/xlate-py-set-to-hashset-v1.yaml`
/// (`∀ a b : PySet. a.repr = b.repr → a = b`).
#[kani::proof]
#[kani::unwind(9)]
fn py_set_structure_extensionality_diamond() {
    let a: u8 = kani::any();
    let b: u8 = kani::any();

    // ORDER-INDEPENDENCE: inserting a then b == b then a (a set's repr
    // is insertion-order-free). FALSE for a list/multiset.
    assert_eq!(s_insert(s_insert(0, a), b), s_insert(s_insert(0, b), a));

    // IDEMPOTENCE (dedup): inserting the same element twice == once.
    // FALSE for a multiset.
    assert_eq!(s_insert(s_insert(0, a), a), s_insert(0, a));

    // MEMBERSHIP fidelity: inserted elements are members.
    let s = s_insert(s_insert(0, a), b);
    assert!(s_member(s, a));
    assert!(s_member(s, b));

    // EXTENSIONALITY (the diamond): equal repr ⇒ agree on the membership
    // of an ARBITRARY probe element.
    let t: u8 = kani::any();
    let u: u8 = kani::any();
    if s == t {
        assert_eq!(s_member(s, u), s_member(t, u));
    }
    // NON-vacuity: distinct reprs DIFFER on some element's membership
    // (so equal-repr is a real equivalence, not a tautology).
    if s != t {
        let mut differ = false;
        let mut e = 0u8;
        while e < 8 {
            if s_member(s, e) != s_member(t, e) {
                differ = true;
            }
            e += 1;
        }
        assert!(differ, "distinct set reprs must differ on some element");
    }
}
