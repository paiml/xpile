//! Bigint runtime for xpile-generated code.
//!
//! Wraps [`num_bigint::BigInt`] as the slow-path side of the Layer-1
//! contract `C-PY-INT-ARITH` (see `contracts/py-int-arith-v1.yaml`).
//! When the frontend can prove that an arithmetic computation may
//! overflow `i64`, it lowers the function in *BigInt mode* and the
//! Rust/Ruchy backends emit `xpile_bigint::BigInt` instead of `i64` —
//! no `.checked_*().expect(...)` panics, because BigInt is unbounded
//! and matches Python `int` semantics exactly.
//!
//! Lean's `Int` is already unbounded, so the Lean backend treats
//! `Type::I64` and `Type::BigInt` identically.
//!
//! ## Why re-export rather than newtype
//!
//! Generated code is the most common consumer. A newtype would force
//! every emitted expression through `.0` or method calls; a re-export
//! lets `+`, `-`, `*`, `==`, `<` work directly on the type the user
//! sees in the generated source. Future extensions (Python-floor
//! division via `num_integer::Integer::div_floor`) will be exposed as
//! free helper functions on this module rather than method overrides.
//!
//! See also: [audit-design.md §6](../../../docs/specifications/audit-design.md)
//! for the five-whys → provable-contract pattern this runtime closes.

pub use num_bigint::BigInt;

/// Python-floor division (`a // b`) for `BigInt`. Matches the i64 path's
/// `checked_div_euclid` semantics — round toward negative infinity, not
/// toward zero — so generated code can stay on a single floor-div op
/// across both modes.
pub fn div_floor(a: &BigInt, b: &BigInt) -> BigInt {
    use num_integer::Integer;
    a.div_floor(b)
}

/// Python-floor modulo (`a % b`) for `BigInt`. Companion to
/// [`div_floor`]; result has the sign of `b`.
pub fn mod_floor(a: &BigInt, b: &BigInt) -> BigInt {
    use num_integer::Integer;
    a.mod_floor(b)
}
