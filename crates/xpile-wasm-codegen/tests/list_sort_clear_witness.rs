//! PMAT-1288 — EXECUTED FULL-PIPELINE (Python source → meta-HIR → WASM) witness
//! for native-WASM `xs.sort()` / `xs.sort(reverse=True)` / `xs.clear()` over a
//! `list[int]` / `list[float]` — completing the [`xpile_meta_hir::Stmt::ListMutate`]
//! family on the WASM lane (`reverse` shipped in PMAT-1286; `sort`/`sort_desc`/
//! `clear` refused until this slice).
//!
//! ## Why this witness exists
//!
//! `list.sort()` sorts the record IN PLACE and returns nothing. Unlike the
//! allocating `sorted(xs)` (PMAT-1251/1252, which RETURNS a fresh record), this
//! reorders the SAME region: the typed `$__wasm_list_sort_{i64,f64}` helpers are
//! the `sorted` pair MINUS the alloc+copy phase — the identical STABLE insertion
//! sort (the inner shift fires only on a STRICT `i64.gt_s`/`f64.gt` compare, so
//! equal elements never cross; `sort(reverse=True)` is a stable DESCENDING sort,
//! not ascending-then-reversed) run directly over the payload at `base+8`.
//! Because the sort mutates in place, the base-pointer never moves (every alias
//! observes the new order) and the count is unchanged, so ANY scalar list local
//! — a PARAM included — is accepted with NO spare-capacity precondition, exactly
//! like `reverse`/`del`/`remove`.
//!
//! `list.clear()` is the SAME bare count-header zero a dict/set `.clear()`
//! (PMAT-1236) is: the live-element count is the i32 header at `base+0` for
//! every heap record, so a list clear needs NO helper at all — one
//! `i32.store` of 0. The capacity header is untouched, so a cleared
//! literal-bound list stays appendable from count 0 (reusing its slack).
//!
//! The compare is TYPED (unlike `reverse`'s verbatim word swap), so BOTH sort
//! twins ride the single `needs_list_sort_inplace` gate; the unused twin is
//! harmless dead WAT (like `contains`/`insert`). The helper names are chosen
//! apart: `$__wasm_list_sort_i64` (in-place) vs `$__wasm_list_sorted_i64`
//! (allocating) — the coexistence probe assembles a module declaring BOTH.
//!
//! A hand-built-HIR test would prove the emit handles `Stmt::ListMutate` with
//! `Sort`/`SortDesc`/`Clear` but NOT that the production `PythonFrontend` emits
//! them from real `xs.sort()` / `xs.sort(reverse=True)` / `xs.clear()` source,
//! nor that the emitted WAT assembles and runs value-identically to CPython.
//! This witness lowers REAL Python through the same profile the CLI uses for
//! `--target wasm`, emits, assembles + runs in WABT, and asserts the executed
//! scalar VALUE-MATCHES CPython on the byte-identical program.
//!
//! ## What each probe certifies
//!
//! * `sort_int_asc` — `[3, 1, 2]` → `[1, 2, 3]`; pinned via
//!   `xs[0]*100 + xs[1]*10 + xs[2] == 123`. The canonical ascending sort.
//! * `sort_int_desc` — `xs.sort(reverse=True)`: `[3, 1, 2]` → `[3, 2, 1]`;
//!   `== 321`. The `i32.const 1` direction flag at the call site.
//! * `sort_negative_dupes` — `[5, -3, 5, 0, -3]` → `[-3, -3, 0, 5, 5]`;
//!   polynomial pin `== 13696` (CPython). SIGNED `i64.gt_s` compare (negatives
//!   order below zero) + duplicates survive (strict compare never drops equals).
//! * `sort_already_sorted` — `[1, 2, 3]` → `[1, 2, 3]` (`== 123`): the inner
//!   shift never fires (best case, zero stores beyond the key re-drop).
//! * `sort_single` / `sort_empty` — no-ops (`i32.ge_u` outer guard exits
//!   immediately for `n <= 1`), as in CPython.
//! * `sort_float_asc` / `sort_float_desc` — the f64 twin (`f64.gt`/`f64.lt`
//!   compares, the SAME opcodes as the allocating `sorted` twin): `[2.5, 0.5,
//!   1.5]` → head `0.5` ascending / `2.5` descending.
//! * `sort_then_append` — `[3, 1, 2]` sort → `[1, 2, 3]`, `xs.append(9)` →
//!   `== 1239`. The record stays a valid, still-growable literal-bound list
//!   (base + capacity untouched).
//! * `sort_vs_sorted_coexist` — `sorted(xs)` (allocating) and `xs.sort()`
//!   (in-place) in ONE function: both helper families declared, no symbol
//!   clash (`_sort_` vs `_sorted_`), executed pin `== 431134`.
//! * `sort_in_if_declares_helper` — a sort nested in an `if` body still
//!   declares + calls the helper (the gate walker recurses into `If`/`While`).
//! * `sort_from_param` — `xs.sort()` on a list PARAM lowers (count unchanged →
//!   NO spare-capacity precondition). Emit+assemble witness.
//! * `no_sort_omits_helpers` — a sort-free module carries NEITHER in-place sort
//!   twin (tight gate; `sorted`-only modules keep only the allocating pair).
//! * `sort_bool_list_refuses` — a `list[bool]` (4-byte i32 stride) is refused
//!   at compile time, never an 8-byte-strided miscompile over 4-byte elements.
//! * `clear_int_list` / `clear_float_list` — `xs.clear()` → `len(xs) == 0`,
//!   with NO helper call (a bare header zero, stride-agnostic).
//! * `clear_then_append` — `[1, 2, 3]` clear → `[]`, `append(9)` → `[9]`;
//!   `xs[0]*10 + len(xs) == 91`. The region is REUSED from count 0 (capacity
//!   header untouched by the clear).
//! * `clear_empty_is_idempotent` — clearing an empty list stays `len == 0`.
//! * `clear_from_param` — a param list clears in place (shrink-only, the
//!   base-pointer never moves → alias-safe). Emit+assemble witness.
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
    let dir = std::env::temp_dir().join(format!("xpile-listsort-{}-{}", std::process::id(), tag));
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
// EXECUTED canonical sort — ascending, descending, negatives, duplicates.
// ---------------------------------------------------------------------------

