//! PMAT-1307 — EXECUTED witness for CONTENT-comparing structural equality
//! over str-VALUED dicts (`d1 == d2` / `d1 != d2` where `d: dict[K, str]`),
//! on the bump-heap dict runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! PMAT-1305 shipped str-valued dicts with dict `==` REFUSED: the
//! `$__wasm_dict_eq_<k>` helper compares each 8-byte value slot with
//! `i64.eq`, which for a str value is POINTER identity — while Python
//! `{1: 'a'} == {1: 'a'}` is True across two DISTINCT allocations. This
//! slice wires the named twin: `$__wasm_dict_eq_sv_<k>` — the same size
//! gate, walk-p, and per-key membership probe, with the ONE differing line
//! being the value compare (`i32.wrap_i64` both slots then `$__wasm_str_eq`
//! equality, never `i64.ne`). `emit_binop` routes an `==`/`!=` whose dict
//! operands are str-valued to the twin; int-valued dicts keep the original
//! helper, and dicts whose VALUE kinds disagree refuse (a shared key's
//! values can never compare equal, but `{} == {}` IS True in Python, so
//! neither lane's helper — nor a constant — is correct).
//!
//! ## The silent-miscompile class this witness pins shut
//!
//! The static literal region DEDUPLICATES by content, so two `"x"` literals
//! share ONE pointer — a pointer-identity compare "passes" on all-literal
//! dicts and the miscompile hides. The discriminating pins therefore
//! HEAP-MATERIALISE at least one side of every content-critical probe
//! (concat-built values: distinct allocations, equal bytes — the PMAT-1305
//! standing lesson). `eq_content_cross` / `eq_strkey_cross` fail on ANY
//! pointer-compare regression; the rest pin CPython behaviour (size gate,
//! key-set gate, insertion-order independence, post-`del` swap-into-hole
//! independence, `!=` inversion, empty-dict equality, int-lane coexistence).
//!
//! Every probe is FULL-pipeline (REAL Python → `PythonFrontend` →
//! `emit_module` → `wat2wasm` → `wasm-interp`), value-matched against LIVE
//! python3 executing the IDENTICAL source. Gated on
//! `wasm_runtime_available()` — a clean skip (still asserting emit +
//! helper-carriage + gate tightness + refusals) without WABT.

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

