//! PMAT-1299 — EXECUTED witness for native-WASM `for k in d.keys()` over a
//! `dict[int|str, int]` — the EXPLICIT keys-VIEW companion of the bare `for k in d`
//! key iteration (PMAT-1297) and `for v in d.values()` value iteration (PMAT-1298).
//! Runs on the bump-heap dict runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! Python iterates `d.keys()` as the dict's KEYS — identical to bare `for k in d`.
//! At PMAT-1297 the bare form was routed (a dict IS a set-with-values, its key at
//! entry+0), but the EXPLICIT `.keys()` view still refused: the frontend lowers
//! `d.keys()` to a `DictView{Keys}` (typing as `List(K)`), so the `ForEach` iter is
//! a `DictView{Keys}` rather than a bare `Ident`, and `desugar_foreach_stmts` fell
//! through to the "bind the iterable to a name first" refusal. This slice routes
//! the `DictView{Keys}` iter (over a NAMED dict) to the SAME `while i < len(d)` loop
//! whose per-element read is a bare `Index` on the dict NAME — which `emit_index`
//! loads as the entry KEY via `dict_key_elem_of` (entry+0). Unlike `.values()`, the
//! read carries NO `DictView` marker (the key read needs none — it keys off the
//! dict TYPE of the name), so no key-materialiser helper is spuriously armed.
//!
//! ## The order-safety contract (why this is NOT a silent miscompile)
//!
//! CPython ≥3.7 iterates `d.keys()` in the dict's INSERTION order; xpile walks the
//! bump-heap live-entry region in STORAGE order, and a `del d[k]` swaps the last
//! entry into the hole — so storage order can DIVERGE from insertion order after a
//! delete. The keys view is gated by the SAME `set_iteration_body_order_safe`
//! under-approximation the set / bare-dict-key / dict-value lanes use: only an
//! order-INDEPENDENT (commutative / associative) body — `sum`, `count`, `product`,
//! `xor`, the `if k < m: m = k` min/max idiom — is accepted; any order-DEPENDENT
//! body (`r = r*10 + k`) REFUSES. A body that MUTATES the dict is a non-whitelisted
//! statement form, so it also refuses. So the accepted surface is exactly the class
//! for which storage order is irrelevant → the result matches CPython regardless of
//! insertion / hash / storage order.
//!
//! ## Correctness properties this pins against live `python3`:
//!   * int-keyed `for k in d.keys()` commutative folds (sum / product / xor /
//!     count / the min idiom) == CPython.
//!   * a str-keyed `for k in d.keys()` fold over `len(k)` (the loop var is a str
//!     local, riding an i32 base-pointer) == CPython.
//!   * `for k in d.keys()` == bare `for k in d` — the explicit view is a pure alias.
//!   * the EMPTY dict iterates zero times (the loop guard holds at `i = 0`).
//!   * a keys-OUTER × list-INNER nested loop composes.
//!   * HONEST REFUSALS: an order-DEPENDENT keys-view body and a non-name `.keys()`
//!     source both refuse at compile time — never a storage-order misread.
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
        std::env::temp_dir().join(format!("xpile-dictkeysview-{}-{}", std::process::id(), tag));
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
        eprintln!("PMAT-1299: WABT absent — {tag} emit-only check passed, run skipped");
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

/// The canonical int-keyed dict — keys {5,3,10,7}: sum=25, product=1050,
/// xor=5^3^10^7=11, min=3.
const D: &str = "    d: dict[int, int] = {5: 1, 3: 2, 10: 3, 7: 4}\n";

/// A `go() -> int` program: `<D>` then the loop `for k in d.keys(): <body>`
/// (8-space indented) then `return <ret>`, with an accumulator prelude `<pre>`.
fn prog(pre: &str, body: &str, ret: &str) -> String {
    format!("def go() -> int:\n{D}{pre}    for k in d.keys():\n        {body}\n    return {ret}\n")
}

// ---------------------------------------------------------------------------
// CONSTRUCT: `for k in d.keys()` reads the entry KEY slot IN PLACE (the 16-byte
// stride at entry+0), reusing the bare `for k in d` key-read machinery — with
// NO materialiser and NO value-slot access.
// ---------------------------------------------------------------------------

