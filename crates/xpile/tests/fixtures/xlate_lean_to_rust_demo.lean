/-
  PMAT-124 / PMAT-1432 — shape demo (NOT a witness) for the Lean → Rust translation.

  xpile-runtime-vote: none.

  NO test loads this file, so it casts NO Runtime-stratum vote
  (PMAT-1432). Until PMAT-1432 it did, because the fixture pass of
  `xpile quorum` counted any file that merely CONTAINED a contract
  ID. Wiring a test that loads this file by name is what turns it
  into evidence; on its own it documents a shape, nothing more.

  Shape documented, for:
    C-XLATE-LEAN-TO-RUST

  Small Lean 4 source designed to be parsed by a future
  lean-frontend (XPILE-LEAN-FRONTEND-001) when wired. The
  fixture exercises the body-preservation invariant the
  contract pins down: lowering `def f := body` to Rust `fn f
  { body }` must preserve the function body verbatim.

  lean-frontend doesn't exist as a crate at v0.1.0 — the only
  Lean-side surface is xpile-lean-contract-backend (proof-lane
  emit). The fixture is in place ahead of the lean-frontend
  wiring so the contract can move from 3-stratum QUORUM to full
  4-stratum. A dedicated round-trip test is
  XPILE-XLATE-LEAN-TO-RUST-RUNTIME-001 future work.
-/

def double (n : Int) : Int := n + n

def square (n : Int) : Int := n * n

def doubled_square (n : Int) : Int := square (double n)
