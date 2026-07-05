//! PMAT-1285 — EXECUTED FULL-PIPELINE (Python source → meta-HIR → WASM) witness
//! for native-WASM `xs.remove(v)` over a `list[int]` / `list[float]` — the FIRST
//! list-mutation that deletes by VALUE (not by index like `del xs[i]`).
//!
//! ## Why this witness exists
//!
//! `list.remove(v)` deletes the FIRST element EQUAL to `v`, so it FUSES the two
//! shipped primitives: a linear scan for the first typed match (exactly like
//! `xs.index(v)`, PMAT-1274) followed by the same left-shrinking tail shift as
//! `del xs[i]` (PMAT-1284). The real work lives in a `$__wasm_list_remove_{i64,f64}`
//! helper: scan for the first `i64.eq` / `f64.eq` match, TRAP (`unreachable`) if the
//! value is absent (CPython `list.remove` → ValueError, including ANY value on an
//! empty list), else shift `[slot+1, n)` LEFT by one slot walking LOW→HIGH so
//! nothing is overwritten before it is copied, then drop the count header to `n-1`.
//!
//! Because removal only SHRINKS, the record never relocates and never overruns, so
//! — unlike `append`/`insert`, which grow and demand a literal-bound list's spare
//! capacity — `remove` accepts ANY list local with a valid base-pointer, a PARAM
//! included (exactly like `del`/`pop`). Unlike `del` (a pure 8-byte word move → one
//! helper), the value compare is TYPED, so there is an f64 twin; the SHIFT stays an
//! i64 word move in both.
//!
//! A hand-built-HIR test would prove the emit handles
//! [`xpile_meta_hir::Stmt::ListRemoveValue`] but NOT that the production
//! `PythonFrontend` emits it from real `xs.remove(v)` source, nor that the emitted
//! WAT assembles and runs value-identically to CPython. This witness lowers REAL
//! Python through the same profile the CLI uses for `--target wasm`, emits,
//! assembles + runs in WABT, and asserts the executed scalar VALUE-MATCHES CPython
//! on the byte-identical program.
//!
//! ## What each probe certifies
//!
//! * `remove_middle_int` — the canonical case: `[10, 20, 30]` then `xs.remove(20)`
//!   → `[10, 30]`; pinned via `xs[0]*10 + xs[1] == 130`. Certifies the value scan,
//!   the tail shift-LEFT, and the count drop.
//! * `remove_first_int` — `[10, 20, 30]` then `xs.remove(10)` → `[20, 30]`: the
//!   whole tail shifts, `xs[0]*10 + xs[1] == 230`.
//! * `remove_last_int` — `[10, 20, 30]` then `xs.remove(30)` → `[10, 20]`: removing
//!   the final element (no shift, just count--), `xs[0]*10 + xs[1] == 120`.
//! * `remove_first_of_duplicates` — `[5, 7, 5, 9]` then `xs.remove(5)` → `[7, 5, 9]`:
//!   ONLY the FIRST occurrence is removed (the second `5` survives),
//!   `xs[0]*10 + xs[1] == 75`. The load-bearing `list.remove` semantics.
//! * `remove_negative_value` — an all-negative list `[-3, -1, -2]` then
//!   `xs.remove(-1)` → `[-3, -2]`, `xs[0] + xs[1] == -5`: certifies the SIGNED value
//!   compare (a naive scan would still match, but this pins the negative path).
//! * `remove_float` — the f64 element twin (typed `f64.eq` scan): `[1.5, 2.5, 3.5]`
//!   then `xs.remove(1.5)` → `[2.5, 3.5]`, `xs[0] == 2.5`.
//! * `remove_two_sequential` — `[0, 1, 2, 3, 4]` then `xs.remove(2)` → `[0, 1, 3, 4]`
//!   then `xs.remove(0)` → `[1, 3, 4]`, `xs[0]*100 + xs[1]*10 + xs[2] == 134`:
//!   certifies a re-scan over the already-shrunk record.
//! * `remove_from_param` — `xs.remove(v)` on a list PARAM lowers (removal shrinks,
//!   so NO spare-capacity precondition — the append/insert refusal does NOT apply).
//! * `remove_absent_traps` / `remove_on_empty_traps` — the HONEST boundary: a value
//!   not in the list, and ANY value on an empty list, TRAP (`unreachable`), matching
//!   CPython ValueError — never a silent no-op.
//! * `remove_into_bool_list_refuses` — a `list[bool]` (i32 stride) is refused at
//!   compile time (it needs the i32-stride helper twin, deferred like `insert`),
//!   never a corrupt 8-byte-strided remove over 4-byte elements.
//!
//! Gated on [`wasm_runtime_available`] — a clean skip (still asserting the full
//! pipeline LOWERS + EMITS) on a host without WABT, so free CI stays green.

use std::path::Path;
use std::process::Command;

use depyler_frontend::PythonFrontend;
use xpile_frontend::{AliasSemantics, Frontend, LoweringProfile};
use xpile_meta_hir::Module;
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// The lowering profile the CLI uses for `--target wasm`.
fn wasm_profile() -> LoweringProfile {
    LoweringProfile {
        alias_semantics: AliasSemantics::Reference,
        runtime_abort: true,
    }
}

