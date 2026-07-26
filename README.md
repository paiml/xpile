<p align="center">
  <img src="docs/assets/hero.svg" alt="xpile architecture: a code lane (Python, C, Ruchy, Shell, WebAssembly → meta-HIR → Rust, Ruchy, WebAssembly, PTX, WGSL, SPIR-V, Lean 4, Shell) and a proof lane (LaTeX, Lean theorems, mdBook ↔ YAML contracts)" width="100%"/>
</p>

# xpile

[![ci](https://github.com/paiml/xpile/actions/workflows/ci.yml/badge.svg)](https://github.com/paiml/xpile/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/xpile.svg)](https://crates.io/crates/xpile)
[![docs](https://img.shields.io/badge/docs-book-blue.svg)](https://paiml.github.io/xpile/)
[![rustc](https://img.shields.io/badge/rustc-1.93%2B-orange.svg)](#install)
[![license](https://img.shields.io/crates/l/xpile.svg)](#license)

**A polyglot transpile workbench where every construct is carried by a
machine-checked contract.** xpile lowers four source languages through one
canonical meta-HIR and emits to nine backends — so the same program can target
Rust, WebAssembly, a GPU, or a Lean 4 proof. Its distinguishing promise:
*transpile-success means the output compiles and matches the source's
semantics.* When xpile cannot guarantee that for a construct, it refuses at
transpile time with a reason instead of emitting code that silently diverges.

## Quickstart

```bash
cargo install xpile
```

```python
# factorial.py
def factorial(n: int) -> int:
    return 1 if n <= 1 else n * factorial(n - 1)
```

```bash
$ xpile transpile factorial.py
// xpile-generated from Python module factorial

// xpile-contract: C-PY-INT-ARITH
pub fn factorial(n: i64) -> i64 {
    if (n <= 1i64) { 1i64 } else {
        (n).checked_mul(factorial(
            (n).checked_sub(1i64).expect("… i64 subtraction overflow; bigint slow path not yet implemented")
        )).expect("… i64 multiplication overflow; bigint slow path not yet implemented")
    }
}
```

Every arithmetic operation is `checked_*` and every emitted item cites the
contract it satisfies (`// xpile-contract: C-PY-INT-ARITH`). An `i64` overflow
**panics with a pointer to the unimplemented bigint path** rather than wrapping
silently — CPython's `int` is unbounded, so wrapping would be a wrong answer.
CI compiles this output with `rustc -O` and asserts `factorial(10) == 3628800`.

More runnable programs are in [`examples/`](examples/).

## Same source, four targets

One meta-HIR, many backends:

```bash
$ xpile transpile factorial.py --target ruchy   # → Ruchy   (compiles to Rust)
$ xpile transpile factorial.py --target lean    # → Lean 4  (def + @[xpile_contract] attr)
$ xpile transpile factorial.py --target wasm    # → native WebAssembly text
```

```lean
-- --target lean
@[xpile_contract "C-PY-INT-ARITH"]
def factorial (n : Int) : Int :=
  if (n <= (1: Int)) then (1: Int) else (n * (factorial (n - (1: Int))))
```

Lean's `Int` is unbounded, so the same overflow contract holds *by
construction* — no `checked_*` needed.

> **Caveat — the default Lean emit does not elaborate.** `--contracts on` is
> the default, and `xpile_contract` is not a registered Lean attribute, so
> `lean` rejects the output above with `unexpected token; expected ']'`. Pass
> `--contracts off` for Lean you intend to elaborate. Lean is the only backend
> affected: every other one cites contracts in comments.

## Why xpile

- **Faithful by contract.** Emitted code is verified against the source
  language's semantics (floor-division, `int` overflow, `dict` iteration order,
  string/UTF-8 handling). Divergences are refused, not shipped.
- **One meta-HIR, many targets.** Rust, Ruchy, WebAssembly, PTX, WGSL, SPIR-V,
  Lean 4, and POSIX shell all descend from a single intermediate representation.
- **A proof lane, not just a code lane.** Contracts are shared YAML validated by
  paired Lean 4 refinement theorems and Kani symbolic harnesses, and round-trip
  between LaTeX and mdBook.
- **Hybrid transpilation.** `xpile hybrid <dir> --verify` emits a buildable
  Cargo workspace for cross-language artifacts (Python + C extensions), builds
  it, runs it, and differential-matches the result against the CPython
  reference — the problem separate per-language repos cannot solve.

## Architecture

Two pipelines share one YAML contract substrate.

### Code lane

```
Frontends                        Backends
─────────                        ─────────
Python  ─┐                  ┌─→  Rust          full emission, runtime-verified
Shell   ─┤                  ├─→  Ruchy         full emission (compiles to Rust)
C       ─┼→  meta-HIR  ─→  ─┼─→  WebAssembly   native emission (linear-memory runtime)
WASM    ─┘                  ├─→  Shell         POSIX round-trip (flat-command subset)
                            ├─→  PTX / WGSL / SPIR-V   GPU emission, executed on hardware
                            ├─→  Lean 4        def / theorem forms
                            └─→  forjar.yaml   infrastructure-as-code
```

Python has a full parser; Shell (bashrs) parses the POSIX flat-command subset;
C and the WASM lift frontend are narrower. See [`xpile info`](#usage) for the
live registry.

**Ruchy is an OUTPUT language only.** `--target ruchy` is a full emission, but
`.ruchy` **input** refuses with a non-zero exit — there is no Ruchy parser
(PMAT-1346). It is registered purely so `.ruchy` files get that specific
refusal instead of a generic "no frontend handles" message. Reading Ruchy is
v0.2.0 work, and until it lands it is not counted among the four source
languages above.

### Proof lane

```
ContractFrontends            ContractBackends
─────────────────            ─────────────────
LaTeX  ─┐                       ┌─→  LaTeX (papers)
Lean 4 ─┼→  contracts  ←──←─    ┼─→  Lean 4 theorems
mdBook ─┘                       └─→  mdBook
```

The citation bridge uses **format-native structured constructs** — the
`@[xpile_contract "…"]` attribute in Lean, a `\xpileContract{…}{…}` macro in
LaTeX, a structured comment in mdBook — never a regex over prose.

## Contracts

Every emittable construct is anchored to a contract in [`contracts/`](contracts/),
validated on every commit by `pv lint contracts/`. Each contract carries a Lean
4 refinement theorem and a Kani bounded-model-checking harness, and is scored
against the N-of-M *oracle quorum* bar — ≥1 vote across ≥3 of the four strata
(Semantic / Symbolic / Runtime / Extrinsic). The mature core sits at full
QUORUM while newer contracts are still accreting stratum votes; run
`xpile quorum` for the live per-contract tally:

```text
$ xpile quorum
  totals: 15 QUORUM, 20 PARTIAL, 0 UNVERIFIED (35 contracts total)
```

Full detail — the contract taxonomy, quorum strata, and the "Diamond" theorem
depth program — lives in the specification, not this README:

> **Spec:** [`docs/specifications/xpile-spec.md`](docs/specifications/xpile-spec.md) ·
> **Adversarial audit:** [`docs/specifications/audit-design.md`](docs/specifications/audit-design.md)

## Workspace

31 crates. The core pipeline:

```
crates/
├── xpile/                     CLI binary
├── xpile-core/                session orchestration
├── xpile-meta-hir/            canonical IR (shared by every lane)
├── xpile-contracts/           re-export of provable-contracts (pv)
│
├── depyler-frontend/          Python  (.py, .pyi)   — full parser
├── bashrs-frontend/           Shell   (.sh)         — POSIX flat-command subset
├── decy-frontend/             C       (.c, .h)
├── ruchy-frontend/            Ruchy   (.ruchy)
│
├── xpile-rust-codegen/        Rust         — full emission
├── xpile-ruchy-codegen/       Ruchy        — full emission
├── xpile-wasm-codegen/        WebAssembly  — native linear-memory runtime
├── bashrs-backend/            Shell        — POSIX round-trip
├── xpile-ptx-codegen/         PTX          ┐
├── xpile-wgsl-codegen/        WGSL         ├ GPU emission, executed on hardware
├── xpile-spirv-codegen/       SPIR-V       ┘
├── xpile-lean-codegen/        Lean 4       — def / theorem forms
│
├── latex-contract-frontend/       LaTeX  → contracts   (proof lane)
├── xpile-lean-contract-backend/   contracts → Lean 4 theorems
└── xpile-latex-contract-backend/  contracts → LaTeX papers
```

`depyler` / `decy` / `ruchy` are also exposed as workspace **aliases**, so the
original `cargo install depyler` / `decy` / `ruchy` consumers keep working.

## CI gates

Every pull request runs:

| Gate | Command |
|---|---|
| Format | `cargo fmt --all -- --check` |
| Type check | `cargo check --workspace` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Contracts | `pv lint contracts/` |
| Security | `cargo deny check advisories` |
| Tests | `cargo test --workspace` (incl. `rustc` round-trip of emitted output) |
| Symbolic | `cargo kani` over every harness in `contracts/kani/` |
| Docs | `pmat validate-docs` (link integrity) + `pmat demo-score` |

Two jobs are merge-blocking: **`gate`** (the checklist above minus tests) and
**`workspace-test`** (the full suite, including the execution witnesses). The
other six — `kani`, `lake-build`, `docs`, `wasi`, `lean-models`,
`shader-validate` — run on every PR and go red on a real regression, but are
**advisory**: they do not block a merge. Notably that includes the proof lane,
so a failing Kani harness or Lean build is visible without being blocking; see
[`docs/status/enforcement-handoff.md`](docs/status/enforcement-handoff.md) §2.
Workflow: [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

## Install

```bash
cargo install xpile
```

Requires Rust 1.93+. For source builds and the optional dev tooling (`pv`,
`pmat`, `cargo kani`), see the
[book's Installation chapter](https://paiml.github.io/xpile/installation.html).

## Usage

```bash
$ xpile info                                  # list registered frontends/backends
$ xpile transpile factorial.py                # Python → Rust (default)
$ xpile transpile factorial.py --target ruchy # Python → Ruchy
$ xpile transpile factorial.py --target lean  # Python → Lean 4
$ xpile transpile factorial.py --target wasm  # Python → WebAssembly
$ xpile transpile script.sh    --target shell # POSIX shell round-trip
$ xpile transpile model.py --emit-crate ./out # emit a complete, buildable crate
$ xpile transpile factorial.py --contracts off # suppress the // xpile-contract: citations
$ xpile hybrid ./project --verify             # cross-language build + differential check
$ xpile quorum                                # contract oracle-quorum report
$ xpile diamond                               # Diamond-tier coverage report
```

By default, every emitted construct is annotated with its `// xpile-contract:`
citations across the applicable taxonomy layers (L1 semantics, L2 translation,
L4 hybrid, L5 compile) — on every backend. Pass `--contracts off` for
annotation-free output; the library equivalent is
[`xpile_backend::strip_contract_citations`](crates/xpile-backend/src/lib.rs).

### Universal binary

> **Two DISJOINT WebAssembly paths — they share no code.** `--target wasm`
> (above) is the native WAT emitter: meta-HIR straight to WebAssembly text, and
> it does *not* produce a WASI binary. The universal binary below goes through
> `--emit-crate` → Rust → `wasm32-wasip1`, so it is Rust's toolchain that emits
> the `.wasm`. A program that builds via this section may still refuse under
> `--target wasm`, and vice versa.

`--emit-crate` writes a complete Cargo crate. If the program defines `main()`,
that crate compiles to a **single portable WebAssembly binary** that runs on any
OS/arch under a WASI runtime — no libc, architecture, or OS baked in:

```bash
$ xpile transpile examples/proven-model/model.py --emit-crate /tmp/model
$ cd /tmp/model && cargo build --release --target wasm32-wasip1
$ wasmtime run target/wasm32-wasip1/release/model.wasm    # output matches CPython
```

The emitted function carries its `// xpile-contract:` citation, so the proof
travels with the code — the delivery vehicle for
[proven-model-as-code](examples/proven-model/). Drop the `--target` for a native
binary from the same crate.

The generated Rust is `std`-only **except** for Python `dict`, which lowers to
[`indexmap::IndexMap`](https://docs.rs/indexmap) to preserve insertion order (a
plain `HashMap` iterates non-deterministically). Add `indexmap = "2"` to the
transpiled crate's `Cargo.toml` if its output uses any `dict`.

Full CLI reference and tutorials: **<https://paiml.github.io/xpile/>**.

## Project family

| Repo | Role |
|---|---|
| [`paiml/xpile`](https://github.com/paiml/xpile) (this) | Polyglot transpile workbench |
| [`paiml/aprender`](https://github.com/paiml/aprender) | ML framework; source of `aprender-contracts` (`pv`) |
| [`paiml/depyler`](https://github.com/paiml/depyler) | Python→Rust transpiler — folds into xpile |
| [`paiml/decy`](https://github.com/paiml/decy) | C→Rust transpiler — folds in |
| [`paiml/ruchy`](https://github.com/paiml/ruchy) | Data-science language; xpile's Ruchy frontend/backend |
| [`paiml/paiml-mcp-agent-toolkit`](https://github.com/paiml/paiml-mcp-agent-toolkit) | `pmat` quality toolkit |

## License

MIT OR Apache-2.0. See [`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE).
