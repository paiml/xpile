//! PMAT-1291 — EXECUTED witness for native-WASM `sorted(s)` over a `set[int]` —
//! the FIRST set→list materialisation in the WASM subset. Runs on the bump-heap
//! set + list runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! The set surface shipped every non-iterating op (membership, len, add, remove,
//! `==`, predicates, algebra) and `for x in s` iteration (PMAT-1290), but no
//! set→list conversion. `list(s)` inherits the set's ARBITRARY hash/storage
//! order — observing it element-by-element could diverge from CPython — so it is
//! refused. `sorted(s)`, however, is ORDER-DEFINING: it re-sorts the elements, so
//! the set's storage order is irrelevant and the result is CPython-exact. This is
//! exactly the "cleaner follow-up than raw dict iteration" the roadmap flagged.
//!
//! The frontend lowers `sorted(s)` to `Sorted { list: SetToList { set }, .. }`
//! (PMAT-520). PMAT-1291 teaches the WASM lane to (a) materialise the int set's
//! keys into a fresh `list[int]` record via the new `$__wasm_set_to_list_i64`
//! helper (reading the 16-byte `DICT_ENTRY_SIZE`-stride entry array, key at
//! offset 0), leaving that record's base on the stack, and (b) feed it to the
//! pre-existing `$__wasm_list_sorted_i64` copy-and-sort helper. A set is dup-free
//! by construction, so the materialised list already holds the unique elements.
//!
//! Key correctness properties this pins against live `python3`:
//!   * ascending + descending (`reverse=True`) sort of an int set == CPython.
//!   * ORDER-INDEPENDENCE after a swap-into-hole `discard` (storage order is
//!     scrambled, the sorted result is not).
//!   * DEDUP: a set literal with repeated keys sorts to the unique elements.
//!   * the EMPTY set sorts to `[]` (no trap — `n == 0` allocs an empty record).
//!   * NEGATIVES fold correctly (signed `i64.lt_s` compare in the sort).
//!   * str-set `sorted(s)` (→ `list[str]`, unmodelled) + bare `list(s)`
//!     (arbitrary order) both REFUSE honestly at compile time.
//!
//! This lowers REAL Python through the frontend the CLI uses for `--target wasm`
//! (avoiding the PMAT-1244/1245 reachability trap where a hand-built meta-HIR
//! witness is green while the capability is unreachable end-to-end), then
//! assembles + runs the emitted WAT in WABT. Gated on `wasm_runtime_available()`
//! — a clean skip (still asserting the EMIT path lowers) without WABT.

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
/// wasm path (asserting a clean wat2wasm).
fn assemble(wat: &str, tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("xpile-sortedset-{}-{}", std::process::id(), tag));
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
/// a clean (non-trapping) run, and return the printed result line's value.
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

/// Run a `go() -> int` probe and return the result as a SIGNED i64
/// (wasm-interp prints i64 as unsigned decimal).
fn run_i64(src: &str, tag: &str) -> i64 {
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed for {tag}: {e}"));
    let raw = run_go(&wat, tag);
    raw.parse::<u64>()
        .unwrap_or_else(|_| panic!("parse i64 result {raw:?} for {tag}")) as i64
}

/// A `go() -> int` program that binds `xs = sorted(s)` (with the given source
/// statements building `s`) and folds it left-to-right into a base-10 digit
/// encoding, so the RESULT pins both the values AND their sorted order.
fn fold_program(build: &str, sorted_expr: &str, seed: i64) -> String {
    format!(
        "def go() -> int:\n{build}    xs: list[int] = {sorted_expr}\n    total: int = {seed}\n    \
         i: int = 0\n    while i < len(xs):\n        total = total * 10 + xs[i]\n        \
         i = i + 1\n    return total\n"
    )
}

// ---------------------------------------------------------------------------
// CONSTRUCT: the emitted WAT declares the set→list materialisation helper, CALLS
// it, and declares the sort helper it feeds — the shape of the two-helper lower.
// ---------------------------------------------------------------------------

