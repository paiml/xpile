//! PMAT-1402 — execution witness for the ARITHMETIC half of the WASM i64
//! honesty debt (PMAT-1379 shipped the shift-COUNT half).
//!
//! Raw WASM `i64.add` / `i64.sub` / `i64.mul` WRAP on signed overflow. The
//! lane emitted them bare — under a comment that already said "checked
//! overflow trap posture" — so this program
//!
//! ```python
//! def overflow_mul() -> int:
//!     x = 1
//!     i = 0
//!     while i < 64:
//!         x = x * 2
//!         i = i + 1
//!     return x
//! ```
//!
//! exited 0 and answered `0` under `wasm-interp`, where CPython answers
//! `18446744073709551616`.
//!
//! WHY THIS IS A DEFECT AND NOT A SCOPE DECISION — and why the first test
//! below puts ONE module through TWO backends: the same source through
//! `--target rust` emits
//! `checked_mul(…).expect("xpile: i64 multiplication overflow; …")`. The Rust
//! lane already settled "Python bigint is out of scope" the HONEST way, by
//! failing loudly. WASM did not follow. One backend lied and the other did
//! not, and an asymmetry is only assertable where both lanes are in scope —
//! hence the `xpile-rust-codegen` dev-dependency.
//!
//! The fix routes `+`/`-`/`*` (and the `x * -1` form unary `-x` lowers to)
//! through `$__wasm_add_i64` / `$__wasm_sub_i64` / `$__wasm_mul_i64`, each of
//! which traps with `unreachable` — the WAT analogue of the Rust lane's
//! panicking `expect` — exactly when the true result leaves i64.
//!
//! ## SCOPE, asserted rather than implied
//!
//! * `**` does NOT lower at all: `BinOp::Pow` has no arm in `emit_binop` and
//!   falls through to the honest refusal. Pinned by
//!   `pow_over_i64_is_refused_not_wrapped`, so "we covered `**`" can never be
//!   read off silence.
//! * Arbitrary precision is NOT attempted. Every overflow TRAPS; none
//!   promotes.
//! * The `<<` in-range residual PMAT-1379 pinned
//!   (`shl_in_range_overflow_is_a_known_residual` in `shift_count_witness.rs`)
//!   is UNTOUCHED by this slice: `1 << 63` still wraps to `i64::MIN`. That
//!   test's prose named "the general i64-overflow work (checked
//!   add/sub/mul/neg)" as the owner of the residual, and that work is THIS
//!   slice — so the prose has been corrected there to stop pointing at a
//!   shipped slice, and `shl_residual_and_mul_disagree_on_the_same_value`
//!   below MEASURES the surviving disagreement instead of describing it.
//! * `abs(i64::MIN)` remains the documented `$__wasm_abs_i64` wrap. Out of
//!   scope here; `NumBuiltin` never reaches `emit_binop`.
//!
//! Every executing test gates on `wasm_runtime_available()`.

use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, SourceLang, Stmt, Type, UnOp};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ── Fixture builders ────────────────────────────────────────────────────────

fn bin(op: BinOp, x: i64, y: i64) -> Expr {
    Expr::BinOp {
        op,
        lhs: Box::new(Expr::LitInt(x)),
        rhs: Box::new(Expr::LitInt(y)),
    }
}

/// A zero-arg `fn <name>() -> i64 { <tail> }` module.
fn int_fn(name: &str, tail: Expr) -> Module {
    Module {
        name: name.into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(Function {
            name: name.into(),
            params: vec![],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: tail,
            },
        })],
        ffi_boundaries: Vec::new(),
    }
}

/// The queue's exact repro, as meta-HIR:
///
/// ```python
/// def overflow_mul() -> int:
///     x = 1
///     i = 0
///     while i < 64:
///         x = x * 2
///         i = i + 1
///     return x
/// ```
fn overflow_mul_module() -> Module {
    let ident = |n: &str| Expr::Ident(n.to_string());
    let binop = |op, l: Expr, r: Expr| Expr::BinOp {
        op,
        lhs: Box::new(l),
        rhs: Box::new(r),
    };
    Module {
        name: "overflow_mul".into(),
        source_lang: SourceLang::Python,
        items: vec![Item::Function(Function {
            name: "overflow_mul".into(),
            params: vec![],
            return_type: Type::I64,
            body: Block {
                stmts: vec![
                    Stmt::Let {
                        name: "x".into(),
                        ty: Type::I64,
                        value: Expr::LitInt(1),
                        mutable: true,
                    },
                    Stmt::Let {
                        name: "i".into(),
                        ty: Type::I64,
                        value: Expr::LitInt(0),
                        mutable: true,
                    },
                    Stmt::While {
                        cond: binop(BinOp::Lt, ident("i"), Expr::LitInt(64)),
                        body: vec![
                            Stmt::Assign {
                                name: "x".into(),
                                value: binop(BinOp::Mul, ident("x"), Expr::LitInt(2)),
                            },
                            Stmt::Assign {
                                name: "i".into(),
                                value: binop(BinOp::Add, ident("i"), Expr::LitInt(1)),
                            },
                        ],
                    },
                ],
                trailing_return: ident("x"),
            },
        })],
        ffi_boundaries: Vec::new(),
    }
}

