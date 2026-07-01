//! PMAT-999 — EXECUTED witness for dict/set CAPACITY GROWTH on the bump heap.
//!
//! A dict/set literal pre-sizes a FIXED capacity (`literal_count + 16`); the
//! second differential sweep confirmed that growing past it via `.add`/`[k]=v`
//! (e.g. a loop) TRAPPED — diverging from CPython's unbounded growth. This slice
//! makes `$__wasm_dict_set` GROW (bump-alloc a 2x region, `memory.copy` the
//! header + entries) and RETURN the relocated base-pointer, which every caller
//! `local.set`s back into the dict/set local.
//!
//! This witness builds a set and a dict and grows each well past its initial
//! slack via a `while` loop, then reads it back — executed in WABT and
//! value-matched to CPython. Gated on `wasm_runtime_available()`.

use std::process::Command;

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn lt(l: Expr, r: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Lt,
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}
fn add(l: Expr, r: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Add,
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}
/// `i = i + 1`.
fn incr(name: &str) -> Stmt {
    Stmt::Assign {
        name: name.into(),
        value: add(ident(name), Expr::LitInt(1)),
    }
}
fn let_scalar(name: &str, v: i64) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty: Type::I64,
        mutable: true,
        value: Expr::LitInt(v),
    }
}

fn zero_arg_i64(name: &str, stmts: Vec<Stmt>, tail: Expr) -> Module {
    Module {
        name: name.into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(Function {
            name: name.into(),
            params: vec![],
            return_type: Type::I64,
            body: Block {
                stmts,
                trailing_return: tail,
            },
        })],
        ffi_boundaries: Vec::new(),
    }
}

fn run(name: &str, wat: &str) -> Result<i64, ()> {
    let dir = std::env::temp_dir().join(format!("xpile-grow-{}-{}", name, std::process::id()));
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
    let v: u64 = line
        .rsplit_once(':')
        .expect("scalar")
        .1
        .trim()
        .parse()
        .expect("u64");
    Ok(v as i64)
}

/// `s = {0}; i = 1; while i < n: s.add(i); i += 1` — grows a set to `n` distinct
/// elements (well past the initial slack of 16).
fn grow_set_stmts(n: i64) -> Vec<Stmt> {
    vec![
        Stmt::Let {
            name: "s".into(),
            ty: Type::Set(Box::new(Type::I64)),
            mutable: false,
            value: Expr::SetLit(vec![Expr::LitInt(0)]),
        },
        let_scalar("i", 1),
        Stmt::While {
            cond: lt(ident("i"), Expr::LitInt(n)),
            body: vec![
                Stmt::SetAdd {
                    set_name: "s".into(),
                    elem: ident("i"),
                },
                incr("i"),
            ],
        },
    ]
}

/// `d = {0: 0}; i = 1; while i < n: d[i] = i*2; i += 1` — grows a dict to `n`.
fn grow_dict_stmts(n: i64) -> Vec<Stmt> {
    vec![
        Stmt::Let {
            name: "d".into(),
            ty: Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
            mutable: false,
            value: Expr::DictLit(vec![(Expr::LitInt(0), Expr::LitInt(0))]),
        },
        let_scalar("i", 1),
        Stmt::While {
            cond: lt(ident("i"), Expr::LitInt(n)),
            body: vec![
                Stmt::DictSet {
                    dict_name: "d".into(),
                    key: ident("i"),
                    value: Expr::BinOp {
                        op: BinOp::Mul,
                        lhs: Box::new(ident("i")),
                        rhs: Box::new(Expr::LitInt(2)),
                    },
                },
                incr("i"),
            ],
        },
    ]
}

#[test]
fn dict_set_grow_executes_and_matches_cpython() {
    // CONSTRUCT: the set helper grows (memory.copy + returns a pointer).
    let wat = emit_module(&zero_arg_i64(
        "g",
        grow_set_stmts(50),
        Expr::Len(Box::new(ident("s"))),
    ))
    .expect("growing set lowers");
    assert!(
        wat.contains("memory.copy") && wat.contains("(result i32)"),
        "the set helper must grow (memory.copy) + return the base-pointer:\n{wat}"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-999: skipping executed growth witness — WABT absent");
        return;
    }

    // set grown to 50 distinct elements → len 50 (was: TRAP past 17).
    let m = zero_arg_i64("g", grow_set_stmts(50), Expr::Len(Box::new(ident("s"))));
    assert_eq!(run("g", &emit_module(&m).unwrap()), Ok(50), "grown set len");

    // membership after growth: `1 if 42 in s else 0` → 1 (i64 via if-expr).
    let mem = zero_arg_i64(
        "g",
        grow_set_stmts(50),
        Expr::IfExpr {
            cond: Box::new(Expr::SetContains {
                set: Box::new(ident("s")),
                elem: Box::new(Expr::LitInt(42)),
            }),
            then_expr: Box::new(Expr::LitInt(1)),
            else_expr: Box::new(Expr::LitInt(0)),
        },
    );
    assert_eq!(
        run("g", &emit_module(&mem).unwrap()),
        Ok(1),
        "42 in grown set"
    );

    // dict grown to 40 → d[35] = 70.
    let dm = zero_arg_i64(
        "g",
        grow_dict_stmts(40),
        Expr::DictGet {
            dict: Box::new(ident("d")),
            key: Box::new(Expr::LitInt(35)),
        },
    );
    assert_eq!(
        run("g", &emit_module(&dm).unwrap()),
        Ok(70),
        "grown dict d[35]=35*2"
    );

    // dict len after growth → 40.
    let dl = zero_arg_i64("g", grow_dict_stmts(40), Expr::Len(Box::new(ident("d"))));
    assert_eq!(
        run("g", &emit_module(&dl).unwrap()),
        Ok(40),
        "grown dict len"
    );

    // absent key after growth still TRAPS (KeyError): d[99].
    let dk = zero_arg_i64(
        "g",
        grow_dict_stmts(40),
        Expr::DictGet {
            dict: Box::new(ident("d")),
            key: Box::new(Expr::LitInt(99)),
        },
    );
    assert_eq!(
        run("g", &emit_module(&dk).unwrap()),
        Err(()),
        "d[99] absent → KeyError trap"
    );

    eprintln!(
        "PMAT-999: dict/set GROWTH witness PASSED — a set grown to 50 and a dict \
         to 40 (well past the initial slack of 16) via a while loop return the \
         correct len / membership / value == CPython, absent keys still trap. \
         The fixed-capacity trap is retired; the region 2x-reallocs + copies."
    );
}
