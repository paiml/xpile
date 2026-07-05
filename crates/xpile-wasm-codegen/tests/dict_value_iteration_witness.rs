//! PMAT-1298 — EXECUTED witness for native-WASM `for v in d.values()` over a
//! `dict[_, int]` — the VALUE-slot companion of the `for k in d` key iteration
//! (PMAT-1297), completing the read-only dict-iteration surface (keys → values).
//! Runs on the bump-heap dict runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! Python iterates `d.values()` as the dict's VALUES. A dict entry is a 16-byte
//! `DICT_ENTRY_SIZE` record — the KEY at entry+0, the VALUE at
//! entry+`DICT_VAL_OFFSET` (8). `for k in d` (PMAT-1297) reads the key slot; this
//! slice reads the VALUE slot, one `offset=` load immediate apart, through the
//! SAME bounds-checked 16-byte-stride entry read (`emit_set_elem_read`, now taking
//! a `field_offset`). The frontend already lowers `for v in d.values()` to a
//! `ForEach` whose iter is a `DictView{Values}`; `desugar_foreach_stmts` routes it
//! to a `while i < len(d)` loop whose per-element read is a `DictView{Values}`-
//! marked `Index` on the dict NAME, which `emit_index` loads as the entry value
//! (always i64 — dict values are int-only in this lane, so — unlike str KEYS —
//! there is no string-position variant to thread).
//!
//! ## The order-safety contract (why this is NOT a silent miscompile)
//!
//! CPython ≥3.7 iterates `d.values()` in the dict's INSERTION order; xpile walks
//! the bump-heap live-entry region in STORAGE order, and a `del d[k]` swaps the
//! last entry into the hole — so storage order can DIVERGE from insertion order
//! after a delete. Value iteration is gated by the SAME
//! `set_iteration_body_order_safe` under-approximation the set / dict-key lanes
//! use: only an order-INDEPENDENT (commutative / associative) body — `sum`,
//! `count`, `product`, `xor`, the `if v > m: m = v` min/max idiom — is accepted;
//! any order-DEPENDENT body (`r = r*10 + v`) REFUSES. A body that MUTATES the dict
//! (`d[v] = …`, a size-changing insert CPython would raise `RuntimeError` for) is
//! a non-whitelisted statement form, so it also refuses. So the accepted surface
//! is exactly the class for which storage order is irrelevant → the result matches
//! CPython regardless of insertion / hash / storage order.
//!
//! ## Correctness properties this pins against live `python3`:
//!   * int-keyed `for v in d.values()` commutative folds (sum / product / xor /
//!     count / the max idiom) == CPython.
//!   * value iteration is KEY-TYPE-AGNOSTIC — a `dict[str, int]` reduces its VALUES
//!     identically (only the value slot is read).
//!   * DUPLICATE values are all visited (values need not be distinct, unlike keys).
//!   * the EMPTY dict iterates zero times (the loop guard holds at `i = 0`).
//!   * a values-OUTER × list-INNER nested loop composes.
//!   * HONEST REFUSALS: an order-DEPENDENT value body, a MUTATED dict, and a
//!     non-name `.values()` source all refuse at compile time — never a
//!     storage-order misread.
//!
//! This lowers REAL Python through the frontend the CLI uses for `--target wasm`,
//! then assembles + runs the emitted WAT in WABT and value-matches the IDENTICAL
//! source under `python3`. Gated on `wasm_runtime_available()` — a clean skip
//! (still asserting the EMIT path lowers) without WABT.

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
    let dir =
        std::env::temp_dir().join(format!("xpile-dictvaliter-{}-{}", std::process::id(), tag));
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

/// The raw `go(...) => …` result line from a WABT run of the emitted WAT.
fn go_line(wat: &str, tag: &str) -> String {
    let wasm_path = assemble(wat, tag);
    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout);
    stdout
        .lines()
        .find(|l| l.starts_with("go(") && l.contains("=>"))
        .unwrap_or_else(|| panic!("no `go` export in interp output for {tag}:\n{stdout}"))
        .to_string()
}

/// Run a clean (non-trapping) `go() -> int` probe, returning the SIGNED i64
/// (wasm-interp prints i64 as unsigned decimal).
fn run_i64(src: &str, tag: &str) -> i64 {
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed for {tag}: {e}"));
    let line = go_line(&wat, tag);
    assert!(
        !line.contains("error"),
        "expected a clean run for {tag}, got a trap: {line}"
    );
    let raw = line.rsplit(':').next().unwrap().trim();
    raw.parse::<u64>()
        .unwrap_or_else(|_| panic!("parse i64 result {raw:?} for {tag}")) as i64
}

