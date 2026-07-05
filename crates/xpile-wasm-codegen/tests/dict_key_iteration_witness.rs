//! PMAT-1297 — EXECUTED witness for native-WASM `for k in d` over a `dict[int, _]`
//! / `dict[str, _]` — the FIRST dict ITERATION in the WASM subset, completing the
//! hash-container iteration arc (set → dict) opened by PMAT-1290 (`for x in s`).
//! Runs on the bump-heap dict runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! Python iterates a dict as its KEYS. A dict and a set share the IDENTICAL
//! open-assoc region — an i32 live-count @ base+0, 16-byte `DICT_ENTRY_SIZE`
//! entries @ base+8, the KEY @ entry+0 (a dict additionally stores the value at
//! entry+8, which key iteration never reads). So the set-iteration machinery
//! PMAT-1290 built (`emit_set_elem_read`, gated on the synthetic foreach counter)
//! reads a dict's entry KEY verbatim. `for k in d` desugars (in
//! `desugar_foreach_stmts`) to a `while i < len(d)` loop whose per-element read is
//! `d[i]` — an `Expr::Index` on the dict NAME — which `emit_index` (int keys) and
//! the string-position `emit_str_expr` (str keys) now route to that entry read.
//!
//! ## The order-safety contract (why this is NOT a silent miscompile)
//!
//! CPython ≥3.7 preserves a dict's INSERTION order; xpile walks the bump-heap
//! live-entry region in STORAGE order, and a `del d[k]` swaps the last entry into
//! the hole — so storage order can DIVERGE from insertion order after a delete.
//! Rather than replicate CPython's exact table, `for k in d` is gated by the SAME
//! `set_iteration_body_order_safe` under-approximation the set lane uses: only an
//! order-INDEPENDENT (commutative / associative) body — `sum`, `count`, `product`,
//! `xor`, the `if k < m: m = k` min/max idiom, a str-key `len` fold — is accepted;
//! any order-DEPENDENT body (`r = r*10 + k`, a "first key" flag) REFUSES. A dict
//! that is MUTATED in the function arrives from the frontend as a keys-snapshot +
//! size-change guard (`dict_guard`) and also refuses (unmodelled). So the accepted
//! surface is exactly the class for which storage order is irrelevant → the result
//! matches CPython regardless of insertion / hash order.
//!
//! ## Correctness properties this pins against live `python3`:
//!   * int-keyed `for k in d` commutative folds (sum / product / sum-of-squares /
//!     xor / count / the min idiom) == CPython.
//!   * a str-keyed `for k in d` fold over `len(k)` (the loop var is a str local,
//!     riding an i32 base-pointer) == CPython — incl. a `len(k) > 1` count.
//!   * the EMPTY dict iterates zero times (the loop guard holds at `i = 0`).
//!   * a dict-OUTER × list-INNER nested loop composes.
//!   * HONEST REFUSALS: an order-DEPENDENT body, a MUTATED dict, and the explicit
//!     `.keys()` view iteration all refuse at compile time — never a storage-order
//!     misread. (`for v in d.values()` VALUE iteration is supported as of PMAT-1298
//!     — see `dict_value_iteration_witness.rs`.)
//!
//! This lowers REAL Python through the frontend the CLI uses for `--target wasm`
//! (avoiding the PMAT-1244/1245 reachability trap), then assembles + runs the
//! emitted WAT in WABT and value-matches the IDENTICAL source under `python3`.
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers) without WABT.

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
        std::env::temp_dir().join(format!("xpile-dictkeyiter-{}-{}", std::process::id(), tag));
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
        eprintln!("PMAT-1297: WABT absent — {tag} emit-only check passed, run skipped");
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

/// The canonical int-keyed dict — keys {5,3,10,7} (the SAME key set the reduce
/// witnesses use): sum=25, product=1050, min=3, xor=11.
const D: &str = "    d: dict[int, int] = {5: 1, 3: 2, 10: 3, 7: 4}\n";

