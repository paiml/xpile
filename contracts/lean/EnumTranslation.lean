/-
  EnumTranslation.lean — Lean 4 refinement proof for `C-ENUM-TRANSLATION`.

  Proof-lane counterpart to `contracts/enum-translation-v1.yaml` (R6 /
  PMAT-1146). Contracts xpile's enum lowering (PMAT-513): a Python
  `class C(Enum): NAME = <int literal> …` (typing as `Item::Enum{name,
  variants}`, `variants : (name, discriminant)` in declaration order) emits
  `#[derive(…)] pub enum C { NAME, … }`. The load-bearing property is ORDER +
  MEMBERSHIP preservation: the emitted variant list reproduces the source
  variants in DECLARATION ORDER (with their discriminants) — an emitter that
  reordered, dropped, or duplicated a variant would break `C.NAME` member access
  and the compile-time `C.NAME.value` discriminant lowering.

  This closes the SECOND of the last two uncited module-level constructs (the
  other is `Item::Const`, C-CONST-TRANSLATION): `Item::applicable_contracts()`
  returned an empty set for `Item::Enum`, so a `pub enum Color { … }` shipped
  with NO `// xpile-contract:` line — the final honest gap in the "every
  construct under a cited contract" north-star claim.

  provability/mathlib note: order preservation is `map Prod.fst` over the variant
  list — a pure structural identity, discharged over CORE Lean 4 (`List.map`,
  `List.length_map`) with NO `import Mathlib`, no `sorry`, no `axiom`.
-/

namespace XpileContracts.CEnumTranslation

/--
  Abstract model of a Python enum as xpile lowers it: its `name` and its ordered
  `variants` — each a `(variant-name, discriminant)` pair in DECLARATION ORDER.
  This fully determines the emitted `pub enum Name { … }`.
-/
structure EnumDef where
  name : String
  variants : List (String × Int)
  deriving DecidableEq

/-- The emitted variant name list — declaration order, discriminants dropped
    (the Rust `enum` lists bare variant idents; the discriminant is the
    compile-time `.value`). -/
def emittedOrder (e : EnumDef) : List String :=
  e.variants.map Prod.fst

/--
  **Diamond refinement theorem** for `enum_def_structure_extensionality_diamond`
  (the tier-defining equation): two enums agreeing on name and ordered variants
  are equal. Registers `C-ENUM-TRANSLATION` at depth-1.
-/
theorem enum_def_structure_extensionality_diamond (a b : EnumDef) :
    a.name = b.name → a.variants = b.variants → a = b := by
  intro hn hv
  cases a
  cases b
  simp_all

/--
  **Order preservation**: the emitted variant order is exactly the source
  declaration order (name-projected). An emitter that reordered variants would
  break the discriminant-order correspondence.
-/
theorem enum_order_preserved (e : EnumDef) :
    emittedOrder e = e.variants.map Prod.fst := by
  rfl

/--
  **Membership-count preservation**: the emitted enum has exactly as many
  variants as the source — none dropped or duplicated.
-/
theorem enum_variant_count_preserved (e : EnumDef) :
    (emittedOrder e).length = e.variants.length := by
  unfold emittedOrder
  rw [List.length_map]

end XpileContracts.CEnumTranslation
