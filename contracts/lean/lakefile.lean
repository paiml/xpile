import Lake
open Lake DSL

/-!
  `xpile` proof-lane build — PMAT-903 (Sprint Day 4).

  Turns the proof lane from a `grep sorry` claim into a machine-checked one:
  `lake build` elaborates the PILOT set below against a pinned Lean toolchain
  (`lean-toolchain` → v4.15.0), wired as a SEPARATE ADVISORY CI job (never the
  blocking publish gate — mirrors the kani job posture, so Lean weight cannot
  stall the fast gate or the crates.io window).

  Core-only, no Mathlib: PMAT-903 verified that NOT ONE `contracts/lean/*.lean`
  module carries an `import Mathlib` (or any `import` at all) — the "Mathlib"
  mentions are docstring references to lemma names, not real dependencies. So
  the pilot needs no Mathlib cache and the job stays fast and hermetic.

  Truth-up (PMAT-903): the long-quoted "6 `sorry` across 5 files" debt was a
  naive-`grep` artifact — those hits are docstring prose ("…genuinely provable,
  sorry-free…") plus a `ProofStubReason` inductive constructor literally named
  `sorry` in `Notation.lean`. There is ZERO use of the `sorry` tactic and ZERO
  `axiom` declarations in the whole tree (all "axiom" hits are prose). With
  `warningAsError := true` below, a real `sorry` could not survive a green build
  — so this job is exactly the check that makes "provable" un-falsifiable by
  `grep sorry`.

  PILOT = the 11 modules that elaborate clean under bare core with
  warnings-as-errors (9 from PMAT-903 + the 2 PMAT-904 discharged on Day 5).
  The known-incomplete remainder is 9 modules with REAL elaboration errors
  (termination / type-mismatch / synthesis failures — NOT sorries), enumerated
  honestly in `PROVABILITY-INVENTORY.md` and deliberately EXCLUDED here so this
  advisory job is GREEN without overstating what is proven. Discharging the
  remaining errors and re-adding files is ongoing Day 5+ work.
-/

package «xpileContracts» where
  -- Treat `sorry` as an error inside the pilot: the whole point is that the
  -- pilot set is genuinely hole-free. Anything that regresses to a `sorry`
  -- breaks `lake build`, not just emits a warning.
  leanOptions := #[⟨`warningAsError, true⟩]

@[default_target]
lean_lib «XpileContractsPilot» where
  srcDir := "."
  -- Each module is fully standalone (zero inter-module imports), so listing a
  -- module as a root builds exactly it and nothing else. Keep this list in
  -- lockstep with PROVABILITY-INVENTORY.md's "pilot (machine-checked)" table.
  roots := #[
    `CIntArith,                  -- C-C-INT-ARITH
    `PyFloatArith,               -- C-PY-FLOAT-ARITH            (PMAT-903 nested-comment fix)
    `XlatePyDictToHashmap,       -- C-XLATE-PY-DICT-TO-HASHMAP
    `XlatePyStrToRustString,     -- C-XLATE-PY-STR-TO-RUST-STRING
    `XpileContractBackendTrait,  -- C-XPILE-CONTRACT-BACKEND
    `XlatePyClassToStruct,       -- C-XLATE-PY-CLASS-TO-STRUCT   (was mis-flagged "sorry")
    `XlatePyOptionalToOption,    -- C-XLATE-PY-OPTIONAL-TO-OPTION(was mis-flagged "sorry")
    `XlatePySetToHashset,        -- C-XLATE-PY-SET-TO-HASHSET    (was mis-flagged "sorry")
    `XlatePyTupleToRustTuple,    -- C-XLATE-PY-TUPLE-TO-RUST-TUPLE (was mis-flagged "sorry")
    -- PMAT-904 (Sprint Day 5): discharged the two cheapest elaboration-error
    -- files. XpileBackendTrait: Mathlib-only `tauto` → core `decide` over the
    -- decidable enum cases. XpileContractFrontendTrait: derived `Inhabited
    -- EquationsBlock` for `[0]!`, and re-proved `frame_safety_transitive_platinum`
    -- as a defeq `calc` (`rw` can't see through the `before`/`after` defs).
    `XpileBackendTrait,          -- C-XPILE-BACKEND-TRAIT        (PMAT-904: tauto→decide)
    `XpileContractFrontendTrait, -- C-XPILE-CONTRACT-FRONTEND    (PMAT-904: Inhabited + calc)
    -- PMAT-907 (Sprint Day 8): the new Shell-subprocess FFI contract joins the
    -- pilot at depth-1 — a core-only, import-free STRUCTURE EXTENSIONALITY proof
    -- (same shape as PyFloatArith), machine-checked here so the contract's
    -- depth-1 Diamond is not a string claim.
    `FfiShellSubprocess,         -- C-FFI-SHELL-SUBPROCESS       (PMAT-907)
    -- PMAT-912 (backlog slice): the new C-float arithmetic contract joins the
    -- pilot at depth-1 — core-only STRUCTURE EXTENSIONALITY with TWO bit-width
    -- models (binary32 c_float + binary64 c_double) + an ABI-distinctness lemma,
    -- same shape as PyFloatArith. Discharges the citation PMAT-910/911 deferred.
    `CFloatArith                 -- C-C-FLOAT-ARITH              (PMAT-912)
  ]
