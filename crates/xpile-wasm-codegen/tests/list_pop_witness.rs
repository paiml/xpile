//! PMAT-1278 — EXECUTED FULL-PIPELINE (Python source → meta-HIR → WASM) witness
//! for native-WASM `xs.pop()` over a named `list[int]` / `list[float]` /
//! `list[bool]` — the FIRST list-mutation-that-SHRINKS the WASM lane lowers.
//!
//! ## Why this witness exists
//!
//! `append` (PMAT-1276) GROWS a list and `xs[i] = v` (PMAT-978) writes an element
//! in place; `pop()` is the mirror on the shrink side — it REMOVES the last
//! element and evaluates to it. The op is inline WAT (no `$__wasm_list_pop_*`
//! helper): guard-empty (`unreachable` = Python `IndexError`), load the last
//! element (the result, left on the stack), then decrement the i32 count header
//! at `base+0` IN PLACE. Because the base-pointer never moves, every later
//! `len(xs)` / `xs[i]` / `for x in xs` / reduction observes the shrink, and pop is
//! safe on ANY named scalar list (a param, a literal binding, or a
//! helper-allocated-then-named result) — strictly more general than `append`,
//! which needs the spare capacity only a literal binding reserves.
//!
//! A hand-built-HIR test would prove the emit handles [`xpile_meta_hir::Expr::ListPop`]
//! but NOT that the production `PythonFrontend` emits it from real `.pop()` source
//! with the fields the emit reads, nor that the emitted WAT actually assembles and
//! runs value-identically to CPython. This witness lowers REAL Python through the
//! same profile the CLI uses for `--target wasm`, emits, assembles + runs in WABT,
//! and asserts the executed scalar VALUE-MATCHES CPython on the byte-identical
//! program.
//!
//! ## What each probe certifies
//!
//! * `pop_last_int` — the canonical case: `xs = [10, 20, 30]` then `a = xs.pop()`
//!   removes 30 and `len(xs)` sees 2 → `30 + 2 = 32`. Certifies the count header
//!   decrements so the later `len` observes the removal.
//! * `pop_drain_loop` — append-then-drain interaction: build `[0, 1, 4, 9, 16]`
//!   via `xs.append(...)` in a loop, then drain via `xs.pop()` in a
//!   `while len(xs) > 0` loop, summing → 30. Exercises pop against the append
//!   growth path in one program (the header advances then retreats correctly).
//! * `pop_float` — the f64 element twin (`f64.load`): `[1.5, 2.5, 3.5].pop()`
//!   returns 3.5, read back as an f64 export.
//! * `pop_bool` — the i32 element twin (`list[bool]`, 4-byte stride): the popped
//!   `True` drives an `if` → `1 * 10 + len == 12`.
//! * `pop_two_in_one_expr` — two pops in ONE expression (`xs.pop() + xs.pop()`)
//!   evaluate left-to-right over the shrinking list → `8 + 7 = 15`.
//! * `pop_then_sum_gates_helper` — pop mixed with a `sum(xs)` in one module: the
//!   sum reads the SHRUNK count (`sum([3, 1]) + 2 == 6`) AND the module must
//!   declare `$__wasm_list_sum_i64` alongside the inline pop (a co-existence /
//!   assemble check).
//! * `pop_empty_traps` — the HONEST boundary: popping past the last element TRAPS
//!   (`unreachable`) exactly where CPython raises `IndexError`.
//! * `pop_from_param_lowers` — the "any named scalar list" generality: a list
//!   PARAM pop LOWERS + assembles (emit-only; a param export is not zero-arg so it
//!   is not executed here).
//! * Honest refusals — the INDEXED form `xs.pop(i)` (element-shifting removal), a
//!   NON-NAME receiver (`[1, 2, 3].pop()`), and a helper-allocated temporary
//!   (`sorted(ys).pop()`) must ERROR at compile time, never silently degrade.
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
    let dir = std::env::temp_dir().join(format!("xpile-listpop-{}-{}", std::process::id(), tag));
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

/// Run a `go() -> int` / `go() -> bool` probe and return the result as a SIGNED
/// i64 (wasm-interp prints i64/i32 as unsigned decimal).
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

