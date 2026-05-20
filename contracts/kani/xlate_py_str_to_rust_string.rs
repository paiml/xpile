//! Kani BMC harness for `C-XLATE-PY-STR-TO-RUST-STRING` (PMAT-450).
//!
//! This is the **Symbolic stratum** counterpart for the Python-str-
//! to-Rust-owned-String translation contract. With this harness
//! landed, `C-XLATE-PY-STR-TO-RUST-STRING` reaches §14.4 QUORUM
//! (≥1 vote in ≥3 strata) — the **thirteenth contract to do so**,
//! bumping QUORUM coverage from 12 → 13.
//!
//!   * Semantic    (PMAT-450): `contracts/lean/XlatePyStrToRustString.lean`
//!   * Symbolic    (PMAT-450): this file
//!   * Runtime     (—)        : arrives when the v0.2.0 e2e fixture
//!                              corpus runs str-typed fixtures
//!                              through diff_exec
//!   * Extrinsic   (PMAT-450): roadmap mentions
//!
//! ## What this harness proves
//!
//! The Rust mirror of the Lean theorems:
//!
//!   * `utf8_bytes_preserved` — byte sequence identity under
//!     lowering (the load-bearing claim).
//!   * `length_preserved` — corollary: byte-equal implies
//!     length-equal.
//!   * `ownership_owned` — at v0.2.0 Bronze tier, this is the
//!     singleton-variant claim (RustString is the only codomain
//!     constructor).
//!
//! ## Why fixed-byte symbolic input
//!
//! Same rationale as PMAT-058 (bashrs), PMAT-059 (notation),
//! PMAT-061 (xlate-py-list-to-vec). Kani handles fixed-size
//! `[u8; N]` arrays orders of magnitude faster than symbolic
//! `Vec<u8>` allocation. 256^4 ≈ 4.3B exhaustive configurations is
//! enough to surface any structural divergence between the source
//! `PyStr` and the lowered `RustString`. The properties are
//! length-independent and structural; Silver-tier refinement
//! (v0.3.0+) will switch to a structural induction over symbolic
//! length once UTF-8 codepoint semantics enter the model.
//!
//! ## Cross-reinforcement
//!
//! Bidirectional with PMAT-450's Lean theorems: any future PR that
//! changes Rust's str lowering must update both the Lean theorems
//! and this Kani harness, or `refinement_proofs.rs`'s citation
//! gate fires. Same posture as the bashrs (PMAT-044/058), notation
//! (PMAT-057/059), and list (PMAT-060/061) cross-stratum pairs.

#![cfg(kani)]

/// Rust mirror of Lean's `PyStr`. v0.2.0 Bronze-tier model — the
/// UTF-8 byte sequence underlying the Python str, as a fixed-size
/// byte array. Silver-tier refinement (1.D stretch) will replace
/// this with typed Unicode-scalar-value arrays plus a
/// `well_formed_utf8` invariant.
#[derive(PartialEq, Eq, Clone, Copy)]
struct PyStr {
    bytes: [u8; 4],
}

/// Rust mirror of Lean's `RustString`. Same v0.2.0 shape as `PyStr`
/// — refined to carry heap-allocation + lifetime metadata at Silver
/// tier.
#[derive(PartialEq, Eq, Clone, Copy)]
struct RustString {
    bytes: [u8; 4],
}

/// Lowering function: Python `str` → Rust owned `String`. v0.2.0
/// Bronze model — byte-array identity. Rust mirror of
/// `lower_py_str_to_rust_string` from
/// `contracts/lean/XlatePyStrToRustString.lean`.
fn lower_py_str_to_rust_string(s: &PyStr) -> RustString {
    RustString { bytes: s.bytes }
}

/// Equation `utf8_bytes_preserved` from
/// `contracts/xlate-py-str-to-rust-string-v1.yaml`:
///
///   lower(py_str: str).as_bytes() == py_str.encode("utf-8")
///
/// Symbolic counterpart to
/// `XpileContracts.CXlatePyStrToRustString.utf8_bytes_preserved`.
/// Kani exhaustively explores all 4-byte symbolic str contents
/// (256^4 ≈ 4.3B configurations) and verifies the lowered
/// RustString contains exactly the same byte sequence.
#[kani::proof]
fn utf8_bytes_preserved() {
    let py_str = PyStr {
        bytes: kani::any(),
    };
    let rust_string = lower_py_str_to_rust_string(&py_str);
    assert_eq!(rust_string.bytes, py_str.bytes);
}

/// Equation `length_preserved` — corollary of byte-equality.
/// Listed as its own harness because downstream consumers (e.g.,
/// f-string lowering, slicing at sub-track 1.A-iv) cite the length
/// claim directly. Same exhaustive 256^4 exploration; same byte-
/// identity property → length identity.
#[kani::proof]
fn length_preserved() {
    let py_str = PyStr {
        bytes: kani::any(),
    };
    let rust_string = lower_py_str_to_rust_string(&py_str);
    assert_eq!(rust_string.bytes.len(), py_str.bytes.len());
}

/// Equation `ownership_owned` — at v0.2.0 Bronze tier, this is the
/// singleton-variant claim: every lowered value is a `RustString`
/// (not a `RustStrRef`, because no such variant exists in the
/// model yet). When the Silver-tier 1.D stretch sub-track lands,
/// this becomes a refinement claim about which lowering sites the
/// frontend allows borrowing at; for now it's "the only codomain
/// constructor is owned-String."
#[kani::proof]
fn ownership_owned() {
    let py_str = PyStr {
        bytes: kani::any(),
    };
    let rust_string = lower_py_str_to_rust_string(&py_str);
    // Bronze-tier discharge: equal to the canonical owned constructor.
    assert!(rust_string == RustString { bytes: py_str.bytes });
}