// ── WABT runner ─────────────────────────────────────────────────────────────

/// Per-CALL unique temp dir — a per-TEST dir races when one test execs the
/// runtime many times (the documented multi-exec WABT landmine).
static SEQ: AtomicUsize = AtomicUsize::new(0);

/// Run a zero-arg i64 kernel; `Ok(i64)` value or `Err(())` on a trap.
///
/// `wasm-interp` prints integer exports UNSIGNED, so the printed token is
/// parsed as `u64` and REINTERPRETED at i64 — without that step every negative
/// expectation reads as a mismatch.
fn run(name: &str, wat: &str) -> Result<i64, ()> {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "xpile-i64ovf-{}-{}-{seq}",
        name,
        std::process::id()
    ));
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
        "wat2wasm rejected the emitted arithmetic module:\n{}\n{wat}",
        String::from_utf8_lossy(&a.stderr)
    );
    let r = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&bp)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&r.stdout);
    let stderr = String::from_utf8_lossy(&r.stderr);
    let both = format!("{stdout}{stderr}");
    let line = both
        .lines()
        .find(|l| l.starts_with(&format!("{name}()")))
        .unwrap_or_else(|| panic!("no `{name}()` line in wasm-interp output:\n{both}"));
    if line.contains("unreachable executed") || line.contains("integer overflow") {
        return Err(());
    }
    let raw = line.rsplit_once("i64:").expect("i64 result").1.trim();
    let bits: u64 = raw
        .parse()
        .unwrap_or_else(|_| panic!("parse u64 from {line:?}"));
    Ok(bits as i64)
}

/// Emit + run a zero-arg kernel built from `tail`.
fn eval(name: &str, tail: Expr) -> Result<i64, ()> {
    let wat = emit_module(&int_fn(name, tail)).expect("kernel lowers");
    run(name, &wat)
}

// ── CONSTRUCT: routing, gating, and the two-backend asymmetry ───────────────

/// THE HEADLINE. One meta-HIR module, both backends, confronted in a single
/// test — because "the rust lane is honest and the wasm lane is not" was the
/// whole finding, and a claim about two lanes cannot be checked one lane at a
/// time.
#[test]
fn both_backends_agree_that_i64_overflow_is_an_error() {
    let m = overflow_mul_module();

    let rust = xpile_rust_codegen::emit_module(&m).expect("the repro lowers to Rust");
    assert!(
        rust.contains("checked_mul"),
        "the Rust lane's honest posture is the BASELINE this slice brought the \
         WASM lane up to; if it ever stops emitting `checked_mul` this witness \
         is comparing against nothing:\n{rust}"
    );

    let wat = emit_module(&m).expect("the repro lowers to WAT");
    assert!(
        wat.contains("call $__wasm_mul_i64"),
        "`x * 2` must route through the checked multiply, not a bare \
         `i64.mul`:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_add_i64"),
        "`i + 1` must route through the checked add:\n{wat}"
    );
}

