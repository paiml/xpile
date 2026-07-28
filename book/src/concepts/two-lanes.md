# Two lanes, one substrate

xpile has two parallel pipelines that share the YAML contract substrate.
This is the single most important mental model in the system.

<!-- XPILE-LANEROSTER-001:CODE:BEGIN -->
```text
Frontends                      Backends
─────────                      ─────────
python   ─┐               ┌─→ rust
c        ─┤               ├─→ ruchy
bashrs   ─┼→ meta-HIR ─→ ─┼─→ bashrs
ruchy    ─┤               ├─→ lean
wasm     ─┘               ├─→ wasm
                          ├─→ ptx
                          ├─→ wgsl
                          ├─→ spirv
                          └─→ forjar
```
<!-- XPILE-LANEROSTER-001:CODE:END -->

That is the **code lane** — runnable code in, runnable code out. The
names are the registry keys `xpile info` prints, and this roster is
checked against the live registry in both directions by
`crates/xpile/tests/lane_roster_witness.rs` — a name here that nothing
registers, or a registered name missing here, reds.

**What a name in the left column does and does not mean.** It means the
registry routes that spelling to a frontend, not that the frontend
parses it: `ruchy` is registered so a `.ruchy` input gets a named
refusal, and it refuses every input. `xpile info` is the live word on
which frontends lower — it prints `frontends (5 registered, 4 lowering)`
and tags the exception. **Per-backend maturity is not shown here on
purpose**: it lives in one place, the measured
[Backends → Status](../reference/backends.md#status) table, and it used
to be duplicated into this diagram — where it went stale, marking PTX,
WGSL, SPIR-V and Lean as scaffolds or planned long after all four
emitted (PMAT-1440).

<!-- XPILE-LANEROSTER-001:PROOF:BEGIN -->
```text
ContractFrontends             ContractBackends
─────────────────             ─────────────────
                                ┌─→ latex
latex       ───→ contracts ←──←─┤
                                └─→ lean-theorem
```
<!-- XPILE-LANEROSTER-001:PROOF:END -->

That is the **proof lane** — notation in, notation and proofs out, both
sides talking to the *same* contract YAML.

**The proof lane is the immature one, and the diagram above is a wiring
diagram, not a capability claim.** One contract frontend is registered
and it does parse; both contract backends are **scaffolds** that return
a fixed `_scaffold` payload no field of the contract can influence, so
`xpile info` reports them as `contract_backends (2 registered, 0
rendering)` and tags each. Real rendering is v0.2.0 work — see
[Backends](../reference/backends.md) and PMAT-1429.

**There is no mdBook contract frontend or backend**, and no Lean 4
contract *frontend*. `MdBook` is an enum variant in `xpile-contracts`
with nothing behind it. Through v0.1.617 this page drew both, and drew
three code-lane frontends (C++, Rust, Lean 4) that likewise do not
exist — `.cpp`, `.rs` and `.lean` inputs all exit non-zero — while
omitting the `wasm` frontend and the `wasm` and `forjar` backends that
do (PMAT-1440).

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
- In mdBook: a structured HTML comment — **specified, not implemented**;
  no mdBook `ContractBackend` is registered, so nothing emits this form
  today (PMAT-1440).

Those are the **`ContractBackend`** forms — contract YAML rendered to
theorem text or LaTeX, which is read as prose and never elaborated. The
**code lane** is separate: `xpile transpile x.py --target lean` cites
with a `/-- xpile-contract: … -/` docstring, because a file that `lean`
must actually parse cannot carry an attribute no prelude registers (see
[Reference: backends](../reference/backends.md#lean-4-backend--whats-emitted)).
Both are structured; only the docstring is resolvable out of a live
elaborated environment.

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
