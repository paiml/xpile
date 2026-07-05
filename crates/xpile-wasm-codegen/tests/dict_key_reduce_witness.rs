//! PMAT-1294 — EXECUTED witness for native-WASM `sum(d)` / `min(d)` / `max(d)` /
//! `sorted(d)` over a `dict[int, _]` — the FIRST dict→list MATERIALIZATION in the
//! WASM subset, reducing/sorting the dict's KEYS. Runs on the bump-heap dict +
//! list runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! Python iterates a dict as its KEYS, so `sum(d)` / `min(d)` / `max(d)` /
//! `sorted(d)` reduce/sort the keys. The frontend materialises a reduction's
//! iterable arg (PMAT-521), lowering these to `Sum { list: DictView{Keys} }`,
//! `ListMinMax { list: DictView{Keys} }`, and `Sorted { list: DictView{Keys} }`
//! — exactly the `SetToList` shape the set lane (PMAT-1290..1293) produces, but
//! with a `DictView{Keys}` operand.
//!
//! PMAT-1294 teaches the WASM lane to route those `DictView{Keys}` operands
//! through the SAME `$__wasm_set_to_list_i64` materialiser the set lane uses. This
//! is sound because a dict and a set share the IDENTICAL open-assoc region: an
//! i32 live-count @ base+0, 16-byte entries @ base+8, the KEY @ entry+0 (a dict
//! additionally stores the value at entry+8, which the key-only materialiser never
//! reads). NO new helper and NO new gate are minted — a `DictView{Keys}` arms the
//! `needs_set_to_list` gate and the `Sum`/`ListMinMax`/`Sorted` nodes already arm
//! their fold/sort gates.
//!
//! ## Correctness properties this pins against live `python3`:
//!   * `sum(d)` / `min(d)` / `max(d)` / `sorted(d)` over an int-keyed dict ==
//!     CPython (the reductions over the KEYS).
//!   * ORDER-INDEPENDENCE after a swap-into-hole `del d[k]` (sum/min/max are blind
//!     to the scrambled storage order; `sorted` re-sorts, so also order-blind).
//!   * DEDUP: a dict literal with a repeated key keeps the last value but ONE key,
//!     so the key reduction runs over the unique keys.
//!   * NEGATIVES fold correctly (signed `i64.add` / `i64.lt_s` / `i64.gt_s`).
//!   * `sum({}) == 0`; `min`/`max` of an EMPTY dict TRAP (`unreachable`), matching
//!     Python `ValueError` — the same posture as `min(set())` / `min([])`.
//!   * a compound `sum(d) * 2 + max(d)` composes with the scalar subset.
//!   * `sorted(d)` and `sorted(d, reverse=True)` produce a CPython-exact
//!     `list[int]` (verified by indexing the result).
//!   * HONEST REFUSALS: a str-keyed dict key reduction (→ `list[str]` ABI) and a
//!     `sum(d.values())` / value reduction (a distinct materialiser, not yet
//!     wired) both refuse at compile time, never a base-pointer misread.
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
    let dir = std::env::temp_dir().join(format!("xpile-dictkey-{}-{}", std::process::id(), tag));
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

/// The canonical int-keyed dict local used across the arithmetic probes — keys
/// {5,3,10,7} (deliberately the SAME key set as the set-reduce witness so the
/// expected sum/min/max match): sum=25, min=3, max=10.
const D: &str = "    d: dict[int, int] = {5: 1, 3: 2, 10: 3, 7: 4}\n";

// ---------------------------------------------------------------------------
// CONSTRUCT: the emitted WAT routes each dict-KEY reduction through the SAME
// set→list materialiser and the matching pre-existing fold/sort helper.
// ---------------------------------------------------------------------------

#[test]
fn dict_sum_lowers_through_set_materialiser_and_sum_helper() {
    let src = prog(D, "sum(d)");
    let wat = emit(&src).expect("`sum(d)` over a dict[int,_] must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_set_to_list_i64")
            && wat.contains("call $__wasm_set_to_list_i64"),
        "sum(dict) must declare AND call the (reused) set→list materialiser:\n{wat}"
    );
    assert!(
        wat.contains("(func $__wasm_list_sum_i64") && wat.contains("call $__wasm_list_sum_i64"),
        "sum(dict) must fold the materialised list via the int-sum helper:\n{wat}"
    );
}

#[test]
fn dict_minmax_lowers_through_set_materialiser_and_minmax_helper() {
    let src = prog(D, "max(d)");
    let wat = emit(&src).expect("`max(d)` over a dict[int,_] must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_set_to_list_i64")
            && wat.contains("call $__wasm_set_to_list_i64"),
        "max(dict) must declare AND call the (reused) set→list materialiser:\n{wat}"
    );
    assert!(
        wat.contains("(func $__wasm_list_minmax_i64")
            && wat.contains("call $__wasm_list_minmax_i64"),
        "max(dict) must fold the materialised list via the min/max helper:\n{wat}"
    );
}

