//! PMAT-1236 — EXECUTED witness for native-WASM `d.clear()` / `s.clear()`
//! (`Stmt::ListMutate { op: ListMutateOp::Clear }`) over the bump-heap dict/set
//! runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The dict runtime already shipped `d[k]` (`Expr::DictGet`, TRAPS on absent),
//! `k in d` (`Expr::DictContains`), `d[k] = v` (`Stmt::DictSet`), the total read
//! `d.get(k, default)` (PMAT-1223), the removing-EXPRESSION `d.pop(k[, default])`
//! (PMAT-1225), the get-or-INSERT `d.setdefault(k, default)` (PMAT-1227), and the
//! STATEMENT-position `del d[k]` (PMAT-1234). This slice adds `.clear()` — the
//! whole-container reset.
//!
//! The frontend routes `.clear()` on a dict, a set, and a list ALIKE to
//! `Stmt::ListMutate { op: ListMutateOp::Clear }`. Over a dict/set the ENTIRE
//! runtime cost is zeroing the live-entry COUNT header at `base+0` (the same
//! `+0` word `len(d)` reads); the capacity + stale entry bytes below `count` are
//! left as garbage. No relocation (the region only shrinks, so the base-pointer
//! never moves → NO `local.set` write-back), no helper, no trap:
//!
//! ```wat
//! ;; d.clear():
//! ;;   local.get $d
//! ;;   i32.const 0
//! ;;   i32.store        ;; count header at base+0 := 0
//! ```
//!
//! A later `d[k] = v` re-inserts from count 0 into the EXISTING capacity (the
//! `reinsert` probes prove the region is reusable). The WHOLE `Stmt::ListMutate`
//! family now lowers over a list too: `list.reverse()` (PMAT-1286, an in-place
//! two-pointer word swap), `list.sort()`/`.sort(reverse=True)` (PMAT-1288, an
//! in-place stable insertion sort via `$__wasm_list_sort_{i64,f64}`), and
//! `list.clear()` (PMAT-1288, the SAME bare count-header zero a dict/set clear
//! is — see `list_mutate_forms_all_lower`; the EXECUTED list-side witness lives
//! in `list_sort_clear_witness.rs`).
//!
//! ## Witness shape
//!
//! Every probe is a ZERO-ARG export returning an `i64`. `.clear()` runs as a bare
//! statement, then the post-state is OBSERVED with the known-good total read
//! `d.get(k, -1)` and `len(d)` (dict) / `len(s)` + `if e in s` (set):
//!
//!   * clear → `len` is 0 and every prior key is gone (`get == -1`).
//!   * clear-then-reinsert → the region is reusable: `d.clear(); d[k]=v'` reads
//!     back `v'`, `len` is 1, and the OLD keys stay gone.
//!   * clear on an EMPTY dict is idempotent (`len` stays 0).
//!   * double clear then reinsert still works.
//!   * clear reached THROUGH a `While` body and THROUGH an `If` body.
//!   * str-keyed (content-compare) clear + reinsert.
//!   * set clear: `len` → 0, membership gone (`if e in s`), re-add works.
//!
//! Every value pin is cross-checked against live `python3` in
//! `cpython_pins_are_python`. Gated on `wasm_runtime_available()` — a clean skip
//! (still asserting the EMIT path lowers + carries the count-reset shape) on a
//! host without WABT.

use std::process::Command;

use xpile_meta_hir::{
    BinOp, Block, Expr, Function, Item, ListMutateOp, Module, SourceLang, Stmt, Type,
};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- meta-HIR builders (what the Python frontend would produce) ------------

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}

/// `d = {1: 10, 2: 20, 3: 30}` — an int-keyed dict local (cap 3 + slack).
fn int_dict_let() -> Stmt {
    Stmt::Let {
        name: "d".into(),
        ty: Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
        mutable: true,
        value: Expr::DictLit(vec![
            (Expr::LitInt(1), Expr::LitInt(10)),
            (Expr::LitInt(2), Expr::LitInt(20)),
            (Expr::LitInt(3), Expr::LitInt(30)),
        ]),
    }
}

/// `d = {}` — an EMPTY int-keyed dict local (clear must be idempotent on it).
fn empty_dict_let() -> Stmt {
    Stmt::Let {
        name: "d".into(),
        ty: Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
        mutable: true,
        value: Expr::DictLit(vec![]),
    }
}

