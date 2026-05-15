# xpile — Current Status

**Last updated:** 2026-05-15
**Last session:** [2026-05-15-scaffold.md](2026-05-15-scaffold.md)
**Phase:** 0 (Scaffold + pv wiring) — **DONE**
**Next phase:** 1 (Architectural contracts)

This is the single source of truth for "where xpile is right now." A future session should read this top-to-bottom before making changes.

---

## ✅ Done

### Repository

- [x] `~/src/xpile/` created; git initialized on `main`
- [x] 14-crate Cargo workspace compiles clean (`cargo check --workspace`)
- [x] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [x] `cargo run -p xpile` prints scaffold banner
- [x] Top-level files: README.md, LICENSE-MIT, LICENSE-APACHE, .gitignore, rust-toolchain.toml (1.93.0), deny.toml

### Crates (all scaffold-stage)

- [x] `xpile` (CLI binary)
- [x] `xpile-core` — `TranspileSession::new()`, frontend registration
- [x] `xpile-agent` — `Session`, `Budget` (defaults: 8 iter / 200K tok / 300s)
- [x] `xpile-oracle` — `Oracle` trait, `Fixture`, `CapturedOutputs`, `ComparisonResult`
- [x] `xpile-llm` — `CacheKey::compute()` with sha256 4-tuple
- [x] `xpile-mcp` — `McpServer` stub
- [x] `xpile-contracts` — re-exports `provable_contracts` modules + `XpileContractLayer` enum
- [x] `xpile-rust-codegen` — `emit_module()` stub
- [x] `xpile-meta-hir` — `Module`, `SourceLang` (Python/C/Cpp/Cuda/Ruchy), `Item`, `FfiBoundary`
- [x] `xpile-ffi-manifest` — `FfiManifest`, `FfiEntry`
- [x] `xpile-frontend` — `Frontend` trait
- [x] `depyler-frontend` — `PythonFrontend` (returns empty `Module`)
- [x] `decy-frontend` — `CFrontend` (returns empty `Module`)
- [x] `ruchy-frontend` — `RuchyFrontend` (returns empty `Module`)

### Contracts

- [x] `provable-contracts` (the `aprender-contracts` crate, producing `pv` CLI) wired as workspace path-dep
- [x] 4 example contracts pass `pv lint` 8/8 gates:
  - `contracts/xpile-frontend-trait-v1.yaml` (Layer 3, `kind: pattern`)
  - `contracts/py-int-arith-v1.yaml` (Layer 1, `kind: kernel`)
  - `contracts/xlate-py-list-to-vec-v1.yaml` (Layer 2, `kind: kernel`)
  - `contracts/ffi-cpython-ext-v1.yaml` (Layer 4, `kind: pattern`)
- [x] Current `pv lint` result: **PASS** (0 errors, 28 advisory warnings, mean score 0.58)

### Documentation

- [x] Canonical spec: [`docs/specifications/xpile-spec.md`](../specifications/xpile-spec.md) (350 lines, 23 sections, TOC + summaries linking to sub-specs)
- [x] All 23 sub-specs written under [`docs/specifications/sub/`](../specifications/sub/)
- [x] [`README.md`](../../README.md) updated to point at canonical spec and quality regime
- [x] [`contracts/README.md`](../../contracts/README.md) and [`skills/README.md`](../../skills/README.md) initial drafts
- [x] Legacy specs archived to [`docs/specifications/legacy/`](../specifications/legacy/)
- [x] This `docs/status/` directory created

---

## 🔄 In progress

(nothing in progress — Phase 0 closed at end of 2026-05-15 session)

---

## ⏭ Next actions

Priority order. Each item is intended to become a pmat work item once `pmat work create` is wired:

### High priority (Phase 1 prerequisites)

1. **Commit the initial scaffold.** Currently everything is uncommitted on `main`. Plan:
   - Create branch `feat/scaffold-v0.1.0`
   - Stage all 40+ files in one commit per logical group (workspace, crates, contracts, docs/specifications, docs/status)
   - Create GitHub repo `paiml/xpile` and push
   - Open PR; confirm CI green
2. **Register xpile in the kaizen fleet** ([§20](../specifications/sub/kaizen-fleet.md)):
   - Run `pv kaizen --register xpile` from the xpile root
   - Confirm xpile appears in `pv kaizen` fleet rollup
3. **Wire CI** ([§18](../specifications/sub/ci-gates.md)):
   - Create `.github/workflows/ci.yml` with the gate matrix
   - Add `scripts/check_provenance.sh` (placeholder until Phase 2)
   - Confirm `pmat tdg` ≥ A- on the scaffold
