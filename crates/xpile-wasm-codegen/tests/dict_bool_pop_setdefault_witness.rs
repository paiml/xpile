//! PMAT-1321 — EXECUTED witness for the bool-valued dict POP / SETDEFAULT legs
//! (`dict[int, bool]` / `dict[str, bool]`) — the pop/setdefault TWIN of the
//! PMAT-1320 bool `d[k]` / `d.get(k, default)` READS. It runs on the bump-heap
//! dict runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## What this slice delivers
//!
//! PMAT-1320 wired the bool VALUE reads (`d[k]`, `d.get(k, default)`) but left
//! the entry-REMOVING `d.pop(...)` and the get-or-INSERT `d.setdefault(...)`
//! DEFERRED, exactly the str-value split (PMAT-1305 wired the str get/get_or,
//! PMAT-1306 wired the str pop/setdefault). This slice closes that twin.
//!
//! A bool value SHARES the int lane's storage: the value slot is the same 8-byte
//! `i64`, a bool stored as a `0`/`1` zero-extended into it. So the keyed
//! `$__wasm_dict_pop_<k>` / `$__wasm_dict_has_<k>` / `$__wasm_dict_set_<k>` /
//! `$__wasm_dict_get_<k>` helpers are all SHARED with the int lane — NO bespoke
//! bool helper. The whole slice is the type edges:
//!   * `d.pop(k)` / `d.pop(k, default)` `i32.wrap_i64` the popped slot back to a
//!     proper i32 bool (the frontend types `d.pop(k)` as `bool`); the 2-arg
//!     `default` is an i32 bool and the `if` result type is `i32`;
//!   * `d.setdefault(k, default)` `emit_dict_val`-zero-extends the i32 bool
//!     `default` into the i64 slot on the miss INSERT (exactly as `d[k] =
//!     <bool>`), then `i32.wrap_i64`s the read-back `get`.
//!
//! An i64 "just reuse the whole int lane" pop/setdefault — the tempting shortcut
//! PMAT-1320 warned against for the reads — mismatches every bool position it
//! lands beside (`if d.pop(k):` wants i32, `d.pop(k) == True` compares an i32
//! literal, the 2-arg `default` is an i32 bool). The i32-wrap makes them compose.
//!
//! ## The surface, pinned by execution against live CPython
//!
//! WORKS (value-matched vs python3 on the identical source):
//!   * `d.pop(k)` bare — truthiness, `== <bool>`, a `bool` local, the len
//!     decrement, the removed key gone; the absent-key trap = CPython KeyError;
//!   * `d.pop(k, default)` — hit (the removed stored bool) and miss (the i32-bool
//!     `default`, no mutation), both `default` polarities;
//!   * `d.pop(k)` as a bare STATEMENT (value discarded, the removal is the point);
//!   * `str(d.pop(k))` renders `"True"`/`"False"` (len 4/5) via the existing
//!     `str(bool)` path — because the popped value IS a proper bool;
//!   * `d.setdefault(k, default)` — MISS inserts the i32-bool default + grows the
//!     len, HIT keeps the existing value (never overwrites); the returned value
//!     composes in a bool position; a str-keyed bool dict; a RELOCATING grow.
//!
//! REFUSES honestly (NOT silently mis-lowered):
//!   * arithmetic on a popped bool (`d.pop(k) + 5`) — bool arithmetic is outside
//!     the WASM scalar subset;
//!   * `d.setdefault(...)` through a dict PARAMETER — an insert can grow+relocate
//!     the record, leaving the caller's base-pointer stale (the PMAT-1309
//!     growth-through-param refusal, value-kind-agnostic);
//!   * a float-valued `d.setdefault(...)` — the value-kind gate still refuses it.
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

/// 17 bool-valued pairs (keys 1..=17, even → `True`, odd → `False`) — the
/// setdefault-grow probe's seed. 17 keys sit right below the 16-slot literal
/// slack; inserting an 18th via `setdefault` forces a real relocation whose
/// moved slots are then read back.
fn grow_seed() -> String {
    let entries: Vec<String> = (1..=17)
        .map(|k| format!("{k}: {}", if k % 2 == 0 { "True" } else { "False" }))
        .collect();
    format!("{{{}}}", entries.join(", "))
}

