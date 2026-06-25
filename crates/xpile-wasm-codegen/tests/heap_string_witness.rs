//! PMAT-993 (slice 2) — EXECUTED string-BUILDING witness for the native WASM
//! EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! Slice 1 (`str_program_witness.rs`) proved READ-ONLY string access — a `str`
//! PARAMETER as a length-prefixed UTF-8 byte region, `len(s)`, `ord(s[i])` —
//! and REFUSED every string-RETURNING op with "needs heap allocator (slice
//! 2)". This slice ships that allocator (a linear-memory bump heap:
//! `$__heap_ptr` global + `$__alloc`) and the FIRST string-RETURNING op,
//! **string concatenation `a + b`**, plus `chr(n)`. A function RETURNING a
//! `str` now works.
//!
//! The witness proves it the SAME way the list/str-read witnesses do: lower a
//! real Python string-BUILDING program through the production `emit_module`,
//! splice a self-contained `(data …)` driver that preloads the input strings,
//! assemble + run in WABT, then READ BACK the CONSTRUCTED string's bytes from
//! the returned heap pointer and assert they VALUE-MATCH CPython.
//!
//! ## The real program
//!
//! ```python
//! def join(a: str, b: str) -> str:
//!     return a + b        # string concat → a NEW heap string
//! ```
//!
//! Run over the ASCII fixtures `a = "Hi "`, `b = "WASM 42!"`; the constructed
//! string is `"Hi WASM 42!"`, byte-exact (ASCII) to CPython `a + b`.
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports and prints scalar
//! results. The kernel `$join` takes two `i32` base-pointers and RETURNS an
//! `i32` (the constructed string's base-pointer) — neither zero-arg nor a
//! readable scalar. So the test lowers `join` through the REAL `emit_module`,
//! then splices a self-contained driver:
//!   1. two length-prefixed `(data …)` segments preload `a` and `b` at fixed
//!      addresses BELOW `HEAP_BASE` (= 1024);
//!   2. a zero-arg `run_byte_i(idx)` family — one export per output byte —
//!      calls `$join(a_ptr, b_ptr)`, adds `8 + idx`, and `i32.load8_u`s that
//!      byte of the CONSTRUCTED string, returning it as an `i32`;
//!   3. a `run_len` export returns the constructed string's i32 byte count.
//!
//! WABT assembles + executes; the test reassembles the bytes and asserts the
//! reconstructed string == the CPython `a + b`.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (asserting the EMIT path
//! still lowers + carries the heap shape) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// The two ASCII input fixtures and their CPython concatenation.
const FIX_A: &str = "Hi ";
const FIX_B: &str = "WASM 42!";
/// `python3 -c "print(repr('Hi ' + 'WASM 42!'))"` == `'Hi WASM 42!'`.
const CPYTHON_CONCAT: &str = "Hi WASM 42!";

/// Fixed linear-memory addresses for the two input strings, both below
/// `HEAP_BASE` (= 1024) so the bump heap (where `join` allocates its result)
/// never overlaps them. Each is a length-prefixed region (i32 count @ base+0,
/// bytes @ base+8), matching the PMAT-986/993 ABI.
const A_ADDR: i32 = 16;
const B_ADDR: i32 = 256;

