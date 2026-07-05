//! PMAT-1276 — EXECUTED FULL-PIPELINE (Python source → meta-HIR → WASM) witness
//! for native-WASM `xs.append(v)` over a named `list[int]` / `list[float]` — the
//! FIRST list-mutation-that-GROWS the WASM lane lowers.
//!
//! ## Why this witness exists
//!
//! Every prior list op was READ-ONLY (`len`/`sum`/`min`/`max`/`sorted`/…/query)
//! or an in-place ELEMENT write (`xs[i] = v`, which never changes length). Append
//! is the first op that changes the live-element COUNT, and it does so IN PLACE:
//! [`emit_list_lit`] now over-allocates a `ListLit` with `LIST_GROWTH_SLACK` spare
//! slots and records a fixed slot-capacity at `base+4`; append writes at the
//! count, bumps the count, and TRAPS at the capacity boundary. Because the record
//! never relocates, every alias (and every later `len`/`xs[i]`/`for x in xs`/
//! reduction) observes the appended element — the alias-safe posture the old
//! PMAT-1033 growth-refusal could not offer.
//!
//! A hand-built-HIR test would prove the emit handles [`Stmt::ListAppend`] but NOT
//! that the production `PythonFrontend` emits it from real `.append(...)` source
//! with the fields the emit reads, nor that the emitted WAT actually assembles and
//! runs value-identically to CPython. This witness lowers REAL Python through the
//! same profile the CLI uses for `--target wasm`, emits, assembles + runs in WABT,
//! and asserts the executed scalar VALUE-MATCHES CPython on the byte-identical
//! program.
//!
//! ## What each probe certifies
//!
//! * `append_build_loop` — the canonical build-from-empty idiom: `xs = []` then
//!   `xs.append(i*i)` in a `while` loop, then `sum`/`len`. Certifies the count
//!   header advances so the later reduction + `len` see every appended element.
//! * `append_nonempty_literal` — appending PAST a non-empty literal's entries into
//!   its reserved slack (`[10, 20]` then two appends), read back via `xs[i]`.
//! * `float_append` — the f64 element-store twin (`f64.store`), read back + `len`.
//! * `append_past_capacity_traps` — the HONEST bounded-capacity boundary: 20
//!   appends into an empty list (capacity `0 + LIST_GROWTH_SLACK == 16`) TRAP at
//!   the 17th, exactly the point past which a realloc-free bump heap cannot grow.
//!   The differential is "WASM traps where CPython would keep growing" — a
//!   documented capacity limit, never a silent heap overrun.
//! * `nested_reduce_elem_gates_helpers` — the GATE-HOLE guard: the appended value
//!   nests a `sum(...)` reduction, so the module must declare `$__wasm_list_sum_i64`.
//!   A missed gate-walker recursion into `ListAppend.elem` would leave it
//!   undeclared and `wat2wasm` would hard-fail.
//! * Honest refusals — append to a param / an alias / a `sorted` result (none of
//!   which carry spare capacity) must ERROR at compile time, not miscompile.
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
    let dir = std::env::temp_dir().join(format!("xpile-listappend-{}-{}", std::process::id(), tag));
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
// EXECUTED build-from-empty via `append` in a loop, then `sum` + `len`. The
// count header must advance so BOTH the reduction and `len` see every element.
// ---------------------------------------------------------------------------

#[test]
fn append_build_loop_executes_and_matches_cpython() {
    // xs = [0, 1, 4, 9, 16] (five appends of i*i); sum = 30, len = 5 →
    // 30*100 + 5 = 3005. CPython running the same program returns 3005.
    let src = "def go() -> int:\n    xs: list[int] = []\n    i: int = 0\n    while i < 5:\n        xs.append(i * i)\n        i = i + 1\n    total: int = 0\n    for x in xs:\n        total = total + x\n    return total * 100 + len(xs)\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("pipeline failed to lower+emit append: {e}"));
    // The capacity header + append guard must be present in the emitted WAT.
    assert!(
        wat.contains("i32.store offset=4"),
        "list literal must record a capacity header at base+4"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1276: WABT absent — emit-only append check passed, execution skipped");
        return;
    }

    let got = run_i64(src, "append_build_loop");
    assert_eq!(got, 3005, "append build loop: wasm={got} cpython=3005");
}

// ---------------------------------------------------------------------------
// EXECUTED append PAST a non-empty literal's entries into its reserved slack,
// read back via `xs[i]`.
// ---------------------------------------------------------------------------

