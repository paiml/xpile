//! PMAT-1329 — EXECUTED witness for `sum(d.values())` over a BOOL-valued dict
//! (`dict[_, bool]`). This CLOSES the last of the PMAT-1320 bool-value reductions
//! that was still refused: `sum(d.values())` over bool fell to the non-name-list
//! refusal because Python's `sum` counts each `True` as 1 (bool is an int
//! subtype), so the frontend (PMAT-565) wraps `d.values()` in a bool→int `Map`
//! and the WASM `emit_list_sum` had no `Map` arm. It runs on the bump-heap dict
//! runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## What this slice delivers
//!
//! A `bool` value is STORED as a `0`/`1` zero-extended into the 8-byte i64 slot
//! (the PMAT-1320 store), so the frontend's bool→int `Map` is a runtime NO-OP:
//! the raw value slots ARE the 0/1 ints. `emit_list_sum` recognises the
//! `Map { list: DictView{Values}, lambda: int(__b) }` shape over a bool-valued
//! dict, materialises the values to a fresh `list[int]` via the SHARED
//! `$__wasm_dict_values_to_list_i64` (duplicates kept), and folds them with the
//! SHARED `$__wasm_list_sum_i64`. `sum` is COMMUTATIVE+ASSOCIATIVE over integers,
//! so the dict's arbitrary storage order is irrelevant → the total (the count of
//! `True`) is CPython-EXACT. No bespoke bool-sum helper; NO new meta-HIR variant.
//! The `Map` arm added to `expr_has_dict_values_to_list` arms the materialiser
//! (else it would be undeclared at its call site — the recurring gate-hole class).
//!
//! Every observable is an INT (the count of `True`), the same text run by live
//! python3, so `wasm-interp` and CPython compare exactly.
//!
//! WORKS (value-matched vs python3 on the identical source):
//!   * `sum(d.values())` over a mixed / all-True / all-False dict;
//!   * a single-entry dict and a str-keyed bool dict;
//!   * sum AFTER a store / overwrite / delete mutates the live values;
//!   * an EMPTIED dict (del the last entry) → `sum == 0` (the empty-list guard);
//!   * a RELOCATING grow (20 keys outrun the 16-slot literal slack) then sum;
//!   * the sum composed in int arithmetic (`sum(d.values()) * 10 + len(d)`);
//!   * a bool dict flowing across a FUNCTION param, summed in the callee.
//!
//! REFUSES honestly (NOT silently mis-lowered — regression guards):
//!   * `sum(d.values())` over a FLOAT dict (order-sensitive `+`-fold — still
//!     `of_float`-refused);
//!   * `sum(xs)` over a bare `list[bool]` NAME (the `Map` wraps an `Ident`, not a
//!     `DictView{Values}` — out of this slice's dict lane, still refused);
//!   * `sorted(d.values())` over bool (a distinct-stride `list[bool]` helper is
//!     deferred).
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

/// 20 bool-valued pairs (`k` → `k` is odd) — the grow probe's source. 20 net-new
/// keys OUTRUN the 16-slot literal slack, forcing a real relocation whose moved
/// value slots are then summed. 10 odd keys are `True`, so the sum is 10.
fn grow_pairs() -> String {
    let entries: Vec<String> = (1..=20)
        .map(|k| format!("{k}: {}", if k % 2 == 1 { "True" } else { "False" }))
        .collect();
    format!("{{{}}}", entries.join(", "))
}

/// The zero-arg observable probes (the exports). Each `def <name>() -> int`
/// returns the bool-value sum (an int count of `True`) — the same text is run by
/// live python3.
fn observable_probes() -> Vec<(&'static str, String)> {
    vec![
        // ── mixed / homogeneous dicts ──────────────────────────────────────────
        (
            "sum_mixed",
            "    d: dict[int, bool] = {1: True, 2: False, 3: True}\n    return sum(d.values())\n"
                .to_string(),
        ),
        (
            "sum_all_true",
            "    d: dict[int, bool] = {1: True, 2: True, 3: True}\n    return sum(d.values())\n"
                .to_string(),
        ),
        (
            "sum_all_false",
            "    d: dict[int, bool] = {1: False, 2: False}\n    return sum(d.values())\n".to_string(),
        ),
        // ── single-entry dict ──────────────────────────────────────────────────
        (
            "sum_single_true",
            "    d: dict[int, bool] = {5: True}\n    return sum(d.values())\n".to_string(),
        ),
        (
            "sum_single_false",
            "    d: dict[int, bool] = {5: False}\n    return sum(d.values())\n".to_string(),
        ),
        // ── str-keyed bool dict (the value slot is what sum reads) ──────────────
        (
            "sum_str_key",
            "    d: dict[str, bool] = {\"a\": True, \"b\": False, \"c\": True}\n    return sum(d.values())\n"
                .to_string(),
        ),
        // ── sum AFTER a mutation of the live values ────────────────────────────
        (
            "after_store",
            "    d: dict[int, bool] = {1: True}\n    d[2] = True\n    d[3] = False\n    return sum(d.values())\n"
                .to_string(),
        ),
        (
            "after_overwrite",
            "    d: dict[int, bool] = {1: True, 2: True}\n    d[1] = False\n    return sum(d.values())\n"
                .to_string(),
        ),
        (
            "after_del",
            "    d: dict[int, bool] = {1: True, 2: False, 3: True}\n    del d[3]\n    return sum(d.values())\n"
                .to_string(),
        ),
        // ── an EMPTIED dict → sum == 0 (the empty-list guard) ──────────────────
        (
            "empty_after_del",
            "    d: dict[int, bool] = {1: True}\n    del d[1]\n    return sum(d.values())\n".to_string(),
        ),
        // ── a RELOCATING grow then sum over the moved slots ────────────────────
        (
            "grow_sum",
            format!(
                "    d: dict[int, bool] = {}\n    return sum(d.values())\n",
                grow_pairs()
            ),
        ),
        // ── the sum composed in int arithmetic ─────────────────────────────────
        (
            "arith_compose",
            "    d: dict[int, bool] = {1: True, 2: False, 3: True}\n    return sum(d.values()) * 10 + len(d)\n"
                .to_string(),
        ),
    ]
}

