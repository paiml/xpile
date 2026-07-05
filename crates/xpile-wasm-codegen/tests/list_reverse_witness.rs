//! PMAT-1286 — EXECUTED FULL-PIPELINE (Python source → meta-HIR → WASM) witness
//! for native-WASM `xs.reverse()` over a `list[int]` / `list[float]` — the FIRST
//! in-place list REORDERING (the append/insert/del/remove streak all grew or
//! shrank the count; this one leaves the count fixed and permutes the payload).
//!
//! ## Why this witness exists
//!
//! `list.reverse()` reverses the record IN PLACE and returns nothing. Unlike the
//! allocating `reversed(xs)` / `xs[::-1]` (PMAT-1253, which RETURNS a fresh list),
//! this reverses the SAME region: a two-pointer 8-byte word swap
//! `base[i] <-> base[n-1-i]` for `i` in `0..n/2` via the single
//! `$__wasm_list_reverse` helper. Because it mutates in place, the base-pointer
//! never moves, so every alias observes the reversal — and because the count is
//! unchanged (a reversal neither grows nor shrinks), the call site accepts ANY
//! scalar list local (a PARAM included) with NO spare-capacity precondition,
//! exactly like `del`/`remove`.
//!
//! ONE helper serves BOTH `list[int]` and `list[float]`: a swap MOVES two 8-byte
//! words verbatim and NEVER interprets them, so an f64 bit pattern moved as an
//! i64 word is lossless (the same insight that lets `reversed`/`concat` use one
//! helper each; only the typed-compare ops — `sorted`/`min`/`max` — need int/float
//! twins). The `i32.ge_s` loop guard is SIGNED so the EMPTY list (`n == 0` →
//! `j == -1`) and the single-element list (`i == j == 0`) both loop zero times.
//!
//! A hand-built-HIR test would prove the emit handles
//! [`xpile_meta_hir::Stmt::ListMutate`] with `Reverse` but NOT that the production
//! `PythonFrontend` emits it from real `xs.reverse()` source, nor that the emitted
//! WAT assembles and runs value-identically to CPython. This witness lowers REAL
//! Python through the same profile the CLI uses for `--target wasm`, emits,
//! assembles + runs in WABT, and asserts the executed scalar VALUE-MATCHES CPython
//! on the byte-identical program.
//!
//! ## What each probe certifies
//!
//! * `reverse_even_int` — `[10, 20, 30, 40]` → `[40, 30, 20, 10]`; pinned via
//!   `xs[0]*1000 + xs[1]*100 + xs[2]*10 + xs[3] == 43210`. The canonical even-count
//!   swap (every element moves).
//! * `reverse_odd_int` — `[1, 2, 3]` → `[3, 2, 1]`; `xs[0]*100 + xs[1]*10 + xs[2]
//!   == 321`. The MIDDLE element stays put (`i == j` at the centre is not swapped).
//! * `reverse_single_int` — `[7]` → `[7]` (no-op); `xs[0] == 7`. The single-element
//!   loop runs zero times.
//! * `reverse_empty_int` — `[]` → `[]` (no-op); `len(xs) == 0`. The SIGNED guard
//!   (`j == -1` when `n == 0`) never touches memory.
//! * `reverse_negative_int` — `[-1, -2, -3]` → `[-3, -2, -1]`; `xs[0] == -3`. The
//!   word swap is byte-verbatim, so a negative payload survives unchanged.
//! * `reverse_float` — the f64 element kind through the SAME one helper (byte-move,
//!   no f64 twin): `[1.5, 2.5, 3.5]` → `[3.5, 2.5, 1.5]`; `xs[0] == 3.5`.
//! * `reverse_twice_is_identity` — `[1, 2, 3, 4]` reversed twice → `[1, 2, 3, 4]`;
//!   `xs[0]*1000 + … == 1234`. `reverse ∘ reverse == id`, re-running over the
//!   same region.
//! * `reverse_then_append` — `[1, 2, 3]` reverse → `[3, 2, 1]`, `xs.append(9)` →
//!   `[3, 2, 1, 9]`; `== 3219`. Certifies the record stays a valid, still-growable
//!   literal-bound list after an in-place reverse (base + capacity untouched).
//! * `reverse_in_if_declares_helper` — a reverse nested in an `if` body still
//!   declares + calls the helper (the gate walker recurses into `If`/`While`);
//!   `[1, 2, 3]` → `[3, 2, 1]`, `xs[0] == 3`.
//! * `reverse_from_param` — `xs.reverse()` on a list PARAM lowers (reversal keeps
//!   the count, so NO spare-capacity precondition — the append/insert refusal does
//!   NOT apply). Emit+assemble witness (a param `go` can't be zero-arg-run).
//! * `no_reverse_omits_helper` — a module with no reverse carries NO dead
//!   `$__wasm_list_reverse` (the gate is tight).
//! * `reverse_bool_list_refuses` — a `list[bool]` (4-byte i32 stride) is refused at
//!   compile time (it needs the i32-stride helper twin, deferred like `sorted`),
//!   never a corrupt 8-byte-strided swap over 4-byte elements.
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
    let dir = std::env::temp_dir().join(format!("xpile-listrev-{}-{}", std::process::id(), tag));
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
// EXECUTED canonical reverse — two-pointer word swap over the payload.
// ---------------------------------------------------------------------------

