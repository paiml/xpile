// PMAT-124 — Runtime witness for Rust fn → Lean theorem lifting.
//
// Provides a Runtime-stratum vote for:
//   C-XLATE-RUST-FN-TO-LEAN-THM
//
// Small Rust source designed to be lifted to a Lean theorem by a
// future rust-frontend → xpile-lean-contract-backend chain
// (XPILE-RUST-FRONTEND-001 + XPILE-LEAN-CONTRACT-BACKEND-EMIT-001).
// The fixture exercises the body-preservation invariant the
// contract pins down: lifting `fn f -> R { body }` to a Lean
// `def f : R := body` must preserve the function body verbatim.
// The bidirectional partner direction (Lean → Rust) ships its
// own dedicated fixture and is at full §14.4 QUORUM in tandem
// as of PMAT-072/073.
//
// rust-frontend doesn't exist as a crate at v0.1.0. The fixture
// is in place ahead of the rust-frontend wiring so the contract
// can move from 3-stratum QUORUM to full 4-stratum. A dedicated
// round-trip test asserting byte-identical Lean output is
// XPILE-XLATE-RUST-TO-LEAN-THM-RUNTIME-001 future work.

pub fn double(n: i64) -> i64 {
    n + n
}

pub fn square(n: i64) -> i64 {
    n * n
}

pub fn doubled_square(n: i64) -> i64 {
    square(double(n))
}
