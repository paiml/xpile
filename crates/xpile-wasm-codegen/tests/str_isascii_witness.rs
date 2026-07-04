//! PMAT-1199 — EXECUTED `s.isascii()` witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM`).
//!
//! `s.isascii()` is the SEVENTH of the `str` `is*` PREDICATE family to reach the
//! WASM lane (after PMAT-1189 `s.isdigit()`, 1191 `s.isalpha()`, 1193
//! `s.isspace()`, 1195 `s.isalnum()`, 1197 `s.isupper()`/`s.islower()`) — and the
//! ODD ONE OUT: it is FULLY DECIDABLE at the byte level. It is a bool (i32 0/1)
//! result from a single left-to-right scan of the payload bytes, so it does NOT
//! ride the `needs_heap` gate and carries no bump allocator. `Expr::StrMethod {
//! op: IsAscii }` in a value position lowers via the `$__wasm_str_isascii` helper;
//! before this slice it fell through to the honest catch-all refusal.
//!
//! ## The real program
//!
//! ```python
//! def is_ascii(s: str) -> bool:
//!     return s.isascii()
//! ```
//!
//! ## Semantics — every byte < 0x80
//!
//! Python `str.isascii()` is `True` iff every code point is in the ASCII range
//! (U+0000..=U+007F). In UTF-8 that is exactly "every byte is `< 0x80`" (an ASCII
//! code point is one byte `< 0x80`; any non-ASCII code point has a lead/continuation
//! byte `>= 0x80`), so the helper answers it with a plain byte scan.
//!
//! ## The distinguishing shape — NEVER traps, needs NO empty guard
//!
//! Unlike the six sibling predicates (which ask an undecidable Unicode-category
//! question the moment a non-ASCII byte is reached, and so TRAP on it), `isascii()`
//! asks *exactly* the byte-level question, so:
//!   * a byte `>= 0x80` is the DEFINITIVE `False` — the helper returns `0`, it does
//!     NOT execute `unreachable`. There is no trap arm at all
//!     (`non_ascii_returns_false_and_never_traps`), so non-ASCII inputs are
//!     value-matched against CPython here (impossible for the trapping siblings).
//!   * the empty string is `True` — Python `"".isascii()` is `True` (unlike the
//!     isdigit family's vacuous-`False`), so the helper has NO empty guard; the
//!     loop simply falls through to `1` for a zero-length `s`.
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel
//! (`$is_ascii`) takes an `i32` (the `s` param base-pointer, preloaded into a
//! `(data …)` region below `LITERAL_BASE`) and returns the i32 bool directly. The
//! witness adds only a zero-arg `run` export that pushes the constant `S_ADDR`,
//! calls the kernel, and returns its i32.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_str_isascii` helper, declares memory, pulls
//! in NO bump allocator, and — distinctively — emits NO `unreachable` trap arm) on
//! a host without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and
/// the bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0,
/// UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;

/// Python `str.isascii()` reference — `True` iff every UTF-8 byte is `< 0x80`
/// (equivalently `s.is_ascii()`). Byte-exact against CPython for EVERY input
/// (ASCII → True, any non-ASCII → False), so — unlike the trapping siblings —
/// non-ASCII inputs can be pinned AND executed. Used both to PIN the expectations
/// and as the ground truth the witness value-matches.
fn py_isascii(s: &str) -> bool {
    s.bytes().all(|b| b < 0x80)
}

