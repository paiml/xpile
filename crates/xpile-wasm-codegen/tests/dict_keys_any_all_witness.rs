//! PMAT-1334 — EXECUTED witness for `any(d)` / `all(d)` / `any(d.keys())` /
//! `all(d.keys())` over an int-KEYED dict — the KEY-view TWIN of the PMAT-1333
//! `.values()` truthiness reduce. This CLOSES the follow-up the PMAT-1333 docstring
//! named: a dict-KEYS `any`/`all` fell to the non-name-list refusal because the
//! reduce source is a `Map` over a `DictView{Keys}`, never a bare list `Ident`. It
//! runs on the bump-heap dict runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## What this slice delivers
//!
//! Python iterates a dict as its KEYS, so `any(d)` / `all(d)` reduce the KEYS'
//! truthiness — identical to `any(d.keys())` / `all(d.keys())`. The frontend
//! materialises the dict arg to `DictView{Keys}` (the same view `sum(d)`/`max(d)`/
//! `list(d)` use) and applies Python's per-element truthiness. For an INT-keyed
//! dict that is a NONZERO map, so the reduce source is:
//!   * `BoolReduce { list: Map { DictView{Keys}, __x != 0 } }`   (int-keyed dict)
//!
//! `emit_bool_reduce` recognises this (`dict_keys_truthy_dict`), materialises the
//! KEY slots into a fresh `list[int]` via the SHARED set-layout key materialiser
//! `$__wasm_set_to_list_i64` (a dict shares the set's open-assoc region, so the
//! materialiser reads the key at `entry+0` verbatim — duplicates cannot exist among
//! keys, storage order is irrelevant to a truthiness fold), and folds by NONZERO
//! via the SAME `$__wasm_list_int_truthy_reduce` PMAT-1332 uses. NO bespoke helper;
//! NO new meta-HIR variant → NO serial all-codegen edit. The `Map`/`BoolReduce`
//! arms added to `expr_has_set_to_list` arm the key materialiser (else it would be
//! undeclared at its call site — the recurring gate-hole class).
//!
//! ## The load-bearing edges
//!
//! Dict KEYS are `int|str`, NEVER float/bool (keys are hashable — no reinterpret
//! slot dance the VALUES lane needs for floats). So the fold is ALWAYS the i64
//! nonzero reduce. Keys are also UNIQUE, so an "all zero" dict is exactly `{0: v}`
//! (one key). `any({0: v}) == False`, `all({0: v}) == False`; the empty-dict
//! IDENTITIES `any({}) == False` / `all({}) == True` fall out of the helper's
//! identity return. A NEGATIVE key is truthy (nonzero) — pins `!= 0`, not `> 0`.
//!
//! WORKS (value-matched vs python3 on the identical source):
//!   * `any(d)` / `all(d)` over an int-keyed dict — mixed (a 0 key present),
//!     all-nonzero, the single-0-key dict, single-nonzero, with negative keys;
//!   * the EXPLICIT `any(d.keys())` / `all(d.keys())` view (a pure alias of the
//!     bare form — same `DictView{Keys}`);
//!   * the empty-dict IDENTITIES `any({}) == False`, `all({}) == True` (via del);
//!   * a reduce AFTER a store / delete mutates the live key set;
//!   * a RELOCATING grow (20 keys outrun the 16-slot literal slack) then reduce;
//!   * `not any(d)` composed in a boolean context;
//!   * a dict flowing across a FUNCTION param, reduced in the callee.
//!
//! REFUSES honestly (NOT silently mis-lowered — regression guards):
//!   * a VALUES view (`any(d.values())`) stays in the PMAT-1333 lane (this witness
//!     only pins the KEYS twin);
//!   * the lazy short-circuiting GENERATOR form (`any(k > 0 for k in d)`).
//!
//! (A str-KEYED dict `any(d)` / `all(d.keys())` was refused here at PMAT-1334;
//! PMAT-1336 turns it into a WORKING `len(k) != 0` fold — see
//! `str_truthy_reduce_witness.rs`.)
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

