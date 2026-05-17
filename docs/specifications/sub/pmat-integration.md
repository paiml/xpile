# Quality Regime (`pmat`)

**Section 12 of [xpile-spec.md](../xpile-spec.md).**

## pmat is the work controller

[`pmat`](https://github.com/paiml/paiml-mcp-agent-toolkit) (Pragmatic Multi-language Agent Toolkit) owns:

- **Work items.** Every phase of the xpile rollout is a pmat work item pointed at a spec in `docs/specifications/`.
- **Quality grades.** `pmat tdg` produces an A+ → F technical-debt grade. xpile target: **A-** (not yet a required CI gate — XPILE-CI-PMAT-TDG-001).
- **Code search.** `pmat query` is the canonical search tool — semantic, quality-annotated, ranked.
- **Context generation.** `pmat context` produces AI-ready briefs for any task.
- **The kaizen loop.** The `kaizen-paiml` skill walks open work items and drives them to completion.

## Work-item-driven development

```
1. Open work item:    pmat work create --spec docs/specifications/sub/foo.md
2. Pick item up:      pmat work claim <id>
3. Implement until:   pv lint 8/8  AND  cargo test --workspace green
                      AND  cargo fmt --check  AND  cargo clippy -D warnings
                      AND  cargo deny check advisories
                      (pmat tdg ≥ A- is the v0.2.0+ target — XPILE-CI-PMAT-TDG-001)
4. Close work item:   pmat work complete <id>
5. Next item:         pmat work list --open --sorted-by priority
```

This is what the `kaizen-paiml` skill automates.

## Quality gates (mandatory in CI at v0.1.0)

Live `.github/workflows/ci.yml` enforces these — see [ci-gates.md](ci-gates.md) for full per-gate detail:

| Gate | Tool | Threshold |
|---|---|---|
| Format | `cargo fmt --all -- --check` | clean |
| Type check | `cargo check --workspace` | clean |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | zero warnings |
| Provable-contracts | `pv lint contracts/` | 8/8 gates pass, 0 errors (12 contracts at v0.1.0) |
| Security advisories | `cargo deny check advisories` | zero unyanked |
| Tests | `cargo test --workspace` | all pass — including the §14.4 stratum gates `refinement_proofs`, `kani_harnesses`, `kani_verify`, `quorum`, `attestations` |
| Kani BMC (optional `kani` job) | `cargo kani` over `contracts/kani/*.rs` | 12 harnesses must verify successfully (~3.7s total) |

PRs that fail any required gate are rejected. No `--no-verify`, no manual overrides.

### Gates planned but not yet wired (post-v0.1.0)

These were in the original v0.0.1 plan but were sequenced behind the substrate-completion work that just shipped (PMAT-058..077). Each is a candidate post-v0.1.0 ticket:

| Planned gate | Tool | Threshold | Tracking ticket |
|---|---|---|---|
| Technical-debt grade | `pmat tdg` | ≥ A- | XPILE-CI-PMAT-TDG-001 |
| Line coverage | `cargo llvm-cov` (NOT tarpaulin) | ≥ 95% | XPILE-CI-COVERAGE-001 |
| Mutation coverage | `cargo mutants` | ≥ 80% on changed code | XPILE-CI-MUTANTS-001 |
| Contract score | `pv score` | no regression vs main | XPILE-CI-SCORE-001 |
| Provenance check | `scripts/check_provenance.sh` | no orphan markers | XPILE-CI-PROVENANCE-001 |

These weren't dropped — they're sequenced post-substrate-completion. With 12 contracts at QUORUM the substrate-side prerequisite is met; the v0.2.0+ work is wiring them into CI without slowing fast-feedback gates.

## Why `pmat tdg` over individual metrics

PMAT TDG combines six orthogonal metrics into a single grade:

1. Cyclomatic complexity
2. Cognitive complexity
3. Function length
4. Coupling (fan-in/fan-out)
5. Coverage
6. Mutation kill rate

A single number is faster to react to than six. A repo that's A- on overall TDG might be C on coverage; the report explains where to focus.

## Code search via pmat query

xpile follows the family rule (from the user's CLAUDE.md): **never use grep/glob for code search; always prefer `pmat query`.**

Examples:

```bash
pmat query "error handling" --limit 10              # semantic
pmat query "serialize" --min-grade A                # high-quality only
pmat query "unwrap" --faults --exclude-tests        # fault patterns
pmat query "tokenize" --include-source              # with source code
pmat query --regex "fn\s+handle_\w+" --limit 10     # regex
pmat query --literal "unwrap()" -A 3 -B 1           # like rg -F + context
pmat query --coverage-gaps --limit 30 --exclude-tests   # coverage gaps by impact
```

## Kaizen loop in practice

The `kaizen-paiml` skill drives xpile forward without per-step human direction:

1. Reads `docs/status/CURRENT.md` to know where the project is
2. Picks the highest-priority open pmat work item
3. Reads the linked spec
4. Implements until all gates pass
5. Updates `docs/status/CURRENT.md`
6. Repeats

A future Claude session that runs `kaizen-paiml` should be able to start from `xpile/docs/status/CURRENT.md` and make forward progress autonomously.

## Status synchronization

`docs/status/CURRENT.md` is the **single source of truth** for "where is xpile right now." It must reflect:

- Which phase we're in
- Which work items are done / in-progress / blocked
- What the next session should pick up
- Any blockers requiring human decision

See [docs/status/CURRENT.md](../../status/CURRENT.md) for the live version.
