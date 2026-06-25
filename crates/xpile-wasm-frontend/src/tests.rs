//! Round-trip fixed-point witness for the WAT lift (PMAT-954).
//!
//! The lift is lossy, so we do NOT claim `lift(emit(M)) == M`. The honest,
//! checkable invariant is that the lift is a **right-inverse of emit on its
//! WAT image**: `emit(lift(emit(M))) == emit(M)`. Each fixture below builds
//! a straight-line scalar meta-HIR module, emits WAT, lifts it back, and
//! asserts the re-emitted WAT is byte-identical — an executed proof that the
//! lift reconstructs every instruction the emit produced.

use super::*;
use std::path::Path;
use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, Stmt, Type};

fn emit(m: &Module) -> String {
    xpile_wasm_codegen::emit_module(m).expect("emit should succeed for a scalar fixture")
}

/// Assert the round-trip fixed point and return the lifted module for
/// further structural assertions.
fn roundtrip(m: &Module) -> Module {
    let wat1 = emit(m);
    let lifted = lift_wat(&m.name, &wat1)
        .unwrap_or_else(|e| panic!("lift failed for `{}`: {e}\n--- WAT ---\n{wat1}", m.name));
    assert_eq!(
        lifted.source_lang,
        SourceLang::Wasm,
        "lifted module must be tagged SourceLang::Wasm"
    );
    let wat2 = emit(&lifted);
    assert_eq!(
        wat1, wat2,
        "round-trip fixed point failed for `{}`\n--- emit(M) ---\n{wat1}\n--- emit(lift(emit(M))) ---\n{wat2}",
        m.name
    );
    lifted
}

fn p(name: &str, ty: Type) -> Param {
    Param {
        name: name.to_string(),
        ty,
        mutable: false,
    }
}

fn func(name: &str, params: Vec<Param>, return_type: Type, body: Block) -> Item {
    Item::Function(Function {
        name: name.to_string(),
        params,
        return_type,
        body,
    })
}

fn module(name: &str, items: Vec<Item>) -> Module {
    Module {
        name: name.to_string(),
        source_lang: SourceLang::Python, // origin is irrelevant to the emit
        items,
        ffi_boundaries: Vec::new(),
    }
}

fn ident(n: &str) -> Expr {
    Expr::Ident(n.to_string())
}

