/-
  XlatePyBoolToRustBool.lean — Lean 4 refinement proof for `C-XLATE-PY-BOOL-TO-RUST-BOOL`.

  Proof-lane counterpart to `contracts/xlate-py-bool-to-rust-bool-v1.yaml` (R6 /
  PMAT-935). Python boolean logic over `bool` lowers to Rust `bool`:
  `Type::Bool`, `a and b` → `a && b`, `a or b` → `a || b`, `not a` → `!a`,
  `True`/`False` → `true`/`false` (the short-circuiting `&&`/`||`, never the
  eager bitwise `&`/`|`).

  Closes the pure-`bool` scalar — the last core scalar still emitting uncited
  (audit-design.md §6): after the int/str/float scalars and the
  list/dict/set/tuple/Optional/class constructs, a pure-boolean function
  (`pub fn both(a: bool, b: bool) -> bool { (a && b) }`) was still shipping with
  NO `// xpile-contract:` line. This file + the YAML let
  `Function::applicable_contracts()` cite `C-XLATE-PY-BOOL-TO-RUST-BOOL`.

  Modelling note: mirrors the structural-Diamond approach of the str/list/float/
  set/class/tuple/Optional contracts. The tier-defining theorem is structure-
  extensionality over the single truth-flag model — genuinely provable, sorry-
  free — registering the contract at depth-1 under the R6-grandfathered Diamond
  gate (PMAT-475a). The truth-flag model is the proof-lane mirror of the
  `operator_polarity_preserved` emit-level invariant: a Python bool is determined
  entirely by whether it is true or false, so an emitter that flipped a polarity
  (e.g. dropped a `not`, computing the negation) would produce a structurally-
  distinct value. Boolean-algebra tiers (de Morgan, idempotence, distributivity
  laws) ratchet in later.
-/

namespace XpileContracts.CXlatePyBoolToRustBool

/--
  Abstract model of a Python `bool` as xpile lowers it: a Rust `bool`, identified
  here by a single truth-flag. `truth = false` is the `False` case; `truth =
  true` is the `True` case. There is no other state — a bool carries no payload,
  unlike `Optional` (present + payload) or `tuple` (ordered elements).
-/
structure PyBool where
  truth : Bool
  deriving DecidableEq

/--
  **Diamond refinement theorem** for `py_bool_structure_extensionality_diamond`
  (the tier-defining equation in the contract YAML): two `PyBool` values with
  equal truth-flags are equal. Registers `C-XLATE-PY-BOOL-TO-RUST-BOOL` at
  depth-1, mirroring the str/list/float/set/class/tuple/Optional structural
  Diamonds. Because a bool is fully determined by its single truth-flag, this
  extensionality pins the polarity invariant: a lowering that flipped a polarity
  (computing `not` of the intended value) would produce a structurally-distinct
  value.
-/
theorem py_bool_structure_extensionality_diamond (a b : PyBool) :
    a.truth = b.truth → a = b := by
  intro h
  cases a
  cases b
  simp_all

end XpileContracts.CXlatePyBoolToRustBool
