//! PMAT-1293 — EXECUTED witness for native-WASM `sum(s)` / `min(s)` / `max(s)`
//! over a `set[int]` — the FIRST scalar REDUCTIONS of a set in the WASM subset,
//! and the order-INDEPENDENT sibling of the order-DEFINING `sorted(s)`
//! (PMAT-1291). Runs on the bump-heap set + list runtime
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! `for x in s` (PMAT-1290) established that a set has NO defined iteration order
//! (CPython walks a hash table; the bump-heap walks storage order + swaps-last-
//! into-hole on `discard`), so only ORDER-INDEPENDENT observations of a set are
//! CPython-exact. `sum`, `min`, and `max` are exactly that class — a commutative+
//! associative fold and two order-independent extrema — so their result does not
//! depend on the storage order the set happens to hold. This is why they are the
//! clean next increment after `sorted(s)`: no new helper, no order hazard.
//!
//! The frontend materialises a reduction's iterable arg (PMAT-521), lowering
//! `sum(s)` to `Sum { list: SetToList { set } }` and `min(s)`/`max(s)` to
//! `ListMinMax { list: SetToList { set }, .. }` — exactly the `Sorted { list:
//! SetToList }` shape `sorted(s)` produces. PMAT-1293 teaches the WASM lane to
//! route those `SetToList` operands through the existing `$__wasm_set_to_list_i64`
//! materialiser (a fresh dup-free `list[int]` of the set's keys), whose base the
//! PRE-EXISTING `$__wasm_list_sum_i64` / `$__wasm_list_minmax_i64` folds then
//! consume. NO new helper and NO new gate are minted — `Expr::SetToList` already
//! arms `needs_set_to_list` (recursing into `Sum`/`ListMinMax`) and the `Sum`/
//! `ListMinMax` nodes already arm `needs_list_sum` / `needs_list_minmax`.
//!
//! Key correctness properties this pins against live `python3`:
//!   * `sum(s)` / `min(s)` / `max(s)` over an int set == CPython.
//!   * ORDER-INDEPENDENCE after a swap-into-hole `discard` (the fold is blind to
//!     the scrambled storage order).
//!   * DEDUP: a set literal with repeated keys reduces over the unique elements.
//!   * NEGATIVES fold correctly (signed `i64.add` / `i64.lt_s` / `i64.gt_s`).
//!   * `sum(empty set) == 0`; `min`/`max` of an EMPTY set TRAP (`unreachable`),
//!     matching Python `ValueError` — the same posture as `min([])`/`max([])`.
//!   * a compound `sum(s) * 2 + max(s)` composes with the scalar subset.
//!   * a str-set reduction (`min({"a","b"})` → a str result) REFUSES honestly.
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
    let dir = std::env::temp_dir().join(format!("xpile-setreduce-{}-{}", std::process::id(), tag));
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

// ---------------------------------------------------------------------------
// CONSTRUCT: the emitted WAT routes each reduction through the set→list
// materialiser and the matching pre-existing fold helper.
// ---------------------------------------------------------------------------

#[test]
fn set_sum_lowers_through_materialiser_and_sum_helper() {
    let src = prog("    s: set[int] = {5, 3, 10, 7}\n", "sum(s)");
    let wat = emit(&src).expect("`sum(s)` over a set[int] must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_set_to_list_i64")
            && wat.contains("call $__wasm_set_to_list_i64"),
        "sum(set) must declare AND call the set→list materialiser:\n{wat}"
    );
    assert!(
        wat.contains("(func $__wasm_list_sum_i64") && wat.contains("call $__wasm_list_sum_i64"),
        "sum(set) must fold the materialised list via the int-sum helper:\n{wat}"
    );
}

