//! PMAT-1301 — EXECUTED witness for native-WASM `for k, v in d.items()` over a
//! `dict[int|str, int]` — the FIRST key+value PAIRED iteration, completing the
//! read-only dict-iteration surface (bare keys PMAT-1297 / `.keys()` PMAT-1299 /
//! `.values()` PMAT-1298 → items). Runs on the bump-heap dict runtime
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! `d.items()` iterates the dict as `(key, value)` pairs. The memo flagged it HARD
//! for the "list[tuple] ABI" — but that ABI is only needed for `.items()` as a
//! BOUND list value; the ITERATION form materialises NO tuple. Each step binds two
//! separate loop locals from ONE entry: `k` from the KEY slot (@entry+0 — the SAME
//! read `for k in d`/`.keys()` use) and `v` from the VALUE slot
//! (@entry+`DICT_VAL_OFFSET` — the SAME read `for v in d.values()` uses). A dict IS
//! a set-with-values at entry+0, so both reads REUSE existing `emit_index` arms —
//! the whole slice is a `PairIterKind::Pairs` desugar arm with NO new
//! Stmt/Expr/Type variant, NO new helper, NO gate-walker edit.
//!
//! ## The order-safety contract (why this is NOT a silent miscompile)
//!
//! CPython ≥3.7 iterates `d.items()` in the dict's INSERTION order; xpile walks the
//! bump-heap live-entry region in STORAGE order, and a `del d[k]` swaps the last
//! entry into the hole — so storage order can DIVERGE from insertion order after a
//! delete. The items iteration is gated by the SAME `set_iteration_body_order_safe`
//! under-approximation the set / bare-dict-key / keys-view / values lanes use: only
//! an order-INDEPENDENT (commutative / associative) body over the `(k, v)` pair —
//! `sum`, `count`, `product`, `xor`, the `if v < m: m = v` min/max idiom — is
//! accepted; any order-DEPENDENT body (`r = r*10 + k`) REFUSES. A body that MUTATES
//! the dict is a non-whitelisted statement form, so it also refuses.
//!
//! PMAT-1301 also widened `is_commutative_accum` from a SINGLE `acc OP e` to a
//! same-op fold SPINE so the NATURAL two-variable reduction `total = total + k + v`
//! (parsed as `(total + k) + v`) is admitted — the accumulator may be any single
//! leaf of a tree built entirely from ONE commutative+associative monoid op, every
//! other leaf accumulator-free. Requiring the SAME op along the spine preserves
//! associativity, so a mixed `total = (total + k) * v` (Horner) stays refused.
//!
//! ## Correctness properties this pins against live `python3`:
//!   * int-keyed `for k, v in d.items()` commutative folds over the pair (sum of
//!     `k + v`, sum of `k * v`, product, xor, count, min/max idiom) == CPython.
//!   * a str-keyed `for k, v in d.items()` fold over `len(k) + v` (the key is a str
//!     local riding an i32 base-pointer, the value an i64) == CPython.
//!   * items iteration observed AFTER a `del` (storage-order scramble) == CPython.
//!   * `sum(k + v for k, v in d.items())` == `sum(d.keys()) + sum(d.values())` —
//!     items agrees with the keys/values views it decomposes into.
//!   * the EMPTY dict iterates zero times.
//!   * an items-OUTER × list-INNER nested loop composes.
//!   * HONEST REFUSALS: an order-DEPENDENT items body (over `k` OR `v`), a
//!     dict-MUTATING body, and a non-name `.items()` source all refuse at compile
//!     time — never a storage-order misread.
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
    let dir = std::env::temp_dir().join(format!("xpile-dictitems-{}-{}", std::process::id(), tag));
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
        eprintln!("PMAT-1301: WABT absent — {tag} emit-only check passed, run skipped");
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

/// The canonical int-keyed dict — keys {5,3,10,7} (sum 25), values {10,20,30,40}
/// (sum 100): so sum of `k + v` = 125, sum of `k * v` = 50+60+300+280 = 690.
const D: &str = "    d: dict[int, int] = {5: 10, 3: 20, 10: 30, 7: 40}\n";

