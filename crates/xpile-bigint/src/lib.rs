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
/// semantics — round toward negative infinity, not toward zero — so
/// generated code can stay on a single floor-div op across both modes.
/// PMAT-538: that i64 path is `checked_div` + a floor correction, NOT
/// `checked_div_euclid`; Euclidean division rounds toward negative
/// infinity only for a positive divisor, so it is not floor division.
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

/// Left shift `a << b` on `BigInt`. The exponent must be non-negative
/// and fit in `usize` (per `num-bigint`'s `Shl<usize>` impl). PMAT-026.
/// Matches Python `a << b` semantics: `b < 0` raises `ValueError`,
/// which we surface here as a panic naming the contract.
pub fn shl(a: &BigInt, b: &BigInt) -> BigInt {
    use num_traits::ToPrimitive;
    let n = b.to_usize().expect(
        "xpile: BigInt shift amount must fit in usize and be non-negative \
         (contract C-PY-INT-ARITH)",
    );
    a << n
}

/// Right shift `a >> b` on `BigInt`. Same constraints as [`shl`].
/// PMAT-026.
pub fn shr(a: &BigInt, b: &BigInt) -> BigInt {
    use num_traits::ToPrimitive;
    let n = b.to_usize().expect(
        "xpile: BigInt shift amount must fit in usize and be non-negative \
         (contract C-PY-INT-ARITH)",
    );
    a >> n
}

/// Power `a ** b` on `BigInt`. Exponent must fit in `u32` per
/// `num-bigint`'s `Pow<u32>` impl. Python `a ** b` with `b < 0`
/// returns `Float`, which v0.1.0's type lattice has no representation
/// for — surfaces here as a panic naming the contract. PMAT-026.
pub fn pow(a: &BigInt, b: &BigInt) -> BigInt {
    use num_traits::{Pow, ToPrimitive};
    let e: u32 = b.to_u32().expect(
        "xpile: BigInt exponent must fit in u32 and be non-negative \
         — Python returns Float for negative exponents which v0.1.0 \
         cannot represent (contract C-PY-INT-ARITH)",
    );
    a.clone().pow(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shl_doubles_repeatedly() {
        let a = BigInt::from(1);
        let b = BigInt::from(10);
        assert_eq!(shl(&a, &b), BigInt::from(1024));
    }

    #[test]
    fn shr_halves_repeatedly() {
        let a = BigInt::from(1024);
        let b = BigInt::from(10);
        assert_eq!(shr(&a, &b), BigInt::from(1));
    }

    #[test]
    fn pow_squares_and_cubes() {
        let two = BigInt::from(2);
        let three = BigInt::from(3);
        assert_eq!(pow(&two, &BigInt::from(10)), BigInt::from(1024));
        assert_eq!(pow(&three, &BigInt::from(5)), BigInt::from(243));
    }

    #[test]
    fn pow_handles_overflow_beyond_i64() {
        // 2^100 doesn't fit in i64; BigInt handles it natively.
        // 1267650600228229401496703205376 = 2^100
        let two = BigInt::from(2);
        let result = pow(&two, &BigInt::from(100));
        assert_eq!(result.to_string(), "1267650600228229401496703205376");
    }

    #[test]
    #[should_panic(expected = "xpile: BigInt shift amount")]
    fn shl_panics_on_negative_amount() {
        let a = BigInt::from(1);
        let b = BigInt::from(-1);
        let _ = shl(&a, &b);
    }
}
