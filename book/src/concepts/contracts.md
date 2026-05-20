# Contracts and the 5-layer taxonomy

> **Governing contracts:** [`C-XPILE-FRONTEND-TRAIT`](../reference/contracts.md#c-xpile-frontend-trait),
> [`C-XPILE-BACKEND-TRAIT`](../reference/contracts.md#c-xpile-backend-trait),
> [`C-XPILE-CONTRACT-FRONTEND-TRAIT`](../reference/contracts.md#c-xpile-contract-frontend-trait),
> [`C-XPILE-CONTRACT-BACKEND-TRAIT`](../reference/contracts.md#c-xpile-contract-backend-trait)
> — these are the "structural" Layer-3 contracts that govern the trait
> surfaces themselves.

A **contract** in xpile is a YAML file in `contracts/` that pins down one
fact about the transpile pipeline. Each file declares:

- **id** — `C-PY-INT-ARITH`, etc. (unique, globally cited)
- **layer** — one of the 5 layers (see below)
- **lane** — `code`, `proof`, or both
- **kind** — `kernel` (a specific construct) or `pattern` (a structural
  invariant)
- **equations** — the actual statements being claimed
- **stratum_votes** — which oracles have ratified each statement

`pv lint contracts/` validates every YAML against the published schema.
At v0.1.0, all 12 contracts pass with **0 errors and 0 warnings**.

## The 5 layers

| Layer | Name | What it pins down | Example |
|---|---|---|---|
| 1 | **Semantics** | Behaviour of a specific language construct | `C-PY-INT-ARITH` — Python int arithmetic |
| 2 | **Translation** | A specific frontend↔backend lowering | `C-XLATE-PY-LIST-TO-VEC` — Python list → Rust Vec |
| 3 | **Architectural** | Trait surfaces + structural invariants | `C-XPILE-FRONTEND-TRAIT` — Frontend trait |
| 4 | **Hybrid** | Cross-language boundaries | `C-FFI-CPYTHON-EXT` — CPython C extensions |
| 5 | **Compile** | Backend code-generation invariants | `C-COMPILE-RUST-TO-PTX-MMA` — PTX emission |

Every layer has both a code-lane and a proof-lane shadow. At v0.1.0
there are **12 contracts** spanning all 5 layers; see the
[reference table](../reference/contracts.md) for the full list.

## The 4-stratum quorum

Per the ruchy 5.0 §14.4 **N-of-M oracle quorum** rule, a contract is
**discharged** when ≥1 vote arrives from ≥3 of these 4 strata:

| Stratum | What it is | How it's recorded |
|---|---|---|
| **Semantic** | Lean 4 refinement theorems | `contracts/lean/<Name>.lean` files cited as `lean_theorem:` in the YAML |
| **Symbolic** | Kani BMC harnesses | `contracts/kani/<name>.rs` files cited as `kani_harness:` in the YAML |
| **Runtime** | Diff-exec / fixture runs | files under `tests/fixtures/` referencing the contract ID |
| **Extrinsic** | Human-attested mentions | references to the contract ID in `docs/roadmaps/roadmap.yaml` work items |

At v0.1.0:

```text
$ xpile quorum
  ... (12 contracts, all at QUORUM)
  totals: 12 QUORUM, 0 PARTIAL, 0 UNVERIFIED (12 contracts total)
```

— 638 stratum-vote artifacts total (285 Semantic + 53 Symbolic + 15
Runtime + 285 Extrinsic).

## Why YAML (not Lean)?

A natural question: why not declare contracts directly in Lean? Two
reasons:

1. **Non-experts must read them.** A C++ backend implementer who doesn't
   know Lean still needs to know what `C-COMPILE-RUST-TO-PTX-MMA`
   actually says about `mma.sync` instruction scheduling. YAML is the
   floor; Lean is the ceiling.
2. **Multiple oracles, one source of truth.** Kani BMC harnesses, Lean
   theorems, and runtime fixtures all reference the same equation. If
   the equation lived in Lean, you'd have to round-trip through Lean
   even to spell it out in a Kani comment.

So YAML is the substrate; Lean theorems and Kani harnesses are *bound*
to YAML equations via citation. The
[`C-NOTATION-LATEX-MATH-TO-EQUATION`](../reference/contracts.md#c-notation-latex-math-to-equation)
contract governs that citation bridge.

## What "kind: kernel" vs "kind: pattern" means

A **kernel** contract pins down one specific construct end-to-end. For
example, `C-PY-INT-ARITH` says exactly: "Python `int` is unbounded; an
`i64` codomain must emit `.checked_*().expect(...)`".

A **pattern** contract pins down a structural invariant that any
implementation must satisfy. For example, `C-XPILE-FRONTEND-TRAIT` says:
"Any type implementing the `Frontend` trait must produce a deterministic
parse — same input, same `MetaHirModule`."

The distinction matters because *kernels* compose by addition (more
constructs supported), but *patterns* compose by intersection (more
invariants required). Pattern contracts are typically thinner but apply
to every implementation; kernel contracts are typically larger but apply
only to one construct.

## What comes next

- [The Diamond-tier substrate](diamond-substrate.md) — how `pv`
  enforces *algebraic* invariants on top of the equations.
- [Reference: contracts at v0.1.0](../reference/contracts.md) — the
  exhaustive 12-contract table.
