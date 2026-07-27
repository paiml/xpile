//! XPILE-REPORTERVAC-001 — the three contract reporters never report a
//! stratum they did not read (PMAT-1386).
//!
//! `xpile quorum`, `xpile diamond` and `xpile attestations` are the substrate's
//! own §14.4 scoreboards. They disagreed about what an unmeasurable input is.
//! Given a directory holding no contract YAML, `attestations` refused; `quorum`
//! printed
//!
//!     totals: 0 QUORUM, 0 PARTIAL, 0 UNVERIFIED (0 contracts total)   rc=0
//!     {"contracts":[]}                                               rc=0
//!
//! and `diamond` printed `0 Diamond theorems across 0 contracts`, both at
//! exit 0. A consumer reading `unverified == 0` sees a clean board over a
//! universe that was never discovered.
//!
//! Sharper, and the reason this witness exists at all: `quorum` read its
//! `--roadmap` with `.unwrap_or_default()`. A path that does not exist scored
//! the ENTIRE Extrinsic stratum at 0 for EVERY contract, silently, at exit 0 —
//! measured on the live tree, 702 mentions collapsed to 0 and 10 of 35
//! contracts fell QUORUM -> PARTIAL. PMAT-1367 wrote a stderr notice for
//! exactly this trap on `--witness-dir`; the sibling path arguments in the same
//! function had none.
//!
//! The properties held here:
//!
//!   (1) an empty contract universe REFUSES on all three reporters — they
//!       agree, rather than two of them scoring it;
//!   (2) a `--roadmap` that does not exist REFUSES rather than reading as a
//!       measured zero — it is the sole source of a whole stratum;
//!   (3) a `--fixtures-dir` that does not exist is ANNOUNCED on stderr and is
//!       non-fatal (it is half of a union, and PMAT-1367 already settled that
//!       posture for the other half), and the loss it announces is REAL.
//!
//! Every property carries a vacuity guard on BOTH sides: the measurable
//! control must actually measure something (a non-zero Extrinsic total, a
//! non-zero Diamond total, a strictly larger Runtime total), so a future
//! change that makes the reporters refuse EVERYTHING cannot pass this file.
//!
//! No external toolchain is involved — the subject is the shipped `xpile`
//! binary — so this witness has no skip path and always executes.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_xpile"))
}

/// Workspace root. An integration test's CWD is the PACKAGE root
/// (`crates/xpile`), not the workspace root — the exact confusion that left
/// four `audit` tests reading a nonexistent fixture path until PMAT-1385.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn run(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(bin())
        .args(args)
        .output()
        .expect("spawn xpile");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A per-CALL unique temp dir — a per-TEST dir would be shared by the probes
/// inside it and one probe's files would leak into the next probe's scan.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xpile-reportervac-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Sum an integer field over the `contracts` array of a reporter's `--json`.
/// Hand-rolled to match the reporters' own hand-rolled emitters — pulling in a
/// JSON dep to read a flat array of flat objects would be the tail wagging.
fn sum_field(json: &str, field: &str) -> i64 {
    let needle = format!("\"{field}\":");
    let mut total = 0i64;
    let mut rest = json;
    while let Some(i) = rest.find(&needle) {
        rest = &rest[i + needle.len()..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '-')
            .unwrap_or(rest.len());
        total += rest[..end].parse::<i64>().unwrap_or_else(|e| {
            panic!(
                "field {field} did not parse as an integer ({e}); tail was {:.60}",
                rest
            )
        });
        rest = &rest[end..];
    }
    total
}

fn count_status(json: &str, status: &str) -> usize {
    json.matches(&format!("\"status\":\"{status}\"")).count()
}

// ── (1) an empty contract universe refuses on ALL THREE reporters ──────────

#[test]
fn an_empty_contract_universe_refuses_on_every_reporter() {
    let dir = scratch("empty-universe");
    // Not merely an empty dir: a dir with a real file that carries no
    // `metadata.id:`, so the walk RUNS and finds nothing, rather than the
    // walk being skipped. Both spellings collapse to zero rows.
    std::fs::write(dir.join("notes.txt"), "hello\n").expect("write notes");
    std::fs::write(dir.join("nolabel.yaml"), "foo: bar\nbaz: 1\n").expect("write yaml");
    let d = dir.to_str().unwrap();

    for (name, args) in [
        ("quorum", vec!["quorum", "--contracts-dir", d]),
        ("diamond", vec!["diamond", "--contracts-dir", d]),
        ("attestations", vec!["attestations", "--contracts-dir", d]),
    ] {
        let (ok, stdout, stderr) = run(&args);
        assert!(
            !ok,
            "`xpile {name}` scored a contract universe it never discovered \
             (exit 0). stdout was:\n{stdout}"
        );
        assert!(
            stderr.contains("no contract IDs discovered"),
            "`xpile {name}` must say WHICH input was empty; stderr was:\n{stderr}"
        );
        assert!(
            !stdout.contains("0 contracts total") && !stdout.contains("across 0 contracts"),
            "`xpile {name}` printed a zero-row scoreboard before refusing; \
             stdout was:\n{stdout}"
        );
    }
}

