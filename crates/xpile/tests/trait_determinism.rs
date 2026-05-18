//! Asserting Runtime witness for trait determinism (PMAT-125 /
//! XPILE-TRAIT-DETERMINISM-RUNTIME-001).
//!
//! Closes the follow-on ticket from PMAT-123's fixture
//! (`crates/xpile/tests/fixtures/trait_determinism_demo.py`).
//! PMAT-123 added the fixture so `xpile quorum`'s Runtime-stratum
//! scanner would count it. This test actually exercises the
//! fixture — two transpile invocations with identical input must
//! produce byte-identical output. That's the determinism property
//! the trait contracts (`C-XPILE-FRONTEND-TRAIT` +
//! `C-XPILE-BACKEND-TRAIT`) pin down.
//!
//! Uses the subprocess pattern from `transpile_e2e.rs` so no
//! dev-dependencies need to be added to the xpile binary crate.
//!
//! ## What this asserts
//!
//! For each target T in {rust, ruchy, lean}, running
//! `xpile transpile trait_determinism_demo.py --target T` twice
//! produces byte-identical stdout. This is the combined property
//! of `Frontend::parse_and_lower` determinism + `Backend::lower`
//! determinism — if either side flakes, the bytes differ.
//!
//! ## What this does NOT assert
//!
//! - Determinism over a *symbolic* input (that's the Kani harness
//!   PMAT-063 / PMAT-065)
//! - Equivalence of outputs across targets (different backends are
//!   allowed to emit different syntax)
//! - Determinism of the rendered emission's *behavior*; this test
//!   only compares emit bytes, not their downstream execution

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_xpile"))
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join("trait_determinism_demo.py")
}

fn run_transpile(target: &str) -> String {
    let out = Command::new(bin())
        .arg("transpile")
        .arg(fixture())
        .arg("--target")
        .arg(target)
        .output()
        .expect("spawn xpile transpile");
    assert!(
        out.status.success(),
        "xpile transpile --target {target} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("xpile stdout is utf-8")
}

#[test]
fn transpile_python_to_rust_is_byte_identical_on_repeat() {
    // C-XPILE-FRONTEND-TRAIT + C-XPILE-BACKEND-TRAIT
    // (Rust target).
    let first = run_transpile("rust");
    let second = run_transpile("rust");
    assert_eq!(
        first, second,
        "xpile transpile --target rust must be deterministic per \
         C-XPILE-FRONTEND-TRAIT + C-XPILE-BACKEND-TRAIT"
    );
}

#[test]
fn transpile_python_to_ruchy_is_byte_identical_on_repeat() {
    // C-XPILE-FRONTEND-TRAIT + C-XPILE-BACKEND-TRAIT
    // (Ruchy target).
    let first = run_transpile("ruchy");
    let second = run_transpile("ruchy");
    assert_eq!(
        first, second,
        "xpile transpile --target ruchy must be deterministic per \
         C-XPILE-FRONTEND-TRAIT + C-XPILE-BACKEND-TRAIT"
    );
}

#[test]
fn transpile_python_to_lean_is_byte_identical_on_repeat() {
    // C-XPILE-FRONTEND-TRAIT + C-XPILE-BACKEND-TRAIT
    // (Lean target).
    let first = run_transpile("lean");
    let second = run_transpile("lean");
    assert_eq!(
        first, second,
        "xpile transpile --target lean must be deterministic per \
         C-XPILE-FRONTEND-TRAIT + C-XPILE-BACKEND-TRAIT"
    );
}

#[test]
fn bashrs_round_trip_is_byte_identical_on_repeat() {
    // PMAT-126 / C-BASHRS-POSIX-IDEMPOTENCE — CLI-level
    // determinism for the shell domain. Same subprocess
    // pattern as the Python tests above but exercises
    // bashrs-frontend → bashrs-backend rather than
    // depyler-frontend → Rust/Ruchy/Lean codegen.
    //
    // Complements PMAT-043's `shell_diff_exec.rs` which checks
    // *semantic* equivalence between CPython subprocess.run
    // and the bashrs-emitted shell. This test asserts
    // *byte-level* determinism: two transpile invocations
    // produce byte-identical output.
    let shell_fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join("bashrs_realistic_demo.sh");
    let run = |_: ()| {
        let out = Command::new(bin())
            .arg("transpile")
            .arg(&shell_fixture)
            .arg("--target")
            .arg("shell")
            .output()
            .expect("spawn xpile transpile");
        assert!(
            out.status.success(),
            "bashrs round-trip failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("utf-8 stdout")
    };
    let first = run(());
    let second = run(());
    assert_eq!(
        first, second,
        "xpile transpile foo.sh --target shell must be deterministic \
         per C-BASHRS-POSIX-IDEMPOTENCE"
    );
}
