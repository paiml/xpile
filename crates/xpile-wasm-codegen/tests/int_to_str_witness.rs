//! PMAT-1060 — EXECUTED `str(int)` witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The prior string-heap slices materialised NEW strings from EXISTING string
//! bytes (`chr(n)`, `a + b`, `s[i]`, `s[lo:hi]`). This slice adds the first
//! op that manufactures a string from a NON-string value: `str(n)` /
//! `repr(n)` over an `int` — an i64 → decimal-ASCII heap string.
//!
//! The `$__wasm_int_to_str` helper works in the UNSIGNED magnitude, so the two
//! adversarial boundaries are covered exactly:
//!   * `str(0)` → `"0"` (the "at least one digit" special case), and
//!   * `str(-9223372036854775808)` (`i64::MIN`) → `"-9223372036854775808"` —
//!     where a naive `0 - n` negation would OVERFLOW; the wrapping i64 sub
//!     yields the correct u64 magnitude bit pattern, decoded via `i64.div_u` /
//!     `i64.rem_u`.
//!
//! ## The real program
//!
//! ```python
//! def to_s(n: int) -> str:
//!     return str(n)          # i64 → decimal-ASCII heap string
//! ```
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel `$to_s`
//! takes one `i64` (the `n` param) and RETURNS an `i32` (the constructed
//! string's base-pointer). The witness adds only zero-arg wrappers that push a
//! CONSTANT `n`, call `$to_s`, and read back the result:
//!   1. `run_len` — the constructed string's i32 byte count (header @ result+0);
//!   2. a `run_byte_i` family — each re-runs `$to_s(n)`, adds `8 + i`, and
//!      `i32.load8_u`s that payload byte of the constructed string.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_int_to_str` helper + call) on a host
//! without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Build the meta-HIR `Module` the frontend produces for
/// `def to_s(n: int) -> str: return str(n)`.
fn to_s_module() -> Module {
    let body = Expr::ToStr {
        value: Box::new(Expr::Ident("n".into())),
        of_float: false,
    };
    let f = Function {
        name: "to_s".into(),
        params: vec![Param {
            name: "n".into(),
            ty: Type::I64,
            mutable: false,
        }],
        return_type: Type::Str,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "to_s_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// Build the meta-HIR `Module` for `def label(n: int) -> str: return "n=" +
/// str(n)` — proves `str(int)` COMPOSES as a concat operand (the operand is
/// re-materialised in the length + copy passes, the accepted heap-waste pattern
/// the other materialising operands use).
fn label_module() -> Module {
    let body = Expr::Concat {
        lhs: Box::new(Expr::LitStr("n=".into())),
        rhs: Box::new(Expr::ToStr {
            value: Box::new(Expr::Ident("n".into())),
            of_float: false,
        }),
    };
    let f = Function {
        name: "label".into(),
        params: vec![Param {
            name: "n".into(),
            ty: Type::I64,
            mutable: false,
        }],
        return_type: Type::Str,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "label_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// Splice zero-arg wrappers that invoke `$KERNEL(n)` and read back the result
/// onto the emitted module, before its closing `)`. `n_out` = the expected
/// decimal string byte length. `kernel` is the emitted function name to call.
fn build_witness_wat_for(kernel_wat: &str, kernel: &str, n: i64, n_out: usize) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1060 witness: run the kernel for a constant n, read back\n");
    wat.push_str(&format!(
        "  (func (export \"run_len\") (result i32)\n    i64.const {n}\n    call ${kernel}\n    i32.load)\n"
    ));
    for i in 0..n_out {
        wat.push_str(&format!(
            "  (func (export \"run_byte_{i}\") (result i32)\n    \
               i64.const {n}\n    call ${kernel}\n    \
               i32.const {off}\n    i32.add\n    i32.load8_u)\n",
            off = 8 + i
        ));
    }
    wat.push_str(")\n");
    wat
}

/// Splice zero-arg wrappers that invoke `$to_s(n)` and read back the result
/// onto the emitted module, before its closing `)`. `n_out` = the expected
/// decimal string byte length.
fn build_witness_wat(kernel_wat: &str, n: i64, n_out: usize) -> String {
    build_witness_wat_for(kernel_wat, "to_s", n, n_out)
}

/// Parse a `name() => i32:<value>` line for a given export name.
fn parse_i32_export(stdout: &str, name: &str) -> i32 {
    let needle = format!("{name}() => i32:");
    let line = stdout
        .lines()
        .find(|l| l.contains(&needle))
        .unwrap_or_else(|| panic!("no `{name}` i32 export in interp output:\n{stdout}"));
    let idx = line.find("=> i32:").unwrap();
    line[idx + "=> i32:".len()..]
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("parse i32 for {name} from {line:?}"))
}

/// Lower `str(n)`, run it in WABT, and reconstruct the decimal string. Returns
/// `None` when WABT is absent (the caller skips the value assertion).
fn exec_str_int(n: i64, expected: &str) -> Option<String> {
    let kernel_wat = emit_module(&to_s_module()).expect("str(int) program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let n_out = expected.len();
    let wat = build_witness_wat(&kernel_wat, n, n_out);
    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-int-to-str-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("to_s.wat");
    let wasm_path = dir.join("to_s.wasm");
    std::fs::write(&wat_path, &wat).expect("write wat");
    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed for str({n}):\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&assemble.stderr)
    );
    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "wasm-interp run failed for str({n}): stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    let got_len = parse_i32_export(&stdout, "run_len");
    assert_eq!(
        got_len as usize, n_out,
        "str({n}) byte length: WASM={got_len} CPython={n_out}\nWAT:\n{wat}"
    );
    let mut bytes = Vec::with_capacity(n_out);
    for i in 0..n_out {
        bytes.push(parse_i32_export(&stdout, &format!("run_byte_{i}")) as u8);
    }
    Some(String::from_utf8(bytes).expect("constructed decimal string bytes are valid UTF-8"))
}

#[test]
fn cpython_str_int_values_are_pinned() {
    // The pinned CPython ground truth this witness value-matches. `str` over an
    // int is pure ASCII decimal with a leading `-` for negatives.
    assert_eq!(format!("{}", 0), "0");
    assert_eq!(format!("{}", -5), "-5");
    assert_eq!(format!("{}", i64::MAX), "9223372036854775807");
    assert_eq!(format!("{}", i64::MIN), "-9223372036854775808");
    // i64::MIN is exactly where a naive negation overflows — the boundary the
    // unsigned-magnitude helper is designed to survive.
    assert_eq!(i64::MIN.unsigned_abs(), 9_223_372_036_854_775_808_u64);
}

#[test]
fn to_s_emits_int_to_str_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): the str(int) program
    // lowers through the production emitter, carrying the helper + call.
    let wat =
        emit_module(&to_s_module()).expect("the str(n) program must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_int_to_str (param $n i64) (result i32)"),
        "the int→str helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_int_to_str"),
        "$to_s must call the int→str helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $to_s (param $n i64) (result i32)"),
        "str return → i32 result (heap pointer):\n{wat}"
    );
    // Materialising a string → needs the bump heap.
    assert!(
        wat.contains("(func $__alloc"),
        "str(int) needs the bump heap:\n{wat}"
    );
}