/// Build the meta-HIR `Module` the Python frontend would produce for
/// `def join(a: str, b: str) -> str: return a + b`.
fn join_module() -> Module {
    let f = Function {
        name: "join".into(),
        params: vec![
            Param {
                name: "a".into(),
                ty: Type::Str,
                mutable: false,
            },
            Param {
                name: "b".into(),
                ty: Type::Str,
                mutable: false,
            },
        ],
        return_type: Type::Str,
        body: Block {
            stmts: vec![],
            // return a + b  (string concat → a new heap string)
            trailing_return: Expr::Concat {
                lhs: Box::new(Expr::Ident("a".into())),
                rhs: Box::new(Expr::Ident("b".into())),
            },
        },
    };
    Module {
        name: "join_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// Escape an `i32` as a little-endian WAT `(data …)` string-literal (the
/// PMAT-986 byte-count header).
fn i32_data_escape(v: i32) -> String {
    v.to_le_bytes()
        .iter()
        .map(|b| format!("\\{b:02x}"))
        .collect()
}

/// Escape raw bytes as a WAT `(data …)` string-literal (each byte `\xx`).
fn bytes_data_escape(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
}

/// Splice the two length-prefixed input `(data …)` regions + per-byte readers
/// onto the emitted module, before its closing `)`. `n_out` = the expected
/// constructed-string byte length (so we emit exactly that many byte readers).
fn build_witness_wat(kernel_wat: &str, n_out: usize) -> String {
    let a = FIX_A.as_bytes();
    let b = FIX_B.as_bytes();
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-993 witness: preload the two length-prefixed input strings\n");
    // a @ A_ADDR.
    wat.push_str(&format!(
        "  (data (i32.const {A_ADDR}) \"{}\")\n",
        i32_data_escape(a.len() as i32)
    ));
    wat.push_str(&format!(
        "  (data (i32.const {}) \"{}\")\n",
        A_ADDR + 8,
        bytes_data_escape(a)
    ));
    // b @ B_ADDR.
    wat.push_str(&format!(
        "  (data (i32.const {B_ADDR}) \"{}\")\n",
        i32_data_escape(b.len() as i32)
    ));
    wat.push_str(&format!(
        "  (data (i32.const {}) \"{}\")\n",
        B_ADDR + 8,
        bytes_data_escape(b)
    ));
    // run_len: the constructed string's i32 byte count (header at result+0).
    wat.push_str(&format!(
        "  (func (export \"run_len\") (result i32)\n    i32.const {A_ADDR}\n    i32.const {B_ADDR}\n    call $join\n    i32.load)\n"
    ));
    // run_byte_i: byte i of the constructed string. Each export re-runs join
    // (the bump heap is deterministic from a fresh module instance per
    // invocation under --run-all-exports), adds 8+i, and load8_u's that byte.
    for i in 0..n_out {
        wat.push_str(&format!(
            "  (func (export \"run_byte_{i}\") (result i32)\n    \
               i32.const {A_ADDR}\n    i32.const {B_ADDR}\n    call $join\n    \
               i32.const {off}\n    i32.add\n    i32.load8_u)\n",
            off = 8 + i
        ));
    }
    wat.push_str(")\n");
    wat
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

#[test]
fn cpython_concat_is_ascii_and_pinned() {
    assert!(
        FIX_A.is_ascii() && FIX_B.is_ascii(),
        "slice-2 ASCII fixtures"
    );
    assert_eq!(
        format!("{FIX_A}{FIX_B}"),
        CPYTHON_CONCAT,
        "pinned CPython a + b must equal the fixture concatenation"
    );
}

#[test]
fn concat_emits_allocator_and_memory_copy() {
    // CONSTRUCT assertion (holds with or without WABT): the string-building
    // program lowers through the production emitter, exercising the heap
    // allocator + concat lowering.
    let wat = emit_module(&join_module())
        .expect("the str-returning concat program must lower through emit_module");
    assert!(
        wat.contains("(global $__heap_ptr (mut i32)") && wat.contains("(func $__alloc"),
        "PMAT-993 bump allocator must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__alloc"),
        "concat must allocate via $__alloc:\n{wat}"
    );
    assert!(
        wat.contains("memory.copy"),
        "concat must byte-copy operands via memory.copy:\n{wat}"
    );
    assert!(
        wat.contains("(func $join (param $a i32) (param $b i32) (result i32)"),
        "str return → i32 result (heap pointer):\n{wat}"
    );
    assert!(
        wat.contains("(memory (export \"mem\") 1)"),
        "heap needs the linear memory:\n{wat}"
    );
}

#[test]
fn real_concat_program_executes_in_wasm_and_matches_cpython() {
    let kernel_wat = emit_module(&join_module())
        .expect("str-returning concat program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-993: skipping EXECUTED string-building witness — WABT \
             (wat2wasm / wasm-interp) absent. The concat program lowered \
             through emit_module (asserted in `concat_emits_allocator_and_memory_copy`); \
             a box with WABT also runs it and asserts the CONSTRUCTED string == \
             CPython {CPYTHON_CONCAT:?}. Free CI skips execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-993: running EXECUTED string-building (join = a + b) witness via WABT");

    let n_out = CPYTHON_CONCAT.len();
    let wat = build_witness_wat(&kernel_wat, n_out);

    let dir = std::env::temp_dir().join(format!("xpile-wasm-heap-string-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("join.wat");
    let wasm_path = dir.join("join.wasm");
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

    // Read back the constructed length + each byte, reassemble the string.
    let got_len = parse_i32_export(&stdout, "run_len");
    assert_eq!(
        got_len as usize, n_out,
        "constructed string length: WASM={got_len} CPython={n_out}\nWAT:\n{wat}"
    );
    let mut bytes = Vec::with_capacity(n_out);
    for i in 0..n_out {
        let b = parse_i32_export(&stdout, &format!("run_byte_{i}"));
        bytes.push(b as u8);
    }
    let got = String::from_utf8(bytes).expect("constructed bytes are valid UTF-8 (ASCII)");

    assert_eq!(
        got, CPYTHON_CONCAT,
        "executed WASM a + b = {got:?} but CPython a + b = {CPYTHON_CONCAT:?}\nWAT:\n{wat}"
    );

    eprintln!(
        "PMAT-993: EXECUTED string-building witness PASSED — `join(a, b) = a + b` \
         lowered through emit_module, bump-allocated + memory.copy'd a NEW \
         length-prefixed string in linear memory, and executed in WABT to \
         {got:?} (len {got_len}), value-matching the CPython result \
         {CPYTHON_CONCAT:?} for the ASCII fixtures {FIX_A:?} + {FIX_B:?}. \
         PMAT-986 slice 2 (heap allocator + string concat) is real."
    );
    eprintln!("--- emitted join WAT (emit_module over meta-HIR) ---\n{kernel_wat}");
}
