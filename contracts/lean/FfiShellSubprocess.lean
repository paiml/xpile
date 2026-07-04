/-
  FfiShellSubprocess.lean — Lean 4 refinement proof for `C-FFI-SHELL-SUBPROCESS`.

  Proof-lane counterpart to `contracts/ffi-shell-subprocess-v1.yaml` (PMAT-907,
  Sprint Day 8). When the hybrid reconciler resolves a boundary whose callee
  language is `Shell`, `emit_shell_shim` (crates/xpile-ffi-manifest) emits a safe,
  `unsafe`-free wrapper over `std::process::Command`: argv strings in, an exit
  code + captured output out. This contract governs that strategy — the
  calling-site companion of `C-BASHRS-POSIX-IDEMPOTENCE` (which governs the
  emitted shell *script*).

  Modelling note: like `XlatePyStrToRustString` / `PyFloatArith`, the
  depth-1-registering, tier-defining claim is STRUCTURE EXTENSIONALITY. The
  observable result of a shell-shim invocation is fully determined by its three
  fields — the resolved program name, the forwarded argv (order-preserving), and
  the POSIX exit code surfaced in `Output.status`. We model that as a record and
  prove field equality implies record equality, which registers
  `C-FFI-SHELL-SUBPROCESS` at depth-1 under the R6 Diamond gate (PMAT-475a).

  PMAT-957 (de-vacuity hardening): the structure-extensionality Diamond is a
  genuine claim about the RECORD SHAPE, but on its own it certifies NOTHING about
  whether `emit_shell_shim` actually FORWARDS the argv or PROPAGATES the exit
  code — the two load-bearing YAML equations (`argv_passthrough`,
  `exit_code_propagation`) carried no `lean_theorem`, so the contract's actual
  correctness was unmodelled (the over-claim class the de-vacuity queue closes:
  a green structure-ext proof ≠ the semantic claim). Both equations are now
  discharged at the MODEL level below (`argv_passthrough` / `exit_code_propagation`),
  each with a `≠` NON-VACUITY DUAL exhibiting the exact defective shim the YAML
  names — proved TRUE for the faithful shim AND FALSE for the adversarial one, so
  neither can be vacuous. The live Python `subprocess.run` → Shell `FfiBoundary`
  producer already landed (PMAT-932); what remains deferred is only the RUNTIME
  differential oracle (`shell_diff_exec`) diffing a real subprocess against the
  emitted `Command` shim — the kernel-tier promotion.

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

