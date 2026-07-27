//! PMAT-1395 — EXECUTED witness for the **C → WASM** scalar-ABI return path:
//! the decy (`.c`) source lane on `--target wasm`, value-matched against the
//! REAL C compiler executing the identical source.
//!
//! ## Why this file exists at all
//!
//! Every other file in this directory drives the WASM emitter from **Python**.
//! That is not a stylistic accident — it is the structural reason PMAT-1395
//! existed. Three of the emitter's scalar types (`Type::CUInt`, `Type::F32`,
//! and `Type::CLong`) are produced **only** by `decy-frontend`; no Python
//! annotation reaches them. So the C→WASM path had a live emitter, a live CLI
//! flag, and **zero witnesses**, and it shipped broken WAT for 5 distinct
//! source shapes across 3 ABI tokens without a single test noticing.
//!
//! ## The defect this pins
//!
//! `emit_function` had TWO return paths and only ONE of them was type-checked:
//! the early `Stmt::Return` arm went through `emit_expr_typed`, while the
//! TRAILING return — the path a normal single-`return` C function takes — went
//! through the bare, unchecked `emit_expr`. A literal therefore lowered at its
//! OWN natural WAT type instead of the function's DECLARED one:
//!
//! ```text
//! $ printf 'unsigned int f(void) { return 2; }\n' > u.c
//! $ xpile transpile u.c --target wasm > u.wat ; echo $?
//! 0                                    # <-- exit 0, "success"
//! $ wat2wasm u.wat -o u.wasm
//! u.wat:48:5: error: type mismatch in implicit return, expected [i32] but got [i64]
//!     i64.const 2
//! ```
//!
//! Five shapes were broken this way — `unsigned int` ← int literal, `float` ←
//! int literal, `float` ← float literal, `double` ← int literal, and every
//! negated form of those. `int` / `long` / `long long` were unaffected only
//! because `i64` happens to be the literal's natural type.
//!
//! ## The three assertions, and which one is load-bearing
//!
//!   1. `every_c_scalar_probe_that_emits_assembles` — the CLASS gate:
//!      `Ok(wat) ⟹ wat2wasm accepts it`, per probe. This is the one that would
//!      have caught PMAT-1395 on the day the CUInt lowering landed, and it will
//!      catch the next member of the family without anyone predicting it.
//!   2. `c_scalar_returns_match_the_real_c_compiler` — the VALUE gate. Assembly
//!      only proves the WAT is well-typed; it does not prove `-2` from an
//!      `unsigned int` is `4294967294` or that `5000000000` wraps to
//!      `705032704`. Ground truth is `cc` compiling and running the identical C.
//!   3. `pmat_1395_shapes_emit_at_the_declared_width` — a STATIC pin that never
//!      skips. Assertions 1 and 2 both need WABT / a C compiler; without this
//!      third one the whole file would go quietly green on a bare runner, which
//!      is the skip-as-green shape XPILE-WITNESS-002 exists to kill.
//!
//! Plus `refused_c_tokens_refuse_at_the_NAMED_stage`: the tokens OUTSIDE the
//! subset must refuse, and refuse at the stage claimed for them. Asserting a
//! bare `Err(_)` proves nothing about WHY (the PMAT-1350 lesson) — a frontend
//! refusal and a backend refusal are indistinguishable from the CLI, so a
//! mis-attributed rationale would pass green.
//!
//! ## Reading `wasm-interp` output correctly
//!
//! `wasm-interp` prints integer exports **UNSIGNED** at the declared width, so
//! `int i_neg(void) { return -5; }` reads back as `i64:18446744073709551611`.
//! The differential reinterprets per the DECLARED C type rather than parsing
//! the printed digits at face value; a witness that skipped that step would
//! report a false divergence on every negative value.

use std::path::Path;
use std::process::Command;

