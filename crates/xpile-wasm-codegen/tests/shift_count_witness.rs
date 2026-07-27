//! PMAT-1379 — execution witness for the SHIFT-COUNT half of the WASM i64
//! honesty debt.
//!
//! Raw WASM `i64.shl` / `i64.shr_s` MASK the shift count to its low 6 bits.
//! Before this slice the lane emitted them bare, so — measured against live
//! `python3` on 2026-07-27 — it returned four SILENT WRONG ANSWERS:
//!
//! | source      | emitted (before) | CPython                 |
//! |-------------|------------------|-------------------------|
//! | `1 << 70`   | `64`             | `1180591620717411303424`|
//! | `1024 >> 70`| `16`             | `0`                     |
//! | `8 << -1`   | `0`              | raises `ValueError`     |
//! | `8 >> -1`   | `0`              | raises `ValueError`     |
//!
//! `70 & 63 == 6`, hence `64` and `16`. Nothing about a 64-bit word makes
//! `x << 70` mean `x << 6`, so this was wrong even under fixed-width
//! semantics — and a NEGATIVE count, which Python rejects outright, masked
//! to a large positive one and returned a value.
//!
//! The fix routes both shifts through `$__wasm_shl_i64` / `$__wasm_shr_i64`,
//! which take the same posture the lane's `i64.div_s` zero-divisor trap
//! already takes:
//!
//! * negative count → `unreachable` (the `ValueError` analogue);
//! * `>>` with `n >= 64` → clamp to 63, which is EXACT: an arithmetic right
//!   shift that far is `0` for `x >= 0` and `-1` for `x < 0`, exactly what
//!   CPython returns for an arbitrary-precision `x`;
//! * `<<` with `n >= 64` → `0` when `x == 0` (the one representable case),
//!   `unreachable` otherwise, since every non-zero `x` overflows i64.
//!
//! SCOPE — asserted, not hidden. This slice fixes the shift COUNT only. A
//! count in `0..=63` still uses the raw instruction, so `1 << 63` still
//! WRAPS to `i64::MIN`. `shl_in_range_overflow_is_a_known_residual` PINS
//! that residual so it is machine-recorded rather than quietly implied; it
//! belongs to the general i64-overflow work (checked add/sub/mul/neg), which
//! is L/XL and sits on the emitter's hot path.
//!
//! Gated on `wasm_runtime_available()` like every other execution witness.

use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, SourceLang, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// `x << n` / `x >> n` over two integer literals.
fn shift(op: BinOp, x: i64, n: i64) -> Expr {
    Expr::BinOp {
        op,
        lhs: Box::new(Expr::LitInt(x)),
        rhs: Box::new(Expr::LitInt(n)),
    }
}

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

/// Per-CALL unique temp dir — a per-TEST dir races when one test execs the
/// runtime many times (the documented multi-exec WABT landmine).
static SEQ: AtomicUsize = AtomicUsize::new(0);

/// Run a zero-arg i64 kernel; `Ok(i64)` value or `Err(())` on a trap.
///
/// `wasm-interp` prints integer exports UNSIGNED, so the printed token is
/// parsed as `u64` and REINTERPRETED at the declared width — without that
/// step every negative expectation reads as a mismatch.
fn run(name: &str, wat: &str) -> Result<i64, ()> {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("xpile-shift-{}-{}-{seq}", name, std::process::id()));
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
        "wat2wasm rejected the emitted shift module:\n{}\n{wat}",
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
    if line.contains("unreachable executed") {
        return Err(());
    }
    let raw = line.rsplit_once("i64:").expect("i64 result").1.trim();
    let bits: u64 = raw
        .parse()
        .unwrap_or_else(|_| panic!("parse u64 from {line:?}"));
    Ok(bits as i64)
}

