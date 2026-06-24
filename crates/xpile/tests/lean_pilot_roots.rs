//! Lean proof-lane pilot regression guard (PMAT-904).
//!
//! The advisory `lake build` CI job (PMAT-903) is the *machine-checked*
//! source of truth for which `contracts/lean/*.lean` modules elaborate
//! clean — but it needs `lean`/`lake` in PATH, which the blocking Rust
//! `workspace-test` gate does NOT have. So a regression that silently
//! drops a discharged module out of `lakefile.lean`'s `roots` (shrinking
//! the proven pilot) would pass the Rust gate unnoticed until someone ran
//! the Lean job.
//!
//! This test is the cheap, lean-free guard for that: it parses the
//! lakefile `roots` and pins the pilot at the modules that elaborate today
//! (9 from PMAT-903 + 2 from PMAT-904 + FfiShellSubprocess/PMAT-907 +
//! CFloatArith/PMAT-912 + XpileFrontendTrait/PMAT-913 +
//! XlateRustFnToLeanThm/PMAT-914 + XlateLeanToRust/PMAT-915 +
//! Notation/PMAT-916 + Bashrs/PMAT-928 +
//! XlatePyBoolToRustBool/PMAT-935 + XlatePyListToVec/PMAT-936 +
//! FfiCpythonExt/PMAT-937 = 21), and asserts
//! the `PROVABILITY-INVENTORY.md` PILOT count stays in lockstep. It does
//! NOT re-prove anything — only that the bookkeeping the Lean job relies on
//! can't drift behind the Rust gate's back.

use std::fs;
use std::path::PathBuf;

fn lean_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("contracts")
        .join("lean")
}

/// Extract the module identifiers listed in the lakefile `roots := #[ … ]`
/// block. Each root is a line whose first non-whitespace char is a Lean
/// name-quote backtick, e.g. `` `XpileBackendTrait, -- comment``.
fn lakefile_roots() -> Vec<String> {
    let src = fs::read_to_string(lean_dir().join("lakefile.lean")).expect("read lakefile.lean");
    let mut in_roots = false;
    let mut roots = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("roots :=") {
            in_roots = true;
            continue;
        }
        if !in_roots {
            continue;
        }
        if trimmed.starts_with(']') {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix('`') {
            // `Name,  -- comment`  →  `Name`
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                roots.push(name);
            }
        }
    }
    roots
}

