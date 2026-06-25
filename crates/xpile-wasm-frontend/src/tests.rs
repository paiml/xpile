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

// ─── Structured control-flow round-trip (PMAT-959) ──────────────────
//
// The lift now RECOVERS the canonical control shapes xpile emits — the
// control half of bidirectional WASM. Each fixture asserts the executed
// round-trip fixed point `emit(lift(emit(M))) == emit(M)` (the honest
// right-inverse-on-image property; NOT `lift(emit(M)) == M`) AND that the
// lifted meta-HIR has the expected structured node.

fn whileloop(cond: Expr, body: Vec<Stmt>) -> Stmt {
    Stmt::While { cond, body }
}

fn let_mut(name: &str, value: Expr) -> Stmt {
    Stmt::Let {
        name: name.to_string(),
        ty: Type::I64,
        value,
        mutable: true,
    }
}

fn assign(name: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: name.to_string(),
        value,
    }
}

#[test]
fn roundtrip_while_sum() {
    // def count(n): i = 0; total = 0; while i < n: total = total + i; i = i + 1; return total
    let m = module(
        "while_mod",
        vec![func(
            "count",
            vec![p("n", Type::I64)],
            Type::I64,
            Block {
                stmts: vec![
                    let_mut("i", Expr::LitInt(0)),
                    let_mut("total", Expr::LitInt(0)),
                    whileloop(
                        binop(BinOp::Lt, ident("i"), ident("n")),
                        vec![
                            assign("total", binop(BinOp::Add, ident("total"), ident("i"))),
                            assign("i", binop(BinOp::Add, ident("i"), Expr::LitInt(1))),
                        ],
                    ),
                ],
                trailing_return: ident("total"),
            },
        )],
    );
    let lifted = roundtrip(&m);
    let Item::Function(f) = &lifted.items[0] else {
        panic!();
    };
    // The `(block $brk (loop $cont …))` idiom recovered as a `While`.
    let has_while = f.body.stmts.iter().any(|s| matches!(s, Stmt::While { .. }));
    assert!(has_while, "while loop recovered: {:?}", f.body.stmts);
    // The loop body's two `Assign`s round-trip.
    let Some(Stmt::While { cond, body }) = f
        .body
        .stmts
        .iter()
        .find(|s| matches!(s, Stmt::While { .. }))
    else {
        panic!();
    };
    assert!(matches!(cond, Expr::BinOp { op: BinOp::Lt, .. }));
    assert_eq!(body.len(), 2, "two assigns in the loop body");
}

#[test]
fn roundtrip_if_else_statement_max() {
    // def maxst(a, b): if a > b: return a else: return b; return b
    // The if/else statement shape (decy-style) → `Stmt::If` with `Return`
    // arms; the trailing `local.get $b` is the fallthrough.
    let m = module(
        "ifst_mod",
        vec![func(
            "maxst",
            vec![p("a", Type::I64), p("b", Type::I64)],
            Type::I64,
            Block {
                stmts: vec![Stmt::If {
                    cond: binop(BinOp::Gt, ident("a"), ident("b")),
                    then_body: vec![Stmt::Return(ident("a"))],
                    else_body: vec![Stmt::Return(ident("b"))],
                }],
                trailing_return: ident("b"),
            },
        )],
    );
    let lifted = roundtrip(&m);
    let Item::Function(f) = &lifted.items[0] else {
        panic!();
    };
    assert_eq!(f.body.stmts.len(), 1);
    let Stmt::If {
        cond,
        then_body,
        else_body,
    } = &f.body.stmts[0]
    else {
        panic!("expected a statement-if, got {:?}", f.body.stmts[0]);
    };
    assert!(matches!(cond, Expr::BinOp { op: BinOp::Gt, .. }));
    assert!(matches!(then_body.as_slice(), [Stmt::Return(_)]));
    assert!(matches!(else_body.as_slice(), [Stmt::Return(_)]));
}