/// CONSTRUCT (no WABT): shifts must route through the helpers, and the bare
/// masking instructions must appear ONLY inside those helper bodies.
#[test]
fn shifts_route_through_the_count_honest_helpers() {
    let wat = emit_module(&int_fn("s", shift(BinOp::Shl, 1, 3))).expect("`<<` lowers");
    assert!(
        wat.contains("call $__wasm_shl_i64"),
        "`<<` must call the count-honest helper, not emit a bare i64.shl:\n{wat}"
    );
    let wat = emit_module(&int_fn("s", shift(BinOp::Shr, 8, 1))).expect("`>>` lowers");
    assert!(
        wat.contains("call $__wasm_shr_i64"),
        "`>>` must call the count-honest helper, not emit a bare i64.shr_s:\n{wat}"
    );
    // The helper bodies are the ONLY place a raw masking instruction may
    // appear: exactly one `i64.shl` (in `$__wasm_shl_i64`) and one
    // `i64.shr_s` (in `$__wasm_shr_i64`). A future emission site that
    // reintroduces a bare shift reds this. Counted over INSTRUCTION lines —
    // the helpers' own `;;` comments name both mnemonics, so a substring
    // count over the whole module would score them twice.
    let instr = |m: &str| -> usize {
        wat.lines()
            .map(str::trim)
            .filter(|l| !l.starts_with(";;"))
            .filter(|l| *l == m)
            .count()
    };
    assert_eq!(
        instr("i64.shl"),
        1,
        "exactly one raw i64.shl (inside $__wasm_shl_i64) may exist:\n{wat}"
    );
    assert_eq!(
        instr("i64.shr_s"),
        1,
        "exactly one raw i64.shr_s (inside $__wasm_shr_i64) may exist:\n{wat}"
    );
}

/// CONSTRUCT: the helpers are GATED. A module with no shift in it must carry
/// neither helper — they contain `unreachable`, and emitting them everywhere
/// would put a trap in every module the backend produces (which is exactly
/// what made `len_of_list_param_reads_header`'s "len needs no trap"
/// assertion unassertable on the first cut of this slice).
#[test]
fn shift_helpers_are_absent_from_a_shiftless_module() {
    let m = Module {
        name: "plain".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(Function {
            name: "plain".into(),
            params: vec![],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::BinOp {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::LitInt(2)),
                    rhs: Box::new(Expr::LitInt(3)),
                },
            },
        })],
        ffi_boundaries: Vec::new(),
    };
    let wat = emit_module(&m).expect("plain module lowers");
    assert!(
        !wat.contains("__wasm_shl_i64") && !wat.contains("__wasm_shr_i64"),
        "a shiftless module must not carry the shift helpers:\n{wat}"
    );
    assert!(
        !wat.contains("unreachable"),
        "a shiftless module must carry no trap at all:\n{wat}"
    );
}

/// EXECUTED: every count Python rejects TRAPS, and every count whose
/// fixed-width answer is exact is COMPUTED exactly.
#[test]
fn out_of_range_shift_counts_trap_or_saturate() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1379: skipping shift-count witness — WABT absent");
        return;
    }

    // ── the four measured silent-wrong-answer cases ──
    // `1 << 70` was 64. CPython gives a 71-bit integer; i64 cannot hold it,
    // so the honest answer is a trap.
    assert_eq!(
        run(
            "a",
            &emit_module(&int_fn("a", shift(BinOp::Shl, 1, 70))).unwrap()
        ),
        Err(()),
        "1 << 70 must TRAP (CPython 1180591620717411303424 is unrepresentable in i64; \
         the masked answer 64 was a silent wrong value)"
    );
    // `1024 >> 70` was 16. Clamping to 63 reproduces CPython EXACTLY.
    assert_eq!(
        run(
            "b",
            &emit_module(&int_fn("b", shift(BinOp::Shr, 1024, 70))).unwrap()
        ),
        Ok(0),
        "1024 >> 70 == 0 in CPython; the masked answer was 16"
    );
    // Negative counts raise ValueError in CPython; both were returning 0.
    assert_eq!(
        run(
            "c",
            &emit_module(&int_fn("c", shift(BinOp::Shl, 8, -1))).unwrap()
        ),
        Err(()),
        "8 << -1 must TRAP (CPython raises ValueError)"
    );
    assert_eq!(
        run(
            "d",
            &emit_module(&int_fn("d", shift(BinOp::Shr, 8, -1))).unwrap()
        ),
        Err(()),
        "8 >> -1 must TRAP (CPython raises ValueError)"
    );

    // ── the exact out-of-range arms ──
    // `0 << n` is representable for ANY n — it must NOT trap.
    for n in [64, 70, 1000, i64::MAX] {
        assert_eq!(
            run(
                "e",
                &emit_module(&int_fn("e", shift(BinOp::Shl, 0, n))).unwrap()
            ),
            Ok(0),
            "0 << {n} == 0 in CPython and is representable — it must not trap"
        );
    }
    // An arithmetic `>>` past the width saturates to the sign bit, for both
    // signs and arbitrarily far out. Each of these equals CPython exactly.
    for (x, n, want) in [
        (1024_i64, 64_i64, 0_i64),
        (1024, 70, 0),
        (1024, i64::MAX, 0),
        (-1, 64, -1),
        (-1, 70, -1),
        (-8, 100, -1),
        (-8, i64::MAX, -1),
        (i64::MIN, 64, -1),
        (i64::MAX, 64, 0),
    ] {
        assert_eq!(
            run(
                "f",
                &emit_module(&int_fn("f", shift(BinOp::Shr, x, n))).unwrap()
            ),
            Ok(want),
            "{x} >> {n} == {want} in CPython (arithmetic saturation to the sign bit)"
        );
    }
    // A non-zero `<<` past the width always overflows i64 → trap.
    for (x, n) in [(1_i64, 64_i64), (1, 65), (3, 100), (-1, 64), (1, i64::MAX)] {
        assert_eq!(
            run(
                "g",
                &emit_module(&int_fn("g", shift(BinOp::Shl, x, n))).unwrap()
            ),
            Err(()),
            "{x} << {n} must TRAP — every non-zero x overflows i64 at n >= 64"
        );
    }
}

