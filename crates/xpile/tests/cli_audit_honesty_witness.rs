//! XPILE-AUDITHON-001 — `xpile audit` never reports a coverage number it
//! did not measure (PMAT-1385).
//!
//! `xpile audit` is the repo's OWN falsifier reporter: it computes F1
//! (Layer-1 contract-citation coverage) over a corpus, and its `--json`
//! payload is documented as feeding CI dashboards and the `XPILE-SOTA-XXX`
//! dossier. Before PMAT-1385 it answered
//!
//!     coverage (F1)       : 100.0%   [OK]        rc=0
//!     {"…","f1_pct":100.0,"f1_status":"OK","errors":0}
//!
//! for a path THAT DOES NOT EXIST, for a directory holding no source file
//! xpile recognises, and for a corpus where every single file failed to
//! lower. The 100% is a `coverage_pct()` convention for a 0 denominator; the
//! bug is that a corpus which was never measured is indistinguishable, in
//! both output modes and in the exit status, from one measured at ceiling.
//!
//! The property held here is the honest half of that:
//!
//!     the audit reports a NUMERIC F1 only when it measured something
//!     (`functions_requiring_citation > 0`); otherwise it either refuses
//!     (bad path / no sources) or reports the status `VACUOUS`.
//!
//! It is asserted over the WHOLE probe corpus below, including the rows that
//! are expected to be measurable, so a future change that starts reporting a
//! number for an unmeasured corpus is caught rather than going quietly green.
//! Both sides carry a vacuity guard: the measurable row must actually report
//! a non-zero denominator, and the unmeasurable rows must actually reach the
//! reporter (not fail for some unrelated reason).
//!
//! No external toolchain is involved — the subject is the shipped `xpile`
//! binary and the fixtures it writes into a temp dir — so this witness has no
//! skip path and always executes.

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_xpile"))
}

fn run_audit(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(bin())
        .args(["audit"])
        .args(args)
        .output()
        .expect("spawn xpile");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A per-CALL unique temp dir. A per-TEST dir would be shared by the probes
/// inside it and one probe's fixtures would leak into the next probe's scan.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xpile-audithon-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn write(dir: &Path, name: &str, body: &str) {
    std::fs::write(dir.join(name), body).expect("write fixture");
}

/// A function that DOES require a citation (integer arithmetic ⇒
/// `applicable_contracts()` is non-empty), so the F1 denominator is > 0.
const MEASURABLE_PY: &str = "def add(a: int, b: int) -> int:\n    return a + b\n";

/// Parses as Python but REFUSES at lowering (an undeclared, uninferable
/// `self` field). Scanned, never emitted — the "corpus of failures" shape.
const UNLOWERABLE_PY: &str =
    "class Foo:\n    def __init__(self):\n        self.cb = lambda z: z + 1\n";

// ── 1. a path that does not exist must not report a score ─────────────

#[test]
fn audit_refuses_a_nonexistent_path() {
    let missing = std::env::temp_dir().join("xpile-audithon-does-not-exist-ever");
    let _ = std::fs::remove_dir_all(&missing);
    let (ok, stdout, stderr) = run_audit(&[missing.to_str().unwrap()]);
    assert!(
        !ok,
        "audit of a NONEXISTENT path must fail, not report a score:\n{stdout}"
    );
    assert!(
        !stdout.contains("100.0%"),
        "audit of a nonexistent path reported a coverage number:\n{stdout}"
    );
    assert!(
        stderr.contains("does not exist"),
        "the refusal must name the reason:\n{stderr}"
    );
}

// ── 2. a real directory with nothing xpile recognises ─────────────────

#[test]
fn audit_refuses_a_corpus_with_no_recognised_source() {
    let dir = scratch("nosource");
    write(&dir, "notes.txt", "hello\n");
    write(&dir, "README.md", "# hi\n");
    let (ok, stdout, stderr) = run_audit(&[dir.to_str().unwrap()]);
    assert!(
        !ok,
        "audit of a corpus with no source file must fail, not report 100%:\n{stdout}"
    );
    assert!(
        !stdout.contains("100.0%"),
        "audit of an empty corpus reported a coverage number:\n{stdout}"
    );
    assert!(
        stderr.contains("no source file"),
        "the refusal must name the reason:\n{stderr}"
    );

    // The same hole through the single-FILE spelling: `xpile audit foo.txt`
    // pointed at one unrecognised file also scanned nothing.
    let (ok_file, stdout_file, _) = run_audit(&[dir.join("notes.txt").to_str().unwrap()]);
    assert!(
        !ok_file,
        "audit of a single unrecognised FILE must fail too:\n{stdout_file}"
    );
}

