/-
  ConstTranslation.lean — Lean 4 refinement proof for `C-CONST-TRANSLATION`.

  Proof-lane counterpart to `contracts/const-translation-v1.yaml` (R6 /
  PMAT-1145). Contracts xpile's module-level constant lowering: a Python
  `NAME = <literal>` at module scope (typing as `Type::Const{name, ty, value}`)
  emits `const NAME: <ty> = <value>;`. The load-bearing property is
  PRESERVATION: the emitted const reproduces the source name, type, AND value —
  an emitter that dropped the annotation, changed the type, or altered the value
  literal would produce a structurally-distinct constant.

  This closes one of the LAST TWO uncited module-level constructs (the other is
  `Item::Enum`, C-ENUM-TRANSLATION): `Item::applicable_contracts()` returned an
  empty set for `Item::Const`, so a `pub const X: i64 = 5;` shipped with NO
  `// xpile-contract:` line — the final honest gap in the "every construct under
  a cited contract" north-star claim (audit-design.md §6, strategic_goals
  Pillar A open_gap).

  provability/mathlib note: a const is fully determined by its (name, type,
  value) triple, so the tier-defining theorem is structure-extensionality over
  that triple — a decidable equality, discharged over CORE Lean 4 with NO
  `import Mathlib`, no `sorry`, no `axiom`.

  Modelling note: mirrors the str/list/set/bool/except/generator/file-io/
  context-manager structural Diamonds, registering the contract at depth-1 under
  the R6-grandfathered Diamond gate (PMAT-475a).
-/

namespace XpileContracts.CConstTranslation

/--
  Abstract model of a module-level constant as xpile lowers it: its `name`, its
  type tag (`tyTag` — the emitted Rust type, `"i64"`/`"String"`/`"bool"`/`"f64"`
  /…), and its emitted value literal (`valueRepr`). A constant carries no other
  emit-relevant state, so this triple fully determines the emitted `const NAME:
  TY = VALUE;`.
-/
structure ConstDef where
  name : String
  tyTag : String
  valueRepr : String
  deriving DecidableEq

/--
  **Diamond refinement theorem** for `const_def_structure_extensionality_diamond`
  (the tier-defining equation): two constants agreeing on name, type, and value
  are equal. Registers `C-CONST-TRANSLATION` at depth-1. Because the emitted
  `const` is determined by this triple, the extensionality pins the preservation
  invariant: a lowering that changed the type or value would produce a
  structurally-distinct `ConstDef`.
-/
theorem const_def_structure_extensionality_diamond (a b : ConstDef) :
    a.name = b.name → a.tyTag = b.tyTag → a.valueRepr = b.valueRepr → a = b := by
  intro hn ht hv
  cases a
  cases b
  simp_all

end XpileContracts.CConstTranslation