fn binop(op: BinOp, l: Expr, r: Expr) -> Expr {
    Expr::BinOp {
        op,
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}

// ─── Fixtures ───────────────────────────────────────────────────────

#[test]
fn roundtrip_identity() {
    // fn identity(x: i64) -> i64 { x }
    let m = module(
        "ident_mod",
        vec![func(
            "identity",
            vec![p("x", Type::I64)],
            Type::I64,
            Block {
                stmts: vec![],
                trailing_return: ident("x"),
            },
        )],
    );
    let lifted = roundtrip(&m);
    assert_eq!(lifted.items.len(), 1);
    let Item::Function(f) = &lifted.items[0] else {
        panic!("expected a function");
    };
    assert_eq!(f.name, "identity");
    assert_eq!(f.params.len(), 1);
    assert_eq!(f.params[0].name, "x");
    assert_eq!(f.params[0].ty, Type::I64);
    assert_eq!(f.return_type, Type::I64);
}

#[test]
fn roundtrip_add() {
    // fn add(a: i64, b: i64) -> i64 { a + b }
    let m = module(
        "add_mod",
        vec![func(
            "add",
            vec![p("a", Type::I64), p("b", Type::I64)],
            Type::I64,
            Block {
                stmts: vec![],
                trailing_return: binop(BinOp::Add, ident("a"), ident("b")),
            },
        )],
    );
    let lifted = roundtrip(&m);
    let Item::Function(f) = &lifted.items[0] else {
        panic!();
    };
    // The body reconstructed as a binary Add over the two params.
    assert!(matches!(
        f.body.trailing_return,
        Expr::BinOp { op: BinOp::Add, .. }
    ));
}

#[test]
fn roundtrip_with_let() {
    // fn f(a, b) { let c = a + b; c * c }
    let m = module(
        "let_mod",
        vec![func(
            "f",
            vec![p("a", Type::I64), p("b", Type::I64)],
            Type::I64,
            Block {
                stmts: vec![Stmt::Let {
                    name: "c".to_string(),
                    ty: Type::I64,
                    value: binop(BinOp::Add, ident("a"), ident("b")),
                    mutable: false,
                }],
                trailing_return: binop(BinOp::Mul, ident("c"), ident("c")),
            },
        )],
    );
    let lifted = roundtrip(&m);
    let Item::Function(f) = &lifted.items[0] else {
        panic!();
    };
    // The `(local $c)` reconstructed as a `let c = …` statement.
    assert_eq!(f.body.stmts.len(), 1);
    assert!(matches!(
        &f.body.stmts[0],
        Stmt::Let { name, .. } if name == "c"
    ));
}

#[test]
fn roundtrip_floordiv() {
    // fn fd(a, b) { a // b } — exercises the floordiv helper-call lift.
    let m = module(
        "fd_mod",
        vec![func(
            "fd",
            vec![p("a", Type::I64), p("b", Type::I64)],
            Type::I64,
            Block {
                stmts: vec![],
                trailing_return: binop(BinOp::FloorDiv, ident("a"), ident("b")),
            },
        )],
    );
    let lifted = roundtrip(&m);
    let Item::Function(f) = &lifted.items[0] else {
        panic!();
    };
    // `call $__wasm_floordiv_i64` lifted back to the high-level FloorDiv.
    assert!(matches!(
        f.body.trailing_return,
        Expr::BinOp {
            op: BinOp::FloorDiv,
            ..
        }
    ));
}

#[test]
fn roundtrip_comparison_returns_bool() {
    // fn lt(a, b) -> bool { a < b } — i32-result + i64.lt_s lift.
    let m = module(
        "cmp_mod",
        vec![func(
            "lt",
            vec![p("a", Type::I64), p("b", Type::I64)],
            Type::Bool,
            Block {
                stmts: vec![],
                trailing_return: binop(BinOp::Lt, ident("a"), ident("b")),
            },
        )],
    );
    let lifted = roundtrip(&m);
    let Item::Function(f) = &lifted.items[0] else {
        panic!();
    };
    assert_eq!(f.return_type, Type::Bool);
}

#[test]
fn roundtrip_float_arith() {
    // fn scale(x: f64) -> f64 { x * 2.0 } — f64.const + f64.mul lift.
    let m = module(
        "scale_mod",
        vec![func(
            "scale",
            vec![p("x", Type::F64)],
            Type::F64,
            Block {
                stmts: vec![],
                trailing_return: Expr::FloatBinOp {
                    op: FloatOp::Mul,
                    lhs: Box::new(ident("x")),
                    rhs: Box::new(Expr::LitFloat(2.0)),
                },
            },
        )],
    );
    let lifted = roundtrip(&m);
    let Item::Function(f) = &lifted.items[0] else {
        panic!();
    };
    assert_eq!(f.params[0].ty, Type::F64);
    assert!(matches!(
        f.body.trailing_return,
        Expr::FloatBinOp {
            op: FloatOp::Mul,
            ..
        }
    ));
}

#[test]
fn roundtrip_intra_module_call() {
    // fn sq(x) { x * x }  fn g(x) { sq(x) + sq(x) } — intra-module Call lift.
    let m = module(
        "call_mod",
        vec![
            func(
                "sq",
                vec![p("x", Type::I64)],
                Type::I64,
                Block {
                    stmts: vec![],
                    trailing_return: binop(BinOp::Mul, ident("x"), ident("x")),
                },
            ),
            func(
                "g",
                vec![p("x", Type::I64)],
                Type::I64,
                Block {
                    stmts: vec![],
                    trailing_return: binop(
                        BinOp::Add,
                        Expr::Call {
                            callee: "sq".to_string(),
                            args: vec![ident("x")],
                        },
                        Expr::Call {
                            callee: "sq".to_string(),
                            args: vec![ident("x")],
                        },
                    ),
                },
            ),
        ],
    );
    let lifted = roundtrip(&m);
    // Both user functions lifted (the synthetic helpers were skipped).
    assert_eq!(lifted.items.len(), 2);
    let names: Vec<&str> = lifted
        .items
        .iter()
        .map(|it| match it {
            Item::Function(f) => f.name.as_str(),
            _ => "?",
        })
        .collect();
    assert_eq!(names, vec!["sq", "g"]);
}

// ─── Honest-refusal (lossy boundary) ────────────────────────────────

#[test]
fn refuses_control_flow() {
    // A function with a `while` loop emits `(block …)`/`(loop …)`/`br_if` —
    // structured recovery is deferred (PMAT-952), so the lift REFUSES it
    // rather than mis-reconstructing.
    let m = module(
        "loop_mod",
        vec![func(
            "count",
            vec![p("n", Type::I64)],
            Type::I64,
            Block {
                stmts: vec![
                    Stmt::Let {
                        name: "i".to_string(),
                        ty: Type::I64,
                        value: Expr::LitInt(0),
                        mutable: true,
                    },
                    Stmt::While {
                        cond: binop(BinOp::Lt, ident("i"), ident("n")),
                        body: vec![Stmt::Assign {
                            name: "i".to_string(),
                            value: binop(BinOp::Add, ident("i"), Expr::LitInt(1)),
                        }],
                    },
                ],
                trailing_return: ident("i"),
            },
        )],
    );
    let wat = emit(&m);
    let err = lift_wat(&m.name, &wat).expect_err("control flow must be refused");
    let msg = format!("{err}");
    assert!(
        msg.contains("outside the lift subset") || msg.contains("PMAT-952"),
        "refusal must name the lossy boundary, got: {msg}"
    );
}

// ─── Frontend trait wiring ──────────────────────────────────────────

#[test]
fn frontend_trait_surface() {
    let fe = WasmFrontend::new();
    assert_eq!(fe.name(), "wasm");
    assert_eq!(fe.extensions(), &["wat"]);
    assert!(fe.matches_path(Path::new("foo.wat")));
    assert!(!fe.matches_path(Path::new("foo.py")));
}

#[test]
fn frontend_parse_and_lower_recovers_module_name() {
    // The `;; source module: <name>` comment round-trips through the
    // frontend's path-based entry point.
    let m = module(
        "named_mod",
        vec![func(
            "id",
            vec![p("x", Type::I64)],
            Type::I64,
            Block {
                stmts: vec![],
                trailing_return: ident("x"),
            },
        )],
    );
    let wat = emit(&m);
    let fe = WasmFrontend::new();
    let lifted = fe
        .parse_and_lower(Path::new("/tmp/whatever.wat"), &wat)
        .expect("parse_and_lower should succeed");
    // The emit wrote `;; source module: named_mod`; the lift recovers it
    // (NOT the file stem `whatever`).
    assert_eq!(lifted.name, "named_mod");
}
