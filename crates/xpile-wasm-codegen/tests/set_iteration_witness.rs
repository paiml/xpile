//! PMAT-1290 — EXECUTED witness for native-WASM `for x in s` over a set — the
//! FIRST iteration over a hash container in the WASM subset. Runs on the
//! bump-heap set runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! The list/dict/set surface shipped every non-iterating set op (membership,
//! len, add, remove, `==`, predicates, algebra) but refused iteration: dict
//! iteration is HARD (CPython ≥3.7 guarantees INSERTION order, but the bump-heap
//! removal is swap-last-into-hole, so post-delete order diverges). A SET has NO
//! defined order, so `for x in s` sidesteps that entirely: xpile iterates the
//! live-entry region `0..count` in STORAGE order, and every legitimate use
//! observes the elements COMMUTATIVELY (sum / max / count / membership). Storage
//! order is irrelevant to a commutative fold — both xpile and CPython agree on
//! the MULTISET, which is all such a fold sees.
//!
//! The desugar (`desugar_foreach_stmts`) already lowered `for x in s` to a
//! `while` loop whose per-element read is `s[i]` (`Expr::Index` on a set NAME) —
//! it previously refused at `emit_index`. PMAT-1290 teaches `emit_index` (and the
//! string-position `emit_str_expr`, for a `set[str]` element) to read entry `i`'s
//! KEY from the 16-byte `DICT_ENTRY_SIZE`-stride entry array (key at entry offset
//! 0), gated on the index being the synthetic foreach counter so a user-written
//! `s[i]` (a Python set is NOT subscriptable — `TypeError`) stays refused.
//!
//! Key correctness properties this pins against live `python3`:
//!   * int-set sum / max / count over iteration == CPython (commutative folds).
//!   * order-INDEPENDENCE after a swap-into-hole `discard` (the storage order is
//!     scrambled, the sum is not).
//!   * the EMPTY set iterates zero times (the loop guard `i >= count` holds at
//!     `i=0`), a single-element set once, negatives fold correctly.
//!   * NESTED `for x in a: for y in b:` over two sets.
//!   * `set[str]` iteration: the loop var is a str local, so `len(w)` and a
//!     length predicate compose (str keys ride an i32 base-pointer).
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the stride-16 read) without WABT.

use std::process::Command;

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- meta-HIR builders (what the Python frontend would produce) ------------

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}

fn binop(op: BinOp, l: Expr, r: Expr) -> Expr {
    Expr::BinOp {
        op,
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}

/// `<name>: set[int] = {v0, v1, …}` — a mutable int-elem set local.
fn iset(name: &str, vals: &[i64]) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty: Type::Set(Box::new(Type::I64)),
        mutable: true,
        value: Expr::SetLit(vals.iter().copied().map(Expr::LitInt).collect()),
    }
}

/// `<name>: set[str] = {"v0", "v1", …}` — a mutable str-elem set local.
fn sset(name: &str, vals: &[&str]) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty: Type::Set(Box::new(Type::Str)),
        mutable: true,
        value: Expr::SetLit(vals.iter().map(|s| Expr::LitStr((*s).into())).collect()),
    }
}

/// `<name>: int = <v>` — a mutable i64 accumulator local.
fn acc(name: &str, v: i64) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty: Type::I64,
        mutable: true,
        value: Expr::LitInt(v),
    }
}

/// `<name>.discard(e)` — a removal that reorders the entry array (swap-into-hole).
fn discard(name: &str, elem: Expr) -> Stmt {
    Stmt::SetRemove {
        set_name: name.into(),
        elem,
        error_if_absent: false,
    }
}

/// `<name> = <value>`.
fn assign(name: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: name.into(),
        value,
    }
}