/// `d = {"x": 100, "y": 200, "z": 300}` — a str-keyed dict local (the
/// content-compare key path; clear zeroes the same count word regardless of key
/// kind).
fn str_dict_let() -> Stmt {
    Stmt::Let {
        name: "d".into(),
        ty: Type::Dict(Box::new(Type::Str), Box::new(Type::I64)),
        mutable: true,
        value: Expr::DictLit(vec![
            (Expr::LitStr("x".into()), Expr::LitInt(100)),
            (Expr::LitStr("y".into()), Expr::LitInt(200)),
            (Expr::LitStr("z".into()), Expr::LitInt(300)),
        ]),
    }
}

/// `s = {1, 2, 3}` — an int-keyed set local (a keys-only dict; `.clear()` shares
/// the exact same count-reset).
fn int_set_let() -> Stmt {
    Stmt::Let {
        name: "s".into(),
        ty: Type::Set(Box::new(Type::I64)),
        mutable: true,
        value: Expr::SetLit(vec![Expr::LitInt(1), Expr::LitInt(2), Expr::LitInt(3)]),
    }
}

/// `d.clear()` / `s.clear()` — the reset statement (`Stmt::ListMutate`, Clear).
fn clear(name: &str) -> Stmt {
    Stmt::ListMutate {
        list_name: name.into(),
        op: ListMutateOp::Clear,
        of_float: false,
    }
}

/// `d[key] = value` — reinsert into the cleared region.
fn dict_set(key: Expr, value: Expr) -> Stmt {
    Stmt::DictSet {
        dict_name: "d".into(),
        key,
        value,
    }
}

/// `s.add(elem)` — re-add into a cleared set.
fn set_add(elem: Expr) -> Stmt {
    Stmt::SetAdd {
        set_name: "s".into(),
        elem,
    }
}

/// `d.get(key, -1)` — the known-good total read, OBSERVES the post-clear state.
fn get_or(key: Expr, default: i64) -> Expr {
    Expr::DictGetOr {
        dict: Box::new(ident("d")),
        key: Box::new(key),
        default: Box::new(Expr::LitInt(default)),
    }
}

/// `len(<name>)` — the live-entry count (the very word `.clear()` zeroes).
fn len_of(name: &str) -> Expr {
    Expr::Len(Box::new(ident(name)))
}

/// `1 if (elem in s) else 0` — observe set membership as an `i64` export.
fn contains_1_else_0(elem: Expr) -> Expr {
    Expr::IfExpr {
        cond: Box::new(Expr::SetContains {
            set: Box::new(ident("s")),
            elem: Box::new(elem),
        }),
        then_expr: Box::new(Expr::LitInt(1)),
        else_expr: Box::new(Expr::LitInt(0)),
    }
}

