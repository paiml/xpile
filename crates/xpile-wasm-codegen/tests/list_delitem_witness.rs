//! PMAT-1284 — EXECUTED FULL-PIPELINE (Python source → meta-HIR → WASM) witness
//! for native-WASM `del xs[i]` over a `list[int]` / `list[float]` — the in-place
//! MIRROR of `insert` (grow+shift-right ↔ shrink+shift-left) and the FIRST
//! list-mutation that SHRINKS *and* SHIFTS.
//!
//! ## Why this witness exists
//!
//! `pop` (PMAT-1278) shrinks a list only at the END and `insert` (PMAT-1282)
//! grows+shifts; `del xs[i]` removes the element at an ARBITRARY position and
//! slides the tail LEFT to close the hole. The real work lives in a single
//! `$__wasm_list_delitem` helper: normalise the index CPython-style (a negative
//! index adds the length), TRAP (`unreachable`) if it is still out of `[0, n)`
//! — matching CPython `del list[i]` → IndexError, NOT the forgiving `insert`
//! clamp — shift `[slot+1, n)` LEFT by one slot walking LOW→HIGH so nothing is
//! overwritten before it is copied, then drop the count header to `n - 1`.
//!
//! Because deletion only SHRINKS, the record never relocates and never overruns,
//! so — unlike `append`/`insert`, which grow and demand a literal-bound list's
//! spare capacity — `del` accepts ANY list local with a valid base-pointer, a
//! PARAM included (exactly like `pop`). The shift is a pure 8-byte word move, so
//! ONE helper serves both the `list[int]` (i64) and `list[float]` (f64) kinds.
//!
//! A hand-built-HIR test would prove the emit handles [`xpile_meta_hir::Stmt::DelItem`]
//! but NOT that the production `PythonFrontend` emits it from real `del xs[i]`
//! source with `is_dict == false`, nor that the emitted WAT assembles and runs
//! value-identically to CPython. This witness lowers REAL Python through the same
//! profile the CLI uses for `--target wasm`, emits, assembles + runs in WABT, and
//! asserts the executed scalar VALUE-MATCHES CPython on the byte-identical program.
//!
//! ## What each probe certifies
//!
//! * `del_middle_int` — the canonical case: `[10, 20, 30]` then `del xs[1]`
//!   → `[10, 30]`; pinned via `xs[0]*10 + xs[1] == 130`. Certifies the tail
//!   shift-LEFT and the count drop.
//! * `del_first_int` — `[10, 20, 30]` then `del xs[0]` → `[20, 30]`: the whole
//!   tail shifts, `xs[0]*10 + xs[1] == 230`.
//! * `del_last_int` — `[10, 20, 30]` then `del xs[2]` → `[10, 20]`: deleting the
//!   final element (no shift, just count--), `xs[0]*10 + xs[1] == 120`.
//! * `del_negative_one` — `[10, 20, 30]` then `del xs[-1]` → `[10, 20]`: a
//!   negative index normalises (`i += n`), `xs[0]*10 + xs[1] == 120`.
//! * `del_negative_deep` — `[1, 2, 3, 4]` then `del xs[-3]` → `[1, 3, 4]`: a
//!   negative index into the middle, pinned via
//!   `xs[0]*100 + xs[1]*10 + len == 1*100 + 3*10 + 3 == 133`.
//! * `del_float` — the f64 element twin (the shared 8-byte-word helper): `[1.5,
//!   2.5, 3.5]` then `del xs[0]` → `[2.5, 3.5]`, `xs[0] == 2.5`.
//! * `del_loop_drain_front` — repeated front-delete in a loop: `[0,1,2,3,4]` then
//!   `while i<2: del xs[0]` → `[2, 3, 4]`, pinned to `234`.
//! * `del_from_param` — `del` on a list PARAM lowers (deletion shrinks, so no
//!   spare-capacity precondition — the append/insert refusal does NOT apply).
//! * `del_out_of_range_traps` / `del_on_empty_traps` — the HONEST boundary: an
//!   out-of-range index and ANY index on an empty list TRAP (`unreachable`),
//!   matching CPython IndexError — never a silent `Vec::remove`-style wrap.
//! * `del_into_bool_list_refuses` — a `list[bool]` (i32 stride) is refused at
//!   compile time (it needs the i32-stride shift twin, deferred like `insert`),
//!   never a corrupt 8-byte-strided delete over 4-byte elements.
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
    let dir = std::env::temp_dir().join(format!("xpile-listdel-{}-{}", std::process::id(), tag));
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

/// Run a `go() -> int` probe and return the result as a SIGNED i64 (wasm-interp
/// prints i64/i32 as unsigned decimal).
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
// EXECUTED canonical delete — remove the middle element, the tail shifts LEFT.
// ---------------------------------------------------------------------------