#[test]
fn real_str_int_program_executes_in_wasm_and_matches_cpython() {
    let kernel_wat =
        emit_module(&to_s_module()).expect("str(n) program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1060: skipping EXECUTED str(int) witness — WABT (wat2wasm / \
             wasm-interp) absent. The to_s program lowered through emit_module \
             (asserted in `to_s_emits_int_to_str_helper_and_call`); a box with \
             WABT also runs it and asserts the CONSTRUCTED string == CPython."
        );
        return;
    }

    eprintln!("PMAT-1060: running EXECUTED str(int) (to_s = str(n)) witness via WABT");

    // The headline: str(42) — a multi-digit positive, the common case.
    let expected = "42";
    let got = exec_str_int(42, expected).expect("WABT present");
    assert_eq!(
        got, expected,
        "executed WASM str(42) = {got:?} but CPython = {expected:?}"
    );

    eprintln!(
        "PMAT-1060: EXECUTED str(int) witness PASSED — `to_s(n) = str(n)` \
         lowered through emit_module and executed in WABT to {got:?} for n=42, \
         value-matching the CPython result {expected:?}."
    );
    eprintln!("--- emitted to_s WAT (emit_module over meta-HIR) ---\n{kernel_wat}");
}

// ─── PMAT-1060: EXECUTED sign / zero / boundary edge cases ───────────────────
//
// The headline proves the digit-extraction + alloc + copy path on str(42). The
// SIGN handling (leading `-`), the ZERO special case (at-least-one-digit), and
// the i64::MIN/MAX magnitude boundaries are exactly where an int→str bug hides,
// so each is executed on silicon and value-matched to CPython (adversarial-
// verify discipline, not just asserted from the emit text).

