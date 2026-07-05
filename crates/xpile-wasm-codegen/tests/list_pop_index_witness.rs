//! PMAT-1289 — EXECUTED FULL-PIPELINE (Python source → meta-HIR → WASM) witness
//! for native-WASM `xs.pop(i)` — the INDEXED pop — over a named `list[int]` /
//! `list[float]`, via the typed `$__wasm_list_pop_idx_{i64,f64}` helper pair.
//!
//! ## Why this witness exists
//!
//! The no-index `xs.pop()` (PMAT-1278) shrinks at the END — inline WAT, no
//! helper, no shift. The INDEXED form was refused there ("must shift every later
//! element down one slot"). `del xs[i]` (PMAT-1284) then shipped exactly that
//! shift as `$__wasm_list_delitem`; `xs.pop(i)` is its VALUE-RETURNING sibling:
//! the same CPython index normalise (negative `+= n`) + `IndexError` trap (an
//! index still out of `[0, n)` after normalising — ANY index on an empty list
//! included) + low→high left shift + count drop, PLUS a typed load of the
//! removed element BEFORE the shift closes the hole, returned as the expression
//! value. The value load/return is TYPED (like `remove`/`insert`, unlike `del`'s
//! single shared helper), so an f64 twin exists and BOTH twins ride the single
//! `needs_list_pop_idx` gate (the node carries no element-kind discriminant; the
//! unused twin is harmless dead WAT).
//!
//! Because a pop only SHRINKS (the base-pointer never moves), there is NO
//! growable-list precondition: a PARAM, a literal binding, and a helper-result
//! bound to a name all qualify — exactly like `del`/`remove`/`pop()`.
//!
//! ## What each probe certifies
//!
//! * `pop_index_middle_int` — the canonical case: `[1, 2, 3].pop(1)` removes 2,
//!   `len` sees 2 → `2*10 + 2 = 22`. Also pins the helper DECLARATION.
//! * `pop_index_negative` — CPython negative-index math: `pop(-2)` then `pop(0)`
//!   over `[10, 20, 30, 40]` → `30*100 + 10*10 + 2 = 3102`.
//! * `pop_index_last_equiv` — `xs.pop(-1)` behaves exactly like `xs.pop()`.
//! * `pop_index_front_drain_loop` — FIFO drain `xs.pop(0)` in a while loop with
//!   an order-pinning positional fold → 1234 (the shift preserves order).
//! * `pop_index_float_twin` — the f64 helper: two float pops in one expression,
//!   left-to-right over the shrinking list.
//! * `pop_index_then_append` — a popped literal-bound list STAYS appendable
//!   (base+4 capacity header untouched by the shrink).
//! * `pop_index_nested_query_gates_both_helpers` — GATE-HOLE guard, outward:
//!   `xs.pop(ys.index(30))` must declare BOTH `$__wasm_list_pop_idx_i64` AND
//!   `$__wasm_list_index_i64` in one module (the new walker recurses into the
//!   index operand).
//! * `pop_index_nested_in_query_needle` — GATE-HOLE guard, inward (the
//!   load-bearing proof of the 19-sibling-walker fan-out): `ys.index(xs.pop(0))`
//!   — the LIST-QUERY gate must see THROUGH the `Expr::ListPop` node to find its
//!   own helper use nested in the needle.
//! * `pop_index_out_of_range_traps` / `pop_index_too_negative_traps` /
//!   `pop_index_on_empty_traps` — the HONEST boundary: every out-of-range form
//!   TRAPS (`unreachable`) exactly where CPython raises `IndexError`, never a
//!   silent wrap or a last-element fallback.
//! * `pop_index_from_param_lowers` — the "any named scalar list" generality: a
//!   list PARAM pop(0) LOWERS + assembles (no capacity precondition).
//! * `pop_index_tight_gate_no_dead_helper` — gate PRECISION: a module using only
//!   the no-index `xs.pop()` (inline) must NOT declare the indexed helpers.
//! * Honest refusals — a `list[bool]` receiver (i32 stride vs the 8-byte word
//!   shift; the NO-index pop does accept bool) and a NON-NAME receiver
//!   (`[1, 2, 3].pop(0)`) must ERROR at compile time.
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
/// wasm path (asserts a clean `wat2wasm`).
fn assemble(wat: &str, tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("xpile-popidx-{}-{}", std::process::id(), tag));
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
/// a clean (non-trapping) run, and return the printed result line's value string.
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
/// (wasm-interp prints i64/i32 as unsigned decimal).
fn run_int(src: &str, tag: &str) -> i64 {
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed for {tag}: {e}"));
    let raw = run_go(&wat, tag);
    raw.parse::<u64>()
        .unwrap_or_else(|_| panic!("parse int result {raw:?} for {tag}")) as i64
}

