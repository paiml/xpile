//! PMAT-1151 — EXECUTED witness for str-op HELPERS reached through the WRITE
//! side of a str-keyed dict/set on the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## The latent gate hole this slice closes
//!
//! PMAT-1150 closed the READ side — the `Expr::DictGet` / `Expr::DictContains` /
//! `Expr::SetContains` arms of every `expr_has_*` helper-gate walker — so a
//! COMPUTED key/elem in `d[str(n)]` / `s[0:2] in q` declares its callee helper.
//! But the WRITE side is a STATEMENT, not an expression:
//!
//!   * `d[k] = v`   → `Stmt::DictSet { key, value }`   (`emit_dict_set`)
//!   * `s.add(e)`   → `Stmt::SetAdd  { elem }`          (`emit_set_add`)
//!   * `xs[i] = v`  → `Stmt::IndexAssign { indices, value }` (`emit_index_assign`)
//!
//! `emit_dict_set`/`emit_set_add` route a str key/elem through `emit_dict_key` →
//! `emit_str_expr`, and `emit_index_assign` emits the INDEX expression — each can
//! emit `call $__wasm_int_to_str` / `$__wasm_str_slice` / `$__wasm_str_repeat`.
//! Yet NO helper-gate STMT-walker scanned these three statement forms: they saw
//! only `Let`/`Assign`/`Return`/`If`/`While`/`IndexAssign{value}`/`FieldAssign`/
//! `SideEffectCall` — `DictSet`/`SetAdd` not at all, and `IndexAssign`'s `indices`
//! never (only its `value`). So `d[str(n)] = 5` over a str-keyed dict emitted a
//! `call $__wasm_int_to_str` against a helper NEVER DECLARED — a hard `wat2wasm`
//! "undefined function" failure the value-only scans never triggered.
//!
//! Every stmt-walker (`stmt_touches_str`, `stmt_has_str_slice`,
//! `stmt_has_int_to_str`, `stmt_uses_str_method`, `stmt_has_str_contains`,
//! `stmt_has_str_repeat`, `stmt_has_str_eq`, `stmt_has_heap_op`) now carries the
//! `DictSet` (key+value) and `SetAdd` (elem) arms and scans `IndexAssign`'s
//! `indices`. `helpers_are_declared_for_str_keyed_writes` is the regression guard
//! (fails on the pre-fix scans, no WABT needed); the executed witness is the
//! backstop. Same class as PMAT-1148/1150: a new expr/stmt shape that can host a
//! string TEMPORARY must be added to EVERY gate walker or a helper goes
//! undeclared.
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
fn to_str(n: i64) -> Expr {
    Expr::ToStr {
        value: Box::new(Expr::LitInt(n)),
        of_float: false,
    }
}
/// `"s"[lo:hi]` — a str Slice temporary (of_str, un-stepped).
fn slice_lit(s: &str, lo: i64, hi: i64) -> Expr {
    Expr::Slice {
        collection: Box::new(lit_s(s)),
        lo: Some(Box::new(Expr::LitInt(lo))),
        hi: Some(Box::new(Expr::LitInt(hi))),
        of_str: true,
        step: None,
    }
}
/// `"s" * k` — a str Repeat temporary.
fn repeat_lit(s: &str, k: i64) -> Expr {
    Expr::Repeat {
        seq: Box::new(lit_s(s)),
        n: Box::new(Expr::LitInt(k)),
        of_str: true,
    }
}