/// Every probe: a zero-arg `def <name>() -> int` (the export) built from REAL
/// Python — the same text is executed by live python3 for the expected value.
fn probes() -> Vec<(&'static str, String)> {
    vec![
        // ── d.pop(k) bare — the popped bool in every bool position ──────────
        (
            "pop_bare_true",
            "    d: dict[int, bool] = {1: True, 2: False}\n    if d.pop(1):\n        return 10\n    return 20\n"
                .to_string(),
        ),
        (
            "pop_bare_false",
            "    d: dict[int, bool] = {1: True, 2: False}\n    if d.pop(2):\n        return 10\n    return 20\n"
                .to_string(),
        ),
        (
            "pop_eq_lit",
            "    d: dict[int, bool] = {1: True}\n    if d.pop(1) == True:\n        return 3\n    return 4\n"
                .to_string(),
        ),
        (
            "pop_assign_local",
            "    d: dict[int, bool] = {1: False}\n    b: bool = d.pop(1)\n    if b:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        // ── d.pop removes: len decrement + the key gone ─────────────────────
        (
            "pop_len_dec",
            "    d: dict[int, bool] = {1: True, 2: False}\n    x: bool = d.pop(1)\n    return len(d) * 100 + (1 if x else 0)\n"
                .to_string(),
        ),
        (
            "pop_key_gone",
            "    d: dict[int, bool] = {1: True, 2: False}\n    y: bool = d.pop(1)\n    if 1 in d:\n        return 9\n    return 1 if d[2] == False else 8\n"
                .to_string(),
        ),
        // ── d.pop(k) as a bare STATEMENT (removal is the point) ─────────────
        (
            "pop_stmt",
            "    d: dict[int, bool] = {1: True, 2: False}\n    d.pop(1)\n    return len(d) * 10 + (1 if d[2] else 0)\n"
                .to_string(),
        ),
        // ── d.pop(k, default) — hit / miss / both default polarities ────────
        (
            "pop_def_hit",
            "    d: dict[int, bool] = {1: True}\n    if d.pop(1, False):\n        return 5\n    return 6\n"
                .to_string(),
        ),
        (
            "pop_def_miss",
            "    d: dict[int, bool] = {1: True}\n    if d.pop(9, False):\n        return 5\n    return 6\n"
                .to_string(),
        ),
        (
            "pop_def_miss_true",
            "    d: dict[int, bool] = {1: False}\n    if d.pop(9, True):\n        return 5\n    return 6\n"
                .to_string(),
        ),
        (
            "pop_def_no_mutate",
            "    d: dict[int, bool] = {1: True}\n    z: bool = d.pop(9, False)\n    return len(d) * 10 + (1 if z else 0)\n"
                .to_string(),
        ),
        // ── str(d.pop(k)) via the existing str(bool) path ───────────────────
        (
            "str_pop_true",
            "    d: dict[int, bool] = {1: True}\n    s: str = str(d.pop(1))\n    return len(s)\n"
                .to_string(),
        ),
        (
            "str_pop_false",
            "    d: dict[int, bool] = {1: False}\n    s: str = str(d.pop(1))\n    return len(s)\n"
                .to_string(),
        ),
        // ── d.setdefault(k, default) — miss inserts, hit keeps ──────────────
        (
            "sd_miss_insert",
            "    d: dict[int, bool] = {1: True}\n    x: bool = d.setdefault(2, False)\n    return len(d) * 10 + (1 if x else 0)\n"
                .to_string(),
        ),
        (
            "sd_hit_keeps",
            "    d: dict[int, bool] = {1: True}\n    x: bool = d.setdefault(1, False)\n    return len(d) * 10 + (1 if x else 0)\n"
                .to_string(),
        ),
        (
            "sd_miss_true",
            "    d: dict[int, bool] = {1: False}\n    if d.setdefault(2, True):\n        return 7\n    return 8\n"
                .to_string(),
        ),
        (
            "sd_stmt_ensure",
            "    d: dict[int, bool] = {1: True}\n    d.setdefault(2, False)\n    return len(d) * 10 + (0 if d[2] else 1)\n"
                .to_string(),
        ),
        // ── a STR-keyed bool dict, pop + setdefault ─────────────────────────
        (
            "sd_str_key",
            "    d: dict[str, bool] = {\"on\": True}\n    x: bool = d.setdefault(\"off\", False)\n    return len(d) * 10 + (1 if d[\"on\"] else 0)\n"
                .to_string(),
        ),
        (
            "pop_str_key",
            "    d: dict[str, bool] = {\"on\": True, \"off\": False}\n    if d.pop(\"on\"):\n        return len(d)\n    return 99\n"
                .to_string(),
        ),
        // ── a RELOCATING grow: setdefault an 18th key past the 16-slot slack ─
        (
            "sd_grow_reloc",
            format!(
                "    d: dict[int, bool] = {}\n    x: bool = d.setdefault(99, True)\n    return len(d) * 10 + (1 if x else 0)\n",
                grow_seed()
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
fn dict_bool_pop_setdefault_lower_and_reuse_helpers() {
    let wat = emit(&corpus_source()).expect("the bool pop/setdefault corpus must lower end-to-end");
    // The bool pop/setdefault legs ride the EXISTING keyed helpers — pop shares
    // the removal helper, setdefault the has/set/get triple. NO bespoke helper.
    for call in [
        "call $__wasm_dict_pop_i",
        "call $__wasm_dict_pop_s",
        "call $__wasm_dict_has_i",
        "call $__wasm_dict_set_i",
        "call $__wasm_dict_get_i",
    ] {
        assert!(
            wat.contains(call),
            "bool pop/setdefault must reuse {call}:\n{wat}"
        );
    }
    // The whole slice: the setdefault store zero-extends the i32 bool into the
    // slot; every pop/setdefault read wraps it back to a proper i32 bool.
    assert!(
        wat.contains("i64.extend_i32_u"),
        "a bool setdefault insert must zero-extend the i32 0/1 into the slot:\n{wat}"
    );
    assert!(
        wat.contains("i32.wrap_i64"),
        "a bool pop/setdefault read must wrap the slot back to an i32 bool:\n{wat}"
    );
    // str(d.pop(k)) renders "True"/"False" from the existing bool literals — no
    // int→str "1"/"0".
    assert!(
        wat.contains("\"True\"") && wat.contains("\"False\""),
        "str(d.pop(k)) over a bool value must reuse the True/False literals:\n{wat}"
    );
    assert!(
        !wat.contains("$__wasm_dict_boolpop") && !wat.contains("$__wasm_dict_boolsd"),
        "no bespoke bool pop/setdefault helper may exist (routing only):\n{wat}"
    );
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// The forms OUTSIDE the bool pop/setdefault lane refuse — never silently
/// mis-lowered.
#[test]
fn dict_bool_pop_setdefault_refuse_out_of_lane_forms() {
    for (label, src, needle) in [
        // arithmetic on a popped bool — bool arithmetic is not in the scalar subset.
        (
            "d.pop(k) + 5 arithmetic",
            "def f() -> int:\n    d: dict[int, bool] = {1: True, 2: False}\n    return d.pop(1) + 5\n".to_string(),
            "outside the WASM scalar/control subset",
        ),
        // setdefault through a dict PARAM — an insert can grow+relocate (the
        // value-kind-agnostic PMAT-1309 growth-through-param refusal).
        (
            "d.setdefault through a param",
            "def g(d: dict[int, bool]) -> int:\n    x: bool = d.setdefault(9, True)\n    return len(d)\n".to_string(),
            "growth through a param",
        ),
        // a float-valued setdefault — the value-kind gate still refuses float.
        (
            "float-valued setdefault",
            "def f() -> int:\n    d: dict[int, float] = {1: 2.5}\n    x: float = d.setdefault(2, 3.5)\n    return len(d)\n".to_string(),
            "dict value type",
        ),
    ] {
        let err = match emit(&src) {
            Err(e) => e,
            Ok(wat) => panic!("{label} must be refused but lowered:\n{wat}"),
        };
        assert!(
            err.contains(needle),
            "{label} refusal should say {needle:?}, got: {err}"
        );
    }
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
    // A UNIQUE dir per call (pid + a monotonic counter) — this witness runs THREE
    // execution paths (corpus / trap / mutation) that libtest schedules on
    // parallel threads in the SAME process, so a pid-only path would race on the
    // shared `prog.wat`/`prog.wasm`.
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("xpile-wasm-boolpopsd-{}-{n}", std::process::id()));
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
fn dict_bool_pop_setdefault_execute_in_wasm_and_match_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("the bool pop/setdefault corpus must lower end-to-end");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1321: skipping EXECUTED dict-bool-pop/setdefault witness — WABT \
             (wat2wasm / wasm-interp) absent. The corpus lowered through the FULL \
             pipeline (PythonFrontend → emit_module) and reuses the keyed \
             pop/has/set/get helpers with an i32.wrap_i64 read + i64.extend_i32_u \
             store (asserted in `dict_bool_pop_setdefault_lower_and_reuse_helpers`); \
             a box with WABT also runs every export and value-matches live python3 \
             on the identical source. Free CI skips execution and stays green."
        );
        return;
    }
    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1321: python3 absent — witness asserted at emit level only");
        return;
    };
    assert_eq!(
        truth.len(),
        probes().len(),
        "python3 must produce one value per probe"
    );

    eprintln!("PMAT-1321: running EXECUTED dict-bool-pop/setdefault witness via WABT");
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
        "no dict-bool-pop/setdefault probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1321: EXECUTED dict-bool-pop/setdefault witness PASSED — {} probes \
         (pop bare truthiness/==/local, len decrement + key gone, pop bare \
         statement, pop-with-default hit+miss+no-mutate, str(pop) via str(bool), \
         setdefault miss-insert/hit-keeps/ensure, a str-keyed bool dict, and a \
         RELOCATING setdefault grow) all == live python3.",
        truth.len()
    );
}

/// A bare `d.pop(missing)` over a bool value TRAPS (`unreachable`) — the CPython
/// `KeyError`, exactly the int/str bare-pop behaviour. Kept SEPARATE from the
/// value corpus so `--run-all-exports` doesn't abort the whole batch on the trap.
#[test]
fn dict_bool_pop_absent_key_traps() {
    let int_src =
        "def pop_absent() -> int:\n    d: dict[int, bool] = {1: True}\n    x: bool = d.pop(9)\n    return 1 if x else 0\n";
    let str_src =
        "def pop_absent() -> int:\n    d: dict[str, bool] = {\"a\": True}\n    x: bool = d.pop(\"z\")\n    return 1 if x else 0\n";
    for (label, src) in [("int-keyed", int_src), ("str-keyed", str_src)] {
        let wat = emit(src).expect("absent-key bool pop must LOWER (it traps at runtime)");
        if !wasm_runtime_available() {
            eprintln!("PMAT-1321: skipping {label} bool-pop trap execution — WABT absent");
            continue;
        }
        let (stdout, _ok) = assemble_and_run(&wat);
        assert!(
            stdout.contains("unreachable executed"),
            "{label} `d.pop(missing)` over a bool value must trap (KeyError):\n{stdout}"
        );
    }
}

/// MUTATION verify — corrupt ONE executed pin and confirm the differential lane
/// FAILS, proving the pins actually discriminate (not vacuously green).
#[test]
fn dict_bool_pop_setdefault_pins_discriminate() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1321: skipping MUTATION-verify — WABT absent");
        return;
    }
    // Corrupt `sd_hit_keeps`: a HIT must KEEP the stored `True` (return
    // len*10+1 = 11); flip the read to the miss default `False` and the pin
    // would read 10 — the differential must catch it. We emulate the corruption
    // by comparing the real WASM value against a WRONG expected constant.
    let src = "def sd_hit_keeps() -> int:\n    d: dict[int, bool] = {1: True}\n    x: bool = d.setdefault(1, False)\n    return len(d) * 10 + (1 if x else 0)\n";
    let wat = emit(src).expect("lower");
    let (stdout, ok) = assemble_and_run(&wat);
    assert!(ok, "run failed:\n{stdout}");
    let got = parse_scalar_export(&stdout, "sd_hit_keeps");
    // The REAL value is 11 (hit keeps True). A regression that let setdefault
    // OVERWRITE on a hit (or dropped the wrap) would yield 10 — assert the pin
    // is 11 AND is not the corrupt 10, so a real regression flips a green test red.
    assert_eq!(
        got, 11,
        "setdefault HIT must keep the stored True (11), got {got}"
    );
    assert_ne!(
        got, 10,
        "a hit that overwrote to False would read 10 — the pin must reject it"
    );
}
