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
    // PMAT-289 added Bashrs (Layer 2/4 hybrid) to depth-3, total = 6.
    assert!(
        depth_3_plus >= 6,
        "Diamond depth-3 across-layers milestone (PMAT-241..245 + PMAT-289): expected ≥6 \
         contracts at depth-3+, got {depth_3_plus}.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_4_opened() {
    let json = run_diamond_json();
    let depth_4_plus = read_aggregate_field(&json, "depth_4_plus");
    // PMAT-247 (PyIntArith) + PMAT-248 (CompileRustToPtxMma) opened depth-4.
    // PMAT-288 added refcount_inverse_diamond → C-FFI-CPYTHON-EXT joins depth-4.
    // Gate tightened to ≥3 contracts across layers (Layer 1, Layer 4, Layer 5).
    assert!(
        depth_4_plus >= 3,
        "Diamond depth-4 ACROSS LAYERS milestone (PMAT-247, PMAT-248, PMAT-288): \
         expected ≥3 contracts at depth-4+ (Layer 1 + Layer 4 + Layer 5), got {depth_4_plus}.\n{json}"
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
    // PMAT-286 opened depth-5 on C-PY-INT-ARITH (Layer 1):
    //   semiring + Euclidean + shift-monoid + power-monoid + bitwise-AND-monoid.
    // PMAT-287 extended to C-COMPILE-RUST-TO-PTX-MMA (Layer 5):
    //   bounded-monoid + closure + join-lattice + meet-lattice + absorption.
    // Gate now asserts depth-5 ACROSS LAYERS (≥2 contracts on distinct
    // taxonomy layers at depth-5+).
    assert!(
        depth_5_plus >= 2,
        "Diamond depth-5 ACROSS LAYERS milestone (PMAT-286, PMAT-287): \
         expected ≥2 contracts at depth-5+ (Layer 1 + Layer 5), got {depth_5_plus}.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_6_opened() {
    let json = run_diamond_json();
    let depth_6_plus = read_aggregate_field(&json, "depth_6_plus");
    // PMAT-290 opened depth-6 on C-PY-INT-ARITH (Layer 1): negation-involution.
    // PMAT-291 extended to C-COMPILE-RUST-TO-PTX-MMA (Layer 5): distributive lattice.
    // Gate now asserts depth-6 ACROSS LAYERS (≥2 contracts at depth-6+).
    assert!(
        depth_6_plus >= 2,
        "Diamond depth-6 ACROSS LAYERS milestone (PMAT-290, PMAT-291): \
         expected ≥2 contracts at depth-6+ (Layer 1 + Layer 5), got {depth_6_plus}.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_7_opened() {
    let json = run_diamond_json();
    let depth_7_plus = read_aggregate_field(&json, "depth_7_plus");
    // PMAT-292 opened depth-7 on C-PY-INT-ARITH (Layer 1): order-distributive-lattice.
    // PMAT-293 extended to C-COMPILE-RUST-TO-PTX-MMA (Layer 5): bounded lattice with top+bottom.
    // Gate now asserts depth-7 ACROSS LAYERS (≥2 contracts at depth-7+).
    assert!(
        depth_7_plus >= 2,
        "Diamond depth-7 ACROSS LAYERS milestone (PMAT-292, PMAT-293): \
         expected ≥2 contracts at depth-7+ (Layer 1 + Layer 5), got {depth_7_plus}.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_8_opened() {
    let json = run_diamond_json();
    let depth_8_plus = read_aggregate_field(&json, "depth_8_plus");
    // PMAT-294 opened depth-8 on C-PY-INT-ARITH (Layer 1): divisibility-preorder.
    // PMAT-295 extended to C-COMPILE-RUST-TO-PTX-MMA (Layer 5): cancellative monoid.
    // Gate now asserts depth-8 ACROSS LAYERS (≥2 contracts at depth-8+).
    assert!(
        depth_8_plus >= 2,
        "Diamond depth-8 ACROSS LAYERS milestone (PMAT-294, PMAT-295): \
         expected ≥2 contracts at depth-8+ (Layer 1 + Layer 5), got {depth_8_plus}.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_9_opened() {
    let json = run_diamond_json();
    let depth_9_plus = read_aggregate_field(&json, "depth_9_plus");
    // PMAT-298 opened depth-9 on C-PY-INT-ARITH (Layer 1): linear-order trichotomy.
    // PMAT-299 extended to C-COMPILE-RUST-TO-PTX-MMA (Layer 5): ordered monoid.
    // PMAT-300 took PyIntArith to depth-10 (still ≥2 at depth-9+).
    // Gate asserts depth-9 ACROSS LAYERS (≥2 contracts at depth-9+).
    assert!(
        depth_9_plus >= 2,
        "Diamond depth-9 ACROSS LAYERS milestone (PMAT-298, PMAT-299): \
         expected ≥2 contracts at depth-9+ (Layer 1 + Layer 5), got {depth_9_plus}.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_10_opened() {
    let json = run_diamond_json();
    let depth_10_plus = read_aggregate_field(&json, "depth_10_plus");
    // PMAT-300 opened depth-10 on C-PY-INT-ARITH (Layer 1): RING-distributivity.
    // PMAT-301 extended to C-COMPILE-RUST-TO-PTX-MMA (Layer 5): additive-lattice.
    // PMAT-302 took PyIntArith to depth-11 (still ≥2 at depth-10+).
    // Gate asserts depth-10 ACROSS LAYERS (≥2 contracts at depth-10+).
    assert!(
        depth_10_plus >= 2,
        "Diamond depth-10 ACROSS LAYERS milestone (PMAT-300, PMAT-301): \
         expected ≥2 contracts at depth-10+ (Layer 1 + Layer 5), got {depth_10_plus}.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_11_opened() {
    let json = run_diamond_json();
    let depth_11_plus = read_aggregate_field(&json, "depth_11_plus");
    // PMAT-302 opened depth-11 on C-PY-INT-ARITH: INTEGRAL DOMAIN axioms
    // (no zero divisors + multiplicative cancellation + nontrivial identity).
    // The 11th orthogonal category STRENGTHENS PMAT-300 RING with the
    // no-zero-divisors axiom — Z/6Z satisfies all ring axioms but is not
    // an integral domain (2*3=0).
    assert!(
        depth_11_plus >= 1,
        "Diamond depth-11 milestone (PMAT-302): expected ≥1 contract at depth-11+, \
         got {depth_11_plus}.\n{json}"
    );
}