#[test]
fn str_int_sign_zero_boundary_edges_match_cpython() {
    // (n, CPython str(n)) — pinned to the exact decimal forms.
    let cases: &[(i64, &str)] = &[
        (0, "0"),                           // the at-least-one-digit case
        (7, "7"),                           // single positive digit
        (-5, "-5"),                         // single negative digit
        (-42, "-42"),                       // multi-digit negative
        (100, "100"),                       // trailing zeros preserved
        (1_000_000, "1000000"),             // interior + trailing zeros
        (i64::MAX, "9223372036854775807"),  // largest positive (19 digits)
        (i64::MIN, "-9223372036854775808"), // overflow boundary (20 bytes)
    ];
    for &(n, expected) in cases {
        // CONSTRUCT: every form lowers through the production emitter.
        let wat = emit_module(&to_s_module()).expect("str(int) lowers");
        assert!(wat.contains("call $__wasm_int_to_str"));
        // EXECUTE (when WABT present): value-match CPython.
        match exec_str_int(n, expected) {
            Some(got) => assert_eq!(
                got, expected,
                "executed str({n}) = {got:?} but CPython = {expected:?}"
            ),
            None => {
                eprintln!(
                    "PMAT-1060: WABT absent — skipped executing str({n}) \
                     (expected {expected:?}); emit path asserted."
                );
                return;
            }
        }
    }
    eprintln!(
        "PMAT-1060: all 8 sign/zero/boundary edge cases executed in WABT and \
         value-matched CPython (incl. i64::MIN = -9223372036854775808)."
    );
}

/// Lower `label(n) = "n=" + str(n)`, run it in WABT, reconstruct the string.
fn exec_label(n: i64, expected: &str) -> Option<String> {
    let kernel_wat = emit_module(&label_module()).expect("label program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let n_out = expected.len();
    let wat = build_witness_wat_for(&kernel_wat, "label", n, n_out);
    let dir = std::env::temp_dir().join(format!("xpile-wasm-int-label-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("label.wat");
    let wasm_path = dir.join("label.wasm");
    std::fs::write(&wat_path, &wat).expect("write wat");
    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed for label({n}):\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&assemble.stderr)
    );
    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "wasm-interp run failed for label({n}): stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    let got_len = parse_i32_export(&stdout, "run_len");
    assert_eq!(
        got_len as usize, n_out,
        "label({n}) byte length: WASM={got_len} CPython={n_out}\nWAT:\n{wat}"
    );
    let mut bytes = Vec::with_capacity(n_out);
    for i in 0..n_out {
        bytes.push(parse_i32_export(&stdout, &format!("run_byte_{i}")) as u8);
    }
    Some(String::from_utf8(bytes).expect("constructed label bytes are valid UTF-8"))
}

#[test]
fn str_int_composes_as_a_concat_operand_matches_cpython() {
    // `"n=" + str(n)` — proves str(int) COMPOSES with concat (the operand is
    // re-materialised across the length + copy passes; a bug there would
    // desync the header length from the copied bytes). Value-matched to
    // CPython `"n=" + str(n)` over positive, negative, and zero.
    let cases: &[(i64, &str)] = &[(42, "n=42"), (-7, "n=-7"), (0, "n=0")];
    // CONSTRUCT: the concat-of-str(int) lowers through the emitter.
    let wat = emit_module(&label_module()).expect("label lowers");
    assert!(wat.contains("call $__wasm_int_to_str"));
    assert!(wat.contains("call $__alloc"));
    for &(n, expected) in cases {
        match exec_label(n, expected) {
            Some(got) => assert_eq!(
                got, expected,
                "executed \"n=\" + str({n}) = {got:?} but CPython = {expected:?}"
            ),
            None => {
                eprintln!(
                    "PMAT-1060: WABT absent — skipped executing \"n=\" + str({n}) \
                     (expected {expected:?}); emit path asserted."
                );
                return;
            }
        }
    }
    eprintln!(
        "PMAT-1060: str(int) composes as a concat operand — \"n=\" + str(n) \
         executed in WABT and value-matched CPython for 42, -7, 0."
    );
}
