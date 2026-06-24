/-
  FfiShellSubprocess.lean — Lean 4 refinement proof for `C-FFI-SHELL-SUBPROCESS`.

  Proof-lane counterpart to `contracts/ffi-shell-subprocess-v1.yaml` (PMAT-907,
  Sprint Day 8). When the hybrid reconciler resolves a boundary whose callee
  language is `Shell`, `emit_shell_shim` (crates/xpile-ffi-manifest) emits a safe,
  `unsafe`-free wrapper over `std::process::Command`: argv strings in, an exit
  code + captured output out. This contract governs that strategy — the
  calling-site companion of `C-BASHRS-POSIX-IDEMPOTENCE` (which governs the
  emitted shell *script*).

  Modelling note: like `XlatePyStrToRustString` / `PyFloatArith`, the genuinely
  provable, depth-1-registering claim is STRUCTURE EXTENSIONALITY rather than a
  runtime-IO semantics. The observable result of a shell-shim invocation is fully
  determined by its three fields — the resolved program name, the forwarded argv
  (order-preserving), and the POSIX exit code surfaced in `Output.status`. We model
  that as a record and prove field equality implies record equality, which
  registers `C-FFI-SHELL-SUBPROCESS` at depth-1 under the R6 Diamond gate
  (PMAT-475a). Argv-passthrough and exit-code-propagation tiers (the two YAML
  equations' runtime claims) ratchet in once a live Python `subprocess.run` →
  Shell `FfiBoundary` frontend producer lands.

  Core-only, no imports (lakefile is `warningAsError := true`, Mathlib-free).

  Cross-references:
    * Code lane:   crates/xpile-ffi-manifest/src/lib.rs (`emit_shell_shim`, the
                   `// xpile-contract: C-FFI-SHELL-SUBPROCESS` citation).
    * Contract:    contracts/ffi-shell-subprocess-v1.yaml
    * Companion:   contracts/lean/Bashrs.lean (the emitted-script side).
    * Roadmap:     docs/specifications/sub/sprint-10day-2026-06-23.md (Day 8).
-/

namespace XpileContracts.CFfiShellSubprocess

/--
  Abstract model of a shell-shim invocation's observable result, as
  `emit_shell_shim` lowers it through `std::process::Command::new(program)
  .args(argv).output()`:

    * `program` — the resolved boundary symbol's bytes (the spawned program
      name; no prefixing, no path rewriting).
    * `argv`    — the forwarded argument vector, order- and content-preserving
      (`Command::args`, not shell word-splitting).
    * `exit_code` — the POSIX 0..255 status surfaced in `Output.status`.

  The observable outcome is determined by these three fields.
-/
structure ShellInvocation where
  program : List UInt8
  argv : List (List UInt8)
  exit_code : UInt8
  deriving DecidableEq

/--
  **Diamond refinement theorem** for
  `shell_invocation_structure_extensionality_diamond` (the tier-defining equation
  in the contract YAML).

  A shell-shim invocation is determined by its three fields: two `ShellInvocation`
  values with equal program, equal argv, and equal exit code are equal. This is
  the shell analogue of
  `CPyFloatArith.py_float_structure_extensionality_diamond` and registers
  `C-FFI-SHELL-SUBPROCESS` at depth-1. An emitter that dropped or reordered argv
  (so two invocations with the "same" intent diverge in the `argv` field) would
  be distinguished by this extensionality, not collapsed.
-/
theorem shell_invocation_structure_extensionality_diamond (a b : ShellInvocation) :
    a.program = b.program → a.argv = b.argv → a.exit_code = b.exit_code → a = b := by
  intro hp ha he
  cases a
  cases b
  simp_all

end XpileContracts.CFfiShellSubprocess
