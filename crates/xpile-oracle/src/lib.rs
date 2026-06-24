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
// PMAT-902 (Sprint Day 3 — NORTH STAR): the CPython hybrid reference.
//
// For a hybrid Python+C module the differential reference is "what CPython prints
// when it runs the program against the real C extension". We get that without
// building a Python C-extension module by `cc`-compiling the C source(s) into a
// shared object and binding each boundary symbol via `ctypes` — the exact
// foreign-call mechanism a CPython C extension uses at runtime. The Python entry
// (`app.py`) has its C-extension relative import stripped (the ctypes prologue
// supplies those names) and its `main()` is run under `python3`; stdout is the
// reference the executed Rust+linked-C hybrid artifact is checked against. If the
// emitted FFI shim mis-marshals the ABI, the two stdouts diverge.
// ─────────────────────────────────────────────────────────────────────────────

/// One boundary symbol's `ctypes` ABI for the CPython hybrid reference.
/// `argtypes`/`restype` are bare `ctypes` type names (e.g. `"c_int"`,
/// `"c_double"`); `restype == None` binds a `void` callee (`restype = None`).
#[derive(Debug, Clone)]
pub struct CtypesBinding {
    pub symbol: String,
    pub argtypes: Vec<&'static str>,
    pub restype: Option<&'static str>,
}

/// Capture the CPython reference for a hybrid Python+C module: `cc`-compile
/// `c_sources` into a shared object, bind each `CtypesBinding` symbol via
/// `ctypes`, strip the C-extension relative import from `py_source`, and run its
/// `main()` under `python3`, returning stdout (trailing newline trimmed).
/// Requires `cc` + `python3`; callers should graceful-skip when either is absent
/// (mirror [`PythonOracle::available`] / [`CExtensionOracle::available`]).
pub fn capture_cpython_hybrid_ref(
    py_source: &str,
    c_sources: &[(String, String)],
    bindings: &[CtypesBinding],
) -> Result<String, OracleError> {
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    // PMAT-933: a process-wide counter so CONCURRENT captures in the same process
    // (e.g. two `#[test]`s in one test binary, which share `pid`) never collide on
    // the `.so` / C-source temp names — without it, one capture's shared object
    // overwrote another's, surfacing as a spurious `undefined symbol` ctypes error.
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    // 1) Materialize the C sources and cc-compile them into one shared object.
    let mut c_paths = Vec::new();
    for (i, (name, content)) in c_sources.iter().enumerate() {
        let p = dir.join(format!("xpile_hybref_{pid}_{seq}_{i}_{name}"));
        std::fs::write(&p, content)
            .map_err(|e| OracleError::Capture(format!("writing C source {name}: {e}")))?;
        c_paths.push(p);
    }
    let so = dir.join(format!("libxpile_hybref_{pid}_{seq}.so"));
    let mut cc = std::process::Command::new("cc");
    cc.arg("-shared").arg("-fPIC");
    for p in &c_paths {
        cc.arg(p);
    }
    cc.arg("-o").arg(&so);
    let compiled = cc.output();
    for p in &c_paths {
        let _ = std::fs::remove_file(p);
    }
    let compiled = compiled.map_err(|e| OracleError::Capture(format!("spawning cc: {e}")))?;
    if !compiled.status.success() {
        let _ = std::fs::remove_file(&so);
        return Err(OracleError::Capture(format!(
            "cc -shared failed: {}",
            String::from_utf8_lossy(&compiled.stderr).trim()
        )));
    }

    // 2) Build the ctypes prologue binding every boundary symbol.
    let mut prologue = String::new();
    prologue.push_str("import ctypes as _ct\n");
    prologue.push_str(&format!("_lib = _ct.CDLL({:?})\n", so.to_string_lossy()));
    for b in bindings {
        prologue.push_str(&format!("{0} = _lib.{0}\n", b.symbol));
        let args: Vec<String> = b.argtypes.iter().map(|t| format!("_ct.{t}")).collect();
        prologue.push_str(&format!("{}.argtypes = [{}]\n", b.symbol, args.join(", ")));
        let rt = match b.restype {
            Some(t) => format!("_ct.{t}"),
            None => "None".to_string(),
        };
        prologue.push_str(&format!("{}.restype = {}\n", b.symbol, rt));
    }

    // 3) Strip the C-extension relative imports (`from ._core import …`); the
    //    ctypes prologue supplies those names. Keep every other line verbatim.
    let stripped: String = py_source
        .lines()
        .filter(|l| !l.trim_start().starts_with("from ."))
        .collect::<Vec<_>>()
        .join("\n");

    // 4) Run the composed program under python3 and capture stdout.
    let program = format!("{prologue}\n{stripped}\nmain()\n");
    let run = std::process::Command::new("python3")
        .arg("-c")
        .arg(&program)
        .output();
    let _ = std::fs::remove_file(&so);
    let run = run.map_err(|e| OracleError::Capture(format!("spawning python3: {e}")))?;
    if !run.status.success() {
        return Err(OracleError::Capture(format!(
            "python3 exited {}: {}",
            run.status,
            String::from_utf8_lossy(&run.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&run.stdout)
        .trim_end_matches('\n')
        .to_string())
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

    #[test]
    fn cpython_hybrid_ref_binds_c_via_ctypes() {
        // The Day-3 reference path: CPython runs `app.py`'s real `main()` against
        // the cc-compiled C extension bound via ctypes. Proves EXECUTION (x*x),
        // not an echo: square_sum(7) = 49. Gated on cc + python3.
        if !CExtensionOracle::available() || !PythonOracle::available() {
            eprintln!("cc/python3 unavailable — skipping CPython hybrid-ref test");
            return;
        }
        let py = "from ._core import square_sum\ndef main() -> None:\n    print(square_sum(7))";
        let c = vec![(
            "_core.c".to_string(),
            "int square_sum(int x){return x*x;}\n".to_string(),
        )];
        let bindings = vec![CtypesBinding {
            symbol: "square_sum".to_string(),
            argtypes: vec!["c_int"],
            restype: Some("c_int"),
        }];
        let out = capture_cpython_hybrid_ref(py, &c, &bindings).expect("captures CPython ref");
        assert_eq!(out, "49");
    }
}
