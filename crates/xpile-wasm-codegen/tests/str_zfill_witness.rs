//! PMAT-1173 — EXECUTED `s.zfill(width)` witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! `s.zfill(width)` materialises a NEW heap string equal to `s` left-padded with
//! ASCII `'0'` to `width` CODE POINTS, **sign-aware** — a leading `'+'` / `'-'`
//! stays first and the zeros go AFTER it. It joins the allocating string-method
//! family (`removeprefix` / `removesuffix` / `replace`) on the WASM lane: an
//! `Expr::StrMethod { op: ZFill }` in a string position lowers via the allocating
//! `$__wasm_str_zfill` helper (calls `$__alloc` + `memory.fill` + `memory.copy`,
//! rides the `needs_heap` gate) and uses `$__wasm_str_charlen` for the width math.
//!
//! ## The real program
//!
//! ```python
//! def zf(s: str, w: int) -> str:
//!     return s.zfill(w)
//! ```
//!
//! ## Why it is char-exact (no Unicode fold, unlike `.upper()`)
//!
//! The pad count is `max(0, width - charlen(s))`. The `'0'` bytes are pure ASCII
//! inserted at a code-point boundary (the very start, or immediately after a
//! 1-byte `'+'`/`'-'` sign), and the rest of `s` is copied byte-for-byte, so the
//! result is CORRECT for any valid UTF-8 (`"café".zfill(6)` == `"00café"`) — the
//! op has no per-character case/fold logic, so it needs no case table and takes
//! no ASCII-only posture.
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel `$zf`
//! takes an `i32` (the `s` param base-pointer, preloaded into a `(data …)`
//! region below `LITERAL_BASE`) and an `i64` (the width), returning the
//! constructed string's `i32` base-pointer. The witness adds only zero-arg
//! wrappers that push the constant `S_ADDR` + `w`, call `$zf`, and read back the
//! result: `run_len` (the i32 byte-count header @ result+0) and a `run_byte_i`
//! family (each re-runs `$zf` and `i32.load8_u`s payload byte `i`).
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_str_zfill` helper + call) on a host without
//! WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and
/// the bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0,
/// UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;

/// (input, width, CPython `input.zfill(width)`) — pinned to the exact CPython
/// ground truth (verified with python3). Covers every boundary the sign-aware
/// pad path can hit.
const CASES: &[(&str, i64, &str)] = &[
    ("42", 5, "00042"),    // the headline: pad a plain numeric string
    ("-42", 5, "-0042"),   // NEGATIVE sign — zeros go after the '-'
    ("+7", 4, "+007"),     // POSITIVE sign — zeros go after the '+'
    ("42", 1, "42"),       // width < len → plain copy (no pad)
    ("", 3, "000"),        // EMPTY string → three zeros, no sign read past end
    ("abc", 2, "abc"),     // non-numeric, width < len → copy
    ("-", 3, "-00"),       // sign ONLY (tail length 0 after the sign)
    ("café", 6, "00café"), // NON-ASCII — char-exact (é is 2 bytes, 1 code point)
    ("x", 0, "x"),         // width 0 → copy
    ("42", -1, "42"),      // NEGATIVE width → copy (pad clamps to 0)
    ("-5", 6, "-00005"),   // longer negative pad
    ("007", 5, "00007"),   // already-zeroed input pads further
];

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def zf(s: str, w: int) -> str: return s.zfill(w)`.
fn zf_module() -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::ZFill,
        args: vec![Expr::Ident("w".into())],
    };
    let f = Function {
        name: "zf".into(),
        params: vec![
            Param {
                name: "s".into(),
                ty: Type::Str,
                mutable: false,
            },
            Param {
                name: "w".into(),
                ty: Type::I64,
                mutable: false,
            },
        ],
        return_type: Type::Str,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "zf_program".into(),
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

/// Splice the preloaded `s` `(data …)` region + zero-arg read-back exports
/// (`run_len` / `run_byte_i`) onto the emitted module, before its closing `)`.
/// `n_out` = the expected result byte length.
fn build_witness_wat(kernel_wat: &str, s: &str, w: i64, n_out: usize) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1173 witness: preload the s param (below LITERAL_BASE)\n");
    let bytes = s.as_bytes();
    wat.push_str(&format!(
        "  (data (i32.const {S_ADDR}) \"{}\")\n",
        i32_data_escape(bytes.len() as i32)
    ));
    wat.push_str(&format!(
        "  (data (i32.const {}) \"{}\")\n",
        S_ADDR + 8,
        bytes_data_escape(bytes)
    ));
    wat.push_str(&format!(
        "  (func (export \"run_len\") (result i32)\n    \
           i32.const {S_ADDR}\n    i64.const {w}\n    call $zf\n    i32.load)\n"
    ));
    for i in 0..n_out {
        wat.push_str(&format!(
            "  (func (export \"run_byte_{i}\") (result i32)\n    \
               i32.const {S_ADDR}\n    i64.const {w}\n    call $zf\n    \
               i32.const {off}\n    i32.add\n    i32.load8_u)\n",
            off = 8 + i
        ));
    }
    wat.push_str(")\n");
    wat
}

