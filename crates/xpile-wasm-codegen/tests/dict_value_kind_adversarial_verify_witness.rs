//! PMAT-1323 — ADVERSARIAL-VERIFY differential witness over the WHOLE non-int
//! dict VALUE surface (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`). This is a
//! SKEPTIC pass, not a feature: it tries to REFUTE the PMAT-1305 (str),
//! PMAT-1320/1321 (bool), and PMAT-1322 (float) value-kind claims by driving
//! CROSS-CUTTING corners the per-slice witnesses do NOT individually stress, and
//! value-matching every one against LIVE python3 on the identical source.
//!
//! ## The refutation targets (what a bug here would look like)
//!
//! The three non-int value kinds all SHARE the 8-byte `i64` value slot but read
//! back through DIFFERENT transports — a str reads the `i32` base-pointer, a bool
//! `i32.wrap_i64`s to `0`/`1`, a float `f64.reinterpret_i64`s the bits back. A
//! read that used the WRONG transport (an i64 where an f64/bool/pointer is due), a
//! store that skipped its reinterpret/extend, or a value-kind scope flag that
//! failed to register at some binding site would surface as a run-time divergence
//! or an emit-time type mismatch. The probes are built to trip exactly those:
//!
//!   * FLOAT numeric corners — `-0.0 == 0.0` (different bits, equal value),
//!     `1e308` vs `1e-300`, the non-dyadic `0.1 + 0.2`, overwrite, two-read `<`,
//!     `update()`-merge (raw i64-slot copy is bit-preserving), `pop`/`setdefault`
//!     feeding float arithmetic, a negative-float `pop` default, and a RELOCATING
//!     grow (20 keys outrun the 16-slot slack) read back through the moved slots;
//!   * BOOL boolean-ops — `and` / `or` / `not` / `!=` over `d[k]` reads (each
//!     wants the wrapped `i32`, an i64 read would mis-type), overwrite `True→False`,
//!     `update()`-merge, a `get` hit returning the stored `False` over a `True`
//!     default;
//!   * CROSS-KIND no-crosstalk — a `float` / `bool` / `int` / `str` dict all live
//!     in ONE function, each read correctly (no value-kind scope bleed);
//!   * WALKER PARITY — a float/bool dict declared INSIDE a `while` / `if` / `for`
//!     body still registers its value kind (the collector recurses; `ForEach` is
//!     pre-desugared to `While`), so its reads take the right transport;
//!   * BOUNDARY flows — a float/bool/str dict across a function PARAM and a RETURN
//!     (the value kind tracks at both binding sites);
//!   * STR value — content `==`, `pop`, f-string interpolation, `len`.
//!
//! ## Honesty battery (refuses, never silently mis-lowered)
//!
//! The genuine-correctness edges each still refuse through the FULL pipeline:
//! whole-dict `==` over float values (the `i64.eq` slot compare is unsound for
//! `±0.0` / `NaN`), a float/int value-kind-mismatched `==`, `sum`/`min`(`.values()`)
//! and a `for … in d.values()` over float/bool values, `str(d[k])` / f-string of a
//! float, `setdefault` through a dict PARAM (aliasing grow), a 1-arg `.get(k)`
//! (Optional return), bool `d[k] + n` arithmetic, and a NESTED value type.
//!
//! Every probe is FULL-pipeline (REAL Python → `PythonFrontend` → `emit_module` →
//! `wat2wasm` → `wasm-interp`). Gated on `wasm_runtime_available()` — a clean skip
//! (still asserting emit + the refusals) without WABT. Result of the sweep at the
//! PMAT-1322 head: NOTHING REFUTED.

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

/// Module-level helpers for the boundary-flow probes. They ride the emitted
/// module as exports too, but `--run-all-exports` skips a param-requiring export
/// cleanly (verified: exit 0), and `python_truth` never calls them — so they are
/// invisible to the differential.
fn preamble() -> &'static str {
    "def mk_f() -> dict[int, float]:\n\
    \x20   d: dict[int, float] = {0: 8.25}\n\
    \x20   return d\n\
    def take_f(d: dict[int, float]) -> float:\n\
    \x20   return d[0]\n\
    def take_b(d: dict[int, bool]) -> bool:\n\
    \x20   return d[0]\n\
    def take_s(d: dict[int, str]) -> str:\n\
    \x20   return d[0]\n"
}

