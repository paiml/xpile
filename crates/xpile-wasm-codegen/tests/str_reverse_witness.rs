//! PMAT-1213 — EXECUTED `s[::-1]` witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! `s[::-1]` materialises a NEW heap string with the CODE POINTS of `s` in reverse
//! order. The Python frontend lowers the reversed-slice `s[::-1]` (over a `str`) to
//! an `Expr::StrMethod { op: Reverse }` (PMAT-530), which in a string position lowers
//! via the allocating `$__wasm_str_reverse` helper (calls `$__alloc` + `memory.copy`,
//! rides the `needs_heap` gate). It joins the allocating string-method family
//! (`removeprefix` / `removesuffix` / `replace` / `zfill` / the pad + case-fold ops).
//!
//! ## The real program
//!
//! ```python
//! def rev(s: str) -> str:
//!     return s[::-1]
//! ```
//!
//! ## CHAR-EXACT with NO trap arm — strictly stronger than the case-fold family
//!
//! Unlike `upper` / `lower` / `title` / `swapcase` (which need a Unicode case table
//! and TRAP on a non-ASCII byte), reversing by code point needs NO table: the UTF-8
//! lead byte alone gives each code point's byte length (1 for `< 0x80`, 2 for
//! `0xC0`–`0xDF`, 3 for `0xE0`–`0xEF`, 4 for `>= 0xF0`). The helper copies each code
//! point as an INTACT unit to a descending output position, so a multi-byte code
//! point is moved WHOLE (never byte-reversed, which would corrupt its encoding). So
//! the result is char-exact for ANY valid UTF-8 (`"café"[::-1] == "éfac"`), matching
//! CPython's code-point reversal AND the rust / ruchy `.chars().rev()` lane — with no
//! runtime refusal. `NON_ASCII_CASES` proves this end-to-end in WABT: multi-byte
//! Greek / CJK / `€` / emoji inputs reverse char-exactly and NEVER trap.
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel `$rev` takes
//! an `i32` (the `s` param base-pointer, preloaded into a `(data …)` region below
//! `LITERAL_BASE`) and returns the reversed string's `i32` base-pointer. The witness
//! adds only zero-arg wrappers that push the constant `S_ADDR`, call the kernel, and
//! read back the result: `run_len` (the i32 byte-count header @ result+0) and a
//! `run_byte_i` family (each re-runs the kernel and `i32.load8_u`s payload byte `i`).
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT path
//! lowers + carries the `$__wasm_str_reverse` helper + call) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and the
/// bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0, UTF-8
/// bytes @ base+8).
const S_ADDR: i32 = 16;

/// Pure-ASCII reverse cases — `(input, CPython input[::-1])`, pinned to the exact
/// CPython ground truth (verified with python3). Reversal preserves the byte count.
const ASCII_CASES: &[(&str, &str)] = &[
    ("abc", "cba"),                     // the headline
    ("", ""),                           // empty -> empty (no payload)
    ("a", "a"),                         // single char -> itself
    ("hello", "olleh"),                 // odd length
    ("racecar", "racecar"),             // palindrome -> itself
    ("ab cd", "dc ba"),                 // spaces reverse with the rest
    ("Hello, World!", "!dlroW ,olleH"), // punctuation + space + mixed case
    ("1234", "4321"),                   // digits
];