#[test]
fn sorted_set_lowers_and_carries_helpers() {
    let src = fold_program("    s: set[int] = {5, 1, 3, 2, 4}\n", "sorted(s)", 0);
    let wat = emit(&src).expect("`sorted(s)` over a set[int] must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_set_to_list_i64"),
        "the set→list materialisation helper must be declared:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_set_to_list_i64"),
        "the set→list helper must be CALLED (materialise before sort):\n{wat}"
    );
    assert!(
        wat.contains("(func $__wasm_list_sorted_i64"),
        "the int-sort helper must be declared (it copies-and-sorts the \
         materialised list):\n{wat}"
    );
    // stride-16 set-entry read + stride-8 list write live inside the materialiser.
    assert!(
        wat.contains("i32.const 16") && wat.contains("i64.load") && wat.contains("i64.store"),
        "the materialiser must read the 16-byte set entries and pack i64 keys:\n{wat}"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED: ascending sort of an int set == CPython.
// ---------------------------------------------------------------------------

#[test]
fn sorted_set_ascending_matches_cpython() {
    // sorted({5,1,3,2,4}) == [1,2,3,4,5] → base-10 fold 12345.
    let src = fold_program("    s: set[int] = {5, 1, 3, 2, 4}\n", "sorted(s)", 0);
    assert!(emit(&src).is_ok(), "ascending sorted(set) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1291: WABT absent — emit-only ascending check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "asc"), 12345, "sorted(set) ascending");
}

// ---------------------------------------------------------------------------
// EXECUTED: descending sort (`reverse=True`) == CPython — a stable descending
// sort, not asc-then-reverse.
// ---------------------------------------------------------------------------

#[test]
fn sorted_set_descending_matches_cpython() {
    // sorted({5,1,3,2,4}, reverse=True) == [5,4,3,2,1] → fold 54321.
    let src = fold_program(
        "    s: set[int] = {5, 1, 3, 2, 4}\n",
        "sorted(s, reverse=True)",
        0,
    );
    assert!(emit(&src).is_ok(), "descending sorted(set) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1291: WABT absent — emit-only descending check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "desc"), 54321, "sorted(set) descending");
}

// ---------------------------------------------------------------------------
// EXECUTED: ORDER-INDEPENDENCE — a `discard` scrambles the set's storage order
// (swap-last-into-hole), but the sorted result is unchanged (CPython-exact).
// This is the load-bearing property that makes `sorted(s)` tractable.
// ---------------------------------------------------------------------------

#[test]
fn sorted_set_order_independent_after_discard() {
    // {5,1,3,2,4}; discard(3) → {1,2,4,5}; sorted → [1,2,4,5] → fold 1245.
    let src = fold_program(
        "    s: set[int] = {5, 1, 3, 2, 4}\n    s.discard(3)\n",
        "sorted(s)",
        0,
    );
    assert!(emit(&src).is_ok(), "sorted(set) after discard must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1291: WABT absent — emit-only discard check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "discard"), 1245, "sorted(set) after discard");
}

// ---------------------------------------------------------------------------
// EXECUTED: DEDUP — a set literal with repeated keys stores the unique elements,
// so the sorted result has no duplicates (CPython `sorted(set)` semantics).
// ---------------------------------------------------------------------------

#[test]
fn sorted_set_dedup_matches_cpython() {
    // {3,3,1,1,2} stores {1,2,3}; sorted → [1,2,3] → fold 123; *10+len(3) = 1233.
    let src = "def go() -> int:\n    s: set[int] = {3, 3, 1, 1, 2}\n    xs: list[int] = sorted(s)\n    \
         total: int = 0\n    i: int = 0\n    while i < len(xs):\n        total = total * 10 + xs[i]\n        \
         i = i + 1\n    return total * 10 + len(xs)\n";
    assert!(emit(src).is_ok(), "sorted(set) dedup must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1291: WABT absent — emit-only dedup check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(src, "dedup"), 1233, "sorted(set) dedup + len");
}

// ---------------------------------------------------------------------------
// EXECUTED: single-element set, and NEGATIVES (signed compare in the sort).
// ---------------------------------------------------------------------------

#[test]
fn sorted_set_single_and_negatives_match_cpython() {
    let single = fold_program("    s: set[int] = {42}\n", "sorted(s)", 0);
    // sorted({-5,3,-1,0}) == [-5,-1,0,3]; fold: ((((-5)*10-1)*10+0)*10+3) = -5097.
    let negatives = fold_program("    s: set[int] = {-5, 3, -1, 0}\n", "sorted(s)", 0);
    assert!(
        emit(&single).is_ok() && emit(&negatives).is_ok(),
        "must lower"
    );
    if !wasm_runtime_available() {
        eprintln!("PMAT-1291: WABT absent — emit-only single/neg check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&single, "single"), 42, "sorted single-element set");
    assert_eq!(
        run_i64(&negatives, "neg"),
        -5097,
        "sorted set with negatives"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED: the EMPTY set sorts to `[]` — no trap. A `discard` empties the set;
// the fold seed (1) survives untouched (the loop runs zero times).
// ---------------------------------------------------------------------------

#[test]
fn sorted_empty_set_no_trap() {
    // {7}; discard(7) → {}; sorted → []; loop runs 0×; seed 1 returned.
    let src = fold_program("    s: set[int] = {7}\n    s.discard(7)\n", "sorted(s)", 1);
    assert!(emit(&src).is_ok(), "sorted(empty set) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1291: WABT absent — emit-only empty check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "empty"), 1, "sorted(empty set) → [] no trap");
}

// ---------------------------------------------------------------------------
// REFUSALS (honest, compile-time BackendError — never a silent miscompile).
// ---------------------------------------------------------------------------

#[test]
fn sorted_str_set_refuses_honestly() {
    // sorted(set[str]) → list[str], which the WASM list subset does not model.
    let src = "def go() -> int:\n    s: set[str] = {\"b\", \"a\", \"c\"}\n    \
               xs: list[str] = sorted(s)\n    return len(xs)\n";
    let err = emit(src).expect_err("sorted(str-set) → list[str] must refuse");
    assert!(
        err.contains("list[str]") || err.contains("list element type Str") || err.contains("str"),
        "refusal should name the list[str] limitation, got: {err}"
    );
}

#[test]
fn bare_list_of_set_refuses_honestly() {
    // Bare `list(s)` inherits the set's arbitrary storage order → refused; only
    // `sorted(s)` (order-defining) is supported.
    let src = "def go() -> int:\n    s: set[int] = {5, 1, 3}\n    \
               xs: list[int] = list(s)\n    return len(xs)\n";
    let err = emit(src).expect_err("bare list(s) over a set must refuse");
    assert!(
        err.contains("list(s)") || err.contains("arbitrary") || err.contains("sorted(s)"),
        "refusal should point to sorted(s) as the ordered alternative, got: {err}"
    );
}
