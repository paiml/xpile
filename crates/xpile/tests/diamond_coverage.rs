//! Diamond-tier coverage gate (PMAT-251, grandfathered at PMAT-475).
//!
//! Asserts, over the live `xpile diamond --json` report:
//!   * Every contract has at least 1 wired Diamond equation. This is the one
//!     genuinely universal claim in this file.
//!   * The GRANDFATHERED pre-R6 cohort (`GRANDFATHERED_DEPTH13`) still meets
//!     each floor from depth-2 up to depth-13. A contract outside that named
//!     set joins at depth-1+ and is deliberately not checked against them.
//!
//! PMAT-1448: this header used to describe the depth-2/3/4 gates as claims
//! about EVERY contract — "Every contract has at least 2 wired Diamond
//! equations (depth-2 UNIVERSAL)". PMAT-475 had replaced those aggregate
//! `depth_N_plus == contracts_total` checks with the grandfathered floor
//! precisely so a new contract joining at depth-1+ would not trip them, and
//! `assert_grandfathered_floor` says so 130 lines below — while the header
//! went on asserting the property that had been deliberately given up.
//! Measured on the tree that still carried it: 14 of 35 contracts were at
//! depth-2+, so the second bullet described 40% of the corpus as all of it.
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

/// PMAT-475 (R6): the contracts that reached **depth-13 UNIVERSAL** before the
/// gate was grandfathered (the depth-13 broadening completed at PMAT-442). These
/// 13 are held to the *full* depth they earned at each gate below; a NEW contract
/// (R6 onward) joins at **depth-1+** and is exempt from depths 2..13 — that is the
/// whole point of R6's gate change (adding a 14th contract must NOT cost 13
/// Diamond theorems). Removing/renaming a grandfathered contract is caught by
/// [`grandfathered_failures`] (the id goes missing → failure).
const GRANDFATHERED_DEPTH13: [&str; 13] = [
    "C-PY-INT-ARITH",
    "C-COMPILE-RUST-TO-PTX-MMA",
    "C-BASHRS-POSIX-IDEMPOTENCE",
    "C-FFI-CPYTHON-EXT",
    "C-NOTATION-LATEX-MATH-TO-EQUATION",
    "C-XLATE-LEAN-TO-RUST",
    "C-XLATE-PY-LIST-TO-VEC",
    "C-XLATE-PY-STR-TO-RUST-STRING",
    "C-XLATE-RUST-FN-TO-LEAN-THM",
    "C-XPILE-BACKEND-TRAIT",
    "C-XPILE-CONTRACT-BACKEND-TRAIT",
    "C-XPILE-CONTRACT-FRONTEND-TRAIT",
    "C-XPILE-FRONTEND-TRAIT",
];

/// Parse the per-contract `"contracts":[{"id":…,"depth":"depth-N+"}]` array into
/// `id → depth` integers (the `depth` string is `"depth-N"` / `"depth-N+"`).
fn contract_depths(json: &str) -> std::collections::HashMap<String, u64> {
    let parsed: serde_json::Value =
        serde_json::from_str(json).expect("diamond --json is valid JSON");
    let mut out = std::collections::HashMap::new();
    for c in parsed["contracts"]
        .as_array()
        .expect("`contracts` array in diamond --json")
    {
        let id = c["id"]
            .as_str()
            .expect("contract id is a string")
            .to_string();
        let depth_str = c["depth"].as_str().expect("contract depth is a string");
        // "depth-13+" / "depth-21" → 13 / 21.
        let n: u64 = depth_str
            .trim_start_matches("depth-")
            .trim_end_matches('+')
            .parse()
            .unwrap_or_else(|_| panic!("unparseable depth {depth_str:?} for {id}"));
        out.insert(id, n);
    }
    out
}