/// EXECUTED: in-range counts are UNREGRESSED — each value below was checked
/// against live `python3` and matches exactly.
#[test]
fn in_range_shift_counts_are_unregressed() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1379: skipping shift-count regression witness — WABT absent");
        return;
    }
    // `<<` — in range, no overflow.
    for (x, n, want) in [
        (1_i64, 0_i64, 1_i64),
        (7, 0, 7),
        (1, 10, 1024),
        (-3, 4, -48),
        (1, 62, 4611686018427387904),
    ] {
        assert_eq!(
            run(
                "h",
                &emit_module(&int_fn("h", shift(BinOp::Shl, x, n))).unwrap()
            ),
            Ok(want),
            "{x} << {n} == {want} (live python3)"
        );
    }
    // `>>` — in range, both signs, including the 63 boundary.
    for (x, n, want) in [
        (1024_i64, 3_i64, 128_i64),
        (-9, 1, -5),
        (5, 63, 0),
        (-1, 63, -1),
        (i64::MIN, 63, -1),
        (i64::MAX, 62, 1),
        (0, 0, 0),
    ] {
        assert_eq!(
            run(
                "i",
                &emit_module(&int_fn("i", shift(BinOp::Shr, x, n))).unwrap()
            ),
            Ok(want),
            "{x} >> {n} == {want} (live python3)"
        );
    }
}

/// PINNED RESIDUAL — this slice fixed the shift COUNT, not i64 overflow.
///
/// `1 << 63` is in range for the count, so it takes the raw `i64.shl` and
/// WRAPS to `i64::MIN`, while CPython answers `9223372036854775808`. That is
/// still a silent wrong answer and this test says so out loud rather than
/// letting the lane read as fully honest about shifts. It flips to a trap
/// when the general overflow-checked arithmetic lands, at which point this
/// expectation is the thing that must be updated.
#[test]
fn shl_in_range_overflow_is_a_known_residual() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1379: skipping shift residual pin — WABT absent");
        return;
    }
    assert_eq!(
        run(
            "j",
            &emit_module(&int_fn("j", shift(BinOp::Shl, 1, 63))).unwrap()
        ),
        Ok(i64::MIN),
        "KNOWN RESIDUAL: 1 << 63 wraps to i64::MIN; CPython says 9223372036854775808. \
         The shift-COUNT fix (PMAT-1379) does not cover in-range overflow — that is the \
         checked add/sub/mul/neg work. If this now TRAPS, the residual was fixed: update \
         this pin and the SHIFT_HELPERS scope note."
    );
}
