//! Round-trip fixed-point witness for the WAT lift (PMAT-954).
//!
//! The lift is lossy, so we do NOT claim `lift(emit(M)) == M`. The honest,
//! checkable invariant is that the lift is a **right-inverse of emit on the
//! part of its WAT image the lift accepts**: `emit(lift(emit(M))) ==
//! emit(M)`. Each fixture below builds a straight-line scalar meta-HIR
//! module, emits WAT, lifts it back, and asserts the re-emitted WAT is
//! byte-identical — an executed proof that the lift reconstructs every
//! instruction the emit produced.
//!
//! That qualifier is PMAT-1422's, and
//! `the_emit_image_round_trip_hole_is_measured_not_asserted` pins the set of
//! emitted constructs the lift refuses.
//!
//! ⚠️ **The measurement is only as wide as its corpus, and this file has been
//! caught by that twice.** The paragraph above used to end "…at exactly
//! `not` and float `/`" — established over 7 rows containing no float
//! builtin, no unary float `-` and no `F32`, so it could not have found the
//! other ten. PMAT-1423 re-measured over
//! [`emit_image_corpus`], which reaches every scalar construct the emit
//! accepts, and the hole is twelve. The sentence "a fixture corpus cannot
//! establish an unqualified claim about the whole image" was already in this
//! doc comment when the claim it warns about was written one screen below
//! it — so the corpus, not the prose, is the thing to grow.
//!
//! A second lesson from the same slice: the prior witness's oracle was
//! `lift_wat(..).is_ok()`, and four constructs passed it while the lift was
//! silently corrupting them. `Ok` is not the invariant — the FIXED POINT is.
//! See [`lift_ok_implies_the_lifted_module_still_emits`].

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

// ─── PMAT-1422: bare `f64.div`, and the measured emit-image hole ─────────

/// The f64 corpus: `(mnemonic, lhs, rhs, still_bare_in_the_emit)`.
///
/// `f64.div` appears three times — with a zero divisor (where WASM's IEEE
/// `inf`/`NaN` and Python's `ZeroDivisionError` provably differ) AND with an
/// ordinary divisor (where they agree exactly). Both must refuse: the lift
/// sees an OPCODE, not a runtime divisor, so the boundary is per-mnemonic.
/// The agreeing row is what makes that explicit rather than incidental.
///
/// `f64.{add,sub,mul}` are the CONTROL — still emitted bare and exact under
/// both semantics (IEEE 754 doubles either way), so an "everything refuses"
/// regression cannot pass.
const BARE_F64_CORPUS: &[(&str, &str, &str, bool)] = &[
    // guarded by the emit (PMAT-1002) — must refuse
    ("f64.div", "1.0", "0.0", false),
    ("f64.div", "0.0", "0.0", false),
    ("f64.div", "6.0", "3.0", false),
    // still bare in the emit — must accept AND stay value-preserving
    ("f64.add", "1.5", "2.25", true),
    ("f64.sub", "1.5", "2.25", true),
    ("f64.mul", "1.5", "2.25", true),
];