#[test]
fn del_middle_int_executes_and_matches_cpython() {
    // xs = [10, 20, 30]; del xs[1] → [10, 30]; xs[0]*10 + xs[1] == 130.
    let src = "def go() -> int:\n    xs: list[int] = [10, 20, 30]\n    del xs[1]\n    return xs[0] * 10 + xs[1]\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("pipeline failed to lower+emit del: {e}"));
    // The single delete helper must be declared and called.
    assert!(
        wat.contains("call $__wasm_list_delitem"),
        "del must call the delitem helper"
    );
    assert!(
        wat.contains("__wasm_list_delitem (param $base i32) (param $idx i64)"),
        "del must declare the delitem helper"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1284: WABT absent — emit-only del check passed, execution skipped");
        return;
    }

    let got = run_int(src, "del_middle_int");
    assert_eq!(got, 130, "del middle int: wasm={got} cpython=130");
}

// ---------------------------------------------------------------------------
// EXECUTED delete of the FIRST element — the whole tail shifts down one.
// ---------------------------------------------------------------------------

#[test]
fn del_first_int_executes_and_matches_cpython() {
    // xs = [10, 20, 30]; del xs[0] → [20, 30]; xs[0]*10 + xs[1] == 230.
    let src = "def go() -> int:\n    xs: list[int] = [10, 20, 30]\n    del xs[0]\n    return xs[0] * 10 + xs[1]\n";

    if !wasm_runtime_available() {
        assert!(emit(src).is_ok(), "del-first must lower");
        eprintln!("PMAT-1284: WABT absent — emit-only del-first check passed, run skipped");
        return;
    }

    let got = run_int(src, "del_first");
    assert_eq!(got, 230, "del first: wasm={got} cpython=230");
}

// ---------------------------------------------------------------------------
// EXECUTED delete of the LAST element — no shift, count-- only.
// ---------------------------------------------------------------------------

#[test]
fn del_last_int_executes_and_matches_cpython() {
    // xs = [10, 20, 30]; del xs[2] → [10, 20]; xs[0]*10 + xs[1] == 120.
    let src = "def go() -> int:\n    xs: list[int] = [10, 20, 30]\n    del xs[2]\n    return xs[0] * 10 + xs[1]\n";

    if !wasm_runtime_available() {
        assert!(emit(src).is_ok(), "del-last must lower");
        eprintln!("PMAT-1284: WABT absent — emit-only del-last check passed, run skipped");
        return;
    }

    let got = run_int(src, "del_last");
    assert_eq!(got, 120, "del last: wasm={got} cpython=120");
}

// ---------------------------------------------------------------------------
// EXECUTED `del xs[-1]` — a negative index normalises (`i += n`).
// ---------------------------------------------------------------------------

#[test]
fn del_negative_one_executes_and_matches_cpython() {
    // xs = [10, 20, 30]; del xs[-1] → [10, 20]; xs[0]*10 + xs[1] == 120.
    let src = "def go() -> int:\n    xs: list[int] = [10, 20, 30]\n    del xs[-1]\n    return xs[0] * 10 + xs[1]\n";

    if !wasm_runtime_available() {
        assert!(emit(src).is_ok(), "del(-1) must lower");
        eprintln!("PMAT-1284: WABT absent — emit-only del(-1) check passed, run skipped");
        return;
    }

    let got = run_int(src, "del_neg_one");
    assert_eq!(got, 120, "del(-1): wasm={got} cpython=120");
}

// ---------------------------------------------------------------------------
// EXECUTED `del xs[-3]` — a negative index into the middle of the list.
// ---------------------------------------------------------------------------

#[test]
fn del_negative_deep_executes_and_matches_cpython() {
    // xs = [1, 2, 3, 4]; del xs[-3] → [1, 3, 4]; xs[0]*100 + xs[1]*10 + len == 133.
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3, 4]\n    del xs[-3]\n    return xs[0] * 100 + xs[1] * 10 + len(xs)\n";

    if !wasm_runtime_available() {
        assert!(emit(src).is_ok(), "del(-3) must lower");
        eprintln!("PMAT-1284: WABT absent — emit-only del(-3) check passed, run skipped");
        return;
    }

    let got = run_int(src, "del_neg_deep");
    assert_eq!(got, 133, "del(-3): wasm={got} cpython=133");
}

// ---------------------------------------------------------------------------
// EXECUTED float delete — the f64 element twin (shared 8-byte-word helper).
// ---------------------------------------------------------------------------

