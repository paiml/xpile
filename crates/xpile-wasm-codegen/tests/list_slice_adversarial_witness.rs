//! PMAT-1272 — EXECUTED FULL-PIPELINE (Python source → meta-HIR → WASM)
//! ADVERSARIAL witness for the native-WASM `xs[lo:hi]` list slice (PMAT-1256)
//! over a named `list[int]` / `list[float]`.
//!
//! ## Why this witness exists (the zero-coverage gap it closes)
//!
//! The list-slice op shipped (PMAT-1256) with its frontend-reachability guarded
//! by the shared allocating-family witness, but NO executed witness pinned its
//! *Python slice semantics* on the WASM lane. Python's `xs[lo:hi]` is not a raw
//! `[lo, hi)` cut: a negative bound wraps (`-k` ⇒ `len+k`), an out-of-range bound
//! CLAMPS to `[0, len]`, and an inverted range (`lo >= hi` after normalisation)
//! yields the EMPTY list — never a trap, never a wrap-around read. Those three
//! rules are exactly where a naive `memcpy(base+lo*8, (hi-lo)*8)` lowering
//! silently miscompiles (negative index → wild pointer; `hi > len` → OOB read;
//! `lo > hi` → negative length). This witness lowers REAL Python through the
//! same profile the CLI uses for `--target wasm`, emits, assembles + runs in
//! WABT, and asserts the executed scalar VALUE-MATCHES CPython running the
//! byte-identical program across all three adversarial edges.
//!
//! ## What each probe certifies
//!
//! Every `go()` folds the sliced payload into one order-sensitive scalar
//! `len(ys) * 1e8 + Σ acc*100 + (v + 50)`, so a single matching result certifies
//! BOTH the slice length AND its element order/content: an off-by-one bound, a
//! wrong wrap, or a stray/clamped element changes the fingerprint. The `+ 50`
//! bias keeps each folded term in `[0, 100)` so the base-100 positional digits
//! stay unambiguous.
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

/// Assemble the real-emitted WAT + run its zero-arg `go` export in WABT.
fn run_go(wat: &str, tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!("xpile-slice-adv-{}-{}", std::process::id(), tag));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("go.wat");
    let wasm_path = dir.join("go.wasm");
    std::fs::write(&wat_path, wat).expect("write wat");

    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed for {tag}:\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&assemble.stderr)
    );

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

/// Run a `go() -> float` probe and return the result as f64.
fn run_f64(src: &str, tag: &str) -> f64 {
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed for {tag}: {e}"));
    let raw = run_go(&wat, tag);
    raw.parse::<f64>()
        .unwrap_or_else(|_| panic!("parse f64 result {raw:?} for {tag}"))
}

/// Wrap a slice `xs[lo:hi]` over the given int list literal in the shared fold.
fn int_body(list_lit: &str, lo: &str, hi: &str) -> String {
    format!(
        "    xs: list[int] = {list_lit}\n    ys: list[int] = xs[{lo}:{hi}]\n    acc: int = 0\n    for v in ys:\n        acc = acc * 100 + (v + 50)\n    return len(ys) * 100000000 + acc"
    )
}

// ---------------------------------------------------------------------------
// EXECUTED int slice — the three Python-semantics edges (negative-wrap,
// OOB-clamp, inverted→empty), each value-matched to CPython. Fingerprints are
// `python3 -c` of the byte-identical fold on the identical list.
// ---------------------------------------------------------------------------

