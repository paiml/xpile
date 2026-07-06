//! PMAT-1322 — EXECUTED witness for native-WASM FLOAT-valued dicts
//! (`dict[int, float]` / `dict[str, float]`) — the THIRD non-int dict VALUE kind,
//! after PMAT-1305's str values and PMAT-1320/1321's bool values. It runs on the
//! bump-heap dict runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## What this slice delivers
//!
//! A `float` value SHARES the int STORE lane — the value slot is an 8-byte `i64`
//! either way. But where a bool zero-extends its `i32` `0`/`1` into the slot, a
//! float `i64.reinterpret_f64`s its 64 f64 BITS into it (bit-preserving, no numeric
//! convert), and the READ `f64.reinterpret_i64`s them straight back to an f64. That
//! bit-exact transport is the whole slice: `d[k]` IS a proper `float` (the frontend
//! types it so), so it composes with float arithmetic / comparison / a `float`
//! local. `emit_dict_get` / `emit_dict_get_or` / `emit_dict_pop` /
//! `emit_dict_set_default` each reinterpret the slot back to f64, distinguished by
//! `dict_val_is_float`.
//!
//! ## The surface, pinned by execution against live CPython
//!
//! Every observable is an INT (a comparison / arithmetic guard over EXACT DYADIC
//! floats — 1.5, 2.25, 0.5, … all representable with zero rounding, so WASM f64 ==
//! CPython float bit-for-bit and the boundary value is a clean integer that
//! `wasm-interp` and python3 agree on). A wrong reinterpret direction would surface
//! as an emit-time type mismatch (i64 vs the f64 literal) OR a false comparison at
//! run time — the observables discriminate.
//!
//! WORKS (value-matched vs python3 on the identical source):
//!   * literal `{k: 1.5}` store, `d[k] = <float>` write + overwrite;
//!   * `d[k]` read composed with float `==` / `<` / `+` / `*` (exact dyadic);
//!   * a `x: float = d[k]` local round-trips;
//!   * `d.get(k, default)` — hit (stored float) and miss (float default);
//!   * `d.pop(k, default)` — hit (removes + returns) and miss (float default);
//!   * `d.setdefault(k, default)` — insert-if-absent + existing-key, both return a
//!     float;
//!   * a float value copied dict→dict (`b[k] = a[j]`), stored raw and read back;
//!   * negative floats;
//!   * key-based forms survive over float values — `len`, `k in d`, `del d[k]`,
//!     `for k in d`, `a.update(b)`;
//!   * a str-keyed float dict;
//!   * a RELOCATING grow (20 keys outrun the 16-slot literal slack) then float
//!     reads through the moved slots;
//!   * float dicts flowing across a FUNCTION param and a RETURN (the PMAT-1309/1310
//!     boundary lane, for free — the value kind tracks at both binding sites).
//!
//! REFUSES honestly (NOT silently mis-lowered):
//!   * whole-dict `==` over float values — `$__wasm_dict_eq_<k>` compares slots with
//!     `i64.eq`, but a float slot holds reinterpreted BITS: `0.0 == -0.0` (different
//!     bits) and `nan != nan` (equal bits) would BOTH be mis-answered. A per-value
//!     `f64.eq` compare is not wired; refused (the genuine correctness contribution);
//!   * `.values()` iteration / `sum`/`min`/`max`(`d.values()`) — the value
//!     materialiser fills a `list[i64]`, not the `list[float]` the frontend types,
//!     AND a float `+`-fold is order-sensitive (non-associative) so storage-order
//!     iteration could differ from CPython's insertion order in the last ULP;
//!   * `str(d[k])` / `f"{d[k]}"` — routes through the existing `str(float)` dtoa
//!     refusal;
//!   * a NESTED value (`dict[K, dict…]`) still refuses at the value-kind gate.
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

