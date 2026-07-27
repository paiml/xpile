//! PMAT-1366 — EXECUTED witness for module-level scalar CONSTANTS in the WASM
//! lane: Python `NAME = <int/bool/float literal>` at module scope, lowered to an
//! IMMUTABLE `(global $NAME <ty> (<ty>.const v))`. Runs on the scalar runtime
//! (`C-COMPILE-RUST-TO-WASM`).
//!
//! ## What this slice delivers
//!
//! Before it, ANY `Item::Const` in the module hard-refused —
//! "module-level const `X` (only scalar/control functions are in the WASM
//! subset)" — so a single `MAX = 100` at the top of a file took the WHOLE module
//! out of the WASM lane, however scalar the functions below it were. Now:
//!
//!   * each const emits once as an immutable global, laid down before the
//!     `$__heap_ptr` mutable global and every function;
//!   * a body reference resolves `local.get` FIRST and falls back to
//!     `global.get $NAME` (locals and globals are separate WAT index spaces);
//!   * the int consts are seen by the f-string int-classifier, so `f"{MAX}"`
//!     auto-stringifies through `str(int)` exactly like a param would.
//!
//! ## Why IMMUTABLE is the exact encoding (not a simplification)
//!
//! Nothing in the supported subset can rebind a module const: the frontend
//! REFUSES both a parameter and a function-local assignment that shadows one
//! (it cannot emit a Rust binding that shadows a `const`). Both refusals are
//! asserted below, so the "immutable" claim rests on a checked upstream
//! guarantee rather than on optimism — and `global.set` is never emitted.
//!
//! ## What stays REFUSED
//!
//! A `str` (or collection) module constant never becomes an `Item::Const` at
//! all — `try_const_decl` (PMAT-502bj) only accepts a folded int/bool/float
//! literal, and the frontend refuses the rest with its own message. Asserted
//! below so the boundary is pinned from this side too.
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

/// FULL pipeline: Python source → meta-HIR → WAT.
fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

// ---- the probe corpus --------------------------------------------------------

/// The module-level constants every probe below reads. Deliberately covers all
/// three representable kinds AND both signs of the numeric ones — a negative
/// literal is FOLDED into the const by the frontend (`-3`, not `UnOp::Neg`),
/// which is the only shape a WASM constant init expression accepts.
const CONST_HEADER: &str = "\
SCALE: int = 7
LIMIT: int = 3
NEG: int = -4
ZERO: int = 0
RATE: float = 2.5
NEG_RATE: float = -1.5
ON: bool = True
OFF: bool = False
BARE = 11
";

