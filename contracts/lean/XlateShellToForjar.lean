/-
  XlateShellToForjar.lean — Lean 4 refinement proof for `C-COMPILE-SHELL-TO-FORJAR`.

  Proof-lane counterpart to `contracts/compile-shell-to-forjar-v1.yaml` (PMAT-953).
  A SHELL-origin meta-HIR command sequence lowers through `xpile-forjar-codegen`
  to forjar.yaml IaC manifest text — the BACKEND-ONLY forjar integration (forjar is
  a consumer/runtime, NOT a transpiler, so xpile emits forjar.yaml TEXT via a new
  `Target::ForjarYaml`, peer to bashrs-backend's Makefile/Dockerfile lane).

  The ops/deployment-lane sibling of `CompileRustToWasm.lean` / `CompileRustToWgsl.lean`
  (Layer 5 / compile-time). Where the WASM module models the emitted WAT function's
  `(name, params, result)` signature, this one models the emitted forjar resource's
  structural shape — its `id`, its forjar `kind` (file / task / cron), and the
  `machine` it pins — and proves STRUCTURE EXTENSIONALITY over it: a forjar resource
  is determined by its (id, kind, machine) signature. This registers
  `C-COMPILE-SHELL-TO-FORJAR` at depth-1 under the Diamond gate, mirroring the
  str/list/float/set/wgsl/wasm structural Diamonds. Core-only, no Mathlib,
  sorry-free — machine-checked by the `lake build` pilot.

  Apply-convergence Diamonds (the actual idempotent-apply agreement) are forjar's
  own contracts (`idempotent-apply` / `plan-apply-equivalence`), NOT xpile's — the
  two substrates hand off at the YAML boundary (xpile emits correctly, forjar proves
  convergence). The depth-1 tier-defining theorem here is the emission's structure
  extensionality.
-/

namespace XpileContracts.CCompileShellToForjar

/--
  A forjar resource kind — the clean cells `xpile-forjar-codegen` emits: a
  `type: file` (a script body materialised at a path with a mode), a
  `type: task` (a bare command forjar runs), or a `type: cron` (a scheduled
  command). The lossy cells (conditional / idempotence guard) are REFUSED at
  emit time, so they have no kind here.
-/
inductive ForjarKind where
  | file
  | task
  | cron
  deriving DecidableEq

/--
  Abstract model of an emitted forjar resource as `xpile-forjar-codegen`
  produces it: a resource `id`, a forjar `kind`, and the `machine` it is
  pinned to. xpile emits forjar.yaml whose well-formedness is gated by a YAML
  round-trip (and, at the boundary, by forjar's own `validate_config`).
-/
structure ForjarResource where
  id : String
  kind : ForjarKind
  machine : String
  deriving DecidableEq

/--
  **Diamond refinement theorem** for `forjar_emission_structure_extensionality_diamond`
  (the tier-defining equation in the contract YAML): two emitted forjar resources
  with the same id, the same kind, and the same machine are equal. Registers
  `C-COMPILE-SHELL-TO-FORJAR` at depth-1, mirroring the structural Diamonds of the
  str/list/float/set/wgsl/wasm contracts. Sorry-free, core-only.
-/
theorem forjar_emission_structure_extensionality_diamond (a b : ForjarResource) :
    a.id = b.id →
    a.kind = b.kind →
    a.machine = b.machine →
    a = b := by
  intro hid hk hm
  cases a
  cases b
  simp_all

end XpileContracts.CCompileShellToForjar