/// PMAT-475 (R6): the grandfathered contracts that FAIL the depth-`floor`
/// requirement — either missing from the report (removed/renamed) or below the
/// floor. An empty result means the grandfathered set still meets the floor;
/// non-grandfathered (new) contracts are intentionally NOT checked here (they
/// join at depth-1+, enforced only by the depth-1 universal gate). Pure function
/// of the JSON so the unit tests can exercise it with synthetic reports.
fn grandfathered_failures(json: &str, floor: u64) -> Vec<String> {
    let depths = contract_depths(json);
    let mut fails = Vec::new();
    for id in GRANDFATHERED_DEPTH13 {
        match depths.get(id) {
            None => fails.push(format!("{id} (missing from report)")),
            Some(&d) if d < floor => {
                fails.push(format!("{id} (depth-{d} < required depth-{floor})"))
            }
            Some(_) => {}
        }
    }
    fails
}

/// PMAT-475 (R6): assert the grandfathered depth-13 set still meets `floor`.
/// Shared by the depth-2..13 gates below; replaces the old
/// `depth_N_plus == contracts_total` aggregate check so a new contract joining
/// at depth-1+ does not trip the deep gates.
fn assert_grandfathered_floor(json: &str, floor: u64, milestone: &str) {
    let fails = grandfathered_failures(json, floor);
    assert!(
        fails.is_empty(),
        "Diamond depth-{floor} UNIVERSAL milestone ({milestone}): every grandfathered \
         (pre-R6) contract must be at depth-{floor}+, but these are not: {fails:?}\n{json}"
    );
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
    // PMAT-475 (R6): grandfathered to the depth-13 cohort. The 13 pre-R6
    // contracts must each still have ≥2 distinct Diamond equations; NEW contracts
    // join at depth-1+ and are exempt from depths 2..13 (no treadmill).
    assert_grandfathered_floor(&json, 2, "PMAT-228..250 / PMAT-454");
}

#[test]
fn substrate_diamond_depth_3_universal() {
    let json = run_diamond_json();
    // PMAT-475 (R6): grandfathered — the 13 pre-R6 contracts at depth-3+.
    assert_grandfathered_floor(&json, 3, "PMAT-336 / PMAT-454");
}

#[test]
fn substrate_diamond_depth_4_universal() {
    let json = run_diamond_json();
    // PMAT-475 (R6): grandfathered — the 13 pre-R6 contracts at depth-4+.
    assert_grandfathered_floor(&json, 4, "PMAT-344 / PMAT-454");
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
    // PMAT-475 (R6): grandfathered — the 13 pre-R6 contracts at depth-5+.
    assert_grandfathered_floor(&json, 5, "PMAT-354 / PMAT-454");
}

#[test]
fn substrate_diamond_depth_6_opened() {
    let json = run_diamond_json();
    // PMAT-475 (R6): grandfathered — the 13 pre-R6 contracts at depth-6+ (the
    // old interim `>= 9` relaxation is subsumed; all 13 reached depth-6 at PMAT-365).
    assert_grandfathered_floor(&json, 6, "PMAT-365");
}

#[test]
fn substrate_diamond_depth_7_opened() {
    let json = run_diamond_json();
    // PMAT-475 (R6): grandfathered — the 13 pre-R6 contracts at depth-7+.
    assert_grandfathered_floor(&json, 7, "PMAT-376");
}

#[test]
fn substrate_diamond_depth_8_opened() {
    let json = run_diamond_json();
    // PMAT-475 (R6): grandfathered — the 13 pre-R6 contracts at depth-8+.
    assert_grandfathered_floor(&json, 8, "PMAT-387");
}

#[test]
fn substrate_diamond_depth_9_opened() {
    let json = run_diamond_json();
    // PMAT-475 (R6): grandfathered — the 13 pre-R6 contracts at depth-9+.
    assert_grandfathered_floor(&json, 9, "PMAT-398 / PMAT-454");
}