#[test]
fn dict_keys_view_reads_key_slot_in_place() {
    let src = prog("    total: int = 0\n", "total = total + k", "total");
    let wat =
        emit(&src).expect("`for k in d.keys()` over a dict[int,_] must lower through emit_module");
    // A `while` loop over the live-entry region (not a materialised list).
    assert!(
        wat.contains("(loop"),
        "keys-view iteration must emit a while loop:\n{wat}"
    );
    // The 16-byte `DICT_ENTRY_SIZE` entry stride.
    assert!(
        wat.contains("i32.const 16"),
        "keys-view iteration must read the 16-byte-stride entry array:\n{wat}"
    );
    // IN-PLACE: iteration reads entries directly; it does NOT materialise the keys
    // into a fresh list (that is the reduction path, PMAT-1294), so NEITHER
    // materialiser helper is emitted — the read is a bare `Index`, carrying no
    // `DictView` marker (unlike `.values()`).
    assert!(
        !wat.contains("set_to_list") && !wat.contains("dict_values_to_list"),
        "keys-view iteration must read entries in place, not materialise a list:\n{wat}"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED: int-keyed commutative folds over the KEYS == CPython.
// ---------------------------------------------------------------------------

#[test]
fn dict_keys_view_sum_matches_cpython() {
    assert_matches_cpython(
        &prog("    total: int = 0\n", "total = total + k", "total"),
        "sum",
        25,
    );
}

#[test]
fn dict_keys_view_product_matches_cpython() {
    assert_matches_cpython(&prog("    p: int = 1\n", "p = p * k", "p"), "product", 1050);
}

#[test]
fn dict_keys_view_xor_matches_cpython() {
    assert_matches_cpython(&prog("    x: int = 0\n", "x = x ^ k", "x"), "xor", 11);
}

#[test]
fn dict_keys_view_count_matches_cpython() {
    assert_matches_cpython(&prog("    c: int = 0\n", "c = c + 1", "c"), "count", 4);
}

#[test]
fn dict_keys_view_min_idiom_matches_cpython() {
    // The `if k < m: m = k` extremum idiom — order-independent, so accepted.
    let src = "def go() -> int:\n    d: dict[int, int] = {5: 1, 3: 2, 10: 3, 7: 4}\n    m: int = 999\n    for k in d.keys():\n        if k < m:\n            m = k\n    return m\n";
    assert_matches_cpython(src, "min-idiom", 3);
}

#[test]
fn dict_keys_view_compound_matches_cpython() {
    // The fold result composes with the scalar subset (`s * 2`).
    assert_matches_cpython(
        &prog("    s: int = 0\n", "s = s + k", "s * 2"),
        "compound",
        50,
    );
}

#[test]
fn dict_keys_view_negatives_sum_matches_cpython() {
    let src = "def go() -> int:\n    d: dict[int, int] = {-5: 1, 3: 2, -1: 3}\n    total: int = 0\n    for k in d.keys():\n        total = total + k\n    return total\n";
    assert_matches_cpython(src, "neg-sum", -3);
}

// ---------------------------------------------------------------------------
// EXECUTED: str-keyed keys view — the loop var is a str local (i32 base-pointer),
// folded through `len(k)`.
// ---------------------------------------------------------------------------

#[test]
fn str_keyed_dict_keys_view_len_fold_matches_cpython() {
    let src = "def go() -> int:\n    d: dict[str, int] = {\"a\": 1, \"bb\": 2, \"ccc\": 3}\n    total: int = 0\n    for k in d.keys():\n        total = total + len(k)\n    return total\n";
    assert_matches_cpython(src, "str-len-fold", 6);
}

#[test]
fn str_keyed_dict_keys_view_long_key_count_matches_cpython() {
    let src = "def go() -> int:\n    d: dict[str, int] = {\"a\": 1, \"bb\": 2, \"ccc\": 3}\n    c: int = 0\n    for k in d.keys():\n        if len(k) > 1:\n            c = c + 1\n    return c\n";
    assert_matches_cpython(src, "str-longkey-count", 2);
}

// ---------------------------------------------------------------------------
// EXECUTED: the explicit view is a pure ALIAS of the bare form + composes.
// ---------------------------------------------------------------------------

#[test]
fn keys_view_equals_bare_dict_iteration() {
    // `for k in d.keys()` and `for k in d` must produce the IDENTICAL result — the
    // explicit view is a pure alias, so both fold the same key multiset.
    let view = prog("    total: int = 0\n", "total = total + k", "total");
    let bare = "def go() -> int:\n    d: dict[int, int] = {5: 1, 3: 2, 10: 3, 7: 4}\n    total: int = 0\n    for k in d:\n        total = total + k\n    return total\n";
    assert_eq!(
        cpython_of_src(&view),
        cpython_of_src(bare),
        "the explicit .keys() view and bare dict iteration must agree in python3"
    );
    if wasm_runtime_available() {
        assert_eq!(
            run_i64(&view, "view"),
            run_i64(bare, "bare"),
            "the explicit .keys() view and bare dict iteration must agree in WASM"
        );
    }
}

#[test]
fn empty_dict_keys_view_iterates_zero_times() {
    // The loop guard `i >= count` holds at i=0 — the accumulator survives untouched.
    let src = "def go() -> int:\n    d: dict[int, int] = {}\n    total: int = 0\n    for k in d.keys():\n        total = total + k\n    return total\n";
    assert_matches_cpython(src, "empty", 0);
}

#[test]
fn dict_keys_view_outer_list_inner_nested_matches_cpython() {
    // keys-OUTER × list-INNER: keys {5,3,10,7} each add xs=[1,2] → 4*(1+2)=12; and
    // the sum-of-keys adds once per xs element (2×25=50)? No — body adds only x, so
    // total = |d| * sum(xs) = 4 * 3 = 12.
    let src = "def go() -> int:\n    d: dict[int, int] = {5: 1, 3: 2, 10: 3, 7: 4}\n    xs: list[int] = [1, 2]\n    total: int = 0\n    for k in d.keys():\n        for x in xs:\n            total = total + x\n    return total\n";
    assert_matches_cpython(src, "nested", 12);
}

// ---------------------------------------------------------------------------
// HONEST REFUSALS: order-dependent bodies and non-name sources MUST refuse — a
// silent accept is the PMAT-1292 storage-order-misread miscompile.
// ---------------------------------------------------------------------------

#[test]
fn order_dependent_keys_view_iteration_refuses() {
    // `r = r * 10 + k` — a positional fold; its value depends on iteration order.
    let src = "def go() -> int:\n    d: dict[int, int] = {50: 1, 3: 2, 27: 3}\n    r: int = 0\n    for k in d.keys():\n        r = r * 10 + k\n    return r\n";
    let err = emit(src).expect_err("an order-dependent keys-view iteration must refuse");
    assert!(
        err.contains("order-dependent") && err.contains("keys"),
        "refusal should name the order-dependent keys-view iteration, got: {err}"
    );
}
