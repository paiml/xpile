//! PMAT-1336 — EXECUTED witness for the STRING truthiness reduce: `any(s)` /
//! `all(s)` over a `set[str]` and `any(d)` / `all(d)` / `all(d.keys())` over a
//! str-KEYED dict. It runs on the bump-heap set/dict + length-prefixed str runtime
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## What this slice delivers
//!
//! Python iterates a set as its ELEMENTS and a dict as its KEYS, so `any`/`all`
//! reduce those STRINGS' truthiness — and a `str` is True iff `len(s) != 0`. The
//! frontend lowers the reduce to `BoolReduce { Map { <view>, len(__x) != 0 } }`
//! over a `SetToList{set}` (set) or `DictView{Keys}` (dict). BEFORE this slice the
//! backend REFUSED that str shape honestly ("a per-element str truthiness over
//! materialised i64 pointer slots is not in the WASM subset yet") — the int
//! materialisers (`$__wasm_set_to_list_i64` / `emit_dict_keys_to_list`) are I64-only:
//! a str set/dict would need a `list[str]`, which the WASM list subset does not
//! model.
//!
//! PMAT-1336 folds the str KEYS DIRECTLY out of the open-assoc region instead of
//! materialising a list. A set and a dict share the SAME layout (16-byte entries: a
//! KEY @ `entry+0`, `i32` live-count @ `base+0`, entries @ `base+8`), and a str KEY
//! is stored as an `i32` POINTER at `entry+0`; the length-prefixed str ABI puts the
//! byte count as an `i32` header @ that pointer. So the new fused helper
//! `$__wasm_hash_strkey_truthy_reduce(base, is_all)` walks entries `[0, n)`, reads
//! each str pointer at `entry+0`, loads its length header, and folds `len != 0`.
//! The set local / dict local IS the region base, so BOTH forms pass it straight in —
//! ONE helper, no bespoke set-vs-dict split (a dict IS a set-with-values at
//! `entry+0`). Non-allocating (a pointer-slot fold), so it co-emits under the SAME
//! `needs_list_bool_reduce` gate as the scalar truthy folds; NO new meta-HIR variant
//! → NO serial all-codegen edit.
//!
//! ## The load-bearing edges
//!
//! `len(s)` is a CHAR count in Python but the str ABI header is a BYTE count — for
//! TRUTHINESS this is exact: a string is empty iff 0 chars iff 0 bytes, so
//! `bytes != 0` ⟺ `chars != 0` ⟺ truthy. The EMPTY STRING `""` is a valid
//! element/key (header 0 → falsey), so `any({""}) == False` and `all({"", "x"}) ==
//! False` land exactly. `any`/`all` COMMUTE, so the fold is blind to the arbitrary
//! bump-heap storage order → CPython-exact even after `add`/`discard`/`del`. The
//! empty-region IDENTITIES `any(∅) == False` / `all(∅) == True` fall out of the
//! helper's identity return.
//!
//! WORKS (value-matched vs python3 on the identical source):
//!   * `any(s)` / `all(s)` over a `set[str]` — mixed (an `""` present), all-nonempty,
//!     the single-`""` set, single-nonempty;
//!   * `any(d)` / `all(d)` / `all(d.keys())` over a str-keyed dict — mixed, all
//!     nonempty keys, the explicit `.keys()` view;
//!   * the empty-region IDENTITIES `any == False` / `all == True` (set via `discard`,
//!     dict via `del`);
//!   * a reduce AFTER an `add` / `discard` / `del` mutates the live key set;
//!   * a RELOCATING grow (20 keys outrun the 16-slot literal slack) then reduce,
//!     including a grow whose moved slots contain the empty string;
//!   * `not any(s)` composed in a boolean context;
//!   * a `set[str]` / `dict[str, int]` flowing across a FUNCTION param, reduced in
//!     the callee.
//!
//! REFUSES honestly (NOT silently mis-lowered — regression guards):
//!   * a str-VALUED dict `any(d.values())` (`dict[_, str]`) — a per-element str
//!     truthiness over the VALUE slot (`entry+8`) is NOT in this slice (the keys twin
//!     only); still refused with its PMAT-1333 message;
//!   * the lazy short-circuiting GENERATOR form (`any(len(x) > 0 for x in s)`).
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

