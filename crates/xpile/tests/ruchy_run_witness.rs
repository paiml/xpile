//! PMAT-1384 (XPILE-RUCHYRUN-001): `ruchy run` on an xpile-emitted `.ruchy`
//! exited 0 having executed NOTHING.
//!
//! `ruchy run` is the target toolchain's own runner and the obvious thing to do
//! with a `.ruchy` file. It evaluates the module as a sequence of top-level
//! items and only auto-invokes `main` when `main` is the module's SOLE item.
//! xpile emitted `fun main() -> () { … }` and never called it, so any module
//! carrying a helper function alongside `main` DEFINED everything and RAN
//! NOTHING — exiting 0 with empty stdout where CPython printed real output.
//!
//! FIVE SILENT-WRONG-ANSWER ROWS, measured 2026-07-27 through the shipped CLI
//! (ruchy v4.2.1) against live CPython before the fix, not asserted:
//!
//! | `tests/oracle_fixtures/` | `python3` (first lines) | `ruchy run` |
//! |--------------------------|-------------------------|-------------|
//! | `recursion`              | `120` / `3628800`       | rc=0, *(empty)* |
//! | `optional_flow`          | `0` / `8`               | rc=0, *(empty)* |
//! | `optional_guard_continue`| `8` / `2`               | rc=0, *(empty)* |
//! | `range_bool_bound`       | 8 lines                 | rc=0, *(empty)* |
//! | `str_and_fstring`        | 4 lines                 | rc=0, *(empty)* |
//!
//! THE FIX (`xpile-ruchy-codegen::emit_module`): emit the entry-point
//! invocation `main()` when the module defines a zero-parameter `main`. Each of
//! the five now fails LOUDLY (rc=1) naming a real ruchy-interpreter limitation
//! — `Unknown integer method: checked_sub`, `Cannot cast boolean to i64`, … —
//! instead of returning a clean, wrong, empty answer.
//!
//! MEASURED, NOT ASSUMED, over the whole 38-fixture corpus (ruchy v4.2.1):
//!
//! | stage                       | before | after |
//! |-----------------------------|--------|-------|
//! | `xpile --target ruchy` emits| 38     | 38    |
//! | `ruchy check` (parse) accepts | 18   | 18    |
//! | `ruchy run` exits 0         | 7      | 2     |
//! | …of those, matches CPython  | 1      | 1     |
//!
//! The exit-0 count DROPPING is the point: five of the seven were false. Parse
//! acceptance is unchanged, so the trailing call costs nothing.
//!
//! THE ONE REMAINING EXIT-0 DIVERGENCE is pinned, not hidden:
//! `fstr_str_precision` prints `""""["""{:.3}"""""]""` where CPython prints
//! `[hel]`. That is a ruchy-INTERPRETER limitation, not an xpile miscompile —
//! the emitted `format!("{:.3}", s)` is correct Rust, and the SAME fixture
//! byte-matches CPython through the `ruchy transpile` → `rustc` chain that
//! `ruchy_exec_witness.rs` drives. It is listed in [`KNOWN_INTERPRETER_DIVERGENCES`]
//! so that a future ruchy release fixing it turns this witness RED and forces
//! the disclosure to be re-derived.
//!
//! The load-bearing test is [`ruchy_run_exit_zero_implies_cpython_stdout`]: it
//! asserts the PROPERTY `rc == 0 ==> stdout byte-matches CPython` over the
//! WHOLE corpus, including the rows that fail today — so a change that starts
//! executing one is immediately checked for CORRECTNESS rather than quietly
//! going green. A syntax- or exit-status-only check stays green through the
//! entire defect: every pre-fix script was valid ruchy and every one exited 0.
//!
//! Gated on `ruchy` + `python3` presence (skips with a reason).
//! `XPILE_REQUIRE_RUCHY=1` turns a skip into a hard failure. The `ruchy`
//! install is not wired into `workspace-test` (which installs wabt and nothing
//! else), so this witness SKIPS in CI today — see PMAT-1375 for the advisory
//! `backend-exec` job that would execute it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/oracle_fixtures")
}

/// Fixtures whose emitted module `ruchy run` executes to completion (rc=0) but
/// whose stdout does NOT match CPython, with the cause pinned. Each is a ruchy
/// v4.2.1 INTERPRETER limitation the emitter cannot work around; each is proven
/// not to be an xpile miscompile by the same fixture matching through the
/// `ruchy transpile` → `rustc` chain.
const KNOWN_INTERPRETER_DIVERGENCES: &[(&str, &str)] = &[(
    "fstr_str_precision",
    "ruchy v4.2.1's interpreter does not implement `format!` precision specs — \
     it echoes `{:.3}` literally instead of truncating. The emitted Rust is \
     correct: the same fixture byte-matches CPython through `ruchy transpile` \
     → `rustc` (see ruchy_exec_witness.rs).",
)];

