//! kani-deps: xpile-wasm-codegen
//!
//! PMAT-2151: a proof over the SHIPPED `align8` in `xpile-wasm-codegen`, the
//! rounding that lays string literals out in linear memory (PMAT-994:
//! `align8(8 + byte_len)` bytes per literal). A wrong rounding makes the next
//! literal's base mis-aligned or overlap the previous one.
//!
//! Precondition: `0 <= n <= i32::MAX - 7`. Above that `n + 7` overflows `i32`;
//! a literal region that large cannot exist in a 32-bit linear memory, and
//! the proof states the bound rather than hiding it.
//!
//! ## Falsification, executed
//!
//! Changing `& !7` to `& !3` in the shipped `align8` turns this proof FAILED;
//! restoring it returns SUCCESSFUL (recorded in PR for PMAT-2151).

use xpile_wasm_codegen::proof_seams::align8;

/// The result is the least multiple of 8 that is `>= n`.
#[kani::proof]
fn align8_is_the_least_multiple_of_8_at_or_above_n() {
    let n: i32 = kani::any();
    kani::assume(n >= 0 && n <= i32::MAX - 7);
    let r = align8(n);
    assert!(r % 8 == 0);
    assert!(r >= n);
    assert!(r - n < 8);
}
