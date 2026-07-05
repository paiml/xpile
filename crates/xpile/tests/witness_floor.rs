//! XPILE-WITNESS-002 — the executed-vs-skipped witness-floor MANIFEST.
//!
//! XPILE-WITNESS-001 turned a missing WASM runtime into a hard panic for the
//! *one* WASM lane. This manifest kills the silent-skip CLASS: it STATICALLY
//! counts the execution witnesses that back each differential lane and asserts
//! a per-lane FLOOR. Deleting or silencing a batch of witnesses drops the
//! count below the floor and fails here — regardless of whether the runtime
//! that would execute them is present. That makes the gate robust to the
//! runtime-skip question (a skipped witness still *exists* and still counts;
//! a DELETED witness does not), which is the exact CF-4 "skip-as-green"
//! signature the architectural review flagged
//! (`docs/specifications/fable-architectural-review.md`, XPILE-WITNESS-002).
//!
//! Lanes as `(source of the count) -> (current, floor)`. Floors are the review
//! DoD's stated minimums; each floor is `<=` the current count, so this is
//! green today and only goes RED on a real regression:
//!
//! - wasm: `#[test]`s in `crates/xpile-wasm-codegen/tests/*_witness.rs` -> (444, 400)
//! - shell: `#[test]`s in `tests/shell_diff_exec.rs` -> (7, 7)
//! - rust-differential: `tests/oracle_fixtures/*.py` + `FixtureCfg` rows in `tests/diff_exec.rs` -> (34+10=44, 44)
//! - hybrid: `#[test]`s in `tests/hybrid_verify{,_float,_multiarg}.rs` -> (3, 3)
//! - wasi: the `examples/proven-model/model.py` input the CI `wasi` job runs -> (1, 1)
//!
//! GPU lanes (ptx/wgsl/spirv) run under `cargo test --workspace` but
//! SKIP-WITH-REASON on hosted runners (no CUDA toolchain / no Vulkan adapter).
//! The review requires they never go SILENTLY ABSENT: their `gpu_witness.rs`
//! must exist, carry a witness, gate on an availability probe, and print a
//! skip notice — asserted by `gpu_lanes_skip_with_reason_never_silently_absent`.
//!
//! All paths are derived from `CARGO_MANIFEST_DIR` (the `xpile` crate dir,
//! which Cargo sets for every `cargo test` invocation regardless of CWD), so
//! the walk is workspace-relative, never absolute-hardcoded.

use std::fs;
use std::path::{Path, PathBuf};

// ── Floors (review DoD minimums; current counts noted inline) ───────────────
const WASM_FLOOR: usize = 400; // current 444
const SHELL_FLOOR: usize = 7; // current 7
const RUST_DIFF_FLOOR: usize = 44; // current 44 (34 oracle + 10 diff_exec)
const HYBRID_FLOOR: usize = 3; // current 3
const WASI_FLOOR: usize = 1; // current 1

// ── Path helpers ────────────────────────────────────────────────────────────
fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Repo root = two levels up from `crates/xpile`.
fn repo_root() -> PathBuf {
    crate_dir().join("..").join("..")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("witness-floor: cannot read {}: {e}", path.display()))
}

// ── Counting primitives ─────────────────────────────────────────────────────
/// Count lines that, once trimmed, are EXACTLY `#[test]`. The exact match (not
/// a substring scan) means a `#[test]` mentioned inside a doc comment or a
/// string literal is not counted — only real attributes.
fn count_test_attrs(src: &str) -> usize {
    src.lines().filter(|l| l.trim() == "#[test]").count()
}

/// Count lines containing `needle` (structural markers that occur once per
/// record, e.g. a `FixtureCfg`'s `file: "` field).
fn count_lines_containing(src: &str, needle: &str) -> usize {
    src.lines().filter(|l| l.contains(needle)).count()
}

/// Sum `#[test]` attributes across every `*_witness.rs` file in `dir`.
fn count_witness_tests_in_dir(dir: &Path) -> usize {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("witness-floor: cannot read dir {}: {e}", dir.display()));
    let mut total = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_witness = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with("_witness.rs"));
        if is_witness {
            total += count_test_attrs(&read(&path));
        }
    }
    total
}

/// Count `*.py` files in `dir`.
fn count_py_files(dir: &Path) -> usize {
    fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("witness-floor: cannot read dir {}: {e}", dir.display()))
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("py"))
        .count()
}

// ── Per-lane counts ─────────────────────────────────────────────────────────
fn wasm_witness_count() -> usize {
    count_witness_tests_in_dir(&crate_dir().join("../xpile-wasm-codegen/tests"))
}

fn shell_witness_count() -> usize {
    count_test_attrs(&read(&crate_dir().join("tests/shell_diff_exec.rs")))
}

fn rust_differential_witness_count() -> usize {
    let oracle = count_py_files(&crate_dir().join("tests/oracle_fixtures"));
    let diff_exec =
        count_lines_containing(&read(&crate_dir().join("tests/diff_exec.rs")), "file: \"");
    oracle + diff_exec
}

fn hybrid_witness_count() -> usize {
    [
        "tests/hybrid_verify.rs",
        "tests/hybrid_verify_float.rs",
        "tests/hybrid_verify_multiarg.rs",
    ]
    .iter()
    .map(|rel| count_test_attrs(&read(&crate_dir().join(rel))))
    .sum()
}

fn wasi_witness_count() -> usize {
    // The single wasi execution witness input: the proven model the CI `wasi`
    // job emits -> wasm32-wasip1 -> wasmtime and diffs vs CPython.
    usize::from(repo_root().join("examples/proven-model/model.py").is_file())
}