/// (input, CPython `input.isascii()`) — pinned to the exact CPython ground truth.
/// Both ASCII (→ True) and non-ASCII (→ False) inputs are present: the non-ASCII
/// cases return False by EXECUTION (never a trap), the property that sets isascii
/// apart from every other `is*` predicate on this lane.
const CASES: &[(&str, bool)] = &[
    ("", true),                  // empty -> True (Python; NO empty guard — the odd one out)
    ("abc", true),               // all ASCII letters
    ("ABC123", true),            // ASCII letters + digits
    ("hello world!", true),      // ASCII letters + space + punctuation
    ("\u{0000}", true),          // NUL (0x00) — the lowest ASCII byte
    ("\u{007f}", true),          // DEL (0x7f) — the highest ASCII byte, still ASCII
    ("~", true),                 // '~' (0x7e) — printable ASCII just below 0x7f
    ("\t\n\r", true),            // ASCII control whitespace (all < 0x80)
    ("caf\u{00e9}", false),      // café — é = U+00E9 (0xc3 0xa9), a non-ASCII byte -> False
    ("na\u{00ef}ve", false),     // naïve — ï = U+00EF (0xc3 0xaf)
    ("\u{00c1}", false),         // Á = U+00C1 (0xc3 0x81) — leading non-ASCII byte
    ("\u{03c0}", false),         // π = U+03C0 (0xcf 0x80)
    ("\u{65e5}\u{672c}", false), // 日本 — 3-byte CJK code points
    ("a\u{00e9}", false),        // ASCII prefix then non-ASCII -> scan reaches 0xc3 -> False
    ("\u{00e9}a", false),        // non-ASCII first -> False on byte 0
];

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def is_ascii(s: str) -> bool: return s.isascii()`.
fn isascii_module(name: &str) -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::IsAscii,
        args: vec![],
    };
    let f = Function {
        name: name.into(),
        params: vec![Param {
            name: "s".into(),
            ty: Type::Str,
            mutable: false,
        }],
        return_type: Type::Bool,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: format!("{name}_program"),
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

/// Splice the preloaded `s` `(data …)` region + a zero-arg `run` export
/// (`$<kernel>(S_ADDR)`) onto the emitted module, before its closing `)`.
fn build_witness_wat(kernel_wat: &str, kernel: &str, s: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1199 witness: preload the s param (below LITERAL_BASE)\n");
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
        "  (func (export \"run\") (result i32)\n    \
           i32.const {S_ADDR}\n    call ${kernel})\n"
    ));
    wat.push_str(")\n");
    wat
}

/// Parse a `run() => i32:<value>` line from `wasm-interp --run-all-exports`.
fn parse_run_i32(stdout: &str) -> i32 {
    let line = stdout
        .lines()
        .find(|l| l.contains("run() => i32:"))
        .unwrap_or_else(|| panic!("no `run` i32 export in interp output:\n{stdout}"));
    let idx = line.find("=> i32:").unwrap();
    line[idx + "=> i32:".len()..]
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("parse i32 from {line:?}"))
}

/// A short, filesystem-safe tag for `s` used in temp-dir names (non-ASCII bytes
/// can't sit in a path component on every host, so hash the bytes instead).
fn dir_tag(s: &str) -> usize {
    s.len()
        .wrapping_mul(131)
        .wrapping_add(s.bytes().map(|b| b as usize).sum::<usize>())
}

/// Lower `is_ascii(s) = s.isascii()`, run it in WABT with `s` preloaded, return
/// the bool. `None` when WABT is absent (caller skips the value assertion).
fn exec_case(s: &str) -> Option<bool> {
    let kernel = "is_ascii";
    let kernel_wat = emit_module(&isascii_module(kernel)).expect("isascii program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, kernel, s);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-isascii-{}-{}",
        std::process::id(),
        dir_tag(s)
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("case.wat");
    let wasm_path = dir.join("case.wasm");
    std::fs::write(&wat_path, &wat).expect("write wat");
    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed for {s:?}.isascii():\n{}\n---WAT---\n{wat}",
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
        "wasm-interp run failed for {s:?}.isascii(): stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    Some(parse_run_i32(&stdout) != 0)
}

#[test]
fn cpython_isascii_ground_truth_is_pinned() {
    // Every pin equals the `str.isascii()` reference (all bytes < 0x80). Verified
    // vs python3 when this slice landed.
    for &(s, want) in CASES {
        assert_eq!(py_isascii(s), want, "pinned {s:?}.isascii()");
    }
    // The empty string must be present and True — the distinguishing no-empty-guard
    // case (Python `"".isascii()` is True, the opposite of the isdigit family).
    assert!(CASES.iter().any(|&(s, want)| s.is_empty() && want));
    // At least one ASCII True and one non-ASCII False must be present (else the
    // predicate could be a constant) …
    assert!(CASES.iter().any(|&(s, want)| s.is_ascii() && want));
    assert!(CASES.iter().any(|&(s, want)| !s.is_ascii() && !want));
    // … both ASCII byte extremes must be pinned True (0x00 and 0x7f) …
    assert!(CASES.iter().any(|&(s, want)| s == "\u{0000}" && want));
    assert!(CASES.iter().any(|&(s, want)| s == "\u{007f}" && want));
    // … and a non-ASCII byte AFTER an ASCII prefix must be exercised (the scan
    // must reach it, not just answer on byte 0):
    assert!(CASES.iter().any(|&(s, want)| s == "a\u{00e9}" && !want));
}

#[test]
fn isascii_emit_helper_call_memory_no_allocator_and_no_trap() {
    // CONSTRUCT assertion (holds with or without WABT): the program lowers through
    // the production emitter, carrying the helper + call, declaring memory (the
    // scan reads the str bytes), pulling in NO bump allocator (a bool
    // materialises nothing), and — the DISTINGUISHING shape of isascii — emitting
    // NO `unreachable` trap arm (it is fully decidable, so a non-ASCII byte is a
    // definitive `0`, never a trap; every trapping sibling asserts the opposite).
    let wat = emit_module(&isascii_module("is_ascii")).expect("the s.isascii() program must lower");
    assert!(
        wat.contains("(func $__wasm_str_isascii (param $s i32) (result i32)"),
        "the isascii helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_isascii"),
        "$is_ascii must call the helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $is_ascii (param $s i32) (result i32)"),
        "bool return → i32 result, str param → i32:\n{wat}"
    );
    assert!(
        wat.contains("(memory (export \"mem\")"),
        "the scan reads the str payload → memory must be declared:\n{wat}"
    );
    // A bool predicate allocates nothing — no bump allocator, no heap gate.
    assert!(
        !wat.contains("(func $__alloc"),
        "isascii is non-allocating — it must NOT pull in the bump allocator:\n{wat}"
    );
    // The DISTINGUISHING assertion: isascii is fully decidable, so its HELPER emits
    // NO trap. (The isdigit/isalpha/isspace/isalnum/isupper/islower witnesses each
    // assert the OPPOSITE — the helper CONTAINS `unreachable` — for their
    // undecidable non-ASCII byte.) The check is scoped to the `$__wasm_str_isascii`
    // function body, since any str-touching module co-emits an unrelated
    // `$__wasm_str_char_at` bounds helper whose out-of-range arm DOES trap
    // (`unreachable ;; string index out of range`) — that trap is not ours.
    let helper = isascii_helper_body(&wat);
    assert!(
        !helper.contains("unreachable"),
        "isascii is fully decidable — the $__wasm_str_isascii helper must NOT emit \
         an `unreachable` trap arm (a non-ASCII byte is a definitive False, not a \
         trap):\n{helper}"
    );
}

/// Slice out the `$__wasm_str_isascii` function body — from its `(func …)` header
/// to the start of the next `(func ` (or end of module) — so the no-trap check
/// is scoped to OUR helper, not any unrelated co-emitted helper (e.g. the
/// `$__wasm_str_char_at` bounds trap).
fn isascii_helper_body(wat: &str) -> &str {
    let start = wat
        .find("(func $__wasm_str_isascii")
        .expect("the isascii helper must be emitted");
    let rest = &wat[start..];
    // The next function header after the helper (helper has no nested `(func`).
    let end = rest[1..]
        .find("(func ")
        .map(|i| i + 1)
        .unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn real_isascii_program_executes_in_wasm_and_matches_cpython() {
    if !wasm_runtime_available() {
        // Still exercise the lowering (asserted structurally elsewhere).
        emit_module(&isascii_module("is_ascii")).expect("isascii lowers");
        eprintln!(
            "PMAT-1199: skipping EXECUTED isascii witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module (asserted \
             in `isascii_emit_helper_call_memory_no_allocator_and_no_trap`); a box \
             with WABT also runs it and asserts the bool == CPython."
        );
        return;
    }
    eprintln!("PMAT-1199: running EXECUTED s.isascii() witnesses via WABT");
    let mut ran = 0usize;
    let mut non_ascii = 0usize;
    for &(s, want) in CASES {
        let got = exec_case(s).expect("WABT present");
        assert_eq!(
            got, want,
            "executed WASM {s:?}.isascii() = {got} but CPython = {want}"
        );
        ran += 1;
        if !s.is_ascii() {
            non_ascii += 1;
        }
    }
    assert_eq!(ran, CASES.len());
    // The whole point: non-ASCII inputs were EXECUTED (not skipped, not trapped)
    // and value-matched CPython — isascii is the one predicate that can.
    assert!(
        non_ascii >= 5,
        "the witness must execute several non-ASCII inputs (they return False, \
         never trap) — only {non_ascii} present"
    );
    eprintln!(
        "PMAT-1199: all {ran} inputs executed in WABT and value-matched CPython \
         ({non_ascii} of them non-ASCII → False by execution, NOT a trap; ''/'abc'/\
         '\\x00'/'\\x7f'→True incl. the empty string WITHOUT an empty guard)."
    );
}

#[test]
fn non_ascii_returns_false_and_never_traps() {
    // The distinguishing correctness of isascii vs every other `is*` predicate on
    // this lane: a non-ASCII byte is a DEFINITIVE `False` reached by EXECUTION —
    // the helper returns `0`, it does NOT trap (`unreachable`). "café" → False,
    // "π" → False, matching CPython exactly (both `"café".isascii()` and
    // `"π".isascii()` are False).
    for s in ["caf\u{00e9}", "\u{03c0}", "a\u{00e9}"] {
        assert!(!s.is_ascii(), "the fixture must carry a non-ASCII byte");
        emit_module(&isascii_module("is_ascii")).expect("isascii program lowers");
        if !wasm_runtime_available() {
            eprintln!(
                "PMAT-1199: skipping non-ASCII execution witness ({s:?}) — WABT \
                 absent. The no-trap shape is asserted structurally in \
                 `isascii_emit_helper_call_memory_no_allocator_and_no_trap` \
                 (the emitted module contains no `unreachable`)."
            );
            continue;
        }
        // A clean run that returns 0 (False) — never a trap.
        let got = exec_case(s).expect("WABT present");
        assert!(
            !got,
            "{s:?}.isascii() must return False (every non-ASCII byte is a definitive \
             False), matching CPython"
        );
        assert!(
            !py_isascii(s),
            "CPython ground truth for {s:?}.isascii() is False"
        );
        eprintln!(
            "PMAT-1199: {s:?}.isascii() correctly returned False by EXECUTION (a \
             non-ASCII byte is a definitive answer, never a trap) — the property no \
             other `is*` predicate on this lane has."
        );
    }
}