/// `d: dict[str, int] = {}` — an EMPTY str-keyed dict local (the write target).
fn empty_str_dict() -> Stmt {
    Stmt::Let {
        name: "d".into(),
        ty: Type::Dict(Box::new(Type::Str), Box::new(Type::I64)),
        mutable: true,
        value: Expr::DictLit(vec![]),
    }
}
/// `d[key] = v` — the DictSet under test (str key materialised via emit_str_expr).
fn dict_set(key: Expr, v: i64) -> Stmt {
    Stmt::DictSet {
        dict_name: "d".into(),
        key,
        value: Expr::LitInt(v),
    }
}
/// `d[literal] ` — read back via a LITERAL key (already a scanned expr).
fn dict_get_lit(k: &str) -> Expr {
    Expr::DictGet {
        dict: Box::new(ident("d")),
        key: Box::new(lit_s(k)),
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

/// One zero-arg export per computed WRITE-key shape; each returns exactly what
/// CPython computes (cross-checked live in `cpython_pins_are_python`).
fn probe_module() -> Module {
    Module {
        name: "str_dictset_stmt".into(),
        source_lang: SourceLang::Rust,
        items: vec![
            // d[str(42)] = 5; d["42"] == 5   (int→str as a DictSet KEY)
            func(
                "w_intstr_key",
                Type::I64,
                vec![empty_str_dict(), dict_set(to_str(42), 5)],
                dict_get_lit("42"),
            ),
            // d["hello"[1:4]] = 7; d["ell"] == 7   (SLICE as a DictSet KEY)
            func(
                "w_slice_key",
                Type::I64,
                vec![empty_str_dict(), dict_set(slice_lit("hello", 1, 4), 7)],
                dict_get_lit("ell"),
            ),
            // d["ab" * 2] = 8; d["abab"] == 8   (REPEAT as a DictSet KEY)
            func(
                "w_repeat_key",
                Type::I64,
                vec![empty_str_dict(), dict_set(repeat_lit("ab", 2), 8)],
                dict_get_lit("abab"),
            ),
            // q = {"cd"}; q.add("xabx"[1:3]); int("ab" in q) == 1  (SLICE as a SetAdd ELEM)
            func(
                "w_set_slice",
                Type::Bool,
                vec![
                    Stmt::Let {
                        name: "q".into(),
                        ty: Type::Set(Box::new(Type::Str)),
                        mutable: true,
                        value: Expr::SetLit(vec![lit_s("cd")]),
                    },
                    Stmt::SetAdd {
                        set_name: "q".into(),
                        elem: slice_lit("xabx", 1, 3),
                    },
                ],
                Expr::SetContains {
                    set: Box::new(ident("q")),
                    elem: Box::new(lit_s("ab")),
                },
            ),
            // xs = [0,0,0,0]; xs[len(str(70))] = 9; xs[2] == 9
            //   (int→str inside an IndexAssign INDEX — the `indices` scan gap)
            func(
                "w_index_intstr",
                Type::I64,
                vec![
                    Stmt::Let {
                        name: "xs".into(),
                        ty: Type::List(Box::new(Type::I64)),
                        mutable: true,
                        value: Expr::ListLit(vec![
                            Expr::LitInt(0),
                            Expr::LitInt(0),
                            Expr::LitInt(0),
                            Expr::LitInt(0),
                        ]),
                    },
                    Stmt::IndexAssign {
                        list_name: "xs".into(),
                        indices: vec![Expr::Len(Box::new(to_str(70)))],
                        value: Expr::LitInt(9),
                    },
                ],
                Expr::Index {
                    collection: Box::new(ident("xs")),
                    index: Box::new(Expr::LitInt(2)),
                },
            ),
        ],
        ffi_boundaries: Vec::new(),
    }
}

/// `(export, expected)` — the CPython value for every probe export.
const PINS: &[(&str, i64)] = &[
    ("w_intstr_key", 5),
    ("w_slice_key", 7),
    ("w_repeat_key", 8),
    ("w_set_slice", 1),
    ("w_index_intstr", 9),
];

// ---- CONSTRUCT assertion (holds with or without WABT) ----------------------

/// The pre-fix bug: a computed dict/set-WRITE key/elem/index CALLED a str helper
/// the STMT-scan never declared. Assert every such callee helper is DEFINED, not
/// just called — this fails on the pre-PMAT-1151 scans (no `DictSet`/`SetAdd`
/// arm, `IndexAssign` value-only), with no WABT needed.
#[test]
fn helpers_are_declared_for_str_keyed_writes() {
    let wat = emit_module(&probe_module()).expect("str-keyed WRITE program lowers");
    for helper in [
        "$__wasm_int_to_str", // d[str(42)] = … and xs[len(str(70))] = …
        "$__wasm_str_slice",  // d["hello"[1:4]] = … and q.add("xabx"[1:3])
        "$__wasm_str_repeat", // d["ab" * 2] = …
    ] {
        assert!(
            wat.contains(&format!("call {helper}")),
            "expected a `call {helper}` (a computed write-key uses it):\n{wat}"
        );
        assert!(
            wat.contains(&format!("(func {helper}")),
            "REGRESSION: `{helper}` is CALLED but never DEFINED — a str-op gate \
             STMT-scan lost its DictSet/SetAdd arm or its IndexAssign `indices` \
             scan (the PMAT-1151 latent gap):\n{wat}"
        );
    }
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn str_keyed_write_programs_execute_and_match_cpython() {
    let wat = emit_module(&probe_module()).expect("str-keyed WRITE module lowers");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1151: skipping EXECUTED str-keyed-WRITE witness — WABT (wat2wasm / \
             wasm-interp) absent. The module lowered and DECLARED its callee str \
             helpers (asserted in helpers_are_declared_for_str_keyed_writes); the \
             pinned outcomes {PINS:?} are the CPython ground truth."
        );
        return;
    }

    let dir = std::env::temp_dir().join(format!("xpile-dictset-{}", std::process::id()));
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
        let ty = if name == "w_set_slice" { "i32" } else { "i64" };
        let needle = format!("{name}() => {ty}:{expect}");
        assert!(
            stdout.contains(&needle),
            "str-keyed write `{name}` must execute to {expect} (== CPython), got:\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1151: str-keyed WRITE witness PASSED — a slice / str(int) / repeat \
         used as a DictSet key, a slice used as a SetAdd elem, and str(int) inside \
         an IndexAssign index all lower + execute value-matching CPython {PINS:?}; \
         the DictSet/SetAdd/IndexAssign-indices STMT-scan gap is closed."
    );
}

// ---- CPython differential cross-check (gated on python3) --------------------

#[test]
fn cpython_pins_are_python() {
    let py = "\
d1 = {}\n\
d1[str(42)] = 5\n\
d2 = {}\n\
d2['hello'[1:4]] = 7\n\
d3 = {}\n\
d3['ab' * 2] = 8\n\
q = {'cd'}\n\
q.add('xabx'[1:3])\n\
xs = [0, 0, 0, 0]\n\
xs[len(str(70))] = 9\n\
vals = {\n\
 'w_intstr_key': d1['42'],\n\
 'w_slice_key': d2['ell'],\n\
 'w_repeat_key': d3['abab'],\n\
 'w_set_slice': int('ab' in q),\n\
 'w_index_intstr': xs[2],\n\
}\n\
print(';'.join(f'{k}={v}' for k, v in vals.items()))\n";
    let out = match Command::new("python3").arg("-c").arg(py).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("PMAT-1151: python3 absent — pins asserted against the WABT witness only");
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
