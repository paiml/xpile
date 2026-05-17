# References

**Section 23 of [xpile-spec.md](../xpile-spec.md).** (Not literally Section 23 in the TOC — see [Status](../../status/CURRENT.md) for the actual Section 23.)

## Family repositories (`~/src/`)

| Repo | Role | Notes |
|---|---|---|
| [`paiml/xpile`](../../..) | This repo | Polyglot transpile workbench — v0.1.0, 12 contracts at 100% §14.4 QUORUM, 27 workspace crates, 4 real backends (Rust/Ruchy/Lean/Shell) |
| [`paiml/aprender`](../../../../aprender) | ML framework + provable-contracts source | `aprender-contracts` crate produces `pv` |
| [`paiml/provable-contracts`](../../../../provable-contracts) | Canonical `pv-spec.md` and contract framework | The reference for this spec structure |
| [`paiml/depyler`](../../../../depyler) | Python → Rust transpiler | depyler-frontend in-tree as `crates/depyler-frontend/` (per [migration.md](migration.md) PMAT-097); legacy repo still exists as separate downstream consumer at v0.1.0 |
| [`paiml/decy`](../../../../decy) | C → Rust transpiler | decy-frontend in-tree as `crates/decy-frontend/` (scaffold-stage); legacy repo still exists |
| [`paiml/ruchy`](../../../../ruchy) | Modern language for data science | ruchy-frontend in-tree as `crates/ruchy-frontend/` (scaffold-stage) |
| [`paiml/bashrs`](../../../../bashrs) | POSIX shell transpiler | bashrs-frontend + bashrs-backend in-tree as `crates/bashrs-{frontend,backend}/` (real emission, 54 tests, post PMAT-037..058 merger); legacy repo still exists |
| [`paiml/paiml-mcp-agent-toolkit`](../../../../paiml-mcp-agent-toolkit) | `pmat` source | Work + quality controller |
| [`pymc-labs/alchemize`](../../../../alchemize) | LLM transpile compiler for probabilistic models | Source of the four-tool agent loop pattern |

## Tools

- **`pv`** — `/home/noah/.cargo/bin/pv` (v0.32.0 at time of writing)
  - 34 subcommands: `validate`, `scaffold`, `kani`, `probar`, `generate`, `lint`, `score`, `query`, `codegen`, `kaizen`, `coverage`, `graph`, `lean`, `roofline`, `audit`, `diff`, `coq`, `fuzz`, `mirai`, …
  - Canonical spec: [`provable-contracts/docs/specifications/pv-spec.md`](../../../../provable-contracts/docs/specifications/pv-spec.md)
- **`pmat`** — Pragmatic Multi-language Agent Toolkit
  - Source: [paiml/paiml-mcp-agent-toolkit](https://github.com/paiml/paiml-mcp-agent-toolkit)
  - Used commands: `pmat tdg`, `pmat query`, `pmat work *`, `pmat context`
- **Kani** — Bounded model checker for Rust
  - Install: `cargo install --locked kani-verifier`
  - Used via `pv kani` codegen + `cargo kani` runner
- **Lean 4** — Theorem prover
  - Used via `pv lean` codegen for math-dense contract theorems

## Specs and prior art

- **alchemize compiler.py** — [pymc-labs/alchemize/alchemize/compiler.py](https://github.com/pymc-labs/alchemize/blob/main/alchemize/compiler.py) — the agent loop pattern xpile adapts
- **alchemize skills/** — [pymc-labs/alchemize/alchemize/skills](https://github.com/pymc-labs/alchemize/tree/main/alchemize/skills) — the markdown skill format xpile inherits
- **aprender contracts/** — [paiml/aprender/contracts](https://github.com/paiml/aprender/tree/main/contracts) — 303+ kernel contracts as templates
- **provable-contracts pv-spec.md** — the structural template for this spec
- **depyler repair-mode spec** — [`depyler/docs/specifications/depyler-repair-mode.md`](../../../../depyler/docs/specifications/depyler-repair-mode.md) (branch `feat/repair-mode-spec`)

## Papers (foundations)

- **Eiffel DbC** — Bertrand Meyer, *Object-Oriented Software Construction* (1988). Design-by-Contract foundations used in xpile contract obligations.
- **Falsificationism** — Karl Popper, *Conjectures and Refutations* (1963). The "every claim must be falsifiable" principle behind `falsification_tests`.
- **Kani BMC** — Aman Sharma et al., *Kani Rust Verifier* (Amazon, 2022+). Bounded model checking for Rust.
- **Lean 4** — Leonardo de Moura et al., *The Lean 4 Theorem Prover and Programming Language* (CADE 2021).

## Internal status documents

- [`docs/status/CURRENT.md`](../../status/CURRENT.md) — live status; future-session pickup point
- [`docs/status/INDEX.md`](../../status/INDEX.md) — index of session logs
- [`docs/status/2026-05-15-scaffold.md`](../../status/2026-05-15-scaffold.md) — initial scaffold session log

## Legacy specs (archived)

These are NOT authoritative. Kept for traceability only.

- [`docs/specifications/legacy/xpile-architecture-v1.md`](../legacy/xpile-architecture-v1.md)
- [`docs/specifications/legacy/xpile-contract-driven-design-v1.md`](../legacy/xpile-contract-driven-design-v1.md)