#[test]
fn dict_sorted_lowers_through_set_materialiser_and_sort_helper() {
    let src = "def go() -> int:\n    d: dict[int, int] = {5: 1, 3: 2, 10: 3, 7: 4}\n    xs: list[int] = sorted(d)\n    return xs[0]\n";
    let wat = emit(src).expect("`sorted(d)` over a dict[int,_] must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_set_to_list_i64")
            && wat.contains("call $__wasm_set_to_list_i64"),
        "sorted(dict) must declare AND call the (reused) set→list materialiser:\n{wat}"
    );
    assert!(
        wat.contains("(func $__wasm_list_sorted_i64")
            && wat.contains("call $__wasm_list_sorted_i64"),
        "sorted(dict) must sort the materialised list via the list-sort helper:\n{wat}"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED: sum / min / max of an int dict's keys == CPython.
// ---------------------------------------------------------------------------

#[test]
fn dict_sum_matches_cpython() {
    let src = prog(D, "sum(d)");
    assert!(emit(&src).is_ok(), "sum(dict) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1294: WABT absent — emit-only sum check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "sum"), 25);
    assert_eq!(
        run_i64(&src, "sum"),
        cpython_i64("    d={5:1,3:2,10:3,7:4}\n    return sum(d)")
    );
}

#[test]
fn dict_min_matches_cpython() {
    let src = prog(D, "min(d)");
    assert!(emit(&src).is_ok(), "min(dict) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1294: WABT absent — emit-only min check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "min"), 3);
    assert_eq!(
        run_i64(&src, "min"),
        cpython_i64("    d={5:1,3:2,10:3,7:4}\n    return min(d)")
    );
}

#[test]
fn dict_max_matches_cpython() {
    let src = prog(D, "max(d)");
    assert!(emit(&src).is_ok(), "max(dict) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1294: WABT absent — emit-only max check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "max"), 10);
    assert_eq!(
        run_i64(&src, "max"),
        cpython_i64("    d={5:1,3:2,10:3,7:4}\n    return max(d)")
    );
}

// ---------------------------------------------------------------------------
// EXECUTED: DEDUP — a repeated key keeps ONE key; the key reduction runs over
// the unique keys.
// ---------------------------------------------------------------------------

#[test]
fn dict_sum_dedup_matches_cpython() {
    // {7:_,7:_,3:_,5:_} stores keys {7,3,5}; sum == 15.
    let src = prog(
        "    d: dict[int, int] = {7: 1, 7: 2, 3: 3, 5: 4}\n",
        "sum(d)",
    );
    assert!(emit(&src).is_ok(), "sum(dedup dict) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1294: WABT absent — emit-only dedup check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "dedup"), 15);
}

// ---------------------------------------------------------------------------
// EXECUTED: ORDER-INDEPENDENCE — a `del d[k]` scrambles the dict's storage order
// (swap-last-into-hole), but sum/min are blind to it. This is the load-bearing
// property that makes dict-key reductions CPython-exact.
// ---------------------------------------------------------------------------

#[test]
fn dict_sum_order_independent_after_del() {
    // {1,2,4,5}; del d[2] → keys {1,4,5}; sum == 10.
    let src = prog(
        "    d: dict[int, int] = {1: 9, 2: 9, 4: 9, 5: 9}\n    del d[2]\n",
        "sum(d)",
    );
    assert!(emit(&src).is_ok(), "sum(dict) after del must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1294: WABT absent — emit-only del check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "del"), 10);
}

// ---------------------------------------------------------------------------
// EXECUTED: NEGATIVES — signed fold + signed compare over the keys.
// ---------------------------------------------------------------------------

#[test]
fn dict_reductions_negatives_match_cpython() {
    let build = "    d: dict[int, int] = {-5: 1, -1: 2, 0: 3, 3: 4}\n";
    if !wasm_runtime_available() {
        assert!(emit(&prog(build, "min(d)")).is_ok());
        eprintln!("PMAT-1294: WABT absent — negatives run skipped");
        return;
    }
    assert_eq!(run_i64(&prog(build, "min(d)"), "negmin"), -5);
    assert_eq!(run_i64(&prog(build, "max(d)"), "negmax"), 3);
    assert_eq!(run_i64(&prog(build, "sum(d)"), "negsum"), -3);
}

// ---------------------------------------------------------------------------
// EXECUTED: EMPTY dict — `sum` → 0 (no trap), `min` → TRAP (Python ValueError).
// ---------------------------------------------------------------------------

#[test]
fn dict_sum_empty_is_zero() {
    // {5:_}; del d[5] → {}; sum == 0 (the empty-list guard in the fold helper).
    let src = prog("    d: dict[int, int] = {5: 9}\n    del d[5]\n", "sum(d)");
    assert!(emit(&src).is_ok(), "sum(empty dict) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1294: WABT absent — empty-sum run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "emptysum"), 0);
}

#[test]
fn dict_min_empty_traps_like_cpython_valueerror() {
    // {5:_}; del d[5] → {}; min(empty) → ValueError → WASM `unreachable` trap.
    let src = prog("    d: dict[int, int] = {5: 9}\n    del d[5]\n", "min(d)");
    let wat = emit(&src).expect("min(empty dict) must lower (the trap is at runtime)");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1294: WABT absent — empty-min trap run skipped");
        return;
    }
    let line = go_line(&wat, "emptymin");
    assert!(
        line.contains("error") || line.contains("unreachable"),
        "min of an empty dict must TRAP (Python ValueError), got: {line}"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED: composition — a dict-key reduction feeds the scalar subset.
// ---------------------------------------------------------------------------

#[test]
fn dict_reduction_composes_in_arithmetic() {
    // sum(keys{10,20,30})*2 + max(keys) = 120 + 30 = 150.
    let src = prog(
        "    d: dict[int, int] = {10: 1, 20: 2, 30: 3}\n",
        "sum(d) * 2 + max(d)",
    );
    assert!(emit(&src).is_ok(), "compound dict reduction must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1294: WABT absent — compound run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "compound"), 150);
}

// ---------------------------------------------------------------------------
// EXECUTED: sorted(d) / sorted(d, reverse=True) produce a CPython-exact list[int]
// of the keys (verified by indexing the materialised result).
// ---------------------------------------------------------------------------

#[test]
fn dict_sorted_ascending_matches_cpython() {
    // keys {5,3,10,7} → sorted [3,5,7,10]; index 0 == 3, index 3 == 10.
    let lo = "def go() -> int:\n    d: dict[int, int] = {5: 1, 3: 2, 10: 3, 7: 4}\n    xs: list[int] = sorted(d)\n    return xs[0]\n";
    let hi = "def go() -> int:\n    d: dict[int, int] = {5: 1, 3: 2, 10: 3, 7: 4}\n    xs: list[int] = sorted(d)\n    return xs[3]\n";
    assert!(
        emit(lo).is_ok() && emit(hi).is_ok(),
        "sorted(dict) must lower"
    );
    if !wasm_runtime_available() {
        eprintln!("PMAT-1294: WABT absent — sorted run skipped");
        return;
    }
    assert_eq!(run_i64(lo, "sortlo"), 3);
    assert_eq!(run_i64(hi, "sorthi"), 10);
}

#[test]
fn dict_sorted_reverse_matches_cpython() {
    // keys {5,3,10,7} → sorted(reverse) [10,7,5,3]; index 0 == 10.
    let src = "def go() -> int:\n    d: dict[int, int] = {5: 1, 3: 2, 10: 3, 7: 4}\n    xs: list[int] = sorted(d, reverse=True)\n    return xs[0]\n";
    assert!(emit(src).is_ok(), "sorted(dict, reverse=True) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1294: WABT absent — sorted-reverse run skipped");
        return;
    }
    assert_eq!(run_i64(src, "sortrev"), 10);
}

// ---------------------------------------------------------------------------
// HONEST REFUSAL: a str-keyed dict key reduction (a `list[str]` result ABI) is
// not modelled — it must refuse at compile time, never a base-pointer misread.
// ---------------------------------------------------------------------------

#[test]
fn str_keyed_dict_reduction_refuses() {
    // min over a str-keyed dict's keys returns a str; the WASM str subset does
    // not model a reduction result in string position → a hard error.
    let src = "def go() -> int:\n    d: dict[str, int] = {\"aa\": 1, \"bb\": 2}\n    return len(min(d))\n";
    let err = emit(src).expect_err("min(dict[str,_]) key reduction must refuse (str result)");
    assert!(
        err.contains("str") || err.contains("dict") || err.contains("string position"),
        "refusal should name the unmodelled str-keyed reduction, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// HONEST REFUSAL: a dict VALUE reduction (`sum(d.values())`) reads entry+8, a
// DISTINCT materialiser not yet wired — refuse, never silently reuse the KEY
// materialiser (which would sum the keys instead of the values).
// ---------------------------------------------------------------------------

#[test]
fn dict_values_reduction_refuses() {
    let src =
        "def go() -> int:\n    d: dict[int, int] = {1: 10, 2: 20}\n    return sum(d.values())\n";
    let err = emit(src).expect_err("sum(d.values()) must refuse (value materialiser not wired)");
    assert!(
        !err.is_empty(),
        "value reduction must refuse with a hard error, got empty"
    );
}

// ---------------------------------------------------------------------------
// REGRESSION: bare set / list reductions are unaffected by the dict routing.
// ---------------------------------------------------------------------------

#[test]
fn set_and_list_reductions_still_work() {
    let set_src = prog(
        "    s: set[int] = {5, 3, 10, 7}\n",
        "sum(s) + min(s) + max(s)",
    );
    let list_src = prog(
        "    xs: list[int] = [5, 3, 10, 7]\n",
        "sum(xs) + min(xs) + max(xs)",
    );
    assert!(
        emit(&set_src).is_ok() && emit(&list_src).is_ok(),
        "set + list reductions must still lower"
    );
    if !wasm_runtime_available() {
        eprintln!("PMAT-1294: WABT absent — set/list-regression run skipped");
        return;
    }
    // {5,3,10,7}: 25 + 3 + 10 = 38.
    assert_eq!(run_i64(&set_src, "setreg"), 38);
    assert_eq!(run_i64(&list_src, "listreg"), 38);
}
