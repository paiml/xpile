# Fable architectural review — EV-ordered, falsifiable engineering roadmap

> Point-in-time adversarial architecture + gate-integrity review of `paiml/xpile`,
> produced 2026-07-05 by Claude Fable 5 from the `fable5-xpile-roadmap-prompt`
> exercise (the §-references below are to that prompt's grounding contract and
> output format). Grounded at `main` = `4ac45b56`; every claim cites a fetched
> artifact (repo file, `gh` API response, crates.io metadata, or a CI run).
> The counts are the drifting inventory as of that commit — re-verify at HEAD
> before reusing any number.

**Grounded at `paiml/xpile` `main` = `4ac45b56` (2026-07-05). Produced 2026-07-05.**

**Operating assumption (§0): live read access was available** — a full local git checkout fetched to `origin/main` (`4ac45b56`, verified against github.com), the GitHub API via `gh` (rulesets, check-runs, run logs, sibling repos), and crates.io over HTTPS. Every claim below cites one of those artifacts; nothing is carried from training data or from the prompt's snapshot. Four parallel read-only auditors swept the contract corpus, CI surface, oracle/lowering matrix, and source-of-truth topology; their load-bearing numeric claims were independently re-verified (one auditor claim — the `bashrs-posix-idempotence` falsifier gap — survived an adversarial re-check that my own naive grep initially got wrong).

---

## 7a. Verification ledger