/-! ## PMAT-957 — non-vacuous `argv_passthrough` + `exit_code_propagation`.

    De-vacuity hardening (see the header note). The structure-extensionality
    Diamond above certifies the record SHAPE but says nothing about whether the
    emitted shim actually forwards argv / propagates the exit code. We model the
    `emit_shell_shim` lowering as a function of the caller's `(sym, args,
    subExit)` carrying two Bool defect flags that name the EXACT falsifiers the
    YAML calls out — `drops_argv` (omits `.args(args)`) and `swallows_exit`
    (maps every run to status `0`). The faithful xpile shim sets both `false`;
    the two YAML equations then hold BY that faithfulness (load-bearing — flip a
    flag and the equation is FALSE), and each carries a `≠` NON-VACUITY DUAL
    exhibiting the defective shim that breaks it. A property provably TRUE for
    the faithful shim AND provably FALSE for the adversarial one cannot be
    vacuous. Model-level (Bronze) discharge, mirroring
    `PyExceptAllowlist`'s dispatch semantics and `PyFileIoRoundtrip`'s
    String-state round-trip; the runtime differential oracle stays deferred. -/

/--
  Model of `emit_shell_shim`'s lowering as an observable-producing function. A
  FAITHFUL shim forwards the caller's argv verbatim and surfaces the
  subprocess's POSIX exit code unchanged; two Bool flags model the two concrete
  defects the YAML falsifiers name:
    * `drops_argv`    — omits the `.args(args)` forwarding (argv → `[]`),
    * `swallows_exit` — maps every run to status `0` (drops the real exit code).
-/
structure ShimBackend where
  drops_argv : Bool
  swallows_exit : Bool
  deriving DecidableEq

/--
  Run the modelled shim on `(sym, args, subExit)` — `subExit` is the POSIX
  status the spawned subprocess actually returned (what `subprocess.run`
  observes). The spawned program name is always the resolved boundary symbol
  `sym` (the shim never prefixes or path-rewrites), so any divergence must show
  up in the `argv` or `exit_code` field, exactly where the two equations look.
-/
def run_shim (b : ShimBackend) (sym : List UInt8) (args : List (List UInt8))
    (subExit : UInt8) : ShellInvocation :=
  { program := sym
    argv := if b.drops_argv then [] else args
    exit_code := if b.swallows_exit then 0 else subExit }

/-- The canonical faithful xpile shell shim — forwards argv, propagates exit. -/
def xpileShellShim : ShimBackend := { drops_argv := false, swallows_exit := false }

/-- A defective shim that omits `.args(args)` — the argv-passthrough falsifier. -/
def argvDroppingShim : ShimBackend := { drops_argv := true, swallows_exit := false }

/-- A defective shim that maps every run to status `0` — the exit-propagation
    falsifier the YAML names ("mapping all runs to status 0"). -/
def exitSwallowingShim : ShimBackend := { drops_argv := false, swallows_exit := true }

/--
  **`argv_passthrough`** (the YAML `argv_passthrough` equation, model-level).

  The faithful xpile shim spawns exactly the resolved boundary symbol AND
  forwards the caller's argv verbatim, in order: `program = sym ∧ argv = args`
  for every `(sym, args, subExit)`. This holds ONLY because
  `xpileShellShim.drops_argv = false` — the faithfulness is load-bearing (flip
  it and the argv equality breaks, as `argv_dropping_shim_violates_passthrough`
  witnesses). Not a reflexivity tautology: the `≠` dual below is UNPROVABLE for
  a shim that genuinely forwarded argv.
-/
theorem argv_passthrough
    (sym : List UInt8) (args : List (List UInt8)) (subExit : UInt8) :
    (run_shim xpileShellShim sym args subExit).program = sym
      ∧ (run_shim xpileShellShim sym args subExit).argv = args := by
  simp [run_shim, xpileShellShim]

/-- **`≠` NON-VACUITY DUAL** for `argv_passthrough`: an emitter that omits
    `.args(args)` (the YAML's "reordered or dropped argv elements" falsifier)
    forwards a DIFFERENT argv — there is a `(sym, args)` whose argv it fails to
    preserve. If `argv_passthrough` were vacuous this would be UNPROVABLE. -/
theorem argv_dropping_shim_violates_passthrough :
    ∃ (sym : List UInt8) (args : List (List UInt8)) (subExit : UInt8),
      (run_shim argvDroppingShim sym args subExit).argv ≠ args := by
  refine ⟨[], [[0]], 0, ?_⟩
  decide

/--
  **`exit_code_propagation`** (the YAML `exit_code_propagation` equation,
  model-level).

  The faithful xpile shim surfaces the subprocess's POSIX exit code unchanged:
  `exit_code = subExit` for every `(sym, args, subExit)`. This holds ONLY because
  `xpileShellShim.swallows_exit = false` — the faithfulness is load-bearing (flip
  it and the equation is FALSE, as `exit_swallowing_shim_violates_propagation`
  witnesses). Not a reflexivity tautology: the `≠` dual below is UNPROVABLE for
  a shim that genuinely propagated the exit code.
-/
theorem exit_code_propagation
    (sym : List UInt8) (args : List (List UInt8)) (subExit : UInt8) :
    (run_shim xpileShellShim sym args subExit).exit_code = subExit := by
  simp [run_shim, xpileShellShim]

/-- **`≠` NON-VACUITY DUAL** for `exit_code_propagation`: an emitter that maps
    every run to status `0` (the YAML's "swallowed a non-zero exit" falsifier)
    reports the WRONG code — there is a non-zero subprocess exit it fails to
    surface. If `exit_code_propagation` were vacuous this would be UNPROVABLE. -/
theorem exit_swallowing_shim_violates_propagation :
    ∃ (sym : List UInt8) (args : List (List UInt8)) (subExit : UInt8),
      (run_shim exitSwallowingShim sym args subExit).exit_code ≠ subExit := by
  refine ⟨[], [], 1, ?_⟩
  decide

end XpileContracts.CFfiShellSubprocess
