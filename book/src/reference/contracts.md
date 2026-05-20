# Contracts at v0.1.0

All 12 contracts that ship at v0.1.0, in source order. Each row links
to the contract YAML, its Lean theorems, and its Kani harness.

`pv lint contracts/` → PASS, **0 errors and 0 warnings**.
`xpile quorum` → 12 QUORUM, 0 PARTIAL, 0 UNVERIFIED.

| Contract | `pv` kind | Layer × Lane | What it pins down |
|---|---|---|---|
| [`xpile-frontend-trait-v1.yaml`](https://github.com/paiml/xpile/blob/main/contracts/xpile-frontend-trait-v1.yaml) | pattern | 3 architectural / code | Frontend trait invariants |
| [`xpile-backend-trait-v1.yaml`](https://github.com/paiml/xpile/blob/main/contracts/xpile-backend-trait-v1.yaml) | pattern | 3 / code | Backend trait + structural compile-contract citation |
| [`xpile-contract-frontend-trait-v1.yaml`](https://github.com/paiml/xpile/blob/main/contracts/xpile-contract-frontend-trait-v1.yaml) | pattern | 3 / proof | ContractFrontend trait invariants |
| [`xpile-contract-backend-trait-v1.yaml`](https://github.com/paiml/xpile/blob/main/contracts/xpile-contract-backend-trait-v1.yaml) | pattern | 3 / proof | ContractBackend + citation bridge via structured attrs |
| [`py-int-arith-v1.yaml`](https://github.com/paiml/xpile/blob/main/contracts/py-int-arith-v1.yaml) | kernel | 1 semantics / code | Python `int` arithmetic with bigint promotion |
| [`bashrs-posix-idempotence-v1.yaml`](https://github.com/paiml/xpile/blob/main/contracts/bashrs-posix-idempotence-v1.yaml) | pattern | 1 / code | POSIX shell idempotence, Python↔bashrs cross-domain |
| [`xlate-py-list-to-vec-v1.yaml`](https://github.com/paiml/xpile/blob/main/contracts/xlate-py-list-to-vec-v1.yaml) | kernel | 2 translation / code | Python list → Rust Vec, alias-preserving |
| [`xlate-lean-to-rust-v1.yaml`](https://github.com/paiml/xpile/blob/main/contracts/xlate-lean-to-rust-v1.yaml) | kernel | 2 / code | All Lean 4 constructs → Rust |
| [`xlate-rust-fn-to-lean-thm-v1.yaml`](https://github.com/paiml/xpile/blob/main/contracts/xlate-rust-fn-to-lean-thm-v1.yaml) | kernel | 2 / proof | Rust fn + contract → Lean 4 theorem |
| [`notation-latex-math-to-equation-v1.yaml`](https://github.com/paiml/xpile/blob/main/contracts/notation-latex-math-to-equation-v1.yaml) | kernel | 2 / proof | LaTeX math + theorem envs → contract equations |
| [`ffi-cpython-ext-v1.yaml`](https://github.com/paiml/xpile/blob/main/contracts/ffi-cpython-ext-v1.yaml) | pattern | 4 hybrid / code | CPython C-extension boundary semantics |
| [`compile-rust-to-ptx-mma-v1.yaml`](https://github.com/paiml/xpile/blob/main/contracts/compile-rust-to-ptx-mma-v1.yaml) | pattern | **5 compile / code** | PTX emission: `mma.sync`, `cp.async` pipelining, SMEM budget |

The Lean theorems and Kani harnesses for each contract live at the
parallel paths
`contracts/lean/<Name>.lean` and `contracts/kani/<name>.rs` in the
repository.

## C-PY-INT-ARITH

> Layer 1 (semantics) / code lane / kind: kernel

Python `int` is unbounded; emitted target code must either:

1. **discharge by construction** (Lean's `Int` is unbounded — no
   action), or
2. **discharge by checked op + bigint fallback** (Rust/Ruchy's `i64`
   needs `.checked_*().expect("…C-PY-INT-ARITH slow path…")` until the
   bigint slow path is implemented).

Diamond depth: **21** (deepest contract in the substrate). See the
[Python → Rust tutorial](../tutorials/python-to-rust.md).

## C-BASHRS-POSIX-IDEMPOTENCE

> Layer 1 (semantics) / code lane / kind: pattern

The supported POSIX-shell subset round-trips without drift, and the
emitted constructs are idempotent (`mkdir -p`, conditional file
creation, redirects). Covers cross-domain Python↔shell.

See the [shell-roundtrip tutorial](../tutorials/shell-roundtrip.md).

## C-XLATE-PY-LIST-TO-VEC

> Layer 2 (translation) / code lane / kind: kernel

Python `list` → Rust `Vec`, with alias-preservation semantics. Pins
down what happens when `a = [1,2,3]; b = a; b.append(4)` — both `a`
and `b` see the mutation.

## C-XLATE-LEAN-TO-RUST

> Layer 2 (translation) / code lane / kind: kernel

All Lean 4 constructs (`def`, `partial`, `inductive`, `instance`,
`axiom`, …) lower to Rust. The inverse direction of the
Python→Lean flow.

## C-XLATE-RUST-FN-TO-LEAN-THM

> Layer 2 (translation) / proof lane / kind: kernel

A Rust `fn` annotated with a contract citation lifts to a Lean 4
theorem carrying the `@[xpile_contract "..."]` attribute. The
proof-lane analogue of `C-XLATE-LEAN-TO-RUST`.

## C-NOTATION-LATEX-MATH-TO-EQUATION

> Layer 2 (translation) / proof lane / kind: kernel

LaTeX math environments and `theorem`/`lemma` blocks lower to
contract equations. Governs the bidirectional notation bridge between
human-written math and machine-checked YAML equations.

## C-XPILE-FRONTEND-TRAIT

> Layer 3 (architectural) / code lane / kind: pattern

Every `Frontend` implementation must produce a deterministic parse
(same input → same `MetaHirModule`) and preserve source location
information for diagnostic round-trip.

## C-XPILE-BACKEND-TRAIT

> Layer 3 (architectural) / code lane / kind: pattern

Every `Backend` emission must carry a structural contract citation
(`// xpile-contract: <ID>`). Error paths must name the governing
contract.

## C-XPILE-CONTRACT-FRONTEND-TRAIT

> Layer 3 (architectural) / proof lane / kind: pattern

Every `ContractFrontend` (LaTeX, mdBook, Lean) must produce a
deterministic parse of its notation source into contract equations.

## C-XPILE-CONTRACT-BACKEND-TRAIT

> Layer 3 (architectural) / proof lane / kind: pattern

Every `ContractBackend` must use **format-native structured
constructs** for the citation bridge — never regex over body text.
In Lean: `@[xpile_contract "..."]`. In LaTeX: `\xpileContract{...}{...}`.
In mdBook: a structured HTML comment.

## C-FFI-CPYTHON-EXT

> Layer 4 (hybrid) / code lane / kind: pattern

The semantics of a CPython C-extension boundary: refcount discipline,
GIL acquire/release at the boundary, error-propagation rules for
`PyErr_Occurred()`.

## C-COMPILE-RUST-TO-PTX-MMA

> **Layer 5 (compile)** / code lane / kind: pattern

The deepest layer — emitted PTX must respect the `mma.sync` shape
constraints, `cp.async` pipelining, and SMEM budget. Diamond depth:
**20**. The PTX backend at v0.1.0 ships as a scaffold; the contract
holds the placeholder pinned down for the real implementation to
discharge.
