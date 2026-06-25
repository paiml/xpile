//! PMAT-986 (slice 1) — EXECUTED real-string-program witness for the native
//! WASM EMIT lane (`C-COMPILE-RUST-TO-WASM`).
//!
//! The list witnesses (`real_program_witness.rs`, `wasm_witness.rs`) prove
//! the scalar + `list[scalar]` subset executes in a wasm runtime and
//! value-matches CPython. This slice ships the FIRST string support — a
//! `str` PARAMETER as a length-prefixed UTF-8 byte region in linear memory,
//! `len(s)` over it, and `ord(s[i])` byte access — and proves it the SAME
//! way: lower a real Python string program through the production
//! `emit_module`, splice a self-contained `(data …)` driver that preloads
//! the string bytes, assemble + run in WABT, and assert the executed result
//! VALUE-MATCHES CPython on an ASCII fixture.
//!
//! ## The real program
//!
//! ```python
//! def code_sum(s: str) -> int:
//!     total = 0
//!     i = 0
//!     while i < len(s):          # len(s) over the str param (byte count)
//!         total = total + ord(s[i])   # ord(s[i]) → per-byte i32.load8_u
//!         i = i + 1
//!     return total               # Σ ord(s[i])
//! ```
//!
//! For an ASCII string the byte count equals the Python char count and each
//! byte equals `ord` of the char, so the executed WASM `Σ ord(s[i])` equals
//! the CPython `sum(ord(c) for c in s)` exactly. (PMAT-986 slice 1 is
//! ASCII-restricted; a multi-byte UTF-8 string would diverge — slice 2/3
//! ship a real string runtime. The fixture is deliberately ASCII so the
//! witness is exact.)
//!
//! ## Witness shape (mirrors `real_program_witness.rs`)
//!
//! `wasm-interp --run-all-exports` only invokes zero-arg exports, and the
//! kernel takes an `(i32 base-pointer)` argument. So the test lowers the
//! real program through the REAL `emit_module`, then splices a
//! self-contained driver: a length-prefixed `(data …)` segment pre-loads the
//! fixture string at base 0 (an `i32` byte-count header at base+0, the raw
//! UTF-8 bytes at base+8 — the exact PMAT-986 ABI), and one zero-arg `run`
//! export calls `$code_sum` with base-pointer `0`. WABT assembles
//! (`wat2wasm`) and executes (`wasm-interp`) it; the executed `i64` result is
//! asserted to VALUE-MATCH the CPython result of the identical program.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (asserting the EMIT
//! path still lowers + carries the string shape) on a host without WABT, so
//! free CI stays green.

use std::process::Command;

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, Param, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// The ASCII fixture string the real program runs over — mixed case, a
/// digit, a space, and punctuation, all single-byte ASCII so `len` and `ord`
/// are byte-exact (the PMAT-986 slice-1 restriction).
const STR_FIXTURE: &str = "Hello, WASM 42!";

/// CPython reference: `sum(ord(c) for c in STR_FIXTURE)`. Computed
/// independently (asserted below to equal the byte sum, since ASCII) and
/// pinned. `python3 -c "print(sum(ord(c) for c in 'Hello, WASM 42!'))"` =
/// 1055.
const CPYTHON_RESULT: i64 = 1055;