/// 20 distinct NON-empty str keys `"s1".."s20"` — the all-nonempty grow's source. 20
/// net-new keys OUTRUN the 16-slot literal slack, forcing a real relocation whose
/// moved key slots are then folded → `all` is True.
fn grow_all_nonempty() -> String {
    (1..=20)
        .map(|k| format!("\"s{k}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// 20 str keys INCLUDING the empty string `""` → `all` is False (the `""` key), `any`
/// is still True (19 nonempty). A relocating grow whose moved slots include `""`.
fn grow_has_empty() -> String {
    let mut v: Vec<String> = (1..=19).map(|k| format!("\"s{k}\"")).collect();
    v.push("\"\"".to_string());
    v.join(", ")
}

/// The zero-arg observable probes (the exports). Each `def <name>() -> bool` returns
/// an `any`/`all` over some `set[str]` or str-keyed dict — the same text is run by
/// live python3 (its bool coerced to `int` for the differential).
fn observable_probes() -> Vec<(&'static str, String)> {
    vec![
        // ── set[str]: bare `any(s)` / `all(s)` (Python iterates the ELEMENTS) ─────
        (
            "set_any_mixed",
            "    s: set[str] = {\"a\", \"\"}\n    return any(s)\n".to_string(),
        ),
        (
            "set_all_mixed",
            "    s: set[str] = {\"a\", \"\"}\n    return all(s)\n".to_string(),
        ),
        (
            "set_all_nonempty",
            "    s: set[str] = {\"a\", \"bc\"}\n    return all(s)\n".to_string(),
        ),
        (
            "set_any_single_empty",
            "    s: set[str] = {\"\"}\n    return any(s)\n".to_string(),
        ),
        (
            "set_all_single_empty",
            "    s: set[str] = {\"\"}\n    return all(s)\n".to_string(),
        ),
        (
            "set_any_single_nonempty",
            "    s: set[str] = {\"x\"}\n    return any(s)\n".to_string(),
        ),
        // ── the empty-set IDENTITIES (via discard of the last element) ─────────────
        (
            "set_any_empty_after_discard",
            "    s: set[str] = {\"x\"}\n    s.discard(\"x\")\n    return any(s)\n".to_string(),
        ),
        (
            "set_all_empty_after_discard",
            "    s: set[str] = {\"x\"}\n    s.discard(\"x\")\n    return all(s)\n".to_string(),
        ),
        // ── reduce AFTER a mutation of the live element set ────────────────────────
        (
            "set_any_after_add",
            "    s: set[str] = {\"\"}\n    s.add(\"y\")\n    return any(s)\n".to_string(),
        ),
        (
            "set_all_after_discard_empty",
            "    s: set[str] = {\"\", \"a\", \"b\"}\n    s.discard(\"\")\n    return all(s)\n"
                .to_string(),
        ),
        // ── RELOCATING grows (20 keys outrun the 16-slot slack) ────────────────────
        (
            "set_all_grow_nonempty",
            format!(
                "    s: set[str] = {{{}}}\n    return all(s)\n",
                grow_all_nonempty()
            ),
        ),
        (
            "set_any_grow_has_empty",
            format!(
                "    s: set[str] = {{{}}}\n    return any(s)\n",
                grow_has_empty()
            ),
        ),
        (
            "set_all_grow_has_empty",
            format!(
                "    s: set[str] = {{{}}}\n    return all(s)\n",
                grow_has_empty()
            ),
        ),
        // ── composed in a boolean context ──────────────────────────────────────────
        (
            "set_not_any_mixed",
            "    s: set[str] = {\"\", \"a\"}\n    return not any(s)\n".to_string(),
        ),
        // ── str-KEYED dict: `any(d)` / `all(d)` (Python iterates the KEYS) ─────────
        (
            "dict_any_mixed",
            "    d: dict[str, int] = {\"a\": 1, \"\": 2}\n    return any(d)\n".to_string(),
        ),
        (
            "dict_all_mixed",
            "    d: dict[str, int] = {\"a\": 1, \"\": 2}\n    return all(d)\n".to_string(),
        ),
        (
            "dict_all_nonempty_keys",
            "    d: dict[str, int] = {\"a\": 1, \"bc\": 2}\n    return all(d)\n".to_string(),
        ),
        // the explicit `.keys()` view
        (
            "dict_all_keys_view",
            "    d: dict[str, int] = {\"a\": 1, \"\": 2}\n    return all(d.keys())\n".to_string(),
        ),
        (
            "dict_any_keys_view",
            "    d: dict[str, int] = {\"a\": 1, \"\": 2}\n    return any(d.keys())\n".to_string(),
        ),
        // ── the empty-dict IDENTITIES (via del of the last key) ────────────────────
        (
            "dict_any_empty_after_del",
            "    d: dict[str, int] = {\"x\": 1}\n    del d[\"x\"]\n    return any(d)\n".to_string(),
        ),
        (
            "dict_all_empty_after_del",
            "    d: dict[str, int] = {\"x\": 1}\n    del d[\"x\"]\n    return all(d)\n".to_string(),
        ),
        // ── reduce AFTER a key mutation ────────────────────────────────────────────
        (
            "dict_all_after_del_empty",
            "    d: dict[str, int] = {\"\": 0, \"a\": 1}\n    del d[\"\"]\n    return all(d)\n"
                .to_string(),
        ),
    ]
}

/// The corpus source: the observable exports PLUS two boundary helpers (a `set[str]`
/// and a `dict[str, int]` param reduced in the callee) and their observable callers.
fn corpus_source() -> String {
    let mut src = String::new();
    for (name, body) in observable_probes() {
        src.push_str(&format!("def {name}() -> bool:\n{body}\n"));
    }
    // Boundary: a set[str] flows across a param and its elements are reduced.
    src.push_str("def reduce_set_param(s: set[str]) -> bool:\n    return all(s)\n");
    src.push_str(
        "def call_set_param() -> bool:\n    s: set[str] = {\"a\", \"b\", \"c\"}\n    return reduce_set_param(s)\n",
    );
    // Boundary: a dict[str, int] flows across a param and its keys are reduced.
    src.push_str("def reduce_dict_param(d: dict[str, int]) -> bool:\n    return all(d)\n");
    src.push_str(
        "def call_dict_param() -> bool:\n    d: dict[str, int] = {\"a\": 1, \"b\": 2}\n    return reduce_dict_param(d)\n",
    );
    src
}

/// Every observable export name — the corpus probes plus the two boundary callers
/// (NOT the `reduce_*_param` helpers, which take a param).
fn observable_names() -> Vec<String> {
    let mut names: Vec<String> = observable_probes()
        .iter()
        .map(|(n, _)| n.to_string())
        .collect();
    names.push("call_set_param".to_string());
    names.push("call_dict_param".to_string());
    names
}

// ---- CONSTRUCT assertions (hold with or without WABT) ------------------------

#[test]
fn str_truthy_reduce_uses_the_fused_helper() {
    let wat = emit(&corpus_source()).expect("the str truthy corpus must lower end-to-end");
    // Both the set[str] and str-keyed dict forms fold via the ONE fused helper —
    // the set/dict local passed straight in as the region base.
    assert!(
        wat.contains("call $__wasm_hash_strkey_truthy_reduce"),
        "str any/all must fold via the fused str-key helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $__wasm_hash_strkey_truthy_reduce"),
        "the fused str-key helper must be DECLARED (the BoolReduce gate), else \
         wat2wasm rejects the module:\n{wat}"
    );
    // A str set/dict does NOT materialise a `list[int]` first (a str element → a
    // `list[str]`, unmodelled) — the fused fold reads the region DIRECTLY. The int
    // materialiser may still be DECLARED (a dead but valid helper — the frontend's
    // `SetToList`/`DictView` node still trips its gate) but it is never CALLED here.
    assert!(
        !wat.contains("call $__wasm_set_to_list_i64")
            && !wat.contains("call $__wasm_dict_values_to_list_i64"),
        "a str set/dict any/all must NOT CALL an int materialiser:\n{wat}"
    );
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// The forms OUTSIDE the str-KEY any/all lane refuse — never silently mis-lowered.
#[test]
fn str_truthy_reduce_refuse_out_of_lane_forms() {
    for (label, src, needle) in [
        // a str-VALUED dict `any(d.values())` — the VALUE slot (`entry+8`) str
        // truthiness is NOT in this (keys-only) slice; still refused (PMAT-1333).
        (
            "any(str-valued d.values())",
            "def f() -> bool:\n    d: dict[int, str] = {1: \"a\", 2: \"\"}\n    return any(d.values())\n"
                .to_string(),
            "str-valued dict",
        ),
        // the lazy short-circuiting GENERATOR form — a per-element predicate lambda.
        (
            "any(<generator over s>)",
            "def f() -> bool:\n    s: set[str] = {\"\", \"a\"}\n    return any(len(x) > 0 for x in s)\n"
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-strtruthy-{}", std::process::id()));
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

/// Execute the IDENTICAL corpus source in live python3, returning `name=value` pairs
/// for the observable exports — the differential ground truth. Each bool is coerced
/// to `int` (0/1) to match `wasm-interp`'s i32 printing.
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
fn str_truthy_reduce_executes_in_wasm_and_matches_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("str truthy corpus must lower");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1336: WABT (wat2wasm / wasm-interp) absent — witness asserted at \
             emit level only (the executed leg runs every export and value-matches \
             live python3 on the identical source when WABT is present)."
        );
        return;
    }

    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1336: python3 absent — witness asserted at emit level only");
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
            "str truthy export `{name}`: wasm {got} != cpython {expected}\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1336: {} str-truthiness observables (`any`/`all` over set[str] and \
         str-keyed dicts — mixed/all-nonempty/single-empty/single-nonempty, the \
         empty-region identities via discard/del, after add/discard/del, three \
         relocating 20-key grows, a boolean compose, and two param-boundary reduces) \
         all == live python3.",
        truth.len()
    );
}