const EXPECTED_PILOT: &[&str] = &[
    "CIntArith",
    "PyFloatArith",
    "XlatePyDictToHashmap",
    "XlatePyStrToRustString",
    "XpileContractBackendTrait",
    "XlatePyClassToStruct",
    "XlatePyOptionalToOption",
    "XlatePySetToHashset",
    "XlatePyTupleToRustTuple",
    // PMAT-904 discharged (Sprint Day 5):
    "XpileBackendTrait",
    "XpileContractFrontendTrait",
    // PMAT-907 (Sprint Day 8): new Shell-subprocess FFI contract joins at
    // depth-1 (core-only STRUCTURE EXTENSIONALITY, same shape as PyFloatArith).
    "FfiShellSubprocess",
    // PMAT-912 (backlog slice): new C-float arithmetic contract joins at depth-1
    // (core-only STRUCTURE EXTENSIONALITY, two bit-width models + ABI-distinctness
    // lemma). Discharges the C-C-FLOAT-ARITH citation PMAT-910/911 deferred.
    "CFloatArith",
    // PMAT-913 (backlog slice): discharged the Layer-3 frontend trait — NOT a
    // termination error but the PMAT-904 class (a missing-parens precedence bug
    // collapsing a 4-way conjunction, and Mathlib-only `tauto` → core `decide`).
    "XpileFrontendTrait",
    // PMAT-914 (backlog slice): discharged the cheapest termination-led head.
    // The "fail to show termination" was actually a NAME-SHADOWING bug:
    // `def NonEmptyPreconditionList.val (n) := n.val` resolved `n.val` to itself
    // (non-terminating recursion); the broken `.val` then poisoned every
    // downstream `n.val` into the `n.property` / `Subtype.ext` mismatches. Fix =
    // use the positional `.1` Subtype projection (`n.1`) in the body.
    "XlateRustFnToLeanThm",
    // PMAT-915 (backlog slice): discharged the next cheapest termination-led
    // head — again the PMAT-914 NAME-SHADOWING class, not a real termination
    // argument: `def WarningLineCount.val (w) := w.val` resolved `w.val` to
    // itself (non-terminating self-call), poisoning the derived `DecidableEq`,
    // the `.property` refinement theorems, and the `Subtype.ext` proof. Fix =
    // positional `.1` projection in the body + an explicit `DecidableEq
    // WarningLineCount` instance (`unfold` + `infer_instance`) for the two
    // structures `deriving DecidableEq` over that opaque-`def` subtype field.
    "XlateLeanToRust",
    // PMAT-916 (backlog slice): discharged a 7-error head whose two faults are
    // both ALREADY-ESTABLISHED classes (no new termination territory):
    // (a) the PMAT-914/915 NAME-SHADOWING — `def NonEmptyDefinition.val (n) :=
    //     n.val` resolved `n.val` to itself (non-terminating self-call) →
    //     `fail to show termination`, poisoning the `.property`/`Subtype.ext`
    //     proofs; fix = positional `.1` projection in the body.
    // (b) the PMAT-904/913 Mathlib-only `tauto` — `cases k <;> tauto` over the
    //     decidable `LatexDisplayKind` enum → core `cases k <;> decide`.
    "Notation",
    // PMAT-928 (backlog slice): discharged the `Bashrs` MIXED head — one
    // already-known fault and one genuinely new (the inventory had flagged it):
    // (a) the PMAT-914/915/916 NAME-SHADOWING — `def SuccessfulOutcome.val (s)
    //     := s.val` resolved `s.val` to itself (non-terminating self-call) →
    //     `:213 fail to show termination`, poisoning the Gold equality, the
    //     `.property` witnesses, and the `Subtype.ext` proof; fix = positional
    //     `.1` projection in the body.
    // (b) NEW — a genuine Mathlib gap (not a name-shadow): the Int-sign Diamond
    //     used Mathlib's `|·|` notation (`:683` parse error) + `abs_nonneg` +
    //     `simp`; restated over CORE `Int.natAbs` (`Nat.zero_le` for
    //     non-negativity, `rw`+`rfl` for zero-abs-of-zero) + core
    //     `Int.lt_trichotomy`. Same Int-sign Diamond claim, no `import Mathlib`.
    "Bashrs",
    // PMAT-935 (R6 backlog slice): the NEW pure-`bool` translation contract joins
    // the pilot at depth-1 — a core-only, import-free `PyBool` single-truth-flag
    // STRUCTURE EXTENSIONALITY proof (same shape as PyFloatArith / the
    // str/list/set/tuple/Optional structural Diamonds). This is a NEW contract
    // (not a discharge of a previously-excluded module), so the pilot grows
    // 18 → 19 and closes the last uncited core scalar.
    "XlatePyBoolToRustBool",
    // PMAT-936 (backlog slice): discharged the `XlatePyListToVec` MIXED head
    // (8 errors), exactly as the inventory flagged — four sound classes, no new
    // termination territory:
    // (a) the PMAT-914/915/916/928 NAME-SHADOWING — `def
    //     NonEmptyHomogeneousList.val (n) := n.val` resolved `n.val` to itself
    //     (non-terminating self-call) → `:593 fail to show termination`,
    //     poisoning the `:632` `n.property` witness and the `:1257`
    //     `Subtype.ext` proof; fix = positional `.1` projection in the body.
    // (b) `:796` — `simp [List.length_append]` with no preceding `unfold` could
    //     not fold through the `def` → reused the discharged Platinum companion
    //     `lower_length_homomorphism_platinum`.
    // (c) `:980` — core `List.length_reverse` now takes the list explicitly →
    //     `List.length_reverse l.elems` (CORE lemma, not Mathlib).
    // (d) `:1359` — `Array.toList_length` is not a constant; core lemma is
    //     `Array.length_toList` → `exact Array.length_toList`. Layer-2
    //     Python-list → Rust-Vec contract now machine-checked. Pilot 19 → 20.
    "XlatePyListToVec",
    // PMAT-937 (backlog slice): discharged the `FfiCpythonExt` head — the
    // Layer-4 hybrid CPython-extension contract, cheapest of the three
    // remaining non-elaborating modules by `error:` count (20). FOUR sound
    // classes, no new termination territory:
    // (a) the PMAT-914/915/916/928/936 NAME-SHADOWING — `def
    //     BoundedRefcountDelta.val (b) := b.val` resolved `b.val` to itself
    //     (non-terminating self-call) → `:979 fail to show termination`,
    //     poisoning the `:987`/`:993` `deriving DecidableEq`, the `:1037`
    //     `.property`, the `:1750` `Subtype.ext`, and the `:1786`-`:1788`
    //     canonical `rfl`/`decide`s; fix = positional `.1` projection in the
    //     body + an explicit `DecidableEq BoundedRefcountDelta` instance.
    // (b) `:1235`/`:1236` — Mathlib-only `use` tactic → core `refine ⟨_, ?_⟩`.
    // (c) `:1466` — Mathlib `|·|`/`lt_trichotomy`/`abs_of_pos`/`Int.sign_mul_abs`
    //     restated over CORE `Int.natAbs` (PMAT-928 lesson).
    // (d) `:1808`+ — `lift_ffi_call_bronze_to_silver` was annotated the wrong
    //     structure (`FfiCallSilver`, which lacks `symbol`/…) → retargeted to
    //     `FfiCallStructuredSilver`. Layer-4 contract now machine-checked.
    //     Pilot 20 → 21; KNOWN-INCOMPLETE 3 → 2.
    "FfiCpythonExt",
];

