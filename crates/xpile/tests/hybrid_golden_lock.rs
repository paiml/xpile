//! PMAT-903 (Sprint Day 4) — GOLDEN-LOCK on the Day-3 north-star verdict.
//!
//! Day 3 (PMAT-902, `hybrid_verify.rs`) proved the executing differential works
//! at all (exit 0, MATCH, "49" present). This test pins the verdict
//! BYTE-EXACTLY so the witness cannot silently drift: it locks the full verdict
//! line — the MATCH framing, the line count (`1 line(s)`), AND the exact
//! CPython reference repr (`"49"`). A regression that changed the fixture's
//! value, mangled the i64/c_int marshalling so the artifact produced a
//! different number, altered the line count, or reworded the verdict would all
//! flip this from green to red, whereas a loose `contains("MATCH")` would not.
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

/// The exact verdict line emitted by `verify_hybrid` for the real `hybrid_sum`
/// fixture: `square_sum(7) = 7*7 = 49`, printed by `app.py main()`, byte-identical
/// across the CPython-via-ctypes reference and the executed Rust+linked-C artifact.
const GOLDEN_VERDICT: &str = "✓ MATCH — stdout byte-identical (1 line(s)): \"49\"";

#[test]
fn hybrid_verify_verdict_is_byte_locked() {
    if !tool_available("cc") || !tool_available("python3") || !tool_available("cargo") {
        eprintln!("cc/python3/cargo unavailable — skipping hybrid golden-lock test");
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
        "verify must exit 0 (MATCH);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains(GOLDEN_VERDICT),
        "verdict drifted from the golden-locked Day-3 witness.\n\
         expected the line to contain:\n  {GOLDEN_VERDICT}\n\
         got stdout:\n{stdout}"
    );
}
