import Mathlib

/-!
# OLS uniqueness — simple linear regression (slope + intercept)

The next rung after the constant model: over data `x y : Fin n → ℝ`, a
predictor `t ↦ a·t + b`, and the sum of squared errors
`Q(a,b) = ∑ (yᵢ − a·xᵢ − b)²`.

Stated as the **normal-equations characterisation** — the genuine OLS result and
exactly the condition a fitted model satisfies: any `(a,b)` whose residuals are
orthogonal to `1` and to `x` (`∑ eᵢ = 0`, `∑ eᵢ·xᵢ = 0`) is the UNIQUE minimiser,
provided `x` has positive spread (`∑ (xᵢ − x̄)² > 0`, i.e. the xᵢ are not all
equal — the identifiability condition).
-/

namespace XpileModels

open Finset

/-- Sum of squared errors of the affine predictor `t ↦ a·t + b`. -/
noncomputable def slrSSE {n : ℕ} (x y : Fin n → ℝ) (a b : ℝ) : ℝ :=
  ∑ i, (y i - (a * x i + b)) ^ 2

variable {n : ℕ} (x y : Fin n → ℝ) (a b a' b' : ℝ)

/-- **Decomposition.** If `(a,b)` solves the two normal equations (residuals
orthogonal to `1` and to `x`), the SSE at ANY `(a',b')` exceeds the SSE at
`(a,b)` by exactly the non-negative penalty `∑ ((a'−a)·xᵢ + (b'−b))²`. -/
theorem slr_decomp
    (hres0 : ∑ i, (y i - (a * x i + b)) = 0)
    (hresx : ∑ i, (y i - (a * x i + b)) * x i = 0) :
    slrSSE x y a' b' = slrSSE x y a b + ∑ i, ((a' - a) * x i + (b' - b)) ^ 2 := by
  simp only [slrSSE]
  have key : ∀ i : Fin n,
      (y i - (a' * x i + b')) ^ 2
        = (y i - (a * x i + b)) ^ 2
          - 2 * ((y i - (a * x i + b)) * ((a' - a) * x i + (b' - b)))
          + ((a' - a) * x i + (b' - b)) ^ 2 := by
    intro i; ring
  rw [sum_congr rfl (fun i _ => key i), sum_add_distrib, sum_sub_distrib]
  have hcross :
      ∑ i, 2 * ((y i - (a * x i + b)) * ((a' - a) * x i + (b' - b))) = 0 := by
    have expand : ∀ i : Fin n,
        2 * ((y i - (a * x i + b)) * ((a' - a) * x i + (b' - b)))
          = 2 * (a' - a) * ((y i - (a * x i + b)) * x i)
            + 2 * (b' - b) * (y i - (a * x i + b)) := by
      intro i; ring
    rw [sum_congr rfl (fun i _ => expand i), sum_add_distrib, ← mul_sum, ← mul_sum,
      hres0, hresx]
    ring
  rw [hcross]; ring

/-- The normal-equations point **is** a minimiser: its SSE is `≤` that of any
other `(a',b')`. -/
theorem slr_min
    (hres0 : ∑ i, (y i - (a * x i + b)) = 0)
    (hresx : ∑ i, (y i - (a * x i + b)) * x i = 0) :
    slrSSE x y a b ≤ slrSSE x y a' b' := by
  rw [slr_decomp x y a b a' b' hres0 hresx]
  have : (0 : ℝ) ≤ ∑ i, ((a' - a) * x i + (b' - b)) ^ 2 :=
    sum_nonneg fun i _ => sq_nonneg _
  linarith

/-- The normal-equations point is the **unique** minimiser, given positive spread
in `x`. (Uses `x̄ = (∑ x)/n`.) -/
theorem slr_unique
    (hres0 : ∑ i, (y i - (a * x i + b)) = 0)
    (hresx : ∑ i, (y i - (a * x i + b)) * x i = 0)
    (hSxx : 0 < ∑ i, (x i - (∑ j, x j) / (n : ℝ)) ^ 2)
    (heq : slrSSE x y a' b' = slrSSE x y a b) :
    a' = a ∧ b' = b := by
  -- n > 0, else the spread sum would be empty (= 0), contradicting hSxx.
  have hn : 0 < n := by
    rcases Nat.eq_zero_or_pos n with h | h
    · subst h; simp at hSxx
    · exact h
  have hnR : (n : ℝ) ≠ 0 := by positivity
  -- The penalty vanishes.
  rw [slr_decomp x y a b a' b' hres0 hresx] at heq
  have hpen : ∑ i, ((a' - a) * x i + (b' - b)) ^ 2 = 0 := by linarith
  -- Hence every term vanishes: (a'−a)·xᵢ + (b'−b) = 0 for all i.
  have hterm : ∀ i : Fin n, (a' - a) * x i + (b' - b) = 0 := by
    intro i
    have hz := (sum_eq_zero_iff_of_nonneg (fun j _ => sq_nonneg _)).mp hpen i (mem_univ i)
    exact pow_eq_zero_iff (by norm_num) |>.mp hz
  -- (a'−a)·x̄ = −(b'−b): average the pointwise relation.
  set xbar := (∑ j, x j) / (n : ℝ) with hxbar_def
  have havg : (a' - a) * xbar = -(b' - b) := by
    have hsum : ∑ i, ((a' - a) * x i + (b' - b)) = 0 :=
      sum_eq_zero fun i _ => hterm i
    rw [sum_add_distrib, ← mul_sum, sum_const, card_univ, Fintype.card_fin,
      nsmul_eq_mul] at hsum
    have hxbar : (n : ℝ) * xbar = ∑ i, x i := by
      rw [hxbar_def]; field_simp
    -- (a'−a)·∑x + n·(b'−b) = 0, and ∑x = n·xbar
    rw [← hxbar] at hsum
    have hfactor : (n : ℝ) * ((a' - a) * xbar + (b' - b)) = 0 := by
      linear_combination hsum
    rcases mul_eq_zero.mp hfactor with h | h
    · exact absurd h hnR
    · linarith
  -- So (a'−a)·(xᵢ − x̄) = 0 for all i; sum of squares ⇒ (a'−a)²·Sxx = 0.
  have hdev : ∀ i : Fin n, (a' - a) * (x i - xbar) = 0 := by
    intro i
    have h1 := hterm i
    have : (a' - a) * x i = -(b' - b) := by linarith
    rw [mul_sub, this, havg]; ring
  have hSxxzero : (a' - a) ^ 2 * ∑ i, (x i - xbar) ^ 2 = 0 := by
    rw [mul_sum]
    apply sum_eq_zero
    intro i _
    have hi := hdev i
    have hsq : (a' - a) ^ 2 * (x i - xbar) ^ 2 = ((a' - a) * (x i - xbar)) ^ 2 := by ring
    rw [hsq, hi]; ring
  -- Sxx > 0 forces (a'−a)² = 0, i.e. a' = a; then b' = b from a term.
  have hSxx' : 0 < ∑ i, (x i - xbar) ^ 2 := hSxx
  have ha : a' = a := by
    have : (a' - a) ^ 2 = 0 := by
      rcases mul_eq_zero.mp hSxxzero with h | h
      · exact h
      · exact absurd h (ne_of_gt hSxx')
    have : a' - a = 0 := pow_eq_zero_iff (by norm_num) |>.mp this
    linarith
  refine ⟨ha, ?_⟩
  have hb := hterm ⟨0, hn⟩
  rw [ha] at hb
  simp only [sub_self, zero_mul, zero_add] at hb
  linarith

/-- **Non-vacuity dual.** Any `(a',b')` differing from the normal-equations point
has STRICTLY larger SSE — so the minimiser is genuinely unique, not vacuously so.
-/
theorem slr_strict
    (hres0 : ∑ i, (y i - (a * x i + b)) = 0)
    (hresx : ∑ i, (y i - (a * x i + b)) * x i = 0)
    (hSxx : 0 < ∑ i, (x i - (∑ j, x j) / (n : ℝ)) ^ 2)
    (hne : ¬(a' = a ∧ b' = b)) :
    slrSSE x y a b < slrSSE x y a' b' := by
  rcases lt_or_eq_of_le (slr_min x y a b a' b' hres0 hresx) with h | h
  · exact h
  · exact absurd (slr_unique x y a b a' b' hres0 hresx hSxx h.symm) hne

end XpileModels
