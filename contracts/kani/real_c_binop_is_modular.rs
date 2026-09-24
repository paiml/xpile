//! kani-deps: xpile-rust-codegen, xpile-meta-hir
//!
//! PMAT-2151: a proof over the SHIPPED `c_binop_is_modular` in
//! `xpile-rust-codegen`. The C lane reduces an out-of-range literal modulo 2^N
//! BEFORE an operator only when that operator is modular (PMAT-1399); an
//! operator wrongly admitted here makes `--target rust` answer differently
//! from C (`5000000000u / 2`).
//!
//! Two properties:
//!
//! 1. **Soundness, semantically.** For every operator the shipped predicate
//!    admits, computing on operands already reduced mod 2^8 gives the same
//!    result mod 2^8 as computing first, on symbolic 16-bit operands. The
//!    property is width-uniform for these operators; 8/16 bits keep the
//!    multiplier small (PMAT-151: a symbolic 64-bit multiply hung Kani for
//!    105 min).
//! 2. **The exact set**, pinned by index, so removing an operator also goes
//!    red. `Shl` and `Pow` are excluded for the reasons the shipped doc gives
//!    (a shift COUNT, `checked_pow` panics), not because they fail (1).
//!
//! ## Falsification, executed
//!
//! Adding `BinOp::FloorDiv` to the shipped set turns both proofs FAILED
//! (the semantic one on a concrete counterexample); restoring it returns
//! SUCCESSFUL (recorded in PR for PMAT-2151).

use xpile_meta_hir::BinOp;
use xpile_rust_codegen::proof_seams::c_binop_is_modular;

/// The 19 `BinOp` variants, indexed so Kani can choose one symbolically. Same
/// table as `real_binop_governed_set.rs`: indices 0..=18 are Add, Sub, Mul,
/// FloorDiv, Mod, Eq, NotEq, Lt, LtEq, Gt, GtEq, And, Or, BitAnd, BitOr, BitXor,
/// Shl, Shr, Pow.
fn op_of(i: u8) -> BinOp {
    match i {
        0 => BinOp::Add,
        1 => BinOp::Sub,
        2 => BinOp::Mul,
        3 => BinOp::FloorDiv,
        4 => BinOp::Mod,
        5 => BinOp::Eq,
        6 => BinOp::NotEq,
        7 => BinOp::Lt,
        8 => BinOp::LtEq,
        9 => BinOp::Gt,
        10 => BinOp::GtEq,
        11 => BinOp::And,
        12 => BinOp::Or,
        13 => BinOp::BitAnd,
        14 => BinOp::BitOr,
        15 => BinOp::BitXor,
        16 => BinOp::Shl,
        17 => BinOp::Shr,
        _ => BinOp::Pow,
    }
}
/// `a OP b` at 16 bits, or `None` for an operator with no integer evaluation
/// here (comparisons, logic, shifts, pow). A `None` for an admitted operator
/// fails soundness: the predicate claims modularity nothing can check.
fn eval16(i: u8, a: u16, b: u16) -> Option<u16> {
    Some(match i {
        0 => a.wrapping_add(b),
        1 => a.wrapping_sub(b),
        2 => a.wrapping_mul(b),
        3 => a.checked_div(b)?,
        4 => a.checked_rem(b)?,
        13 => a & b,
        14 => a | b,
        15 => a ^ b,
        _ => return None,
    })
}

/// Soundness: an admitted operator commutes with reduction mod 2^8.
#[kani::proof]
fn every_admitted_operator_commutes_with_reduction() {
    let i: u8 = kani::any();
    kani::assume(i < 19);
    let a: u16 = kani::any();
    let b: u16 = kani::any();
    if c_binop_is_modular(op_of(i)) {
        let wide = eval16(i, a, b);
        let reduced = eval16(i, a as u8 as u16, b as u8 as u16);
        match (wide, reduced) {
            (Some(w), Some(r)) => assert!(w as u8 == r as u8),
            // Division by a zero that only appears after reduction is itself
            // a divergence.
            _ => assert!(false, "admitted operator has no modular evaluation"),
        }
    }
}

/// The exact set: `+ - * & | ^` and nothing else.
#[kani::proof]
fn the_modular_set_is_exactly_add_sub_mul_and_bitwise() {
    let i: u8 = kani::any();
    kani::assume(i < 19);
    let spec = matches!(i, 0..=2 | 13..=15);
    assert!(c_binop_is_modular(op_of(i)) == spec);
}
