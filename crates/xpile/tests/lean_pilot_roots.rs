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
//! lakefile `roots` and pins the pilot at the 11 modules that elaborate
//! today (the 9 from PMAT-903 + the 2 discharged in PMAT-904), and asserts
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
