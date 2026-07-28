# The Diamond-tier substrate

> **Source of truth:** [`docs/specifications/sub/diamond-taxonomy.md`](https://github.com/paiml/xpile/blob/main/docs/specifications/sub/diamond-taxonomy.md)
> in the canonical spec. The Diamond program runs in parallel to the
> book and is the deepest layer of contract enforcement.

The 4-stratum quorum (Semantic, Symbolic, Runtime, Extrinsic) is the
floor: it answers "does the contract hold?". The Diamond-tier substrate
answers a stronger question: "**which algebraic invariants** of the
contract hold, and **at what depth**?"

Concretely: each contract carries a growing portfolio of `_diamond`
theorems in `contracts/lean/`, each proving a structural property that
any conforming implementation must satisfy.

## Refinement tiers

A contract can be discharged at increasing levels of confidence:

| Tier | Meaning | Example |
|---|---|---|
| **Bronze** | The equation type-checks by construction (`rfl` proof) | Every Layer-1 equation gets a Bronze theorem for free |
| **Silver** | The equation holds for the *intended canonical implementation* | reached by the founding-twelve equations |
| **Gold** | The equation holds as a **subtype refinement** — any value satisfying preconditions also satisfies postconditions | reached across the founding twelve |
| **Platinum** | The equation holds **up to observational equivalence** — the contract is closed under composition | reached across the founding twelve |
| **Diamond** | Additional **algebraic** theorems proving deeper invariants (extensionality, completeness, identity, round-trips) | `xpile diamond` for the live per-contract count |

Higher tiers strictly entail lower tiers. Bronze is by construction;
Diamond is by careful axiomatization.

## What "depth-N UNIVERSAL" means

A Diamond program isn't proved in one go — it grows monotonically. We
say the substrate is at **depth-N UNIVERSAL** when *every* contract has
at least N distinct Diamond theorem categories.

**Read the universal depth off `xpile diamond`, not off this page.** The
totals block prints how many contracts sit at each `depth-N+`; the
universal depth is the largest N whose count still equals
`contracts_total`.

Eleven UNIVERSAL milestones (depth-3 through depth-13) were achieved
over the **founding twelve** contracts, each via a "broadening sweep"
that extended a previously narrow-deep pattern out to the whole
substrate of the day. That deep core is still there — a group of
contracts carries ≥13 Diamond categories, and two go past depth-20.

But the substrate has since grown well past twelve, and
`crates/xpile/tests/diamond_coverage.rs` deliberately **grandfathers**
the depth-13 gate: a new contract joins at depth-1+ rather than paying
a depth-13 treadmill on arrival. So over the *whole* population the
universal depth is far lower than the deep core's — most contracts
carry a single Diamond category. This page said "depth-1..13
UNIVERSAL — all 12 contracts have ≥13 Diamond categories" long after
that stopped describing every contract, which under the definition
directly above it is the difference between a claim about all
contracts and a claim about thirteen of them.

## The 13 recurring templates

By v0.1.0 the substrate had discovered **13 recurring algebraic
templates** that show up across many contracts:

| # | Template | Coverage |
|---|---|---|
| 1 | Structure extensionality | 32+ contracts |
| 2 | Array.size structure | 11 contracts |
| 3 | Enum distinctness | 3 contracts |
| 4 | Nat structure | 1 contract |
| 5 | Reverse involution | 1 contract |
| 6 | String.length Nat-structure | 3 contracts |
| 7 | Int-sign decomposition | 2 contracts |
| 8 | Enum completeness | 3 contracts |
| 9 | Gold-tier subtype extensionality | 11 contracts |
| 10 | Tier-projection homomorphism (Silver→Bronze) | 9 contracts |
| 11 | Canonical identity element | 10 contracts |
| 12 | Bronze→Silver canonical-lift homomorphism | 10 contracts |
| 13 | Bronze↔Silver round-trip identity | 10 contracts |

Templates 10–13 form a **compositional suite**: lift Bronze→Silver,
project Silver→Bronze, and the composition equals identity. This is
the substrate-level proof that the canonical refinement-tier model is
internally coherent.

## Inspecting the Diamond state

<!-- DIAMOND-TRANSCRIPT:BEGIN -->
```bash
$ xpile diamond
xpile diamond — Diamond-tier coverage (PMAT-249)
depth: 0 Diamonds = none, N Diamonds = depth-N (exact — the column is never bucketed; the `depth-N+` figures in the totals block are CUMULATIVE counts, not classifications)

  contract                                 diamond  depth
  ------------------------------------------------------------
  C-PY-INT-ARITH                                21  depth-21
  C-COMPILE-RUST-TO-PTX-MMA                     20  depth-20
  C-BASHRS-POSIX-IDEMPOTENCE                    13  depth-13
  C-FFI-CPYTHON-EXT                             13  depth-13
  C-NOTATION-LATEX-MATH-TO-EQUATION             13  depth-13
  ...

totals: <N> Diamond theorems across <N> contracts
  depth-1+: <N> contracts, depth-2+: <N> contracts, ...
```
<!-- DIAMOND-TRANSCRIPT:END -->

The legend line and every contract row above are compared to the live
binary, by equality, in `crates/xpile/tests/diamond_depth_label_witness.rs`.
The earlier copy of this transcript **omitted the legend** — an unmarked
elision, which is why the honest repair of this page never saw that the
legend was the falsehood: it announced a `depth-3+` bucket the reporter
could not produce, directly above a column whose first three rows read
`depth-21+`, `depth-20`, `depth-13` (PMAT-1448).

The totals block is reproduced here as a **shape**, not as numbers. A
pasted numeral is a claim that nothing re-derives, and the numerals
that used to sit here (`171 Diamond theorems across 12 contracts`,
`depth-1+..depth-13+: 12 contracts each (UNIVERSAL)`) outlived the tree
they described by twenty-three contracts.

JSON output is available via `xpile diamond --json` and is parsed by
the CI gate `crates/xpile/tests/diamond_coverage.rs`, which holds the
depth-13 floor over a **named, grandfathered set** of contracts — the
ones that had reached it when the gate was written. A contract outside
that set is deliberately not checked against the floor, so the gate
protects the deep core against regression and does not claim anything
about the rest.

## What comes next

- [Tutorial: Python → Rust](../tutorials/python-to-rust.md) — the
  complete `C-PY-INT-ARITH` story, from spec to emit.
- [Reference: contracts](../reference/contracts.md) — the founding-twelve
  catalogue with links to Lean theorems and Kani harnesses.