#[test]
fn int_slice_adversarial_executes_and_matches_cpython() {
    // (tag, list-literal, lo, hi, CPython fingerprint of the identical program).
    let cases: &[(&str, &str, &str, &str, i64)] = &[
        // plain in-range interior cut → [20, 30, 40].
        ("in_range", "[10, 20, 30, 40, 50]", "1", "4", 300_708_090),
        // hi WAY past the end → Python clamps to len; [20, 30] (not an OOB read).
        ("oob_high", "[10, 20, 30]", "1", "100", 200_007_080),
        // BOTH bounds negative → each wraps by +len; [30, 40].
        ("neg_both", "[10, 20, 30, 40, 50]", "-3", "-1", 200_008_090),
        // negative lo + positive hi mixed → wrap then interior; [20, 30].
        ("neg_lo_pos_hi", "[10, 20, 30, 40]", "-3", "3", 200_007_080),
        // lo > hi after normalisation → EMPTY (never a negative-length copy).
        ("lo_gt_hi", "[10, 20, 30, 40]", "3", "1", 0),
        // both bounds past the end → EMPTY (both clamp to len).
        ("past_both", "[1, 2]", "5", "9", 0),
        // explicit whole-list bounds → identity copy [7, 8, 9].
        ("whole_explicit", "[7, 8, 9]", "0", "3", 300_575_859),
    ];

    // The pipeline must ALWAYS lower + emit (frontend reachability), even
    // without WABT.
    for (tag, lit, lo, hi, _) in cases {
        let src = format!("def go() -> int:\n{}\n", int_body(lit, lo, hi));
        assert!(
            emit(&src).is_ok(),
            "pipeline failed to lower+emit slice probe {tag}: {:?}",
            emit(&src)
        );
    }

    if !wasm_runtime_available() {
        eprintln!("PMAT-1272: WABT absent — emit-only int check passed, execution skipped");
        return;
    }

    for (tag, lit, lo, hi, expect) in cases {
        let src = format!("def go() -> int:\n{}\n", int_body(lit, lo, hi));
        let got = run_i64(&src, tag);
        assert_eq!(got, *expect, "int slice {tag}: wasm={got} cpython={expect}");
    }
}

// ---------------------------------------------------------------------------
// EXECUTED float slice — the f64 element-copy twin, over a negative-wrap range.
// `[1.5, 2.5, 3.5, 4.5][-3:-1]` → [2.5, 3.5], summed = 6.0.
// ---------------------------------------------------------------------------

#[test]
fn float_slice_executes_and_matches_cpython() {
    let src = "def go() -> float:\n    xs: list[float] = [1.5, 2.5, 3.5, 4.5]\n    ys: list[float] = xs[-3:-1]\n    acc: float = 0.0\n    for v in ys:\n        acc = acc + v\n    return acc\n";

    assert!(
        emit(src).is_ok(),
        "pipeline failed to lower+emit float slice: {:?}",
        emit(src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1272: WABT absent — emit-only float check passed, execution skipped");
        return;
    }

    let got = run_f64(src, "float_slice");
    assert!(
        (got - 6.0).abs() < 1e-9,
        "float slice: wasm={got} cpython=6.0"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED empty-list slice — slicing a real length-0 header is EMPTY, never a
// trap. Uses a param so the empty list is a genuine 0-count region.
// ---------------------------------------------------------------------------

#[test]
fn empty_list_slice_is_empty() {
    let src = "def probe(xs: list[int]) -> int:\n    ys: list[int] = xs[0:5]\n    return len(ys)\ndef go() -> int:\n    xs: list[int] = []\n    return probe(xs)\n";

    assert!(
        emit(src).is_ok(),
        "pipeline failed to lower+emit empty slice: {:?}",
        emit(src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1272: WABT absent — emit-only empty check passed, execution skipped");
        return;
    }

    let got = run_i64(src, "empty");
    assert_eq!(got, 0, "empty-list slice must be empty (len 0), got {got}");
}

// ---------------------------------------------------------------------------
// HONEST REFUSAL — a STEPPED slice `xs[i:j:k]` is outside the step-1 subset and
// must error, not silently drop the stride and miscompile.
// ---------------------------------------------------------------------------

#[test]
fn stepped_list_slice_refuses_honestly() {
    let src = "def go() -> int:\n    xs: list[int] = [1, 2, 3, 4, 5, 6]\n    ys: list[int] = xs[0:6:2]\n    return len(ys)\n";
    let err = emit(src).expect_err("a stepped list slice must refuse");
    assert!(
        err.contains("STEPPED list slice"),
        "stepped-slice refusal should name the shape, got: {err}"
    );
}
