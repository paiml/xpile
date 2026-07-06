//! PMAT-1330 — EXECUTED witness for `len()` of a dict VIEW: `len(d.keys())` /
//! `len(d.values())` / `len(d.items())`. This closes a value/key-kind-AGNOSTIC
//! `len` hole — all three forms previously fell to `emit_len`'s non-name-collection
//! refusal ("len() of a non-name collection") because the collection was a
//! `DictView{..}`, not an `Ident`. It runs on the bump-heap dict runtime
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## What this slice delivers
//!
//! Python guarantees a dict view's length EQUALS the dict's live-entry count for
//! ALL THREE kinds (`keys`/`values`/`items` — a view is a lazy proxy over the
//! dict, never a materialised copy). So `emit_len` gains a `DictView` arm that
//! reads the dict's i32 count header at base+0 — EXACTLY as `len(d)` does
//! (PMAT-995) — and NEVER interprets a key/value slot. Hence it is:
//!   * CPython-EXACT for ANY key/value kind (int/str/bool/float) — the count is
//!     read raw, so a str/bool/float value slot is never touched or misread;
//!   * ORDER-INDEPENDENT — a count carries no ordering, so the dict's arbitrary
//!     storage order is irrelevant;
//!   * ALLOCATION-FREE — a single header load, no materialiser.
//!
//! Because the value/key MATERIALISERS are NOT reached, the Keys/Values gate
//! walkers carve THIS `Len(DictView{..})` shape out (`expr_has_set_to_list` for
//! `Keys`, `expr_has_dict_values_to_list` for `Values`) so no DEAD
//! `$__wasm_set_to_list_i64` / `$__wasm_dict_values_to_list_i64` helper is
//! declared. The `Items` kind arms no materialiser gate at all.
//!
//! Every observable is an INT (a count / a count-derived arithmetic), the same
//! text run by live python3, so `wasm-interp` and CPython compare exactly.
//!
//! WORKS (value-matched vs python3 on the identical source):
//!   * `len(d.values())` / `len(d.keys())` / `len(d.items())` over an int dict;
//!   * over a str-VALUED, str-KEYED, bool-valued, and float-valued dict (the count
//!     header is read regardless of the value/key kind);
//!   * `len` of a view AFTER a store / delete mutates the entry count;
//!   * an EMPTIED dict (del the last entry) → `len == 0`;
//!   * a RELOCATING grow (20 keys outrun the 16-slot slack) then `len(d.items())`;
//!   * `len(d.values())` composed in int arithmetic (`* 10 + len(d.keys())`);
//!   * the three-view INVARIANT `keys == values == items == len(d)` (a summed
//!     identity that is `0` iff all four agree);
//!   * a dict flowing across a FUNCTION param, its view `len`-taken in the callee.
//!
//! REFUSES honestly (NOT silently mis-lowered — a regression guard):
//!   * `len(d.values())` over a dict LITERAL / temporary (a non-name receiver —
//!     the view has no named dict whose header to read).
//!
//! Every probe is FULL-pipeline (REAL Python → `PythonFrontend` → `emit_module` →
//! `wat2wasm` → `wasm-interp`), value-matched against LIVE python3 executing the
//! IDENTICAL source. Gated on `wasm_runtime_available()` — a clean skip (still
//! asserting emit + the refusal) without WABT.

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

/// 20 int pairs (`k` → `k * 10`) — the grow probe's source. 20 net-new keys
/// OUTRUN the 16-slot literal slack, forcing a real relocation; the view `len`
/// then reads the RELOCATED dict's count header (== 20).
fn grow_pairs() -> String {
    let entries: Vec<String> = (1..=20).map(|k| format!("{k}: {}", k * 10)).collect();
    format!("{{{}}}", entries.join(", "))
}