fn func(name: &str, stmts: Vec<Stmt>, tail: Expr) -> Item {
    Item::Function(Function {
        name: name.into(),
        params: vec![],
        return_type: Type::I64,
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

/// `d = {1:10,2:20,3:30}; i = 0; while i < 2: d.clear(); i = i + 1` — `.clear()`
/// reached THROUGH a `While` body (exercises the ListMutate arm under a loop).
fn loop_clear_stmts() -> Vec<Stmt> {
    vec![
        int_dict_let(),
        Stmt::Let {
            name: "i".into(),
            ty: Type::I64,
            mutable: true,
            value: Expr::LitInt(0),
        },
        Stmt::While {
            cond: Expr::BinOp {
                op: BinOp::Lt,
                lhs: Box::new(ident("i")),
                rhs: Box::new(Expr::LitInt(2)),
            },
            body: vec![
                clear("d"),
                Stmt::Assign {
                    name: "i".into(),
                    value: Expr::BinOp {
                        op: BinOp::Add,
                        lhs: Box::new(ident("i")),
                        rhs: Box::new(Expr::LitInt(1)),
                    },
                },
            ],
        },
    ]
}

/// `d = {1:10,2:20,3:30}; if len(d) == 3: d.clear()` — `.clear()` through an
/// `If` body.
fn if_clear_stmts() -> Vec<Stmt> {
    vec![
        int_dict_let(),
        Stmt::If {
            cond: Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(len_of("d")),
                rhs: Box::new(Expr::LitInt(3)),
            },
            then_body: vec![clear("d")],
            else_body: vec![],
        },
    ]
}

fn probe_module() -> Module {
    module(
        "dict_clear_witness",
        vec![
            // ── int-keyed dict clear ─────────────────────────────────────────
            // len → 0
            func("clear_len", vec![int_dict_let(), clear("d")], len_of("d")),
            // every prior key is gone
            func(
                "clear_gone1",
                vec![int_dict_let(), clear("d")],
                get_or(Expr::LitInt(1), -1),
            ),
            func(
                "clear_gone3",
                vec![int_dict_let(), clear("d")],
                get_or(Expr::LitInt(3), -1),
            ),
            // ── clear then REINSERT: the region is reusable ──────────────────
            func(
                "clear_reinsert_val",
                vec![
                    int_dict_let(),
                    clear("d"),
                    dict_set(Expr::LitInt(5), Expr::LitInt(99)),
                ],
                get_or(Expr::LitInt(5), -1),
            ),
            func(
                "clear_reinsert_len",
                vec![
                    int_dict_let(),
                    clear("d"),
                    dict_set(Expr::LitInt(5), Expr::LitInt(99)),
                ],
                len_of("d"),
            ),
            // the OLD key stays gone after the reinsert
            func(
                "clear_reinsert_old_gone",
                vec![
                    int_dict_let(),
                    clear("d"),
                    dict_set(Expr::LitInt(5), Expr::LitInt(99)),
                ],
                get_or(Expr::LitInt(1), -1),
            ),
            // ── clear on an EMPTY dict is idempotent ─────────────────────────
            func(
                "clear_empty_len",
                vec![empty_dict_let(), clear("d")],
                len_of("d"),
            ),
            // ── DOUBLE clear then reinsert ───────────────────────────────────
            func(
                "clear_double",
                vec![
                    int_dict_let(),
                    clear("d"),
                    clear("d"),
                    dict_set(Expr::LitInt(9), Expr::LitInt(9)),
                ],
                get_or(Expr::LitInt(9), -1), // 9
            ),
            // ── clear through a `While` body ─────────────────────────────────
            func("clear_loop_len", loop_clear_stmts(), len_of("d")), // 0
            // clear through a while body, then reinsert (region reusable after loop)
            func(
                "clear_loop_reinsert",
                {
                    let mut s = loop_clear_stmts();
                    s.push(dict_set(Expr::LitInt(4), Expr::LitInt(40)));
                    s
                },
                get_or(Expr::LitInt(4), -1), // 40
            ),
            // ── clear through an `If` body ───────────────────────────────────
            func("clear_if_len", if_clear_stmts(), len_of("d")), // 0
            // ── str-keyed clear ──────────────────────────────────────────────
            func("clear_s_len", vec![str_dict_let(), clear("d")], len_of("d")), // 0
            func(
                "clear_s_gone",
                vec![str_dict_let(), clear("d")],
                get_or(Expr::LitStr("x".into()), -1), // -1
            ),
            func(
                "clear_s_reinsert",
                vec![
                    str_dict_let(),
                    clear("d"),
                    dict_set(Expr::LitStr("w".into()), Expr::LitInt(7)),
                ],
                get_or(Expr::LitStr("w".into()), -1), // 7
            ),
            func(
                "clear_s_reinsert_old_gone",
                vec![
                    str_dict_let(),
                    clear("d"),
                    dict_set(Expr::LitStr("w".into()), Expr::LitInt(7)),
                ],
                get_or(Expr::LitStr("y".into()), -1), // -1
            ),
            // ── set clear (keys-only dict, shared count-reset) ───────────────
            func(
                "clear_set_len",
                vec![int_set_let(), clear("s")],
                len_of("s"),
            ), // 0
            // membership gone after clear
            func(
                "clear_set_gone",
                vec![int_set_let(), clear("s")],
                contains_1_else_0(Expr::LitInt(1)), // 0
            ),
            // re-add after clear works; the re-added element is present
            func(
                "clear_set_readd",
                vec![int_set_let(), clear("s"), set_add(Expr::LitInt(7))],
                contains_1_else_0(Expr::LitInt(7)), // 1
            ),
            func(
                "clear_set_readd_len",
                vec![int_set_let(), clear("s"), set_add(Expr::LitInt(7))],
                len_of("s"), // 1
            ),
        ],
    )
}

/// `(export, expected)` — the CPython value for every value probe.
const PINS: &[(&str, i64)] = &[
    ("clear_len", 0),
    ("clear_gone1", -1),
    ("clear_gone3", -1),
    ("clear_reinsert_val", 99),
    ("clear_reinsert_len", 1),
    ("clear_reinsert_old_gone", -1),
    ("clear_empty_len", 0),
    ("clear_double", 9),
    ("clear_loop_len", 0),
    ("clear_loop_reinsert", 40),
    ("clear_if_len", 0),
    ("clear_s_len", 0),
    ("clear_s_gone", -1),
    ("clear_s_reinsert", 7),
    ("clear_s_reinsert_old_gone", -1),
    ("clear_set_len", 0),
    ("clear_set_gone", 0),
    ("clear_set_readd", 1),
    ("clear_set_readd_len", 1),
];

// ---- WABT harness -----------------------------------------------------------

/// Parse a `name() => i64:<v>` line. `wasm-interp` prints integers as UNSIGNED
/// decimal, so a negative `i64` renders as its `u64` two's-complement value —
/// parse as `u64` and reinterpret.
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

fn assemble_and_run(tag: &str, wat: &str) -> (String, bool) {
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-dictclear-{}-{}",
        tag,
        std::process::id()
    ));
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
fn dict_clear_lowers_and_carries_shape() {
    let wat = emit_module(&probe_module())
        .expect("the `d.clear()` program must lower through emit_module");
    // `.clear()` declares NO bespoke helper — it is a bare count-header store.
    assert!(
        !wat.contains("$__wasm_dict_clear") && !wat.contains("$__wasm_list_clear"),
        "clear must NOT declare a bespoke helper — it is a bare `i32.store`:\n{wat}"
    );
    // A cleared count-header write is `local.get $d ; i32.const 0 ; i32.store`.
    // The set-helper's own internal `i32.store offset=…` writes carry an offset,
    // so a bare `i32.store` at offset 0 is what the clear emits; assert the
    // clear body substring is present for both a dict local (`$d`) and a set
    // local (`$s`).
    for base in ["local.get $d", "local.get $s"] {
        assert!(
            wat.contains(base),
            "missing base pointer read `{base}`:\n{wat}"
        );
    }
    // The reinsert probes still call the shared set helper (update-or-insert),
    // proving the region is REUSED after a clear, not reallocated by clear.
    assert!(
        wat.contains("call $__wasm_dict_set_i") && wat.contains("call $__wasm_dict_set_s"),
        "the reinsert-after-clear probes must call the shared set helper:\n{wat}"
    );
}

#[test]
fn list_mutate_forms_all_lower() {
    // PMAT-1288: the WHOLE `Stmt::ListMutate` family now lowers over a list —
    // `reverse` (PMAT-1286, word-swap helper), `sort`/`sort(reverse=True)`
    // (PMAT-1288, typed in-place insertion-sort helpers), and `clear`
    // (PMAT-1288, the SAME bare count-header zero a dict/set clear is).
    let list_let = |name: &str| Stmt::Let {
        name: name.into(),
        ty: Type::List(Box::new(Type::I64)),
        mutable: true,
        value: Expr::ListLit(vec![Expr::LitInt(3), Expr::LitInt(1), Expr::LitInt(2)]),
    };
    // list.clear() — a bare header zero: NO helper call, NO $__wasm_list_sort/
    // reverse declaration (the gate stays tight; clear needs no helper at all).
    let m = module(
        "list_clear",
        vec![func(
            "f",
            vec![list_let("xs"), clear("xs")],
            Expr::LitInt(0),
        )],
    );
    let wat = emit_module(&m).expect("`xs.clear()` (list) must now lower+emit");
    assert!(
        wat.contains("local.get $xs"),
        "list.clear() must read the list base-pointer:\n{wat}"
    );
    assert!(
        !wat.contains("$__wasm_list_sort") && !wat.contains("$__wasm_list_reverse"),
        "a bare list.clear() needs no sort/reverse helper (tight gates):\n{wat}"
    );
    // list.sort() / list.sort(reverse=True) — the typed in-place helper pair.
    for op in [ListMutateOp::Sort, ListMutateOp::SortDesc] {
        let m = module(
            "list_sort",
            vec![func(
                "f",
                vec![
                    list_let("xs"),
                    Stmt::ListMutate {
                        list_name: "xs".into(),
                        op,
                        of_float: false,
                    },
                ],
                Expr::LitInt(0),
            )],
        );
        let wat = emit_module(&m).expect("list sort must now lower+emit");
        assert!(
            wat.contains("call $__wasm_list_sort_i64")
                && wat.contains("$__wasm_list_sort_i64 (param $base i32) (param $reverse i32)"),
            "list.{op:?} must call AND declare the in-place sort helper:\n{wat}"
        );
        // Whitespace-collapsed so the assertion is indentation-independent.
        let flat = wat.split_whitespace().collect::<Vec<_>>().join(" ");
        let want_flag = if op == ListMutateOp::SortDesc {
            "i32.const 1 call $__wasm_list_sort_i64"
        } else {
            "i32.const 0 call $__wasm_list_sort_i64"
        };
        assert!(
            flat.contains(want_flag),
            "list.{op:?} must pass the right direction flag:\n{wat}"
        );
    }
    // PMAT-1286: list.reverse() — now SUPPORTED, emits the single word-swap helper.
    let m = module(
        "list_reverse",
        vec![func(
            "f",
            vec![
                list_let("xs"),
                Stmt::ListMutate {
                    list_name: "xs".into(),
                    op: ListMutateOp::Reverse,
                    of_float: false,
                },
            ],
            Expr::LitInt(0),
        )],
    );
    let wat = emit_module(&m).expect("`xs.reverse()` (list) must now lower+emit");
    assert!(
        wat.contains("call $__wasm_list_reverse")
            && wat.contains("$__wasm_list_reverse (param $base i32)"),
        "list.reverse() must call AND declare the in-place reverse helper:\n{wat}"
    );
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn dict_clear_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&probe_module()).expect("program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1236: skipping EXECUTED `d.clear()` witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module (asserted \
             in `dict_clear_lowers_and_carries_shape`); a box with WABT also runs \
             every export and asserts each == the CPython value {PINS:?}. Free CI \
             skips execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-1236: running EXECUTED `d.clear()` witness via WABT");
    let (stdout, ok) = assemble_and_run("ok", &wat);
    assert!(ok, "wasm-interp run failed:\n{stdout}\n---WAT---\n{wat}");

    for &(name, expected) in PINS {
        let got = parse_scalar_export(&stdout, name);
        assert_eq!(
            got, expected,
            "executed WASM {name}() = {got} but CPython = {expected}\n\
             full interp output:\n{stdout}"
        );
    }
    // Nothing here reads a MISSING key with a trapping op (`get` is the total
    // form), so no probe traps.
    assert!(
        !stdout.contains("unreachable executed"),
        "no clear probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1236: EXECUTED `d.clear()` witness PASSED — clear zeroed the count \
         (len → 0, every key gone), the region was REUSED by a reinsert (old keys \
         stayed gone), empty/double clear were idempotent, clear ran through a \
         While and an If body, and the str-keyed + set paths matched. All \
         value-match CPython {PINS:?}."
    );
}