#[test]
fn bare_f64_div_lift_is_value_preserving_or_refuses_execution_differential() {
    // Same RELATION as the PMAT-1421 differential, over live execution:
    //   * lift accepted ⟹ the round-tripped module runs to the SAME outcome
    //   * lift refused  ⟹ the mnemonic is one the emit never produces bare
    //
    // Pre-PMAT-1422 all six rows were ACCEPTED, and the two zero-divisor rows
    // ran to a different outcome than their source (`inf` → TRAP, `nan` →
    // TRAP), so the first arm fails on both — red-then-green. The third
    // `f64.div` row (`6.0/3.0`) agreed even then, which is exactly why no
    // fixture caught this.
    if !xpile_wasm_codegen::wasm_runtime_available() {
        eprintln!("SKIP bare-f64 execution differential: WABT not invocable");
        return;
    }

    let mut accepted = 0usize;
    let mut refused = 0usize;
    let mut diverged: Vec<String> = Vec::new();
    for (i, (op, lhs, rhs, still_bare)) in BARE_F64_CORPUS.iter().enumerate() {
        let src = format!(
            "(module\n  ;; source module: f{i}\n  (func $f (result f64)\n    \
             f64.const {lhs}\n    f64.const {rhs}\n    {op}\n  )\n  \
             (export \"f\" (func $f))\n)\n"
        );
        let reference = interp_outcome(&src, &format!("fref{i}"));

        match lift_wat(&format!("f{i}"), &src) {
            Ok(m) => {
                accepted += 1;
                let observed = interp_outcome(&emit(&m), &format!("frt{i}"));
                if observed != reference {
                    diverged.push(format!(
                        "{op} {lhs} {rhs}: source {reference}, round trip {observed}"
                    ));
                }
                assert_eq!(
                    observed, reference,
                    "{op} {lhs} {rhs}: the lift ACCEPTED the module but the \
                     round-tripped WAT runs to {observed} where the source runs \
                     to {reference} — a divergence at exit 0 on every leg"
                );
                assert!(
                    *still_bare,
                    "{op} is guarded by the emit (PMAT-1002), so the bare opcode \
                     is outside the lift image and must refuse"
                );
            }
            Err(e) => {
                refused += 1;
                assert!(
                    !*still_bare,
                    "{op} is still emitted bare and is exact under both \
                     semantics, so refusing it is over-refusal: {e}"
                );
                let msg = format!("{e}");
                assert!(
                    msg.contains("outside the lift subset"),
                    "refusal must use the honest-boundary phrasing: {msg}"
                );
                assert!(
                    msg.contains("f64.eq") && msg.contains("unreachable"),
                    "refusal must name the zero-divisor guard the emit uses \
                     instead of the bare opcode: {msg}"
                );
            }
        }
    }
    assert!(
        diverged.is_empty(),
        "accepted-but-divergent rows: {diverged:?}"
    );
    // Vacuity guards: neither arm may be empty.
    assert_eq!(refused, 3, "every `f64.div` row is outside the lift image");
    assert_eq!(
        accepted, 3,
        "f64 add/sub/mul are still emitted bare and stay accepted"
    );
    eprintln!(
        "witness[PMAT-1422]: {accepted} accepted (value-preserving, executed) \
         / {refused} refused, all 6 references executed under wasm-interp"
    );
}

#[test]
fn bare_f64_div_refuses_at_all_three_lift_sites() {
    // The guard lives in `float_binop`, the SINGLE decision point shared by
    // the straight-line body, the loop condition and the loop body. Pinned
    // per SITE so re-adding the arm to one path cannot pass.
    let sites: [(&str, &str); 3] = [
        (
            "straight-line body",
            "(module\n  ;; source module: s\n  (func $f (result f64)\n    \
             f64.const 1.0\n    f64.const 0.0\n    f64.div\n  )\n)",
        ),
        (
            "loop condition",
            "(module\n  ;; source module: s\n  (func $f (result f64)\n    \
             (local $x f64)\n    f64.const 0.0\n    local.set $x\n    \
             (block $brk (loop $cont\n      f64.const 1.0\n      f64.const 0.0\n      \
             f64.div\n      i32.eqz\n      br_if $brk\n      br $cont\n    ))\n    \
             local.get $x\n  )\n)",
        ),
        (
            "loop body",
            "(module\n  ;; source module: s\n  (func $f (result f64)\n    \
             (local $x f64)\n    f64.const 0.0\n    local.set $x\n    \
             (block $brk (loop $cont\n      i32.const 0\n      i32.eqz\n      \
             br_if $brk\n      f64.const 1.0\n      f64.const 0.0\n      f64.div\n      \
             local.set $x\n      br $cont\n    ))\n    local.get $x\n  )\n)",
        ),
    ];
    for (site, wat) in sites {
        let Err(err) = lift_wat("s", wat) else {
            panic!("bare `f64.div` must be refused in the {site}");
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("bare `f64.div`") && msg.contains("ZeroDivisionError"),
            "{site}: refusal must name the opcode and the semantics it does \
             not carry, got: {msg}"
        );
    }
}

