# CI Pipeline and Gates

**Section 18 of [xpile-spec.md](../xpile-spec.md).**

> **Status (2026-05-18 / PMAT-101 sweep):** This document was
> authored at v0.0.1 scaffold time describing an aspirational
> quality regime. Several originally-planned gates (llvm-cov,
> cargo-mutants, pmat tdg, pv score, check_provenance.sh) did
> NOT ship as required CI gates at v0.1.0; the actual workflow
> in `.github/workflows/ci.yml` is leaner and gates only what
> the substrate-completion run actually relies on. The "Gates
> that hard-fail the PR" table below is now anchored to the
> live workflow. Originally-planned gates that haven't shipped
> are listed in "Gates planned but not yet wired" below for the
> Popperian falsification trace.

## Live workflow (`.github/workflows/ci.yml`)

```yaml
# (sketch — see .github/workflows/ci.yml for the actual source)
jobs:
  gate:                  # required status check
    - cargo fmt --all -- --check
    - cargo check --workspace
    - cargo clippy --workspace --all-targets -- -D warnings
    - pv lint contracts/                          # via aprender-contracts-cli
    - cargo deny check advisories

  workspace-test:        # required status check
    - cargo test --workspace                       # includes every_kani_harness_discharges
                                                   # citation-gate, refinement_proofs, quorum,
                                                   # attestations

  kani:                  # optional check (not yet required)
    - cargo install --locked kani-verifier
    - cargo kani --version                         # bootstrap toolchain
    - cargo test -p xpile --test kani_verify -- --nocapture
                                                   # actually invokes `cargo kani` on every
                                                   # contracts/kani/*.rs harness
```

## Gates that hard-fail the PR (live)

| Gate | Command | Threshold |
|---|---|---|
| Format | `cargo fmt --all -- --check` | clean |
| Type check | `cargo check --workspace` | clean |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | zero warnings |
| Provable-contracts (lint) | `pv lint contracts/` | 0 errors |
| Security advisories | `cargo deny check advisories` | zero unyanked |
| Tests | `cargo test --workspace` | all pass — includes the §14.4 stratum gates `refinement_proofs.rs` (Lean theorem citation gate), `kani_harnesses.rs` (Kani citation gate), `kani_verify.rs` (actual `cargo kani` verification on every harness), `quorum.rs` (C-PY-INT-ARITH full-stratum assertion), `attestations.rs` (Extrinsic-stratum scanner) |
| Kani BMC | `cargo test -p xpile --test kani_verify` | 43 harnesses must all return `VERIFICATION:- SUCCESSFUL` (post-XPILE-QUORUM-006 / PMAT-147..151) |

## Gates that are recommended at the local pre-push checklist

Per `CLAUDE.md`, contributors run these locally before pushing:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
pv lint contracts/
cargo deny check advisories
```

Plus, after substrate-completion work (PMAT-058..077):

```bash
cargo test --workspace            # exercises the stratum gates
cargo test -p xpile --test kani_verify   # if `cargo-kani` is installed
```

## Gates planned but not yet wired (post-v0.1.0)

These were in the original v0.0.1 ci-gates plan but did not ship as required CI status checks at v0.1.0. Each is a candidate post-v0.1.0 ticket:

| Planned gate | Why deferred | Tracking ticket |
|---|---|---|
| `cargo llvm-cov` ≥95% line coverage | Coverage tooling installation is heavy; baseline measurement needed before threshold enforcement | XPILE-CI-COVERAGE-001 |
| `cargo mutants` ≥80% mutation coverage on changed code | Same; mutation testing is slow per-PR | XPILE-CI-MUTANTS-001 |
| `pv score --no-regression-vs main` | Requires `pvscore` baseline maintenance | XPILE-CI-SCORE-001 |
| `pmat tdg --min-grade A-` | Requires baseline grade calibration | XPILE-CI-PMAT-TDG-001 |
| `scripts/check_provenance.sh` | Repair-mode output isn't generated at v0.1.0 (agent loop is scaffold-only) | XPILE-CI-PROVENANCE-001 |
| `pv coverage` (cross-contract obligation matrix) | Computes a global matrix — designed for nightly | XPILE-CI-PV-COVERAGE-001 |
| `pv audit` (paper→proof traversal) | Requires audit chain to be wired through paper/proof artifacts | XPILE-CI-PV-AUDIT-001 |

These weren't dropped — they're sequenced behind the substrate-completion work that just shipped. The substrate was the load-bearing prerequisite (you can't measure coverage of contracts that don't exist, or mutation-test invariants that haven't been formalized). With 12 contracts at QUORUM, several of these become tractable for v0.2.0+.

## No-skip rules

- **No `--no-verify` on commits.** `pre-commit` hooks run the local pre-push checklist.
- **No bypassing `pv lint`.** Even draft PRs must lint clean. There is no `[skip-pv]` directive.
- **No bypassing the Kani gate** (once flipped to required). At v0.1.0 Kani is an optional check; the citation gate (`kani_harnesses.rs`) is required via the `workspace-test` job.

## Branch protection

The `main` branch is protected via an **org-level ruleset rule** (live as of v0.1.0, verifiable with `gh api repos/paiml/xpile/rules/branches/main`):

- Required status checks: `gate` only at v0.1.0 (the load-bearing fast-feedback gate; workspace-test and kani both run on every PR but aren't yet flipped to "required" at the ruleset layer — that's post-v0.1.0 work once Kani has bedded in)
- Pull request required (zero approving reviews required at v0.1.0 — autonomous shipping per this repo's `CLAUDE.md`)
- `non_fast_forward` enforced — no force pushes to main
- Allowed merge methods: merge / squash / rebase

The ruleset is enforced at the GitHub org level (paiml organization), not via the older `repos/owner/repo/branches/main/protection` API. The newer rulesets API is the canonical source.

## Failure-recovery patterns

| Failure | What to do |
|---|---|
| Clippy warning | Fix the lint; don't `#[allow(...)]` without a justification comment |
| `pv lint` fail | Fix the contract; don't edit `pv` |
| `cargo deny advisories` fail | Audit the advisory; update the dependency or add a justified `[advisories.ignore]` entry |
| Kani timeout | The harness is too symbolic — switch to fixed-size byte-array modelling (see PMAT-058 for the canonical example: symbolic `String` timed out at 628s; `[u8; 4]` verified in 1s) |
| Workspace test fail | Read the failure carefully; the `refinement_proofs` / `kani_harnesses` gates produce highly-specific error messages naming the contract YAML field that's misaligned |

## CI cost (live, not projected)

| Job | Wall-clock | Cost |
|---|---|---|
| `gate` | ~22-28s | minimal (GitHub-hosted runners) |
| `workspace-test` | ~31-38s | minimal |
| `kani` | ~33-42s | minimal (after Kani toolchain is cached) |
| Total per-PR CI | ~1-2 min (parallel) | minimal |

The originally-planned cost gates (per-PR repair-token cost, per-PR $ cost) were aspirational at v0.0.1 when the design assumed heavy LLM use. The substrate-completion run shipped without any LLM-mediated repair, so these gates are post-agent-loop work.