/// 20 float-valued pairs (`k` → `k / 2` as an exact dyadic float) — the grow
/// probe's source. 20 net-new keys OUTRUN the 16-slot literal slack, forcing a
/// real relocation whose moved value slots are then read back.
fn grow_pairs() -> String {
    let entries: Vec<String> = (1..=20)
        .map(|k| format!("{k}: {}", format_args!("{:.1}", f64::from(k) / 2.0)))
        .collect();
    format!("{{{}}}", entries.join(", "))
}

/// The zero-arg observable probes (the exports). Each `def <name>() -> int` returns
/// an integer computed from float reads — the same text is run by live python3.
fn observable_probes() -> Vec<(&'static str, String)> {
    vec![
        // ── literal store + subscript read composed with float ops ─────────────
        (
            "eq_read",
            "    d: dict[int, float] = {1: 1.5, 2: 2.5}\n    if d[1] == 1.5:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        (
            "lt_cross",
            "    d: dict[int, float] = {1: 1.5, 2: 2.5}\n    if d[1] < d[2]:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        (
            "arith_sum",
            "    d: dict[int, float] = {1: 1.5, 2: 2.5, 3: 0.25}\n    if d[1] * d[2] + d[3] == 4.0:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        // ── a `x: float = d[k]` local round-trips ──────────────────────────────
        (
            "assign_local",
            "    d: dict[int, float] = {1: 3.25}\n    x: float = d[1]\n    if x == 3.25:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        // ── negative float round-trip ──────────────────────────────────────────
        (
            "neg_float",
            "    d: dict[int, float] = {1: -2.5}\n    if d[1] == -2.5:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        // ── d.get(k, default): hit + miss ──────────────────────────────────────
        (
            "get_hit",
            "    d: dict[int, float] = {1: 1.5}\n    if d.get(1, 9.0) == 1.5:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        (
            "get_miss",
            "    d: dict[int, float] = {1: 1.5}\n    if d.get(9, 7.0) == 7.0:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        // ── d.pop(k, default): hit (removes) + miss ────────────────────────────
        (
            "pop_hit",
            "    d: dict[int, float] = {1: 1.5, 2: 2.5}\n    v: float = d.pop(1, 0.0)\n    if v == 1.5:\n        return len(d)\n    return 0\n"
                .to_string(),
        ),
        (
            "pop_miss",
            "    d: dict[int, float] = {1: 1.5}\n    v: float = d.pop(9, 6.5)\n    if v == 6.5:\n        return len(d)\n    return 0\n"
                .to_string(),
        ),
        // ── d.setdefault(k, default): insert-if-absent + existing ──────────────
        (
            "setdefault_insert",
            "    d: dict[int, float] = {1: 1.5}\n    v: float = d.setdefault(5, 3.5)\n    if v == 3.5:\n        return len(d)\n    return 0\n"
                .to_string(),
        ),
        (
            "setdefault_existing",
            "    d: dict[int, float] = {1: 1.5}\n    v: float = d.setdefault(1, 9.9)\n    if v == 1.5:\n        return len(d)\n    return 0\n"
                .to_string(),
        ),
        // ── write + overwrite ──────────────────────────────────────────────────
        (
            "store_new",
            "    d: dict[int, float] = {1: 1.5}\n    d[2] = 2.5\n    return len(d) * 10 + (1 if d[2] == 2.5 else 0)\n"
                .to_string(),
        ),
        (
            "overwrite",
            "    d: dict[int, float] = {1: 1.5}\n    d[1] = 8.5\n    if d[1] == 8.5:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        // ── a float copied dict→dict, stored raw and read back ─────────────────
        (
            "copy_val",
            "    a: dict[int, float] = {1: 1.5}\n    b: dict[int, float] = {5: 5.5}\n    b[9] = a[1]\n    return (1 if b[9] == 1.5 else 0) * 10 + (1 if b[5] == 5.5 else 0)\n"
                .to_string(),
        ),
        // ── a STR-keyed float dict ─────────────────────────────────────────────
        (
            "str_key",
            "    d: dict[str, float] = {\"a\": 1.5, \"b\": 2.5}\n    if d[\"a\"] == 1.5:\n        return len(d)\n    return 0\n"
                .to_string(),
        ),
        // ── key-based forms survive over float VALUES ──────────────────────────
        (
            "del_len",
            "    d: dict[int, float] = {1: 1.5, 2: 2.5}\n    del d[1]\n    return len(d) * 10 + (1 if d[2] == 2.5 else 0)\n"
                .to_string(),
        ),
        (
            "contains_gate",
            "    d: dict[int, float] = {1: 1.5}\n    if 1 in d:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        (
            "key_iter_fold",
            "    d: dict[int, float] = {3: 1.5, 7: 2.5}\n    n: int = 0\n    for k in d:\n        n = n + k\n    return n\n"
                .to_string(),
        ),
        (
            "merge_read",
            "    a: dict[int, float] = {1: 1.5}\n    b: dict[int, float] = {2: 2.5}\n    a.update(b)\n    return len(a) * 10 + (1 if a[1] == 1.5 else 0)\n"
                .to_string(),
        ),
        // ── a RELOCATING grow (20 keys outrun the 16-slot slack) ───────────────
        (
            "grow_reloc",
            format!(
                "    d: dict[int, float] = {}\n    n: int = 0\n    if d[20] == 10.0:\n        n = n + 100\n    if d[1] == 0.5:\n        n = n + 10\n    return n + len(d)\n",
                grow_pairs()
            ),
        ),
    ]
}

/// The corpus source: the observable exports PLUS two helper `def`s the boundary
/// probes call (`read_param`, `make`). The helpers are not observables themselves
/// (they take a param / are called), so they are appended verbatim and excluded
/// from the differential name list.
fn corpus_source() -> String {
    let mut src = String::new();
    for (name, body) in observable_probes() {
        src.push_str(&format!("def {name}() -> int:\n{body}\n"));
    }
    // Boundary helpers + their observable callers.
    src.push_str(
        "def read_param(d: dict[int, float]) -> int:\n    if d[2] == 2.25:\n        return 1\n    return 0\n",
    );
    src.push_str(
        "def call_param() -> int:\n    d: dict[int, float] = {1: 1.5, 2: 2.25}\n    return read_param(d)\n",
    );
    src.push_str(
        "def make() -> dict[int, float]:\n    d: dict[int, float] = {7: 3.5}\n    return d\n",
    );
    src.push_str(
        "def call_return() -> int:\n    d: dict[int, float] = make()\n    d[8] = 4.5\n    if d[7] == 3.5:\n        return len(d) * 10 + (1 if d[8] == 4.5 else 0)\n    return 0\n",
    );
    src
}

/// Every observable export name — the 20 corpus probes plus the two boundary
/// callers (NOT `read_param` / `make`, which take a param / are called).
fn observable_names() -> Vec<String> {
    let mut names: Vec<String> = observable_probes()
        .iter()
        .map(|(n, _)| n.to_string())
        .collect();
    names.push("call_param".to_string());
    names.push("call_return".to_string());
    names
}

// ---- CONSTRUCT assertions (hold with or without WABT) ------------------------

#[test]
fn dict_float_values_lower_and_reuse_helpers() {
    let wat = emit(&corpus_source()).expect("the float-value corpus must lower end-to-end");
    // A float value rides the EXISTING keyed helpers — the store is the shared
    // update-or-insert, the read is the shared get/pop. NO bespoke float helper.
    for call in [
        "call $__wasm_dict_set_i",
        "call $__wasm_dict_get_i",
        "call $__wasm_dict_pop_i",
        "call $__wasm_dict_set_s",
        "call $__wasm_dict_get_s",
        "call $__wasm_dict_update_i",
    ] {
        assert!(
            wat.contains(call),
            "float-value dict must reuse {call}:\n{wat}"
        );
    }
    // The store reinterprets the f64 BITS into the i64 slot; the read reinterprets
    // them straight back to f64 (the whole slice).
    assert!(
        wat.contains("i64.reinterpret_f64"),
        "a float-value store must reinterpret the f64 into the i64 slot:\n{wat}"
    );
    assert!(
        wat.contains("f64.reinterpret_i64"),
        "a float-value read must reinterpret the slot back to an f64:\n{wat}"
    );
    assert!(
        !wat.contains("$__wasm_dict_floatval"),
        "no bespoke float-value helper may exist (routing only):\n{wat}"
    );
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// The forms OUTSIDE the float-value read lane refuse — never silently mis-lowered.
#[test]
fn dict_float_values_refuse_out_of_lane_forms() {
    for (label, src, needle) in [
        // whole-dict `==` over float values — the genuine correctness contribution:
        // i64.eq over reinterpreted bits mis-answers +0.0/-0.0 and NaN.
        (
            "float dict ==",
            "def f() -> bool:\n    a: dict[int, float] = {1: 1.5}\n    b: dict[int, float] = {1: 1.5}\n    return a == b\n".to_string(),
            "float-valued dicts",
        ),
        // .values() reductions — the value materialiser folds list[i64], not
        // list[float], and a float +-fold is order-sensitive.
        (
            "sum(d.values())",
            "def f() -> float:\n    d: dict[int, float] = {1: 1.5, 2: 2.5}\n    return sum(d.values())\n".to_string(),
            "dict values",
        ),
        (
            "max(d.values())",
            "def f() -> float:\n    d: dict[int, float] = {1: 1.5, 2: 2.5}\n    return max(d.values())\n".to_string(),
            "dict values",
        ),
        // .values() iteration (the loop-var-keyed / count shapes that pass the
        // order gate) — explicitly refused for a float value slot.
        (
            "for v in d.values()",
            "def f() -> int:\n    d: dict[int, float] = {1: 1.5, 2: 2.5, 3: 3.5}\n    c: int = 0\n    for v in d.values():\n        c = c + 1\n    return c\n".to_string(),
            "float",
        ),
        // str(float read) — routes through the existing str(float) dtoa refusal.
        (
            "str(d[k])",
            "def f() -> int:\n    d: dict[int, float] = {1: 1.5}\n    s: str = str(d[1])\n    return len(s)\n".to_string(),
            "str(float)",
        ),
    ] {
        let err = match emit(&src) {
            Err(e) => e,
            Ok(wat) => {
                panic!("{label} must be refused for a float-valued dict but lowered:\n{wat}")
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-dictfloatval-{}", std::process::id()));
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
fn dict_float_values_execute_in_wasm_and_match_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("the float-value corpus must lower end-to-end");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1322: skipping EXECUTED dict-float-values witness — WABT \
             (wat2wasm / wasm-interp) absent. The corpus lowered through the FULL \
             pipeline (PythonFrontend → emit_module) and reuses the keyed \
             get/set/pop/update helpers with an f64.reinterpret_i64 read (asserted \
             in `dict_float_values_lower_and_reuse_helpers`); a box with WABT also \
             runs every export and value-matches live python3 on the identical \
             source. Free CI skips execution and stays green."
        );
        return;
    }
    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1322: python3 absent — witness asserted at emit level only");
        return;
    };
    assert_eq!(
        truth.len(),
        observable_names().len(),
        "python3 must produce one value per observable probe"
    );

    eprintln!("PMAT-1322: running EXECUTED dict-float-values witness via WABT");
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
        "no dict-float-values probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1322: EXECUTED dict-float-values witness PASSED — {} observables \
         (literal/store/overwrite, ==/</arithmetic over exact dyadic floats, a \
         float local, get/pop with default hit+miss, setdefault insert+existing, \
         dict→dict value copy, a str-keyed float dict, del/contains/keys-iteration/\
         update surviving over float values, a RELOCATING 20-key grow, and float \
         dicts across a param AND a return boundary) all == live python3.",
        truth.len()
    );
}