/// CONSTRUCT: the bare wrapping instructions survive ONLY inside the helper
/// bodies. A future emission site that reintroduces one reds here.
///
/// Counted over INSTRUCTION lines: the helpers' own `;;` comments name the
/// mnemonics, so a substring count over the module would score them twice.
#[test]
fn bare_wrapping_instructions_exist_only_inside_the_helpers() {
    // One module that pulls in all three helpers at once.
    let tail = Expr::BinOp {
        op: BinOp::Mul,
        lhs: Box::new(bin(BinOp::Add, 3, 4)),
        rhs: Box::new(bin(BinOp::Sub, 9, 2)),
    };
    let wat = emit_module(&int_fn("all_three", tail)).expect("lowers");
    let instr = |m: &str| -> usize {
        wat.lines()
            .map(str::trim)
            .filter(|l| !l.starts_with(";;"))
            .filter(|l| *l == m)
            .count()
    };
    assert_eq!(
        instr("i64.add"),
        1,
        "exactly one raw i64.add (inside $__wasm_add_i64) may exist:\n{wat}"
    );
    // `i64.mul` and `i64.sub` also occur inside the PRE-EXISTING
    // `FLOOR_HELPERS`, which are emitted UNCONDITIONALLY:
    // `$__wasm_floordiv_i64` carries the `q - 1` floor correction, and
    // `$__wasm_floormod_i64` is literally `a - b * floordiv(a, b)`. Both are
    // exact by construction (their operands came out of a division), so they
    // stay raw. The counts are PINNED at their live values rather than waved
    // at, so a fourth `i64.sub` — the shape this test exists to catch — has to
    // be justified by whoever adds it.
    assert_eq!(
        instr("i64.mul"),
        2,
        "raw i64.mul may appear only in $__wasm_mul_i64 and the floormod \
         helper:\n{wat}"
    );
    assert_eq!(
        instr("i64.sub"),
        3,
        "raw i64.sub may appear only in $__wasm_sub_i64, the floordiv floor \
         correction, and the floormod helper:\n{wat}"
    );
}

/// CONSTRUCT: the helpers are gated INDIVIDUALLY. They each carry
/// `unreachable`, so a module that only adds must not acquire a multiply's
/// trap — the property `len_of_list_param_reads_header`'s "len needs no trap"
/// assertion rests on.
#[test]
fn each_arithmetic_helper_is_laid_down_only_when_called() {
    let add_only = emit_module(&int_fn("a", bin(BinOp::Add, 1, 2))).expect("lowers");
    assert!(add_only.contains("$__wasm_add_i64"), "{add_only}");
    assert!(
        !add_only.contains("$__wasm_sub_i64") && !add_only.contains("$__wasm_mul_i64"),
        "an add-only module must not carry the sub/mul helpers:\n{add_only}"
    );

    let sub_only = emit_module(&int_fn("s", bin(BinOp::Sub, 1, 2))).expect("lowers");
    assert!(sub_only.contains("$__wasm_sub_i64"), "{sub_only}");
    assert!(
        !sub_only.contains("$__wasm_add_i64") && !sub_only.contains("$__wasm_mul_i64"),
        "a sub-only module must not carry the add/mul helpers:\n{sub_only}"
    );

    let mul_only = emit_module(&int_fn("m", bin(BinOp::Mul, 1, 2))).expect("lowers");
    assert!(mul_only.contains("$__wasm_mul_i64"), "{mul_only}");
    assert!(
        !mul_only.contains("$__wasm_add_i64") && !mul_only.contains("$__wasm_sub_i64"),
        "a mul-only module must not carry the add/sub helpers:\n{mul_only}"
    );

    // ... and a module with no i64 arithmetic carries none of them, and no
    // trap. Without this arm the three above would also pass if the gate were
    // "emit everything the module does not obviously exclude".
    let none = emit_module(&int_fn("n", Expr::LitInt(7))).expect("lowers");
    assert!(
        !none.contains("$__wasm_add_i64")
            && !none.contains("$__wasm_sub_i64")
            && !none.contains("$__wasm_mul_i64")
            && !none.contains("unreachable"),
        "an arithmetic-free module must carry no helper and no trap:\n{none}"
    );
}

/// CONSTRUCT: unary `-x` lowers to `x * -1` THROUGH the checked multiply. The
/// comment at that emission site claimed "negation of MIN traps" while
/// emitting a bare `i64.mul` that wrapped; the claim is only true now.
#[test]
fn unary_negation_routes_through_the_checked_multiply() {
    let m = int_fn(
        "neg",
        Expr::UnOp {
            op: UnOp::Neg,
            operand: Box::new(Expr::LitInt(5)),
        },
    );
    let wat = emit_module(&m).expect("`-x` lowers");
    assert!(
        wat.contains("call $__wasm_mul_i64"),
        "`-x` must go through the checked multiply:\n{wat}"
    );
}

