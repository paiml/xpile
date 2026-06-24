/-
  CFloatArith.lean — Lean 4 refinement proofs for `C-C-FLOAT-ARITH`.

  Proof-lane counterpart to `contracts/c-c-float-arith-v1.yaml` (R6 / PMAT-912).
  The decy C frontend now lowers BOTH C float widths ABI-honestly (PMAT-910/911):
  C `double` → meta-HIR `Type::F64` → Rust `f64` → `c_double` (64-bit), and C
  `float` → meta-HIR `Type::F32` → Rust `f32` → `c_float` (32-bit), kept DISTINCT
  exactly as `I64`/`CLong` are for the integer widths. The xpile-rust-codegen C
  path emits IEEE-754 infix arithmetic (`+ - * / %`, plain — never the
  two's-complement `wrapping_*` that `C-C-INT-ARITH` models) at the matching
  width.

  This file closes the citation-honesty gap PMAT-910/911 deliberately left open:
  those slices emitted a C float/double function with an UNCITED placeholder note
  (`// xpile-arith: … (C-C-FLOAT-ARITH queued, uncited)`) precisely because there
  was NO on-disk contract to cite without minting a phantom id for the PMAT-475
  citation gate. With this YAML + Lean file authored, `emit_c_function` now emits
  a real `// xpile-contract: C-C-FLOAT-ARITH` line that resolves on disk.

  Modelling note: Lean's `Float` is opaque (extern), so its arithmetic carries no
  provable algebraic structure — the same reason `C-PY-FLOAT-ARITH` is structural,
  not arithmetic. Following `PyFloatArith` (and `XlatePyStrToRustString` before
  it), we model a C float by its IEEE-754 bit pattern and prove STRUCTURE
  EXTENSIONALITY, which is genuinely provable, sorry-free, and registers the
  contract at depth-1 under the R6-grandfathered Diamond gate (PMAT-475a). Unlike
  the Python-float sibling, this contract carries TWO bit-width models — the
  32-bit `c_float` and the 64-bit `c_double` — and a documentary lemma that the
  two ABI slots are distinct sizes (so `float` is NEVER widened through
  `c_double`, the 32-vs-64 ABI lie PMAT-909 fixed for the integer lanes).
  Arithmetic-shaped tiers (ordered-field laws, NaN / signed-zero edge semantics)
  ratchet in later sub-slices.

  Cross-references:
    * Code lane:   crates/decy-frontend/src/lib.rs (C `float`/`double` → F32/F64),
                   crates/xpile-ffi-manifest/src/lib.rs (F32→c_float, F64→c_double),
                   crates/xpile-rust-codegen/src/lib.rs (IEEE f32/f64 emit + the
                   `// xpile-contract: C-C-FLOAT-ARITH` citation).
    * Contract:    contracts/c-c-float-arith-v1.yaml
    * Sibling:     contracts/lean/PyFloatArith.lean (the structural template),
                   contracts/lean/CIntArith.lean (the C-int arithmetic sibling).
    * Roadmap:     docs/specifications/audit-design.md §7.3 (R6 / PMAT-475),
                   roadmap.yaml PMAT-910/911 (the deferred citation this discharges).

  Tier (per ruchy 5.0 §14.10.5): **Diamond** shape (structure-extensionality,
  one tier-defining theorem per width), joining the substrate at **depth-1**
  under the R6-grandfathered Diamond gate (PMAT-475a) — NOT forced to the
  depth-13 floor. Deeper tiers ratchet later.
-/

namespace XpileContracts.CCFloatArith

/--
  Abstract model of a C `float` (32-bit, `c_float`) as xpile lowers it: an
  IEEE-754 binary32 value, identified by its 32-bit pattern. xpile emits Rust
  `f32`; the value is determined by its bits.
-/
structure CFloat32 where
  bits : UInt32
  deriving DecidableEq

/--
  Abstract model of a C `double` (64-bit, `c_double`) as xpile lowers it: an
  IEEE-754 binary64 value, identified by its 64-bit pattern. xpile emits Rust
  `f64`; the value is determined by its bits.
-/
structure CFloat64 where
  bits : UInt64
  deriving DecidableEq

/--
  **Diamond refinement theorem** for `c_float32_structure_extensionality_diamond`
  (a tier-defining equation in the contract YAML).

  A C `float` is determined by its IEEE-754 binary32 bit pattern: two `CFloat32`
  values with equal bits are equal. The 32-bit analogue of
  `CPyFloatArith.py_float_structure_extensionality_diamond`; registers the
  32-bit lane of `C-C-FLOAT-ARITH` at depth-1.
-/
theorem c_float32_structure_extensionality_diamond (a b : CFloat32) :
    a.bits = b.bits → a = b := by
  intro h
  cases a
  cases b
  simp_all

/--
  **Diamond refinement theorem** for `c_float64_structure_extensionality_diamond`
  (a tier-defining equation in the contract YAML).

  A C `double` is determined by its IEEE-754 binary64 bit pattern: two `CFloat64`
  values with equal bits are equal. The 64-bit analogue, registering the 64-bit
  lane of `C-C-FLOAT-ARITH` at depth-1.
-/
theorem c_float64_structure_extensionality_diamond (a b : CFloat64) :
    a.bits = b.bits → a = b := by
  intro h
  cases a
  cases b
  simp_all

/--
  **ABI-honesty lemma** for `c_float_abi_widths_distinct` (the load-bearing
  ABI commitment shared with PMAT-909/910/911).

  The two C float ABI slots are DISTINCT sizes: `c_float` is 32-bit, `c_double`
  is 64-bit, and 32 ≠ 64. xpile therefore NEVER widens a C `float` through
  `c_double` — `CFloat32` (`UInt32` bits) and `CFloat64` (`UInt64` bits) are
  separate models exactly as `I64`/`CLong` are for the integer widths. This is
  the float-lane statement of the 32-vs-64 ABI lie PMAT-909 fixed for ints.
-/
theorem c_float_abi_widths_distinct : (32 : Nat) ≠ 64 := by decide

end XpileContracts.CCFloatArith