/// Lower Python source → meta-HIR the way the CLI does for a WASM target.
fn lower(src: &str) -> Result<Module, String> {
    PythonFrontend
        .parse_and_lower_profiled(Path::new("witness.py"), src, wasm_profile())
        .map_err(|e| format!("frontend: {e}"))
}

/// The FULL pipeline: Python source → meta-HIR → WAT text.
fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

/// Assemble the real-emitted WAT into a `.wasm` in a scratch dir; returns the
/// wasm path (asserts a clean `wat2wasm`).
fn assemble(wat: &str, tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("xpile-listrm-{}-{}", std::process::id(), tag));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("go.wat");
    let wasm_path = dir.join("go.wasm");
    std::fs::write(&wat_path, wat).expect("write wat");

    let out = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        out.status.success(),
        "wat2wasm failed for {tag}:\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&out.stderr)
    );
    wasm_path
}

/// Assemble + run the real-emitted WAT's zero-arg `go` export in WABT, asserting
/// a clean (non-trapping) run, and return the printed result line's value string.
fn run_go(wat: &str, tag: &str) -> String {
    let wasm_path = assemble(wat, tag);
    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "wasm-interp run failed for {tag}: stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    let line = stdout
        .lines()
        .find(|l| l.starts_with("go(") && l.contains("=>"))
        .unwrap_or_else(|| panic!("no `go` export in interp output for {tag}:\n{stdout}"));
    line.rsplit(':').next().unwrap().trim().to_string()
}

/// Run a `go() -> int` probe and return the result as a SIGNED i64 (wasm-interp
/// prints i64/i32 as unsigned decimal).
fn run_int(src: &str, tag: &str) -> i64 {
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed for {tag}: {e}"));
    let raw = run_go(&wat, tag);
    raw.parse::<u64>()
        .unwrap_or_else(|_| panic!("parse int result {raw:?} for {tag}")) as i64
}

/// Run a `go() -> float` probe and return the result as an f64.
fn run_f64(src: &str, tag: &str) -> f64 {
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed for {tag}: {e}"));
    let raw = run_go(&wat, tag);
    raw.parse::<f64>()
        .unwrap_or_else(|_| panic!("parse f64 result {raw:?} for {tag}"))
}

/// Assemble + run and assert the program TRAPS (CPython raises → `unreachable`).
fn assert_traps(src: &str, tag: &str) {
    let wat = emit(src).unwrap_or_else(|e| panic!("{tag} emit failed: {e}"));
    if !wasm_runtime_available() {
        eprintln!("PMAT-1285: WABT absent — emit-only {tag} check passed, run skipped");
        return;
    }
    let wasm_path = assemble(&wat, tag);
    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        combined.contains("unreachable") || !run.status.success(),
        "{tag} must TRAP (CPython ValueError); got clean run: {combined}"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED canonical remove — delete the first value-match, the tail shifts LEFT.
// ---------------------------------------------------------------------------

#[test]
fn remove_middle_int_executes_and_matches_cpython() {
    // xs = [10, 20, 30]; xs.remove(20) → [10, 30]; xs[0]*10 + xs[1] == 130.
    let src = "def go() -> int:\n    xs: list[int] = [10, 20, 30]\n    xs.remove(20)\n    return xs[0] * 10 + xs[1]\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("pipeline failed to lower+emit remove: {e}"));
    // The typed remove helpers must be declared and the int helper called.
    assert!(
        wat.contains("call $__wasm_list_remove_i64"),
        "remove must call the i64 remove helper"
    );
    assert!(
        wat.contains("$__wasm_list_remove_i64 (param $base i32) (param $needle i64)"),
        "remove must declare the i64 remove helper"
    );
    assert!(
        wat.contains("$__wasm_list_remove_f64 (param $base i32) (param $needle f64)"),
        "remove must declare the f64 twin (single gate emits both, like `contains`)"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1285: WABT absent — emit-only remove check passed, execution skipped");
        return;
    }

    let got = run_int(src, "remove_middle_int");
    assert_eq!(got, 130, "remove middle int: wasm={got} cpython=130");
}

#[test]
fn remove_first_int_executes_and_matches_cpython() {
    // xs = [10, 20, 30]; xs.remove(10) → [20, 30]; xs[0]*10 + xs[1] == 230.
    let src = "def go() -> int:\n    xs: list[int] = [10, 20, 30]\n    xs.remove(10)\n    return xs[0] * 10 + xs[1]\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "remove_first_int"), 230);
}

#[test]
fn remove_last_int_executes_and_matches_cpython() {
    // xs = [10, 20, 30]; xs.remove(30) → [10, 20] (no shift, count--); == 120.
    let src = "def go() -> int:\n    xs: list[int] = [10, 20, 30]\n    xs.remove(30)\n    return xs[0] * 10 + xs[1]\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "remove_last_int"), 120);
}

// ---------------------------------------------------------------------------
// The load-bearing `list.remove` semantics — ONLY the FIRST occurrence is removed.
// ---------------------------------------------------------------------------

#[test]
fn remove_first_of_duplicates_executes_and_matches_cpython() {
    // xs = [5, 7, 5, 9]; xs.remove(5) → [7, 5, 9] (second 5 survives);
    // xs[0]*10 + xs[1] == 75.
    let src = "def go() -> int:\n    xs: list[int] = [5, 7, 5, 9]\n    xs.remove(5)\n    return xs[0] * 10 + xs[1]\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(
        run_int(src, "remove_first_of_duplicates"),
        75,
        "remove must delete ONLY the first occurrence (CPython list.remove)"
    );
}