/// Every probe: a zero-arg `def <name>() -> int` (the export) built from REAL
/// Python — the same text is executed by live python3 for the expected value.
fn probes() -> Vec<(&'static str, &'static str)> {
    vec![
        // ── FLOAT numeric corners + ops ─────────────────────────────────────
        (
            "f_negzero",
            "    d: dict[int, float] = {0: -0.0}\n    return 1 if d[0] == 0.0 else 0\n",
        ),
        (
            "f_large_small",
            "    d: dict[int, float] = {0: 1e308, 1: 1e-300}\n    return 1 if d[0] > d[1] else 0\n",
        ),
        (
            "f_nondyadic",
            "    d: dict[int, float] = {0: 0.1, 1: 0.2}\n    return 1 if (d[0] + d[1]) == (0.1 + 0.2) else 0\n",
        ),
        (
            "f_overwrite",
            "    d: dict[int, float] = {0: 1.5}\n    d[0] = 2.5\n    return 1 if d[0] == 2.5 else 0\n",
        ),
        (
            "f_two_read_lt",
            "    d: dict[int, float] = {0: 1.5, 1: 2.5}\n    return 1 if d[0] < d[1] else 0\n",
        ),
        (
            "f_update_merge",
            "    x: dict[int, float] = {0: 1.5}\n    y: dict[int, float] = {1: 2.5}\n    x.update(y)\n    return 1 if (x[0] == 1.5 and x[1] == 2.5) else 0\n",
        ),
        (
            "f_pop_arith",
            "    d: dict[int, float] = {0: 4.0, 1: 5.0}\n    v: float = d.pop(0) * 2.0\n    return 1 if v == 8.0 else 0\n",
        ),
        (
            "f_setdefault_arith",
            "    d: dict[int, float] = {0: 1.0}\n    x: float = d.setdefault(1, 2.5) + 1.0\n    return 1 if x == 3.5 else 0\n",
        ),
        (
            "f_pop_miss_neg",
            "    d: dict[int, float] = {0: 1.5}\n    v: float = d.pop(9, -3.5)\n    return 1 if v == -3.5 else 0\n",
        ),
        (
            "f_relocate",
            "    d: dict[int, float] = {}\n    i: int = 0\n    while i < 20:\n        d[i] = 0.5\n        i = i + 1\n    return 1 if d[19] == 0.5 else 0\n",
        ),
        // ── BOOL boolean-ops + corners ──────────────────────────────────────
        (
            "b_and",
            "    d: dict[int, bool] = {0: True, 1: False}\n    if d[0] and d[1]:\n        return 1\n    return 0\n",
        ),
        (
            "b_or",
            "    d: dict[int, bool] = {0: False, 1: True}\n    if d[0] or d[1]:\n        return 1\n    return 0\n",
        ),
        (
            "b_not",
            "    d: dict[int, bool] = {0: False}\n    if not d[0]:\n        return 1\n    return 0\n",
        ),
        (
            "b_two_read_ne",
            "    d: dict[int, bool] = {0: True, 1: False}\n    return 1 if d[0] != d[1] else 0\n",
        ),
        (
            "b_overwrite",
            "    d: dict[int, bool] = {0: True}\n    d[0] = False\n    if d[0]:\n        return 1\n    return 0\n",
        ),
        (
            "b_update_merge",
            "    x: dict[int, bool] = {0: True}\n    y: dict[int, bool] = {1: False}\n    x.update(y)\n    if x[0] and not x[1]:\n        return 1\n    return 0\n",
        ),
        (
            "b_get_hit_false",
            "    d: dict[int, bool] = {0: False}\n    v: bool = d.get(0, True)\n    if v:\n        return 1\n    return 0\n",
        ),
        // ── CROSS-KIND no-crosstalk: four value kinds, one function ─────────
        (
            "mixed_kinds",
            "    f: dict[int, float] = {0: 1.5}\n    b: dict[int, bool] = {0: True}\n    n: dict[int, int] = {0: 7}\n    s: dict[int, str] = {0: \"hi\"}\n    if f[0] == 1.5 and b[0] and n[0] == 7 and len(s[0]) == 2:\n        return 1\n    return 0\n",
        ),
        // ── WALKER PARITY: dict declared inside nested control-flow bodies ──
        (
            "f_in_while",
            "    total: int = 0\n    n: int = 0\n    while n < 2:\n        d: dict[int, float] = {0: 2.5}\n        if d[0] == 2.5:\n            total = total + 1\n        n = n + 1\n    return total\n",
        ),
        (
            "f_in_if",
            "    flag: int = 1\n    if flag == 1:\n        d: dict[int, float] = {0: 3.75}\n        return 1 if d[0] == 3.75 else 0\n    return 0\n",
        ),
        (
            "b_in_for",
            "    total: int = 0\n    for i in range(3):\n        d: dict[int, bool] = {0: True}\n        if d[0]:\n            total = total + 1\n    return total\n",
        ),
        // ── BOUNDARY flows across a PARAM + RETURN, all three value kinds ───
        (
            "f_param_return",
            "    d: dict[int, float] = mk_f()\n    return 1 if take_f(d) == 8.25 else 0\n",
        ),
        (
            "b_param",
            "    d: dict[int, bool] = {0: True}\n    return 1 if take_b(d) else 0\n",
        ),
        (
            "s_param",
            "    d: dict[int, str] = {0: \"hey\"}\n    return len(take_s(d))\n",
        ),
        // ── STR value ───────────────────────────────────────────────────────
        (
            "s_read_len",
            "    d: dict[int, str] = {0: \"hello\"}\n    return len(d[0])\n",
        ),
        (
            "s_eq_content",
            "    d1: dict[int, str] = {0: \"ab\"}\n    d2: dict[int, str] = {0: \"ab\"}\n    return 1 if d1 == d2 else 0\n",
        ),
        (
            "s_pop",
            "    d: dict[int, str] = {0: \"xy\", 1: \"z\"}\n    v: str = d.pop(0)\n    return len(v)\n",
        ),
        (
            "s_fstring",
            "    d: dict[int, str] = {0: \"ab\"}\n    s: str = f\"[{d[0]}]\"\n    return len(s)\n",
        ),
    ]
}

