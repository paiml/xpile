<p align="center">
  <img src="docs/assets/hero.svg" alt="xpile architecture diagram: a code lane (Python, C, C++, Rust, Ruchy, Lean 4 → meta-HIR → Rust, Ruchy, PTX, WGSL, SPIR-V, Lean 4) and a proof lane (LaTeX, Lean theorems, mdBook ↔ contracts)" width="100%"/>
</p>

# xpile

[![ci](https://github.com/paiml/xpile/actions/workflows/ci.yml/badge.svg)](https://github.com/paiml/xpile/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/xpile.svg)](https://crates.io/crates/xpile)
[![license](https://img.shields.io/crates/l/xpile.svg)](#license)

**A polyglot transpile workbench with provable contracts at every layer.** Six language frontends (Python, C, C++, Rust, Ruchy, Lean 4) share one canonical meta-HIR and dispatch through six backends (Rust, Ruchy, PTX, WGSL, SPIR-V, Lean 4), all alongside a **proof lane** that round-trips between LaTeX, Lean 4 theorems, and mdBook through a shared YAML contract substrate. Built to solve **hybrid transpilation** — single artifacts that cross language boundaries (CPython + C extensions, Python + CUDA kernels) — which separate per-language repos cannot.

## Status — v0.1.0

**It transpiles, semantic round-trip verified in CI.** A non-trivial recursive Python function transpiles to Rust that compiles _and computes the right values_:

```python
# factorial.py
def factorial(n: int) -> int:
    return 1 if n <= 1 else n * factorial(n - 1)
```

```bash
$ xpile transpile factorial.py
// xpile-generated from Python module factorial

pub fn factorial(n: i64) -> i64 {
    if (n <= 1i64) { 1i64 } else { (n * factorial((n - 1i64))) }
}
```

CI runs `rustc -O` on the output and asserts `factorial(10) == 3628800` — the test is `factorial_emitted_rust_computes_correct_values`.

Same source, three different targets:

```bash
$ xpile transpile factorial.py --target ruchy
fun factorial(n: i64) -> i64 {
    if (n <= 1i64) { 1i64 } else { (n * factorial((n - 1i64))) }
}

$ xpile transpile factorial.py --target lean
def factorial (n : Int) : Int :=
  if (n <= (1: Int)) then (1: Int) else (n * (factorial (n - (1: Int))))
```

**By the numbers (live, not aspirational):**

- 24 workspace crates · all compile clean (`cargo check --workspace`)
- 11 contracts · `pv lint` PASS with 0 errors
- Python subset shipped: top-level `def name(p): return expr`, identifiers, int literals, `+ - * // %  ==  !=  <  <=  >  >=`, ternary `x if cond else y`. Type inference: comparisons → bool, else i64.
- Rust target: real emission with Python-floor semantics (`div_euclid` / `rem_euclid` for `//` / `%`)
- Ruchy target: real emission with `fun ... -> T { ... }` syntax
- CI: `gate` + `workspace-test` required on every PR
- Published: [`xpile 0.0.1`](https://crates.io/crates/xpile) (name reservation; v0.1.0+ is real)

> **Canonical spec:** [`docs/specifications/xpile-spec.md`](docs/specifications/xpile-spec.md) — TOC + 25 sections, each linking to a `sub/<topic>.md`.
>
> **Adversarial audit:** [`docs/specifications/audit-design.md`](docs/specifications/audit-design.md) — Popperian falsification record (4 hypotheses).

## Two lanes, one substrate

xpile has two parallel pipelines that share the YAML contract substrate. Trait-level detail in [`sub/frontend-trait.md`](docs/specifications/sub/frontend-trait.md), [`sub/backend-trait.md`](docs/specifications/sub/backend-trait.md), [`sub/contract-frontend-trait.md`](docs/specifications/sub/contract-frontend-trait.md), [`sub/contract-backend-trait.md`](docs/specifications/sub/contract-backend-trait.md).

### Code lane (executable code)

```
Frontends                      Backends
─────────                      ─────────
Python   ─┐               ┌─→ Rust        ✅ real emission
C        ─┤               ├─→ Ruchy       ✅ real emission
C++      ─┼→ meta-HIR ─→ ─┼─→ PTX         🚧 scaffold + Layer-5 contract
Rust     ─┤               ├─→ WGSL        🚧 scaffold
Ruchy    ─┤               ├─→ SPIR-V      🚧 planned
Lean 4   ─┘               └─→ Lean 4      🚧 scaffold
```

### Proof lane (notation + proofs)

```
ContractFrontends             ContractBackends
─────────────────             ─────────────────
LaTeX       ─┐                  ┌─→ LaTeX (papers)
Lean 4 thm  ─┼─→ contracts ←──←─┼─→ Lean 4 theorems
mdBook      ─┘                  └─→ mdBook
```

Lean 4 spans both lanes. LaTeX is proof-lane-only. Citation bridge uses **format-native structured constructs** (`@[xpile_contract "..."]` attribute in Lean, `\xpileContract{...}{...}` macro in LaTeX, structured comment in mdBook) — never regex over body text. Revised post-audit; see [`sub/contract-backend-trait.md`](docs/specifications/sub/contract-backend-trait.md) §"Citation bridge".

## Quick orientation

| Question | Section |
|---|---|
| What is xpile and why does it exist? | [§1 Vision and Architecture](docs/specifications/sub/vision.md) |
| How do I add a new language? | [§17 Frontend Onboarding](docs/specifications/sub/frontend-onboarding.md) |
| Lean 4 in both lanes? | [§24 Lean 4 Bidirectional](docs/specifications/sub/lean-bidirectional.md) |
| LaTeX in the proof lane? | [§25 LaTeX Bidirectional](docs/specifications/sub/latex-bidirectional.md) |
| What is hybrid transpilation? | [§16 Hybrid Transpile Flow](docs/specifications/sub/hybrid-transpile-flow.md) |
| How does the agent loop work? | [§7 Bounded Agent Repair Loop](docs/specifications/sub/agent-loop.md) |
| How are contracts validated? | [§11 Provable Contracts (`pv`)](docs/specifications/sub/pv-integration.md) |
| What's the contract taxonomy? | [§13 Contract Taxonomy](docs/specifications/sub/contract-taxonomy.md) (5 layers × 2 lanes) |
| What are the quality gates? | [§12 `pmat`](docs/specifications/sub/pmat-integration.md) + [§18 CI Pipeline](docs/specifications/sub/ci-gates.md) |

## Contracts at v0.1.0 (11)

| Contract | `pv` kind | Layer × Lane | What it pins down |
|---|---|---|---|
| `xpile-frontend-trait-v1.yaml` | pattern | 3 architectural / code | Frontend trait invariants |
| `xpile-backend-trait-v1.yaml` | pattern | 3 / code | Backend trait + structural compile-contract citation |
| `xpile-contract-frontend-trait-v1.yaml` | pattern | 3 / proof | ContractFrontend trait invariants |
| `xpile-contract-backend-trait-v1.yaml` | pattern | 3 / proof | ContractBackend + citation bridge via structured attrs |
| `py-int-arith-v1.yaml` | kernel | 1 semantics / code | Python `int` arithmetic with bigint promotion |
| `xlate-py-list-to-vec-v1.yaml` | kernel | 2 translation / code | Python list → Rust Vec, alias-preserving |
| `xlate-lean-to-rust-v1.yaml` | kernel | 2 / code | All Lean 4 constructs (def, partial, inductive, instance, axiom, ...) → Rust |
| `xlate-rust-fn-to-lean-thm-v1.yaml` | kernel | 2 / proof | Rust fn + contract → Lean 4 theorem with `@[xpile_contract]` attr |
| `notation-latex-math-to-equation-v1.yaml` | kernel | 2 / proof | LaTeX math + theorem envs → contract equations |
| `ffi-cpython-ext-v1.yaml` | pattern | 4 hybrid / code | CPython C-extension boundary semantics |
| `compile-rust-to-ptx-mma-v1.yaml` | pattern | **5 compile / code** | PTX emission: `mma.sync`, `cp.async` pipelining, SMEM budget |

`pv lint contracts/` → PASS, 0 errors.

## Workspace (24 crates)

```
crates/
├── xpile/                           CLI binary
├── xpile-core/                      session orchestration + default_session()
├── xpile-agent/                     bounded agent loop (from alchemize)
├── xpile-oracle/                    Oracle trait — capture & compare execution
├── xpile-llm/                       model invocation + content-addressed cache
├── xpile-mcp/                       MCP server
├── xpile-contracts/                 re-export of provable-contracts (pv)
├── xpile-meta-hir/                  canonical IR
├── xpile-ffi-manifest/              cross-language boundary registry
│
├── xpile-frontend/                  Frontend trait (code lane)
├── xpile-backend/                   Backend trait (code lane)
├── xpile-contract-frontend/         ContractFrontend trait (proof lane)
├── xpile-contract-backend/          ContractBackend trait (proof lane)
│
├── depyler-frontend/                Python   (.py, .pyi) — REAL parser
├── decy-frontend/                   C        (.c, .h)    — scaffold
├── ruchy-frontend/                  Ruchy    (.ruchy)    — scaffold
│
├── xpile-rust-codegen/              Rust    — REAL emission
├── xpile-ruchy-codegen/             Ruchy   — REAL emission
├── xpile-ptx-codegen/               PTX     — scaffold + Layer-5 contract
├── xpile-wgsl-codegen/              WGSL    — scaffold
├── xpile-lean-codegen/              Lean 4  — scaffold
│
├── latex-contract-frontend/         LaTeX   — scaffold
├── xpile-lean-contract-backend/     Lean theorems — scaffold (attr citation)
└── xpile-latex-contract-backend/    LaTeX papers  — scaffold (macro citation)
```

`depyler` / `decy` / `ruchy` are also exposed as workspace **aliases** so the original `cargo install depyler` / `cargo install decy` / `cargo install ruchy` consumers keep working when the merge plan in [`sub/migration.md`](docs/specifications/sub/migration.md) lands.

## CI gates (live)

Every PR runs:

| Step | Command |
|---|---|
| Formatting | `cargo fmt --all -- --check` |
| Type check | `cargo check --workspace` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Provable contracts | `pv lint contracts/` (via `aprender-contracts-cli`) |
| Security advisories | `cargo deny check advisories` |
| Tests | `cargo test --workspace` (incl. e2e rustc round-trip) |

Workflow: [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

## Family

| Repo | Role |
|---|---|
| `paiml/xpile` (this) | Polyglot transpile workbench |
| `paiml/aprender` | ML framework; source of `aprender-contracts` (`pv`) |
| `paiml/depyler` | Python→Rust transpiler — folds into xpile per [§19](docs/specifications/sub/migration.md) |
| `paiml/decy` | C→Rust transpiler — folds in |
| `paiml/ruchy` | Modern data science language; xpile's third frontend |
| `paiml/paiml-mcp-agent-toolkit` | `pmat` |
| `pymc-labs/alchemize` | Source of the four-tool agent loop pattern |

## License

MIT OR Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.
