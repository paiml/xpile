//! PMAT-1295 — EXECUTED witness for native-WASM `sum(d.values())` /
//! `min(d.values())` / `max(d.values())` / `sorted(d.values())` over a
//! `dict[_, int]` — the dict-VALUE reduction/sort, the symmetric sibling of the
//! dict-KEY reduction PMAT-1294 shipped. Runs on the bump-heap dict + list
//! runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! `d.values()` types as `List(V)`, so the frontend lowers `sum/min/max/sorted`
//! over it to `Sum { list: DictView{Values} }`, `ListMinMax { list:
//! DictView{Values} }`, and `Sorted { list: DictView{Values} }` — exactly the
//! `DictView{Keys}` shape PMAT-1294 handles, but with a `Values` kind.
//!
//! PMAT-1294 reused the SET/KEY materialiser `$__wasm_set_to_list_i64` (reads the
//! key at `entry+0`). A value reduction needs a DISTINCT materialiser: the WASM
//! dict stores a 16-byte entry with the KEY at `entry+0` and the VALUE at
//! `entry+8` (`DICT_VAL_OFFSET`). PMAT-1295 adds `$__wasm_dict_values_to_list_i64`
//! — `$__wasm_set_to_list_i64` with a single change (`i64.load offset=8` instead
//! of `entry+0`) — and routes `DictView{Values}` through it. Dicts store i64
//! values only (`dict_value_is_supported`), so the values pack into a `list[int]`.
//!
//! ## Correctness properties this pins against live `python3`:
//!   * `sum(d.values())` / `min(d.values())` / `max(d.values())` over an
//!     int-valued dict == CPython (the reductions over the VALUES).
//!   * KEY-TYPE-AGNOSTIC: the SAME value reduction works over a str-KEYED dict
//!     (`dict[str, int]`) — only the value slot is read, never the key.
//!   * DUPLICATE VALUES are KEPT (values need not be distinct, unlike keys):
//!     `sum` / `sorted` over `{1:7, 2:7}` sees two 7s.
//!   * ORDER-INDEPENDENCE after a swap-into-hole `del d[k]` (sum/min/max are blind
//!     to the scrambled storage order; `sorted` re-sorts, so also order-blind).
//!   * NEGATIVE values fold correctly (signed `i64.add` / `i64.lt_s` / `i64.gt_s`).
//!   * `sum({}.values()) == 0`; `min`/`max` of an EMPTY dict's values TRAP
//!     (`unreachable`), matching Python `ValueError`.
//!   * a compound `sum(d.values()) * 2 + max(d.values())` composes with the
//!     scalar subset.
//!   * `sorted(d.values())` and `sorted(d.values(), reverse=True)` produce a
//!     CPython-exact `list[int]` (verified by indexing the result).
//!   * The value path routes through its OWN helper (`$__wasm_dict_values_to_list_i64`),
//!     NEVER the key materialiser — a values-only reduction carries no key helper.
//!   * HONEST REFUSAL: a non-name dict (`sum({...}.values())`) refuses at compile
//!     time — bind the dict to a name first, never a base-pointer misread.
//!
//! This lowers REAL Python through the frontend the CLI uses for `--target wasm`
//! (avoiding the PMAT-1244/1245 reachability trap), then assembles + runs the
//! emitted WAT in WABT. Gated on `wasm_runtime_available()` — a clean skip (still
//! asserting the EMIT path lowers) without WABT.

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
    let dir = std::env::temp_dir().join(format!("xpile-dictval-{}-{}", std::process::id(), tag));
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