/// Every probe: a zero-arg `def <name>() -> int` (the export) built from REAL
/// Python — the same text is executed by live python3 for the expected value.
fn probes() -> Vec<(&'static str, String)> {
    vec![
        // ── THE content pin: equal bytes, DISTINCT allocations ──────────────
        // Both values are "abc" but each is concat-BUILT (heap-materialised),
        // so the two value slots hold different pointers. Pointer identity
        // answers 0 here; Python (and the sv twin) answer 1.
        (
            "eq_content_cross",
            "    p: str = \"ab\"\n    q: str = \"bc\"\n    a: dict[int, str] = {1: p + \"c\"}\n    b: dict[int, str] = {1: \"a\" + q}\n    if a == b:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        // Same discriminating shape over STR keys (the `_s` twin).
        (
            "eq_strkey_cross",
            "    p: str = \"vv\"\n    q: str = \"vw\"\n    a: dict[str, str] = {\"k\": p + \"w\"}\n    b: dict[str, str] = {\"k\": \"v\" + q}\n    if a == b:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        // `!=` inversion on the same cross-allocation shape (equal content →
        // 0, i.e. the inverted twin result, never pointer-inequality's 1).
        (
            "ne_content_cross",
            "    p: str = \"ab\"\n    a: dict[int, str] = {1: p + \"c\"}\n    b: dict[int, str] = {1: \"a\" + \"bc\"}\n    if a != b:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        // ── behavioural pins (CPython ground truth) ─────────────────────────
        ("eq_lit", "    a: dict[int, str] = {1: \"x\", 2: \"yy\"}\n    b: dict[int, str] = {1: \"x\", 2: \"yy\"}\n    if a == b:\n        return 1\n    return 0\n".to_string()),
        ("neq_value", "    a: dict[int, str] = {1: \"x\"}\n    b: dict[int, str] = {1: \"y\"}\n    if a == b:\n        return 1\n    return 0\n".to_string()),
        ("neq_size", "    a: dict[int, str] = {1: \"x\"}\n    b: dict[int, str] = {1: \"x\", 2: \"y\"}\n    if a == b:\n        return 1\n    return 0\n".to_string()),
        ("neq_keys", "    a: dict[int, str] = {1: \"x\"}\n    b: dict[int, str] = {2: \"x\"}\n    if a == b:\n        return 1\n    return 0\n".to_string()),
        // Insertion order is irrelevant to dict equality.
        ("eq_order_indep", "    a: dict[int, str] = {1: \"a\", 2: \"b\"}\n    b: dict[int, str] = {2: \"b\", 1: \"a\"}\n    if a == b:\n        return 1\n    return 0\n".to_string()),
        // A `del` reorders storage (swap-last-into-hole) — equality is
        // key-probed, so the result survives the reorder.
        ("eq_after_del", "    a: dict[int, str] = {1: \"a\", 2: \"b\", 3: \"c\"}\n    del a[2]\n    b: dict[int, str] = {3: \"c\", 1: \"a\"}\n    if a == b:\n        return 1\n    return 0\n".to_string()),
        // Mutating one side to convergence, then eq.
        ("eq_after_set", "    a: dict[int, str] = {1: \"x\"}\n    b: dict[int, str] = {1: \"zz\"}\n    b[1] = \"x\"\n    if a == b:\n        return 1\n    return 0\n".to_string()),
        // Two EMPTY str-valued dicts are equal (the size gate's 0==0 case —
        // also why mixed VALUE kinds must refuse rather than constant-fold).
        ("eq_empty", "    a: dict[int, str] = {}\n    b: dict[int, str] = {}\n    if a == b:\n        return 1\n    return 0\n".to_string()),
        // The INT-valued lane coexists in the same module: `$__wasm_dict_eq_i`
        // and `$__wasm_dict_eq_sv_i` are distinct helpers, each routed by its
        // dict's value kind.
        ("int_eq_coexists", "    c: dict[int, int] = {1: 2}\n    e: dict[int, int] = {1: 2}\n    if c == e:\n        return 1\n    return 0\n".to_string()),
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
fn dict_str_eq_lowers_and_carries_the_sv_twin() {
    let wat = emit(&corpus_source()).expect("the str-value eq corpus must lower end-to-end");
    // Both key kinds' sv twins are declared AND called; the value compare
    // rides `$__wasm_str_eq`; the int-lane helper coexists for the
    // int-valued probe.
    for needle in [
        "func $__wasm_dict_eq_sv_i",
        "call $__wasm_dict_eq_sv_i",
        "func $__wasm_dict_eq_sv_s",
        "call $__wasm_dict_eq_sv_s",
        "call $__wasm_str_eq",
        "call $__wasm_dict_eq_i",
    ] {
        assert!(
            wat.contains(needle),
            "the corpus WAT must contain {needle}:\n{wat}"
        );
    }
}

/// Gate TIGHTNESS: a module full of str-valued dict READS but with no dict
/// `==`/`!=` must NOT carry the sv twin (the gate hunts equality hosts, not
/// dict bindings).
#[test]
fn dict_str_eq_twin_is_gated_on_an_equality() {
    let wat = emit(
        "def f() -> int:\n    d: dict[int, str] = {1: \"abc\"}\n    d[2] = \"zz\"\n    return len(d[1]) + len(d.get(2, \"q\"))\n",
    )
    .expect("a no-eq str-valued dict module must lower");
    assert!(
        !wat.contains("$__wasm_dict_eq_sv_"),
        "no dict equality in the source → the sv twin must not be emitted:\n{wat}"
    );
}

/// The INT-keyed twin's value compare needs `$__wasm_str_eq` even when the
/// module has NO other string comparison — the gate must FORCE it (a missed
/// force is a call against an undeclared helper, a hard wat2wasm failure;
/// the executed witness would catch it, but pin the emit shape here too).
#[test]
fn dict_str_eq_int_keyed_twin_forces_str_eq() {
    let wat = emit(
        "def f() -> int:\n    a: dict[int, str] = {1: \"x\"}\n    b: dict[int, str] = {1: \"x\"}\n    if a == b:\n        return 1\n    return 0\n",
    )
    .expect("an int-keyed str-valued dict eq must lower");
    assert!(
        wat.contains("func $__wasm_str_eq"),
        "the sv twin calls $__wasm_str_eq — the gate must force its emission:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_dict_eq_sv_i"),
        "the eq must route to the sv twin, not the int-lane helper:\n{wat}"
    );
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// Dicts whose VALUE kinds disagree refuse `==` — the int helper would
/// compare a number against a pointer, and constant-False would miscompile
/// `{} == {}` (True in Python even when the annotations differ).
#[test]
fn dict_str_eq_refuses_mixed_value_kinds() {
    let src = "def f() -> int:\n    a: dict[int, int] = {1: 2}\n    b: dict[int, str] = {1: \"x\"}\n    if a == b:\n        return 1\n    return 0\n";
    let err = emit(src).expect_err("mixed value kinds must refuse");
    assert!(
        err.contains("different VALUE kinds"),
        "refusal should name the value-kind mismatch, got: {err}"
    );
}

/// Dict ORDERING stays refused in the str-valued lane exactly as in the int
/// lane — Python dicts have no `<`.
#[test]
fn dict_str_eq_ordering_still_refused() {
    let src = "def f() -> int:\n    a: dict[int, str] = {1: \"x\"}\n    b: dict[int, str] = {1: \"y\"}\n    if a < b:\n        return 1\n    return 0\n";
    let err = emit(src).expect_err("dict ordering must refuse");
    assert!(
        err.contains("ordering") || err.contains("not in the WASM dict subset"),
        "refusal should name unsupported dict ordering, got: {err}"
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-dictstreq-{}", std::process::id()));
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

/// Execute the IDENTICAL corpus source in live python3, returning
/// `name=value` pairs — the differential ground truth.
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
fn dict_str_eq_executes_in_wasm_and_matches_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("the str-value eq corpus must lower end-to-end");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1307: skipping EXECUTED dict-str-eq witness — WABT \
             (wat2wasm / wasm-interp) absent. The corpus lowered through the \
             FULL pipeline (PythonFrontend → emit_module) and carries the \
             `$__wasm_dict_eq_sv_<k>` twins + `$__wasm_str_eq` (asserted in \
             `dict_str_eq_lowers_and_carries_the_sv_twin`); a box with WABT \
             also runs every export and value-matches live python3 on the \
             identical source. Free CI skips execution and stays green."
        );
        return;
    }
    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1307: python3 absent — witness asserted at emit level only");
        return;
    };
    assert_eq!(
        truth.len(),
        probes().len(),
        "python3 must produce one value per probe"
    );

    eprintln!("PMAT-1307: running EXECUTED dict-str-eq witness via WABT");
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
}
