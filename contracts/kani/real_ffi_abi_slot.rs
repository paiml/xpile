//! kani-deps: xpile-ffi-manifest, xpile-meta-hir
//!
//! PMAT-2151: a proof over the SHIPPED `c_abi_type` and `wrapper_native` in
//! `xpile-ffi-manifest`, the two tables that decide which C ABI slot an FFI
//! boundary value rides and which Rust type the safe wrapper speaks. A wrong
//! row reinterprets or truncates a value at the boundary: `CUInt` on the
//! signed `c_int` slot turns 2^31 negative (PMAT-918), `CULong` on the 32-bit
//! `c_uint` slot truncates at 2^32 (PMAT-921), `F32` on `c_double` passes the
//! wrong width to a C `float` (PMAT-911).
//!
//! The spec is NOT a copy of the table. The C type each meta-HIR variant
//! stands for comes from its documentation in `xpile-meta-hir` (C `int`,
//! `long long`, `unsigned`, `unsigned long long`, `float`, `double`). The
//! width and signedness of each ABI slot and each native type come from `std`
//! (`size_of`, `MIN`), never from a string the table also contains.
//!
//! `CChar` is excluded from the width/sign property: it reaches a boundary
//! only as a pointer pointee and `c_char` signedness is platform-defined. The
//! proof checks only that it rides an 8-bit slot.
//!
//! ## Falsification, executed
//!
//! Mapping `Type::CUInt` to `c_int` in the shipped `c_abi_type` turns
//! `abi_slot_matches_the_c_type_and_the_wrapper` FAILED; so does mapping
//! `Type::CULong` to `c_uint`. Restoring the table returns SUCCESSFUL
//! (recorded in the PR for PMAT-2151).

use std::mem::size_of;
use std::os::raw::{c_char, c_double, c_float, c_int, c_longlong, c_uint, c_ulonglong};
use xpile_ffi_manifest::proof_seams::c_abi_type;
use xpile_ffi_manifest::wrapper_native;
use xpile_meta_hir::Type;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Signed,
    Unsigned,
    Float,
    Char,
}

fn int_kind(min_is_negative: bool) -> Kind {
    if min_is_negative {
        Kind::Signed
    } else {
        Kind::Unsigned
    }
}

/// Width in bits and kind of an ABI slot, from `std` alone.
fn abi_shape(slot: &str) -> Option<(usize, Kind)> {
    Some(match slot {
        "::std::os::raw::c_int" => (size_of::<c_int>() * 8, int_kind((c_int::MIN as i128) < 0)),
        "::std::os::raw::c_longlong" => (
            size_of::<c_longlong>() * 8,
            int_kind((c_longlong::MIN as i128) < 0),
        ),
        "::std::os::raw::c_uint" => (size_of::<c_uint>() * 8, int_kind((c_uint::MIN as i128) < 0)),
        "::std::os::raw::c_ulonglong" => (
            size_of::<c_ulonglong>() * 8,
            int_kind((c_ulonglong::MIN as i128) < 0),
        ),
        "::std::os::raw::c_double" => (size_of::<c_double>() * 8, Kind::Float),
        "::std::os::raw::c_float" => (size_of::<c_float>() * 8, Kind::Float),
        "::std::os::raw::c_char" => (size_of::<c_char>() * 8, Kind::Char),
        _ => return None,
    })
}

/// Width in bits and kind of the wrapper's native type, from `std` alone.
fn native_shape(ty: &str) -> Option<(usize, Kind)> {
    Some(match ty {
        "i64" => (size_of::<i64>() * 8, int_kind((i64::MIN as i128) < 0)),
        "u32" => (size_of::<u32>() * 8, int_kind((u32::MIN as i128) < 0)),
        "u64" => (size_of::<u64>() * 8, int_kind((u64::MIN as i128) < 0)),
        "f64" => (size_of::<f64>() * 8, Kind::Float),
        "f32" => (size_of::<f32>() * 8, Kind::Float),
        _ => return None,
    })
}

/// The C type each meta-HIR variant stands for (its `xpile-meta-hir` docs),
/// as (bits, kind). `None` = not a C scalar: must have no ABI slot.
fn intended_c(i: u8) -> (Type, Option<(usize, Kind)>) {
    match i {
        0 => (Type::I64, Some((32, Kind::Signed))), // C `int` (decy lowers it to I64)
        1 => (Type::Bool, Some((32, Kind::Signed))), // forward guard onto `int`
        2 => (Type::CLong, Some((64, Kind::Signed))), // `long long` / `int64_t`
        3 => (Type::CUInt, Some((32, Kind::Unsigned))), // `unsigned` / `uint32_t`
        4 => (Type::CULong, Some((64, Kind::Unsigned))), // `unsigned long long` / `uint64_t`
        5 => (Type::F32, Some((32, Kind::Float))),  // `float`
        6 => (Type::F64, Some((64, Kind::Float))),  // `double`
        7 => (Type::CChar, Some((8, Kind::Char))),  // `char`, pointee only
        8 => (Type::Str, None),
        9 => (Type::BigInt, None),
        10 => (Type::Unit, None),
        11 => (Type::ShellString, None),
        _ => (Type::ExitCode, None),
    }
}

/// Every variant rides the ABI slot of the C type it stands for, and the
/// wrapper's native type holds that slot's values without loss: same kind,
/// same width for floats and unsigned ints, at least as wide for signed ints.
///
/// The variants are enumerated concretely (all 13), not drawn with
/// `kani::any()`: a symbolic index makes every `&str` comparison in the two
/// shape tables a `memcmp` of unknown length, which CBMC cannot bound.
#[kani::proof]
#[kani::unwind(40)]
fn abi_slot_matches_the_c_type_and_the_wrapper() {
    for i in 0..=12u8 {
        check_variant(i);
    }
}

fn check_variant(i: u8) {
    let (ty, intended) = intended_c(i);
    let slot = c_abi_type(&ty);
    match intended {
        None => assert!(slot.is_none(), "a non-C-scalar got an ABI slot"),
        Some((bits, kind)) => {
            let got = abi_shape(slot.expect("a C scalar has an ABI slot"));
            assert!(got == Some((bits, kind)), "wrong ABI slot for the C type");
            if kind != Kind::Char {
                let (nbits, nkind) = native_shape(wrapper_native(&ty)).expect("known native");
                assert!(nkind == kind, "wrapper changes signedness or float-ness");
                match kind {
                    Kind::Signed => assert!(nbits >= bits, "wrapper narrows a signed slot"),
                    _ => assert!(nbits == bits, "wrapper changes an unsigned/float width"),
                }
            }
        }
    }
}
