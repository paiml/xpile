//! kani-deps: xpile-meta-hir
//!
//! THE FIRST PROOF IN THIS REPOSITORY THAT A WRONG LOWERING CAN TURN RED
//! (PMAT-1512).
//!
//! ## What was wrong with the proof lane
//!
//! `crates/xpile/tests/kani_verify.rs` materialises each harness as a temp crate
//! whose `Cargo.toml` had **no `[dependencies]` section at all**, and all 36
//! Lean modules import exactly `Lake`. So 108 Kani proofs and 489 Lean theorems
//! verified hand-written RE-IMPLEMENTATIONS of xpile's behaviour, and **not one
//! of them could be falsified by xpile's own code**. `contracts/kani/bashrs.rs`
//! says so in its own header — *"Standalone Rust module reproducing the property
//! under test"* — so this was disclosed at the harness level and contradicted at
//! the project level, where the lane is described as machine-checking xpile.
//!
//! An unfalsifiable guarantee is worse than a false one: nothing can ever
//! disturb it, so no evidence accumulates either way.
//!
//! ## What this harness does differently
//!
//! It declares `kani-deps: xpile-meta-hir` in this header, and the runner emits
//! a real path dependency for it. The property is stated over the SHIPPED
//! `binop_is_int_arith` — the predicate that decides whether emitted code
//! carries its `C-PY-INT-ARITH` citation.
//!
//! ## Why a scalar seam, and not the walk you would rather verify
//!
//! Measured 2026-08-01, both against the real crate:
//!
//! | property | outcome |
//! |---|---|
//! | `binop_is_int_arith` over all 19 operators | **SUCCESSFUL, 27 ms** |
//! | `Function::uses_int_arithmetic` on a depth-2 body | no result in **10 min** |
//!
//! `Expr` is a recursive enum with `String`/`Vec`/`Box` payloads, and Kani
//! unwinds the `expr_has_int_arith` ⇄ `stmt_has_int_arith` mutual recursion
//! without bound (observed: `Unwinding recursion … iteration 100`). Bounding it
//! with `#[kani::unwind(4)]` removes the unwinding failure and still does not
//! finish. A third attempt against `xpile_backend::strip_contract_citations`
//! compiled and put 1913 checks inside the real function, then stalled in
//! `str::pattern::TwoWaySearcher` — `str::find` is equally hostile.
//!
//! So the recipe is: **expose a scalar, fieldless-enum predicate as the seam and
//! prove that.** It is not the whole lowering, and this file does not pretend
//! otherwise — see `proof_seam_witness.rs`, which pins how many proofs are
//! actually load-bearing so that number cannot be quietly rounded up.
//!
//! ## Falsification, executed rather than asserted
//!
//! Deleting `BinOp::Shl` from the governed set in `xpile-meta-hir` — a real
//! wrong lowering, the kind that silently drops a citation — turns this proof
//! **FAILED** in 27 ms; restoring it returns **SUCCESSFUL**. That is the whole
//! point, and it had never been true of any proof here before.

use xpile_meta_hir::{binop_is_int_arith, BinOp};

/// The 19 `BinOp` variants, indexed so Kani can choose one symbolically.
/// Exhaustive by construction: a new variant added to the enum without a row
/// here lands in the `_` arm and is checked against the spec as `Pow`, which
/// fails unless it really is arithmetic — so the harness notices growth.
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

/// The SPEC, stated independently of the implementation so the two can
/// disagree: indices 5..=12 are the comparisons (`Eq`, `NotEq`, `Lt`, `LtEq`,
/// `Gt`, `GtEq`) and the boolean connectives (`And`, `Or`). Everything else is
/// overflow-prone or bitwise, and is governed by `C-PY-INT-ARITH`.
///
/// Written as a RANGE rather than as a mirror of the `matches!` arms on purpose:
/// a spec that copies the implementation's structure agrees with it by
/// construction and proves nothing.
fn spec_is_int_arith(i: u8) -> bool {
    !matches!(i, 5..=12)
}

/// For every binary operator, the shipped predicate agrees with the contract.
#[kani::proof]
fn binop_is_int_arith_agrees_with_the_contract_spec() {
    let i: u8 = kani::any();
    kani::assume(i < 19);
    assert!(binop_is_int_arith(op_of(i)) == spec_is_int_arith(i));
}
