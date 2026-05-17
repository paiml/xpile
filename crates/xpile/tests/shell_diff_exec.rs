//! Shell-side differential-execution gate (PMAT-043 /
//! XPILE-BASHRS-MERGER-001).
//!
//! This is the bashrs-domain twin of `tests/diff_exec.rs`. For each
//! fixture in this gate's curated set, two pipelines must produce
//! identical stdout:
//!
//!   1. CPython: `exec(open(file).read()); demo()` — the function's
//!      `subprocess.run([...])` calls fire and their stdout flows
//!      through.
//!
//!   2. Shell:   `xpile transpile file --target shell | /bin/sh` —
//!      depyler-frontend recognises each `subprocess.run` and lowers
//!      it to a `Stmt::Cmd`; bashrs-backend emits real POSIX sh;
//!      `/bin/sh` executes the same commands directly.
//!
//! Architectural role: this gate is the **Runtime stratum witness**
//! for `C-BASHRS-POSIX-IDEMPOTENCE`. Pre-PMAT-043 the contract
//! showed UNVERIFIED on `xpile quorum` (no stratum witnesses). Post-
//! this PR, Runtime ≥1 because this gate observes the bashrs-
//! emitted shell actually executing and producing output equivalent
//! to the Python source.
//!
//! Skip behaviour: if `python3` or `/bin/sh` is missing from PATH,
//! the test prints a warning and exits OK — same posture as
//! `tests/diff_exec.rs`.

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

fn have_python_and_sh() -> bool {
    let py = Command::new("python3").arg("--version").output().is_ok();
    let sh = Command::new("/bin/sh")
        .arg("-c")
        .arg("true")
        .output()
        .is_ok();
    py && sh
}

/// Run the Python source via CPython by `exec`ing the file and then
/// invoking the named entry function. Returns stdout trimmed of
/// trailing whitespace (so a stray newline at the end doesn't trip
/// the byte-for-byte compare).
fn run_cpython(fixture_path: &std::path::Path, entry: &str) -> Result<String, String> {
    let src = fixture_path.to_str().ok_or("non-utf8 fixture path")?;
    // The fixture intentionally omits `import subprocess` because
    // depyler-frontend only accepts `def` + `from __future__ import
    // annotations` at top level. We inject the import on the
    // CPython side so the fixture's subprocess.run calls resolve.
    // The shell side doesn't need any import — bashrs-backend
    // emits the commands as bare shell statements.
    //
    // Note: we deliberately discard the return value of `entry()` —
    // we only care about its side-effect stdout. Matches the shell
    // side's behaviour (shell scripts produce stdout, not return
    // values to the calling process).
    let prog = format!("import subprocess; exec(open(r'{src}').read()); {entry}()");
    let out = Command::new("python3")
        .args(["-c", &prog])
        .output()
        .map_err(|e| format!("spawn python3: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "python3 exited non-zero: stderr={}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// Transpile the Python source to shell and pipe through /bin/sh.
/// Captures stdout the same way as `run_cpython`.
fn run_shell(fixture_path: &std::path::Path) -> Result<String, String> {
    // Stage 1: xpile transpile <file> --target shell → stdout = sh source
    let transpile = Command::new(bin())
        .args([
            "transpile",
            fixture_path.to_str().unwrap(),
            "--target",
            "shell",
        ])
        .output()
        .map_err(|e| format!("spawn xpile: {e}"))?;
    if !transpile.status.success() {
        return Err(format!(
            "xpile transpile failed: stderr={}",
            String::from_utf8_lossy(&transpile.stderr)
        ));
    }
    let shell_source = String::from_utf8_lossy(&transpile.stdout).to_string();

    // Stage 2: /bin/sh -c <shell-source> → stdout = run result
    let run = Command::new("/bin/sh")
        .arg("-c")
        .arg(&shell_source)
        .output()
        .map_err(|e| format!("spawn /bin/sh: {e}"))?;
    if !run.status.success() {
        return Err(format!(
            "/bin/sh exited non-zero (script ran but commands failed?):\n\
             === stderr ===\n{}\n\
             === transpiled source ===\n{shell_source}",
            String::from_utf8_lossy(&run.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&run.stdout).trim_end().to_string())
}

#[test]
fn shell_diff_demo_cpython_vs_bashrs_emit_agree() {
    if !have_python_and_sh() {
        eprintln!(
            "warning: skipping PMAT-043 shell_diff_exec — python3 and/or /bin/sh \
             not on PATH. CI environments with both will still run this gate."
        );
        return;
    }
    let py_path = fixture("bashrs_diff_demo.py");
    let py_out = run_cpython(&py_path, "demo").expect("CPython run");
    let sh_out = run_shell(&py_path).expect("shell run");
    assert_eq!(
        py_out, sh_out,
        "CPython and bashrs-emitted shell diverge on bashrs_diff_demo.py:\n\
         === CPython ===\n{py_out}\n\
         === Shell  ===\n{sh_out}\n\
         The Runtime-stratum witness for C-BASHRS-POSIX-IDEMPOTENCE has \
         broken. Either depyler-frontend's subprocess.run lowering or \
         bashrs-backend's emit changed in a way that no longer matches \
         the Python observable behaviour."
    );
    // Anchor the *content* too so a future change to the fixture
    // can't accidentally make this test pass on no-output input.
    assert!(
        py_out.contains("starting") && py_out.contains("done"),
        "expected `starting` and `done` lines in output; got: {py_out}"
    );
}
