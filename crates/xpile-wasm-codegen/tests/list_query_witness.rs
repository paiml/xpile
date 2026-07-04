//! PMAT-1274 — EXECUTED FULL-PIPELINE (Python source → meta-HIR → WASM) witness
//! for native-WASM `xs.count(v)` / `xs.index(v)` over a named `list[int]` /
//! `list[float]` — the FIRST list-QUERY op the WASM lane lowers.
//!
//! ## Why this witness exists (the PMAT-1244 guard, query edition)
//!
//! `xs.count(v)` / `xs.index(v)` reach the codegen as an
//! [`xpile_meta_hir::Expr::ListQuery`] carrying a `ListQueryOp::Count` /
//! `ListQueryOp::Index` discriminant. A hand-built-HIR witness would prove the
//! emit handles that node but NOT that the production `PythonFrontend` actually
//! emits it with the fields the emit reads (`list`/`op`/`arg`) from real
//! `.count()` / `.index()` source. This witness lowers REAL Python through the
//! same profile the CLI uses for `--target wasm`, emits, assembles + runs in
//! WABT, and asserts the executed scalar VALUE-MATCHES CPython running the
//! byte-identical program.
//!
//! ## What each probe certifies
//!
//! * `count` inspects EVERY element and returns the total number of matches (no
//!   short-circuit), so a hit at several positions / duplicates / a zero-count
//!   absent value all fall out — `[].count(x) == 0` and `xs.count(absent) == 0`.
//! * `index` returns the FIRST (lowest) matching position via a left-to-right
//!   scan, and TRAPS (`unreachable`) on a miss — exactly where CPython raises
//!   `ValueError`. The trap probe asserts the miss aborts the module (the
//!   differential is "both abnormally terminate").
//! * The `nested_reduce` probe (`xs.count(sum(ys))`, `idxs.count(max(ys))`) is
//!   the load-bearing GATE-HOLE guard: the needle nests a `sum`/`max` reduction,
//!   so the module must declare `$__wasm_list_sum_i64` AND
//!   `$__wasm_list_minmax_i64` AND `$__wasm_list_count_i64` — a missed
//!   gate-walker recursion into the needle would leave a helper undeclared and
//!   `wat2wasm` would hard-fail here.
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
/// wasm path (or a wat2wasm failure).
fn assemble(wat: &str, tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("xpile-listquery-{}-{}", std::process::id(), tag));
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

// ---------------------------------------------------------------------------
// EXECUTED int `xs.count(v)` — duplicates, all positions, absent (0), each
// value-matched to CPython running the identical program.
// ---------------------------------------------------------------------------

