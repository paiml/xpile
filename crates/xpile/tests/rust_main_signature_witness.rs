//! XPILE-RUSTMAIN-001 (xpile#2165, epic #2126): `def main() -> int` was accepted
//! at exit 0 and emitted `fn main() -> i64`, which rustc rejects (E0277). The
//! Rust lane must either refuse or emit something rustc accepts.

use std::path::PathBuf;
use std::process::Command;

fn run(name: &str, src: &str) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("xpile-rustmain-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let py: PathBuf = dir.join(format!("{name}.py"));
    std::fs::write(&py, src).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .args(["transpile", py.to_str().unwrap(), "--target", "rust"])
        .output()
        .unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn main_returning_int_is_refused() {
    let (ok, out, err) = run("mint", "def main() -> int:\n    return 0\n");
    assert!(!ok, "accepted; emitted:\n{out}");
    assert!(err.contains("main"), "refusal must name main: {err}");
}

#[test]
fn main_with_a_parameter_is_refused() {
    let (ok, out, _) = run("marg", "def main(x: int) -> None:\n    pass\n");
    assert!(!ok, "accepted; emitted:\n{out}");
}

#[test]
fn control_main_returning_none_still_transpiles() {
    let (ok, out, err) = run("mnone", "def main() -> None:\n    print(1)\n");
    assert!(ok, "control refused: {err}");
    assert!(out.contains("fn main()"), "{out}");
}

#[test]
fn control_non_main_int_function_still_transpiles() {
    let (ok, out, err) = run("fint", "def f() -> int:\n    return 0\n");
    assert!(ok, "control refused: {err}");
    assert!(out.contains("fn f() -> i64"), "{out}");
}
