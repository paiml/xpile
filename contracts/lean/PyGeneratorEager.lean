/-
  PyGeneratorEager.lean — Lean 4 refinement proof for `C-PY-GENERATOR-EAGER`.

  Proof-lane counterpart to `contracts/py-generator-eager-v1.yaml` (R6 /
  PMAT-1122). Contracts xpile's EAGER generator lowering (PMAT-1071): a
  `def g() -> T: … yield e …` function is rewritten into a list-BUILDING one —
  `__gen_result: list[T] = []`, each `yield e` → `__gen_result.append(e)`, a
  trailing `return __gen_result` — so `for x in g()` / `list(g())` / `sum(g())`
  ride the existing list machinery. The load-bearing correctness property is
  FAITHFULNESS: the materialized list is EXACTLY the ordered sequence of yielded
  values (same values, same order, same length) — an emitter that dropped,
  duplicated, or reordered a yield would produce a different list.

  provability/mathlib note: faithfulness is `foldl (fun acc v => acc ++ [v]) []
  yields = yields` — a standard structural induction over `List`. Discharged
  over CORE Lean 4 (`List.foldl`, `List.append_assoc`, `List.nil_append`,
  structural `induction`) — NO `import Mathlib`, no `sorry`, no `axiom`. This is
  genuine inductive content (not `rfl`), and it needs nothing from Mathlib's
  real-analysis / linear-algebra: the eager materialization is a pure list fold.
  The purity invariant holds.

  Modelling note: the structure-extensionality Diamond registers the contract at
  depth-1 (mirroring the str/list/set/bool/except structural Diamonds); the
  three fold theorems below (`materialize_eq_yields`, `materialize_length`,
  `materialize_append_step`) pin the actual append-per-yield lowering the emitter
  produces.
-/

namespace XpileContracts.CPyGeneratorEager

/--
  Model of one generator RUN as xpile eagerly materializes it: the ordered list
  of yielded values (here `Int` as a concrete carrier — the argument is
  element-generic). A run carries no other state that survives materialization
  (locals die; the `return __gen_result` is exactly this list), so the yield
  sequence fully determines the emitted list.
-/
structure GeneratorRun where
  yields : List Int
  deriving DecidableEq

/--
  The emitted eager lowering, abstractly: start from `[]` and append each
  yielded value in order (the proof-lane mirror of `__gen_result = []`; per
  `yield v`: `__gen_result.append(v)`). A left fold that snoc's each element.
-/
def materialize (ys : List Int) : List Int :=
  ys.foldl (fun acc v => acc ++ [v]) []

/--
  **Diamond refinement theorem** for
  `generator_run_structure_extensionality_diamond` (the tier-defining equation):
  two runs with equal yield sequences are equal. Registers
  `C-PY-GENERATOR-EAGER` at depth-1.
-/
theorem generator_run_structure_extensionality_diamond (a b : GeneratorRun) :
    a.yields = b.yields → a = b := by
  intro h
  cases a
  cases b
  simp_all

/--
  Helper: folding-with-snoc from a non-empty prefix threads the prefix through —
  `foldl (· ++ [·]) pre ys = pre ++ ys`. The general form the faithfulness
  theorem specializes at `pre = []`. Proved by structural induction on `ys`,
  generalizing the prefix, over core `List` primitives only.
-/
theorem foldl_snoc_prefix (ys : List Int) :
    ∀ pre : List Int, ys.foldl (fun acc v => acc ++ [v]) pre = pre ++ ys := by
  induction ys with
  | nil => intro pre; simp
  | cons y ys ih =>
    intro pre
    simp only [List.foldl_cons]
    rw [ih (pre ++ [y])]
    simp [List.append_assoc]

/--
  **Faithfulness** (the reason this contract exists): the eager materialization
  reproduces EXACTLY the yielded sequence — no value dropped, duplicated, or
  reordered. This is the proof-lane guarantee that the emitted
  `__gen_result.append(e)` per yield builds precisely `list(g())`.
-/
theorem materialize_eq_yields (ys : List Int) : materialize ys = ys := by
  unfold materialize
  have h := foldl_snoc_prefix ys []
  simpa using h

/--
  **Length faithfulness**: `len(list(g()))` equals the number of yields — a
  consumer like `sum(g())` / `len(list(g()))` sees the right count.
-/
theorem materialize_length (ys : List Int) : (materialize ys).length = ys.length := by
  rw [materialize_eq_yields]

/--
  **Append-per-yield step**: materializing one more yield `v` appends `v` to the
  end (order-preserving), exactly the emitted `__gen_result.append(v)`.
-/
theorem materialize_append_step (ys : List Int) (v : Int) :
    materialize (ys ++ [v]) = materialize ys ++ [v] := by
  rw [materialize_eq_yields, materialize_eq_yields]

end XpileContracts.CPyGeneratorEager