// ── Per-lane floor gates ────────────────────────────────────────────────────
#[test]
fn wasm_witness_floor() {
    let n = wasm_witness_count();
    eprintln!("witness-manifest[wasm]: {n} executed witnesses (floor {WASM_FLOOR})");
    assert!(
        n >= WASM_FLOOR,
        "wasm witness floor breached: {n} < {WASM_FLOOR} — a batch of WASM \
         execution witnesses was deleted or silenced (XPILE-WITNESS-002)"
    );
}

#[test]
fn shell_witness_floor() {
    let n = shell_witness_count();
    eprintln!("witness-manifest[shell]: {n} executed witnesses (floor {SHELL_FLOOR})");
    assert!(
        n >= SHELL_FLOOR,
        "shell witness floor breached: {n} < {SHELL_FLOOR} (XPILE-WITNESS-002)"
    );
}

#[test]
fn rust_differential_witness_floor() {
    let n = rust_differential_witness_count();
    eprintln!(
        "witness-manifest[rust-differential]: {n} executed witnesses (floor {RUST_DIFF_FLOOR})"
    );
    assert!(
        n >= RUST_DIFF_FLOOR,
        "rust-differential witness floor breached: {n} < {RUST_DIFF_FLOOR} — \
         oracle fixtures and/or diff_exec FixtureCfg rows were deleted \
         (XPILE-WITNESS-002)"
    );
}

#[test]
fn hybrid_witness_floor() {
    let n = hybrid_witness_count();
    eprintln!("witness-manifest[hybrid]: {n} executed witnesses (floor {HYBRID_FLOOR})");
    assert!(
        n >= HYBRID_FLOOR,
        "hybrid witness floor breached: {n} < {HYBRID_FLOOR} (XPILE-WITNESS-002)"
    );
}

#[test]
fn wasi_witness_floor() {
    let n = wasi_witness_count();
    eprintln!("witness-manifest[wasi]: {n} execution witness input (floor {WASI_FLOOR})");
    assert!(
        n >= WASI_FLOOR,
        "wasi witness floor breached: {n} < {WASI_FLOOR} — \
         examples/proven-model/model.py missing (XPILE-WITNESS-002)"
    );
    // The wasi lane is wired into hosted CI via the wasm32-wasip1 target; if
    // the whole lane is dropped, this marker disappears from ci.yml.
    let ci = read(&repo_root().join(".github/workflows/ci.yml"));
    assert!(
        ci.contains("wasm32-wasip1"),
        "the wasi CI lane (wasm32-wasip1 -> wasmtime) vanished from ci.yml \
         (XPILE-WITNESS-002)"
    );
}

// ── GPU lanes: skipped-with-reason, never silently absent ───────────────────
#[test]
fn gpu_lanes_skip_with_reason_never_silently_absent() {
    let lanes = [
        ("ptx", "../xpile-ptx-codegen/tests/gpu_witness.rs"),
        ("wgsl", "../xpile-wgsl-codegen/tests/gpu_witness.rs"),
        ("spirv", "../xpile-spirv-codegen/tests/gpu_witness.rs"),
    ];
    for (lane, rel) in lanes {
        let path = crate_dir().join(rel);
        assert!(
            path.is_file(),
            "GPU lane {lane} silently absent: {} missing (XPILE-WITNESS-002)",
            path.display()
        );
        let src = read(&path);
        let tests = count_test_attrs(&src);
        let has_guard = src.contains("available()");
        let has_skip_notice = src.contains("skipping");
        eprintln!(
            "witness-manifest[gpu/{lane}]: {tests} witness(es), guard={has_guard}, skip_notice={has_skip_notice}"
        );
        assert!(
            tests >= 1,
            "GPU lane {lane}: no #[test] witness present (XPILE-WITNESS-002)"
        );
        assert!(
            has_guard,
            "GPU lane {lane}: no availability guard — a witness that runs \
             unconditionally can't skip-with-reason (XPILE-WITNESS-002)"
        );
        assert!(
            has_skip_notice,
            "GPU lane {lane}: no skip-with-reason notice string (XPILE-WITNESS-002)"
        );
    }
}

// ── Aggregate manifest emitter (also asserts the grand-total floor) ─────────
#[test]
fn witness_floor_manifest_emitted() {
    let wasm = wasm_witness_count();
    let shell = shell_witness_count();
    let rustd = rust_differential_witness_count();
    let hybrid = hybrid_witness_count();
    let wasi = wasi_witness_count();
    let total = wasm + shell + rustd + hybrid + wasi;
    let floor_total = WASM_FLOOR + SHELL_FLOOR + RUST_DIFF_FLOOR + HYBRID_FLOOR + WASI_FLOOR;

    eprintln!("== XPILE-WITNESS-002 witness-floor manifest ==");
    eprintln!("  lane                executed  floor");
    eprintln!("  wasm                {wasm:>8}  {WASM_FLOOR}");
    eprintln!("  shell               {shell:>8}  {SHELL_FLOOR}");
    eprintln!("  rust-differential   {rustd:>8}  {RUST_DIFF_FLOOR}");
    eprintln!("  hybrid              {hybrid:>8}  {HYBRID_FLOOR}");
    eprintln!("  wasi                {wasi:>8}  {WASI_FLOOR}");
    eprintln!("  TOTAL               {total:>8}  {floor_total}");

    assert!(
        total >= floor_total,
        "aggregate witness floor breached: {total} < {floor_total} (XPILE-WITNESS-002)"
    );
}
