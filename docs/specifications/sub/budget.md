# Budget Discipline

**Section 9 of [xpile-spec.md](../xpile-spec.md).**

## Defaults

| Budget | Default | Override |
|---|---|---|
| Max iterations | 8 | `--repair-max-iterations=N` |
| Max tokens (in+out) | 200,000 | `--repair-max-tokens=N` |
| Max wall-clock (seconds) | 300 | `--repair-max-seconds=N` |

One iteration = one full `(read_diagnostics → write_rust → cargo_build)` cycle.

## Why these defaults

| Default | Reasoning |
|---|---|
| 8 iterations | Most real repair sessions in alchemize complete in 4-9 iterations; 8 covers the median + one σ |
| 200K tokens | Roughly two book chapters of context; well under Claude's window; bounded cost per file |
| 300s wall-clock | Long enough for compile + oracle + ~6 model calls; short enough to abort runaway sessions |

## Fail-closed on exhaustion

Per `repair-budget-v1.yaml` (depyler) — ported to xpile in Phase 1:

```
budget_exhausted => exit_status == ORIGINAL_STATIC_ERROR  AND  no_partial_rust_emitted
```

When any cap exhausts:

1. The in-flight tool call is allowed to complete (no mid-call termination)
2. Any speculative `.rs` written in the current iteration is discarded
3. The original static-pass error is surfaced to the user
4. Exit code matches the static-pass failure (NOT a special "budget exhausted" code, because the user shouldn't have to handle a new error class)

## Telemetry

Every session records:

```json
{
  "session_id": "...",
  "input_path": "foo.py",
  "cache_key": "<hex>",
  "model_id": "claude-sonnet-4-6",
  "iterations_used": 5,
  "tokens_used": 87234,
  "wall_clock_seconds": 92.3,
  "exit": "Match"   // or "BudgetExhausted" or "OracleDivergence"
}
```

These are aggregated by `pv kaizen` into fleet-level repair-mode statistics ([kaizen-fleet.md](kaizen-fleet.md)).

## Aggregate (CI-level) budgets

Per-file caps prevent denial-of-wallet on a single file. CI-level caps prevent a runaway PR:

```yaml
# In xpile's CI workflow
env:
  XPILE_REPAIR_PR_BUDGET_TOKENS: 5_000_000   # 25× single-file cap
  XPILE_REPAIR_PR_BUDGET_SECONDS: 1800       # 30 minutes total
```

A PR that exceeds the aggregate cap is rejected with `AgentError::PrBudgetExceeded`. The author must shrink the PR or split it.

## Cost transparency

Every PR that invokes repair mode posts a comment with:

- Total tokens spent
- Total wall-clock spent
- Cache hit rate (if any inputs were already cached)
- Estimated $ cost at current model pricing

Reviewers can spot repair-mode abuse early.
