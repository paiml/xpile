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

  PILOT = the 16 modules that elaborate clean under bare core with
  warnings-as-errors (9 from PMAT-903 + 2 PMAT-904 + FfiShellSubprocess/907 +
  CFloatArith/912 + XpileFrontendTrait/913 + XlateRustFnToLeanThm/914 +
  XlateLeanToRust/915).
  The known-incomplete remainder is 6 modules with REAL elaboration errors
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
    `CFloatArith,                -- C-C-FLOAT-ARITH              (PMAT-912)
    -- PMAT-913 (backlog slice): discharged the cheapest non-elaborating
    -- module — NOT a termination error but the PMAT-904 class: (a) clause (c)
    -- of `parse_and_lower_function_diamond` lacked parens, so its `∀ f'`
    -- right-extended and swallowed clause (d) (the 4-way `refine ⟨…⟩` then hit
    -- a `∀`, not an inductive); (b) `source_lang_enum_completeness_diamond`
    -- used Mathlib-only `tauto` over the decidable `SourceLang` disjunction →
    -- core `decide`. Layer-3 frontend trait now machine-checked, not excluded.
    `XpileFrontendTrait,          -- C-XPILE-FRONTEND-TRAIT       (PMAT-913)
    -- PMAT-914 (backlog slice): discharged the cheapest termination-led head.
    -- The fault was NOT a genuine missing termination argument but a NAME
    -- SHADOWING bug: `def NonEmptyPreconditionList.val (n) := n.val` resolved
    -- `n.val` by dot-notation to *itself* (a non-terminating recursive call,
    -- `n` unchanged), and that broken `.val` then poisoned every downstream
    -- `n.val`, cascading into the `n.property` and `Subtype.ext` type
    -- mismatches. Fix = use the positional `.1` Subtype projection in the body
    -- (`n.1`), breaking the self-reference; all three errors clear at once.
    `XlateRustFnToLeanThm,         -- C-XLATE-RUST-FN-TO-LEAN-THM (PMAT-914)
    -- PMAT-915 (backlog slice): discharged the next cheapest termination-led
    -- head — again NOT a genuine termination argument but the PMAT-914 NAME
    -- SHADOWING class: `def WarningLineCount.val (w) := w.val` resolved `w.val`
    -- by dot-notation to *itself* (a non-terminating self-call, `w` unchanged),
    -- poisoning the derived `DecidableEq`, the `.property` refinement theorems,
    -- and the `Subtype.ext` extensionality proof. Two-part fix: (a) positional
    -- `.1` Subtype projection in the body breaks the self-reference (clears the
    -- termination + `.property` + `Subtype.ext` errors at once); (b) an explicit
    -- `DecidableEq WarningLineCount` instance (`unfold` + `infer_instance`) so
    -- the two structures `deriving DecidableEq` over that opaque-`def` subtype
    -- field synthesize. Layer-2 Lean→Rust translation now machine-checked.
    `XlateLeanToRust,              -- C-XLATE-LEAN-TO-RUST       (PMAT-915)
    -- PMAT-916 (backlog slice): discharged a 7-error head whose two faults are
    -- BOTH already-established classes (no new termination territory):
    -- (a) the PMAT-914/915 NAME SHADOWING — `def NonEmptyDefinition.val (n) :=
    --     n.val` resolved `n.val` by dot-notation to *itself* (a non-terminating
    --     self-call, `n` unchanged → `fail to show termination` at :816),
    --     poisoning the `.property` (:858) and `Subtype.ext` (:1477) proofs;
    --     fix = positional `.1` Subtype projection in the body.
    -- (b) the PMAT-904/913 Mathlib-only `tauto` — `cases k <;> tauto` over the
    --     decidable `LatexDisplayKind` enum (:1387) → core `cases k <;> decide`.
    -- Layer-5 LaTeX-math notation contract now machine-checked, not excluded.
    `Notation                      -- C-NOTATION-LATEX-MATH-TO-EQUATION (PMAT-916)
  ]