#[test]
fn set_minmax_lowers_through_materialiser_and_minmax_helper() {
    let src = prog("    s: set[int] = {5, 3, 10, 7}\n", "max(s)");
    let wat = emit(&src).expect("`max(s)` over a set[int] must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_set_to_list_i64")
            && wat.contains("call $__wasm_set_to_list_i64"),
        "max(set) must declare AND call the set→list materialiser:\n{wat}"
    );
    assert!(
        wat.contains("(func $__wasm_list_minmax_i64")
            && wat.contains("call $__wasm_list_minmax_i64"),
        "max(set) must fold the materialised list via the min/max helper:\n{wat}"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED: sum / min / max of an int set == CPython.
// ---------------------------------------------------------------------------

#[test]
fn set_sum_matches_cpython() {
    let build = "    s: set[int] = {5, 3, 10, 7}\n";
    let src = prog(build, "sum(s)");
    assert!(emit(&src).is_ok(), "sum(set) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1293: WABT absent — emit-only sum check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "sum"), 25);
    assert_eq!(
        run_i64(&src, "sum"),
        cpython_i64("    s={5,3,10,7}\n    return sum(s)")
    );
}

#[test]
fn set_min_matches_cpython() {
    let build = "    s: set[int] = {5, 3, 10, 7}\n";
    let src = prog(build, "min(s)");
    assert!(emit(&src).is_ok(), "min(set) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1293: WABT absent — emit-only min check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "min"), 3);
    assert_eq!(
        run_i64(&src, "min"),
        cpython_i64("    s={5,3,10,7}\n    return min(s)")
    );
}

#[test]
fn set_max_matches_cpython() {
    let build = "    s: set[int] = {5, 3, 10, 7}\n";
    let src = prog(build, "max(s)");
    assert!(emit(&src).is_ok(), "max(set) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1293: WABT absent — emit-only max check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "max"), 10);
    assert_eq!(
        run_i64(&src, "max"),
        cpython_i64("    s={5,3,10,7}\n    return max(s)")
    );
}

// ---------------------------------------------------------------------------
// EXECUTED: DEDUP — repeated keys reduce over the unique elements.
// ---------------------------------------------------------------------------

#[test]
fn set_sum_dedup_matches_cpython() {
    // {7,7,3,3,5} stores {3,5,7}; sum == 15.
    let src = prog("    s: set[int] = {7, 7, 3, 3, 5}\n", "sum(s)");
    assert!(emit(&src).is_ok(), "sum(dedup set) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1293: WABT absent — emit-only dedup check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "dedup"), 15);
}

// ---------------------------------------------------------------------------
// EXECUTED: ORDER-INDEPENDENCE — a `discard` scrambles the set's storage order
// (swap-last-into-hole), but sum/min are blind to it. This is the load-bearing
// property that makes set reductions CPython-exact.
// ---------------------------------------------------------------------------

#[test]
fn set_sum_order_independent_after_discard() {
    // {1,2,4,5}; discard(2) → {1,4,5}; sum == 10.
    let src = prog(
        "    s: set[int] = {1, 2, 4, 5}\n    s.discard(2)\n",
        "sum(s)",
    );
    assert!(emit(&src).is_ok(), "sum(set) after discard must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1293: WABT absent — emit-only discard check passed, run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "discard"), 10);
}

// ---------------------------------------------------------------------------
// EXECUTED: NEGATIVES — signed fold + signed compare.
// ---------------------------------------------------------------------------

#[test]
fn set_reductions_negatives_match_cpython() {
    let build = "    s: set[int] = {-5, -1, 0, 3}\n";
    if !wasm_runtime_available() {
        assert!(emit(&prog(build, "min(s)")).is_ok());
        eprintln!("PMAT-1293: WABT absent — negatives run skipped");
        return;
    }
    assert_eq!(run_i64(&prog(build, "min(s)"), "negmin"), -5);
    assert_eq!(run_i64(&prog(build, "max(s)"), "negmax"), 3);
    assert_eq!(run_i64(&prog(build, "sum(s)"), "negsum"), -3);
}

// ---------------------------------------------------------------------------
// EXECUTED: EMPTY set — `sum` → 0 (no trap), `min` → TRAP (Python ValueError).
// ---------------------------------------------------------------------------

#[test]
fn set_sum_empty_is_zero() {
    // {5}; discard(5) → {}; sum == 0 (the empty-list guard in the fold helper).
    let src = prog("    s: set[int] = {5}\n    s.discard(5)\n", "sum(s)");
    assert!(emit(&src).is_ok(), "sum(empty set) must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1293: WABT absent — empty-sum run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "emptysum"), 0);
}

#[test]
fn set_min_empty_traps_like_cpython_valueerror() {
    // {5}; discard(5) → {}; min(empty) → ValueError → WASM `unreachable` trap.
    let src = prog("    s: set[int] = {5}\n    s.discard(5)\n", "min(s)");
    let wat = emit(&src).expect("min(empty set) must lower (the trap is at runtime)");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1293: WABT absent — empty-min trap run skipped");
        return;
    }
    let line = go_line(&wat, "emptymin");
    assert!(
        line.contains("error") || line.contains("unreachable"),
        "min of an empty set must TRAP (Python ValueError), got: {line}"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED: composition — a reduction feeds the scalar subset.
// ---------------------------------------------------------------------------

#[test]
fn set_reduction_composes_in_arithmetic() {
    // sum({10,20,30})*2 + max({10,20,30}) = 120 + 30 = 150.
    let src = prog("    s: set[int] = {10, 20, 30}\n", "sum(s) * 2 + max(s)");
    assert!(emit(&src).is_ok(), "compound set reduction must lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1293: WABT absent — compound run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "compound"), 150);
}

// ---------------------------------------------------------------------------
// HONEST REFUSAL: a str-set reduction (a str result) is not modelled — it must
// refuse at compile time, never emit a base-pointer misread.
// ---------------------------------------------------------------------------

#[test]
fn str_set_reduction_refuses() {
    // min over a str set returns a str; the WASM str subset does not model a
    // reduction result in string position, so this must be a hard error.
    let src =
        "def go() -> int:\n    s: set[str] = {\"aa\", \"bb\", \"cc\"}\n    return len(min(s))\n";
    let err = emit(src).expect_err("min(set[str]) must refuse (str result unmodelled)");
    assert!(
        err.contains("string position") || err.contains("set") || err.contains("str"),
        "refusal should name the unmodelled str reduction, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// REGRESSION: bare `list[int]` reductions are unaffected by the set routing.
// ---------------------------------------------------------------------------

#[test]
fn list_reductions_still_work() {
    let src = prog(
        "    xs: list[int] = [5, 3, 10, 7]\n",
        "sum(xs) + min(xs) + max(xs)",
    );
    assert!(emit(&src).is_ok(), "list reductions must still lower");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1293: WABT absent — list-regression run skipped");
        return;
    }
    assert_eq!(run_i64(&src, "listred"), 38);
}