/// The differential value CPython computes for the same reduction body.
fn cpython_i64(body: &str) -> i64 {
    let out = Command::new("python3")
        .arg("-c")
        .arg(format!("def go():\n{body}\nprint(go())"))
        .output()
        .expect("spawn python3");
    assert!(
        out.status.success(),
        "python3 failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<i64>()
        .expect("cpython i64")
}

/// A `go() -> int` program whose body is `build` then `return <expr>`.
fn prog(build: &str, ret: &str) -> String {
    format!("def go() -> int:\n{build}    return {ret}\n")
}

/// The canonical int-keyed dict whose VALUES are {5,7,3,4}: sum=19, min=3, max=7.
const D: &str = "    d: dict[int, int] = {10: 5, 20: 7, 30: 3, 40: 4}\n";

// ---------------------------------------------------------------------------
// CONSTRUCT: the emitted WAT routes each dict-VALUE reduction through the VALUE
// materialiser (`$__wasm_dict_values_to_list_i64`, reads entry+8) and the
// matching pre-existing fold/sort helper — NEVER the key materialiser.
// ---------------------------------------------------------------------------

#[test]
fn value_sum_lowers_through_value_materialiser_and_sum_helper() {
    let src = prog(D, "sum(d.values())");
    let wat = emit(&src).expect("`sum(d.values())` must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_dict_values_to_list_i64")
            && wat.contains("call $__wasm_dict_values_to_list_i64"),
        "sum(d.values()) must declare AND call the VALUE materialiser (entry+8):\n{wat}"
    );
    assert!(
        wat.contains("(func $__wasm_list_sum_i64") && wat.contains("call $__wasm_list_sum_i64"),
        "sum(d.values()) must fold the materialised list via the int-sum helper:\n{wat}"
    );
    // Values-only: the KEY materialiser must NOT be emitted/called.
    assert!(
        !wat.contains("$__wasm_set_to_list_i64"),
        "a values-only reduction must carry NO key materialiser (entry+0):\n{wat}"
    );
}

#[test]
fn value_minmax_lowers_through_value_materialiser_and_minmax_helper() {
    let src = prog(D, "max(d.values())");
    let wat = emit(&src).expect("`max(d.values())` must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_dict_values_to_list_i64")
            && wat.contains("call $__wasm_dict_values_to_list_i64"),
        "max(d.values()) must declare AND call the VALUE materialiser:\n{wat}"
    );
    assert!(
        wat.contains("(func $__wasm_list_minmax_i64")
            && wat.contains("call $__wasm_list_minmax_i64"),
        "max(d.values()) must fold the materialised list via the min/max helper:\n{wat}"
    );
}

#[test]
fn value_sorted_lowers_through_value_materialiser_and_sort_helper() {
    let src = "def go() -> int:\n    d: dict[int, int] = {10: 5, 20: 7, 30: 3}\n    xs: list[int] = sorted(d.values())\n    return xs[0]\n";
    let wat = emit(src).expect("`sorted(d.values())` must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_dict_values_to_list_i64")
            && wat.contains("call $__wasm_dict_values_to_list_i64"),
        "sorted(d.values()) must declare AND call the VALUE materialiser:\n{wat}"
    );
    assert!(
        wat.contains("(func $__wasm_list_sorted_i64")
            && wat.contains("call $__wasm_list_sorted_i64"),
        "sorted(d.values()) must sort the materialised list via the list-sort helper:\n{wat}"
    );
}