#[test]
fn roundtrip_if_expr_max() {
    // def maxx(a, b): return a if a > b else b — the `if (result i64) …`
    // shape lifts to an `Expr::IfExpr`.
    let m = module(
        "ifexpr_mod",
        vec![func(
            "maxx",
            vec![p("a", Type::I64), p("b", Type::I64)],
            Type::I64,
            Block {
                stmts: vec![],
                trailing_return: Expr::IfExpr {
                    cond: Box::new(binop(BinOp::Gt, ident("a"), ident("b"))),
                    then_expr: Box::new(ident("a")),
                    else_expr: Box::new(ident("b")),
                },
            },
        )],
    );
    let lifted = roundtrip(&m);
    let Item::Function(f) = &lifted.items[0] else {
        panic!();
    };
    assert!(
        matches!(f.body.trailing_return, Expr::IfExpr { .. }),
        "if-expr recovered: {:?}",
        f.body.trailing_return
    );
}

#[test]
fn roundtrip_nested_while_with_if_break() {
    // A nested case: a while loop whose body contains an if/else statement
    // that `break`s — exercises recursion (While ⊃ If ⊃ Break) and the
    // `br $brk` → Break / `br $cont` → Continue recovery.
    //
    // def f(n): i = 0; while i < n: if i == 3: break else: i = i + 1; return i
    let m = module(
        "nested_mod",
        vec![func(
            "f",
            vec![p("n", Type::I64)],
            Type::I64,
            Block {
                stmts: vec![
                    let_mut("i", Expr::LitInt(0)),
                    whileloop(
                        binop(BinOp::Lt, ident("i"), ident("n")),
                        vec![Stmt::If {
                            cond: binop(BinOp::Eq, ident("i"), Expr::LitInt(3)),
                            then_body: vec![Stmt::Break],
                            else_body: vec![assign(
                                "i",
                                binop(BinOp::Add, ident("i"), Expr::LitInt(1)),
                            )],
                        }],
                    ),
                ],
                trailing_return: ident("i"),
            },
        )],
    );
    let lifted = roundtrip(&m);
    let Item::Function(f) = &lifted.items[0] else {
        panic!();
    };
    // While ⊃ If ⊃ Break recovered through the recursion.
    let Some(Stmt::While { body, .. }) = f
        .body
        .stmts
        .iter()
        .find(|s| matches!(s, Stmt::While { .. }))
    else {
        panic!("outer while recovered");
    };
    let Some(Stmt::If {
        then_body,
        else_body,
        ..
    }) = body.iter().find(|s| matches!(s, Stmt::If { .. }))
    else {
        panic!("inner if recovered inside the loop body");
    };
    assert!(
        matches!(then_body.as_slice(), [Stmt::Break]),
        "break recovered in the then-arm"
    );
    assert!(
        matches!(else_body.as_slice(), [Stmt::Assign { .. }]),
        "assign recovered in the else-arm"
    );
}

// ─── Honest-refusal (the moved lossy boundary) ──────────────────────

#[test]
fn refuses_noncanonical_block() {
    // The lift is a right-inverse ON THE EMIT IMAGE — a hand-written
    // `(block …)` whose label is NOT the canonical `$brk` is OUTSIDE the
    // image, so the lift REFUSES it rather than mis-reconstructing. (The
    // honest boundary moved by PMAT-959, it did not disappear.)
    let wat = "\
(module
  ;; source module: weird_mod
  (func $weird (param $n i64) (result i64)
    (block $other
      local.get $n
      br_if $other
    )
    local.get $n
  )
)";
    let err = lift_wat("weird_mod", wat).expect_err("non-canonical block must be refused");
    let msg = format!("{err}");
    assert!(
        msg.contains("non-canonical") || msg.contains("outside the lift subset"),
        "refusal must name the moved boundary, got: {msg}"
    );
}

#[test]
fn refuses_unknown_instruction() {
    // A WAT instruction xpile's emit never produces (here a `memory.grow`)
    // is refused — the lift only inverts the codegen image.
    let wat = "\
(module
  ;; source module: mem_mod
  (func $g (param $n i64) (result i64)
    local.get $n
    memory.grow
  )
)";
    let err = lift_wat("mem_mod", wat).expect_err("unknown instruction must be refused");
    assert!(format!("{err}").contains("outside the lift subset"));
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
