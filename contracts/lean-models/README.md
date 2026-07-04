# Model-proof lane (Mathlib)

The **deliberate, walled-off Mathlib lane** for provable-model-as-code
(PMAT-956). This is the *only* place in the repo that depends on Mathlib.

## Why it's separate

The core proof lane [`../lean/`](../lean/) is **hermetic and Mathlib-free** by
policy — every module elaborates under bare core Lean, so its `lake build` runs
in seconds with no cache to fetch. Those are cheap per-construct
structure-extensionality proofs.

This lane proves a different *kind* of thing — the **uniqueness of a fitted
model's optimum** — which needs real analysis / linear algebra that would be a
multi-month formalisation to re-derive from core primitives. So it, and only it,
depends on Mathlib. It is a separate Lake package with its own
`lake-manifest.json` and its own advisory CI job (`lean-models`), so the
multi-GB Mathlib cache never touches the fast core lane. `warningAsError := true`
makes a green build an un-fakeable "no `sorry`" claim.

> **Policy:** Mathlib is permitted *only* here. The core lane stays hermetic.

## What's proven (`Models/Basic.lean`)

The **OLS-minimiser uniqueness certificate for the constant (intercept-only)
model**: over data `x : Fin n → ℝ`, the sample mean is the *unique* constant
predictor minimising `∑ (xᵢ − c)²`.

| theorem | statement |
|---|---|
| `sse_decomp` | completing the square: `sse x c = sse x mean + n·(c − mean)²` |
| `sse_mean_le` | the mean **is** a minimiser (`sse x mean ≤ sse x c`) |
| `sse_eq_mean_iff` | it is the **unique** minimiser (`sse x c = sse x mean ↔ c = mean`, for `n > 0`) |
| `sse_lt_of_ne` | **non-vacuity dual**: `c ≠ mean → sse x mean < sse x c` (strictly, so uniqueness is not vacuous) |

This is the 1-parameter OLS problem; the general normal-equations case builds on
the same completing-the-square identity.

## Build

```bash
cd contracts/lean-models
lake exe cache get   # fetch prebuilt Mathlib oleans (resolves lake-manifest.json)
lake build           # elaborate the certificate
```

## Status

Done: the isolated Mathlib lane + the constant-model uniqueness certificate +
CI. **Next** (not yet wired): a governing contract YAML in `contracts/`, the
general `k`-parameter normal-equations uniqueness (positive-definite Gram
matrix), and the emit path lowering a fitted model → meta-HIR `const + fn`
carrying a `// xpile-contract:` citation of this certificate.
