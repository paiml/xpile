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
