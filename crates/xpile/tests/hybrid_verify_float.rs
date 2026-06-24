//! PMAT-931 (correctness-hunt) — the executing hybrid differential on a
//! `c_double` (float) FFI boundary, gating the PMAT-930 whole-number-float
//! `repr` divergence.
//!
//! PMAT-930 surfaced a confirmed open divergence: a `double`-returning C symbol
//! crossed the hybrid boundary and a WHOLE-VALUED result printed `10` from the
//! executed Rust+linked-C artifact while the CPython-via-`ctypes` reference
//! printed Python's `10.0` (CPython's `ctypes` returns a `float`, and
//! `str(10.0) == "10.0"`). The root cause: the Python frontend lowers `app.py`
//! BEFORE the C sibling is dispatched + reconciled, so a call to the FFI symbol
//! was typed with the unknown-callee `I64` default — `print(scale2(...))` took
//! the integer `{}` `Stmt::Print` arm (no trailing `.0`), and a captured
//! `let r: float = scale2(...)` even emitted `let r: i64 = …` (rustc E0308).
//!
//! The fix (`xpile_ffi_manifest::retype_float_ffi_sites` →
//! `xpile_meta_hir::retype_ffi_float_call_sites`) runs ONCE after reconciliation,
//! when the FFI symbols' real C return types are known, and re-types the
//! `double`-returning call sites in the calling module so the existing float-repr
//! path (`Expr::ToStr { of_float: true }`, the `f64` `Let` annotation) is selected
//! exactly as for a native float.
//!
//! `scale2(2.0, 5.0) = 10.0` — a WHOLE float, the exact shape that regressed.
//! The two independent host languages (CPython via `ctypes`, the executed Rust +
//! linked-C artifact) must agree byte-for-byte on `10.0`. Without the fix the
//! artifact prints `10` and the differential diverges.
//!
//! Same cc + python3 + cargo graceful-skip as the sibling hybrid-verify tests, so
//! a constrained runner stays green.

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
fn hybrid_verify_matches_cpython_on_whole_float_fixture() {
    if !tool_available("cc") || !tool_available("python3") || !tool_available("cargo") {
        eprintln!("cc/python3/cargo unavailable — skipping float hybrid --verify test");
        return;
    }
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("hybrid")
        .arg(fixture("hybrid_scale2"))
        .arg("--verify")
        .output()
        .expect("run xpile hybrid --verify");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "verify of the c_double hybrid_scale2 fixture must exit 0 (MATCH);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // The two-argument `c_double` boundary was reconciled (regression guard).
    assert!(
        stdout.contains("scale2 : Python → C"),
        "expected the c_double C boundary to reconcile:\n{stdout}"
    );
    // GOLDEN VERDICT — the PMAT-930 divergence: scale2(2.0, 5.0) = 10.0, a WHOLE
    // float that Python prints with the trailing `.0`. The CPython-via-ctypes
    // reference and the executed Rust+linked-C artifact must be byte-identical on
    // `10.0`. Pre-fix the artifact printed `10` (bare Display of the FFI f64) and
    // diverged.
    assert!(
        stdout.contains("✓ MATCH") && stdout.contains("\"10.0\""),
        "expected a MATCH verdict on the whole-float \"10.0\" (not \"10\"):\n{stdout}"
    );
}
