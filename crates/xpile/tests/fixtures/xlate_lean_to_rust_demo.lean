/-
  PMAT-124 — Runtime witness for the Lean → Rust translation.

  Provides a Runtime-stratum vote for:
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