use decy_frontend::CFrontend;
use xpile_frontend::{AliasSemantics, Frontend, LoweringProfile};
use xpile_meta_hir::Module;
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- frontend lowering (the CLI's `.c --target wasm` path) ------------------

/// Exactly `lowering_profile_for(Target::Wasm)` in the CLI — if these drift the
/// witness stops testing the shipped path.
fn wasm_profile() -> LoweringProfile {
    LoweringProfile {
        alias_semantics: AliasSemantics::Reference,
        runtime_abort: true,
    }
}

fn lower(src: &str) -> Result<Module, String> {
    CFrontend
        .parse_and_lower_profiled(Path::new("witness.c"), src, wasm_profile())
        .map_err(|e| format!("frontend: {e}"))
}

/// FULL pipeline: C source → meta-HIR → WAT.
fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

// ---- the probe corpus -------------------------------------------------------

/// How a probe's value is compared across the two runtimes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Width {
    /// `unsigned int` — an `i32` carrier read as `u32`; C prints it with `%u`.
    U32,
    /// `int` / `long` / `long long` — an `i64` carrier read as `i64`; C prints
    /// it with `%lld`.
    I64,
    /// `float` / `double` — compared as the `%.6f` text both runtimes emit.
    Float,
}

struct Probe {
    name: &'static str,
    ret: &'static str,
    body: &'static str,
    width: Width,
}

/// Every probe is a ZERO-ARG C function so `wasm-interp --run-all-exports` can
/// call it. The corpus covers each decy scalar ABI token that lowers, and for
/// the three that were broken it covers the literal EDGES (zero, the unsigned
/// maximum, a value that must wrap, a negation) rather than just one happy
/// value — a single `return 2` per token would have greened on a fix that only
/// handled small positive constants.
fn probes() -> Vec<Probe> {
    vec![
        // ── `unsigned int` (Type::CUInt → i32). WAS BROKEN. ──────────────────
        Probe {
            name: "u_lit",
            ret: "unsigned int",
            body: "return 2;",
            width: Width::U32,
        },
        Probe {
            name: "u_zero",
            ret: "unsigned int",
            body: "return 0;",
            width: Width::U32,
        },
        // C17 6.3.1.3p2: conversion to an unsigned type is DEFINED-modular.
        // `-2` is `4294967294`, not a trap and not a clamp.
        Probe {
            name: "u_neg",
            ret: "unsigned int",
            body: "return -2;",
            width: Width::U32,
        },
        Probe {
            name: "u_max",
            ret: "unsigned int",
            body: "return 4294967295;",
            width: Width::U32,
        },
        // Out of `unsigned int` range in the source: wraps to 705032704.
        Probe {
            name: "u_wrap",
            ret: "unsigned int",
            body: "return 5000000000;",
            width: Width::U32,
        },
        // ── `float` (Type::F32). WAS BROKEN both ways: an int literal emitted
        //    `i64.const`, a double literal emitted `f64.const`. ──────────────
        Probe {
            name: "f_int_lit",
            ret: "float",
            body: "return 2;",
            width: Width::Float,
        },
        Probe {
            name: "f_flt_lit",
            ret: "float",
            body: "return 2.5;",
            width: Width::Float,
        },
        // 0.1 is not representable in binary — pins that the f64→f32 rounding
        // matches C's rather than being re-derived from the decimal text.
        Probe {
            name: "f_tenth",
            ret: "float",
            body: "return 0.1;",
            width: Width::Float,
        },
        Probe {
            name: "f_neg",
            ret: "float",
            body: "return -3.25;",
            width: Width::Float,
        },
        // ── `double` (Type::F64). An int literal WAS BROKEN. ────────────────
        Probe {
            name: "d_int_lit",
            ret: "double",
            body: "return 2;",
            width: Width::Float,
        },
        Probe {
            name: "d_flt_lit",
            ret: "double",
            body: "return 2.5;",
            width: Width::Float,
        },
        Probe {
            name: "d_tenth",
            ret: "double",
            body: "return 0.1;",
            width: Width::Float,
        },
        Probe {
            name: "d_neg",
            ret: "double",
            body: "return -3.25;",
            width: Width::Float,
        },
        // The EARLY-return path (`Stmt::Return`), not the trailing one — both
        // now funnel through `emit_scalar_ret`, and this is what proves the
        // early arm still converts rather than regressing to a type mismatch.
        Probe {
            name: "d_early",
            ret: "double",
            body: "double a = 1.0; if (a > 0.0) { return 2; } return 3.5;",
            width: Width::Float,
        },
        // ── `int` / `long` / `long long` (Type::I64 / Type::CLong). These were
        //    always correct — they are the CONTROL group. If the fix had been
        //    written as a blanket coercion they would break, so they stay. ───
        Probe {
            name: "i_lit",
            ret: "int",
            body: "return 2;",
            width: Width::I64,
        },
        Probe {
            name: "i_neg",
            ret: "int",
            body: "return -5;",
            width: Width::I64,
        },
        Probe {
            name: "l_lit",
            ret: "long",
            body: "return 1234567890123;",
            width: Width::I64,
        },
        Probe {
            name: "ll_neg",
            ret: "long long",
            body: "return -1234567890123;",
            width: Width::I64,
        },
    ]
}