| # | claim | snapshot value | HEAD value | source | verdict |
|---|-------|----------------|------------|--------|---------|
| 1 | HEAD of main | 2026-07-05 unauth snapshot | `4ac45b56` "feat(wasm,capability,list): xs.sort()/clear() (PMAT-1288) (#1871)", 2026-07-05 | `git fetch origin main` | VERIFIED |
| 2 | Workspace = 31 members, list per §1 | 31 named | 31, byte-for-byte match (12 core + 5 frontends + 9 codegen backends + 5 proof/contract) | `Cargo.toml` `[workspace].members` | VERIFIED |
| 3 | `xpile-meta-hir` is the pinch point | asserted | 20/31 crates depend on it — all 5 frontends + all 9 backends; 5,235-line `lib.rs` | `grep -l xpile-meta-hir crates/*/Cargo.toml` | VERIFIED |
| 4 | `xpile-oracle` = differential/semantic engine | asserted | **Behavioral, not syntactic**: spawns `python3` / `cc`, byte-identical-stdout diff pinning first divergent line (`diff_stdout`, `crates/xpile-oracle/src/lib.rs:110-126`); references = CPython, native C, ctypes-bound `.so` | `crates/xpile-oracle/src/lib.rs:87-374` | VERIFIED |
| 5 | SPIR-V is `WGSL→naga→spv`, downstream of WGSL | asserted | Confirmed in code: `emit_from_wgsl` → `wgsl_to_spirv_words` (`crates/xpile-spirv-codegen/src/lib.rs:249-258`); enum doc: "NOT a hand-written SPIR-V assembler" (`xpile-backend/src/lib.rs:25-30`) | code | VERIFIED |
| 6 | `rust-version = "1.93"`, edition 2021, toolchain file present | 1.93 / 2021 / present | Same; `rust-toolchain.toml` pins `channel = "1.93.0"` | `Cargo.toml`, `rust-toolchain.toml` | VERIFIED |
| 7 | MSRV possibly not CI-verified (`@stable` risk) | threat per §4 | All 4 Rust CI jobs use `dtolnay/rust-toolchain@stable` (ci.yml:23,59,87,203) but the repo-root toolchain file resolves cargo to 1.93.0 anyway; **no explicit MSRV job** | `.github/workflows/ci.yml` | VERIFIED (mitigated; residual: no named MSRV job, rustfmt/clippy components from `@stable`) |
| 8 | Required checks = `ci / gate` + `workspace-test` (stack convention) | assumed | **As measured 2026-07-05, org ruleset 13878864 required ONLY the context `gate`** — no longer true; `workspace-test` was added and, since 2026-07-27, is supplied by ruleset `19814559`, so both block merges today (PMAT-1475) — (non-strict, PR required, 0 approvals, non-fast-forward). `workspace-test` is claimed "Required check" at ci.yml:50 **comment only** — not in the ruleset. Check-runs report as bare job names (`gh pr checks 1871`), so the binding is to the `gate` job | `gh api repos/paiml/xpile/rules/branches/main` | **STALE — doctrine ≠ enforcement (finding F1)** |
| 9 | Ids PMAT-xxx + XPILE-xxx in use; cited 012/037/951/953/954/960 | in use | All six resolve in `docs/roadmaps/roadmap.yaml` (lines 11808, 12208, 3009, 3046, 3066, 2936), all `status: done`. No private-infra dangling id encountered | roadmap.yaml | VERIFIED |
| 10 | No `scripts/` dir at HEAD | none | Absent | `ls` | VERIFIED |
| 11 | Publish lag vs HEAD | compute | crates.io `xpile` 0.1.616 @ 2026-07-04T15:55; workspace `version = "0.1.616"`; tag `v0.1.616` exists; lag = **31 commits / <1 day**. `xpile-core`, `depyler-frontend` also 0.1.616 @ 2026-07-04 | crates.io API, git | VERIFIED (healthy) |
| 12 | Contract corpus size | enumerate | **35 YAMLs**: 10 `xlate-*`, 5 `compile-*`, 2 `ffi-*`, 6 `py-*`, 3 `c-*`, 4 `*-trait`, 5 other | `contracts/*.yaml` | VERIFIED |
| 13 | Real falsifiers vs stubs | enumerate | `falsification_tests` **31/35** (missing: `bashrs-posix-idempotence` — key appears in a comment only, self-declared "scaffold stage"; `ffi-shell-subprocess`; `py-float-arith`; `xlate-py-set-to-hashset`). `kani_harness` **24/35** (101 `#[kani::proof]` across 24 files). `lean_theorem` **35/35**. **0 pure stubs** | grep + spot-reads | VERIFIED |
| 14 | Proof-present ≠ proof-valid: does CI verify Lean discharges? | THE load-bearing question | Proofs are REAL: 35 modules + lakefile (21,659 lines), `warningAsError := true`, **0 real `sorry`/`admit`/`axiom`** (regex-verified; the 37/239 naive-grep hits are all prose/identifiers). CI job `lake-build` RUNS `lake build` per-PR (ci.yml:141-143) — **but advisory**; `refinement_proofs.rs` in the required path is existence-only | `contracts/lean/`, ci.yml, `crates/xpile/tests/` | VERIFIED (theater REFUTED on substance; enforcement gap = F2) |
| 15 | Kani verified in CI? | ? | `kani` job bootstraps `cargo kani --version` then runs `kani_verify.rs`, which RUNS `cargo kani` per harness and requires exit-0 + `VERIFICATION:- SUCCESSFUL` (kani_verify.rs:164-173). Advisory; **silently no-ops green if kani absent from PATH** (real in CI since the bootstrap step would fail first) | ci.yml:103-107, kani_verify.rs | VERIFIED (real but advisory + PATH-skip hole = F9) |
| 16 | Mathlib lane separate | memory/doctrine | `contracts/lean-models/`: 4 modules, 13 theorems, own lakefile + toolchain (v4.15.0), 0 escape hatches; advisory `lean-models` CI job (`lake exe cache get` + `lake build`) | contracts/lean-models/, ci.yml:267-269 | VERIFIED |
| 17 | CLI surface | count at HEAD | **7 subcommands** (`info, transpile, audit, attestations, quorum, diamond, hybrid`, main.rs:38-173); **9 targets** (`rust, ruchy, ptx, wgsl, spirv, wasm, lean, shell, forjar`, parse_target main.rs:791-815); **5 frontends** dispatched by file extension (xpile-core lib.rs:72-85) | `crates/xpile/src/main.rs` | VERIFIED |
| 18 | README "five source languages → nine backends" | claim | Counts match the crate registry exactly (5×9). Caveats: `SourceLang` has unreachable `Cpp/Cuda/Rust/Lean` variants (test-only); **PTX is CLI-unreachable** — all 3 CLI module-construction sites hardcode `hardware: None` (main.rs:390,665,930) and `PtxBackend` hard-requires `HwProfile::Ptx` (ptx-codegen lib.rs:241-244) → every `transpile --target ptx` refuses loudly | README.md, code | VERIFIED with caveat (= F4) |
| 19 | `docs/roadmaps/roadmap.yaml` planned work | pull | 15,022 lines, **1,233 items**: 1,168 done / 3 in_progress / 3 open / 14 planned / 45 obsolete. `strategic_goals:` spine present (north star + pillars A–E + frontier), inserted per PMAT-954 | roadmap.yaml | VERIFIED |
| 20 | Recency (last 14 days) | atom feed | **428 commits** since 2026-06-21, all PR-merged (13 PRs #1859–#1871 landed 2026-07-05 alone); latest main CI run 28742633266: **all 7 jobs green** (gate 52s, workspace-test 3m05s, kani 2m02s, lake-build 16s, lean-models 1m34s, docs 12s, wasi 40s) | git log, `gh run view` | VERIFIED |
| 21 | Blocked/critical tickets | issue/PR search | **0 open PRs, 0 open GitHub issues** — all work tracked in roadmap.yaml + queue.yaml | `gh pr list`, `gh issue list` | VERIFIED |
| 22 | bashrs-merged vs depyler-split asymmetry | investigate | Dual source-of-truth CONFIRMED: standalone `paiml/depyler` un-archived, last code push 2026-04-19, crates.io `depyler` 4.1.1 @ 2026-02-15, and doctrine names it **"the maintenance home for the parser + codegen rules; xpile vendors a snapshot"** (`docs/specifications/sub/v0.2.0-depyler-merger.md:194-197`) — while xpile ships a **re-implemented** (migration.md:47), fresher-published subset (`depyler-frontend` 0.1.616 @ 2026-07-04). Shim collapse explicitly **⏳ incomplete** (migration.md:69). Same shape for bashrs (6.66.1 @ 2026-04-27), decy (2.2.0), ruchy (4.2.1) — all dormant since Apr/May 2026 | gh + crates.io + specs | VERIFIED (= F6) |
| 23 | mdBook leg of the proof lane | pull | `book.yml`: `mdbook build book` per-PR; Pages deploy on main push only | .github/workflows/book.yml:38-49 | VERIFIED |
| 24 | Claims-gate analog (no `scripts/`) | confirm | Only claims-adjacent gate is the **advisory** `docs` job: `pmat validate-docs --fail-on-error` (links) + `pmat demo-score ≥ 6.0` (presentation floor) — **no semantic claims-vs-repo-state gate exists**. Live drift instances: `PROVABILITY-INVENTORY.md:11` says "all **25** elaborate" vs its own header "**35** modules" (line 42); `strategic_goals` says "27 machine-checked modules" (dated 2026-06-25) vs 35; pillar D says WASM lift "COMPLETE" while PMAT-952 is `status: planned` | ci.yml:160-188, artifacts quoted | VERIFIED — gate absent, drift live (= F8) |

**Stable doctrine (does not drift):** north star = provable polyglot transpilation over one meta-HIR, contract+Lean+execution proof per edge; pillars A–E; queue invariant "R6 contract-integrity ahead of contractless breadth" (queue.yaml); refuse-loudly posture. **Drifting inventory:** crate/contract/command counts (31 / 35 / 7 today), Kani 24/35, Lean 35 modules, lowering-path coverage, publish version 0.1.616 — every number above re-verifies from the cited artifact.

---

## STEP 2 — Surface map (frontend → meta-HIR → backend, with proof state)

Reachable frontends (by extension): Python `.py/.pyi`, C `.c/.h`, Ruchy `.ruchy`, Shell `.sh/.bash/.zsh/.mk/Makefile/Dockerfile`, WASM `.wat`. The CLI imposes no pair table — refusal happens inside each backend (shape / source_lang / hardware gates).

| Lane | State | Strongest witness | Runs in CI? |
|------|-------|-------------------|-------------|
| Python→Rust | wired | `oracle_differential.rs` (34 no-hand-expectation fixtures, CPython vs rustc) + `diff_exec.rs` (10 fixtures × 2 paths × N=10 generated inputs) | **YES** (workspace-test — unenforced check, F1) |
| Python/C→Rust hybrid (C FFI) | wired | `hybrid --verify`: CPython+ctypes ref vs cargo-built Rust+C shim, byte-diff (main.rs:448-558); 1 golden + 2 single-tests | YES (same caveat) |
| Python→WASM | wired (scalar/list/str/dict subset; loud refusals) | **424 executing `#[test]`s across 87 witness files** (WABT `wat2wasm` + `wasm-interp` vs CPython) + two-emitter DiffExec | **NO — silently skips** (F3) |
| Python→WASI binary | wired (emit-crate) | CI `wasi` job: model.py → wasm32-wasip1 → wasmtime, byte-diff vs CPython | YES (1 program; advisory) |
| Shell→Shell (bashrs round-trip) | wired (flat + full v0.1.0 control flow) | 7 executing `shell_diff_exec.rs` round-trips (CPython vs `/bin/sh`) | YES (unenforced check) |
| Shell→forjar.yaml | wired (only Shell accepted) | structural serde_yaml round-trip only (documented backend-only posture) | string/structural only |
| Python/C/Ruchy→WGSL→(naga)→SPIR-V | partial (single scalar element-wise fn shape) | GPU witnesses on real Vulkan **when adapter present**; hosted CI falls back to naga validate + `NotRun` | GPU parts: NO (guarded skip; F5) |
| →PTX | **CLI-unreachable** (`hardware: None` hardcoded) | nvcc/GPU quorum witnesses, ptxas assemble — all GPU/toolchain-guarded, hand-constructed modules | NO (F4) |
| →Ruchy | wired | **string-compare only** — no ruchy execution anywhere in the test suite | no behavioral witness (F7) |
| →Lean | partial (fn subset) | string-compare in-crate; proof lane validated out-of-band by advisory `lake build` | advisory |
| WASM lift (→Rust etc.) | partial/lossy (structured control refused) | round-trip fixed-point witness | yes (unenforced) |

---

## STEP 3+4 — Gate-integrity findings and threat register (CF-4 hunt)

| id | finding / threat | status | artifact |
|----|------------------|--------|----------|
| F1 | **Believed-required check unenforced** *(as of 2026-07-05; RESOLVED — see row 8)*: the ruleset then required `gate` alone; `workspace-test` — the job that executes every in-tree differential witness — was required-by-comment only (ci.yml:50) | **FIRED** | ruleset JSON vs ci.yml |
| F2 | Proof lane real but **advisory**: lake-build, kani, lean-models all run per-PR, all ≤2m — none required | live | ci.yml comments "(yet)" |
| F3 | **WASM witnesses silently skip in CI**: no WABT install step anywhere in ci.yml; `wasm_runtime_available()` (wasm_diffexec.rs:177-184) needs `wat2wasm`+`wasm-interp` → all 424 witnesses skip-as-green on hosted runners. The only CI-executed WASM artifact is 1 wasi program — one point sampled from the most active lane (13 WASM PRs merged 2026-07-05 alone). Textbook CF-4 | **FIRED** | ci.yml grep, code |
| F4 | PTX advertised (README "target … a GPU"; 9-backend claim) but CLI-unreachable; refusal is loud/honest, claims surface isn't | FIRED (honesty gap, not silent-wrong) | main.rs:390,665,930 |
| F5 | naga monoculture: WGSL+SPIR-V are ONE oracle, not two; no naga-bump canary; no offline validate gate in CI (PMAT-482 exactly covers this, `status: planned`) | live watch-signal: any `Cargo.lock` naga bump | spirv lib.rs:249-258 |
| F6 | Dual source-of-truth: doctrine names dormant standalone depyler the "maintenance home" while xpile out-publishes a re-implemented subset; 4 sibling repos un-archived | live latent (dormant since Apr/May) | merger spec, gh, crates.io |
| F7 | Ruchy backend: shipped, advertised, **string-compare only** | live | no ruchy spawn in tests |
| F8 | Claims drift with no semantic gate: INVENTORY "25 vs 35" (its own sync gate checks a different literal), strategic_goals "27 modules", pillar D "lift COMPLETE" vs PMAT-952 `planned`, ci.yml required-check comments | **FIRED** (3+ live instances) | quoted above |
| F9 | `kani_verify.rs` no-ops green when `cargo kani` absent from PATH | latent (CI job bootstraps kani, so currently real) | kani_verify.rs |
| F10 | 4/35 contracts carry no falsifier; bashrs-posix-idempotence is a comment-only scaffold while its 7 executing witnesses exist unwired to the YAML | live | contracts grep + spot-read |
| — | MSRV drift | mitigated (toolchain file pins 1.93.0); residual: no named MSRV job | ci.yml |
| — | Set-iteration-order nondeterminism (2026-07-03 finding) | **RETIRED** — sets lower to `indexmap::IndexSet` at HEAD (rust-codegen lib.rs:1866) | code |
| — | Proof-lane theater (sorry/admit/axiom escape hatches) | **REFUTED** — 0 real escape hatches across 21,659 lines, warningAsError enforced | regex audit |

---

## 7b. EV-ranked backlog

```yaml
- id: XPILE-RULESET-001
  surface: infra
  type: gate
  ev_rank: 1
  ev_rationale: Every other gate inherits its meaning from enforcement; the entire executing-witness surface lives under an unenforced check (F1), and the fix is a minutes-cheap ruleset edit with zero CI-latency cost (workspace-test = 3m05s).
  definition_of_done: "Ruleset for main lists required contexts `gate` AND `workspace-test` (and strict=true decided explicitly). Mutation that must turn it RED: a throwaway PR with one deliberately failing #[test] shows workspace-test red AND GitHub refuses the merge. Also reconcile ci.yml:12/50 comments with the actual ruleset in the same change."
  blocked_by: none (needs paiml org-admin — ruleset 13878864 is org-sourced, not a repo file)
  artifact_on_completion: falsifier (the blocked-merge probe PR) + ruleset JSON snapshot committed to docs/status/
  workflow_note: org ruleset edit (outside PR flow) + a comment-fix PR → `ci / gate`

- id: XPILE-WITNESS-001
  surface: backend:wasm
  type: gate
  ev_rank: 2
  ev_rationale: 424 executing witnesses exist and CI runs none of them (F3) — the largest wired-but-unsampled surface in the repo, on the most active lane; fix is one apt-get step plus an anti-skip guard.
  definition_of_done: "CI installs WABT (wat2wasm + wasm-interp) before workspace-test; a CI-set env var (e.g. XPILE_REQUIRE_WASM_RUNTIME=1) makes wasm_runtime_available()==false a panic, not a skip. Mutations that must turn it RED: (a) delete the WABT install step → job fails instead of passing-by-skip; (b) flip one compare opcode in the in-place-sort WAT emit → the sort witness fails IN CI."
  blocked_by: none (XPILE-RULESET-001 makes it enforced rather than advisory-green)
  artifact_on_completion: falsifier (CI-red run under both mutations, linked in PR)
  workflow_note: ticket → branch → PR → `ci / gate`

- id: XPILE-WITNESS-002
  surface: cross-cutting
  type: gate
  ev_rank: 3
  ev_rationale: Kills the silent-skip CLASS (WABT/GPU/python3/cc guards all share the skip-as-green pattern) rather than one instance; skip-as-green is the exact CF-4 signature.
  definition_of_done: "A required test emits an executed-vs-skipped witness manifest per lane and asserts hosted-CI floors: wasm ≥400, shell ≥7, rust-differential ≥44, hybrid ≥3, wasi ≥1; GPU lanes must report skipped-with-reason (expected on hosted runners), never silently absent. Mutation that must turn it RED: force any one lane's availability guard to false → floor assertion fails."
  blocked_by: XPILE-WITNESS-001 (the wasm floor is unmeetable before WABT lands)
  artifact_on_completion: falsifier
  workflow_note: ticket → branch → PR → `ci / gate`

- id: XPILE-RULESET-002
  surface: proof-lane
  type: gate
  ev_rank: 4
  ev_rationale: lake-build (16s), kani (2m02s), lean-models (1m34s) are real, currently green, and latency-free to promote; an advisory proof lane compounds no confidence (§4.2), and this closes the kani PATH-skip hole (F9).
  definition_of_done: "lake-build and kani added to required contexts; kani_verify.rs fails (not warns) when cargo-kani is missing and CI=true. Mutations that must turn it RED: (a) a PR adding `sorry` to any pilot module → lake-build red, merge blocked (warningAsError makes sorry→sorryAx an error); (b) a PR inverting one asserted property in an existing harness → kani red."
  blocked_by: XPILE-RULESET-001 (same ruleset-edit session)
  artifact_on_completion: falsifier + the existing 35-module lean_proof corpus becoming enforceable
  workflow_note: org ruleset edit + repo PR for the kani_verify hard-fail → `ci / gate`

- id: XPILE-CLAIMS-001
  surface: cross-cutting
  type: gate
  ev_rank: 5
  ev_rationale: Three live drift instances (F8) and no gate samples any of them; deriving counts from the code kills the class permanently at trivial cost — the direct analog of the missing claims-gate the §3 note predicted.
  definition_of_done: "Extend the lean_pilot_roots-style sync gates: (a) README's 'five source languages'/'nine backends' derived from the frontend registry and Target enum; (b) every module-count literal in PROVABILITY-INVENTORY.md (including the line-11 reproduce block) synced to lakefile roots; (c) a roadmap-consistency lint: an id claimed COMPLETE in strategic_goals must not be status:planned (catches the PMAT-952 case). Mutation that must turn it RED: the gate must first fail on the EXISTING '25 vs 35' drift before that drift is fixed in the same PR (red-then-green proof)."
  blocked_by: none
  artifact_on_completion: falsifier
  workflow_note: ticket → branch → PR → `ci / gate`

- id: XPILE-CONTRACT-001
  surface: proof-lane
  type: contract
  ev_rank: 6
  ev_rationale: Doctrine §4.3 is violated by exactly 4 of 35 contracts (F10); bashrs-posix-idempotence already HAS 7 executing witnesses — wiring them into the contract YAML is low-cost, high-honesty. Re-scopes stale PMAT-468 ("10 placeholder contracts" — the true current gap is these 4).
  definition_of_done: "bashrs-posix-idempotence (de-scaffold: real equations), ffi-shell-subprocess, py-float-arith, xlate-py-set-to-hashset each carry ≥1 falsification_tests entry naming an EXISTING executing test; pv lint stays green. Mutation per contract that must turn its named test RED: e.g. reorder emitted shell statements → the named shell_diff_exec round-trip fails; flip IndexSet to HashSet in set lowering → the named set witness fails."
  blocked_by: none
  artifact_on_completion: pv_contract + falsifier
  workflow_note: ticket → branch → PR → `ci / gate` (pv lint is on the required path already)

- id: PMAT-482
  surface: backend:wgsl
  type: gate
  ev_rank: 7
  ev_rationale: Already planned in-repo; hosted CI executes zero GPU-lane artifacts and naga is a single shared oracle for WGSL+SPIR-V (F5) — an offline naga+spirv-val gate on free CI converts skip into validated and makes every naga bump a full-corpus re-validation by construction.
  definition_of_done: "Per-PR job validates every corpus WGSL emission via naga parse+validate and every SPIR-V word-stream via spirv-val, CPU-only, no GPU, no skip path. Mutation that must turn it RED: introduce a syntax error into the WGSL emitter template → job red on a GPU-less runner."
  blocked_by: none
  artifact_on_completion: falsifier
  workflow_note: ticket → branch → PR → `ci / gate`; promote to required once stable

- id: XPILE-CLEANROOM-001
  surface: infra
  type: gate
  ev_rank: 8
  ev_rationale: Doctrine §4.5 names clean-room a HARD release gate; xpile publishes 31 crates via a manual Friday process with no publish workflow and no dry-run/clean-room gate anywhere in CI (spec prose only — xpile-spec.md:803). Healthy today (lag 31 commits) — gate it before it fires.
  definition_of_done: "A release workflow (tag- or dispatch-triggered) runs cargo publish --workspace --dry-run plus a clean-room build of the published crate set under an isolated CARGO_HOME (no sibling paths). Mutations that must turn it RED: (a) strip version= from one workspace path-dep → dry-run red; (b) introduce a path-only dependency on an unpublished crate → clean-room red."
  blocked_by: none
  artifact_on_completion: falsifier + PR
  workflow_note: ends in a publish → this item IS the clean-room gate named first (§4.5); ticket → branch → PR → `ci / gate`

- id: XPILE-WITNESS-003
  surface: backend:ruchy
  type: preservation
  ev_rank: 9
  ev_rationale: Ruchy is a shipped, README-advertised backend whose strongest check is string-compare (F7) — the only text-only lane among executable targets; converting it is a preservation gate on existing surface, which outranks any new breadth (§4.1).
  definition_of_done: "A witness runs the emitted Ruchy (pinned cargo-install ruchy, or ruchy→Rust→rustc chain) and byte-diffs stdout vs CPython on ≥10 oracle fixtures, floor-asserted per XPILE-WITNESS-002. Mutation that must turn it RED: swap +/− in the ruchy emitter's BinOp mapping → witness red."
  blocked_by: none hard; XPILE-WITNESS-002 for the floor wiring
  artifact_on_completion: falsifier
  workflow_note: ticket → branch → PR → `ci / gate`

- id: XPILE-SOT-001
  surface: cross-cutting
  type: infra
  ev_rank: 10
  ev_rationale: Live doctrine contradiction (F6): the merger spec names dormant standalone depyler the "maintenance home" while xpile re-implemented and out-publishes it; four un-archived siblings can drift the moment anyone commits there. Governance-cheap, correctness-load-bearing.
  definition_of_done: "A decision lands: EITHER (a) execute the declared collapse — paiml/depyler|decy|ruchy|bashrs become ~50-LoC re-export shims (bashrs-merger.md:9) with frozen/archive notes — OR (b) amend v0.2.0-depyler-merger.md + migration.md to declare xpile the maintenance home; plus a standing watch-signal (documented periodic sibling-HEAD diff). Verification is by reading the amended doctrine and repo states (no code mutation applies — governance item)."
  blocked_by: user/owner decision (archiving external repos is outside PR flow)
  artifact_on_completion: PR (doctrine) + sibling repo-state change
  workflow_note: needs explicit owner approval for the external-repo half; in-repo doctrine PR → `ci / gate`

- id: PMAT-1008
  surface: meta-hir
  type: preservation
  ev_rank: 11
  ev_rationale: The deepest semantic-preservation gap (Python reference aliasing, value-vs-reference) at the pinch point — but it is already contained by the alias-then-mutate clean-reject stopgap (queue.yaml V29-2), which is why minutes-cheap enforcement gates outrank it despite its leverage. Already in_progress in roadmap.yaml.
  definition_of_done: "The aliasing corpus (alias-as-move E0382 case + arg-clone-drops-mutation + list-alias subscript-write, incl. PMAT-1035's over-refusal) either matches CPython differentially or refuses loudly — zero silent divergence. Falsifier that must stay RED until done: `ys = xs; ys.append(1); print(len(xs))` must never print a wrong length."
  blocked_by: none (architectural, multi-slice)
  artifact_on_completion: falsifier + pv_contract (aliasing equations added to C-XLATE-PY-LIST-TO-VEC)
  workflow_note: ticket → branch → PR → `ci / gate`

- id: XPILE-PTX-001
  surface: backend:ptx
  type: gate
  ev_rank: 12
  ev_rationale: A README-implied GPU path that no CLI invocation can reach (F4 — hardware: None hardcoded at main.rs:390,665,930) is an honesty gap even though the refusal is loud; the plumbing is small and the alternative (documenting witness-lane-only status) is nearly free.
  definition_of_done: "EITHER `xpile transpile foo.py --target ptx --hardware ptx` emits PTX validated by the existing ptxas offline path, OR README/docs state PTX is witness-lane-only and XPILE-CLAIMS-001 pins that sentence. If plumbed, mutation that must turn the witness RED: corrupt the emitted PTX header → ptxas validation fails."
  blocked_by: none
  artifact_on_completion: falsifier | PR
  workflow_note: ticket → branch → PR → `ci / gate`

- id: PMAT-487
  surface: infra
  type: infra
  ev_rank: 13
  ev_rationale: With PMAT-489/490/488 — converts the GPU lanes' local-only Run≥1 witnesses into per-PR CI execution (Run≥2 in the roadmap's own words); high ops cost is why it sits below every free gate above.
  definition_of_done: "Self-hosted sm_89 (RTX 4090) and AMD-Vulkan runners registered and green; gpu_witness.rs executes per-PR on them; GPU floors added to the XPILE-WITNESS-002 manifest. Mutation that must turn it RED: corrupt the PTX emitter's fma lowering → GPU witness job red."
  blocked_by: hardware/ops access (lambda-labs + intel boxes)
  artifact_on_completion: falsifier + benchmark
  workflow_note: runner bring-up (ops) → then per-PR jobs → `ci / gate`

- id: PMAT-476
  surface: cross-cutting
  type: infra
  ev_rank: 14
  ev_rationale: Repo-doctrine item with a HARD CI-gate date (2026-08-15) — calendar-bound, not EV-relitigated.
  definition_of_done: "Per its roadmap.yaml entry: the 2026-Q3 SOTA dossier lands with its CI gate before 2026-08-15."
  blocked_by: none
  artifact_on_completion: PR + the dated gate
  workflow_note: ticket → branch → PR → `ci / gate`

- id: PMAT-985-DICT-ITER
  surface: backend:wasm
  type: breadth
  ev_rank: 15
  ev_rationale: The highest-leverage CAPABILITY gap (for-in-dict unlocks update/merge/values/items on the WASM lane; heap/string/dict frontier per strategic_goals) — ranked last because §4.1 puts every falsifiable gate above breadth, and pillar B requires it ship contract-carried ("never bare codegen").
  definition_of_done: "`for k in d:` (and the unlocked family) executes CPython-exact under WABT witnesses including deletion-order composed-mutation fuzz (the PMAT-1287 pattern); contract equations extended in the dict xlate contract; unsupported forms keep loud refusals. Mutation that must turn it RED: skip the swap-last-into-hole order adjustment after a del-during-iteration fixture → witness red."
  blocked_by: XPILE-WITNESS-001 (its witnesses must actually run in CI first)
  artifact_on_completion: falsifier + pv_contract
  workflow_note: ticket → branch → PR → `ci / gate`
```

---

## 7c. Do-not-do list

1. **Any new frontend/backend or Nth construct without a semantic-preservation contract + executing witness** — xpile's own doctrine, twice over: queue.yaml ("R6 contract-integrity … ahead of contractless breadth") and pillar B ("never bare codegen"). §4.1 EV rule.
2. **Syntactic/AST-shape/formatted-source equality as an acceptance gate** — theater (§4.2); xpile's established bar is executed differentials (`oracle_differential`, `shell_diff_exec`, WABT witnesses). Any PR whose only evidence is string-compare on emitted text does not close a preservation claim.
3. **General-purpose optimizer parity vs rustc/LLVM** — *not from repo doctrine* (flagged per §2): nothing at HEAD pursues it, and the north star explicitly names provability, "not idiom coverage," as the differentiator. Confirm with the owner before this ever enters a backlog.
4. **cuda-oxide as a 3rd PTX emitter** — xpile's own pillar C: "now MARGINAL — the 2-emitter anti-correlation it was meant to enable is already real; … deprioritized nightly-lane option."
5. **Mathlib imports into `contracts/lean/`** — the pilot is import-free/Mathlib-free by construction (PROVABILITY-INVENTORY.md lines 23-32); Mathlib work lives only in the separate `contracts/lean-models/` lane with its own lakefile/toolchain.
6. **Padding Kani breadth past 24/35** — the 11 uncovered contracts (the 4 `compile-*` emission contracts, c-wasm-heap, ffi-shell-subprocess, and the 4 behavioral `py-*`) need effect/codegen modeling, not BMC harnesses. *Rationale from prior-session tiering analysis, not a repo artifact — marked as such; confirm against `docs/specifications/sub/provability-roadmap.md` before relitigating.*
7. **Re-implementing standalone depyler 4.x's full surface in-tree while `paiml/depyler` stays live** — the duplicate-maintenance hazard the merger spec's snapshot-and-mirror doctrine exists to prevent; blocked pending XPILE-SOT-001.
8. **Folding kani/GPU jobs into workspace-test** — ci.yml:68-80 documents the deliberate separation (fast-feedback latency + at-a-glance diagnosability); promotion to *required* (XPILE-RULESET-002) is the right move, consolidation is not.

---

## 7d. UNVERIFIED / needs-live-access appendix

| gap | exact artifact a human must fetch to close it |
|-----|-----------------------------------------------|
| That ubuntu-latest truly lacks `wat2wasm`/`wasm-interp` (F3 rests on: no install step in ci.yml + the guard's requirements; runner-image contents were inferred, not fetched) | `gh run view <latest workspace-test job> --log \| grep -i "skip"` — the job log showing the wasm witnesses' skip lines (or the runner image manifest at github.com/actions/runner-images) |
| That the ruleset context `gate` binds to ci.yml's `gate` job and not some other check provider with the same name | A throwaway PR with a deliberate `cargo fmt` failure: observe that merge is blocked by exactly the `gate` check-run from workflow `ci` |
| That the CI kani job genuinely verified all 101 harnesses in 2m02s (kani_verify panics on any failure, and the job was green — but I did not read the log) | `gh run view 28742633266 --log` for the kani job; count `VERIFICATION:- SUCCESSFUL` occurrences = 101 |
| PMAT-953's "forjar golden `validate_config` validation is evaluated separately" — no in-tree evaluation was found (dev-dep comment defers it; structural serde_yaml round-trip only) | The PMAT-953 PR body/diff (`gh pr list --search "PMAT-953" --state merged`) or a fleet-wide search for where that separate evaluation lives |
| Whether any standalone-depyler patches since the merger were actually "mirrored downstream" per doctrine (v0.2.0-depyler-merger.md:194-197) | Diff `paiml/depyler` commits 2026-02→2026-04 against xpile's CHANGELOG/depyler-frontend history for the corresponding rules |
| Publish completeness across all 31 member crates (verified: xpile, xpile-core, depyler-frontend at 0.1.616) | `for c in <members>; do curl -A ua "https://crates.io/api/v1/crates/$c"; done` — newest_version per crate |
| Private-infra PMAT-9xx ids | **None outstanding** — all six §1-cited ids resolved in-repo with `status: done`; no dangling id was encountered in this analysis |

**§8 self-check:** every 7b item names its surface, the exact mutation (or decision) that constitutes done, and its source artifacts from 7a/STEP 3 — a reviewer can open the ticket or reject it from the block alone. `XPILE-SOT-001` is the one item that is a decision rather than a gate; it is specified as such rather than dressed as code work.