// ── 3. a corpus that IS scanned but yields no measurable function ──────

#[test]
fn audit_reports_vacuous_when_every_file_failed_to_lower() {
    let dir = scratch("allfail");
    write(&dir, "bad.py", UNLOWERABLE_PY);

    let (ok, stdout, stderr) = run_audit(&[dir.to_str().unwrap()]);
    // This one is a measurement OUTCOME, not an input error: the corpus is
    // real and was scanned. `audit` stays a reporter (exit 0) — but it must
    // not claim a ceiling score for a corpus it never measured.
    assert!(ok, "a real corpus must still be reported:\n{stderr}");
    assert!(
        stdout.contains("files scanned       : 1"),
        "vacuity guard: the file must actually have been scanned:\n{stdout}"
    );
    assert!(
        stdout.contains("VACUOUS"),
        "a corpus where nothing was measured must say so:\n{stdout}"
    );
    assert!(
        !stdout.contains("100.0%"),
        "audit claimed a ceiling score for a corpus it never measured:\n{stdout}"
    );
    assert!(
        stdout.contains("errors (1)"),
        "the lowering failure must be disclosed:\n{stdout}"
    );

    // …and the dashboard payload must say the same thing. A consumer testing
    // `f1_status == "OK"` saw OK for this corpus before PMAT-1385.
    let (ok_j, json, _) = run_audit(&[dir.to_str().unwrap(), "--json"]);
    assert!(ok_j, "json mode must agree with text mode on exit status");
    assert!(
        json.contains("\"f1_status\":\"VACUOUS\""),
        "json must carry the VACUOUS status:\n{json}"
    );
    assert!(
        json.contains("\"f1_pct\":null"),
        "json must not carry a number it did not measure:\n{json}"
    );
    assert!(
        json.contains("\"errors\":1"),
        "json must keep disclosing the failure count:\n{json}"
    );
}

// ── 4. the measurable half — the property has to be able to be TRUE ───

#[test]
fn audit_reports_a_number_only_when_it_measured_one() {
    let dir = scratch("measurable");
    write(&dir, "add.py", MEASURABLE_PY);

    let (ok, stdout, stderr) = run_audit(&[dir.to_str().unwrap()]);
    assert!(ok, "a measurable corpus must be reported:\n{stderr}");
    assert!(
        stdout.contains("100.0%") && stdout.contains("[OK]"),
        "the citation pipeline fires for integer arithmetic:\n{stdout}"
    );
    // Vacuity guard on the OTHER side: the ceiling score above must rest on a
    // non-zero denominator, otherwise this test would pass on the very bug.
    assert!(
        !stdout.contains("require citation    : 0"),
        "vacuity guard: 100% must rest on a non-zero denominator:\n{stdout}"
    );

    let (_, json, _) = run_audit(&[dir.to_str().unwrap(), "--json"]);
    assert!(
        json.contains("\"f1_pct\":100.0") && json.contains("\"f1_status\":\"OK\""),
        "json must carry the measured number:\n{json}"
    );
    assert!(
        !json.contains("\"functions_requiring_citation\":0"),
        "vacuity guard: the json denominator must be non-zero:\n{json}"
    );
}

// ── 5. the detector's own blind spot: raw identifiers ─────────────────