#[test]
fn int_count_executes_and_matches_cpython() {
    // count(3)=3 (×1), count(7)=1 (×10), count(100)=0 (×100), count(9)=1 (×1000)
    // → 3 + 10 + 0 + 1000 = 1013. The value 3 appears at positions 0/2/3, so a
    // full-payload scan (not a short-circuit) is required.
    let src = "def go() -> int:\n    xs: list[int] = [3, 7, 3, 3, 9]\n    acc: int = 0\n    acc = acc + xs.count(3) * 1\n    acc = acc + xs.count(7) * 10\n    acc = acc + xs.count(100) * 100\n    acc = acc + xs.count(9) * 1000\n    return acc\n";

    assert!(
        emit(src).is_ok(),
        "pipeline failed to lower+emit int count: {:?}",
        emit(src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1274: WABT absent — emit-only int count check passed, execution skipped");
        return;
    }

    let got = run_i64(src, "int_count");
    assert_eq!(got, 1013, "int count: wasm={got} cpython=1013");
}

// ---------------------------------------------------------------------------
// EXECUTED int `xs.index(v)` — the FIRST matching position (a duplicate resolves
// to its lowest index), at the first / middle / last slot.
// ---------------------------------------------------------------------------

#[test]
fn int_index_executes_and_matches_cpython() {
    // idxs = [5, 7, 7, 9]: index(5)=0 (×1), index(7)=1 FIRST of two (×10),
    // index(9)=3 (×100) → 0 + 10 + 300 = 310.
    let src = "def go() -> int:\n    idxs: list[int] = [5, 7, 7, 9]\n    acc: int = 0\n    acc = acc + idxs.index(5) * 1\n    acc = acc + idxs.index(7) * 10\n    acc = acc + idxs.index(9) * 100\n    return acc\n";

    assert!(
        emit(src).is_ok(),
        "pipeline failed to lower+emit int index: {:?}",
        emit(src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1274: WABT absent — emit-only int index check passed, execution skipped");
        return;
    }

    let got = run_i64(src, "int_index");
    assert_eq!(got, 310, "int index: wasm={got} cpython=310");
}

// ---------------------------------------------------------------------------
// EXECUTED float `xs.count(v)` / `xs.index(v)` — the f64 element load + `f64.eq`
// compare twins. Both return an i64 (Python `int`).
// ---------------------------------------------------------------------------

#[test]
fn float_query_executes_and_matches_cpython() {
    // xs = [1.5, 2.5, 1.5]: count(1.5)=2 (×1), count(2.5)=1 (×10),
    // count(9.9)=0 (×100), index(2.5)=1 (×1000) → 2 + 10 + 0 + 1000 = 1012.
    let src = "def go() -> int:\n    xs: list[float] = [1.5, 2.5, 1.5]\n    acc: int = 0\n    acc = acc + xs.count(1.5) * 1\n    acc = acc + xs.count(2.5) * 10\n    acc = acc + xs.count(9.9) * 100\n    acc = acc + xs.index(2.5) * 1000\n    return acc\n";

    assert!(
        emit(src).is_ok(),
        "pipeline failed to lower+emit float query: {:?}",
        emit(src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1274: WABT absent — emit-only float query check passed, execution skipped");
        return;
    }

    let got = run_i64(src, "float_query");
    assert_eq!(got, 1012, "float query: wasm={got} cpython=1012");
}

// ---------------------------------------------------------------------------
// EXECUTED query over a list PARAM — the common shape (a helper querying its
// list argument), not just a local literal. Also covers `xs.count(absent) == 0`
// over an EMPTY list param (a real length-0 header).
// ---------------------------------------------------------------------------

#[test]
fn param_query_executes_and_matches_cpython() {
    // cnt(xs, 8) folds over a param list; empty-list count is 0.
    let src = "def cnt(xs: list[int], v: int) -> int:\n    return xs.count(v)\ndef go() -> int:\n    xs: list[int] = [8, 1, 8]\n    empty: list[int] = []\n    return cnt(xs, 8) * 100 + cnt(xs, 5) * 10 + cnt(empty, 8)\n";

    assert!(
        emit(src).is_ok(),
        "pipeline failed to lower+emit param query: {:?}",
        emit(src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1274: WABT absent — emit-only param check passed, execution skipped");
        return;
    }

    // cnt(xs,8)=2 (×100), cnt(xs,5)=0 (×10), cnt(empty,8)=0 → 200.
    let got = run_i64(src, "param_query");
    assert_eq!(got, 200, "param query: wasm={got} cpython=200");
}

// ---------------------------------------------------------------------------
// GATE-HOLE guard — the query needle nests a `sum`/`max` reduction, so the
// module must declare `$__wasm_list_sum_i64` + `$__wasm_list_minmax_i64` +
// `$__wasm_list_count_i64`. A missed gate-walker recursion into the needle would
// leave one undeclared and `wat2wasm` would hard-fail on `assemble`.
// ---------------------------------------------------------------------------

#[test]
fn nested_reduce_needle_gates_all_helpers() {
    // xs.count(sum(ys)) = xs.count(6) = 0 (×1); idxs.count(max(ys)) =
    // idxs.count(3) = 0 (×10) → 0. The RESULT is 0 either way; the point is that
    // the module ASSEMBLES (every nested helper declared).
    let src = "def go() -> int:\n    xs: list[int] = [3, 7, 3]\n    idxs: list[int] = [5, 7, 9]\n    ys: list[int] = [1, 2, 3]\n    acc: int = 0\n    acc = acc + xs.count(sum(ys)) * 1\n    acc = acc + idxs.count(max(ys)) * 10\n    return acc\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("nested-reduce emit failed: {e}"));
    assert!(
        wat.contains("$__wasm_list_sum_i64")
            && wat.contains("$__wasm_list_minmax_i64")
            && wat.contains("$__wasm_list_count_i64"),
        "nested-reduce module must declare sum + minmax + count helpers"
    );

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1274: WABT absent — emit-only nested-reduce check passed, execution skipped"
        );
        return;
    }

    // Asserts wat2wasm accepts the module (all helpers declared) AND it runs.
    let got = run_i64(src, "nested_reduce");
    assert_eq!(got, 0, "nested-reduce query: wasm={got} cpython=0");
}

// ---------------------------------------------------------------------------
// EXECUTED `xs.index(absent)` — Python raises `ValueError`; the WASM lane traps
// (`unreachable`). The differential is "both abnormally terminate": the module
// ASSEMBLES (the `unreachable` is valid WAT) but the run does NOT succeed.
// ---------------------------------------------------------------------------

#[test]
fn index_miss_traps() {
    let src = "def go() -> int:\n    xs: list[int] = [1, 2]\n    return xs.index(9)\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("index-miss emit failed: {e}"));

    if !wasm_runtime_available() {
        eprintln!("PMAT-1274: WABT absent — emit-only index-miss check passed, execution skipped");
        return;
    }

    // The module assembles (unreachable is valid), but running it must TRAP —
    // exactly where CPython raises `ValueError: 9 is not in list`.
    let wasm_path = assemble(&wat, "index_miss");
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
        "xs.index(absent) must trap (Python ValueError); got clean run: {combined}"
    );
}

// ---------------------------------------------------------------------------
// HONEST REFUSALS — the query shapes NOT yet in the lane must error, not
// silently miscompile.
// ---------------------------------------------------------------------------

#[test]
fn non_name_list_query_refuses_honestly() {
    // `[1, 2, 3].count(2)` — a list LITERAL, not a name; the lane needs a
    // declared `list[scalar]` base-pointer. Refuse rather than miscompile.
    let src = "def go() -> int:\n    return [1, 2, 3].count(2)\n";
    let err = emit(src).expect_err("query on a list literal must refuse");
    assert!(
        err.contains("non-name list"),
        "non-name query refusal should name the shape, got: {err}"
    );
}

#[test]
fn bool_list_query_refuses_honestly() {
    // `bs.count(True)` over a `list[bool]` — elements load as i32 (0/1), not the
    // i64/f64 the query helpers compare. Refuse rather than miscompile.
    let src = "def go(bs: list[bool]) -> int:\n    return bs.count(True)\n";
    let err = emit(src).expect_err("list[bool] query must refuse");
    assert!(
        err.contains("i32") || err.contains("element kind"),
        "list[bool] query refusal should name the element kind, got: {err}"
    );
}
