//! PMAT-1058 — EXECUTED string-SLICE witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! Slice 3a (`literal_string_witness.rs`) shipped `s[i]` as a 1-char heap
//! string; this slice adds the general half-open `s[lo:hi]` — a char-exact
//! heap SUBSTRING with full Python slice semantics (negative-bound normalise,
//! clamp to `[0, len]`, `hi = max(hi, lo)`, a missing bound → `0` / `len`).
//!
//! The witness proves char-EXACTNESS (not byte slicing) by slicing across a
//! MULTI-BYTE code point: the fixture `"abécd"` has `é` as a 2-byte UTF-8 char
//! at char index 2, so `s[1:4]` must return `"béc"` (chars b, é, c → bytes
//! `[98, 195, 169, 99]`). A byte slice would split `é` and return garbage; the
//! `$__wasm_str_slice` char-walk returns the exact 4 payload bytes.
//!
//! ## The real program
//!
//! ```python
//! def mid(s: str) -> str:
//!     return s[1:4]          # char-exact half-open slice
//! ```
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel `$mid`
//! takes one `i32` base-pointer (the `s` str param) and RETURNS an `i32` (the
//! constructed substring's base-pointer). The witness adds only:
//!   1. one length-prefixed `(data …)` segment preloading `s` at a fixed
//!      address (below `LITERAL_BASE`, so it never overlaps the bump heap);
//!   2. a zero-arg `run_byte_i` family — each re-runs `$mid`, adds `8 + i`, and
//!      `i32.load8_u`s that payload byte of the CONSTRUCTED substring;
//!   3. a `run_len` export returning the substring's i32 byte count.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_str_slice` helper + call) on a host
//! without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// The multi-byte str-param fixture: `é` (U+00E9) is a 2-byte UTF-8 char.
const FIX_S: &str = "abécd";
/// `python3 -c "print('abécd'[1:4])"` == `béc` (chars b, é, c).
const CPYTHON_MID: &str = "béc";
/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and
/// the bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0,
/// UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def mid(s: str) -> str: return s[1:4]`.
fn mid_module() -> Module {
    let body = Expr::Slice {
        collection: Box::new(Expr::Ident("s".into())),
        lo: Some(Box::new(Expr::LitInt(1))),
        hi: Some(Box::new(Expr::LitInt(4))),
        of_str: true,
        step: None,
    };
    let f = Function {
        name: "mid".into(),
        params: vec![Param {
            name: "s".into(),
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
        name: "mid_program".into(),
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

/// Splice the `s` param `(data …)` region + per-byte readers onto the emitted
/// module, before its closing `)`. `n_out` = the expected substring byte length.
fn build_witness_wat(kernel_wat: &str, n_out: usize) -> String {
    let s = FIX_S.as_bytes();
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1058 witness: preload the str param (below LITERAL_BASE)\n");
    // s @ S_ADDR (length-prefixed: i32 BYTE count header + UTF-8 bytes).
    wat.push_str(&format!(
        "  (data (i32.const {S_ADDR}) \"{}\")\n",
        i32_data_escape(s.len() as i32)
    ));
    wat.push_str(&format!(
        "  (data (i32.const {}) \"{}\")\n",
        S_ADDR + 8,
        bytes_data_escape(s)
    ));
    // run_len: the substring's i32 byte count (header at result+0).
    wat.push_str(&format!(
        "  (func (export \"run_len\") (result i32)\n    i32.const {S_ADDR}\n    call $mid\n    i32.load)\n"
    ));
    // run_byte_i: byte i of the constructed substring. Each export re-runs mid
    // (fresh bump heap per invocation under --run-all-exports).
    for i in 0..n_out {
        wat.push_str(&format!(
            "  (func (export \"run_byte_{i}\") (result i32)\n    \
               i32.const {S_ADDR}\n    call $mid\n    \
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
fn cpython_slice_is_pinned_and_multibyte() {
    // `é` is 2 bytes in UTF-8 — the fixture genuinely exercises char (not byte)
    // indexing, and the pinned CPython value is the ground truth.
    assert!(!FIX_S.is_ascii(), "PMAT-1058 fixture must be multi-byte");
    assert_eq!(FIX_S, "abécd");
    assert_eq!(
        &FIX_S.chars().skip(1).take(3).collect::<String>(),
        CPYTHON_MID,
        "s[1:4] over 'abécd' must be 'béc' (chars b, é, c)"
    );
    // The é splits into two UTF-8 bytes — proving a BYTE slice would be wrong.
    assert_eq!(CPYTHON_MID.as_bytes(), &[98u8, 195, 169, 99]);
}

#[test]
fn mid_emits_slice_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): the slice program
    // lowers through the production emitter, carrying the slice helper + call.
    let wat = emit_module(&mid_module())
        .expect("the s[1:4] slice program must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_str_slice (param $s i32) (param $lo i64) (param $hi i64)"),
        "the slice helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_slice"),
        "$mid must call the slice helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $mid (param $s i32) (result i32)"),
        "str return → i32 result (heap pointer):\n{wat}"
    );
    // The slice materialises a substring → needs the bump heap + char helpers.
    assert!(
        wat.contains("(func $__alloc") && wat.contains("(func $__wasm_str_charlen"),
        "slice needs the bump heap + char-walk helpers:\n{wat}"
    );
}

#[test]
fn real_slice_program_executes_in_wasm_and_matches_cpython() {
    let kernel_wat =
        emit_module(&mid_module()).expect("s[1:4] slice program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1058: skipping EXECUTED string-slice witness — WABT \
             (wat2wasm / wasm-interp) absent. The mid program lowered through \
             emit_module (asserted in `mid_emits_slice_helper_and_call`); a box \
             with WABT also runs it and asserts the CONSTRUCTED substring == \
             CPython {CPYTHON_MID:?}. Free CI skips execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-1058: running EXECUTED string-slice (mid = s[1:4]) witness via WABT");

    let n_out = CPYTHON_MID.len(); // 4 bytes ('b' + 2-byte 'é' + 'c')
    let wat = build_witness_wat(&kernel_wat, n_out);

    let dir = std::env::temp_dir().join(format!("xpile-wasm-str-slice-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("mid.wat");
    let wasm_path = dir.join("mid.wasm");
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

    // Read back the constructed length + each byte, reassemble the substring.
    let got_len = parse_i32_export(&stdout, "run_len");
    assert_eq!(
        got_len as usize, n_out,
        "constructed substring byte length: WASM={got_len} CPython={n_out}\nWAT:\n{wat}"
    );
    let mut bytes = Vec::with_capacity(n_out);
    for i in 0..n_out {
        let b = parse_i32_export(&stdout, &format!("run_byte_{i}"));
        bytes.push(b as u8);
    }
    let got = String::from_utf8(bytes).expect("constructed substring bytes are valid UTF-8");

    assert_eq!(
        got, CPYTHON_MID,
        "executed WASM s[1:4] = {got:?} but CPython = {CPYTHON_MID:?}\nWAT:\n{wat}"
    );

    eprintln!(
        "PMAT-1058: EXECUTED string-slice witness PASSED — `mid(s) = s[1:4]` \
         lowered through emit_module and executed in WABT to {got:?} (len \
         {got_len}), value-matching the CPython result {CPYTHON_MID:?} over the \
         MULTI-BYTE fixture s={FIX_S:?}. Char-exact: the 2-byte `é` is copied \
         whole, never split — a byte slice would have been wrong."
    );
    eprintln!("--- emitted mid WAT (emit_module over meta-HIR) ---\n{kernel_wat}");
}

// ─── PMAT-1058: EXECUTED clamp / negative / open-bound / empty edge cases ────
//
// The headline witness proves the char-walk + alloc + copy path on `s[1:4]`.
// The Python CLAMP semantics — negative-bound normalise, out-of-range clamp
// (NEVER trap), `hi = max(hi, lo)` (empty slice), and the missing-bound
// defaults — are exactly where a slice-helper bug hides, so each is executed on
// silicon and value-matched to CPython (adversarial-verify discipline, not
// just asserted from the emit text).

/// Build `def f(s: str) -> str: return s[lo:hi]` for the given optional bounds.
fn slice_module(lo: Option<i64>, hi: Option<i64>) -> Module {
    let body = Expr::Slice {
        collection: Box::new(Expr::Ident("s".into())),
        lo: lo.map(|v| Box::new(Expr::LitInt(v))),
        hi: hi.map(|v| Box::new(Expr::LitInt(v))),
        of_str: true,
        step: None,
    };
    let f = Function {
        name: "mid".into(),
        params: vec![Param {
            name: "s".into(),
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
        name: "slice_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// Lower `s[lo:hi]` over `FIX_S`, run it in WABT, and reconstruct the substring.
/// Returns `None` when WABT is absent (the caller skips the value assertion).
fn exec_slice(lo: Option<i64>, hi: Option<i64>, expected: &str) -> Option<String> {
    let kernel_wat = emit_module(&slice_module(lo, hi)).expect("slice program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let n_out = expected.len();
    let wat = build_witness_wat(&kernel_wat, n_out);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-slice-edge-{}-{lo:?}-{hi:?}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("s.wat");
    let wasm_path = dir.join("s.wasm");
    std::fs::write(&wat_path, &wat).expect("write wat");
    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed for s[{lo:?}:{hi:?}]:\n{}\n---WAT---\n{wat}",
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
        "wasm-interp run failed for s[{lo:?}:{hi:?}]: stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    let got_len = parse_i32_export(&stdout, "run_len");
    assert_eq!(
        got_len as usize, n_out,
        "s[{lo:?}:{hi:?}] byte length: WASM={got_len} CPython={n_out}"
    );
    let mut bytes = Vec::with_capacity(n_out);
    for i in 0..n_out {
        bytes.push(parse_i32_export(&stdout, &format!("run_byte_{i}")) as u8);
    }
    Some(String::from_utf8(bytes).expect("valid UTF-8"))
}

#[test]
fn slice_clamp_negative_open_empty_edges_match_cpython() {
    // (lo, hi, CPython s[lo:hi] over "abécd") — pinned to python3 ground truth.
    let cases: &[(Option<i64>, Option<i64>, &str)] = &[
        (Some(2), None, "écd"),       // open hi (missing → charlen)
        (None, Some(2), "ab"),        // open lo (missing → 0)
        (Some(-2), None, "cd"),       // negative lo normalised (+= charlen)
        (Some(1), Some(100), "bécd"), // over-range hi CLAMPS, never traps
        (Some(3), Some(1), ""),       // hi < lo → empty, never negative length
        (None, None, "abécd"),        // s[:] → the whole string
    ];
    for &(lo, hi, expected) in cases {
        // CONSTRUCT: every form lowers through the production emitter.
        let wat = emit_module(&slice_module(lo, hi)).expect("edge slice lowers");
        assert!(wat.contains("call $__wasm_str_slice"));
        // EXECUTE (when WABT present): value-match CPython.
        match exec_slice(lo, hi, expected) {
            Some(got) => assert_eq!(
                got, expected,
                "executed s[{lo:?}:{hi:?}] = {got:?} but CPython = {expected:?}"
            ),
            None => {
                eprintln!(
                    "PMAT-1058: WABT absent — skipped executing s[{lo:?}:{hi:?}] \
                     (expected {expected:?}); emit path asserted."
                );
                return;
            }
        }
    }
    eprintln!(
        "PMAT-1058: all 6 clamp/negative/open/empty edge cases executed in WABT \
         and value-matched CPython over the multi-byte fixture {FIX_S:?}."
    );
}
