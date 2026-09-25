//! #2139 — `xpile hybrid --verify` CHECKS `float`, `unsigned long long` and
//! `long long` C boundaries instead of skipping them.
//!
//! Until #2139, `ctypes_name` had no binding for `F32`, `CULong` or `CLong`, so
//! `--verify` printed `boundary … has a non-ABI-mappable type — skipping` and
//! exited 0. For `float` and `unsigned long long` the workspace behind that
//! skip did not compile (E0308): the disclosed-pass-in-front-of-a-broken-emit
//! shape PMAT-1353 removed for `unsigned int`. Each type now gets the binding
//! the shim already speaks, and each fixture pins a different outcome:
//!
//! * `hybrid_float32` MATCHes CPython under plain `--verify`: the workspace
//!   bridges `f64 ↔ f32` exactly as ctypes' `c_float` does, and the float
//!   retype now covers an `F32` return (`y = twice(2.5)` used in arithmetic).
//! * `hybrid_ulong` is REPORTED, exit non-zero, naming the `u64` E0308. There
//!   is no lossless `i64` bridge (ctypes returns ints above 2^63), so a loud
//!   failure is the honest result, and `--repair` converges on it.
//! * `hybrid_long` MATCHes: its workspace always built; the skip only meant
//!   that nothing compared its output.
//!
//! Gated on cc + python3 + cargo, like the other executing hybrid witnesses.

use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn toolchain() -> bool {
    ["cc", "python3", "cargo"]
        .iter()
        .all(|t| Command::new(t).arg("--version").output().is_ok())
}

fn verify(name: &str, repair: bool) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xpile"));
    cmd.arg("hybrid").arg(fixture(name)).arg("--verify");
    if repair {
        cmd.arg("--repair");
    }
    cmd.output().expect("run xpile hybrid --verify")
}

fn text(o: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn float_boundary_matches_cpython_without_repair() {
    if !toolchain() {
        eprintln!("cc/python3/cargo unavailable — skipping float boundary test");
        return;
    }
    let out = verify("hybrid_float32", false);
    let (stdout, stderr) = text(&out);
    assert!(
        out.status.success(),
        "`--verify` on hybrid_float32 must exit 0;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // CPython through ctypes' c_float, measured: f32 rounding shows in the repr,
    // and a bool argument converts as ctypes does: `c_float(True)` is 1.0 (#2150).
    assert!(
        stdout.contains(
            r#"✓ MATCH — stdout byte-identical (6 line(s)): "2.200000047683716\n6.0\n0.20000000298023224\n6.0\n2.0\n0.0""#
        ),
        "expected CPython's exact c_float output:\n{stdout}"
    );
    assert!(
        !stdout.contains("non-ABI-mappable"),
        "a float boundary must be checked, not skipped (#2139):\n{stdout}"
    );
}

#[test]
fn unsigned_long_boundary_is_reported_not_skipped() {
    if !toolchain() {
        eprintln!("cc/python3/cargo unavailable — skipping unsigned long test");
        return;
    }
    let out = verify("hybrid_ulong", false);
    let (stdout, stderr) = text(&out);
    assert!(
        !out.status.success(),
        "`--verify` on an uncompilable workspace must exit NON-ZERO, not skip at 0 \
         (#2139);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("non-ABI-mappable"),
        "an unsigned long boundary must be checked, not skipped:\n{stdout}"
    );
    assert!(
        stderr.contains("expected `u64`, found `i64`"),
        "the failure must be the unsigned-long call site specifically:\n{stderr}"
    );
}

#[test]
fn unsigned_long_repair_converges_even_above_i64_max() {
    if !toolchain() {
        eprintln!("cc/python3/cargo unavailable — skipping unsigned long --repair test");
        return;
    }
    let out = verify("hybrid_ulong", true);
    let (stdout, stderr) = text(&out);
    assert!(
        out.status.success() && stdout.contains("✓ REPAIRED"),
        "`--verify --repair` must converge on hybrid_ulong, whose second line \
         (2^63) no i64 can hold;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn long_long_boundary_is_compared_not_skipped() {
    if !toolchain() {
        eprintln!("cc/python3/cargo unavailable — skipping long long test");
        return;
    }
    let out = verify("hybrid_long", false);
    let (stdout, stderr) = text(&out);
    assert!(
        out.status.success()
            && stdout.contains(
                r#"✓ MATCH — stdout byte-identical (4 line(s)): "-21\n9223372036854775806\n3\n0""#
            ),
        "expected a MATCH against CPython's c_longlong, a bool argument included (#2150);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
