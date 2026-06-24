//! PMAT-897 (Sprint-2 Tier 2 — Phase-5 hybrid): the `xpile hybrid <dir>` CLI.
//!
//! Drives Phase 1 (dispatch every file in a hybrid module dir to its frontend)
//! then Phase 2 (reconcile cross-language FFI boundaries) end-to-end through the
//! real binary. The positive fixture (`hybrid_sum/`) has a Python relative import
//! of a symbol the sibling C file exports → resolves, exit 0. The negative
//! fixture (`hybrid_missing/`) imports a symbol the C side does NOT export →
//! reconciliation fails loud, exit non-zero (the `manifest_completeness` gate of
//! C-FFI-CPYTHON-EXT).

use std::path::PathBuf;
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn hybrid_cli_reconciles_resolved_boundary() {
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("hybrid")
        .arg(fixture("hybrid_sum"))
        .output()
        .expect("run xpile hybrid");
    assert!(
        out.status.success(),
        "expected exit 0 for a resolved hybrid dir"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("2 module(s) dispatched"),
        "dispatch line:\n{stdout}"
    );
    assert!(
        stdout.contains("square_sum") && stdout.contains("Python") && stdout.contains("[shim_"),
        "resolved boundary line:\n{stdout}"
    );
}

#[test]
fn hybrid_cli_python_sibling_is_not_a_boundary() {
    // PMAT-898: a relative import of a PYTHON sibling (`from .helpers import
    // greet`) is resolved to Python→Python and dropped — not a false Python→C
    // FFI boundary. Two modules dispatch, zero boundaries, exit 0.
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("hybrid")
        .arg(fixture("hybrid_pysibling"))
        .output()
        .expect("run xpile hybrid");
    assert!(
        out.status.success(),
        "same-language imports are not FFI failures"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("2 module(s) dispatched") && stdout.contains("no cross-language FFI"),
        "expected zero FFI boundaries:\n{stdout}"
    );
}

#[test]
fn hybrid_cli_emit_shims_writes_compilable_file() {
    // PMAT-899 Phase 4: `--emit-shims <path>` lowers the reconciled manifest to a
    // real Rust FFI shim file. Drive it through the binary against hybrid_sum/
    // (`int square_sum(int)` in C), then prove the emitted file is a genuine,
    // standalone-compilable artifact — not a placeholder.
    let pid = std::process::id();
    let out_path = std::env::temp_dir().join(format!("xpile_emit_shims_cli_{pid}.rs"));
    let _ = std::fs::remove_file(&out_path);

    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("hybrid")
        .arg(fixture("hybrid_sum"))
        .arg("--emit-shims")
        .arg(&out_path)
        .output()
        .expect("run xpile hybrid --emit-shims");
    assert!(
        out.status.success(),
        "emit-shims should exit 0 for a resolved dir; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("emitted 1 FFI shim") && stdout.contains("square_sum"),
        "emit confirmation line:\n{stdout}"
    );

    let emitted = std::fs::read_to_string(&out_path).expect("shim file written");
    assert!(emitted.contains("// xpile-contract: C-FFI-CPYTHON-EXT"));
    assert!(emitted.contains("extern \"C\" {"));
    assert!(emitted.contains("fn square_sum(x: ::std::os::raw::c_int) -> ::std::os::raw::c_int;"));
    assert!(emitted.contains("pub fn square_sum_shim(x: i64) -> i64 {"));

    // The load-bearing gate: the emitted file actually compiles (extern symbol
    // stays unresolved at lib build — no C object needed). Compiled under BOTH
    // edition 2021 and 2024, since `cargo new` defaults to 2024 and a bare
    // `extern "C"` block is a hard error there. Skipped if no rustc.
    if Command::new("rustc").arg("--version").output().is_ok() {
        for edition in ["2021", "2024"] {
            let rlib =
                std::env::temp_dir().join(format!("libxpile_emit_shims_cli_{edition}_{pid}.rlib"));
            let status = Command::new("rustc")
                .args([
                    "--crate-type",
                    "lib",
                    "--edition",
                    edition,
                    "-D",
                    "warnings",
                ])
                .arg(&out_path)
                .arg("-o")
                .arg(&rlib)
                .status()
                .expect("spawn rustc");
            let _ = std::fs::remove_file(&rlib);
            assert!(
                status.success(),
                "emitted shim file must compile under edition {edition} -D warnings"
            );
        }
    }
    let _ = std::fs::remove_file(&out_path);
}

#[test]
fn hybrid_cli_fails_on_unresolved_boundary() {
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("hybrid")
        .arg(fixture("hybrid_missing"))
        .output()
        .expect("run xpile hybrid");
    assert!(
        !out.status.success(),
        "an unresolved FFI boundary must exit non-zero"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("square_sum") && stderr.contains("defines"),
        "unresolved diagnostic:\n{stderr}"
    );
}