/// The corpus source: the observable exports PLUS a boundary helper (`sum_param`)
/// and its observable caller. The helper takes a param (not an observable itself),
/// so it is appended verbatim and excluded from the differential name list.
fn corpus_source() -> String {
    let mut src = String::new();
    for (name, body) in observable_probes() {
        src.push_str(&format!("def {name}() -> int:\n{body}\n"));
    }
    // Boundary: a bool dict flows across a param and is summed in the callee.
    src.push_str("def sum_param(d: dict[int, bool]) -> int:\n    return sum(d.values())\n");
    src.push_str(
        "def call_param() -> int:\n    d: dict[int, bool] = {1: True, 2: False, 3: True, 4: True}\n    return sum_param(d)\n",
    );
    src
}

/// Every observable export name — the corpus probes plus the boundary caller (NOT
/// `sum_param`, which takes a param).
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
fn dict_bool_values_sum_reuses_shared_helpers() {
    let wat = emit(&corpus_source()).expect("the bool-value sum corpus must lower end-to-end");
    // The sum rides the EXISTING helpers — the values materialiser and the shared
    // int sum fold. NO bespoke bool-sum helper.
    for call in [
        "call $__wasm_dict_values_to_list_i64",
        "call $__wasm_list_sum_i64",
    ] {
        assert!(
            wat.contains(call),
            "bool-value sum must reuse {call}:\n{wat}"
        );
    }
    assert!(
        !wat.contains("$__wasm_dict_boolval_sum"),
        "no bespoke bool-sum helper may exist (routing only):\n{wat}"
    );
    // Both helpers must be DECLARED (the `Map` arm on the gate walker), else
    // wat2wasm rejects the module at the call site.
    assert!(
        wat.contains("(func $__wasm_dict_values_to_list_i64"),
        "the value materialiser must be DECLARED (the Map-arm gate fix):\n{wat}"
    );
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// The forms OUTSIDE the bool-value SUM lane refuse — never silently mis-lowered.
#[test]
fn dict_bool_values_sum_refuse_out_of_lane_forms() {
    for (label, src, needle) in [
        // sum over FLOAT dict values — order-sensitive `+`-fold, still refused.
        (
            "sum(float d.values())",
            "def f() -> float:\n    d: dict[int, float] = {1: 1.5, 2: 2.5}\n    return sum(d.values())\n".to_string(),
            "int-valued dict",
        ),
        // sum over a bare list[bool] NAME — the Map wraps an Ident (not a
        // DictView{Values}); this slice covers only the dict lane, so it stays
        // refused at the non-name-list arm.
        (
            "sum(list[bool])",
            "def f() -> int:\n    xs: list[bool] = [True, False, True]\n    return sum(xs)\n".to_string(),
            "non-name list",
        ),
        // sorted over bool values — a distinct-stride list[bool] helper is deferred.
        (
            "sorted(d.values())",
            "def f() -> int:\n    d: dict[int, bool] = {1: True, 2: False}\n    xs: list[bool] = sorted(d.values())\n    return len(xs)\n".to_string(),
            "sorted",
        ),
    ] {
        let err = match emit(&src) {
            Err(e) => e,
            Ok(wat) => panic!("{label} must be refused but lowered:\n{wat}"),
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-dictboolsum-{}", std::process::id()));
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
fn dict_bool_values_sum_execute_in_wasm_and_match_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("bool sum corpus must lower");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1329: WABT (wat2wasm / wasm-interp) absent — witness asserted at \
             emit level only (the executed leg runs every export and value-matches \
             live python3 on the identical source when WABT is present)."
        );
        return;
    }

    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1329: python3 absent — witness asserted at emit level only");
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
            "bool sum export `{name}`: wasm {got} != cpython {expected}\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1329: {} bool-value sum observables (mixed/homogeneous/single, \
         str-keyed, after store/overwrite/del, an emptied dict, a relocating 20-key \
         grow, an int arith compose, and a param-boundary sum) all == live python3.",
        truth.len()
    );
}
