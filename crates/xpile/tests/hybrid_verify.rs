//! PMAT-902 (Sprint Day 3 — NORTH STAR): `xpile hybrid <dir> --verify`.
//!
//! The executing hybrid differential — the architecture's reason to exist. Emit
//! the buildable workspace, `cargo build` it (build.rs cc-compiles + links the C
//! side through the emitted `unsafe extern "C"` shim), run the linked artifact,
//! and byte-compare its stdout against the CPython reference: the SAME C
//! extension bound via `ctypes` and driven by the original `app.py` `main()`.
//! Two independent host languages calling one cc-compiled `square_sum` must
//! agree — the differential that proves the emitted FFI shim is ABI-faithful.
//! Gated on cc + python3 + cargo so a constrained runner skips gracefully.

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

/// A POSIX shell does not answer `--version` — `/bin/sh` is dash here and exits
/// **2**. That is survivable only because `tool_available` above tests whether
/// the process SPAWNED, not whether it succeeded; the identically-named helper
/// in `wasm_source_lang_refusal_witness.rs` tests `status.success()`, and
/// unifying the two — an obviously correct-looking cleanup — would have turned
/// every `sh` guard here into a permanent silent skip. `sh -c true` is the
/// spelling that is right under both (XPILE-SKIPGUARD-001, PMAT-1505).
fn shell_available() -> bool {
    Command::new("sh")
        .args(["-c", "true"])
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn hybrid_verify_matches_cpython_on_real_fixture() {
    if !tool_available("cc") || !tool_available("python3") || !tool_available("cargo") {
        eprintln!("cc/python3/cargo unavailable — skipping hybrid --verify test");
        return;
    }
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("hybrid")
        .arg(fixture("hybrid_sum"))
        .arg("--verify")
        .output()
        .expect("run xpile hybrid --verify");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "verify of the real hybrid_sum fixture must exit 0 (MATCH);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // Golden-lock the verdict: square_sum(7) = 7*7 = 49, byte-identical across the
    // CPython-via-ctypes reference and the executed Rust+linked-C artifact. If the
    // emitted shim mis-marshalled the i64/c_int boundary, the two would diverge.
    assert!(
        stdout.contains("✓ MATCH") && stdout.contains("\"49\""),
        "expected a MATCH verdict on \"49\":\n{stdout}"
    );
}

/// PMAT-1352: the FALSIFIER for the above — the `ComparisonResult::Divergence`
/// arm, which had ZERO coverage. All three pre-existing `--verify` witnesses
/// (`hybrid_verify{,_float,_multiarg}.rs`) are green-path only, so nothing
/// proved that `--verify` can FAIL. A differential that has never been observed
/// to go red is indistinguishable from one that always returns Match.
///
/// FINDING that shaped this fixture: an FFI **mis-cast** cannot make the two
/// sides disagree. `ctypes_binding_for` (the CPython reference's binding) and
/// the emitted `extern "C"` shim are both derived from the SAME meta-HIR types,
/// so any error is made identically on both sides and cancels. Probed
/// exhaustively before settling on the fixture below: an `int` boundary agrees
/// even when the product overflows `int` (both truncate to 1410065408); a `long`
/// boundary is honestly REFUSED as non-ABI-mappable rather than mis-bound; a
/// whole-number `double` return agrees (`2.0`, not Rust's bare `2`); a
/// bool-shaped `int` return agrees. The divergence therefore has to come from
/// the PYTHON side, where the reference is CPython and the artifact is
/// transpiled Rust — which is exactly the class this differential exists to
/// catch.
///
/// The fixture's FFI boundary is identical to `hybrid_sum`'s and AGREES (`49` on
/// line 1); the divergence is on line 2 and is a documented, still-open one
/// (CHANGELOG "Known divergences" item 5, OPEN POLICY: int literals in a
/// float-annotated list). See the fixture's own header for why that coupling is
/// deliberate.
#[test]
fn hybrid_verify_reports_divergence_and_exits_nonzero() {
    if !tool_available("cc") || !tool_available("python3") || !tool_available("cargo") {
        eprintln!("cc/python3/cargo unavailable — skipping hybrid --verify divergence test");
        return;
    }
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("hybrid")
        .arg(fixture("hybrid_divergent"))
        .arg("--verify")
        .output()
        .expect("run xpile hybrid --verify");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    // 1. It must FAIL. A green exit here would mean the differential cannot
    //    detect a divergence it is looking straight at.
    assert!(
        !out.status.success(),
        "verify of a DIVERGENT fixture must exit non-zero;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // 2. It must NAME the divergence — verdict, side-by-side values, and the
    //    bail reason. An exit code alone would not tell an operator what broke.
    assert!(
        stderr.contains("✗ DIVERGENT"),
        "expected the DIVERGENT verdict on stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("CPython:  [1, 2.5]") && stderr.contains("artifact: [1.0, 2.5]"),
        "the verdict must print BOTH sides so the divergence is diagnosable:\n{stderr}"
    );
    assert!(
        stderr.contains("hybrid verify: artifact diverged from the CPython reference"),
        "expected the bail reason:\n{stderr}"
    );

    // 3. The reported line number is 1-BASED (PMAT-1352 fixed the raw 0-based
    //    index, which reported a first-line divergence as "line 0"). The
    //    agreeing `49` is line 1, so the diverging `print(xs)` is line 2.
    assert!(
        stderr.contains("✗ DIVERGENT at line 2:"),
        "expected a 1-based line number pointing at the SECOND printed line:\n{stderr}"
    );

    // 4. NON-VACUITY: the boundary itself reconciled and the differential
    //    actually ran. Without this the test would still pass if `--verify`
    //    failed for an unrelated reason (a build error, a missing tool), which
    //    is the failure mode that makes a red-path test worthless.
    assert!(
        stdout.contains("square_sum : Python → C"),
        "the FFI boundary must still reconcile — this is a differential failure, \
         not a reconcile failure:\n{stdout}"
    );
    assert!(
        stdout.contains("--verify: CPython reference"),
        "the differential must have reached the comparison step:\n{stdout}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// PMAT-1362 — the SHELL half of the executing hybrid differential.
//
// `verify_hybrid` filtered `to_lang == C`, so the `hybrid_shell` fixture
// reconciled a real `Python → Shell` shim and then printed "no C FFI boundary
// to execute — nothing to verify" and exited 0: a green run that had executed
// NOTHING. The shell lane now builds the artifact, puts the RE-EMITTED script
// on PATH, runs it, and byte-diffs against the ORIGINAL script under `sh`.
//
// NON-VACUITY (both arms demonstrated red before landing, and both are pinned
// by assertions below):
//   * drop the PATH prepend → the artifact panics with
//     "spawning shell boundary `_tool`: No such file or directory", exit 1;
//   * perturb one line of the re-emitted script → `✗ DIVERGENT at line 2`,
//     exit 1.
// So a MATCH here means the artifact really spawned a real program and really
// read its stdout. The stdout assertions pin the CONTENT, which is what keeps
// an "empty == empty" match from passing.
//
// The reference is `sh`, NOT CPython: a shell boundary is invoked by program
// name and the shim returns `io::Result<Output>`, which no lowered Python call
// consumes, so the driver is generated rather than being `app.py`'s `main()`.
// argv marshalling stays string-compare-only (the driver passes `&[]`).
// ─────────────────────────────────────────────────────────────────────────────

/// The FLAT-command shell boundary: one `echo`. Locks the verdict line
/// byte-exactly, the way `hybrid_golden_lock.rs` does for the C lane.
#[test]
fn hybrid_verify_executes_the_flat_shell_boundary() {
    if !shell_available() || !tool_available("cargo") {
        eprintln!("sh/cargo unavailable — skipping shell hybrid --verify test");
        return;
    }
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("hybrid")
        .arg(fixture("hybrid_shell"))
        .arg("--verify")
        .output()
        .expect("run xpile hybrid --verify");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "verify of the shell fixture must exit 0 (MATCH);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // The boundary reconciled AND the shell lane was entered — without this a
    // regression back to the C-only filter would still print a green run.
    assert!(
        stdout.contains("_tool : Python → Shell"),
        "the Python→Shell boundary must reconcile:\n{stdout}"
    );
    assert!(
        stdout.contains("`sh` reference (original _tool) vs executed shim-spawned artifact:"),
        "the shell differential must have reached the comparison step \
         (a C-only filter regression prints 'nothing to verify' and exits 0):\n{stdout}"
    );
    assert!(
        stdout.contains("✓ MATCH — stdout byte-identical (1 line(s)): \"running tool\""),
        "verdict drifted from the golden-locked shell witness:\n{stdout}"
    );
}

/// The CONTROL-FLOW shell boundary: `for` + `while` + `if`/`else` + top-level
/// `case`, i.e. the whole v0.1.0 shell control-flow surface, executed through
/// the emitted subprocess shim. This is what makes the lane more than a
/// one-`echo` smoke test — every construct's round-trip has to be
/// EXECUTION-faithful, not just parse-faithful, for the 8 lines to match.
#[test]
fn hybrid_verify_executes_the_control_flow_shell_boundary() {
    if !shell_available() || !tool_available("cargo") {
        eprintln!("sh/cargo unavailable — skipping control-flow shell hybrid --verify test");
        return;
    }
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("hybrid")
        .arg(fixture("hybrid_shell_exec"))
        .arg("--verify")
        .output()
        .expect("run xpile hybrid --verify");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "verify of the control-flow shell fixture must exit 0 (MATCH);\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // Byte-locked: all 8 lines, in order. `for` emitted 3, `while` 2, `if` its
    // then-arm, `case` its first arm. A construct that silently dropped its
    // body would shorten this and flip the test red.
    assert!(
        stdout.contains(
            "✓ MATCH — stdout byte-identical (8 line(s)): \
             \"running tool\\nitem alpha\\nitem beta\\nitem gamma\\n\
             tick 0\\ntick 1\\ncounted to 2\\ncase two\""
        ),
        "verdict drifted from the golden-locked control-flow shell witness:\n{stdout}"
    );
}

/// A fixture with NO executable boundary must say so honestly — and must not
/// claim a C-shaped reason for it. `hybrid_pysibling`'s Python→Python import is
/// dropped by `reconcile`, so there is no boundary at all.
#[test]
fn hybrid_verify_reports_nothing_to_verify_without_a_c_shaped_excuse() {
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("hybrid")
        .arg(fixture("hybrid_pysibling"))
        .arg("--verify")
        .output()
        .expect("run xpile hybrid --verify");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "no boundary is not a failure:\n{stdout}"
    );
    assert!(
        stdout.contains("--verify: no FFI boundary to execute — nothing to verify"),
        "expected the paradigm-neutral message:\n{stdout}"
    );
    assert!(
        !stdout.contains("no C FFI boundary"),
        "the C-only framing outlived the C-only filter:\n{stdout}"
    );
}