fn corpus_source() -> String {
    let mut src = String::from(preamble());
    for (name, body) in probes() {
        src.push_str(&format!("def {name}() -> int:\n{body}\n"));
    }
    src
}

// ---- CONSTRUCT assertions (hold with or without WABT) ------------------------

#[test]
fn dict_value_kind_corpus_lowers_with_the_right_transports() {
    let wat = emit(&corpus_source())
        .expect("the cross-value-kind adversarial corpus must lower end-to-end");
    // Each non-int value kind reads back through its OWN transport — the three
    // must all be present in one module (they coexist without scope bleed).
    for token in [
        // float: bits in via reinterpret_f64, out via reinterpret_i64.
        "i64.reinterpret_f64",
        "f64.reinterpret_i64",
        // bool: 0/1 zero-extended in, wrapped back out.
        "i64.extend_i32_u",
        "i32.wrap_i64",
        // int/bool/float/str VALUES all ride the SAME key-kind-keyed getter (every
        // dict here is int-keyed) — no bespoke per-value-kind helper.
        "call $__wasm_dict_get_i",
    ] {
        assert!(
            wat.contains(token),
            "the cross-value-kind corpus must emit `{token}`:\n{wat}"
        );
    }
    // No bespoke float/bool value helper — the value kinds ride the int/str lanes.
    for forbidden in [
        "$__wasm_dict_floatget",
        "$__wasm_dict_boolget",
        "$__wasm_dict_get_f",
    ] {
        assert!(
            !wat.contains(forbidden),
            "no bespoke value-kind helper may exist ({forbidden}) — routing only:\n{wat}"
        );
    }
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// The genuine-correctness edges each refuse — never silently mis-lowered. Each
/// needle pins the SPECIFIC reason, so a refusal that fired for the wrong cause
/// (or a form that quietly started lowering) is caught.
#[test]
fn dict_value_kind_honesty_battery_refuses() {
    for (label, src, needle) in [
        (
            "float dict `==` (unsound i64.eq over reinterpreted bits)",
            "def f() -> int:\n    d1: dict[int, float] = {0: 1.5}\n    d2: dict[int, float] = {0: 1.5}\n    return 1 if d1 == d2 else 0\n",
            "float-valued dicts",
        ),
        (
            "float vs int value-kind-mismatched `==`",
            "def f() -> int:\n    x: dict[int, float] = {0: 1.0}\n    y: dict[int, int] = {0: 1}\n    return 1 if x == y else 0\n",
            "float-valued dicts",
        ),
        (
            "sum(d.values()) over float values",
            "def f() -> int:\n    d: dict[int, float] = {0: 1.5, 1: 2.5}\n    return 1 if sum(d.values()) == 4.0 else 0\n",
            "sum() over dict values",
        ),
        (
            "min(d.values()) over float values",
            "def f() -> int:\n    d: dict[int, float] = {0: 1.5, 1: 2.5}\n    return 1 if min(d.values()) == 1.5 else 0\n",
            "min() over dict values",
        ),
        (
            "for v in d.values() over bool values",
            "def f() -> int:\n    d: dict[int, bool] = {0: True, 1: False}\n    c: int = 0\n    for v in d.values():\n        if v:\n            c = c + 1\n    return c\n",
            "`.values()` of a `dict[_, bool]`",
        ),
        (
            "f-string of a float value (str(float) dtoa)",
            "def f() -> int:\n    d: dict[int, float] = {0: 1.5}\n    s: str = f\"{d[0]}\"\n    return len(s)\n",
            "str(float) / repr(float) on the WASM lane",
        ),
        (
            "setdefault through a dict PARAM (aliasing grow)",
            "def g(d: dict[int, float]) -> int:\n    x: float = d.setdefault(9, 5.5)\n    return len(d)\n",
            "an insert can grow + relocate",
        ),
        (
            "1-arg `.get(k)` (Optional return)",
            "def f() -> int:\n    d: dict[int, float] = {0: 1.5}\n    v = d.get(0)\n    return 1 if v == 1.5 else 0\n",
            "Optional(F64)",
        ),
        (
            "arithmetic on a bool value read",
            "def f() -> int:\n    d: dict[int, bool] = {0: True}\n    return d[0] + 5\n",
            "outside the WASM scalar/control subset",
        ),
        (
            "a NESTED value type",
            "def f() -> int:\n    d: dict[int, dict[int, int]] = {}\n    return len(d)\n",
            "dict value type",
        ),
    ] {
        let err = match emit(src) {
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-dictvalverify-{}", std::process::id()));
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
/// pairs for every PROBE (never the preamble helpers) — the differential ground
/// truth.
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
fn dict_value_kind_adversarial_execute_and_match_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("the cross-value-kind adversarial corpus must lower end-to-end");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1323: skipping EXECUTED cross-value-kind adversarial witness — WABT \
             (wat2wasm / wasm-interp) absent. The corpus lowered through the FULL \
             pipeline (PythonFrontend → emit_module) and emits all three value-kind \
             transports (reinterpret_f64/i64 for float, extend/wrap for bool, the \
             shared keyed helpers for str) — asserted in \
             `dict_value_kind_corpus_lowers_with_the_right_transports`; a box with \
             WABT also runs every export and value-matches live python3 on the \
             identical source. Free CI skips execution and stays green."
        );
        return;
    }
    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1323: python3 absent — witness asserted at emit level only");
        return;
    };
    assert_eq!(
        truth.len(),
        probes().len(),
        "python3 must produce one value per probe"
    );

    eprintln!("PMAT-1323: running EXECUTED cross-value-kind adversarial witness via WABT");
    let (stdout, ok) = assemble_and_run(&wat);
    assert!(ok, "wasm-interp run failed:\n{stdout}");

    for (name, expected) in &truth {
        let got = parse_scalar_export(&stdout, name);
        assert_eq!(
            got, *expected,
            "executed WASM {name}() = {got} but live CPython = {expected} on the \
             IDENTICAL source — a value-kind transport REFUTED\nfull interp output:\n{stdout}"
        );
    }
    assert!(
        !stdout.contains("unreachable executed"),
        "no cross-value-kind probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1323: EXECUTED cross-value-kind adversarial witness PASSED — {} probes \
         (float numeric corners incl. -0.0/1e308/0.1+0.2/relocate-grow, bool \
         and/or/not/!=, four-kind no-crosstalk, dict-in-while/if/for walker parity, \
         float/bool/str param+return boundary flows, str content-eq/pop/f-string) \
         all == live python3. NOTHING REFUTED.",
        truth.len()
    );
}
