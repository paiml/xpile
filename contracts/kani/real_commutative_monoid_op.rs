//! kani-deps: xpile-wasm-codegen, xpile-meta-hir
//!
//! PMAT-2151: a proof over the SHIPPED `is_commutative_monoid_op` in
//! `xpile-wasm-codegen`. It decides which `acc = acc OP e` folds may be
//! reordered; an operator wrongly admitted gives a loop whose answer depends on
//! iteration order, i.e. a silent wrong answer on the WASM lane.
//!
//! Two properties:
//!
//! 1. **Soundness, semantically.** Every admitted operator is commutative and
//!    associative, on symbolic 8-bit operands (wrapping, as the lane's integer
//!    arithmetic is; the laws are width-uniform) and on booleans for `and`/`or`.
//! 2. **The exact set**, pinned by index, so removing an operator also goes red.
//!
//! ## Falsification, executed
//!
//! Adding `BinOp::Sub` to the shipped set turns both proofs FAILED; restoring
//! it returns SUCCESSFUL (recorded in PR for PMAT-2151).

use xpile_meta_hir::BinOp;
use xpile_wasm_codegen::proof_seams::is_commutative_monoid_op;

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
/// `a OP b` at 8 bits; `and`/`or` on the operands' truthiness. `None` for an
/// operator with no evaluation here.
fn eval8(i: u8, a: u8, b: u8) -> Option<u8> {
    Some(match i {
        0 => a.wrapping_add(b),
        1 => a.wrapping_sub(b),
        2 => a.wrapping_mul(b),
        11 => ((a != 0) && (b != 0)) as u8,
        12 => ((a != 0) || (b != 0)) as u8,
        13 => a & b,
        14 => a | b,
        15 => a ^ b,
        16 => a.checked_shl(b as u32)?,
        17 => a.checked_shr(b as u32)?,
        _ => return None,
    })
}

/// Soundness: an admitted operator is commutative and associative.
#[kani::proof]
fn every_admitted_operator_is_commutative_and_associative() {
    let i: u8 = kani::any();
    kani::assume(i < 19);
    let (a, b, c): (u8, u8, u8) = (kani::any(), kani::any(), kani::any());
    if is_commutative_monoid_op(op_of(i)) {
        let e = |x, y| eval8(i, x, y).expect("admitted operator has an evaluation");
        assert!(e(a, b) == e(b, a), "not commutative");
        assert!(e(e(a, b), c) == e(a, e(b, c)), "not associative");
    }
}

/// The exact set: `+ * and or & | ^` and nothing else.
#[kani::proof]
fn the_monoid_set_is_exactly_add_mul_logic_and_bitwise() {
    let i: u8 = kani::any();
    kani::assume(i < 19);
    let spec = matches!(i, 0 | 2 | 11..=15);
    assert!(is_commutative_monoid_op(op_of(i)) == spec);
}
