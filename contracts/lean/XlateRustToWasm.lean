/-
  XlateRustToWasm.lean — Lean 4 refinement proof for `C-COMPILE-RUST-TO-WASM`.

  Proof-lane counterpart to `contracts/compile-rust-to-wasm-v1.yaml` (PMAT-951).
  A meta-HIR scalar/control function lowers through `xpile-wasm-codegen` to a
  WebAssembly Text (WAT) function — natively, NOT via the Ruchy `WasmEmitter` hop.
  This is the EMIT half of first-class bidirectional native WASM.

  The WASM sibling of `CompileRustToWgsl.lean` (Layer 5 / compile-time). Where the
  WGSL module models the emitted kernel's `(entry, workgroup_size, bindings)`
  signature, this one models the emitted WAT function's structural shape — its
  `$name`, the ordered list of param WASM value-types, and the result value-type —
  and proves STRUCTURE EXTENSIONALITY over it: a WAT function is determined by its
  (name, params, result) signature. This registers `C-COMPILE-RUST-TO-WASM` at
  depth-1 under the Diamond gate, mirroring the str/list/float/set/wgsl structural
  Diamonds. Core-only, no Mathlib, sorry-free — machine-checked by the `lake build`
  pilot.

  Execution-semantics Diamonds (the actual numeric agreement attested by a
  wasm-runtime DiffExec witness — the two-emitter §29 quorum) are the
  runtime-stratum half and ratchet into deeper tiers later (PMAT-952); the depth-1
  tier-defining theorem here is the emission's structure extensionality.
-/

namespace XpileContracts.CCompileRustToWasm

/--
  A WASM value type — the lowered shape of a meta-HIR scalar type as
  `xpile-wasm-codegen` produces it: `i64` (I64/CLong), `i32` (Bool/CUInt),
  `f64` (F64), `f32` (F32).
-/
inductive WasmValTy where
  | i64
  | i32
  | f64
  | f32
  deriving DecidableEq

/--
  Abstract model of an emitted WAT function as `xpile-wasm-codegen` produces it:
  a `$name`, the ordered list of param WASM value-types, and the result WASM
  value-type. xpile emits WAT whose well-formedness is gated by `wat2wasm`.
-/
structure WasmFunc where
  name : String
  params : List WasmValTy
  result : WasmValTy
  deriving DecidableEq

/--
  **Diamond refinement theorem** for `wasm_emission_structure_extensionality_diamond`
  (the tier-defining equation in the contract YAML): two emitted WAT functions with
  the same name, the same param value-types, and the same result value-type are
  equal. Registers `C-COMPILE-RUST-TO-WASM` at depth-1, mirroring the structural
  Diamonds of the str/list/float/set/wgsl contracts. Sorry-free, core-only.
-/
theorem wasm_emission_structure_extensionality_diamond (a b : WasmFunc) :
    a.name = b.name →
    a.params = b.params →
    a.result = b.result →
    a = b := by
  intro hn hp hr
  cases a
  cases b
  simp_all

end XpileContracts.CCompileRustToWasm