#[test]
fn substrate_diamond_depth_10_opened() {
    let json = run_diamond_json();
    // PMAT-475 (R6): grandfathered — the 13 pre-R6 contracts at depth-10+.
    assert_grandfathered_floor(&json, 10, "PMAT-409 / PMAT-454");
}

#[test]
fn substrate_diamond_depth_11_opened() {
    let json = run_diamond_json();
    // PMAT-475 (R6): grandfathered — the 13 pre-R6 contracts at depth-11+.
    assert_grandfathered_floor(&json, 11, "PMAT-420 / PMAT-454");
}

#[test]
fn substrate_diamond_depth_12_opened() {
    let json = run_diamond_json();
    // PMAT-475 (R6): grandfathered — the 13 pre-R6 contracts at depth-12+.
    assert_grandfathered_floor(&json, 12, "PMAT-431 / PMAT-454");
}

#[test]
fn substrate_diamond_depth_13_opened() {
    let json = run_diamond_json();
    // PMAT-475 (R6): grandfathered — the 13 pre-R6 contracts at depth-13+. This is
    // the depth-13 UNIVERSAL floor of the *grandfathered cohort* (PMAT-442); a NEW
    // contract joins at depth-1+ and is exempt (the R6 anti-treadmill change). The
    // `>= 14` "across-layers" gates below are unaffected (they count frontier
    // contracts, not a universal floor).
    assert_grandfathered_floor(&json, 13, "PMAT-442 / PMAT-454");
}

