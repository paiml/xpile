# Glossary

**Section 22 of [xpile-spec.md](../xpile-spec.md).**

## Core concepts

**Agent loop** — The bounded LLM-driven repair sequence in `xpile-agent`. Adapted from alchemize's four-tool pattern (`read_file`, `write_*`, `cargo_build`, `validate_*`). Opt-in via `--repair`.

**Bounded model checking** — Kani's verification strategy: prove a property for all inputs up to a fixed size. xpile uses BMC for arithmetic contracts at i8 bit width.

**Budget exhaustion** — When the agent loop hits its iteration / token / wall-clock cap. Fails closed: surfaces the original static error, never partial Rust.

**Cache key** — `sha256(source || xpile_version || model_id || skills_hash)`. The receipt that converts stochastic LLM output into reproducible artifacts.

**Codegen** — `xpile-rust-codegen`. Takes meta-HIR, emits idiomatic Rust. Language-neutral by design.

**Contract** — A YAML file under `contracts/`, validated by `pv lint`. The canonical artifact from which Rust stubs, tests, proofs, and docs are generated.

**Contract layer** — xpile's organizational tag. Layer 1 (language semantics) and Layer 2 (translation) are `kind: kernel`; Layer 3 (architectural) and Layer 4 (hybrid) are `kind: pattern`.

**Determinism** — The property that the default `xpile transpile foo.py` (no `--repair`) is reproducible across runs and machines. The cache + provenance marker extend this to repair-pass outputs.

**FFI manifest** — The cross-language boundary registry in `xpile-ffi-manifest`. The single source of truth for hybrid-transpile sessions.

**Frontend** — A type implementing `xpile-frontend::Frontend`. The only language-specific abstraction in xpile.

**Hybrid transpile** — A session that crosses language boundaries (e.g., Python + C, Python + CUDA, Python + shell). The load-bearing motivation for xpile. First shipped hybrid example: `subprocess.run([...])` recognition lowering Python to shell via the meta-HIR `Stmt::Cmd` variant (PMAT-040).

**Layer B variants** — The meta-HIR shell-domain variants added across PMAT-039..056: `Stmt::{Cmd, Pipeline, ShellLoop, ShellAssign}`, `Expr::{LitStr, QuotedString, ShellVar, CommandSubstitution, ShellSpecial}`, `Type::{ShellString, ExitCode}`, plus `QuotingStrategy` and `LoopKind` enums. Produced by `bashrs-frontend`, consumed by `bashrs-backend`; other backends return `Unsupported`.

**bashrs domain** — The POSIX shell (sh / bash / zsh / Makefile / Dockerfile) family absorbed into xpile per the 2026-05-17 merger reversal (see [bashrs-merger.md](bashrs-merger.md)). `crates/bashrs-frontend` parses; `crates/bashrs-backend` emits. `C-BASHRS-POSIX-IDEMPOTENCE` is the Layer-1 semantic anchor.

**Kaizen** — Continuous improvement. The `kaizen-paiml` skill drives the loop: open work item → implement → all gates pass → close → repeat.

**Kernel contract** — A `pv` contract with `kind: kernel`. Must have non-empty `proof_obligations`, `falsification_tests`, AND `kani_harnesses`.

**Meta-HIR** — `xpile-meta-hir::Module`. The canonical IR every frontend lowers to.

**Oracle** — A `xpile-oracle::Oracle` implementation. Captures the original source's behavior on a fixture; compares to the transpiled Rust. The semantic exit gate of the agent loop.

**Pattern contract** — A `pv` contract with `kind: pattern`. Cross-cutting / architectural / process invariant. Lighter requirements than kernel contracts.

**PMAT TDG** — Technical Debt Grade produced by `pmat tdg`. A+ → F. xpile's minimum: A-.

**Provable contract** — Synonym for a `pv`-validated contract that ships with `kani_harnesses` and/or `lean_theorem` references.

**Provenance marker** — The first-line comment on every repair-pass `.rs`: `// xpile-repaired: <hex> via <model> at <utc>`. The receipt back into the cache.

**`pv`** — Provable Contracts CLI (v0.32.0). Installed at `~/.cargo/bin/pv`. The design controller across the fleet.

**`pmat`** — Pragmatic Multi-language Agent Toolkit. The work controller across the fleet.

**PVScore** — `pv score`'s output: a numeric quality grade per contract. xpile's mean at v0.1.0 is 0.56 across 12 contracts (canonical value is whatever `pv lint contracts/` reports on the current `main` branch).

**QUORUM** — A contract's §14.4 N-of-M oracle status. The ruchy 5.0 rule: ≥1 vote in ≥3 strata (Semantic / Symbolic / Runtime / Extrinsic) ⇒ QUORUM; 1-2 strata ⇒ PARTIAL; 0 strata ⇒ UNVERIFIED. At v0.1.0 (post PMAT-058..077 substrate completion) the whole then-12-contract substrate reached QUORUM; the substrate has grown since and coverage is **partial, not total** — new contracts land ahead of their Lean or Kani votes and sit at PARTIAL until the missing stratum arrives. `xpile quorum` reports the live per-contract state and the totals; do not retype them here (PMAT-1451).

**Repair mode** — Opt-in via `--repair`. Invokes the agent loop on static-pass failure.

**Skill** — A markdown file under `crates/xpile-agent/skills/`. Pulled into agent context via `apply_skill(name)`. A holding pen for patterns awaiting graduation to static rules.

**Skill graduation** — Promotion of a skill into a deterministic rule in `xpile-rust-codegen`, with the skill markdown deleted in the same PR. The success signal for the agent loop.

**Static pass** — The non-LLM transpile path. Default behavior. Always reproducible.

**TranspileSession** — The lifetime boundary for a single transpile invocation, including any agent-driven repair. Defined in `xpile-core`.

## Fleet concepts

**Fleet** — The set of 40+ repos under `pv kaizen` enforcement. xpile is repo #41.

**Kernel tier** — Fleet tier for repos that produce verifiable kernels (aprender, trueno, realizar, entrenar). Graded on postcondition density.

**Tool tier** — Fleet tier for repos that consume kernels (pmat, depyler, decy). Graded on call-site coverage.

**Cross-repo binding** — A contract dependency that spans repos. E.g., xpile's `xlate-py-int-to-i64` depending on trueno's `C-I64-WRAPPING-ADD-V1`.

## Acronyms

- **AST** — Abstract Syntax Tree
- **BMC** — Bounded Model Checking
- **CI** — Continuous Integration
- **CLI** — Command-Line Interface
- **FFI** — Foreign Function Interface
- **GIL** — Global Interpreter Lock (Python)
- **HIR** — High-level Intermediate Representation
- **IR** — Intermediate Representation
- **LLM** — Large Language Model
- **MCP** — Model Context Protocol
- **PMAT** — Pragmatic Multi-language Agent Toolkit
- **TDG** — Technical Debt Grade
