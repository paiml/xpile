# Contracts and the 5-layer taxonomy

> **Governing contracts:** [`C-XPILE-FRONTEND-TRAIT`](../reference/contracts.md#c-xpile-frontend-trait),
> [`C-XPILE-BACKEND-TRAIT`](../reference/contracts.md#c-xpile-backend-trait),
> [`C-XPILE-CONTRACT-FRONTEND-TRAIT`](../reference/contracts.md#c-xpile-contract-frontend-trait),
> [`C-XPILE-CONTRACT-BACKEND-TRAIT`](../reference/contracts.md#c-xpile-contract-backend-trait)
> — these are the "structural" Layer-3 contracts that govern the trait
> surfaces themselves. The invariants it pins: `extension_ownership`,
> `target_ownership`, `format_ownership`, `parse_idempotency`,
> `lower_idempotency`, `render_idempotency`, `citation_preservation`,
> `compile_contract_citation`.

A **contract** in xpile is a YAML file in `contracts/` that pins down one
fact about the transpile pipeline. Each file declares:

- **id** — `C-PY-INT-ARITH`, etc. (unique, globally cited)
- **layer** — one of the 5 layers (see below)
- **lane** — `code`, `proof`, or both
- **kind** — `kernel` (a specific construct) or `pattern` (a structural
  invariant)
- **equations** — the actual statements being claimed
- **stratum_votes** — which oracles have ratified each statement

`pv lint contracts/` validates every YAML against the published schema
and must report **0 errors** — it runs in the pre-push gate, so a
contract that does not lint cannot land. Run it for the live count and
warning tally.

## The 5 layers

| Layer | Name | What it pins down | Example |
|---|---|---|---|
| 1 | **Semantics** | Behaviour of a specific language construct | `C-PY-INT-ARITH` — Python int arithmetic |
| 2 | **Translation** | A specific frontend↔backend lowering | `C-XLATE-PY-LIST-TO-VEC` — Python list → Rust Vec |
| 3 | **Architectural** | Trait surfaces + structural invariants | `C-XPILE-FRONTEND-TRAIT` — Frontend trait |
| 4 | **Hybrid** | Cross-language boundaries | `C-FFI-CPYTHON-EXT` — CPython C extensions |
| 5 | **Compile** | Backend code-generation invariants | `C-COMPILE-RUST-TO-PTX-MMA` — PTX emission |

Every layer has both a code-lane and a proof-lane shadow. `ls
contracts/*.yaml` is the live population — it spans all 5 layers and
grows most sprints; see the
[reference table](../reference/contracts.md) for the annotated list.

## The 4-stratum quorum

Per the ruchy 5.0 §14.4 **N-of-M oracle quorum** rule, a contract is
**discharged** when ≥1 vote arrives from ≥3 of these 4 strata:

| Stratum | What it is | How it's recorded |
|---|---|---|
| **Semantic** | Lean 4 refinement theorems | `contracts/lean/<Name>.lean` files cited as `lean_theorem:` in the YAML |
| **Symbolic** | Kani BMC harnesses | `contracts/kani/<name>.rs` files cited as `kani_harness:` in the YAML |
| **Runtime** | Diff-exec / fixture runs | files under `tests/fixtures/` referencing the contract ID |
| **Extrinsic** | Human-attested mentions | references to the contract ID in `docs/roadmaps/roadmap.yaml` work items |

`xpile quorum` prints one row per contract and ends with a totals line:

```text
totals: <N> QUORUM, <N> PARTIAL, <N> UNVERIFIED (<N> contracts total)
```

No numerals are reproduced here on purpose. This page carried a pasted
totals line — twelve contracts, all of them at quorum, none partial —
for two months after the substrate had grown past it, and every numeral
in it was wrong by the end. A transcript is a claim, and a claim in
prose is not re-derived when the tree moves. Run the command.
**Not every contract is at quorum**: a contract that lands
before its Lean theorem or Kani harness sits at PARTIAL until the
missing stratum votes, and the totals line is where that shows.

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
