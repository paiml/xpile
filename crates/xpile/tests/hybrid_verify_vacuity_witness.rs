//! XPILE-HYBRIDVAC-001 (PMAT-1387) — `xpile hybrid --verify` reported `✓ MATCH`
//! for a differential in which NOTHING WAS OBSERVED.
//!
//! `--verify` is the PMAT-902 NORTH STAR: the executing hybrid differential,
//! "the architecture's reason to exist". Both of its lanes ended in
//!
//! ```text
//! ComparisonResult::Match => println!("  ✓ MATCH — stdout byte-identical …")
//! ```
//!
//! with no predicate on whether the compared strings held anything. When the
//! reference side produced no output — a `main()` whose body is `pass` (C lane),
//! or an empty `.sh` (shell lane) — both sides were `""`, byte-identity held
//! TRIVIALLY, and the command printed
//!
//! ```text
//!   ✓ MATCH — stdout byte-identical (1 line(s)): ""
//! ```
//!
//! and exited 0. MEASURED on the live tree at 903d7aab, both lanes, before the
//! fix. The reconciled FFI boundary was never called; the C function could have
//! returned anything. The `.max(1)` on the line count even asserted that ONE
//! line had been compared when zero had. An empty reference is not agreement —
//! it is the absence of evidence, and a differential indistinguishable from one
//! that never ran must not be reported as a pass.
//!
//! **Refusal, not skip** (PMAT-1385's split-by-KIND doctrine). A missing
//! toolchain is an ENVIRONMENT absence: disclose it and stay green. A vacuous
//! differential is a FIXTURE defect on a check the operator explicitly asked
//! for, and the verdict it would otherwise print is false — so it exits
//! non-zero.
//!
//! The three control tests below are the vacuity guard on THIS witness: a
//! "fix" that made `--verify` refuse everything, or that reclassified the
//! divergence arm as vacuous, fails them.

use std::path::PathBuf;
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn tool_available(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

fn c_lane_ready() -> bool {
    tool_available("cc") && tool_available("python3") && tool_available("cargo")
}

fn shell_lane_ready() -> bool {
    cfg!(unix) && tool_available("sh") && tool_available("cargo")
}

fn verify(name: &str) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("hybrid")
        .arg(fixture(name))
        .arg("--verify")
        .output()
        .expect("run xpile hybrid --verify");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The C lane's defect arm. `hybrid_vacuous_c` is `hybrid_sum` with the SAME
/// reconciled `Python → C square_sum` boundary and a `main()` that prints
/// nothing.
#[test]
fn vacuous_c_differential_refuses_instead_of_reporting_match() {
    if !c_lane_ready() {
        eprintln!("cc/python3/cargo unavailable — skipping the C-lane vacuity witness");
        return;
    }
    let (ok, stdout, stderr) = verify("hybrid_vacuous_c");

    assert!(
        !ok,
        "a differential over EMPTY output on both sides proves nothing and must exit \
         non-zero;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // The exit code alone is not the finding — the operator has to be told WHY,
    // or a vacuous fixture is indistinguishable from a real divergence.
    assert!(
        stderr.contains("✗ VACUOUS"),
        "expected the VACUOUS verdict on stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("VACUOUS differential — the CPython reference produced no output"),
        "the bail reason must name the reference side that produced nothing:\n{stderr}"
    );
    // The regression proper: the old green verdict must be GONE, not merely
    // accompanied by a non-zero exit.
    assert!(
        !stdout.contains("✓ MATCH"),
        "a vacuous run must NOT also print a MATCH verdict:\n{stdout}"
    );
    // …and specifically not the `(1 line(s)): ""` shape, which claimed a line
    // count of 1 for zero compared lines.
    assert!(
        !stdout.contains("byte-identical (1 line(s)): \"\""),
        "the pre-PMAT-1387 verdict string must not be reachable:\n{stdout}"
    );
}

/// The shell lane's defect arm — same shape, different reference (`sh` running
/// the ORIGINAL script, which here is empty). Proves the guard is on the shared
/// verdict path and not bolted onto one lane.
#[test]
fn vacuous_shell_differential_refuses_instead_of_reporting_match() {
    if !shell_lane_ready() {
        eprintln!("unix sh/cargo unavailable — skipping the shell-lane vacuity witness");
        return;
    }
    let (ok, stdout, stderr) = verify("hybrid_vacuous_shell");

    assert!(
        !ok,
        "an empty `.sh` round-trips to an empty script; that differential proves nothing \
         and must exit non-zero;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("✗ VACUOUS"),
        "expected the VACUOUS verdict on stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("VACUOUS differential — the `sh` reference produced no output"),
        "the bail reason must name the `sh` reference:\n{stderr}"
    );
    assert!(
        !stdout.contains("✓ MATCH"),
        "a vacuous run must NOT also print a MATCH verdict:\n{stdout}"
    );
}

/// CONTROL 1 — a real C differential still passes. Without this, a fix that
/// refused every `--verify` invocation would satisfy the two tests above.
#[test]
fn non_vacuous_c_differential_still_matches() {
    if !c_lane_ready() {
        eprintln!("cc/python3/cargo unavailable — skipping the C-lane control");
        return;
    }
    let (ok, stdout, stderr) = verify("hybrid_sum");
    assert!(
        ok,
        "the vacuity guard must not red a real differential;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("✓ MATCH — stdout byte-identical (1 line(s)): \"49\""),
        "the honest MATCH verdict must survive verbatim:\n{stdout}"
    );
}

/// CONTROL 2 — the shell lane's real differential still passes.
#[test]
fn non_vacuous_shell_differential_still_matches() {
    if !shell_lane_ready() {
        eprintln!("unix sh/cargo unavailable — skipping the shell-lane control");
        return;
    }
    let (ok, stdout, stderr) = verify("hybrid_shell");
    assert!(
        ok,
        "the vacuity guard must not red a real shell differential;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("✓ MATCH — stdout byte-identical (1 line(s)): \"running tool\""),
        "the honest MATCH verdict must survive verbatim:\n{stdout}"
    );
}