/// CONSTRUCT — SCOPE PIN. `**` is named in this slice's scope, so its status
/// must be MEASURED rather than left to silence. It does not lower at all:
/// `BinOp::Pow` has no `emit_binop` arm and reaches the honest refusal. If a
/// future slice wires `**`, this test reds and whoever wires it has to decide
/// the overflow posture deliberately.
#[test]
fn pow_over_i64_is_refused_not_wrapped() {
    let err = emit_module(&int_fn("p", bin(BinOp::Pow, 2, 70)))
        .expect_err("`**` over i64 must be REFUSED, not silently lowered");
    let msg = err.to_string();
    assert!(
        msg.contains("Pow") && msg.contains("not in the scalar/control subset"),
        "the `**` refusal must name the operator and the subset, so the reason \
         is readable at the CLI: {msg}"
    );
}

// ── EXECUTED: the repro, and the boundary ───────────────────────────────────

/// EXECUTED: the queue's exact repro. Answered `0` before this slice; must
/// TRAP now. This is the acceptance criterion, run rather than asserted.
#[test]
fn the_repro_traps_instead_of_answering_zero() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1402: skipping repro witness — WABT absent");
        return;
    }
    let wat = emit_module(&overflow_mul_module()).expect("the repro lowers");
    assert_eq!(
        run("overflow_mul", &wat),
        Err(()),
        "`x *= 2` sixty-four times must TRAP — CPython answers 2**64, and the \
         pre-PMAT-1402 lane answered 0"
    );

    // The SAME loop stopped one iteration short stays exactly representable
    // and must NOT trap, so the test above is not passing because the loop
    // traps for some unrelated reason.
    let mut m = overflow_mul_module();
    let Item::Function(f) = &mut m.items[0] else {
        panic!("fixture shape");
    };
    let Stmt::While { cond, .. } = &mut f.body.stmts[2] else {
        panic!("fixture shape");
    };
    *cond = Expr::BinOp {
        op: BinOp::Lt,
        lhs: Box::new(Expr::Ident("i".into())),
        rhs: Box::new(Expr::LitInt(62)),
    };
    let wat62 = emit_module(&m).expect("lowers");
    assert_eq!(
        run("overflow_mul", &wat62),
        Ok(1i64 << 62),
        "2**62 fits in i64 and must be COMPUTED, not trapped"
    );
}

/// EXECUTED: the exact boundary of each operator. Every arm is a value whose
/// true result either fits i64 (must be computed) or does not (must trap) —
/// no arm is a round number chosen for looks.
#[test]
fn overflow_traps_and_in_range_arithmetic_is_exact() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1402: skipping boundary witness — WABT absent");
        return;
    }
    const MAX: i64 = i64::MAX;
    const MIN: i64 = i64::MIN;

    // (op, lhs, rhs, expected) — `None` means "must trap".
    let cases: &[(BinOp, i64, i64, Option<i64>)] = &[
        // ── + ──
        (BinOp::Add, MAX, 0, Some(MAX)),
        (BinOp::Add, MAX, 1, None),
        (BinOp::Add, MAX - 1, 1, Some(MAX)),
        (BinOp::Add, MIN, 0, Some(MIN)),
        (BinOp::Add, MIN, -1, None),
        (BinOp::Add, MIN + 1, -1, Some(MIN)),
        // Opposite signs can never overflow, however extreme.
        (BinOp::Add, MAX, MIN, Some(-1)),
        // ── - ──
        (BinOp::Sub, MIN, 1, None),
        (BinOp::Sub, MIN, 0, Some(MIN)),
        (BinOp::Sub, MIN + 1, 1, Some(MIN)),
        (BinOp::Sub, MAX, -1, None),
        (BinOp::Sub, MAX - 1, -1, Some(MAX)),
        // `0 - MIN` is `-MIN`, which does not fit.
        (BinOp::Sub, 0, MIN, None),
        (BinOp::Sub, 0, MAX, Some(-MAX)),
        // ── * ──
        (BinOp::Mul, 0, MIN, Some(0)),
        (BinOp::Mul, MIN, 0, Some(0)),
        (BinOp::Mul, 1, MIN, Some(MIN)),
        (BinOp::Mul, MIN, 1, Some(MIN)),
        // The divide-back's one un-evaluable input: `i64.div_s` traps on
        // MIN/-1, and that trap IS the right answer (|MIN| does not fit).
        (BinOp::Mul, -1, MIN, None),
        (BinOp::Mul, MIN, -1, None),
        (BinOp::Mul, -1, MAX, Some(-MAX)),
        // 2**31 * 2**31 == 2**62 fits; 2**32 * 2**32 == 2**64 does not.
        (BinOp::Mul, 1 << 31, 1 << 31, Some(1 << 62)),
        (BinOp::Mul, 1 << 32, 1 << 32, None),
        (BinOp::Mul, MAX, 2, None),
        (BinOp::Mul, MAX / 2, 2, Some(MAX - 1)),
        (BinOp::Mul, MIN / 2, 2, Some(MIN)),
        (BinOp::Mul, MIN / 2, -2, None),
    ];

    for (i, (op, l, r, want)) in cases.iter().enumerate() {
        let name = format!("c{i}");
        let got = eval(&name, bin(*op, *l, *r));
        match want {
            Some(v) => assert_eq!(
                got,
                Ok(*v),
                "{l} {op:?} {r} must compute {v} (case {i}), got {got:?}"
            ),
            None => assert_eq!(
                got,
                Err(()),
                "{l} {op:?} {r} overflows i64 and must TRAP (case {i}), got {got:?}"
            ),
        }
    }
}