#[test]
fn reverse_even_int_executes_and_matches_cpython() {
    // xs = [10, 20, 30, 40]; xs.reverse() → [40, 30, 20, 10];
    // xs[0]*1000 + xs[1]*100 + xs[2]*10 + xs[3] == 43210.
    let src = "def go() -> int:\n    xs: list[int] = [10, 20, 30, 40]\n    xs.reverse()\n    return xs[0] * 1000 + xs[1] * 100 + xs[2] * 10 + xs[3]\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("pipeline failed to lower+emit reverse: {e}"));
    // The single reverse helper must be declared and called (one helper, both kinds).
    assert!(
        wat.contains("call $__wasm_list_reverse"),
        "reverse must call the in-place reverse helper"
    );
    assert!(
        wat.contains("$__wasm_list_reverse (param $base i32)"),
        "reverse must declare the single in-place reverse helper"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1286: WABT absent — emit-only reverse check passed, execution skipped");
        return;
    }

    let got = run_int(src, "reverse_even_int");
    assert_eq!(got, 43210, "reverse even int: wasm={got} cpython=43210");
}

#[test]
fn reverse_odd_int_executes_and_matches_cpython() {
    // xs = [1, 2, 3]; xs.reverse() → [3, 2, 1] (middle 2 stays); == 321.
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    xs.reverse()\n    return xs[0] * 100 + xs[1] * 10 + xs[2]\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "reverse_odd_int"), 321);
}

#[test]
fn reverse_single_int_executes_and_matches_cpython() {
    // xs = [7]; xs.reverse() → [7] (no-op, loop runs zero times); xs[0] == 7.
    let src = "def go() -> int:\n    xs: list[int] = [7]\n    xs.reverse()\n    return xs[0]\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "reverse_single_int"), 7);
}

#[test]
fn reverse_empty_int_executes_and_matches_cpython() {
    // xs = []; xs.reverse() → [] (no-op; SIGNED guard, j == -1 when n == 0);
    // len(xs) == 0.
    let src = "def go() -> int:\n    xs: list[int] = []\n    xs.reverse()\n    return len(xs)\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "reverse_empty_int"), 0);
}

#[test]
fn reverse_negative_int_executes_and_matches_cpython() {
    // xs = [-1, -2, -3]; xs.reverse() → [-3, -2, -1]; xs[0] == -3.
    // A verbatim word swap preserves the (signed) payload.
    let src =
        "def go() -> int:\n    xs: list[int] = [-1, -2, -3]\n    xs.reverse()\n    return xs[0]\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "reverse_negative_int"), -3);
}

// ---------------------------------------------------------------------------
// The f64 element kind — the SAME one helper (byte-move, no typed twin).
// ---------------------------------------------------------------------------

#[test]
fn reverse_float_executes_and_matches_cpython() {
    // xs = [1.5, 2.5, 3.5]; xs.reverse() → [3.5, 2.5, 1.5]; xs[0] == 3.5.
    let src = "def go() -> float:\n    xs: list[float] = [1.5, 2.5, 3.5]\n    xs.reverse()\n    return xs[0]\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("float reverse emit failed: {e}"));
    // ONE helper serves both kinds — a list[float] reverse calls the SAME
    // `$__wasm_list_reverse` (no `_f64` twin, unlike the typed `sorted` pair).
    assert!(
        wat.contains("call $__wasm_list_reverse"),
        "a list[float] reverse must call the same one reverse helper"
    );
    assert!(
        !wat.contains("$__wasm_list_reverse_f64"),
        "reverse has NO f64 twin — a word swap moves bytes verbatim"
    );
    if !wasm_runtime_available() {
        return;
    }
    let got = run_f64(src, "reverse_float");
    assert!(
        (got - 3.5).abs() < 1e-12,
        "reverse float: wasm={got} cpython=3.5"
    );
}

