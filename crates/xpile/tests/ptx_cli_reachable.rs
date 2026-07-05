//! XPILE-PTX-001 — the PTX backend is CLI-reachable via `--hardware ptx`.
//!
//! Before this gate, all three CLI Module-construction sites hardcoded
//! `hardware: None` (main.rs) so every `xpile transpile --target ptx` refused
//! with `MissingHardware` — the README "target … a GPU" / 9-backend claim was
//! true for the library but unreachable from the CLI (fable review F4).
//!
//! These tests drive the real `xpile` binary as a subprocess and assert:
//!   * `--target ptx --hardware ptx` emits well-formed PTX (structural markers
//!     the offline `ptxas`/`validate_ptx` witnesses already assemble), AND
//!   * `--target ptx` WITHOUT `--hardware` still refuses loudly (the honest
//!     refusal is preserved — the flag adds reachability, it does not paper
//!     over the contract's compute-capability requirement).
//!
//! No CUDA toolkit / GPU is required: this is pure emitted-text reachability,
//! so it runs on free CI under `workspace-test`. The `ptxas`-assembles leg
//! lives in `crates/xpile-ptx-codegen/tests/ptxas_validate.rs` (graceful-skip
//! when `ptxas` is absent).

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_xpile"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn run_xpile(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("spawn xpile")
}

#[test]
fn transpile_ptx_with_hardware_emits_valid_ptx() {
    let py = fixture("ptx_kernel.py");
    let out = run_xpile(&[
        "transpile",
        py.to_str().unwrap(),
        "--target",
        "ptx",
        "--hardware",
        "ptx",
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "`--target ptx --hardware ptx` must succeed (PTX is CLI-reachable): \
         stderr={stderr}\nstdout={stdout}"
    );
    // Structural markers of the hand-emitted, ptxas-assemblable kernel — the
    // same text `validate_ptx` accepts and `ptxas` assembles on a CUDA box.
    assert!(
        stdout.contains(".visible .entry xpile_kernel("),
        "expected the PTX kernel entry point in:\n{stdout}"
    );
    assert!(
        stdout.contains(".target sm_80"),
        "expected the contract-floor compute capability `sm_80` in:\n{stdout}"
    );
}

#[test]
fn transpile_ptx_with_explicit_compute_cap_targets_it() {
    let py = fixture("ptx_kernel.py");
    let out = run_xpile(&[
        "transpile",
        py.to_str().unwrap(),
        "--target",
        "ptx",
        "--hardware",
        "ptx:sm_89",
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "`--hardware ptx:sm_89` must succeed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains(".target sm_89"),
        "explicit `ptx:sm_89` must set `.target sm_89` in:\n{stdout}"
    );
}

#[test]
fn transpile_ptx_without_hardware_refuses_loudly() {
    let py = fixture("ptx_kernel.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "ptx"]);
    assert!(
        !out.status.success(),
        "`--target ptx` without `--hardware` must still refuse (the PTX \
         contract requires a compute capability — reachability must not paper \
         over that): stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("missing hardware profile"),
        "refusal must name the missing hardware profile, got:\n{stderr}"
    );
}
