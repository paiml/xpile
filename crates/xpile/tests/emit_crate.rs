//! `xpile transpile --emit-crate <dir>` writes a complete, buildable binary
//! crate (`Cargo.toml` + `src/main.rs`) — the front half of the "universal
//! binary" path: the emitted crate compiles for the native host AND for
//! `wasm32-wasip1` (a single portable `.wasm` that runs under any WASI runtime).
//!
//! This test locks the EMISSION contract (files, bin target, crate name from
//! the file stem, `indexmap`-only-when-`dict`, and the `--target rust`-only
//! guard) so it can't silently regress. The EXECUTED wasm↔CPython differential
//! (build to `wasm32-wasip1`, run under wasmtime, diff vs `python3`) lives in
//! the advisory `wasi` CI job, which supplies the wasm target + runtime.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xpile_emit_crate_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn emit(fixture_name: &str, dir: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("transpile")
        .arg(fixture(fixture_name))
        .arg("--emit-crate")
        .arg(dir)
        .output()
        .expect("run xpile transpile --emit-crate")
}

#[test]
fn emit_crate_writes_buildable_binary_crate() {
    let dir = tmp("linear");
    let out = emit("emit_crate_linear.py", &dir);
    assert!(
        out.status.success(),
        "emit-crate must exit 0:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cargo = std::fs::read_to_string(dir.join("Cargo.toml")).expect("Cargo.toml written");
    let main = std::fs::read_to_string(dir.join("src/main.rs")).expect("src/main.rs written");

    // A runnable binary crate, named from the input file stem.
    assert!(
        cargo.contains("[[bin]]"),
        "Cargo.toml declares a bin target:\n{cargo}"
    );
    assert!(
        cargo.contains("name = \"emit_crate_linear\""),
        "crate is named from the file stem:\n{cargo}"
    );
    // The transpiled program, including the `fn main` entry point.
    assert!(
        main.contains("pub fn predict"),
        "emitted fn present:\n{main}"
    );
    assert!(
        main.contains("fn main("),
        "runnable entry point present:\n{main}"
    );
    // A scalar program pulls in no external dependencies.
    assert!(
        !cargo.contains("indexmap"),
        "a scalar program needs no indexmap dep:\n{cargo}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emit_crate_adds_indexmap_only_for_dict() {
    let dir = tmp("dict");
    let out = emit("emit_crate_dict.py", &dir);
    assert!(
        out.status.success(),
        "emit-crate must exit 0:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cargo = std::fs::read_to_string(dir.join("Cargo.toml")).expect("Cargo.toml written");
    // Python `dict` lowers to `indexmap::IndexMap`, so the crate must declare it.
    assert!(
        cargo.contains("indexmap = \"2\""),
        "a dict-using program pulls in the indexmap dep:\n{cargo}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emit_crate_rejects_non_rust_target() {
    let dir = tmp("reject");
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("transpile")
        .arg(fixture("emit_crate_linear.py"))
        .arg("--target")
        .arg("wasm")
        .arg("--emit-crate")
        .arg(&dir)
        .output()
        .expect("run xpile transpile --emit-crate --target wasm");
    // The crate is a Rust crate — a non-rust target must be refused, not
    // silently produce a broken project.
    assert!(
        !out.status.success(),
        "--emit-crate with --target wasm must be refused"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--emit-crate"),
        "the error names the flag:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
