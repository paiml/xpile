//! PMAT-1003 — witness for str-param `len(s)` on the WASM lane, testing the
//! ACTUAL node the Python frontend emits.
//!
//! The Python frontend lowers `len(s)` over a str to `StrMethod(CharCount)`
//! (Python counts Unicode code points, so a str len must NOT reuse `Expr::Len` =
//! byte length). The PMAT-986 witness built `Expr::Len` DIRECTLY, so it proved
//! the backend byte-count read but NOT the frontend→WASM str-len path a user
//! hits — which REFUSED end-to-end. This witness builds `StrMethod(CharCount)`
//! (the frontend's real node) + a str param, and confirms it lowers to a REAL
//! code-point count (the `$__wasm_str_charlen` helper since PMAT-1032 — exact
//! for non-ASCII too, see str_char_semantics_witness.rs) and executes to the
//! correct length; and that a non-len string method still refuses honestly.
//!
//! (The full frontend→CLI path was also verified by hand:
//! `xpile transpile 'def f(s:str)->int: return len(s)' --target wasm` now emits
//! + executes to len 5 over "hello", and a `code_sum` loop over `len(s)` to 198.)

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

fn charcount(recv: &str) -> Expr {
    Expr::StrMethod {
        recv: Box::new(Expr::Ident(recv.into())),
        op: StrMethodOp::CharCount,
        args: vec![],
    }
}

fn str_kernel(name: &str, tail: Expr) -> Module {
    Module {
        name: name.into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(Function {
            name: name.into(),
            params: vec![Param {
                name: "s".into(),
                ty: Type::Str,
                mutable: false,
            }],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: tail,
            },
        })],
        ffi_boundaries: Vec::new(),
    }
}

#[test]
fn str_len_charcount_lowers_to_header_read() {
    // The frontend's `StrMethod(CharCount)` over a str param now lowers (was a
    // generic refusal) — to the SAME byte-count header read `Expr::Len` uses.
    let wat = emit_module(&str_kernel("f", charcount("s"))).expect("len(s) via CharCount lowers");
    assert!(
        wat.contains("(func $f (param $s i32) (result i64)")
            && wat.contains("call $__wasm_str_charlen"),
        "str len counts code points via the PMAT-1032 charlen helper:\n{wat}"
    );
    // A non-len string method is still refused honestly.
    let err = emit_module(&str_kernel(
        "g",
        Expr::StrMethod {
            recv: Box::new(Expr::Ident("s".into())),
            op: StrMethodOp::Upper,
            args: vec![],
        },
    ))
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("string method") && err.contains("Upper"),
        "a non-len string method refuses honestly: {err}"
    );
}

#[test]
fn str_len_executes_over_a_driven_fixture() {
    let kernel = emit_module(&str_kernel("f", charcount("s"))).expect("lowers");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1003: skipping executed str-len witness — WABT absent");
        return;
    }
    // Splice a driver: preload "hello" (i32 count=5 @ 0, bytes @ 8), call f(0).
    let close = kernel.rfind(')').expect("closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel[..close]);
    wat.push_str("  (data (i32.const 0) \"\\05\\00\\00\\00\")\n");
    wat.push_str("  (data (i32.const 8) \"hello\")\n");
    wat.push_str("  (func (export \"run\") (result i64)\n    i32.const 0\n    call $f)\n)\n");

    let dir = std::env::temp_dir().join(format!("xpile-strlen-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wp = dir.join("p.wat");
    let bp = dir.join("p.wasm");
    std::fs::write(&wp, &wat).unwrap();
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
    assert!(
        stdout.contains("run() => i64:5"),
        "len(\"hello\") must execute to 5 (== CPython), got:\n{stdout}"
    );
    eprintln!(
        "PMAT-1003: str-len witness PASSED — the frontend's StrMethod(CharCount) \
         over a str param now lowers + executes to 5 for \"hello\" == CPython len \
         (ASCII: byte count == code-point count). The end-to-end str-len gap is closed."
    );
}