/// Non-ASCII reverse cases — the CHAR-EXACT, NO-TRAP differentiator vs the case-fold
/// family. Multi-byte UTF-8 code points reverse by code point (moved whole), matching
/// CPython. `(input, CPython input[::-1])`, pinned vs python3.
const NON_ASCII_CASES: &[(&str, &str)] = &[
    ("café", "éfac"), // 'é' = 2-byte (0xC3 0xA9) — moved whole to the front
    ("αβγ", "γβα"),   // Greek, three 2-byte code points
    ("日本", "本日"), // CJK, two 3-byte code points
    ("a€b", "b€a"),   // '€' = 3-byte (0xE2 0x82 0xAC), ASCII on both ends
    ("🎉x", "x🎉"),   // '🎉' = 4-byte astral code point
    ("π=3", "3=π"),   // mixed 2-byte + ASCII
];

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def rev(s: str) -> str: return s[::-1]`.
fn rev_module(name: &str) -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::Reverse,
        args: vec![],
    };
    let f = Function {
        name: name.into(),
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

/// Splice the preloaded `s` `(data …)` region + zero-arg read-back exports
/// (`run_len` / `run_byte_i`) onto the emitted module, before its closing `)`.
/// `kernel` = the emitted kernel function name (`rev`); `n_out` = the expected result
/// byte length.
fn build_witness_wat(kernel_wat: &str, kernel: &str, s: &str, n_out: usize) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1213 witness: preload the s param (below LITERAL_BASE)\n");
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
           i32.const {S_ADDR}\n    call ${kernel}\n    i32.load)\n"
    ));
    for i in 0..n_out {
        wat.push_str(&format!(
            "  (func (export \"run_byte_{i}\") (result i32)\n    \
               i32.const {S_ADDR}\n    call ${kernel}\n    \
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

/// Lower `rev(s) = s[::-1]`, run it in WABT with `s` preloaded, and reconstruct the
/// reversed string. `None` when WABT is absent (caller skips the value assertion).
/// Asserts the WASM byte length matches CPython.
fn exec_case(s: &str, expected: &str) -> Option<String> {
    let kernel_wat = emit_module(&rev_module("rev")).expect("rev program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let n_out = expected.len();
    let wat = build_witness_wat(&kernel_wat, "rev", s, n_out);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-reverse-{}-{}",
        std::process::id(),
        s.len().wrapping_mul(131).wrapping_add(n_out)
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
        "wat2wasm failed for {s:?}[::-1]:\n{}\n---WAT---\n{wat}",
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
        "wasm-interp run failed for {s:?}[::-1]: stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    let got_len = parse_i32_export(&stdout, "run_len");
    assert_eq!(
        got_len as usize, n_out,
        "{s:?}[::-1] byte length: WASM={got_len} CPython={n_out}\nWAT:\n{wat}"
    );
    let mut bytes = Vec::with_capacity(n_out);
    for i in 0..n_out {
        bytes.push(parse_i32_export(&stdout, &format!("run_byte_{i}")) as u8);
    }
    // The reversed bytes MUST be valid UTF-8 — proof that multi-byte code points were
    // moved whole (a byte-reversal would corrupt the encoding into invalid UTF-8).
    Some(String::from_utf8(bytes).expect("reversed string bytes are valid UTF-8 (code-point move)"))
}

#[test]
fn cpython_reverse_ground_truth_is_pinned() {
    // The pinned CPython forms the witness value-matches. Rust's `.chars().rev()`
    // reverses by Unicode scalar value, exactly like Python `s[::-1]` — and byte
    // length is preserved (reversal only reorders code points). (Pins verified vs
    // python3 when this slice landed.)
    for &(s, rev) in ASCII_CASES.iter().chain(NON_ASCII_CASES) {
        assert_eq!(
            s.chars().rev().collect::<String>(),
            rev,
            "pinned {s:?}[::-1]"
        );
        assert_eq!(
            s.len(),
            rev.len(),
            "reversal preserves byte length for {s:?}"
        );
    }
    // The differentiator vs the case-fold family: NON_ASCII_CASES really are
    // non-ASCII (they exercise the multi-byte code-point copy, which upper/title
    // would TRAP on).
    for &(s, _) in NON_ASCII_CASES {
        assert!(
            !s.is_ascii(),
            "non-ASCII case {s:?} must contain a >= 0x80 byte"
        );
    }
}

#[test]
fn reverse_emits_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): the program lowers through
    // the production emitter, carrying the helper + call + heap.
    let wat = emit_module(&rev_module("rev"))
        .expect("the s[::-1] program must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_str_reverse (param $s i32) (result i32)"),
        "the reverse helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_reverse"),
        "$rev must call the reverse helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $rev (param $s i32) (result i32)"),
        "str return → i32 result (heap pointer), str param → i32:\n{wat}"
    );
    // Materialising a reversed string → needs the bump heap.
    assert!(
        wat.contains("(func $__alloc"),
        "reverse needs the bump heap:\n{wat}"
    );
    // The code-point copy uses `memory.copy` (an intact-unit move, not a byte flip).
    assert!(
        wat.contains("memory.copy"),
        "the helper must copy each code point as a unit via memory.copy:\n{wat}"
    );
    // CHAR-EXACT, NO trap arm: unlike the case-fold family (which `unreachable`s on a
    // non-ASCII byte), the reverse HELPER ITSELF carries no `unreachable`. (Scope the
    // check to the helper's own function chunk — the always-emitted bump `$__alloc`
    // carries an OOM-guard `unreachable`, so the whole module is not trap-free.)
    let helper = wat
        .split("(func ")
        .find(|chunk| chunk.starts_with("$__wasm_str_reverse "))
        .expect("the reverse helper chunk must be present");
    assert!(
        !helper.contains("unreachable"),
        "reverse is char-exact with NO trap arm — the `$__wasm_str_reverse` helper \
         must carry no `unreachable`:\n{helper}"
    );
}

#[test]
fn real_reverse_program_executes_in_wasm_and_matches_cpython() {
    let rev_wat = emit_module(&rev_module("rev")).expect("rev program lowers through emit_module");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1213: skipping EXECUTED reverse witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module \
             (asserted in `reverse_emits_helper_and_call`); a box with WABT also runs \
             it and asserts the CONSTRUCTED string == CPython."
        );
        return;
    }
    eprintln!("PMAT-1213: running EXECUTED s[::-1] witness via WABT");
    let mut ran = 0usize;
    for &(s, rev) in ASCII_CASES.iter().chain(NON_ASCII_CASES) {
        let got = exec_case(s, rev).expect("WABT present");
        assert_eq!(
            got, rev,
            "executed WASM {s:?}[::-1] = {got:?} but CPython = {rev:?}"
        );
        ran += 1;
    }
    assert_eq!(ran, ASCII_CASES.len() + NON_ASCII_CASES.len());
    eprintln!(
        "PMAT-1213: all {ran} inputs executed in WABT and value-matched CPython \
         (incl. the CHAR-EXACT, NO-TRAP non-ASCII cases 'café'->'éfac', 'αβγ'->'γβα', \
         '日本'->'本日', 'a€b'->'b€a', '🎉x'->'x🎉' — multi-byte code points moved \
         whole, reversed bytes valid UTF-8).\n\
         --- emitted rev WAT (emit_module over meta-HIR) ---\n{rev_wat}"
    );
}