/// 20 int keys (`k` → `k`, all NONZERO) — the `all_grow` probe's source. 20 net-new
/// keys OUTRUN the 16-slot literal slack, forcing a real relocation whose moved KEY
/// slots are then folded → `all` is True.
fn grow_all_pairs() -> String {
    (1..=20)
        .map(|k| format!("{k}: {k}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// 20 keys `-10..=9` (INCLUDING `0`) → `all` is False (the `0` key), `any` is still
/// True (19 nonzero). A relocating grow whose moved slots include the zero key.
fn grow_haszero_pairs() -> String {
    (-10..=9)
        .map(|k| format!("{k}: 1"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The zero-arg observable probes (the exports). Each `def <name>() -> bool`
/// returns an `any`/`all` over some int-keyed dict's KEYS — the same text is run by
/// live python3 (its bool coerced to `int` for the differential).
fn observable_probes() -> Vec<(&'static str, String)> {
    vec![
        // ── bare `any(d)` / `all(d)` (Python iterates the KEYS) ───────────────
        (
            "any_has_zero_key",
            "    d: dict[int, int] = {3: 10, 0: 20, 5: 30}\n    return any(d)\n".to_string(),
        ),
        (
            "all_all_nonzero",
            "    d: dict[int, int] = {3: 10, 7: 20, 5: 30}\n    return all(d)\n".to_string(),
        ),
        (
            "all_has_zero_key",
            "    d: dict[int, int] = {0: 10, 7: 20}\n    return all(d)\n".to_string(),
        ),
        // The single-0-key dict — the only "all keys zero" a dict can hold (keys
        // are unique). `any` and `all` are both False.
        (
            "any_single_zero",
            "    d: dict[int, int] = {0: 99}\n    return any(d)\n".to_string(),
        ),
        (
            "all_single_zero",
            "    d: dict[int, int] = {0: 99}\n    return all(d)\n".to_string(),
        ),
        (
            "any_single_nonzero",
            "    d: dict[int, int] = {5: 99}\n    return any(d)\n".to_string(),
        ),
        // NEGATIVE key is truthy (nonzero) — pins `!= 0`, not `> 0`.
        (
            "any_negative_key",
            "    d: dict[int, int] = {0: 1, -5: 2}\n    return any(d)\n".to_string(),
        ),
        (
            "all_negative_keys",
            "    d: dict[int, int] = {-3: 1, -7: 2}\n    return all(d)\n".to_string(),
        ),
        // ── the EXPLICIT `d.keys()` view (a pure alias of the bare form) ───────
        (
            "any_keys_view",
            "    d: dict[int, int] = {3: 1, 0: 2, 9: 3}\n    return any(d.keys())\n".to_string(),
        ),
        (
            "all_keys_view",
            "    d: dict[int, int] = {3: 1, 7: 2, 9: 3}\n    return all(d.keys())\n".to_string(),
        ),
        // ── the empty-dict IDENTITIES (via del of the last entry) ──────────────
        (
            "any_empty_after_del",
            "    d: dict[int, int] = {1: 5}\n    del d[1]\n    return any(d)\n".to_string(),
        ),
        (
            "all_empty_after_del",
            "    d: dict[int, int] = {1: 5}\n    del d[1]\n    return all(d)\n".to_string(),
        ),
        // ── reduce AFTER a mutation of the live key set ────────────────────────
        (
            "any_after_store",
            "    d: dict[int, int] = {0: 1}\n    d[3] = 9\n    return any(d)\n".to_string(),
        ),
        (
            "all_after_del_zero",
            "    d: dict[int, int] = {0: 1, 5: 2, 7: 3}\n    del d[0]\n    return all(d)\n"
                .to_string(),
        ),
        // ── RELOCATING grows (20 keys outrun the 16-slot slack) ────────────────
        (
            "all_grow_nonzero",
            format!(
                "    d: dict[int, int] = {{{}}}\n    return all(d)\n",
                grow_all_pairs()
            ),
        ),
        (
            "any_grow_haszero",
            format!(
                "    d: dict[int, int] = {{{}}}\n    return any(d)\n",
                grow_haszero_pairs()
            ),
        ),
        (
            "all_grow_haszero",
            format!(
                "    d: dict[int, int] = {{{}}}\n    return all(d)\n",
                grow_haszero_pairs()
            ),
        ),
        // ── composed in a boolean context ──────────────────────────────────────
        (
            "not_any_haszero",
            "    d: dict[int, int] = {0: 1, 2: 2}\n    return not any(d)\n".to_string(),
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
    // Boundary: a dict flows across a param and its keys are reduced in the callee.
    src.push_str("def reduce_param(d: dict[int, int]) -> bool:\n    return all(d)\n");
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
fn dict_keys_any_all_reuses_shared_helpers() {
    let wat = emit(&corpus_source()).expect("the dict-keys any/all corpus must lower end-to-end");
    // The reduce rides the EXISTING helpers — the set-layout KEY materialiser and
    // the shared i64 truthiness fold. NO bespoke helper, NO value materialiser.
    for call in [
        "call $__wasm_set_to_list_i64",
        "call $__wasm_list_int_truthy_reduce",
    ] {
        assert!(
            wat.contains(call),
            "dict-keys any/all must reuse {call}:\n{wat}"
        );
    }
    assert!(
        !wat.contains("$__wasm_dict_keys_truthy") && !wat.contains("$__wasm_dict_any_all"),
        "no bespoke dict-key-reduce helper may exist (routing only):\n{wat}"
    );
    // The KEY fold must NOT route through the VALUES materialiser (it reads entry+0,
    // not entry+8).
    assert!(
        !wat.contains("call $__wasm_dict_values_to_list_i64"),
        "an int-keyed any(d)/all(d) must NOT materialise the VALUES:\n{wat}"
    );
    // Every reused helper must be DECLARED — the key materialiser via the
    // `Map`/`BoolReduce` arms on `expr_has_set_to_list`, the fold via the
    // `BoolReduce` gate.
    for decl in [
        "(func $__wasm_set_to_list_i64",
        "(func $__wasm_list_int_truthy_reduce",
    ] {
        assert!(
            wat.contains(decl),
            "helper {decl} must be DECLARED (the gate walkers), else wat2wasm rejects the module:\n{wat}"
        );
    }
}

/// `any(d)` and `any(d.keys())` over the SAME int-keyed dict emit identical reduce
/// wiring — the explicit view is a pure alias of the bare iteration.
#[test]
fn dict_keys_any_all_bare_and_view_are_aliases() {
    let bare = emit("def f() -> bool:\n    d: dict[int, int] = {3: 1, 0: 2}\n    return any(d)\n")
        .expect("bare any(d) must lower");
    let view =
        emit("def f() -> bool:\n    d: dict[int, int] = {3: 1, 0: 2}\n    return any(d.keys())\n")
            .expect("any(d.keys()) must lower");
    for call in [
        "call $__wasm_set_to_list_i64",
        "call $__wasm_list_int_truthy_reduce",
    ] {
        assert!(
            bare.contains(call) && view.contains(call),
            "both forms reuse {call}"
        );
    }
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// The forms OUTSIDE the int-KEYS any/all lane refuse — never silently
/// mis-lowered. (The str-KEYED dict `any(d)`/`all(d.keys())` moved from a refusal
/// to a WORKING `len(k) != 0` fold in PMAT-1336 — see
/// `str_truthy_reduce_witness.rs`; it is no longer a refusal.)
#[test]
fn dict_keys_any_all_refuse_out_of_lane_forms() {
    // The lazy short-circuiting GENERATOR form — a per-element predicate lambda.
    let label = "any(<generator over d>)";
    let src = "def f() -> bool:\n    d: dict[int, int] = {0: 1, 5: 2}\n    return any(k > 0 for k in d)\n";
    let err = match emit(src) {
        Err(e) => e,
        Ok(wat) => panic!("{label} must be refused but lowered:\n{wat}"),
    };
    assert!(
        err.contains("<generator>"),
        "{label} refusal should mention \"<generator>\", got: {err}"
    );
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-dictkeyanyall-{}", std::process::id()));
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
fn dict_keys_any_all_execute_in_wasm_and_match_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("dict-keys any/all corpus must lower");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1334: WABT (wat2wasm / wasm-interp) absent — witness asserted at \
             emit level only (the executed leg runs every export and value-matches \
             live python3 on the identical source when WABT is present)."
        );
        return;
    }

    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1334: python3 absent — witness asserted at emit level only");
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
            "dict-keys any/all export `{name}`: wasm {got} != cpython {expected}\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1334: {} dict-keys any/all observables (bare `any(d)`/`all(d)` and the \
         explicit `.keys()` view over int-keyed dicts — mixed/all-nonzero/single-0/\
         single-nonzero/negatives, the empty-dict identities, after store/del, three \
         relocating 20-key grows, a boolean compose, and a param-boundary reduce) all \
         == live python3.",
        truth.len()
    );
}
