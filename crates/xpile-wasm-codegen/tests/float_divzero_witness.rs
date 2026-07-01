//! PMAT-1002 — regression witness for Python `ZeroDivisionError` on float
//! division, found by the third adversarial CPython-differential sweep.
//!
//! CPython raises `ZeroDivisionError` for `x / 0.0`; a bare WASM `f64.div`
//! silently returns an IEEE value (`1.0/0.0` → +inf, `0.0/0.0` → NaN,
//! `-1.0/0.0` → -inf). The fix guards the divisor against `0.0` before the
//! divide and TRAPS (`unreachable`) — the ZeroDivisionError analogue, matching
//! the lane's fail-loud discipline (and integer `//0`, which already traps via
//! `i64.div_s`). `-0.0 == 0.0` in IEEE, so both signed zeros are caught.
//!
//! This witness drives zero-arg float kernels through the production
//! `emit_module`, runs them in WABT, and asserts a zero divisor TRAPS while
//! ordinary division produces the correct value. Gated on
//! `wasm_runtime_available()`.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, FloatOp, Function, Item, Module, SourceLang, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

fn fdiv(a: f64, b: f64) -> Expr {
    Expr::FloatBinOp {
        op: FloatOp::Div,
        lhs: Box::new(Expr::LitFloat(a)),
        rhs: Box::new(Expr::LitFloat(b)),
    }
}

fn float_fn(name: &str, tail: Expr) -> Module {
    Module {
        name: name.into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(Function {
            name: name.into(),
            params: vec![],
            return_type: Type::F64,
            body: Block {
                stmts: vec![],
                trailing_return: tail,
            },
        })],
        ffi_boundaries: Vec::new(),
    }
}

/// Run a zero-arg f64 kernel; `Ok(f64)` value or `Err(())` on a trap.
fn run(name: &str, wat: &str) -> Result<f64, ()> {
    let dir = std::env::temp_dir().join(format!("xpile-fdz-{}-{}", name, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wp = dir.join("p.wat");
    let bp = dir.join("p.wasm");
    std::fs::write(&wp, wat).unwrap();
    let a = Command::new("wat2wasm")
        .arg(&wp)
        .arg("-o")
        .arg(&bp)
        .output()
        .unwrap();
    assert!(
        a.status.success(),
        "wat2wasm:\n{}\n{wat}",
        String::from_utf8_lossy(&a.stderr)
    );
    let r = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&bp)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&r.stdout);
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&format!("{name}()")))
        .unwrap_or("");
    if line.contains("unreachable executed") {
        return Err(());
    }
    let raw = line.rsplit_once("f64:").expect("f64 result").1.trim();
    Ok(raw
        .parse::<f64>()
        .unwrap_or_else(|_| panic!("parse f64 from {line:?}")))
}

#[test]
fn float_div_emits_zero_divisor_guard() {
    // CONSTRUCT (no WABT): float `/` emits the divisor==0 guard + trap.
    let wat = emit_module(&float_fn("d", fdiv(1.0, 2.0))).expect("float div lowers");
    assert!(
        wat.contains("f64.eq") && wat.contains("unreachable") && wat.contains("f64.div"),
        "float `/` must guard the divisor against 0.0 (f64.eq + unreachable) then f64.div:\n{wat}"
    );
    assert!(
        wat.contains("(local $__wasm_fdiv_d f64)"),
        "the f64 divisor scratch must be declared:\n{wat}"
    );
}

#[test]
fn float_div_by_zero_traps_else_divides() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1002: skipping float div-by-zero witness — WABT absent");
        return;
    }
    // Zero divisor → TRAP (ZeroDivisionError analogue), for every sign of dividend.
    for (name, num) in [("z1", 1.0), ("z2", 0.0), ("z3", -1.0)] {
        let m = float_fn(name, fdiv(num, 0.0));
        assert_eq!(
            run(name, &emit_module(&m).unwrap()),
            Err(()),
            "{num} / 0.0 must TRAP (CPython raises ZeroDivisionError)"
        );
    }
    // Ordinary division → correct value (unregressed).
    for (name, a, b, want) in [
        ("q1", 7.0, 2.0, 3.5),
        ("q2", -6.0, 2.0, -3.0),
        ("q3", 9.0, 4.0, 2.25),
    ] {
        let got = run(name, &emit_module(&float_fn(name, fdiv(a, b))).unwrap())
            .unwrap_or_else(|_| panic!("{a}/{b} should not trap"));
        assert!(
            (got - want).abs() < 1e-9,
            "{a}/{b}: WASM {got} but CPython {want}"
        );
    }
    eprintln!(
        "PMAT-1002: float div-by-zero witness PASSED — x/0.0 TRAPS (== CPython \
         ZeroDivisionError) for +/-/zero dividends; ordinary float division \
         value-matches CPython. Executed in WABT."
    );
}