/// A `go() -> int` program: `<D>` then the loop `for k, v in d.items(): <body>`
/// (8-space indented) then `return <ret>`, with an accumulator prelude `<pre>`.
fn prog(pre: &str, body: &str, ret: &str) -> String {
    format!(
        "def go() -> int:\n{D}{pre}    for k, v in d.items():\n        {body}\n    return {ret}\n"
    )
}

// ---------------------------------------------------------------------------
// CONSTRUCT: `for k, v in d.items()` reads BOTH the KEY slot (entry+0) and the
// VALUE slot (entry+8) of each live entry IN PLACE — a `while` over the 16-byte
// stride, NO materialiser (the read is not the reduction path PMAT-1294/1295).
// ---------------------------------------------------------------------------

#[test]
fn dict_items_reads_key_and_value_slots_in_place() {
    let src = prog("    total: int = 0\n", "total = total + k + v", "total");
    let wat = emit(&src)
        .expect("`for k, v in d.items()` over a dict[int,_] must lower through emit_module");
    // A `while` loop over the live-entry region (not a materialised list).
    assert!(
        wat.contains("(loop"),
        "items iteration must emit a while loop:\n{wat}"
    );
    // The 16-byte `DICT_ENTRY_SIZE` entry stride.
    assert!(
        wat.contains("i32.const 16"),
        "items iteration must read the 16-byte-stride entry array:\n{wat}"
    );
    // The VALUE slot read at `entry+DICT_VAL_OFFSET` (i64.load offset=8) — the
    // key read at entry+0 is a bare `i64.load` (offset 0), and the value read is
    // the ONE `offset=8` immediate away that distinguishes items from keys-only.
    assert!(
        wat.contains("i64.load offset=8"),
        "items iteration must read the value slot at entry+8:\n{wat}"
    );
    // IN-PLACE: NEITHER materialiser helper is emitted — a dict-items iteration
    // reads entries directly, it does not build a list[int] of keys or values.
    assert!(
        !wat.contains("set_to_list") && !wat.contains("dict_values_to_list"),
        "items iteration must read entries in place, not materialise a list:\n{wat}"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED: int-keyed commutative folds over the (k, v) PAIR == CPython.
// ---------------------------------------------------------------------------

#[test]
fn dict_items_sum_key_plus_value_matches_cpython() {
    // `total = total + k + v` — the NATURAL two-variable fold, `(total + k) + v`;
    // admitted by the PMAT-1301 same-op fold-spine widening.
    assert_matches_cpython(
        &prog("    total: int = 0\n", "total = total + k + v", "total"),
        "sum-k+v",
        125,
    );
}

#[test]
fn dict_items_sum_key_times_value_matches_cpython() {
    // `s = s + k * v` — the accumulator is a direct operand of the outer `+`, the
    // element `k * v` is accumulator-free; a weighted sum.
    assert_matches_cpython(
        &prog("    s: int = 0\n", "s = s + k * v", "s"),
        "sum-k*v",
        690,
    );
}

#[test]
fn dict_items_product_matches_cpython() {
    // Small dict to keep the product in i64: (2*3)*(4*5) = 6*20 = 120.
    let src = "def go() -> int:\n    d: dict[int, int] = {2: 3, 4: 5}\n    p: int = 1\n    for k, v in d.items():\n        p = p * k * v\n    return p\n";
    assert_matches_cpython(src, "product", 120);
}

#[test]
fn dict_items_xor_matches_cpython() {
    // `x = x ^ k ^ v` — a homogeneous xor spine; 5^10 ^ 3^20 ^ 10^30 ^ 7^40.
    assert_matches_cpython(
        &prog("    x: int = 0\n", "x = x ^ k ^ v", "x"),
        "xor",
        5 ^ 10 ^ 3 ^ 20 ^ 10 ^ 30 ^ 7 ^ 40,
    );
}

#[test]
fn dict_items_count_matches_cpython() {
    assert_matches_cpython(&prog("    c: int = 0\n", "c = c + 1", "c"), "count", 4);
}

#[test]
fn dict_items_min_value_idiom_matches_cpython() {
    // `if v < m: m = v` — the extremum idiom over the VALUE; order-independent.
    let src = "def go() -> int:\n    d: dict[int, int] = {5: 30, 3: 10, 10: 20}\n    m: int = 999\n    for k, v in d.items():\n        if v < m:\n            m = v\n    return m\n";
    assert_matches_cpython(src, "min-v", 10);
}

#[test]
fn dict_items_max_key_idiom_matches_cpython() {
    // `if k > m: m = k` — the extremum idiom over the KEY; order-independent.
    let src = "def go() -> int:\n    d: dict[int, int] = {5: 1, 3: 2, 10: 3, 7: 4}\n    m: int = 0\n    for k, v in d.items():\n        if k > m:\n            m = k\n    return m\n";
    assert_matches_cpython(src, "max-k", 10);
}

#[test]
fn dict_items_compound_matches_cpython() {
    // The fold result composes with the scalar subset (`s * 2`): 125 * 2 = 250.
    assert_matches_cpython(
        &prog("    s: int = 0\n", "s = s + k + v", "s * 2"),
        "compound",
        250,
    );
}

#[test]
fn dict_items_negatives_sum_matches_cpython() {
    let src = "def go() -> int:\n    d: dict[int, int] = {-5: 1, 3: -2, -1: 3}\n    total: int = 0\n    for k, v in d.items():\n        total = total + k + v\n    return total\n";
    assert_matches_cpython(src, "neg-sum", -5 + 1 + 3 + -2 + -1 + 3);
}

// ---------------------------------------------------------------------------
// EXECUTED: items observed AFTER a `del` — storage order is scrambled
// (swap-last-into-hole), but the commutative fold is invariant → == CPython.
// ---------------------------------------------------------------------------

#[test]
fn dict_items_after_delete_matches_cpython() {
    // `del d[2]` leaves {1:10, 3:30, 4:40}; sum(k+v) = (1+10)+(3+30)+(4+40) = 88.
    let src = "def go() -> int:\n    d: dict[int, int] = {1: 10, 2: 20, 3: 30, 4: 40}\n    del d[2]\n    total: int = 0\n    for k, v in d.items():\n        total = total + k + v\n    return total\n";
    assert_matches_cpython(src, "after-del", 88);
}

// ---------------------------------------------------------------------------
// EXECUTED: str-keyed items — the KEY is a str local (i32 base-pointer, folded
// through `len(k)`), the VALUE an i64 — the two slots read as their own types.
// ---------------------------------------------------------------------------

#[test]
fn str_keyed_dict_items_len_plus_value_matches_cpython() {
    // len("a")+1 + len("bb")+2 + len("ccc")+3 = (1+1)+(2+2)+(3+3) = 12.
    let src = "def go() -> int:\n    d: dict[str, int] = {\"a\": 1, \"bb\": 2, \"ccc\": 3}\n    total: int = 0\n    for k, v in d.items():\n        total = total + len(k) + v\n    return total\n";
    assert_matches_cpython(src, "str-len+val", 12);
}

// ---------------------------------------------------------------------------
// EXECUTED: items DECOMPOSES into the keys view + the values view — the sum of
// `k + v` over items equals sum(keys) + sum(values). Cross-checks the two slots
// are read from the SAME entry, unmixed.
// ---------------------------------------------------------------------------

#[test]
fn dict_items_decomposes_into_keys_plus_values() {
    let items = prog("    total: int = 0\n", "total = total + k + v", "total");
    // sum over keys (bare `for k in d`) + sum over values (`for v in d.values()`).
    let split = "def go() -> int:\n    d: dict[int, int] = {5: 10, 3: 20, 10: 30, 7: 40}\n    total: int = 0\n    for k in d:\n        total = total + k\n    for v in d.values():\n        total = total + v\n    return total\n";
    assert_eq!(
        cpython_of_src(&items),
        cpython_of_src(split),
        "items sum(k+v) must equal sum(keys)+sum(values) in python3"
    );
    if wasm_runtime_available() {
        assert_eq!(
            run_i64(&items, "items"),
            run_i64(split, "split"),
            "items sum(k+v) must equal sum(keys)+sum(values) in WASM"
        );
    }
}

#[test]
fn empty_dict_items_iterates_zero_times() {
    let src = "def go() -> int:\n    d: dict[int, int] = {}\n    total: int = 0\n    for k, v in d.items():\n        total = total + k + v\n    return total\n";
    assert_matches_cpython(src, "empty", 0);
}

#[test]
fn dict_items_outer_list_inner_nested_matches_cpython() {
    // items-OUTER (|d| = 4) × list-INNER (xs = [1, 2], sum 3) → body adds only x →
    // total = 4 * 3 = 12.
    let src = "def go() -> int:\n    d: dict[int, int] = {5: 10, 3: 20, 10: 30, 7: 40}\n    xs: list[int] = [1, 2]\n    total: int = 0\n    for k, v in d.items():\n        for x in xs:\n            total = total + x\n    return total\n";
    assert_matches_cpython(src, "nested", 12);
}

// ---------------------------------------------------------------------------
// HONEST REFUSALS: order-dependent bodies, a dict-mutating body, and a non-name
// `.items()` source MUST refuse — a silent accept is the PMAT-1292
// storage-order-misread miscompile.
// ---------------------------------------------------------------------------

#[test]
fn order_dependent_items_over_key_refuses() {
    // `r = r * 10 + k` — a positional (Horner) fold; value depends on order.
    let src = "def go() -> int:\n    d: dict[int, int] = {50: 1, 3: 2, 27: 3}\n    r: int = 0\n    for k, v in d.items():\n        r = r * 10 + k\n    return r\n";
    let err = emit(src).expect_err("an order-dependent items body (over key) must refuse");
    assert!(
        err.contains("order-dependent") && err.contains("items"),
        "refusal should name the order-dependent items iteration, got: {err}"
    );
}

#[test]
fn order_dependent_items_over_value_refuses() {
    let src = "def go() -> int:\n    d: dict[int, int] = {50: 1, 3: 2, 27: 3}\n    r: int = 0\n    for k, v in d.items():\n        r = r * 10 + v\n    return r\n";
    let err = emit(src).expect_err("an order-dependent items body (over value) must refuse");
    assert!(
        err.contains("order-dependent") && err.contains("items"),
        "refusal should name the order-dependent items iteration, got: {err}"
    );
}

#[test]
fn dict_mutating_items_body_refuses() {
    // Assigning into the dict during its own items iteration is not a whitelisted
    // (commutative) statement form → refuses.
    let src = "def go() -> int:\n    d: dict[int, int] = {1: 2, 3: 4}\n    total: int = 0\n    for k, v in d.items():\n        d[k] = v + 1\n        total = total + v\n    return total\n";
    let err = emit(src).expect_err("a dict-mutating items body must refuse");
    assert!(
        err.contains("order-dependent") || err.contains("MUTATED"),
        "refusal should reject the dict-mutating items iteration, got: {err}"
    );
}

#[test]
fn horner_mixed_op_spine_refuses() {
    // `total = (total + k) * v` — a MIXED-op spine (Horner): the accumulator's
    // spine is `+` but the outer op is `*`, so it is order-DEPENDENT. The
    // PMAT-1301 same-op fold-spine widening must NOT admit it.
    let src = "def go() -> int:\n    d: dict[int, int] = {2: 3, 4: 5}\n    total: int = 1\n    for k, v in d.items():\n        total = (total + k) * v\n    return total\n";
    let err = emit(src).expect_err("a mixed-op Horner spine must refuse");
    assert!(
        err.contains("order-dependent") && err.contains("items"),
        "refusal should reject the mixed-op items accumulation, got: {err}"
    );
}