#[test]
fn the_emit_still_guards_every_float_division_in_a_user_body() {
    // The PREMISE of the refusal, re-derived from the EMITTER rather than
    // asserted. Note the shape difference from the PMAT-1421 premise test: a
    // user body DOES contain the token `f64.div` — what makes the bare opcode
    // out-of-image is that the emit always precedes it with the zero-divisor
    // guard. Asserting "no `f64.div` in a user body" would be false; asserting
    // "no UNGUARDED `f64.div`" is the real premise. If a future slice drops
    // the guard, this reds and says the refusal has become over-refusal.
    let m = module(
        "premise",
        vec![func(
            "d",
            vec![p("x", Type::F64), p("y", Type::F64)],
            Type::F64,
            Block {
                stmts: vec![],
                trailing_return: Expr::FloatBinOp {
                    op: xpile_meta_hir::FloatOp::Div,
                    lhs: Box::new(ident("x")),
                    rhs: Box::new(ident("y")),
                },
            },
        )],
    );
    let wat = emit(&m);
    let user_bodies: String = wat
        .split("\n  (func ")
        .filter(|f| !f.starts_with("$__wasm_"))
        .collect::<Vec<_>>()
        .join("\n");
    // Vacuity: the fixture really did emit a division.
    assert!(
        user_bodies.contains("f64.div"),
        "fixture must emit an `f64.div` or the guard assertion below is \
         vacuous:\n{user_bodies}"
    );
    let before = user_bodies
        .split("f64.div")
        .next()
        .expect("split always yields a first segment");
    assert!(
        before.contains("f64.eq") && before.contains("unreachable"),
        "the emit produced an UNGUARDED `f64.div` in a user body — the \
         PMAT-1422 refusal has become over-refusal and the `float_binop` arm \
         must be restored:\n{user_bodies}"
    );
}

// ─── PMAT-1423: the emit-image hole, MEASURED over a corpus that can
//     falsify the claim ────────────────────────────────────────────────

