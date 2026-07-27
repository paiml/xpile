//! PMAT-1378 — the WASM emitter used to exit 0 with a module that `wat2wasm`
//! REJECTS. This witness pins the refusals that close that, and the
//! neighbouring shapes that must keep working.
//!
//! ## The defect
//!
//! `tests/wasm_contract_surface.rs` states the lane's claim in its own header:
//! an emitted program "assembles under `wat2wasm`, and executes under
//! `wasm-interp`". Five ordinary source shapes falsified it. Each transpiled
//! with **exit 0**, printed WAT, and then failed assembly — or, in the two
//! mixed cases, assembled and disagreed with Python about what the name means:
//!
//! | source | before PMAT-1378 |
//! |---|---|
//! | `def g()` twice | exit 0 → `redefinition of function "$g"` |
//! | `N: int = 1` / `N: int = 2` | exit 0 → `redefinition of global "$N"` |
//! | bare `N = 1` / `N = 2` | exit 0 → `redefinition of global "$N"` |
//! | `__heap_ptr = 5` + any heap use | exit 0 → `redefinition of global "$__heap_ptr"` |
//! | `def __wasm_floordiv_i64()` + any `//` | exit 0 → `redefinition of function` |
//!
//! ## Why the two "mixed" spellings are the worst of the set
//!
//! `def g(): return 1` followed by `g: int = 5` puts a `(func $g)` and a
//! `(global $g)` into two DIFFERENT WAT index spaces. `wat2wasm` is perfectly
//! happy. The module then exports `g` as a callable returning `1`, while
//! Python's `g` is the integer `5` and is not callable at all — a silent wrong
//! answer with a clean assembly, the failure shape this repo ranks above all
//! others. Reversed (`g: int = 5` then `def g()`), Python keeps the FUNCTION
//! while a body reading `g` resolves the global and yields `5`.
//!
//! ## And why the reserved-name shape is the most treacherous
//!
//! Whether `__heap_ptr = 5` breaks depended on which HELPERS the module
//! happened to pull in — it emits fine on its own, and adding an unrelated
//! list literal three lines later breaks assembly. The refusal is therefore
//! UNCONDITIONAL (asserted both ways below): the name is refused whether or
//! not this particular module also touches the heap.
//!
//! ## What must NOT be refused
//!
//! A PARAMETER or a function-LOCAL named `__heap_ptr` is untouched: WAT locals
//! and globals are separate index spaces, so nothing collides. Both are
//! EXECUTED here against live python3, so the fix is pinned as narrow rather
//! than asserted to be.

use std::path::Path;
use std::process::Command;