#[test]
fn the_same_three_reporters_still_score_the_live_contract_corpus() {
    // Vacuity guard for the test above: the refusal must be specific to an
    // empty universe, not a reporter that now refuses everything.
    let root = workspace_root();
    let contracts = root.join("contracts");
    let contracts = contracts.to_str().unwrap();
    let roadmap = root.join("docs/roadmaps/roadmap.yaml");
    let roadmap = roadmap.to_str().unwrap();
    let fixtures = root.join("crates/xpile/tests/fixtures");
    let fixtures = fixtures.to_str().unwrap();

    let (ok, quorum_json, stderr) = run(&[
        "quorum",
        "--contracts-dir",
        contracts,
        "--fixtures-dir",
        fixtures,
        "--roadmap",
        roadmap,
        "--json",
    ]);
    assert!(ok, "live quorum must succeed; stderr was:\n{stderr}");
    let rows = quorum_json.matches("\"id\":").count();
    assert!(
        rows >= 12,
        "expected the live contract corpus (≥12 contracts), got {rows} rows"
    );
    assert!(
        sum_field(&quorum_json, "extrinsic") > 0,
        "the Extrinsic stratum measured 0 over the LIVE roadmap — the control \
         is vacuous and every other assertion in this file is worthless"
    );
    assert!(
        sum_field(&quorum_json, "runtime") > 0,
        "the Runtime stratum measured 0 over the live fixtures + witnesses"
    );

    let (ok, diamond_json, stderr) = run(&["diamond", "--contracts-dir", contracts, "--json"]);
    assert!(ok, "live diamond must succeed; stderr was:\n{stderr}");
    assert!(
        sum_field(&diamond_json, "diamond_count") > 0,
        "the Diamond reporter measured 0 theorems over the live corpus"
    );

    let (ok, attest_stdout, stderr) = run(&[
        "attestations",
        "--contracts-dir",
        contracts,
        "--roadmap",
        roadmap,
    ]);
    assert!(ok, "live attestations must succeed; stderr was:\n{stderr}");
    assert!(
        attest_stdout.contains("mentions across"),
        "the attestation reporter produced no mention rows; stdout was:\n{attest_stdout}"
    );
}

// ── (2) a missing --roadmap refuses rather than scoring Extrinsic at 0 ─────

#[test]
fn a_missing_roadmap_refuses_rather_than_reading_as_a_measured_zero() {
    let root = workspace_root();
    let contracts = root.join("contracts");
    let contracts = contracts.to_str().unwrap();
    let fixtures = root.join("crates/xpile/tests/fixtures");
    let fixtures = fixtures.to_str().unwrap();
    let missing = scratch("missing-roadmap").join("does-not-exist.yaml");
    let missing = missing.to_str().unwrap().to_string();

    let (ok, stdout, stderr) = run(&[
        "quorum",
        "--contracts-dir",
        contracts,
        "--fixtures-dir",
        fixtures,
        "--roadmap",
        &missing,
        "--json",
    ]);
    assert!(
        !ok,
        "a nonexistent --roadmap scored the whole Extrinsic stratum at 0 and \
         exited 0. stdout was:\n{stdout}"
    );
    assert!(
        stderr.contains("read roadmap") && stderr.contains("does-not-exist.yaml"),
        "the refusal must name the roadmap path it could not read; \
         stderr was:\n{stderr}"
    );
    assert!(
        !stdout.contains("\"extrinsic\":"),
        "no Extrinsic vote may be reported for a roadmap that was never read; \
         stdout was:\n{stdout}"
    );
}

#[test]
fn attestations_agrees_that_a_missing_roadmap_is_an_input_error() {
    // `attestations` has always refused this; the assertion pins the agreement
    // so a future edit cannot re-introduce the asymmetry from the other side.
    let root = workspace_root();
    let contracts = root.join("contracts");
    let missing = scratch("missing-roadmap-attest").join("does-not-exist.yaml");
    let (ok, _stdout, stderr) = run(&[
        "attestations",
        "--contracts-dir",
        contracts.to_str().unwrap(),
        "--roadmap",
        missing.to_str().unwrap(),
    ]);
    assert!(!ok, "attestations must refuse a roadmap it cannot read");
    assert!(stderr.contains("read roadmap"), "stderr was:\n{stderr}");
}

// ── (3) a missing --fixtures-dir is announced, non-fatal, and the loss real ─

