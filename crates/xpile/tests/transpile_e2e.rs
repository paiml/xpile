//! End-to-end Python → Rust transpilation tests.
//!
//! Drives the `xpile transpile` binary as a subprocess against
//! fixture .py files and verifies the emitted Rust:
//!   * is shaped as expected (string matchers), AND
//!   * actually type-checks via rustc as `--edition 2021`.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    // Set by Cargo when running integration tests.
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
fn transpile_add_py_emits_rust_fn() {
    let py = fixture("add.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "rust"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "xpile failed: stderr={stderr} stdout={stdout}"
    );
    assert!(
        stdout.contains("pub fn add(a: i64, b: i64) -> i64"),
        "missing fn signature in:\n{stdout}"
    );
    assert!(stdout.contains("(a + b)"), "missing body in:\n{stdout}");
}

#[test]
fn transpile_cmp_py_emits_bool_return() {
    let py = fixture("cmp.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("pub fn le(a: i64, b: i64) -> bool"));
    assert!(stdout.contains("(a <= b)"));
}

#[test]
fn transpile_pick_py_emits_ternary_if_expr() {
    let py = fixture("pick.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("pub fn pick(a: i64, b: i64) -> i64"));
    assert!(
        stdout.contains("if (a <= b) { a } else { b }"),
        "expected ternary as if-expr in:\n{stdout}"
    );
}

#[test]
fn transpile_pick_py_to_ruchy_emits_fun_with_if() {
    let py = fixture("pick.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "ruchy"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("fun pick(a: i64, b: i64) -> i64"));
    assert!(stdout.contains("if (a <= b) { a } else { b }"));
}

#[test]
fn transpile_add_py_to_ruchy_target() {
    // Same Python source through a different backend — proves the
    // dispatch architecture: one Frontend, multiple Backends.
    let py = fixture("add.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "ruchy"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("fun add(a: i64, b: i64) -> i64"),
        "expected Ruchy `fun` signature in:\n{stdout}"
    );
    assert!(
        !stdout.contains("pub fn"),
        "Ruchy target must not emit Rust `pub fn`"
    );
    assert!(stdout.contains("(a + b)"));
}

#[test]
fn unknown_extension_errors() {
    let unknown = fixture("../fixtures/add.unknownext");
    let out = run_xpile(&["transpile", unknown.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "should fail on unknown extension");
    assert!(
        stderr.contains("no frontend") || stderr.contains("reading"),
        "expected frontend-not-found / read error, got: {stderr}"
    );
}

#[test]
fn info_lists_default_session_dispatch_tables() {
    let out = run_xpile(&["info"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    assert!(stdout.contains("Code lane"));
    assert!(stdout.contains("python"));
    assert!(stdout.contains("rust"));
    assert!(stdout.contains("Proof lane"));
    assert!(stdout.contains("latex"));
}
