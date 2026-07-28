// PMAT-124 / PMAT-1432 — shape demo (NOT a witness) for Rust fn → Lean theorem lifting.
//
// xpile-runtime-vote: none.
//
// NO test loads this file, so it casts NO Runtime-stratum vote
// (PMAT-1432). Until PMAT-1432 it did, because the fixture pass of
// `xpile quorum` counted any file that merely CONTAINED a contract
// ID. Wiring a test that loads this file by name is what turns it
// into evidence; on its own it documents a shape, nothing more.
//
// Shape documented, for:
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