#[test]
fn remove_negative_value_executes_and_matches_cpython() {
    // xs = [-3, -1, -2]; xs.remove(-1) → [-3, -2]; xs[0] + xs[1] == -5.
    // Certifies the SIGNED value compare on an all-negative list.
    let src = "def go() -> int:\n    xs: list[int] = [-3, -1, -2]\n    xs.remove(-1)\n    return xs[0] + xs[1]\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "remove_negative_value"), -5);
}

// ---------------------------------------------------------------------------
// The f64 element twin — a typed `f64.eq` value scan, an i64 word-move shift.
// ---------------------------------------------------------------------------

#[test]
fn remove_float_executes_and_matches_cpython() {
    // xs = [1.5, 2.5, 3.5]; xs.remove(1.5) → [2.5, 3.5]; xs[0] == 2.5.
    let src = "def go() -> float:\n    xs: list[float] = [1.5, 2.5, 3.5]\n    xs.remove(1.5)\n    return xs[0]\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("float remove emit failed: {e}"));
    assert!(
        wat.contains("call $__wasm_list_remove_f64"),
        "a list[float] remove must call the f64 helper"
    );
    if !wasm_runtime_available() {
        return;
    }
    let got = run_f64(src, "remove_float");
    assert!(
        (got - 2.5).abs() < 1e-12,
        "remove float: wasm={got} cpython=2.5"
    );
}

// ---------------------------------------------------------------------------
// Sequential removes — a re-scan over the already-shrunk record.
// ---------------------------------------------------------------------------

#[test]
fn remove_two_sequential_executes_and_matches_cpython() {
    // xs = [0, 1, 2, 3, 4]; xs.remove(2) → [0, 1, 3, 4]; xs.remove(0) → [1, 3, 4];
    // xs[0]*100 + xs[1]*10 + xs[2] == 134.
    let src = "def go() -> int:\n    xs: list[int] = [0, 1, 2, 3, 4]\n    xs.remove(2)\n    xs.remove(0)\n    return xs[0] * 100 + xs[1] * 10 + xs[2]\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "remove_two_sequential"), 134);
}

// ---------------------------------------------------------------------------
// `remove` on a list PARAM lowers — removal shrinks, so NO spare-capacity gate
// (the append/insert growable-list refusal does not apply), exactly like `del`.
// ---------------------------------------------------------------------------

#[test]
fn remove_from_param_lowers_and_emits() {
    // A list PARAM has no spare capacity, but `remove` never grows, so it is
    // accepted (the record only shrinks in place; the base-pointer never moves).
    let src = "def go(xs: list[int]) -> int:\n    xs.remove(1)\n    return len(xs)\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("param remove must lower+emit: {e}"));
    assert!(
        wat.contains("call $__wasm_list_remove_i64"),
        "param remove must call the remove helper (removal has no capacity precondition)"
    );
    // A param `go` can't be zero-arg-run under --run-all-exports, so this is an
    // emit+assemble witness, not an execute witness.
    if wasm_runtime_available() {
        assemble(&wat, "remove_param");
    }
}

// ---------------------------------------------------------------------------
// HONEST boundary — an absent value and any value on an empty list TRAP.
// ---------------------------------------------------------------------------

#[test]
fn remove_absent_traps() {
    // xs = [1, 2]; xs.remove(5) → CPython ValueError → `unreachable` trap.
    let src =
        "def go() -> int:\n    xs: list[int] = [1, 2]\n    xs.remove(5)\n    return len(xs)\n";
    assert_traps(src, "remove_absent");
}

#[test]
fn remove_on_empty_traps() {
    // xs = []; xs.remove(1) → CPython ValueError → `unreachable` trap.
    let src = "def go() -> int:\n    xs: list[int] = []\n    xs.remove(1)\n    return len(xs)\n";
    assert_traps(src, "remove_empty");
}

// ---------------------------------------------------------------------------
// HONEST REFUSAL — a `list[bool]` (i32 stride) needs the i32 helper twin.
// ---------------------------------------------------------------------------

#[test]
fn remove_into_bool_list_refuses_honestly() {
    // `list[bool]` elements are i32 (4-byte), not the 8-byte word the remove
    // helper's shift moves — refused, exactly like `insert`.
    let src = "def go() -> int:\n    xs: list[bool] = [True, False, True]\n    xs.remove(False)\n    return len(xs)\n";
    let err = emit(src).expect_err("bool-list remove must refuse");
    assert!(
        err.contains("remove") && err.contains("i32"),
        "bool-list-remove refusal should name the i32 element kind, got: {err}"
    );
}