// ---------------------------------------------------------------------------
// reverse ∘ reverse == identity, and reverse leaves a still-growable list.
// ---------------------------------------------------------------------------

#[test]
fn reverse_twice_is_identity_executes_and_matches_cpython() {
    // xs = [1, 2, 3, 4]; reverse; reverse → [1, 2, 3, 4]; == 1234.
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3, 4]\n    xs.reverse()\n    xs.reverse()\n    return xs[0] * 1000 + xs[1] * 100 + xs[2] * 10 + xs[3]\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "reverse_twice"), 1234);
}

#[test]
fn reverse_then_append_executes_and_matches_cpython() {
    // xs = [1, 2, 3]; reverse → [3, 2, 1]; xs.append(9) → [3, 2, 1, 9]; == 3219.
    // Certifies the in-place reverse leaves the record a valid, still-growable
    // literal-bound list (base + capacity header untouched).
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    xs.reverse()\n    xs.append(9)\n    return xs[0] * 1000 + xs[1] * 100 + xs[2] * 10 + xs[3]\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    assert!(
        wat.contains("call $__wasm_list_reverse"),
        "reverse+append must call the reverse helper"
    );
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "reverse_then_append"), 3219);
}

// ---------------------------------------------------------------------------
// Gate-walker recursion — a reverse nested in an `if` still declares the helper.
// ---------------------------------------------------------------------------

#[test]
fn reverse_in_if_declares_helper_and_matches_cpython() {
    // The reverse lives in an `if` body; the gate walker must recurse into
    // `If`/`While` to declare + emit the helper (else the call is undeclared and
    // wat2wasm fails). [1, 2, 3] → [3, 2, 1]; xs[0] == 3.
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    if len(xs) > 0:\n        xs.reverse()\n    return xs[0]\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("nested reverse emit failed: {e}"));
    assert!(
        wat.contains("$__wasm_list_reverse (param $base i32)"),
        "a reverse nested in an `if` must still DECLARE the helper (gate recurses)"
    );
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "reverse_in_if"), 3);
}

// ---------------------------------------------------------------------------
// `reverse` on a list PARAM lowers — reversal keeps the count, so NO
// spare-capacity gate (the append/insert growable-list refusal does not apply).
// ---------------------------------------------------------------------------

#[test]
fn reverse_from_param_lowers_and_emits() {
    // A list PARAM has no spare capacity, but `reverse` never grows, so it is
    // accepted (the record permutes in place; the base-pointer never moves).
    let src = "def go(xs: list[int]) -> int:\n    xs.reverse()\n    return xs[0]\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("param reverse must lower+emit: {e}"));
    assert!(
        wat.contains("call $__wasm_list_reverse"),
        "param reverse must call the reverse helper (no capacity precondition)"
    );
    // A param `go` can't be zero-arg-run under --run-all-exports, so this is an
    // emit+assemble witness, not an execute witness.
    if wasm_runtime_available() {
        assemble(&wat, "reverse_param");
    }
}

// ---------------------------------------------------------------------------
// Tight gate — a module with no reverse carries NO dead helper.
// ---------------------------------------------------------------------------

#[test]
fn no_reverse_omits_helper() {
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    return xs[0] + xs[1] + xs[2]\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    assert!(
        !wat.contains("$__wasm_list_reverse"),
        "a module with no `xs.reverse()` must NOT emit the dead reverse helper"
    );
}

// ---------------------------------------------------------------------------
// HONEST REFUSAL — a `list[bool]` (i32 stride) needs the i32 helper twin.
// ---------------------------------------------------------------------------

#[test]
fn reverse_bool_list_refuses_honestly() {
    // `list[bool]` elements are i32 (4-byte), not the 8-byte word the reverse
    // helper's swap moves — refused, exactly like `sorted`/`reversed`.
    let src = "def go() -> int:\n    xs: list[bool] = [True, False, True]\n    xs.reverse()\n    return len(xs)\n";
    let err = emit(src).expect_err("bool-list reverse must refuse");
    assert!(
        err.contains("reverse") && err.contains("i32"),
        "bool-list-reverse refusal should name the i32 element kind, got: {err}"
    );
}
