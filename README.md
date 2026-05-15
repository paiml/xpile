# xpile

**A polyglot transpile workbench.** Pluggable language frontends (Python, C, Ruchy, and future C++, CUDA, ...) share one Rust codegen backend, one bounded agent repair loop, one oracle abstraction, one MCP server, and one provable-contracts framework. Built to solve **hybrid transpilation** — single artifacts that cross language boundaries (CPython + C extensions, Python + CUDA kernels) — which separate per-language repos cannot.

**Status:** Scaffold (v0.1.0). 14 workspace crates compile clean; `provable-contracts` (the `pv` framework) is wired; 4 example contracts pass `pv lint` 8/8 gates; no working transpilation logic yet.

> **Canonical spec:** [`docs/specifications/xpile-spec.md`](docs/specifications/xpile-spec.md) — TOC + 23 sections, each linking to a `sub/<topic>.md`.
>
> **Current status:** [`docs/status/CURRENT.md`](docs/status/CURRENT.md) — future-session pickup point.

## Quick orientation

| Question | Section |
|---|---|
| What is xpile and why does it exist? | [§1 Vision and Architecture](docs/specifications/sub/vision.md) |
| How do I add a new language? | [§17 Frontend Onboarding](docs/specifications/sub/frontend-onboarding.md) |
| What is hybrid transpilation? | [§16 Hybrid Transpile Flow](docs/specifications/sub/hybrid-transpile-flow.md) |
| How does the agent loop work? | [§7 Bounded Agent Repair Loop](docs/specifications/sub/agent-loop.md) |
| How are contracts validated? | [§11 Provable Contracts (`pv`)](docs/specifications/sub/pv-integration.md) |
| What are the quality gates? | [§12 Quality Regime (`pmat`)](docs/specifications/sub/pmat-integration.md) + [§18 CI Pipeline](docs/specifications/sub/ci-gates.md) |
| What's the rollout plan? | [§21 Phased Rollout](docs/specifications/sub/phased-rollout.md) |
| Where are we now? | [docs/status/CURRENT.md](docs/status/CURRENT.md) |

## Architecture (one screen)

```
crates/
├── xpile/                  # CLI binary
├── xpile-core/             # session orchestration
├── xpile-agent/            # bounded agent loop (from alchemize)
├── xpile-oracle/           # Oracle trait — capture & compare execution
├── xpile-llm/              # model invocation + content-addressed cache
├── xpile-mcp/              # MCP server
├── xpile-contracts/        # re-export of provable-contracts (pv)
├── xpile-rust-codegen/     # shared Rust emission
├── xpile-meta-hir/         # canonical IR
├── xpile-ffi-manifest/     # cross-language boundary registry
├── xpile-frontend/         # Frontend trait
│
├── depyler-frontend/       # Python (extensions: py, pyi)
├── decy-frontend/          # C       (extensions: c, h)
└── ruchy-frontend/         # Ruchy   (extensions: ruchy)
```

Dependency direction is strictly downward. New frontends are added at the bottom; nothing above them changes.

## Quality regime

xpile uses [`provable-contracts`](https://github.com/paiml/provable-contracts) (`pv` CLI) as the design controller and [`pmat`](https://github.com/paiml/paiml-mcp-agent-toolkit) as the work controller. YAML contracts under `contracts/` are canonical; Rust trait stubs, property tests, Kani harnesses, Lean 4 theorems, mdBook pages, and README claims are *generated from* them.

CI-enforced quality gates (see [§18 CI Pipeline](docs/specifications/sub/ci-gates.md)):

| Gate | Tool | Threshold |
|---|---|---|
| PMAT TDG grade | `pmat tdg` | ≥ A- |
| Line coverage | `cargo llvm-cov` | ≥ 95% |
| Mutation coverage | `cargo mutants` | ≥ 80% on changed code |
| Provable-contracts | `pv lint` | 8/8 gates pass |
| Contract score | `pv score` | no regression |
| Clippy | `cargo clippy -- -D warnings` | zero warnings |
| Security advisories | `cargo deny check` | zero unyanked |

## Contracts at v0.1.0

Four example contracts under `contracts/`, all passing `pv lint` 8/8:

| Contract | `pv` kind | xpile layer | What it pins down |
|---|---|---|---|
| `xpile-frontend-trait-v1.yaml` | pattern | Layer 3 (architectural) | Frontend trait invariants |
| `py-int-arith-v1.yaml` | kernel | Layer 1 (language semantics) | Python `int` arithmetic with bigint promotion |
| `xlate-py-list-to-vec-v1.yaml` | kernel | Layer 2 (translation) | Python list → Rust Vec, alias-preserving |
| `ffi-cpython-ext-v1.yaml` | pattern | Layer 4 (hybrid) | CPython C-extension boundary semantics |

Current `pv lint` output:

```
  Gate 1: validate             ✓  (4 contracts, 0 errors, 2 warnings)
  Gate 2: audit                ✓  (4 contracts, 0 findings)
  Gate 3: score                ✓  (4 contracts, mean=0.58, threshold=0.00)
  Gate 4: verify               ✓
  Gate 5: enforce              ✓  (10 eqs, 4 pre, 0 post)
  Gate 6: enforcement-level    ✓
  Gate 7: reverse-coverage     ⏭  (no --binding provided yet)
  Gate 8: composition          ✓
Result: PASS
```

## Roadmap

Seven phases (full detail in [§21 Phased Rollout](docs/specifications/sub/phased-rollout.md)):

- [x] **Phase 0** — Scaffold + `pv` wiring
- [ ] **Phase 1** — Architectural contracts (6 Layer-3 contracts enforced)
- [ ] **Phase 2** — Python semantics starter set (5 Layer-1 kernel contracts)
- [ ] **Phase 3** — Codegen replacement (≥3 Python constructs via generated codegen)
- [ ] **Phase 4** — Kani equivalence proofs (all arithmetic Kani-green)
- [ ] **Phase 5** — Hybrid pipeline demo (NumPy-using `.py` + companion `.c`)
- [ ] **Phase 6** — Lean theorems (≥3 closed on math-dense contracts)

## Family

| Repo | Role |
|---|---|
| `paiml/xpile` (this) | Polyglot transpile workbench |
| `paiml/aprender` | ML framework; source of `provable-contracts` |
| `paiml/provable-contracts` | Canonical `pv-spec.md` |
| `paiml/depyler` | Python→Rust transpiler (→ folds into xpile per [§19](docs/specifications/sub/migration.md)) |
| `paiml/decy` | C→Rust transpiler (→ folds in) |
| `paiml/ruchy` | Modern data science language; xpile's third frontend |
| `paiml/paiml-mcp-agent-toolkit` | `pmat` |
| `pymc-labs/alchemize` | Source of the four-tool agent loop pattern |

## License

MIT OR Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.
