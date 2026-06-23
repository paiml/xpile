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

// ─────────────────────────────────────────────────────────────────────────────
// PMAT-900 (Sprint-2 Day 1): the concrete C-extension oracle.
//
// In a hybrid Python+C module the C side is the semantic reference for the FFI
// boundary — the Python `app.py` often just dispatches into it (hybrid_sum's
// `main()` is `pass`), so there is nothing on the Python side to diff against
// until the fixture is upgraded. This oracle `cc`-compiles the C source with a
// generated driver that calls the exported symbol on each fixture input and
// captures the printed results, giving the hybrid differential harness a
// reference to check the executed C+shim artifact against. First increment: a
// single `int`-arg / `int`-return symbol (the decy + hybrid_sum shape).
// ─────────────────────────────────────────────────────────────────────────────

/// The `cc`-compiled C reference oracle for a single `int(int)` symbol.
#[derive(Debug, Clone)]
pub struct CExtensionOracle {
    /// The exported C symbol the generated driver calls (e.g. `square_sum`).
    pub symbol: String,
}

impl CExtensionOracle {
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
        }
    }

    /// Is a C compiler (`cc`) available on PATH? Lets tests/CI skip gracefully
    /// when no toolchain is installed, mirroring [`PythonOracle::available`].
    pub fn available() -> bool {
        std::process::Command::new("cc")
            .arg("--version")
            .output()
            .is_ok()
    }

    /// Compile `c_source` together with a generated driver that calls
    /// `self.symbol(x)` for each integer in `inputs`, run it, and return one
    /// captured `i64` per input (parsed from the driver's `printf("%d\n", …)`).
    /// `#include <stdio.h>` is appended AFTER the source — legal C, since an
    /// include directive may follow a definition — so the C file needs no edit.
    fn compile_and_run(&self, c_source: &str, inputs: &[i64]) -> Result<Vec<i64>, OracleError> {
        let mut calls = String::new();
        for n in inputs {
            calls.push_str(&format!("    printf(\"%d\\n\", {}({}));\n", self.symbol, n));
        }
        let program = format!(
            "{c_source}\n#include <stdio.h>\nint main(void) {{\n{calls}    return 0;\n}}\n"
        );

        let dir = std::env::temp_dir();
        let pid = std::process::id();
        let csrc = dir.join(format!("xpile_cext_{}_{pid}.c", self.symbol));
        let cbin = dir.join(format!("xpile_cext_{}_{pid}.bin", self.symbol));
        std::fs::write(&csrc, &program)
            .map_err(|e| OracleError::Capture(format!("writing C driver: {e}")))?;

        let compiled = std::process::Command::new("cc")
            .arg(&csrc)
            .arg("-o")
            .arg(&cbin)
            .output();
        let compiled = match compiled {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_file(&csrc);
                return Err(OracleError::Capture(format!("spawning cc: {e}")));
            }
        };
        if !compiled.status.success() {
            let _ = std::fs::remove_file(&csrc);
            return Err(OracleError::Capture(format!(
                "cc failed: {}",
                String::from_utf8_lossy(&compiled.stderr).trim()
            )));
        }

        let run = std::process::Command::new(&cbin).output();
        let _ = std::fs::remove_file(&csrc);
        let _ = std::fs::remove_file(&cbin);
        let run = run.map_err(|e| OracleError::Capture(format!("running C binary: {e}")))?;
        if !run.status.success() {
            return Err(OracleError::Capture(format!(
                "C binary exited {}: {}",
                run.status,
                String::from_utf8_lossy(&run.stderr).trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&run.stdout);
        let mut out = Vec::new();
        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let v = line
                .parse::<i64>()
                .map_err(|e| OracleError::Capture(format!("parsing C output `{line}`: {e}")))?;
            out.push(v);
        }
        Ok(out)
    }
}

