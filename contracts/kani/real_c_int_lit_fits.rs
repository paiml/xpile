//! kani-deps: xpile-rust-codegen
//!
//! PMAT-2151: a proof over the SHIPPED `c_int_lit_fits` in
//! `xpile-rust-codegen`, the predicate that decides whether a C integer
//! literal is emitted verbatim or converted modulo 2^N (PMAT-1399). A wrong
//! answer here makes `--target rust` emit either a literal `rustc` rejects
//! (`deny(overflowing_literals)`) or a conversion C would not perform.
//!
//! The spec is stated as numeric bounds, independent of the implementation's
//! `try_from` calls, so the two can disagree.
//!
//! ## Falsification, executed
//!
//! Changing the `"u32"` arm of `c_int_lit_fits` from `u32::try_from` to
//! `i32::try_from` turns this proof FAILED; restoring it returns SUCCESSFUL
//! (recorded in PR for PMAT-2151).

use xpile_rust_codegen::proof_seams::{c_int_lit_fits, CIntWidth};

fn width_of(i: u8) -> CIntWidth {
    match i {
        0 => CIntWidth::I32,
        1 => CIntWidth::I64,
        2 => CIntWidth::U32,
        3 => CIntWidth::U64,
        4 => CIntWidth::F32,
        _ => CIntWidth::F64,
    }
}

/// The C17 value ranges of each width, for an `i64` literal.
fn spec_fits(v: i64, i: u8) -> bool {
    match i {
        0 => (-2_147_483_648..=2_147_483_647).contains(&v),
        2 => (0..=4_294_967_295).contains(&v),
        3 => v >= 0,
        // i64 is the literal's own width; an int literal at a float width is
        // always representable.
        _ => true,
    }
}

/// For every `i64` literal and every width, the shipped predicate agrees with
/// the C range.
#[kani::proof]
fn c_int_lit_fits_agrees_with_the_c_ranges() {
    let v: i64 = kani::any();
    let i: u8 = kani::any();
    kani::assume(i < 6);
    assert!(c_int_lit_fits(v, width_of(i)) == spec_fits(v, i));
}
