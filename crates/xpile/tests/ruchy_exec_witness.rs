//! XPILE-WITNESS-003 (PARTIAL) — the Ruchy lane's FIRST *executing* witness.
//!
//! The architectural review's finding F7 flagged Ruchy as "the only text-only
//! lane among executable targets" — its strongest check was a string-compare of
//! the emitted source, never an execution. This witness converts that: for a
//! curated set of oracle fixtures it drives the FULL chain
//!
//!     python fixture
//!        │  xpile transpile --target ruchy
//!        ▼
//!     emitted .ruchy
//!        │  `ruchy transpile`   (Ruchy → Rust; ruchy v4.2.1)
//!        ▼
//!     Rust  ── rustc -O ──▶ binary ──▶ stdout
//!
//! and byte-diffs that stdout against CPython (via `xpile_oracle::PythonOracle`,
//! the same reference the Rust differential uses). A fixture in this set must
//! (a) survive the whole chain AND (b) match CPython exactly — so swapping `+`
//! for `-` in the Ruchy emitter's BinOp mapping flips an output and reds this
//! gate, which a string-compare against a frozen expectation cannot catch.
//!
//! HONEST SCOPE (why curated, not all 34 fixtures): ruchy v4.2.1 executes only a
//! SUBSET of what xpile emits. Measured over the 34 `oracle_fixtures/` (2026-07-05,
//! `ruchy` v4.2.1): `xpile --target ruchy` emits 34/34, but `ruchy check` (parse)
//! accepts 16/34, `ruchy run` (interpret) executes 5/34, and this
//! `ruchy transpile`→`rustc`→run chain completes 7/34. The chain's two ceilings
//! are ruchy-toolchain limitations, NOT xpile bugs:
//!   * the interpreter lacks Rust methods the emitter uses (`checked_add`, …);
//!   * `ruchy transpile` DROPS the parentheses xpile emits around
//!     `(__r < 0) != (__fb < 0)` (floor-div / modulo), producing a chained
//!     comparison rustc rejects.
//!
//! Every fixture that DOES complete the chain matches CPython byte-for-byte —
//! i.e. where Ruchy can run xpile's output, xpile's semantics are correct. Full
//! WITNESS-003 (≥10 fixtures) is blocked on the ruchy toolchain, documented in
//! `docs/specifications/audit-design.md`.
//!
//! Skips with reason when `python3` / `ruchy` / `rustc` is absent (e.g. hosted
//! CI without a pinned ruchy install) — never silently green. Wiring a pinned
//! `cargo install ruchy` into CI to make this merge-blocking is the follow-up
//! (the WABT-for-WASM analogue, XPILE-WITNESS-001).

use std::path::{Path, PathBuf};
use std::process::Command;
use xpile_oracle::{diff_stdout, ComparisonResult, PythonOracle};

/// Fixtures verified (2026-07-05, ruchy v4.2.1) to complete the
/// ruchy→rust→rustc→run chain AND match CPython. Each MUST keep doing both.
const RUCHY_EXECUTABLE_FIXTURES: &[&str] = &[
    "alt_form_radix",
    "fstr_str_precision",
    "optional_flow",
    "optional_guard_continue",
    "optional_list_literal",
    "recursion",
    "tuple_swap",
];

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/oracle_fixtures")
}