/// Every scalar construct the emit accepts, as a meta-HIR module. This is
/// the corpus the hole is measured over, and it is deliberately WIDER than
/// the claim it establishes.
///
/// PMAT-1422 measured the same claim over 7 rows — `not`, float `/`, int
/// `+`, int `//`, int `&`, float `+`, a comparison — and concluded the hole
/// was exactly the first two. It had no float builtin in it, no unary float
/// `-`, no `F32` and no `abs`/`min`/`max`/`sqrt`, so it could not have found
/// the other ten. That is the failure mode this crate's own test-module doc
/// warns about one screen above where the claim was written.
fn emit_image_corpus() -> Vec<(&'static str, Module)> {
    use xpile_meta_hir::{FloatOp, NumBuiltinOp, UnOp};

    fn one(ps: Vec<Param>, rt: Type, e: Expr) -> Module {
        module(
            "h",
            vec![func(
                "f",
                ps,
                rt,
                Block {
                    stmts: vec![],
                    trailing_return: e,
                },
            )],
        )
    }
    let i64_pp = || vec![p("a", Type::I64), p("b", Type::I64)];
    let f64_pp = || vec![p("x", Type::F64), p("y", Type::F64)];
    let f32_p = || vec![p("s", Type::F32)];
    let un = |op, e: Expr| Expr::UnOp {
        op,
        operand: Box::new(e),
    };
    let nb = |op, args: Vec<Expr>, of_float| Expr::NumBuiltin { op, args, of_float };
    let fb = |op, l: Expr, r: Expr| Expr::FloatBinOp {
        op,
        lhs: Box::new(l),
        rhs: Box::new(r),
    };

    vec![
        // ── integer arithmetic / bitwise ──
        (
            "int +",
            one(
                i64_pp(),
                Type::I64,
                binop(BinOp::Add, ident("a"), ident("b")),
            ),
        ),
        (
            "int //",
            one(
                i64_pp(),
                Type::I64,
                binop(BinOp::FloorDiv, ident("a"), ident("b")),
            ),
        ),
        (
            "int &",
            one(
                i64_pp(),
                Type::I64,
                binop(BinOp::BitAnd, ident("a"), ident("b")),
            ),
        ),
        (
            "int <<",
            one(
                i64_pp(),
                Type::I64,
                binop(BinOp::Shl, ident("a"), ident("b")),
            ),
        ),
        (
            "comparison",
            one(
                i64_pp(),
                Type::Bool,
                binop(BinOp::Lt, ident("a"), ident("b")),
            ),
        ),
        // ── unary ──
        (
            "unary - (int)",
            one(i64_pp(), Type::I64, un(UnOp::Neg, ident("a"))),
        ),
        (
            "unary ~ (int)",
            one(i64_pp(), Type::I64, un(UnOp::BitNot, ident("a"))),
        ),
        (
            "unary - (float)",
            one(f64_pp(), Type::F64, un(UnOp::Neg, ident("x"))),
        ),
        (
            "unary - (f32)",
            one(f32_p(), Type::F32, un(UnOp::Neg, ident("s"))),
        ),
        (
            "not",
            one(
                vec![p("a", Type::Bool)],
                Type::Bool,
                un(UnOp::Not, ident("a")),
            ),
        ),
        // ── float arithmetic ──
        (
            "float +",
            one(
                f64_pp(),
                Type::F64,
                fb(FloatOp::Add, ident("x"), ident("y")),
            ),
        ),
        (
            "float /",
            one(
                f64_pp(),
                Type::F64,
                fb(FloatOp::Div, ident("x"), ident("y")),
            ),
        ),
        // ── numeric builtins (the `$__wasm_*` helper-call family) ──
        (
            "abs (int)",
            one(
                i64_pp(),
                Type::I64,
                nb(NumBuiltinOp::Abs, vec![ident("a")], false),
            ),
        ),
        (
            "abs (float)",
            one(
                f64_pp(),
                Type::F64,
                nb(NumBuiltinOp::Abs, vec![ident("x")], true),
            ),
        ),
        (
            "min (int)",
            one(
                i64_pp(),
                Type::I64,
                nb(NumBuiltinOp::Min, vec![ident("a"), ident("b")], false),
            ),
        ),
        (
            "max (int)",
            one(
                i64_pp(),
                Type::I64,
                nb(NumBuiltinOp::Max, vec![ident("a"), ident("b")], false),
            ),
        ),
        (
            "math.sqrt",
            one(
                f64_pp(),
                Type::F64,
                nb(NumBuiltinOp::Sqrt, vec![ident("x")], true),
            ),
        ),
        (
            "math.floor",
            one(
                f64_pp(),
                Type::I64,
                nb(NumBuiltinOp::Floor, vec![ident("x")], true),
            ),
        ),
        (
            "math.ceil",
            one(
                f64_pp(),
                Type::I64,
                nb(NumBuiltinOp::Ceil, vec![ident("x")], true),
            ),
        ),
        // ── literals / type identity ──
        ("f64 literal", one(vec![], Type::F64, Expr::LitFloat(2.5))),
        ("f32 literal", one(vec![], Type::F32, Expr::LitFloat(2.5))),
        ("f32 passthrough", one(f32_p(), Type::F32, ident("s"))),
    ]
}

/// The instruction lines of the USER functions of an emitted module — the
/// `$__wasm_*` prelude is dropped by the lift, so it is dropped here too.
/// Signature, comment, `(local …)` declaration and closing-paren lines are
/// filtered out, leaving the mnemonics the lift actually has to invert.
fn user_body_lines(wat: &str) -> Vec<String> {
    wat.split("\n  (func ")
        .skip(1)
        .filter(|f| !f.starts_with("$__wasm_"))
        .flat_map(|f| f.lines().skip(1))
        .map(str::trim)
        .filter(|l| {
            !l.is_empty() && !l.starts_with(";;") && !l.starts_with('(') && !l.starts_with(')')
        })
        .map(str::to_string)
        .collect()
}

/// How one corpus row behaves under `emit → lift → emit`.
#[derive(Debug, PartialEq, Eq)]
enum RoundTrip {
    /// `emit(lift(emit(M))) == emit(M)` byte for byte.
    FixedPoint,
    /// Token streams agree; the bytes differ only in layout. Semantically a
    /// fixed point, and recorded separately rather than normalised away so
    /// the cosmetic residual stays visible.
    FixedPointModuloLayout,
    /// The lift refused — the honest hole.
    LiftRefused,
    /// ⚠️ The lift returned `Ok` and the re-emit then refused. This is the
    /// PMAT-1423 defect: a module the lift silently corrupted while
    /// reporting success. Must be empty.
    ReEmitRefused(String),
    /// ⚠️ Both legs succeeded but the WAT genuinely differs. Must be empty.
    NotAFixedPoint,
}

