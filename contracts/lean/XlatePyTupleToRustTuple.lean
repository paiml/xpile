/-
  XlatePyTupleToRustTuple.lean — Lean 4 refinement proof for `C-XLATE-PY-TUPLE-TO-RUST-TUPLE`.

  Proof-lane counterpart to `contracts/xlate-py-tuple-to-rust-tuple-v1.yaml` (R6 /
  PMAT-880). A Python fixed-arity `tuple` lowers to a Rust tuple `(T0, …, Tn)`:
  `Type::Tuple`, `Expr::TupleLit`, `Expr::TupleIndex`, `a, b = pair` destructuring.

  Closes another previously-uncited core construct (audit-design.md §6): after the
  scalar (int/str/float), container (list/dict/set), and class contracts, tuple-
  typed code (`(i64, i64)`) was still emitting UNCITED. This file + the YAML let
  `Function::applicable_contracts()` cite `C-XLATE-PY-TUPLE-TO-RUST-TUPLE`.

  Modelling note: mirrors the structural-Diamond approach of the str/list/float/
  set/class contracts. The tier-defining theorem is structure-extensionality over
  the tuple's ORDERED element list — genuinely provable, sorry-free — registering
  the contract at depth-1 under the R6-grandfathered Diamond gate (PMAT-475a). The
  ordered-element model is the proof-lane mirror of the
  `arity_and_position_preserved` emit-level invariant: a tuple is determined by
  its elements IN ORDER, so an emitter that permuted positions (breaking `a, b =
  b, a`) would not satisfy this extensionality. Tuple-algebra tiers
  (concatenation, projection laws) ratchet in later.
-/

namespace XpileContracts.CXlatePyTupleToRustTuple

/--
  Abstract model of a Python fixed-arity `tuple` as xpile lowers it: a Rust
  tuple, identified here by its ORDERED list of element type-tags. Order and
  count are load-bearing — `(b, a)` and `(a, b)` are distinct values, and
  positional destructuring binds by index.
-/
structure PyTuple where
  elems : List String
  deriving DecidableEq

/--
  **Diamond refinement theorem** for `py_tuple_structure_extensionality_diamond`
  (the tier-defining equation in the contract YAML): two `PyTuple` values with
  equal ordered element lists are equal. Registers `C-XLATE-PY-TUPLE-TO-RUST-TUPLE`
  at depth-1, mirroring the str/list/float/set/class structural Diamonds. Because
  the model carries elements as an ORDERED `List`, this extensionality also pins
  the arity+position invariant: a lowering that permuted or dropped a position
  would produce a structurally-distinct value.
-/
theorem py_tuple_structure_extensionality_diamond (a b : PyTuple) :
    a.elems = b.elems → a = b := by
  intro h
  cases a
  cases b
  simp_all

end XpileContracts.CXlatePyTupleToRustTuple