/// Run a `go() -> float` probe and return the result as an f64.
fn run_f64(src: &str, tag: &str) -> f64 {
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed for {tag}: {e}"));
    let raw = run_go(&wat, tag);
    raw.parse::<f64>()
        .unwrap_or_else(|_| panic!("parse f64 result {raw:?} for {tag}"))
}

/// Assemble + run a probe whose `go` export must TRAP (Python raises).
fn assert_traps(src: &str, tag: &str, why: &str) {
    let wat = emit(src).unwrap_or_else(|e| panic!("{tag} emit failed: {e}"));
    if !wasm_runtime_available() {
        eprintln!("PMAT-1289: WABT absent — emit-only {tag} check passed, run skipped");
        return;
    }
    let wasm_path = assemble(&wat, tag);
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
        "{why}; got clean run: {combined}"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED canonical indexed pop — remove a MIDDLE element, `len` sees the
// shrink, and the helper is declared.
// ---------------------------------------------------------------------------

#[test]
fn pop_index_middle_int_executes_and_matches_cpython() {
    // xs = [1, 2, 3]; v = xs.pop(1) == 2; len(xs) == 2 → 2*10 + 2 = 22.
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    v: int = xs.pop(1)\n    return v * 10 + len(xs)\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("pipeline failed to lower+emit pop(i): {e}"));
    assert!(
        wat.contains("(func $__wasm_list_pop_idx_i64"),
        "indexed pop must declare the i64 helper"
    );
    assert!(
        wat.contains("call $__wasm_list_pop_idx_i64"),
        "indexed pop must call the i64 helper"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1289: WABT absent — emit-only pop(i) check passed, execution skipped");
        return;
    }

    let got = run_int(src, "pop_index_middle");
    assert_eq!(got, 22, "pop(1) middle int: wasm={got} cpython=22");
}

// ---------------------------------------------------------------------------
// EXECUTED negative indices — the CPython `i += n` normalise.
// ---------------------------------------------------------------------------

#[test]
fn pop_index_negative_executes_and_matches_cpython() {
    // xs = [10, 20, 30, 40]; a = xs.pop(-2) == 30 (xs = [10, 20, 40]);
    // b = xs.pop(0) == 10 (xs = [20, 40]) → 30*100 + 10*10 + 2 = 3102.
    let src = "def go() -> int:\n    xs: list[int] = [10, 20, 30, 40]\n    a: int = xs.pop(-2)\n    b: int = xs.pop(0)\n    return a * 100 + b * 10 + len(xs)\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("negative-index emit failed: {e}"));

    if !wasm_runtime_available() {
        eprintln!("PMAT-1289: WABT absent — emit-only negative-index check passed, run skipped");
        let _ = wat;
        return;
    }

    let got = run_int(src, "pop_index_negative");
    assert_eq!(got, 3102, "pop negative indices: wasm={got} cpython=3102");
}

#[test]
fn pop_index_last_equiv_executes_and_matches_cpython() {
    // xs.pop(-1) ≡ xs.pop(): [7, 8, 9].pop(-1) == 9, len == 2 → 92.
    let src = "def go() -> int:\n    xs: list[int] = [7, 8, 9]\n    v: int = xs.pop(-1)\n    return v * 10 + len(xs)\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("pop(-1) emit failed: {e}"));

    if !wasm_runtime_available() {
        eprintln!("PMAT-1289: WABT absent — emit-only pop(-1) check passed, run skipped");
        let _ = wat;
        return;
    }

    let got = run_int(src, "pop_index_last_equiv");
    assert_eq!(got, 92, "pop(-1) last-equiv: wasm={got} cpython=92");
}