/// The whole corpus as ONE C translation unit — one lowering, one `wat2wasm`,
/// one `wasm-interp` run, one `cc` run.
fn corpus_source() -> String {
    probes()
        .iter()
        .map(|p| format!("{} {}(void) {{ {} }}\n", p.ret, p.name, p.body))
        .collect()
}

/// A single probe as its own translation unit, so the per-probe class gate can
/// name the exact offender instead of failing the whole module.
fn probe_source(p: &Probe) -> String {
    format!("{} {}(void) {{ {} }}\n", p.ret, p.name, p.body)
}

// ---- runtimes ---------------------------------------------------------------

/// A work dir unique per PROCESS **and per call site** — two witnesses sharing
/// `prog.wat` race under libtest's thread pool (the multi-execution-path
/// gotcha this repo has already been bitten by).
fn work_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("xpile-wasm-cabi-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    dir
}

/// `Ok(stdout)` from `wasm-interp --run-all-exports`, `Err(message)` if either
/// WABT step rejected the module.
fn assemble_and_run(wat: &str, tag: &str) -> Result<String, String> {
    let dir = work_dir(tag);
    let wat_path = dir.join("prog.wat");
    let wasm_path = dir.join("prog.wasm");
    std::fs::write(&wat_path, wat).expect("write wat");

    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    if !assemble.status.success() {
        return Err(format!(
            "wat2wasm REJECTED the emitted WAT:\n{}",
            String::from_utf8_lossy(&assemble.stderr)
        ));
    }

    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    if !run.status.success() {
        return Err(format!("wasm-interp run failed:\n{stdout}"));
    }
    Ok(stdout)
}

/// The canonical text for one export, read out of `wasm-interp` stdout and
/// REINTERPRETED at the declared C width (the interpreter prints integers
/// unsigned regardless of signedness).
fn wasm_value(stdout: &str, p: &Probe) -> String {
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&format!("{}() => ", p.name)))
        .unwrap_or_else(|| panic!("no `{}` export in interp output:\n{stdout}", p.name));
    let raw = line
        .rsplit_once(':')
        .unwrap_or_else(|| panic!("malformed export line {line:?}"))
        .1
        .trim();
    match p.width {
        Width::U32 => {
            assert!(
                line.contains("=> i32:"),
                "`{}` is an `unsigned int` and must ride an i32 carrier: {line:?}",
                p.name
            );
            raw.parse::<u32>()
                .unwrap_or_else(|_| panic!("u32 value from {line:?}"))
                .to_string()
        }
        Width::I64 => {
            assert!(
                line.contains("=> i64:"),
                "`{}` must ride an i64 carrier: {line:?}",
                p.name
            );
            (raw.parse::<u64>()
                .unwrap_or_else(|_| panic!("u64 value from {line:?}")) as i64)
                .to_string()
        }
        // Both runtimes render a float as fixed-point with 6 fractional
        // digits, so the printed text IS the comparison.
        Width::Float => raw.to_string(),
    }
}