#[test]
fn sort_int_asc_executes_and_matches_cpython() {
    // xs = [3, 1, 2]; xs.sort() → [1, 2, 3]; xs[0]*100 + xs[1]*10 + xs[2] == 123.
    let src = "def go() -> int:\n    xs: list[int] = [3, 1, 2]\n    xs.sort()\n    return xs[0] * 100 + xs[1] * 10 + xs[2]\n";

    let wat = emit(src).unwrap_or_else(|e| panic!("pipeline failed to lower+emit sort: {e}"));
    assert!(
        wat.contains("call $__wasm_list_sort_i64"),
        "sort must call the in-place int sort helper:\n{wat}"
    );
    assert!(
        wat.contains("$__wasm_list_sort_i64 (param $base i32) (param $reverse i32)"),
        "sort must declare the in-place int sort helper:\n{wat}"
    );
    // Ascending → direction flag 0 at the call site (whitespace-collapsed so
    // the assertion is indentation-independent).
    let flat = wat.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.contains("i32.const 0 call $__wasm_list_sort_i64"),
        "plain sort() must pass direction flag 0 (ascending):\n{wat}"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1288: WABT absent — emit-only sort check passed, execution skipped");
        return;
    }
    let got = run_int(src, "sort_int_asc");
    assert_eq!(got, 123, "sort int asc: wasm={got} cpython=123");
}

#[test]
fn sort_int_desc_executes_and_matches_cpython() {
    // xs = [3, 1, 2]; xs.sort(reverse=True) → [3, 2, 1]; == 321.
    let src = "def go() -> int:\n    xs: list[int] = [3, 1, 2]\n    xs.sort(reverse=True)\n    return xs[0] * 100 + xs[1] * 10 + xs[2]\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("desc sort emit failed: {e}"));
    let flat = wat.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.contains("i32.const 1 call $__wasm_list_sort_i64"),
        "sort(reverse=True) must pass direction flag 1 (descending):\n{wat}"
    );
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "sort_int_desc"), 321);
}