/// Parse a `name() => i32:<value>` line from `wasm-interp --run-all-exports`.
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

/// Lower `zf(s, w) = s.zfill(w)`, run it in WABT with `s` preloaded, and
/// reconstruct the padded string. `None` when WABT is absent (caller skips the
/// value assertion). Asserts the WASM byte length matches CPython.
fn exec_zfill(s: &str, w: i64, expected: &str) -> Option<String> {
    let kernel_wat = emit_module(&zf_module()).expect("zfill program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let n_out = expected.len();
    let wat = build_witness_wat(&kernel_wat, s, w, n_out);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-zfill-{}-{}",
        std::process::id(),
        s.len().wrapping_mul(131).wrapping_add(w as usize)
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("zf.wat");
    let wasm_path = dir.join("zf.wasm");
    std::fs::write(&wat_path, &wat).expect("write wat");
    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed for {s:?}.zfill({w}):\n{}\n---WAT---\n{wat}",
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
        "wasm-interp run failed for {s:?}.zfill({w}): stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    let got_len = parse_i32_export(&stdout, "run_len");
    assert_eq!(
        got_len as usize, n_out,
        "{s:?}.zfill({w}) byte length: WASM={got_len} CPython={n_out}\nWAT:\n{wat}"
    );
    let mut bytes = Vec::with_capacity(n_out);
    for i in 0..n_out {
        bytes.push(parse_i32_export(&stdout, &format!("run_byte_{i}")) as u8);
    }
    Some(String::from_utf8(bytes).expect("constructed zfill string bytes are valid UTF-8"))
}

#[test]
fn cpython_zfill_ground_truth_is_pinned() {
    // The pinned CPython forms the witness value-matches. zfill left-pads with
    // ASCII '0' to `width` code points, keeping a leading sign first. `width` no
    // larger than the current length (incl. a negative width) is a plain copy.
    // (Rust has no `zfill`, so the ground truth is pinned here, verified vs
    // python3 when this slice landed.)
    assert_eq!("42", "42"); // sanity anchor for the literal encoding below
    for &(s, w, expected) in CASES {
        // char length of the result is max(width, char-len(s)).
        let cl = |t: &str| t.chars().count();
        assert_eq!(
            cl(expected),
            cl(s).max(if w < 0 { 0 } else { w as usize }),
            "pinned {s:?}.zfill({w}) = {expected:?} must have char-len max(width, len(s))"
        );
        // the pad is pure ASCII '0'; the non-'0' code points come only from s.
        assert!(
            expected.chars().filter(|c| *c != '0').count() <= cl(s),
            "pinned {s:?}.zfill({w}) = {expected:?} must add only '0' padding"
        );
    }
}

#[test]
fn zf_emits_zfill_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): the zfill program lowers
    // through the production emitter, carrying the helper + call + heap.
    let wat =
        emit_module(&zf_module()).expect("the s.zfill(w) program must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_str_zfill (param $s i32) (param $w i64) (result i32)"),
        "the zfill helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_zfill"),
        "$zf must call the zfill helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $zf (param $s i32) (param $w i64) (result i32)"),
        "str return → i32 result (heap pointer), int width → i64 param:\n{wat}"
    );
    // Materialising a padded string → needs the bump heap + the char helper.
    assert!(
        wat.contains("(func $__alloc"),
        "zfill needs the bump heap:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_charlen"),
        "zfill's width math needs the char-count helper:\n{wat}"
    );
}

#[test]
fn real_zfill_program_executes_in_wasm_and_matches_cpython() {
    let kernel_wat = emit_module(&zf_module()).expect("zfill program lowers through emit_module");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1173: skipping EXECUTED zfill witness — WABT (wat2wasm / \
             wasm-interp) absent. The zf program lowered through emit_module \
             (asserted in `zf_emits_zfill_helper_and_call`); a box with WABT also \
             runs it and asserts the CONSTRUCTED string == CPython."
        );
        return;
    }
    eprintln!("PMAT-1173: running EXECUTED s.zfill(w) witness via WABT");
    let mut ran = 0usize;
    for &(s, w, expected) in CASES {
        let got = exec_zfill(s, w, expected).expect("WABT present");
        assert_eq!(
            got, expected,
            "executed WASM {s:?}.zfill({w}) = {got:?} but CPython = {expected:?}"
        );
        ran += 1;
    }
    assert_eq!(ran, CASES.len());
    eprintln!(
        "PMAT-1173: all {ran} zfill cases executed in WABT and value-matched \
         CPython (incl. sign-aware '-42'->'-0042', non-ASCII 'café'->'00café', \
         empty ''->'000', negative width, sign-only '-'->'-00').\n\
         --- emitted zf WAT (emit_module over meta-HIR) ---\n{kernel_wat}"
    );
}
