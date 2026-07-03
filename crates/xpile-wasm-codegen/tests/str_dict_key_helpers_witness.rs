//! PMAT-1150 — EXECUTED witness for str-op HELPERS reached through a str-keyed
//! dict/set KEY on the native WASM EMIT lane (`C-COMPILE-RUST-TO-WASM` +
//! `C-WASM-HEAP`).
//!
//! ## The latent gate hole this slice closes
//!
//! A Python dict/set subscript over a `str`-keyed collection lowers to
//! `Expr::DictGet` / `Expr::DictContains` / `Expr::SetContains` — NOT
//! `Expr::Index` — and its KEY (`d[s[1:3]]`, `d[str(n)]`, `d["ab" * 2]`,
//! `s[0:2] in q`) is materialised via `emit_str_expr`, which emits a
//! `call $__wasm_str_slice` / `$__wasm_int_to_str` / `$__wasm_str_repeat`.
//!
//! The helper-requirement scans (`expr_has_str_slice`, `expr_has_int_to_str`,
//! `expr_has_str_repeat`, `expr_uses_str_method`, `expr_has_str_contains`,
//! `expr_has_str_eq`, `expr_has_heap_op`) previously had NO arm for the
//! dict/set container nodes, so they never recursed into a computed key. A key
//! that is the SOLE site of its op therefore emitted a `call $HELPER` against a
//! helper NEVER DECLARED — a hard `wat2wasm` "undefined function" failure the
//! literal-key dict witnesses (PMAT-995) never triggered. Every scan now carries
//! the `DictGet | DictContains` and `SetContains` arms;
//! `helpers_are_declared_for_str_keyed_ops` is the regression guard (it fails on
//! the pre-fix scans, no WABT needed) and the executed witness is the backstop.
//!
//! This is the same class as PMAT-1148 (the `StrMethod`-recv scan gap): whenever
//! a new expr shape can host a nested string TEMPORARY, EVERY `expr_has_*` gate
//! walker must gain that shape's arm or a downstream helper goes undeclared.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + DECLARES the callee helpers) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- meta-HIR builders (what the Python frontend would produce) ------------

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn lit_s(s: &str) -> Expr {
    Expr::LitStr(s.into())
}

/// `d: dict[str, int] = { … }` — a str-keyed dict local (content-compare path).
fn str_dict_let(pairs: Vec<(&str, i64)>) -> Stmt {
    Stmt::Let {
        name: "d".into(),
        ty: Type::Dict(Box::new(Type::Str), Box::new(Type::I64)),
        mutable: false,
        value: Expr::DictLit(
            pairs
                .into_iter()
                .map(|(k, v)| (lit_s(k), Expr::LitInt(v)))
                .collect(),
        ),
    }
}

/// `q: set[str] = { … }` — a str set local.
fn str_set_let(elems: Vec<&str>) -> Stmt {
    Stmt::Let {
        name: "q".into(),
        ty: Type::Set(Box::new(Type::Str)),
        mutable: false,
        value: Expr::SetLit(elems.into_iter().map(lit_s).collect()),
    }
}

/// `"hello"[1:4]` — a str Slice temporary (of_str, un-stepped).
fn slice_lit(s: &str, lo: i64, hi: i64) -> Expr {
    Expr::Slice {
        collection: Box::new(lit_s(s)),
        lo: Some(Box::new(Expr::LitInt(lo))),
        hi: Some(Box::new(Expr::LitInt(hi))),
        of_str: true,
        step: None,
    }
}

fn dict_get(key: Expr) -> Expr {
    Expr::DictGet {
        dict: Box::new(ident("d")),
        key: Box::new(key),
    }
}
fn set_has(elem: Expr) -> Expr {
    Expr::SetContains {
        set: Box::new(ident("q")),
        elem: Box::new(elem),
    }
}

fn func(name: &str, ret: Type, stmts: Vec<Stmt>, tail: Expr) -> Item {
    Item::Function(Function {
        name: name.into(),
        params: vec![],
        return_type: ret,
        body: Block {
            stmts,
            trailing_return: tail,
        },
    })
}

/// One zero-arg export per computed-key shape; each is exactly what CPython
/// computes (cross-checked live in `cpython_pins_are_python`).
fn probe_module() -> Module {
    Module {
        name: "str_dict_key".into(),
        source_lang: SourceLang::Rust,
        items: vec![
            // d["hello"[1:4]] == d["ell"] == 7  (SLICE as a dict key)
            func(
                "k_slice",
                Type::I64,
                vec![str_dict_let(vec![("ell", 7), ("xy", 9)])],
                dict_get(slice_lit("hello", 1, 4)),
            ),
            // d[str(42)] == d["42"] == 5  (int→str as a dict key)
            func(
                "k_intstr",
                Type::I64,
                vec![str_dict_let(vec![("42", 5), ("7", 3)])],
                dict_get(Expr::ToStr {
                    value: Box::new(Expr::LitInt(42)),
                    of_float: false,
                }),
            ),
            // d["ab" * 2] == d["abab"] == 8  (string repeat as a dict key)
            func(
                "k_repeat",
                Type::I64,
                vec![str_dict_let(vec![("abab", 8), ("ab", 1)])],
                dict_get(Expr::Repeat {
                    seq: Box::new(lit_s("ab")),
                    n: Box::new(Expr::LitInt(2)),
                    of_str: true,
                }),
            ),
            // "xabx"[1:3] in {"ab", "cd"} == "ab" in q == 1  (SLICE as a set elem)
            func(
                "e_slice",
                Type::Bool,
                vec![str_set_let(vec!["ab", "cd"])],
                set_has(slice_lit("xabx", 1, 3)),
            ),
        ],
        ffi_boundaries: Vec::new(),
    }
}

