//! Unified §14.4 quorum reporter gate (PMAT-033).
//!
//! Asserts the live state we shipped over PMAT-017..032:
//!   * `C-PY-INT-ARITH` has all four strata represented (Semantic via
//!     7 Lean theorems, Symbolic via 1 Kani harness, Runtime via fixture
//!     witnesses, Extrinsic via roadmap mentions).
//!   * Its quorum status is `QUORUM`.
//!   * The reporter walks all 11 contracts.
//!
//! This is the integration counterpart to the unit tests in
//! `quorum_tests` inside the binary crate — those exercise the threshold
//! logic against synthetic inputs; this one exercises the *live state*
//! the workspace claims about itself.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn xpile_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_xpile"))
}

#[test]
fn c_py_int_arith_has_full_four_stratum_quorum() {
    let root = workspace_root();
    let out = Command::new(xpile_bin())
        .args([
            "quorum",
            "--json",
            "--contracts-dir",
            root.join("contracts").to_str().unwrap(),
            "--fixtures-dir",
            root.join("crates/xpile/tests/fixtures").to_str().unwrap(),
            "--roadmap",
            root.join("docs/roadmaps/roadmap.yaml").to_str().unwrap(),
        ])
        .output()
        .expect("run xpile quorum");
    assert!(
        out.status.success(),
        "xpile quorum failed:\n  stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Hand-parse: locate the C-PY-INT-ARITH entry's status field.
    let id_marker = "\"id\":\"C-PY-INT-ARITH\"";
    let idx = stdout
        .find(id_marker)
        .unwrap_or_else(|| panic!("expected C-PY-INT-ARITH in quorum JSON:\n{stdout}"));
    let tail = &stdout[idx..];
    // Extract each field by scanning forward from the id marker.
    let read_field = |name: &str| -> &str {
        let key = format!("\"{name}\":");
        let kidx = tail.find(&key).expect("missing field");
        let after = &tail[kidx + key.len()..];
        let end = after.find([',', '}']).expect("delimiter");
        after[..end].trim().trim_matches('"')
    };
    let semantic: u64 = read_field("semantic").parse().unwrap();
    let symbolic: u64 = read_field("symbolic").parse().unwrap();
    let runtime: u64 = read_field("runtime").parse().unwrap();
    let extrinsic: u64 = read_field("extrinsic").parse().unwrap();
    let status = read_field("status");

    assert!(semantic >= 1, "expected Semantic ≥1, got {semantic}");
    assert!(symbolic >= 1, "expected Symbolic ≥1, got {symbolic}");
    assert!(runtime >= 1, "expected Runtime ≥1, got {runtime}");
    assert!(extrinsic >= 1, "expected Extrinsic ≥1, got {extrinsic}");
    assert_eq!(
        status, "QUORUM",
        "C-PY-INT-ARITH should have full quorum at v0.1.0 \
         (Sem={semantic}, Sym={symbolic}, Run={runtime}, Ext={extrinsic})"
    );
}

#[test]
fn quorum_reporter_walks_all_contract_yamls() {
    // Counts the discovered contracts and asserts the number matches
    // what's in contracts/. Catches the regression where the reporter
    // silently misses YAMLs (wrong extension filter, dir not walked, etc.).
    let root = workspace_root();
    let contracts_dir = root.join("contracts");
    let yaml_count = std::fs::read_dir(&contracts_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("yaml"))
        .count();
    assert!(
        yaml_count >= 11,
        "expected at least 11 contracts/*.yaml, found {yaml_count}"
    );

    let out = Command::new(xpile_bin())
        .args([
            "quorum",
            "--json",
            "--contracts-dir",
            contracts_dir.to_str().unwrap(),
            "--fixtures-dir",
            root.join("crates/xpile/tests/fixtures").to_str().unwrap(),
            "--roadmap",
            root.join("docs/roadmaps/roadmap.yaml").to_str().unwrap(),
        ])
        .output()
        .expect("run xpile quorum");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Count `"id":"` occurrences in the JSON — one per row.
    let n_rows = stdout.matches("\"id\":\"").count();
    assert_eq!(
        n_rows, yaml_count,
        "quorum reporter saw {n_rows} contracts; filesystem has {yaml_count}"
    );
}
