# Bounded Agent Repair Loop

**Section 7 of [xpile-spec.md](../xpile-spec.md).**

## Inheritance

Adapted from [alchemize](https://github.com/pymc-labs/alchemize)'s four-tool loop. The translation: alchemize compiles probabilistic models with `validate_logp` as its oracle; xpile compiles arbitrary languages with `run_hybrid_oracle` as its oracle. Same loop shape, broader domain.

## Tool surface (v1)

| Tool | Purpose |
|---|---|
| `read_file(path)` | Inspect any workspace file |
| `write_file_in_lang(lang, path, content)` | Overwrite a generated `.rs` |
| `cargo_build()` | Compile; structured diagnostics from `--message-format=json` |
| `cargo_test()` | Run quickcheck + oracle tests |
| `run_hybrid_oracle()` | Capture original-language behavior on fixture; compare |
| `apply_skill(name)` | Pull a markdown skill into agent context |
| `consult_ffi_manifest(symbol)` | Look up a known boundary's Rust shim signature |

**Not exposed in v1:**

- `add_cargo_dependency` — out of scope; manifest is the surface for FFI, deps are pinned
- `git_commit` — agent never commits
- `delete_file` — agent never deletes

## Exit condition

The agent exits successfully if and only if:

```
cargo_build() == ExitStatus(0)
AND
run_hybrid_oracle().compare(fixture_outputs, rust_outputs) == ComparisonResult::Match
```

Either failure mode keeps the loop running (subject to the budget in [budget.md](budget.md)).

## Failure mode: budget exhaustion

If the budget exhausts before both gates pass, the loop:

1. Allows the in-flight tool call to complete (no mid-call termination)
2. Discards any speculative Rust written in this iteration
3. Returns `AgentError::BudgetExhausted` with the original static error

The user sees the *original static failure*, never a partial repair. This is non-negotiable per [`contracts/repair-budget-v1.yaml`](../../../../depyler/contracts/repair-budget-v1.yaml) (ported to xpile in Phase 1).

## Failure mode: oracle divergence at exit

If `cargo_build` passes but oracle diverges, the loop reports the divergent input index and the expected/actual outputs. No silent acceptance of "close enough" outputs.

## Determinism story

The agent itself is stochastic (LLM-driven). The *output artifact* is reproducible via cache. See [cache-determinism-provenance.md](cache-determinism-provenance.md) for the full story.

## Opt-in only

The agent is never invoked unless the user passes `--repair`. The default `xpile transpile foo.py` is pure static. This is invariant `opt_in_required` in `repair-determinism-v1.yaml`.

## What the agent is good at

- Fixing typed import errors (E0433) when the stdlib map is incomplete
- Resolving lifetime/borrow conflicts in generated code
- Bridging missing trait impls
- Repairing semantic divergence on edge cases

## What the agent is bad at

- Designing the right meta-HIR shape (humans do this via contracts)
- Choosing fixture inputs (fixtures come from annotations or static inference)
- Discovering new translation strategies (those become Layer-2 contracts, not skills)

The agent is a *repair* tool, not a *design* tool.