/// A Python name that collides with a Rust keyword is emitted as a RAW
/// identifier — `def move` → `pub fn r#move` (Ruchy does the same). The
/// citation IS emitted above it; the audit's detector matched the bare name
/// and could not see the declaration, so the function landed in the F1
/// denominator and never in the numerator. An under-count, i.e. the reporter
/// reporting a number that is not true.
#[test]
fn audit_sees_a_citation_on_a_raw_identifier() {
    let dir = scratch("rawident");
    write(
        &dir,
        "kw.py",
        "def move(type: int) -> int:\n    return type + 1\n",
    );

    for target in ["rust", "ruchy"] {
        let (ok, json, stderr) = run_audit(&[dir.to_str().unwrap(), "--target", target, "--json"]);
        assert!(ok, "[{target}] audit must succeed: {stderr}");
        // Vacuity guard: the function must be IN the denominator (it does
        // integer arithmetic), or the numerator claim below is empty.
        assert!(
            json.contains("\"functions_requiring_citation\":1"),
            "[{target}] `move` must require a citation:\n{json}"
        );
        assert!(
            json.contains("\"functions_with_citation\":1"),
            "[{target}] the citation on `r#move` must be counted:\n{json}"
        );
    }
}

/// The reported percentage must never OVERSTATE the ratio it was computed
/// from: `{:.1}` rounds, so 2166/2167 (99.954%) rendered as a flat `100.0%`.
/// The reporters truncate now, which can only understate.
///
/// Honest limit of this assertion: no corpus available here has a ratio whose
/// third decimal rounds it up a tenth, so it holds under BOTH truncation and
/// rounding today. It is a guard against a corpus that later does, not a proof
/// that the change took effect — that is what the comment on `display_pct`
/// and the 2166/2167 measurement in the CHANGELOG record.
#[test]
fn audit_never_reports_a_percentage_above_its_own_ratio() {
    let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut measured = 0usize;
    for target in ["rust", "ruchy", "lean"] {
        let (ok, json, stderr) =
            run_audit(&[corpus.to_str().unwrap(), "--target", target, "--json"]);
        assert!(ok, "[{target}] audit must succeed: {stderr}");
        let requiring = json_usize(&json, "functions_requiring_citation");
        let with = json_usize(&json, "functions_with_citation");
        if requiring == 0 {
            continue;
        }
        measured += 1;
        let exact = (with as f64) / (requiring as f64) * 100.0;
        let reported: f64 = json
            .split("\"f1_pct\":")
            .nth(1)
            .and_then(|s| s.split(',').next())
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| panic!("[{target}] no numeric f1_pct in {json}"));
        assert!(
            reported <= exact + 1e-9,
            "[{target}] reported {reported} > exact {exact} ({with}/{requiring}):\n{json}"
        );
    }
    assert_eq!(measured, 3, "all three citation-bearing lanes must measure");
}

fn json_usize(json: &str, field: &str) -> usize {
    let key = format!("\"{field}\":");
    let rest = json
        .split(&key)
        .nth(1)
        .unwrap_or_else(|| panic!("field `{field}` missing from: {json}"));
    rest.chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or_else(|_| panic!("field `{field}` is not an integer in: {json}"))
}

// ── 6. the same class on `transpile`: a flag accepted and then dropped ─

