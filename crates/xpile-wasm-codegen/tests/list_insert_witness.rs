//! PMAT-1282 — EXECUTED FULL-PIPELINE (Python source → meta-HIR → WASM) witness
//! for native-WASM `xs.insert(i, v)` over a named `list[int]` / `list[float]` —
//! the FIRST list-mutation that both GROWS the count AND SHIFTS the tail.
//!
//! ## Why this witness exists
//!
//! `append` (PMAT-1276) grows a list only at the END and `pop` (PMAT-1278) shrinks
//! it only at the end; `insert` is the first op that grows the count AND relocates
//! the elements after the insertion point. The real work lives in a
//! `$__wasm_list_insert_{i64,f64}` helper: clamp the index CPython-style (a negative
//! index adds the length and, if still negative, pins to `0`; an index past the end
//! pins to `len` — never a `Vec::insert`-style panic), shift `[slot, n)` right by one
//! slot walking HIGH→LOW so nothing is overwritten before it is copied, write the
//! value at `slot`, and bump the count header. The record is mutated IN PLACE (the
//! base-pointer never moves), so — like `append` — only a literal-bound list with the
//! spare capacity `emit_list_lit` reserved qualifies, and a full record TRAPS.
//!
//! A hand-built-HIR test would prove the emit handles [`xpile_meta_hir::Stmt::ListInsert`]
//! but NOT that the production `PythonFrontend` emits it from real `.insert(...)` source
//! with the fields the emit reads, nor that the emitted WAT assembles and runs
//! value-identically to CPython. This witness lowers REAL Python through the same
//! profile the CLI uses for `--target wasm`, emits, assembles + runs in WABT, and
//! asserts the executed scalar VALUE-MATCHES CPython on the byte-identical program.
//!
//! ## What each probe certifies
//!
//! * `insert_middle_int` — the canonical case: `[10, 20, 30]` then `xs.insert(1, 99)`
//!   → `[10, 99, 20, 30]`; the full order is pinned via
//!   `xs[0]*1000 + xs[1]*100 + xs[2]*10 + xs[3] == 20130`. Certifies the tail shift
//!   and the count bump.
//! * `insert_front_negative_clamps` — `[1, 2]` then `xs.insert(-100, 9)` → `[9, 1, 2]`:
//!   a huge-magnitude negative index pins to the FRONT (not a wild negative slot).
//! * `insert_past_end_clamps_to_append` — `[1, 2]` then `xs.insert(5, 9)` → `[1, 2, 9]`:
//!   an index past the end pins to `len` (append position), `len == 3`.
//! * `insert_negative_one` — `[10, 20, 30]` then `xs.insert(-1, 99)` → `[10, 20, 99, 30]`:
//!   `-1` inserts BEFORE the last element (CPython `i += n`), pinned via
//!   `xs[2]*10 + xs[3] == 1020`.
//! * `insert_into_empty` — `[]` then `xs.insert(0, 7)` → `[7]`: insert into a
//!   zero-length list (no shift), `xs[0] == 7`.
//! * `insert_float` — the f64 element twin (`f64.store` value): `[1.5, 3.5]` then
//!   `xs.insert(1, 2.5)` → `[1.5, 2.5, 3.5]`, `xs[1] == 2.5`.
//! * `insert_loop_front_build` — repeated front-insert in a loop: `[]` then
//!   `for i in range(4): xs.insert(0, i)` → `[3, 2, 1, 0]`, pinned to `3210`.
//!   Exercises the shift against a list grown one slot at a time.
//! * `insert_then_sum_gates_helper` — insert mixed with `sum(xs)` in one module:
//!   `[1, 2, 3]` then `xs.insert(1, 10)`; `sum(xs) == 16` reads the GROWN count AND
//!   the module must declare `$__wasm_list_sum_i64` alongside the insert helper
//!   (a co-existence / assemble check).
//! * `insert_past_capacity_traps` — the HONEST boundary: inserting past the fixed
//!   capacity a `[]` literal reserved TRAPS (`unreachable`), never a heap overrun.
//! * Honest refusals — a list PARAM, an ALIAS, and a `list[bool]` receiver must
//!   ERROR at compile time (insert grows the count, so it needs the spare capacity
//!   only a literal `list[int]`/`list[float]` binding reserves), never silently
//!   corrupt adjacent heap.
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
    let dir = std::env::temp_dir().join(format!("xpile-listins-{}-{}", std::process::id(), tag));
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

// ---------------------------------------------------------------------------
// EXECUTED canonical insert — insert in the middle, the tail shifts right.
// ---------------------------------------------------------------------------

