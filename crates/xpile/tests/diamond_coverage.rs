//! UNIVERSAL Diamond depth-2 gate (PMAT-251).
//!
//! Asserts the live substrate state shipped over PMAT-214..250:
//!   * Every contract has at least 1 wired Diamond equation (depth-1 UNIVERSAL).
//!   * Every contract has at least 2 wired Diamond equations (depth-2 UNIVERSAL).
//!   * At least 5 contracts have ≥3 Diamond equations (depth-3 across layers).
//!   * At least 2 contracts have ≥4 Diamond equations (depth-4 OPENED).
//!
//! Integration counterpart to the unit tests in `diamond_tests` inside
//! the binary crate. The unit tests exercise the depth-label classifier
//! against synthetic inputs; this one exercises the LIVE state the
//! workspace claims about itself.
//!
//! If a future PR weakens Diamond coverage (e.g., removes a `_diamond`
//! equation from any contract YAML), this test fires loudly. Reporter
//! → gate transition for Diamond-tier coverage.

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

fn run_diamond_json() -> String {
    let root = workspace_root();
    let out = Command::new(xpile_bin())
        .args([
            "diamond",
            "--json",
            "--contracts-dir",
            root.join("contracts").to_str().unwrap(),
        ])
        .output()
        .expect("run xpile diamond");
    assert!(
        out.status.success(),
        "xpile diamond failed:\n  stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Read a numeric field from the aggregate trailer of the JSON output.
/// Format: `...],"total_diamonds":N,"contracts_total":M,"depth_1_plus":...`
fn read_aggregate_field(json: &str, name: &str) -> u64 {
    let key = format!("\"{name}\":");
    let idx = json
        .rfind(&key)
        .unwrap_or_else(|| panic!("missing aggregate field {name} in:\n{json}"));
    let after = &json[idx + key.len()..];
    let end = after
        .find([',', '}'])
        .expect("delimiter after aggregate field");
    after[..end].trim().parse().expect("parse aggregate field")
}

#[test]
fn substrate_diamond_depth_1_universal() {
    let json = run_diamond_json();
    let contracts_total = read_aggregate_field(&json, "contracts_total");
    let depth_1_plus = read_aggregate_field(&json, "depth_1_plus");
    assert_eq!(
        depth_1_plus, contracts_total,
        "Diamond depth-1 UNIVERSAL milestone: every contract should have ≥1 Diamond equation, \
         but only {depth_1_plus} of {contracts_total} do.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_2_universal() {
    let json = run_diamond_json();
    let contracts_total = read_aggregate_field(&json, "contracts_total");
    let depth_2_plus = read_aggregate_field(&json, "depth_2_plus");
    assert_eq!(
        depth_2_plus, contracts_total,
        "Diamond depth-2 UNIVERSAL milestone (PMAT-228..250): every contract should have ≥2 \
         distinct Diamond equations, but only {depth_2_plus} of {contracts_total} do.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_3_across_layers() {
    let json = run_diamond_json();
    let depth_3_plus = read_aggregate_field(&json, "depth_3_plus");
    // PMAT-241..245 milestone: one contract per layer at depth-3 = 5.
    assert!(
        depth_3_plus >= 5,
        "Diamond depth-3 across-layers milestone (PMAT-241..245): expected ≥5 contracts at \
         depth-3+, got {depth_3_plus}.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_4_opened() {
    let json = run_diamond_json();
    let depth_4_plus = read_aggregate_field(&json, "depth_4_plus");
    // PMAT-247 (PyIntArith) + PMAT-248 (CompileRustToPtxMma) opened depth-4.
    assert!(
        depth_4_plus >= 2,
        "Diamond depth-4 milestone (PMAT-247, PMAT-248): expected ≥2 contracts at depth-4+, \
         got {depth_4_plus}.\n{json}"
    );
}

#[test]
fn substrate_diamond_aggregate_total_at_least_30() {
    let json = run_diamond_json();
    let total_diamonds = read_aggregate_field(&json, "total_diamonds");
    // Post-PMAT-250 baseline: 31 wired Diamond equations across 12 contracts
    // (12 depth-1 + 12 depth-2 + 5 depth-3 + 2 depth-4 = 31).
    assert!(
        total_diamonds >= 30,
        "expected substrate to have ≥30 wired Diamond equations, got {total_diamonds}.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_5_opened() {
    let json = run_diamond_json();
    let depth_5_plus = read_aggregate_field(&json, "depth_5_plus");
    // PMAT-286 opened depth-5: PyIntArith has 5 Diamond categories
    // (semiring + Euclidean + shift-monoid + power-monoid + bitwise-AND-monoid).
    assert!(
        depth_5_plus >= 1,
        "Diamond depth-5 milestone (PMAT-286): expected ≥1 contract at depth-5+, \
         got {depth_5_plus}.\n{json}"
    );
}
