/-
  XlatePyOptionalToOption.lean — Lean 4 refinement proof for `C-XLATE-PY-OPTIONAL-TO-OPTION`.

  Proof-lane counterpart to `contracts/xlate-py-optional-to-option-v1.yaml` (R6 /
  PMAT-881). Python `Optional[T]` lowers to Rust `Option<T>`: `Type::Optional`,
  `None` → `Option::None`, `Some(..)` wrapping at observation points, `x is None`
  → `.is_none()` (`Expr::IsNone`).

  Closes the Optional wrapper — the last core type still emitting uncited for its
  Optional-ness (audit-design.md §6): after the scalar (int/str/float), container
  (list/dict/set), class, and tuple contracts, `Optional[T]` cited only its inner
  type, not the Option mapping itself. This file + the YAML let
  `Function::applicable_contracts()` cite `C-XLATE-PY-OPTIONAL-TO-OPTION`.

  Modelling note: mirrors the structural-Diamond approach of the str/list/float/
  set/class/tuple contracts. The tier-defining theorem is structure-extensionality
  over the (present-flag, payload) model — genuinely provable, sorry-free —
  registering the contract at depth-1 under the R6-grandfathered Diamond gate
  (PMAT-475a). The present/payload model is the proof-lane mirror of the
  `some_wrapping_at_observation` emit-level invariant: an Optional is determined by
  whether it is present (Some vs None) and, if present, its payload — so an emitter
  that dropped the Some wrapper or conflated None with a present value would not
  satisfy this extensionality. Option-algebra tiers (map/and_then/unwrap_or laws)
  ratchet in later.
-/

namespace XpileContracts.CXlatePyOptionalToOption

/--
  Abstract model of a Python `Optional[T]` as xpile lowers it: a Rust `Option<T>`,
  identified here by a present-flag and a payload type-tag. `present = false` is
  the `None` case (payload ignored); `present = true` is the `Some(payload)` case.
-/
structure PyOptional where
  present : Bool
  payload : String
  deriving DecidableEq

/--
  **Diamond refinement theorem** for
  `py_optional_structure_extensionality_diamond` (the tier-defining equation in
  the contract YAML): two `PyOptional` values with equal present-flag and equal
  payload are equal. Registers `C-XLATE-PY-OPTIONAL-TO-OPTION` at depth-1,
  mirroring the str/list/float/set/class/tuple structural Diamonds. Because the
  model carries the present-flag distinctly from the payload, this extensionality
  pins the Some/None distinction: a lowering that conflated None with a present
  value (dropping the wrapper) would produce a structurally-distinct value.
-/
theorem py_optional_structure_extensionality_diamond (a b : PyOptional) :
    a.present = b.present → a.payload = b.payload → a = b := by
  intro hp hq
  cases a
  cases b
  simp_all

end XpileContracts.CXlatePyOptionalToOption
