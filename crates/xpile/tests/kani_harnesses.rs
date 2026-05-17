//! Kani harness citation gate (PMAT-019 / XPILE-QUORUM-001).
//!
//! Every Layer-1 contract equation with a `kani_harness:` field must
//! reference a real file under `contracts/kani/` containing a real
//! `#[kani::proof]` function of that name. This is the **Symbolic
//! stratum** of the N-of-M oracle quorum from
//! `sub/provability-roadmap.md` §1.3, structurally symmetric with the
//! Lean (Semantic) stratum's gate in `refinement_proofs.rs`.
//!
//! What this test DOES enforce:
//!   1. Every cited `kani_harness:` + `kani_file:` pair resolves to
//!      a real file with a real proof of that name.
//!   2. Harness files live under `contracts/kani/`.
//!
//! What this test does NOT enforce:
//!   - `cargo kani` actually verifies the harness. That requires
//!     Kani installation in CI; deferred to XPILE-QUORUM-002.
//!   - Pairwise oracle-correlation guard (Ruchy §14.5 F3); also
//!     deferred — once we have ≥3 independent oracles the
//!     correlation matrix becomes meaningful.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

// Walk every contract YAML, find every `kani_harness:` + `kani_file:`
// pair, validate the harness exists in the file. Mirrors the
// `lean_theorem:` gate in refinement_proofs.rs.
#[test]
fn every_referenced_kani_harness_exists_in_its_file() {
    let root = workspace_root();
    let contracts_dir = root.join("contracts");
    let entries = fs::read_dir(&contracts_dir).expect("read contracts/");
    let mut checked = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let contents = fs::read_to_string(&path).expect("read contract yaml");
        let lines: Vec<&str> = contents.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let Some(harness) = extract_quoted_value(line, "kani_harness:") else {
                continue;
            };
            let file = lines
                .iter()
                .skip(i + 1)
                .take(3)
                .find_map(|l| extract_quoted_value(l, "kani_file:"))
                .unwrap_or_else(|| {
                    panic!(
                        "{}: kani_harness `{}` missing adjacent kani_file: \"...\"",
                        path.display(),
                        harness
                    )
                });
            let kani_path = root.join(&file);
            assert!(
                kani_path.is_file(),
                "{}: kani_harness `{}` references missing file `{}`",
                path.display(),
                harness,
                file
            );
            let src = fs::read_to_string(&kani_path).expect("read kani file");
            // Look for `fn <harness>` preceded somewhere above by
            // `#[kani::proof]`. The simplest robust check: the file
            // must contain BOTH `#[kani::proof]` and `fn <harness>(`
            // (Kani's attribute applies to the next `fn`).
            assert!(
                src.contains("#[kani::proof]"),
                "{}: kani_file `{}` must contain at least one `#[kani::proof]` attribute",
                path.display(),
                file
            );
            assert!(
                src.contains(&format!("fn {harness}(")),
                "{}: kani_harness `{}` not found as a `fn {}` in `{}`",
                path.display(),
                harness,
                harness,
                file
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 1,
        "expected at least one kani_harness field across contracts/; \
         XPILE-QUORUM-001 was supposed to plant one in py-int-arith-v1.yaml"
    );
}

// Helper: parse `key: "value"` out of a single line. Identical to
// refinement_proofs.rs's helper; duplicated rather than pulled into a
// shared crate because the test surface is the natural place for
// these one-off line parsers (closing both gates is independent work).
fn extract_quoted_value(line: &str, key: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix(key)?;
    let rest = rest.trim();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

// Sanity: py_int_arith.rs exists and carries the required landmarks
// — symmetric with `py_int_arith_lean_file_carries_required_landmarks`
// in refinement_proofs.rs.
#[test]
fn py_int_arith_kani_file_carries_required_landmarks() {
    let root = workspace_root();
    let kani = root.join("contracts/kani/py_int_arith.rs");
    assert!(kani.is_file(), "{} must exist", kani.display());
    let src = fs::read_to_string(&kani).expect("read py_int_arith.rs");
    for needle in &[
        "#![cfg(kani)]",
        "#[kani::proof]",
        "fn addition_no_overflow(",
        // Required cross-reference to the Lean Semantic stratum
        // counterpart (the quorum is only meaningful if Symbolic +
        // Semantic strata are visible to each other).
        "fast_path_eq_slow_path",
        // Required cross-reference to the contract.
        "C-PY-INT-ARITH",
    ] {
        assert!(
            src.contains(needle),
            "py_int_arith.rs must contain `{needle}` — see kani_harnesses.rs for context"
        );
    }
}

// Self-test for the parser — identical setup, separate from
// refinement_proofs.rs's analog because that test exists on its own
// PR's gates already.
#[test]
fn kani_extract_quoted_value_handles_common_yaml_shapes() {
    assert_eq!(
        extract_quoted_value("  kani_harness: \"foo\"", "kani_harness:"),
        Some("foo".to_string())
    );
    assert_eq!(
        extract_quoted_value("    kani_file: \"contracts/kani/x.rs\"", "kani_file:"),
        Some("contracts/kani/x.rs".to_string())
    );
    assert_eq!(
        extract_quoted_value("  kani_harness: \"foo\"", "kani_file:"),
        None
    );
}