#[test]
fn sort_negative_dupes_executes_and_matches_cpython() {
    // xs = [5, -3, 5, 0, -3]; xs.sort() → [-3, -3, 0, 5, 5]; polynomial pin
    // xs[0] + xs[1]*7 + xs[2]*49 + xs[3]*343 + xs[4]*2401 == 13696 (CPython).
    // SIGNED compare (negatives below zero) + duplicates survive the sort.
    let src = "def go() -> int:\n    xs: list[int] = [5, -3, 5, 0, -3]\n    xs.sort()\n    return xs[0] + xs[1] * 7 + xs[2] * 49 + xs[3] * 343 + xs[4] * 2401\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "sort_negative_dupes"), 13696);
}

#[test]
fn sort_already_sorted_executes_and_matches_cpython() {
    // xs = [1, 2, 3]; xs.sort() → [1, 2, 3] (inner shift never fires); == 123.
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    xs.sort()\n    return xs[0] * 100 + xs[1] * 10 + xs[2]\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "sort_already_sorted"), 123);
}

#[test]
fn sort_single_executes_and_matches_cpython() {
    // xs = [7]; xs.sort() → [7] (outer guard: 1 >= 1 exits immediately).
    let src = "def go() -> int:\n    xs: list[int] = [7]\n    xs.sort()\n    return xs[0]\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "sort_single"), 7);
}

#[test]
fn sort_empty_executes_and_matches_cpython() {
    // xs = []; xs.sort() → [] (outer guard: 1 >= 0 unsigned exits immediately).
    let src = "def go() -> int:\n    xs: list[int] = []\n    xs.sort()\n    return len(xs)\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "sort_empty"), 0);
}

// ---------------------------------------------------------------------------
// The f64 twin — IEEE-754 f64.gt/f64.lt compares (same opcodes as `sorted`).
// ---------------------------------------------------------------------------

#[test]
fn sort_float_asc_executes_and_matches_cpython() {
    // xs = [2.5, 0.5, 1.5]; xs.sort() → [0.5, 1.5, 2.5]; xs[0] == 0.5.
    let src = "def go() -> float:\n    xs: list[float] = [2.5, 0.5, 1.5]\n    xs.sort()\n    return xs[0]\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("float sort emit failed: {e}"));
    assert!(
        wat.contains("call $__wasm_list_sort_f64"),
        "a list[float] sort must call the f64 twin (typed compare):\n{wat}"
    );
    if !wasm_runtime_available() {
        return;
    }
    let got = run_f64(src, "sort_float_asc");
    assert!(
        (got - 0.5).abs() < 1e-12,
        "sort float asc: wasm={got} cpython=0.5"
    );
}

#[test]
fn sort_float_desc_executes_and_matches_cpython() {
    // xs = [2.5, 0.5, 1.5]; xs.sort(reverse=True) → [2.5, 1.5, 0.5]; xs[0] == 2.5.
    let src = "def go() -> float:\n    xs: list[float] = [2.5, 0.5, 1.5]\n    xs.sort(reverse=True)\n    return xs[0]\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    let got = run_f64(src, "sort_float_desc");
    assert!(
        (got - 2.5).abs() < 1e-12,
        "sort float desc: wasm={got} cpython=2.5"
    );
}

// ---------------------------------------------------------------------------
// sort leaves a still-growable list; in-place sort and allocating sorted coexist.
// ---------------------------------------------------------------------------

#[test]
fn sort_then_append_executes_and_matches_cpython() {
    // xs = [3, 1, 2]; sort → [1, 2, 3]; xs.append(9) → [1, 2, 3, 9]; == 1239.
    // Certifies the in-place sort leaves the record a valid, still-growable
    // literal-bound list (base + capacity header untouched).
    let src = "def go() -> int:\n    xs: list[int] = [3, 1, 2]\n    xs.sort()\n    xs.append(9)\n    return xs[0] * 1000 + xs[1] * 100 + xs[2] * 10 + xs[3]\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "sort_then_append"), 1239);
}

