/-
  CompileRustToSpirv.lean — Lean 4 refinement proof for `C-COMPILE-RUST-TO-SPIRV`.

  Proof-lane counterpart to `contracts/compile-rust-to-spirv-v1.yaml` (PMAT-960).
  A Rust compute kernel lowers through `xpile-spirv-codegen` to SPIR-V — the
  native Vulkan IR — by REUSING the WGSL emission and compiling it
  WGSL → naga (`wgsl-in`) → naga (`spv-out`). The §29 Multi-Emitter Oracle
  Quorum then RUNS the emitted SPIR-V on a real wgpu Vulkan adapter and
  numerically compares two categorically-independent emitters' executed outputs.

  The SPIR-V sibling of `CompileRustToWgsl.lean` (Layer 5 / compile-time). Where
  the WGSL module models the emitted WGSL kernel's structural shape, this one
  models the emitted SPIR-V module's structural shape — the SPIR-V magic word,
  the version word, the declared id-bound, and the ordered entry-point names —
  and proves STRUCTURE EXTENSIONALITY over it: a SPIR-V module is determined by
  its (magic, version, idBound, entryPoints) signature. This registers
  `C-COMPILE-RUST-TO-SPIRV` at depth-1 under the Diamond gate, mirroring the
  WGSL / str / list / float / set structural Diamonds. Core-only, no Mathlib,
  sorry-free — machine-checked by the `lake build` pilot.

  Execution-semantics Diamonds (the actual `2*x+1` numeric agreement attested by
  the wgpu Vulkan SPIR-V DiffExec witness) are the runtime-stratum half of the
  §29 quorum and ratchet into deeper tiers later; the depth-1 tier-defining
  theorem here is the emission's structure extensionality.
-/

namespace XpileContracts.CCompileRustToSpirv

/--
  Abstract model of an emitted SPIR-V module as `xpile-spirv-codegen` produces
  it (via the reused WGSL → naga → spv path): the SPIR-V magic word, the version
  word, the declared id-bound, and the ordered list of `@compute` entry-point
  names. xpile emits SPIR-V whose well-formedness is gated by `validate_spirv`
  (magic = 0x07230203, a complete header, a non-zero id-bound).
-/
structure SpirvModule where
  magic : Nat
  version : Nat
  idBound : Nat
  entryPoints : List String
  deriving DecidableEq

/--
  **Diamond refinement theorem** for `spirv_emission_structure_extensionality_diamond`
  (the tier-defining equation in the contract YAML): two emitted SPIR-V modules
  with the same magic word, the same version, the same id-bound, and the same
  ordered entry-point list are equal. Registers `C-COMPILE-RUST-TO-SPIRV` at
  depth-1, mirroring the structural Diamonds of the WGSL / str / list / float /
  set contracts. Sorry-free, core-only.
-/
theorem spirv_emission_structure_extensionality_diamond (a b : SpirvModule) :
    a.magic = b.magic →
    a.version = b.version →
    a.idBound = b.idBound →
    a.entryPoints = b.entryPoints →
    a = b := by
  intro hm hv hi he
  cases a
  cases b
  simp_all

end XpileContracts.CCompileRustToSpirv