/// Build the meta-HIR `Module` the Python frontend would produce for the
/// `code_sum` program above. Built in-crate (the Python→meta-HIR frontend is
/// not reachable from this codegen crate), exactly as the list witnesses are.
/// `ord(s[i])` is the frontend shape `Expr::Ord { value: Expr::StrCharAt {
/// string: Ident(s), index: Ident(i) } }`.
fn code_sum_module() -> Module {
    // total = total + ord(s[i])
    let acc_step = Stmt::Assign {
        name: "total".into(),
        value: Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::Ident("total".into())),
            rhs: Box::new(Expr::Ord {
                value: Box::new(Expr::StrCharAt {
                    string: Box::new(Expr::Ident("s".into())),
                    index: Box::new(Expr::Ident("i".into())),
                }),
            }),
        },
    };
    // i = i + 1
    let i_step = Stmt::Assign {
        name: "i".into(),
        value: Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::Ident("i".into())),
            rhs: Box::new(Expr::LitInt(1)),
        },
    };

    let f = Function {
        name: "code_sum".into(),
        params: vec![Param {
            name: "s".into(),
            ty: Type::Str,
            mutable: false,
        }],
        return_type: Type::I64,
        body: Block {
            stmts: vec![
                Stmt::Let {
                    name: "total".into(),
                    ty: Type::I64,
                    value: Expr::LitInt(0),
                    mutable: true,
                },
                Stmt::Let {
                    name: "i".into(),
                    ty: Type::I64,
                    value: Expr::LitInt(0),
                    mutable: true,
                },
                // while i < len(s): { total += ord(s[i]); i += 1 }
                Stmt::While {
                    cond: Expr::BinOp {
                        op: BinOp::Lt,
                        lhs: Box::new(Expr::Ident("i".into())),
                        rhs: Box::new(Expr::Len(Box::new(Expr::Ident("s".into())))),
                    },
                    body: vec![acc_step, i_step],
                },
            ],
            trailing_return: Expr::Ident("total".into()),
        },
    };

    Module {
        name: "code_sum_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// Escape an `i32` as a little-endian WAT `(data …)` string-literal (the
/// PMAT-986 byte-count header at base+0).
fn i32_data_escape(v: i32) -> String {
    let mut s = String::new();
    for b in v.to_le_bytes() {
        s.push_str(&format!("\\{b:02x}"));
    }
    s
}

/// Escape raw bytes as a WAT `(data …)` string-literal (each byte `\xx`).
fn bytes_data_escape(bytes: &[u8]) -> String {
    let mut s = String::new();
    for b in bytes {
        s.push_str(&format!("\\{b:02x}"));
    }
    s
}

/// Splice the length-prefixed fixture string `(data …)` region + a zero-arg
/// `run` export (calling `$code_sum` with base-pointer 0) onto the emitted
/// module text, just before its closing `)`. Lets
/// `wasm-interp --run-all-exports` drive the str-taking kernel.
fn build_witness_wat(kernel_wat: &str) -> String {
    let bytes = STR_FIXTURE.as_bytes();
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-986 witness: preload the length-prefixed UTF-8 string\n");
    // i32 byte-count header at base+0.
    wat.push_str(&format!(
        "  (data (i32.const 0) \"{}\")\n",
        i32_data_escape(bytes.len() as i32)
    ));
    // raw UTF-8 bytes at base+8 (LIST_ELEMS_OFFSET).
    wat.push_str(&format!(
        "  (data (i32.const 8) \"{}\")\n",
        bytes_data_escape(bytes)
    ));
    // Zero-arg driver: code_sum(base-pointer 0).
    wat.push_str("  (func (export \"run\") (result i64)\n    i32.const 0\n    call $code_sum)\n");
    wat.push_str(")\n");
    wat
}

#[test]
fn fixture_byte_sum_is_ascii_and_matches_pinned_cpython() {
    // The fixture is ASCII, so the byte sum equals Σ ord(c) — assert that
    // invariant and that the pinned CPYTHON_RESULT matches it, so the witness
    // can't silently drift from CPython semantics.
    assert!(
        STR_FIXTURE.is_ascii(),
        "PMAT-986 slice 1 is ASCII-restricted"
    );
    let byte_sum: i64 = STR_FIXTURE.bytes().map(|b| b as i64).sum();
    let char_ord_sum: i64 = STR_FIXTURE.chars().map(|c| c as i64).sum();
    assert_eq!(
        byte_sum, char_ord_sum,
        "ASCII: byte sum == Σ ord(c) (the slice-1 equivalence)"
    );
    assert_eq!(
        byte_sum, CPYTHON_RESULT,
        "pinned CPython result must equal the fixture's Σ ord(c)"
    );
}

#[test]
fn real_code_sum_program_executes_in_wasm_and_matches_cpython() {
    // CONSTRUCT assertion holds with or without WABT: the real string program
    // must LOWER through the production emitter, exercising the str-param +
    // len(s) + ord(s[i]) combo in one function.
    let kernel_wat = emit_module(&code_sum_module())
        .expect("the real code_sum string program must lower through emit_module");
    assert!(
        kernel_wat.contains("(param $s i32)"),
        "str param → i32 base-pointer:\n{kernel_wat}"
    );
    assert!(
        kernel_wat.contains("i32.load8_u"),
        "ord(s[i]) → per-byte i32.load8_u:\n{kernel_wat}"
    );
    assert!(
        kernel_wat.contains("i32.load") && kernel_wat.contains("i64.extend_i32_u"),
        "len(s) → header i32.load + i64 extend:\n{kernel_wat}"
    );
    assert!(
        kernel_wat.contains("(loop $cont") && kernel_wat.contains("i64.lt_s"),
        "while i < len(s) → loop + i64 comparison:\n{kernel_wat}"
    );
    assert!(
        kernel_wat.contains("unreachable"),
        "PMAT-986 bounds guard on ord(s[i]):\n{kernel_wat}"
    );

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-986: skipping EXECUTED real-string-program witness — WABT \
             (wat2wasm / wasm-interp) absent. The string program lowered \
             through emit_module (asserted above); a box with WABT also runs \
             it and asserts the WASM result == CPython {CPYTHON_RESULT}. Free \
             CI skips the execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-986: running EXECUTED real-string-program (code_sum) witness via WABT");

    let wat = build_witness_wat(&kernel_wat);

    let dir = std::env::temp_dir().join(format!("xpile-wasm-str-program-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("code_sum.wat");
    let wasm_path = dir.join("code_sum.wasm");
    std::fs::write(&wat_path, &wat).expect("write wat");

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
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "wasm-interp run failed: stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );

    // `run() => i64:<value>` — the single executed result of the program.
    let line = stdout
        .lines()
        .find(|l| l.contains("=> i64:"))
        .unwrap_or_else(|| panic!("no i64 export in interp output:\n{stdout}"));
    let idx = line.find("=> i64:").unwrap();
    let got: i64 = line[idx + "=> i64:".len()..]
        .trim()
        .parse()
        .expect("parse i64 program result");

    assert_eq!(
        got, CPYTHON_RESULT,
        "executed WASM code_sum={got} but CPython code_sum={CPYTHON_RESULT}\nWAT:\n{wat}"
    );

    eprintln!(
        "PMAT-986: EXECUTED real-string-program witness PASSED — the composed \
         code_sum program (str param + len(s) + ord(s[i]) + while + i64 \
         arithmetic + int return) executed in WABT to {got}, value-matching \
         the CPython result {CPYTHON_RESULT} for the ASCII fixture \
         {STR_FIXTURE:?}. PMAT-986 slice 1 (read-only string access) is real."
    );
    eprintln!("--- emitted code_sum WAT (emit_module over meta-HIR) ---\n{kernel_wat}");
}
