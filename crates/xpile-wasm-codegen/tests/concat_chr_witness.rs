//! PMAT-998 — regression witness for the concat-of-string-RETURNING-operands
//! miscompile found by the adversarial CPython-differential sweep.
//!
//! Before the fix, a `Concat` whose operands were THEMSELVES string-returning
//! ops (`chr(n)`, `s[i]`) shared the single `$__wasm_str_dst` scratch local with
//! its own destination: evaluating each operand `local.set`'d `$__wasm_str_dst`,
//! CLOBBERING the concat destination, so `chr(65) + chr(66)` returned the 1-char
//! `"B"` (the last operand) instead of `"AB"`. The fix gives the concat a
//! DEDICATED `$__wasm_concat_dst` local that survives operand evaluation.
//!
//! This witness lowers `chr(65) + chr(66) == "AB"` (and the reversed / negated /
//! literal-mixed / 3-operand variants) through the production `emit_module`,
//! assembles + runs each in WABT, and asserts the executed bool VALUE-MATCHES
//! CPython. Gated on `wasm_runtime_available()` (clean skip without WABT).

use std::process::Command;

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, SourceLang, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

fn chr(n: i64) -> Expr {
    Expr::Chr {
        value: Box::new(Expr::LitInt(n)),
    }
}
fn concat(l: Expr, r: Expr) -> Expr {
    Expr::Concat {
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}
fn eq(l: Expr, r: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}
fn ne(l: Expr, r: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::NotEq,
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}
fn lit(s: &str) -> Expr {
    Expr::LitStr(s.into())
}

fn bool_fn(name: &str, tail: Expr) -> Item {
    Item::Function(Function {
        name: name.into(),
        params: vec![],
        return_type: Type::Bool,
        body: Block {
            stmts: vec![],
            trailing_return: tail,
        },
    })
}

/// `(name, meta-HIR bool expr, expected CPython bool)`.
fn cases() -> Vec<(&'static str, Expr, bool)> {
    vec![
        // chr + chr == "AB"  → True   (the headline bug: was "B" → False)
        ("ab", eq(concat(chr(65), chr(66)), lit("AB")), true),
        // reversed operand order of the compare
        ("ab_rev", eq(lit("AB"), concat(chr(65), chr(66))), true),
        // negated: chr+chr != "AB"  → False
        ("ab_ne", ne(concat(chr(65), chr(66)), lit("AB")), false),
        // literal + chr == "AB"  → True
        ("lit_chr", eq(concat(lit("A"), chr(66)), lit("AB")), true),
        // chr + literal == "AB"  → True
        ("chr_lit", eq(concat(chr(65), lit("B")), lit("AB")), true),
        // 3-operand chr concat == "XYZ"  → True
        (
            "xyz",
            eq(concat(concat(chr(88), chr(89)), chr(90)), lit("XYZ")),
            true,
        ),
        // chr + chr == "B" (the collapsed-to-last-char false match)  → False
        ("not_b", eq(concat(chr(65), chr(66)), lit("B")), false),
    ]
}

fn run_bool(name: &str, wat: &str) -> Option<bool> {
    if !wasm_runtime_available() {
        return None;
    }
    let dir =
        std::env::temp_dir().join(format!("xpile-concat-chr-{}-{}", name, std::process::id()));
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
        "wat2wasm failed for {name}:\n{}\n{wat}",
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
        .find(|l| l.starts_with(&format!("{name}() => ")))
        .unwrap_or_else(|| panic!("no `{name}` export:\n{stdout}"));
    Some(line.trim().ends_with("i32:1"))
}

#[test]
fn concat_dst_is_distinct_from_operand_scratch() {
    // CONSTRUCT (no WABT needed): the concat of two chr operands must declare the
    // DEDICATED concat-dst local AND still emit the str-dst local the chr
    // operands use — proving they are two different locals.
    let m = Module {
        name: "cc".into(),
        source_lang: SourceLang::Rust,
        items: vec![bool_fn("ab", eq(concat(chr(65), chr(66)), lit("AB")))],
        ffi_boundaries: Vec::new(),
    };
    let wat = emit_module(&m).expect("concat-of-chr lowers");
    assert!(
        wat.contains("(local $__wasm_concat_dst i32)"),
        "the concat must use a DEDICATED destination local:\n{wat}"
    );
    assert!(
        wat.contains("(local $__wasm_str_dst i32)"),
        "the chr operands still use the str-dst scratch (must be distinct):\n{wat}"
    );
}

#[test]
fn chr_concat_executes_and_matches_cpython() {
    for (name, expr, expected) in cases() {
        let m = Module {
            name: "cc".into(),
            source_lang: SourceLang::Rust,
            items: vec![bool_fn(name, expr)],
            ffi_boundaries: Vec::new(),
        };
        let wat = emit_module(&m).unwrap_or_else(|e| panic!("case {name} must lower: {e}"));
        match run_bool(name, &wat) {
            None => {
                eprintln!("PMAT-998: skipping executed concat-chr witness ({name}) — WABT absent");
                return;
            }
            Some(got) => assert_eq!(
                got, expected,
                "case {name}: executed WASM = {got} but CPython = {expected}\nWAT:\n{wat}"
            ),
        }
    }
    eprintln!(
        "PMAT-998: concat-of-string-returning-operands witness PASSED — chr(65)+\
         chr(66) et al. now construct the correct multi-char string (not the \
         last-operand collapse), value-matching CPython."
    );
}