#[test]
fn a_missing_fixtures_dir_is_announced_on_stderr_and_is_not_fatal() {
    // Same posture PMAT-1367 settled for `--witness-dir`: the Runtime stratum
    // is a UNION of two passes, either half may legitimately be absent, so the
    // command still reports — but it must SAY that a source contributed 0
    // rather than folding the absence into the score.
    let root = workspace_root();
    let contracts = root.join("contracts");
    let contracts = contracts.to_str().unwrap();
    let witnesses = root.join("crates/xpile-wasm-codegen/tests");
    let witnesses = witnesses.to_str().unwrap();
    let roadmap = root.join("docs/roadmaps/roadmap.yaml");
    let roadmap = roadmap.to_str().unwrap();
    let real_fixtures = root.join("crates/xpile/tests/fixtures");
    let real_fixtures = real_fixtures.to_str().unwrap();
    let absent = scratch("missing-fixtures").join("no-such-fixtures");
    let absent = absent.to_str().unwrap().to_string();

    let base = |fixtures: &str| {
        run(&[
            "quorum",
            "--contracts-dir",
            contracts,
            "--fixtures-dir",
            fixtures,
            "--witness-dir",
            witnesses,
            "--roadmap",
            roadmap,
            "--json",
        ])
    };

    let (ok_present, json_present, stderr_present) = base(real_fixtures);
    assert!(
        ok_present,
        "control run failed; stderr was:\n{stderr_present}"
    );
    assert!(
        !stderr_present.contains("is not a directory"),
        "the control run must produce NO notice; stderr was:\n{stderr_present}"
    );

    let (ok_absent, json_absent, stderr_absent) = base(&absent);
    assert!(
        ok_absent,
        "a missing --fixtures-dir must not be fatal; stderr was:\n{stderr_absent}"
    );
    assert!(
        stderr_absent.contains("no-such-fixtures") && stderr_absent.contains("0 Runtime votes"),
        "expected a one-line notice naming the missing fixtures dir; \
         stderr was:\n{stderr_absent}"
    );
    assert_eq!(
        stderr_absent.matches("is not a directory").count(),
        1,
        "the notice must fire once, not once per contract; stderr was:\n{stderr_absent}"
    );

    // The loss the notice announces must be REAL. Without this the notice
    // could fire over a fixtures dir that contributed nothing and the whole
    // property would be decoration.
    let runtime_present = sum_field(&json_present, "runtime");
    let runtime_absent = sum_field(&json_absent, "runtime");
    assert!(
        runtime_absent < runtime_present,
        "the missing fixtures dir announced a loss it did not cause: \
         Runtime {runtime_present} -> {runtime_absent}"
    );
    assert!(
        runtime_absent > 0,
        "the witness-dir half of the Runtime union scored 0, so this probe is \
         not measuring the union at all"
    );
}

// ── the shape that made the roadmap trap dangerous: a silent status flip ───

#[test]
fn an_unread_stratum_can_change_a_status_which_is_why_it_must_refuse() {
    // Documents the CONSEQUENCE, not just the mechanism: with the Extrinsic
    // stratum zeroed, contracts sitting at exactly 3 represented strata fall
    // from QUORUM to PARTIAL. Before PMAT-1386 that flip was reachable at
    // exit 0 from a single mistyped path. Here it is reached deliberately, by
    // pointing at a roadmap that EXISTS and mentions nothing — an honest zero,
    // which is exactly the case that must stay distinguishable from the
    // unreadable one asserted above.
    let root = workspace_root();
    let contracts = root.join("contracts");
    let contracts = contracts.to_str().unwrap();
    let fixtures = root.join("crates/xpile/tests/fixtures");
    let fixtures = fixtures.to_str().unwrap();
    let real_roadmap = root.join("docs/roadmaps/roadmap.yaml");
    let real_roadmap = real_roadmap.to_str().unwrap();
    let empty_roadmap = scratch("empty-roadmap").join("roadmap.yaml");
    std::fs::write(&empty_roadmap, "roadmap: []\n").expect("write empty roadmap");
    let empty_roadmap = empty_roadmap.to_str().unwrap().to_string();

    let quorum_with = |roadmap: &str| {
        let (ok, stdout, stderr) = run(&[
            "quorum",
            "--contracts-dir",
            contracts,
            "--fixtures-dir",
            fixtures,
            "--roadmap",
            roadmap,
            "--json",
        ]);
        assert!(ok, "quorum failed for {roadmap}; stderr was:\n{stderr}");
        stdout
    };

    let live = quorum_with(real_roadmap);
    let empty = quorum_with(&empty_roadmap);

    assert!(
        sum_field(&live, "extrinsic") > 0,
        "vacuity guard: the live roadmap contributed no Extrinsic votes"
    );
    assert_eq!(
        sum_field(&empty, "extrinsic"),
        0,
        "a roadmap mentioning no contract must contribute 0 Extrinsic votes"
    );
    assert!(
        count_status(&empty, "QUORUM") < count_status(&live, "QUORUM"),
        "zeroing Extrinsic did not move any contract off QUORUM, so the \
         stratum this reporter reads is not load-bearing: live QUORUM {} vs \
         zeroed {}",
        count_status(&live, "QUORUM"),
        count_status(&empty, "QUORUM")
    );
    // An EXISTING roadmap that mentions nothing is a legitimate outcome and
    // still reports. Only the UNREADABLE path refuses. That distinction is
    // the whole fix.
    assert!(empty.contains("\"extrinsic\":0"));
}
