//! PMAT-1335 — EXECUTED witness for `any(s)` / `all(s)` over a `set[int]` — the SET
//! TWIN of the PMAT-1334 dict-KEYS truthiness reduce (and, transitively, the
//! PMAT-1332 list / PMAT-1333 dict-values reduce). It runs on the bump-heap
//! set/dict runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## What this slice delivers
//!
//! Python iterates a set as its ELEMENTS, so `any(s)` / `all(s)` reduce the
//! elements' truthiness. BEFORE this slice the frontend mis-typed a set arg to
//! `any`/`all` as `I64` (the set skipped the `Type::List` arm — only a dict was
//! materialised) and the whole function was REJECTED at lowering ("body produces
//! I64"), so `any(s)` / `all(s)` was broken on EVERY backend. The frontend now
//! materialises a set arg to `SetToList` (the same view `sum(s)` / `sorted(s)` use,
//! → `List(elem)`), so the per-element truthiness map applies. For an INT set that
//! is a NONZERO map, so the reduce source is:
//!   * `BoolReduce { list: Map { SetToList{set}, __x != 0 } }`   (int set)
//!
//! `emit_bool_reduce` recognises this (`set_truthy_target`), materialises the set's
//! elements into a fresh `list[int]` via the set materialiser
//! `$__wasm_set_to_list_i64` (the SAME helper the dict-keys reduce reuses — a dict
//! IS a set-with-values at `entry+0`), and folds by NONZERO via the SAME
//! `$__wasm_list_int_truthy_reduce` PMAT-1332 uses. NO bespoke helper; NO new
//! meta-HIR variant → NO serial all-codegen edit. The `Map`/`BoolReduce` arms
//! PMAT-1334 already added to `expr_has_set_to_list` arm the materialiser here too
//! (the reduce's `SetToList` sits under `Map` under `BoolReduce`), so NO gate-walker
//! edit is needed either.
//!
//! ## The load-bearing edges
//!
//! Set elements are `int|str`, NEVER float/bool (hashable — no `i64.reinterpret_f64`
//! slot dance the dict-VALUES lane needs for floats). So the fold is ALWAYS the i64
//! nonzero reduce. `any`/`all` COMMUTE, so the reduce is blind to the set's
//! arbitrary bump-heap storage order → CPython-exact even after an `add`/`discard`.
//! The empty-set IDENTITIES `any(set()) == False` / `all(set()) == True` fall out of
//! the helper's identity return. A NEGATIVE element is truthy (nonzero) — pins
//! `!= 0`, not `> 0`.
//!
//! WORKS (value-matched vs python3 on the identical source):
//!   * `any(s)` / `all(s)` over an int set — mixed (a 0 element present),
//!     all-nonzero, the single-0 set, single-nonzero, with negative elements;
//!   * the empty-set IDENTITIES `any(set()) == False`, `all(set()) == True` (via
//!     discard of the last element);
//!   * a reduce AFTER an `add` / `discard` mutates the live element set;
//!   * a RELOCATING grow (20 elements outrun the 16-slot literal slack) then reduce;
//!   * `not any(s)` composed in a boolean context;
//!   * a set flowing across a FUNCTION param, reduced in the callee.
//!
//! REFUSES honestly (NOT silently mis-lowered — regression guards):
//!   * a str SET (`any(s)` over `set[str]`) — the frontend wraps a `len(e) != 0`
//!     map; the recognizer detects that shape and refuses with a PRECISE message (a
//!     per-element str truthiness over materialised i64 pointer slots is deferred),
//!     NOT the generic non-name-list tail;
//!   * the lazy short-circuiting GENERATOR form (`any(x > 0 for x in s)`).
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