/// Each `(name, return-kw, body)` becomes a zero-arg `def <name>() -> <kw>`.
fn probes() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        // ── the bare reads, one per kind ──────────────────────────────────────
        ("c_int", "int", "    return SCALE\n"),
        ("c_int_neg", "int", "    return NEG\n"),
        ("c_int_zero", "int", "    return ZERO\n"),
        ("c_float", "float", "    return RATE\n"),
        ("c_float_neg", "float", "    return NEG_RATE\n"),
        ("c_bool_true", "bool", "    return ON\n"),
        ("c_bool_false", "bool", "    return OFF\n"),
        // A BARE `NAME = 11` (no annotation) is a const too.
        ("c_bare", "int", "    return BARE\n"),
        // ── arithmetic, both operand orders ───────────────────────────────────
        ("c_int_mul", "int", "    return 6 * SCALE\n"),
        ("c_int_mul_rev", "int", "    return SCALE * 6\n"),
        ("c_int_sub", "int", "    return SCALE - LIMIT\n"),
        ("c_int_neg_add", "int", "    return NEG + 10\n"),
        ("c_int_floordiv", "int", "    return SCALE // LIMIT\n"),
        ("c_int_mod", "int", "    return SCALE % LIMIT\n"),
        ("c_float_add", "float", "    return RATE + 1.0\n"),
        ("c_float_mul", "float", "    return RATE * NEG_RATE\n"),
        // ── two consts in one expression (both globals live at once) ─────────
        ("c_two_consts", "int", "    return SCALE + LIMIT * 2\n"),
        // ── a const combined with a LOCAL ─────────────────────────────────────
        (
            "c_with_local",
            "int",
            "    n: int = 5\n    return n * SCALE\n",
        ),
        // ── comparison / boolean positions ────────────────────────────────────
        ("c_cmp", "bool", "    return SCALE > LIMIT\n"),
        ("c_cmp_float", "bool", "    return RATE < 3.0\n"),
        ("c_bool_and", "bool", "    return ON and not OFF\n"),
        // ── a const as an `if` CONDITION and inside a branch ─────────────────
        (
            "c_in_if",
            "int",
            "    if ON:\n        return SCALE\n    return 0\n",
        ),
        (
            "c_in_if_cmp",
            "int",
            "    if SCALE > LIMIT:\n        return 1\n    return 2\n",
        ),
        // ── a const as an if-EXPRESSION operand ───────────────────────────────
        ("c_in_ifexpr", "int", "    return SCALE if ON else LIMIT\n"),
        // ── a const as a LOOP BOUND — read on every iteration ────────────────
        (
            "c_loop_bound",
            "int",
            "    t: int = 0\n    i: int = 0\n    while i < LIMIT:\n        t = t + SCALE\n        i = i + 1\n    return t\n",
        ),
        // ── a const initialising a LOCAL, which is then mutated ──────────────
        (
            "c_seeds_local",
            "int",
            "    t: int = SCALE\n    t = t + 1\n    return t\n",
        ),
        // ── a const as a LIST INDEX and inside a list literal ────────────────
        (
            "c_list_index",
            "int",
            "    xs: list[int] = [10, 20, 30, 40]\n    return xs[LIMIT]\n",
        ),
        (
            "c_in_list_lit",
            "int",
            "    xs: list[int] = [SCALE, LIMIT]\n    return xs[0] + xs[1]\n",
        ),
        // ── a const through the NumBuiltin ops (incl. this slice's round) ────
        ("c_abs", "int", "    return abs(NEG)\n"),
        ("c_min_max", "int", "    return min(SCALE, LIMIT) + max(SCALE, LIMIT)\n"),
        ("c_round", "int", "    return round(RATE)\n"),
        ("c_round_neg", "int", "    return round(NEG_RATE)\n"),
        // ── the f-string / str(int) thread over a const ──────────────────────
        (
            "c_str_concat_eq",
            "bool",
            "    return \"s=\" + str(SCALE) == \"s=7\"\n",
        ),
    ]
}

/// The corpus source: the const header, the observable exports, and a
/// param-boundary pair (a const is passed as an ARGUMENT and read in the callee,
/// which also reads the const directly — proving the global is module-wide, not
/// re-emitted per function).
fn corpus_source() -> String {
    let mut src = String::from(CONST_HEADER);
    src.push('\n');
    for (name, ret, body) in probes() {
        src.push_str(&format!("def {name}() -> {ret}:\n{body}\n"));
    }
    src.push_str("def scaled(n: int) -> int:\n    return n * SCALE\n");
    src.push_str("def c_across_call() -> int:\n    return scaled(LIMIT)\n");
    src
}

fn observable_names() -> Vec<String> {
    let mut names: Vec<String> = probes().iter().map(|(n, _, _)| n.to_string()).collect();
    names.push("c_across_call".to_string());
    names
}

// ---- CONSTRUCT assertions (hold with or without WABT) ------------------------

