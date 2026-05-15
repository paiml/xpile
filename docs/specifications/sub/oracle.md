# Oracle and Hybrid Validation

**Section 6 of [xpile-spec.md](../xpile-spec.md).**

## Purpose

The oracle is the **semantic gate** the agent must pass to exit successfully. It captures the original source's behavior on an input fixture and compares against the transpiled Rust output.

This is borrowed from alchemize: extract reference values *before* generating, then validate against them. Stronger than property-based tests because the equivalence claim is over the *actual program's behavior*, not random inputs.

## Trait

```rust
pub trait Oracle: Send + Sync {
    fn language(&self) -> &'static str;

    fn capture(&self, source: &Path, fixture: &Fixture) -> Result<CapturedOutputs, OracleError>;

    fn compare(
        &self,
        expected: &CapturedOutputs,
        actual: &CapturedOutputs,
    ) -> ComparisonResult;
}
```

`Fixture` carries inputs (JSON values); `CapturedOutputs` carries outputs. `ComparisonResult` is `Match` or `Divergence { index, expected, actual }`.

## Per-language implementations

| Language | How `capture` runs the original |
|---|---|
| Python | `python3 -c "import json, foo; out=foo.f(*json.loads(in)); print(json.dumps(out))"` |
| C | `gcc -c` + linker stub that calls the exported symbol on each input |
| Ruchy | `ruchy run` with fixture-driven entry point |
| Hybrid | Original Python imports the original .so; both sides run as one process |

## Equality predicates

Type-dependent equality:

| Type | Equality |
|---|---|
| `int` | Bitwise |
| `str` / `bytes` | Exact |
| `float` | Tolerance-based: `abs(a - b) ≤ 1e-9 || rel_err ≤ 1e-6` (configurable) |
| `list` / `tuple` / `vec` | Structural, element-wise |
| `dict` / `map` | Structural, key-by-key |
| `ndarray` | Shape + dtype + element-wise (with float tolerance) |
| `PyObject*` (FFI) | Refcount balance + value equality of the referenced object |

## Fixture sources

A fixture comes from one of two deterministic sources:

1. **Annotation-provided** — author wrote `# xpile: oracle_inputs = [...]` in the source
2. **Frontend-synthesized** — depyler/decy/ruchy generates inputs during static type inference

The agent never chooses fixtures. This prevents the agent from cherry-picking inputs that make the Rust output look correct.

## Hybrid oracle

For a hybrid session, the oracle runs the **original multi-language artifact** as a single process:

```bash
# Pseudo: how the oracle captures hybrid behavior
python3 -c "
import json, sys
from foo_module import f      # imports both .py and _foo_core.so
for line in sys.stdin:
    inputs = json.loads(line)
    print(json.dumps(f(*inputs)))
"
```

Then the transpiled Rust runs the same fixture inputs through `cargo run --bin foo_oracle_harness`. Comparison happens on the captured outputs.

## What the oracle catches

- Silent divergence on edge cases (e.g., `i64::MIN * -1` overflow handling)
- Refcount leaks in FFI translations (the most common CPython bug)
- Float precision regressions (when tolerance is set)
- Iteration order changes (e.g., HashMap-mediated unordered output)
- Buffer-protocol copies (zero-copy contract violation)

## What the oracle does NOT catch

- Performance regressions (use `cargo bench` or roofline contracts via `pv roofline`)
- Memory leaks outside the FFI boundary (use Miri or AddressSanitizer)
- Undefined behavior in unsafe blocks (use Miri)
- Cross-platform behavior (oracle runs on one platform per CI shard)
