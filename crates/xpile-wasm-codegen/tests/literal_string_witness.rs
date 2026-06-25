//! PMAT-994 (slice 3a) — EXECUTED string-LITERAL witness for the native WASM
//! EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! Slice 2 (`heap_string_witness.rs`) shipped the bump allocator + string
//! concat `a + b` + `chr(n)` over str PARAMS — and REFUSED string LITERALS (a
//! `"..."` operand needs a static `(data)` segment), `s[i]` as a 1-char string,
//! and string equality. This slice CLOSES those: string literals are
//! materialised at emit time into static `(data …)` segments in
//! `[LITERAL_BASE, HEAP_BASE)` (a `LitStr` lowers to a constant `i32.const
//! <base>`), `s[i]` as a 1-char string materialises a new heap string, and
//! `a == b` lowers to a real content-compare helper.
//!
//! The witness proves the headline (literals) the SAME way the slice-2 witness
//! does: lower a real Python string-LITERAL program through the production
//! `emit_module`, splice a self-contained `(data …)` driver that preloads the
//! single str param (BELOW the emitter-owned literal region), assemble + run in
//! WABT, then READ BACK the CONSTRUCTED string's bytes from the returned heap
//! pointer and assert they VALUE-MATCH CPython.
//!
//! ## The real program
//!
//! ```python
//! def greet(name: str) -> str:
//!     return "Hello, " + name + "!"     # two string LITERALS + a str param
//! ```
//!
//! Run over the ASCII fixture `name = "WASM"`; the constructed string is
//! `"Hello, WASM!"`, byte-exact (ASCII) to CPython `"Hello, " + name + "!"`.
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports and prints scalar
//! results. The kernel `$greet` takes one `i32` base-pointer (the `name` str
//! param) and RETURNS an `i32` (the constructed string's base-pointer). The two
//! string literals are EMITTER-OWNED static `(data …)` segments the production
//! emitter already laid down — the witness adds only:
//!   1. one length-prefixed `(data …)` segment preloading `name` at a fixed
//!      address BELOW `LITERAL_BASE` (= 512), so it never overlaps the
//!      emitter's literal region or the bump heap above it;
//!   2. a zero-arg `run_byte_i(idx)` family — one export per output byte —
//!      calls `$greet(name_ptr)`, adds `8 + idx`, and `i32.load8_u`s that byte
//!      of the CONSTRUCTED string, returning it as an `i32`;
//!   3. a `run_len` export returns the constructed string's i32 byte count.
//!
//! WABT assembles + executes; the test reassembles the bytes and asserts the
//! reconstructed string == the CPython value.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (asserting the EMIT path
//! still lowers + carries the literal `(data …)` shape) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, Param, SourceLang, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// The ASCII str-param fixture and the CPython result.
const FIX_NAME: &str = "WASM";
/// `python3 -c "print('Hello, ' + 'WASM' + '!')"` == `Hello, WASM!`.
const CPYTHON_GREET: &str = "Hello, WASM!";

/// Fixed linear-memory address for the `name` str param, BELOW `LITERAL_BASE`
/// (= 512) so the host-preloaded param never overlaps the emitter-owned static
/// literal region `[512, 1024)` or the bump heap above (`>= 1024`). A
/// length-prefixed region (i32 count @ base+0, bytes @ base+8).
const NAME_ADDR: i32 = 16;