// The VALUE materialiser reads entry+8; the KEY materialiser reads entry+0. A
// witness that the value helper genuinely carries the `offset=8` load (so a
// future collapse into the key helper — which would sum KEYS not VALUES —
// is caught structurally, not only by the differential run below).
#[test]
fn value_materialiser_reads_the_value_slot_at_offset_8() {
    let wat = emit(&prog(D, "sum(d.values())")).expect("lower");
    let helper = wat
        .split("(func $__wasm_dict_values_to_list_i64")
        .nth(1)
        .expect("value materialiser present");
    let body = helper.split("(func ").next().unwrap();
    assert!(
        body.contains("i64.load offset=8"),
        "the value materialiser must read the VALUE slot (i64.load offset=8):\n{body}"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED: sum / min / max of an int dict's VALUES == CPython.
// ---------------------------------------------------------------------------

#[test]
fn value_sum_matches_cpython() {
    let src = prog(D, "sum(d.values())");
    assert!(emit(&src).is_ok(), "sum(d.values()) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1295: WABT absent — emit-only sum check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "sum"), 19);
    assert_eq!(
        run_i64(&src, "sum"),
        cpython_i64("    d={10:5,20:7,30:3,40:4}\n    return sum(d.values())")
    );
}

#[test]
fn value_min_matches_cpython() {
    let src = prog(D, "min(d.values())");
    assert!(emit(&src).is_ok(), "min(d.values()) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1295: WABT absent — emit-only min check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "min"), 3);
    assert_eq!(
        run_i64(&src, "min"),
        cpython_i64("    d={10:5,20:7,30:3,40:4}\n    return min(d.values())")
    );
}

#[test]
fn value_max_matches_cpython() {
    let src = prog(D, "max(d.values())");
    assert!(emit(&src).is_ok(), "max(d.values()) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1295: WABT absent — emit-only max check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "max"), 7);
    assert_eq!(
        run_i64(&src, "max"),
        cpython_i64("    d={10:5,20:7,30:3,40:4}\n    return max(d.values())")
    );
}

// ---------------------------------------------------------------------------
// EXECUTED: KEY-TYPE-AGNOSTIC — the SAME value reduction over a str-KEYED dict
// (`dict[str, int]`). Only the i64 value slot is read, never the key.
// ---------------------------------------------------------------------------

#[test]
fn str_keyed_dict_value_sum_matches_cpython() {
    let src = "def go() -> int:\n    d: dict[str, int] = {\"a\": 10, \"b\": 20, \"c\": 5}\n    return sum(d.values())\n";
    assert!(
        emit(src).is_ok(),
        "sum over a str-keyed dict's values must lower"
    );
    if !wasm_runtime_available() {
        eprintln!("PMAT-1295: WABT absent — str-keyed value-sum run skipped");
        return;
    }
    assert_eq!(run_i64(src, "strsum"), 35);
    assert_eq!(
        run_i64(src, "strsum"),
        cpython_i64("    d={'a':10,'b':20,'c':5}\n    return sum(d.values())")
    );
}

// ---------------------------------------------------------------------------
// EXECUTED: DUPLICATE VALUES are kept — values need NOT be distinct (unlike
// keys). `sum`/`max` over `{1:7, 2:7, 3:1}` sees two 7s.
// ---------------------------------------------------------------------------

#[test]
fn duplicate_values_are_kept() {
    // values {7,7,1}: sum == 15, max == 7.
    let build = "    d: dict[int, int] = {1: 7, 2: 7, 3: 1}\n";
    assert!(
        emit(&prog(build, "sum(d.values())")).is_ok(),
        "dup-value sum must lower"
    );
    if !wasm_runtime_available() {
        eprintln!("PMAT-1295: WABT absent — dup-value run skipped");
        return;
    }
    assert_eq!(run_i64(&prog(build, "sum(d.values())"), "dupsum"), 15);
    assert_eq!(run_i64(&prog(build, "max(d.values())"), "dupmax"), 7);
}

// ---------------------------------------------------------------------------
// EXECUTED: ORDER-INDEPENDENCE — a `del d[k]` scrambles the dict's storage order
// (swap-last-into-hole), but sum/min are blind to it.
// ---------------------------------------------------------------------------

#[test]
fn value_sum_order_independent_after_del() {
    // values start {9,2,4,5}; del d[20] drops value 2 → {9,4,5}; sum == 18.
    let src = prog(
        "    d: dict[int, int] = {10: 9, 20: 2, 30: 4, 40: 5}\n    del d[20]\n",
        "sum(d.values())",
    );
    assert!(emit(&src).is_ok(), "sum(d.values()) after del must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1295: WABT absent — del run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "del"), 18);
}

// ---------------------------------------------------------------------------
// EXECUTED: NEGATIVES — signed fold + signed compare over the values.
// ---------------------------------------------------------------------------

#[test]
fn value_reductions_negatives_match_cpython() {
    let build = "    d: dict[int, int] = {1: -5, 2: -1, 3: 0, 4: 3}\n";
    if !wasm_runtime_available() {
        assert!(emit(&prog(build, "min(d.values())")).is_ok());
        eprintln!("PMAT-1295: WABT absent — negatives run skipped");
        return;
    }
    assert_eq!(run_i64(&prog(build, "min(d.values())"), "negmin"), -5);
    assert_eq!(run_i64(&prog(build, "max(d.values())"), "negmax"), 3);
    assert_eq!(run_i64(&prog(build, "sum(d.values())"), "negsum"), -3);
}

// ---------------------------------------------------------------------------
// EXECUTED: EMPTY dict — `sum` → 0 (no trap), `min` → TRAP (Python ValueError).
// ---------------------------------------------------------------------------

#[test]
fn value_sum_empty_is_zero() {
    // {5:9}; del d[5] → {}; sum(d.values()) == 0 (the empty-list guard).
    let src = prog(
        "    d: dict[int, int] = {5: 9}\n    del d[5]\n",
        "sum(d.values())",
    );
    assert!(emit(&src).is_ok(), "sum(empty dict values) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1295: WABT absent — empty-sum run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "emptysum"), 0);
}

#[test]
fn value_min_empty_traps_like_cpython_valueerror() {
    // {5:9}; del d[5] → {}; min(empty values) → ValueError → WASM `unreachable`.
    let src = prog(
        "    d: dict[int, int] = {5: 9}\n    del d[5]\n",
        "min(d.values())",
    );
    let wat = emit(&src).expect("min(empty dict values) must lower (trap is at runtime)");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1295: WABT absent — empty-min trap run skipped");
        return;
    }
    let line = go_line(&wat, "emptymin");
    assert!(
        line.contains("error") || line.contains("unreachable"),
        "min of an empty dict's values must TRAP (Python ValueError), got: {line}"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED: composition — a dict-value reduction feeds the scalar subset.
// ---------------------------------------------------------------------------

#[test]
fn value_reduction_composes_in_arithmetic() {
    // sum(values{5,7,3})*2 + max(values) = 30 + 7 = 37.
    let src = prog(
        "    d: dict[int, int] = {10: 5, 20: 7, 30: 3}\n",
        "sum(d.values()) * 2 + max(d.values())",
    );
    assert!(
        emit(&src).is_ok(),
        "compound dict-value reduction must lower"
    );
    if !wasm_runtime_available() {
        eprintln!("PMAT-1295: WABT absent — compound run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "compound"), 37);
}

// ---------------------------------------------------------------------------
// EXECUTED: sorted(d.values()) / sorted(d.values(), reverse=True) produce a
// CPython-exact list[int] of the VALUES (verified by indexing).
// ---------------------------------------------------------------------------

#[test]
fn value_sorted_ascending_matches_cpython() {
    // values {5,7,3,4} → sorted [3,4,5,7]; index 0 == 3, index 3 == 7.
    let lo = "def go() -> int:\n    d: dict[int, int] = {10: 5, 20: 7, 30: 3, 40: 4}\n    xs: list[int] = sorted(d.values())\n    return xs[0]\n";
    let hi = "def go() -> int:\n    d: dict[int, int] = {10: 5, 20: 7, 30: 3, 40: 4}\n    xs: list[int] = sorted(d.values())\n    return xs[3]\n";
    assert!(
        emit(lo).is_ok() && emit(hi).is_ok(),
        "sorted(d.values()) must lower"
    );
    if !wasm_runtime_available() {
        eprintln!("PMAT-1295: WABT absent — sorted run skipped");
        return;
    }
    assert_eq!(run_i64(lo, "sortlo"), 3);
    assert_eq!(run_i64(hi, "sorthi"), 7);
}

#[test]
fn value_sorted_reverse_matches_cpython() {
    // values {5,7,3,4} → sorted(reverse) [7,5,4,3]; index 0 == 7.
    let src = "def go() -> int:\n    d: dict[int, int] = {10: 5, 20: 7, 30: 3, 40: 4}\n    xs: list[int] = sorted(d.values(), reverse=True)\n    return xs[0]\n";
    assert!(
        emit(src).is_ok(),
        "sorted(d.values(), reverse=True) must lower"
    );
    if !wasm_runtime_available() {
        eprintln!("PMAT-1295: WABT absent — sorted-reverse run skipped");
        return;
    }
    assert_eq!(run_i64(src, "sortrev"), 7);
}

// ---------------------------------------------------------------------------
// EXECUTED: sorted keeps DUPLICATE values (a list result, not a set).
// ---------------------------------------------------------------------------

#[test]
fn value_sorted_keeps_duplicates() {
    // values {7,3,7,3} → sorted [3,3,7,7]; index 1 == 3, index 2 == 7 (dups kept).
    let a = "def go() -> int:\n    d: dict[int, int] = {1: 7, 2: 3, 3: 7, 4: 3}\n    xs: list[int] = sorted(d.values())\n    return xs[1]\n";
    let b = "def go() -> int:\n    d: dict[int, int] = {1: 7, 2: 3, 3: 7, 4: 3}\n    xs: list[int] = sorted(d.values())\n    return xs[2]\n";
    assert!(
        emit(a).is_ok() && emit(b).is_ok(),
        "sorted dup-values must lower"
    );
    if !wasm_runtime_available() {
        eprintln!("PMAT-1295: WABT absent — sorted-dup run skipped");
        return;
    }
    assert_eq!(run_i64(a, "sdupa"), 3);
    assert_eq!(run_i64(b, "sdupb"), 7);
}

// ---------------------------------------------------------------------------
// HONEST REFUSAL: a non-name dict (`sum({...}.values())`) — the materialiser
// needs a NAMED dict local, never a temporary; refuse, never a misread.
// ---------------------------------------------------------------------------

#[test]
fn value_reduction_over_non_name_dict_refuses() {
    let src = "def go() -> int:\n    return sum({1: 10, 2: 20}.values())\n";
    let err = emit(src).expect_err("sum({...}.values()) over a non-name dict must refuse");
    assert!(
        err.contains("non-name") || err.contains("NAMED") || err.contains("bind"),
        "refusal should name the non-name dict operand, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// REGRESSION: the dict-KEY reduction (PMAT-1294) and bare set/list reductions
// are unaffected by the value routing.
// ---------------------------------------------------------------------------

#[test]
fn key_and_set_reductions_still_work() {
    // KEY reduction: keys {10,20,30}; sum == 60.
    let key_src = prog("    d: dict[int, int] = {10: 5, 20: 7, 30: 3}\n", "sum(d)");
    // set reduction unaffected: {5,3,10,7} sum+min+max = 38.
    let set_src = prog(
        "    s: set[int] = {5, 3, 10, 7}\n",
        "sum(s) + min(s) + max(s)",
    );
    assert!(
        emit(&key_src).is_ok() && emit(&set_src).is_ok(),
        "key + set reductions must still lower"
    );
    if !wasm_runtime_available() {
        eprintln!("PMAT-1295: WABT absent — key/set-regression run skipped");
        return;
    }
    assert_eq!(run_i64(&key_src, "keyreg"), 60);
    assert_eq!(run_i64(&set_src, "setreg"), 38);
}

// A program that reduces BOTH keys AND values carries BOTH materialisers (they
// are distinct helpers — entry+0 vs entry+8 — and must coexist cleanly).
#[test]
fn keys_and_values_reduction_coexist() {
    let src = prog(
        "    d: dict[int, int] = {10: 5, 20: 7, 30: 3}\n",
        "sum(d) + sum(d.values())",
    );
    let wat = emit(&src).expect("sum(d) + sum(d.values()) must lower");
    assert!(
        wat.contains("(func $__wasm_set_to_list_i64")
            && wat.contains("(func $__wasm_dict_values_to_list_i64"),
        "a keys+values program must declare BOTH materialisers:\n{wat}"
    );
    if !wasm_runtime_available() {
        eprintln!("PMAT-1295: WABT absent — coexist run skipped");
        return;
    }
    // keys {10,20,30}=60 + values {5,7,3}=15 → 75.
    assert_eq!(run_i64(&src, "coexist"), 75);
}