/// Compile and run the IDENTICAL C with the real C compiler, returning
/// `name -> value` in the same canonical text. `None` if no compiler is
/// invocable.
fn c_truth(src: &str) -> Option<Vec<(String, String)>> {
    let dir = work_dir("cc");
    let c_path = dir.join("truth.c");
    let bin_path = dir.join("truth");

    let mut driver = String::from("#include <stdio.h>\n");
    driver.push_str(src);
    driver.push_str("int main(void) {\n");
    for p in probes() {
        let (fmt, cast) = match p.width {
            Width::U32 => ("%u", "(unsigned int)"),
            Width::I64 => ("%lld", "(long long)"),
            Width::Float => ("%.6f", "(double)"),
        };
        driver.push_str(&format!(
            "    printf(\"{}={}\\n\", {}{}());\n",
            p.name, fmt, cast, p.name
        ));
    }
    driver.push_str("    return 0;\n}\n");
    std::fs::write(&c_path, &driver).expect("write truth.c");

    // `-w`: `u_wrap` is a deliberate out-of-range constant and warns.
    let build = Command::new("cc")
        .arg("-w")
        .arg("-o")
        .arg(&bin_path)
        .arg(&c_path)
        .output()
        .ok()?;
    if !build.status.success() {
        panic!(
            "the witness corpus must be valid C:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
    }
    let run = Command::new(&bin_path).output().ok()?;
    assert!(run.status.success(), "the compiled C corpus must run");
    Some(
        String::from_utf8_lossy(&run.stdout)
            .lines()
            .map(|l| {
                let (k, v) = l.split_once('=').expect("k=v");
                (k.to_string(), v.to_string())
            })
            .collect(),
    )
}

// ---- 1. the CLASS gate ------------------------------------------------------

#[test]
fn every_c_scalar_probe_that_emits_assembles() {
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1395: WABT absent — the assemble gate is skipped, but \
             `pmat_1395_shapes_emit_at_the_declared_width` below is STATIC and \
             still pins every shape this slice fixed."
        );
        return;
    }
    let mut emitted = 0usize;
    let mut refused: Vec<&str> = Vec::new();
    for p in probes() {
        match emit(&probe_source(&p)) {
            Ok(wat) => {
                emitted += 1;
                if let Err(why) = assemble_and_run(&wat, p.name) {
                    panic!(
                        "PMAT-1395 CLASS VIOLATION — `{} {}(void) {{ {} }}` emitted WAT \
                         at exit 0 that the real assembler rejects.\n{why}\n---WAT---\n{wat}",
                        p.ret, p.name, p.body
                    );
                }
            }
            Err(_) => refused.push(p.name),
        }
    }
    assert!(
        refused.is_empty(),
        "these probes stopped emitting: {refused:?}. Every one is inside the \
         declared C scalar subset, so a refusal here is a capability REGRESSION, \
         not a tightening — fix the emitter rather than deleting the probe"
    );
    assert_eq!(
        emitted,
        probes().len(),
        "non-vacuity: the class gate is only meaningful over probes that EMIT"
    );
    eprintln!("PMAT-1395: {emitted}/{emitted} C scalar-return probes assemble under wat2wasm");
}

// ---- 2. the VALUE gate ------------------------------------------------------

#[test]
fn c_scalar_returns_match_the_real_c_compiler() {
    let src = corpus_source();
    let wat = emit(&src).expect("the C scalar corpus must lower");

    if !wasm_runtime_available() {
        eprintln!("PMAT-1395: WABT absent — differential skipped (emit asserted above)");
        return;
    }
    let Some(truth) = c_truth(&src) else {
        eprintln!("PMAT-1395: no C compiler invocable — differential skipped");
        return;
    };
    assert_eq!(
        truth.len(),
        probes().len(),
        "the C driver must print one value per probe"
    );

    let stdout = assemble_and_run(&wat, "corpus").expect("the corpus module must assemble and run");
    for p in probes() {
        let expected = truth
            .iter()
            .find(|(k, _)| k == p.name)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| panic!("no C truth for `{}`", p.name));
        let got = wasm_value(&stdout, &p);
        assert_eq!(
            got, expected,
            "`{} {}(void) {{ {} }}`: wasm {got} != cc {expected}\n{stdout}",
            p.ret, p.name, p.body
        );
    }
    eprintln!(
        "PMAT-1395: {} C scalar returns (unsigned-int modular conversion incl. \
         -2 -> 4294967294 and 5000000000 -> 705032704, f32/f64 literal widening, \
         and the i64 control group) all == live cc.",
        truth.len()
    );
}

// ---- 3. the STATIC pin (never skips) ---------------------------------------

