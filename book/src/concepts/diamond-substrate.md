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
| **Silver** | The equation holds for the *intended canonical implementation* | 42 equations at Silver as of v0.1.0 |
| **Gold** | The equation holds as a **subtype refinement** — any value satisfying preconditions also satisfies postconditions | 12/12 contracts at Gold |
| **Platinum** | The equation holds **up to observational equivalence** — the contract is closed under composition | 12/12 contracts at Platinum |
| **Diamond** | Additional **algebraic** theorems proving deeper invariants (extensionality, completeness, identity, round-trips) | 171 wired Diamond theorems at depth-13 UNIVERSAL |

Higher tiers strictly entail lower tiers. Bronze is by construction;
Diamond is by careful axiomatization.

## What "depth-N UNIVERSAL" means

A Diamond program isn't proved in one go — it grows monotonically. We
say the substrate is at **depth-N UNIVERSAL** when *every* contract has
at least N distinct Diamond theorem categories.

| Depth | Status at v0.1.0 |
|---|---|
| depth-1..13 | **UNIVERSAL** — all 12 contracts have ≥13 Diamond categories |
| depth-14+ | 2 contracts: `C-PY-INT-ARITH` (depth-21) and `C-COMPILE-RUST-TO-PTX-MMA` (depth-20) |

Eleven UNIVERSAL milestones (depth-3 through depth-13) have been
achieved, each via a "broadening sweep" that extends a previously
narrow-deep contract pattern out to the full 12-contract substrate.

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

```bash
$ xpile diamond
xpile diamond — Diamond-tier coverage (PMAT-249)

  contract                                 diamond  depth
  ------------------------------------------------------------
  C-PY-INT-ARITH                                21  depth-21+
  C-COMPILE-RUST-TO-PTX-MMA                     20  depth-20
  C-BASHRS-POSIX-IDEMPOTENCE                    13  depth-13
  C-FFI-CPYTHON-EXT                             13  depth-13
  C-NOTATION-LATEX-MATH-TO-EQUATION             13  depth-13
  ...

totals: 171 Diamond theorems across 12 contracts
  depth-1+..depth-13+: 12 contracts each (UNIVERSAL)
  depth-14+..depth-20+: 2 contracts each
  depth-21+: 1 contract
```

JSON output is available via `xpile diamond --json` and is parsed by
the CI gate `crates/xpile/tests/diamond_coverage.rs`. The gate has 22
integration tests covering depth-1..depth-13 UNIVERSAL invariants —
**any regression fails the build**.

## What comes next

- [Tutorial: Python → Rust](../tutorials/python-to-rust.md) — the
  complete `C-PY-INT-ARITH` story, from spec to emit.
- [Reference: contracts](../reference/contracts.md) — the 12-contract
  catalogue with links to Lean theorems and Kani harnesses.