4. **File pmat work items for Phase 1**:
   - One work item per architectural contract to port from depyler (4 + 1 new = 5 items)
   - Each work item points at its target contract path

### Medium priority (Phase 1 substance)

5. **Port depyler's 4 architectural contracts** to xpile:
   - `repair-determinism-v1.yaml` → `xpile-determinism-v1.yaml`
   - `repair-budget-v1.yaml` → `xpile-budget-v1.yaml`
   - `repair-provenance-v1.yaml` → `xpile-provenance-v1.yaml`
   - `repair-oracle-v1.yaml` → `xpile-oracle-v1.yaml`
6. **Author new `xpile-ffi-manifest-v1.yaml`** (Layer 3 architectural)
7. **Wire bindings** (`binding: <crate>::<symbol>`) in each contract so Gate 7 (reverse-coverage) passes
8. **Move architectural contracts** from `draft` → `enforced` status

### Lower priority (Phase 1 polish)

9. Address the 28 advisory warnings on existing contracts (add `lean_theorem` per equation, add `qa_gate`)
10. Update `contracts/README.md` with the live contract list and lint score
11. Create `.github/CODEOWNERS`

---

## 🚫 Blocked / open questions

| Question | Owner | Notes |
|---|---|---|
| Cache location: `~/.cache/xpile/` vs project-local `<repo>/.xpile-cache/`? | TBD | Decision deferred to Phase 2. Project-local enables committed reproducibility but bloats the repo. |
| In-process Anthropic SDK vs out-of-process via `xpile-mcp`? | TBD | Phase 2 decision. Probably both — SDK for CI, MCP for IDE. |
| Path-dep on `~/src/aprender/...` vs crates.io version pin for `aprender-contracts`? | upstream | Blocked on aprender-contracts having a stable crates.io release. |
| Migration timing — Phase A (extract) starts when? | upstream | Coordinated with depyler maintainers; not pre-Phase-2. |

---

## 📊 Metrics at v0.1.0

| Metric | Value | Target |
|---|---|---|
| Workspace crates | 14 | (grows with frontends) |
| Contracts under `contracts/` | 4 | ≥10 by end of Phase 1 |
| `pv lint` gates passing | 7/8 (Gate 7 skipped pending bindings) | 8/8 |
| `pv score` mean | 0.58 | ≥0.7 by end of Phase 2 |
| `cargo clippy -- -D warnings` | clean | always clean |
| `cargo check --workspace` | clean | always clean |
| PMAT TDG grade | TBD (not yet measured) | ≥ A- |
| Line coverage | TBD (no real tests yet) | ≥ 95% (after Phase 2) |
| Kani proofs | 0 enforced (3 contracts have harness blocks) | ≥3 enforced after Phase 4 |
| Lean theorems | 0 closed | ≥3 closed after Phase 6 |
| Fleet membership | not yet registered | registered + recurring rollups |

---

## 🗂 Where to find things

| Looking for... | Where |
|---|---|
| Canonical spec | [`docs/specifications/xpile-spec.md`](../specifications/xpile-spec.md) |
| Architecture sub-specs | [`docs/specifications/sub/*.md`](../specifications/sub/) |
| Legacy/archived specs | [`docs/specifications/legacy/`](../specifications/legacy/) |
| Contracts | [`contracts/*.yaml`](../../contracts/) |
| Skills (markdown) | `crates/xpile-agent/skills/` (not yet created — Phase 3) |
| Build / lint commands | See "How to verify" section below |
| Per-session logs | This directory (`docs/status/`) |

---

## 🧪 How to verify (any session)

```bash
cd ~/src/xpile

# Build & static checks
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                          # currently nothing meaningful

# Run the scaffold binary
cargo run -p xpile

# Contracts
pv lint                                          # 8/8 should pass (Gate 7 skipped)
pv score contracts/                              # mean ≥ 0.58 at v0.1.0
pv validate contracts/xpile-frontend-trait-v1.yaml   # individual file

# Once CI lands
pmat tdg                                         # should be ≥ A-
cargo llvm-cov --workspace --fail-under-lines 95 # post-Phase-2 only
```

If any of these regress, **don't move forward** until the regression is understood and fixed.

---

## 📞 Pickup script for a future session

Copy-pastable opening for the next session:

> Read `~/src/xpile/docs/status/CURRENT.md`. We're at end-of-Phase-0. The highest-priority next action is creating the GitHub repo + initial commit, then porting depyler's 4 architectural contracts into xpile. Verify `pv lint` still passes 8/8 before doing anything else.
