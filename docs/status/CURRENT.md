# xpile — Current Status

**Last refreshed:** 2026-05-18 (PMAT-233 — post-PMAT-228..232 Diamond depth-2 UNIVERSAL session — every layer of the taxonomy now has at least one contract with two distinct Diamond categories)
**Canonical source of truth for the supported subset:** [`/CHANGELOG.md`](../../CHANGELOG.md)

This file used to enumerate every implemented crate / contract / construct, and went stale within hours of each PR. The previous 180-line snapshot is preserved in git history (last useful version: commit `cdcece9`, the initial bootstrap). Going forward, this file is a thin index — anything that needs to stay accurate lives in `CHANGELOG.md`.

## High-water mark (v0.1.0, 2026-05-18 substrate completion)

- 27 workspace crates compile clean; `cargo check`, `cargo clippy -D warnings`, `cargo fmt --check`, `cargo deny check advisories` all green
- 12 contracts pass `pv lint` (0 errors, **0 warnings** — full clean state as of PMAT-138 closing XPILE-REFINE-005). Every equation carries domain-grounded pre/postconditions, every equation is anchored to a Lean refinement theorem, every contract declares a `qa_gate`.
- **100% §14.4 QUORUM, 100% Silver, 100% Gold, 100% Platinum, 100% Diamond — UNIVERSAL 5-TIER COVERAGE + UNIVERSAL DIAMOND DEPTH-2** — every contract has paired Lean refinement theorem + Kani BMC harness, every equation has Silver-tier refinement (PMAT-183), every contract has at least one Gold-tier refinement-subtype theorem (PMAT-197), every contract has at least one Platinum-tier compositional theorem (PMAT-212), AND every contract has at least one Diamond-tier multi-axiom theorem (PMAT-226 — UNIVERSAL Diamond depth-1). **PMAT-228..232 opened Diamond depth-2** — every one of the 5 layers in the contract taxonomy now has at least one contract with TWO distinct Diamond categories. `xpile quorum` → `12 QUORUM, 0 PARTIAL, 0 UNVERIFIED`. **247 Lean theorems (53 Bronze + 108 Silver + 24 Gold + 39 Platinum + 23 Diamond) + 43 Kani harnesses = 290 stratum-vote artifacts** post-PMAT-232. **Twelve Diamond algebraic categories** at depth-1 (one per contract): commutative-monoid/semiring (PMAT-214), pure-function (PMAT-215), abelian-group (PMAT-216), equivalence-relation (PMAT-217), bounded-monoid (PMAT-218), string-monoid (PMAT-219), free list-monoid (PMAT-221), inductive-monoid (PMAT-222), precondition-list-monoid (PMAT-223), frontend equivalence-class (PMAT-224), backend equivalence-class (PMAT-225), citation render-monoid (PMAT-226). **Five Diamond depth-2 categories** spanning all 5 layers: Euclidean-domain on Layer 1 (PMAT-228), NonEmpty section-retraction on Layer 2 (PMAT-229), constant-projection on Layer 3 (PMAT-232), GIL-invariant preservation on Layer 4 (PMAT-230), join-semilattice on Layer 5 (PMAT-231).
- **`pmat tdg .` score: 95.7 / 100 (Grade A-)** — meets the originally-planned XPILE-CI-PMAT-TDG-001 ≥ A- threshold without explicit CI enforcement.
- Four real backends: Rust (`pub fn`), Ruchy (`fun`), Lean 4 (`def`), Shell/bashrs (POSIX subset). PTX / WGSL / SPIR-V still scaffolded.
- Python subset supported: see [`CHANGELOG.md`](../../CHANGELOG.md) §"Python subset (live, runtime-verified)"
- Shell subset supported: POSIX tokenizing (quoted strings, $NAME/${NAME}, $(cmd), backtick subst, NAME=value, pipelines, ShellLoop, special parameters); see CHANGELOG PMAT-037..058
- Multiple runtime-verified semantic fixtures: factorial, fib, gcd, abs_val, sign, bits, square_plus, range_size, sum_to, for_sum / range_with_start / range_with_step, factorial_iter, bigint_factorial — plus shell `bashrs_realistic_demo.sh` round-tripping byte-identically
- **43 Kani BMC harnesses** verify on every CI run via dedicated `kani` job + `every_kani_harness_discharges` workspace test (XPILE-QUORUM-006 series PMAT-147..151 added per-equation symbolic coverage for the 5 multi-equation contracts)
- CI: `gate` + `kani` + `workspace-test` all run on every PR; `gate` is the load-bearing required status check via the org-level ruleset rule (verifiable with `gh api repos/paiml/xpile/rules/branches/main`). `kani` and `workspace-test` are not yet required-status-checks but in practice green on every merged PR — flipping them required is post-v0.1.0 work.
- crates.io: `xpile 0.0.1` published as a name reservation; v0.1.0+ unreleased
- 184 PRs merged on `main`

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
