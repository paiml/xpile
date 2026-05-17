/-
  XlatePyListToVec.lean — Lean 4 refinement proofs for
  `C-XLATE-PY-LIST-TO-VEC`.

  This file is the proof-lane counterpart to
  `contracts/xlate-py-list-to-vec-v1.yaml` (PMAT-060). The YAML carries
  the *equations* describing how Python `list` lowers to Rust `Vec<T>`;
  this file carries the *theorem* that locks in the modelling
  commitment for the `iteration_order_preserved` equation.

  Cross-references:
    * Code lane:   crates/depyler-frontend/src/lib.rs
                   (currently scaffolded — list lowering arrives at
                   Layer 2 v0.2.0; this contract is the load-bearing
                   semantic anchor for that work).
    * Contract:    contracts/xlate-py-list-to-vec-v1.yaml
    * Citation:    every emitted Rust artifact for a list-shaped
                   meta-HIR input carries
                   `# xpile-contract: C-XLATE-PY-LIST-TO-VEC` above
                   its emitted Vec construction (PMAT-011 idiom).
    * Roadmap:     docs/specifications/xpile-spec.md §3 (translation
                   contracts), audit-design.md §6.

  Tier (per ruchy 5.0 §14.10.5): refinement target is Bronze at
  v0.1.0 — both the Python list and the Rust Vec are modelled as
  `Array UInt8`, and `lower_py_list_to_rust_vec` is the identity at
  the byte-array level. The theorem reduces to `rfl` by our
  modelling choice. Silver tier (v0.3.0+) refines `PyList` and
  `RustVec` to carry typed elements and an alias-graph annotation;
  the iteration-order claim then becomes a structural induction on
  list length.

  This is the *third contract Lean theorem* the project has
  (PMAT-044's Bashrs.lean was the first, PMAT-057's Notation.lean
  the second). Same scaffold posture — documentary modelling
  commitment locked in by `rfl`.
-/

namespace XpileContracts.CXlatePyListToVec

/--
  Abstract model of a Python `list` value as it lands in the
  meta-HIR Layer-1 representation. At v0.1.0 we model the
  contents as a `UInt8` array — enough to capture order and
  length, the two load-bearing properties of the
  `iteration_order_preserved` equation. Silver-tier refinement
  (XPILE-REFINE-XLATE-LIST-***+) replaces this with a typed
  `Array α` plus alias metadata.
-/
structure PyList where
  elems : Array UInt8
deriving DecidableEq

/--
  Abstract model of a Rust `Vec<T>` value as emitted by the
  Rust codegen. Same v0.1.0 shape as `PyList` — refined to carry
  Rust-side ownership semantics at Silver tier.
-/
structure RustVec where
  elems : Array UInt8
deriving DecidableEq

/--
  Lowering function: Python `list` → Rust `Vec`. v0.1.0 model:
  byte-array identity (length and order both trivially preserved
  by our representation choice).
-/
def lower_py_list_to_rust_vec (l : PyList) : RustVec :=
  { elems := l.elems }

/--
  **Refinement theorem** for `iteration_order_preserved` (the
  load-bearing claim from the equation block in the contract YAML).

  Iterating the lowered Rust Vec produces the same element sequence
  as iterating the source Python list. Proof is `rfl` by our
  modelling choice — Bronze tier per ruchy 5.0 §14.10.5.

  Documentary value: any future change to `lower_py_list_to_rust_vec`
  (e.g., adding reverse-order optimisation, or introducing a
  `SmallVec` fast path) must either preserve `rfl`-equivalence OR
  invalidate this theorem (and `refinement_proofs.rs`'s citation
  gate fires).

  Falsification: if a future PR ships a depyler-frontend whose
  list lowering reorders elements, *either* this theorem must be
  invalidated *or* the two paths stay artificially aligned and a
  runtime witness catches the divergence. Same Semantic + Runtime
  stratum cross-reinforcement as PMAT-044's bashrs theorem.

  Status: **discharged at v0.1.0 (PMAT-060)**. Tier: Bronze.
-/
theorem iteration_order_preserved (l : PyList) :
    (lower_py_list_to_rust_vec l).elems = l.elems := by
  rfl

/--
  **Length preservation** (auxiliary refinement claim, also from
  the equation block). Trivially `rfl` at v0.1.0 because we use
  the same underlying `Array UInt8` for both sides. Listed
  separately so the Silver-tier refinement (where `PyList` and
  `RustVec` get distinct element types) has a separate proof
  obligation rather than bundling order + length.
-/
theorem length_preserved (l : PyList) :
    (lower_py_list_to_rust_vec l).elems.size = l.elems.size := by
  rfl

end XpileContracts.CXlatePyListToVec