fn run_transpile(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(bin())
        .args(["transpile"])
        .args(args)
        .output()
        .expect("spawn xpile");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// `--out` names a file the caller wants written. The emit path returned on
/// `--emit-crate` first, so passing both wrote the crate, never wrote the
/// file, and exited 0.
#[test]
fn transpile_refuses_two_output_destinations_instead_of_dropping_one() {
    let dir = scratch("twodest");
    write(&dir, "add.py", MEASURABLE_PY);
    let src = dir.join("add.py");
    let out_file = dir.join("out.rs");
    let crate_dir = dir.join("thecrate");

    let (ok, stdout, stderr) = run_transpile(&[
        src.to_str().unwrap(),
        "--out",
        out_file.to_str().unwrap(),
        "--emit-crate",
        crate_dir.to_str().unwrap(),
    ]);
    assert!(
        !ok,
        "--out + --emit-crate must refuse, not silently drop --out:\n{stdout}\n{stderr}"
    );
    assert!(
        !out_file.exists(),
        "the refusal must not have written anything"
    );
    assert!(
        !crate_dir.exists(),
        "the refusal must not have written the crate either"
    );
    assert!(
        stderr.contains("two different output destinations"),
        "the refusal must name the reason:\n{stderr}"
    );

    // Vacuity guard: each destination ALONE must still work, or the assert
    // above would pass on a `transpile` that refuses everything.
    let (ok_out, _, e1) =
        run_transpile(&[src.to_str().unwrap(), "--out", out_file.to_str().unwrap()]);
    assert!(ok_out && out_file.exists(), "--out alone must work: {e1}");
    let (ok_crate, _, e2) = run_transpile(&[
        src.to_str().unwrap(),
        "--emit-crate",
        crate_dir.to_str().unwrap(),
    ]);
    assert!(
        ok_crate && crate_dir.join("Cargo.toml").exists(),
        "--emit-crate alone must work: {e2}"
    );
}

/// `--hardware` only ever builds a PTX profile, so every non-PTX target
/// accepted it and then ignored it — `--target rust --hardware ptx:sm_89`
/// exited 0 emitting plain Rust.
#[test]
fn transpile_refuses_hardware_on_a_target_that_cannot_consume_it() {
    let dir = scratch("hwflag");
    write(&dir, "add.py", MEASURABLE_PY);
    let src = dir.join("add.py");

    for target in [
        "rust", "wasm", "shell", "lean", "ruchy", "forjar", "spirv", "wgsl",
    ] {
        let (ok, stdout, stderr) = run_transpile(&[
            src.to_str().unwrap(),
            "--target",
            target,
            "--hardware",
            "ptx:sm_89",
        ]);
        assert!(
            !ok,
            "--target {target} ignores --hardware and must say so:\n{stdout}\n{stderr}"
        );
    }

    // A misspelled VALUE still reports as one rather than as a target
    // mismatch — the value is parsed before applicability.
    let (_, _, bad) = run_transpile(&[
        src.to_str().unwrap(),
        "--target",
        "rust",
        "--hardware",
        "banana",
    ]);
    assert!(
        bad.contains("unknown --hardware"),
        "a bad --hardware value must report as a bad value:\n{bad}"
    );

    // Vacuity guard: the flag must still reach the one target that consumes
    // it, otherwise this test would pass on a CLI that refuses --hardware
    // outright.
    let (ok_ptx, _, e) = run_transpile(&[
        src.to_str().unwrap(),
        "--target",
        "ptx",
        "--hardware",
        "ptx:sm_89",
    ]);
    assert!(
        ok_ptx,
        "--target ptx --hardware ptx:sm_89 must still succeed: {e}"
    );
}

// ── 7. the property itself, over the whole corpus ─────────────────────

#[test]
fn no_probe_reports_a_coverage_number_it_did_not_measure() {
    let dir = scratch("property");
    write(&dir, "add.py", MEASURABLE_PY);
    let empty = scratch("property-empty");
    let bad = scratch("property-bad");
    write(&bad, "bad.py", UNLOWERABLE_PY);
    let missing = std::env::temp_dir().join("xpile-audithon-property-missing");
    let _ = std::fs::remove_dir_all(&missing);

    let probes: Vec<(&str, PathBuf)> = vec![
        ("measurable", dir),
        ("no-source", empty),
        ("all-lowering-failures", bad),
        ("nonexistent", missing),
    ];

    let mut measured_rows = 0usize;
    for (name, p) in &probes {
        let (ok, json, stderr) = run_audit(&[p.to_str().unwrap(), "--json"]);
        if !ok {
            // Refused. A refusal may not smuggle a score out on stdout.
            assert!(
                !json.contains("f1_pct"),
                "[{name}] refused but still emitted an F1 payload:\n{json}\n{stderr}"
            );
            continue;
        }
        let denominator_zero = json.contains("\"functions_requiring_citation\":0");
        if denominator_zero {
            assert!(
                json.contains("\"f1_pct\":null") && json.contains("\"f1_status\":\"VACUOUS\""),
                "[{name}] reported a number for a 0 denominator:\n{json}"
            );
        } else {
            measured_rows += 1;
            assert!(
                !json.contains("\"f1_pct\":null"),
                "[{name}] measured a corpus but reported no number:\n{json}"
            );
        }
    }

    // Corpus vacuity guard: if every probe refused or came back VACUOUS the
    // loop above would pass while asserting nothing about the measured side.
    assert_eq!(
        measured_rows, 1,
        "exactly one probe (`measurable`) must reach a measured F1"
    );
}