// ---------------------------------------------------------------------------
// EXECUTED canonical pop — remove the last element, `len` sees the shrink.
// ---------------------------------------------------------------------------

#[test]
fn pop_last_int_executes_and_matches_cpython() {
    // xs = [10, 20, 30]; a = xs.pop() == 30; len(xs) == 2 → 30 + 2 = 32.
    let src = "def go() -> int:\n    xs: list[int] = [10, 20, 30]\n    a: int = xs.pop()\n    return a + len(xs)\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("pipeline failed to lower+emit pop: {e}"));
    // The empty-guard trap + inline count-decrement must be present.
    assert!(
        wat.contains("pop from empty list"),
        "pop must emit the empty-guard trap"
    );
    assert!(
        wat.contains("count = count - 1"),
        "pop must emit the inline count-header decrement"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1278: WABT absent — emit-only pop check passed, execution skipped");
        return;
    }

    let got = run_int(src, "pop_last_int");
    assert_eq!(got, 32, "pop last int: wasm={got} cpython=32");
}

// ---------------------------------------------------------------------------
// EXECUTED append-then-drain — build via `append`, drain via `pop` in a loop.
// ---------------------------------------------------------------------------

#[test]
fn pop_drain_loop_executes_and_matches_cpython() {
    // xs = [0, 1, 4, 9, 16] (five appends), then drain via pop summing → 30.
    let src = "def go() -> int:\n    xs: list[int] = []\n    i: int = 0\n    while i < 5:\n        xs.append(i * i)\n        i = i + 1\n    total: int = 0\n    while len(xs) > 0:\n        total = total + xs.pop()\n    return total\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("drain-loop emit failed: {e}"));

    if !wasm_runtime_available() {
        eprintln!("PMAT-1278: WABT absent — emit-only drain-loop check passed, run skipped");
        let _ = wat;
        return;
    }

    let got = run_int(src, "pop_drain_loop");
    assert_eq!(got, 30, "pop drain loop: wasm={got} cpython=30");
}

// ---------------------------------------------------------------------------
// EXECUTED float pop — the `f64.load` element twin.
// ---------------------------------------------------------------------------