/// CONTROL 3 — the DIVERGENT arm is not swallowed by the new guard, and its
/// side-by-side diagnostic keeps its column alignment through the extraction of
/// the shared `differential_verdict` helper (the two lanes' labels are now
/// padded rather than hardcoded).
#[test]
fn divergence_is_not_reclassified_as_vacuous() {
    if !c_lane_ready() {
        eprintln!("cc/python3/cargo unavailable — skipping the divergence control");
        return;
    }
    let (ok, stdout, stderr) = verify("hybrid_divergent");
    assert!(
        !ok,
        "a divergent fixture must still exit non-zero:\n{stdout}"
    );
    assert!(
        stderr.contains("✗ DIVERGENT at line 2:"),
        "the divergence verdict must survive:\n{stderr}"
    );
    assert!(
        !stderr.contains("✗ VACUOUS"),
        "a real divergence is not a vacuous differential:\n{stderr}"
    );
    assert!(
        stderr.contains("      CPython:  [1, 2.5]")
            && stderr.contains("      artifact: [1.0, 2.5]"),
        "the two sides must stay column-aligned after the helper extraction:\n{stderr}"
    );
}

/// The fixtures themselves are the experiment. If someone "repairs" them by
/// adding a `print` or an `echo`, the two defect tests above would pass for the
/// wrong reason — they would no longer be vacuous. Pin the property directly.
#[test]
fn the_vacuity_fixtures_are_actually_vacuous() {
    let sh = fixture("hybrid_vacuous_shell").join("_tool.sh");
    let sh_src = std::fs::read_to_string(&sh).expect("read the vacuous shell fixture");
    assert!(
        sh_src.trim().is_empty(),
        "hybrid_vacuous_shell/_tool.sh must stay EMPTY — it is the reference side that \
         must produce no output; found:\n{sh_src}"
    );

    let py = fixture("hybrid_vacuous_c").join("app.py");
    let py_src = std::fs::read_to_string(&py).expect("read the vacuous C fixture");
    let body: Vec<&str> = py_src
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect();
    assert!(
        !body.iter().any(|l| l.contains("print(")),
        "hybrid_vacuous_c/app.py's main() must print NOTHING:\n{py_src}"
    );
    // …and it must still reconcile a real boundary, or the test would be
    // measuring "no boundary" rather than "a boundary that was never exercised".
    assert!(
        body.iter()
            .any(|l| l.contains("from ._core import square_sum")),
        "hybrid_vacuous_c/app.py must keep its C FFI boundary:\n{py_src}"
    );
}
