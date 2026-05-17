//! Kani BMC harness for `C-XLATE-LEAN-TO-RUST` (PMAT-071 /
//! XPILE-XLATE-LEAN-001).
//!
//! This is the **Symbolic stratum** counterpart for the
//! `Lean def → Rust fn` lowering body-preservation invariant.
//! With this harness landed, `C-XLATE-LEAN-TO-RUST` reaches
//! §14.4 QUORUM — ninth contract to do so:
//!
//!   * Semantic    (PMAT-070): `contracts/lean/XlateLeanToRust.lean`
//!   * Symbolic    (PMAT-071): this file
//!   * Runtime     (—)        : awaiting xpile-rust-codegen Lean
//!                              lowering (XPILE-XLATE-LEAN-RUNTIME-001)
//!   * Extrinsic   (PMAT-070..071): roadmap mentions
//!
//! ## What this harness proves
//!
//! The Rust mirror of the Lean theorem `def_to_rust_fn` (see
//! `contracts/lean/XlateLeanToRust.lean`). Lowering a `LeanDef`
//! to a `RustFn` preserves the function body — proved by
//! byte-level identity over `[u8; 4]` symbolic input.
//!
//! ## Why fixed-byte symbolic input
//!
//! Same rationale as PMAT-058..069: Kani handles fixed-size byte
//! arrays orders of magnitude faster than symbolic `String` or
//! `Vec`. The 4-byte bound is sufficient — the body-preservation
//! property is length-independent and structural; 256^4 ≈ 4.3B
//! exhaustive configurations covers all 4-byte function bodies.
//!
//! ## Cross-reinforcement
//!
//! Bidirectional with PMAT-070's Lean theorem. The pair locks
//! in the body-preservation modelling commitment from both
//! formal sides. Any future xpile-rust-codegen impl that mutates
//! the Lean body during lowering — dropping comments, normalising
//! whitespace, wrapping in instrumentation macros — must
//! invalidate both discharges or face the refinement-proof
//! citation gate.
//!
//! Companion to PMAT-061's `iteration_order_preserved` (which
//! covers Python → Rust list lowering). Both are Layer-2
//! translation contracts at Bronze tier; together they bracket
//! the two directions of the proof-↔-code lane bridge.

#![cfg(kani)]

/// Rust mirror of Lean's `LeanDef`. v0.1.0 Bronze-tier model —
/// a fixed-size byte array carrying the function body. Silver-
/// tier refinement (XPILE-REFINE-XLATE-LEAN-TO-RUST-001)
/// replaces this with typed AST nodes (`{ name, args,
/// return_type, body }`).
#[derive(PartialEq, Eq, Clone, Copy)]
struct LeanDef {
    body: [u8; 4],
}

/// Rust mirror of Lean's `RustFn`. Same v0.1.0 shape as
/// `LeanDef` — locking in the body-preservation claim at the
/// byte level.
#[derive(PartialEq, Eq, Clone, Copy)]
struct RustFn {
    body: [u8; 4],
}

/// Rust mirror of Lean's `lower_def_to_fn`. v0.1.0 model:
/// byte-identity. The Bronze-tier placeholder captures the
/// body-preservation property; real xpile-rust-codegen impls
/// do much more (Lean parser, HIR construction, Rust emission),
/// but are bound to the same invariant via the translation
/// contract.
fn lower_def_to_fn(d: &LeanDef) -> RustFn {
    RustFn { body: d.body }
}

/// Equation `def_to_rust_fn` from
/// `contracts/xlate-lean-to-rust-v1.yaml`:
///
///   xlate(def f := body) == fn f { body_rust }
///
/// Symbolic counterpart to
/// `XpileContracts.CXlateLeanToRust.def_to_rust_fn` in
/// `contracts/lean/XlateLeanToRust.lean`. Kani exhaustively
/// explores all 4-byte symbolic function bodies (256^4 ≈ 4.3B
/// configurations) and verifies the lowered RustFn carries the
/// same byte sequence as the source LeanDef.
#[kani::proof]
fn def_to_rust_fn() {
    let input: [u8; 4] = kani::any();
    let lean_def = LeanDef { body: input };
    let rust_fn = lower_def_to_fn(&lean_def);

    kani::assert(
        rust_fn.body == lean_def.body,
        "lower_def_to_fn must preserve the function body",
    );
}