/// 20 int elements `1..=20` (all NONZERO) — the `all_grow` probe's source. 20
/// net-new elements OUTRUN the 16-slot literal slack, forcing a real relocation
/// whose moved element slots are then folded → `all` is True.
fn grow_all_elems() -> String {
    (1..=20)
        .map(|k| k.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// 20 elements `-10..=9` (INCLUDING `0`) → `all` is False (the `0` element), `any`
/// is still True (19 nonzero). A relocating grow whose moved slots include zero.
fn grow_haszero_elems() -> String {
    (-10..=9)
        .map(|k| k.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The zero-arg observable probes (the exports). Each `def <name>() -> bool`
/// returns an `any`/`all` over some int set — the same text is run by live python3
/// (its bool coerced to `int` for the differential).
fn observable_probes() -> Vec<(&'static str, String)> {
    vec![
        // ── bare `any(s)` / `all(s)` (Python iterates the ELEMENTS) ───────────
        (
            "any_has_zero",
            "    s: set[int] = {3, 0, 5}\n    return any(s)\n".to_string(),
        ),
        (
            "all_all_nonzero",
            "    s: set[int] = {3, 7, 5}\n    return all(s)\n".to_string(),
        ),
        (
            "all_has_zero",
            "    s: set[int] = {0, 7}\n    return all(s)\n".to_string(),
        ),
        // The single-0 set — `any` and `all` are both False.
        (
            "any_single_zero",
            "    s: set[int] = {0}\n    return any(s)\n".to_string(),
        ),
        (
            "all_single_zero",
            "    s: set[int] = {0}\n    return all(s)\n".to_string(),
        ),
        (
            "any_single_nonzero",
            "    s: set[int] = {5}\n    return any(s)\n".to_string(),
        ),
        // NEGATIVE element is truthy (nonzero) — pins `!= 0`, not `> 0`.
        (
            "any_negative",
            "    s: set[int] = {0, -5}\n    return any(s)\n".to_string(),
        ),
        (
            "all_negatives",
            "    s: set[int] = {-3, -7}\n    return all(s)\n".to_string(),
        ),
        // ── the empty-set IDENTITIES (via discard of the last element) ─────────
        (
            "any_empty_after_discard",
            "    s: set[int] = {5}\n    s.discard(5)\n    return any(s)\n".to_string(),
        ),
        (
            "all_empty_after_discard",
            "    s: set[int] = {5}\n    s.discard(5)\n    return all(s)\n".to_string(),
        ),
        // ── reduce AFTER a mutation of the live element set ────────────────────
        (
            "any_after_add",
            "    s: set[int] = {0}\n    s.add(3)\n    return any(s)\n".to_string(),
        ),
        (
            "all_after_discard_zero",
            "    s: set[int] = {0, 5, 7}\n    s.discard(0)\n    return all(s)\n".to_string(),
        ),
        // ── RELOCATING grows (20 elements outrun the 16-slot slack) ────────────
        (
            "all_grow_nonzero",
            format!(
                "    s: set[int] = {{{}}}\n    return all(s)\n",
                grow_all_elems()
            ),
        ),
        (
            "any_grow_haszero",
            format!(
                "    s: set[int] = {{{}}}\n    return any(s)\n",
                grow_haszero_elems()
            ),
        ),
        (
            "all_grow_haszero",
            format!(
                "    s: set[int] = {{{}}}\n    return all(s)\n",
                grow_haszero_elems()
            ),
        ),
        // ── composed in a boolean context ──────────────────────────────────────
        (
            "not_any_haszero",
            "    s: set[int] = {0, 2}\n    return not any(s)\n".to_string(),
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
    // Boundary: a set flows across a param and its elements are reduced in the callee.
    src.push_str("def reduce_param(s: set[int]) -> bool:\n    return all(s)\n");
    src.push_str(
        "def call_param() -> bool:\n    s: set[int] = {1, 2, 3}\n    return reduce_param(s)\n",
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
fn set_any_all_reuses_shared_helpers() {
    let wat = emit(&corpus_source()).expect("the set any/all corpus must lower end-to-end");
    // The reduce rides the EXISTING helpers — the set materialiser and the shared
    // i64 truthiness fold. NO bespoke helper.
    for call in [
        "call $__wasm_set_to_list_i64",
        "call $__wasm_list_int_truthy_reduce",
    ] {
        assert!(wat.contains(call), "set any/all must reuse {call}:\n{wat}");
    }
    assert!(
        !wat.contains("$__wasm_set_truthy") && !wat.contains("$__wasm_set_any_all"),
        "no bespoke set-reduce helper may exist (routing only):\n{wat}"
    );
    // A set has no VALUE slot to fold — the materialiser reads the element at
    // entry+0, never the dict VALUES materialiser (entry+8).
    assert!(
        !wat.contains("call $__wasm_dict_values_to_list_i64"),
        "an int-set any(s)/all(s) must NOT materialise dict VALUES:\n{wat}"
    );
    // Every reused helper must be DECLARED — the materialiser via the `Map`/
    // `BoolReduce` arms on `expr_has_set_to_list`, the fold via the `BoolReduce`
    // gate.
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

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// The forms OUTSIDE the int-set any/all lane refuse — never silently mis-lowered.
#[test]
fn set_any_all_refuse_out_of_lane_forms() {
    for (label, src, needle) in [
        // a str SET — the frontend wraps a `len(e) != 0` map; the recognizer detects
        // that shape and refuses with a PRECISE message (a per-element str truthiness
        // over materialised i64 pointer slots is deferred).
        (
            "any(str set)",
            "def f() -> bool:\n    s: set[str] = {\"a\", \"\"}\n    return any(s)\n".to_string(),
            "str set",
        ),
        (
            "all(str set)",
            "def f() -> bool:\n    s: set[str] = {\"a\", \"\"}\n    return all(s)\n".to_string(),
            "str set",
        ),
        // the lazy short-circuiting GENERATOR form — a per-element predicate lambda.
        (
            "any(<generator over s>)",
            "def f() -> bool:\n    s: set[int] = {0, 5}\n    return any(x > 0 for x in s)\n"
                .to_string(),
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-setanyall-{}", std::process::id()));
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
fn set_any_all_execute_in_wasm_and_match_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("set any/all corpus must lower");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1335: WABT (wat2wasm / wasm-interp) absent — witness asserted at \
             emit level only (the executed leg runs every export and value-matches \
             live python3 on the identical source when WABT is present)."
        );
        return;
    }

    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1335: python3 absent — witness asserted at emit level only");
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
            "set any/all export `{name}`: wasm {got} != cpython {expected}\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1335: {} set any/all observables (`any(s)`/`all(s)` over int sets — \
         mixed/all-nonzero/single-0/single-nonzero/negatives, the empty-set \
         identities via discard, after add/discard, three relocating 20-element \
         grows, a boolean compose, and a param-boundary reduce) all == live python3.",
        truth.len()
    );
}