/// EXECUTED: unary `-x`. `-i64::MIN` is the one input whose true value leaves
/// i64; it WRAPPED to `i64::MIN` before this slice, matching neither CPython
/// (`9223372036854775808`) nor the Rust lane (`checked_neg().expect(…)`).
#[test]
fn negating_i64_min_traps_and_every_other_negation_is_exact() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1402: skipping negation witness — WABT absent");
        return;
    }
    let neg = |v: i64| Expr::UnOp {
        op: UnOp::Neg,
        operand: Box::new(Expr::LitInt(v)),
    };
    assert_eq!(
        eval("nmin", neg(i64::MIN)),
        Err(()),
        "-i64::MIN does not fit in i64 and must TRAP (it wrapped to i64::MIN)"
    );
    for v in [0i64, 1, -1, 7, -7, i64::MAX, i64::MIN + 1] {
        assert_eq!(
            eval("nv", neg(v)),
            Ok(v.wrapping_neg()),
            "-({v}) must be computed exactly"
        );
    }
}

/// EXECUTED, against LIVE `python3`: a sweep of in-range values must be
/// unchanged by this slice. Trapping on overflow is worthless if it also
/// perturbed the answers that were already right.
#[test]
fn in_range_arithmetic_still_matches_cpython() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1402: skipping CPython differential — WABT absent");
        return;
    }
    let pairs: &[(i64, i64)] = &[
        (0, 0),
        (1, 1),
        (7, 3),
        (-7, 3),
        (7, -3),
        (-7, -3),
        (1_000_000, 1_000_003),
        (i32::MAX as i64, i32::MAX as i64),
        (-(i32::MAX as i64), 5),
        (4_294_967_296, 2),
    ];
    for (i, (a, b)) in pairs.iter().enumerate() {
        for (op, py) in [(BinOp::Add, "+"), (BinOp::Sub, "-"), (BinOp::Mul, "*")] {
            let out = Command::new("python3")
                .arg("-c")
                .arg(format!("print(({a}) {py} ({b}))"))
                .output()
                .expect("python3 must be invocable for the differential");
            assert!(out.status.success(), "python3 failed on {a} {py} {b}");
            let want: i64 = String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse()
                .expect("the sweep is chosen to stay inside i64");
            let name = format!("d{i}");
            assert_eq!(
                eval(&name, bin(op, *a, *b)),
                Ok(want),
                "{a} {py} {b}: WASM must still agree with live CPython"
            );
        }
    }
}

/// EXECUTED — the SURVIVING inconsistency, measured rather than described.
///
/// `2 * (1 << 62)` and `1 << 63` are the same mathematical value, `2**63`.
/// After this slice the multiply TRAPS on it and the shift still WRAPS to
/// `i64::MIN`, because PMAT-1379 fixed the shift COUNT and deliberately left
/// the shift VALUE alone. Recording that as an executed disagreement means it
/// cannot be forgotten, and it reds the day someone closes it — at which point
/// this test becomes the place to state the new posture.
#[test]
fn shl_residual_and_mul_disagree_on_the_same_value() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1402: skipping residual witness — WABT absent");
        return;
    }
    assert_eq!(
        eval("mul_2_63", bin(BinOp::Mul, 2, 1i64 << 62)),
        Err(()),
        "2 * 2**62 == 2**63 leaves i64 and now traps"
    );
    assert_eq!(
        eval("shl_2_63", bin(BinOp::Shl, 1, 63)),
        Ok(i64::MIN),
        "`1 << 63` is the SAME value and still wraps — PMAT-1379's pinned \
         shift-VALUE residual, untouched by PMAT-1402. If this stops wrapping, \
         update this test and the residual note in shift_count_witness.rs \
         together"
    );
}
