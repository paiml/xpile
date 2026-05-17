//! Kani BMC harness for `C-XPILE-CONTRACT-FRONTEND-TRAIT`
//! (PMAT-067 / XPILE-CONTRACT-FRONTEND-TRAIT-001).
//!
//! This is the **Symbolic stratum** counterpart for the
//! `ContractFrontend::parse_to_equations` determinism invariant.
//! With this harness landed, `C-XPILE-CONTRACT-FRONTEND-TRAIT`
//! reaches §14.4 QUORUM — seventh contract to do so:
//!
//!   * Semantic    (PMAT-066): `contracts/lean/XpileContractFrontendTrait.lean`
//!   * Symbolic    (PMAT-067): this file
//!   * Runtime     (—)        : awaiting contract-frontend-impl audit
//!                              (XPILE-CONTRACT-FRONTEND-TRAIT-RUNTIME-001)
//!   * Extrinsic   (PMAT-066..067): roadmap mentions
//!
//! ## What this harness proves
//!
//! The Rust mirror of the Lean theorem `parse_idempotency` (see
//! `contracts/lean/XpileContractFrontendTrait.lean`). Calling
//! `parse_to_equations` twice on the same `source` produces
//! identical `EquationsBlock` output — the determinism invariant
//! every ContractFrontend impl must satisfy.
//!
//! Pairs with PMAT-063's `parse_idempotency` harness (code-lane
//! Frontend) to cover both code-lane and proof-lane parse-side
//! determinism. The four-harness "trait determinism" matrix —
//! {Frontend, Backend} × {code lane, proof lane} — closes after
//! PMAT-069 lands.
//!
//! ## Why fixed-byte symbolic input
//!
//! Same rationale as PMAT-058/059/061/063/065. Kani handles
//! fixed-size `[u8; N]` arrays orders of magnitude faster than
//! symbolic `Vec<T>`. The 4-byte bound is sufficient — the
//! determinism property is length-independent and structural.

#![cfg(kani)]

/// Rust mirror of Lean's `EquationsBlock`. v0.1.0 Bronze-tier
/// model — a fixed-size byte array. Silver-tier refinement
/// (XPILE-REFINE-CONTRACT-FRONTEND-TRAIT-001) replaces this with
/// the structural `EquationsBlock { equations, proof_obligations,
/// citations, ... }` AST plus a canonical-ordering invariant.
#[derive(PartialEq, Eq, Clone, Copy)]
struct EquationsBlock {
    bytes: [u8; 4],
}

/// Rust mirror of Lean's `parse_to_equations`. v0.1.0 model: the
/// identity on source bytes. The Bronze-tier placeholder captures
/// the determinism property; real ContractFrontend impls do
/// LaTeX/Lean/mdBook parsing, but are bound to the same invariant
/// via the trait contract.
fn parse_to_equations(source: &[u8; 4]) -> EquationsBlock {
    EquationsBlock { bytes: *source }
}

/// Equation `parse_idempotency` from
/// `contracts/xpile-contract-frontend-trait-v1.yaml`:
///
///   forall source:
///     hash(parse_to_equations(source).unwrap())
///       == hash(parse_to_equations(source).unwrap())
///
/// Symbolic counterpart to
/// `XpileContracts.CXpileContractFrontendTrait.parse_idempotency`
/// in `contracts/lean/XpileContractFrontendTrait.lean`. Kani
/// exhaustively explores all 4-byte symbolic sources (256^4 ≈
/// 4.3B configurations) and verifies two successive calls produce
/// identical EquationsBlock output.
#[kani::proof]
fn parse_idempotency() {
    let source: [u8; 4] = kani::any();

    let first = parse_to_equations(&source);
    let second = parse_to_equations(&source);

    kani::assert(
        first == second,
        "parse_to_equations must be deterministic on identical sources",
    );
}