#[test]
fn substrate_diamond_depth_14_opened() {
    let json = run_diamond_json();
    let depth_14_plus = read_aggregate_field(&json, "depth_14_plus");
    // PMAT-310 opened depth-14 on C-PY-INT-ARITH (Layer 1): NAT-CAST RING HOMOMORPHISM.
    // PMAT-311 extended to C-COMPILE-RUST-TO-PTX-MMA (Layer 5): SUBTYPE EXTENSIONALITY.
    // PMAT-312 took PyIntArith to depth-15 (still ≥2 at depth-14+).
    // Gate asserts depth-14 ACROSS LAYERS (≥2 contracts at depth-14+).
    assert!(
        depth_14_plus >= 2,
        "Diamond depth-14 ACROSS LAYERS milestone (PMAT-310, PMAT-311): \
         expected ≥2 contracts at depth-14+ (Layer 1 + Layer 5), got {depth_14_plus}.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_15_opened() {
    let json = run_diamond_json();
    let depth_15_plus = read_aggregate_field(&json, "depth_15_plus");
    // PMAT-312 opened depth-15 on C-PY-INT-ARITH (Layer 1): INT-EMOD QUOTIENT HOM.
    // PMAT-313 extended to C-COMPILE-RUST-TO-PTX-MMA (Layer 5): NAT-MOD QUOTIENT HOM.
    // PMAT-315 took PyIntArith to depth-16 (still ≥2 at depth-15+).
    // Gate asserts depth-15 ACROSS LAYERS (≥2 contracts at depth-15+).
    assert!(
        depth_15_plus >= 2,
        "Diamond depth-15 ACROSS LAYERS milestone (PMAT-312, PMAT-313): \
         expected ≥2 contracts at depth-15+ (Layer 1 + Layer 5), got {depth_15_plus}.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_16_opened() {
    let json = run_diamond_json();
    let depth_16_plus = read_aggregate_field(&json, "depth_16_plus");
    // PMAT-315 opened depth-16 on C-PY-INT-ARITH (Layer 1): GCD MONOID + BÉZOUT.
    // PMAT-316 extended to C-COMPILE-RUST-TO-PTX-MMA (Layer 5): NAT GCD MONOID.
    // PMAT-317 took PyIntArith to depth-17 (still ≥2 at depth-16+).
    // Gate asserts depth-16 ACROSS LAYERS (≥2 contracts at depth-16+).
    assert!(
        depth_16_plus >= 2,
        "Diamond depth-16 ACROSS LAYERS milestone (PMAT-315, PMAT-316): \
         expected ≥2 contracts at depth-16+ (Layer 1 + Layer 5), got {depth_16_plus}.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_17_opened() {
    let json = run_diamond_json();
    let depth_17_plus = read_aggregate_field(&json, "depth_17_plus");
    // PMAT-317 opened depth-17 on C-PY-INT-ARITH (Layer 1): UNIT GROUP `{1,-1}≅Z/2Z`.
    // PMAT-318 extended to C-COMPILE-RUST-TO-PTX-MMA (Layer 5): NAT POWER-MONOID.
    // PMAT-320 took PyIntArith to depth-18 (still ≥2 at depth-17+).
    // Gate asserts depth-17 ACROSS LAYERS (≥2 contracts at depth-17+).
    assert!(
        depth_17_plus >= 2,
        "Diamond depth-17 ACROSS LAYERS milestone (PMAT-317, PMAT-318): \
         expected ≥2 contracts at depth-17+ (Layer 1 + Layer 5), got {depth_17_plus}.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_18_opened() {
    let json = run_diamond_json();
    let depth_18_plus = read_aggregate_field(&json, "depth_18_plus");
    // PMAT-320 opened depth-18 on C-PY-INT-ARITH (Layer 1): SIGN FUNCTION monoid hom.
    // PMAT-321 extended to C-COMPILE-RUST-TO-PTX-MMA (Layer 5): NAT INTEGRAL DOMAIN.
    // PMAT-322 took PyIntArith to depth-19 (still ≥2 at depth-18+).
    // Gate asserts depth-18 ACROSS LAYERS (≥2 contracts at depth-18+).
    assert!(
        depth_18_plus >= 2,
        "Diamond depth-18 ACROSS LAYERS milestone (PMAT-320, PMAT-321): \
         expected ≥2 contracts at depth-18+ (Layer 1 + Layer 5), got {depth_18_plus}.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_19_opened() {
    let json = run_diamond_json();
    let depth_19_plus = read_aggregate_field(&json, "depth_19_plus");
    // PMAT-322 opened depth-19 on C-PY-INT-ARITH (Layer 1): NEGATION-ORDER COMPAT.
    // PMAT-323 extended to C-COMPILE-RUST-TO-PTX-MMA (Layer 5): NAT TRUNCATED SUB.
    // PMAT-325 took PyIntArith to depth-20 (still ≥2 at depth-19+).
    // Gate asserts depth-19 ACROSS LAYERS (≥2 contracts at depth-19+).
    assert!(
        depth_19_plus >= 2,
        "Diamond depth-19 ACROSS LAYERS milestone (PMAT-322, PMAT-323): \
         expected ≥2 contracts at depth-19+ (Layer 1 + Layer 5), got {depth_19_plus}.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_20_opened() {
    let json = run_diamond_json();
    let depth_20_plus = read_aggregate_field(&json, "depth_20_plus");
    // PMAT-325 opened depth-20 on C-PY-INT-ARITH (Layer 1): Int.toNat partial inverse.
    // PMAT-326 extended to C-COMPILE-RUST-TO-PTX-MMA (Layer 5): Nat power monotonicity.
    // PMAT-327 took PyIntArith to depth-21 (still ≥2 at depth-20+).
    // Gate asserts depth-20 ACROSS LAYERS (≥2 contracts at depth-20+).
    assert!(
        depth_20_plus >= 2,
        "Diamond depth-20 ACROSS LAYERS milestone (PMAT-325, PMAT-326): \
         expected ≥2 contracts at depth-20+ (Layer 1 + Layer 5), got {depth_20_plus}.\n{json}"
    );
}

#[test]
fn substrate_diamond_depth_21_opened() {
    let json = run_diamond_json();
    let depth_21_plus = read_aggregate_field(&json, "depth_21_plus");
    // PMAT-327 opened depth-21 on C-PY-INT-ARITH: NAT-CAST ORDER EMBEDDING
    // (Nat.cast preserves ≤, <, =; complement to PMAT-310 ring-hom direction).
    // Together with PMAT-310, captures Nat.cast as an OrderRingHom.
    assert!(
        depth_21_plus >= 1,
        "Diamond depth-21 milestone (PMAT-327): expected ≥1 contract at depth-21+, \
         got {depth_21_plus}.\n{json}"
    );
}

// ── PMAT-475 (R6): grandfather-gate unit tests ──────────────────────────────
// These exercise the grandfather logic against SYNTHETIC reports, so the
// "new contract joins at depth-1+" path and the regression-catch are proven
// directly — without yet authoring a 14th contract YAML (that is the next R6
// sub-slice). They are pure-function tests: no `xpile` binary invocation.

/// Build a synthetic `diamond --json` report body from `(id, depth)` pairs.
fn synthetic_report(contracts: &[(&str, u64)]) -> String {
    let items: Vec<String> = contracts
        .iter()
        .map(|(id, d)| {
            format!("{{\"id\":\"{id}\",\"diamond_count\":{d},\"depth\":\"depth-{d}+\"}}")
        })
        .collect();
    format!(
        "{{\"contracts\":[{}],\"contracts_total\":{}}}",
        items.join(","),
        contracts.len()
    )
}

#[test]
fn r6_grandfather_allows_new_contract_at_depth_1() {
    // The 13 grandfathered contracts at depth-13, plus a brand-new R6 contract
    // at depth-1. The new contract must NOT trip any of the depth-2..13 gates.
    let mut cs: Vec<(&str, u64)> = GRANDFATHERED_DEPTH13.iter().map(|id| (*id, 13)).collect();
    cs.push(("C-C-INT-ARITH", 1)); // hypothetical R6 newcomer at the depth-1 floor
    let json = synthetic_report(&cs);
    for floor in 2..=13 {
        assert!(
            grandfathered_failures(&json, floor).is_empty(),
            "a new contract at depth-1 must not trip the grandfathered depth-{floor} gate, \
             but got failures: {:?}",
            grandfathered_failures(&json, floor)
        );
    }
}

#[test]
fn r6_grandfather_catches_regressed_grandfathered_contract() {
    // A grandfathered contract slipping below the depth-13 floor must fail —
    // the substrate guarantee for the pre-R6 cohort is NOT weakened.
    let mut cs: Vec<(&str, u64)> = GRANDFATHERED_DEPTH13.iter().map(|id| (*id, 13)).collect();
    cs[0].1 = 12; // C-PY-INT-ARITH regressed depth-13 → depth-12
    let json = synthetic_report(&cs);
    let fails = grandfathered_failures(&json, 13);
    assert_eq!(
        fails.len(),
        1,
        "exactly one regressed contract expected: {fails:?}"
    );
    assert!(fails[0].contains(GRANDFATHERED_DEPTH13[0]));
}

#[test]
fn r6_grandfather_catches_removed_grandfathered_contract() {
    // Dropping a grandfathered contract from the report (removed/renamed) must
    // fail loudly — the cohort membership is part of the guarantee.
    let cs: Vec<(&str, u64)> = GRANDFATHERED_DEPTH13
        .iter()
        .skip(1)
        .map(|id| (*id, 13))
        .collect();
    let json = synthetic_report(&cs);
    let fails = grandfathered_failures(&json, 13);
    assert!(
        fails.iter().any(|f| f.contains("missing")),
        "a removed grandfathered contract must be reported missing: {fails:?}"
    );
}

/// The live report still contains every grandfathered contract at the full
/// depth-13 floor — i.e. the R6 change is behavior-preserving at 13 contracts.
#[test]
fn r6_grandfather_live_report_meets_depth_13() {
    let json = run_diamond_json();
    assert!(
        grandfathered_failures(&json, 13).is_empty(),
        "all 13 grandfathered contracts must still be at depth-13+ in the live report"
    );
}
