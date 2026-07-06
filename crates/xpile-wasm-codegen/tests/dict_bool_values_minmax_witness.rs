//! PMAT-1328 — EXECUTED witness for `min(d.values())` / `max(d.values())` over a
//! BOOL-valued dict (`dict[_, bool]`). This CLOSES a value-kind hole that emitted
//! INVALID WAT: the `DictView{Values}` min/max arm guarded only `of_float`, so a
//! bool-valued dict fell through, folded the 0/1 slots with the i64 min/max helper,
//! and returned an `i64` into a `bool` (i32) position — `wat2wasm` rejected it
//! (`type mismatch, expected [i32] got [i64]`). It runs on the bump-heap dict
//! runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## What this slice delivers
//!
//! A `bool` value stores as a `0`/`1` zero-extended into the 8-byte i64 slot (the
//! PMAT-1320 store). min/max are ORDER-INDEPENDENT extrema and `0`/`1` needs no
//! interpretation to compare, so the shared `$__wasm_list_minmax_i64` helper folds
//! the RAW value slots CORRECTLY (`max([True, False])` == `max(1, 0)` == `1`). The
//! ONLY fix is to WRAP the i64 extremum back to an i32 so the result IS a proper
//! `bool` (the frontend types `max(d.values())` as `bool`) — `i32.wrap_i64`,
//! mirroring `emit_dict_get`'s bool read. No float/NaN corner (a bool can't be
//! NaN) and no order/associativity corner (min/max are both), so the reduction is
//! CPython-EXACT — unlike the honestly-refused float and sum-of-bool value
//! reductions, which stay refused (regression-guarded below).
//!
//! Every observable is an INT (a bool composed through `1 if … else 0`), the same
//! text run by live python3, so `wasm-interp` and CPython compare exactly.
//!
//! WORKS (value-matched vs python3 on the identical source):
//!   * `max(d.values())` / `min(d.values())` over a mixed/all-True/all-False dict;
//!   * a single-entry dict;
//!   * a str-keyed bool dict;
//!   * min/max AFTER a store / overwrite / delete mutates the live values;
//!   * a RELOCATING grow (20 keys outrun the 16-slot literal slack) then min/max
//!     over the moved slots;
//!   * min/max composed in an int arithmetic guard;
//!   * a bool dict flowing across a FUNCTION param, reduced in the callee.
//!
//! NB `sum(d.values())` over bool was refused here in PMAT-1328 and is now WIRED
//! in PMAT-1329 (the bool→int `Map` the frontend inserts is a NO-OP on the 0/1
//! i64 slots — see `dict_bool_values_sum_witness.rs`).
//!
//! REFUSES honestly (NOT silently mis-lowered — regression guards):
//!   * `sorted(d.values())` / `reversed(d.values())` over bool (a distinct-stride
//!     `list[bool]` helper is deferred);
//!   * `for v in d.values()` over bool (the order-safety fold gate has no bool
//!     vocabulary);
//!   * `min`/`max`(`d.values()`) over a FLOAT dict (still `of_float`-refused).
//!
//! Every probe is FULL-pipeline (REAL Python → `PythonFrontend` → `emit_module` →
//! `wat2wasm` → `wasm-interp`), value-matched against LIVE python3 executing the
//! IDENTICAL source. Gated on `wasm_runtime_available()` — a clean skip (still
//! asserting emit + refusals) without WABT.

use std::path::Path;
use std::process::Command;