use depyler_frontend::PythonFrontend;
use xpile_frontend::{AliasSemantics, Frontend, LoweringProfile};
use xpile_meta_hir::Module;
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- pipeline helpers (the CLI's `--target wasm` path) ----------------------

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

fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

/// Assert `src` is refused **by the BACKEND**, not by the frontend and not by a
/// panic. Pinning the STAGE matters: a refusal that silently migrated upstream
/// into the frontend would still make this file green while leaving
/// `emit_module` free to emit an unassemblable module for any OTHER caller
/// (the library facade, the hybrid lane, a future frontend). So the frontend
/// leg is asserted to SUCCEED first.
fn assert_backend_refuses(label: &str, src: &str, needle: &str) {
    let module = lower(src).unwrap_or_else(|e| {
        panic!(
            "{label}: the FRONTEND must still accept this source — it is legal \
             Python. If lowering now refuses it, the PMAT-1378 refusal has \
             migrated upstream and `emit_module` is unguarded again.\n{e}"
        )
    });
    let err = match emit_module(&module) {
        Err(e) => e.to_string(),
        Ok(wat) => panic!(
            "{label}: emit_module SUCCEEDED. Before PMAT-1378 this is exactly what \
             happened — exit 0 with WAT that `wat2wasm` then rejects (or that \
             assembles and disagrees with Python).\n---WAT---\n{wat}"
        ),
    };
    assert!(
        err.contains(needle),
        "{label}: refused, but not for the PMAT-1378 reason. Expected the message \
         to contain {needle:?} so the diagnostic points at the real cause.\ngot: {err}"
    );
}

// ---- the refused corpus ------------------------------------------------------

/// `(label, source)` for every shape that must refuse because a TOP-LEVEL name
/// is bound twice. Python rebinds — last binding wins; WASM has no rebinding,
/// so both definitions emit.
const DUPLICATE_BINDINGS: &[(&str, &str)] = &[
    (
        "two `def`s of the same name",
        "def g() -> int:\n    return 1\n\ndef g() -> int:\n    return 2\n",
    ),
    (
        "two annotated consts of the same name",
        "N: int = 1\nN: int = 2\n\ndef f() -> int:\n    return N\n",
    ),
    (
        "two bare consts of the same name",
        "N = 1\nN = 2\n\ndef f() -> int:\n    return N\n",
    ),
    (
        "two consts of DIFFERENT kinds",
        "N: int = 1\nN: float = 2.5\n\ndef f() -> float:\n    return N\n",
    ),
    // The two that ASSEMBLED and then lied — see the module header.
    (
        "`def g` then `g: int` (assembled; export disagreed with Python)",
        "def g() -> int:\n    return 1\n\ng: int = 5\n\ndef f() -> int:\n    return g\n",
    ),
    (
        "`g: int` then `def g` (assembled; body read the global, Python has the fn)",
        "g: int = 5\n\ndef g() -> int:\n    return 1\n\ndef f() -> int:\n    return g\n",
    ),
];

/// `(label, source)` for every shape that must refuse because a top-level name
/// lands inside the namespace the emitted runtime reserves for itself.
const RESERVED_NAMES: &[(&str, &str)] = &[
    // WITH a heap user present — the shape that actually broke assembly.
    (
        "`__heap_ptr` const beside a list literal",
        "__heap_ptr: int = 5\n\ndef f() -> int:\n    xs: list[int] = [1, 2, 3]\n    return xs[0] + __heap_ptr\n",
    ),
    // WITHOUT one — refused just the same, so the boundary is not
    // "whichever helpers this module happens to need".
    (
        "`__heap_ptr` const with NO heap user",
        "__heap_ptr: int = 5\n\ndef f() -> int:\n    return __heap_ptr\n",
    ),
    (
        "`__alloc` const",
        "__alloc: int = 5\n\ndef f() -> int:\n    return __alloc\n",
    ),
    (
        "`def __heap_ptr()`",
        "def __heap_ptr() -> int:\n    return 1\n\ndef f() -> int:\n    return 2\n",
    ),
    (
        "`def __wasm_floordiv_i64()` beside a `//`",
        "def __wasm_floordiv_i64() -> int:\n    return 1\n\ndef f() -> int:\n    a: int = 7\n    b: int = 2\n    return a // b\n",
    ),
    (
        "`def __wasm_anything()` — the prefix is reserved WHOLESALE",
        "def __wasm_not_a_real_helper() -> int:\n    return 1\n",
    ),
];

#[test]
fn a_duplicate_top_level_binding_refuses() {
    for (label, src) in DUPLICATE_BINDINGS {
        assert_backend_refuses(label, src, "bound more than once");
    }
    eprintln!(
        "PMAT-1378: {} duplicate-top-level-binding shapes refuse at the backend \
         (was: exit 0 + `redefinition of …`, or an assembling module whose export \
         disagreed with CPython).",
        DUPLICATE_BINDINGS.len()
    );
}

#[test]
fn a_reserved_runtime_name_refuses() {
    for (label, src) in RESERVED_NAMES {
        assert_backend_refuses(label, src, "RESERVED by the WASM runtime");
    }
    eprintln!(
        "PMAT-1378: {} reserved-namespace shapes refuse at the backend, \
         UNCONDITIONALLY — not only when the module happens to pull in the \
         colliding helper.",
        RESERVED_NAMES.len()
    );
}

#[test]
fn the_refusal_message_tells_the_caller_what_to_do() {
    // A refusal that does not say "rename it, and locals are fine" sends the
    // reader hunting through the emitter. Pin the actionable half.
    let module = lower("__heap_ptr: int = 5\n\ndef f() -> int:\n    return __heap_ptr\n")
        .expect("frontend accepts");
    let err = emit_module(&module).expect_err("must refuse").to_string();
    for needle in ["$__heap_ptr", "$__wasm_", "Rename", "separate index space"] {
        assert!(
            err.contains(needle),
            "the reserved-name diagnostic must contain {needle:?}; got: {err}"
        );
    }
    let module =
        lower("N = 1\nN = 2\n\ndef f() -> int:\n    return N\n").expect("frontend accepts");
    let err = emit_module(&module).expect_err("must refuse").to_string();
    for needle in ["Python REBINDS", "separate index spaces"] {
        assert!(
            err.contains(needle),
            "the duplicate-binding diagnostic must contain {needle:?}; got: {err}"
        );
    }
}

// ---- the NOT-refused neighbours ------------------------------------------------

/// A reserved NAME in a position that has no collision: WAT locals and params
/// are indexed separately from globals, so these are legal and must keep
/// working. Each returns 5 so the executed leg below has one expectation.
const RESERVED_NAME_IN_A_LOCAL_POSITION: &[(&str, &str)] = &[
    (
        "local",
        "def f() -> int:\n    __heap_ptr: int = 4\n    xs: list[int] = [1]\n    return xs[0] + __heap_ptr\n",
    ),
    (
        "param",
        "def g(__heap_ptr: int) -> int:\n    xs: list[int] = [1]\n    return xs[0] + __heap_ptr\n\ndef f() -> int:\n    return g(4)\n",
    ),
    (
        "loop variable",
        "def f() -> int:\n    __wasm_i: int = 0\n    for __heap_ptr in range(3):\n        __wasm_i = __wasm_i + __heap_ptr\n    return __wasm_i + 2\n",
    ),
];

#[test]
fn a_reserved_name_in_a_local_position_is_not_refused() {
    for (label, src) in RESERVED_NAME_IN_A_LOCAL_POSITION {
        let wat = emit(src).unwrap_or_else(|e| {
            panic!(
                "{label}: OVER-REFUSAL. A local/param/loop-variable named \
                 `__heap_ptr` collides with nothing — WAT indexes locals \
                 separately from globals. PMAT-1378 must stay narrow.\n{e}"
            )
        });
        // The property that matters: whatever the name is used for locally,
        // the module defines the `$__heap_ptr` GLOBAL at most once — the
        // runtime's own, and only when this module needs a heap at all.
        let globals = wat
            .lines()
            .filter(|l| l.trim_start().starts_with("(global $__heap_ptr"))
            .count();
        assert!(
            globals <= 1,
            "{label}: `$__heap_ptr` defined as a global {globals} times\n{wat}"
        );
        assert!(
            wat.contains("$__heap_ptr"),
            "{label}: the local/param should still carry its source name\n{wat}"
        );
    }
}

/// Distinct top-level names across every binder kind — the ordinary case, which
/// must be entirely unaffected.
const DISTINCT_TOP_LEVEL: &str = "\
SCALE: int = 7
LIMIT = 3

def half(n: int) -> int:
    return n // 2

def total() -> int:
    return half(SCALE) + LIMIT
";

#[test]
fn distinct_top_level_names_still_emit() {
    let wat = emit(DISTINCT_TOP_LEVEL).expect("the ordinary case must still emit");
    for needle in [
        "(global $SCALE",
        "(global $LIMIT",
        "(func $half",
        "(func $total",
        "(func $__wasm_floordiv_i64",
    ] {
        assert!(wat.contains(needle), "missing {needle} in:\n{wat}");
    }
}

#[test]
fn a_struct_beside_a_function_of_a_different_name_still_emits() {
    // `Item::Struct` participates in the uniqueness scan (a struct and a
    // function of the same name is still a Python rebinding), so check the
    // legal arrangement is untouched.
    let src = "\
class Point:
    x: int
    y: int

    def sum(self) -> int:
        return self.x + self.y

def build() -> int:
    p: Point = Point(2, 3)
    return p.sum()
";
    let wat = emit(src).expect("a struct + a distinctly-named fn must still emit");
    assert!(wat.contains("(func $Point.sum"), "method missing:\n{wat}");
    assert!(wat.contains("(func $build"), "free fn missing:\n{wat}");
}

// ---- WABT harness --------------------------------------------------------------

/// Per-CALL unique work dir — several tests here assemble, and a shared
/// per-process directory races when the harness runs them in parallel.
fn assemble(wat: &str, tag: &str) -> Result<std::path::PathBuf, String> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-idcollide-{}-{tag}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let wat_path = dir.join("prog.wat");
    let wasm_path = dir.join("prog.wasm");
    std::fs::write(&wat_path, wat).map_err(|e| e.to_string())?;
    let out = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(wasm_path)
}

