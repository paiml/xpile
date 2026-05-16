# xpile — Polyglot Transpile Workbench Specification v0.1.0

**A monorepo for hybrid transpilation: Python, C, Ruchy (and future C++, CUDA, ...) → Rust, with shared agent loop, oracle, codegen, and verification.**

**Canonical spec.** This is the ONE spec. All other specs are sub-specs under `sub/`, linked from the table of contents. Anything in `legacy/` is archived and not authoritative. Drift between this spec and the code, contracts, or sub-specs is a contract defect — fail it in CI.

**Status:** v0.1.0 — **transpiles end-to-end with semantic round-trip verification**. 24 workspace crates compile clean; `aprender-contracts` (the `pv` library) wired from crates.io 0.33; 11 contracts pass `pv lint`; three real backends (Rust, Ruchy, Lean 4); recursive Python (`factorial(10) == 3628800`, etc.) runs correctly through CI. See [Section 23 — Status](#23-status) and `CHANGELOG.md`.

**Foundations:**

- **Pluggable frontends** (depyler/decy/ruchy/...) implementing one [`Frontend` trait](sub/frontend-trait.md)
- **Canonical meta-HIR** that every frontend lowers to ([`xpile-meta-hir`](sub/meta-hir.md))
- **FFI manifest** that makes hybrid transpilation tractable ([`xpile-ffi-manifest`](sub/ffi-manifest.md))
- **Bounded agent loop** adapted from [alchemize](https://github.com/pymc-labs/alchemize) ([`xpile-agent`](sub/agent-loop.md))
- **Oracle pattern** — capture original-language execution, validate Rust equivalence ([`xpile-oracle`](sub/oracle.md))
- **Provable contracts via [pv](https://crates.io/crates/aprender-contracts)** — design is YAML; spec/stubs/tests/proofs are generated ([Section 11](#11-provable-contracts-pv-integration))
- **Quality regime via [pmat](https://github.com/paiml/paiml-mcp-agent-toolkit)** — work items, kaizen loop, PMAT TDG grading ([Section 12](#12-quality-regime-pmat))

---

## Table of Contents

| # | Section | Sub-spec |
|---|---------|----------|
| 1 | [Vision and Architecture](#1-vision-and-architecture) | [sub/vision.md](sub/vision.md) |
| 2 | [Polyglot Frontend Trait](#2-polyglot-frontend-trait) | [sub/frontend-trait.md](sub/frontend-trait.md) |
| 2b | [Contract Frontend Trait (proof lane)](#2b-contract-frontend-trait-proof-lane) | [sub/contract-frontend-trait.md](sub/contract-frontend-trait.md) |
| 3 | [Canonical Meta-HIR](#3-canonical-meta-hir) | [sub/meta-hir.md](sub/meta-hir.md) |
| 4 | [FFI Manifest](#4-ffi-manifest) | [sub/ffi-manifest.md](sub/ffi-manifest.md) |
| 5 | [Rust Codegen Backend](#5-rust-codegen-backend) | [sub/rust-codegen.md](sub/rust-codegen.md) |
| 5b | [Polyglot Backend Trait](#5b-polyglot-backend-trait) | [sub/backend-trait.md](sub/backend-trait.md) |
| 5c | [Contract Backend Trait (proof lane)](#5c-contract-backend-trait-proof-lane) | [sub/contract-backend-trait.md](sub/contract-backend-trait.md) |
| 6 | [Oracle and Hybrid Validation](#6-oracle-and-hybrid-validation) | [sub/oracle.md](sub/oracle.md) |
| 7 | [Bounded Agent Repair Loop](#7-bounded-agent-repair-loop) | [sub/agent-loop.md](sub/agent-loop.md) |
| 8 | [Cache, Determinism, Provenance](#8-cache-determinism-provenance) | [sub/cache-determinism-provenance.md](sub/cache-determinism-provenance.md) |
| 9 | [Budget Discipline](#9-budget-discipline) | [sub/budget.md](sub/budget.md) |
| 10 | [Skills System](#10-skills-system) | [sub/skills.md](sub/skills.md) |
| 11 | [Provable Contracts (`pv`) Integration](#11-provable-contracts-pv-integration) | [sub/pv-integration.md](sub/pv-integration.md) |
| 12 | [Quality Regime (`pmat`)](#12-quality-regime-pmat) | [sub/pmat-integration.md](sub/pmat-integration.md) |
| 13 | [Contract Taxonomy](#13-contract-taxonomy) | [sub/contract-taxonomy.md](sub/contract-taxonomy.md) |
| 14 | [CLI Reference (`xpile`)](#14-cli-reference-xpile) | [sub/cli.md](sub/cli.md) |
| 15 | [MCP Server](#15-mcp-server) | [sub/mcp.md](sub/mcp.md) |
| 16 | [Hybrid Transpile Flow](#16-hybrid-transpile-flow) | [sub/hybrid-transpile-flow.md](sub/hybrid-transpile-flow.md) |
| 17 | [Frontend Onboarding](#17-frontend-onboarding) | [sub/frontend-onboarding.md](sub/frontend-onboarding.md) |
| 18 | [CI Pipeline and Gates](#18-ci-pipeline-and-gates) | [sub/ci-gates.md](sub/ci-gates.md) |
| 19 | [Migration from depyler / decy](#19-migration-from-depyler--decy) | [sub/migration.md](sub/migration.md) |
| 20 | [Kaizen Fleet Membership](#20-kaizen-fleet-membership) | [sub/kaizen-fleet.md](sub/kaizen-fleet.md) |
| 21 | [Phased Rollout](#21-phased-rollout) | [sub/phased-rollout.md](sub/phased-rollout.md) |
| 22 | [Glossary](#22-glossary) | [sub/glossary.md](sub/glossary.md) |
| 23 | [Status](#23-status) | [docs/status/CURRENT.md](../status/CURRENT.md) |
| 24 | [Lean 4 Bidirectional Integration](#24-lean-4-bidirectional-integration) | [sub/lean-bidirectional.md](sub/lean-bidirectional.md) |
| 25 | [LaTeX Bidirectional Integration](#25-latex-bidirectional-integration) | [sub/latex-bidirectional.md](sub/latex-bidirectional.md) |

---

## 1. Vision and Architecture

**Sub-spec**: [sub/vision.md](sub/vision.md)

xpile is a polyglot transpile workbench. Every supported source language plugs in by implementing one `Frontend` trait; everything below it — meta-HIR, oracle protocol, agent loop, MCP, codegen, contracts — is shared. The load-bearing motivation is **hybrid transpilation**: single artifacts that cross language boundaries (CPython + C extensions, Python + CUDA kernels, Python + Ruchy data layer) that no per-language transpiler can handle alone.

The repo is a Cargo workspace of 14 crates. Front-ends are language-specific leaves (`depyler-frontend`, `decy-frontend`, `ruchy-frontend`); shared crates (`xpile-core`, `xpile-agent`, `xpile-oracle`, `xpile-llm`, `xpile-mcp`, `xpile-contracts`, `xpile-rust-codegen`, `xpile-meta-hir`, `xpile-ffi-manifest`, `xpile-frontend`) cover the rest. Foundations: alchemize's four-tool agent loop, aprender's provable-contracts framework, depyler's repair-mode pattern, decy's HIR/ownership patterns.

---

## 2. Polyglot Frontend Trait

**Sub-spec**: [sub/frontend-trait.md](sub/frontend-trait.md)

The `Frontend` trait is the only language-specific abstraction in xpile's **code lane**. Three methods: `name()`, `extensions()`, `parse_and_lower()`. A new language is implemented by writing one type that implements this trait — no other architecture changes. Invariants codified in [`contracts/xpile-frontend-trait-v1.yaml`](../../contracts/xpile-frontend-trait-v1.yaml): extension ownership uniqueness, parse idempotency, source_lang consistency, outgoing-only FFI boundary recording.

Implementations at v0.1.0: `PythonFrontend` (extensions: `py`, `pyi`), `CFrontend` (`c`, `h`), `RuchyFrontend` (`ruchy`). All are scaffold-stage placeholders that return empty modules; real parser integration is Phase 2 of the rollout. Lean 4 (`LeanFrontend`) is planned and spans both lanes — see §2b.

---

## 2b. Contract Frontend Trait (proof lane)

**Sub-spec**: [sub/contract-frontend-trait.md](sub/contract-frontend-trait.md)

xpile has two parallel pipelines that share the contract substrate. The **code lane** (Frontend → meta-HIR → Backend) models executable code. The **proof lane** (ContractFrontend → contract equations → ContractBackend) models proofs and mathematical notation — LaTeX math, Lean theorem text, mdBook. The `ContractFrontend` trait is the entry point to the proof lane: three methods `name()`, `formats()`, `parse_to_equations()`. Invariants codified in [`contracts/xpile-contract-frontend-trait-v1.yaml`](../../contracts/xpile-contract-frontend-trait-v1.yaml): format ownership, parse idempotency, equations-only (no meta-HIR pollution), citation preservation.

Planned implementations: `LatexContractFrontend` (math mode + theorem/proof/lemma environments per decision #2), `LeanContractFrontend` (read-only theorem extraction, Lean 4 only per decision #1), `MdBookContractFrontend` (vendored from `pv`). Lean has dual citizenship: a `.lean` file is parsed by `LeanFrontend` for executable code AND `LeanContractFrontend` for theorem statements, in disjoint passes.

---

## 3. Canonical Meta-HIR

**Sub-spec**: [sub/meta-hir.md](sub/meta-hir.md)

`xpile-meta-hir` is the shared intermediate representation every frontend lowers to. Intentionally minimal at v0.1.0: `Module`, `SourceLang` enum (Python, C, Cpp, Cuda, Ruchy), `Item::Function`, and `FfiBoundary`. The architecture is **federated**: each frontend keeps its own internal HIR (e.g., `depyler-hir`, `decy-hir`) and lowers to meta-HIR only when crossing into shared infrastructure (codegen, FFI manifest, oracle).

Federated > unified because we don't yet have hybrid demos to validate the right shape of a richer meta-IR; over-designing now would lock in mistakes. Meta-HIR grows as hybrid-transpile cases demand.

---

## 4. FFI Manifest

**Sub-spec**: [sub/ffi-manifest.md](sub/ffi-manifest.md)

`xpile-ffi-manifest` is the source of truth for cross-language calls in a hybrid transpile session. Each entry maps a source-language symbol to its target Rust shim: `(symbol, from_lang, to_lang, source_signature, rust_shim_signature, shim_id)`. The manifest is what makes Python+C, Python+CUDA, etc., tractable — both transpilers operate independently but agree on the boundary because both consume the same manifest.

Contract: [`contracts/ffi-cpython-ext-v1.yaml`](../../contracts/ffi-cpython-ext-v1.yaml) governs the end-to-end behavior, including refcount balance, GIL invariance, and buffer-protocol zero-copy passthrough.

---

## 5. Rust Codegen Backend

**Sub-spec**: [sub/rust-codegen.md](sub/rust-codegen.md)

`xpile-rust-codegen` is the **concrete Rust backend** — one of several `Backend` impls in xpile (see §5b for the abstraction). It takes meta-HIR as input and emits idiomatic Rust. Language-neutral by design: language-specific quirks (Python int promotion, C pointer arithmetic, Ruchy pipeline operator) are normalized in each frontend before reaching codegen. Generated code carries a provenance marker on the first line.

At v0.1.0 codegen is a one-function stub. Real emission is driven by per-language translation contracts (e.g., [`contracts/xlate-py-list-to-vec-v1.yaml`](../../contracts/xlate-py-list-to-vec-v1.yaml) — homogeneous list to `Vec<T>`, alias-preserving, no silent `Vec<Box<dyn Any>>`).

---

## 5b. Polyglot Backend Trait

**Sub-spec**: [sub/backend-trait.md](sub/backend-trait.md)

The `Backend` trait is the **code lane's emission abstraction**, parallel to `Frontend`. Three methods: `name()`, `targets()`, `lower()`. Implementations cover the full target matrix: `RustBackend` (scaffolded), `RuchyBackend`, `PtxBackend`, `WgslBackend`, `SpirvBackend`, `LeanBackend` (all planned). The trait makes Rust→CUDA/PTX/shaders, Rust→Ruchy, and Rust→Lean tractable without architecture changes — each new target is a new `Backend` impl.

A `BackendConfig` carries `Target`, `Profile` (the two-mHIR decision for asymmetric Rust↔Ruchy), and an optional `HwProfile` for hardware-sensitive targets (PTX `compute_capability`, WGSL feature set). Layer-5 compile contracts (see §13) pin invariants about emitted IR text. Architectural invariants TBD in `contracts/xpile-backend-trait-v1.yaml` (pending).

---

## 5c. Contract Backend Trait (proof lane)

**Sub-spec**: [sub/contract-backend-trait.md](sub/contract-backend-trait.md)

The `ContractBackend` trait is the proof lane's emission abstraction — symmetric with `ContractFrontend` (§2b). Three methods: `name()`, `formats()`, `render()`. Renders a parsed contract into LaTeX (publication-quality math + theorem environments), Lean 4 theorem text (with `@[xpile_contract]` attribute carrying the verbatim contract ID per decision #4), or mdBook. Invariants codified in [`contracts/xpile-contract-backend-trait-v1.yaml`](../../contracts/xpile-contract-backend-trait-v1.yaml): format ownership, render idempotency, citation round-trip, `citation_via_structured_attribute` (the citation bridge — citations live in format-native structured constructs parseable by the host elaborator/parser, not by regex; revised post-audit), falsification-render-optional.

A single `.lean` file may carry both lanes' output: code-lane Lean from `LeanBackend` (executable `def`) plus proof-lane Lean theorems from `LeanContractBackend`, separated by `section`/`end` markers. `TranspileSession` orchestrates the merge.

---

## 6. Oracle and Hybrid Validation

**Sub-spec**: [sub/oracle.md](sub/oracle.md)

`xpile-oracle` runs the *original* source (CPython, gcc-compiled C, the ruchy interpreter) on an input fixture, captures outputs, and compares them against the transpiled Rust output. This is the semantic gate the agent must pass to exit successfully.

The pattern is borrowed from alchemize: extract reference values *before* the agent runs, then validate against them — stronger than property-based tests because the equivalence claim is over *the actual program's behavior*, not random inputs. Per-type equality predicates: bitwise for ints, exact for strings, configurable tolerance for floats, structural for collections, refcount-balance for FFI.

---

## 7. Bounded Agent Repair Loop

**Sub-spec**: [sub/agent-loop.md](sub/agent-loop.md)

`xpile-agent` adapts [alchemize](https://github.com/pymc-labs/alchemize)'s four-tool loop. Tools exposed to the LLM agent: `read_file`, `write_file_in_lang`, `cargo_build`, `cargo_test`, `run_hybrid_oracle`, `apply_skill`. Exit condition: `cargo_build && run_hybrid_oracle` both pass. Failure mode: budget exhaustion, fails closed to the original static error — never partial or speculative Rust.

Default-off: the agent is opt-in via `xpile transpile --repair`. The static path never invokes the LLM. This preserves determinism-by-default per [`contracts/repair-determinism-v1.yaml`](../../contracts/repair-determinism-v1.yaml) in depyler (the same contract applies here once ported).

---

## 8. Cache, Determinism, Provenance

**Sub-spec**: [sub/cache-determinism-provenance.md](sub/cache-determinism-provenance.md)

Content-addressed cache keyed by `sha256(source || xpile_version || model_id || skills_hash)`. On cache hit, returns byte-identical bytes — across runs, machines, and team members. The cache is what converts a stochastic agent into a reproducible artifact pipeline.

Every repaired `.rs` carries a provenance marker as the first line: `// xpile-repaired: <hash> via <model_id> at <utc>`. The marker is a *receipt* into the cache. Static-pass files never carry the marker. Confusing the two would let stochastic output masquerade as deterministic.

---

## 9. Budget Discipline

**Sub-spec**: [sub/budget.md](sub/budget.md)

Per-file caps on the agent loop: max 8 iterations, max 200K tokens, max 300s wall-clock (configurable). Budget exhaustion is non-negotiable: the in-flight call completes, then the loop exits with the *original static error*. No partial Rust ever ships from a budget-exhausted session — that would be the worst-of-both-worlds output (broken code with a "ran out of time" comment).

Per-file budgets prevent denial-of-wallet failure modes and bound CI cost. Aggregate fleet-level budgets are in [Section 20 — Kaizen Fleet Membership](#20-kaizen-fleet-membership).

---

## 10. Skills System

**Sub-spec**: [sub/skills.md](sub/skills.md)

`crates/xpile-agent/skills/` holds markdown skills — short, prescriptive snippets the agent loads when it hits a recurring failure idiom (e.g., `lifetimes.md`, `generators.md`, `cuda_kernel_launch.md`). Skills are tagged by `lang:` and `boundary:` in frontmatter and pulled in via the `apply_skill(name)` tool.

**Skills are a holding pen, not a permanent backstop.** A skill firing ≥50 times across ≥10 files in a quarter is a graduation candidate: the team lifts it into a deterministic rule in `xpile-rust-codegen` and *deletes* the skill markdown in the same PR. The success signal for the agent loop is repair-invocation rate trending *down* per corpus over time. See [`contracts/skill-graduation-v1.yaml`](../../contracts/skill-graduation-v1.yaml) (ported from depyler).

---

## 11. Provable Contracts (`pv`) Integration

**Sub-spec**: [sub/pv-integration.md](sub/pv-integration.md)

xpile delegates its entire contract framework to [`provable-contracts`](https://github.com/paiml/provable-contracts) — the upstream `pv` CLI and Rust library. YAML contracts under `contracts/` are canonical; Rust stubs, property tests, Kani harnesses, Lean theorems, mdBook pages, and README quality claims are *generated from* them via `pv scaffold`, `pv probar`, `pv kani`, `pv lean`, `pv book-gen`, `pv readme-gen`.

The xpile-contracts crate is a thin re-export of `provable_contracts` plus an `XpileContractLayer` metadata enum tagging contracts by taxonomy layer ([Section 13](#13-contract-taxonomy)). At v0.1.0, all 4 xpile contracts pass `pv lint` 8/8 gates with `mean=0.58` score.

---

## 12. Quality Regime (`pmat`)

**Sub-spec**: [sub/pmat-integration.md](sub/pmat-integration.md)

[`pmat`](https://github.com/paiml/paiml-mcp-agent-toolkit) is the work controller across the fleet. Every phase of the xpile rollout is a pmat work item with its spec in `docs/specifications/` (this directory). Quality gates enforced in CI: `pmat tdg` ≥ A-, `cargo llvm-cov` ≥ 95%, `cargo mutants` ≥ 80%, `pv lint` 8/8 gates, zero clippy warnings, `cargo deny check` clean.

The `kaizen-paiml` skill drives the continuous-improvement loop: pick a pmat work item, implement, run gates, mark done, move on. xpile is repo #41 in the fleet once we register per [Section 20](#20-kaizen-fleet-membership).

---

## 13. Contract Taxonomy

**Sub-spec**: [sub/contract-taxonomy.md](sub/contract-taxonomy.md)

xpile uses pv's existing taxonomy. Two real `kind` values are used; the four-layer organization is metadata-only (not enforced by `pv`):

| `pv` kind | xpile layer | Examples |
|---|---|---|
| `kernel` | Language semantics (Layer 1), Translation (Layer 2) | `py-int-arith-v1.yaml`, `xlate-py-list-to-vec-v1.yaml` |
| `pattern` | Architectural (Layer 3), Hybrid pipeline (Layer 4) | `xpile-frontend-trait-v1.yaml`, `ffi-cpython-ext-v1.yaml` |

Kernel contracts MUST have non-empty `proof_obligations`, `falsification_tests`, AND `kani_harnesses`, with `falsification_tests.len() ≥ proof_obligations.len()`. Pattern contracts have lighter requirements.

---

## 14. CLI Reference (`xpile`)

**Sub-spec**: [sub/cli.md](sub/cli.md)

```bash
xpile transpile foo.py                        # static path
xpile transpile foo.py --repair               # static → if fail, agent loop
xpile transpile foo.py --repair=cached        # cache hit required; never call model
xpile transpile foo.py --repair=force         # bypass cache; always re-run agent
xpile transpile --hybrid foo_module/          # multi-language session
xpile lint                                    # delegates to `pv lint`
xpile score                                   # delegates to `pv score`
xpile mcp                                     # launch MCP server (see Section 15)
```

Budget overrides: `--repair-max-iterations=N`, `--repair-max-tokens=N`, `--repair-max-seconds=N`. Default budgets in [Section 9](#9-budget-discipline).

---

## 15. MCP Server

**Sub-spec**: [sub/mcp.md](sub/mcp.md)

`xpile-mcp` exposes xpile's transpile / repair / inspect tools as MCP (Model Context Protocol) endpoints, callable from Claude Code, Claude Desktop, VS Code, and other IDE assistants. Mirrors the pattern in `depyler-mcp` and `decy-mcp`. Six initial tools: `transpile_file`, `transpile_hybrid`, `inspect_meta_hir`, `inspect_ffi_manifest`, `lint_contracts` (delegates to `pv lint`), `score_contracts` (delegates to `pv score`).

---

## 16. Hybrid Transpile Flow

**Sub-spec**: [sub/hybrid-transpile-flow.md](sub/hybrid-transpile-flow.md)

The load-bearing flow that no per-language transpiler can do alone:

```text
$ xpile transpile --hybrid foo_module/
  1. Each frontend lowers its file → meta-HIR Module
  2. xpile-ffi-manifest reconciles cross-language calls
  3. xpile-rust-codegen emits Rust on both sides + FFI shim
  4. xpile-oracle captures end-to-end CPython behavior on a fixture
  5. Validates Rust matches CPython on every fixture input
  6. If fails → xpile-agent loops with cargo + oracle errors
```

First target: CPython C extension (NumPy-using `.py` + companion `.c`). Governed by [`contracts/ffi-cpython-ext-v1.yaml`](../../contracts/ffi-cpython-ext-v1.yaml).

---

## 17. Frontend Onboarding

**Sub-spec**: [sub/frontend-onboarding.md](sub/frontend-onboarding.md)

Adding a new source language is a 2-4 week effort, not a fork-the-architecture exercise:

1. Add a variant to `xpile_meta_hir::SourceLang`
2. Create `crates/<lang>-frontend/` with one type implementing `xpile_frontend::Frontend`
3. Wire its parse/lower (often by adopting an existing parser crate)
4. Write a Layer-1 semantics contract for one core construct
5. Write a Layer-2 translation contract for the same construct → Rust
6. Add a corpus regression test
7. Register the frontend in `xpile-core` via `TranspileSession::register_frontend`

Ruchy is the canary for this onboarding path at v0.1.0.

---

## 18. CI Pipeline and Gates

**Sub-spec**: [sub/ci-gates.md](sub/ci-gates.md)

Every PR runs:

```text
1. cargo fmt --check
2. cargo clippy --workspace -- -D warnings
3. cargo check --workspace
4. cargo test --workspace
5. cargo llvm-cov  (≥95% line coverage)
6. cargo mutants   (≥80% mutation coverage on changed code)
7. cargo deny check
8. pv lint contracts/  (8/8 gates pass)
9. pv score contracts/ (no score regression)
10. pmat tdg ≥ A-
11. Provenance check: no repaired .rs files in PR without provenance marker
12. (optional) cargo kani --workspace  (gated; bounded model checks)
```

Hard-failures on any gate. No `--no-verify`, no manual overrides.

---

## 19. Migration from depyler / decy

**Sub-spec**: [sub/migration.md](sub/migration.md)

Two-step migration over ~8 weeks: **extract first, merge second.**

1. **Extract (weeks 1-6):** Move shared concerns into the xpile workspace as crates.io-published crates. depyler and decy depend on them. Per-language repos shrink as functionality moves into xpile. xpile and per-language repos coexist.
2. **Merge (weeks 7-8):** `git filter-repo` + `git subtree add` to fold depyler and decy into xpile, preserving history. Per-language repos become thin shims that re-export from xpile.

The merge is the *implementation* of the monorepo; the extract phase already gives 80% of the benefit by deduplicating crates.

---

## 20. Kaizen Fleet Membership

**Sub-spec**: [sub/kaizen-fleet.md](sub/kaizen-fleet.md)

Per [pv-spec §31](../../../provable-contracts/docs/specifications/sub/kaizen-fleet-enforcement.md), the fleet is at Grade A across 40 repos, 294 contracts, 1025 Lean theorems, 20,110 assertions. xpile becomes repo #41 once first contracts pass `pv lint` (✅ done at v0.1.0) and `pv kaizen --register xpile` is run.

Fleet-level quarterly rollups via `pv score` aggregate xpile's grade alongside the others. xpile contracts contribute to fleet-level audit-chain integrity.

---

## 21. Phased Rollout

**Sub-spec**: [sub/phased-rollout.md](sub/phased-rollout.md)

Seven phases, each tracked as a pmat work item with spec in `docs/specifications/`:

| Phase | Scope | Exit criterion |
|---|---|---|
| 0 | Dep wiring + scaffold | `cargo check` clean, `pv lint` 8/8 ✅ |
| 1 | Architectural contracts enforced | 6 Layer-3 contracts in `enforced` status |
| 2 | Python semantics starter set | 5 Layer-1 kernel contracts, Kani-passing |
| 3 | Codegen replacement | ≥3 Python constructs end-to-end via generated codegen |
| 4 | Kani equivalence proofs | All arith contracts Kani-green at default unwind |
| 5 | Hybrid pipeline demo | NumPy-using `.py` + companion `.c` oracle-passing |
| 6 | Lean theorems | ≥3 Lean theorems closed on math-dense contracts |

Phase 0 is done. Current status in [Section 23](#23-status).

---

## 22. Glossary

**Sub-spec**: [sub/glossary.md](sub/glossary.md)

Key terms: **meta-HIR**, **Frontend trait**, **FFI manifest**, **oracle**, **agent loop**, **skill graduation**, **provenance marker**, **kernel contract vs pattern contract**, **kaizen fleet**, **PVScore**, **PMAT TDG**.

---

## 23. Status

**Sub-spec**: [docs/status/CURRENT.md](../status/CURRENT.md). Live source of truth for the supported subset: `CHANGELOG.md`.

v0.1.0 — **end-to-end transpiler with semantic round-trip verification**:

- ✅ 24 workspace crates compile clean (`cargo check`, `cargo clippy -- -D warnings`)
- ✅ `aprender-contracts` (`pv`) wired via crates.io 0.33 (path-dep removed in PR #3 fix)
- ✅ 11 contracts pass `pv lint` (0 errors)
- ✅ Three real backends (Rust, Ruchy, Lean 4); PTX/WGSL/SPIR-V still scaffolded
- ✅ Python subset: typed `def`, multi-statement body, all binary + unary ops, ternary, if/elif/else, function calls including self-recursion
- ✅ Semantic round-trip verified for 5 fixtures (factorial, fib, gcd, abs_val, sign)
- ✅ CI gate enforced on PRs (fmt, check, clippy -D warnings, pv lint, cargo deny, workspace tests)
- ✅ Branch protection on `main`; crates.io reservation at `xpile 0.0.1`
- ⏳ Bigint promotion (`py-int-arith-v1.yaml` slow path) — fast-path
  overflow is now load-bearing (Rust + Ruchy emit
  `.checked_*().expect(...)`, contract name appears in the panic
  message); the slow path itself is still unimplemented
- ⏳ Loops, multi-assignment if-branches, types beyond int/bool
- ⏳ Real C frontend (decy-frontend currently stub)
- ⏳ Real PTX/WGSL emission (scaffolds in place)

The next-session pickup point is `docs/status/CURRENT.md` and any open pmat work items.

---

## 24. Lean 4 Bidirectional Integration

**Sub-spec**: [sub/lean-bidirectional.md](sub/lean-bidirectional.md)

Lean 4 is the only language that participates in both lanes. `.lean` files carry executable declarations (code lane via `lean-frontend` + `xpile-lean-codegen`) AND theorem declarations (proof lane via `lean-contract-frontend` + `xpile-lean-contract-backend`). `TranspileSession` orchestrates the merge — a single `.lean` output file can hold both halves separated by section markers. The citation bridge (`@[xpile_contract "C-X", xpile_equation "name"]`) is parsed by Lean's elaborator, not regex.

Lean 4 only — Lean 3 is end-of-life. All Lean executable constructs in scope (`def`, `partial def`, `inductive`, `structure`, `instance`, `axiom`, `noncomputable def`), with translation rules codified in [`contracts/xlate-lean-to-rust-v1.yaml`](../../contracts/xlate-lean-to-rust-v1.yaml). Theorem rendering is governed by [`contracts/xlate-rust-fn-to-lean-thm-v1.yaml`](../../contracts/xlate-rust-fn-to-lean-thm-v1.yaml).

---

## 25. LaTeX Bidirectional Integration

**Sub-spec**: [sub/latex-bidirectional.md](sub/latex-bidirectional.md)

LaTeX is proof-lane-only — it has no executable semantics. `latex-contract-frontend` parses math mode AND theorem-class environments (`theorem`, `lemma`, `corollary`, `proposition`, `definition`, `remark`, `proof`) into `EquationsBlock`. `xpile-latex-contract-backend` renders contracts as publication-quality LaTeX, suitable as the formal section of an arXiv paper.

Citation bridge: `\xpileContract{C-X}{equation_name}` macro expands to `\label{xpile:C-X:equation_name}` — indexed natively by `latexmk` / `biblatex`. The `xpile-contracts.sty` package is vendored as a sidecar artifact. Layer 2 contract: [`contracts/notation-latex-math-to-equation-v1.yaml`](../../contracts/notation-latex-math-to-equation-v1.yaml).

---

## License

MIT OR Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.

## Contributing

Open a draft PR with: (a) a contract YAML if you're adding a new construct; (b) `pv lint` 8/8 passing; (c) the linked pmat work item ID. No bare code changes without a corresponding contract or pmat item.
