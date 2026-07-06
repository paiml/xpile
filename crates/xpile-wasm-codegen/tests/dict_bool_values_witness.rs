//! PMAT-1320 — EXECUTED witness for native-WASM BOOL-valued dicts
//! (`dict[int, bool]` / `dict[str, bool]`) — the SECOND non-int dict VALUE kind,
//! after PMAT-1305's str values. It runs on the bump-heap dict runtime
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## What this slice actually delivers (and how it corrects a false start)
//!
//! A `bool` value SHARES the int STORE lane — the value slot is an 8-byte `i64`
//! either way, and the store zero-extends the bool's `i32` `0`/`1` into it
//! exactly as PMAT-1305 extends a str base-pointer. But the READ is DISTINCT and
//! that is the whole slice: `emit_dict_get` / `emit_dict_get_or` WRAP the slot
//! back `i64`→`i32` so `d[k]` is a proper BOOL, because the frontend types
//! `d[k]` as `bool` (an i32). An i64 "reuse the int lane" read — the tempting
//! shortcut — mismatches EVERY bool operand it lands beside: `if d[k]:` wants an
//! i32, `d[k] == True` compares against an i32 bool literal, `d.get(k, False)`
//! has an i32 default. The i32-wrap read makes all of them compose.
//!
//! ## The surface, pinned by execution against live CPython
//!
//! WORKS (value-matched vs python3 on the identical source):
//!   * literal `{k: True/False}` store, `d[k] = <bool>` write + overwrite;
//!   * `d[k]` read in every bool position — truthiness `if d[k]:` / `if not
//!     d[k]:`, `==`/`!=` against a bool literal AND cross-dict, a `b: bool =
//!     d[k]` local;
//!   * `d.get(k, default)` — a TOTAL read, hit (the stored bool) and miss (the
//!     i32-bool default), both branches;
//!   * `str(d[k])` / `f"{d[k]}"` — because the read IS a bool, these route
//!     through the EXISTING `str(bool)` path and render `"True"`/`"False"` (len
//!     4/5), NOT the int→str `"1"`/`"0"`; no special handling was needed;
//!   * a bool value copied dict→dict (`b[k] = a[j]`), stored raw and read back;
//!   * key-based forms survive over bool values — `len`, `k in d`, `del d[k]`,
//!     `for k in d`, `a.update(b)`;
//!   * a RELOCATING grow (20 keys outrun the 16-slot literal slack) then bool
//!     reads through the moved slots.
//!
//! REFUSES honestly (NOT silently mis-lowered):
//!   * arithmetic on a bool read (`d[k] + 5`) — bool arithmetic is not in the
//!     WASM scalar subset;
//!   * `.values()` iteration / `sum`/`min`/`max`(`d.values()`) — `d.values()`
//!     types as `list[bool]`, not the `list[i64]` the value materialiser folds;
//!   * `d.pop(...)` / `d.setdefault(...)` — the bool value-return legs are
//!     DEFERRED, exactly the str-value split (PMAT-1305 wired get/get_or,
//!     PMAT-1306 wired pop/setdefault);
//!   * the value-kind gate still refuses float/nested values.
//!
//! Every probe is FULL-pipeline (REAL Python → `PythonFrontend` → `emit_module`
//! → `wat2wasm` → `wasm-interp`), value-matched against LIVE python3 executing
//! the IDENTICAL source. Gated on `wasm_runtime_available()` — a clean skip
//! (still asserting emit + refusals) without WABT.

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

/// 20 bool-valued pairs (`k` even → `True`, odd → `False`) — the grow probe's
/// source. 20 net-new keys OUTRUN the 16-slot literal slack, forcing a real
/// relocation whose moved value slots are then read back.
fn grow_pairs() -> String {
    let entries: Vec<String> = (1..=20)
        .map(|k| format!("{k}: {}", if k % 2 == 0 { "True" } else { "False" }))
        .collect();
    format!("{{{}}}", entries.join(", "))
}