fn classify(m: &Module) -> RoundTrip {
    let wat1 = emit(m);
    let lifted = match lift_wat(&m.name, &wat1) {
        Ok(l) => l,
        Err(_) => return RoundTrip::LiftRefused,
    };
    match xpile_wasm_codegen::emit_module(&lifted) {
        Err(e) => RoundTrip::ReEmitRefused(format!("{e}")),
        Ok(wat2) if wat2 == wat1 => RoundTrip::FixedPoint,
        Ok(wat2)
            if wat2.split_whitespace().collect::<Vec<_>>()
                == wat1.split_whitespace().collect::<Vec<_>>() =>
        {
            RoundTrip::FixedPointModuloLayout
        }
        Ok(_) => RoundTrip::NotAFixedPoint,
    }
}

/// **The load-bearing invariant.** `lift_wat` returning `Ok` must mean the
/// lifted module is a module the emit accepts — anything less is a silent
/// corruption reported as success.
///
/// This is the property the PMAT-1423 defect violated, and the reason it
/// went unseen is that the prior witness's oracle was `lift_wat(..).is_ok()`.
/// Under that oracle `abs(int)`, `min`, `max` and `math.sqrt` all read as
/// "liftable" while the lift was reconstructing a call to a function pass 2
/// had just dropped. Measuring the FIXED POINT instead of the `Ok` is what
/// makes the four visible.
#[test]
fn lift_ok_implies_the_lifted_module_still_emits() {
    let mut corrupted: Vec<(&str, String)> = Vec::new();
    for (label, m) in emit_image_corpus() {
        if let RoundTrip::ReEmitRefused(why) = classify(&m) {
            corrupted.push((label, why));
        }
    }
    assert!(
        corrupted.is_empty(),
        "the lift reported SUCCESS for {} construct(s) whose lifted module the \
         emit then refused — a silent corruption, not a lift. Before PMAT-1423 \
         this was `abs(int)`/`min`/`max`/`math.sqrt`, each reconstructed as a \
         call to a `$__wasm_*` helper the lift had just dropped:\n{corrupted:#?}",
        corrupted.len()
    );
}

/// No lifted module may reference a function it does not define.
///
/// The general form of the same defect, keyed on the SHAPE (an unresolved
/// callee) rather than on the `$__wasm_*` namespace — so a future arm that
/// invents a callee for any other reason reds here too.
#[test]
fn no_lifted_module_references_a_function_it_does_not_define() {
    fn callees(e: &Expr, out: &mut Vec<String>) {
        match e {
            Expr::Call { callee, args } => {
                out.push(callee.clone());
                args.iter().for_each(|a| callees(a, out));
            }
            Expr::BinOp { lhs, rhs, .. } | Expr::FloatBinOp { lhs, rhs, .. } => {
                callees(lhs, out);
                callees(rhs, out);
            }
            Expr::UnOp { operand, .. } => callees(operand, out),
            Expr::NumBuiltin { args, .. } => args.iter().for_each(|a| callees(a, out)),
            Expr::IfExpr {
                cond,
                then_expr,
                else_expr,
            } => {
                callees(cond, out);
                callees(then_expr, out);
                callees(else_expr, out);
            }
            _ => {}
        }
    }

    let mut checked = 0usize;
    let mut dangling: Vec<(&str, String)> = Vec::new();
    for (label, m) in emit_image_corpus() {
        let Ok(lifted) = lift_wat(&m.name, &emit(&m)) else {
            continue;
        };
        checked += 1;
        let fns = || {
            lifted.items.iter().filter_map(|it| match it {
                Item::Function(f) => Some(f),
                _ => None,
            })
        };
        let defined: Vec<&str> = fns().map(|f| f.name.as_str()).collect();
        for f in fns() {
            let mut found = Vec::new();
            callees(&f.body.trailing_return, &mut found);
            for c in found {
                if !defined.contains(&c.as_str()) {
                    dangling.push((label, c));
                }
            }
        }
    }
    assert!(
        checked >= 8,
        "vacuity guard: only {checked} corpus rows lifted at all, so this \
         property would pass for free — the guard has become over-refusal"
    );
    assert!(
        dangling.is_empty(),
        "lifted module(s) call a function they do not define: {dangling:#?}"
    );
    eprintln!("witness[PMAT-1423]: {checked} lifted modules, 0 dangling callees");
}

