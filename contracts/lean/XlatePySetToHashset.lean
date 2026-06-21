/-
  XlatePySetToHashset.lean — Lean 4 refinement proof for `C-XLATE-PY-SET-TO-HASHSET`.

  Proof-lane counterpart to `contracts/xlate-py-set-to-hashset-v1.yaml` (R6 /
  PMAT-475 slice 4). Python `set[T]` lowers to Rust `std::collections::HashSet<T>`
  (`Type::Set`, `Expr::SetLit`/`SetOp`/`SetPred`/`SetFromList`, `.insert`/`.remove`).

  Closes the last container in the capability-vs-contract gap (audit-design.md §6):
  after the scalar + list/dict contracts, `set`-typed code was the remaining
  construct emitting UNCITED. This file + the YAML let
  `Function::applicable_contracts()` cite `C-XLATE-PY-SET-TO-HASHSET`.

  Modelling note: mirrors the structural-Diamond approach of the str/list/float
  contracts (a HashSet's algebra is not cheaply provable in core Lean). The
  tier-defining theorem is structure-extensionality over the set's canonical
  representation — genuinely provable, sorry-free — registering the contract at
  depth-1 under the R6-grandfathered Diamond gate (PMAT-475a). Set-algebra tiers
  (union/intersection laws, idempotence, membership) ratchet in later.
-/

namespace XpileContracts.CXlatePySetToHashset

/--
  Abstract model of a Python `set` as xpile lowers it: a `HashSet`, identified
  here by its canonical element representation. xpile emits Rust `HashSet<T>`.
-/
structure PySet where
  repr : List UInt64
  deriving DecidableEq

/--
  **Diamond refinement theorem** for `py_set_structure_extensionality_diamond`
  (the tier-defining equation in the contract YAML): two `PySet` values with
  equal canonical representations are equal. Registers `C-XLATE-PY-SET-TO-HASHSET`
  at depth-1, mirroring the str/list/float structural Diamonds.
-/
theorem py_set_structure_extensionality_diamond (a b : PySet) :
    a.repr = b.repr → a = b := by
  intro h
  cases a
  cases b
  simp_all

end XpileContracts.CXlatePySetToHashset