#[test]
fn pop_float_executes_and_matches_cpython() {
    // xs: list[float] = [1.5, 2.5, 3.5]; xs.pop() == 3.5.
    let src = "def go() -> float:\n    xs: list[float] = [1.5, 2.5, 3.5]\n    return xs.pop()\n";

    assert!(
        emit(src).is_ok(),
        "pipeline failed to lower+emit float pop: {:?}",
        emit(src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1278: WABT absent — emit-only float-pop check passed, run skipped");
        return;
    }

    let got = run_f64(src, "pop_float");
    assert!(
        (got - 3.5).abs() < 1e-12,
        "float pop: wasm={got} cpython=3.5"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED bool pop — the i32 element twin (`list[bool]`, 4-byte stride).
// ---------------------------------------------------------------------------

#[test]
fn pop_bool_executes_and_matches_cpython() {
    // xs = [True, False, True]; a = xs.pop() == True → 1*10 + len(2) = 12.
    let src = "def go() -> int:\n    xs: list[bool] = [True, False, True]\n    a: bool = xs.pop()\n    r: int = 0\n    if a:\n        r = 1\n    return r * 10 + len(xs)\n";

    assert!(
        emit(src).is_ok(),
        "pipeline failed to lower+emit bool pop: {:?}",
        emit(src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1278: WABT absent — emit-only bool-pop check passed, run skipped");
        return;
    }

    let got = run_int(src, "pop_bool");
    assert_eq!(got, 12, "bool pop: wasm={got} cpython=12");
}

// ---------------------------------------------------------------------------
// EXECUTED two pops in one expression — left-to-right over the shrinking list.
// ---------------------------------------------------------------------------

#[test]
fn pop_two_in_one_expr_executes_and_matches_cpython() {
    // xs = [5, 6, 7, 8]; xs.pop() + xs.pop() == 8 + 7 = 15 (left operand first).
    let src =
        "def go() -> int:\n    xs: list[int] = [5, 6, 7, 8]\n    return xs.pop() + xs.pop()\n";

    assert!(
        emit(src).is_ok(),
        "pipeline failed to lower+emit two-pop expr: {:?}",
        emit(src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1278: WABT absent — emit-only two-pop check passed, run skipped");
        return;
    }

    let got = run_int(src, "pop_two_expr");
    assert_eq!(got, 15, "two pops in one expr: wasm={got} cpython=15");
}

// ---------------------------------------------------------------------------
// EXECUTED pop mixed with a reduction — `sum` reads the SHRUNK count, and the
// module declares the sum helper alongside the inline pop (co-existence check).
// ---------------------------------------------------------------------------

#[test]
fn pop_then_sum_gates_helper() {
    // xs = [3, 1, 2]; a = xs.pop() == 2; sum([3, 1]) + 2 == 6.
    let src = "def go() -> int:\n    xs: list[int] = [3, 1, 2]\n    a: int = xs.pop()\n    return sum(xs) + a\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("pop-then-sum emit failed: {e}"));
    assert!(
        wat.contains("$__wasm_list_sum_i64"),
        "pop-then-sum must still declare the sum helper"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1278: WABT absent — emit-only pop-then-sum check passed, run skipped");
        return;
    }

    let got = run_int(src, "pop_then_sum");
    assert_eq!(got, 6, "pop then sum: wasm={got} cpython=6");
}

// ---------------------------------------------------------------------------
// HONEST boundary — popping past the last element TRAPS (Python IndexError).
// ---------------------------------------------------------------------------

#[test]
fn pop_empty_traps() {
    // xs = [5]; first pop → 5; second pop from the now-empty list must TRAP.
    let src = "def go() -> int:\n    xs: list[int] = [5]\n    a: int = xs.pop()\n    b: int = xs.pop()\n    return a + b\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("empty-pop emit failed: {e}"));

    if !wasm_runtime_available() {
        eprintln!("PMAT-1278: WABT absent — emit-only empty-pop check passed, run skipped");
        return;
    }

    // The module assembles (`unreachable` is valid WAT) but the second pop TRAPS.
    let wasm_path = assemble(&wat, "pop_empty");
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
        "pop from empty list must TRAP (IndexError); got clean run: {combined}"
    );
}

// ---------------------------------------------------------------------------
// The "any named scalar list" generality — a list PARAM pop LOWERS + assembles.
// (A param export is not zero-arg, so it is not executed here.)
// ---------------------------------------------------------------------------

#[test]
fn pop_from_param_lowers() {
    let src = "def drain(xs: list[int]) -> int:\n    a: int = xs.pop()\n    return a + len(xs)\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("param pop must LOWER (any named list): {e}"));
    assert!(
        wat.contains("pop from empty list"),
        "param pop must emit the empty-guard trap"
    );
    if wasm_runtime_available() {
        // Assembles even though we do not call the 1-arg export.
        let _ = assemble(&wat, "pop_param");
    }
}

// ---------------------------------------------------------------------------
// HONEST REFUSALS — indexed pop, a non-name receiver, a helper temporary.
// ---------------------------------------------------------------------------

#[test]
fn pop_indexed_refuses_honestly() {
    // `xs.pop(0)` needs to shift every later element down one slot — not lowered.
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    return xs.pop(0)\n";
    let err = emit(src).expect_err("indexed pop must refuse");
    assert!(
        err.contains("pop") && err.contains("index"),
        "indexed-pop refusal should name the index, got: {err}"
    );
}

#[test]
fn pop_non_name_refuses_honestly() {
    // `[1, 2, 3].pop()` — a list literal / temporary has no decrementable header.
    let src = "def go() -> int:\n    return [1, 2, 3].pop()\n";
    let err = emit(src).expect_err("non-name pop must refuse");
    assert!(
        err.contains("pop") && err.contains("non-name"),
        "non-name-pop refusal should name the non-name receiver, got: {err}"
    );
}

#[test]
fn pop_temporary_refuses_honestly() {
    // `sorted(ys).pop()` — a helper-allocated temporary, not a decrementable NAME.
    let src = "def go(ys: list[int]) -> int:\n    return sorted(ys).pop()\n";
    let err = emit(src).expect_err("temporary pop must refuse");
    assert!(
        err.contains("pop") && err.contains("non-name"),
        "temporary-pop refusal should name the non-name receiver, got: {err}"
    );
}