/// `for <var> in <set_name>: <body>` — the single-var set-iteration shape the
/// frontend produces (`over_keys` is false: a set is not a dict).
fn for_in_set(var: &str, set_name: &str, elem_ty: Type, body: Vec<Stmt>) -> Stmt {
    Stmt::ForEach {
        var: var.into(),
        iter: ident(set_name),
        elem_ty,
        body,
        over_keys: false,
        dict_guard: None,
        mutate_elems: false,
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

// ---- the probe module -------------------------------------------------------

fn probe_module() -> Module {
    module(
        "set_iteration_witness",
        vec![
            // ── int-set SUM over iteration → 25 ───────────────────────────────
            func(
                "sum_int",
                vec![
                    iset("s", &[5, 3, 10, 7]),
                    acc("total", 0),
                    for_in_set(
                        "x",
                        "s",
                        Type::I64,
                        vec![assign(
                            "total",
                            binop(BinOp::Add, ident("total"), ident("x")),
                        )],
                    ),
                ],
                ident("total"),
            ),
            // ── order-INDEPENDENCE after a swap-into-hole discard → 12 ────────
            // discard(3) swaps the last entry into slot 2, scrambling STORAGE
            // order; the commutative sum is unaffected.
            func(
                "sum_after_discard",
                vec![
                    iset("s", &[1, 2, 3, 4, 5]),
                    discard("s", Expr::LitInt(3)),
                    acc("total", 0),
                    for_in_set(
                        "x",
                        "s",
                        Type::I64,
                        vec![assign(
                            "total",
                            binop(BinOp::Add, ident("total"), ident("x")),
                        )],
                    ),
                ],
                ident("total"),
            ),
            // ── MAX via iteration (if x > m: m = x) → 40 ─────────────────────
            func(
                "max_int",
                vec![
                    iset("s", &[5, 3, 40, 7, 12]),
                    acc("m", -1_000_000),
                    for_in_set(
                        "x",
                        "s",
                        Type::I64,
                        vec![Stmt::If {
                            cond: binop(BinOp::Gt, ident("x"), ident("m")),
                            then_body: vec![assign("m", ident("x"))],
                            else_body: vec![],
                        }],
                    ),
                ],
                ident("m"),
            ),
            // ── COUNT via iteration → 5 ──────────────────────────────────────
            func(
                "count_int",
                vec![
                    iset("s", &[1, 2, 3, 4, 5]),
                    acc("n", 0),
                    for_in_set(
                        "x",
                        "s",
                        Type::I64,
                        vec![assign("n", binop(BinOp::Add, ident("n"), Expr::LitInt(1)))],
                    ),
                ],
                ident("n"),
            ),
            // ── EMPTY set iterates zero times → 0 (the loop guard is load-bearing)
            func(
                "sum_empty",
                vec![
                    iset("s", &[]),
                    acc("total", 7),
                    for_in_set(
                        "x",
                        "s",
                        Type::I64,
                        vec![assign(
                            "total",
                            binop(BinOp::Add, ident("total"), ident("x")),
                        )],
                    ),
                ],
                // total stays 7 if the loop body never runs; CPython sum starts 0,
                // so start the accumulator at 0 to compare an honest empty sum.
                binop(BinOp::Sub, ident("total"), Expr::LitInt(7)),
            ),
            // ── SINGLE-element set → 99 ──────────────────────────────────────
            func(
                "sum_single",
                vec![
                    iset("s", &[99]),
                    acc("total", 0),
                    for_in_set(
                        "x",
                        "s",
                        Type::I64,
                        vec![assign(
                            "total",
                            binop(BinOp::Add, ident("total"), ident("x")),
                        )],
                    ),
                ],
                ident("total"),
            ),
            // ── NEGATIVES fold correctly → 2 (-5 + 10 + -3) ──────────────────
            func(
                "sum_neg",
                vec![
                    iset("s", &[-5, 10, -3]),
                    acc("total", 0),
                    for_in_set(
                        "x",
                        "s",
                        Type::I64,
                        vec![assign(
                            "total",
                            binop(BinOp::Add, ident("total"), ident("x")),
                        )],
                    ),
                ],
                ident("total"),
            ),
            // ── NESTED for x in a: for y in b: total += x + y → 102 ──────────
            func(
                "nested",
                vec![
                    iset("a", &[1, 2, 3]),
                    iset("b", &[10, 20]),
                    acc("total", 0),
                    for_in_set(
                        "x",
                        "a",
                        Type::I64,
                        vec![for_in_set(
                            "y",
                            "b",
                            Type::I64,
                            vec![assign(
                                "total",
                                binop(
                                    BinOp::Add,
                                    ident("total"),
                                    binop(BinOp::Add, ident("x"), ident("y")),
                                ),
                            )],
                        )],
                    ),
                ],
                ident("total"),
            ),
            // ── str-set: total length via len(w) → 6 (2 + 3 + 1) ─────────────
            func(
                "str_total_len",
                vec![
                    sset("s", &["aa", "bbb", "c"]),
                    acc("total", 0),
                    for_in_set(
                        "w",
                        "s",
                        Type::Str,
                        vec![assign(
                            "total",
                            binop(BinOp::Add, ident("total"), Expr::Len(Box::new(ident("w")))),
                        )],
                    ),
                ],
                ident("total"),
            ),
            // ── str-set: count elements with len > 5 → 2 (banana, cherry) ────
            func(
                "str_count_long",
                vec![
                    sset("s", &["apple", "banana", "cherry"]),
                    acc("n", 0),
                    for_in_set(
                        "w",
                        "s",
                        Type::Str,
                        vec![Stmt::If {
                            cond: binop(
                                BinOp::Gt,
                                Expr::Len(Box::new(ident("w"))),
                                Expr::LitInt(5),
                            ),
                            then_body: vec![assign(
                                "n",
                                binop(BinOp::Add, ident("n"), Expr::LitInt(1)),
                            )],
                            else_body: vec![],
                        }],
                    ),
                ],
                ident("n"),
            ),
        ],
    )
}

/// The CPython-pinned truth for every export (cross-checked in
/// `cpython_pins_are_python`).
const PINS: &[(&str, i64)] = &[
    ("sum_int", 25),
    ("sum_after_discard", 12),
    ("max_int", 40),
    ("count_int", 5),
    ("sum_empty", 0),
    ("sum_single", 99),
    ("sum_neg", 2),
    ("nested", 102),
    ("str_total_len", 6),
    ("str_count_long", 2),
];

// ---- WABT harness -----------------------------------------------------------

fn parse_i64_export(stdout: &str, name: &str) -> i64 {
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&format!("{name}() => ")))
        .unwrap_or_else(|| panic!("no `{name}` export in interp output:\n{stdout}"));
    line.rsplit_once(':')
        .unwrap_or_else(|| panic!("malformed export line {line:?}"))
        .1
        .trim()
        .parse::<i64>()
        .unwrap_or_else(|_| panic!("parse i64 for {name} from {line:?}"))
}