/// Every probe: a zero-arg `def <name>() -> int` (the export) built from REAL
/// Python — the same text is executed by live python3 for the expected value.
fn probes() -> Vec<(&'static str, String)> {
    vec![
        // ── literal store + subscript read in bool positions ────────────────
        (
            "truthy_true",
            "    d: dict[int, bool] = {1: True, 2: False}\n    if d[1]:\n        return 11\n    return 22\n"
                .to_string(),
        ),
        (
            "truthy_false",
            "    d: dict[int, bool] = {1: True, 2: False}\n    if d[2]:\n        return 11\n    return 22\n"
                .to_string(),
        ),
        (
            "not_read",
            "    d: dict[int, bool] = {1: False}\n    if not d[1]:\n        return 8\n    return 9\n"
                .to_string(),
        ),
        // ── `==` / `!=` against a bool literal AND cross-dict ───────────────
        (
            "eq_true_lit",
            "    d: dict[int, bool] = {1: True}\n    if d[1] == True:\n        return 3\n    return 4\n"
                .to_string(),
        ),
        (
            "eq_false_lit",
            "    d: dict[int, bool] = {1: False}\n    if d[1] == False:\n        return 3\n    return 4\n"
                .to_string(),
        ),
        (
            "neq_cross",
            "    a: dict[int, bool] = {1: True}\n    b: dict[int, bool] = {2: False}\n    if a[1] != b[2]:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        (
            "eq_cross_same",
            "    a: dict[int, bool] = {1: True}\n    b: dict[int, bool] = {2: True}\n    if a[1] == b[2]:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        // ── a `b: bool = d[k]` local composes ───────────────────────────────
        (
            "assign_local",
            "    d: dict[int, bool] = {1: True}\n    b: bool = d[1]\n    if b:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        // ── the TOTAL read: get(k, default) — hit (stored) + miss (default) ─
        (
            "get_hit",
            "    d: dict[int, bool] = {1: True}\n    if d.get(1, False):\n        return 5\n    return 6\n"
                .to_string(),
        ),
        (
            "get_miss",
            "    d: dict[int, bool] = {1: True}\n    if d.get(9, False):\n        return 5\n    return 6\n"
                .to_string(),
        ),
        (
            "get_miss_true_default",
            "    d: dict[int, bool] = {1: False}\n    if d.get(9, True):\n        return 5\n    return 6\n"
                .to_string(),
        ),
        // ── str(d[k]) / f-string render via the EXISTING str(bool) path ─────
        (
            "str_true_len",
            "    d: dict[int, bool] = {1: True}\n    s: str = str(d[1])\n    return len(s)\n"
                .to_string(),
        ),
        (
            "str_false_len",
            "    d: dict[int, bool] = {1: False}\n    s: str = str(d[1])\n    return len(s)\n"
                .to_string(),
        ),
        (
            "fstr_len",
            "    d: dict[int, bool] = {1: True}\n    s: str = f\"v={d[1]}\"\n    return len(s)\n"
                .to_string(),
        ),
        // ── write + overwrite ───────────────────────────────────────────────
        (
            "store_new",
            "    d: dict[int, bool] = {1: True}\n    d[2] = False\n    return len(d) * 10 + (1 if d[1] else 0)\n"
                .to_string(),
        ),
        (
            "overwrite",
            "    d: dict[int, bool] = {1: True}\n    d[1] = False\n    if d[1]:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        // ── a bool copied dict→dict, stored raw and read back ───────────────
        (
            "copy_val",
            "    a: dict[int, bool] = {1: True}\n    b: dict[int, bool] = {5: False}\n    b[9] = a[1]\n    return (1 if b[9] else 0) * 10 + (1 if b[5] else 0)\n"
                .to_string(),
        ),
        // ── a STR-keyed bool dict ───────────────────────────────────────────
        (
            "str_key",
            "    d: dict[str, bool] = {\"on\": True, \"off\": False}\n    if d[\"on\"]:\n        return len(d)\n    return 0\n"
                .to_string(),
        ),
        // ── key-based forms survive over bool VALUES ────────────────────────
        (
            "del_len",
            "    d: dict[int, bool] = {1: True, 2: False}\n    del d[1]\n    return len(d) * 10 + (1 if d[2] else 0)\n"
                .to_string(),
        ),
        (
            "contains_gate",
            "    d: dict[int, bool] = {1: True}\n    if 1 in d:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        (
            "key_iter_fold",
            "    d: dict[int, bool] = {3: True, 7: False}\n    n: int = 0\n    for k in d:\n        n = n + k\n    return n\n"
                .to_string(),
        ),
        (
            "merge_read",
            "    a: dict[int, bool] = {1: True}\n    b: dict[int, bool] = {2: False}\n    a.update(b)\n    return len(a) * 10 + (1 if a[1] else 0)\n"
                .to_string(),
        ),
        // ── a RELOCATING grow (20 keys outrun the 16-slot slack) ────────────
        (
            "grow_reloc",
            format!(
                "    d: dict[int, bool] = {}\n    n: int = 0\n    if d[20]:\n        n = n + 100\n    if d[1]:\n        n = n + 10\n    return n + len(d)\n",
                grow_pairs()
            ),
        ),
    ]
}

fn corpus_source() -> String {
    let mut src = String::new();
    for (name, body) in probes() {
        src.push_str(&format!("def {name}() -> int:\n{body}\n"));
    }
    src
}

// ---- CONSTRUCT assertions (hold with or without WABT) ------------------------

#[test]
fn dict_bool_values_lower_and_reuse_helpers() {
    let wat = emit(&corpus_source()).expect("the bool-value corpus must lower end-to-end");
    // A bool value rides the EXISTING keyed helpers — the store is the shared
    // update-or-insert, the read is the shared get; str(bool) reuses the bool
    // string literals. NO bespoke bool-value helper.
    for call in [
        "call $__wasm_dict_set_i",
        "call $__wasm_dict_get_i",
        "call $__wasm_dict_set_s",
        "call $__wasm_dict_get_s",
        "call $__wasm_dict_update_i",
    ] {
        assert!(
            wat.contains(call),
            "bool-value dict must reuse {call}:\n{wat}"
        );
    }
    // The store zero-extends the bool into the slot; the read wraps it back to a
    // proper i32 bool (the whole slice).
    assert!(
        wat.contains("i64.extend_i32_u"),
        "a bool-value store must zero-extend the i32 0/1 into the slot:\n{wat}"
    );
    assert!(
        wat.contains("i32.wrap_i64"),
        "a bool-value read must wrap the slot back to an i32 bool:\n{wat}"
    );
    // str(bool) of a read renders "True"/"False" from the existing literals.
    assert!(
        wat.contains("\"True\"") && wat.contains("\"False\""),
        "str(d[k]) over a bool value must reuse the True/False string literals:\n{wat}"
    );
    assert!(
        !wat.contains("$__wasm_dict_boolval"),
        "no bespoke bool-value helper may exist (routing only):\n{wat}"
    );
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// The forms OUTSIDE the bool-value read lane refuse — never silently mis-lowered.
#[test]
fn dict_bool_values_refuse_out_of_lane_forms() {
    for (label, src, needle) in [
        // arithmetic on a bool read — bool arithmetic is not in the scalar subset.
        (
            "d[k] + 5 arithmetic",
            "def f() -> int:\n    d: dict[int, bool] = {1: True, 2: False}\n    return d[1] + d[2] + 5\n".to_string(),
            "outside the WASM scalar/control subset",
        ),
        // .values() reductions — d.values() types as list[bool], not the
        // list[i64] the value materialiser folds.
        (
            "sum(d.values())",
            "def f() -> int:\n    d: dict[int, bool] = {1: True, 2: False}\n    return sum(d.values())\n".to_string(),
            "sum() of a non-name list",
        ),
        // pop/setdefault bool value-return legs are DEFERRED (the str split).
        (
            "d.pop(k, default)",
            "def f() -> int:\n    d: dict[int, bool] = {1: True}\n    if d.pop(1, False):\n        return 1\n    return 0\n".to_string(),
            "bool pop/setdefault legs are not wired yet",
        ),
        (
            "d.setdefault(k, default)",
            "def f() -> int:\n    d: dict[int, bool] = {1: True}\n    x: bool = d.setdefault(2, False)\n    return len(d)\n".to_string(),
            "bool pop/setdefault legs are not wired yet",
        ),
    ] {
        let err = match emit(&src) {
            Err(e) => e,
            Ok(wat) => panic!("{label} must be refused for a bool-valued dict but lowered:\n{wat}"),
        };
        assert!(
            err.contains(needle),
            "{label} refusal should say {needle:?}, got: {err}"
        );
    }
}

/// The value-kind gate still refuses the UNMODELLED kinds (float/nested) — adding
/// bool must not have opened anything else.
#[test]
fn dict_value_gate_still_refuses_float() {
    let err = emit("def f() -> int:\n    d: dict[int, float] = {1: 2.5}\n    return len(d)\n")
        .expect_err("a float-valued dict must still be refused");
    assert!(
        err.contains("dict value type"),
        "float-value refusal should come from the value gate, got: {err}"
    );
}

// ---- WABT harness -------------------------------------------------------------

/// Parse a `name() => <ty>:<v>` line. `wasm-interp` prints integers as UNSIGNED
/// decimal; every pin here is non-negative, so `u64` → `i64` is exact.
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-dictboolval-{}", std::process::id()));
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
/// pairs — the differential ground truth.
fn python_truth(src: &str) -> Option<Vec<(String, i64)>> {
    let names: Vec<&str> = probes().iter().map(|(n, _)| *n).collect();
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
fn dict_bool_values_execute_in_wasm_and_match_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("the bool-value corpus must lower end-to-end");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1320: skipping EXECUTED dict-bool-values witness — WABT \
             (wat2wasm / wasm-interp) absent. The corpus lowered through the \
             FULL pipeline (PythonFrontend → emit_module) and reuses the keyed \
             get/set/update helpers with an i32.wrap_i64 read (asserted in \
             `dict_bool_values_lower_and_reuse_helpers`); a box with WABT also \
             runs every export and value-matches live python3 on the identical \
             source. Free CI skips execution and stays green."
        );
        return;
    }
    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1320: python3 absent — witness asserted at emit level only");
        return;
    };
    assert_eq!(
        truth.len(),
        probes().len(),
        "python3 must produce one value per probe"
    );

    eprintln!("PMAT-1320: running EXECUTED dict-bool-values witness via WABT");
    let (stdout, ok) = assemble_and_run(&wat);
    assert!(ok, "wasm-interp run failed:\n{stdout}");

    for (name, expected) in &truth {
        let got = parse_scalar_export(&stdout, name);
        assert_eq!(
            got, *expected,
            "executed WASM {name}() = {got} but live CPython = {expected} on the \
             IDENTICAL source\nfull interp output:\n{stdout}"
        );
    }
    assert!(
        !stdout.contains("unreachable executed"),
        "no dict-bool-values probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1320: EXECUTED dict-bool-values witness PASSED — {} probes \
         (literal/store/overwrite, truthiness/not, == / != against a bool literal \
         AND cross-dict, a bool local, get-with-default hit+miss, str/f-string \
         via the str(bool) path, dict→dict value copy, a str-keyed bool dict, \
         del/contains/keys-iteration/update surviving over bool values, and a \
         RELOCATING 20-key grow) all == live python3.",
        truth.len()
    );
}
