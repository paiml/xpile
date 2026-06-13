/-
  CIntArith.lean — Lean 4 refinement proofs for `C-C-INT-ARITH`.

  Proof-lane counterpart to `contracts/c-int-arith-v1.yaml` (R6 / PMAT-475b).
  The YAML carries the *equation* describing how the decy C frontend +
  xpile-rust-codegen lower C `int` arithmetic; this file carries the
  *theorem* that locks in the modelling commitment.

  The load-bearing commitment: xpile lowers C `int` `+` to Rust
  `i32::wrapping_add`, deliberately REPLACING C's source-level
  signed-overflow undefined behavior with defined two's-complement
  wraparound. We model the C `int` value space as `BitVec 32` and the
  lowered `+` as `BitVec` addition, so the emitted operation is total and
  forms the finite commutative monoid `(Z/2^32, +, 0)`.

  Cross-references:
    * Code lane:   crates/decy-frontend/src/lib.rs (C int lowering),
                   crates/xpile-rust-codegen/src/lib.rs (emit C path —
                   `wrapping_add`/`wrapping_mul`, `i32` width, the
                   `// xpile-contract: C-C-INT-ARITH` citation).
    * Contract:    contracts/c-int-arith-v1.yaml
    * Citation:    every emitted Rust artifact for a C-`int` meta-HIR input
                   carries `// xpile-contract: C-C-INT-ARITH`.
    * Roadmap:     docs/specifications/xpile-spec.md §30 (R6 / PMAT-475),
                   sub/v0.2.0-decy-merger.md (§ The substrate work),
                   audit-design.md §7.3 (the falsification this closes).

  Tier (per ruchy 5.0 §14.10.5): **Diamond** shape (a single tier-defining
  theorem combining commutativity + associativity + identity into the
  commutative-monoid axiomatization), but the contract joins the substrate
  at **depth-1** under the R6-grandfathered Diamond gate (PMAT-475a) — it is
  NOT forced to the depth-13 floor. Deeper tiers (ring/ordered-ring laws,
  `wrapping_mul` monoid, the `/`/`%` truncation laws) ratchet in later
  sub-slices.

  This is the **fourteenth contract Lean theorem file** the project has —
  the first authored after the depth-13 Diamond gate was grandfathered.
-/

namespace XpileContracts.CCIntArith

/--
  Abstract model of a C `int` value as xpile lowers it: a 32-bit
  two's-complement word. xpile emits Rust `i32`; the value space and the
  wraparound arithmetic are exactly those of `BitVec 32`.
-/
abbrev CInt := BitVec 32

/--
  Lowering of C `int` `+` chosen by xpile: `i32::wrapping_add`, modelled as
  `BitVec` addition (defined wraparound — never UB).
-/
def wrappingAdd (a b : CInt) : CInt := a + b

/--
  **Diamond refinement theorem** for `c_int_wrapping_add_commutative_monoid_diamond`
  (the load-bearing claim from the equation block in the contract YAML).

  xpile's chosen lowering of C `int` `+` to `i32::wrapping_add` forms the
  commutative monoid `(Z/2^32, +, 0)`: it is commutative, associative, and
  has `0` as a (left) identity. Because the lowering is total `BitVec 32`
  addition, the emitted Rust is UB-free — unlike C's source-level
  signed-overflow undefined behavior — while preserving the observable
  result in the defined domain.

  The three laws are the `AddCommMonoid` structure that `BitVec 32` carries,
  so the proof is the conjunction of the corresponding `BitVec` lemmas
  (Bronze→Diamond by combination, per ruchy 5.0 §14.10.5).

  Documentary value: any future change to the C `int` `+` lowering (e.g.
  switching to `checked_add` + UB-trap, or to a widened `i64`) must either
  preserve these monoid laws OR invalidate this theorem (and the citation
  gate fires).

  Status: **discharged at v1.0.0 (PMAT-475b)**. Tier: Diamond shape, depth-1.
-/
theorem c_int_wrapping_add_commutative_monoid_diamond (a b c : CInt) :
    (wrappingAdd a b = wrappingAdd b a)
      ∧ (wrappingAdd (wrappingAdd a b) c = wrappingAdd a (wrappingAdd b c))
      ∧ (wrappingAdd 0 a = a) := by
  refine ⟨?_, ?_, ?_⟩
  · exact BitVec.add_comm a b
  · exact BitVec.add_assoc a b c
  · exact BitVec.zero_add a

end XpileContracts.CCIntArith
