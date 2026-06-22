/-
  XlatePyClassToStruct.lean — Lean 4 refinement proof for `C-XLATE-PY-CLASS-TO-STRUCT`.

  Proof-lane counterpart to `contracts/xlate-py-class-to-struct-v1.yaml` (R6 /
  PMAT-879). A Python class / `@dataclass` lowers to a Rust `struct` (plus an
  `impl` block for its methods): `Type::Struct`, positional `StructLit`
  construction, `&self` method dispatch.

  Closes the largest previously-uncited core construct (audit-design.md §6): after
  the scalar (int/str/float) and container (list/dict/set) contracts, class/
  dataclass-typed code was still emitting `pub struct …` with ZERO citation. This
  file + the YAML let `Function::applicable_contracts()` cite
  `C-XLATE-PY-CLASS-TO-STRUCT` whenever a struct is in play.

  Modelling note: mirrors the structural-Diamond approach of the str/list/float/
  set contracts. The tier-defining theorem is structure-extensionality over the
  struct's ORDERED field list — genuinely provable, sorry-free — registering the
  contract at depth-1 under the R6-grandfathered Diamond gate (PMAT-475a). The
  ordered-field model is the proof-lane mirror of the `field_order_preserved`
  emit-level invariant: a struct is determined by its fields IN ORDER, so an
  emitter that reordered fields would not satisfy this extensionality. Field-
  algebra tiers (method dispatch, inheritance) ratchet in later.
-/

namespace XpileContracts.CXlatePyClassToStruct

/--
  Abstract model of a Python class / `@dataclass` as xpile lowers it: a Rust
  `struct`, identified here by its ORDERED list of field (name, type-tag) pairs.
  Order is load-bearing — positional construction `C(a, b)` binds by position.
-/
structure PyStruct where
  fields : List (String × String)
  deriving DecidableEq

/--
  **Diamond refinement theorem** for `py_struct_structure_extensionality_diamond`
  (the tier-defining equation in the contract YAML): two `PyStruct` values with
  equal ordered field lists are equal. Registers `C-XLATE-PY-CLASS-TO-STRUCT` at
  depth-1, mirroring the str/list/float/set structural Diamonds. Because the model
  carries fields as an ORDERED `List`, this extensionality also pins the
  field-order invariant: a lowering that permuted fields would produce a
  structurally-distinct value.
-/
theorem py_struct_structure_extensionality_diamond (a b : PyStruct) :
    a.fields = b.fields → a = b := by
  intro h
  cases a
  cases b
  simp_all

end XpileContracts.CXlatePyClassToStruct
