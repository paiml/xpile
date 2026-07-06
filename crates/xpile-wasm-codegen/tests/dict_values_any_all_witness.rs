//! PMAT-1333 — EXECUTED witness for `any(d.values())` / `all(d.values())` over a
//! bool/int/float-VALUED dict — the dict-view TWIN of the PMAT-1332 list
//! truthiness reduce. This CLOSES the follow-up the PMAT-1332 docstring named: a
//! dict-sourced `any`/`all` fell to the non-name-list refusal because the reduce
//! source is a `DictView{Values}` (bool) or a `Map` over one (int/float), never a
//! bare list `Ident`. It runs on the bump-heap dict runtime
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## What this slice delivers
//!
//! The frontend lowers `d.values()` to `DictView{Values}` and applies Python's
//! PER-ELEMENT truthiness: a `bool` value straight through (its stored 0/1 slot
//! IS its truthiness), an `int` via `__x != 0`, a `float` via `__x != 0.0`. So
//! the reduce source is one of:
//!   * `BoolReduce { list: DictView{Values} }`                       (bool dict)
//!   * `BoolReduce { list: Map { DictView{Values}, __x != 0 } }`     (int dict)
//!   * `BoolReduce { list: Map { DictView{Values}, __x != 0.0 } }`   (float dict)
//!
//! `emit_bool_reduce` recognises these (`dict_values_truthy_dict`), materialises
//! the value slots into a fresh `list[int]` via the SHARED
//! `$__wasm_dict_values_to_list_i64` (duplicates kept, storage order irrelevant to
//! a truthiness fold), and folds by NONZERO via the SAME PMAT-1332 helpers. NO
//! bespoke dict-reduce helper; NO new meta-HIR variant → NO serial all-codegen
//! edit. The `BoolReduce` arm added to `expr_has_dict_values_to_list` arms the
//! materialiser (else it would be undeclared at its call site — the recurring
//! gate-hole class).
//!
//! ## The load-bearing correctness edge: the FOLD helper follows the VALUE KIND
//!
//! The materialiser copies each i64 value slot VERBATIM. A bool/int value is a
//! plain i64, folded by the i64 helper (`i64.ne 0`). A FLOAT value's i64 slot
//! holds `i64.reinterpret_f64` BITS, so it MUST fold by the f64 helper (`f64.ne
//! 0.0`) to honour IEEE truthiness: `bool(-0.0) == False` (`-0.0 != 0.0` is false)
//! and `bool(NaN) == True` (`NaN != 0.0` is true). A raw i64 `!= 0` on `-0.0`'s
//! bits (`0x8000_0000_0000_0000`) would WRONGLY read truthy — so `any_float_negzero`
//! ({−0.0, 0.0}) == False is the witness that pins the kind-driven helper split.
//!
//! WORKS (value-matched vs python3 on the identical source):
//!   * `any`/`all` over a bool / int / float dict — mixed, all-true, all-false,
//!     single-entry, with negatives and large values (int nonzero ≠ ==1);
//!   * the empty-dict IDENTITIES `any({}) == False`, `all({}) == True` (via del);
//!   * a reduce AFTER a store / overwrite / delete mutates the live values;
//!   * a str-KEYED dict (the VALUE slot is what the fold reads);
//!   * a RELOCATING grow (20 keys outrun the 16-slot literal slack) then reduce;
//!   * the IEEE float edge `any({−0.0, 0.0}) == False`;
//!   * `not any(d.values())` composed in a boolean context;
//!   * a dict flowing across a FUNCTION param, reduced in the callee.
//!
//! REFUSES honestly (NOT silently mis-lowered — regression guards):
//!   * a str-VALUED dict (`any(d.values())` over `dict[_, str]`) — the frontend
//!     wraps a `len(v) != 0` map the recognizer does not match, so it falls to the
//!     non-name-list refusal (a per-element str payload fold is deferred);
//!   * a KEYS view (`any(d.keys())`) — a `DictView{Keys}`-sourced map, out of this
//!     slice's VALUES lane (a keys materialisation + fold is a separate follow-up);
//!   * the lazy short-circuiting GENERATOR form (`any(P(v) for v in d.values())`).
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

