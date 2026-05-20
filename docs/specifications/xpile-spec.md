# xpile — Polyglot Transpile Workbench Specification v0.1.0

**A monorepo for hybrid transpilation: Python, C, Ruchy (and future C++, CUDA, ...) → Rust, with shared agent loop, oracle, codegen, and verification.**

**Canonical spec.** This is the ONE spec. All other specs are sub-specs under `sub/`, linked from the table of contents. Anything in `legacy/` is archived and not authoritative. Drift between this spec and the code, contracts, or sub-specs is a contract defect — fail it in CI.

**Status:** v0.1.0 — **transpiles end-to-end with semantic round-trip verification AND 100% §14.4 contract QUORUM coverage**. 27 workspace crates compile clean; `aprender-contracts` (the `pv` library) wired from crates.io 0.33; **12 contracts pass `pv lint` and all 12 reach §14.4 N-of-M QUORUM** via paired Lean refinement theorems + Kani BMC harnesses (Bronze tier); four real backends (Rust, Ruchy, Lean 4, Shell/bashrs); recursive Python (`factorial(10) == 3628800`, etc.) AND iterative Python (`sum_to(100) == 5050`, `factorial_iter(10) == 3628800`) both run correctly through CI. See [Section 23 — Status](#23-status) and `CHANGELOG.md`.

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
| 19 | [Migration from depyler / decy / bashrs](#19-migration-from-depyler--decy--bashrs) | [sub/migration.md](sub/migration.md), [sub/bashrs-merger.md](sub/bashrs-merger.md) |
| 20 | [Kaizen Fleet Membership](#20-kaizen-fleet-membership) | [sub/kaizen-fleet.md](sub/kaizen-fleet.md) |
| 21 | [Phased Rollout](#21-phased-rollout) | [sub/phased-rollout.md](sub/phased-rollout.md) |
| 22 | [Glossary](#22-glossary) | [sub/glossary.md](sub/glossary.md) |
| 23 | [Status](#23-status) | [docs/status/CURRENT.md](../status/CURRENT.md) |
| 24 | [Lean 4 Bidirectional Integration](#24-lean-4-bidirectional-integration) | [sub/lean-bidirectional.md](sub/lean-bidirectional.md) |
| 25 | [LaTeX Bidirectional Integration](#25-latex-bidirectional-integration) | [sub/latex-bidirectional.md](sub/latex-bidirectional.md) |
| 26 | [Audit-acknowledged Caveats](#26-audit-acknowledged-caveats) | [audit-design.md](audit-design.md) |
| 27 | [Provability Roadmap — ruchy 5.0 alignment](#27-provability-roadmap--ruchy-50-alignment) | [sub/provability-roadmap.md](sub/provability-roadmap.md) |
| 28 | [Diamond-Tier Refinement Taxonomy](#28-diamond-tier-refinement-taxonomy) | [sub/diamond-taxonomy.md](sub/diamond-taxonomy.md) |
| 29 | [Layer-5 Multi-Emitter Oracle Quorum](#29-layer-5-multi-emitter-oracle-quorum) | [sub/layer5-multi-emitter-quorum.md](sub/layer5-multi-emitter-quorum.md) |

---

## 1. Vision and Architecture

**Sub-spec**: [sub/vision.md](sub/vision.md)

xpile is a polyglot transpile workbench. Every supported source language plugs in by implementing one `Frontend` trait; everything below it — meta-HIR, oracle protocol, agent loop, MCP, codegen, contracts — is shared. The load-bearing motivation is **hybrid transpilation**: single artifacts that cross language boundaries (CPython + C extensions, Python + CUDA kernels, Python + Ruchy data layer) that no per-language transpiler can handle alone.

The repo is a Cargo workspace of 27 crates (as of v0.1.0). Front-ends are language-specific leaves (`depyler-frontend`, `decy-frontend`, `ruchy-frontend`, `bashrs-frontend`, `latex-contract-frontend`); shared crates (`xpile-core`, `xpile-agent`, `xpile-oracle`, `xpile-llm`, `xpile-mcp`, `xpile-contracts`, `xpile-rust-codegen`, `xpile-ruchy-codegen`, `xpile-lean-codegen`, `xpile-ptx-codegen`, `xpile-wgsl-codegen`, `bashrs-backend`, `xpile-meta-hir`, `xpile-ffi-manifest`, `xpile-bigint`, `xpile-frontend`, `xpile-backend`, and the proof-lane equivalents) cover the rest. Foundations: alchemize's four-tool agent loop, aprender's provable-contracts framework, depyler's repair-mode pattern, decy's HIR/ownership patterns, bashrs's POSIX-deterministic shell pattern.

**Scope is deliberately scoped, not universal.** The supported language set is **one tier, not two** (the previous "native vs federated" split was reversed on 2026-05-17 — see [sub/bashrs-merger.md](sub/bashrs-merger.md)):

- **Native frontends/backends inside xpile** — Python, C, C++, CUDA, Rust, Ruchy, Lean 4, **bash, zsh, POSIX sh, Makefile, Dockerfile**. These cover the seed hybrid-transpile cases that motivated the architecture (CPython↔C extensions, Python↔CUDA, Rust↔Ruchy) plus the shell domain absorbed from bashrs. All lower to meta-HIR (which is expanding to represent shell semantics — see §3) and share one Oracle + agent-loop + contract substrate.

Languages outside this set — Julia, R, JNI (Java/Kotlin), JavaScript/TypeScript — are *not* on the roadmap. The adversarial audit ([§4 of `audit-design.md`](audit-design.md)) characterises this as a "Sovereign AI" stance inherited from the broader `aprender` ecosystem; it is acknowledged as a tradeoff, not papered over. Wasm reaches the workbench indirectly via `ruchy`'s `WasmEmitter`, not via a native xpile Wasm backend.

---

## 2. Polyglot Frontend Trait

**Sub-spec**: [sub/frontend-trait.md](sub/frontend-trait.md)

The `Frontend` trait is the only language-specific abstraction in xpile's **code lane**. Three methods: `name()`, `extensions()`, `parse_and_lower()`. A new language is implemented by writing one type that implements this trait — no other architecture changes. Invariants codified in [`contracts/xpile-frontend-trait-v1.yaml`](../../contracts/xpile-frontend-trait-v1.yaml): extension ownership uniqueness, parse idempotency, source_lang consistency, outgoing-only FFI boundary recording.

Implementations at v0.1.0:

- `PythonFrontend` (extensions: `py`, `pyi`) — **real implementation**. Parses with `rustpython-parser` and lowers the live subset described in [`/CHANGELOG.md`](../../CHANGELOG.md) (typed `def`, all binary + unary ops including bitwise + power, ternary, if/elif/else with single- *or multi-*assignment branches, function calls, while loops with mutable rebinding, `for target in range(...)` with positive *or negative* integer-literal step).
- `CFrontend` (`c`, `h`), `RuchyFrontend` (`ruchy`) — scaffold-stage placeholders that return empty modules; real parser integration is Phase 2 of the rollout.
- Lean 4 (`LeanFrontend`) — planned, spans both lanes — see §2b.

---

## 2b. Contract Frontend Trait (proof lane)

**Sub-spec**: [sub/contract-frontend-trait.md](sub/contract-frontend-trait.md)

xpile has two parallel pipelines that share the contract substrate. The **code lane** (Frontend → meta-HIR → Backend) models executable code. The **proof lane** (ContractFrontend → contract equations → ContractBackend) models proofs and mathematical notation — LaTeX math, Lean theorem text, mdBook. The `ContractFrontend` trait is the entry point to the proof lane: three methods `name()`, `formats()`, `parse_to_equations()`. Invariants codified in [`contracts/xpile-contract-frontend-trait-v1.yaml`](../../contracts/xpile-contract-frontend-trait-v1.yaml): format ownership, parse idempotency, equations-only (no meta-HIR pollution), citation preservation.

Planned implementations: `LatexContractFrontend` (math mode + theorem/proof/lemma environments per decision #2), `LeanContractFrontend` (read-only theorem extraction, Lean 4 only per decision #1), `MdBookContractFrontend` (vendored from `pv`). Lean has dual citizenship: a `.lean` file is parsed by `LeanFrontend` for executable code AND `LeanContractFrontend` for theorem statements, in disjoint passes.

---

## 3. Canonical Meta-HIR

**Sub-spec**: [sub/meta-hir.md](sub/meta-hir.md)

`xpile-meta-hir` is the shared intermediate representation every frontend lowers to — **all four Sovereign AI Stack transpilers (`depyler`, `decy`, `ruchy`, `bashrs`) target the same IR post-merge** (see [sub/bashrs-merger.md](sub/bashrs-merger.md) Layer B). At v0.1.0 the IR is intentionally minimal: `Module`, `SourceLang` enum (Python, C, Cpp, Cuda, Ruchy, Rust, Lean), `Item::Function`, `FfiBoundary`, and a small `Type` lattice (`I64`, `Bool`, `BigInt`). The architecture is **federated**: each frontend keeps its own internal HIR (e.g., `depyler-hir`, `decy-hir`) and lowers to meta-HIR only when crossing into shared infrastructure (codegen, FFI manifest, oracle).

**v0.2.0 expansion** (per the bashrs merger) adds shell-domain variants: `Stmt::Cmd`, `Stmt::Pipeline`, `Stmt::ShellLoop`, `Expr::ShellVar`, `Expr::QuotedString`, `Expr::CommandSubstitution`, `Type::ShellString`, `Type::ExitCode`, plus `QuotingStrategy` and `LoopKind` enums. Backends that don't consume shell variants (Rust, Ruchy, Lean, PTX, WGSL, SPIR-V) return `Unsupported` for them at first — same pattern as Lean's `Stmt::While` between PMAT-006 and PMAT-010. The merger's check-back at v0.3.0 (see [sub/bashrs-merger.md](sub/bashrs-merger.md) "What's now load-bearing") falsifies the IR merger if no cross-domain consumer of shell variants materialises; reversal would be ticketed `XPILE-UNMERGE-001`.

Federated > unified because we don't yet have hybrid demos to validate the right shape of a richer meta-IR; over-designing now would lock in mistakes. Meta-HIR grows as hybrid-transpile cases demand.

---

## 4. FFI Manifest

**Sub-spec**: [sub/ffi-manifest.md](sub/ffi-manifest.md)

`xpile-ffi-manifest` is the source of truth for cross-language calls in a hybrid transpile session. Each entry maps a source-language symbol to its target Rust shim: `(symbol, from_lang, to_lang, source_signature, rust_shim_signature, shim_id)`. The manifest is what makes Python+C, Python+CUDA, etc., tractable — both transpilers operate independently but agree on the boundary because both consume the same manifest.

Contract: [`contracts/ffi-cpython-ext-v1.yaml`](../../contracts/ffi-cpython-ext-v1.yaml) governs the end-to-end behavior, including refcount balance, GIL invariance, and buffer-protocol zero-copy passthrough. The refcount-balance equation `C-FFI-CPYTHON-REFCOUNT` was hardened via a five-whys analysis on a memory-leak incident — see [audit-design.md §6](audit-design.md) for the full root-cause walk and the Kani harness that now enforces refcount invariance on the error path.

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

**Known scope limits** (see [§26](#26-audit-acknowledged-caveats) for the full caveat list): the Oracle is only as strong as the *user-supplied* fixtures — the agent does not synthesize them — and it is structurally blind to internal memory state and hardware-level races that don't surface through STDOUT/STDERR or the return value. Those gaps are closed at the contract layer (refcount balance via `C-FFI-CPYTHON-REFCOUNT`, hardware semantics via Layer-5 compile contracts), not the Oracle.

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

The xpile-contracts crate is a thin re-export of `provable_contracts` plus an `XpileContractLayer` metadata enum tagging contracts by taxonomy layer ([Section 13](#13-contract-taxonomy)). At v0.1.0 (post-substrate-completion PMAT-058..077, post-quality-sweep PMAT-127..138), all 12 xpile contracts pass `pv lint` 8/8 gates with **zero warnings substrate-wide** (every equation carries domain-grounded pre/postconditions; every equation is anchored to a Lean refinement theorem; every contract declares a `qa_gate`), and all 12 are at §14.4 N-of-M QUORUM via paired Lean refinement theorems + Kani BMC harnesses. The canonical mean is whatever `pv lint contracts/` reports on the current `main` branch.

---

## 12. Quality Regime (`pmat`)

**Sub-spec**: [sub/pmat-integration.md](sub/pmat-integration.md)

[`pmat`](https://github.com/paiml/paiml-mcp-agent-toolkit) is the work controller across the fleet. Every phase of the xpile rollout is a pmat work item with its spec in `docs/specifications/` (this directory). Live CI gates at v0.1.0 (per [sub/ci-gates.md](sub/ci-gates.md)): `pv lint` 8/8 gates with 0 errors, zero clippy warnings (`-D warnings`), `cargo deny check advisories` clean, `cargo test --workspace` green (includes `every_kani_harness_discharges` over all 43 BMC harnesses + the §14.4 stratum gates `refinement_proofs` / `kani_harnesses` / `kani_verify` / `quorum` / `attestations` / `qa_gate`), and the optional `kani` job verifying every harness symbolically. The originally-planned `pmat tdg ≥ A-`, `cargo llvm-cov ≥ 95%`, `cargo mutants ≥ 80%` gates are scheduled post-v0.1.0 under XPILE-CI-* tickets.

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

**How contracts get authored.** New contracts are not designed top-down; they arrive from the **five-whys → provable-contract** feedback loop. When the Oracle catches a divergence, or production surfaces a bug the agent loop missed (e.g., a memory leak that the Oracle's STDOUT/STDERR check can't see), the team walks the failure back to its root cause and codifies the missing guarantee as a contract YAML with `proof_obligations` + `kani_harnesses`. The [`C-FFI-CPYTHON-REFCOUNT`](../../contracts/ffi-cpython-ext-v1.yaml) case study in [audit-design.md §6](audit-design.md) is the canonical worked example of this cycle.

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

Every PR runs (live `.github/workflows/ci.yml` — see [sub/ci-gates.md](sub/ci-gates.md) for the full per-gate detail):

```text
gate job (required status check):
  1. cargo fmt --all -- --check
  2. cargo check --workspace
  3. cargo clippy --workspace --all-targets -- -D warnings
  4. pv lint contracts/                 (8/8 gates pass; 12 contracts, 0 errors)
  5. cargo deny check advisories

workspace-test job (required status check):
  6. cargo test --workspace
       — includes every_kani_harness_discharges (Kani BMC over all 43 harnesses)
       — includes the §14.4 stratum gates: refinement_proofs, kani_harnesses,
         kani_verify, quorum, attestations

kani job (optional status check, scheduled to flip required):
  7. cargo kani over each contracts/kani/*.rs harness
```

Hard-failures on any required gate. No `--no-verify`, no manual overrides.

Originally-planned but not yet wired (post-v0.1.0):

```text
   cargo llvm-cov ≥ 95% line coverage     (XPILE-CI-COVERAGE-001)
   cargo mutants ≥ 80% mutation coverage  (XPILE-CI-MUTANTS-001)
   pv score contracts/ no regression       (XPILE-CI-SCORE-001)
   pmat tdg ≥ A-                           (XPILE-CI-PMAT-TDG-001)
   scripts/check_provenance.sh             (XPILE-CI-PROVENANCE-001)
```

These were in the original v0.0.1 CI plan but were sequenced behind the substrate-completion work that just shipped. With 12 contracts at QUORUM, several of these become tractable for v0.2.0+. See [sub/ci-gates.md](sub/ci-gates.md) "Gates planned but not yet wired" for the Popperian falsification trace.

---

## 19. Migration from depyler / decy / bashrs

**Sub-spec**: [sub/migration.md](sub/migration.md). bashrs-specific merger detail: [sub/bashrs-merger.md](sub/bashrs-merger.md).

Two-step migration over ~8 weeks per transpiler: **extract first, merge second.**

1. **Extract (weeks 1-6):** Move shared concerns into the xpile workspace as crates.io-published crates. depyler / decy / ruchy / bashrs depend on them. Per-language repos shrink as functionality moves into xpile. xpile and per-language repos coexist.
2. **Merge (weeks 7-8):** `git filter-repo` + `git subtree add` to fold depyler / decy / ruchy / bashrs into xpile, preserving history. Per-language repos become thin shims that re-export from xpile.

The merge is the *implementation* of the monorepo; the extract phase already gives 80% of the benefit by deduplicating crates.

**All four Sovereign AI Stack transpilers follow the same plan.** depyler / decy / ruchy were the initial three; bashrs joins on the same terms after the 2026-05-17 reversal of the earlier federation plan. The reversal acknowledges that the cost of one extra workspace member (absorbed release cadence, larger CI surface) is outweighed by the value of one unified quality regime + one IR + one Oracle + one citation bridge. The IR-level merger (meta-HIR growing shell variants) is the load-bearing half of that decision; it has a v0.3.0 check-back falsifier — see [sub/bashrs-merger.md](sub/bashrs-merger.md) "What's now load-bearing".

`paiml/bashrs` becomes a re-export shim post-merge, exactly like `paiml/depyler` and `paiml/decy`. Existing `cargo install bashrs` invocations continue to work; the binary is xpile-internal.

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

- ✅ 27 workspace crates compile clean (`cargo check`, `cargo clippy -- -D warnings`)
- ✅ `aprender-contracts` (`pv`) wired via crates.io 0.33 (path-dep removed in PR #3 fix)
- ✅ 12 contracts pass `pv lint` (0 errors, 0 warnings — full clean state as of PMAT-138 closing XPILE-REFINE-005)
- ✅ **100% §14.4 N-of-M QUORUM coverage** — all 12 contracts have paired Lean refinement theorems (`contracts/lean/*.lean`) AND Kani BMC harnesses (`contracts/kani/*.rs`) at Bronze tier; **50 Lean theorems + 43 Kani harnesses** (post-XPILE-QUORUM-006 series PMAT-147..151 fanning out per-equation Kani coverage for the 5 multi-equation contracts). PMAT-058..077 shipped the substrate-completion run; PMAT-127..138 closed all PV-ENF warnings; PMAT-147..151 closed the per-equation Sym gap. See `xpile quorum` and CHANGELOG entries for each contract.
- ✅ Four real backends (Rust, Ruchy, Lean 4, Shell/bashrs); PTX/WGSL/SPIR-V still scaffolded
- ✅ Python subset (canonical: [`/CHANGELOG.md`](../../CHANGELOG.md)): typed `def`, multi-statement body, all binary + unary ops including bitwise / power, ternary, if/elif/else with single- *or multi-*assignment branches, function calls including self-recursion, **while loops with mutable rebinding** (PMAT-006), **for-in-range with positive *or negative* literal steps** (PMAT-007, PMAT-008), **`subprocess.run([...])` cross-domain to bashrs** (PMAT-040..058)
- ✅ Shell subset (POSIX): quoted strings (single + double + escape sequences), `$NAME` / `${NAME}` variable expansion, `$(cmd)` and backtick command substitution, NAME=value assignment, pipelines, ShellLoop (for/while/until), POSIX special parameters ($1..9, $@, $#, etc.). See PMAT-037..058 entries.
- ✅ Semantic round-trip verified for 11+ fixtures (factorial, fib, gcd, abs_val, sign, bits, square_plus, range_size, sum_to, for_sum / range_with_start / range_with_step, factorial_iter) plus shell `bashrs_realistic_demo.sh` (PMAT-052)
- ✅ CI gate enforced on PRs (fmt, check, clippy -D warnings, pv lint, cargo deny, workspace tests including `every_kani_harness_discharges`, dedicated `kani` job runs all 43 BMC harnesses post-XPILE-QUORUM-006)
- ✅ Branch protection on `main`; crates.io reservation at `xpile 0.0.1`
- ⏳ Bigint promotion (`py-int-arith-v1.yaml` slow path) — fast-path
  overflow is now load-bearing (Rust + Ruchy emit
  `.checked_*().expect(...)`, contract name appears in the panic
  message); the slow path itself is still unimplemented
- ⏳ Types beyond int/bool (str, float, collections)
- ⏳ Lean encoding for `while` (`partial def` tail-recursion follow-up)
- ⏳ `for` over non-range iterables (blocked on collection types)
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

## 26. Audit-acknowledged Caveats

**Sub-spec**: [audit-design.md](audit-design.md) (full adversarial review)

xpile's design has been subjected to an adversarial Popperian audit (`audit-design.md`). The architecture survives the four core falsification hypotheses, but the audit also surfaces structural caveats that this spec acknowledges rather than papers over. Each item below points back to the audit for the full critique.

| Caveat | One-line summary | Audit ref |
|---|---|---|
| **Deliberate ecosystem isolation** | No Julia / R / JNI / JS frontends; the xpile + aprender stack is intentionally "Sovereign AI" in pure Rust. Limits general-purpose polyglot ambition. | `audit-design.md §4` |
| **WebAssembly via Ruchy proxy** | No native Wasm backend; Ruchy's `WasmEmitter` is the route to Wasm. Couples xpile's Wasm story to Ruchy's release cadence. | `audit-design.md §4` |
| **Fixture overfitting** | The Oracle is only as strong as its fixtures — the agent does not synthesize them, so untested edge cases (negative zero, FFI aliasing, alignment) can pass the gate and fail in production. | `audit-design.md §4` |
| **Federated HIR myopia** | The federated meta-HIR resolves cross-language semantics at the `FfiBoundary` node. Some hybrid lifecycle patterns (Python-held C pointer crossing into another C extension) may require unified semantic analysis the federated HIR can't provide. | `audit-design.md §4` |
| **Citation-bridge fragility** | The Lean/LaTeX → contract citation pipeline previously relied on regex; reinforced by structured citation constructs post-audit (see [`sub/contract-frontend-trait.md`](sub/contract-frontend-trait.md)). Manual renaming in the proof lane can still break `citation_round_trip` unless mediated by structured tooling. | `audit-design.md §4` |
| **Determinism edge cases** | The content-addressed cache assumes deterministic captures; non-deterministic source-language features (Python hash randomization, ASLR-affected pointer comparisons) can cause the Oracle to flap and the agent loop to thrash. | `audit-design.md §4` |
| **Oracle hardware blind spots re-emerge** | Layer-5 contracts bound *which* hardware instructions can be emitted, but the Oracle generally cannot observe deep races / thread divergence. Hardware safety hinges on Layer-5 contract completeness — a single point of failure. | `audit-design.md §4` |

These caveats are load-bearing: surface them in design conversations rather than discovering them at integration time. Each maps to a planned or open `pmat work` item for closing or constraining the gap.

---

## 27. Provability Roadmap — ruchy 5.0 alignment

**Sub-spec**: [sub/provability-roadmap.md](sub/provability-roadmap.md)

Ruchy 5.0 ([`/home/noah/src/ruchy/docs/specifications/ruchy-5.0-sovereign-platform.md`](../../../ruchy/docs/specifications/ruchy-5.0-sovereign-platform.md), 1051 lines) ships a "provability mandate" (its §14) that publishes pre-committed falsifier thresholds, stratifies oracles by epistemic source, and gates escape hatches by deadline. Several of those mechanisms apply directly to xpile's correctness claim — and where they don't, the *reason* they don't is worth recording so future readers don't re-litigate the boundary.

This section is the index. The full per-item disposition lives in [sub/provability-roadmap.md](sub/provability-roadmap.md).

### Planned for adoption (each has a PMAT prefix)

| # | Ruchy §14 mechanism | xpile-spec home | PMAT prefix |
|---|---|---|---|
| 1 | Pre-committed **falsifier thresholds** (analog of §14.5 F1–F12) — % of transpiled fns with at least one cited contract, panic-message coverage, oracle/transpile divergence rate, etc., each with a published "we're wrong if it falls below X" line | §6 Oracle + §26 caveats | `XPILE-FALSIFY-XXX` |
| 2 | **Time-bounded escape hatches** (§14.7) — `unimplemented!()` / `expect("...slow path not yet implemented")` strings must carry `(reason, until = "vN.Y.Z", ticket)` and `build.rs` hard-fails when `CARGO_PKG_VERSION ≥ until`. Closes the "could rot forever" hole in our current panic messages | §11 pv integration | `XPILE-EXEMPT-XXX` |
| 3 | **N-of-M stratified oracle quorum** (§14.4) — current Oracle is one stratum (behavioral capture). Adding Kani (symbolic) and probar / Lean (semantic) as parallel oracles, with the spec's pairwise-correlation guard, would let xpile claim more than empirical equivalence | §6 Oracle | `XPILE-QUORUM-XXX` |
| 4 | **Differential execution check** (§14.10.4) — automatically run interpreter vs transpiled-binary on `N` probar-generated inputs per Layer-1-contract'd function; divergence = release block. Generalises our 11 hand-authored runtime-verified fixtures | §6 Oracle | `XPILE-DIFF-XXX` |
| 5 | **Refinement proofs via Lean** (§14.10.5) — for `C-PY-INT-ARITH`, prove in Lean that the i64 fast path equals the BigInt slow path within range. We have the Lean target (§24); we just don't use it for the provability claim yet | §24 Lean | `XPILE-REFINE-XXX` |
| 6 | **Quarterly SOTA-gap dossier** (§14.F-Audit-8 / F6) — audit-design.md is a single snapshot from 2026-05-15. A standing quarterly publication of "what beats xpile where" closes the procedural-stagnation falsifier | §26 caveats / audit-design.md | `XPILE-SOTA-XXX` |

### In-spirit, scope-deferred

Items where the *mechanism* is interesting but adopting it would require a meta-HIR change bigger than the value at v0.1.0. Captured for posterity in [sub/provability-roadmap.md](sub/provability-roadmap.md):

- **`Secret<T>` / `Public<T>` information-flow types** (Ruchy §14.10.1) — useful if we ever transpile cryptographic Python; today meta-HIR has no info-flow story.
- **Capability types for effects** (Ruchy §14.10.2) — closely related to FFI bound proofs (`C-FFI-CPYTHON-REFCOUNT` is exactly this domain), but our current contracts don't carry capability obligations.
- **Totality markers (`@total` / `decreases`)** (Ruchy §14.10.3) — would let the Lean partial-def encoding (PMAT-010) emit `def` rather than `partial def` when termination is provable.

### Explicitly NOT adopted

Each is named here so the boundary is visible, not so it gets re-litigated:

- **The 9 pillars themselves** (Correctness / Compute / Infra / Scripting / Learning / Visualization / Simulation / Testing / Embedding) — those are components of *ruchy-the-language*, not *xpile-the-transpiler*. xpile already federates with bashrs (Pillar 4) per §19, and `aprender` (Pillar 5) is xpile's contract-substrate provider. The other seven pillars are intentionally out of scope.
- **Graduate workflow** (interpret → embed → compile, Ruchy §7) — xpile has no interpreter and no plans for one. Same-source-three-modes is a ruchy-language property; xpile's same-source claim is across *target languages*, not execution modes.
- **Language-level new keywords** (`requires`, `ensures`, `invariant`, `decreases`, `infra`, `signal`, `yield` — Ruchy §4) — xpile transpiles existing languages; it does not invent syntax.

### Honest reading

Ruchy 5.0 is meaningfully ahead of xpile on **three** specific axes:

1. **Commits to numbers** — published falsifier thresholds + quarterly dossiers + deadline-enforced exemptions.
2. **Multiple independent oracles** — symbolic + semantic + extrinsic strata with anti-correlation guards.
3. **Self-reflection tooling** — `ruchy tier` reports on Ruchy's own contract coverage with eight CI gates, regression baselines, TOML config, JSON / markdown output. xpile has nothing analogous.

audit-design.md §6 already shows we know *how* the five-whys → provable-contract loop is supposed to work; this section commits to applying it to ourselves with the same rigor ruchy 5.0 commits to applying it to its own stdlib. Each "planned for adoption" row above is sized to be one PR; the implementation order follows [sub/provability-roadmap.md](sub/provability-roadmap.md).

---

## 28. Diamond-Tier Refinement Taxonomy

**Sub-spec**: [sub/diamond-taxonomy.md](sub/diamond-taxonomy.md)

The substrate's Diamond-tier program (PMAT-214..387) ships **121 wired Diamond equations across 12 contracts**, demonstrating 110+ distinct algebraic categories grouped into 34+ families. **Six UNIVERSAL milestones now hold**: **depth-3 UNIVERSAL** (PMAT-336), **depth-4 UNIVERSAL** (PMAT-344), **depth-5 UNIVERSAL** (PMAT-354), **depth-6 UNIVERSAL** (PMAT-365), **depth-7 UNIVERSAL** (PMAT-376), and **depth-8 UNIVERSAL** (PMAT-387) — every contract has ≥8 Diamond categories. The structure-extensionality pattern (introduced at PMAT-311) became a substrate-wide recurring theme demonstrated on **32+ distinct record/subtype contracts**. Eight recurring algebraic templates emerged during the broadening sweeps.

**Path β extension recap (this work stream):**

1. **Path β depth grind** (PMAT-298..327): pushed PyIntArith from depth-8 → depth-21 (13 new tiers), CompileRustToPtxMma from depth-8 → depth-20 (12 new tiers) — each tier added genuinely orthogonal algebraic categories (ring, integral-domain, ordered-ring, normed-ring, sign function, gcd/PID, unit group, partial-inverse, order-embedding, etc.).
2. **Strategic pivot to BROADENING** (PMAT-328..365):
   - **First broadening wave (PMAT-328..336):** pushed 8 contracts to depth-3+ via the structure-extensionality pattern, achieving **depth-3 UNIVERSAL across all 12 contracts** (PMAT-336) and **depth-4 ACROSS ALL 5 TAXONOMY LAYERS** (PMAT-330).
   - **Second broadening wave (PMAT-338..344):** pushed 7 contracts from depth-3 to depth-4 via five recurring algebraic templates, achieving **depth-4 UNIVERSAL across all 12 contracts** (PMAT-344).
   - **Third broadening wave (PMAT-346..354):** pushed 9 contracts from depth-4 to depth-5, achieving **depth-5 ACROSS ALL 5 TAXONOMY LAYERS** (PMAT-347) and then **depth-5 UNIVERSAL across all 12 contracts** (PMAT-354). Expanded the structure-extensionality template family (PMAT-349, 352, 353, 354) and introduced the String.length Nat-structure as a sixth recurring template (PMAT-346, 350).
   - **Fourth broadening wave (PMAT-356..365):** pushed 10 contracts from depth-5 to depth-6, achieving **depth-6 ACROSS ALL 5 TAXONOMY LAYERS** (PMAT-358) and then **depth-6 UNIVERSAL across all 12 contracts** (PMAT-365). The wave was dominated by the structure-extensionality template (PMAT-356, 359, 360, 361, 362, 363, 364), with PMAT-352/365 closing the Rust↔Lean Array.size invariant on both sides and PMAT-353/354/361/364 closing inner/outer record extensionality on the ContractFrontend↔ContractBackend trait pair at both abstraction levels.
   - **Fifth broadening wave (PMAT-367..376):** pushed 10 contracts from depth-6 to depth-7, achieving **depth-7 ACROSS ALL 5 TAXONOMY LAYERS** (PMAT-369) and then **depth-7 UNIVERSAL across all 12 contracts** (PMAT-376). The wave continued the structure-extensionality template (PMAT-367, 368, 371, 373, 374) and Array.size template (PMAT-375, 376), and introduced the **enum completeness** template as a 7th recurring algebraic family (PMAT-370 Target, PMAT-372 LatexDisplayKind). PMAT-359/PMAT-369 closed the Frontend↔Backend trait input-record extensionality pair, and PMAT-375/PMAT-376 closed the inner-record Array.size invariant on the ContractFrontend↔ContractBackend trait pair.
   - **Sixth broadening wave (PMAT-378..387):** pushed 10 contracts from depth-7 to depth-8, achieving **depth-8 ACROSS ALL 5 TAXONOMY LAYERS** (PMAT-380) and then **depth-8 UNIVERSAL across all 12 contracts** (PMAT-387). The wave continued the structure-extensionality template (PMAT-378/381..385) and Array.size template (PMAT-386/387), adding Bronze-tier struct-ext demonstrations (PMAT-381 Artifact, PMAT-379 Outcome length on Bronze Outcome) as a new substrate-wide pattern, and added a third instance of the enum-completeness template (PMAT-380 SourceLang).

### Coverage state (v0.1.0+, post-PMAT-286..387)

| Depth | Coverage | Mechanism |
|---|---|---|
| Diamond depth-1 | 12/12 contracts (UNIVERSAL) | PMAT-214..226 |
| Diamond depth-2 | 12/12 contracts (UNIVERSAL, CI-enforced) | PMAT-228..250, CI gate via PMAT-251 |
| **Diamond depth-3** | **12/12 contracts (UNIVERSAL, post-PMAT-336)** | PMAT-241..245 + PMAT-289 + PMAT-331..336 broadening sweep |
| **Diamond depth-4** | **12/12 contracts (UNIVERSAL, post-PMAT-344)** | PMAT-247/248/288/329/330 (one per layer) + PMAT-338..344 (broadening sweep) |
| **Diamond depth-5** | **12/12 contracts (UNIVERSAL, post-PMAT-354)** | PMAT-286/287/328 (initial) + PMAT-346..354 broadening sweep |
| **Diamond depth-6** | **12/12 contracts (UNIVERSAL, post-PMAT-365)** | PMAT-290/291 (initial) + PMAT-356..365 broadening sweep |
| **Diamond depth-7** | **12/12 contracts (UNIVERSAL, post-PMAT-376)** | PMAT-292/293 (initial) + PMAT-367..376 broadening sweep |
| **Diamond depth-8** | **12/12 contracts (UNIVERSAL, post-PMAT-387)** | PMAT-294/295 (initial) + PMAT-378..387 broadening sweep |
| Diamond depth-9 | 2 contracts ACROSS LAYERS | PMAT-298 (PyIntArith), PMAT-299 (CompileRustToPtxMma) |
| Diamond depth-10 | 2 contracts ACROSS LAYERS | PMAT-300 (PyIntArith), PMAT-301 (CompileRustToPtxMma) |
| Diamond depth-11 | 2 contracts ACROSS LAYERS | PMAT-302 (PyIntArith), PMAT-303 (CompileRustToPtxMma) |
| Diamond depth-12 | 2 contracts ACROSS LAYERS | PMAT-305 (PyIntArith), PMAT-306 (CompileRustToPtxMma) |
| Diamond depth-13 | 2 contracts ACROSS LAYERS | PMAT-307 (PyIntArith), PMAT-308 (CompileRustToPtxMma) |
| Diamond depth-14 | 2 contracts ACROSS LAYERS | PMAT-310 (PyIntArith), PMAT-311 (CompileRustToPtxMma) |
| Diamond depth-15 | 2 contracts ACROSS LAYERS | PMAT-312 (PyIntArith), PMAT-313 (CompileRustToPtxMma) |
| Diamond depth-16 | 2 contracts ACROSS LAYERS | PMAT-315 (PyIntArith), PMAT-316 (CompileRustToPtxMma) |
| Diamond depth-17 | 2 contracts ACROSS LAYERS | PMAT-317 (PyIntArith), PMAT-318 (CompileRustToPtxMma) |
| Diamond depth-18 | 2 contracts ACROSS LAYERS | PMAT-320 (PyIntArith), PMAT-321 (CompileRustToPtxMma) |
| Diamond depth-19 | 2 contracts ACROSS LAYERS | PMAT-322 (PyIntArith), PMAT-323 (CompileRustToPtxMma) |
| Diamond depth-20 | 2 contracts ACROSS LAYERS | PMAT-325 (PyIntArith), PMAT-326 (CompileRustToPtxMma) |
| **Diamond depth-21** | **1 contract (PyIntArith, deepest in substrate)** | PMAT-327 (NAT-CAST ORDER EMBEDDING) |

### Deep-depth contracts at v0.1.0+

**`C-PY-INT-ARITH` (Layer 1) — 21 categories:**
1. PMAT-214: SEMIRING (+, *)
2. PMAT-228: EUCLIDEAN DOMAIN (fdiv, fmod)
3. PMAT-241: SHIFT-MONOID (shl)
4. PMAT-247: POWER-MONOID (pow)
5. PMAT-286: BITWISE-AND-COMMUTATIVE-MONOID (&)
6. PMAT-290: ABELIAN-GROUP-ENRICHMENT (neg)
7. PMAT-292: ORDER-DISTRIBUTIVE-LATTICE (min, max)
8. PMAT-294: DIVISIBILITY-PREORDER (∣)
9. PMAT-298: LINEAR-ORDER TRICHOTOMY (<)
10. PMAT-300: RING-DISTRIBUTIVITY (neg × mul)
11. PMAT-302: INTEGRAL DOMAIN (no zero divisors)
12. PMAT-305: ORDERED RING (sign rules)
13. PMAT-307: ABSOLUTE VALUE / NORM
14. PMAT-310: NAT-CAST RING HOMOMORPHISM
15. PMAT-312: INT-EMOD QUOTIENT HOMOMORPHISM
16. PMAT-315: GCD MONOID + BÉZOUT IDENTITY — Int is a PID
17. PMAT-317: UNIT GROUP `{1, -1} ≅ Z/2Z`
18. PMAT-320: SIGN FUNCTION MONOID HOMOMORPHISM `Int → {-1, 0, 1}`
19. PMAT-322: NEGATION-ORDER COMPATIBILITY — Int is an `OrderedAddCommGroup`
20. PMAT-325: Int.toNat PARTIAL INVERSE — section-retraction of the Nat → Int embedding
21. PMAT-327: NAT-CAST ORDER EMBEDDING — Mathlib's `OrderRingHom Nat Int` shape (DEEPEST in substrate)

**`C-COMPILE-RUST-TO-PTX-MMA` (Layer 5) — 20 categories:**
1. PMAT-218: bounded-monoid (additive)
2. PMAT-287: closure (subalgebra well-definedness)
3. PMAT-231: join-semilattice (max)
4. PMAT-242: meet-semilattice (min)
5. PMAT-248: lattice absorption
6. PMAT-291: distributive lattice
7. PMAT-293: bounded lattice (top + bottom)
8. PMAT-295: cancellative monoid
9. PMAT-299: ordered monoid (monotone preorder)
10. PMAT-301: additive-lattice distributivity (tropical-semiring axiom)
11. PMAT-303: discrete order
12. PMAT-306: max/min monotonicity
13. PMAT-308: GLB/LUB universal property
14. PMAT-311: subtype extensionality (BoundedSmem ↔ Nat .val)
15. PMAT-313: Nat-mod quotient homomorphism
16. PMAT-316: Nat GCD monoid
17. PMAT-318: Nat power-monoid
18. PMAT-321: NAT INTEGRAL DOMAIN — multiplicative no-zero-divisors + zero absorption
19. PMAT-323: NAT TRUNCATED SUBTRACTION — `Nat.sub` saturates at 0 (semiring-minus structure)
20. PMAT-326: NAT POWER-MONOTONICITY — `Nat.pow` order-preservation (base + exponent)

### Tooling

- `xpile diamond` (PMAT-249): live per-contract Diamond count + depth classification, with `--json` output for CI dashboards. Depth labels: `none` / `depth-1` / ... / `depth-20` / `depth-21+`.
- `crates/xpile/tests/diamond_coverage.rs` (PMAT-251..387): CI gate — **22 integration tests** enforce depth-1/2/3/4/5/6/7/8 UNIVERSAL (all 12 contracts), depth-9..21 across 2 layers (or single deepest), plus aggregate-total-≥30; substrate-wide Diamond coverage cannot regress.

### Canonical reference

Per-category catalog, proof-pattern recipes, and "when to add a new Diamond" decision rubric live in [sub/diamond-taxonomy.md](sub/diamond-taxonomy.md). Every new Diamond PR should cross-reference its algebraic family there.

### Falsification posture

If a future PR weakens Diamond coverage — removes a `_diamond` equation, breaks a contract's depth-2 invariant, etc. — the `diamond_coverage.rs` gate fails the build. This is the **enforcement** counterpart to the **reporter** posture of `xpile quorum`.

---

## 29. Layer-5 Multi-Emitter Oracle Quorum

**Sub-spec**: [sub/layer5-multi-emitter-quorum.md](sub/layer5-multi-emitter-quorum.md)

Layer-5 compile contracts (`C-COMPILE-RUST-TO-PTX-MMA` and future siblings for WGSL, SPIR-V) have rich Semantic-stratum coverage via the Diamond program but currently only single-vote Runtime stratum (the "Run=1 demo fixture" caveat in audit-design.md §4). This section commits to closing that gap via §14.4 N-of-M oracle quorum applied to backend emitters.

### Design

For each Layer-5 target, xpile-N-codegen routes through:

- **General emitter** (mandatory) — handles any contract-conforming input. For PTX: `rustc_codegen_nvvm`. For WGSL: `naga`. For SPIR-V: `rspirv`.
- **Specialist emitter** (optional) — handles a domain-specific subset with hand-tuned templates. For PTX: aprender-gpu's GEMM/MMA kernels. For shell: bashrs-realistic's 17k+ corpus-tuned patterns.

When BOTH emitters fire on the same input, their outputs become two independent oracle votes at the Runtime stratum. A `DiffExec` quorum policy executes both PTX programs on test inputs and compares numerical outputs — divergence falsifies the contract.

### Why this matters

The Diamond proofs (PMAT-218/231/242/248 on C-COMPILE-RUST-TO-PTX-MMA) currently prove things about a `BoundedSmem` MODEL, not about emitted PTX text. They are *in-vacuum*. Adding multi-emitter quorum at the Runtime stratum creates the gate that connects model to emission: if either emitter produces PTX violating the modeled invariants, runtime divergence catches it.

The two emitters fail in **categorically independent** ways (LLVM bug vs hand-tuned-template bug) — the §14.10 anti-correlation guard is satisfied by construction.

### Generalization

The pattern is not PTX-specific. Same shape applies to WGSL (`naga` + WebGPU specialists), SPIR-V (`rspirv` + Vulkan compute specialists), shell (`bashrs-backend` + `bashrs-realistic` corpus), and C extensions (`pyo3` + hand-tuned `cffi`).

### Implementation roadmap

- ✅ **PMAT-259** — design + sub-spec + schema sketch
- ⏳ **PMAT-260** — `audit-design.md` "Oracle Hardware Blind Spots" entry marked Mitigated via Multi-Emitter Quorum
- ✅ **PMAT-261** — `EmitterRole`, `QuorumPolicy`, `QuorumStatus`, `DiffExecResult`, `ViaEntry` data model (serde round-trip tested)
- ✅ **PMAT-262** — `Artifact.quorum_status` field + backwards-compat default for legacy payloads
- ✅ **PMAT-263** — `TargetEmitter` trait + `MultiEmitterBackend` routing layer (mock-emitter unit tests cover the four routing cases: specialist-missing, specialist-unmatched, prefer-specialist, strict-match/divergent)
- ✅ **PMAT-264** — `PtxBackend` refactored to wrap `MultiEmitterBackend` (Section 29 architecture validated in production code, not just mock tests)
- ✅ **PMAT-265** — `WgslBackend` mirrored the wrapper-refactor pattern, confirming the §29 routing is a real reusable abstraction
- ✅ **PMAT-266** — 7 adversarial invariant tests for the routing layer (citation provenance under Strict divergence, PreferSpecialist hides divergence by design, error propagation from general/specialist, general-None as contract violation, NotRun reason carries tolerance, DiffExec does not short-circuit on text equality)
- ⏳ **PMAT-26X+** — light up `rustc_codegen_nvvm` path in xpile-ptx-codegen (replaces `ScaffoldPtxEmitter` in the `general` slot)
- ⏳ **PMAT-26Y+** — cross-repo binding to `aprender-gpu` specialist (plugs into the `specialist` slot via `MultiEmitterBackend::new_with_specialist`)
- ⏳ **PMAT-26Z+** — `DiffExec` engine + `xpile quorum` reporting of multi-vote Runtime stratum (replaces the `DiffExecResult::NotRun` branch with real execution comparison)
- ⏳ **PMAT-A5** — `pv lint` schema extension for `compile_targets.via.role` (cross-repo work in `../provable-contracts/`)

**Status at v0.1.0+ (post-PMAT-264..266):** the §29 routing layer is production-wired into both code-lane GPU backends (PTX and WGSL). The architecture has been validated by both happy-path mock tests (PMAT-263) and adversarial invariant tests (PMAT-266). What remains is implementation of the actual specialist emitters (`rustc_codegen_nvvm`, `aprender-gpu`) and the `DiffExec` execution engine that consumes their outputs.

Full per-phase scope and pros/cons in [sub/layer5-multi-emitter-quorum.md](sub/layer5-multi-emitter-quorum.md).

### Falsification posture

Once the spec is implemented, the following weaken the substrate (fail CI):

1. PTX produced by either emitter executed on test inputs returns outputs that diverge from the quorum (tolerance configured per contract).
2. The specialist emitter is silently dropped without a `quorum_policy: PreferSpecialist` annotation in the YAML.
3. The general emitter is removed (no fallback for unknown shapes).
4. `pv lint` is weakened to allow `compile_targets.via` without a `role: general` entry.

---

## License

MIT OR Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.

## Contributing

Open a draft PR with: (a) a contract YAML if you're adding a new construct; (b) `pv lint` 8/8 passing; (c) the linked pmat work item ID. No bare code changes without a corresponding contract or pmat item.