fn assemble_and_run(wat: &str) -> (String, bool) {
    let dir = std::env::temp_dir().join(format!("xpile-wasm-setiter-{}", std::process::id()));
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
fn set_iteration_lowers_with_stride16_read() {
    let wat = emit_module(&probe_module())
        .expect("the set-iteration program must lower through emit_module");
    // The per-element read walks the 16-byte DICT_ENTRY_SIZE-stride entry array,
    // NOT the 8-byte packed-slot stride a list uses.
    assert!(
        wat.contains("i32.const 16"),
        "set-iteration read must use the 16-byte entry stride:\n{wat}"
    );
    // No dict/set helper is introduced by iteration itself (it is inline loads),
    // and the module carries the bump-heap memory the set literals force.
    assert!(
        wat.contains("(memory"),
        "a set-carrying module must export linear memory:\n{wat}"
    );
    // The str-set element read lands in a string position — the loop var `w`
    // must resolve as a str local, so `len(w)` reads its i32 header.
    assert!(
        wat.contains("(func $str_total_len") && wat.contains("(func $str_count_long"),
        "str-set iteration functions must lower:\n{wat}"
    );
}

// ---- honest-refusal assertions ---------------------------------------------

#[test]
fn direct_set_subscript_refuses_typeerror() {
    // A user-written `s[0]` on a set is a Python TypeError; the WASM lane must
    // NOT silently read storage slot 0 (set indexing exists only as the internal
    // foreach lowering, gated on the synthetic counter name).
    let m = module(
        "bad_subscript",
        vec![func(
            "f",
            vec![iset("s", &[5, 3, 10])],
            Expr::Index {
                collection: Box::new(ident("s")),
                index: Box::new(Expr::LitInt(0)),
            },
        )],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("not") && err.contains("subscriptable"),
        "direct set subscript must refuse as a TypeError, got: {err}"
    );
}

#[test]
fn order_dependent_for_in_dict_still_refuses() {
    // PMAT-1297 opened `for k in d` for ORDER-INDEPENDENT (commutative) bodies
    // (the positive witnesses live in dict_key_iteration_witness.rs), but an
    // ORDER-DEPENDENT body must STAY refused — a dict's storage order can diverge
    // from CPython's insertion order after a swap-into-hole `del`, so emitting a
    // positional fold (`total = total * 10 + k`) would silently miscompile. This
    // pins that honesty guard.
    let m = module(
        "dict_iter",
        vec![func(
            "f",
            vec![
                Stmt::Let {
                    name: "d".into(),
                    ty: Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
                    mutable: true,
                    value: Expr::DictLit(vec![
                        (Expr::LitInt(1), Expr::LitInt(10)),
                        (Expr::LitInt(2), Expr::LitInt(20)),
                    ]),
                },
                acc("total", 0),
                Stmt::ForEach {
                    var: "k".into(),
                    iter: ident("d"),
                    elem_ty: Type::I64,
                    // `total = total * 10 + k` — the position of each key matters,
                    // so the result depends on iteration order.
                    body: vec![assign(
                        "total",
                        binop(
                            BinOp::Add,
                            binop(BinOp::Mul, ident("total"), Expr::LitInt(10)),
                            ident("k"),
                        ),
                    )],
                    over_keys: true,
                    dict_guard: None,
                    mutate_elems: false,
                },
            ],
            ident("total"),
        )],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("order-dependent") && err.contains("dict"),
        "an order-dependent dict iteration must stay refused, got: {err}"
    );
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn set_iteration_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&probe_module()).expect("program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1290: skipping EXECUTED set-iteration witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module and carries \
             the stride-16 set read (asserted in `set_iteration_lowers_with_stride16_read`); \
             a box with WABT also runs every export and asserts each == the CPython \
             value {PINS:?}. Free CI skips execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-1290: running EXECUTED set-iteration witness via WABT");
    let (stdout, ok) = assemble_and_run(&wat);
    assert!(ok, "wasm-interp run failed:\n{stdout}\n---WAT---\n{wat}");

    for &(name, expected) in PINS {
        let got = parse_i64_export(&stdout, name);
        assert_eq!(
            got, expected,
            "executed WASM {name}() = {got} but CPython = {expected}\n\
             full interp output:\n{stdout}"
        );
    }
    assert!(
        !stdout.contains("unreachable executed"),
        "no set-iteration probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1290: EXECUTED set-iteration witness PASSED — `for x in s` is the \
         FIRST hash-container iteration in the WASM subset, all {} commutative-fold \
         exports == CPython {PINS:?}.",
        PINS.len()
    );
}

