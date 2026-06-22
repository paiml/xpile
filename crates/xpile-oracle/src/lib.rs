//! Oracle trait.
//!
//! An [`Oracle`] runs the *original* source (CPython, gcc-compiled C,
//! the ruchy interpreter) on an input fixture, captures outputs, and
//! lets the agent compare them against transpiled Rust output. This
//! is the semantic gate the agent must pass to exit successfully.
//!
//! The pattern is borrowed from alchemize: extract reference values
//! *before* the agent runs, then validate against them.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum OracleError {
    #[error("capture failed: {0}")]
    Capture(String),
    #[error("comparison failed: {0}")]
    Compare(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fixture {
    pub inputs: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedOutputs {
    pub outputs: Vec<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub enum ComparisonResult {
    Match,
    Divergence {
        index: usize,
        expected: String,
        actual: String,
    },
}

pub trait Oracle: Send + Sync {
    fn language(&self) -> &'static str;

    fn capture(&self, source: &Path, fixture: &Fixture) -> Result<CapturedOutputs, OracleError>;

    fn compare(&self, expected: &CapturedOutputs, actual: &CapturedOutputs) -> ComparisonResult;
}

// ─────────────────────────────────────────────────────────────────────────────
// PMAT-890 (Sprint-2 Tier 1): the concrete CPython oracle.
//
// This promotes the manual `python3`-vs-transpiled-`rustc` differential-hunt
// methodology into a reusable, CI-wireable reference-capture. An oracle fixture
// is a self-contained Python module that defines `def main() -> None:` printing
// its results; the oracle runs CPython on it and returns stdout, which a
// differential harness compares against the same module transpiled to Rust and
// executed (the transpiled `def main()` lowers to a `pub fn main()` — a valid
// Rust entry point — so no hand-written driver or expected value is needed).
// First increment of making the (previously scaffold-only) oracle real.
// ─────────────────────────────────────────────────────────────────────────────

/// The CPython reference oracle. Captures the stdout of running a Python module's
/// `main()` under the system `python3`.
#[derive(Debug, Default, Clone, Copy)]
pub struct PythonOracle;

impl PythonOracle {
    pub fn new() -> Self {
        PythonOracle
    }

    /// Is `python3` available on PATH? Lets callers (tests/CI) skip gracefully
    /// when the interpreter isn't installed, mirroring the rustc-presence gate.
    pub fn available() -> bool {
        std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_ok()
    }

    /// Run `python3` on `source` with a trailing `main()` call and return its
    /// stdout (trailing newline trimmed). The source must define `def main()`.
    /// Errors if `python3` is missing or the program exits non-zero (the stderr
    /// is surfaced — a CPython exception is itself a meaningful reference result
    /// the differential harness can assert the Rust side also fails on).
    pub fn run_main(&self, source: &str) -> Result<String, OracleError> {
        let program = format!("{source}\nmain()\n");
        let out = std::process::Command::new("python3")
            .arg("-c")
            .arg(&program)
            .output()
            .map_err(|e| OracleError::Capture(format!("spawning python3: {e}")))?;
        if !out.status.success() {
            return Err(OracleError::Capture(format!(
                "python3 exited {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string())
    }
}

/// Compare two captured stdout strings (CPython reference vs transpiled-Rust).
/// Returns [`ComparisonResult::Match`] when byte-identical, else a `Divergence`
/// pinning the first differing line for a readable failure message.
pub fn diff_stdout(reference: &str, actual: &str) -> ComparisonResult {
    if reference == actual {
        return ComparisonResult::Match;
    }
    let r: Vec<&str> = reference.lines().collect();
    let a: Vec<&str> = actual.lines().collect();
    let idx = r
        .iter()
        .zip(a.iter())
        .position(|(x, y)| x != y)
        .unwrap_or(r.len().min(a.len()));
    ComparisonResult::Divergence {
        index: idx,
        expected: r.get(idx).copied().unwrap_or("<no line>").to_string(),
        actual: a.get(idx).copied().unwrap_or("<no line>").to_string(),
    }
}

impl Oracle for PythonOracle {
    fn language(&self) -> &'static str {
        "python"
    }

    fn capture(&self, source: &Path, _fixture: &Fixture) -> Result<CapturedOutputs, OracleError> {
        let src = std::fs::read_to_string(source)
            .map_err(|e| OracleError::Capture(format!("reading {}: {e}", source.display())))?;
        let stdout = self.run_main(&src)?;
        Ok(CapturedOutputs {
            outputs: vec![serde_json::Value::String(stdout)],
        })
    }

    fn compare(&self, expected: &CapturedOutputs, actual: &CapturedOutputs) -> ComparisonResult {
        let exp = expected
            .outputs
            .first()
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let act = actual
            .outputs
            .first()
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        diff_stdout(exp, act)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_stdout_matches_identical() {
        assert!(matches!(
            diff_stdout("1\n2", "1\n2"),
            ComparisonResult::Match
        ));
    }

    #[test]
    fn diff_stdout_pins_first_divergent_line() {
        match diff_stdout("1\n2\n3", "1\nX\n3") {
            ComparisonResult::Divergence {
                index,
                expected,
                actual,
            } => {
                assert_eq!(index, 1);
                assert_eq!(expected, "2");
                assert_eq!(actual, "X");
            }
            ComparisonResult::Match => panic!("expected a divergence"),
        }
    }

    #[test]
    fn python_oracle_captures_main_stdout() {
        if !PythonOracle::available() {
            eprintln!("warning: python3 not on PATH; skipping CPython capture test");
            return;
        }
        let src = "def main() -> None:\n    print(2 + 3)\n    print('ok')";
        let out = PythonOracle::new().run_main(src).expect("python3 runs");
        assert_eq!(out, "5\nok");
    }
}
