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
