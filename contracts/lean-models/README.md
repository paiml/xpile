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

This is the 1-parameter OLS problem.

### Simple linear regression (`Models/SimpleLinear.lean`)

The next rung: slope + intercept, `t ↦ a·t + b`, `slrSSE = ∑ (yᵢ − a·xᵢ − b)²`,
stated as the **normal-equations characterisation** — exactly the condition a
fitted model satisfies (residuals orthogonal to `1` and to `x`). Unique given
**positive spread** in `x` (`∑ (xᵢ − x̄)² > 0`, the identifiability condition).

| theorem | statement |
|---|---|
| `slr_decomp` | `slrSSE a' b' = slrSSE a b + ∑ ((a'−a)·xᵢ + (b'−b))²` when `(a,b)` solves the normal equations |
| `slr_min` | the normal-equations point **is** a minimiser |
| `slr_unique` | it is the **unique** minimiser (positive spread) |
| `slr_strict` | **non-vacuity dual**: strictly larger off the optimum |

### General k-parameter linear model (`Models/GeneralLinear.lean`)

The capstone. `k` feature functions `φ : Fin k → Fin n → ℝ`, predictor
`i ↦ ∑ⱼ βⱼ·φⱼ(i)`, `olsSSE = ∑ᵢ (yᵢ − ∑ⱼ βⱼ φⱼ(i))²`. Normal-equations
characterisation (residual ⊥ every feature), unique given **full column rank** —
stated directly as "the only coefficients mapping to the zero prediction are
`0`", avoiding matrix machinery.

| theorem | statement |
|---|---|
| `ols_decomp` | `olsSSE β' = olsSSE β + ∑ᵢ (∑ⱼ (β'ⱼ−βⱼ)·φⱼ(i))²` when `β` solves the normal equations |
| `ols_min` | the normal-equations point **is** a minimiser |
| `ols_unique` | it is the **unique** minimiser (full column rank) |
| `ols_strict` | **non-vacuity dual**: strictly larger off the optimum |

**Subsumes** the constant model (`k = 1`, `φ₀ ≡ 1`) and simple linear regression
(`k = 2`, `φ₀ ≡ 1`, `φ₁ = x`) as special cases of one general theorem.

## Build

```bash
cd contracts/lean-models
lake exe cache get   # fetch prebuilt Mathlib oleans (resolves lake-manifest.json)
lake build           # elaborate the certificate
```

## Status

Done: the isolated Mathlib lane + CI + OLS-uniqueness certificates for the
**constant** model (`Basic`), **simple linear regression** (`SimpleLinear`), and
the **general k-parameter** linear model (`GeneralLinear`, which subsumes the
first two).

**The emit path is wired** (this README previously said "not yet wired" — stale):
- the governing contract `contracts/ols-model-uniqueness-v1.yaml`
  (`C-OLS-MODEL-UNIQUENESS`), plus a core-lane companion
  `contracts/lean/OlsModelUniqueness.lean` and the kani harness
  `contracts/kani/ols_model_uniqueness.rs`;
- `Function::is_ols_linear_model()` structurally recognises the fitted-regression
  shape (≥2 distinctly-weighted float features + a literal bias, pure-expression
  body) and `applicable_contracts()` emits the `C-OLS-MODEL-UNIQUENESS` citation
  next to the predictor on every backend that carries citations
  (`examples/proven-model/model.py` demonstrates it end-to-end).

Honesty: recognition is STRUCTURAL — it asserts the function is a linear-model
predictor of the class the certificate governs; it does NOT verify the weights
are a least-squares fit (that precondition is the modeller's assertion). Both
directions are pinned by witnesses: the positive citation by
`crates/xpile/tests/contract_citation_integrity.rs` (fixture `ols_model.py`), and
the recognition DISCRIMINATION — that near-misses (no bias, one feature, bare
features, a product of parameters, an integer model) do NOT get stamped — by
`crates/xpile/tests/ols_recognition.rs`.