/// The zero-arg observable probes (the exports). Each `def <name>() -> int`
/// returns a count (or a count-derived arithmetic) — the same text is run by live
/// python3.
fn observable_probes() -> Vec<(&'static str, String)> {
    vec![
        // ── the three views over an int dict (all == len(d) == 3) ──────────────
        (
            "values_int",
            "    d: dict[int, int] = {1: 10, 2: 20, 3: 30}\n    return len(d.values())\n".to_string(),
        ),
        (
            "keys_int",
            "    d: dict[int, int] = {1: 10, 2: 20, 3: 30}\n    return len(d.keys())\n".to_string(),
        ),
        (
            "items_int",
            "    d: dict[int, int] = {1: 10, 2: 20, 3: 30}\n    return len(d.items())\n".to_string(),
        ),
        // ── over other value/key kinds (the count header is kind-agnostic) ─────
        (
            "values_str_valued",
            "    d: dict[int, str] = {1: \"a\", 2: \"bb\"}\n    return len(d.values())\n".to_string(),
        ),
        (
            "keys_str_keyed",
            "    d: dict[str, int] = {\"a\": 1, \"b\": 2, \"c\": 3}\n    return len(d.keys())\n"
                .to_string(),
        ),
        (
            "values_bool",
            "    d: dict[int, bool] = {1: True, 2: False, 3: True}\n    return len(d.values())\n"
                .to_string(),
        ),
        (
            "values_float",
            "    d: dict[int, float] = {1: 1.5, 2: 2.5}\n    return len(d.values())\n".to_string(),
        ),
        // ── len of a view AFTER a mutation of the entry count ──────────────────
        (
            "after_store",
            "    d: dict[int, int] = {1: 1}\n    d[2] = 2\n    d[3] = 3\n    return len(d.values())\n"
                .to_string(),
        ),
        (
            "after_del",
            "    d: dict[int, int] = {1: 1, 2: 2, 3: 3}\n    del d[2]\n    return len(d.keys())\n"
                .to_string(),
        ),
        // ── an EMPTIED dict → len == 0 ─────────────────────────────────────────
        (
            "empty_after_del",
            "    d: dict[int, int] = {1: 1}\n    del d[1]\n    return len(d.values())\n".to_string(),
        ),
        // ── a RELOCATING grow then len over the relocated count header ─────────
        (
            "grow_items",
            format!(
                "    d: dict[int, int] = {}\n    return len(d.items())\n",
                grow_pairs()
            ),
        ),
        // ── len composed in int arithmetic ─────────────────────────────────────
        (
            "arith_compose",
            "    d: dict[int, int] = {1: 1, 2: 2, 3: 3}\n    return len(d.values()) * 10 + len(d.keys())\n"
                .to_string(),
        ),
        // ── the three-view INVARIANT: keys == values == items == len(d) → 0 ────
        (
            "views_agree",
            "    d: dict[int, int] = {1: 1, 2: 2, 3: 3, 4: 4}\n    return len(d.values()) + len(d.keys()) + len(d.items()) - 3 * len(d)\n"
                .to_string(),
        ),
    ]
}

/// The corpus source: the observable exports PLUS a boundary helper
/// (`len_param`) and its observable caller. The helper takes a param (not an
/// observable itself), so it is appended verbatim and excluded from the
/// differential name list.
fn corpus_source() -> String {
    let mut src = String::new();
    for (name, body) in observable_probes() {
        src.push_str(&format!("def {name}() -> int:\n{body}\n"));
    }
    // Boundary: a dict flows across a param and its view `len` is taken in the callee.
    src.push_str("def len_param(d: dict[int, int]) -> int:\n    return len(d.values())\n");
    src.push_str(
        "def call_param() -> int:\n    d: dict[int, int] = {1: 1, 2: 2, 3: 3, 4: 4}\n    return len_param(d)\n",
    );
    src
}

/// Every observable export name — the corpus probes plus the boundary caller
/// (NOT `len_param`, which takes a param).
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
fn dict_view_len_declares_no_dead_materialiser() {
    let wat = emit(&corpus_source()).expect("the dict-view len corpus must lower end-to-end");
    // A view `len` reads the dict's COUNT header directly — it never materialises
    // the keys/values. The gate-walker carve-outs (`Len(DictView{Keys})` on
    // `expr_has_set_to_list`, `Len(DictView{Values})` on
    // `expr_has_dict_values_to_list`) must therefore leave NEITHER materialiser
    // declared in a len-ONLY corpus — else a DEAD helper bloats the module.
    for helper in ["$__wasm_set_to_list_i64", "$__wasm_dict_values_to_list_i64"] {
        assert!(
            !wat.contains(helper),
            "a len-only dict-view corpus must declare NO {helper} (dead materialiser):\n{wat}"
        );
    }
    // The view `len` reads the count header with an `i32.load` then widens it —
    // the same shape `len(d)` emits (PMAT-995).
    assert!(
        wat.contains("i64.extend_i32_u"),
        "the view len must widen the i32 count header to i64:\n{wat}"
    );
}

// ---- honest refusal (through the FULL pipeline) ------------------------------

/// `len(d.values())` over a dict LITERAL (a non-name receiver) refuses — the view
/// has no named dict whose header to read; never silently mis-lowered.
#[test]
fn dict_view_len_refuses_non_name_receiver() {
    let src = "def f() -> int:\n    return len({1: 10, 2: 20}.values())\n";
    let err = match emit(src) {
        Err(e) => e,
        Ok(wat) => panic!("len over a dict-literal view must be refused but lowered:\n{wat}"),
    };
    assert!(
        err.contains("non-name dict"),
        "the refusal should mention a non-name dict, got: {err}"
    );
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-dictviewlen-{}", std::process::id()));
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
fn dict_view_len_execute_in_wasm_and_match_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("dict-view len corpus must lower");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1330: WABT (wat2wasm / wasm-interp) absent — witness asserted at \
             emit level only (the executed leg runs every export and value-matches \
             live python3 on the identical source when WABT is present)."
        );
        return;
    }

    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1330: python3 absent — witness asserted at emit level only");
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
            "dict-view len export `{name}`: wasm {got} != cpython {expected}\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1330: {} dict-view len observables (keys/values/items over \
         int/str-valued/str-keyed/bool/float dicts, after store/del, an emptied \
         dict, a relocating 20-key grow, an int arith compose, the three-view \
         invariant, and a param-boundary len) all == live python3.",
        truth.len()
    );
}
