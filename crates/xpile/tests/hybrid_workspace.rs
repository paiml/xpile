//! PMAT-901 (Sprint-2 Day 2 — Phase 5a): `xpile hybrid <dir> --emit-workspace`.
//!
//! Emits a buildable Cargo workspace, then PROVES it end-to-end: `cargo build`
//! compiles the Rust side AND `build.rs` cc-compiles + links the C side through
//! the emitted `extern "C"` shim, and the linked binary runs exit-0. This is the
//! first *executing* hybrid artifact (the differential check vs CPython is Day 3).
//! Gated on `cc` + `cargo` so a constrained runner skips gracefully.

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
fn hybrid_cli_emit_workspace_builds_and_runs() {
    if !tool_available("cc") || !tool_available("cargo") {
        eprintln!("cc/cargo unavailable — skipping hybrid workspace build test");
        return;
    }
    let ws = std::env::temp_dir().join(format!("xpile_hybrid_ws_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);

    // 1) Emit the workspace through the real binary.
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("hybrid")
        .arg(fixture("hybrid_sum"))
        .arg("--emit-workspace")
        .arg(&ws)
        .output()
        .expect("run xpile hybrid --emit-workspace");
    assert!(
        out.status.success(),
        "emit-workspace should exit 0; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        ws.join("Cargo.toml").exists()
            && ws.join("build.rs").exists()
            && ws.join("csrc/_core.c").exists()
            && ws.join("src/ffi_shims.rs").exists()
            && ws.join("src/main.rs").exists(),
        "emitted workspace is missing expected files"
    );

    // 2) Build it. Force `--target-dir` (highest precedence — overrides both the
    //    inherited CARGO_TARGET_DIR that `cargo test` sets AND the parent repo's
    //    .cargo/config) so the artifact lands in a known, isolated location and
    //    can't collide with the parent build. current_dir = ws keeps it a
    //    standalone crate. This runs build.rs (cc-compiles _core.c) + links
    //    ffi_shims.rs.
    let target = ws.join("target");
    let build = Command::new("cargo")
        .current_dir(&ws)
        .arg("build")
        .arg("--target-dir")
        .arg(&target)
        .output()
        .expect("cargo build the emitted workspace");
    assert!(
        build.status.success(),
        "cargo build of the emitted hybrid workspace failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    // 3) The linked binary runs exit-0 (Day 2 DoD; the differential check is Day 3).
    let bin = target.join("debug/xpile-hybrid-artifact");
    let run = Command::new(&bin)
        .output()
        .expect("run the hybrid artifact");
    assert!(run.status.success(), "the hybrid artifact exited non-zero");

    let _ = std::fs::remove_dir_all(&ws);
}