/// `(export, expected)` — the CPython value for every probe export.
const PINS: &[(&str, i64)] = &[
    ("k_slice", 7),
    ("k_intstr", 5),
    ("k_repeat", 8),
    ("e_slice", 1),
];

// ---- CONSTRUCT assertion (holds with or without WABT) ----------------------

/// The pre-fix bug: a computed dict/set KEY CALLED a str helper the scan never
/// declared. Assert every such callee helper is DEFINED, not just called — this
/// fails on the pre-PMAT-1150 scans (missing `(func $__wasm_str_slice …)` etc.),
/// with no WABT needed.
#[test]
fn helpers_are_declared_for_str_keyed_ops() {
    let wat = emit_module(&probe_module()).expect("str-keyed computed-key program lowers");
    // The str-keyed dict/set machinery always pulls in content-compare.
    assert!(
        wat.contains("(func $__wasm_str_eq"),
        "str-keyed dict/set needs a DEFINED content-compare helper:\n{wat}"
    );
    for helper in [
        "$__wasm_str_slice",  // d["hello"[1:4]] and "xabx"[1:3] in q
        "$__wasm_int_to_str", // d[str(42)]
        "$__wasm_str_repeat", // d["ab" * 2]
    ] {
        assert!(
            wat.contains(&format!("call {helper}")),
            "expected a `call {helper}` (a computed key uses it):\n{wat}"
        );
        assert!(
            wat.contains(&format!("(func {helper}")),
            "REGRESSION: `{helper}` is CALLED but never DEFINED — a str-op gate \
             scan lost its DictGet/DictContains/SetContains arm (the PMAT-1150 \
             latent gap):\n{wat}"
        );
    }
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn str_keyed_computed_key_programs_execute_and_match_cpython() {
    let wat = emit_module(&probe_module()).expect("str-keyed computed-key module lowers");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1150: skipping EXECUTED str-keyed-key witness — WABT (wat2wasm / \
             wasm-interp) absent. The module lowered and DECLARED its callee str \
             helpers (asserted in helpers_are_declared_for_str_keyed_ops); the \
             pinned outcomes {PINS:?} are the CPython ground truth."
        );
        return;
    }

    let dir = std::env::temp_dir().join(format!("xpile-strkey-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wp = dir.join("p.wat");
    let bp = dir.join("p.wasm");
    std::fs::write(&wp, &wat).unwrap();

    let a = Command::new("wat2wasm")
        .arg(&wp)
        .arg("-o")
        .arg(&bp)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        a.status.success(),
        "wat2wasm failed (a called-but-undeclared helper is the classic cause):\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&a.stderr)
    );
    let r = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&bp)
        .output()
        .expect("spawn wasm-interp");
    assert!(r.status.success(), "wasm-interp: {r:?}");
    let stdout = String::from_utf8_lossy(&r.stdout);

    for &(name, expect) in PINS {
        let ty = if name.starts_with("e_") { "i32" } else { "i64" };
        let needle = format!("{name}() => {ty}:{expect}");
        assert!(
            stdout.contains(&needle),
            "str-keyed key `{name}` must execute to {expect} (== CPython), got:\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1150: str-keyed computed-KEY witness PASSED — a slice / str(int) / \
         repeat used as a dict key, and a slice used as a set elem, all lower + \
         execute value-matching CPython {PINS:?}; the DictGet/DictContains/\
         SetContains scan gap is closed."
    );
}

// ---- CPython differential cross-check (gated on python3) --------------------

#[test]
fn cpython_pins_are_python() {
    let py = "\
d1 = {'ell': 7, 'xy': 9}\n\
d2 = {'42': 5, '7': 3}\n\
d3 = {'abab': 8, 'ab': 1}\n\
q = {'ab', 'cd'}\n\
vals = {\n\
 'k_slice': d1['hello'[1:4]],\n\
 'k_intstr': d2[str(42)],\n\
 'k_repeat': d3['ab' * 2],\n\
 'e_slice': int('xabx'[1:3] in q),\n\
}\n\
print(';'.join(f'{k}={v}' for k, v in vals.items()))\n";
    let out = match Command::new("python3").arg("-c").arg(py).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("PMAT-1150: python3 absent — pins asserted against the WABT witness only");
            return;
        }
    };
    let mut seen = 0;
    for kv in out.trim().split(';') {
        let (k, v) = kv.split_once('=').expect("k=v");
        let expected: i64 = v.parse().expect("int");
        let pinned = PINS
            .iter()
            .find(|(n, _)| *n == k)
            .unwrap_or_else(|| panic!("python produced an unpinned key {k}"))
            .1;
        assert_eq!(pinned, expected, "pin {k} drifted from CPython");
        seen += 1;
    }
    assert_eq!(seen, PINS.len(), "python3 must cover every pin");
}