// ---------------------------------------------------------------------------
// EXECUTED FIFO drain — pop(0) in a loop; the positional fold pins ORDER, so a
// wrong shift direction / off-by-one would change the value, not just the sum.
// ---------------------------------------------------------------------------

#[test]
fn pop_index_front_drain_loop_executes_and_matches_cpython() {
    // xs = [1, 2, 3, 4]; drain front-first: total = total*10 + xs.pop(0) → 1234.
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3, 4]\n    total: int = 0\n    while len(xs) > 0:\n        total = total * 10 + xs.pop(0)\n    return total\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("front-drain emit failed: {e}"));

    if !wasm_runtime_available() {
        eprintln!("PMAT-1289: WABT absent — emit-only front-drain check passed, run skipped");
        let _ = wat;
        return;
    }

    let got = run_int(src, "pop_index_front_drain");
    assert_eq!(got, 1234, "pop(0) FIFO drain: wasm={got} cpython=1234");
}

// ---------------------------------------------------------------------------
// EXECUTED float twin — the `$__wasm_list_pop_idx_f64` helper (typed load).
// ---------------------------------------------------------------------------

#[test]
fn pop_index_float_twin_executes_and_matches_cpython() {
    // xs = [1.5, 2.5, 3.5]; pop(1) == 2.5 (xs = [1.5, 3.5]); pop(0) == 1.5 →
    // 2.5 + 1.5 = 4.0 (left-to-right over the shrinking list).
    let src = "def go() -> float:\n    xs: list[float] = [1.5, 2.5, 3.5]\n    return xs.pop(1) + xs.pop(0)\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("float pop(i) emit failed: {e}"));
    assert!(
        wat.contains("call $__wasm_list_pop_idx_f64"),
        "float indexed pop must call the f64 twin"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1289: WABT absent — emit-only float pop(i) check passed, run skipped");
        return;
    }

    let got = run_f64(src, "pop_index_float");
    assert!(
        (got - 4.0).abs() < 1e-12,
        "float pop(i): wasm={got} cpython=4.0"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED shrink-then-grow — a popped literal-bound list STAYS appendable (the
// base+4 capacity header is untouched by the shrink).
// ---------------------------------------------------------------------------

#[test]
fn pop_index_then_append_executes_and_matches_cpython() {
    // xs = [1, 2, 3]; xs.pop(0) → [2, 3]; xs.append(9) → [2, 3, 9];
    // positional fold → 239.
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    a: int = xs.pop(0)\n    xs.append(9)\n    total: int = 0\n    for x in xs:\n        total = total * 10 + x\n    return total\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("pop-then-append emit failed: {e}"));

    if !wasm_runtime_available() {
        eprintln!("PMAT-1289: WABT absent — emit-only pop-then-append check passed, run skipped");
        let _ = wat;
        return;
    }

    let got = run_int(src, "pop_index_then_append");
    assert_eq!(got, 239, "pop(0) then append: wasm={got} cpython=239");
}

// ---------------------------------------------------------------------------
// GATE-HOLE guards — nested gated ops in BOTH directions.
// ---------------------------------------------------------------------------

#[test]
fn pop_index_nested_query_gates_both_helpers() {
    // OUTWARD: the pop INDEX nests a gated query — `xs.pop(ys.index(30))`.
    // ys.index(30) == 0 → xs.pop(0) == 10, len(xs) == 2 → 102. The module must
    // declare BOTH helper families or wat2wasm hard-fails.
    let src = "def go() -> int:\n    xs: list[int] = [10, 20, 30]\n    ys: list[int] = [30, 10]\n    v: int = xs.pop(ys.index(30))\n    return v * 10 + len(xs)\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("nested-query emit failed: {e}"));
    assert!(
        wat.contains("(func $__wasm_list_pop_idx_i64"),
        "nested-query module must declare the pop-idx helper"
    );
    assert!(
        wat.contains("(func $__wasm_list_index_i64"),
        "nested-query module must declare the list-index helper (the pop walker \
         must recurse into the index operand)"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1289: WABT absent — emit-only nested-query check passed, run skipped");
        return;
    }

    let got = run_int(src, "pop_index_nested_query");
    assert_eq!(got, 102, "pop(ys.index(30)): wasm={got} cpython=102");
}

#[test]
fn pop_index_nested_in_query_needle_gates_both_helpers() {
    // INWARD (the load-bearing proof of the sibling-walker fan-out): a pop
    // nested in a query NEEDLE — `ys.index(xs.pop(0))`. The LIST-QUERY gate
    // walker must see THROUGH the `Expr::ListPop` node, else
    // `$__wasm_list_index_i64` is undeclared → hard wat2wasm failure.
    // xs.pop(0) == 5 (xs = [6]); ys.index(5) == 1 → 1*10 + 1 = 11.
    let src = "def go() -> int:\n    xs: list[int] = [5, 6]\n    ys: list[int] = [7, 5, 6]\n    p: int = ys.index(xs.pop(0))\n    return p * 10 + len(xs)\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("nested-in-needle emit failed: {e}"));
    assert!(
        wat.contains("(func $__wasm_list_index_i64"),
        "the query gate must see through a ListPop nested in its needle"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1289: WABT absent — emit-only nested-in-needle check passed, run skipped");
        return;
    }

    let got = run_int(src, "pop_index_in_needle");
    assert_eq!(got, 11, "ys.index(xs.pop(0)): wasm={got} cpython=11");
}

// ---------------------------------------------------------------------------
// HONEST trap boundaries — every out-of-range form is a deterministic trap
// exactly where CPython raises IndexError.
// ---------------------------------------------------------------------------

#[test]
fn pop_index_out_of_range_traps() {
    // xs = [1, 2, 3]; xs.pop(5) → IndexError → trap.
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    return xs.pop(5)\n";
    assert_traps(
        src,
        "pop_idx_oob",
        "pop(5) on a 3-element list must TRAP (IndexError)",
    );
}

#[test]
fn pop_index_too_negative_traps() {
    // xs = [1, 2, 3]; xs.pop(-100) → still negative after += n → trap.
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    return xs.pop(-100)\n";
    assert_traps(
        src,
        "pop_idx_negoob",
        "pop(-100) on a 3-element list must TRAP (IndexError)",
    );
}

#[test]
fn pop_index_on_empty_traps() {
    // xs = []; xs.pop(0) → ANY index on an empty list → trap.
    let src = "def go() -> int:\n    xs: list[int] = []\n    return xs.pop(0)\n";
    assert_traps(
        src,
        "pop_idx_empty",
        "pop(0) on an empty list must TRAP (IndexError)",
    );
}

#[test]
fn pop_index_deep_negative_boundary_traps() {
    // ★ THE DOUBLE-NORMALIZE REGRESSION GUARD: `[5].pop(-2)` → CPython
    // normalises ONCE (-2 + 1 = -1, still negative → IndexError). The frontend
    // pre-rewrites the negative literal to `len(xs) - 2` (PMAT-570, for the
    // Rust lane's `usize` remove); if the WASM emit passed that PRE-normalised
    // value to the helper, the helper's own `+= n` would re-add the length
    // (-1 + 1 = 0) and SILENTLY pop slot 0 where CPython raises. The emit must
    // unwrap the rewrite back to the raw -2 so exactly ONE normalise applies.
    let src = "def go() -> int:\n    xs: list[int] = [5]\n    return xs.pop(-2)\n";
    assert_traps(
        src,
        "pop_idx_deepneg",
        "pop(-2) on a 1-element list must TRAP (IndexError), never pop slot 0",
    );
}

// ---------------------------------------------------------------------------
// EXECUTED runtime (non-literal) indices — the frontend wraps these in the
// PMAT-609 `__pidx` normalize Block; the emit unwraps to the RAW index and the
// helper applies the one CPython normalise.
// ---------------------------------------------------------------------------

#[test]
fn pop_index_runtime_positive_executes_and_matches_cpython() {
    // i = 1 (a runtime NAME, Block-wrapped by the frontend): pop(i) == 20,
    // len == 2 → 20*10 + 2 = 202.
    let src = "def go() -> int:\n    xs: list[int] = [10, 20, 30]\n    i: int = 1\n    v: int = xs.pop(i)\n    return v * 10 + len(xs)\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("runtime-index emit failed: {e}"));
    assert!(
        wat.contains("call $__wasm_list_pop_idx_i64"),
        "runtime-index pop must route through the helper (Block unwrapped)"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1289: WABT absent — emit-only runtime-index check passed, run skipped");
        return;
    }

    let got = run_int(src, "pop_index_runtime_pos");
    assert_eq!(got, 202, "pop(i) runtime positive: wasm={got} cpython=202");
}

#[test]
fn pop_index_runtime_negative_executes_and_matches_cpython() {
    // i = -2 (runtime): CPython normalises -2 + 3 = 1 → pop == 20, len == 2 →
    // 202. Certifies the unwrapped RAW index + helper normalise composition.
    let src = "def go() -> int:\n    xs: list[int] = [10, 20, 30]\n    i: int = -2\n    v: int = xs.pop(i)\n    return v * 10 + len(xs)\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("runtime-negative emit failed: {e}"));

    if !wasm_runtime_available() {
        eprintln!("PMAT-1289: WABT absent — emit-only runtime-negative check passed, run skipped");
        let _ = wat;
        return;
    }

    let got = run_int(src, "pop_index_runtime_neg");
    assert_eq!(got, 202, "pop(i) runtime negative: wasm={got} cpython=202");
}

#[test]
fn pop_index_runtime_deep_negative_traps() {
    // i = -5 on a 3-element list: -5 + 3 = -2, still negative → IndexError →
    // trap (the runtime twin of the deep-negative boundary).
    let src =
        "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    i: int = -5\n    return xs.pop(i)\n";
    assert_traps(
        src,
        "pop_idx_runtime_deepneg",
        "pop(-5) runtime on a 3-element list must TRAP (IndexError)",
    );
}

// ---------------------------------------------------------------------------
// Generality — a PARAM pops by index (no growable-list precondition).
// ---------------------------------------------------------------------------

#[test]
fn pop_index_from_param_lowers_and_assembles() {
    // A list PARAM (caller-sized, NO capacity slack) still qualifies — a shrink
    // reads a header it can only make smaller. Emit-only: a param export is not
    // zero-arg, so it is not executed here.
    let src = "def take_first(xs: list[int]) -> int:\n    return xs.pop(0)\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("param pop(i) must lower, got: {e}"));
    assert!(
        wat.contains("call $__wasm_list_pop_idx_i64"),
        "param pop(0) must route through the indexed helper"
    );
    if wasm_runtime_available() {
        assemble(&wat, "pop_idx_param");
    }
}

// ---------------------------------------------------------------------------
// Gate PRECISION — the inline no-index pop must NOT arm the indexed gate.
// ---------------------------------------------------------------------------

#[test]
fn pop_index_tight_gate_no_dead_helper() {
    // Only the INLINE no-index pop is used → the indexed helper pair must NOT
    // be declared (the gate keys on `index: Some`).
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    return xs.pop()\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("no-index pop emit failed: {e}"));
    assert!(
        !wat.contains("$__wasm_list_pop_idx_"),
        "a module with only no-index pop must not declare the indexed helpers"
    );
}

// ---------------------------------------------------------------------------
// HONEST REFUSALS — list[bool] (i32 stride) and a non-name receiver.
// ---------------------------------------------------------------------------

#[test]
fn pop_index_bool_list_refuses_honestly() {
    // The indexed pop shifts 8-byte words; a list[bool] (4-byte i32 stride)
    // would need an i32-stride twin. The NO-index pop DOES accept bool — only
    // the indexed form refuses.
    let src = "def go() -> bool:\n    xs: list[bool] = [True, False, True]\n    return xs.pop(0)\n";
    let err = emit(src).expect_err("list[bool] indexed pop must refuse");
    assert!(
        err.contains("pop(i)") && err.contains("i32-stride"),
        "bool-list indexed-pop refusal should name the stride gap, got: {err}"
    );
}

#[test]
fn pop_index_non_name_refuses_honestly() {
    // `[1, 2, 3].pop(0)` — a literal/temporary has no shiftable named record.
    let src = "def go() -> int:\n    return [1, 2, 3].pop(0)\n";
    let err = emit(src).expect_err("non-name indexed pop must refuse");
    assert!(
        err.contains("pop") && err.contains("non-name"),
        "non-name indexed-pop refusal should name the receiver, got: {err}"
    );
}