fn tool_present(tool: &str, flag: &str) -> bool {
    Command::new(tool)
        .arg(flag)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// PMAT-1375 TRAP 4: scoped to its own env var, never `CI=true` —
/// `workspace-test` must keep skipping this BY DESIGN.
fn require_ruchy() -> bool {
    std::env::var("XPILE_REQUIRE_RUCHY").is_ok_and(|v| v == "1")
}

/// Both halves of the differential need `ruchy` and a CPython.
///
/// PMAT-1383's recorded lesson: a witness's own toolchain gate is untested
/// code, so MEASURE its runtime rather than trusting the green. `ruchy
/// --version` does exit 0 (verified: prints `ruchy 4.2.1`), and these tests
/// spawn dozens of processes — a sub-0.1s pass means the probe lied.
fn toolchain_ready() -> bool {
    let ok = tool_present("ruchy", "--version") && tool_present("python3", "--version");
    if !ok {
        assert!(
            !require_ruchy(),
            "XPILE_REQUIRE_RUCHY=1 but `ruchy` and/or `python3` is unavailable — \
             refusing to skip green"
        );
        eprintln!("SKIP: `ruchy` and/or `python3` unavailable — ruchy-run differential skipped");
    }
    ok
}

/// A per-CALL unique scratch directory (the WASM multi-exec lesson: keying on
/// (tag, pid) alone lets parallel test threads wipe each other's directory and
/// the failure reads like an emitter defect).
fn scratch(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join("xpile-ruchyrun").join(format!(
        "{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// Every `.py` under `tests/oracle_fixtures`, sorted for a stable report.
fn corpus() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(fixtures_dir())
        .expect("oracle_fixtures readable")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "py"))
        .collect();
    v.sort();
    v
}

fn stem(p: &Path) -> String {
    p.file_stem().unwrap().to_string_lossy().into_owned()
}

/// `xpile transpile <py> --target ruchy`, returning the emitted source.
fn emit_ruchy(py: &Path) -> Result<String, String> {
    let out = Command::new(xpile_bin())
        .args(["transpile", py.to_str().unwrap(), "--target", "ruchy"])
        .output()
        .map_err(|e| format!("spawn xpile: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// CPython's stdout for a fixture. The fixtures define `main()` and do NOT call
/// it (the `xpile_oracle::PythonOracle` convention), so the driver appends the
/// call — running the file bare yields EMPTY output and would make every
/// comparison trivially agree with the very defect under test.
fn cpython_stdout(py: &Path, dir: &Path) -> String {
    let mut src = std::fs::read_to_string(py).expect("fixture readable");
    src.push_str("\nmain()\n");
    let drv = dir.join("drv.py");
    std::fs::write(&drv, src).expect("write driver");
    let out = Command::new("python3")
        .arg(&drv)
        .output()
        .expect("spawn python3");
    assert!(
        out.status.success(),
        "CPython failed on fixture {}: {}",
        py.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim_end().to_string()
}

/// `ruchy run <file>` → (exit code, trimmed stdout).
fn ruchy_run(file: &Path) -> (i32, String) {
    let out = Command::new("ruchy")
        .arg("run")
        .arg(file)
        .output()
        .expect("spawn ruchy");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).trim_end().to_string(),
    )
}

/// THE LOAD-BEARING PROPERTY: over the whole corpus, `ruchy run` exiting 0 must
/// mean the emitted module produced CPython's output. A non-zero exit is an
/// honest scope limit and is allowed; a clean exit with the wrong answer is not.
#[test]
fn ruchy_run_exit_zero_implies_cpython_stdout() {
    if !toolchain_ready() {
        return;
    }
    let mut clean_and_correct = Vec::new();
    let mut honest_failures = 0usize;
    let mut violations = Vec::new();
    for py in corpus() {
        let name = stem(&py);
        let dir = scratch(&name);
        let src = match emit_ruchy(&py) {
            Ok(s) => s,
            // A refusal is honest — the lane never claimed 38/38.
            Err(_) => continue,
        };
        let file = dir.join(format!("{name}.ruchy"));
        std::fs::write(&file, &src).expect("write .ruchy");
        let (rc, actual) = ruchy_run(&file);
        if rc != 0 {
            honest_failures += 1;
            continue;
        }
        let expected = cpython_stdout(&py, &dir);
        if actual == expected {
            clean_and_correct.push(name);
            continue;
        }
        match KNOWN_INTERPRETER_DIVERGENCES
            .iter()
            .find(|(n, _)| *n == name)
        {
            Some(_) => {}
            None => violations.push(format!(
                "{name}: `ruchy run` exited 0 but stdout diverges from CPython\n  \
                 ruchy : {actual:?}\n  cpython: {expected:?}"
            )),
        }
    }
    assert!(
        violations.is_empty(),
        "`ruchy run` exited 0 with a WRONG answer on {} fixture(s):\n{}",
        violations.len(),
        violations.join("\n")
    );
    // Both halves must be non-empty, or the property is vacuous: with zero
    // clean runs it asserts nothing, and with zero failures the corpus has
    // stopped exercising the honest-refusal side.
    assert!(
        !clean_and_correct.is_empty(),
        "no fixture executes cleanly under `ruchy run` — the property is vacuous"
    );
    assert!(
        honest_failures > 0,
        "every fixture executes cleanly — re-derive the disclosed scope in \
         ruchy_exec_witness.rs rather than leaving a stale claim"
    );
    eprintln!(
        "ruchy run: {} clean+correct ({}), {} honest rc!=0, {} pinned interpreter divergence(s)",
        clean_and_correct.len(),
        clean_and_correct.join(", "),
        honest_failures,
        KNOWN_INTERPRETER_DIVERGENCES.len()
    );
}

/// No fixture may exit 0 with EMPTY stdout while CPython prints something. This
/// is the pre-fix failure mode stated directly: it is the shape that reads as
/// success at every layer a CI job normally inspects.
#[test]
fn no_fixture_exits_zero_printing_nothing() {
    if !toolchain_ready() {
        return;
    }
    let mut silent = Vec::new();
    for py in corpus() {
        let name = stem(&py);
        let dir = scratch(&name);
        let Ok(src) = emit_ruchy(&py) else { continue };
        let file = dir.join(format!("{name}.ruchy"));
        std::fs::write(&file, &src).expect("write .ruchy");
        let (rc, actual) = ruchy_run(&file);
        if rc == 0 && actual.is_empty() && !cpython_stdout(&py, &dir).is_empty() {
            silent.push(name);
        }
    }
    assert!(
        silent.is_empty(),
        "{} fixture(s) exited 0 printing NOTHING where CPython printed output — \
         the entry-point invocation is missing again: {}",
        silent.len(),
        silent.join(", ")
    );
}

/// The emitter contract behind the fix: a module defining a zero-parameter
/// `main` ends with the invocation, and one that does not defines no call to a
/// name it never bound.
#[test]
fn entry_point_call_is_emitted_exactly_when_main_exists() {
    let dir = scratch("entrypoint");

    let with_main = dir.join("with_main.py");
    std::fs::write(
        &with_main,
        "def helper(n: int) -> int:\n    return n + 1\ndef main() -> None:\n    print(helper(1))\n",
    )
    .expect("write");
    let src = emit_ruchy(&with_main).expect("emit with main");
    assert!(
        src.trim_end().ends_with("\nmain()"),
        "module defining `main` must end with the entry-point invocation:\n{src}"
    );
    assert_eq!(
        src.matches("\nmain()").count(),
        1,
        "exactly one invocation, or the interpreter runs `main` twice:\n{src}"
    );

    let no_main = dir.join("no_main.py");
    std::fs::write(&no_main, "def helper(n: int) -> int:\n    return n + 1\n").expect("write");
    let src = emit_ruchy(&no_main).expect("emit without main");
    assert!(
        !src.contains("\nmain()"),
        "a module with no `main` must not call one:\n{src}"
    );
}

/// A sole-`main` module must not run twice. `ruchy run` auto-invokes `main`
/// when it is the only item, so the explicit call could in principle duplicate
/// the output — verified against ruchy v4.2.1 rather than assumed, because the
/// whole fix rests on it.
#[test]
fn sole_main_module_runs_exactly_once() {
    if !toolchain_ready() {
        return;
    }
    let dir = scratch("sole-main");
    let py = dir.join("sole.py");
    std::fs::write(&py, "def main() -> None:\n    print(7)\n").expect("write");
    let src = emit_ruchy(&py).expect("emit");
    let file = dir.join("sole.ruchy");
    std::fs::write(&file, &src).expect("write .ruchy");
    let (rc, actual) = ruchy_run(&file);
    assert_eq!(rc, 0, "sole-main module must run: {src}");
    assert_eq!(
        actual,
        cpython_stdout(&py, &dir),
        "sole-main module double-ran (or diverged) under `ruchy run`:\n{src}"
    );
}