use depyler_frontend::PythonFrontend;
use xpile_frontend::{AliasSemantics, Frontend, LoweringProfile};
use xpile_meta_hir::Module;
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- frontend lowering (the CLI's `--target wasm` path) ---------------------

fn wasm_profile() -> LoweringProfile {
    LoweringProfile {
        alias_semantics: AliasSemantics::Reference,
        runtime_abort: true,
    }
}

fn lower(src: &str) -> Result<Module, String> {
    PythonFrontend
        .parse_and_lower_profiled(Path::new("witness.py"), src, wasm_profile())
        .map_err(|e| format!("frontend: {e}"))
}

/// FULL pipeline: Python source (one or more `def`s) → meta-HIR → WAT.
fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

// ---- the probe corpus --------------------------------------------------------

/// 20 bool-valued pairs (`k` → `k == 20`) — the grow probe's source. 20 net-new
/// keys OUTRUN the 16-slot literal slack, forcing a real relocation whose moved
/// value slots are then reduced. Only key 20 is `True`, so max→True, min→False.
fn grow_pairs() -> String {
    let entries: Vec<String> = (1..=20)
        .map(|k| format!("{k}: {}", if k == 20 { "True" } else { "False" }))
        .collect();
    format!("{{{}}}", entries.join(", "))
}

/// The zero-arg observable probes (the exports). Each `def <name>() -> int` returns
/// an integer computed from a bool min/max — the same text is run by live python3.
fn observable_probes() -> Vec<(&'static str, String)> {
    vec![
        // ── mixed dict: max → True, min → False ────────────────────────────────
        (
            "max_mixed",
            "    d: dict[int, bool] = {1: True, 2: False, 3: True}\n    return 1 if max(d.values()) else 0\n".to_string(),
        ),
        (
            "min_mixed",
            "    d: dict[int, bool] = {1: True, 2: False, 3: True}\n    return 1 if min(d.values()) else 0\n".to_string(),
        ),
        // ── homogeneous dicts ──────────────────────────────────────────────────
        (
            "max_all_false",
            "    d: dict[int, bool] = {1: False, 2: False}\n    return 1 if max(d.values()) else 0\n".to_string(),
        ),
        (
            "min_all_true",
            "    d: dict[int, bool] = {1: True, 2: True}\n    return 1 if min(d.values()) else 0\n".to_string(),
        ),
        // ── single-entry dict (min == max == the one value) ────────────────────
        (
            "max_single",
            "    d: dict[int, bool] = {5: True}\n    return 1 if max(d.values()) else 0\n".to_string(),
        ),
        (
            "min_single",
            "    d: dict[int, bool] = {5: False}\n    return 1 if min(d.values()) else 0\n".to_string(),
        ),
        // ── str-keyed bool dict (the value slot is what min/max reads) ──────────
        (
            "str_key_max",
            "    d: dict[str, bool] = {\"a\": False, \"b\": True}\n    return 1 if max(d.values()) else 0\n".to_string(),
        ),
        (
            "str_key_min",
            "    d: dict[str, bool] = {\"a\": False, \"b\": True}\n    return 1 if min(d.values()) else 0\n".to_string(),
        ),
        // ── min/max AFTER a mutation of the live values ────────────────────────
        (
            "after_store",
            "    d: dict[int, bool] = {1: False}\n    d[2] = True\n    return 1 if max(d.values()) else 0\n".to_string(),
        ),
        (
            "after_overwrite",
            "    d: dict[int, bool] = {1: True}\n    d[1] = False\n    return 1 if max(d.values()) else 0\n".to_string(),
        ),
        (
            "after_del",
            "    d: dict[int, bool] = {1: True, 2: False}\n    del d[1]\n    return 1 if max(d.values()) else 0\n".to_string(),
        ),
        // ── a RELOCATING grow then min/max over the moved slots ────────────────
        (
            "grow_max",
            format!("    d: dict[int, bool] = {}\n    return 1 if max(d.values()) else 0\n", grow_pairs()),
        ),
        (
            "grow_min",
            format!("    d: dict[int, bool] = {}\n    return 1 if min(d.values()) else 0\n", grow_pairs()),
        ),
        // ── min/max composed in an int arithmetic guard ────────────────────────
        (
            "arith_guard",
            "    d: dict[int, bool] = {1: True, 2: False}\n    return (1 if max(d.values()) else 0) + (1 if min(d.values()) else 0)\n".to_string(),
        ),
    ]
}

/// The corpus source: the observable exports PLUS a boundary helper (`reduce_param`)
/// and its observable caller. The helper takes a param (not an observable itself),
/// so it is appended verbatim and excluded from the differential name list.
fn corpus_source() -> String {
    let mut src = String::new();
    for (name, body) in observable_probes() {
        src.push_str(&format!("def {name}() -> int:\n{body}\n"));
    }
    // Boundary: a bool dict flows across a param and is reduced in the callee.
    src.push_str(
        "def reduce_param(d: dict[int, bool]) -> int:\n    return 1 if max(d.values()) else 0\n",
    );
    src.push_str(
        "def call_param() -> int:\n    d: dict[int, bool] = {1: False, 2: True}\n    return reduce_param(d)\n",
    );
    src
}

/// Every observable export name — the corpus probes plus the boundary caller (NOT
/// `reduce_param`, which takes a param).
fn observable_names() -> Vec<String> {
    let mut names: Vec<String> = observable_probes()
        .iter()
        .map(|(n, _)| n.to_string())
        .collect();
    names.push("call_param".to_string());
    names
}

// ---- CONSTRUCT assertions (hold with or without WABT) ------------------------

#[test]
fn dict_bool_values_minmax_wrap_and_reuse_helper() {
    let wat = emit(&corpus_source()).expect("the bool-value min/max corpus must lower end-to-end");
    // The reduction rides the EXISTING helpers — the values materialiser and the
    // shared i64 min/max fold. NO bespoke bool min/max helper.
    for call in [
        "call $__wasm_dict_values_to_list_i64",
        "call $__wasm_list_minmax_i64",
    ] {
        assert!(
            wat.contains(call),
            "bool-value min/max must reuse {call}:\n{wat}"
        );
    }
    // The i64 extremum (0/1) is WRAPPED back to an i32 bool — the whole fix.
    assert!(
        wat.contains("i32.wrap_i64"),
        "a bool-value min/max must wrap the i64 extremum back to an i32 bool:\n{wat}"
    );
    assert!(
        !wat.contains("$__wasm_dict_boolval_minmax"),
        "no bespoke bool min/max helper may exist (routing only):\n{wat}"
    );
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// The forms OUTSIDE the bool min/max lane refuse — never silently mis-lowered.
#[test]
fn dict_bool_values_refuse_out_of_lane_forms() {
    for (label, src, needle) in [
        // NB `sum(d.values())` over bool is now WIRED (PMAT-1329) — no longer a
        // refusal; see `dict_bool_values_sum_witness.rs`.
        // sorted over bool values — a distinct-stride list[bool] helper is deferred.
        (
            "sorted(d.values())",
            "def f() -> int:\n    d: dict[int, bool] = {1: True, 2: False}\n    xs: list[bool] = sorted(d.values())\n    return len(xs)\n".to_string(),
            "sorted",
        ),
        // for-loop over bool values — the order-safety fold gate has no bool lane.
        (
            "for v in d.values()",
            "def f() -> int:\n    d: dict[int, bool] = {1: True, 2: False}\n    c: int = 0\n    for v in d.values():\n        c = c + 1\n    return c\n".to_string(),
            "bool",
        ),
        // min/max over FLOAT values — still `of_float`-refused (regression guard so
        // the bool wrap does not accidentally admit float).
        (
            "max(float d.values())",
            "def f() -> float:\n    d: dict[int, float] = {1: 1.5, 2: 2.5}\n    return max(d.values())\n".to_string(),
            "float",
        ),
    ] {
        let err = match emit(&src) {
            Err(e) => e,
            Ok(wat) => {
                panic!("{label} must be refused but lowered:\n{wat}")
            }
        };
        assert!(
            err.contains(needle),
            "{label} refusal should mention {needle:?}, got: {err}"
        );
    }
}

// ---- WABT harness -------------------------------------------------------------

/// Parse a `name() => <ty>:<v>` line. `wasm-interp` prints integers as UNSIGNED
/// decimal; every observable here is a non-negative int, so `u64` → `i64` is exact.
fn parse_scalar_export(stdout: &str, name: &str) -> i64 {
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&format!("{name}() => ")))
        .unwrap_or_else(|| panic!("no `{name}` export in interp output:\n{stdout}"));
    line.rsplit_once(':')
        .unwrap_or_else(|| panic!("malformed export line {line:?}"))
        .1
        .trim()
        .parse::<u64>()
        .map(|u| u as i64)
        .unwrap_or_else(|_| panic!("parse scalar for {name} from {line:?}"))
}