/// The user-visible half, and the reason the frontend has to be the one to
/// refuse: **a dangling call is not caught by every backend.**
///
/// `--target wasm` refused the corrupted module ("not a function of this
/// WASM module"), which is exactly why the defect read as caught. It was
/// not: this pins that the Rust backend emits the same module at exit 0,
/// referencing a callee it never defines. Measured through the CLI on the
/// real defect, `rustc` rejects that output with `error[E0425]: cannot find
/// function `__wasm_sqrt_f64` in this scope`.
///
/// The module here is hand-built, NOT lifted — it must keep witnessing the
/// backend's behaviour after the frontend stops producing such modules,
/// which is the whole point.
#[test]
fn a_dangling_call_is_not_caught_by_the_rust_backend() {
    let m = module(
        "dangle",
        vec![func(
            "f",
            vec![p("a", Type::F64)],
            Type::F64,
            Block {
                stmts: vec![],
                trailing_return: Expr::Call {
                    callee: "__wasm_sqrt_f64".to_string(),
                    args: vec![ident("a")],
                },
            },
        )],
    );

    let rust = xpile_rust_codegen::emit_module(&m)
        .expect("the Rust backend accepts a module with an unresolved callee — that is the point");
    assert!(
        rust.contains("__wasm_sqrt_f64(a)"),
        "expected the unresolved callee to be emitted verbatim:\n{rust}"
    );
    assert!(
        !rust.contains("fn __wasm_sqrt_f64"),
        "the callee must be REFERENCED but never DEFINED — that is what makes \
         the output uncompilable:\n{rust}"
    );

    // The contrast that makes the frontend guard load-bearing: the WASM
    // backend DOES refuse, so a wasm-only check would have read as safe.
    assert!(
        xpile_wasm_codegen::emit_module(&m).is_err(),
        "the WASM backend is expected to refuse the same module — if it \
         stopped, the note above about why this went unseen is stale"
    );
    eprintln!(
        "witness[PMAT-1423]: rust backend emits an unresolved callee at exit 0; \
         wasm backend refuses the same module"
    );
}