/// Each const emits EXACTLY ONE immutable global, whatever the reference count —
/// and reads go through `global.get`, never a per-function copy.
#[test]
fn each_const_emits_one_immutable_global() {
    let wat = emit(&corpus_source()).expect("const corpus must lower");
    for (name, decl) in [
        ("SCALE", "(global $SCALE i64 (i64.const 7))"),
        ("LIMIT", "(global $LIMIT i64 (i64.const 3))"),
        ("NEG", "(global $NEG i64 (i64.const -4))"),
        ("ZERO", "(global $ZERO i64 (i64.const 0))"),
        ("RATE", "(global $RATE f64 (f64.const 2.5))"),
        ("NEG_RATE", "(global $NEG_RATE f64 (f64.const -1.5))"),
        ("ON", "(global $ON i32 (i32.const 1))"),
        ("OFF", "(global $OFF i32 (i32.const 0))"),
        ("BARE", "(global $BARE i64 (i64.const 11))"),
    ] {
        assert_eq!(
            wat.matches(decl).count(),
            1,
            "const `{name}` must be declared EXACTLY once as {decl}:\n{wat}"
        );
        // Immutability is the whole encoding claim: a `(mut …)` global would
        // admit a store the Python semantics forbid.
        assert!(
            !wat.contains(&format!("(global ${name} (mut")),
            "const `{name}` must be IMMUTABLE — a `(mut …)` global admits a store \
             Python's module constant does not:\n{wat}"
        );
        assert!(
            !wat.contains(&format!("global.set ${name}")),
            "nothing may store to const `{name}`:\n{wat}"
        );
    }
    assert!(
        wat.contains("global.get $SCALE"),
        "a const READ must be a global.get, not an inlined literal copy:\n{wat}"
    );
}

/// A module const does NOT force the heap/memory machinery on by itself — a
/// scalar-only module with consts stays a pure scalar module.
#[test]
fn scalar_const_module_needs_no_heap() {
    let wat = emit("K: int = 3\n\ndef f() -> int:\n    return K + 1\n")
        .expect("scalar const module must lower");
    assert!(
        wat.contains("(global $K i64 (i64.const 3))"),
        "the const must be declared:\n{wat}"
    );
    assert!(
        !wat.contains("$__heap_ptr"),
        "a scalar const allocates nothing — no bump heap should be armed:\n{wat}"
    );
}

/// The int consts reach the f-string int-classifier, so an interpolated const
/// materialises through `str(int)` rather than refusing in a string position.
/// (Same touchpoint class as PMAT-1342's `min`/`max` miss.)
#[test]
fn int_const_in_fstring_wraps_via_str_int() {
    let wat = emit("MAX: int = 42\n\ndef g() -> str:\n    return f\"m={MAX}\"\n")
        .expect("f-string of an int const must lower");
    assert!(
        wat.contains("global.get $MAX") && wat.contains("call $__wasm_int_to_str"),
        "an f-string interpolating an int const must read the global THEN \
         str(int)-materialise:\n{wat}"
    );
    let bare = emit("MAX: int = 42\n\ndef g() -> str:\n    return f\"{MAX}\"\n")
        .expect("bare f-string of an int const must lower");
    assert!(
        bare.contains("global.get $MAX") && bare.contains("call $__wasm_int_to_str"),
        "a BARE f-string of an int const must also str(int)-materialise:\n{bare}"
    );
}

// ---- honest refusals ----------------------------------------------------------

/// The immutability claim rests on the frontend: BOTH shadowing shapes (a
/// parameter and a function-local assignment) are refused upstream, so no
/// emitted body can ever want to write the global.
#[test]
fn shadowing_a_const_is_refused_upstream() {
    for (label, src) in [
        (
            "param shadows const",
            "K: int = 3\n\ndef f(K: int) -> int:\n    return K\n",
        ),
        (
            "local assign shadows const",
            "K: int = 3\n\ndef f() -> int:\n    K = 5\n    return K\n",
        ),
    ] {
        let err = match emit(src) {
            Err(e) => e,
            Ok(wat) => panic!("{label} must be refused but lowered:\n{wat}"),
        };
        assert!(
            err.contains("shadows the module-level constant"),
            "{label} must refuse naming the shadowed constant, got: {err}"
        );
    }
}

