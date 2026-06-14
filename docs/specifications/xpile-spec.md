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
| 30 | [v0.2.0 Roadmap — Three Mergers](#30-v020-roadmap--three-mergers) | [sub/v0.2.0-depyler-merger.md](sub/v0.2.0-depyler-merger.md), [sub/v0.2.0-decy-merger.md](sub/v0.2.0-decy-merger.md), [sub/v0.2.0-bashrs-checkback.md](sub/v0.2.0-bashrs-checkback.md) |

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

**v0.1.12 — SHIPPED 2026-06-12** — incremental release on top of v0.1.11. Adds **early returns** (guard clauses) — PMAT-479, the post-v0.1.4 audit's R10 (the load-bearing control-flow item). meta-HIR gains `Stmt::Return(Expr)` for *non-final* returns; a guard clause `if (n<=1) { return 1; } return n*fact(n-1);` lowers the early return to `Stmt::Return` (Rust/Ruchy emit `return e;`) while the final value still flows through `Block::trailing_return` (Lean refuses, keeping the single-trailing-return shape). This is the tractable slice — it unlocks the dominant guard-clause idiom WITHOUT changing the load-bearing every-function-yields-one-value invariant (the early return is additive; the full `trailing_return → Option` change is a follow-up). Exit: recursive `fact` via guard clause `fact(5)==120`; `sign(7)/sign(-3)/sign(0) == 1/-1/0`. Produced by the decy C frontend. Substrate unchanged at QUORUM. `transpile_e2e` at 84 tests. **This completes the EV ladder R1–R5, R7–R10** (R6 — contract-integrity gap + Diamond-gate grandfather — remains, sequenced for careful substrate work).

**v0.1.11 — SHIPPED 2026-06-12** — incremental release on top of v0.1.10. Adds **`Stmt::If`** — C `if`/`else` statements (PMAT-478, the post-v0.1.4 audit's R9), the decy frontend's first statement-level branching beyond the ternary. meta-HIR gains `Stmt::If { cond, then_body, else_body }`; decy parses `if (c) { … } else { … }` (incl. `else if` chains), Rust/Ruchy emit `if c { … } else { … }`, Lean refuses (its executable encoding uses the if-expression form). Branch bodies are statement lists; a local reassigned in a branch is inferred `mut`. Early returns inside branches are NOT yet supported (R10). Exit: `max3(1,5,3)==5`, `clamp(15,0,10)==10`, `clamp(-3,0,10)==0`. Python keeps its if-as-let lowering (Python→Stmt::If migration is a follow-up). Substrate unchanged at QUORUM. `transpile_e2e` at 83 tests.

**v0.1.10 — SHIPPED 2026-06-12** — incremental release on top of v0.1.9. Adds the Python **`float`** type (PMAT-477, the post-v0.1.4 audit's R8) — xpile's first non-integer numeric type. `Type::F64` + `Expr::LitFloat` + `Expr::FloatBinOp` in the meta-HIR; `float` lowers to Rust/Ruchy `f64`, Lean `Float`. Float arithmetic (`+ - * /`) is plain infix (IEEE-754 saturates, no `checked_*`), `/` is true division (not floor); float comparisons reuse `Expr::BinOp` (plain infix, `Bool`). Exit (epsilon-tolerance rustc round-trip): `lerp(0,10,0.5)==5.0`, `average(3,4)==3.5`. No governing contract yet (capability-ahead-of-contract; `C-PY-FLOAT-ARITH` queued — float fns cite nothing, not a phantom contract). `//`/`%`/`**` on floats + mixed int/float coercion deferred. Substrate unchanged at QUORUM. `transpile_e2e` at 82 tests.

**v0.1.9 — SHIPPED 2026-06-12** — incremental release on top of v0.1.8. Adds Python **keyword arguments** in calls `f(x=1, y=2)` (PMAT-474, the post-v0.1.4 audit's R5). The module signature table (from R2) is extended to record ordered parameter names (`FnSig { ret, params }`); a keyword call is reordered to positional at lowering using that order, then emitted as a plain positional call (no backend change). `area(1, 2, h=4, w=3)` → `area(1, 2, 3, 4)`. Exit: `mixed() == 10`, `all_kw() == 100`. Every parameter must be supplied (positionally or by keyword) — defaults and `**kwargs` are not supported (clear errors). Substrate unchanged at QUORUM. `transpile_e2e` at 81 tests.

**v0.1.8 — SHIPPED 2026-06-12** — incremental release on top of v0.1.7. Adds Python **list comprehensions** `[elem for var in iter]` (PMAT-473, the post-v0.1.4 audit's R4). A comprehension is an expression but the meta-HIR has no block-expression, so it materialises to `let mut <acc>: list[T] = []` + `for var in iter { acc.append(elem) }` — in assignment position (`ys = [x+x for x in xs]`) and return position (`return [x*x for x in xs]`, hoisted to a temp). Reuses the shipped `.append()`/for-each machinery; no new IR. Exit: `squares -> [1,4,9,16]`, `total_sq -> 14`. Slice: single generator, no filter, list-typed iterable (range/dict/filters deferred). Substrate unchanged at QUORUM. `transpile_e2e` at 80 tests.

**v0.1.7 — SHIPPED 2026-06-11** — incremental release on top of v0.1.6. Adds Python **dict iteration** `for k in d:` (PMAT-472, the post-v0.1.4 audit's R3), completing the dict lane (read/write/`.get`/`in`/`len` shipped at v0.1.2). A dict iterates its keys → Rust `for k in d.keys().cloned()` (a new `over_keys` flag on `Stmt::ForEach`; list iteration unchanged). Exit (order-independent rustc round-trip): `sum_keys` → 6, `sum_values` → 60 over `{1:10,2:20,3:30}`. Note: HashMap key order is unspecified, so iteration order is not yet faithful to CPython insertion order (deferred). Substrate unchanged at QUORUM. `transpile_e2e` at 79 tests.

**v0.1.6 — SHIPPED 2026-06-11** — incremental release on top of v0.1.5. Adds **cross-function return-type inference** (PMAT-471, the post-v0.1.4 audit's R2): a module-level signature table (pre-pass over top-level `def`s) records each function's declared return type; `Expr::Call` inference consults it instead of the old hardcoded `Type::I64` fallback. So `s = make_scores()` (returning `dict[str,int]`) types `s` as `HashMap<String,i64>` — was `let s: i64`, which silently broke `s["alice"]` under rustc. Fixes multi-function programs composing dict/list/str helpers. Frontend-only; no meta-HIR/backend change. Exit: a `make_scores → alice_score/total` composition computes `total() == 30`. Substrate unchanged at QUORUM. `transpile_e2e` at 78 tests.

**v0.1.5 — SHIPPED 2026-06-11** — incremental release on top of v0.1.4. Adds Python **augmented assignment** (PMAT-470, the post-v0.1.4 audit's R1): `x += e` and the family `-= *= //= %= &= |= ^= <<= >>= **=`, desugared to `x = x <op> e` reusing the `BinOp` machinery (overflow-checking + str-concat detection intact — `s += "!"` → `format!`, `p *= x` → `checked_mul`) with correct `let mut` inference. No meta-HIR or backend change. Unblocks the most-used Python loop idiom (counters/accumulators). Exit: `count_up(100) == 4950`. Subscript-target aug-assign (`d[k] += v`) deferred (use `d[k] = d[k] + v`). Substrate unchanged at QUORUM. `transpile_e2e` at 77 tests.

**v0.1.4 — SHIPPED 2026-06-11** — incremental release on top of v0.1.3. Extends the `decy` C → Rust frontend (PMAT-467, slice 2) from recursion-only to **iterative** C: `while` loops, variable reassignment with correct `let mut` inference (a local is `mut` iff reassigned, incl. inside a loop — clean under `rustc -D warnings`), and C truncating `/` `%` → Rust `wrapping_div`/`wrapping_rem` (truncation toward zero — `-7/2 == -3`, not Python floor `-4`). Exit (rustc round-trip): iterative `sum_to` computes `sum_to(100) == 5050`. Deferred: `if`/`else` statements, early returns (meta-HIR single trailing return; `return` inside a loop body is rejected), pointers/structs/strings, the `C-C-INT-ARITH` contract substrate, and C → Ruchy/Lean. Substrate unchanged at QUORUM (13 contracts, depth-13 UNIVERSAL). decy-frontend at 8 unit tests; `transpile_e2e` at 76 tests.

**v0.1.3 — SHIPPED 2026-06-11** — incremental release on top of v0.1.2. Adds **xpile's second source language** via the EV-ranked **P2** of the §30 roadmap: a real `decy` C → Rust frontend (PMAT-467) for the stack-only int subset — `int` function defs + params, local `int x = expr;` decls, trailing `return`, and expressions (literals, idents, recursion, `+ - *`, comparisons, `&& ||`, unary `- !`, ternary, parens). C lowers with **C arithmetic semantics** (`int` → `i32`, `+ - *` → `wrapping_*` for signed-overflow-as-UB) via an isolated rust-codegen path, distinct from Python's `i64` + `checked_*`/bigint and leaving the Python/Ruchy codegen untouched. Exit criterion verified by rustc round-trip: `int add(int,int)` and recursive `int factorial(int)` compile and compute correct values (`factorial(12) == 479001600`); functions cite `C-C-INT-ARITH`. Deferred: C `/` `%`, `if`/`while` statements, pointers/structs/strings, the `C-C-INT-ARITH` contract substrate (authoring it would trip the depth-13 UNIVERSAL floor frozen per §30 — sequenced separately), and C → Ruchy/Lean. Substrate unchanged at QUORUM (13 contracts, 184 Diamond theorems, depth-13 UNIVERSAL). `transpile_e2e` at 76 tests.

**v0.1.2 — SHIPPED 2026-06-11** — incremental release on top of v0.1.1. Completes the Python **dict operations** lane (PMAT-466, the EV-ranked P1 of the §30 roadmap): `d[k]` read, `d[k] = v` write, `d.get(k, default)`, `k in d` membership, `len(d)`, empty annotated literal `{}`, and `name: T = value` annotated locals with correct `mut` inference. Rust + Ruchy emit a real `HashMap` pipeline (verified by rustc round-trip); Lean refuses dict ops (deferred to the `Std.HashMap` encoding, v0.3.0). The diff passed a two-round adversarial multi-agent review (11 defects found + fixed, then regression-verified). Substrate unchanged at QUORUM (13 contracts, 184 Diamond theorems, depth-13 UNIVERSAL — the Diamond ratchet is frozen at depth-13 per §30). The `C-XLATE-PY-DICT-TO-HASHMAP` contract substrate, dict iteration (`for k in d:`), and the Track 2 / Track 3 mergers remain queued.

**v0.1.1 — SHIPPED 2026-05-22** — incremental release on top of v0.1.0. Adds the Python types lane (`str` + `list[T]` r/w + `dict[K, V]` foundation, PMAT-449..462) without touching the substrate at QUORUM (still 13 contracts, 184 Diamond theorems, depth-13 UNIVERSAL strict gate). Lean iteration / mutation, dict operations, and the Track 2 / Track 3 mergers from spec §30 remain queued for v0.2.0.

v0.1.0 — **SHIPPED 2026-05-20** — first real release; `cargo install xpile` works for end users:

- ✅ **All 27 workspace crates published to crates.io at v0.1.0** (topological order; published over ~3.5h on the new-crate 5/hour rate-limit budget). xpile is no longer a name reservation — it's a working CLI.
- ✅ `aprender-contracts` (`pv`) wired via crates.io 0.33
- ✅ 12 contracts pass `pv lint` (0 errors, 0 warnings)
- ✅ **100% §14.4 N-of-M QUORUM coverage** — all 12 contracts have paired Lean refinement theorems AND Kani BMC harnesses; **638 stratum-vote artifacts** total (285 Semantic + 53 Symbolic + 15 Runtime + 285 Extrinsic) across all 5 taxonomy layers. See `xpile quorum`.
- ✅ **Eleven UNIVERSAL Diamond milestones depth-3..13** (PMAT-336..442); **171 wired Diamond theorems** across 12 contracts; 13 recurring algebraic templates. Deepest contracts: `C-PY-INT-ARITH` at depth-21, `C-COMPILE-RUST-TO-PTX-MMA` at depth-20. Diamond coverage CI-enforced via `crates/xpile/tests/diamond_coverage.rs` (22 integration tests, depth-1..13 UNIVERSAL gates).
- ✅ **Four real backends** (Rust, Ruchy, Lean 4, Shell/bashrs); PTX/WGSL/SPIR-V still scaffolded
- ✅ Python subset (canonical: [`/CHANGELOG.md`](../../CHANGELOG.md)): typed `def`, multi-statement body, all binary + unary ops including bitwise / power, ternary, if/elif/else with single- *or multi-*assignment branches, function calls including self-recursion, **while loops with mutable rebinding** (PMAT-006), **for-in-range with positive *or negative* literal steps** (PMAT-007, PMAT-008), **`subprocess.run([...])` cross-domain to bashrs** (PMAT-040..058)
- ✅ Shell subset (POSIX): quoted strings (single + double + escape sequences), `$NAME` / `${NAME}` variable expansion, `$(cmd)` and backtick command substitution, NAME=value assignment, pipelines, ShellLoop (for/while/until), POSIX special parameters
- ✅ **297 workspace tests**, all green; semantic round-trip verified for 11+ Python fixtures (factorial, fib, gcd, abs_val, sign, bits, square_plus, range_size, sum_to, for_sum / range_with_start / range_with_step, factorial_iter) plus shell `bashrs_realistic_demo.sh`
- ✅ CI: `gate` + `workspace-test` + `kani` + new `book` workflow all green on every PR
- ✅ **Branch protection on `main`**; tag `v0.1.0` pushed at commit `7a82b23`; GitHub Release live at https://github.com/paiml/xpile/releases/tag/v0.1.0
- ✅ **End-user-facing mdBook deployed** at https://paiml.github.io/xpile/ (PMAT-446) — 16 chapters covering introduction, install, quick start, concepts/contracts/Diamond-substrate, tutorials for Python→Rust/Lean and shell round-trip, full CLI + frontend + backend + contract reference, and contributing guides. Every concept/tutorial page begins with a `> **Governing contract:**` quote-block linking to the contract row, so the citation graph stays joinable from prose to YAML to Lean to Kani. `pmat comply asset-validate` → 4 pass, 0 warn, 3 skip.
- ⏳ Bigint promotion (`py-int-arith-v1.yaml` slow path) — fast-path overflow is load-bearing (Rust + Ruchy emit `.checked_*().expect(...)`, contract name appears in the panic message); the slow path itself is still unimplemented
- 🟢 Types beyond int/bool — **v0.2.0 Track 1.A SHIPPED + Track 1.B full read/write + Track 1.C foundation** (PMAT-449..462): str lane (depth-13 UNIVERSAL `C-XLATE-PY-STR-TO-RUST-STRING`); list lane with full read/write API (`Type::List(Box<Type>)` + `list[int]/list[str]/list[bool]/list[list[int]]` + `xs[i]` read + `xs[i] = v` write + `for x in xs:` iteration + `len(xs)` + `xs.append(v)`); **dict lane foundation** `Type::Dict(Box<Type>, Box<Type>)` + `dict[K, V]` annotation + `{...}` literal lowering to Rust `HashMap<K, V>` and Lean `List (K × V)`. `Type` is no longer `Copy`; call sites refactored to `&Type` or `.clone()`. Substrate: **13 contracts at QUORUM**, **184 Diamond theorems**. Lean iteration/mutation, `dict[K, V]` substrate authoring, `.extend()`/`.insert()`/`.pop()`, slicing, and `float` remain.
- ⏳ Lean encoding for `while` (`partial def` tail-recursion follow-up)
- 🟢 `for` over non-range iterables — **shipped at v0.2.0 Track 1.B** (PMAT-458): `for x in xs:` over a `Type::List(T)` parameter or expression lowers to `Stmt::ForEach`; Rust/Ruchy emit `for x in xs.iter().cloned() { ... }`. Lean still refuses (monadic iteration encoding deferred to v0.3.0). Closes the original v0.1.0 ⏳ entry. Fixture: `sum_list.py` runs `total(vec![1..5]) == 15` green via rustc round-trip.
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

The substrate's Diamond-tier program (PMAT-214..442) ships **171 wired Diamond equations across 12 contracts**, demonstrating 160+ distinct algebraic categories grouped into 39+ families. **Eleven UNIVERSAL milestones now hold**: **depth-3..13** (PMAT-336/344/354/365/376/387/398/409/420/431/442) — every contract has ≥13 Diamond categories. **Template 13 (Bronze→Silver→Bronze round-trip identity)** introduced at the depth-13 broadening wave (PMAT-433) is on **10 distinct round-trip diamonds** (PMAT-433..442) — captures the correctness relationship between Templates 10 (projection) and 12 (lift). Thirteen recurring algebraic templates emerged during the broadening sweeps.

**Path β extension recap (this work stream):**

1. **Path β depth grind** (PMAT-298..327): pushed PyIntArith from depth-8 → depth-21 (13 new tiers), CompileRustToPtxMma from depth-8 → depth-20 (12 new tiers) — each tier added genuinely orthogonal algebraic categories (ring, integral-domain, ordered-ring, normed-ring, sign function, gcd/PID, unit group, partial-inverse, order-embedding, etc.).
2. **Strategic pivot to BROADENING** (PMAT-328..365):
   - **First broadening wave (PMAT-328..336):** pushed 8 contracts to depth-3+ via the structure-extensionality pattern, achieving **depth-3 UNIVERSAL across all 12 contracts** (PMAT-336) and **depth-4 ACROSS ALL 5 TAXONOMY LAYERS** (PMAT-330).
   - **Second broadening wave (PMAT-338..344):** pushed 7 contracts from depth-3 to depth-4 via five recurring algebraic templates, achieving **depth-4 UNIVERSAL across all 12 contracts** (PMAT-344).
   - **Third broadening wave (PMAT-346..354):** pushed 9 contracts from depth-4 to depth-5, achieving **depth-5 ACROSS ALL 5 TAXONOMY LAYERS** (PMAT-347) and then **depth-5 UNIVERSAL across all 12 contracts** (PMAT-354). Expanded the structure-extensionality template family (PMAT-349, 352, 353, 354) and introduced the String.length Nat-structure as a sixth recurring template (PMAT-346, 350).
   - **Fourth broadening wave (PMAT-356..365):** pushed 10 contracts from depth-5 to depth-6, achieving **depth-6 ACROSS ALL 5 TAXONOMY LAYERS** (PMAT-358) and then **depth-6 UNIVERSAL across all 12 contracts** (PMAT-365). The wave was dominated by the structure-extensionality template (PMAT-356, 359, 360, 361, 362, 363, 364), with PMAT-352/365 closing the Rust↔Lean Array.size invariant on both sides and PMAT-353/354/361/364 closing inner/outer record extensionality on the ContractFrontend↔ContractBackend trait pair at both abstraction levels.
   - **Fifth broadening wave (PMAT-367..376):** pushed 10 contracts from depth-6 to depth-7, achieving **depth-7 ACROSS ALL 5 TAXONOMY LAYERS** (PMAT-369) and then **depth-7 UNIVERSAL across all 12 contracts** (PMAT-376). The wave continued the structure-extensionality template (PMAT-367, 368, 371, 373, 374) and Array.size template (PMAT-375, 376), and introduced the **enum completeness** template as a 7th recurring algebraic family (PMAT-370 Target, PMAT-372 LatexDisplayKind). PMAT-359/PMAT-369 closed the Frontend↔Backend trait input-record extensionality pair, and PMAT-375/PMAT-376 closed the inner-record Array.size invariant on the ContractFrontend↔ContractBackend trait pair.
   - **Sixth broadening wave (PMAT-378..387):** pushed 10 contracts from depth-7 to depth-8, achieving **depth-8 ACROSS ALL 5 TAXONOMY LAYERS** (PMAT-380) and then **depth-8 UNIVERSAL across all 12 contracts** (PMAT-387). The wave continued the structure-extensionality template (PMAT-378/381..385) and Array.size template (PMAT-386/387), adding Bronze-tier struct-ext demonstrations (PMAT-381 Artifact, PMAT-379 Outcome length on Bronze Outcome) as a new substrate-wide pattern, and added a third instance of the enum-completeness template (PMAT-380 SourceLang).
   - **Seventh broadening wave (PMAT-389..398):** pushed 10 contracts from depth-8 to depth-9, achieving **depth-9 UNIVERSAL across all 12 contracts** (PMAT-398). The wave introduced **Template 9 (Gold-tier subtype-extensionality)** as a new substrate-wide recurring family, exercising every contract's Gold-tier refinement subtype: PMAT-389 (BorrowedRefManifestEntry struct-ext on FfiCpythonExt, transitional), PMAT-390 (SuccessfulOutcome on Bashrs), PMAT-391 (FrameSafeTransition on ContractFrontendTrait), PMAT-392 (ConsistentBackendInput on BackendTrait), PMAT-393 (ConsistentFrontendOutput on FrontendTrait), PMAT-394 (CitationCompleteContract on ContractBackendTrait), PMAT-395 (NonEmptyHomogeneousList α on PyListToVec — first polymorphic subtype-ext), PMAT-396 (WarningLineCount on XlateLeanToRust), PMAT-397 (NonEmptyPreconditionList on XlateRustFnToLeanThm), PMAT-398 (NonEmptyDefinition on Notation — depth-9 UNIVERSAL finale). Closes Frontend↔Backend trait Gold-tier subtype-ext symmetry pair (PMAT-392/393) and ContractFrontend↔ContractBackend Gold-tier subtype-ext pair (PMAT-391/394).
   - **Eighth broadening wave (PMAT-400..409):** pushed 10 contracts from depth-9 to depth-10, achieving **depth-10 UNIVERSAL across all 12 contracts** (PMAT-409). The wave introduced **Template 10 (Tier-projection homomorphism)** as a new substrate-wide recurring family — defining canonical forgetful maps Silver→Bronze on each contract's tiered model and proving the projection is structure-preserving. PMAT-400 (BoundedRefcountDelta subtype-ext on FfiCpythonExt — transitional Template 9 extension), then 9 PRs introducing Template 10: PMAT-401 (silver_to_bronze on Bashrs Outcome — Template 10 introduction), PMAT-402 (ArtifactSilver→Artifact), PMAT-403 (MetaHirModuleSilver→MetaHirModule), PMAT-404 (TranspileSession→Array EquationsBlock), PMAT-405 (RenderedDocSilver→RenderedDoc), PMAT-406 (HomogeneousListSilver α→PyListSilver α — second polymorphic projection), PMAT-407 (LeanDefSilver→LeanDef), PMAT-408 (RustFnSilver→RustFn), PMAT-409 (DefinitionEnvSilver→DefinitionEnv — depth-10 UNIVERSAL finale). Closes Frontend↔Backend trait Silver→Bronze tier-projection pair (PMAT-402/403), ContractFrontend↔ContractBackend Silver→Bronze tier-projection pair (PMAT-404/405), and Rust↔Lean Silver→Bronze tier-projection pair (PMAT-407/408).
   - **Ninth broadening wave (PMAT-411..420):** pushed 10 contracts from depth-10 to depth-11, achieving **depth-11 UNIVERSAL across all 12 contracts** (PMAT-420). The wave introduced **Template 11 (Canonical identity element)** as a new substrate-wide recurring family — defining distinguished identity/zero elements on each contract's Silver/Gold tiered model and proving their structural properties. PMAT-411 (balanced_refcount_delta on FfiCpythonExt — Template 11 introduction), then 9 PRs broadening across all remaining contracts: PMAT-412 (empty_success_outcome on Bashrs), PMAT-413 (empty_rust_artifact on BackendTrait), PMAT-414 (empty_python_module on FrontendTrait — closes F↔B pair with PMAT-413), PMAT-415 (empty_session on ContractFrontendTrait), PMAT-416 (empty_contract on ContractBackendTrait — closes CF↔CB pair with PMAT-415), PMAT-417 (empty_py_list_silver α on PyListToVec — third polymorphic canonical), PMAT-418 (empty_lean_def_silver on XlateLeanToRust), PMAT-419 (empty_rust_fn_silver on XlateRustFnToLeanThm — closes Rust↔Lean pair with PMAT-418), PMAT-420 (empty_definition_env_silver on Notation — depth-11 UNIVERSAL finale).
   - **Tenth broadening wave (PMAT-422..431):** pushed 10 contracts from depth-11 to depth-12, achieving **depth-12 UNIVERSAL across all 12 contracts** (PMAT-431). The wave introduced **Template 12 (Bronze→Silver canonical-lift homomorphism)** as a new substrate-wide recurring family — defining canonical lifts from Bronze types to Silver types with default values for the new Silver fields. Inverse direction of Template 10 (Silver→Bronze projection). PMAT-422 (FfiCall→FfiCallSilver — Template 12 introduction), PMAT-423 (Outcome→OutcomeSilver), PMAT-424 (Artifact→ArtifactSilver), PMAT-425 (MetaHirModule→MetaHirModuleSilver — closes F↔B pair with PMAT-424), PMAT-426 (EquationsBlock→TranspileSession), PMAT-427 (RenderedDoc→RenderedDocSilver — closes CF↔CB pair with PMAT-426), PMAT-428 (PyList→PyListSilver UInt8 — UInt8-specialized), PMAT-429 (LeanDef→LeanDefSilver), PMAT-430 (RustFn→RustFnSilver — closes Rust↔Lean pair with PMAT-429), PMAT-431 (DefinitionEnv→DefinitionEnvSilver — depth-12 UNIVERSAL finale).
   - **Eleventh broadening wave (PMAT-433..442):** pushed 10 contracts from depth-12 to depth-13, achieving **depth-13 UNIVERSAL across all 12 contracts** (PMAT-442). The wave introduced **Template 13 (Bronze→Silver→Bronze round-trip identity)** as a new substrate-wide recurring family — proves that the composition of Template 10 projection and Template 12 lift equals identity on the Bronze type, capturing the correctness relationship between the two directional homomorphisms. PMAT-433 (FfiCall round-trip — Template 13 introduction), PMAT-434 (Outcome), PMAT-435 (Artifact), PMAT-436 (MetaHirModule — closes F↔B pair), PMAT-437 (EquationsBlock singleton variant), PMAT-438 (RenderedDoc — closes CF↔CB pair), PMAT-439 (PyList UInt8 variant), PMAT-440 (LeanDef), PMAT-441 (RustFn — closes Rust↔Lean pair), PMAT-442 (DefinitionEnv — depth-13 UNIVERSAL finale).

### Coverage state (v0.1.0+, post-PMAT-286..442)

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
| **Diamond depth-9** | **12/12 contracts (UNIVERSAL, post-PMAT-398)** | PMAT-296/297 (initial) + PMAT-389..398 broadening sweep (Template 9 Gold-tier subtype-ext) |
| **Diamond depth-10** | **12/12 contracts (UNIVERSAL, post-PMAT-409)** | PMAT-300/301 (initial) + PMAT-400..409 broadening sweep (Template 10 Tier-projection homomorphism) |
| **Diamond depth-11** | **12/12 contracts (UNIVERSAL, post-PMAT-420)** | PMAT-302/303 (initial) + PMAT-411..420 broadening sweep (Template 11 Canonical identity element) |
| **Diamond depth-12** | **12/12 contracts (UNIVERSAL, post-PMAT-431)** | PMAT-305/306 (initial) + PMAT-422..431 broadening sweep (Template 12 Bronze→Silver canonical-lift) |
| **Diamond depth-13** | **12/12 contracts (UNIVERSAL, post-PMAT-442)** | PMAT-307/308 (initial) + PMAT-433..442 broadening sweep (Template 13 Bronze↔Silver round-trip identity) |
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
- `crates/xpile/tests/diamond_coverage.rs` (PMAT-251..442): CI gate — **22 integration tests** enforce depth-1/2/3/4/5/6/7/8/9/10/11/12/13 UNIVERSAL (all 12 contracts), depth-14..21 across 2 layers (or single deepest), plus aggregate-total-≥30; substrate-wide Diamond coverage cannot regress.

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
- ⏳ **PMAT-26X+** (→ §30 Track 4 **PMAT-485**, offline prereq **PMAT-481**) — light up the `general` PTX emitter, replacing `ScaffoldPtxEmitter`. The first real path is the upstream **`nvptx64-nvidia-cuda` rustc target** (lighter than `rustc_codegen_nvvm`), gated offline by `ptxas`-assembles. **Candidate third general emitter:** NVlabs `cuda-oxide` (pure-Rust MIR→Pliron→LLVM→PTX) — categorically independent of NVVM-IR, so it strengthens the §14.10 anti-correlation guard, and executable on the now-available Lambda sm_90/sm_100 hardware. Still tracked-not-scheduled — its controlling "unbuilt consumer" blocker is retired the moment **PMAT-485 + PMAT-488** land; remaining gates are cuda-oxide's nightly+`rustc-dev` pin and alpha maturity; see §30 "Watched bets."
- ⏳ **PMAT-26Y+** (→ §30 Track 4 **PMAT-491**) — cross-repo binding to the `aprender-gpu` specialist (plugs into the `specialist` slot via `MultiEmitterBackend::new_with_specialist`)
- ⏳ **PMAT-26Z+** (→ §30 Track 4 **PMAT-486** interface/hook, then **PMAT-488** Cuda + **PMAT-490** Vulkan engines) — `DiffExec` engine + `xpile quorum` reporting of the multi-vote Runtime stratum (replaces the `DiffExecResult::NotRun` branch with real execution comparison). These slices flip falsification-posture #1 from latent to enforced and discharge the audit §4 Run=1 caveat.
- ⏳ **PMAT-A5** (in-tree half → §30 Track 4 **PMAT-484**) — structured `compile_targets.via.role` lifted into the PTX contract YAML + self-validating in-tree test (honors posture #4); the cross-repo `pv`-engine enforcement in `../provable-contracts/` stays the residual PMAT-A5.

**Sliced for autonomous pickup as §30 Track 4 — PMAT-481..491, smallest-PMAT-first; see that track for the per-slice CI / hardware story.**

**Status at v0.1.0+ (post-PMAT-264..266):** the §29 routing layer is production-wired into both code-lane GPU backends (PTX and WGSL). The architecture has been validated by both happy-path mock tests (PMAT-263) and adversarial invariant tests (PMAT-266). What remains is implementation of the actual specialist emitters (`rustc_codegen_nvvm`, `aprender-gpu`) and the `DiffExec` execution engine that consumes their outputs.

Full per-phase scope and pros/cons in [sub/layer5-multi-emitter-quorum.md](sub/layer5-multi-emitter-quorum.md).

### Runtime-stratum hardware (available 2026-06-12)

The audit's "Run=1 demo fixture" caveat ([`audit-design.md`](../audit-design.md) §4 Oracle blind spots) held the Runtime stratum at single-vote partly because no hardware was wired to *execute* emitted GPU artifacts. That gate is now retired — the hardware to discharge `DiffExec` (PMAT-26Z+) exists across all three GPU lanes:

| Lane | Host | Capability | Executes |
|---|---|---|---|
| **PTX (NVIDIA)** | **Lambda Cloud** (on-demand, by the minute) | Hopper **H100/GH200 (sm_90)** — full TMA + real thread-block clusters; Blackwell **B200 (sm_100)** | sm_90/sm_100 PTX incl. cuda-oxide's advanced TMA/cluster/WGMMA/tcgen05 intrinsics |
| **PTX (NVIDIA), local** | **GX10** = ASUS Ascent GX10 / NVIDIA **GB10 Grace-Blackwell (sm_121)** | Real CUDA device; TMA yes, but clusters effectively 1×1×1, no DSMEM/TMA-multicast/tcgen05; many cubins omit sm_121 → JIT-from-`compute_120` PTX | baseline + TMA PTX; *not* full sm_90 cluster kernels |
| **SPIR-V / WGSL** | **"intel"** = Intel Xeon W-3245 Mac Pro (Ubuntu), **2× AMD Radeon Pro W5700X** (Navi10/RDNA1) via **Vulkan 1.3 RADV** | SPIR-V via the AMD Vulkan compute path; WGSL lowers to SPIR-V (naga/wgpu). *Not* an Intel-GPU/oneAPI box — a Level-Zero lane would need an Intel Arc/Max GPU | SPIR-V / WGSL compute |
| **CPU rustc round-trip** | **"intel"** Xeon host (rustc/cargo) | native x86_64 build/test host | the R-ladder's `rustc`-round-trip verification |

Implication: the Runtime-stratum `DiffExec` engine is no longer *hardware*-blocked — what remains is engineering (real emitters + the `DiffExec` comparison engine). This is **now scheduled as the co-equal §30 Track 4 (PMAT-481..491)**, front-loaded with free-CI offline gates (`ptxas`/`naga`/structural) so the loop starts with zero hardware. The audit §4 "in-vacuum, cannot execute" caveat is now *dischargeable*, but is **discharged only when the on-hardware execution slices land — PMAT-488 (PTX) and PMAT-490 (WGSL)** — not by the offline scaffolding slices, which preserve `NotRun`. (A self-hosted NVIDIA runner can be the on-box RTX 4090 (sm_89); Lambda H100/B200 + GX10 cover sm_90/sm_100 advanced kernels out-of-band.) The cuda-oxide watched bet (§30) inherits this: its hardware blocker is retired; its surviving gates are the unbuilt consumer (PMAT-485/488 here), the nightly+`rustc-dev` toolchain pin, and alpha maturity.

### Falsification posture

Once the spec is implemented, the following weaken the substrate (fail CI):

1. PTX produced by either emitter executed on test inputs returns outputs that diverge from the quorum (tolerance configured per contract).
2. The specialist emitter is silently dropped without a `quorum_policy: PreferSpecialist` annotation in the YAML.
3. The general emitter is removed (no fallback for unknown shapes).
4. `pv lint` is weakened to allow `compile_targets.via` without a `role: general` entry.

---

## 30. v0.2.0 Roadmap — Three Mergers

**Sub-specs:**
[`sub/v0.2.0-depyler-merger.md`](sub/v0.2.0-depyler-merger.md),
[`sub/v0.2.0-decy-merger.md`](sub/v0.2.0-decy-merger.md),
[`sub/v0.2.0-bashrs-checkback.md`](sub/v0.2.0-bashrs-checkback.md).
Pattern precedent: [`sub/bashrs-merger.md`](sub/bashrs-merger.md) — the v0.1.0 bashrs merger ran the same shape and shipped successfully (PMAT-037..119).

### Premise (load-bearing)

v0.1.0 is "real transpiler for int/bool Python, real POSIX shell round-trip." It does not yet replace the standalone `paiml/depyler` for Python work because xpile's `depyler-frontend` is a **1,586-line single-file extract** (int/bool only) while standalone depyler is **18 sub-crates / 310+ source files** with `str` / `list` / `dict` / `float` / borrowing analysis already implemented. `decy-frontend` is even further behind — a 35-line scaffold against standalone `paiml/decy`'s **16 sub-crates**.

**v0.2.0 is the realization that this is not a build problem — it's a port problem.** All three mature standalone transpilers (depyler, decy, bashrs) already exist. The v0.1.0 bashrs merger demonstrated the absorb-into-substrate pattern works at production cadence. v0.2.0 runs that same pattern twice more.

### Status update (post-v0.1.1, 2026-05-22)

Track 1's str + list + dict-foundation work was sliced off as the **v0.1.1 incremental release** (PMAT-449..462) rather than waiting for the full v0.2.0 cut. This preserves the "three mergers" framing of v0.2.0 — Tracks 2 (decy) and 3 (bashrs check-back) are still its load-bearing scope, plus the remaining Track 1 work (dict operations, dict contract substrate, Lean iteration/mutation, `float`, `&str` borrowing, list `.extend()`/`.insert()`/etc.).

See [`sub/v0.2.0-depyler-merger.md`](sub/v0.2.0-depyler-merger.md) for the per-sub-track status table.

### Three concurrent tracks

| Track | What | Effort | Discharges |
|---|---|---|---|
| **1. Depyler merger** | Port `str` + `list[T]` + `dict[str, T]` (and optionally `&str`/borrowing as stretch) from standalone depyler into xpile-meta-hir + depyler-frontend + xpile-rust-codegen + xpile-lean-codegen. New contracts at Layer 2. | ~5 weeks (3 sub-tracks: str/list/dict, sequential) | "Can xpile replace depyler?" closes from "No" to "Yes for the str/list/dict subset" |
| **2. Decy merger** | Port `decy-parser` + `decy-codegen` int/bool subset into xpile-folded `decy-frontend` + `xpile-rust-codegen`. New Layer-1 contract `C-C-INT-ARITH` (sibling of `C-PY-INT-ARITH`, capturing C's truncating-`/`, UB-signed-overflow semantics → `wrapping_*`). Stack-only — no pointers. | ~3 weeks (parser + new contract + codegen) | C → Rust transpile works for stack-only int/bool C |
| **3. Bashrs check-back** | Discharge XPILE-UNMERGE-001 falsifier with a **second** independent oracle: Lean theorem for shell composition idempotence (option (c) from `sub/bashrs-merger.md`'s acceptance set). | ~3 days | Falsifier survives loss of option (a) `subprocess.run` discharge → defense in depth |

Tracks 1 and 2 are independent and can run in parallel. Track 3 is small and slots in opportunistically.

### Autonomous execution priority — EV-ranked (re-evaluated post-v0.1.4, 2026-06-11)

**This subsection is the canonical pickup order for autonomous sessions.** A session **picks the highest-EV open item and executes it to a tagged release without asking**, per [`/CLAUDE.md`](../../CLAUDE.md). The open pickup set is now **two co-equal fronts** — take whichever lead item is higher-EV (ties: pick either): **(a)** the highest-ranked open row of the R-table below — R1–R10 are **SHIPPED** (R1–R5/R7–R10 at v0.1.5–v0.1.12; **R6 COMPLETE at v0.1.195–197** — gate grandfathered + both contracts authored), so the **entire R-table is now closed**; and **(b)** the lowest-numbered open slice of **Track 4 — Runtime-stratum DiffExec** (PMAT-481..491, defined after this table). Track 4's free-CI slices (PMAT-481..486) are pickable in any session; its GPU-execution slices route to a self-hosted runner (see that track). Ranking is by **expected value = (capability or epistemic gain) ÷ effort**, biased toward items that (a) unblock the most real-world code *or convert in-vacuum proofs into executed witnesses*, (b) are verifiable by `rustc` round-trip or `ptxas`/`naga` offline checks (the proven methods), and (c) are **not** the frozen depth-13 Diamond treadmill.

**Shipped since the prior (PMAT-465) ranking — which is now closed, its P1/P2 done:** **v0.1.2** Python dict operations (PMAT-466 capability); **v0.1.3** decy C → Rust frontend — xpile's *second source language* (PMAT-467); **v0.1.4** iterative C (`while` / reassignment / truncating `/` `%`). The ladder below is the post-v0.1.4 re-evaluation (4-dimension read-only audit + synthesis, 2026-06-11; tracked in [`docs/roadmaps/roadmap.yaml`](../roadmaps/roadmap.yaml)).

**The dominant ceiling, confirmed by the audit:** the meta-HIR `Block { stmts, trailing_return }` single-trailing-return invariant blocks **early returns and if-as-control-flow across every frontend at once** — the load-bearing unlock (R10), but high-risk, so the cheap capability wins go first.

| Rank | Work item | Why high-EV | Effort | Risk |
|---|---|---|---|---|
| **R1** | **Augmented assignment** `x += 1` (`-= *= //= %= &= ...`) in depyler-frontend. | The single most-used Python loop idiom — unblocks **60–80% of real Python** (every counter/accumulator). Pure desugar to `x = x <op> rhs`; **no meta-HIR or backend change**. Highest capability-per-hour in the audit. | hours | low |
| **R2** | **Cross-function return-type inference** — a module-level signature table on `LoweringCtx`; consult it instead of the hardcoded `Type::I64` call-result fallback. | Fixes code that *should* transpile but silently emits wrong Rust (`d = make_dict(); d[k]` → `let d: i64` → rustc rejects). Frontend-only; a prerequisite for any multi-function dict/list/str program. | days | low |
| **R3** | **Dict iteration `for k in d:`** — desugar to iterate `d.keys()`. | Completes the dict lane (read/write/`.get`/`in`/`len` already shipped). 40–60% of dict code. Localized to `lower_for_stmt`. | ~1 day | low |
| **R4** | **List comprehensions** `[f(x) for x in xs]` — desugar to `tmp = []` + for-loop `tmp.append(...)`. | 60–80% of codebases use them. Reuses shipped `.append()` + for machinery; no new IR. Opens dict/set comprehensions later. | days | low |
| **R5** | **Keyword arguments** `f(x=1, y=2)` — reorder to positional at lowering (uses R2's signature table). | 40–60% of codebases. Pure desugar once signatures are known; synergizes with R2. | ~1–2 days | low |
| **R6** | **Close the contract-integrity gap** — author `C-C-INT-ARITH` + `C-XLATE-PY-DICT-TO-HASHMAP` YAMLs (Bronze→QUORUM) **and grandfather the depth-13 Diamond gate** so new contracts join at depth-1+ without the treadmill. | Emitted C cites `C-C-INT-ARITH` and dict code shipped with **no on-disk contract** — a live falsification of the "every construct under contract" claim. **Blocker:** adding a 14th YAML trips `diamond_coverage.rs` (`depth_N_plus == contracts_total`); the gate must *first* be changed to require depth-13 of the existing 13 and depth-1+ of new contracts (the §30 intent). That gate change is the real work and the only sanctioned reason to touch the Diamond machinery. 🔨 **sub-slice 1 (PMAT-475a) SHIPPED v0.1.195** — the gate is now grandfathered (depth-2..13 check the 13 pre-R6 contracts; new contracts join at depth-1+; behavior-preserving at 13, proven by synthetic unit tests). 🔨 **sub-slice 2 (PMAT-475b) SHIPPED v0.1.196** — `C-C-INT-ARITH` authored at depth-1 (`contracts/c-int-arith-v1.yaml` + `contracts/lean/CIntArith.lean`: C `int` → `i32::wrapping_*`, a depth-1 Diamond commutative-monoid theorem; first contract to join via the grandfathered gate → 14 contracts, depth-2+ still 13). Closes the **C-arithmetic half** of the §7.3 falsification. ✅ **sub-slice 3 (PMAT-475c) SHIPPED v0.1.197 — R6 COMPLETE** — `C-XLATE-PY-DICT-TO-HASHMAP` authored at depth-1 (`contracts/xlate-py-dict-to-hashmap-v1.yaml` + `contracts/lean/XlatePyDictToHashmap.lean`: dict→HashMap structure-preservation Diamond). The §7.3 "every construct under contract" falsification is **restored to true** — 15 on-disk contracts, both R6 newcomers at depth-1 via the grandfathered gate, depth-2+ still the grandfathered 13. | days | medium |
| **R7** | **2026-Q3 SOTA dossier** — append §7 to [`audit-design.md`](../audit-design.md), bump the deadline line. | Hard-dated CI gate (see below). Non-coding; cannot be parallelized away. | days | low |
| **R8** | **Float** (`Type::F64` + `LitFloat`) end-to-end + a forward-referenced `C-PY-FLOAT-ARITH` contract. | Unblocks ML / scientific / physics Python. Touches the `Type` enum + all backends (wider blast radius), IEEE-754-vs-arbitrary-precision note; verify with epsilon-tolerance asserts (not `assert_eq!`). | ~1 wk | medium |
| **R9** | **`Stmt::If`** in meta-HIR + decy/Python — if/else as a *statement*. | Biggest C-coverage multiplier (~15–25% → ~45–55% of real C) and lifts Python's if-as-control-flow cap. Genuine meta-HIR addition rippling to all backends; pairs with R10. | ~1 wk | medium |
| **R10** | **Early returns** — `Block.trailing_return → Option`, add `Stmt::Return` mixed into `stmts`; CFG-termination CI check. | The load-bearing structural unlock — guard clauses / search / early-exit across **Python AND C**. Sequence after R9 (a control-flow target exists) and the cheap wins. Touches the "every function yields one value" invariant (§3) + all backends. | ~1 wk+ | high |

**Execution posture:** ship each item (R-rank or Track-4 slice) as its own PR (full CI gate green) → squash-merge → tag a release, then take the next. **R1–R5 and R7–R10 shipped as v0.1.5–v0.1.12 (2026-06-11/12); R6 COMPLETE at v0.1.195–197 (gate grandfathered + `C-C-INT-ARITH` + `C-XLATE-PY-DICT-TO-HASHMAP`) — the entire R-table is now closed.** **Track 4 (PMAT-481..491) is co-equal with R6** — front-loaded with free-CI offline slices (PMAT-481..486), then self-hosted-runner GPU slices. **Always commit before launching an in-tree review agent** — agents have reverted uncommitted work (see [`/home/noah/.claude/.../feedback-commit-before-tree-agents.md`]); re-run the full gate after any review pass.

### Track 4 — Runtime-stratum DiffExec lighting (co-equal; PMAT-481..491)

**Co-equal with R6 and Mergers Tracks 1–3 — not deferred.** The deferral rationale ("no hardware can execute emitted GPU artifacts") was retired 2026-06-12 (see §29 "Runtime-stratum hardware"). This track incrementally lights up the §29 Layer-5 Multi-Emitter Quorum from scaffold to a real multi-vote Runtime witness. The routing layer (`MultiEmitterBackend`, `QuorumStatus`, `DiffExecResult`) is already 100% wired and adversarially tested (PMAT-261..266 ✅ — **not re-opened by this track**); what is missing is *implementation, not interface*, so each slice slots into an existing trait slot without changing the public surface. Pick **smallest open PMAT first**; the free-CI offline slices are front-loaded so the loop starts with zero hardware.

**Honest discharge boundary:** PMAT-481..486 (+ runner bring-up 487/489) are offline/structural/interface scaffolding that *prepares* the discharge and explicitly preserve `DiffExecResult::NotRun`. The audit §4 "Run=1 / demo-fixture" caveat for `C-COMPILE-RUST-TO-PTX-MMA` is discharged **only** when **PMAT-488** (PTX) executes a real multi-vote `DiffExec` witness on hardware (and **PMAT-490** for the WGSL lane). **Do not edit `audit-design.md` §4/§62's "single demo-fixture witness" line until that witness exists.**

| Slice | What | Effort | Risk | CI |
|---|---|---|---|---|
| **PMAT-481** | Offline PTX **well-formedness** gate: structural asserts on emitted text + a `ptxas`-assembles step. Asserts run against *current* output (initially the scaffold's comment-only shape); the `.visible .entry` / `.target sm_NN` / `.address_size 64` checks + the `ptxas` assemble **activate once real PTX exists (PMAT-485)**. `ptxas -arch` is **derived from `HwProfile::Ptx.compute_capability`**, never hard-coded. (A well-formedness gate — *not* the model→emission gate; that is PMAT-488.) | hours | low | free runner (CUDA-toolkit apt installer; no GPU) |
| **PMAT-482** | Offline WGSL/SPIR-V gate: `naga` validates emitted WGSL + `spirv-val` on CPU. | hours | low | free runner (no GPU) |
| **PMAT-483** | `QuorumPolicy::Strict` golden-text regression-lock on emitted PTX (activates once PMAT-485 emits real text — codegen drift turns the test red without a GPU). | hours | low | free runner |
| **PMAT-484** ✅ SHIPPED v0.1.24 | Structured `compile_targets.via_roles` (general/specialist + crate/cross_repo/shape_filter) + `quorum_policy{DiffExec,tolerance}` added **additively** to the PTX contract (flat `via:` kept; `pv lint` confirmed 0 errors). In-tree validator `contract_via_roles.rs` (4 cases, serde_yaml) is authoritative: exactly one general, no specialist-only, DiffExec tolerance, crate-named general. Cross-repo `pv`-engine enforcement = residual PMAT-A5. | days | low | free runner |
| **PMAT-485** | Real **general** PTX emitter via the upstream **`nvptx64-nvidia-cuda` rustc path** (verified on-box) — replaces `ScaffoldPtxEmitter::try_emit`. Lights up the general slot with the *first real assemblable PTX*; does **not** yet satisfy the contract's `mma.sync.aligned` headline invariant (an elementwise kernel emits no MMA — that is later/specialist work). **One committed CI story:** a contained **nightly emit job** in `ci.yml` (separate, like the `kani` job — *never* the default stable-1.93.0 build; needs nightly + `rust-src`/`llvm-tools`/`llvm-bitcode-linker`, `core`-only `no_std`) runs the emit + the PMAT-481 `ptxas` gate. | ~1 wk | medium | dedicated nightly job + free `ptxas` gate |
| **PMAT-486** ✅ SHIPPED v0.1.22 | `DiffExecEngine` trait + `Option<Arc<dyn DiffExecEngine>>` hook in `MultiEmitterBackend` (+ `with_diff_exec_engine` builder). No engine → benign `NotRun{no-engine}`; an *installed* engine that errors propagates a hard `BackendError` (not swallowed). Pure interface/wiring; PMAT-266/280 tests stay green; 3 new tests. | days | medium | free runner |
| **PMAT-487** | **Self-hosted NVIDIA runner bring-up** (prerequisite — the repo has *zero* self-hosted runner infra today): register the on-box **RTX 4090 (sm_89; ptxas/nvcc/driver already present)** as `runs-on: [self-hosted, linux, gpu, cuda]`; add a `gpu-ci` maintainer-approval label so fork PRs never auto-run on local hardware; ephemeral / NVIDIA-Container-Toolkit job; a trivial GPU smoke job proven green. Advisory (non-required) in branch protection until stable. | days | medium | establishes the self-hosted GPU lane |
| **PMAT-488** | **`CudaDiffExecEngine`** — real GPU execute+compare via `cudarc` (dynamic libcuda load → non-GPU CI still compiles): load both emitters' PTX, launch on fixture inputs, element-wise compare within tolerance → `Match{max_abs_diff}` / `Divergent`. **First Run≥2 Runtime vote — discharges the audit §4 caveat for the PTX contract.** Advanced sm_90/sm_100 (TMA/cluster) kernels verified out-of-band on Lambda H100/B200 or GX10. | ~1 wk+ | high | self-hosted GPU runner (deps **485, 486, 487**) |
| **PMAT-489** | **Self-hosted AMD-Vulkan runner bring-up** (prerequisite): register the "intel" box's 2× AMD Radeon Pro W5700X (RADV) as `runs-on: [self-hosted, linux, vulkan, amd]`; same label-gating + advisory posture. | days | medium | establishes the SPIR-V/WGSL execute lane |
| **PMAT-490** | **`VulkanDiffExecEngine`** — WGSL/SPIR-V execute+compare via a wgpu/vulkano compute pipeline on the AMD runner; Run≥2 for the WGSL contract; discharges the §4 caveat for the WGSL lane. Proves DiffExec is target-agnostic (the spec's explicit claim). | ~1 wk | high | self-hosted Vulkan runner (deps **482, 486, 489**) |
| **PMAT-491** | Cross-repo **`aprender-gpu` specialist** binding (the old PMAT-26Y+) — real categorically-independent specialist (hand-tuned MMA templates vs LLVM/NVVM) replacing the `MatmulSpecialistEmitter` mock; satisfies the §14.10 anti-correlation guarantee that makes `DiffExec` divergence high-signal. **Multi-PR** (changes live in another repo); sequenced last. | multi-wk | high | self-hosted GPU runner + free `ptxas` gate (deps **488, 484**) |

**CI tiers:** (1) **free** GitHub-hosted runners carry PMAT-481..486 (offline `ptxas`/`naga`/structural/quorum-serialization tests + the contained nightly emit job) — run the heavy CUDA-toolkit/`naga` installs as *separate* jobs (like `kani`), never folded into the required fast `gate`/`workspace-test`. (2) **self-hosted** GPU/Vulkan runners (stood up by PMAT-487/489) carry the `DiffExec` execution slices behind a maintainer-approval label, **advisory** in branch protection until proven, then promoted. (3) Lambda H100/B200 + GX10 used **out-of-band** for sm_90/sm_100 advanced-intrinsic kernels. `cudarc` links libcuda at *runtime*, so non-GPU jobs still compile.

### 10-day autonomous sprint (2026-06-12 → 2026-06-22) — self-selected, no questions

**Posture:** per [`/CLAUDE.md`](../../CLAUDE.md) + the 2026-06-12 directive, an autonomous session **self-selects the highest-EV open item from the queue below, ships it to a full-CI-green PR → squash-merge → GitHub release tag, then takes the next — without asking.** **Cadence is continuous** — ship slices back-to-back, taking the next open item immediately after each merge; do *not* stop at a daily cap (keep going until the queue is exhausted or interrupted). R6 and the lowest open Track-4/capability PMAT are co-equal leads (pick the higher EV-per-hour).

**Release cadence (load-bearing policy):**
- **GitHub tags — frequent:** every shipped slice gets its own `vX.Y.Z` tag + GitHub release (the proven per-slice cadence used for v0.1.5–v0.1.12).
- **crates.io — Fridays only, once per week:** publish the accumulated release line to crates.io **only on a Friday** (windows in this sprint: **2026-06-12** and **2026-06-19**). **Never publish to crates.io on a non-Friday.** Gate: full CI green **and** `cargo publish --dry-run` clean for every workspace crate, published **in dependency order**. crates.io publishes are **irreversible** — on any dry-run/publish failure, abort the whole batch and leave it for the next Friday (GitHub tags still ship daily regardless).

**EV-ranked queue (self-select the highest open item):**

| # | Item | Why high-EV | Effort | CI |
|---|---|---|---|---|
| 1 | **PMAT-481** — Track-4 offline PTX gate | unblocks the whole GPU lane; zero hardware | hours | free |
| 2 | **PMAT-482** — Track-4 offline WGSL/SPIR-V gate | second lane, near-zero effort | hours | free |
| 3 | ~~PMAT-492 string methods~~ — ✅ **COMPLETE** — `upper`/`lower`/`strip` (v0.1.15), `startswith`/`endswith` (v0.1.16), `split` (v0.1.17), `join` (v0.1.18). Full `Expr::StrMethod` family. | — | done | — |
| 4 | ~~PMAT-493 f-strings~~ — ✅ **already shipped as PMAT-452** (v0.2.0 Track 1.A); `f"{x}"` → chained `Concat`/`format!`. Do NOT re-pick. | — | done | — |
| 5 | ~~PMAT-494 tuples~~ — ✅ **COMPLETE** — literals + `tuple[...]` + multiple-return (v0.1.19), unpacking `a, b = f()` (`Stmt::LetTuple`, v0.1.20). `for k, v in …` target unpacking folds into PMAT-495. | — | done | — |
| 6 | ✅ **R6 / PMAT-475** — contract-integrity gap + Diamond-gate grandfather **(COMPLETE v0.1.195–197)** | closed the live "every construct under contract" falsification — 15 on-disk contracts | days | free |
| 7 | ~~PMAT-484 structured via_roles~~ — ✅ SHIPPED v0.1.24 (additive `via_roles` + `quorum_policy` in PTX contract; in-tree validator; pv green) | — | done | — |
| 8 | ~~PMAT-495 enumerate/zip~~ — ✅ SHIPPED v0.1.23 (`Stmt::ForEachPair` + `PairIterKind`; 2-name targets over lists). | — | done | — |
| 9 | ~~PMAT-496 slicing~~ — ✅ SHIPPED v0.1.21 (`Expr::Slice`, bounded `xs[a:b]` for list+str). Open-ended / step / negative deferred. | — | done | — |
| 10 | **PMAT-485** — Track-4 real general PTX emitter (`nvptx64`) | first real emission | ~1 wk | nightly job |
| 11 | ~~PMAT-486 DiffExecEngine trait + hook~~ — ✅ SHIPPED v0.1.22 (unblocks GPU `DiffExec`; real engines = needs_hardware PMAT-488/490) | — | done | — |
| 12 | **PMAT-483** — Track-4 `Strict` golden-text lock | codegen-drift guard (after 485) | hours | free |

The Track-4 **GPU-execution** slices (**PMAT-487..491**, `needs_hardware`) are picked up **out-of-band** once the self-hosted runner is stood up (the on-box RTX 4090) — not part of the free-CI daily cadence. The queue is a *guide*, not a straitjacket: a session may reorder by live EV (e.g. take a quick capability win before a `~1 wk` item), but never picks a `needs_hardware` slice into the free-CI loop and never publishes crates.io off-Friday.

### Tranche 2 — capability backlog (continuous; the numbered queue is drained)

Per the 2026-06-12 "never pause, never ask" directive: when the numbered sprint
queue is exhausted, the loop pulls from this ranked backlog (and invents the next
high-EV slice if it empties). All are slice-sized, free-CI, `rustc`-round-trip-
verifiable; add `PMAT-497+` tickets as each is taken.

| Item | Sketch | Status |
|---|---|---|
| **PMAT-497** — aug-subscript assign `d[k] += v` / `xs[i] += v` | desugar to `d[k] = d[k] <op> v`; reuses DictSet/IndexAssign — no new IR | ✅ SHIPPED v0.1.25 |
| **PMAT-498** — numeric builtins `abs`/`min`/`max` (`Expr::NumBuiltin`) | `abs(x)`→`(x).abs()`, `min/max(a,b)`→`(a).min/max(b)` | ✅ SHIPPED v0.1.26 |
| **PMAT-498b** — `sum(xs)` (`Expr::Sum`) | `sum(xs)`→`xs.iter().sum::<i64\|f64>()` | ✅ SHIPPED v0.1.27 (1-arg list min/max → PMAT-502e v0.1.36) |
| **PMAT-499** — `range(start, stop, step)` | full 3-arg range + **negative steps** (countdown) | ✅ already satisfied (PMAT-008; cond flips `<`/`>` on step sign). Only non-literal steps remain (deferred — needs runtime direction). |
| **PMAT-500** — sets: literal + `x in s` + `len` (v0.1.29) + **`.add()`** mutation (`Stmt::SetAdd`, v0.1.31) | `HashSet` lane | ✅ SHIPPED (set algebra ∪∩−^ → PMAT-502g v0.1.39; empty `set()` → PMAT-502i v0.1.41; set comp unblocked) |
| **PMAT-501** — dict comprehensions (v0.1.30) + **set comprehensions** `{e for …}` (v0.1.32) | materialise via for-DictSet / for-SetAdd; no new IR | ✅ SHIPPED — completes list/dict/set comprehensions |
| **PMAT-502** — `Stmt::If` branch generalization | dispatcher: if-as-let for `name=expr` branches, else a real `Stmt::If` (subscript assigns / `.append` / dict mutation) — unblocks the histogram idiom | ✅ SHIPPED v0.1.28 |
| **PMAT-502b** — `str.replace(old, new)` (`StrMethodOp::Replace`, 2-arg) | `.replace(&(old)[..], &(new)[..])` | ✅ SHIPPED v0.1.33 |
| **PMAT-502c** — `sorted(xs)` (`Expr::Sorted`) | `{ let mut __xv = xs.clone(); __xv.sort(); __xv }` | ✅ SHIPPED v0.1.34 (`reverse=` → PMAT-502f v0.1.38; `key=` → follow-up) |
| **PMAT-502d** — `reversed(xs)` (`Expr::Reversed`) | `{ let mut __xv = xs.clone(); __xv.reverse(); __xv }`; `list(reversed(xs))` unwraps to the same | ✅ SHIPPED v0.1.35 |
| **PMAT-502e** — 1-arg `min(xs)`/`max(xs)` over a list (`Expr::ListMinMax`) | `xs.iter().copied().min().unwrap()` / `.max()`; `list[int]` | ✅ SHIPPED v0.1.36 (`list[float]` → PMAT-502h v0.1.40) |
| **PMAT-502f** — `sorted(xs, reverse=True)` (`Expr::Sorted.reverse`) | `{ … __xv.sort(); __xv.reverse(); __xv }` (descending); non-bool/`key=` falls through | ✅ SHIPPED v0.1.38 |
| **PMAT-502g** — set algebra `a\|b` `a&b` `a-b` `a^b` (`Expr::SetOp`) | `(a).union/intersection/difference/symmetric_difference(&(b)).cloned().collect::<HashSet<_>>()`; both operands `Set`-typed (disambiguates from int BinOp) | ✅ SHIPPED v0.1.39 |
| **PMAT-502h** — `min(xs)`/`max(xs)` over `list[float]` (`Expr::ListMinMax.of_float`) | `xs.iter().copied().fold(f64::INFINITY, f64::min)` / `(f64::NEG_INFINITY, f64::max)` (f64 has no `Ord`) | ✅ SHIPPED v0.1.40 |
| **PMAT-502i** — empty constructors `set()`/`dict()`/`list()` | pure-frontend → empty `SetLit`/`DictLit`/`ListLit` → `HashSet::new()`/`HashMap::new()`/`vec![]`; element type from annotation or later `.add()`/`.append()` | ✅ SHIPPED v0.1.41 |
| **PMAT-502j** — `all(xs)`/`any(xs)` over `list[bool]` (`Expr::BoolReduce`) | `xs.iter().all(\|&__b\| __b)` / `.any(…)`; completes the reduction-over-list family (`sum`/`min`/`max`/`all`/`any`) | ✅ SHIPPED v0.1.42 |
| **PMAT-502k** — sequence repetition `"x"*n` / `[0]*n` (`Expr::Repeat`) | `(seq).repeat(((n).max(0)) as usize)` (str→String, list→Vec; negative clamps to empty); disambiguated from numeric `*` by operand type | ✅ SHIPPED v0.1.43 |
| **PMAT-502l** — more str methods `.lstrip`/`.rstrip`/`.find`/`.count` (`StrMethodOp`) | `.trim_start/.trim_end().to_string()` (Str); `.find(&sub[..]).map(\|i\| i as i64).unwrap_or(-1)` + `.matches(&sub[..]).count() as i64` (Int) | ✅ SHIPPED v0.1.44 |
| **PMAT-502m** — numeric conversions `int(x)` / `float(x)` (`Expr::NumCast`) | `((x) as i64)` (truncate toward zero) / `((x) as f64)`; numeric args only (`int("..")` parse + `str(x)` → separate slices) | ✅ SHIPPED v0.1.45 |
| **PMAT-502n** — `divmod(a, b)` (pure desugar) | `(a // b, a % b)` tuple reusing floor-div + mod (consistent with `//`/`%`, inherits C-PY-INT-ARITH); return + unpack forms | ✅ SHIPPED v0.1.46 |
| **PMAT-502o** — substring containment `sub in s` (`Expr::StrContains`) | `(s).contains(&(sub)[..])`; `not in` → `!(…)`; chosen over Set/Dict-contains by RHS type; fills the last `in`-operator gap | ✅ SHIPPED v0.1.47 |
| **PMAT-502p** — chained comparisons `a < b < c` (pure desugar) | `lower_compare` folds N ops into `(a OP1 b) && (b OP2 c) && …`; single comparison unchanged | ✅ SHIPPED v0.1.48 |
| **PMAT-502q** — tuple constant-index `t[N]` (`Expr::TupleIndex`) | `(t).N.clone()` (Rust tuple field access, distinct from list/dict `Expr::Index`); compile-time literal N in range; negative/non-literal deferred | ✅ SHIPPED v0.1.49 |
| **PMAT-502r** — open-ended slices `xs[a:]`/`xs[:b]`/`xs[:]` (`Expr::Slice.lo/hi` → `Option`) | absent bound → open Rust range (`a..`, `..b`, `..`) + `.to_vec()`/`.to_string()`; list + str; bounded `xs[a:b]` unchanged | ✅ SHIPPED v0.1.50 |
| **PMAT-502s** — negative list index `xs[-k]` (pure desugar) | `xs[len(xs) - k]` reusing `Len`+`Sub`+`Index` (inherits C-PY-INT-ARITH checked sub); list only; str/slice-bound negatives deferred | ✅ SHIPPED v0.1.51 |
| **PMAT-502t** — reverse-slice idiom `xs[::-1]` (pure desugar) | step −1 + no bounds over a list → `Expr::Reversed` (reuses v0.1.35); other steps + `str[::-1]` deferred | ✅ SHIPPED v0.1.52 |
| **PMAT-502u** — list query `xs.count(x)`/`xs.index(x)` (`Expr::ListQuery`) | `.iter().filter(…).count() as i64` / `.iter().position(…).map(…).expect(…)`; `list[int]`; `.count` disambiguated from str by recv type | ✅ SHIPPED v0.1.53 |
| **PMAT-502v** — dict views `d.keys()`/`d.values()` (`Expr::DictView`) | `.keys()/.values().cloned().collect::<Vec<_>>()` → `List(K)`/`List(V)`; composes w/ `sorted`/`sum`; `.items()` follows | ✅ SHIPPED v0.1.54 |
| **PMAT-502w** — context-aware `len(x)` (ctx-path intercept) | lowers the arg via `lower_expr_in_ctx` so `len(d.keys())`/`len(sorted(xs))` work; bare `len(xs)` unchanged | ✅ SHIPPED v0.1.55 |
| **PMAT-502x** — `d.items()` (`DictViewKind::Items`) | `.iter().map(\|(k,v)\| (k.clone(), v.clone())).collect::<Vec<_>>()` → `List(Tuple[K,V])`; composes w/ `sorted`/`len`; completes dict views | ✅ SHIPPED v0.1.56 |
| **PMAT-502y** — `for k, v in d.items()` loop (`PairIterKind::Pairs`) | `for (k,v) in <iter>.iter().cloned()`; iter typing as `List(Tuple[A,B])` destructured; not enumerate/zip | ✅ SHIPPED v0.1.57 |
| **PMAT-502z** — `sorted(xs, key=lambda p: e)` (`Sorted.key`/`SortKey`) | `sort_by_key(\|__k\| { let p = __k.clone(); e })`; **first lambda support** (param unbound → arith/`len` bodies; str-method keys deferred); composes w/ `reverse=` | ✅ SHIPPED v0.1.58 |
| **PMAT-502aa** — `min/max(xs, key=lambda)` (`ListMinMax.key`) | `.iter().cloned().min_by_key/max_by_key(\|__k\| { let p = __k.clone(); e }).unwrap()`; returns element (any type — only key needs `Ord`) | ✅ SHIPPED v0.1.59 |
| **PMAT-502ab** — `filter(lambda p: pred, xs)` (`Expr::Filter`) | `.iter().cloned().filter(\|__k\| { let p = __k.clone(); pred }).collect::<Vec<_>>()`; Bool predicate; `list(filter(…))` unwraps; result = input list type | ✅ SHIPPED v0.1.60 |
| **PMAT-502ac** — `map(lambda p: e, xs)` (`Expr::Map`) | `.iter().cloned().map(\|__k\| { let p = __k.clone(); e }).collect::<Vec<_>>()`; result = `List(<body type>)`; `list(map(…))` unwraps; 5th lambda position | ✅ SHIPPED v0.1.61 |
| **PMAT-502ad** — `str(x)` over an int (`Expr::ToStr`) | `format!("{}", x)` → `Str`; unblocks `"prefix" + str(n)` concat; `str(float)` deferred (formatting diffs) | ✅ SHIPPED v0.1.62 |
| **PMAT-502ae** — `str(b)` over a bool (pure desugar → `IfExpr`) | `"True" if b else "False"` → `if b { String::from("True") } else {…"False"…}`; matches Python's capitalization | ✅ SHIPPED v0.1.63 |
| **PMAT-502af** — `str(x)` over a float (`ToStr.of_float`) | block: `nan`→"nan", finite whole→`format!("{}.0",…)`, else `format!("{}",…)`; matches Python (`2.0`→"2.0"); completes str() int/float/bool | ✅ SHIPPED v0.1.64 |
| **PMAT-502ag** — str predicates `.isdigit`/`.isalpha`/`.isspace` (`StrMethodOp`) | `(!(s).is_empty() && (s).chars().all(\|__c\| __c.is_ascii_digit/is_alphabetic/is_whitespace()))` → `Bool`; empty→False (Python) | ✅ SHIPPED v0.1.65 |
| **PMAT-502ah** — `s.capitalize()` (`StrMethodOp::Capitalize`) | block: first char `.to_uppercase()` + rest `.to_lowercase()`; empty→""; matches Python | ✅ SHIPPED v0.1.66 |
| **PMAT-502ai** — standalone `enumerate(xs)`/`zip(a,b)` (`Expr::Enumerate`/`Zip`) | `.iter().cloned().enumerate().map(…).collect()` → `List(Tuple[I64,e])`; `.zip(…).collect()` → `List(Tuple[eL,eR])`; `list(…)` unwraps; composes w/ for-pair loop + `len` | ✅ SHIPPED v0.1.67 |
| **PMAT-502aj** — `s.title()` (`StrMethodOp::Title`) | fold: first alpha of each word `.to_uppercase()`, rest `.to_lowercase()`, non-alpha = boundary; Python-exact (incl. `"it's"`→`"It'S"`) | ✅ SHIPPED v0.1.68 |
| **PMAT-502ak** — `round(x)` 1-arg (`Expr::RoundToInt`) | float → `((x).round_ties_even() as i64)` (banker's rounding, Python-exact: `round(2.5)==2`); int → identity | ✅ SHIPPED v0.1.69 |
| **PMAT-502al** — `round(x, n)` 2-arg (`Expr::RoundToDigits`) | n≥0 → `format!("{:.n}", x).parse::<f64>()` (Rust `{:.}` is round-half-to-even = Python's decimal rounding, exact incl. `round(2.675,2)==2.67`); n<0 → scale+`round_ties_even`; → Float; completes `round` | ✅ SHIPPED v0.1.70 |
| **PMAT-502am** — f-string format specs (`Expr::FormatSpec`; ctx-aware f-string) | static subset `.Nf`/`0Nd`/`Nd`/`>N`/`<N`/`^N` → translated Rust `format!("{:<spec>}", v)`; conversion flags + dynamic + unsupported error | ✅ SHIPPED v0.1.71 |
| **PMAT-502an** — list membership `x in xs` (`Expr::ListContains`) | `(xs).contains(&(x))`; `not in` → `!(…)`; chosen by RHS type; fills the `in`-operator gap (dict/set/str/list) | ✅ SHIPPED v0.1.72 |
| **PMAT-502ao** — assert with message `assert cond, msg` (`Stmt::Assert.msg`) | `assert!(<cond>, "{}", <msg>);`; bare form unchanged; frontend validates `msg: Str`; Lean ignores message | ✅ SHIPPED v0.1.73 |
| **PMAT-502ap** — in-place list mutators `xs.sort()`/`.reverse()`/`.clear()` (`Stmt::ListMutate`) | `<list>.sort()` (`Vec<i64>`) / `.sort_by(\|a,b\| a.partial_cmp(b).unwrap())` (`Vec<f64>`) / `.reverse()` / `.clear()`; receiver `mut`; Lean refuses | ✅ SHIPPED v0.1.74 |
| **PMAT-502aq** — in-place list concatenation `xs.extend(ys)` (`Stmt::ListExtend`) | `<list>.extend((<ys>).iter().cloned());`; clones elements (Python `extend` doesn't consume its arg); receiver `mut`; Lean refuses | ✅ SHIPPED v0.1.75 |
| **PMAT-502ar** — positional list insertion `xs.insert(i, x)` (`Stmt::ListInsert`) | `<list>.insert((<i>) as usize, <x>);`; int index, in-range non-negative first cut (neg/past-end deferred); receiver `mut`; Lean refuses | ✅ SHIPPED v0.1.76 |
| **PMAT-502as** — list pop (expr) `xs.pop()` / `xs.pop(i)` (`Expr::ListPop`) | `(<list>).pop().unwrap()` (last) / `(<list>).remove((<i>) as usize)`; result = elem type; receiver `mut` via `count_pop_receivers` pre-pass (param + local); bare-stmt discard deferred; Lean refuses | ✅ SHIPPED v0.1.77 |
| **PMAT-502at** — item deletion `del coll[key]` (`Stmt::DelItem`) | list → `<name>.remove((<k>) as usize);`; dict → `<name>.remove(&(<k>));`; `is_dict` from receiver type; receiver `mut`; absent-dict-key KeyError deferred; Lean refuses | ✅ SHIPPED v0.1.78 |
| **PMAT-502au** — dict pop (expr) `d.pop(k)` / `d.pop(k, default)` (`Expr::DictPop`) | `(<dict>).remove(&(<k>)).unwrap()` (KeyError if absent) / `.unwrap_or(<default>)`; result = value type; receiver `mut` via `count_pop_receivers`; Lean refuses | ✅ SHIPPED v0.1.79 |
| **PMAT-502av** — set removal `s.remove(x)` / `s.discard(x)` (`Stmt::SetRemove`) | `remove` → `assert!(<set>.remove(&(<x>)), "…KeyError…");`; `discard` → `<set>.remove(&(<x>));`; receiver `mut`; completes set add/remove/discard; Lean refuses | ✅ SHIPPED v0.1.80 |
| **PMAT-502aw** — str padding `s.rjust(w)` / `s.ljust(w)` (`StrMethodOp::RJust`/`LJust`) | `format!("{:>1$}", <s>, (<w>) as usize)` / `{:<1$}`; format width = minimum (no truncation, matches Python); fill-char arg deferred; Lean refuses | ✅ SHIPPED v0.1.81 |
| **PMAT-502ax** — dict get-or-insert `d.setdefault(k, default)` (`Expr::DictSetDefault`) | `(<dict>).entry((<k>).clone()).or_insert(<default>).clone()`; result = value type; receiver `mut` via generalized `count_pop_receivers` (pop+setdefault); 2-arg first cut; Lean refuses | ✅ SHIPPED v0.1.82 |
| **PMAT-502ay** — filtered list comprehension `[e for v in xs if cond]` (extends `desugar_list_comp`) | desugars to `for v in xs { if cond { t.append(e); } }` (reuses `Stmt::If`); filter must be Bool; single `if` (multi → use `and`); Lean refuses | ✅ SHIPPED v0.1.83 |
| **PMAT-502az** — filtered dict + set comprehensions `{k:v for v in xs if cond}` / `{e for v in xs if cond}` (extends `desugar_dict_comp`/`desugar_set_comp`) | `if cond` guards the desugared `DictSet`/`SetAdd`; completes comp-filter across list/dict/set; Lean refuses | ✅ SHIPPED v0.1.84 |
| **PMAT-502ba** — list comprehension over `range(...)` `[e for x in range(n)]` (extends `desugar_list_comp` + `comp_range_bounds`) | desugars to counter `let mut x=start; while (x<cmp>stop) { t.append(e); x+=step; }`; composes with `if` filter; literal step; Lean refuses | ✅ SHIPPED v0.1.85 |
| **PMAT-502bb** — in-place dict merge `a.update(b)` (`Stmt::DictUpdate`) | `<dict>.extend((<b>).iter().map(\|(k,v)\| (k.clone(), v.clone())));`; overwrites; doesn't consume `b`; receiver `mut`; rounds out dict mutation; Lean refuses | ✅ SHIPPED v0.1.86 |
| **PMAT-502bc** — general slice step `xs[a:b:c]` over a list (`Expr::Slice.step`) | `<c>[<range>].iter().step_by(<c>).cloned().collect::<Vec<_>>()`; positive literal step; neg-step (except `[::-1]`) + str-step deferred; Lean refuses | ✅ SHIPPED v0.1.87 |
| **PMAT-502bd** — dict + set comprehensions over `range(...)` (extends `desugar_dict_comp`/`desugar_set_comp`) | counter while-loop around accumulator via shared `comp_range_bounds`/`comp_filter`/`comp_range_stmts`; completes comp-over-range across list/dict/set; Lean refuses | ✅ SHIPPED v0.1.88 |
| **PMAT-502be** — `bool(x)` truthiness cast (pure desugar) | int → `x != 0`; str/list/dict/set → `len(x) != 0`; bool → identity (reuses `BinOp::NotEq`+`Len`, no new variant); float deferred; all backends | ✅ SHIPPED v0.1.91 |
| **PMAT-502bf** — `int(s)` / `float(s)` string parse (`NumCast.from_str`) | `(<s>).trim().parse::<i64\|f64>().expect(…)` (trims like Python; panics = ValueError); numeric cast still `as`; Lean refuses | ✅ SHIPPED v0.1.92 |
| **PMAT-502bg** — list concatenation `xs + ys` (`Expr::ListConcat`) | `(<lhs>).iter().chain((<rhs>).iter()).cloned().collect::<Vec<_>>()`; fresh Vec, consumes neither; chosen by both-operands-List; companion of str `Concat`; Lean refuses | ✅ SHIPPED v0.1.93 |
| **PMAT-502bh** — `str.format` sequential `{}` (`Expr::StrFormat`) | `"<fmt>".format(args…)` → `format!("<fmt>", args…)`; str-literal recv; `count_simple_placeholders` validates `{}`/`{{`/`}}`, rejects `{0}`/`{name}`/`{:spec}`; int/str args (bool/float deferred); Lean refuses | ✅ SHIPPED v0.1.94 |
| **PMAT-502bi** — `s.index(sub)` substring index (`StrMethodOp::StrIndex`) | `(s).find(&(sub)[..]).map(\|i\| i as i64).expect(…)` (panics if absent = ValueError, vs `.find` `-1`); disambiguated from `list.index` by recv type; completes str find/count/index; Lean refuses | ✅ SHIPPED v0.1.95 |
| **PMAT-502bj** — module-level constants `NAME = <literal>` (`Item::Const`) | int/bool/float literal (neg folded) → `const NAME: TY = VALUE;` (Rust/Ruchy) / `def NAME : TY := VALUE` (Lean); pre-pass types refs in fn bodies (param shadows); str/collection/computed deferred | ✅ SHIPPED v0.1.96 |
| **PMAT-502bk** — `continue` / `break` loop control (`Stmt::Continue`/`Break`) | → `continue;` / `break;`; compose with `If`/`ForEach`; `continue` in a `range(...)` for-loop rejected (tail-increment skip); `break` everywhere OK; Lean refuses | ✅ SHIPPED v0.1.97 |
| **PMAT-502bl** — void functions `-> None` (`Type::Unit` + `Expr::Unit`) | last stmt is a regular stmt, body → `()`; emit `fn … -> () { …; () }`; `None` annot parses to Unit; `mut` lift kept; Lean refuses; caller-visible arg mutation deferred (`&mut` aliasing) | ✅ SHIPPED v0.1.98 |
| **PMAT-502bm** — early returns / guard clauses + terminal `if/elif/else` (Python frontend; reuses `Stmt::Return` + `Expr::IfExpr`) | non-last `return <e>` → `Stmt::Return`; terminal all-return if/elif/else → nested `IfExpr` trailing return (`terminal_if_as_expr`); bare return deferred; Lean refuses early return | ✅ SHIPPED v0.1.99 |
| **PMAT-502bn** — `pass` no-op statement (frontend) | lowers to no statements (empty `Vec`); enables empty `if`/branch + `pass`-only void body; `pass`-last in a value fn still errors (no trailing return); no meta-HIR/backend change | ✅ SHIPPED v0.1.100 |
| **PMAT-502bo** — negative float literals `-3.14` (`lower_unary_op` fold) | `UnaryOp(USub, LitFloat f)` → `LitFloat(-f)` → `-3.14f64` (avoids i64-only `checked_neg`); keeps neg-float consts const-safe; float-var `-x` deferred (needs ctx typing) | ✅ SHIPPED v0.1.101 |
| **PMAT-502bp** — float-variable negation `-x` (`x: float`) (ctx-aware `UnaryOp` arm) | new ctx-aware `UnaryOp(USub)` arm in `lower_expr_in_ctx_inner`: float-typed non-literal operand → `FloatBinOp { Sub, 0.0, x }` → `(0f64 - x)`; float literals keep the v0.1.101 negative-literal fold; int negation unchanged (`checked_neg`) | ✅ SHIPPED v0.1.102 |
| **PMAT-502bq** — float augmented assignment `x += y` / `-= *= /=` (`combine_aug` float branch) | `combine_aug` takes the AST op + routes F64 operands to `FloatBinOp` (infix `(x + y)`) instead of the i64-only `checked_*`; float branch runs *before* `lower_binop` so `/=` (true division) works; covers `name`/`d[k]`/`xs[i]` targets; int aug unchanged (`checked_*`), str `+=` still `format!` | ✅ SHIPPED v0.1.103 |
| **PMAT-502br** — float floor-division `//` + modulo `%` (`FloatOp::FloorDiv`/`Mod`) | new `FloatOp::FloorDiv`/`Mod` variants; `float_op_from_ast` maps `//`/`%`; codegen emits Python floor semantics `(a / b).floor()` + `a - b * (a / b).floor()` (mod follows divisor sign, not Rust dividend-sign); regular + aug (`//=`/`%=`); Lean → `Float.floor (a / b)` | ✅ SHIPPED v0.1.104 |
| **PMAT-502bs** — Python 3 true division `/` → float (`to_f64_operand` cast) | `/` always yields f64, even `int / int` (`7/2==3.5`); both expr-position BinOp arms special-case Div → `FloatBinOp { Div, .. }` with non-float operands wrapped in `(x) as f64` (`NumCast`); fixes mixed `float / int_literal` (`f64 / i64` mismatch); `float / float` cast-free; int `/=` stays unsupported (would retype binding) | ✅ SHIPPED v0.1.105 |
| **PMAT-502bt** — float power `a ** b` → `(a).powf(b)` (`FloatOp::Pow`) | new `FloatOp::Pow`; both expr-position BinOp arms special-case `**` w/ a float operand, cast operands to f64, emit `(a).powf(b)`; unlocks negative/fractional exponents (`2.0 ** -1`, `9 ** 0.5`); `int ** int` unchanged (`checked_pow`); Lean → `Float.pow a b`. **Completes float arithmetic family (`+ - * / // % **`).** | ✅ SHIPPED v0.1.106 |
| **PMAT-502bu** — float aug-assign w/ non-float rhs + `**=` (`combine_aug` cast) | `combine_aug` float branch now casts both operands via `to_f64_operand` (fixes `x += 1`/`x /= 2`/etc. emitting `f64 op i64`); `float_op_from_ast` maps `**` so `x **= 2` → `(x).powf(..)` (was int `checked_pow`); `x += y` (both float) cast-free; int aug unchanged. **Rounds out float aug-assign (`+= -= *= /= //= %= **=`).** | ✅ SHIPPED v0.1.107 |
| **PMAT-502bv** — bare `return` (no value) in void fn | Python `return None`; in a void fn (`fn_return_type == Unit`) → `Stmt::Return(Expr::Unit)` → `return ();`; enables early-exit guard clause `if invalid: return`; value-returning fn keeps rejecting (clearer message) | ✅ SHIPPED v0.1.108 |
| **PMAT-502bw** — `print(...)` builtin (`Stmt::Print`) | new `Stmt::Print(Vec<Expr>)`; frontend detects `print` call before list-method/subprocess paths; rust/ruchy emit `println!("{} {} …", …)` (single-space sep, newline); bare `print()`→`println!()`; f-strings (String) print; int/str only first cut (bool/float/sep=/end=/file= deferred); Lean refuses (no IO). **Byte-identical stdout to python3.** | ✅ SHIPPED v0.1.109 |
| **PMAT-502bx** — `print` of bool/float args | bool arg → `str(bool)` desugar (`"True"/"False"`, capitalised); float arg → `str(float)` block (`ToStr{of_float}`, whole floats get `.0`); reuses existing machinery; list/dict/set repr deferred; **byte-identical stdout to python3** | ✅ SHIPPED v0.1.110 |
| **PMAT-502by** — `print(sep=…, end=…)` kwargs | `Stmt::Print` carries `sep`/`end` (string literals, defaults `" "`/`"\n"`); args joined by `sep`; `end == "\n"` → `println!`, else `print!` + `end` appended (`end=""` concatenates); `escape_format_literal` escapes `{}`/`"`/`\`/ctrl; non-literal sep/end + `file=` deferred; **byte-identical stdout to python3** | ✅ SHIPPED v0.1.111 |
| **PMAT-502bz** — chained assignment `x = y = z = <literal>` | `lower_chained_assign` desugars to one binding per target; plain-Name targets + scalar-literal value only (re-lower per target, independent copies); `walk_counts` counts every Name target so later mutation lifts `let mut`; non-Name/non-literal deferred; list/dict aliasing out of scope (value semantics) | ✅ SHIPPED v0.1.112 |
| **PMAT-502ca** — `enumerate(xs, start)` 2-arg | `PairIterKind::Enumerate { start: i64 }`; frontend accepts optional int-literal start; backends offset index `__i as i64 + start` (omitted at start 0); `enumerate(xs)` unchanged; non-int/non-literal start deferred | ✅ SHIPPED v0.1.113 |
| **PMAT-502cb** — `str.format` positional `{N}` | `parse_format_placeholders` accepts all-automatic `{}` or all-positional `{N}` (mixing rejected, per Python); positional validates in-range + every arg used (Rust requires it); fmt re-emitted verbatim (Rust shares syntax); reorder/repeat work; `{name}`/`{:spec}` deferred | ✅ SHIPPED v0.1.114 |
| **PMAT-502cc** — context-aware `not <bool var>` | new ctx-aware `UnaryOp(Not)` arm in `lower_expr_in_ctx_inner` uses `infer_type_in_ctx` → `not b` for a `bool` param/local lowers to `(!b)` (was rejected by the context-free path, which mis-infers a bare Ident as I64); non-Bool still errors; `not (cmp)` unchanged | ✅ SHIPPED v0.1.115 |
| **PMAT-502cd** — string indexing `s[i]` (`Expr::StrCharAt`) | str receiver → 1-char string; backends materialise `chars().collect::<Vec<char>>()` + index (Rust String has no positional `[]`); negative index from end, out-of-range panics (≈ IndexError); positive/negative/var int indices; Lean refuses | ✅ SHIPPED v0.1.116 |
| **PMAT-502ce** — context-aware `and`/`or` over bool vars | new `lower_bool_op_in_ctx` (BoolOp arm in `lower_expr_in_ctx_inner`) uses `infer_type_in_ctx` → `a and b` for bool params/locals folds to `(a && b)`/`(a \|\| b)` (was rejected by the context-free path mis-inferring Idents as I64); non-Bool still errors; mixed `active and x > 0` works | ✅ SHIPPED v0.1.117 |
| **PMAT-502cf** — dict comprehension over `d.items()` | `desugar_dict_comp` tuple-target branch: `{k: f(v) for k, v in d.items()}` over a `list[tuple[K,V]]` → `ForEachPair { Pairs }` loop building the dict (mirrors the `for k, v in d.items()` stmt); `if` filter composes; non-2-name targets / non-2-tuple iterables rejected | ✅ SHIPPED v0.1.118 |
| **PMAT-502cg** — list & set comprehensions over `d.items()` | `desugar_list_comp` + `desugar_set_comp` tuple-target branches (mirror the dict-comp one): `[f(k,v) for k,v in d.items()]` / `{f(k,v) for k,v in d.items()}` → `ForEachPair { Pairs }` loop append/add; `if` filter composes. **Completes comprehension-over-items family (list/dict/set).** | ✅ SHIPPED v0.1.119 |
| **PMAT-502ch** — `str.format` with specs `{:.2f}`/`{:05d}` | `lower_str_format` rebuilds the template, translating each Python spec → Rust via `translate_format_spec` per arg type; auto `{}`/`{:spec}` + positional `{N}`/`{N:spec}`; float arg admitted with a `.Nf` spec; `{{`/`}}`/literals preserved; every arg must be used; `{name}` deferred | ✅ SHIPPED v0.1.120 |
| **PMAT-502ci** — `for i in reversed(range(...))` | for-loop unwraps a `reversed(<range>)` wrapper → descending range (step-1 `a..b` → start `b-1`, stop `a-1`, step `-1`, via `BinOp::Sub`); non-default step + BigInt-mode deferred; plain range unchanged | ✅ SHIPPED v0.1.121 |
| **PMAT-502cj** — `list(range(...))` + `list(xs)` | new `Expr::RangeList { start, stop, step }`; backends emit `((start)..(stop)).collect::<Vec<i64>>()` (`+ .step_by` for positive step); detected on the AST; positive literal step only (negative deferred); `list(<list>)` → identity copy (value semantics); Lean refuses | ✅ SHIPPED v0.1.122 |
| **PMAT-502ck** — for-loop over a call iterable | for-loop gate changed from `!Call` to `!is_range_like_call` → only `range(...)`/`reversed(range(...))` drive the counter-while; any other call that lowers to `List` (`reversed(xs)`/`sorted(xs)`/`list(range(n))`) goes through the collection `ForEach` path; range/reversed-range unchanged | ✅ SHIPPED v0.1.123 |
| **PMAT-502cl** — string iteration `for c in s` | new `Expr::StrChars` → `(s).chars().map(\|c\| c.to_string()).collect::<Vec<String>>()` (a `list[str]`); for-loop's Str-iter case wraps the string in `StrChars` and emits `ForEach` (so `.iter().cloned()` yields `String`s, loop var binds `str`); Lean refuses | ✅ SHIPPED v0.1.124 |
| **PMAT-502cm** — `ord(c)` / `chr(n)` builtins | new `Expr::Ord` (str → `((c).chars().next().expect(…) as i64)`) + `Expr::Chr` (int → `char::from_u32((n) as u32).expect(…).to_string()`, OOB panics ≈ ValueError); fixes the prior silent miscompile (undefined `ord`/`chr` Rust fn); compose; Lean refuses | ✅ SHIPPED v0.1.125 |
| **PMAT-502cn** — 2-arg `min`/`max` over str/bool | `NumBuiltin` intercept type guard made op-specific: `abs` numeric-only; `min`/`max` also accept `Str`/`Bool` (all `Ord`); existing `(a).min(b)`/`(a).max(b)` codegen resolves; fixes the str fall-through (undefined `min(...)` fn) | ✅ SHIPPED v0.1.126 |
| **PMAT-502co** — no-arg `str.split()` (whitespace) | new `StrMethodOp::SplitWhitespace` (0-arg) → `(s).split_whitespace().map(\|c\| c.to_string()).collect::<Vec<String>>()` (collapses runs, drops empties — matches Python); frontend special-cases `split` w/ 0 args before generic dispatch; `s.split(sep)` unchanged | ✅ SHIPPED v0.1.127 |
| **PMAT-502cp** — tuple literals as list elements | added the `ast::Expr::Tuple` → `TupleLit` arm to context-free `lower_expr` (list elements lower context-free); `[(1,2),(3,4)]` and iterating it (`for a,b in …`) now work; the ctx-aware path already had the arm | ✅ SHIPPED v0.1.128 |
| **PMAT-502cq** — `str.removeprefix`/`removesuffix` | new `StrMethodOp::RemovePrefix`/`RemoveSuffix` (1-arg, → Str) → block over `str::strip_prefix`/`strip_suffix` returning the receiver unchanged when the affix is absent (matches Python 3.9+); Lean refuses | ✅ SHIPPED v0.1.129 |
| **PMAT-502cr** — `str.swapcase()` | new `StrMethodOp::SwapCase` (0-arg, → Str) → recv-once `.chars().map(\|c\| upper↔lower).collect::<String>()`; non-cased chars unchanged (matches Python); Lean refuses | ✅ SHIPPED v0.1.130 |
| **PMAT-502cs** — `str.zfill(width)` | new `StrMethodOp::ZFill` (1-arg, → Str) → block-form sign-aware zero-pad (leading `-`/`+` kept first, zeros after; already-wide unchanged; char-count width); Lean refuses | ✅ SHIPPED v0.1.131 |
| **PMAT-502ct** — default parameter values | `FnSig.defaults` records each param's default (AST, from ruff `ArgWithDefault`); `reorder_kwargs_to_positional` fills omitted trailing args with keyword→default→error (kwarg + short-positional calls); Rust fn keeps all params, defaults materialised at call sites | ✅ SHIPPED v0.1.132 |
| **PMAT-502cu** — `str.center(width)` | new `StrMethodOp::Center` (1-arg, → Str, block-form) → CPython-exact parity bias `left = marg/2 + (marg & width & 1)` (`"ab".center(5)`=`"  ab "`); already-wide unchanged; **completes justify family (rjust/ljust/center)**; Lean refuses | ✅ SHIPPED v0.1.133 |
| **PMAT-502cv** — `hex`/`oct`/`bin` builtins | new `Expr::IntRadixStr { value, radix }` (+`Radix` enum); sign-first `0x`/`0o`/`0b` radix string via `format!("{}0x{:x}", sign, n.unsigned_abs())` (i64::MIN-safe); fixes the prior silent miscompile (undefined `hex`/etc. Rust fn); Lean refuses | ✅ SHIPPED v0.1.134 |
| **PMAT-502cw** — `set(xs)` materialization | new `Expr::SetFromList` → `(xs).iter().cloned().collect::<HashSet<_>>()` (de-dup); types `set[T]` over the list elem; only empty `set()` worked before; `tuple(xs)` deferred (variable arity); Lean refuses | ✅ SHIPPED v0.1.135 |
| **PMAT-502cx** — `sum(xs, start)` 2-arg | `Expr::Sum` gains optional `start` → `(start) + xs.iter().sum::<T>()`; fixes the prior silent miscompile (2-arg fell to undefined `sum(xs, start)` Rust fn); frontend requires `start` to match the element type (no cast); Lean refuses | ✅ SHIPPED v0.1.136 |
| **PMAT-502cy** — `pow(a, b)` builtin | frontend-only desugar to the `a ** b` machinery (float `powf` / int `checked_pow`); fixes the prior silent miscompile (fell to undefined `pow(a, b)` Rust fn); no new meta-HIR variant; 3-arg `pow(a,b,mod)` deferred | ✅ SHIPPED v0.1.137 |
| **PMAT-502cz** — variadic `min`/`max` | `min`/`max` accept `>= 2` args, chaining `(a).max(b).max(c)`; fixes the prior silent miscompile (3-arg fell to undefined `max(...)` Rust fn); `Expr::NumBuiltin` already held `args: Vec`, so frontend + codegen only (no new variant) | ✅ SHIPPED v0.1.138 |
| **PMAT-502da** — `int(s, base)` radix parse | new `Expr::IntFromStrRadix { value, radix }` → `i64::from_str_radix((s).trim(), base)`; fixes the prior silent miscompile (2-arg `int` fell to undefined `int(s, base)` Rust fn); `base` a literal `2..=36`; `0x`-prefixed strings + variable base deferred; Lean refuses | ✅ SHIPPED v0.1.139 |
| **PMAT-502db** — context-aware ternary branches | builtins in a ternary branch (`abs(n) if … else …`) were silently miscompiled to undefined Rust fns — the `IfExp` fell to the context-free `lower_expr`; new `lower_if_exp_in_ctx` lowers cond + both branches context-aware (frontend-only, reuses `Expr::IfExpr`) | ✅ SHIPPED v0.1.140 |
| **PMAT-502dc** — context-aware comparison operands | builtins in a comparison operand (`abs(n) > 0`, `len(s) > 3`) were silently miscompiled — the ctx-aware `Compare` arm delegated regular comparisons to the context-free `lower_compare`; new `lower_compare_in_ctx` lowers operands context-aware (frontend-only); chained compares fold. **Remaining ctx-free-position siblings: list literals, subscript indices, unary `-` (PMAT-502dd+).** | ✅ SHIPPED v0.1.141 |
| **PMAT-502dd** — context-aware collection literals | builtins in list/dict/set literals (`[abs(a), abs(b)]`, `{"k": abs(v)}`, `{abs(a), abs(b)}`) were silently miscompiled — the literal AST nodes had no ctx-aware arm; new `lower_{list,dict,set}_literal_in_ctx` lower elements context-aware (frontend-only). **Remaining: subscript indices, unary `-` (PMAT-502de).** | ✅ SHIPPED v0.1.142 |
| **PMAT-502de** — context-aware subscript index + unary `-` | builtins in `xs[abs(i)]` / `-abs(n)` were silently miscompiled — the general list-index path fell to context-free `lower_expr` + the `USub` arm re-lowered via context-free `lower_unary_op`; both now lower context-aware (frontend-only, negative-float-literal fold preserved). **CLOSES the ctx-free-position silent-miscompile class** (ternary/comparison/collection-literal/index/unary all fixed; boolop+binop+tuple were already correct). | ✅ SHIPPED v0.1.143 |
| **PMAT-502df** — generator expressions | `sum(f(x) for x in xs)` / `max(abs(x) for x in xs)` / `list(x*2 for x in xs)` were rejected; a `GeneratorExp` now desugars to `Expr::Map` (List-producing), so every List-consuming builtin accepts it (frontend-only, reuses Map+Sum+ListMinMax). Iterable = `range(...)` or list-typed; `if` filter / multi-generator / tuple target deferred. | ✅ SHIPPED v0.1.144 |
| **PMAT-502dg** — filtered generator expressions | a single `if <cond>` clause wraps the iterable in `Expr::Filter` (List-typed), so `Map` composes over it: `sum(x for x in xs if x>0)` → `Map(Filter(iter))` (frontend-only, reuses Filter+Map). Multiple `if` / multi-generator / tuple target deferred. | ✅ SHIPPED v0.1.145 |
| **PMAT-502dh** — `min`/`max(xs, default=)` | the empty-safe `default=` kwarg now returns the default instead of panicking; `Expr::ListMinMax` gains a `default` field → `.unwrap_or(<default>)` (int/key branches) and `.reduce(f64::min/max).unwrap_or(<default>)` (float branch). | ✅ SHIPPED v0.1.146 |
| **PMAT-502di** — `str.isupper`/`.islower`/`.isalnum` | three more 0-arg classification predicates (→ Bool); `isalnum` uses the empty-guarded all-chars shape, `isupper`/`islower` use Python's cased-char rule (`any(uppercase) && !any(lowercase)`); new `StrMethodOp` variants, Lean refuses `StrMethod`. str-method family ~29. | ✅ SHIPPED v0.1.147 |
| **PMAT-502dj** — `str.partition`/`.rpartition` | the 3-tuple `(before, sep, after)` via `split_once`/`rsplit_once` (→ `tuple[str,str,str]`); absent-`sep` case differs (`(s,"","")` vs `("","",s)`); new `StrMethodOp` variants (block-form), inferer returns a 3-Str `Tuple`; Lean refuses. First str method returning a tuple. | ✅ SHIPPED v0.1.148 |
| **PMAT-502dk** — `dict(pairs)` | `dict(<list[tuple[K,V]]>)` → new `Expr::DictFromPairs` → `(<pairs>).iter().cloned().collect::<HashMap<_,_>>()`; types `dict[K,V]`; also covers `dict(zip(a,b))` / `dict(enumerate(xs))` (those produce 2-tuple lists); Lean refuses. Mirror of `set(xs)`. | ✅ SHIPPED v0.1.149 |
| **PMAT-502dl** — `str.splitlines()` | split on Python's full line-boundary set (LF/CR/CRLF/VT/FF/FS/GS/RS/NEL/LS/PS) via an explicit char-walk (Rust `str::lines()` only covers LF/CRLF → would diverge); no trailing empty for a trailing break; new 0-arg `StrMethodOp::SplitLines` (block-form), Lean refuses. `keepends=True` deferred. | ✅ SHIPPED v0.1.150 |
| **PMAT-502dm** — printf `"<tmpl>" % args` | `%`-operator with a str-literal LHS → a Rust `format!` (reuses `Expr::StrFormat`); `%s`(int/str)/`%d`/`%i`/`%f`(→`{:.6}`)/`%%`, single or tuple RHS; `%s` over bool/float + `%x`/`%X`/`%o` + width/precision rejected (silent-divergence-safe). | ✅ SHIPPED v0.1.151 |
| **PMAT-502dn** — printf `%`-format width/precision/flags | `[flags][width][.precision]` → Rust specs (`%.2f`→`{:.2}`, `%5d`→`{:>5}`, `%-5d`→`{:<5}`, `%05d`→`{:05}` sign-aware, `%+d`→`{:+}`); explicit `>` since Python right-aligns `%Ns` (Rust left-aligns strings); `%.Nd`/`%.Ns`-over-int + ` `/`#` flags still rejected. | ✅ SHIPPED v0.1.152 |
| **PMAT-502do** — `%s` over bool/float | str()-converts the arg first (bool → `IfExpr("True"/"False")`, float → `ToStr{of_float}` Python repr) so `{}` yields Python's `str(x)`; width/precision then apply to the resulting `String`. Removes the v0.1.151 deferral. | ✅ SHIPPED v0.1.153 |
| **PMAT-502dp** — printf `%x`/`%X`/`%o` | wraps the int arg as a no-prefix sign-first radix string (`IntRadixStr` gains `prefixed`/`upper`; Rust `{:x}` is two's-complement for negatives, Python sign-first) rendered via `{}`; `%X` upper-case; width-only (no `0`/`+`/precision). Completes the printf conversion set. | ✅ SHIPPED v0.1.154 |
| **PMAT-502dq** — varargs `*args` | a `*args` param → a `list[elem]` parameter (body uses it as a list); `FnSig` gains `variadic`; call sites collect trailing positional args into a `vec![…]` (empty → `vec![]`, type inferred from the sig). Mixed fixed+vararg supported; `**kwargs`/kwonly still rejected. First structural slice past the builtin/str surface. | ✅ SHIPPED v0.1.155 |
| **PMAT-502dr** — nested functions | a nested `def inner(p: T,…) -> R: return <e>` → `Stmt::ClosureLet` (`let inner = \|p: T,…\| { <e> }`), reusing the closure machinery; annotated param types + `-> R` return; captures enclosing locals. Single-`return` body only (multi-stmt deferred); `*args`/`**kwargs`/decorators rejected; Lean refuses. | ✅ SHIPPED v0.1.156 |
| **PMAT-502ds** — `f(*xs)` splat into variadic | a `*`-splat covering the whole vararg tail (`f(fixed…, *xs)`) passes the list directly (`f(fixed…, xs)`); splatted expr must be list-typed; mixed/fixed-slot/non-variadic splats deferred. Completes the varargs feature. | ✅ SHIPPED v0.1.157 |
| **PMAT-502dt** — block expressions + multi-stmt nested fns | new `Expr::Block` (stmts + trailing value → Rust `{ … }`, a reusable primitive); a nested `def` body may now be multiple statements ending in `return <e>` (the leading stmts + trailing become the closure's block body); early `return` returns from the closure; scope snapshot/restored; Lean refuses. | ✅ SHIPPED v0.1.158 |
| **PMAT-502du** — expr-position list comprehensions | `sum([x*x for x in xs])` / `max([…])` / `len([… if …])` lower through the genexpr `Map`/`Filter` machinery (typed as a List); the statement form `name=[comp]` + return special-case still use the for-append desugar; shares the loop-var-unbound limitation (str-method elts need the statement form). | ✅ SHIPPED v0.1.159 |
| **PMAT-502dv** — expr-position set/dict comprehensions | `len({x for x in xs})` / `len({k: v for x in xs})` lower via the same `Map`/`Filter` form wrapped in `SetFromList` (set) / `DictFromPairs` over a `(key,value)`-tuple `Map` (dict); statement + return forms keep their desugars. List/set/dict comps + genexprs now all work in expression position. | ✅ SHIPPED v0.1.160 |
| **PMAT-502dw** — `{**d1, **d2}` dict merge | new `Expr::DictMerge` → `(a).iter().chain((b).iter())….map(clone).collect::<HashMap>()`; chaining means a later dict wins on a key collision (matching Python); ≥2 all-splat entries; mixed splat+explicit deferred; Lean refuses. | ✅ SHIPPED v0.1.161 |
| **PMAT-502dx** — mixed `{**a, "k": v}` dict | generalizes `DictMerge` to `entries: Vec<(Option<k>, v)>` (splat = `None`, explicit = `Some`); chains `std::iter::once((k,v))` per pair + `(d).iter().map(clone)` per splat → later entry wins. Handles `{**defaults, "x": v}` / `{"x": v, **a}`. | ✅ SHIPPED v0.1.162 |
| **PMAT-502dy** — nested subscript assignment | `grid[i][j] = v` (2D/ND list grids) → `Stmt::IndexAssign` gains an `indices` path; frontend peels the subscript chain, requires `list[list[…]]` of matching depth + all-`int` indices → `grid[i as usize][j as usize] = v`. Single `xs[i]`/dict `d[k]` unchanged; nested aug-assign + dict-nested deferred. | ✅ SHIPPED v0.1.163 |
| **PMAT-502dz** — `for _ in range(n)` underscore targets | the range-for/comp counter desugar emitted `let mut _: i64` for a `_` target — invalid Rust (`_` is not a binding), so `for _ in range(n)` / `[… for _ in range(n)]` never compiled. Frontend mints a fresh unique `__xpile_idx{N}` counter and resolves body reads of `_` to it; nested `for _` get distinct counters (outer increment can't hit the inner shadow). Statement-form + list/dict/set comp range desugars; expr-position comps already used `(0..n).map(\|_\| …)` (valid closure param). | ✅ SHIPPED v0.1.164 |
| **PMAT-502ea** — nested augmented subscript assign | `grid[i][j] += v` (2D/ND) → `grid[i][j] = grid[i][j] <op> v`, reusing nested `IndexAssign` write + nested `Index` read (peel/validate shared with plain `= v`). Also fixes a latent mutability gap: the count pre-walk now peels subscript chains to the base Name at any depth for BOTH plain & augmented assign, so a literal-initialised receiver mutated only via `xs[i] += v` / `grid[i][j] = v` is correctly emitted `let mut` (previously only worked for comprehension-built receivers). | ✅ SHIPPED v0.1.165 |
| **PMAT-502eb** — `xs += ys` list in-place extend | `xs += ys` over a list is Python in-place extend, not numeric add. The aug-assign handler routed `+=` through `combine_aug` → `(xs).checked_add(ys)` (no such method on `Vec` → silent miscompile). Name-target arm now detects a list receiver and emits `Stmt::ListExtend` (= `xs.extend(ys)`); `list += <non-list>` and non-`+=` ops on a list are clean errors. Numeric/str/subscript aug-assign unchanged. | ✅ SHIPPED v0.1.166 |
| **PMAT-502ec** — empty list `[]` annotation threading | `xs: list[int] = []` / `return []` rejected ("empty `[]` needs annotation") while empty `{}`/`set()` already threaded. `lower_ann_assign` special-cases empty `[]` against the declared `list[T]`; both return paths route empty `[]`/`{}` through `lower_value_expecting` using `fn_return_type` (any element type — `list[str]`, nested); trailing-return equality check tolerates the empty literal (which `infer_type` defaults to `list[int]`). Unannotated `xs = []` still unsupported. | ✅ SHIPPED v0.1.167 |
| **PMAT-502ed** — f-string lone field + int specs | (1) lone `f"{n}"` (no text/spec) over an `int` returned the bare value (typed `i64`, failed `-> str`) — now stringified via `format!("{:}", n)`. (2) int format specs `:x`/`:X`/`:b`/`:o` (radix), bare width `:5`, zero-pad `:05`/`:04x`/`:08b` translate (Rust int spec syntax matches Python). Int-only for the new forms; lone `float`/`bool` field + float bare-width deferred (repr disagrees: `3.0`→`3`, `true`→`True`). | ✅ SHIPPED v0.1.168 |
| **PMAT-502ee** — `bool` f-string field → `True`/`False` | a bool in an f-string (`f"flag={flag}"`) rendered Rust lowercase `true`/`false` instead of Python `True`/`False` — a silent miscompile (compiled, wrong string). Bool field now desugars to `"True" if b else "False"` (shared `bool_to_python_str`, same as `str(bool)`/`print`/`%s`); also un-defers lone `f"{flag}"` (→ `Str`). | ✅ SHIPPED v0.1.169 |
| **PMAT-502ef** — `float` f-string field → Python repr | a float in an f-string (`f"v={x}"`) rendered Rust `Display` (`3` for whole `3.0`) instead of Python `3.0` — silent miscompile. Float field now reuses `Expr::ToStr { of_float: true }` (same as `str(float)` — nan / `.0` / frac logic); also un-defers lone `f"{x}"`. Completes int/bool/float Python-faithful stringification across str/print/%s/f-string. | ✅ SHIPPED v0.1.170 |
| **PMAT-502eg** — `xs.remove(x)` list remove-by-value | the one unimplemented in-place list mutator (`.append`/`.insert`/`.pop`/`.extend`/`.sort`/`.reverse`/`.clear` already shipped). New `Stmt::ListRemoveValue` → position-find + `Vec::remove`, panicking ≈ Python `ValueError` when absent. Distinct from set `.remove` (by key); receiver type disambiguates. Lean refuses (in-place mutation). | ✅ SHIPPED v0.1.171 |
| **PMAT-502eh** — `d.setdefault(k, v)` bare statement | the value-position form (`x = d.setdefault(...)`) already worked; the bare statement (the "ensure key exists" loop idiom) was rejected. Reuses `Expr::DictSetDefault` (validates arity/types), discards via `let _ = …;`; the mutability pre-walk now scans bare expr-statements so the dict is `let mut`. | ✅ SHIPPED v0.1.172 |
| **PMAT-502ei** — bare callable as `key=` (min/max/sorted) | only `key=lambda p: e` was accepted; `key=abs`/`key=len`/`key=fn` are now synthesized into `lambda __xpile_k: <name>(__xpile_k)` (built as a call AST + lowered) and routed through the same `SortKey` path (shared `lower_sort_key` helper). Composes with the `min_by_key`/`max_by_key`/`sort_by_key` codegen. (`sorted(...)[i]` direct-index is a separate pre-existing block-index limit — assign first.) | ✅ SHIPPED v0.1.173 |
| **PMAT-502ej** — direct-index of a block-producing collection | `sorted(xs)[0]`/`reversed(xs)[0]` lower to a Rust block `{ … }`; `{block}[i]` mis-parses → silent rustc-fail. `Expr::Index` codegen (rust+ruchy) now parenthesizes a collection opening with `{` → `({block})[i]`. Plain/nested index unchanged. | ✅ SHIPPED v0.1.174 |
| **PMAT-502ek** — `math` module functions | first cut: `import math` accepted+skipped; `math.sqrt`/`math.floor`/`math.ceil` lower to `Expr::NumBuiltin` (Sqrt→`(x).sqrt()` float; Floor/Ceil→`(x).floor()/.ceil() as i64` int). Other `math.*` + `math.pi`/`math.e` constants are follow-ups. Any plain `import <module>` is skipped (uses decided at the call site). Lean refuses (NumBuiltin). | ✅ SHIPPED v0.1.175 |
| **PMAT-502el** — more `math` (constants + trig/log) | constants `math.pi`/`math.e`/`math.tau` (bare attr read → `Expr::LitFloat`); functions `sin`/`cos`/`tan`/`exp`/`log`(ln)/`log10`/`log2` (→ `Expr::NumBuiltin`, f64 method emit). `math.inf`/`math.nan` + 2-arg `log(x,b)`/`pow` deferred. | ✅ SHIPPED v0.1.176 |
| **PMAT-502em** — `math.pow` + `math.trunc` | `math.pow(x,y)` always float (even int args) → reuse `FloatBinOp{Pow}` w/ f64-coerced operands → `(x).powf(y)`; `math.trunc(x)` toward-zero int → new `NumBuiltinOp::Trunc` → `(x).trunc() as i64`. 2-arg `log(x,base)`, `hypot`/`atan2`, `inf`/`nan` deferred. | ✅ SHIPPED v0.1.177 |
| **PMAT-502en** — 2-arg `math` (hypot/atan2/log-base) | `math.hypot(x,y)`/`math.atan2(y,x)`/2-arg `math.log(x,base)` → reuse `Expr::FloatBinOp` (3 new `FloatOp` variants, f64-coerced operands) emitting `(a).hypot(b)`/`.atan2(b)`/`.log(b)`. 1-arg `log` stays natural log (`Ln`); arity selects. Lean defers. `inf`/`nan` still deferred. | ✅ SHIPPED v0.1.178 |
| **PMAT-502eo** — set-algebra methods | `a.union(b)`/`a.intersection(b)`/`a.difference(b)`/`a.symmetric_difference(b)` (method forms of `\|`/`&`/`-`/`^`) → reuse `Expr::SetOp` (no new IR). Recognised in the attribute-call dispatch when the receiver types as a set; the arg must also be a set. | ✅ SHIPPED v0.1.179 |
| **PMAT-502ep** — set predicates (subset/superset/disjoint) | methods `issubset`/`issuperset`/`isdisjoint` + operators `<=`/`<`/`>=`/`>` over two sets → new bool-returning `Expr::SetPred` (temp-block over `is_subset`/`is_superset`/`is_disjoint`; proper `<`/`>` add `&& __l != __r`). The operators were a SILENT MISCOMPILE (lowered to ordering `BinOp` on HashSet → rustc reject). `==`/`!=` keep `BinOp`. Lean refuses. | ✅ SHIPPED v0.1.180 |
| **PMAT-502eq** — collection `.copy()` | `xs.copy()`/`d.copy()`/`s.copy()` (list/dict/set shallow copy) → new `Expr::Clone` → `(<inner>).clone()` (rust/ruchy); Lean emits the inner (immutable). Independent copy (mutation doesn't touch the original). 0-arg, receiver list/dict/set. | ✅ SHIPPED v0.1.181 |
| **PMAT-502er** — `min`/`max` over `list[str]` | 1-arg `min(xs)`/`max(xs)` reduction widened from int/float to also `str`/`bool` (both `Ord`); codegen non-float path switched `.copied()`→`.cloned()` (so non-Copy `String` works; i64/bool are `Clone`). `key=`/`default=`/float paths unchanged. | ✅ SHIPPED v0.1.182 |
| **PMAT-502es** — list splat literals | `[*a, *b]` / `[x, *a, y]` fold through `Expr::ListConcat` (each `*e` → list `e`, each plain `x` → singleton `[x]`); fresh `Vec`. A lone `[*a]` wraps in `Expr::Clone` (shallow copy, not a move). Plain list literals unchanged. | ✅ SHIPPED v0.1.183 |
| **PMAT-502et** — set splat literals | `{*a, *b}` / `{*a, x}` fold through `Expr::SetOp{Union}` (each `*e` → set `e`, each plain `x` → singleton `{x}`); fresh `HashSet`. A lone `{*a}` wraps in `Expr::Clone` (shallow copy). Parallels list splat (PMAT-502es); plain set literals unchanged. | ✅ SHIPPED v0.1.184 |
| **PMAT-502eu** — `sorted(d)` over a dict | Python iterates a dict as its keys → `sorted(d)` = sorted key list. Was a SILENT MISCOMPILE (dict fell through to an undefined `sorted(d)` typed `i64`). Now materializes keys (`Expr::DictView{Keys}`) + sorts; `reverse=`/`key=` apply; `sorted(list)` unchanged. | ✅ SHIPPED v0.1.185 |
| **PMAT-502ev** — `sorted(s)` over a str | sorts the characters → list of 1-char strings (`Expr::StrChars` + `Sorted`). Completes the `sorted(X)` family (list / dict-keys / str-chars); `reverse=`/`key=` apply. | ✅ SHIPPED v0.1.186 |
| **PMAT-502ew** — `Optional[T]` return type (Optional epic cut 1) | `-> Optional[T]` → Rust `Option<T>` via new `Type::Optional` + `Expr::OptionExpr`; return site wraps (`return None`→`None`, `return x`→`Some(x)`). `from typing import Optional` accepted+skipped; trailing-return check tolerates bare `None`. **Return-position only** — Optional params/locals + `is None` flow-narrowing deferred. Lean defers. | ✅ SHIPPED v0.1.187 |
| **PMAT-502ex** — Optional params + `is None` test (Optional epic cut 2+3) | `Optional[T]` parameter → `Option<T>`; `x is None`/`x is not None` over an Optional → new bool `Expr::IsNone` → `(x).is_none()`/`.is_some()` (intercepted pre-operand-lowering; non-Optional operand is a clean error). Narrowing-free (param only tested, not used as `T`). Lean defers. | ✅ SHIPPED v0.1.188 |
| **PMAT-502ey** — 1-arg `d.get(k)` → `Optional[V]` (Optional epic) | `.get` with no default → new `Expr::DictGetOpt` → `(d).get(&(k)).cloned()` : `Option<V>`; 2-arg `.get(k, default)` unchanged (`DictGetOr`). Inferers type it `Optional[V]`. Plus a no-double-wrap return fix: a value already typing as `Optional` (an Optional param, or another `.get(k)`) returns verbatim, not re-wrapped into `Some(Option<..>)`. Lean defers. | ✅ SHIPPED v0.1.189 |
| **PMAT-502ez** — Optional flow-narrowing (Optional epic cut 4, keystone) | After a provably-exiting `if x is None: return …`/`raise` guard, a later read of `x` → new `Expr::OptionUnwrap` → `(x).unwrap()` : `T` (so guard-then-use compiles). Sound by construction: `register_none_guard_narrowing` narrows only a non-reassigned (`!mutable`) `Optional` name guarded by an always-exiting None-check (no `else`); other shapes unchanged. Handles stacked guards + str payloads + raise-guards. Lean defers. | ✅ SHIPPED v0.1.190 |
| **PMAT-502fa** — Optional intra-branch narrowing `if x is not None:` (Optional epic) | Complement of cut 4: inside the `if x is not None:` then-body, a read of `x` → `Expr::OptionUnwrap` → `(x).unwrap()` : `T`. In `lower_if_stmt`, condition lowered first (its `x` un-narrowed), then `is_not_none_narrow_target` narrows a non-reassigned `Optional` name for the then-body only (restored after; outer-guard narrowing survives); persists into nested stmts. Scope = then-branch; `is None` else-branch + `is not None … else: return` fall-through route through if-expr/if-as-let (future sub-slices). Lean defers. | ✅ SHIPPED v0.1.191 |
| **PMAT-502fb** — bitwise invert `~x` | Python `~x` == `-(x+1)` == Rust `!x` on a signed int (`~5==-6` both). New `UnOp::BitNot` → Rust/Ruchy `(!(x))`, C-lane `!(x)`, Lean total identity `(-(x + 1))`. Ctx-aware + context-free paths; I64 operand required; not flagged int-arith (no overflow). | ✅ SHIPPED v0.1.192 |
| **PMAT-502fc** — two-generator list comprehension | `[expr for x in a for y in b]` → nested `for` loops appending to the accumulator (was a "single `for` clause" error). Dedicated `desugar_list_comp_2gen` (single-gen path untouched); plain-Name targets over `list[T]` iterables, inner iter lowered with outer var in scope, per-generator `if` wraps its loop. Return + assignment position; range/tuple-target/3+-gen + genexpr map-path deferred. | ✅ SHIPPED v0.1.193 |
| **PMAT-502fd** — two-generator dict & set comprehensions | `{k: v for x in a for y in b}` / `{e for x in a for y in b}` → nested loops inserting/adding to the accumulator. Shared `desugar_comp_2gen` helper (list/dict/set are thin `build`-closure wrappers: `ListAppend`/`DictSet`/`SetAdd`); single-gen paths untouched. Same constraints as PMAT-502fc. | ✅ SHIPPED v0.1.194 |
| **PMAT-503** — exceptions `try/except/raise` | map to `Result`/panic; R10 early-return machinery exists | 🔨 in progress — **PMAT-503a** `raise Exc("msg")` → `panic!("{}", msg)` (`Stmt::Raise`) ✅ SHIPPED v0.1.37; **PMAT-503b** value-with-fallback `try: return <expr> except [E]: return <expr>` → `Expr::TryCatch` → `catch_unwind` over xpile's panic model (catches ZeroDivisionError/IndexError/KeyError; Lean refuses) ✅ SHIPPED v0.1.199; **PMAT-503c** statement-position assignment-form `try: x = <expr> except [E]: x = <expr>` → `let x = <TryCatch>` (mutability pre-walk now descends into try arms) ✅ SHIPPED v0.1.200. **Remaining:** multi-stmt try bodies, `except E as e` bound object, type-specific dispatch, `else`/`finally` |
| **PMAT-504** — closures / `lambda` | first-class fn values | 🔨 in progress — **first cut** `f = lambda y: <body>` → `Stmt::ClosureLet` (`let f = \|y: i64\| { … }`), callable `f(x)` via `Expr::Call`; single `i64` param, return type recorded in `ctx.closure_returns`; no `Type::Closure` (Rust infers); Lean refuses ✅ SHIPPED v0.1.89. **PMAT-504b** multi-param + nullary (`ClosureLet.params: Vec<(String,Type)>`) ✅ SHIPPED v0.1.90; non-i64-param / closures-as-args follow |
| **PMAT-505** — `&str` borrowing | param-position borrow optimization | open |
| **PMAT-506** — classes / dataclasses | Python `class`/`@dataclass` → Rust struct (+ impl) | 🔨 in progress — **506a first cut** field-only / `@dataclass` class → `Item::Struct` → `#[derive(Clone, Debug, PartialEq)] pub struct …` (definition only; Lean refuses) ✅ SHIPPED v0.1.201 *(code labeled `PMAT-505a` — numbering slip; tracked as PMAT-506)*; **506b** value construction `Name(a, b)` → `Expr::StructLit` + field access `obj.f` → `Expr::FieldAccess` via new `Type::Struct` + a module struct registry; struct-typed params/returns/locals ✅ SHIPPED v0.1.202; **506c** field assignment `obj.f = v` → `Stmt::FieldAssign` → `(obj).f = v;` (receiver auto-`mut` via the pre-walk) ✅ SHIPPED v0.1.203; **506d** instance methods `def m(self, …)` → `impl Name { pub fn m(&self, …) }` + `obj.m(args)` → `Expr::MethodCall` (read-only `&self`; self-mutating rejected) ✅ SHIPPED v0.1.204; **506e** keyword construction `Name(x=1, y=2)` (positional-then-keyword fill; fields emitted in declaration order) ✅ SHIPPED v0.1.205; **506f** field defaults `x: T = <literal>` (literal-only defaults; `struct_field_defaults` registry fills omitted fields at construction in declaration order) ✅ SHIPPED v0.1.206; **506g** `@staticmethod` → no-`self` associated `pub fn` + `Class.method(args)` → `Class::method(args)` (reuses `Expr::Call` with a qualified callee registered under a `Class::method` signature key; no new IR; instance-method-via-class-name rejected cleanly) ✅ SHIPPED v0.1.207; **506h** `@classmethod` → no-receiver associated `pub fn` (the `cls` param is dropped); `cls(...)` constructs the enclosing class and `cls.method(...)` calls a sibling static/class method, resolved via a transient `ctx.cls_name`; reuses the 506g call dispatch (no new IR) — completes the decorator trio ✅ SHIPPED v0.1.208; **506i** augmented field assignment `obj.field <op>= v` → `obj.field = obj.field <op> v` (reuses `FieldAccess` + `FieldAssign`, no new IR; walk_counts counts an Attribute aug-target so the receiver is `mut`) ✅ SHIPPED v0.1.209; **506j** `@property` → read-only `&self` method; a bare attribute read `obj.prop` → no-arg `Expr::MethodCall` `(obj).prop()` (new `struct_properties` registry; only registered properties auto-call, so a bare non-property access stays a clean error — no new IR) ✅ SHIPPED v0.1.210. **Remaining:** `&mut self` (mutating) methods (needs type-aware caller-mutability), inheritance |
| **PMAT-510** — `match` statement | structural pattern matching → Rust control flow | 🔨 in progress — **first cut** literal-dispatch `match name: case <lit>: … case _: …` desugars to an `if`/`elif`/`else` chain (`desugar_match_to_if`), reusing all existing `if` lowering — no new IR, no codegen. Name subject, literal value patterns (int/float/str, optionally negated), required trailing `case _:`; works terminal (each case returns → if-expr) + statement-position (`walk_counts` descends into cases) ✅ SHIPPED v0.1.211; **PMAT-512** `\|`-patterns `case 0 \| 1 \| 2:` → OR of equality tests (`ast::Pattern::MatchOr` of literals → `BoolOp{Or}`; no new IR) ✅ SHIPPED v0.1.212; **PMAT-514** match on enums `case Color.RED:` / `case Color.RED \| Color.BLUE:` → enum-member equality (dotted value patterns; reuses PMAT-513 `EnumVariant`; no new IR) ✅ SHIPPED v0.1.214. **Remaining:** capture patterns (`case x:`), guards (`case … if …`), singletons (`True`/`False`/`None`), class/sequence/mapping patterns, non-Name subjects |
| **PMAT-513** — `Enum` classes | Python `class C(Enum)` → Rust enum | 🔨 in progress — **first cut** `class C(Enum): NAME = <int literal>` → `Item::Enum` → `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum C { NAME, … }`; member access `C.NAME` → `Expr::EnumVariant` (`C::NAME`), `C.NAME.value` → discriminant literal; enum-typed values reuse `Type::Struct` (no new `Type` variant); new `enums` registry (pre-pass), Lean refuses ✅ SHIPPED v0.1.213; **PMAT-515** `C.NAME.name` → variant-name string literal (`LitStr`, compile-time; folded with `.value`) ✅ SHIPPED v0.1.215; `case C.NAME:` match → PMAT-514 (v0.1.214). **Remaining:** `auto()`, `IntEnum`/`StrEnum`/`Flag`, enum methods, `c.value`/`c.name` on a variable, `C(1)` value-construction |
| **PMAT-516** — `str.startswith`/`endswith` tuple | correctness: tuple-of-prefixes | ✅ SHIPPED v0.1.216 — `s.startswith((a, b))` / `.endswith((…))` (Python: true if any matches) was a silent miscompile (`…starts_with(&(a,b)[..])`, can't index a tuple); now expands to an OR of per-prefix `starts_with`/`ends_with` checks (`BinOp::Or` of `StrMethod`; empty tuple → `false`; 1-arg form unaffected; no new IR) |
| **PMAT-517** — `str.replace` 3-arg | replace first N occurrences | ✅ SHIPPED v0.1.217 — `s.replace(old, new, count)` → Rust `s.replacen(&(old)[..], &(new)[..], (count) as usize)` (1:1 mapping). New `StrMethodOp::ReplaceN`; dedicated frontend branch routes `replace`/3-args (count must be int); 2-arg `Replace` unchanged |
| **PMAT-518** — `str.split` 2-arg | split with maxsplit | ✅ SHIPPED v0.1.218 — `s.split(sep, maxsplit)` → Rust `s.splitn((maxsplit) as usize + 1, &(sep)[..])…` (Python caps splits → part count = maxsplit+1). New `StrMethodOp::SplitN`; dedicated frontend branch routes `split`/2-args (maxsplit must be int); 1-arg `Split` unchanged |
| **PMAT-519** — `frozenset()` | correctness: immutable set | ✅ SHIPPED v0.1.219 — `frozenset(iterable)` was a silent miscompile (emitted an undefined `frozenset(...)` call); Rust has no frozen set, so it maps to a `HashSet` (immutable = never mutated) via the same `SetFromList`/`SetLit` path as `set()` (no new IR). `frozenset`-as-hashable-key deferred |
| **PMAT-520** — `list(set)`/`sorted(set)` | correctness: set→list | ✅ SHIPPED v0.1.220 — both were silent miscompiles (nested `set(...)` fell through to context-free lowering → undefined `set(...)`/`list(...)`); new `Expr::SetToList` → `(set).iter().cloned().collect::<Vec<_>>()`, routed from the `list()` (Set arg) + `sorted()` (Set arg) handlers; infer → `List(elem)`, Lean refuses |
| **PMAT-521** — reduce over iterable | correctness: `sum`/`max`/`min`(range/set) | ✅ SHIPPED v0.1.221 — `sum(range(n))`, `sum`/`max`/`min(set(...))` were silent miscompiles (arg fell through to context-free → undefined `range(...)`/`set(...)`); shared `materialize_iterable_arg` materialises `range(...)`→Vec + set→`SetToList`, routed from the `sum`/`min`/`max` handlers (no new IR) |
| **PMAT-522** — builtins over range/dict | correctness: `len`/`sorted`/`reversed`(range), `list`(dict) | ✅ SHIPPED v0.1.222 — `len`/`sorted`/`reversed(range(n))` were silent miscompiles (undefined `range(...)`); new `lower_arg_materializing_range` turns a `range(...)` arg into a Vec, routed from those three handlers. `list(<dict>)` → the dict's keys (`DictView{Keys}`). No new IR |
| **PMAT-523** — negative-step range | `list(range(n, 0, -1))` | ✅ SHIPPED v0.1.223 — negative-step `range` materialisation (was deferred; only the counted `for` loop worked). Python `range(start, stop, step<0)` → `((stop)+1 ..= (start)).rev().step_by(\|step\|).collect::<Vec<i64>>()`. Dropped the `s < 1` guard; rust+ruchy `RangeList` emit branches on step sign; no new IR |
| **PMAT-524** — sort-key tuple index | correctness: `key=lambda p: p[1]` | ✅ SHIPPED v0.1.224 — a `sorted`/`min`/`max` `key=` lambda indexing a tuple element was a silent miscompile (param defaulted to `i64` → `p[1]` lowered to generic `[1]` indexing, invalid on a Rust tuple). `lower_sort_key` now binds the key param to the collection's element type (`sort_target_elem_type` helper + `LoweringCtx: Clone`), so `p[1]` → `.1`; no new IR |
| **PMAT-525** — comp/genexpr typed loop var | correctness: `[p[1] for p in ps]` | ✅ SHIPPED v0.1.225 — expression-position comprehensions / generator expressions lowered the body with the loop var unbound (→ `i64`), so tuple-index bodies miscompiled and struct-field bodies were rejected. `lower_comp_to_map` now takes a body-lowering closure and binds the loop var to the iterable's element type before lowering filter + body; no new IR. (`map`/`filter` builtins + bare closures still untyped — follow-up) |
| **PMAT-526** — map/filter typed lambda param | correctness: `map(lambda p: p[1], ps)` | ✅ SHIPPED v0.1.226 — the `map()`/`filter()` builtins lowered the lambda body with the param unbound (→ `i64`), so tuple-index bodies miscompiled. The param now binds to the list's element type before lowering the body; no new IR. Closes the iterable-param type-propagation cases (comp/genexpr/sort-key/map/filter); bare closures (`Stmt::ClosureLet`) remain |
| **PMAT-527** — container truthiness | `if xs:` / `while q:` / `not d` | ✅ SHIPPED v0.1.227 — Python container truthiness in boolean conditions: a `list`/`dict`/`set`/`str` condition lowers to `len(c) != 0` (`not c` → `len(c) == 0`) via the `truthy_condition` helper at the if/if-as-let/terminal-if/while/ternary sites + the `not` arm; reuses `Len` + `BinOp` (no new IR); int/float-truthiness still rejected |
| **PMAT-528** — bare `list.pop()` statement | `while xs: xs.pop()` | ✅ SHIPPED v0.1.228 — `xs.pop()` / `xs.pop(i)` as a bare statement (discard the result) reuses the value-position pop lowering wrapped in a discard `let _ = …;` (receiver auto-`mut`); mirrors the `d.setdefault` statement form; no new IR |
| **PMAT-529** — bare `dict.pop()` statement | `d.pop(stale_key)` | ✅ SHIPPED v0.1.229 — broadens PMAT-528 from `list` to `dict` receivers: `d.pop(k)` / `d.pop(k, default)` as a bare statement reuses the value-position pop lowering wrapped in a discard `let _ = …;` (receiver auto-`mut`); emits `(d).remove(&k).unwrap()` / `.unwrap_or(default)`; no new IR |
| **PMAT-530** — `str` reverse-slice | `s[::-1]` | ✅ SHIPPED v0.1.230 — `s[::-1]` over a `str` (the list form `xs[::-1]` already lowered to `Expr::Reversed`); new `StrMethodOp::Reverse` (0 args) → `.chars().rev().collect::<String>()` (reverse by Unicode scalar value); reuses the `StrMethod` pipeline (no new `Expr` variant); composes with `.upper()[::-1]` and `s == s[::-1]` |
| **PMAT-531** — tuple target in expr-position genexpr/comp | `sum(v for k, v in d.items())` | ✅ SHIPPED v0.1.231 — closes the asymmetry where statement-position list comps supported tuple targets (`ForEachPair`) but expr-position genexprs/comps did not; `lower_comp_to_map` binds a 2-name tuple target via a Rust tuple-destructure closure param (`\|__k\| { let (k, v) = __k.clone(); … }`), splitting the element 2-tuple type; works over `d.items()`/`zip(...)`/`enumerate(...)` with an `if` filter; enables dot-product/weighted-sum idioms; no new IR |
| **PMAT-532** — in-place set/dict mutators | `s.update(t)` / `s.clear()` / `d.clear()` | ✅ SHIPPED v0.1.232 — `set.update` (asymmetric with the already-working `dict.update`) reuses `Stmt::ListExtend` (`s.extend((other).iter().cloned())`, valid for `HashSet`); `set.clear`/`dict.clear` (asymmetric with `list.clear`) reuse `Stmt::ListMutate{Clear}` (`name.clear();`, valid for `HashSet`/`HashMap`); no new IR/codegen; the mutability pre-walk already marks `clear`/`update` receivers `mut` |
| **PMAT-533** — in-place append on a subscript receiver | `g[i].append(e)` / `d[k].append(e)` | ✅ SHIPPED v0.1.233 — nested-list / dict-of-list in-place append (the bare `<name>.append` form already worked, but a subscript receiver fell through); new `Stmt::IndexAppend{base, index, elem, base_is_dict}` → list base `base[(index) as usize].push(elem)`, dict base `base.get_mut(&(index)).unwrap().push(elem)` (KeyError parity); Rust/Ruchy emit, Lean refuses; the mutability pre-walk now marks a subscript receiver's base `mut` |
| **PMAT-534** — `x in range(...)` membership | `x in range(n)` / `x not in range(a, b, s)` | ✅ SHIPPED v0.1.234 — range membership as a **bounds check**, NOT a materialized Vec (`x in range(10**9)` must not allocate); `range(n)`→`0<=x && x<n`, `range(a,b)`→`a<=x && x<b`, 3-arg literal step adds reachability `(x-a) % step == 0` (`rem_euclid` = Python floor-mod); built as meta-HIR `BinOp`/`And`, detected before the rhs is lowered (range isn't a value); `x` must type `int`; composes in genexpr filters; no new IR |
| **PMAT-535** — `int(b)` / `float(b)` over a `bool` | `sum(int(b) for b in bs)` | ✅ SHIPPED v0.1.235 — bool→numeric cast (was a miscompile: `int(bool)` emitted a bare `int(...)`, `float(bool)` rejected — handler only covered int/float/str); Python `True`/`False`→`1`/`0` (`1.0`/`0.0`); Rust allows `bool as i64` but NOT `bool as f64`, so `float(bool)` casts through `i64` first (nested `NumCast`); enables the boolean-count idiom; no new IR; found via differential python3-vs-rust hunt |
| **PMAT-536** — keyword form of `str.format` | `"{x}".format(x=n)` | ✅ SHIPPED v0.1.236 — named-field `str.format` (positional `"{}".format(n)` already worked); rewrites each `{name}` → positional `{N}` (first-occurrence order; repeats reuse the index) and passes referenced kwargs positionally to `lower_str_format`, reusing its spec translation + validation; handles reordering/repeats/specs, tolerates unused kwargs, rejects `**kwargs`/mixed/unknown fields; no new IR |
| **PMAT-537** — dict insertion order | `list(d.keys())` / `for k in d` | ⚠️ DEFERRED (known limitation) — Python dicts preserve insertion order; the transpiler emits `std::collections::HashMap` (arbitrary order), so order-observing dict ops diverge in *order* (values correct). Order-independent ops (sum/len/specific-key/`sorted`) unaffected. Proper fix needs an insertion-ordered map; generated Rust compiles standalone via `rustc` (no external crates) so `indexmap` can't be used — needs a Vec-backed ordered-map prelude across all backends. Surfaced by a differential python3-vs-rust hunt |
| **PMAT-538** — `//` / `%` with a negative divisor | `-7 // -2`, `7 % -3` | ✅ SHIPPED v0.1.237 (correctness) — `div_euclid`/`rem_euclid` only match Python for a positive divisor; Python `//` floors toward −∞ and `%` takes the divisor's sign. Emit the truncating quotient/remainder + a floor correction (subtract 1 / add divisor when remainder sign differs); positive-divisor output unchanged; BigInt `div_floor`/`mod_floor` already correct; C lane untouched; rust+ruchy; found via the differential hunt |
| **PMAT-539** — slice bounds (negatives + clamping) | `xs[:-1]`, `xs[-3:]`, `xs[1:100]` | ✅ SHIPPED v0.1.238 (correctness) — negative/OOB slice bounds **panicked** (`(lo) as usize` wraps negatives, never clamps), so `xs[:-1]`/`xs[-3:]` crashed at runtime. Emit now binds the collection, resolves each bound (neg→`(len+b).max(0)`, else `b.min(len)`), defaults lo→0/hi→len, ensures `hi>=lo`; matches Python from-end + clamp + `lo>hi`→empty; step preserved; rust+ruchy; found via the differential hunt |
| **PMAT-540** — mixed `float`/`int` compare + arithmetic | `x == 3`, `x < n`, `x * 2 + 1` | ✅ SHIPPED v0.1.239 (correctness) — mixed float/int comparison emitted `f64 == i64` (E0308) and arithmetic `f64 + i64` (E0277) — non-compiling Rust. The int operand is promoted to f64 via `to_f64_operand` (float-arith branch wraps both operands; `lower_compare_in_ctx` promotes the int side when the other is float); Python promotes numerically; no new IR; found via the differential hunt |
| **PMAT-541** — mixed-numeric `min`/`max` | `min(x, n)`, `max(x, a, b)` | ✅ SHIPPED v0.1.240 (correctness) — `min`/`max` with mixed float/int operands emitted `f64::min(i64)` (E0308); now promotes every operand to f64 via `to_f64_operand` when any is float (covers N-arg + either order); homogeneous int/float/str min-max untouched; no new IR; found via the differential hunt |
| **PMAT-542** — mixed `float`/`int` ternary | `x if b else 0` | ✅ SHIPPED v0.1.241 (correctness) — a float/int ternary was rejected (Rust `if`-expr arms must share a type) though Python yields float when either branch is float; the int branch is promoted to f64 via `to_f64_operand` (both `lower_if_exp_in_ctx` + context-free); no new IR; completes the mixed-float/int sweep (540 compare+arith, 541 min/max, 542 ternary); found via the differential hunt |
| **PMAT-543** — 2-generator comp over `range` | `[i*j for i in range(n) for j in range(m)]` | ✅ SHIPPED v0.1.242 — the 2-gen comprehension desugar handled only `list[T]` iterables; a bare `range(...)` generator iterable now materializes to a `Vec` via `lower_range_list` (mirroring 1-gen range handling) before the nested-`ForEach` build; works for list + dict comps, per-generator filters, mixed range/list; no new IR |
| **PMAT-544** — `enumerate`/`zip` over a `str` | `for i, c in enumerate(s)` | ✅ SHIPPED v0.1.243 — the paired-loop handler required a `list` iterable; a `str` iterable now materializes to `List(Str)` (1-char strings) via `Expr::StrChars` (same conversion `for c in s` uses) then uses the existing `ForEachPair` path; handles `enumerate(s[, start])` + `zip(s, …)` / `zip(…, s)`; no new IR |
| **PMAT-545** — `str.rfind` / `str.rindex` | `s.rfind("a")` | ✅ SHIPPED v0.1.244 — reverse-search mirrors of `find`/`index`: `StrMethodOp::Rfind` (last-match byte index or `-1`) + `RIndex` (panic on absence) via Rust `str::rfind`; reuses the `StrMethod` pipeline (map/arity/inferers/rust+ruchy codegen); Lean refuses generically |
| **PMAT-546** — comprehension/genexpr over a `str` | `[c.upper() for c in s]` | ✅ SHIPPED v0.1.245 — a `str` comprehension iterable materializes to `List(Str)` (1-char strings) via `Expr::StrChars` at every comp iterable site (shared `str_iter_to_chars` helper, no-op for non-str); works for list/set/dict comps + genexprs, with filters; no new IR |
| **PMAT-547** — tuple-unpack init + augment | `i, total = 0, 0; total += i` | ✅ SHIPPED v0.1.246 (correctness) — `LetTuple` now registers `ctx.bound` (so a later augment is recognised), `walk_counts` counts tuple-of-Names assign targets, and `Stmt::LetTuple` carries a per-name `mutable: Vec<bool>` → emits `let (mut a, b) = …` (only the mutated name is `mut`); fixes the multi-accumulator init idiom |
| **PMAT-548** — negative-step list slice | `xs[::-k]` | ✅ SHIPPED v0.1.247 — unbounded negative-step list slice (k ≥ 2) generalises `xs[::-1]`: lowers to `.iter().rev().step_by(k)` over the clamped range (reuses `Expr::Slice`'s `step` field, negative; codegen branches on sign); bounded negative-step + stepped str slices deferred; no new IR |
| **PMAT-549** — `math.gcd` | `math.gcd(a, b)` | ✅ SHIPPED v0.1.248 — new `Expr::Gcd` (gcd of two ints) → inline Euclidean-algorithm block over `abs` values (`gcd(0,0)==0`, always non-negative); doesn't fit method-style `NumBuiltin`; rippled to meta-HIR + both inferers (→ I64) + rust/ruchy emit; Lean refuses |
| **PMAT-550** — `math.lcm` | `math.lcm(a, b)` | ✅ SHIPPED v0.1.249 — new `Expr::Lcm` (lcm of two ints) → inline `(abs(a)/gcd)*abs(b)` block (divide-before-multiply, `lcm(0,x)==0`, non-negative); the natural pair with `gcd`, shares the `gcd`\|`lcm` `lower_math_call` branch; rippled to meta-HIR + both inferers (→ I64) + rust/ruchy emit; Lean refuses |
| **PMAT-551** — `math.factorial` | `math.factorial(n)` | ✅ SHIPPED v0.1.250 — new `Expr::Factorial` (n! of a non-negative int) → inline `checked_mul` product loop (`0!==1`, overflow panics, negative `n` panics = `ValueError`); completes the gcd/lcm/factorial math-integer trio; rippled to meta-HIR + both inferers (→ I64) + rust/ruchy emit; Lean refuses |
| **PMAT-552** — `math.isqrt` | `math.isqrt(n)` | ✅ SHIPPED v0.1.251 — new `Expr::Isqrt` (exact `⌊√n⌋`) → inline integer-Newton block with a bit-length initial guess (overflow-safe + exact for every `i64` incl. `i64::MAX`; `isqrt(0)==0`; negative `n` panics); rippled to meta-HIR + both inferers (→ I64) + rust/ruchy emit; Lean refuses |
| **PMAT-553** — `math.comb` | `math.comb(n, k)` | ✅ SHIPPED v0.1.252 — new `Expr::Comb` (binomial "n choose k") → inline incremental-product block (`min(k,n-k)` iters, partials stay integer binomials); `k>n`→0, negative args panic (`ValueError`), `checked_mul` overflow panics; rippled to meta-HIR + both inferers (→ I64) + rust/ruchy emit; Lean refuses |
| **PMAT-554** — `math.perm` | `math.perm(n, k)` | ✅ SHIPPED v0.1.253 — new `Expr::Perm` (k-permutations `n!/(n−k)!`) → inline descending-product block (`∏(n−i)`, `k` factors); `k>n`→0, negative args panic (`ValueError`), `checked_mul` overflow panics; one-arg `math.perm(n)`==`n!` reuses `Expr::Factorial`; rippled to meta-HIR + both inferers (→ I64, joined w/ `Comb`) + rust/ruchy emit; Lean refuses. Completes the `comb`/`perm` pair |
| **PMAT-555** — in-place `sort(reverse=)` | `xs.sort(reverse=True)` | ✅ SHIPPED v0.1.254 — new `ListMutateOp::SortDesc` → reversed comparator `.sort_by(\|a, b\| b.cmp(a))` (int) / `b.partial_cmp(a).unwrap()` (float); frontend in-place-mutator handler accepts a single `reverse=<bool literal>` kwarg on `sort` (`reverse=False`→plain `.sort()`); `key=` + other args still rejected; rippled meta-HIR + rust/ruchy emit; Lean refuses. (Non-mutating `sorted(xs, reverse=True)` already shipped) |
| **PMAT-556** — expr-position 2-gen comp | `sum(i*j for i in range(n) for j in range(m))` | ✅ SHIPPED v0.1.255 — expr-position **two-generator** genexpr + list/set/dict comps build a flattened `Vec` via nested loops inside an `Expr::Block` (reuses statement-position `desugar_comp_2gen` on a cloned ctx); accumulator is the block's trailing expr. New helper `lower_comp_2gen_to_block`; set→`SetFromList`, dict→`(k,v)`→`DictFromPairs`. `block_result_type` recovers the block-local accumulator type for `sum`/`max`/`min`/`len`. 3+ generators still a clean reject |
| **PMAT-557** — f-string sign flag | `f"{x:+}"` | ✅ SHIPPED v0.1.256 — Python `+` sign flag → Rust `{:+}` (composes with precision/width/zero-pad/radix: `{:+.2}`/`{:+05}`/`{:+x}`); `-` (default) dropped, space flag rejected; bare sign int-only (bare float `:+` deferred — whole-float repr divergence; explicit `.Nf` is sound); bare `:d`→plain field. All in `translate_format_spec`, no IR change |
| **PMAT-558** — f-string percent | `f"{x:.1%}"` | ✅ SHIPPED v0.1.257 — float percent `:.N%` / `:%` → `Concat(FormatSpec((x)*100.0, ".N"), "%")` (Python scales ×100, N decimals, bare `%`→default 6, append literal `%`); no IR change; int receiver rejects (whole-int promotion deferred, like `.Nf`) |
| **PMAT-559** — subscript-target swap | `xs[i], xs[j] = xs[j], xs[i]` | ✅ SHIPPED v0.1.258 — tuple-unpack with `base[idx]`/`d[k]` targets (in-place swap idiom + parallel assign). RHS (tuple literal, matching arity) lowered into temps FIRST (swap reads both before writing either), then each temp → target: Name (`Assign`/`Let`) or list/dict subscript (`IndexAssign`/`DictSet`, shared `lower_subscript_assign_target`). `walk_counts` marks subscript bases in tuple targets `mut`. All-Name tuple keeps `LetTuple`. No IR change |
| **PMAT-560** — neg-index write (correctness) | `xs[-1] = v` | ✅ SHIPPED v0.1.259 — **fixed OOB panic**: `xs[-k] = v` / `xs[-k] += v` emitted `(-k) as usize`→`usize::MAX`. Now resolves to `xs[len(<recv>) - k]` (symmetric with the read-side PMAT-502s desugar; new `neg_literal_int`); aug-assign list branch too. `IndexAssign` codegen (rust+ruchy) binds a self-referential index to a temp first (avoids `index_mut` E0502: `xs[xs.len()-1]=v` won't compile), conditional via `expr_mentions_ident` so plain-index shape is unchanged. Variable neg index (`xs[i]`, i<0 runtime) still deferred |
| **PMAT-561** — in-place keyed sort | `xs.sort(key=lambda v: e)` | ✅ SHIPPED v0.1.260 — desugars to `xs = sorted(xs, key=…, reverse=…)`, reusing `Expr::Sorted`/`SortKey` (tuple-element keys `p[1]`→`.1`, float keys via comparator). Zero new IR/codegen; receiver already `mut` (pre-walk keys on `sort`); only fires with a `key` kwarg (bare `sort()`/`sort(reverse=)` keep `ListMutate`) |
| **PMAT-562** — three-way zip | `for a, b, c in zip(x, y, z)` | ✅ SHIPPED v0.1.261 — new `Stmt::ForEachZip3` → left-nested `.zip().zip()` chain + `((a, b), c)` destructure (`x.iter().cloned().zip(y…).zip(z…)`), stops at shortest like Python; `str` arg → `StrChars`; rippled meta-HIR + rust/ruchy emit; Lean refuses. Generalizes the 2-way `ForEachPair` zip |
| **PMAT-563** — multi-`if` comp filters | `[x for x in xs if a if b]` | ✅ SHIPPED v0.1.262 — multiple `if` clauses ANDed (`… if a and b`). New `combine_comp_filters` folds all clauses into a left-nested `&&` chain; `comp_filter` + list-comp + `lower_comp_to_map` filter sites route through it; all `ifs.len()>1` rejects removed (2 sites previously silently dropped extra filters — latent miscompile). Covers list/set/dict comps, genexprs, comp-over-range, 2-gen, N filters. Zero new IR |
| **PMAT-564** — `len(str)` Unicode (correctness) | `len("café")` == 4 | ✅ SHIPPED v0.1.263 — **fixed silent miscompile**: `len(str)` emitted `s.len()` (UTF-8 bytes) → 5 not 4. New `StrMethodOp::CharCount` → `.chars().count() as i64`; both `len()` sites route str args to it; list/dict `len` unchanged. Also fixes `key=len` over strings. Found by the differential hunt. Rippled meta-HIR + inferers + arity + rust/ruchy; Lean refuses |
| **PMAT-565** — `bool` is `int` subtype (correctness) | `True+True`==2, `sum(x>0 for x in xs)` | ✅ SHIPPED v0.1.264 — **fixed invalid-Rust**: bool operand in int arith → `checked_add` on `bool` (E0599); `sum(list[bool])` → bare `sum()` (E0425) + counting-genexpr reject; `True in list[int]` → `contains(&true)` (E0308). FIX: coerce bool→i64 (`(b) as i64`) via new `to_i64_operand`/`is_int_arith_binop` in both binop paths; `sum(bool)` maps bool→i64; membership needle coerced. Zero new IR. Found by the differential hunt |
| **PMAT-566** — `str.find` char index (correctness) | `"αβγδ".find("γ")` == 2 | ✅ SHIPPED v0.1.265 — **fixed silent miscompile**: find/rfind/index/rindex returned Rust BYTE offset (4) not Python char index (2). FIX (rust+ruchy): block-form binds recv to temp, `__s.find(&sub[..]).map(\|__b\| __s[..__b].chars().count() as i64)` (`__b` is a char boundary); index/rindex keep ValueError panic, find/rfind keep −1. `.count(sub)` unchanged; ASCII unchanged. Found by the differential hunt |
| **PMAT-567** — str slice char index (correctness) | `"αβγδ"[1:3]` == "βγ" | ✅ SHIPPED v0.1.266 — **fixed wrong-result + char-boundary PANIC** on non-ASCII str slices (was a byte slice). FIX (rust+ruchy): a str slice collects to `Vec<char>` so `__n`/clamping/`__sl[__lo..__hi]` are char-based, then `.iter().collect::<String>()`. List slice unchanged (element-indexed `&Vec`). Completes the str byte-vs-char trio (len/find/slice). Found by the differential hunt |
| **PMAT-568** — max/sort tie semantics (correctness) | `max([3,-3],key=abs)`==3 | ✅ SHIPPED v0.1.267 — **fixed 2 silent miscompiles**: `max(key=)` returned LAST tied (Rust `max_by_key`) vs Python FIRST → `.rev()` before `max_by_key` (min unaffected); `sorted(key=,reverse=True)` `sort_by_key`+`.reverse()` broke Python's STABLE reverse → stable descending `sort_by(\|a,b\| key(b).cmp(&key(a)))`. Covers in-place `xs.sort(key=,reverse=True)`. Rust+ruchy. Found by the differential hunt |
| **PMAT-569** — list-of-list repeat (correctness) | `[[0]] * n` | ✅ SHIPPED v0.1.268 — **fixed E0277** (transpile→invalid Rust): list repeat used slice `repeat` (needs `T: Copy`; `Vec` isn't). New `of_str` on `Expr::Repeat`: str→`String::repeat`, list→clone-repeat `(0..k).flat_map(\|_\| __rep.iter().cloned()).collect::<Vec<_>>()` (any `Clone` elem). Rust+ruchy. Found by the differential hunt |
| **PMAT-570** — negative pop/del (correctness) | `xs.pop(-1)`, `del xs[-1]` | ✅ SHIPPED v0.1.269 — **fixed OOB panic**: `pop(-k)`/`del xs[-k]` emitted `remove((-k) as usize)`→usize::MAX. Now resolve to `len(xs)-k` (via `neg_literal_int`), bound to a temp before `remove` (index references `xs` → E0502); positive indices keep inline form. Rust+ruchy. Found by the differential hunt |
| **PMAT-571** — 3-arg `pow` (modpow) | `pow(a, b, m)` | ✅ SHIPPED v0.1.270 — **fixed E0425** (bare `pow(a,b,c)` undefined fn): new `Expr::PowMod` → inline square-and-multiply, mod-reduced each step via i128 products (no overflow near i64::MAX); base normalised to [0,m) without the overflow-prone `(x%m)+m`; zero modulus / negative exp panic. Rippled meta-HIR + inferers + rust/ruchy; Lean refuses. Found by the differential hunt |
| **PMAT-572** — tuple reassign in loop (correctness) | `a, b = b, a % b` | ✅ SHIPPED v0.1.271 — **fixed Euclid-GCD infinite-loop + iterative-Fibonacci-all-zeros**: a tuple-unpack reassigning already-bound names in a while/for/if body emitted a fresh `let (mut a,mut b)` (shadow dies at block end → outer vars never change). Now routes tuple-literal reassigns of bound names through the shared unpack helper (eval RHS into temps first, then `Assign` each). Fresh unpacks keep `LetTuple`. No IR change. Found by the differential hunt (highest-impact bug) |
| **PMAT-573** — Rust-keyword identifiers (correctness) | `type`/`match`/`loop`/`move` as var/param/fn names | ✅ SHIPPED v0.1.272 — **fixed `rustc` "expected identifier, found keyword"**: a Python identifier that is a Rust keyword but not a Python keyword (`type`,`match`,`loop`,`move`,`ref`,`mut`,`box`,`final`,`do`,`impl`,…; lowercase `true`/`false`) emitted verbatim → broke compilation. FIX: a single IR pre-pass `xpile_meta_hir::escape_rust_reserved_idents` run by the Rust+Ruchy backends on a cloned module, rewriting every identifier-position string to raw form `r#name`. Rewriting the data once (binding **and** reference together) can't drift; walker is exhaustive (no wildcard) so a new `Expr`/`Stmt` variant fails to compile until classified — completeness compiler-enforced. Covers fn name/param/let/reassign/for-var/comp-binder/method-receiver/internal-callee; leaves type/field/method names + non-rawable `crate`/`self`/`Self`/`super` alone (keeps the special-cased `self` receiver intact). Lean uses a different keyword set, does not call it. No new IR. Found by the differential hunt |
| **PMAT-574** — mut receiver in a condition (correctness) | `while xs.pop()>=0:`, `if zs.pop()==9:`, `assert ws.pop()>=0` | ✅ SHIPPED v0.1.273 — **fixed `rustc` E0596** (transpile→invalid Rust): a mutating method (`.pop()`/`.setdefault()`) in a *controlling condition* mutates its receiver, but the mutability pre-walk `count_pop_receivers_in_stmt` only scanned Assign/AugAssign/AnnAssign/Return/Expr **value** positions — never the `while`/`if`/`for`/`assert` controlling expression → receiver stayed immutable → "cannot borrow `xs` as mutable". FIX: add `While`(test, loop bump ≥2 — runs every iteration), `If`(test), `For`(iter), `Assert`(test+msg) arms (reuses the existing `count_pop_receivers` expr-walker). Works for popped param (param+1 + pop≥1 > 1) and popped local (binding+1 + pop); no spurious `mut` (only genuine pop/setdefault receivers counted → `clippy -D unused_mut` green). No new IR. Found by the differential hunt |
| **PMAT-575** — left-shift value overflow (correctness / contract) | `1 << 63`, `3 << 62` | ✅ SHIPPED v0.1.274 — **fixed C-PY-INT-ARITH falsification** (silent wrap): `x << n` used `checked_shl`, which only returns `None` for shift *amount* ≥ 64 — it never detects lost significant bits, so `1i64 << 63` wrapped to `i64::MIN` and the overflow `.expect()` never fired. Python `<<` is exact, so the contract promises a panic until bigint promotion (same posture as checked add/mul/pow). FIX: for left-shift in non-bigint mode emit a reversibility check `(v << n) >> n == v` (arithmetic shift-back, both signs) and panic on mismatch; right-shift + bigint (`xpile_bigint::shl`) keep the plain form. Rust+Ruchy. Valid shifts unaffected incl. `-2 << 62 == i64::MIN`. No new IR. Found by the differential hunt |
| **PMAT-576** — chained comparison double-eval (correctness) | `0 < xs.pop() < 100`, `a == f() == b`, `1 < b() < c() < 9` | ✅ SHIPPED v0.1.275 — **fixed silent miscompile**: a chained comparison desugars to `(a OP b) && (b OP c) && …` where each *interior* operand is shared; `lower_compare_in_ctx` cloned the lowered operand into both → evaluated it **twice** (Python evaluates each once, L→R). A side-effecting middle (`xs.pop()`) popped twice (wrong result / empty-pop panic). FIX: for `ops.len() ≥ 2`, bind each operand to a `__cmpN` temp once in an `Expr::Block`, then fold the sub-comparisons over the temps (short-circuit preserved; single comparison unchanged). Set/float-promote logic factored into `build_chain_cmp`. No new IR. Found by the differential hunt |
| **PMAT-577** — right-shift `n ≥ 64` (correctness) | `x >> 64`, `x >> 100` | ✅ SHIPPED v0.1.276 — **fixed panic-mismatch**: Python defines `x >> n` for any non-negative `n` (saturates to the sign fill — `0` for `x ≥ 0`, `-1` for `x < 0`), but `checked_shr` returns `None` for `n ≥ 64` so the `.expect` panicked. FIX: right-shift in non-bigint mode emits a block clamping the amount to 63 when `n ≥ 64` (exactly the sign fill) and panicking on a negative amount (Python `ValueError`). Left-shift (PMAT-575) + bigint (`xpile_bigint::shr`) untouched. Rust+Ruchy. No new IR. The right-shift companion to PMAT-575 — shift semantics now complete. Found by the differential hunt |
| **PMAT-578** — `sorted()` over a float list (correctness) | `sorted([1.0, 2.0])` | ✅ SHIPPED v0.1.277 — **fixed E0277** (transpile→invalid Rust): a keyless `sorted(list[float])` emitted `Vec<f64>::sort()`, which needs `f64: Ord` (unsatisfied). FIX: added `of_float` to `Expr::Sorted` (frontend sets it from the element type, mirroring `ListMutate`); the keyless float case emits `sort_by(\|a, b\| a.partial_cmp(b).unwrap())` (descending `b.partial_cmp(a)`), like the in-place `xs.sort()` path; int keeps `.sort()`. NaN panics (Python parity). A float-returning `key=` is deferred. Rust+Ruchy. Found by the differential hunt |
| **PMAT-579** — checked i64 `abs` (correctness / contract) | `abs(i64::MIN)` | ✅ SHIPPED v0.1.278 — **fixed C-PY-INT-ARITH falsification** (silent wrap): int `abs(x)` emitted `(x).abs()`, but `i64::MIN.abs()` wraps to `i64::MIN` under `-O`. FIX: added `of_float` to `Expr::NumBuiltin`; the `Abs` arm emits `.checked_abs().expect(…)` for i64 (panics on `i64::MIN`) and `.abs()` for f64. `min`/`max` + float-math builtins ignore the flag. Rust+Ruchy. Completes the i64-overflow contract trio (left-shift PMAT-575 + right-shift PMAT-577 + abs). Found by the differential hunt |
| **PMAT-580** — bool `& \| ^` stays bool (correctness) | `def f(a: bool, b: bool) -> bool: return a & b` | ✅ SHIPPED v0.1.279 — **fixed reject + miscompile**: `&`/`\|`/`^` over two bools is a bool in Python, but xpile inferred int and coerced operands to i64, so a `-> bool` function was rejected ("body produces I64"). FIX: a both-bool bitwise op infers `Bool` and skips the i64 coercion (4 sites: 2 infer + 2 coerce), keeping `a & b` (Rust's `bool: BitAnd` matches); a mixed bool/int op still coerces (`True & 5 == 1`). Codegen unchanged. Rust+Ruchy. Found by the differential hunt |
| **PMAT-581** — float division by zero (correctness) | `1.0 / 0.0`, `1.0 // 0.0`, `1.0 % 0.0`, `1 / 0` | ✅ SHIPPED v0.1.280 — **fixed panic-mismatch**: Python raises `ZeroDivisionError`, but xpile emitted bare IEEE float ops yielding `inf`/`nan`. FIX: the float `Div`/`FloorDiv`/`Mod` arms bind the divisor to `__fz`, check `== 0.0`, and `panic!` (matching Python's raise; caught by a bare `except`). Binding the divisor once also fixes the prior double-eval of operands in `%`. Int true-division (lowers to a float `Div`) is covered by the same guard. Valid divisions unchanged. Rust+Ruchy. No new IR. Found by the differential hunt |
| **PMAT-582** — `repr()` builtin (correctness) | `repr("a'b")`, `repr(42)` | ✅ SHIPPED v0.1.281 — **fixed E0423 / reject**: `repr(x)` fell through to a generic call inferring I64 → rejected, or emitted a bare `repr(...)` (rustc E0423). FIX: a `repr` dispatch — int/float/bool reuse `str()` (`ToStr` / `str(bool)` desugar); `str` → new `Expr::ReprStr` whose codegen replicates CPython (single quotes, switch to double if the string has a `'` but no `"`; escapes `\`, quote, `\n`/`\r`/`\t`), emitted via a raw codegen string (no triple-escaping). Container repr + `{x!r}` deferred. Lean refuses. Rust+Ruchy. Found by the differential hunt |
| **PMAT-583** — float scientific notation (correctness) | `str(1e16)`, `str(1e-5)` | ✅ SHIPPED v0.1.282 — **fixed format-mismatch**: CPython prints sci notation when a float's decimal exponent is `< -4` or `>= 16` (`1e16`→`1e+16`), but xpile spelled them out (`format!("{}", x)`). FIX: the float `ToStr` helper reads the exponent from `format!("{:e}", x)` (exact; no `log10` error) and reformats to Python's `e±NN` style above the threshold; below it keeps the fixed `.0`-if-whole shape (small floats unchanged). All float string paths reuse it. Rust+Ruchy. 19-magnitude diff vs python3 is a perfect match. Found by the differential hunt |
| **PMAT-584** — float `sum()` compensation (correctness) | `sum([1.0, 1e16, 1.0, -1e16])` | ✅ SHIPPED v0.1.283 — **fixed precision divergence**: CPython 3.12+ `sum()` over floats uses Neumaier compensated summation, but xpile emitted naive `.iter().sum::<f64>()` → `0.0` (vs `2.0`) on catastrophic cancellation. FIX: the float `Sum` codegen emits the Neumaier compensated fold (seeded with `start`/`0.0`); int `sum` stays exact. Rust+Ruchy. No new IR. Found by the differential hunt |
| **PMAT-585** — clone non-Copy field read (correctness) | `def get_name(self) -> str: return self.name` | ✅ SHIPPED v0.1.284 — **fixed E0507** (move out of `&self`): a method/`@property` returning a non-Copy field (`String`/list/dict/set/struct) by value emitted `(self).name`. FIX: at the `FieldAccess` lowering, look up the field type; if non-Copy, wrap in the existing `Expr::Clone` → `(obj).field.clone()` (Copy fields read by value). Sound unconditionally — a field is never a mutation receiver (`self.items.append(x)` rejected upstream), so it only appears in read positions; LLVM elides redundant clones. No new IR. First slice of the ownership/borrow cluster. Found by the differential hunt |
| **PMAT-586** — `int()` of a non-finite float (correctness) | `int(float("inf"))`, `int(float("nan"))` | ✅ SHIPPED v0.1.285 — **fixed silent saturation**: Python raises `OverflowError`/`ValueError`, but `((x) as i64)` saturates `inf`→`i64::MAX` / zeroes `nan`→0. FIX: a `from_float` flag on `Expr::NumCast` (set when the `int(...)` source is a float); the int-cast codegen guards a non-finite source and panics. `int(int)`/`float(_)`/`from_str` unchanged; out-of-range *finite* (`int(1e30)`) still saturates (deferred bigint gap). Rust+Ruchy. Found by the differential hunt |
| **PMAT-587** — prelude-type-name collision (correctness) | `class Vec:` + a `list[int]` | ✅ SHIPPED v0.1.286 — **fixed E0107** (transpile→invalid Rust): a class/enum named after a prelude type xpile emits (`Vec`/`String`/`Option`/`Some`/`None`/`HashMap`/`HashSet`) emits a colliding `struct <Name>` (shadows when bare, but E0107 once the generic form is also used). FIX: reject the name at lowering (`rust_prelude_type_collision`) with a rename hint, restoring transpile-success ⟹ valid Rust; limited to emitted types so `Result`/`Box`/… still shadow fine. Auto-escaping type names is a deferred follow-up. Found by the differential hunt |
| **PMAT-588** — clone reused non-Copy call args (correctness) | `helper(xs) + helper(xs)` | ✅ SHIPPED v0.1.287 — **fixed E0382** (use-after-move): a non-Copy variable passed by value to a call and read >1× was moved into the call, breaking the other use. FIX: a per-function read-count pre-walk (`count_name_reads`) on the lowering ctx; a non-Copy `Ident` call argument read more than once is wrapped in `Expr::Clone`. Gated on read-count > 1 → single-use args byte-identical (zero churn / zero perf cost); the clone fires only on previously-failing code. 2nd ownership-cluster slice. No new IR. Found by the differential hunt |
| **PMAT-589** — `int()` out-of-i64-range float (correctness) | `int(1e30)` | ✅ SHIPPED v0.1.288 — **fixed silent saturation**: Python returns an exact bignum for an out-of-range finite float, but `((x) as i64)` saturates to `i64::MAX`. FIX: extend the PMAT-586 int-cast guard with a range check (`__ic < (i64::MIN as f64) \|\| __ic >= (i64::MAX as f64)` → panic), fail-loud until bigint promotion. In-range floats (incl. `9e18`) truncate as before. Completes the int-cast fail-loud story (non-finite + out-of-range). Rust+Ruchy. No new IR. Found by the differential hunt |
| **PMAT-590** — `list.insert` index clamp (correctness) | `xs.insert(100, x)`, `xs.insert(-1, x)` | ✅ SHIPPED v0.1.289 — **fixed panic-mismatch** (transpile-success ⟹ runtime-panic): `xs.insert(i, x)` emitted a bare `xs.insert((i) as usize, x)`, which panics for `i > len` and casts a negative `i` to a huge `usize` that also panics, whereas CPython's `list.insert` (`ins1`) clamps `i > len` → `len` (append) and normalizes `i < 0` → `len + i` (clamped to `0`). FIX: rust+ruchy emit a clamp block (`let __n = len; let mut __i = i; if __i<0 {__i+=__n; if __i<0 {__i=0}} if __i>__n {__i=__n}; insert(__i as usize, x)`). No new IR. Lean refuses. First two findings (#4+#5) of differential hunt #4 — one block fixes both. Found by the differential hunt |
| **PMAT-591** — float `%` CPython `float_rem` (correctness) | `1.0 % 0.3`, `4.0 % -2.0` | ✅ SHIPPED v0.1.290 — **fixed last-ULP miscompile + signed-zero loss**: float `a % b` lowered to `a - b*(a/b).floor()` (PMAT-502br) diverged from CPython in the last ULP on ~60% of non-power-of-two divisors and always returned `+0.0` for a zero remainder (CPython gives the divisor's sign). FIX: emit CPython `float_rem` — `mod = a % b` (Rust `%` IS C `fmod`); `if mod != 0 { if sign(b) != sign(mod) { mod += b } } else { copysign(0.0, b) }`. ZeroDivisionError guard (PMAT-581) preserved; float `//` unchanged (hunt verified only `%` diverged). No new IR. Rust+Ruchy. Bit-exact vs python3. Hunt #4 findings #12+#23 — one rewrite. Found by the differential hunt |
| **PMAT-592** — frozen-dataclass `Eq`+`Hash` (correctness) | `{Coord(1,2)}`, `d[Coord(1,2)]` | ✅ SHIPPED v0.1.291 — **fixed E0277/E0599** (transpile-success ⟹ invalid Rust): a `@dataclass(frozen=True)` is hashable in Python (valid dict key / set element), but every dataclass struct emitted a fixed `#[derive(Clone, Debug, PartialEq)]` — no `Eq`/`Hash` — so a `HashSet<C>` element or `HashMap<C,_>` key was rejected. FIX: track `@dataclass(frozen=True)` on the IR (`Item::Struct.frozen`, set by frontend `class_is_frozen`); rust+ruchy extend the derive with `Eq, Hash` when frozen AND all field types are Eq+Hash-capable (`i64`/`bool`/`String`; a float field disqualifies — `f64` is neither). Non-frozen dataclasses keep the bare derive (Python makes them unhashable) → existing output byte-identical. `#[serde(default)]` on the new field. Hunt #4 findings #10+#22 — one fix. Found by the differential hunt |
| **PMAT-593** — PEP 584 dict union (correctness) | `a \| b`, `a \|= b` | ✅ SHIPPED v0.1.292 — **fixed E0369** (transpile-success ⟹ invalid Rust): `a \| b` / `a \|= b` over two dicts fell through to a generic integer BitOr → `HashMap \| HashMap` (HashMap has no `BitOr`). FIX (frontend, reusing existing IR): `a \| b` → `Expr::DictMerge` (the `{**a,**b}` lowering — chains both iterators into a fresh HashMap, later entry `b` wins via `collect`, matching Python); `a \|= b` → `Stmt::DictUpdate` (≡ `a.update(b)` → `a.extend(...)`), in place. Other binary operators between two dicts (`&`/`-`/`^`/…) + non-`\|=` dict aug-assigns rejected cleanly. No new IR. Hunt #4 finding #6. Found by the differential hunt |
| **PMAT-594** — `enumerate(xs, start=N)` keyword (correctness) | `for j, v in enumerate(xs, start=10)` | ✅ SHIPPED v0.1.293 — **fixed silent output-mismatch**: the for-loop `enumerate` lowering read the start only from the 2nd positional arg, so the keyword spelling dropped it and emitted `+ 0` (Python yields `10,11,12…`; Rust yielded `0,1,2…`). FIX (frontend): resolve start from the 2nd positional arg OR a `start=` keyword (int literal); reject unknown enumerate keywords, a positional+keyword start conflict, and any `zip` keyword (previously silently ignored). Codegen already honors a nonzero start → no codegen change. No new IR. Hunt #4 finding #7. Found by the differential hunt |
| **PMAT-595** — int `sum()`/`enumerate(start)` overflow contract (correctness) | `sum(xs)`, `enumerate(xs, 9223372036854775807)` | ✅ SHIPPED v0.1.294 — **fixed silent i64 wrap** (contract bypass): integer `sum(xs[, start])` emitted a bare `.iter().sum::<i64>()` and `enumerate(xs, start)` a bare `__i as i64 + start`, both bypassing C-PY-INT-ARITH (every other int-arith path uses `checked_*` + a contract `expect`); under `-O` they silently wrapped (Python promotes to bigint). FIX (rust+ruchy): int `sum` → checked left fold seeded with `start` (`(xs).iter().fold(<start\|0i64>, \|__a,&__x\| __a.checked_add(__x).expect(<contract>))`); `enumerate` offset → `(__i as i64).checked_add(start).expect(<contract>)`. Float sum (Neumaier) + start==0 unchanged. i64 arithmetic now uniformly fail-loud. No new IR. Hunt #4 findings #14+#28. Found by the differential hunt |
| **PMAT-596** — `reversed(str)` (correctness) | `"".join(reversed(s))` | ✅ SHIPPED v0.1.295 — **fixed E0425** (transpile-success ⟹ invalid Rust): `reversed(s)` over a `str` fell through to generic call lowering → a bare `reversed(...)` identifier (the handler only recognized `Type::List`). FIX (frontend, reusing existing IR): when the arg infers to `Type::Str`, lower to `Reversed(StrChars(s))` — `StrChars` materializes chars as `list[str]`, `Reversed` preserves the list type — so `reversed(s)` types as `List(Str)` (matching Python's iterator-of-chars) and composes with `"".join(...)` / `list(...)` / `for c in reversed(s)`. The `s[::-1]` slice form (yields `str`) keeps its separate StrMethod::Reverse lowering. No new IR. Hunt #4 finding #13. Found by the differential hunt |
| **PMAT-597** — standalone `format()` builtin (correctness) | `format(n, "x")`, `format(x, ".1%")` | ✅ SHIPPED v0.1.296 — **fixed E0423** (transpile-success ⟹ invalid Rust): the standalone `format(value[, spec])` builtin (distinct from `str.format` / `%`) had no lowering → a bare `format(...)` call, which rustc rejects because `format` is a *macro*, not a function. FIX (frontend, no new IR): factored the f-string field spec-application into a shared helper `apply_nonempty_format_spec`; `format(x)` / `format(x, "")` == `str(x)`, `format(x, "<literal spec>")` reuses the helper. Non-literal / non-string specs rejected; inference is post-lowering so it types as `Str`. Mirrors the repr() fix (PMAT-582). Hunt #4 finding #25. Found by the differential hunt |
| **PMAT-598** — empty `set()` element inference (correctness) | `s = set(); s.add(Coord(..))` | ✅ SHIPPED v0.1.297 — **fixed E0308** (transpile-success ⟹ invalid Rust): `s = set()` lowers to an empty set defaulting to `HashSet<i64>` (no elements to infer from), so a later `.add(struct/str)` was an i64-vs-actual mismatch. FIX (rust+ruchy codegen): for a *mutable* empty `SetLit` binding still at the guessed `Set(I64)` default, suppress the explicit element-type annotation → `let mut s = HashSet::new();`, so rustc infers from the later `.insert(...)` (empty `SetLit` emits a bare `HashSet::new()`, no turbofish; `mutable` guarantees an inference source). Non-empty literals, immutable empty sets, and explicit `set[T]` annotations keep the annotation. No new IR. Hunt #4 finding #11. Found by the differential hunt |
| **PMAT-599** — dict-comp key clone on reused binder (correctness) | `{w: w for w in words}` | ✅ SHIPPED v0.1.298 — **fixed E0382** (transpile-success ⟹ invalid Rust): a dict comprehension reusing a non-Copy loop var in both key and value (`{w: w …}`, `{w: w+"!" …}`, `{k: len(k) …}`) built a `(w, w)` map tuple — the bare-binder key moved the String before the value could use it. FIX (frontend): in the single-generator dict-comp lowering, clone the key when the binder is non-Copy AND read >1× across key+value (`clone_comp_key_if_binder_reused`, reusing `count_reads_expr` + the PMAT-588 non-Copy predicate). Gated on read-count>1 + non-Copy → Copy-binder / single-use comprehensions byte-identical (zero churn). No new IR. Hunt #4 finding #16. Found by the differential hunt |
| **PMAT-600** — C0-separator whitespace (correctness) | `"\x1c".isspace()`, `"\x1cabc".strip()` | ✅ SHIPPED v0.1.299 — **fixed silent miscompile**: Python treats the C0 separators FS/GS/RS/US (U+001C..U+001F) as whitespace for `isspace()` and `strip`/`lstrip`/`rstrip`, but Rust's `char::is_whitespace()`/`trim` excludes exactly those four. FIX (rust+ruchy codegen): augment the predicate with `\|\| matches!(__c, '\u{1c}'..='\u{1f}')` — `isspace` via `.chars().all(...)`, the strip family via `trim_matches`/`trim_start_matches`/`trim_end_matches` against the same closure. isdigit/isalpha/isalnum unchanged. No new IR. Hunt #4 findings #1+#17 (4 string methods, both backends). Found by the differential hunt |
| **PMAT-601** — float `max`/`min` first-arg-wins (correctness) | `max(-0.0, 0.0)`, `max(nan, 1.0)` | ✅ SHIPPED v0.1.300 — **fixed silent miscompile**: 2-arg float `max`/`min` lowered to `f64::max`/`f64::min` (IEEE maxNum: `+0.0 > -0.0`, NaN dropped), but Python returns the FIRST arg on a tie/incomparable compare (`max(-0.0,0.0)`=`-0.0`, `max(nan,1.0)`=`nan`). FIX (rust+ruchy codegen): for `of_float` Min/Max emit a left fold with a STRICT compare (`__m`=args[0]; later arg replaces only on `__x > __m`/`__x < __m`), so ties/NaN keep the earlier value (Python `result=a; if b>result: result=b`). Int/str min/max keep the total-order `.min`/`.max` chain. No new IR. Hunt #4 finding #24. Found by the differential hunt |
| **PMAT-602** — annotation/Optional mismatch reject (correctness) | `x: int = d.get(key)` | ✅ SHIPPED v0.1.301 — **fixed E0308** (transpile-success ⟹ invalid Rust): a non-Optional annotation (`x: int`) over an Optional initializer (1-arg `d.get(k)`, an Optional param) emitted `Option<i64>` into an `i64` binding. FIX (frontend `lower_ann_assign`): reject when the declared type is non-Optional but the initializer infers to `Optional(_)`. Python doesn't enforce annotations (`x: int = d.get("z")` binds `None`), so unwrapping would diverge on the None case — fail-fast is faithful. The error names the fix; `d.get(k, default)` (non-Optional) and `Optional[...]` annotation forms still transpile. No new IR. Hunt #4 finding #21. Found by the differential hunt |
| **PMAT-603** — sort/sorted float `key=` (correctness) | `sorted(xs, key=lambda x: x/2.0)` | ✅ SHIPPED v0.1.302 — **fixed E0277** (transpile-success ⟹ invalid Rust): a float-returning sort key lowered to `sort_by_key` (f64 has no `Ord`). FIX (no new IR): `Expr::Sorted.of_float` now tracks the COMPARED values' float-ness (the key result when keyed, else the element type) via a new `sort_key_is_float` (infers the key body with its param bound to the element type); rust+ruchy emit `sort_by(partial_cmp)` for a float key (ascending + descending-stable + in-place). Int/str keys keep `sort_by_key`/`cmp`. NaN keys panic (like the keyless float sort PMAT-578). Distinct from PMAT-578 (float LIST vs float KEY). Hunt #5 (H5-25). Found by the differential hunt |
| **PMAT-604** — subscript list-concat aug-assign (correctness) | `grid[i] += [..]`, `cube[i][j] += [..]` | ✅ SHIPPED v0.1.303 — **fixed E0599** (transpile-success ⟹ invalid Rust): `grid[i] += [..]` over a nested list routed `+` through `combine_aug` → `BinOp::Add` → `Vec::checked_add` (no such method). Flat `xs += [..]` was already ListExtend; only the subscript/nested aug-assign fell through. FIX (frontend `combine_aug`): list+list `Add` → `Expr::ListConcat` (alongside the str-concat case), fixing single-level + nested subscript paths in one place. No new IR. Hunt #5 (H5-5). Found by the differential hunt |
| **PMAT-605** — `pow(a,b,m)` negative modulus sign (correctness) | `pow(10, 2, -3)` | ✅ SHIPPED v0.1.304 — **fixed silent miscompile**: 3-arg `pow` with a negative modulus returns a result with the modulus's sign in Python (range `(m, 0]`), but the modpow square-multiply loop yields the non-negative Euclidean residue (`pow(10,2,-3)` → 1 instead of -2). FIX (rust+ruchy codegen): re-sign after the loop — `if __pmm < 0 && __pmr != 0 { __pmr += __pmm; }` (mirrors the `//`/`%` floor-mod sign rule). Positive modulus unchanged. No new IR. Hunt #5 (H5-11). Found by the differential hunt |
| **PMAT-606** — `math.floor`/`ceil`/`trunc` range guard (correctness) | `math.floor(1e30)`, `math.floor(inf)` | ✅ SHIPPED v0.1.305 — **fixed silent saturation + panic-mismatch**: `math.floor`/`ceil`/`trunc` lowered to `(x).floor() as i64`; Rust's `as i64` saturates (huge→i64::MAX, inf→i64::MAX, nan→0), but Python returns a bignum for a huge float and raises OverflowError(inf)/ValueError(nan). FIX (rust+ruchy codegen): guard the rounded value `{ let __mf=(x).floor(); if !__mf.is_finite() {panic} if __mf<(i64::MIN as f64)\|\|__mf>=(i64::MAX as f64) {panic} __mf as i64 }`, fail-loud like the `int(float)` guard (PMAT-586/589). In-range values round unchanged. No new IR. Hunt #5 (H5-21+22). Found by the differential hunt |
| **PMAT-607** — `pow()` bool base (correctness) | `pow(True, n)`, `pow(True, n, m)` | ✅ SHIPPED v0.1.306 — **fixed E0425** (transpile-success ⟹ invalid Rust): Python bool is an int subtype (`pow(True,n)`==`pow(1,n)`), but the pow builtin only handled int/float bases; a bool base fell through to a bare `pow(...)` call. FIX (frontend): wrap the pow operands (2-arg + 3-arg) in the existing bool→i64 `to_i64_operand` (no-op for int/float), so it expands to checked_pow/modpow. No new IR. Hunt #5 (H5-13). Found by the differential hunt |
| **PMAT-608** — float `max`/`min` empty → ValueError (correctness) | `max(x/2 for x in xs if x>0)` (all filtered) | ✅ SHIPPED v0.1.307 — **fixed silent ±∞**: float `max`/`min` used `fold(±∞, f64::max/min)`, so an empty sequence returned ∓inf (not Python ValueError) and the fold ignored NaN / mishandled signed-zero ties. FIX (rust+ruchy codegen): float min/max use a strict-compare `reduce` (first-arg-wins, like PMAT-601) → `Option`; empty unwraps to a ValueError-style panic, else the `default=` substitutes. Fixes empty + NaN/tie. Int/str min/max unchanged. No new IR. Hunt #5 (H5-28). Found by the differential hunt |

**Epics (decompose into sub-slices, do NOT skip):** **R6/PMAT-475** — first sub-slice
= grandfather the depth-13 Diamond gate (`diamond_coverage.rs`) as an isolated,
green refactor, *then* author each contract to QUORUM. **PMAT-485** — real
`nvptx64` PTX emitter behind a contained nightly CI job.

**Frozen / deferred (explicitly lower EV — do *not* pick up as default work):**

- **Diamond-depth UNIVERSAL ratchet is frozen at depth-13.** No depth-14+ broadening sweeps as default/background work — diminishing epistemic returns. **New contracts (R6) join at depth-1+ and are NOT forced to the depth-13 floor** — that is the whole point of R6's gate change; the old "a new contract must reach the existing UNIVERSAL floor" rule is **retired** (it *was* the treadmill — it made adding any contract cost 13 Diamond theorems). Depth broadening of *existing* contracts resumes only on explicit user request. See the freeze banner in [`sub/diamond-taxonomy.md`](sub/diamond-taxonomy.md).
- **Fixture-overfitting pay-down** (8/13 contracts still ride single demo fixtures) — real but ~4–5 wk and foundational, not capability-unblocking. Do ONE opportunistically (`C-XLATE-PY-STR-TO-RUST-STRING` is lowest-hanging via the `runtime_strata.rs` template); do not front-load all 8 ahead of R1–R7.
- `&str`/borrowing, exceptions (needs R10 first), tuples, slicing, string methods, closures, sets — medium-EV; pick up after R1–R10.
- Classes/OOP, C pointers — explicitly v0.3.0 (huge scope / correctly out of v0.2.0). JS / TypeScript / Julia / R frontends — deliberate non-goal ([`audit-design.md`](../audit-design.md) §4 "Sovereign AI").

**Watched bets (tracked, not scheduled — external developments to revisit, NOT default pickup):**

- **`cuda-oxide` (NVlabs) — pure-Rust→PTX compiler.** [github.com/NVlabs/cuda-oxide](https://github.com/NVlabs/cuda-oxide). NVIDIA-Research-affiliated, experimental/alpha (first release ~2026-05-07, `v0.2.1` 2026-06-10). Compiles single-source SIMT Rust kernels (`#[kernel]`) directly to PTX via a *fully Rust-native* pipeline — Rust → Stable MIR (`rustc_public`) → Pliron IR (an MLIR-like framework written in Rust, no C++/CMake/tablegen) → LLVM → PTX — so the whole compiler builds with `cargo`.
  - **Why it's on the radar:** it is the upstream realization of xpile's *scaffolded* pure-Rust→PTX lane (§29). The Layer-5 Multi-Emitter Quorum currently names `rustc_codegen_nvvm` (general) + `aprender-gpu` (specialist) as the two PTX emitters; `cuda-oxide` is a natural **third, categorically-independent** general emitter (its MIR→Pliron→LLVM path fails differently than NVVM-IR), which would *strengthen* the §14.10 anti-correlation guard for the PTX `DiffExec` quorum rather than just duplicate it. It also validates the "Sovereign AI / pure-Rust stack" thesis ([`audit-design.md`](../audit-design.md) §4): no NVIDIA C/CUDA toolchain in the build graph.
  - **Why it is NOT scheduled (3 surviving blockers; the hardware blocker is now RETIRED — see §29 "Runtime-stratum hardware"):** (1) **Unbuilt consumer — the controlling blocker.** The xpile PTX lane it would plug into is itself still scaffolded (§29 `PMAT-26X+` general emitter and `PMAT-26Z+` `DiffExec` engine are ⏳; the path is still `ScaffoldPtxEmitter` / `DiffExecResult::NotRun`). A third independent emitter cannot precede its own consumer. (2) **Toolchain pin** — the LLVM-21 *floor* is in fact met by the bundled `llc` (21.1.8), so the original "LLVM bump" framing was too pessimistic; the real cost is that cuda-oxide hard-pins **nightly-2026-04-03 + `rust-src` + `rustc-dev`** (codegen-backend internals), whereas xpile pins **stable 1.93.0** (`rust-toolchain.toml`, `rustfmt`+`clippy` only). Adopting it means standing up a nightly codegen-toolchain lane xpile has deliberately not taken. (3) **Alpha churn** — `v0.2.1` is still self-described early-stage alpha with documented unsoundness (`index_2d(stride)`); building on it now imports that instability. *(Minor, GX10-specific: cuda-oxide names sm_80/sm_90/sm_100/sm_100a as targets; **sm_121 (GB10)** is prose-only and unconfirmed — so the locally-owned GX10 is the least-proven of the available GPUs for this tool. Moot for the disposition since the rented Lambda sm_90/sm_100 GPUs are confirmed targets.)*
  - **Disposition:** **tracked, not scheduled.** The hardware gate — historically the cleanest of the three — is **retired** (2026-06-12): Lambda Cloud rents Hopper (H100/GH200, sm_90, full TMA + real thread-block clusters) and Blackwell (B200, sm_100) on-demand, and the local GX10 (GB10, sm_121) is a real CUDA device — so §29's Runtime-stratum `DiffExec` *can* now execute-verify Hopper/Blackwell PTX. But retiring one of four blockers does not flip the disposition: the controlling gate is the **unbuilt §29 PTX consumer**, plus the nightly+`rustc-dev` pin and alpha maturity. **Re-evaluate** when (a) §29 `PMAT-26X+`/`PMAT-26Z+` light up a real general emitter + `DiffExec` engine (this is now the gating trigger — the consumer must exist first), (b) a decision is taken to carry a nightly + `rust-src` + `rustc-dev` codegen lane, and (c) `cuda-oxide` exits alpha — or at the next SOTA dossier (2026-11-15), whichever is first. It is *not* an R-item and must not be auto-picked-up.

**Hard dated obligation (overrides EV ordering):** the SOTA dossier cadence is **CI-enforced** — `crates/xpile/tests/sota_dossier_deadline.rs` fails the build the moment the `**Next Dossier Deadline:**` date in [`audit-design.md`](../audit-design.md) §0 passes. The 2026-Q3 dossier shipped (R7, 2026-06-12); **next deadline: 2026-11-15** (the 2026-Q4 slot). It must land before then regardless of where it sits in the EV order.

### Substrate impact

v0.2.0 adds **at least 4 new contracts** to the substrate:

| Contract | Layer × Lane | Source track |
|---|---|---|
| `C-XLATE-PY-STR-TO-RUST-STRING` | 2 / code, kind: kernel | 1.A |
| `C-XLATE-PY-DICT-TO-HASHMAP` | 2 / code, kind: kernel | 1.C |
| `C-C-INT-ARITH` | **1 semantics / code**, kind: kernel | 2.B |
| (Optional) `C-XLATE-PY-STR-BORROWING` | 2 / code, kind: kernel | 1.D (stretch) |

Note `C-XLATE-PY-LIST-TO-VEC` already exists at v0.1.0; Track 1.B extends it with new equations, doesn't create a new contract.

The Diamond program broadens from 12 contracts to **16 contracts (or 15 without 1.D)**. The 13 recurring algebraic templates apply mechanically — Templates 1, 6, 9, 10, 11, 12, 13 are all directly applicable to the new contracts.

### Exit criterion

```python
# real_python.py
def greet(name: str) -> str:
    return f"Hello, {name}!"

def count_chars(name: str) -> dict[str, int]:
    counts: dict[str, int] = {}
    for c in name:
        counts[c] = counts.get(c, 0) + 1
    return counts
```

```c
// real_c.c
int add(int a, int b) { return a + b; }
int factorial(int n) { return n <= 1 ? 1 : n * factorial(n - 1); }
```

Both transpile cleanly via `xpile transpile` → Rust + Ruchy + Lean for the Python case; → Rust for the C case. Each emitted function carries a `// xpile-contract: <ID>` citation referencing the appropriate v0.2.0 contract.

`xpile quorum` → 16 (or 15) QUORUM, 0 PARTIAL, 0 UNVERIFIED.
`xpile diamond` → depth-13 UNIVERSAL still holds (CI gate); newer contracts at depth-1+ at minimum, ratcheting up via subsequent broadening waves.

### What this is NOT

- **Not** a "build str/list/dict from scratch" project. The hard semantics work is already done in standalone depyler and decy. The xpile-specific work is the **contract substrate** (Lean theorems, Kani harnesses, Diamond program ratchet) and the meta-HIR adapter layer.
- **Not** the end of the standalone repos. paiml/depyler, paiml/decy, paiml/bashrs continue to be authoritative for their respective AST surfaces; xpile vendors a snapshot and mirrors patches downstream — same posture as the v0.1.0 bashrs merger.
- **Not** a single mega-PR. Each sub-track (1.A, 1.B, 1.C, 2.A, 2.B, 2.C, 3.A) ships as its own PR series with QUORUM + Diamond ratchets, same cadence as the v0.1.0 substrate work.

### Realistic timeline

~6 weeks if Tracks 1 and 2 run in parallel, given the bashrs-merger precedent. Sequential would push to ~8-9 weeks.

The next-session pickup is **the highest-ranked open item in the [Autonomous execution priority](#autonomous-execution-priority--ev-ranked-re-evaluated-post-v014-2026-06-11) table** — currently **R1, augmented assignment** (`x += 1`). Execute it to a tagged release, then take R2, and so on. Do not pick "whichever sub-track has open capacity," and do not pick depth-treadmill work.

---

## License

MIT OR Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.

## Contributing

Open a draft PR with: (a) a contract YAML if you're adding a new construct; (b) `pv lint` 8/8 passing; (c) the linked pmat work item ID. No bare code changes without a corresponding contract or pmat item.