impl Oracle for CExtensionOracle {
    fn language(&self) -> &'static str {
        "c"
    }

    /// `source` is the C file (e.g. `_core.c`); `fixture.inputs` are the integer
    /// arguments fed to `self.symbol`. Returns one captured integer per input.
    fn capture(&self, source: &Path, fixture: &Fixture) -> Result<CapturedOutputs, OracleError> {
        let c_source = std::fs::read_to_string(source)
            .map_err(|e| OracleError::Capture(format!("reading {}: {e}", source.display())))?;
        let mut inputs = Vec::with_capacity(fixture.inputs.len());
        for v in &fixture.inputs {
            let n = v.as_i64().ok_or_else(|| {
                OracleError::Capture(format!("C oracle fixture input is not an integer: {v}"))
            })?;
            inputs.push(n);
        }
        let results = self.compile_and_run(&c_source, &inputs)?;
        Ok(CapturedOutputs {
            outputs: results.into_iter().map(serde_json::Value::from).collect(),
        })
    }

    fn compare(&self, expected: &CapturedOutputs, actual: &CapturedOutputs) -> ComparisonResult {
        for (i, (e, a)) in expected
            .outputs
            .iter()
            .zip(actual.outputs.iter())
            .enumerate()
        {
            if e != a {
                return ComparisonResult::Divergence {
                    index: i,
                    expected: e.to_string(),
                    actual: a.to_string(),
                };
            }
        }
        if expected.outputs.len() != actual.outputs.len() {
            let i = expected.outputs.len().min(actual.outputs.len());
            return ComparisonResult::Divergence {
                index: i,
                expected: format!("{} output(s)", expected.outputs.len()),
                actual: format!("{} output(s)", actual.outputs.len()),
            };
        }
        ComparisonResult::Match
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

    #[test]
    fn c_extension_oracle_compiles_runs_and_captures() {
        if !CExtensionOracle::available() {
            eprintln!("warning: cc not on PATH; skipping C-extension capture test");
            return;
        }
        // A real (non-identity) C function so the test proves EXECUTION, not an
        // input echo: square_sum(x) = x*x → [4, 9, 49] for [2, 3, 7].
        let dir = std::env::temp_dir();
        let pid = std::process::id();
        let core = dir.join(format!("xpile_cext_unit_core_{pid}.c"));
        std::fs::write(&core, "int square_sum(int x) { return x * x; }\n").expect("write core.c");

        let oracle = CExtensionOracle::new("square_sum");
        let fixture = Fixture {
            inputs: vec![
                serde_json::json!(2),
                serde_json::json!(3),
                serde_json::json!(7),
            ],
        };
        let captured = oracle.capture(&core, &fixture).expect("captures");
        let _ = std::fs::remove_file(&core);
        assert_eq!(
            captured.outputs,
            vec![
                serde_json::json!(4),
                serde_json::json!(9),
                serde_json::json!(49)
            ]
        );
    }

    #[test]
    fn c_extension_oracle_compare_matches_and_pins_divergence() {
        // Pure comparison logic — no toolchain needed, so this always runs.
        let oracle = CExtensionOracle::new("f");
        let a = CapturedOutputs {
            outputs: vec![serde_json::json!(4), serde_json::json!(9)],
        };
        assert!(matches!(
            oracle.compare(&a, &a.clone()),
            ComparisonResult::Match
        ));
        let b = CapturedOutputs {
            outputs: vec![serde_json::json!(4), serde_json::json!(8)],
        };
        match oracle.compare(&a, &b) {
            ComparisonResult::Divergence { index, .. } => assert_eq!(index, 1),
            ComparisonResult::Match => panic!("expected a divergence at index 1"),
        }
        // Length mismatch is also a divergence.
        let short = CapturedOutputs {
            outputs: vec![serde_json::json!(4)],
        };
        assert!(matches!(
            oracle.compare(&a, &short),
            ComparisonResult::Divergence { .. }
        ));
    }
}