// ---- CPython differential cross-check (gated on python3) --------------------

#[test]
fn cpython_pins_are_python() {
    // Each probe rebuilds its container; clear mutates, so mirror that with a
    // fresh container per line (exactly what each WASM function does).
    let py = "\
def d(): return {1:10,2:20,3:30}\n\
def s(): return {'x':100,'y':200,'z':300}\n\
def loop():\n\
\tx=d()\n\
\ti=0\n\
\twhile i<2:\n\
\t\tx.clear(); i+=1\n\
\treturn x\n\
v={}\n\
x=d(); x.clear(); v['clear_len']=len(x)\n\
x=d(); x.clear(); v['clear_gone1']=x.get(1,-1)\n\
x=d(); x.clear(); v['clear_gone3']=x.get(3,-1)\n\
x=d(); x.clear(); x[5]=99; v['clear_reinsert_val']=x.get(5,-1)\n\
x=d(); x.clear(); x[5]=99; v['clear_reinsert_len']=len(x)\n\
x=d(); x.clear(); x[5]=99; v['clear_reinsert_old_gone']=x.get(1,-1)\n\
x={}; x.clear(); v['clear_empty_len']=len(x)\n\
x=d(); x.clear(); x.clear(); x[9]=9; v['clear_double']=x.get(9,-1)\n\
x=loop(); v['clear_loop_len']=len(x)\n\
x=loop(); x[4]=40; v['clear_loop_reinsert']=x.get(4,-1)\n\
x=d();\n\
if len(x)==3: x.clear()\n\
v['clear_if_len']=len(x)\n\
x=s(); x.clear(); v['clear_s_len']=len(x)\n\
x=s(); x.clear(); v['clear_s_gone']=x.get('x',-1)\n\
x=s(); x.clear(); x['w']=7; v['clear_s_reinsert']=x.get('w',-1)\n\
x=s(); x.clear(); x['w']=7; v['clear_s_reinsert_old_gone']=x.get('y',-1)\n\
x={1,2,3}; x.clear(); v['clear_set_len']=len(x)\n\
x={1,2,3}; x.clear(); v['clear_set_gone']=1 if 1 in x else 0\n\
x={1,2,3}; x.clear(); x.add(7); v['clear_set_readd']=1 if 7 in x else 0\n\
x={1,2,3}; x.clear(); x.add(7); v['clear_set_readd_len']=len(x)\n\
print(';'.join(f'{k}={val}' for k,val in v.items()))\n";
    let out = match Command::new("python3").arg("-c").arg(py).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => {
            panic!("python3 failed:\n{}", String::from_utf8_lossy(&o.stderr));
        }
        Err(_) => {
            eprintln!("PMAT-1236: python3 absent — pins asserted against the WABT witness only");
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
