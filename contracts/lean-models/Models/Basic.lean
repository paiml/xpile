import Mathlib

/-!
# Ordinary-least-squares uniqueness — the constant (intercept-only) model

The seed of the OLS-minimizer-uniqueness certificate for provable-model-as-code:
over data `x : Fin n → ℝ`, the sample **mean** is the UNIQUE constant predictor
`c` minimising the sum of squared errors `∑ (xᵢ − c)²`. This is the 1-parameter
OLS problem; the general normal-equations case builds on the same
completing-the-square identity.

Everything is stated with an explicit `mean` argument constrained by
`(n:ℝ) * mean = ∑ i, x i` so the theorems are toolchain-portable and the emit
path can cite them against a concrete fitted intercept.
-/

namespace XpileModels

open Finset

/-- Sum of squared errors of the constant predictor `c` against data `x`. -/
noncomputable def sse {n : ℕ} (x : Fin n → ℝ) (c : ℝ) : ℝ :=
  ∑ i, (x i - c) ^ 2

/-- **Completing the square.** The SSE about any `c` decomposes into the SSE
about the mean plus a non-negative penalty `n·(c − mean)²`. -/
theorem sse_decomp {n : ℕ} (x : Fin n → ℝ) (c mean : ℝ)
    (hmean : (n : ℝ) * mean = ∑ i, x i) :
    sse x c = sse x mean + (n : ℝ) * (c - mean) ^ 2 := by
  simp only [sse]
  have key : ∀ i : Fin n,
      (x i - c) ^ 2 = (x i - mean) ^ 2 + (mean - c) * (2 * x i - c - mean) := by
    intro i; ring
  rw [sum_congr rfl (fun i _ => key i), sum_add_distrib]
  congr 1
  rw [← mul_sum]
  have hsum : ∑ i : Fin n, (2 * x i - c - mean) = (n : ℝ) * (mean - c) := by
    rw [sum_sub_distrib, sum_sub_distrib, ← mul_sum, sum_const, sum_const,
      card_univ, Fintype.card_fin]
    rw [← hmean]
    ring
  rw [hsum]; ring

/-- The mean **is** a minimiser: its SSE is `≤` that of any constant `c`. -/
theorem sse_mean_le {n : ℕ} (x : Fin n → ℝ) (c mean : ℝ)
    (hmean : (n : ℝ) * mean = ∑ i, x i) :
    sse x mean ≤ sse x c := by
  have h := sse_decomp x c mean hmean
  have hpen : (0 : ℝ) ≤ (n : ℝ) * (c - mean) ^ 2 := by positivity
  linarith

/-- The mean is the **unique** minimiser: for `n > 0`, SSE-optimality forces
`c = mean`. (`←` direction gives the minimiser; `→` gives uniqueness.) -/
theorem sse_eq_mean_iff {n : ℕ} (hn : 0 < n) (x : Fin n → ℝ) (c mean : ℝ)
    (hmean : (n : ℝ) * mean = ∑ i, x i) :
    sse x c = sse x mean ↔ c = mean := by
  have hnR : (0 : ℝ) < n := by exact_mod_cast hn
  rw [sse_decomp x c mean hmean]
  constructor
  · intro h
    have hpen : (n : ℝ) * (c - mean) ^ 2 = 0 := by linarith
    have hsq : (c - mean) ^ 2 = 0 := by
      rcases mul_eq_zero.mp hpen with h0 | h0
      · exact absurd h0 (ne_of_gt hnR)
      · exact h0
    have : c - mean = 0 := by
      exact pow_eq_zero_iff (by norm_num) |>.mp hsq
    linarith
  · intro h; rw [h]; ring

/-- **Non-vacuity dual.** For `n > 0` and `c ≠ mean` the SSE is STRICTLY larger —
so the minimiser is genuinely unique, not vacuously so. -/
theorem sse_lt_of_ne {n : ℕ} (hn : 0 < n) (x : Fin n → ℝ) (c mean : ℝ)
    (hmean : (n : ℝ) * mean = ∑ i, x i) (hne : c ≠ mean) :
    sse x mean < sse x c := by
  have hnR : (0 : ℝ) < n := by exact_mod_cast hn
  have h := sse_decomp x c mean hmean
  have hpos : (0 : ℝ) < (n : ℝ) * (c - mean) ^ 2 := by
    have hcm : c - mean ≠ 0 := sub_ne_zero.mpr hne
    positivity
  linarith

end XpileModels