/// A `go() -> int` program: `<D>` then the loop `for k in d: <body>` (8-space
/// indented) then `return <ret>`, with an accumulator prelude line `<pre>`.
fn prog(pre: &str, body: &str, ret: &str) -> String {
    format!("def go() -> int:\n{D}{pre}    for k in d:\n        {body}\n    return {ret}\n")
}

// ---------------------------------------------------------------------------
// CONSTRUCT: `for k in d` reads the dict's entry KEY IN PLACE (the 16-byte
// stride, no materialisation) — the set-iteration machinery reused verbatim.
// ---------------------------------------------------------------------------

#[test]
fn dict_key_iteration_reads_entry_in_place() {
    let src = prog("    total: int = 0\n", "total = total + k", "total");
    let wat = emit(&src).expect("`for k in d` over a dict[int,_] must lower through emit_module");
    // A `while` loop over the live-entry region (not a materialised list).
    assert!(
        wat.contains("(loop"),
        "dict key iteration must emit a while loop:\n{wat}"
    );
    // The 16-byte `DICT_ENTRY_SIZE` entry stride — the entry-key read.
    assert!(
        wat.contains("i32.const 16"),
        "dict key iteration must read the 16-byte-stride entry array:\n{wat}"
    );
    // IN-PLACE: iteration reads entries directly; it does NOT materialise the
    // keys into a fresh list (that is the reduction path, PMAT-1294).
    assert!(
        !wat.contains("call $__wasm_set_to_list"),
        "dict key iteration must read entries in place, not materialise a list:\n{wat}"
    );
}

