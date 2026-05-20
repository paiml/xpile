# Two lanes, one substrate

xpile has two parallel pipelines that share the YAML contract substrate.
This is the single most important mental model in the system.

```text
Frontends                      Backends
─────────                      ─────────
Python   ─┐               ┌─→ Rust        ✅ real emission
C        ─┤               ├─→ Ruchy       ✅ real emission
Shell    ─┤               ├─→ Shell       ✅ real emission (POSIX)
C++      ─┼→ meta-HIR ─→ ─┼─→ PTX         🚧 scaffold + Layer-5 contract
Rust     ─┤               ├─→ WGSL        🚧 scaffold
Ruchy    ─┤               ├─→ SPIR-V      🚧 planned
Lean 4   ─┘               └─→ Lean 4      🚧 scaffold
```

That is the **code lane** — runnable code in, runnable code out.

```text
ContractFrontends             ContractBackends
─────────────────             ─────────────────
LaTeX       ─┐                  ┌─→ LaTeX (papers)
Lean 4 thm  ─┼─→ contracts ←──←─┼─→ Lean 4 theorems
mdBook      ─┘                  └─→ mdBook
```

That is the **proof lane** — notation and proofs in, notation and proofs
out, both sides talking to the *same* contract YAML.

## Why two lanes?

The conventional answer for "how do I prove a transpile is correct?"
involves either:

1. **A handwritten paper**: prose argument that the transpile preserves
   semantics. Convincing to a reader, opaque to a machine.
2. **A whole-system mechanization**: every transpile path encoded as a
   theorem in a single proof assistant. Convincing to a machine,
   exhausting to a maintainer.

xpile takes a middle path: contracts in YAML are the **shared
substrate**. Each contract has one fact ("Python `int` overflow is
unbounded; an i64 codomain requires bigint promotion to discharge it"),
expressed three ways:

- **Code lane**: the Rust backend emits `checked_mul().expect(...)` so
  every overflow becomes a panic, not silent wrapping.
- **Proof lane**: a Lean 4 theorem `pyIntArithRefinement` proves the
  refinement of `ℤ` → `Option Int64` at the structural level.
- **Audit lane (extrinsic)**: a Kani BMC harness exhaustively explores
  256⁴ ≈ 4.3B configurations checking the invariant.

When all three voices agree, the contract is at **quorum** — the
mechanically-checked equivalent of "consensus across independent
oracles."

## Lean 4 spans both lanes

Lean 4 is special: it's both a programming language (so it appears in
the code lane as a backend) **and** a proof assistant (so it appears in
the proof lane as the canonical theorem-bearing format). LaTeX is
proof-lane-only.

The citation bridge between the two lanes uses **format-native
structured constructs**, never regex over body text:

- In Lean: `@[xpile_contract "C-PY-INT-ARITH"]` attribute.
- In LaTeX: `\xpileContract{C-PY-INT-ARITH}{Python int arithmetic}`.
- In mdBook: a structured HTML comment.

This is *the* design decision that makes the proof lane robust against
edit churn — see the
[`C-XPILE-CONTRACT-BACKEND-TRAIT`](../reference/contracts.md#c-xpile-contract-backend-trait)
contract for the formal statement.

## What flows through meta-HIR

The middle box in the code-lane diagram is **meta-HIR** — a canonical
intermediate representation that every frontend lowers into and every
backend lowers from. It is intentionally minimal and includes:

- Function signatures with typed parameters and return types
- All binary + unary operators (Python semantics, not C semantics — so
  `//` is floor-division, not truncating-division)
- Function calls including self-recursion
- A `kind: kernel` vs `kind: pattern` distinction in the contract
  taxonomy that disambiguates "specific construct" from "structural
  invariant"

When you add a new frontend or backend, the meta-HIR is the contract
you commit to — see [Adding a frontend](../contributing/adding-a-frontend.md).

## Where to go next

- [Contracts and the 5-layer taxonomy](contracts.md) — the YAML
  substrate that both lanes share.
- [The Diamond-tier substrate](diamond-substrate.md) — how `pv` enforces
  *algebraic* equivalence on top of the contract.
- [Tutorial: Python → Lean 4](../tutorials/python-to-lean.md) — what
  the proof-lane shadow of a code-lane transpile looks like.