/// 20 int-valued pairs (`k` → `k`, all NONZERO) — the `all_grow` probe's source.
/// 20 net-new keys OUTRUN the 16-slot literal slack, forcing a real relocation
/// whose moved value slots are then folded → `all` is True.
fn grow_all_pairs() -> String {
    (1..=20)
        .map(|k| format!("{k}: {k}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// 20 int-valued pairs (`k` → `k % 3`, so 6 of them are ZERO) — the `any_grow`
/// probe's source. A relocating grow whose moved slots include zeros → `any` is
/// still True (14 nonzero), `all` would be False.
fn grow_haszero_pairs() -> String {
    (1..=20)
        .map(|k| format!("{k}: {}", k % 3))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The zero-arg observable probes (the exports). Each `def <name>() -> bool`
/// returns an `any`/`all` over some dict's values — the same text is run by live
/// python3 (its bool coerced to `int` for the differential).
fn observable_probes() -> Vec<(&'static str, String)> {
    vec![
        // ── bool-valued dicts (the 0/1 slot IS the truthiness) ────────────────
        (
            "any_bool_mixed",
            "    d: dict[int, bool] = {1: False, 2: True, 3: False}\n    return any(d.values())\n"
                .to_string(),
        ),
        (
            "all_bool_mixed",
            "    d: dict[int, bool] = {1: True, 2: False}\n    return all(d.values())\n".to_string(),
        ),
        (
            "any_bool_all_false",
            "    d: dict[int, bool] = {1: False, 2: False}\n    return any(d.values())\n".to_string(),
        ),
        (
            "all_bool_all_true",
            "    d: dict[int, bool] = {1: True, 2: True, 3: True}\n    return all(d.values())\n"
                .to_string(),
        ),
        (
            "any_bool_single_true",
            "    d: dict[int, bool] = {5: True}\n    return any(d.values())\n".to_string(),
        ),
        (
            "all_bool_single_false",
            "    d: dict[int, bool] = {5: False}\n    return all(d.values())\n".to_string(),
        ),
        // ── int-valued dicts (per-element NONZERO truthiness) ──────────────────
        (
            "any_int_has_nonzero",
            "    d: dict[int, int] = {1: 0, 2: 0, 3: 7}\n    return any(d.values())\n".to_string(),
        ),
        (
            "all_int_has_zero",
            "    d: dict[int, int] = {1: 5, 2: 0, 3: 9}\n    return all(d.values())\n".to_string(),
        ),
        // NEGATIVE is truthy (nonzero) — pins `!= 0`, not `> 0`.
        (
            "any_int_negative",
            "    d: dict[int, int] = {1: 0, 2: -5}\n    return any(d.values())\n".to_string(),
        ),
        (
            "all_int_negatives",
            "    d: dict[int, int] = {1: -3, 2: -7}\n    return all(d.values())\n".to_string(),
        ),
        (
            "any_int_all_zero",
            "    d: dict[int, int] = {1: 0, 2: 0}\n    return any(d.values())\n".to_string(),
        ),
        (
            "all_int_all_zero",
            "    d: dict[int, int] = {1: 0, 2: 0}\n    return all(d.values())\n".to_string(),
        ),
        // ── float-valued dicts (IEEE truthiness) ───────────────────────────────
        // THE LOAD-BEARING EDGE: `-0.0` is FALSY, so `any({−0.0, 0.0}) == False`.
        // A raw i64 `!= 0` over the reinterpreted bits would WRONGLY read truthy.
        (
            "any_float_negzero",
            "    d: dict[int, float] = {1: -0.0, 2: 0.0}\n    return any(d.values())\n".to_string(),
        ),
        (
            "all_float_haszero",
            "    d: dict[int, float] = {1: 1.5, 2: 0.0}\n    return all(d.values())\n".to_string(),
        ),
        (
            "any_float_nonzero",
            "    d: dict[int, float] = {1: 0.0, 2: -2.5}\n    return any(d.values())\n".to_string(),
        ),
        (
            "all_float_nonzero",
            "    d: dict[int, float] = {1: 1.5, 2: -2.5, 3: 99.9}\n    return all(d.values())\n"
                .to_string(),
        ),
        // ── the empty-dict IDENTITIES (via del of the last entry) ──────────────
        (
            "any_empty_after_del",
            "    d: dict[int, int] = {1: 5}\n    del d[1]\n    return any(d.values())\n".to_string(),
        ),
        (
            "all_empty_after_del",
            "    d: dict[int, int] = {1: 5}\n    del d[1]\n    return all(d.values())\n".to_string(),
        ),
        // ── reduce AFTER a mutation of the live values ─────────────────────────
        (
            "any_after_store",
            "    d: dict[int, int] = {1: 0}\n    d[2] = 3\n    return any(d.values())\n".to_string(),
        ),
        (
            "all_after_overwrite",
            "    d: dict[int, int] = {1: 5, 2: 9}\n    d[1] = 0\n    return all(d.values())\n"
                .to_string(),
        ),
        (
            "all_after_del",
            "    d: dict[int, int] = {1: 5, 2: 0, 3: 7}\n    del d[2]\n    return all(d.values())\n"
                .to_string(),
        ),
        // ── str-KEYED dict (the VALUE slot is what the fold reads) ─────────────
        (
            "any_str_key_bool",
            "    d: dict[str, bool] = {\"a\": False, \"b\": True}\n    return any(d.values())\n"
                .to_string(),
        ),
        // ── RELOCATING grows (20 keys outrun the 16-slot slack) ────────────────
        (
            "all_grow_nonzero",
            format!(
                "    d: dict[int, int] = {{{}}}\n    return all(d.values())\n",
                grow_all_pairs()
            ),
        ),
        (
            "any_grow_haszero",
            format!(
                "    d: dict[int, int] = {{{}}}\n    return any(d.values())\n",
                grow_haszero_pairs()
            ),
        ),
        // ── composed in a boolean context ──────────────────────────────────────
        (
            "not_any_bool",
            "    d: dict[int, bool] = {1: True, 2: False}\n    return not any(d.values())\n"
                .to_string(),
        ),
    ]
}

/// The corpus source: the observable exports PLUS a boundary helper
/// (`reduce_param`) and its observable caller. The helper takes a param (not an
/// observable itself), so it is appended verbatim and excluded from the
/// differential name list.
fn corpus_source() -> String {
    let mut src = String::new();
    for (name, body) in observable_probes() {
        src.push_str(&format!("def {name}() -> bool:\n{body}\n"));
    }
    // Boundary: a dict flows across a param and is reduced in the callee.
    src.push_str("def reduce_param(d: dict[int, int]) -> bool:\n    return all(d.values())\n");
    src.push_str(
        "def call_param() -> bool:\n    d: dict[int, int] = {1: 5, 2: 3, 3: 8}\n    return reduce_param(d)\n",
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
fn dict_values_any_all_reuses_shared_helpers() {
    let wat = emit(&corpus_source()).expect("the dict-values any/all corpus must lower end-to-end");
    // The reduce rides the EXISTING helpers — the values materialiser and the two
    // shared truthiness folds (i64 for bool/int, f64 for float). NO bespoke helper.
    for call in [
        "call $__wasm_dict_values_to_list_i64",
        "call $__wasm_list_int_truthy_reduce",
        "call $__wasm_list_float_truthy_reduce",
    ] {
        assert!(
            wat.contains(call),
            "dict-values any/all must reuse {call}:\n{wat}"
        );
    }
    assert!(
        !wat.contains("$__wasm_dict_values_truthy") && !wat.contains("$__wasm_dict_any_all"),
        "no bespoke dict-reduce helper may exist (routing only):\n{wat}"
    );
    // Every reused helper must be DECLARED — the materialiser via the `BoolReduce`
    // arm on `expr_has_dict_values_to_list`, the folds via the `BoolReduce` gate.
    for decl in [
        "(func $__wasm_dict_values_to_list_i64",
        "(func $__wasm_list_int_truthy_reduce",
        "(func $__wasm_list_float_truthy_reduce",
    ] {
        assert!(
            wat.contains(decl),
            "helper {decl} must be DECLARED (the gate walkers), else wat2wasm rejects the module:\n{wat}"
        );
    }
}

/// A FLOAT-valued dict reduce must route to the f64 fold (IEEE truthiness), a
/// bool/int one to the i64 fold — the kind-driven helper split, not the map's
/// literal.
#[test]
fn dict_values_any_all_float_routes_to_f64_helper() {
    let float_wat = emit(
        "def f() -> bool:\n    d: dict[int, float] = {1: -0.0, 2: 0.0}\n    return any(d.values())\n",
    )
    .expect("float dict any must lower");
    assert!(
        float_wat.contains("call $__wasm_list_float_truthy_reduce"),
        "a float-valued dict any/all must fold via the f64 helper (IEEE truthiness):\n{float_wat}"
    );
    assert!(
        !float_wat.contains("call $__wasm_list_int_truthy_reduce"),
        "a float-valued dict any/all must NOT fold via the i64 helper (would misread -0.0):\n{float_wat}"
    );

    let int_wat = emit(
        "def f() -> bool:\n    d: dict[int, int] = {1: 0, 2: 7}\n    return any(d.values())\n",
    )
    .expect("int dict any must lower");
    assert!(
        int_wat.contains("call $__wasm_list_int_truthy_reduce")
            && !int_wat.contains("call $__wasm_list_float_truthy_reduce"),
        "an int-valued dict any/all must fold via the i64 helper only:\n{int_wat}"
    );
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// The forms OUTSIDE the dict-VALUES any/all lane refuse — never silently
/// mis-lowered.
#[test]
fn dict_values_any_all_refuse_out_of_lane_forms() {
    for (label, src, needle) in [
        // a str-VALUED dict — the frontend wraps a `len(v) != 0` map the
        // recognizer does not match, so it falls to the non-name-list refusal (a
        // per-element str payload fold is deferred).
        (
            "any(str d.values())",
            "def f() -> bool:\n    d: dict[int, str] = {1: \"a\", 2: \"\"}\n    return any(d.values())\n".to_string(),
            "non-name list",
        ),
        // a KEYS view — a `DictView{Keys}`-sourced map, out of this VALUES lane.
        (
            "any(d.keys())",
            "def f() -> bool:\n    d: dict[int, int] = {1: 0, 2: 5}\n    return any(d.keys())\n".to_string(),
            "non-name list",
        ),
        // the lazy short-circuiting GENERATOR form — a per-element predicate lambda.
        (
            "any(<generator over d.values()>)",
            "def f() -> bool:\n    d: dict[int, int] = {1: 0, 2: 5}\n    return any(v > 0 for v in d.values())\n".to_string(),
            "<generator>",
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

/// Parse a `name() => <ty>:<v>` line. Every observable here returns a `bool`,
/// printed by `wasm-interp` as `i32:0` / `i32:1`.
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-dictvalanyall-{}", std::process::id()));
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
/// pairs for the observable exports — the differential ground truth. Each bool is
/// coerced to `int` (0/1) to match `wasm-interp`'s i32 printing.
fn python_truth(src: &str) -> Option<Vec<(String, i64)>> {
    let names = observable_names();
    let driver =
        format!("{src}\nprint(';'.join(f'{{n}}={{int(globals()[n]())}}' for n in {names:?}))\n");
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
fn dict_values_any_all_execute_in_wasm_and_match_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("dict-values any/all corpus must lower");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1333: WABT (wat2wasm / wasm-interp) absent — witness asserted at \
             emit level only (the executed leg runs every export and value-matches \
             live python3 on the identical source when WABT is present)."
        );
        return;
    }

    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1333: python3 absent — witness asserted at emit level only");
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
            "dict-values any/all export `{name}`: wasm {got} != cpython {expected}\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1333: {} dict-values any/all observables (bool/int/float dicts — \
         mixed/homogeneous/single/negatives, the empty-dict identities, after \
         store/overwrite/del, a str-keyed dict, two relocating 20-key grows, the \
         IEEE `any({{−0.0, 0.0}}) == False` edge, a boolean compose, and a \
         param-boundary reduce) all == live python3.",
        truth.len()
    );
}
