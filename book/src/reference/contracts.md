# Contracts — the founding twelve

The twelve contracts that shipped at v0.1.0, in source order. Each
entry links to the contract YAML, its Lean theorems, and its Kani
harness.

**This page is not the full population.** The substrate has grown well
past twelve; `ls contracts/*.yaml` is the live set and `xpile quorum`
prints one row per contract with its per-stratum votes and status. The
entries below are annotated in a depth this page cannot sustain for
every contract, so it stays scoped to the founding set rather than
silently going stale — which is what it did do, presenting itself as
"all N contracts" for two months while the tree grew.

`pv lint contracts/` → PASS with **0 errors**, enforced in the
pre-push gate. Run `xpile quorum` for the live QUORUM / PARTIAL /
UNVERIFIED totals; PARTIAL is routinely non-zero.

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
| [`notation-latex-math-to-equation-v1.yaml`](https://github.com/paiml/xpile/blob/main/contracts/notation-latex-math-to-equation-v1.yaml) | kernel | 2 / proof | LaTeX math → equations; theorem envs → proof obligations |
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

**Modelled only — nothing implements this direction.** The contract
specifies how all Lean 4 constructs (`def`, `partial`, `inductive`,
`instance`, `axiom`, …) *would* lower to Rust, the inverse of the
Python→Lean flow, and carries 40 equations, 33 Lean refinement theorems
and 10 Kani harnesses saying so. There is no Lean frontend: no
registered frontend claims `.lean` (see [frontends](frontends.md)), so
`xpile transpile x.lean --target rust` exits non-zero with *"no frontend
handles `.lean`"* and no `SourceLang::Lean` module can be produced at
all. The proofs range over abstract models — a `LeanDef` is a byte
array — so they hold, and they hold of nothing shipped.

This page previously stated the lowering as present-tense fact. What
kept that readable was the §14.4 quorum reporting `C-XLATE-LEAN-TO-RUST`
at 4-of-4 strata: three of the four strata are satisfied by writing YAML
and roadmap prose, and the fourth, Runtime, was completed by a fixture
file that no test loads. `xpile quorum` now scores it **Runtime 0**
(still QUORUM, on Semantic + Symbolic + Extrinsic — which is the honest
reading: a 3-of-4 quorum needs no implementation).
`crates/xpile/tests/quorum_fixture_evidence_witness.rs` reds the day a
Lean frontend lands, so this paragraph has to move rather than stay
wrong.

## C-XLATE-RUST-FN-TO-LEAN-THM

> Layer 2 (translation) / proof lane / kind: kernel

A Rust `fn` annotated with a contract citation lifts to a Lean 4
theorem carrying the `@[xpile_contract "..."]` attribute. The
proof-lane analogue of `C-XLATE-LEAN-TO-RUST`.

## C-NOTATION-LATEX-MATH-TO-EQUATION

> Layer 2 (translation) / proof lane / kind: kernel

LaTeX math — `$...$`, `\(...\)`, `\[...\]`, and the `equation`,
`align` and `gather` environments — lowers to contract **equations**.
Theorem-class environments (`theorem`, `lemma`, `corollary`,
`proposition`, `claim`, `definition`, `remark`) lower to **proof
obligations**, whose type is `precondition` when the body opens with
`\textbf{Precondition:}` and `postcondition` otherwise. A
`\begin{proof}` body is consumed and never reaches the equations
block. Governs the bidirectional notation bridge between human-written
math and machine-checked YAML.

The exact surface is machine-readable — the `notation_surface` block
at the end of the contract — and
`crates/xpile/tests/notation_claim_witness.rs` checks it **both ways**:
every construct listed as lowering must produce output, and every
construct listed as unimplemented must produce none. Four things are
listed as unimplemented and are not claimed here: the `lean_pointer`
half of proof-env lowering, multi-row `align`/`gather` splitting,
`[label]` resolution to an equation key (it is passed through
verbatim), and nested theorem environments.

> **This paragraph was false until 2026-07-28 (PMAT-1431).** The
> theorem/proof half had never been implemented, and a theorem body's
> math surfaced as a free-standing equation rather than as an
> obligation. The Lean theorems and Kani harnesses that back this
> contract stayed green throughout, because they range over abstract
> models rather than over the shipped parser. The two-way
> `notation_surface` check is what now ties them together.

## C-XPILE-FRONTEND-TRAIT

> Layer 3 (architectural) / code lane / kind: pattern

Every `Frontend` implementation must produce a deterministic parse
(same input → same `xpile_meta_hir::Module`) and preserve source location
information for diagnostic round-trip.

## C-XPILE-BACKEND-TRAIT

> Layer 3 (architectural) / code lane / kind: pattern

Every `Backend` emission must carry a structural contract citation
(`// xpile-contract: <ID>`) — that is `compile_contract_citation`, and
like all twenty of this contract's equations it quantifies over the
**emitted** artifact. The contract says nothing about error paths.
Through v0.1.617 this entry added "Error paths must name the governing
contract", which no equation states and most backends do not do; see
[Backends → Error handling](backends.md#error-handling) for the
measured position (PMAT-1437, PMAT-1438).

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
**20**. The PTX backend shipped as a scaffold at v0.1.0; it now emits
real PTX — `xpile transpile k.py --target ptx --hardware ptx:sm_80`
produces a `.version` / `.target` / `.visible .entry` module for the
scalar element-wise + control subset. `--hardware` is **required** to
reach this backend; without a compute capability it refuses.
