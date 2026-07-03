/-
  PyExceptAllowlist.lean — Lean 4 refinement proof for `C-PY-EXCEPT-ALLOWLIST`.

  Proof-lane counterpart to `contracts/py-except-allowlist-v1.yaml` (R6 /
  PMAT-1120). Contracts xpile's Python exception-dispatch model (the try/except
  lane shipped this session — statement-form try/except, multiple `except`
  clauses, `try/except/finally`, finally-only try). Python exceptions are
  modelled as Rust panics carrying an `xpile: <Type>: <msg>` payload; a handler
  catches by matching the payload's TYPE name against its `except_types`
  ALLOWLIST. The load-bearing correctness property is the PMAT-789 allowlist
  re-raise: a payload whose type is NOT in a non-empty allowlist is re-raised
  (`resume_unwind`), so `except ValueError` does NOT swallow a
  `ZeroDivisionError`.

  Mathlib note (the provability/mathlib decision): every theorem here is
  discharged over CORE Lean 4 primitives (`List.isEmpty`/`List.contains`,
  `Bool.or_false`, structural `cases`) — NO `import Mathlib`, no `sorry`, no
  `axiom`. This mirrors the whole 28-module pilot: the allowlist semantics is a
  decidable Boolean property, so Mathlib's real-analysis / linear-algebra buys
  nothing. The purity invariant (hermetic, fast, warningAsError) holds.

  Modelling note: mirrors the structural-Diamond approach of the
  str/list/dict/set/tuple/Optional/bool contracts — the tier-defining theorem is
  structure-extensionality over the handler's allowlist, registering the
  contract at depth-1 under the R6-grandfathered Diamond gate (PMAT-475a). The
  three semantic theorems below (catch-all, matched-catch, unmatched-propagate)
  pin the actual dispatch invariant the emit-level `resume_unwind` guard relies
  on.
-/

namespace XpileContracts.CPyExceptAllowlist

/--
  The exception-dispatch model: an `except` handler carries a list of
  exception-type NAMES — its allowlist. An EMPTY list is a bare `except:`
  (catch-all); a non-empty list is `except (A, B, …):`. A handler carries no
  other dispatch state (the bound `as e` name and body are emit concerns, not
  dispatch), so the allowlist fully determines WHICH exceptions it catches.
-/
structure ExceptHandler where
  types : List String
  deriving DecidableEq

/--
  Does a handler CATCH an exception whose type name is `exc`? A bare `except:`
  (empty allowlist) catches everything; otherwise it catches iff `exc` is a
  member of the allowlist. This is the proof-lane mirror of the emitted guard
  `__xpile_m.starts_with("xpile: <k>: ")` disjunction over `except_types`, with
  the `else resume_unwind` re-raise being exactly `catches = false`.
-/
def catches (h : ExceptHandler) (exc : String) : Bool :=
  h.types.isEmpty || h.types.contains exc

/--
  **Diamond refinement theorem** for
  `except_handler_structure_extensionality_diamond` (the tier-defining equation
  in the contract YAML): two handlers with equal allowlists are equal.
  Registers `C-PY-EXCEPT-ALLOWLIST` at depth-1, mirroring the
  str/list/dict/set/tuple/Optional/bool structural Diamonds. Because a handler's
  dispatch is fully determined by its allowlist, this extensionality pins the
  dispatch invariant: an emitter that dropped or reordered a type from the
  allowlist would produce a structurally-distinct handler.
-/
theorem except_handler_structure_extensionality_diamond (a b : ExceptHandler) :
    a.types = b.types → a = b := by
  intro h
  cases a
  cases b
  simp_all

/--
  **Catch-all semantics**: a bare `except:` (empty allowlist) catches every
  exception type. This is the Python-required last catch-all clause.
-/
theorem empty_allowlist_catches_all (exc : String) :
    catches ⟨[]⟩ exc = true := by
  simp [catches]

/--
  **Matched-catch**: a type present in the allowlist is caught.
-/
theorem matched_type_is_caught (h : ExceptHandler) (exc : String) :
    h.types.contains exc = true → catches h exc = true := by
  intro hc
  unfold catches
  rw [hc, Bool.or_true]

/--
  **No-swallow invariant** (the PMAT-789 correctness property, the reason this
  contract exists): a type that is NOT in a NON-EMPTY allowlist is NOT caught —
  it propagates (`resume_unwind`). So `except ValueError:` does not swallow a
  `ZeroDivisionError`, and a multi-`except` chain whose clauses all miss re-raises
  rather than silently catching.
-/
theorem unmatched_type_propagates (h : ExceptHandler) (exc : String) :
    h.types ≠ [] → h.types.contains exc = false → catches h exc = false := by
  intro hne hnc
  unfold catches
  rw [hnc, Bool.or_false]
  cases hlist : h.types with
  | nil => exact absurd hlist hne
  | cons _ _ => rfl

end XpileContracts.CPyExceptAllowlist