/// Build the meta-HIR `Module` the Python frontend would produce for
/// `def greet(name: str) -> str: return "Hello, " + name + "!"`.
fn greet_module() -> Module {
    // ("Hello, " + name) + "!"  (left-nested concat, the frontend's shape).
    let body = Expr::Concat {
        lhs: Box::new(Expr::Concat {
            lhs: Box::new(Expr::LitStr("Hello, ".into())),
            rhs: Box::new(Expr::Ident("name".into())),
        }),
        rhs: Box::new(Expr::LitStr("!".into())),
    };
    let f = Function {
        name: "greet".into(),
        params: vec![Param {
            name: "name".into(),
            ty: Type::Str,
            mutable: false,
        }],
        return_type: Type::Str,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "greet_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// Escape an `i32` as a little-endian WAT `(data …)` string-literal.
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

/// Splice the `name` param `(data …)` region + per-byte readers onto the
/// emitted module, before its closing `)`. `n_out` = the expected
/// constructed-string byte length (so we emit exactly that many byte readers).
fn build_witness_wat(kernel_wat: &str, n_out: usize) -> String {
    let name = FIX_NAME.as_bytes();
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-994 witness: preload the str param (below LITERAL_BASE)\n");
    // name @ NAME_ADDR (length-prefixed).
    wat.push_str(&format!(
        "  (data (i32.const {NAME_ADDR}) \"{}\")\n",
        i32_data_escape(name.len() as i32)
    ));
    wat.push_str(&format!(
        "  (data (i32.const {}) \"{}\")\n",
        NAME_ADDR + 8,
        bytes_data_escape(name)
    ));
    // run_len: the constructed string's i32 byte count (header at result+0).
    wat.push_str(&format!(
        "  (func (export \"run_len\") (result i32)\n    i32.const {NAME_ADDR}\n    call $greet\n    i32.load)\n"
    ));
    // run_byte_i: byte i of the constructed string. Each export re-runs greet
    // (a fresh bump heap per invocation under --run-all-exports), adds 8+i, and
    // load8_u's that byte.
    for i in 0..n_out {
        wat.push_str(&format!(
            "  (func (export \"run_byte_{i}\") (result i32)\n    \
               i32.const {NAME_ADDR}\n    call $greet\n    \
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
fn cpython_greet_is_ascii_and_pinned() {
    assert!(FIX_NAME.is_ascii(), "slice-3a ASCII fixture");
    assert_eq!(
        format!("Hello, {FIX_NAME}!"),
        CPYTHON_GREET,
        "pinned CPython 'Hello, ' + name + '!' must equal the fixture greeting"
    );
}

#[test]
fn greet_emits_literal_data_segments_and_heap() {
    // CONSTRUCT assertion (holds with or without WABT): the literal program
    // lowers through the production emitter — two static literal `(data …)`
    // segments + the bump-heap concat path.
    let wat = emit_module(&greet_module())
        .expect("the str-literal greet program must lower through emit_module");
    // The two literals "Hello, " (7 bytes) and "!" (1 byte) become static
    // length-prefixed (data) segments in [LITERAL_BASE, HEAP_BASE) = [512, 1024).
    assert!(
        wat.contains("(data (i32.const 512) \"\\07\\00\\00\\00\")"),
        "literal \"Hello, \" byte-count header @ 512:\n{wat}"
    );
    assert!(
        wat.contains("(data (i32.const 520) \"\\48\\65\\6c\\6c\\6f\\2c\\20\")"),
        "literal \"Hello, \" UTF-8 bytes @ 520:\n{wat}"
    );
    assert!(
        wat.contains("(data (i32.const 528) \"\\01\\00\\00\\00\")"),
        "literal \"!\" byte-count header @ 528 (after the 8+7→align8=16-byte first literal):\n{wat}"
    );
    assert!(
        wat.contains("(global $__heap_ptr (mut i32)") && wat.contains("(func $__alloc"),
        "the concat still needs the bump heap:\n{wat}"
    );
    assert!(
        wat.contains("(func $greet (param $name i32) (result i32)"),
        "str return → i32 result (heap pointer):\n{wat}"
    );
}

#[test]
fn real_literal_program_executes_in_wasm_and_matches_cpython() {
    let kernel_wat =
        emit_module(&greet_module()).expect("str-literal greet program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-994: skipping EXECUTED string-literal witness — WABT \
             (wat2wasm / wasm-interp) absent. The greet program lowered \
             through emit_module (asserted in `greet_emits_literal_data_segments_and_heap`); \
             a box with WABT also runs it and asserts the CONSTRUCTED string == \
             CPython {CPYTHON_GREET:?}. Free CI skips execution and stays green."
        );
        return;
    }

    eprintln!(
        "PMAT-994: running EXECUTED string-literal (greet = \"Hello, \" + name + \"!\") \
         witness via WABT"
    );

    let n_out = CPYTHON_GREET.len();
    let wat = build_witness_wat(&kernel_wat, n_out);

    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-literal-string-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("greet.wat");
    let wasm_path = dir.join("greet.wasm");
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
        got, CPYTHON_GREET,
        "executed WASM greet = {got:?} but CPython greet = {CPYTHON_GREET:?}\nWAT:\n{wat}"
    );

    eprintln!(
        "PMAT-994: EXECUTED string-literal witness PASSED — \
         `greet(name) = \"Hello, \" + name + \"!\"` lowered through emit_module \
         (two static literal (data) segments + bump-heap concat), and executed \
         in WABT to {got:?} (len {got_len}), value-matching the CPython result \
         {CPYTHON_GREET:?} for the ASCII fixture name={FIX_NAME:?}. \
         PMAT-986 strings are substantially complete (literals + s[i] + equality)."
    );
    eprintln!("--- emitted greet WAT (emit_module over meta-HIR) ---\n{kernel_wat}");
}

// ─── PMAT-994: EXECUTED string-EQUALITY witness (literal compare) ───────────

/// Build `def is_done(s: str) -> bool: return s == "done"` — a str-param vs
/// string-LITERAL content comparison. The frontend lowers `s == "done"` to a
/// `BinOp::Eq` over a str-param `Ident` and an `Expr::LitStr`.
fn is_done_module() -> Module {
    let f = Function {
        name: "is_done".into(),
        params: vec![Param {
            name: "s".into(),
            ty: Type::Str,
            mutable: false,
        }],
        return_type: Type::Bool,
        body: Block {
            stmts: vec![],
            trailing_return: Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(Expr::Ident("s".into())),
                rhs: Box::new(Expr::LitStr("done".into())),
            },
        },
    };
    Module {
        name: "is_done_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// Run `is_done` over a given `s` fixture preloaded at `NAME_ADDR` and return
/// the i32 bool the content compare yields.
fn run_is_done(kernel_wat: &str, s: &str) -> i32 {
    let sb = s.as_bytes();
    let close = kernel_wat.rfind(')').expect("closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str(&format!(
        "  (data (i32.const {NAME_ADDR}) \"{}\")\n",
        i32_data_escape(sb.len() as i32)
    ));
    wat.push_str(&format!(
        "  (data (i32.const {}) \"{}\")\n",
        NAME_ADDR + 8,
        bytes_data_escape(sb)
    ));
    wat.push_str(&format!(
        "  (func (export \"run\") (result i32)\n    i32.const {NAME_ADDR}\n    call $is_done)\n"
    ));
    wat.push_str(")\n");

    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-streq-{}-{}",
        std::process::id(),
        s.len()
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("is_done.wat");
    let wasm_path = dir.join("is_done.wasm");
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
    assert!(run.status.success(), "wasm-interp failed: {stdout}");
    parse_i32_export(&stdout, "run")
}

#[test]
fn real_string_equality_executes_in_wasm_and_matches_cpython() {
    let kernel_wat =
        emit_module(&is_done_module()).expect("str == literal program lowers through emit_module");
    // CONSTRUCT assertion (holds without WABT): the content-compare helper is
    // emitted and called (never a base-pointer compare).
    assert!(
        kernel_wat.contains("(func $__wasm_str_eq") && kernel_wat.contains("call $__wasm_str_eq"),
        "str == literal routes to the content-compare helper:\n{kernel_wat}"
    );

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-994: skipping EXECUTED string-equality witness — WABT absent. \
             `is_done(s) = (s == \"done\")` lowered through emit_module (content \
             compare asserted); a WABT box also runs it: is_done(\"done\")==True, \
             is_done(\"busy\")==False, matching CPython."
        );
        return;
    }

    // CPython: ("done" == "done") is True (1); ("busy" == "done") is False (0);
    // ("don" == "done") is False (length differs → fast 0).
    let done = run_is_done(&kernel_wat, "done");
    let busy = run_is_done(&kernel_wat, "busy");
    let short = run_is_done(&kernel_wat, "don");
    assert_eq!(done, 1, "WASM is_done(\"done\") must be 1 (CPython True)");
    assert_eq!(busy, 0, "WASM is_done(\"busy\") must be 0 (CPython False)");
    assert_eq!(
        short, 0,
        "WASM is_done(\"don\") must be 0 (length mismatch → CPython False)"
    );
    eprintln!(
        "PMAT-994: EXECUTED string-equality witness PASSED — `is_done(s) = \
         (s == \"done\")` executed in WABT: is_done(\"done\")={done} (CPython 1), \
         is_done(\"busy\")={busy} (CPython 0), is_done(\"don\")={short} (CPython 0). \
         Content compare, never a base-pointer compare."
    );
}

// ─── PMAT-994: EXECUTED s[i]-as-1-char-string witness ──────────────────────

/// Build `def at(s: str, i: int) -> str: return s[i]` — `s[i]` materialised as
/// a NEW 1-char heap string.
fn at_module() -> Module {
    let f = Function {
        name: "at".into(),
        params: vec![
            Param {
                name: "s".into(),
                ty: Type::Str,
                mutable: false,
            },
            Param {
                name: "i".into(),
                ty: Type::I64,
                mutable: false,
            },
        ],
        return_type: Type::Str,
        body: Block {
            stmts: vec![],
            trailing_return: Expr::StrCharAt {
                string: Box::new(Expr::Ident("s".into())),
                index: Box::new(Expr::Ident("i".into())),
            },
        },
    };
    Module {
        name: "at_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

#[test]
fn real_str_index_executes_in_wasm_and_matches_cpython() {
    let kernel_wat = emit_module(&at_module()).expect("s[i] program lowers through emit_module");
    assert!(
        kernel_wat.contains("call $__alloc")
            && kernel_wat.contains("i32.load8_u")
            && kernel_wat.contains("i32.store8"),
        "s[i] allocates + byte-copies a new 1-char string:\n{kernel_wat}"
    );

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-994: skipping EXECUTED s[i] witness — WABT absent. `at(s, i) = \
             s[i]` lowered through emit_module (1-char materialisation asserted); \
             a WABT box also runs it: at(\"WASM\", 2) == \"S\", matching CPython."
        );
        return;
    }

    // CPython: "WASM"[2] == "S" (the 1-char string at index 2). The fixture
    // string is preloaded at NAME_ADDR; the driver calls $at(s_ptr, 2), then
    // reads the constructed 1-char string's length + its single byte.
    let s = "WASM";
    let idx = 2_i64;
    let expected_byte = s.as_bytes()[idx as usize]; // b'S'
    let sb = s.as_bytes();
    let close = kernel_wat.rfind(')').expect("closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str(&format!(
        "  (data (i32.const {NAME_ADDR}) \"{}\")\n",
        i32_data_escape(sb.len() as i32)
    ));
    wat.push_str(&format!(
        "  (data (i32.const {}) \"{}\")\n",
        NAME_ADDR + 8,
        bytes_data_escape(sb)
    ));
    // run_len: the constructed string's length (must be 1).
    wat.push_str(&format!(
        "  (func (export \"run_len\") (result i32)\n    i32.const {NAME_ADDR}\n    i64.const {idx}\n    call $at\n    i32.load)\n"
    ));
    // run_byte_0: the single byte of the constructed 1-char string.
    wat.push_str(&format!(
        "  (func (export \"run_byte_0\") (result i32)\n    i32.const {NAME_ADDR}\n    i64.const {idx}\n    call $at\n    i32.const 8\n    i32.add\n    i32.load8_u)\n"
    ));
    wat.push_str(")\n");

    let dir = std::env::temp_dir().join(format!("xpile-wasm-strindex-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("at.wat");
    let wasm_path = dir.join("at.wasm");
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
    assert!(run.status.success(), "wasm-interp failed: {stdout}");

    let got_len = parse_i32_export(&stdout, "run_len");
    let got_byte = parse_i32_export(&stdout, "run_byte_0");
    assert_eq!(got_len, 1, "s[i] is a 1-char string (len 1)");
    assert_eq!(
        got_byte as u8, expected_byte,
        "WASM {s:?}[{idx}] byte = {got_byte} but CPython = {expected_byte} ('{}')",
        expected_byte as char
    );
    eprintln!(
        "PMAT-994: EXECUTED s[i] witness PASSED — `at(s, i) = s[i]` executed in \
         WABT: {s:?}[{idx}] = a 1-char string (len {got_len}) holding byte \
         {got_byte} ('{}'), value-matching CPython {s:?}[{idx}] = {:?}.",
        got_byte as u8 as char,
        &s[idx as usize..idx as usize + 1]
    );
}
