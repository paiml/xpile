/-
  OlsModelUniqueness.lean — core-lane Lean 4 refinement proof for
  `C-OLS-MODEL-UNIQUENESS` (PMAT-956, provable-model-as-code).

  Proof-lane counterpart to `contracts/ols-model-uniqueness-v1.yaml`. A fitted
  least-squares linear model is lowered by xpile to a `predict` function over
  CONST coefficients (`predict(x) = ∑ⱼ coeffⱼ · xⱼ`). The emitted predictor is
  fully determined by the ordered coefficient vector, so the tier-defining
  Diamond is STRUCTURE EXTENSIONALITY over that vector: two models agreeing on
  their emitted coefficient list are equal — an emitter that dropped, reordered,
  or altered a coefficient would produce a structurally-distinct model.

  DEPTH split (the deliberate two-lane design, PMAT-956):
    * This CORE module (Mathlib-FREE, `warningAsError`, in the hermetic pilot)
      carries the STRUCTURAL Diamond — same shape as the const/enum/str/list
      structural Diamonds, registering the contract at depth-1.
    * The DEEP semantic content — that the coefficients are the UNIQUE OLS
      minimiser — is machine-checked in the SEPARATE Mathlib lane,
      `contracts/lean-models/Models/GeneralLinear.lean` (`ols_unique`,
      `ols_strict`), and is what makes the const weights meaningful. Mathlib is
      walled off there so this hermetic lane stays cache-free.

  No `import Mathlib`, no `sorry`, no `axiom`.
-/

namespace XpileContracts.COlsModelUniqueness

/--
  Abstract model of a fitted linear model as xpile lowers it: the ordered list
  of emitted coefficient literals (`coeffReprs` — the const values baked into the
  `predict` body, one per feature, in feature order). A linear predictor carries
  no other emit-relevant state — `predict(x) = ∑ⱼ coeffⱼ · xⱼ` is determined by
  this list — so the triple/vector reduces here to a single ordered list.
-/
structure LinearModel where
  coeffReprs : List String
  deriving DecidableEq

/--
  **Diamond refinement theorem** for
  `linear_model_structure_extensionality_diamond` (the tier-defining equation):
  two fitted linear models agreeing on their ordered coefficient list are equal.
  Registers `C-OLS-MODEL-UNIQUENESS` at depth-1. Because the emitted `predict` is
  determined by the coefficient vector, this extensionality pins the emission: a
  lowering that reordered, dropped, or altered a coefficient would yield a
  structurally-distinct `LinearModel`, falsified here.
-/
theorem linear_model_structure_extensionality_diamond (a b : LinearModel) :
    a.coeffReprs = b.coeffReprs → a = b := by
  intro h
  cases a
  cases b
  simp_all

end XpileContracts.COlsModelUniqueness