/// A `str` (or collection) module constant is refused — by the FRONTEND, which
/// never builds an `Item::Const` for it. Pinned from this side so the boundary
/// cannot drift into a silent emit.
#[test]
fn non_scalar_module_constant_refuses() {
    for (label, src) in [
        (
            "str const",
            "MSG = \"hi\"\n\ndef f() -> int:\n    return 1\n",
        ),
        (
            "list const",
            "XS = [1, 2, 3]\n\ndef f() -> int:\n    return 1\n",
        ),
    ] {
        let err = match emit(src) {
            Err(e) => e,
            Ok(wat) => panic!("{label} must be refused but lowered:\n{wat}"),
        };
        assert!(
            err.contains("module-level assignment"),
            "{label} must refuse as an unsupported module-level assignment, got: {err}"
        );
    }
}

// ---- WABT harness -------------------------------------------------------------

/// Parse a `name() => <ty>:<v>` interp line as an `f64`. `wasm-interp` prints
/// integer exports UNSIGNED, so a negative `i64` must be reinterpreted at its
/// declared width (`NEG = -4` prints as `18446744073709551612`).
fn parse_export_f64(stdout: &str, name: &str) -> f64 {
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&format!("{name}() => ")))
        .unwrap_or_else(|| panic!("no `{name}` export in interp output:\n{stdout}"));
    let (ty, raw) = line
        .rsplit_once(" => ")
        .and_then(|(_, v)| v.split_once(':'))
        .unwrap_or_else(|| panic!("malformed export line {line:?}"));
    let raw = raw.trim();
    match ty.trim() {
        "i64" => raw
            .parse::<u64>()
            .map(|u| u as i64 as f64)
            .unwrap_or_else(|_| panic!("parse i64 for {name} from {line:?}")),
        "i32" => raw
            .parse::<u32>()
            .map(|u| u as i32 as f64)
            .unwrap_or_else(|_| panic!("parse i32 for {name} from {line:?}")),
        "f64" | "f32" => raw
            .parse::<f64>()
            .unwrap_or_else(|_| panic!("parse float for {name} from {line:?}")),
        other => panic!("unexpected export type {other:?} in {line:?}"),
    }
}

fn assemble_and_run(wat: &str) -> (String, bool) {
    let dir = std::env::temp_dir().join(format!("xpile-wasm-modconst-{}", std::process::id()));
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

/// Execute the IDENTICAL corpus source in live python3 — the differential truth.
fn python_truth(src: &str) -> Option<Vec<(String, f64)>> {
    let names = observable_names();
    let driver =
        format!("{src}\nprint(';'.join(f'{{n}}={{float(globals()[n]())}}' for n in {names:?}))\n");
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
                (k.to_string(), v.parse::<f64>().expect("float"))
            })
            .collect(),
    )
}

// ---- EXECUTED witness (gated on WABT + python3) --------------------------------

#[test]
fn module_consts_execute_in_wasm_and_match_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("const corpus must lower");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1366: WABT (wat2wasm / wasm-interp) absent — witness asserted at \
             emit level only (the executed leg runs every export and value-matches \
             live python3 on the identical source when WABT is present)."
        );
        return;
    }

    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1366: python3 absent — witness asserted at emit level only");
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
        let got = parse_export_f64(&stdout, name);
        assert_eq!(
            got, *expected,
            "const export `{name}`: wasm {got} != cpython {expected}\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1366: {} module-const observables (int / float / bool / bare and \
         negative literals, read bare, in arithmetic, in comparisons, as an `if` \
         condition, as a loop bound, as a list index, inside a list literal, \
         through abs / min / max / round, in an f-string, and passed across a \
         call) all == live python3.",
        truth.len()
    );
}
