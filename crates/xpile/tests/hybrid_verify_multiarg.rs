//! PMAT-930 (Sprint backlog) — a SECOND executing hybrid differential, broadening
//! the north star past the single one-argument `square_sum` fixture.
//!
//! The Day-3 witness (`hybrid_verify.rs`) proved the executing C-path differential
//! works at all on a ONE-argument boundary (`square_sum(int)->int`). That left an
//! honest "single-fixture, one-arg only" caveat: nothing exercised the emitted
//! `extern "C"` shim's MULTI-argument marshalling — the per-param `i64 -> c_int`
//! cast loop in `emit_c_shim`, the matching `ctypes` `argtypes` list, and a
//! `main()` that forwards two call arguments across the boundary.
//!
//! This fixture closes that gap with a genuinely different C boundary:
//! `int sum_of_squares(int a, int b) { return a*a + b*b; }`, called as
//! `sum_of_squares(3, 4)`. Two independent host languages (CPython via `ctypes`,
//! and the executed Rust+linked-C artifact) must agree on `3*3 + 4*4 = 25`. A
//! regression that mis-ordered the two args, dropped one, or mis-marshalled the
//! second `i64/c_int` slot would diverge here while the one-arg golden lock stayed
//! green — so this adds real, non-duplicate coverage.
//!
//! Same cc + python3 + cargo graceful-skip as the Day-3 test, so a constrained
//! runner stays green.

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
fn hybrid_verify_matches_cpython_on_multiarg_fixture() {
    if !tool_available("cc") || !tool_available("python3") || !tool_available("cargo") {
        eprintln!("cc/python3/cargo unavailable — skipping multi-arg hybrid --verify test");
        return;
    }
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("hybrid")
        .arg(fixture("hybrid_dot2"))
        .arg("--verify")
        .output()
        .expect("run xpile hybrid --verify");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "verify of the two-argument hybrid_dot2 fixture must exit 0 (MATCH);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // The two-argument boundary was reconciled (regression guard on the symbol).
    assert!(
        stdout.contains("sum_of_squares : Python → C"),
        "expected the multi-arg C boundary to reconcile:\n{stdout}"
    );
    // Golden verdict: sum_of_squares(3, 4) = 3*3 + 4*4 = 25, byte-identical across
    // the CPython-via-ctypes reference and the executed Rust+linked-C artifact. If
    // the emitted shim mis-marshalled either `i64/c_int` arg slot or swapped their
    // order, the differential would diverge from 25.
    assert!(
        stdout.contains("✓ MATCH") && stdout.contains("\"25\""),
        "expected a MATCH verdict on \"25\":\n{stdout}"
    );
}
