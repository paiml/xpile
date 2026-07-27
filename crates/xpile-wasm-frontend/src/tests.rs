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

/// PMAT-1402 — the fixture that would have caught PMAT-1379.
///
/// Every operator the emit routes through a `$__wasm_*` helper needs a
/// matching arm in `lift_call`, or `emit(lift(emit(M)))` is not even
/// well-formed for a module using that operator: the lift reconstructs an
/// `Expr::Call` to a helper that is not an item of the lifted module, and the
/// re-emit refuses it. PMAT-1379 moved `<<`/`>>` onto helpers and did not add
/// those arms; nothing went red because NO fixture in this file shifted. This
/// one exercises all five helper-routed i64 operators at once (`+`, `-`, `*`,
/// `<<`, `>>`), so the next operator that moves onto a helper cannot repeat it
/// silently.
#[test]
fn roundtrip_shift_and_arith() {
    // fn mix(a: i64, b: i64) -> i64 { ((a + b) - (a * b)) << ((a >> b) & 1) }
    let m = module(
        "mix_mod",
        vec![func(
            "mix",
            vec![p("a", Type::I64), p("b", Type::I64)],
            Type::I64,
            Block {
                stmts: vec![],
                trailing_return: binop(
                    BinOp::Shl,
                    binop(
                        BinOp::Sub,
                        binop(BinOp::Add, ident("a"), ident("b")),
                        binop(BinOp::Mul, ident("a"), ident("b")),
                    ),
                    binop(
                        BinOp::BitAnd,
                        binop(BinOp::Shr, ident("a"), ident("b")),
                        Expr::LitInt(1),
                    ),
                ),
            },
        )],
    );
    // `roundtrip` itself asserts the fixed point; the emit must additionally
    // have gone through the helpers, else this fixture proves nothing about
    // `lift_call`'s helper arms.
    let wat = emit(&m);
    for helper in [
        "call $__wasm_add_i64",
        "call $__wasm_sub_i64",
        "call $__wasm_mul_i64",
        "call $__wasm_shl_i64",
        "call $__wasm_shr_i64",
    ] {
        assert!(
            wat.contains(helper),
            "fixture must exercise `{helper}`, else the lift arm it covers is \
             untested:\n{wat}"
        );
    }
    let lifted = roundtrip(&m);
    let Item::Function(f) = &lifted.items[0] else {
        panic!();
    };
    // The outermost operator came back as `<<`, not as a call to a helper.
    assert!(matches!(
        f.body.trailing_return,
        Expr::BinOp { op: BinOp::Shl, .. }
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

// ─── PMAT-1392: the `i32.const` bool-encoding boundary ──────────────
//
// Until PMAT-1392 the lift folded EVERY nonzero `i32.const` to `true`
// (`LitBool(v != 0)`) at three code-identical sites. `i32.const 2` therefore
// re-emitted as `i32.const 1`: VALID WAT that `wat2wasm` accepts and
// `wasm-interp` runs to a DIFFERENT value than the source, at exit 0 on
// every leg — the sharpest exit-0-but-false shape, because nothing
// downstream can catch it. `.wat` IS an advertised frontend (`xpile
// transpile foo.wat`), so hand-written / third-party WAT reached this.
//
// The section below pins BOTH halves: the refusal at all THREE sites (each
// mutant derived from LIVE emit, not hand-written, so the surrounding shape
// is canonical by construction), and — under WABT — a live EXECUTION
// differential proving the accepted half is value-preserving and every
// refused literal is one whose runtime value the lift could not have
// represented.

use std::process::Command;

/// A `Stmt::Let` at an explicit type (the shared `let_mut` helper above is
/// I64-only; the bool locals here are what lower to `i32`).
fn let_mut_ty(name: &str, ty: Type, value: Expr) -> Stmt {
    Stmt::Let {
        name: name.to_string(),
        ty,
        value,
        mutable: true,
    }
}

/// A module whose emitted WAT contains exactly THREE `i32.const`
/// occurrences, one per lift site, in text order:
///   0. `i32.const 0` — the `flag` initialiser (straight-line body)
///   1. `i32.const 1` — the `while true` condition (loop condition)
///   2. `i32.const 1` — the `flag = true` assignment (loop body)
///
/// Mutating occurrence *k* therefore targets site *k* precisely.
fn three_site_module() -> Module {
    module(
        "i32const_mod",
        vec![func(
            "f",
            vec![],
            Type::Bool,
            Block {
                stmts: vec![
                    let_mut_ty("flag", Type::Bool, Expr::LitBool(false)),
                    whileloop(
                        Expr::LitBool(true),
                        vec![assign("flag", Expr::LitBool(true)), Stmt::Break],
                    ),
                ],
                trailing_return: ident("flag"),
            },
        )],
    )
}

/// Replace the `n`-th (0-based) occurrence of `needle` in `hay`.
fn replace_nth(hay: &str, needle: &str, n: usize, with: &str) -> String {
    let mut start = 0usize;
    for _ in 0..n {
        let at = hay[start..]
            .find(needle)
            .unwrap_or_else(|| panic!("occurrence {n} of `{needle}` not found"))
            + start;
        start = at + needle.len();
    }
    let at = hay[start..]
        .find(needle)
        .unwrap_or_else(|| panic!("occurrence {n} of `{needle}` not found"))
        + start;
    format!("{}{with}{}", &hay[..at], &hay[at + needle.len()..])
}

#[test]
fn i32_const_outside_the_bool_encoding_is_refused_at_all_three_sites() {
    let wat = emit(&three_site_module());

    // Vacuity guard 1: the UNMUTATED module lifts. Without this the three
    // refusals below would also pass if the fixture were malformed.
    lift_wat("i32const_mod", &wat).expect("the 0/1 fixture is inside the lift subset");

    // Vacuity guard 2: the site indices mean what the doc comment says.
    // `assert_eq!` not `>=` — a 4th occurrence would silently re-point the
    // mutants at the wrong site.
    assert_eq!(
        wat.matches("i32.const ").count(),
        3,
        "fixture must emit exactly one i32.const per lift site:\n{wat}"
    );

    // (occurrence, expected site phrase, phrase that must NOT appear)
    let sites: [(usize, &str, &str); 3] = [
        (0, "`i32.const 2` is outside", "loop"),
        (1, "`i32.const 2` in loop condition", "loop body"),
        (2, "`i32.const 2` in loop body", "loop condition"),
    ];
    for (occ, must, must_not) in sites {
        let mutant = replace_nth(&wat, "i32.const ", occ, "i32.const 2 ;;@ ");
        // The mutant is still WELL-FORMED WAT — the refusal below is a
        // SUBSET decision, not a parse failure.
        let err = lift_wat("i32const_mod", &mutant)
            .expect_err("i32.const 2 must be refused at site {occ}");
        let msg = format!("{err}");
        assert!(
            msg.contains(must),
            "site {occ}: refusal must name `{must}`, got: {msg}\n--- mutant ---\n{mutant}"
        );
        assert!(
            !msg.contains(must_not),
            "site {occ}: refusal mis-attributed (contains `{must_not}`): {msg}"
        );

        // The CONTROL for this site: the SAME position holding `1` still
        // lifts. Without it, a guard that killed the whole arm would pass.
        let control = replace_nth(&wat, "i32.const ", occ, "i32.const 1 ;;@ ");
        lift_wat("i32const_mod", &control)
            .unwrap_or_else(|e| panic!("site {occ} control (`i32.const 1`) must still lift: {e}"));
    }
}

#[test]
fn i32_const_bool_encoding_still_round_trips_to_the_fixed_point() {
    // The accepted half is untouched: `emit(lift(emit(M))) == emit(M)` for a
    // module carrying BOTH bool literals at all three sites.
    let lifted = roundtrip(&three_site_module());
    let Item::Function(f) = &lifted.items[0] else {
        panic!("function recovered");
    };
    assert!(
        matches!(
            f.body.stmts.first(),
            Some(Stmt::Let {
                value: Expr::LitBool(false),
                ..
            })
        ),
        "`i32.const 0` still inverts to LitBool(false), got: {:?}",
        f.body.stmts.first()
    );
    let Some(Stmt::While { cond, body }) = f
        .body
        .stmts
        .iter()
        .find(|s| matches!(s, Stmt::While { .. }))
    else {
        panic!("while recovered");
    };
    assert!(
        matches!(cond, Expr::LitBool(true)),
        "`i32.const 1` still inverts to LitBool(true) in the loop condition, got: {cond:?}"
    );
    assert!(
        matches!(
            body.first(),
            Some(Stmt::Assign {
                value: Expr::LitBool(true),
                ..
            })
        ),
        "`i32.const 1` still inverts to LitBool(true) in the loop body, got: {:?}",
        body.first()
    );
}

/// Assemble + run `wat_src`, returning the single export's printed value
/// (e.g. `"i32:2"`). Each call gets its OWN directory — a per-TEST dir races
/// when one body assembles several modules.
fn interp_single_export(wat_src: &str, tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!("xpile-1392-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat = dir.join("m.wat");
    let wasm = dir.join("m.wasm");
    std::fs::write(&wat, wat_src).expect("write wat");

    let asm = Command::new("wat2wasm")
        .arg(&wat)
        .arg("-o")
        .arg(&wasm)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        asm.status.success(),
        "wat2wasm rejected {tag}:\n{}\n--- src ---\n{wat_src}",
        String::from_utf8_lossy(&asm.stderr)
    );

    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm)
        .output()
        .expect("spawn wasm-interp");
    assert!(run.status.success(), "wasm-interp failed on {tag}");
    let out = String::from_utf8_lossy(&run.stdout);
    out.lines()
        .find_map(|l| l.split_once("=> "))
        .map(|(_, v)| v.trim().to_string())
        .unwrap_or_else(|| panic!("no exported value in wasm-interp output for {tag}:\n{out}"))
}

#[test]
fn i32_const_lift_is_value_preserving_or_refuses_execution_differential() {
    // The load-bearing test: a RELATION over live execution, not a hand-list.
    // For each literal, the reference value comes from actually RUNNING the
    // source module. Then:
    //   * lift accepted  ⟹ the round-tripped module must run to the SAME value
    //   * lift refused   ⟹ the reference value is outside the bool encoding
    // Pre-PMAT-1392 the N∈{2,-5,7,255} rows were all ACCEPTED and all ran to
    // `i32:1`, so the first arm fails on every one of them (red-then-green).
    if !xpile_wasm_codegen::wasm_runtime_available() {
        eprintln!("SKIP i32_const execution differential: WABT not invocable");
        return;
    }

    let mut accepted = 0usize;
    let mut refused = 0usize;
    for (i, n) in [0i64, 1, 2, -5, 7, 255].into_iter().enumerate() {
        let src = format!(
            "(module\n  ;; source module: c{i}\n  (func $f (result i32)\n    \
             i32.const {n}\n  )\n  (export \"f\" (func $f))\n)\n"
        );
        // Every source here is well-formed WAT — asserted inside the helper,
        // so a refusal below is xpile's SUBSET decision, never bad input.
        let reference = interp_single_export(&src, &format!("ref{i}"));

        match lift_wat(&format!("c{i}"), &src) {
            Ok(m) => {
                accepted += 1;
                let observed = interp_single_export(&emit(&m), &format!("rt{i}"));
                assert_eq!(
                    observed, reference,
                    "i32.const {n}: the lift ACCEPTED the module but the \
                     round-tripped WAT runs to {observed} where the source \
                     runs to {reference} — a silent value corruption at exit 0"
                );
                assert!(
                    reference == "i32:0" || reference == "i32:1",
                    "only the 0/1 bool encoding may be accepted, but \
                     i32.const {n} runs to {reference}"
                );
            }
            Err(e) => {
                refused += 1;
                assert!(
                    reference != "i32:0" && reference != "i32:1",
                    "i32.const {n} runs to {reference} — inside the bool \
                     encoding — yet the lift refused it: {e}"
                );
                assert!(
                    format!("{e}").contains("outside the lift subset"),
                    "refusal must use the honest-boundary phrasing: {e}"
                );
            }
        }
    }
    // Vacuity guards: neither arm may be empty. An "everything refuses" or
    // "everything accepts" regression passes every assertion above.
    assert_eq!(
        accepted, 2,
        "exactly i32.const 0 and 1 are inside the subset"
    );
    assert_eq!(refused, 4, "the other four literals are outside it");
    eprintln!(
        "witness[PMAT-1392]: {accepted} accepted (value-preserving, executed) \
         / {refused} refused, all 6 references executed under wasm-interp"
    );
}

// ─── PMAT-1421: bare i64 arithmetic the emit routes through helpers ──

/// Assemble + run a single-export module, returning either the printed
/// value (`"i64:16"`) or `"TRAP"` when the module traps. The PMAT-1392
/// helper above asserts `wasm-interp` succeeded, which cannot express the
/// trap half of this differential — four of the five opcodes below diverge
/// by TRAPPING, and a differential that panics on a trap reds with
/// "wasm-interp failed" instead of naming the divergence.
///
/// Each call gets its OWN directory: a per-TEST dir races when one body
/// assembles several modules.
fn interp_outcome(wat_src: &str, tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!("xpile-1421-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat = dir.join("m.wat");
    let wasm = dir.join("m.wasm");
    std::fs::write(&wat, wat_src).expect("write wat");

    let asm = Command::new("wat2wasm")
        .arg(&wat)
        .arg("-o")
        .arg(&wasm)
        .output()
        .expect("spawn wat2wasm");
    // Both legs must be well-formed WAT: a refusal below is xpile's SUBSET
    // decision and a divergence is a VALUE difference, never bad input.
    assert!(
        asm.status.success(),
        "wat2wasm rejected {tag}:\n{}\n--- src ---\n{wat_src}",
        String::from_utf8_lossy(&asm.stderr)
    );

    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm)
        .output()
        .expect("spawn wasm-interp");
    let out = String::from_utf8_lossy(&run.stdout);
    let err = String::from_utf8_lossy(&run.stderr);
    if out.contains("unreachable executed") || err.contains("unreachable executed") {
        return "TRAP".to_string();
    }
    out.lines()
        .find_map(|l| l.split_once("=> "))
        .map(|(_, v)| v.trim().to_string())
        .unwrap_or_else(|| panic!("no exported value for {tag}:\nstdout:{out}\nstderr:{err}"))
}

/// The corpus: `(mnemonic, lhs, rhs, inside_the_agreeing_domain)`.
///
/// The five helper-routed mnemonics appear with operands OUTSIDE the domain
/// where WASM and Python agree (shift count ≥ 64; arithmetic that overflows
/// i64), where the two semantics provably differ. The three bitwise
/// mnemonics are the CONTROL — still bare in the emit, exact under both
/// semantics — so an "everything refuses" regression cannot pass.
const BARE_OP_CORPUS: &[(&str, &str, &str, bool)] = &[
    // helper-routed (PMAT-1379 shifts, PMAT-1402 arithmetic) — must refuse
    ("i64.shr_s", "1024", "70", false),
    ("i64.shl", "1", "70", false),
    ("i64.add", "9223372036854775807", "1", false),
    ("i64.sub", "-9223372036854775808", "1", false),
    ("i64.mul", "4611686018427387904", "4", false),
    // still bare in the emit — must accept AND stay value-preserving
    ("i64.and", "12", "10", true),
    ("i64.or", "12", "10", true),
    ("i64.xor", "12", "10", true),
];

#[test]
fn bare_i64_arith_lift_is_value_preserving_or_refuses_execution_differential() {
    // The load-bearing test: a RELATION over live execution, not a hand-list.
    // For each mnemonic the reference OUTCOME comes from actually running the
    // hand-written source module. Then:
    //   * lift accepted ⟹ the round-tripped module must run to the SAME outcome
    //   * lift refused  ⟹ the mnemonic must be one the emit no longer produces
    //
    // Pre-PMAT-1421 all eight rows were ACCEPTED, and the five helper-routed
    // ones ran to a different outcome than their source (`i64.shr_s` to
    // `i64:0` against a reference of `i64:16`; the other four to `TRAP`
    // against a defined wraparound), so the first arm fails on every one of
    // them — red-then-green.
    if !xpile_wasm_codegen::wasm_runtime_available() {
        eprintln!("SKIP bare-i64 execution differential: WABT not invocable");
        return;
    }

    let mut accepted = 0usize;
    let mut refused = 0usize;
    let mut diverged: Vec<String> = Vec::new();
    for (i, (op, lhs, rhs, agrees)) in BARE_OP_CORPUS.iter().enumerate() {
        let src = format!(
            "(module\n  ;; source module: b{i}\n  (func $f (result i64)\n    \
             i64.const {lhs}\n    i64.const {rhs}\n    {op}\n  )\n  \
             (export \"f\" (func $f))\n)\n"
        );
        let reference = interp_outcome(&src, &format!("ref{i}"));

        match lift_wat(&format!("b{i}"), &src) {
            Ok(m) => {
                accepted += 1;
                let observed = interp_outcome(&emit(&m), &format!("rt{i}"));
                if observed != reference {
                    diverged.push(format!("{op}: source {reference}, round trip {observed}"));
                }
                assert_eq!(
                    observed, reference,
                    "{op} {lhs} {rhs}: the lift ACCEPTED the module but the \
                     round-tripped WAT runs to {observed} where the source runs \
                     to {reference} — a divergence at exit 0 on every leg"
                );
                assert!(
                    *agrees,
                    "{op} is routed through a `$__wasm_*` helper by the emit, so \
                     the bare opcode is outside the lift image and must refuse"
                );
            }
            Err(e) => {
                refused += 1;
                assert!(
                    !*agrees,
                    "{op} is still emitted bare and is exact under both \
                     semantics, so refusing it is over-refusal: {e}"
                );
                let msg = format!("{e}");
                assert!(
                    msg.contains("outside the lift subset"),
                    "refusal must use the honest-boundary phrasing: {msg}"
                );
                assert!(
                    msg.contains("$__wasm_"),
                    "refusal must name the helper the emit routes {op} through: {msg}"
                );
            }
        }
    }
    assert!(
        diverged.is_empty(),
        "accepted-but-divergent rows: {diverged:?}"
    );
    // Vacuity guards: neither arm may be empty. An "everything refuses" or
    // "everything accepts" regression passes every assertion above.
    assert_eq!(
        refused, 5,
        "the five helper-routed mnemonics are outside the lift image"
    );
    assert_eq!(
        accepted, 3,
        "the three bitwise mnemonics are still emitted bare and stay accepted"
    );
    eprintln!(
        "witness[PMAT-1421]: {accepted} accepted (value-preserving, executed) \
         / {refused} refused, all 8 references executed under wasm-interp"
    );
}

#[test]
fn helper_routed_bare_ops_refuse_at_all_three_lift_sites() {
    // The guard lives in `int_binop`, the SINGLE decision point shared by the
    // straight-line body, the loop condition and the loop body. This pins
    // that claim per SITE, so re-adding the arm to one path cannot pass.
    let sites: [(&str, &str); 3] = [
        (
            "straight-line body",
            "(module\n  ;; source module: s\n  (func $f (result i64)\n    \
             i64.const 1\n    i64.const 2\n    i64.add\n  )\n)",
        ),
        (
            "loop condition",
            "(module\n  ;; source module: s\n  (func $f (result i64)\n    \
             (local $x i64)\n    i64.const 0\n    local.set $x\n    \
             (block $brk (loop $cont\n      i64.const 1\n      i64.const 2\n      \
             i64.add\n      i32.eqz\n      br_if $brk\n      br $cont\n    ))\n    \
             local.get $x\n  )\n)",
        ),
        (
            "loop body",
            "(module\n  ;; source module: s\n  (func $f (result i64)\n    \
             (local $x i64)\n    i64.const 0\n    local.set $x\n    \
             (block $brk (loop $cont\n      i32.const 0\n      i32.eqz\n      \
             br_if $brk\n      i64.const 1\n      i64.const 2\n      i64.add\n      \
             local.set $x\n      br $cont\n    ))\n    local.get $x\n  )\n)",
        ),
    ];
    for (site, wat) in sites {
        let Err(err) = lift_wat("s", wat) else {
            panic!("bare `i64.add` must be refused in the {site}");
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("bare `i64.add`") && msg.contains("$__wasm_add_i64"),
            "{site}: refusal must name the opcode and its helper, got: {msg}"
        );
    }
}

#[test]
fn every_helper_routed_mnemonic_names_its_own_helper() {
    // A refusal COUNT is not attribution (PMAT-1419): assert each mnemonic
    // resolves to ITS helper, so a copy-paste that names `$__wasm_add_i64`
    // for all five cannot pass.
    for (op, helper) in [
        ("i64.add", "$__wasm_add_i64"),
        ("i64.sub", "$__wasm_sub_i64"),
        ("i64.mul", "$__wasm_mul_i64"),
        ("i64.shl", "$__wasm_shl_i64"),
        ("i64.shr_s", "$__wasm_shr_i64"),
    ] {
        let wat = format!(
            "(module\n  ;; source module: h\n  (func $f (result i64)\n    \
             i64.const 8\n    i64.const 2\n    {op}\n  )\n)"
        );
        let Err(err) = lift_wat("h", &wat) else {
            panic!("bare `{op}` must be refused");
        };
        let msg = format!("{err}");
        assert!(
            msg.contains(&format!("bare `{op}`")) && msg.contains(helper),
            "`{op}` must be attributed to `{helper}`, got: {msg}"
        );
    }
}

#[test]
fn the_emit_still_produces_no_bare_helper_routed_mnemonic_in_a_user_body() {
    // The PREMISE of the refusal, re-derived from the emitter rather than
    // asserted. If a future slice un-routes `+` back to a bare `i64.add`,
    // this reds and says the refusal has become over-refusal — the arm must
    // come back. `$__wasm_*` helper bodies DO contain the bare opcodes and
    // are skipped wholesale by the lift, so they are excluded here too.
    let m = module(
        "premise",
        vec![
            func(
                "arith",
                vec![p("a", Type::I64), p("b", Type::I64)],
                Type::I64,
                Block {
                    stmts: vec![],
                    // ((a * b) - a) + b — all three of `*`, `-`, `+`.
                    trailing_return: binop(
                        BinOp::Add,
                        binop(
                            BinOp::Sub,
                            binop(BinOp::Mul, ident("a"), ident("b")),
                            ident("a"),
                        ),
                        ident("b"),
                    ),
                },
            ),
            func(
                "shifts",
                vec![p("a", Type::I64), p("b", Type::I64)],
                Type::I64,
                Block {
                    stmts: vec![],
                    // (a >> b) << b — both `>>` and `<<`.
                    trailing_return: binop(
                        BinOp::Shl,
                        binop(BinOp::Shr, ident("a"), ident("b")),
                        ident("b"),
                    ),
                },
            ),
        ],
    );
    let wat = emit(&m);
    let user_bodies: String = wat
        .split("\n  (func ")
        .filter(|f| !f.starts_with("$__wasm_"))
        .collect::<Vec<_>>()
        .join("\n");
    for op in ["i64.add", "i64.sub", "i64.mul", "i64.shl", "i64.shr_s"] {
        assert!(
            !user_bodies.contains(op),
            "the emit produced a bare `{op}` in a user body — the PMAT-1421 \
             refusal has become over-refusal and the `int_binop` arm must be \
             restored:\n{user_bodies}"
        );
    }
    // Vacuity: the fixture really did exercise all five operators.
    for helper in [
        "$__wasm_add_i64",
        "$__wasm_sub_i64",
        "$__wasm_mul_i64",
        "$__wasm_shl_i64",
        "$__wasm_shr_i64",
    ] {
        assert!(
            user_bodies.contains(helper),
            "fixture must route through `{helper}` or the negative above is \
             vacuous:\n{user_bodies}"
        );
    }
}
