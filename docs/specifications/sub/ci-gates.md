# CI Pipeline and Gates

**Section 18 of [xpile-spec.md](../xpile-spec.md).**

## Every PR runs

```yaml
# .github/workflows/ci.yml (sketch)
jobs:
  gate:
    steps:
      - uses: dtolnay/rust-toolchain@1.93.0

      - run: cargo fmt --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo check --workspace
      - run: cargo test --workspace

      - run: cargo install cargo-llvm-cov --locked
      - run: cargo llvm-cov --workspace --fail-under-lines 95

      - run: cargo install cargo-mutants --locked
      - run: cargo mutants --baseline auto --in-diff origin/main

      - run: cargo install cargo-deny --locked
      - run: cargo deny check advisories licenses

      - run: cargo install --path /home/noah/src/provable-contracts/crates/aprender-contracts-cli
      - run: pv lint contracts/
      - run: pv score contracts/ --threshold 0.5 --no-regression-vs main

      - run: cargo install pmat --locked
      - run: pmat tdg --min-grade A-

      - run: bash scripts/check_provenance.sh   # custom — no repaired .rs without marker
```

## Gates that hard-fail the PR

| Gate | Command | Threshold |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Clippy | `cargo clippy -- -D warnings` | zero warnings |
| Check | `cargo check --workspace` | clean |
| Tests | `cargo test --workspace` | all pass |
| Line coverage | `cargo llvm-cov` | ≥ 95% |
| Mutation coverage | `cargo mutants` | ≥ 80% on changed code |
| Security advisories | `cargo deny check advisories` | zero unyanked |
| License audit | `cargo deny check licenses` | allowed list only |
| Provable-contracts | `pv lint` | 8/8 gates pass |
| Contract score | `pv score --no-regression-vs main` | ≥ baseline |
| Technical debt | `pmat tdg` | ≥ A- |
| Provenance | `scripts/check_provenance.sh` | no orphan markers |

## Gates that hard-fail nightly (not on every PR)

| Gate | Command | Why nightly |
|---|---|---|
| Kani proofs | `cargo kani --workspace` | Slow (minutes per harness) |
| Cross-contract obligation coverage | `pv coverage` | Computes a global matrix |
| Audit chain | `pv audit` | Slow paper→proof traversal |
| Mutation coverage (full) | `cargo mutants` | Full corpus, not just diff |

A nightly failure opens an automated pmat work item; the PR that introduced it is blamed via `git bisect` if needed.

## Repair-mode-specific gates

If a PR introduces or modifies repair-pass files, additional checks:

| Check | What it verifies |
|---|---|
| Provenance marker present | First line matches `// xpile-repaired: <hex> via <model> at <utc>` |
| Cache key consistency | Marker hash equals recomputed cache key |
| Model ID is fully-qualified | Not `claude-sonnet` (alias) but `claude-sonnet-4-6` |
| Timestamp is RFC3339 UTC | Ends with `Z`, parses cleanly |
| Cached replay is byte-identical | 10 consecutive `--repair=cached` runs produce identical sha256 |

Implemented in `scripts/check_provenance.sh` (introduced in Phase 2).

## Cost gates

| Gate | Threshold | Action |
|---|---|---|
| Per-PR repair-token cost | ≤ 5M tokens | Hard fail |
| Per-PR repair-wall-clock cost | ≤ 30 min total | Hard fail |
| Per-PR estimated $ cost | ≤ $5 | Comment + soft warning |

Comment on every PR with the breakdown so reviewers see cost impact.

## No-skip rules

- **No `--no-verify` on commits.** `pre-commit` hooks run all PR-level gates locally too.
- **No bypassing `pv lint`.** Even draft PRs must lint clean. There is no `[skip-pv]` directive.
- **No bypassing `pmat tdg`.** A PR that drops grade triggers a human review for the lowering, not an auto-merge.

## Branch protection

The `main` branch is protected:

- Required status checks: all of the above gates
- Required reviewers: 1 (or 2 if the change touches `contracts/` or `docs/specifications/`)
- No direct pushes; PR only
- Linear history (squash or rebase merge)

## Failure-recovery patterns

| Failure | What to do |
|---|---|
| Clippy warning | Fix the lint; don't `#[allow(...)]` without a justification comment |
| Coverage drop | Add tests; don't lower the threshold |
| Mutation kill drop | Strengthen assertions; don't add `#[mutants::skip]` |
| `pv lint` fail | Fix the contract; don't edit `pv` |
| `pmat tdg` regression | Refactor the hot file; don't suppress the metric |
| Kani timeout | Lower the unwind in the contract; document the bound |