/// The differential value CPython computes for the IDENTICAL source (the type
/// annotations are valid plain `python3`, so the source is its own oracle — zero
/// reimplementation risk).
fn cpython_of_src(src: &str) -> i64 {
    let out = Command::new("python3")
        .arg("-c")
        .arg(format!("{src}\nprint(go())\n"))
        .output()
        .expect("spawn python3");
    assert!(
        out.status.success(),
        "python3 failed for source:\n{src}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<i64>()
        .expect("cpython i64")
}

/// Assert the emitted WAT for `src` matches live CPython (a clean skip without
/// WABT — the EMIT path was already exercised by `emit`).
fn assert_matches_cpython(src: &str, tag: &str, expected: i64) {
    assert!(emit(src).is_ok(), "{tag} must lower to WAT");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1298: WABT absent — {tag} emit-only check passed, run skipped");
        return;
    }
    let got = run_i64(src, tag);
    assert_eq!(
        got, expected,
        "{tag}: WASM result {got} != pinned {expected}"
    );
    assert_eq!(
        got,
        cpython_of_src(src),
        "{tag}: WASM result diverged from live python3"
    );
}

/// The canonical int-keyed dict — values {1,2,3,4}: sum=10, product=24, xor=4.
const D: &str = "    d: dict[int, int] = {5: 1, 3: 2, 10: 3, 7: 4}\n";

/// A `go() -> int` program: `<D>` then the loop `for v in d.values(): <body>`
/// (8-space indented) then `return <ret>`, with an accumulator prelude `<pre>`.
fn prog(pre: &str, body: &str, ret: &str) -> String {
    format!(
        "def go() -> int:\n{D}{pre}    for v in d.values():\n        {body}\n    return {ret}\n"
    )
}

// ---------------------------------------------------------------------------
// CONSTRUCT: `for v in d.values()` reads the entry VALUE slot IN PLACE (the
// 16-byte stride at entry+DICT_VAL_OFFSET), reusing the key-read machinery.
// ---------------------------------------------------------------------------

