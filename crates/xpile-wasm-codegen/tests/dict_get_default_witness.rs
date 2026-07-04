//! PMAT-1223 — EXECUTED witness for native-WASM `d.get(k, default)`
//! (`Expr::DictGetOr`) over the bump-heap dict runtime
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The dict/set runtime (PMAT-995) shipped `d[k]` (`Expr::DictGet`, TRAPS on an
//! absent key — the KeyError analogue), `k in d` (`Expr::DictContains`), and
//! `d[k] = v`. This slice adds the TOTAL read `d.get(k, default)`: it NEVER
//! traps — an absent key yields the int `default`. The lowering reuses the two
//! existing helpers with no new machinery:
//!
//! ```wat
//! local.get $d  <key>  call $__wasm_dict_has_<k>   ;; membership, i32, total
//! if (result i64)
//!   local.get $d  <key>  call $__wasm_dict_get_<k>  ;; value, i64 (only when present)
//! else
//!   <default>                                       ;; i64
//! end
//! ```
//!
//! The membership test GATES the trapping value helper, so `get` runs only for a
//! present key and the absent case falls to `default` — the exact CPython
//! `d.get(k, default)` vs `d[k]` distinction.
//!
//! ## Witness shape
//!
//! Every probe is a ZERO-ARG export returning an `i64`; it builds its dict from a
//! literal on the bump heap then reads it with `.get`. `wasm-interp
//! --run-all-exports` invokes each export and prints the scalar. The probes
//! cover: int-dict present key, int-dict absent key → literal default, absent
//! key → a COMPUTED default (`7 * 6`), str-dict present/absent, and a `str(9)`
//! KEY (absent) — the last stresses the `expr_has_int_to_str` gate walker (a
//! str-keyed `.get(str(n), …)` must still declare `$__wasm_int_to_str`).
//!
//! Crucially, the absent-key probes must RETURN THE DEFAULT — NOT trap. If the
//! emit reused the bare `d[k]` path, `get9`/`sgetZ` would `unreachable`-trap and
//! the export would be missing from the interp output (caught below).
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the helpers) on a host without WABT. Every pinned value
//! is cross-checked against live `python3` in `cpython_pins_are_python`.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- meta-HIR builders (what the Python frontend would produce) ------------

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}

/// `d = {1: 10, 2: 20, 3: 30}` — an int-keyed dict local.
fn int_dict_let() -> Stmt {
    Stmt::Let {
        name: "d".into(),
        ty: Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
        mutable: false,
        value: Expr::DictLit(vec![
            (Expr::LitInt(1), Expr::LitInt(10)),
            (Expr::LitInt(2), Expr::LitInt(20)),
            (Expr::LitInt(3), Expr::LitInt(30)),
        ]),
    }
}

/// `d = {"x": 100, "y": 200}` — a str-keyed dict local (content-compare path).
fn str_dict_let() -> Stmt {
    Stmt::Let {
        name: "d".into(),
        ty: Type::Dict(Box::new(Type::Str), Box::new(Type::I64)),
        mutable: false,
        value: Expr::DictLit(vec![
            (Expr::LitStr("x".into()), Expr::LitInt(100)),
            (Expr::LitStr("y".into()), Expr::LitInt(200)),
        ]),
    }
}

/// `d.get(key, default)` — a TOTAL read.
fn dict_get_or(name: &str, key: Expr, default: Expr) -> Expr {
    Expr::DictGetOr {
        dict: Box::new(ident(name)),
        key: Box::new(key),
        default: Box::new(default),
    }
}

/// `str(n)` — an int-to-str key (stresses the `$__wasm_int_to_str` gate).
fn str_of(n: i64) -> Expr {
    Expr::ToStr {
        value: Box::new(Expr::LitInt(n)),
        of_float: false,
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

fn module(name: &str, items: Vec<Item>) -> Module {
    Module {
        name: name.into(),
        source_lang: SourceLang::Rust,
        items,
        ffi_boundaries: Vec::new(),
    }
}

/// The full non-trapping probe module: one zero-arg export per assertion.
fn probe_module() -> Module {
    module(
        "dict_get_default_witness",
        vec![
            // int dict: present key → stored value
            func(
                "getp",
                Type::I64,
                vec![int_dict_let()],
                dict_get_or("d", Expr::LitInt(2), Expr::LitInt(-1)),
            ),
            // int dict: absent key → literal default (must NOT trap)
            func(
                "geta",
                Type::I64,
                vec![int_dict_let()],
                dict_get_or("d", Expr::LitInt(9), Expr::LitInt(-1)),
            ),
            // int dict: absent key → COMPUTED default `7 * 6` == 42
            func(
                "getc",
                Type::I64,
                vec![int_dict_let()],
                dict_get_or(
                    "d",
                    Expr::LitInt(9),
                    Expr::BinOp {
                        op: xpile_meta_hir::BinOp::Mul,
                        lhs: Box::new(Expr::LitInt(7)),
                        rhs: Box::new(Expr::LitInt(6)),
                    },
                ),
            ),
            // str dict: present key → stored value
            func(
                "sgetp",
                Type::I64,
                vec![str_dict_let()],
                dict_get_or("d", Expr::LitStr("x".into()), Expr::LitInt(-1)),
            ),
            // str dict: absent key → default
            func(
                "sgeta",
                Type::I64,
                vec![str_dict_let()],
                dict_get_or("d", Expr::LitStr("z".into()), Expr::LitInt(-1)),
            ),
            // str dict: a `str(9)` KEY (== "9", absent) → default. Stresses the
            // int-to-str gate walker (the helper must be declared).
            func(
                "sgetk",
                Type::I64,
                vec![str_dict_let()],
                dict_get_or("d", str_of(9), Expr::LitInt(-1)),
            ),
        ],
    )
}

/// `(export, expected)` — the CPython value for every probe export.
const PINS: &[(&str, i64)] = &[
    ("getp", 20),
    ("geta", -1),
    ("getc", 42),
    ("sgetp", 100),
    ("sgeta", -1),
    ("sgetk", -1),
];

// ---- WABT harness -----------------------------------------------------------

/// Parse a `name() => i32:<v>` or `name() => i64:<v>` line for `name`.
/// `wasm-interp` prints integer results as UNSIGNED decimal, so a negative
/// `i64` (e.g. the `-1` default) renders as its `u64` two's-complement value —
/// parse as `u64` and reinterpret (`0xFFFF…FF` → `-1`).
fn parse_scalar_export(stdout: &str, name: &str) -> i64 {
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&format!("{name}() => ")))
        .unwrap_or_else(|| panic!("no `{name}` export in interp output:\n{stdout}"));
    let val = line
        .rsplit_once(':')
        .unwrap_or_else(|| panic!("malformed export line {line:?}"))
        .1
        .trim();
    val.parse::<u64>()
        .map(|u| u as i64)
        .unwrap_or_else(|_| panic!("parse scalar for {name} from {line:?}"))
}