fn tool_present(cmd: &str, arg: &str) -> bool {
    Command::new(cmd)
        .arg(arg)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Run the emitted Ruchy through `ruchy transpile` → `rustc` → execute, and
/// return its stdout (trailing newline trimmed). `Err` means the ruchy TOOLCHAIN
/// could not carry this fixture (a scope limit, not an xpile miscompile) — the
/// caller distinguishes that from a stdout DIVERGENCE (which is a hard failure).
fn ruchy_transpile_compile_run(py_path: &Path, name: &str) -> Result<String, String> {
    let emit = Command::new(xpile_bin())
        .args(["transpile", py_path.to_str().unwrap(), "--target", "ruchy"])
        .output()
        .map_err(|e| format!("spawn xpile: {e}"))?;
    assert!(
        emit.status.success(),
        "xpile MUST emit ruchy for {name} (emit is xpile's own path, not a ruchy \
         limit): {}",
        String::from_utf8_lossy(&emit.stderr).trim()
    );
    let ruchy_src = String::from_utf8(emit.stdout).map_err(|e| format!("utf8: {e}"))?;

    let dir = std::env::temp_dir().join("xpile-ruchy-exec").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    let ruchy_file = dir.join("prog.ruchy");
    std::fs::write(&ruchy_file, &ruchy_src).map_err(|e| format!("write ruchy: {e}"))?;

    // Ruchy → Rust. A non-zero exit here is a ruchy-transpiler scope limit.
    let tp = Command::new("ruchy")
        .arg("transpile")
        .arg(&ruchy_file)
        .output()
        .map_err(|e| format!("spawn ruchy: {e}"))?;
    if !tp.status.success() {
        return Err(format!(
            "ruchy transpile rejected the emitted ruchy: {}",
            String::from_utf8_lossy(&tp.stderr).trim()
        ));
    }
    let rust = String::from_utf8_lossy(&tp.stdout).to_string();
    let rs = dir.join("prog.rs");
    std::fs::write(&rs, &rust).map_err(|e| format!("write rs: {e}"))?;

    let bin = dir.join("prog");
    let compile = Command::new("rustc")
        .arg("--edition=2021")
        .arg("-O")
        .arg("-o")
        .arg(&bin)
        .arg(&rs)
        .output()
        .map_err(|e| format!("spawn rustc: {e}"))?;
    if !compile.status.success() {
        return Err(format!(
            "rustc rejected the ruchy-transpiled Rust: {}",
            String::from_utf8_lossy(&compile.stderr).trim()
        ));
    }
    let run = Command::new(&bin)
        .output()
        .map_err(|e| format!("run: {e}"))?;
    if !run.status.success() {
        return Err(format!(
            "ruchy-derived binary exited {}: {}",
            run.status,
            String::from_utf8_lossy(&run.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&run.stdout)
        .trim_end_matches('\n')
        .to_string())
}

#[test]
fn ruchy_execution_differential_vs_cpython() {
    if !PythonOracle::available() {
        eprintln!("warning: python3 not on PATH; skipping XPILE-WITNESS-003 ruchy exec witness");
        return;
    }
    if !tool_present("ruchy", "--version") {
        eprintln!(
            "warning: `ruchy` not on PATH; skipping XPILE-WITNESS-003 ruchy exec witness. \
             Install a pinned ruchy (`cargo install ruchy`) to run the Ruchy lane's \
             execution witness."
        );
        return;
    }
    if !tool_present("rustc", "--version") {
        eprintln!("warning: rustc not on PATH; skipping XPILE-WITNESS-003 ruchy exec witness");
        return;
    }

    let oracle = PythonOracle::new();
    let dir = fixtures_dir();
    let mut executed = 0usize;
    let mut divergences: Vec<String> = Vec::new();
    let mut toolchain_regressions: Vec<String> = Vec::new();

    for name in RUCHY_EXECUTABLE_FIXTURES {
        let path = dir.join(format!("{name}.py"));
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                divergences.push(format!("{name}: fixture unreadable ({e})"));
                continue;
            }
        };
        let py_out = match oracle.run_main(&src) {
            Ok(o) => o,
            Err(e) => {
                divergences.push(format!("{name}: CPython reference capture failed: {e}"));
                continue;
            }
        };
        match ruchy_transpile_compile_run(&path, name) {
            Ok(ruchy_out) => match diff_stdout(&py_out, &ruchy_out) {
                ComparisonResult::Match => executed += 1,
                ComparisonResult::Divergence {
                    index,
                    expected,
                    actual,
                } => divergences.push(format!(
                    "{name}: line {index} DIVERGES — python3 {expected:?} vs ruchy→rust {actual:?}\n\
                     full python3:\n{py_out}\nfull ruchy→rust:\n{ruchy_out}"
                )),
            },
            // The chain broke at ruchy/rustc — a curated fixture that used to
            // execute no longer does. That's a ruchy-toolchain REGRESSION (or a
            // version change), not an xpile miscompile; surface it distinctly.
            Err(e) => toolchain_regressions.push(format!("{name}: {e}")),
        }
    }

    eprintln!(
        "XPILE-WITNESS-003: Ruchy lane executed {}/{} curated fixtures against CPython \
         (ruchy→rust→rustc→run); {} toolchain regression(s).",
        executed,
        RUCHY_EXECUTABLE_FIXTURES.len(),
        toolchain_regressions.len()
    );

    // A DIVERGENCE is always a hard failure — it means xpile's Ruchy emission is
    // semantically wrong where Ruchy CAN run it.
    assert!(
        divergences.is_empty(),
        "Ruchy execution witness found {} CPython divergence(s):\n\n{}",
        divergences.len(),
        divergences.join("\n\n")
    );
    // Every curated fixture must still complete the chain. If ruchy regresses,
    // this fires (with a clear toolchain-regression message) so the curated set
    // is re-measured rather than silently rotting.
    assert!(
        toolchain_regressions.is_empty(),
        "{} curated Ruchy fixture(s) no longer complete the ruchy→rust→rustc→run \
         chain (ruchy-toolchain regression; re-measure RUCHY_EXECUTABLE_FIXTURES):\n\n{}",
        toolchain_regressions.len(),
        toolchain_regressions.join("\n\n")
    );
    assert_eq!(
        executed,
        RUCHY_EXECUTABLE_FIXTURES.len(),
        "expected all {} curated fixtures to execute-and-match",
        RUCHY_EXECUTABLE_FIXTURES.len()
    );
}