/// `wasm-interp` prints integer exports UNSIGNED — reinterpret at the declared
/// width so a negative `i64` does not read as ~1.8e19.
fn export_i64(stdout: &str, name: &str) -> i64 {
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&format!("{name}() => ")))
        .unwrap_or_else(|| panic!("no `{name}` export in:\n{stdout}"));
    let raw = line.rsplit(':').next().expect("value").trim();
    raw.parse::<u64>().expect("u64") as i64
}

fn run_all_exports(wasm: &std::path::Path) -> String {
    let out = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(wasm)
        .output()
        .expect("spawn wasm-interp");
    assert!(
        out.status.success(),
        "wasm-interp failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn python_value(src: &str, fn_name: &str) -> Option<i64> {
    let driver = format!("{src}\nprint({fn_name}())\n");
    let out = Command::new("python3")
        .arg("-c")
        .arg(&driver)
        .output()
        .ok()?;
    if !out.status.success() {
        panic!(
            "python3 rejected the witness source:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

// ---- EXECUTED legs (gated on WABT) ---------------------------------------------

#[test]
fn a_reserved_name_in_a_local_position_executes_and_matches_cpython() {
    let emitted: Vec<(&str, String)> = RESERVED_NAME_IN_A_LOCAL_POSITION
        .iter()
        .map(|(label, src)| (*label, emit(src).expect("must emit")))
        .collect();
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1378: WABT absent — the local/param/loop-variable neighbours \
             asserted at emit level only."
        );
        return;
    }
    for ((label, src), (_, wat)) in RESERVED_NAME_IN_A_LOCAL_POSITION.iter().zip(&emitted) {
        let wasm = assemble(wat, label).unwrap_or_else(|e| {
            panic!("{label}: wat2wasm rejected the module — {e}\n---WAT---\n{wat}")
        });
        let got = export_i64(&run_all_exports(&wasm), "f");
        let want = python_value(src, "f").expect("python value");
        assert_eq!(
            got, want,
            "{label}: wasm f()={got} != cpython f()={want}. A local named \
             `__heap_ptr` must NOT be aliased onto the runtime's heap cursor."
        );
    }
    eprintln!(
        "PMAT-1378: {} reserved-name-in-a-local-position modules assemble, run \
         and equal live python3.",
        RESERVED_NAME_IN_A_LOCAL_POSITION.len()
    );
}

#[test]
fn the_ordinary_distinct_name_module_still_assembles_and_runs() {
    let wat = emit(DISTINCT_TOP_LEVEL).expect("must emit");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1378: WABT absent — ordinary-case leg asserted at emit level only.");
        return;
    }
    let wasm = assemble(&wat, "distinct")
        .unwrap_or_else(|e| panic!("wat2wasm rejected the ORDINARY case — {e}\n---WAT---\n{wat}"));
    let got = export_i64(&run_all_exports(&wasm), "total");
    let want = python_value(DISTINCT_TOP_LEVEL, "total").expect("python value");
    assert_eq!(got, want, "wasm total()={got} != cpython total()={want}");
}

#[test]
fn every_module_this_backend_still_accepts_here_actually_assembles() {
    // The claim PMAT-1378 restores, stated directly: if `emit_module` returns
    // `Ok`, `wat2wasm` accepts the bytes. Run it over every source in this
    // file — refused and accepted alike — so a future loosening of either
    // check is caught by the property, not only by the enumerated cases.
    let all: Vec<(&str, &str)> = DUPLICATE_BINDINGS
        .iter()
        .chain(RESERVED_NAMES.iter())
        .chain(RESERVED_NAME_IN_A_LOCAL_POSITION.iter())
        .copied()
        .chain(std::iter::once(("ordinary", DISTINCT_TOP_LEVEL)))
        .collect();
    let accepted: Vec<(&str, String)> = all
        .iter()
        .filter_map(|(label, src)| emit(src).ok().map(|wat| (*label, wat)))
        .collect();
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1378: WABT absent — `Ok(wat) => wat2wasm accepts it` asserted \
             at emit level only ({} of {} sources accepted).",
            accepted.len(),
            all.len()
        );
        return;
    }
    for (label, wat) in &accepted {
        assemble(wat, "prop").unwrap_or_else(|e| {
            panic!(
                "`{label}`: emit_module returned Ok but wat2wasm REJECTED the \
                 module. That is precisely the PMAT-1378 defect — exit 0 with an \
                 artifact the next tool cannot consume.\nwat2wasm: {e}\n---WAT---\n{wat}"
            )
        });
    }
    eprintln!(
        "PMAT-1378: {}/{} sources accepted by emit_module; every accepted one \
         assembles under wat2wasm.",
        accepted.len(),
        all.len()
    );
}

#[test]
fn the_mixed_def_plus_const_shape_was_a_silent_divergence_not_a_loud_one() {
    // The red half of the two mixed cases, kept honest: they are refused now,
    // but the reason they rank above the others is that they used to ASSEMBLE.
    // Reconstruct that by hand — the exact two definitions the emitter used to
    // lay down — and prove wat2wasm accepts them, so the historical claim in
    // this file's header is a checked fact rather than a recollection.
    let hand_written = "(module\n  (global $g i64 (i64.const 5))\n  \
                        (func $g (result i64) i64.const 1)\n  \
                        (export \"g\" (func $g))\n)\n";
    if !wasm_runtime_available() {
        eprintln!("PMAT-1378: WABT absent — the historical-assembly leg skipped.");
        return;
    }
    assemble(hand_written, "mixed").expect(
        "a `(func $g)` beside a `(global $g)` is LEGAL WAT — that is why the \
         def+const shape assembled and then disagreed with CPython, and why it \
         has to be refused on semantic grounds rather than caught by the \
         structural belt",
    );
}
