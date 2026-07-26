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