/// The body instructions of `(func $name …)`, without the header line.
fn func_body(wat: &str, name: &str) -> String {
    let start = wat
        .find(&format!("(func ${name} "))
        .unwrap_or_else(|| panic!("no `$name` function in:\n{wat}"));
    let rest = &wat[start..];
    let end = rest.find("\n  )").unwrap_or(rest.len());
    let block = &rest[..end];
    // Drop the header line — it legitimately names the result type.
    block
        .split_once('\n')
        .map(|(_, b)| b)
        .unwrap_or("")
        .to_string()
}

#[test]
fn pmat_1395_shapes_emit_at_the_declared_width() {
    // (probe name, the const instruction the DECLARED type requires)
    let expectations = [
        ("u_lit", "i32.const"),
        ("u_neg", "i32.const"),
        ("u_max", "i32.const"),
        ("u_wrap", "i32.const"),
        ("f_int_lit", "f32.const"),
        ("f_flt_lit", "f32.const"),
        ("f_tenth", "f32.const"),
        ("d_int_lit", "f64.const"),
        // The control group: `i64.const` is CORRECT here, and asserting it
        // keeps the fix from being a blanket rewrite of every literal.
        ("i_lit", "i64.const"),
        ("l_lit", "i64.const"),
    ];
    let all = probes();
    for (name, want) in expectations {
        let p = all
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("probe `{name}` was removed — update this pin"));
        let wat =
            emit(&probe_source(p)).unwrap_or_else(|e| panic!("`{name}` must lower to WAT: {e}"));
        let body = func_body(&wat, name);
        assert!(
            body.contains(want),
            "`{} {name}(void) {{ {} }}` must materialise its return value with \
             `{want}` (its DECLARED width). Body:\n{body}",
            p.ret,
            p.body
        );
        // The precise historical failure: an `i64.const` under a non-i64 result.
        if want != "i64.const" {
            assert!(
                !body.contains("i64.const"),
                "PMAT-1395 REGRESSION — `{name}` returns `{}` but its body still \
                 materialises an `i64.const`, which is exactly the WAT `wat2wasm` \
                 rejects with \"type mismatch in implicit return\". Body:\n{body}",
                p.ret
            );
        }
    }
    eprintln!(
        "PMAT-1395: {} declared-width pins hold (static — this test cannot skip)",
        expectations.len()
    );
}

// ---- 4. refusals, pinned to the stage that actually fires ------------------

#[test]
fn refused_c_tokens_refuse_at_the_named_stage() {
    // FRONTEND refusals — decy does not lift these value types at all, so the
    // WASM backend never sees them and its own posture is irrelevant.
    for (token, src) in [
        ("short", "short f(void) { return 2; }\n"),
        ("char", "char f(void) { return 2; }\n"),
    ] {
        let err = lower(src)
            .expect_err(&format!("`{token}` must refuse"))
            .to_string();
        assert!(
            err.starts_with("frontend:"),
            "`{token}` must refuse at the FRONTEND (decy lifts no such value \
             type); got: {err}"
        );
    }

    // BACKEND refusal — `unsigned long` IS lifted (it is `Type::CULong`), and
    // it is the WASM emitter that has no carrier for a 64-bit unsigned. Pinning
    // the stage is what keeps this row from greening on the wrong rationale.
    let src = "unsigned long f(void) { return 2; }\n";
    lower(src).expect("`unsigned long` must LOWER — decy models it as CULong");
    let err = emit(src).expect_err("`unsigned long` must refuse on the WASM lane");
    assert!(
        err.starts_with("wasm-codegen:"),
        "`unsigned long` must refuse at the BACKEND, not the frontend; got: {err}"
    );

    // A LOCAL of a narrow type still refuses honestly — this slice deliberately
    // converts literals only at the RETURN site, so `unsigned int a = 1;` is a
    // type mismatch rather than a silent i64 store. Widening that is capability
    // work, not a truth fix; pinning it here keeps the boundary visible.
    let err = emit("unsigned int f(void) { unsigned int a = 1; return a; }\n")
        .expect_err("a CUInt local bound to an int literal must still refuse");
    assert!(
        err.contains("type mismatch"),
        "the CUInt-local refusal must be the honest type mismatch; got: {err}"
    );
}
