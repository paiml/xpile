# xpile — Current Status

**Last refreshed:** 2026-05-18 (PMAT-252 — post-PMAT-241..251 Diamond depth-3 UNIVERSAL across layers + depth-4 opened + CI gate enforced)
**Canonical source of truth for the supported subset:** [`/CHANGELOG.md`](../../CHANGELOG.md)

This file used to enumerate every implemented crate / contract / construct, and went stale within hours of each PR. The previous 180-line snapshot is preserved in git history (last useful version: commit `cdcece9`, the initial bootstrap). Going forward, this file is a thin index — anything that needs to stay accurate lives in `CHANGELOG.md`.

## High-water mark (v0.1.0, 2026-05-18 substrate completion)

- 27 workspace crates compile clean; `cargo check`, `cargo clippy -D warnings`, `cargo fmt --check`, `cargo deny check advisories` all green
- 12 contracts pass `pv lint` (0 errors, **0 warnings** — full clean state as of PMAT-138 closing XPILE-REFINE-005). Every equation carries domain-grounded pre/postconditions, every equation is anchored to a Lean refinement theorem, every contract declares a `qa_gate`.
- **100% §14.4 QUORUM, 100% Silver, 100% Gold, 100% Platinum, 100% Diamond — UNIVERSAL 5-TIER + UNIVERSAL Diamond depth-2 (CI-enforced) + UNIVERSAL Diamond depth-3 across all 5 layers + depth-4 opened** — every contract has paired Lean refinement theorem + Kani BMC harness, every equation has Silver-tier refinement (PMAT-183), every contract has at least one Gold-tier theorem (PMAT-197), every contract has at least one Platinum-tier theorem (PMAT-212), every contract has at least one Diamond theorem (PMAT-226 — depth-1 UNIVERSAL), every contract has at least 2 distinct Diamond categories (PMAT-228..250 — depth-2 UNIVERSAL), every one of the 5 layers has at least one contract with 3 distinct Diamond categories (PMAT-241..245 — depth-3 across layers), AND **2 contracts have 4 distinct Diamond categories** (PMAT-247 PyIntArith Layer 1, PMAT-248 CompileRustToPtxMma Layer 5). **Diamond CI gate** (PMAT-251) enforces these invariants — substrate-wide Diamond coverage cannot regress. `xpile quorum` → `12 QUORUM, 0 PARTIAL, 0 UNVERIFIED`. **`xpile diamond`** reporter (PMAT-249) provides live counts. **260 Lean theorems (53 Bronze + 108 Silver + 24 Gold + 39 Platinum + 36 Diamond) + 43 Kani harnesses = 303 stratum-vote artifacts** post-PMAT-251. **31 wired Diamond equations** across 12 contracts: 12 depth-1 + 11 depth-2 + 5 depth-3 + 2 depth-4 + 1 additional wired (PMAT-250) = 31.
- **`pmat tdg .` score: 95.1 / 100 (Grade A-)** — meets the originally-planned XPILE-CI-PMAT-TDG-001 ≥ A- threshold without explicit CI enforcement (slight dip from 95.7 after the +600 lines of Diamond-program documentation; still solidly A-).
- Four real backends: Rust (`pub fn`), Ruchy (`fun`), Lean 4 (`def`), Shell/bashrs (POSIX subset). PTX / WGSL / SPIR-V still scaffolded.
- Python subset supported: see [`CHANGELOG.md`](../../CHANGELOG.md) §"Python subset (live, runtime-verified)"
- Shell subset supported: POSIX tokenizing (quoted strings, $NAME/${NAME}, $(cmd), backtick subst, NAME=value, pipelines, ShellLoop, special parameters); see CHANGELOG PMAT-037..058
- Multiple runtime-verified semantic fixtures: factorial, fib, gcd, abs_val, sign, bits, square_plus, range_size, sum_to, for_sum / range_with_start / range_with_step, factorial_iter, bigint_factorial — plus shell `bashrs_realistic_demo.sh` round-tripping byte-identically
- **43 Kani BMC harnesses** verify on every CI run via dedicated `kani` job + `every_kani_harness_discharges` workspace test (XPILE-QUORUM-006 series PMAT-147..151 added per-equation symbolic coverage for the 5 multi-equation contracts)
- CI (**re-derived against the live API 2026-07-26, PMAT-1347** — the previous sentence here was stale in both directions): eight jobs run on every PR, and org ruleset `13878864` requires exactly two of them.
  <!-- XPILE-ENFORCEMENT REQUIRED-CONTEXTS: gate, workspace-test -->
  **Required (merge-blocking):** `gate`, `workspace-test`. **Advisory (run every PR, red on a real regression, do NOT block a merge):** `kani`, `lake-build`, `docs`, `wasi`, `lean-models`, `shader-validate`. Verify with `gh api repos/paiml/xpile/rules/branches/main`; the claim is pinned by `crates/xpile/tests/ruleset_drift.rs`. Promoting the proof lane to required is an owner-gated org-admin edit — see [`enforcement-handoff.md`](enforcement-handoff.md) §2.
- crates.io: `xpile 0.0.1` published as a name reservation; v0.1.0+ unreleased
- 217 PRs merged on `main` (was 184 — the Diamond program shipped 32 PRs at PMAT-226..257 with the Diamond depth-1/2/3/4 milestones + reporter + CI gate + taxonomy doc + Section 28)

## Where to look next

| You want to know | Read |
|---|---|
| What Python constructs are supported | [`/CHANGELOG.md`](../../CHANGELOG.md) §"Python subset" |
| What's planned next | `pmat work list` |
| How the architecture is shaped | [`/docs/specifications/xpile-spec.md`](../specifications/xpile-spec.md) |
| What the adversarial audit found | [`/docs/specifications/audit-design.md`](../specifications/audit-design.md) |
| How a frontend / backend plugs in | [`sub/frontend-trait.md`](../specifications/sub/frontend-trait.md) / [`sub/backend-trait.md`](../specifications/sub/backend-trait.md) |
| Why Lean and LaTeX are bidirectional | [`sub/lean-bidirectional.md`](../specifications/sub/lean-bidirectional.md) / [`sub/latex-bidirectional.md`](../specifications/sub/latex-bidirectional.md) |

## Why this file is a stub now

Five-whys for the previous 180-line snapshot:

- **Symptom:** every section ("Done", "Crates", "Contracts", "Next steps") drifted from reality within days
- **Why 1:** hand-authored at v0.1.0 scaffold time, never re-authored
- **Why 2:** the same facts were already authoritative elsewhere (Cargo.toml, contracts dir, CHANGELOG)
- **Why 3:** duplicating means two places to keep in sync; only one ever was
- **Root cause:** there was no canonical source for "what's done"; this file was a parallel one. Fix: declare CHANGELOG.md canonical (per PMAT-001) and demote this file to a pointer.
