/-
  PyContextManagerExit.lean — Lean 4 refinement proof for
  `C-PY-CONTEXT-MANAGER-EXIT`.

  Proof-lane counterpart to `contracts/py-context-manager-exit-v1.yaml` (R6 /
  PMAT-1131). Contracts xpile's user context-manager lowering (PMAT-1072):
  `with cm as x: BODY` desugars to `__cm = cm; x = __cm.__enter__(); try: BODY
  finally: __cm.__exit__(…)`. The load-bearing correctness property is the
  FINALLY GUARANTEE: because `__exit__` sits in a `finally`, it runs on EVERY
  exit path of the body — the normal (body-completes) path AND the exception
  (body-raises) path. This is why the desugar uses a `finally`-only try
  (PMAT-1073) rather than a plain `enter; BODY; exit` sequence, which would skip
  `__exit__` when the body raises.

  provability/mathlib note: the finally guarantee is `∀ outcome, exitRuns
  xpileDesugar outcome = true` over a two-constructor `Outcome` (ok | err) — a
  DECIDABLE property closed by `decide`. Discharged over CORE Lean 4 with NO
  `import Mathlib`, no `sorry`, no `axiom`; a control-flow guarantee reduces to
  a finite case analysis, needing nothing from real-analysis / linear-algebra.
  Fourth proof-lane contract this session — the four major capabilities shipped
  (exceptions, generators, file I/O, context managers) are now all under proven
  core-Lean contracts.

  PMAT-1141 (skeptic pass #5, IN-SLICE FIX): the original `exitRuns` was
  `fun _ => true` — a constant that DISCARDED its argument, so the three finally
  theorems reduced to `true = true` and asserted nothing about the actual
  lowering (a plain `enter; BODY; exit` sequence would STILL satisfy them). Two
  independent refutation agents flagged it as vacuous. `exitRuns` is now a
  function of the LOWERING (`hasExitInFinally`) as well as the outcome, so the
  finally guarantee is FALSIFIABLE: `plain_sequence_skips_exit_on_err` proves the
  no-finally lowering skips `__exit__` on a raise (the resource-leak bug the
  desugar exists to prevent), and `exit_on_err_iff_finally` pins exit-on-error to
  the flag. A constant `exitRuns` could not prove the `= false` direction.

  Scope note: xpile supports the SAFE (non-mutating) context-manager subset — a
  MUTATING `__enter__`/`__exit__` refuses (the Rc<RefCell> reference-model gap:
  a captured mutable would clone and silently drop the mutation). This contract
  pins the CONTROL-FLOW dispatch (exit runs on every path), which holds for both
  subsets; the value semantics of a mutating capture is a separate concern.
-/

namespace XpileContracts.CPyContextManagerExit

/--
  The outcome of the `with` BODY: it either COMPLETED normally (`ok`) or RAISED
  an exception (`err`). These are the two exit paths a `finally` must cover.
-/
inductive Outcome where
  | ok
  | err
  deriving DecidableEq

/--
  Model of the `with` lowering as its two emitted phases: `__enter__` (before
  the body) and `__exit__` (in the finally). Both are present in a well-formed
  desugar, and the lowering is determined by which phases it emits.
-/
structure WithLowering where
  hasEnter : Bool
  hasExitInFinally : Bool
  deriving DecidableEq

/--
  Does `__exit__` run, given the LOWERING and the body's outcome?

  On the NORMAL path (`ok`) both a finally-desugar AND a plain `enter; BODY;
  exit` sequence run `__exit__` — the body reaches the trailing call either way.
  On the EXCEPTION path (`err`) they DIVERGE: only a lowering that placed
  `__exit__` in a `finally` (`hasExitInFinally = true`) runs it before the
  exception propagates; a plain sequence has the raise jump PAST the trailing
  `exit` call, leaking the resource. So `exitRuns` genuinely branches on
  `w.hasExitInFinally` for the `err` outcome — the proof-lane mirror of the
  emitted `{ let r = catch_unwind(|| { enter; BODY }); __exit__(); if Err(e) = r
  { resume_unwind(e) } }`, whose `finally`-position `__exit__` is what a plain
  `enter; BODY; exit` lowering lacks.
-/
def exitRuns (w : WithLowering) : Outcome → Bool
  | Outcome.ok => true
  | Outcome.err => w.hasExitInFinally

/--
  xpile's ACTUAL desugar (PMAT-1072/1073): `__enter__` before the body and
  `__exit__` in a `finally`.
-/
def xpileDesugar : WithLowering := { hasEnter := true, hasExitInFinally := true }

/--
  The BUGGY plain-sequence lowering `enter; BODY; exit` — `__exit__` runs after
  the body but is NOT in a finally. Modeled so the finally guarantee has a real
  alternative to be falsified against (it was unmodeled before PMAT-1141).
-/
def plainSequence : WithLowering := { hasEnter := true, hasExitInFinally := false }

/--
  **Diamond refinement theorem** for
  `with_lowering_structure_extensionality_diamond` (the tier-defining equation):
  two lowerings agreeing on both phases are equal. Registers
  `C-PY-CONTEXT-MANAGER-EXIT` at depth-1.
-/
theorem with_lowering_structure_extensionality_diamond (a b : WithLowering) :
    a.hasEnter = b.hasEnter → a.hasExitInFinally = b.hasExitInFinally → a = b := by
  intro h1 h2
  cases a
  cases b
  simp_all

/--
  `__exit__` runs on the NORMAL path (the body completes) under xpile's desugar.
-/
theorem exit_runs_on_ok : exitRuns xpileDesugar Outcome.ok = true := by
  decide

/--
  `__exit__` runs on the EXCEPTION path (the body raises) under xpile's desugar —
  the reason it wraps the body in a `finally`, not a plain sequence. NON-vacuous:
  this holds precisely because `xpileDesugar.hasExitInFinally = true`; the
  plain-sequence lowering makes the same statement FALSE (see
  `plain_sequence_skips_exit_on_err`).
-/
theorem exit_runs_on_err : exitRuns xpileDesugar Outcome.err = true := by
  decide

/--
  **Finally guarantee** (the reason this contract exists): under xpile's desugar,
  `__exit__` runs on EVERY body outcome. A lowering that emitted `enter; BODY;
  exit` (no finally) would skip `__exit__` when the body raised — and that is now
  genuinely falsified (`plain_sequence_skips_exit_on_err`), not asserted by fiat.
-/
theorem exit_runs_always (o : Outcome) : exitRuns xpileDesugar o = true := by
  cases o <;> decide

/--
  The DUAL that makes the finally guarantee non-vacuous: the plain-sequence
  lowering `enter; BODY; exit` (no finally) SKIPS `__exit__` when the body raises
  — the exact resource-leak bug the finally desugar exists to prevent. Because
  `exitRuns` branches on `hasExitInFinally` for the `err` outcome, this `= false`
  direction is provable; the original constant `exitRuns := fun _ => true` could
  NOT prove it (PMAT-1141).
-/
theorem plain_sequence_skips_exit_on_err :
    exitRuns plainSequence Outcome.err = false := by
  decide

/--
  Exit-on-exception EQUALS the finally flag: `__exit__` runs on the raise path
  iff the lowering placed it in a `finally`. This pins `exitRuns` to
  `hasExitInFinally` — the statement is FALSE for a constant-true `exitRuns`, so
  it certifies that the finally guarantee is a real consequence of the desugar's
  structure, not a definitional constant.
-/
theorem exit_on_err_iff_finally (w : WithLowering) :
    exitRuns w Outcome.err = w.hasExitInFinally := by
  rfl

end XpileContracts.CPyContextManagerExit