#[test]
fn lakefile_pilot_matches_discharged_set() {
    let roots = lakefile_roots();
    for module in EXPECTED_PILOT {
        assert!(
            roots.iter().any(|r| r == module),
            "lakefile.lean roots is missing pilot module `{module}` — a discharged \
             proof must stay in the advisory `lake build` set. roots = {roots:?}"
        );
    }
    assert_eq!(
        roots.len(),
        EXPECTED_PILOT.len(),
        "lakefile.lean roots count drifted from the documented pilot \
         ({} modules). Update EXPECTED_PILOT and PROVABILITY-INVENTORY.md together. \
         roots = {roots:?}",
        EXPECTED_PILOT.len()
    );
}

#[test]
fn pmat_904_files_are_in_the_pilot() {
    // The two files PMAT-904 specifically discharged (the cheapest real
    // elaboration errors: Mathlib-only `tauto`, and `Inhabited`/`rw`-through-`def`).
    let roots = lakefile_roots();
    for module in ["XpileBackendTrait", "XpileContractFrontendTrait"] {
        assert!(
            roots.contains(&module.to_string()),
            "PMAT-904 discharged `{module}`; it must be a lakefile root"
        );
    }
}

#[test]
fn inventory_pilot_count_in_sync_with_lakefile() {
    let inventory = fs::read_to_string(lean_dir().join("PROVABILITY-INVENTORY.md"))
        .expect("read PROVABILITY-INVENTORY.md");
    let n = lakefile_roots().len();
    let needle = format!("PILOT — machine-checked ({n} modules");
    assert!(
        inventory.contains(&needle),
        "PROVABILITY-INVENTORY.md PILOT header must say '{needle}' to match the \
         {n} lakefile roots — doc and lakefile drifted apart"
    );
}