#[test]
fn sort_vs_sorted_coexist_executes_and_matches_cpython() {
    // ss = sorted(xs) (allocating, $__wasm_list_sorted_i64) THEN
    // xs.sort(reverse=True) (in-place, $__wasm_list_sort_i64) in ONE function:
    // both helper families are declared with no symbol clash, and the executed
    // values prove `sorted` snapshotted BEFORE the in-place sort ran.
    // xs=[4,1,3]: ss=[1,3,4]; xs→[4,3,1]; pin == 431134 (CPython).
    let src = "def go() -> int:\n    xs: list[int] = [4, 1, 3]\n    ss: list[int] = sorted(xs)\n    xs.sort(reverse=True)\n    return xs[0] * 100000 + xs[1] * 10000 + xs[2] * 1000 + ss[0] * 100 + ss[1] * 10 + ss[2]\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("coexist emit failed: {e}"));
    assert!(
        wat.contains("call $__wasm_list_sorted_i64") && wat.contains("call $__wasm_list_sort_i64"),
        "sorted (allocating) and sort (in-place) must coexist in one module:\n{wat}"
    );
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "sort_vs_sorted"), 431134);
}

// ---------------------------------------------------------------------------
// Gate-walker recursion — a sort nested in an `if` still declares the helpers.
// ---------------------------------------------------------------------------

#[test]
fn sort_in_if_declares_helper_and_matches_cpython() {
    // The sort lives in an `if` body; the gate walker must recurse into
    // `If`/`While` to declare + emit the helpers (else the call is undeclared
    // and wat2wasm fails). [3, 1, 2] → [1, 2, 3]; xs[0] == 1.
    let src = "def go() -> int:\n    xs: list[int] = [3, 1, 2]\n    if len(xs) > 0:\n        xs.sort()\n    return xs[0]\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("nested sort emit failed: {e}"));
    assert!(
        wat.contains("$__wasm_list_sort_i64 (param $base i32) (param $reverse i32)"),
        "a sort nested in an `if` must still DECLARE the helper (gate recurses):\n{wat}"
    );
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "sort_in_if"), 1);
}

// ---------------------------------------------------------------------------
// `sort` on a list PARAM lowers — the count is unchanged, so NO spare-capacity
// precondition (the append/insert growable-list refusal does not apply).
// ---------------------------------------------------------------------------

#[test]
fn sort_from_param_lowers_and_emits() {
    // A list PARAM has no spare capacity, but `sort` never grows, so it is
    // accepted (the record reorders in place; the base-pointer never moves).
    let src = "def go(xs: list[int]) -> int:\n    xs.sort()\n    return xs[0]\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("param sort must lower+emit: {e}"));
    assert!(
        wat.contains("call $__wasm_list_sort_i64"),
        "param sort must call the sort helper (no capacity precondition):\n{wat}"
    );
    // A param `go` can't be zero-arg-run under --run-all-exports, so this is an
    // emit+assemble witness, not an execute witness.
    if wasm_runtime_available() {
        assemble(&wat, "sort_param");
    }
}

// ---------------------------------------------------------------------------
// Tight gate — a sort-free module carries NEITHER in-place twin, and a
// `sorted`-only module keeps only the allocating pair.
// ---------------------------------------------------------------------------

#[test]
fn no_sort_omits_helpers() {
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    return xs[0] + xs[1] + xs[2]\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    assert!(
        !wat.contains("$__wasm_list_sort_i64") && !wat.contains("$__wasm_list_sort_f64"),
        "a module with no `xs.sort()` must NOT emit the dead in-place sort twins"
    );
}

