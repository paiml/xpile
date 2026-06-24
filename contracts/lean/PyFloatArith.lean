/-
  PyFloatArith.lean — Lean 4 refinement proof for `C-PY-FLOAT-ARITH`.

  Proof-lane counterpart to `contracts/py-float-arith-v1.yaml` (R6 / PMAT-475 /
  PMAT-477). Python `float` is an IEEE-754 double; xpile lowers it to Rust `f64`
  (`Type::F64` / `Expr::LitFloat` / `Expr::FloatBinOp`).

  This contract closes the capability-ahead-of-contract gap recorded in
  audit-design.md §6/§7.3: `float` shipped at v0.1.10 emitting `f64`, and after
  PMAT-475 slice 1 str/list/dict each cite their type-translation contract — but
  `float` had NO on-disk contract, so float code emitted UNCITED. This file +
  the YAML let `Function::applicable_contracts()` cite `C-PY-FLOAT-ARITH` for any
  float-typed construct.

  Modelling note: Lean's `Float` is opaque (extern), so its arithmetic carries no
  provable algebraic structure — which is exactly why this contract was deferred.
  Following `XlatePyStrToRustString` (whose Diamonds are STRUCTURAL — `bytes`
  extensionality — not arithmetic), we model a Python float by its IEEE-754 bit
  pattern and prove STRUCTURE EXTENSIONALITY, which is genuinely provable and
  registers the contract at depth-1 under the R6-grandfathered Diamond gate
  (PMAT-475a). Arithmetic-shaped tiers (ordered-field laws, the NaN and signed-zero
  edge semantics) ratchet in later sub-slices.

  Cross-references:
    * Code lane:   crates/xpile-meta-hir/src/lib.rs (Type::F64, applicable_contracts),
                   crates/xpile-rust-codegen/src/lib.rs (f64 emit + the
                   `// xpile-contract: C-PY-FLOAT-ARITH` citation).
    * Contract:    contracts/py-float-arith-v1.yaml
    * Roadmap:     audit-design.md §6 (the drift this closes), §7.3; PMAT-475/477.
-/

namespace XpileContracts.CPyFloatArith

/--
  Abstract model of a Python `float` as xpile lowers it: an IEEE-754 double,
  identified by its 64-bit pattern. xpile emits Rust `f64`; the value is
  determined by its bits.
-/
structure PyFloat where
  bits : UInt64
  deriving DecidableEq

/--
  **Diamond refinement theorem** for `py_float_structure_extensionality_diamond`
  (the tier-defining equation in the contract YAML).

  A Python float is determined by its IEEE-754 bit pattern: two `PyFloat` values
  with equal bits are equal. This is the float analogue of
  `CXlatePyStrToRustString.py_str_structure_extensionality_diamond` (bytes
  extensionality) and registers `C-PY-FLOAT-ARITH` at depth-1.
-/
theorem py_float_structure_extensionality_diamond (a b : PyFloat) :
    a.bits = b.bits → a = b := by
  intro h
  cases a
  cases b
  simp_all

end XpileContracts.CPyFloatArith