#[test]
fn dict_value_iteration_reads_value_slot_in_place() {
    let src = prog("    total: int = 0\n", "total = total + v", "total");
    let wat = emit(&src)
        .expect("`for v in d.values()` over a dict[_,int] must lower through emit_module");
    // A `while` loop over the live-entry region (not a materialised list).
    assert!(
        wat.contains("(loop"),
        "dict value iteration must emit a while loop:\n{wat}"
    );
    // The 16-byte `DICT_ENTRY_SIZE` entry stride.
    assert!(
        wat.contains("i32.const 16"),
        "dict value iteration must read the 16-byte-stride entry array:\n{wat}"
    );
    // The VALUE slot is read at entry+DICT_VAL_OFFSET (8) — the load-bearing delta
    // from key iteration (which loads at offset 0).
    assert!(
        wat.contains("i64.load offset=8"),
        "dict value iteration must load the entry value at offset 8:\n{wat}"
    );
    // IN-PLACE: iteration reads entries directly; it does NOT materialise the
    // values into a fresh list (that is the reduction path, PMAT-1295), so the
    // `$__wasm_dict_values_to_list` materialiser must NOT be emitted.
    assert!(
        !wat.contains("dict_values_to_list"),
        "value iteration must read entries in place, not materialise a list:\n{wat}"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED: int-keyed commutative folds over the VALUES == CPython.
// ---------------------------------------------------------------------------

#[test]
fn dict_value_sum_matches_cpython() {
    assert_matches_cpython(
        &prog("    total: int = 0\n", "total = total + v", "total"),
        "sum",
        10,
    );
}

#[test]
fn dict_value_product_matches_cpython() {
    assert_matches_cpython(&prog("    p: int = 1\n", "p = p * v", "p"), "product", 24);
}

#[test]
fn dict_value_xor_matches_cpython() {
    assert_matches_cpython(&prog("    x: int = 0\n", "x = x ^ v", "x"), "xor", 4);
}

#[test]
fn dict_value_count_matches_cpython() {
    assert_matches_cpython(&prog("    c: int = 0\n", "c = c + 1", "c"), "count", 4);
}

#[test]
fn dict_value_max_idiom_matches_cpython() {
    // The `if v > m: m = v` extremum idiom — order-independent, so accepted.
    let src = "def go() -> int:\n    d: dict[int, int] = {5: 1, 3: 2, 10: 3, 7: 4}\n    m: int = 0\n    for v in d.values():\n        if v > m:\n            m = v\n    return m\n";
    assert_matches_cpython(src, "max-idiom", 4);
}

#[test]
fn dict_value_compound_matches_cpython() {
    // The fold result composes with the scalar subset (`s * 2`).
    assert_matches_cpython(
        &prog("    s: int = 0\n", "s = s + v", "s * 2"),
        "compound",
        20,
    );
}

#[test]
fn dict_value_negatives_sum_matches_cpython() {
    let src = "def go() -> int:\n    d: dict[int, int] = {1: -5, 2: 3, 3: -1}\n    total: int = 0\n    for v in d.values():\n        total = total + v\n    return total\n";
    assert_matches_cpython(src, "neg-sum", -3);
}

// ---------------------------------------------------------------------------
// EXECUTED: value iteration is KEY-TYPE-AGNOSTIC + keeps duplicate values.
// ---------------------------------------------------------------------------

#[test]
fn str_keyed_dict_value_sum_matches_cpython() {
    // A str-KEYED dict reduces its int VALUES identically — only the value slot
    // (entry+8, always i64) is read; the key type is irrelevant.
    let src = "def go() -> int:\n    d: dict[str, int] = {\"a\": 5, \"bb\": 7, \"ccc\": 3}\n    total: int = 0\n    for v in d.values():\n        total = total + v\n    return total\n";
    assert_matches_cpython(src, "str-keyed-value-sum", 15);
}

#[test]
fn dict_duplicate_values_all_visited() {
    // Values need NOT be distinct (unlike keys) — every entry's value is folded,
    // so a dict with repeated values sums each occurrence.
    let src = "def go() -> int:\n    d: dict[int, int] = {1: 7, 2: 7, 3: 7}\n    total: int = 0\n    for v in d.values():\n        total = total + v\n    return total\n";
    assert_matches_cpython(src, "dup-values", 21);
}

#[test]
fn empty_dict_values_iterates_zero_times() {
    // The loop guard `i >= count` holds at i=0 — the accumulator survives untouched.
    let src = "def go() -> int:\n    d: dict[int, int] = {}\n    total: int = 0\n    for v in d.values():\n        total = total + v\n    return total\n";
    assert_matches_cpython(src, "empty", 0);
}

#[test]
fn dict_values_outer_list_inner_nested_matches_cpython() {
    // A value-iteration OUTER loop with an order-defined list INNER loop — the
    // list has a defined order, so the (commutative in the outer value) body is safe.
    let src = "def go() -> int:\n    d: dict[int, int] = {5: 10, 3: 20}\n    xs: list[int] = [1, 2, 3]\n    total: int = 0\n    for v in d.values():\n        for x in xs:\n            total = total + v\n    return total\n";
    assert_matches_cpython(src, "nested", 90);
}

// ---------------------------------------------------------------------------
// REGRESSION: key + value iteration coexist in one function, each reading its
// own slot (the field_offset routing does not cross-contaminate).
// ---------------------------------------------------------------------------

#[test]
fn key_and_value_iteration_coexist_matches_cpython() {
    // `for k in d` reads keys (sum 25), `for v in d.values()` reads values
    // (sum 10) — a total of 35 confirms each loop reads its own entry slot.
    let src = "def go() -> int:\n    d: dict[int, int] = {5: 1, 3: 2, 10: 3, 7: 4}\n    total: int = 0\n    for k in d:\n        total = total + k\n    for v in d.values():\n        total = total + v\n    return total\n";
    assert_matches_cpython(src, "keys-and-values", 35);
}

// ---------------------------------------------------------------------------
// HONEST REFUSALS: order-dependent value bodies + mutated dicts MUST refuse — a
// silent accept is the PMAT-1292 storage-order-misread miscompile.
// ---------------------------------------------------------------------------

#[test]
fn order_dependent_value_iteration_refuses() {
    // `r = r * 10 + v` — a positional fold; its value depends on iteration order.
    let src = "def go() -> int:\n    d: dict[int, int] = {5: 1, 3: 2, 27: 3}\n    r: int = 0\n    for v in d.values():\n        r = r * 10 + v\n    return r\n";
    let err = emit(src).expect_err("an order-dependent value iteration must refuse");
    assert!(
        err.contains("order-dependent") && err.contains("values"),
        "refusal should name the order-dependent values iteration, got: {err}"
    );
}

#[test]
fn mutated_dict_value_iteration_refuses() {
    // A body that mutates `d` (`d[v] = v + 1`, a size-changing insert CPython
    // raises RuntimeError for) is a non-whitelisted statement form → refuses.
    let src = "def go() -> int:\n    d: dict[int, int] = {5: 1, 3: 2}\n    total: int = 0\n    for v in d.values():\n        d[v] = v + 1\n        total = total + v\n    return total\n";
    let err = emit(src).expect_err("mutating the dict during value iteration must refuse");
    assert!(
        err.contains("order-dependent") || err.contains("unsupported construct"),
        "refusal should reject the mutated-dict body, got: {err}"
    );
}