fn assemble_and_run(wat: &str) -> (String, bool) {
    // A per-process-unique dir keeps parallel libtest threads from racing on
    // `prog.wat` (the multi-execution-path witness gotcha).
    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-dictboolminmax-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("prog.wat");
    let wasm_path = dir.join("prog.wasm");
    std::fs::write(&wat_path, wat).expect("write wat");

    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed:\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&assemble.stderr)
    );

    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    (stdout, run.status.success())
}

/// Execute the IDENTICAL corpus source in live python3, returning `name=value`
/// pairs for the observable exports — the differential ground truth.
fn python_truth(src: &str) -> Option<Vec<(String, i64)>> {
    let names = observable_names();
    let driver =
        format!("{src}\nprint(';'.join(f'{{n}}={{globals()[n]()}}' for n in {names:?}))\n");
    let out = Command::new("python3")
        .arg("-c")
        .arg(&driver)
        .output()
        .ok()?;
    if !out.status.success() {
        panic!(
            "python3 failed on the witness corpus:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Some(
        stdout
            .trim()
            .split(';')
            .map(|kv| {
                let (k, v) = kv.split_once('=').expect("k=v");
                (k.to_string(), v.parse::<i64>().expect("int"))
            })
            .collect(),
    )
}

// ---- EXECUTED witness (gated on WABT + python3) --------------------------------

#[test]
fn dict_bool_values_minmax_execute_in_wasm_and_match_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("bool min/max corpus must lower");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1328: WABT (wat2wasm / wasm-interp) absent — witness asserted at \
             emit level only (the executed leg runs every export and value-matches \
             live python3 on the identical source when WABT is present)."
        );
        return;
    }

    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1328: python3 absent — witness asserted at emit level only");
        return;
    };
    assert_eq!(
        truth.len(),
        observable_names().len(),
        "python3 must produce one value per observable probe"
    );

    let (stdout, ok) = assemble_and_run(&wat);
    assert!(ok, "wasm-interp run failed:\n{stdout}");

    for (name, expected) in &truth {
        let got = parse_scalar_export(&stdout, name);
        assert_eq!(
            got, *expected,
            "bool min/max export `{name}`: wasm {got} != cpython {expected}\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1328: {} bool-value min/max observables (mixed/homogeneous/single, \
         str-keyed, after store/overwrite/del, a relocating 20-key grow, an int \
         arith guard, and a param-boundary reduction) all == live python3.",
        truth.len()
    );
}