#[test]
fn append_nonempty_literal_executes_and_matches_cpython() {
    // xs = [10, 20] then append 30, 40 → [10, 20, 30, 40].
    // xs[0]+xs[1]+xs[2]+xs[3] = 100; len = 4 → 100*10 + 4 = 1004.
    let src = "def go() -> int:\n    xs: list[int] = [10, 20]\n    xs.append(30)\n    xs.append(40)\n    return (xs[0] + xs[1] + xs[2] + xs[3]) * 10 + len(xs)\n";

    assert!(
        emit(src).is_ok(),
        "pipeline failed to lower+emit non-empty append: {:?}",
        emit(src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1276: WABT absent — emit-only non-empty append check passed, skipped run");
        return;
    }

    let got = run_i64(src, "append_nonempty");
    assert_eq!(got, 1004, "non-empty append: wasm={got} cpython=1004");
}

// ---------------------------------------------------------------------------
// EXECUTED float append — the `f64.store` element twin, read back + `len`.
// ---------------------------------------------------------------------------

#[test]
fn float_append_executes_and_matches_cpython() {
    // xs: list[float] = [] then append 1.5, 2.5, 3.0. xs[0]+xs[1]+xs[2] == 7.0
    // (int-valued check → ok=1), len = 3 → 1*10 + 3 = 13.
    let src = "def go() -> int:\n    xs: list[float] = []\n    xs.append(1.5)\n    xs.append(2.5)\n    xs.append(3.0)\n    ok: int = 0\n    if xs[0] + xs[1] + xs[2] == 7.0:\n        ok = 1\n    return ok * 10 + len(xs)\n";

    assert!(
        emit(src).is_ok(),
        "pipeline failed to lower+emit float append: {:?}",
        emit(src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1276: WABT absent — emit-only float append check passed, skipped run");
        return;
    }

    let got = run_i64(src, "float_append");
    assert_eq!(got, 13, "float append: wasm={got} cpython=13");
}

// ---------------------------------------------------------------------------
// HONEST bounded-capacity boundary: 20 appends into an EMPTY list (capacity
// 0 + LIST_GROWTH_SLACK == 16) TRAP at the 17th. The differential is "WASM
// traps where CPython keeps growing" — a documented realloc-free capacity
// limit, never a silent heap overrun.
// ---------------------------------------------------------------------------

#[test]
fn append_past_capacity_traps() {
    let src = "def go() -> int:\n    xs: list[int] = []\n    i: int = 0\n    while i < 20:\n        xs.append(i)\n        i = i + 1\n    return len(xs)\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("over-capacity append emit failed: {e}"));

    if !wasm_runtime_available() {
        eprintln!("PMAT-1276: WABT absent — emit-only over-capacity check passed, skipped run");
        return;
    }

    // The module assembles (`unreachable` is valid WAT) but running it must TRAP
    // once the appends exceed the reserved capacity.
    let wasm_path = assemble(&wat, "append_overflow");
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
        "append past capacity must TRAP (bounded bump heap); got clean run: {combined}"
    );
}

// ---------------------------------------------------------------------------
// GATE-HOLE guard — the appended VALUE nests a `sum(...)` reduction, so the
// module must declare `$__wasm_list_sum_i64`. A missed gate-walker recursion
// into `ListAppend.elem` would leave it undeclared and `wat2wasm` would fail.
// ---------------------------------------------------------------------------

#[test]
fn nested_reduce_elem_gates_helpers() {
    // xs = []; xs.append(sum(ys)) with ys = [1, 2, 3] → xs = [6]; len*100 + xs[0]
    // = 1*100 + 6 = 106. The point is the module ASSEMBLES (sum helper declared).
    let src = "def go() -> int:\n    ys: list[int] = [1, 2, 3]\n    xs: list[int] = []\n    xs.append(sum(ys))\n    return len(xs) * 100 + xs[0]\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("nested-reduce append emit failed: {e}"));
    assert!(
        wat.contains("$__wasm_list_sum_i64"),
        "nested-reduce append must declare the sum helper"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1276: WABT absent — emit-only nested-reduce check passed, skipped run");
        return;
    }

    let got = run_i64(src, "nested_reduce_append");
    assert_eq!(got, 106, "nested-reduce append: wasm={got} cpython=106");
}

// ---------------------------------------------------------------------------
// HONEST REFUSALS — appending to a list with NO reserved capacity (a param, an
// alias, or a helper-allocated result) must error at compile time.
// ---------------------------------------------------------------------------

#[test]
fn append_to_param_refuses_honestly() {
    // A list PARAM was sized exactly by the caller — no spare capacity.
    let src = "def push(xs: list[int]) -> int:\n    xs.append(9)\n    return len(xs)\n";
    let err = emit(src).expect_err("append to a param must refuse");
    assert!(
        err.contains("append") && (err.contains("param") || err.contains("spare capacity")),
        "param-append refusal should name the missing capacity, got: {err}"
    );
}

#[test]
fn append_to_alias_refuses_honestly() {
    // `ys = xs` aliases the SAME record; appending through the alias is refused
    // (the alias is not a fresh literal binding with its own slack).
    let src = "def go() -> int:\n    xs: list[int] = [1, 2]\n    ys: list[int] = xs\n    ys.append(3)\n    return len(ys)\n";
    let err = emit(src).expect_err("append to an alias must refuse");
    assert!(
        err.contains("append"),
        "alias-append refusal should name append, got: {err}"
    );
}

#[test]
fn append_to_sorted_result_refuses_honestly() {
    // `xs = sorted(ys)` is a helper-allocated result with NO spare capacity.
    let src = "def go() -> int:\n    ys: list[int] = [3, 1, 2]\n    xs: list[int] = sorted(ys)\n    xs.append(9)\n    return len(xs)\n";
    let err = emit(src).expect_err("append to a sorted result must refuse");
    assert!(
        err.contains("append"),
        "sorted-result append refusal should name append, got: {err}"
    );
}