// ---- CPython differential cross-check (gated on python3) --------------------

#[test]
fn cpython_pins_are_python() {
    // Every pin is a SINGLE-line expression (sum / max / len / comprehension)
    // to keep the embedded probe free of indentation-sensitive block statements.
    let py = "\
v={}\n\
v['sum_int']=sum({5,3,10,7})\n\
s={1,2,3,4,5}; s.discard(3); v['sum_after_discard']=sum(s)\n\
v['max_int']=max({5,3,40,7,12})\n\
v['count_int']=len({1,2,3,4,5})\n\
v['sum_empty']=sum(set())\n\
v['sum_single']=sum({99})\n\
v['sum_neg']=sum({-5,10,-3})\n\
v['nested']=sum(x+y for x in {1,2,3} for y in {10,20})\n\
v['str_total_len']=sum(len(w) for w in {'aa','bbb','c'})\n\
v['str_count_long']=sum(1 for w in {'apple','banana','cherry'} if len(w)>5)\n\
import sys\n\
for k,val in v.items(): sys.stdout.write(f'{k}={val}\\n')\n";

    let out = Command::new("python3").arg("-c").arg(py).output();
    let Ok(out) = out else {
        eprintln!("PMAT-1290: python3 absent — skipping CPython cross-check");
        return;
    };
    if !out.status.success() {
        panic!(
            "python3 probe failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut seen = 0;
    for line in stdout.lines() {
        let (k, v) = line.split_once('=').expect("k=v line");
        let expected = PINS
            .iter()
            .find(|(n, _)| *n == k)
            .unwrap_or_else(|| panic!("python emitted unknown key {k}"))
            .1;
        assert_eq!(
            v.parse::<i64>().unwrap(),
            expected,
            "CPython {k}={v} but PIN={expected}"
        );
        seen += 1;
    }
    assert_eq!(seen, PINS.len(), "CPython must cover every PIN");
}
