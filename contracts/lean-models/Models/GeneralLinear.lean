import Mathlib

/-!
# OLS uniqueness — the general k-parameter linear model

The capstone: `k` feature functions `φ : Fin k → Fin n → ℝ`, target `y`,
predictor `i ↦ ∑ⱼ βⱼ · φⱼ(i)`, and `olsSSE = ∑ᵢ (yᵢ − ∑ⱼ βⱼ φⱼ(i))²`.

Stated as the **normal-equations characterisation**: any `β` whose residual is
orthogonal to every feature (`∑ᵢ residualᵢ · φⱼ(i) = 0` for all `j`) is the
UNIQUE minimiser, provided the features are **linearly independent as vectors in
ℝⁿ** (full column rank — the identifiability / positive-definite-Gram condition,
here stated directly as: the only coefficient vector mapping to the zero
prediction is `0`).

Generalises the constant model (`k = 1`, `φ₀ ≡ 1`) and simple linear regression
(`k = 2`, `φ₀ ≡ 1`, `φ₁ = x`) — mathematically those are instances of this
statement.

⚠️ NOT DERIVED HERE. `Models/Basic.lean` and `Models/SimpleLinear.lean` do not
import this module, and no corollary instantiates `ols_unique` at `k = 1` or
`k = 2`. This file's `lake build` proves the general statement ONLY; the other
two are proved independently. Through v0.1.617 this doc-comment said "Subsumes",
which claims a derivation that does not exist (PMAT-1472).
-/

namespace XpileModels

open Finset

/-- Sum of squared errors of the linear predictor `i ↦ ∑ⱼ βⱼ · φⱼ(i)`. -/
noncomputable def olsSSE {n k : ℕ} (φ : Fin k → Fin n → ℝ) (y : Fin n → ℝ)
    (β : Fin k → ℝ) : ℝ :=
  ∑ i, (y i - ∑ l, β l * φ l i) ^ 2

variable {n k : ℕ} (φ : Fin k → Fin n → ℝ) (y : Fin n → ℝ) (β β' : Fin k → ℝ)

/-- **Decomposition.** If `β` solves the normal equations, the SSE at any `β'`
exceeds the SSE at `β` by exactly the non-negative penalty
`∑ᵢ (∑ⱼ (β'ⱼ − βⱼ)·φⱼ(i))²` — the squared norm of the prediction shift. -/
theorem ols_decomp
    (hortho : ∀ j, ∑ i, (y i - ∑ l, β l * φ l i) * φ j i = 0) :
    olsSSE φ y β' = olsSSE φ y β + ∑ i, (∑ l, (β' l - β l) * φ l i) ^ 2 := by
  simp only [olsSSE]
  have key : ∀ i : Fin n,
      (y i - ∑ l, β' l * φ l i) ^ 2
        = (y i - ∑ l, β l * φ l i) ^ 2
          - 2 * ((y i - ∑ l, β l * φ l i) * (∑ l, (β' l - β l) * φ l i))
          + (∑ l, (β' l - β l) * φ l i) ^ 2 := by
    intro i
    have hsplit : ∑ l, β' l * φ l i
        = (∑ l, β l * φ l i) + ∑ l, (β' l - β l) * φ l i := by
      rw [← sum_add_distrib]; exact sum_congr rfl fun l _ => by ring
    rw [hsplit]; ring
  rw [sum_congr rfl (fun i _ => key i), sum_add_distrib, sum_sub_distrib]
  have hcross :
      ∑ i, 2 * ((y i - ∑ l, β l * φ l i) * (∑ l, (β' l - β l) * φ l i)) = 0 := by
    have e1 : ∀ i : Fin n,
        2 * ((y i - ∑ l, β l * φ l i) * (∑ l, (β' l - β l) * φ l i))
          = ∑ j, 2 * (β' j - β j) * ((y i - ∑ l, β l * φ l i) * φ j i) := by
      intro i
      rw [Finset.mul_sum, Finset.mul_sum]
      exact sum_congr rfl fun j _ => by ring
    rw [sum_congr rfl (fun i _ => e1 i), sum_comm]
    apply sum_eq_zero
    intro j _
    rw [← Finset.mul_sum, hortho j, mul_zero]
  rw [hcross]; ring

/-- The normal-equations point **is** a minimiser. -/
theorem ols_min
    (hortho : ∀ j, ∑ i, (y i - ∑ l, β l * φ l i) * φ j i = 0) :
    olsSSE φ y β ≤ olsSSE φ y β' := by
  rw [ols_decomp φ y β β' hortho]
  have : (0 : ℝ) ≤ ∑ i, (∑ l, (β' l - β l) * φ l i) ^ 2 :=
    sum_nonneg fun i _ => sq_nonneg _
  linarith

/-- The normal-equations point is the **unique** minimiser, given full column
rank (`hindep`: the only coefficients mapping to the zero prediction are `0`). -/
theorem ols_unique
    (hortho : ∀ j, ∑ i, (y i - ∑ l, β l * φ l i) * φ j i = 0)
    (hindep : ∀ γ : Fin k → ℝ, (∀ i, ∑ l, γ l * φ l i = 0) → γ = 0)
    (heq : olsSSE φ y β' = olsSSE φ y β) :
    β' = β := by
  rw [ols_decomp φ y β β' hortho] at heq
  have hpen : ∑ i, (∑ l, (β' l - β l) * φ l i) ^ 2 = 0 := by linarith
  -- Every prediction-shift coordinate vanishes.
  have hzero : ∀ i, ∑ l, (β' l - β l) * φ l i = 0 := by
    intro i
    have := (sum_eq_zero_iff_of_nonneg (fun j _ => sq_nonneg _)).mp hpen i (mem_univ i)
    exact pow_eq_zero_iff (by norm_num) |>.mp this
  -- Full column rank ⇒ the coefficient shift is zero ⇒ β' = β.
  have : (fun l => β' l - β l) = 0 := hindep _ hzero
  funext l
  have hl : β' l - β l = 0 := congrFun this l
  linarith

/-- **Non-vacuity dual.** Any `β' ≠ β` has STRICTLY larger SSE. -/
theorem ols_strict
    (hortho : ∀ j, ∑ i, (y i - ∑ l, β l * φ l i) * φ j i = 0)
    (hindep : ∀ γ : Fin k → ℝ, (∀ i, ∑ l, γ l * φ l i = 0) → γ = 0)
    (hne : β' ≠ β) :
    olsSSE φ y β < olsSSE φ y β' := by
  rcases lt_or_eq_of_le (ols_min φ y β β' hortho) with h | h
  · exact h
  · exact absurd (ols_unique φ y β β' hortho hindep h.symm) hne

end XpileModels
