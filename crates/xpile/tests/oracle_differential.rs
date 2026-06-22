//! PMAT-890 (Sprint-2 Tier 1): the differential ORACLE, wired into CI.
//!
//! This promotes the manual `python3`-vs-transpiled-`rustc` hunt methodology into
//! an automated gate. For every self-contained fixture in `tests/oracle_fixtures/`
//! (a Python module defining `def main() -> None:` that prints its results), the
//! oracle:
//!   1. captures CPython's stdout via [`xpile_oracle::PythonOracle`] (the reference),
//!   2. transpiles the SAME module to Rust, compiles it (`rustc`), runs the binary
//!      — the transpiled `def main()` lowers to a `pub fn main()`, a valid Rust
//!      entry point, so NO hand-written driver or expected value is needed,
//!   3. asserts the two stdouts are byte-identical via [`xpile_oracle::diff_stdout`].
//!
//! Unlike `transpile_e2e.rs` (hand-written `assert_eq!` expectations), the oracle
//! has NO hand-authored expected values — divergence from CPython is caught
//! automatically. This is the first increment of making the previously
//! scaffold-only oracle real; later increments grow the fixture set toward the
//! whole corpus and add the N-of-M stratified quorum.
//!
//! Gated on `python3` + `rustc` presence (skips gracefully, like the e2e harness).

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;
use xpile_oracle::{diff_stdout, ComparisonResult, PythonOracle};

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/oracle_fixtures")
}

fn rustc_present() -> bool {
    Command::new("rustc").arg("--version").output().is_ok()
}

/// Mirror of the e2e harness's indexmap-rlib discovery so dict-using oracle
/// fixtures (which emit `indexmap::IndexMap`) link under bare `rustc`.
fn indexmap_rustc_args() -> Vec<OsString> {
    let Ok(exe) = std::env::current_exe() else {
        return Vec::new();
    };
    let Some(deps) = exe.parent() else {
        return Vec::new();
    };
    let mut rlib = None;
    if let Ok(rd) = std::fs::read_dir(deps) {
        for entry in rd.flatten() {
            let p = entry.path();
            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                if name.starts_with("libindexmap-") && name.ends_with(".rlib") {
                    rlib = Some(p);
                    break;
                }
            }
        }
    }
    let Some(rlib) = rlib else {
        return Vec::new();
    };
    let mut dep = OsString::from("dependency=");
    dep.push(deps);
    let mut ext = OsString::from("indexmap=");
    ext.push(rlib);
    vec!["-L".into(), dep, "--extern".into(), ext]
}

/// Transpile `py_path` to Rust, compile + run it, return its stdout (trimmed).
fn transpile_compile_run(py_path: &std::path::Path, name: &str) -> Result<String, String> {
    let out = Command::new(xpile_bin())
        .args(["transpile", py_path.to_str().unwrap(), "--target", "rust"])
        .output()
        .map_err(|e| format!("spawn xpile: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "transpile failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let rust = String::from_utf8(out.stdout).map_err(|e| format!("utf8: {e}"))?;

    let dir = std::env::temp_dir().join("xpile-oracle").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    let rs = dir.join("prog.rs");
    std::fs::write(&rs, &rust).map_err(|e| format!("write: {e}"))?;
    let bin = dir.join("prog");
    let compile = Command::new("rustc")
        .arg("--edition=2021")
        .arg("-O")
        .arg("-o")
        .arg(&bin)
        .arg(&rs)
        .args(indexmap_rustc_args())
        .output()
        .map_err(|e| format!("spawn rustc: {e}"))?;
    if !compile.status.success() {
        return Err(format!(
            "rustc rejected generated code:\n{}\n--- generated ---\n{rust}",
            String::from_utf8_lossy(&compile.stderr).trim()
        ));
    }
    let run = Command::new(&bin)
        .output()
        .map_err(|e| format!("run binary: {e}"))?;
    if !run.status.success() {
        return Err(format!(
            "binary exited {}: {}",
            run.status,
            String::from_utf8_lossy(&run.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&run.stdout)
        .trim_end_matches('\n')
        .to_string())
}

#[test]
fn oracle_differential_python_vs_rust() {
    if !PythonOracle::available() {
        eprintln!("warning: python3 not on PATH; skipping differential oracle");
        return;
    }
    if !rustc_present() {
        eprintln!("warning: rustc not on PATH; skipping differential oracle");
        return;
    }
    let oracle = PythonOracle::new();
    let dir = fixtures_dir();
    let mut checked = 0usize;
    let mut failures: Vec<String> = Vec::new();

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("oracle_fixtures dir readable")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("py"))
        .collect();
    entries.sort();

    for path in &entries {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(path).expect("read fixture");
        let py_out = match oracle.run_main(&src) {
            Ok(o) => o,
            Err(e) => {
                failures.push(format!("{name}: CPython reference capture failed: {e}"));
                continue;
            }
        };
        match transpile_compile_run(path, &name) {
            Ok(rust_out) => match diff_stdout(&py_out, &rust_out) {
                ComparisonResult::Match => checked += 1,
                ComparisonResult::Divergence {
                    index,
                    expected,
                    actual,
                } => failures.push(format!(
                    "{name}: line {index} diverges — python3 {expected:?} vs rust {actual:?}\n\
                     full python3:\n{py_out}\nfull rust:\n{rust_out}"
                )),
            },
            Err(e) => failures.push(format!("{name}: {e}")),
        }
    }

    assert!(
        failures.is_empty(),
        "differential oracle found {} divergence(s):\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
    assert!(
        checked >= 5,
        "expected the oracle to verify >=5 fixtures against CPython, only {checked} matched"
    );
}
