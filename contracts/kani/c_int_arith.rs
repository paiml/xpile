//! Kani BMC harnesses for `C-C-INT-ARITH` (PMAT-958 / XPILE-QUORUM-001).
//!
//! This file is the **Symbolic stratum** counterpart to:
//!   - `contracts/c-int-arith-v1.yaml` (the equations)
//!   - `contracts/lean/CIntArith.lean` (the Semantic stratum's
//!     refinement-proof side)
//!
//! Per `sub/provability-roadmap.md` §1.3 (XPILE-QUORUM-001) and the
//! ruchy 5.0 §14.4 quorum rule, Layer-1 contract equations require at
//! least one Symbolic vote (Kani BMC) + one Semantic vote (Lean /
//! diff-exec) to be considered discharged. Until now `C-C-INT-ARITH`
//! was Semantic-only (CIntArith.lean + the C-path differential
//! transpile tests); this file adds its missing Symbolic vote.
//!
//! ## How this file is consumed
//!
//! It is NOT part of the workspace `cargo test --workspace` run —
//! `cargo kani` invokes `rustc` with the `kani` cfg flag and a
//! verifier backend that explores all inputs symbolically up to a
//! bound. The CI gate is
//! `crates/xpile/tests/kani_harnesses.rs::every_referenced_kani_harness_exists_in_its_file`,
//! which validates the citation pipeline (YAML → harness file → named
//! `#[kani::proof]` function) without actually running Kani. Running
//! Kani in CI is XPILE-QUORUM-002.
//!
//! ## Authoring conventions (mirrors `py_int_arith.rs`)
//!
//! - One `#[kani::proof]` function per equation in the contract YAML.
//! - Function name matches the YAML equation name (snake_case).
//! - Function body: `kani::any()` for inputs, `assert_eq!(...)` for the
//!   claim. `C-C-INT-ARITH`'s single equation has no `WHEN`
//!   precondition beyond the operand *type*, so no `kani::assume` is
//!   needed — the C `int` domain is captured by the `i32` type itself
//!   and the commutative-monoid laws hold over ALL of `i32`.
//!
//! ## ABI honesty: width is `i32`, not `i64`
//!
//! xpile lowers C `int` to Rust `i32` (32-bit two's-complement), and
//! the equation `c_int_wrapping_add_commutative_monoid_diamond` is
//! stated over `Int32` / `i32::wrapping_add` — deliberately distinct
//! from the Python-int lane's `i64` width in `py_int_arith.rs`. This
//! harness therefore uses `i32` to match the emitted ABI exactly.
//! The monoid laws (commutativity / associativity / left identity) are
//! structural `BitVec 32` identities that CBMC bit-blasts and discharges
//! directly — they need no overflow reasoning, so full `i32` width is
//! both ABI-honest AND BMC-tractable (unlike the `wrapping_mul` /
//! division laws punted to later sub-slices).

#![cfg(kani)]

/// Equation `c_int_wrapping_add_commutative_monoid_diamond` from
/// `contracts/c-int-arith-v1.yaml`:
///
/// ```text
/// ∀ a b c : Int32.
///   (wrapping_add(a, b) = wrapping_add(b, a))                 -- commutativity
/// ∧ (wrapping_add(wrapping_add(a, b), c)                      -- associativity
///      = wrapping_add(a, wrapping_add(b, c)))
/// ∧ (wrapping_add(0, a) = a)                                  -- left identity
/// (commutative monoid (Z/2^32, +, 0))
/// ```
///
/// This is the symbolic counterpart to
/// `XpileContracts.CCIntArith.c_int_wrapping_add_commutative_monoid_diamond`
/// in `contracts/lean/CIntArith.lean`, whose proof is
/// `⟨BitVec.add_comm, BitVec.add_assoc, BitVec.zero_add⟩`. Kani BMC
/// discharges the same three conjuncts over the concrete `i32`
/// (`BitVec 32`) two's-complement model that xpile emits for C `int +`.
///
/// The C `int` width is `i32` (ABI honesty — see the module header):
/// C signed-overflow UB is deliberately replaced by defined
/// two's-complement wraparound, so `wrapping_add` is total and no
/// `kani::assume` domain guard is required.
#[kani::proof]
fn c_int_wrapping_add_commutative_monoid_diamond() {
    let a: i32 = kani::any();
    let b: i32 = kani::any();
    let c: i32 = kani::any();

    // Commutativity: wrapping_add(a, b) == wrapping_add(b, a).
    assert_eq!(a.wrapping_add(b), b.wrapping_add(a));

    // Associativity: (a + b) + c == a + (b + c) over Z/2^32.
    assert_eq!(
        a.wrapping_add(b).wrapping_add(c),
        a.wrapping_add(b.wrapping_add(c))
    );

    // Left identity: 0 + a == a.
    assert_eq!(0i32.wrapping_add(a), a);
}

// ============================================================
// Exhaustive-at-i8 cross-checks. These realize the two inline
// `kani_harnesses:` bodies already documented in
// `contracts/c-int-arith-v1.yaml` (KANI-C-INT-ARITH-001/002). At i8
// width the (a, b) / (a, b, c) space is small enough for CBMC to
// enumerate exhaustively; the result lifts to the emitted `i32` width
// via the `BitVec` monoid theorem in CIntArith.lean. They are NOT the
// gate-wired harness (that is the ABI-honest `i32` proof above) — they
// are an independent, cheaper symbolic witness of the same laws.
// ============================================================

/// KANI-C-INT-ARITH-001: at i8 width (256² pairs, exhaustive),
/// `wrapping_add` is commutative. Lifts to the emitted `i32` width via
/// the Lean `BitVec` monoid theorem.
#[kani::proof]
fn c_int_wrapping_add_commutative_i8() {
    let a: i8 = kani::any();
    let b: i8 = kani::any();
    assert_eq!(a.wrapping_add(b), b.wrapping_add(a));
}

/// KANI-C-INT-ARITH-002: at i8 width, `0` is a left identity and
/// `wrapping_add` is associative — the remaining commutative-monoid
/// laws. Lifts to the emitted `i32` width via the Lean theorem.
#[kani::proof]
fn c_int_wrapping_add_monoid_i8() {
    let a: i8 = kani::any();
    let b: i8 = kani::any();
    let c: i8 = kani::any();
    assert_eq!(0i8.wrapping_add(a), a);
    assert_eq!(
        a.wrapping_add(b).wrapping_add(c),
        a.wrapping_add(b.wrapping_add(c))
    );
}