#[test]
fn str_keyed_dict_iteration_lowers() {
    let src = "def go() -> int:\n    d: dict[str, int] = {\"ab\": 1, \"c\": 2, \"def\": 3}\n    total: int = 0\n    for k in d:\n        total = total + len(k)\n    return total\n";
    assert!(
        emit(src).is_ok(),
        "`for k in d` over a str-keyed dict with a `len(k)` fold must lower"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED: int-keyed commutative folds == CPython.
// ---------------------------------------------------------------------------

#[test]
fn dict_key_sum_matches_cpython() {
    assert_matches_cpython(
        &prog("    total: int = 0\n", "total = total + k", "total"),
        "sum",
        25,
    );
}

#[test]
fn dict_key_product_matches_cpython() {
    assert_matches_cpython(&prog("    p: int = 1\n", "p = p * k", "p"), "product", 1050);
}

#[test]
fn dict_key_sum_of_squares_matches_cpython() {
    // A commutative fold whose element term is NOT expressible as a builtin
    // reduction — genuine new expressiveness over `sum(d)` / `min(d)`.
    assert_matches_cpython(
        &prog("    r: int = 0\n", "r = r + k * k", "r"),
        "sumsq",
        183,
    );
}

#[test]
fn dict_key_xor_matches_cpython() {
    assert_matches_cpython(&prog("    x: int = 0\n", "x = x ^ k", "x"), "xor", 11);
}

#[test]
fn dict_key_count_matches_cpython() {
    assert_matches_cpython(&prog("    c: int = 0\n", "c = c + 1", "c"), "count", 4);
}

#[test]
fn dict_key_min_idiom_matches_cpython() {
    // The `if k < m: m = k` extremum idiom — order-independent, so accepted.
    let src = "def go() -> int:\n    d: dict[int, int] = {5: 1, 3: 2, 10: 3, 7: 4}\n    m: int = 1000\n    for k in d:\n        if k < m:\n            m = k\n    return m\n";
    assert_matches_cpython(src, "min-idiom", 3);
}

#[test]
fn dict_key_compound_matches_cpython() {
    // The fold result composes with the scalar subset (`s * 2`).
    assert_matches_cpython(
        &prog("    s: int = 0\n", "s = s + k", "s * 2"),
        "compound",
        50,
    );
}

#[test]
fn empty_dict_iterates_zero_times() {
    // The loop guard `i >= count` holds at i=0 — the accumulator survives untouched.
    let src = "def go() -> int:\n    d: dict[int, int] = {}\n    total: int = 0\n    for k in d:\n        total = total + k\n    return total\n";
    assert_matches_cpython(src, "empty", 0);
}

#[test]
fn dict_outer_list_inner_nested_matches_cpython() {
    // A dict-key iteration OUTER loop with an order-defined list INNER loop — the
    // list has a defined order, so the (commutative in the outer key) body is safe.
    let src = "def go() -> int:\n    d: dict[int, int] = {5: 1, 3: 2}\n    xs: list[int] = [1, 2, 3]\n    total: int = 0\n    for k in d:\n        for x in xs:\n            total = total + k\n    return total\n";
    assert_matches_cpython(src, "nested", 24);
}

// ---------------------------------------------------------------------------
// EXECUTED: str-keyed folds (the loop var is a str local) == CPython.
// ---------------------------------------------------------------------------

#[test]
fn str_keyed_dict_len_fold_matches_cpython() {
    let src = "def go() -> int:\n    d: dict[str, int] = {\"ab\": 1, \"c\": 2, \"def\": 3}\n    total: int = 0\n    for k in d:\n        total = total + len(k)\n    return total\n";
    assert_matches_cpython(src, "str-len-fold", 6);
}

#[test]
fn str_keyed_dict_long_key_count_matches_cpython() {
    let src = "def go() -> int:\n    d: dict[str, int] = {\"ab\": 1, \"c\": 2, \"def\": 3}\n    c: int = 0\n    for k in d:\n        if len(k) > 1:\n            c = c + 1\n    return c\n";
    assert_matches_cpython(src, "str-longkey-count", 2);
}

// ---------------------------------------------------------------------------
// HONEST REFUSALS: order-dependent bodies, mutated dicts, and value views MUST
// refuse — a silent accept is the PMAT-1292 storage-order-misread miscompile.
// ---------------------------------------------------------------------------

#[test]
fn order_dependent_dict_iteration_refuses() {
    // `r = r * 10 + k` — a positional fold; its value depends on iteration order.
    let src = "def go() -> int:\n    d: dict[int, int] = {50: 1, 3: 2, 27: 3}\n    r: int = 0\n    for k in d:\n        r = r * 10 + k\n    return r\n";
    let err = emit(src).expect_err("an order-dependent dict iteration must refuse");
    assert!(
        err.contains("order-dependent") && err.contains("dict"),
        "refusal should name the order-dependent dict iteration, got: {err}"
    );
}

#[test]
fn mutated_dict_iteration_refuses() {
    // A body that mutates `d` (`d[k] = d[k] + 1`) makes the frontend snapshot the
    // keys + guard the size — a form the WASM subset does not model.
    let src = "def go() -> int:\n    d: dict[int, int] = {5: 1, 3: 2}\n    total: int = 0\n    for k in d:\n        d[k] = d[k] + 1\n        total = total + k\n    return total\n";
    let err = emit(src).expect_err("iterating a MUTATED dict must refuse");
    assert!(
        err.contains("MUTATED dict") || err.contains("unsupported construct"),
        "refusal should name the mutated-dict form, got: {err}"
    );
}

#[test]
fn dict_keys_view_iteration_refuses() {
    // `for k in d.keys()` — the EXPLICIT keys view. `for k in d` (the bare dict,
    // PMAT-1297) and `for v in d.values()` (PMAT-1298) are both supported, but the
    // explicit `.keys()` view is not yet routed — it refuses honestly rather than
    // silently misreading (a follow-up would route it through the same key read).
    let src = "def go() -> int:\n    d: dict[int, int] = {5: 1, 3: 2, 27: 3}\n    total: int = 0\n    for k in d.keys():\n        total = total + k\n    return total\n";
    let err = emit(src).expect_err("iterating d.keys() must refuse");
    assert!(
        err.contains("unsupported construct") || err.contains("WASM"),
        "refusal should name the unsupported view iteration, got: {err}"
    );
}