#[test]
fn sorted_only_module_omits_inplace_twins() {
    // `sorted(xs)` (allocating) must NOT drag in the in-place pair — the two
    // families ride independent gates ("$__wasm_list_sort_i64" is a distinct
    // symbol from "$__wasm_list_sorted_i64", so the substring check is exact
    // with the trailing "_i64"/"_f64").
    let src = "def go() -> int:\n    xs: list[int] = [3, 1, 2]\n    ys: list[int] = sorted(xs)\n    return ys[0]\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    assert!(
        wat.contains("$__wasm_list_sorted_i64"),
        "sorted() must declare the allocating helper:\n{wat}"
    );
    assert!(
        !wat.contains("$__wasm_list_sort_i64") && !wat.contains("$__wasm_list_sort_f64"),
        "a sorted()-only module must NOT emit the in-place sort twins:\n{wat}"
    );
}

// ---------------------------------------------------------------------------
// HONEST REFUSAL — a `list[bool]` (i32 stride) needs an i32 helper twin.
// ---------------------------------------------------------------------------

#[test]
fn sort_bool_list_refuses_honestly() {
    // `list[bool]` elements are i32 (4-byte), not the 8-byte word the sort
    // helpers load/store — refused, exactly like `sorted`/`reversed`/`reverse`.
    let src = "def go() -> int:\n    xs: list[bool] = [True, False, True]\n    xs.sort()\n    return len(xs)\n";
    let err = emit(src).expect_err("bool-list sort must refuse");
    assert!(
        err.contains("sort") && err.contains("i32"),
        "bool-list-sort refusal should name the i32 element kind, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED list.clear() — a bare count-header zero, stride-agnostic, no helper.
// ---------------------------------------------------------------------------

#[test]
fn clear_int_list_executes_and_matches_cpython() {
    // xs = [1, 2, 3]; xs.clear() → []; len(xs) == 0. No helper call: the clear
    // is ONE i32.store of 0 into the count header at base+0.
    let src =
        "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    xs.clear()\n    return len(xs)\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("list clear emit failed: {e}"));
    assert!(
        !wat.contains("$__wasm_list_sort") && !wat.contains("$__wasm_list_reverse"),
        "a bare list.clear() needs no sort/reverse helper (tight gates):\n{wat}"
    );
    if !wasm_runtime_available() {
        eprintln!("PMAT-1288: WABT absent — emit-only clear check passed, execution skipped");
        return;
    }
    assert_eq!(run_int(src, "clear_int"), 0);
}

#[test]
fn clear_then_append_executes_and_matches_cpython() {
    // xs = [1, 2, 3]; clear → []; append(9) → [9]; xs[0]*10 + len(xs) == 91.
    // The capacity header is untouched by the clear, so the region is REUSED
    // from count 0 — same reinsert-after-clear posture the dict/set witness pins.
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3]\n    xs.clear()\n    xs.append(9)\n    return xs[0] * 10 + len(xs)\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "clear_then_append"), 91);
}

#[test]
fn clear_float_list_executes_and_matches_cpython() {
    // Stride-agnostic: a list[float] clears through the SAME bare header zero.
    let src =
        "def go() -> int:\n    xs: list[float] = [1.5, 2.5]\n    xs.clear()\n    return len(xs)\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "clear_float"), 0);
}

#[test]
fn clear_empty_is_idempotent() {
    // xs = []; clear; clear → still len 0 (writing 0 over 0 is idempotent).
    let src = "def go() -> int:\n    xs: list[int] = []\n    xs.clear()\n    xs.clear()\n    return len(xs)\n";
    let _ = emit(src).unwrap_or_else(|e| panic!("emit failed: {e}"));
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run_int(src, "clear_empty"), 0);
}

#[test]
fn clear_from_param_lowers_and_emits() {
    // A param list clears in place (shrink-only, the base-pointer never moves →
    // every alias observes len == 0). No growable precondition.
    let src = "def go(xs: list[int]) -> int:\n    xs.clear()\n    return len(xs)\n";
    let wat = emit(src).unwrap_or_else(|e| panic!("param clear must lower+emit: {e}"));
    if wasm_runtime_available() {
        assemble(&wat, "clear_param");
    }
}