/// The emit-image hole, MEASURED. The enforcement half of the module-doc
/// claim, and it reds in BOTH directions: closing the hole reds a refused
/// row, widening it reds a round-tripping row.
#[test]
fn the_emit_image_round_trip_hole_is_measured_not_asserted() {
    let mut refused: Vec<&str> = Vec::new();
    let mut fixed: Vec<&str> = Vec::new();
    let mut layout: Vec<&str> = Vec::new();
    let mut broken: Vec<(&str, RoundTrip)> = Vec::new();

    for (label, m) in emit_image_corpus() {
        match classify(&m) {
            RoundTrip::LiftRefused => refused.push(label),
            RoundTrip::FixedPoint => fixed.push(label),
            RoundTrip::FixedPointModuloLayout => layout.push(label),
            other => broken.push((label, other)),
        }
    }

    assert!(
        broken.is_empty(),
        "every corpus row must either round-trip or refuse; these did \
         neither:\n{broken:#?}"
    );

    // The hole. PMAT-1422 pinned this at `["not", "float /"]` over a corpus
    // that contained no other candidate; the real hole is six times that.
    assert_eq!(
        refused,
        vec![
            "unary - (float)",
            "unary - (f32)",
            "not",
            "float /",
            "abs (int)",
            "abs (float)",
            "min (int)",
            "max (int)",
            "math.sqrt",
            "math.floor",
            "math.ceil",
            "f32 literal",
        ],
        "the measured emit-image hole changed — update the module doc, the \
         `IN_IMAGE_UNINVERTED` table and the CHANGELOG to match"
    );
    assert_eq!(
        fixed,
        vec![
            "int +",
            "int //",
            "int &",
            "int <<",
            "comparison",
            "unary ~ (int)",
            "float +",
            "f64 literal",
            "f32 passthrough",
        ],
        "the set of constructs that round-trip byte-for-byte changed"
    );
    // Named, not normalised away: the emit writes a blank line after the
    // `i64.const -1` of an integer `-x` that the re-emit does not. Token
    // streams are identical, so this is layout only — a 0.1.619 cosmetic
    // lead, not a semantic divergence.
    assert_eq!(
        layout,
        vec!["unary - (int)"],
        "the set of rows that are a fixed point only MODULO LAYOUT changed"
    );

    // Every refusal must name the REAL reason. A refusal that blames "an
    // arbitrary stack-machine branch" for an in-image construct is what made
    // this hole read as out-of-image input for three slices running.
    for (label, m) in emit_image_corpus()
        .into_iter()
        .filter(|(l, _)| refused.contains(l))
    {
        let msg = format!(
            "{}",
            lift_wat(&m.name, &emit(&m)).expect_err("hole row must refuse")
        );
        assert!(
            msg.contains("IS inside the `xpile-wasm-codegen` emit image")
                || msg.contains("is the `xpile-wasm-codegen` prelude namespace"),
            "`{label}`: the refusal must say the construct is IN the emit \
             image, not blame an arbitrary stack-machine branch: {msg}"
        );
    }
    eprintln!(
        "witness[PMAT-1423]: hole = {} constructs, byte-fixed-point = {}, \
         layout-only = {}",
        refused.len(),
        fixed.len(),
        layout.len()
    );
}

/// The `IN_IMAGE_UNINVERTED` vocabulary must stay in step with the emit, in
/// BOTH directions.
///
/// * Every entry must be REACHED by some emitted construct — the guard
///   against PMAT-1421's shape, where an arm stayed live after the emit
///   stopped producing what it matched. A stale entry here would hand an
///   author a confident, wrong explanation.
/// * Every mnemonic the emit produces in a user body that the lift refuses
///   must BE an entry — the guard against PMAT-1422's shape, where a new
///   emitted opcode falls through to the generic "arbitrary stack-machine
///   branch" message that misdescribes it.
#[test]
fn every_uninverted_in_image_instruction_is_named_and_reachable() {
    // Collect the mnemonics the emit produces in USER bodies (the
    // `$__wasm_*` prelude is skipped by the lift, so it is skipped here).
    let mut produced: Vec<String> = Vec::new();
    for (_, m) in emit_image_corpus() {
        for line in user_body_lines(&emit(&m)) {
            let tok = line.split_whitespace().next().unwrap_or("").to_string();
            if !tok.is_empty() && !produced.contains(&tok) {
                produced.push(tok);
            }
        }
    }
    assert!(
        produced.len() > 15,
        "vacuity guard: only {} mnemonics collected from the corpus's user \
         bodies — the extractor is broken: {produced:?}",
        produced.len()
    );

    // Direction 1: no stale entry.
    let stale: Vec<&str> = IN_IMAGE_UNINVERTED
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !produced.iter().any(|p| p == k))
        .collect();
    assert!(
        stale.is_empty(),
        "`IN_IMAGE_UNINVERTED` names {stale:?} as in-image, but no corpus \
         construct emits them into a user body. Either the emit stopped \
         producing them (delete the entry — PMAT-1421's shape) or the corpus \
         no longer reaches them (add the construct)."
    );

    // Direction 2: every refusal the corpus ACTUALLY produces must name a
    // mnemonic that is in the vocabulary. Measured from the real refusal
    // messages rather than by driving `refuse_control` directly — most
    // emitted mnemonics (`local.get`, `i64.and`, `f64.add`, …) are handled
    // by the lift and never reach a refusal at all, so driving them through
    // it would manufacture a failure that cannot happen.
    let mut refused_rows = 0usize;
    for (label, m) in emit_image_corpus() {
        let Err(e) = lift_wat(&m.name, &emit(&m)) else {
            continue;
        };
        refused_rows += 1;
        let msg = format!("{e}");
        // The `$__wasm_*` prelude-namespace refusal names a callee, not a
        // mnemonic — a separate honest boundary (PMAT-1423).
        if msg.contains("prelude namespace") {
            continue;
        }
        let mnemonic = msg
            .split("WAT instruction `")
            .nth(1)
            .and_then(|s| s.split('`').next())
            .unwrap_or_else(|| {
                panic!("`{label}`: refusal names no WAT instruction: {msg}");
            });
        assert!(
            IN_IMAGE_UNINVERTED.iter().any(|(k, _)| *k == mnemonic),
            "`{label}`: the emit produces `{mnemonic}` in a user body and the \
             lift refuses it, but it is not in `IN_IMAGE_UNINVERTED` — so the \
             author is told they wrote an \"arbitrary stack-machine branch\" \
             they did not write. Add it with the construct it lowers.\n{msg}"
        );
    }
    assert!(
        refused_rows >= 10,
        "vacuity guard: only {refused_rows} corpus rows refused, so direction 2 \
         is checking almost nothing"
    );
    eprintln!(
        "witness[PMAT-1423]: {} in-image uninverted mnemonics, all reachable; \
         {} distinct mnemonics emitted across the corpus",
        IN_IMAGE_UNINVERTED.len(),
        produced.len()
    );
}