#[test]
fn insert_middle_int_executes_and_matches_cpython() {
    // xs = [10, 20, 30]; xs.insert(1, 99) → [10, 99, 20, 30].
    let src = "def go() -> int:\n    xs: list[int] = [10, 20, 30]\n    xs.insert(1, 99)\n    return xs[0] * 1000 + xs[1] * 100 + xs[2] * 10 + xs[3]\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("pipeline failed to lower+emit insert: {e}"));
    // The typed insert helper must be declared and called.
    assert!(
        wat.contains("call $__wasm_list_insert_i64"),
        "insert must call the i64 insert helper"
    );
    assert!(
        wat.contains("__wasm_list_insert_i64 (param $base i32) (param $idx i64) (param $val i64)"),
        "insert must declare the i64 insert helper"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1282: WABT absent — emit-only insert check passed, execution skipped");
        return;
    }

    let got = run_int(src, "insert_middle_int");
    assert_eq!(got, 20130, "insert middle int: wasm={got} cpython=20130");
}

// ---------------------------------------------------------------------------
// EXECUTED negative-index clamp — a huge negative index pins to the FRONT.
// ---------------------------------------------------------------------------

#[test]
fn insert_front_negative_clamps_and_matches_cpython() {
    // xs = [1, 2]; xs.insert(-100, 9) → [9, 1, 2]; xs[0] == 9.
    let src =
        "def go() -> int:\n    xs: list[int] = [1, 2]\n    xs.insert(-100, 9)\n    return xs[0]\n";

    assert!(
        emit(src).is_ok(),
        "front-clamp insert must lower: {:?}",
        emit(src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1282: WABT absent — emit-only front-clamp check passed, run skipped");
        return;
    }

    let got = run_int(src, "insert_front_neg");
    assert_eq!(got, 9, "insert front (neg clamp): wasm={got} cpython=9");
}

// ---------------------------------------------------------------------------
// EXECUTED past-end clamp — an index past the end pins to the append position.
// ---------------------------------------------------------------------------

#[test]
fn insert_past_end_clamps_to_append_and_matches_cpython() {
    // xs = [1, 2]; xs.insert(5, 9) → [1, 2, 9]; xs[2]*10 + len == 93.
    let src = "def go() -> int:\n    xs: list[int] = [1, 2]\n    xs.insert(5, 9)\n    return xs[2] * 10 + len(xs)\n";

    if !wasm_runtime_available() {
        assert!(emit(src).is_ok(), "past-end insert must lower");
        eprintln!("PMAT-1282: WABT absent — emit-only past-end check passed, run skipped");
        return;
    }

    let got = run_int(src, "insert_past_end");
    assert_eq!(
        got, 93,
        "insert past end (append clamp): wasm={got} cpython=93"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED `insert(-1, v)` — inserts BEFORE the last element.
// ---------------------------------------------------------------------------

#[test]
fn insert_negative_one_executes_and_matches_cpython() {
    // xs = [10, 20, 30]; xs.insert(-1, 99) → [10, 20, 99, 30]; xs[2]*10 + xs[3] == 1020.
    let src = "def go() -> int:\n    xs: list[int] = [10, 20, 30]\n    xs.insert(-1, 99)\n    return xs[2] * 10 + xs[3]\n";

    if !wasm_runtime_available() {
        assert!(emit(src).is_ok(), "insert(-1) must lower");
        eprintln!("PMAT-1282: WABT absent — emit-only insert(-1) check passed, run skipped");
        return;
    }

    let got = run_int(src, "insert_neg_one");
    assert_eq!(got, 1020, "insert(-1): wasm={got} cpython=1020");
}

// ---------------------------------------------------------------------------
// EXECUTED insert into an EMPTY list — no shift, count 0 → 1.
// ---------------------------------------------------------------------------

#[test]
fn insert_into_empty_executes_and_matches_cpython() {
    // xs = []; xs.insert(0, 7) → [7]; xs[0] == 7.
    let src = "def go() -> int:\n    xs: list[int] = []\n    xs.insert(0, 7)\n    return xs[0]\n";

    if !wasm_runtime_available() {
        assert!(emit(src).is_ok(), "insert-into-empty must lower");
        eprintln!("PMAT-1282: WABT absent — emit-only insert-into-empty check passed, run skipped");
        return;
    }

    let got = run_int(src, "insert_empty");
    assert_eq!(got, 7, "insert into empty: wasm={got} cpython=7");
}

// ---------------------------------------------------------------------------
// EXECUTED float insert — the `f64.store` element twin.
// ---------------------------------------------------------------------------

#[test]
fn insert_float_executes_and_matches_cpython() {
    // xs: list[float] = [1.5, 3.5]; xs.insert(1, 2.5) → [1.5, 2.5, 3.5]; xs[1] == 2.5.
    let src = "def go() -> float:\n    xs: list[float] = [1.5, 3.5]\n    xs.insert(1, 2.5)\n    return xs[1]\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("float insert must lower+emit: {e}"));
    assert!(
        wat.contains("call $__wasm_list_insert_f64"),
        "float insert must call the f64 insert helper"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1282: WABT absent — emit-only float-insert check passed, run skipped");
        return;
    }

    let got = run_f64(src, "insert_float");
    assert!(
        (got - 2.5).abs() < 1e-12,
        "float insert: wasm={got} cpython=2.5"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED loop front-insert — repeated insert(0, i) against a growing list.
// ---------------------------------------------------------------------------

#[test]
fn insert_loop_front_build_executes_and_matches_cpython() {
    // xs = []; for i in range(4): xs.insert(0, i) → [3, 2, 1, 0]; pinned to 3210.
    let src = "def go() -> int:\n    xs: list[int] = []\n    i: int = 0\n    while i < 4:\n        xs.insert(0, i)\n        i = i + 1\n    return xs[0] * 1000 + xs[1] * 100 + xs[2] * 10 + xs[3]\n";

    if !wasm_runtime_available() {
        assert!(emit(src).is_ok(), "loop front-insert must lower");
        eprintln!("PMAT-1282: WABT absent — emit-only loop-front-insert check passed, run skipped");
        return;
    }

    let got = run_int(src, "insert_loop_front");
    assert_eq!(got, 3210, "loop front-insert: wasm={got} cpython=3210");
}

// ---------------------------------------------------------------------------
// EXECUTED insert + sum in one module — sum reads the GROWN count, helpers co-exist.
// ---------------------------------------------------------------------------

#[test]
fn insert_then_sum_gates_helper_executes_and_matches_cpython() {
    // xs = [1, 2, 3]; xs.insert(1, 10) → [1, 10, 2, 3]; sum(xs) == 16.
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    xs.insert(1, 10)\n    return sum(xs)\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("insert+sum must lower+emit: {e}"));
    // Both the insert helper and the sum helper must be declared (gate co-existence).
    assert!(
        wat.contains("$__wasm_list_insert_i64") && wat.contains("$__wasm_list_sum_i64"),
        "insert+sum module must declare BOTH helpers"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1282: WABT absent — emit-only insert+sum check passed, run skipped");
        return;
    }

    let got = run_int(src, "insert_sum");
    assert_eq!(got, 16, "insert then sum: wasm={got} cpython=16");
}

// ---------------------------------------------------------------------------
// HONEST boundary — inserting past the fixed capacity TRAPS (bounded bump heap).
// ---------------------------------------------------------------------------

#[test]
fn insert_past_capacity_traps() {
    // xs = [] reserves LIST_GROWTH_SLACK (16) slots; a 17th insert TRAPS.
    let src = "def go() -> int:\n    xs: list[int] = []\n    i: int = 0\n    while i < 17:\n        xs.insert(0, i)\n        i = i + 1\n    return len(xs)\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("capacity-trap emit failed: {e}"));

    if !wasm_runtime_available() {
        eprintln!("PMAT-1282: WABT absent — emit-only capacity-trap check passed, run skipped");
        return;
    }

    // The module assembles (`unreachable` is valid WAT) but the 17th insert TRAPS.
    let wasm_path = assemble(&wat, "insert_cap");
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
        "insert past capacity must TRAP; got clean run: {combined}"
    );
}

// ---------------------------------------------------------------------------
// HONEST REFUSALS — a param, an alias, and a `list[bool]` receiver.
// ---------------------------------------------------------------------------

#[test]
fn insert_into_param_refuses_honestly() {
    // A list PARAM carries no spare capacity (the caller sized it exactly) — insert
    // GROWS, so it is refused rather than overrunning the record.
    let src = "def go(xs: list[int]) -> int:\n    xs.insert(0, 9)\n    return len(xs)\n";
    let err = emit(src).expect_err("param insert must refuse");
    assert!(
        err.contains("insert") && (err.contains("capacity") || err.contains("param")),
        "param-insert refusal should name the capacity/param reason, got: {err}"
    );
}

#[test]
fn insert_into_alias_refuses_honestly() {
    // An alias binding (`ys = xs`) is not a literal — no spare capacity reserved.
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    ys: list[int] = xs\n    ys.insert(0, 9)\n    return len(ys)\n";
    let err = emit(src).expect_err("alias insert must refuse");
    assert!(
        err.contains("insert") && err.contains("capacity"),
        "alias-insert refusal should name the capacity reason, got: {err}"
    );
}

#[test]
fn insert_into_bool_list_refuses_honestly() {
    // `list[bool]` elements are i32 (not i64/f64) — a bool insert would need an i32
    // helper twin (deferred, like `append`), so it is refused honestly.
    let src = "def go() -> int:\n    xs: list[bool] = [True, False]\n    xs.insert(1, True)\n    return len(xs)\n";
    let err = emit(src).expect_err("bool-list insert must refuse");
    assert!(
        err.contains("insert") && err.contains("bool"),
        "bool-list-insert refusal should name the bool element kind, got: {err}"
    );
}