#[test]
fn del_float_executes_and_matches_cpython() {
    // xs: list[float] = [1.5, 2.5, 3.5]; del xs[0] → [2.5, 3.5]; xs[0] == 2.5.
    let src = "def go() -> float:\n    xs: list[float] = [1.5, 2.5, 3.5]\n    del xs[0]\n    return xs[0]\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("float del must lower+emit: {e}"));
    // The SAME single helper serves the float list — no `_f64` twin.
    assert!(
        wat.contains("call $__wasm_list_delitem"),
        "float del must call the shared delitem helper"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1284: WABT absent — emit-only float-del check passed, run skipped");
        return;
    }

    let got = run_f64(src, "del_float");
    assert!(
        (got - 2.5).abs() < 1e-12,
        "float del: wasm={got} cpython=2.5"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED loop drain — repeated front-delete against a shrinking list.
// ---------------------------------------------------------------------------

#[test]
fn del_loop_drain_front_executes_and_matches_cpython() {
    // xs = [0,1,2,3,4]; while i<2: del xs[0] → [2, 3, 4]; pinned to 234.
    let src = "def go() -> int:\n    xs: list[int] = [0, 1, 2, 3, 4]\n    i: int = 0\n    while i < 2:\n        del xs[0]\n        i = i + 1\n    return xs[0] * 100 + xs[1] * 10 + xs[2]\n";

    if !wasm_runtime_available() {
        assert!(emit(src).is_ok(), "del-loop must lower");
        eprintln!("PMAT-1284: WABT absent — emit-only del-loop check passed, run skipped");
        return;
    }

    let got = run_int(src, "del_loop_front");
    assert_eq!(got, 234, "del loop drain front: wasm={got} cpython=234");
}

// ---------------------------------------------------------------------------
// `del` on a list PARAM lowers — deletion shrinks, so NO spare-capacity gate
// (the append/insert growable-list refusal does not apply), exactly like `pop`.
// ---------------------------------------------------------------------------

#[test]
fn del_from_param_lowers_and_emits() {
    // A list PARAM has no spare capacity, but `del` never grows, so it is accepted
    // (the record only shrinks in place; the base-pointer never moves).
    let src = "def go(xs: list[int]) -> int:\n    del xs[0]\n    return len(xs)\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("param del must lower+emit: {e}"));
    assert!(
        wat.contains("call $__wasm_list_delitem"),
        "param del must call the delitem helper (deletion has no capacity precondition)"
    );
    // It must assemble (a param `go` can't be zero-arg-run under --run-all-exports,
    // so this is an emit+assemble witness, not an execute witness).
    if wasm_runtime_available() {
        assemble(&wat, "del_param");
    }
}

// ---------------------------------------------------------------------------
// HONEST boundary — an out-of-range index and any index on an empty list TRAP.
// ---------------------------------------------------------------------------

#[test]
fn del_out_of_range_traps() {
    // xs = [1, 2]; del xs[5] → CPython IndexError → `unreachable` trap.
    let src = "def go() -> int:\n    xs: list[int] = [1, 2]\n    del xs[5]\n    return len(xs)\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("oob-del emit failed: {e}"));

    if !wasm_runtime_available() {
        eprintln!("PMAT-1284: WABT absent — emit-only oob-del check passed, run skipped");
        return;
    }

    let wasm_path = assemble(&wat, "del_oob");
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
        "del out of range must TRAP (CPython IndexError); got clean run: {combined}"
    );
}

#[test]
fn del_deep_negative_boundary_traps() {
    // ★ PMAT-1289 REGRESSION GUARD (found by the pop(i) differential fuzz
    // REFUTING shipped PMAT-1284 behaviour): `del xs[-4]` on a 3-element list
    // → CPython normalises ONCE (-4 + 3 = -1, still negative → IndexError).
    // The frontend pre-rewrites the negative literal to `len(xs) - 4`
    // (PMAT-570); the old emit passed that pre-normalised value straight to
    // the helper, whose own `+= n` RE-added the length (-1 + 3 = 2) and
    // SILENTLY deleted slot 2 where CPython raises. The emit must unwrap the
    // rewrite back to the raw -4 so exactly ONE normalise applies.
    let src =
        "def go() -> int:\n    xs: list[int] = [9, 2, -6]\n    del xs[-4]\n    return len(xs)\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("deep-negative-del emit failed: {e}"));

    if !wasm_runtime_available() {
        eprintln!("PMAT-1289: WABT absent — emit-only deep-negative-del check passed, run skipped");
        return;
    }

    let wasm_path = assemble(&wat, "del_deepneg");
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
        "del xs[-4] on 3 elements must TRAP (IndexError), never delete slot 2; \
         got clean run: {combined}"
    );
}

#[test]
fn del_on_empty_traps() {
    // xs = []; del xs[0] → CPython IndexError → `unreachable` trap.
    let src = "def go() -> int:\n    xs: list[int] = []\n    del xs[0]\n    return len(xs)\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("empty-del emit failed: {e}"));

    if !wasm_runtime_available() {
        eprintln!("PMAT-1284: WABT absent — emit-only empty-del check passed, run skipped");
        return;
    }

    let wasm_path = assemble(&wat, "del_empty");
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
        "del on empty must TRAP (CPython IndexError); got clean run: {combined}"
    );
}

// ---------------------------------------------------------------------------
// HONEST REFUSAL — a `list[bool]` (i32 stride) needs the i32 shift twin.
// ---------------------------------------------------------------------------

#[test]
fn del_into_bool_list_refuses_honestly() {
    // `list[bool]` elements are i32 (4-byte), not the 8-byte word the shared
    // delete helper moves — refused, exactly like `insert`.
    let src = "def go() -> int:\n    xs: list[bool] = [True, False, True]\n    del xs[0]\n    return len(xs)\n";
    let err = emit(src).expect_err("bool-list del must refuse");
    assert!(
        err.contains("del") && err.contains("i32"),
        "bool-list-del refusal should name the i32 element kind, got: {err}"
    );
}