/// `INVERTIBLE_HELPERS` must list exactly the `$__wasm_*` names `lift_call`
/// has an arm for — it is quoted verbatim in the refusal message that tells
/// an author which helpers ARE supported, so a drifted list is a confidently
/// wrong answer.
/// The four `$__wasm_*` helpers the emit reaches from a scalar construct
/// with no inverse arm, plus one from the string family to show the guard is
/// keyed on the NAMESPACE rather than on this list.
const UNARMED_HELPERS: &[&str] = &[
    "__wasm_abs_i64",
    "__wasm_min_i64",
    "__wasm_max_i64",
    "__wasm_sqrt_f64",
    "__wasm_str_upper_lower",
];

#[test]
fn the_invertible_helper_list_matches_the_lift_call_arms() {
    // Every candidate's arity is KNOWN here on purpose: the pre-PMAT-1423
    // path found the helper in this very table and used it to build a
    // well-formed call, so a witness that left the arity out would prove
    // nothing about the guard.
    let mut arity: HashMap<String, usize> = INVERTIBLE_HELPERS
        .iter()
        .map(|h| ((*h).to_string(), 2usize))
        .collect();
    for h in UNARMED_HELPERS {
        arity.insert((*h).to_string(), 2);
    }
    let local_names = std::collections::HashSet::new();
    let local_ty = HashMap::new();
    let set_counts = HashMap::new();
    let ctx = BodyCtx {
        arity: &arity,
        local_names: &local_names,
        local_ty: &local_ty,
        set_counts: &set_counts,
        assigned: std::collections::HashSet::new(),
    };

    // Each listed helper must be INVERTED (produce a BinOp), not refused.
    for h in INVERTIBLE_HELPERS {
        let mut stack = vec![ident("a"), ident("b")];
        lift_call((*h).to_string(), &mut stack, &ctx)
            .unwrap_or_else(|e| panic!("`${h}` is listed as invertible but refused: {e}"));
        assert!(
            matches!(stack.as_slice(), [Expr::BinOp { .. }]),
            "`${h}` must invert to a high-level operator, got {stack:?}"
        );
    }

    // A helper NOT on the list must refuse even though its arity IS known.
    for h in UNARMED_HELPERS {
        let mut stack = vec![ident("a"), ident("b")];
        let err = lift_call((*h).to_string(), &mut stack, &ctx)
            .expect_err("an un-armed prelude helper must refuse, not dangle");
        let msg = format!("{err}");
        assert!(
            msg.contains("prelude namespace") && msg.contains(h),
            "the refusal must name the helper and the namespace: {msg}"
        );
        for listed in INVERTIBLE_HELPERS {
            assert!(
                msg.contains(listed),
                "the refusal must list `${listed}` as an invertible helper: {msg}"
            );
        }
    }
}