/// Assemble `wat` and run all exports; returns wasm-interp's (stdout, success).
fn assemble_and_run(tag: &str, wat: &str) -> (String, bool) {
    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-dictget-{}-{}", tag, std::process::id()));
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

// ---- CONSTRUCT assertions (hold with or without WABT) -----------------------

#[test]
fn dict_get_default_lowers_and_carries_helpers() {
    let wat = emit_module(&probe_module())
        .expect("the `d.get(k, default)` program must lower through emit_module");
    // Reuses the existing membership + value helpers (int + str kinds).
    for helper in [
        "$__wasm_dict_get_i",
        "$__wasm_dict_has_i",
        "$__wasm_dict_get_s",
        "$__wasm_dict_has_s",
        "$__wasm_int_to_str", // the `str(9)` key gate
    ] {
        assert!(wat.contains(helper), "missing helper {helper}:\n{wat}");
    }
    // The total read is an `if (result i64)` over the membership test — NOT a
    // bare `d[k]`. It calls `has` before `get`.
    assert!(
        wat.contains("call $__wasm_dict_has_i")
            && wat.contains("if (result i64)")
            && wat.contains("call $__wasm_dict_get_i"),
        "the total `.get` read must be `if has(...) then get(...) else default`:\n{wat}"
    );
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn dict_get_default_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&probe_module()).expect("program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1223: skipping EXECUTED `d.get(k, default)` witness — WABT \
             (wat2wasm / wasm-interp) absent. The program lowered through \
             emit_module (asserted in `dict_get_default_lowers_and_carries_helpers`); \
             a box with WABT also runs every export and asserts each == the \
             CPython value {PINS:?}. Free CI skips execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-1223: running EXECUTED `d.get(k, default)` witness via WABT");
    let (stdout, ok) = assemble_and_run("ok", &wat);
    assert!(ok, "wasm-interp run failed:\n{stdout}\n---WAT---\n{wat}");

    // Every export — INCLUDING the absent-key ones — must have RETURNED a value
    // (the default), never trapped. A trap would drop the export line entirely.
    for &(name, expected) in PINS {
        let got = parse_scalar_export(&stdout, name);
        assert_eq!(
            got, expected,
            "executed WASM {name}() = {got} but CPython = {expected}\n\
             full interp output:\n{stdout}"
        );
    }
    // The whole point of `.get` vs `d[k]`: the absent-key reads did NOT trap.
    assert!(
        !stdout.contains("unreachable executed"),
        "`d.get(k, default)` must NOT trap on an absent key:\n{stdout}"
    );

    eprintln!(
        "PMAT-1223: EXECUTED `d.get(k, default)` witness PASSED — int + str dict \
         present-key reads returned the stored value, absent-key reads returned \
         the (literal and computed) default WITHOUT trapping, and the `str(9)` \
         key path declared `$__wasm_int_to_str`. All value-match CPython {PINS:?}."
    );
}

// ---- CPython differential cross-check (gated on python3) --------------------

#[test]
fn cpython_pins_are_python() {
    let py = "\
d = {1: 10, 2: 20, 3: 30}\n\
sd = {'x': 100, 'y': 200}\n\
vals = {\n\
 'getp': d.get(2, -1), 'geta': d.get(9, -1), 'getc': d.get(9, 7 * 6),\n\
 'sgetp': sd.get('x', -1), 'sgeta': sd.get('z', -1), 'sgetk': sd.get(str(9), -1),\n\
}\n\
print(';'.join(f'{k}={v}' for k, v in vals.items()))\n";
    let out = match Command::new("python3").arg("-c").arg(py).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("PMAT-1223: python3 absent — pins asserted against the WABT witness only");
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
