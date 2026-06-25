/-
  CompileRustToWgsl.lean — Lean 4 refinement proof for `C-COMPILE-RUST-TO-WGSL`.

  Proof-lane counterpart to `contracts/compile-rust-to-wgsl-v1.yaml` (PMAT-950).
  A Rust compute kernel lowers through `xpile-wgsl-codegen` to a WGSL compute
  shader; the §29 Multi-Emitter Oracle Quorum then RUNS the emitted WGSL on a
  real wgpu adapter (Vulkan/Metal/DX12) and numerically compares two
  categorically-independent emitters' executed outputs.

  The WGSL sibling of `CompileRustToPtxMma.lean` (Layer 5 / compile-time). Where
  the PTX module models the emitted PTX text, this one models the emitted WGSL
  kernel's structural shape — the `@compute @workgroup_size(N)` entry point plus
  its ordered storage-binding indices — and proves STRUCTURE EXTENSIONALITY over
  it: a WGSL kernel is determined by its (entry, workgroup_size, bindings)
  signature. This registers `C-COMPILE-RUST-TO-WGSL` at depth-1 under the Diamond
  gate, mirroring the str/list/float/set structural Diamonds. Core-only, no
  Mathlib, sorry-free — machine-checked by the `lake build` pilot.

  Execution-semantics Diamonds (the actual `2*x+1` numeric agreement attested by
  the wgpu DiffExec witness) are the runtime-stratum half of the §29 quorum and
  ratchet into deeper tiers later; the depth-1 tier-defining theorem here is the
  emission's structure extensionality.
-/

namespace XpileContracts.CCompileRustToWgsl

/--
  Abstract model of an emitted WGSL compute kernel as `xpile-wgsl-codegen`
  produces it: a `@compute` entry-point name, its `@workgroup_size`, and the
  ordered list of storage-buffer binding indices it reads/writes. xpile emits
  WGSL whose well-formedness is gated by `validate_wgsl`.
-/
structure WgslKernel where
  entry : String
  workgroupSize : Nat
  bindings : List Nat
  deriving DecidableEq

/--
  **Diamond refinement theorem** for `wgsl_emission_structure_extensionality_diamond`
  (the tier-defining equation in the contract YAML): two emitted WGSL kernels with
  the same entry point, the same workgroup size, and the same binding list are
  equal. Registers `C-COMPILE-RUST-TO-WGSL` at depth-1, mirroring the structural
  Diamonds of the str/list/float/set contracts. Sorry-free, core-only.
-/
theorem wgsl_emission_structure_extensionality_diamond (a b : WgslKernel) :
    a.entry = b.entry →
    a.workgroupSize = b.workgroupSize →
    a.bindings = b.bindings →
    a = b := by
  intro he hw hb
  cases a
  cases b
  simp_all

end XpileContracts.CCompileRustToWgsl
