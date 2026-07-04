//! PMAT-1195 — EXECUTED `s.isalnum()` witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM`).
//!
//! `s.isalnum()` is the FOURTH of the `str` `is*` PREDICATE family to reach the
//! WASM lane (after PMAT-1189 `s.isdigit()`, PMAT-1191 `s.isalpha()` and
//! PMAT-1193 `s.isspace()`) and their direct twin: a bool (i32 0/1) result from a
//! single left-to-right scan of the payload bytes, so it does NOT ride the
//! `needs_heap` gate and carries no bump allocator. It differs from the siblings
//! only in the per-byte ASCII-membership test — the ASCII ALPHANUMERIC set, the
//! direct UNION of the isdigit and isalpha ranges: `0x30`–`0x39` (`'0'`–`'9'`),
//! `0x41`–`0x5A` (`'A'`–`'Z'`) and `0x61`–`0x7A` (`'a'`–`'z'`). `Expr::StrMethod {
//! op: IsAlnum }` in a value position lowers via the non-allocating
//! `$__wasm_str_isalnum` helper; before this slice it fell through to the honest
//! catch-all refusal.
//!
//! ## The real program
//!
//! ```python
//! def is_an(s: str) -> bool:
//!     return s.isalnum()
//! ```
//!
//! ## Semantics — non-empty AND every code point ASCII alphanumeric
//!
//! Python `str.isalnum()` is `True` iff the string is NON-EMPTY and every char is
//! alphanumeric. The empty string is `False` (a vacuous "all" is still `False`
//! here), so the helper returns `0` before the loop when `len == 0`.
//!
//! ## ASCII-only, with an honest boundary — but short-circuited on a definitive
//! ## answer first (the distinguishing correctness of a predicate)
//!
//! Python also accepts non-ASCII Unicode alphanumerics (`"\u{00b2}".isalnum()` — a
//! superscript two — and `"\u{00e9}".isalnum()` are both `True`), which needs a
//! Unicode table this scalar lane does not carry. The scan is therefore ordered so
//! a DEFINITIVE answer never traps:
//!   * a NON-ASCII byte (`>= 0x80`) is examined only when every prior byte was
//!     ASCII alphanumeric — the result is then genuinely undecidable, so it TRAPS
//!     (`unreachable`), exactly like `isdigit` / `isalpha` / `isspace`, rather than
//!     returning a wrong bool (`non_ascii_all_alnum_prefix_traps`);
//!   * a DEFINITIVELY non-alphanumeric ASCII byte (any byte `< 0x80` outside the
//!     three ranges) short-circuits to `0` BEFORE any later non-ASCII byte is
//!     examined, so `"a!\u{00e9}".isalnum()` returns `0` (Python's answer) and
//!     NEVER traps (`non_alnum_ascii_before_non_ascii_returns_false_no_trap`).
//!
//! So a pure-ASCII `s` is answer-exact; a non-ASCII `s` whose ASCII prefix is all
//! alphanumeric aborts; a non-ASCII `s` with an earlier non-alphanumeric ASCII
//! byte returns `0`. It never passes an unmapped non-ASCII byte off as a wrong
//! bool.
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel `$is_an`
//! takes an `i32` (the `s` param base-pointer, preloaded into a `(data …)` region
//! below `LITERAL_BASE`) and returns the i32 bool directly. The witness adds only
//! a zero-arg `run` export that pushes the constant `S_ADDR`, calls the kernel,
//! and returns its i32.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_str_isalnum` helper, declares memory, and
//! pulls in NO bump allocator) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and
/// the bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0,
/// UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;

/// Pure-ASCII Python `str.isalnum()` reference — `True` iff `s` is non-empty and
/// every byte is ASCII alphanumeric (the three contiguous ranges `0x30`–`0x39`,
/// `0x41`–`0x5A` and `0x61`–`0x7A`). For ASCII inputs this is exactly CPython's
/// `str.isalnum()` (which additionally accepts Unicode letters/digits/numerics,
/// out of this lane's scope — those TRAP). Used both to PIN the `CASES`
/// expectations and as the ground truth the witness value-matches.
fn py_isalnum_ascii(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| {
            (0x30..=0x39).contains(&b) || (0x41..=0x5a).contains(&b) || (0x61..=0x7a).contains(&b)
        })
}

/// (input, CPython `input.isalnum()`) — pinned to the exact CPython ground truth.
/// ASCII-only inputs, since the WASM lane traps on a non-ASCII byte reached with
/// an all-alphanumeric prefix (see `non_ascii_all_alnum_prefix_traps`).
const CASES: &[(&str, bool)] = &[
    ("abc", true),    // lowercase letters
    ("ABC", true),    // uppercase letters
    ("123", true),    // digits
    ("a1b", true),    // mixed letter/digit
    ("Hello2", true), // mixed case + digit
    ("0", true),      // '0' (0x30) — lower boundary of the digit range
    ("9", true),      // '9' (0x39) — upper boundary of the digit range
    ("A", true),      // 'A' (0x41) — lower boundary of the uppercase range
    ("Z", true),      // 'Z' (0x5a) — upper boundary of the uppercase range
    ("a", true),      // 'a' (0x61) — lower boundary of the lowercase range
    ("z", true),      // 'z' (0x7a) — upper boundary of the lowercase range
    ("", false),      // empty -> False (Python's vacuous-all is still False)
    ("a b", false),   // interior space (0x20) — not alphanumeric
    ("a!b", false),   // interior '!' (0x21)
    ("a_b", false),   // interior '_' (0x5f, in the upper/lower gap)
    ("1.5", false),   // '.' (0x2e)
    (" ", false),     // a lone space
    ("/", false),     // '/' (0x2f) just below '0' (0x30) — digit-range lower boundary
    (":", false),     // ':' (0x3a) just above '9' (0x39) — digit-range upper boundary
    ("@", false),     // '@' (0x40) just below 'A' (0x41) — upper-range lower boundary
    ("[", false),     // '[' (0x5b) just above 'Z' (0x5a) — upper-range upper boundary
    ("`", false),     // '`' (0x60) just below 'a' (0x61) — lower-range lower boundary
    ("{", false),     // '{' (0x7b) just above 'z' (0x7a) — lower-range upper boundary
];

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def is_an(s: str) -> bool: return s.isalnum()`.
fn isalnum_module(name: &str) -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::IsAlnum,
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
/// (`$is_an(S_ADDR)`) onto the emitted module, before its closing `)`.
fn build_witness_wat(kernel_wat: &str, kernel: &str, s: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1195 witness: preload the s param (below LITERAL_BASE)\n");
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

/// Lower `is_an(s) = s.isalnum()`, run it in WABT with `s` preloaded, return the
/// bool. `None` when WABT is absent (caller skips the value assertion).
fn exec_case(s: &str) -> Option<bool> {
    let kernel_wat = emit_module(&isalnum_module("is_an")).expect("is_an program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, "is_an", s);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-isalnum-{}-{}",
        std::process::id(),
        s.len()
            .wrapping_mul(131)
            .wrapping_add(s.bytes().map(|b| b as usize).sum::<usize>())
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
        "wat2wasm failed for {s:?}.isalnum():\n{}\n---WAT---\n{wat}",
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
        "wasm-interp run failed for {s:?}.isalnum(): stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    Some(parse_run_i32(&stdout) != 0)
}

/// Assemble + run a witness expected to TRAP, returning whether the run trapped.
/// `None` when WABT is absent.
fn exec_expect_trap(s: &str) -> Option<bool> {
    let kernel_wat = emit_module(&isalnum_module("is_an")).expect("is_an program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, "is_an", s);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-isalnum-trap-{}-{}",
        std::process::id(),
        s.len()
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("trap.wat");
    let wasm_path = dir.join("trap.wasm");
    std::fs::write(&wat_path, &wat).expect("write wat");
    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed for the {s:?} trap witness:\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&assemble.stderr)
    );
    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    Some(!run.status.success() || stdout.contains("unreachable") || stderr.contains("unreachable"))
}

#[test]
fn cpython_isalnum_ground_truth_is_pinned() {
    // Every pin equals the ASCII `str.isalnum()` reference (non-empty AND all
    // bytes in 0x30..0x39 / 0x41..0x5a / 0x61..0x7a). These were verified vs
    // python3 when this slice landed.
    for &(s, want) in CASES {
        assert_eq!(py_isalnum_ascii(s), want, "pinned {s:?}.isalnum()");
        assert!(
            s.is_ascii(),
            "witness inputs are ASCII (non-ASCII may trap)"
        );
    }
    // The empty-string False must be present (the vacuous-all guard) …
    assert!(CASES.iter().any(|&(s, want)| s.is_empty() && !want));
    // … a True must be present (else the predicate could be a constant `false`) …
    assert!(CASES.iter().any(|&(_, want)| want));
    // … and ALL SIX boundaries just outside the three alphanumeric ranges must be
    // pinned False (the off-by-one guards on 0x30..0x39, 0x41..0x5a, 0x61..0x7a):
    //   0x2f('/') below '0', 0x3a(':') above '9', 0x40('@') below 'A',
    //   0x5b('[') above 'Z', 0x60('`') below 'a', 0x7b('{') above 'z'.
    assert!(CASES.iter().any(|&(s, want)| s == "/" && !want));
    assert!(CASES.iter().any(|&(s, want)| s == ":" && !want));
    assert!(CASES.iter().any(|&(s, want)| s == "@" && !want));
    assert!(CASES.iter().any(|&(s, want)| s == "[" && !want));
    assert!(CASES.iter().any(|&(s, want)| s == "`" && !want));
    assert!(CASES.iter().any(|&(s, want)| s == "{" && !want));
    // … and both ends of ALL THREE alphanumeric ranges must be pinned True (the
    // inclusive boundaries 0x30/0x39, 0x41/0x5a, 0x61/0x7a):
    assert!(CASES.iter().any(|&(s, want)| s == "0" && want)); // 0x30
    assert!(CASES.iter().any(|&(s, want)| s == "9" && want)); // 0x39
    assert!(CASES.iter().any(|&(s, want)| s == "A" && want)); // 0x41
    assert!(CASES.iter().any(|&(s, want)| s == "Z" && want)); // 0x5a
    assert!(CASES.iter().any(|&(s, want)| s == "a" && want)); // 0x61
    assert!(CASES.iter().any(|&(s, want)| s == "z" && want)); // 0x7a
}

#[test]
fn isalnum_emits_helper_call_memory_and_no_allocator() {
    // CONSTRUCT assertion (holds with or without WABT): the program lowers through
    // the production emitter, carrying the helper + call, declaring memory (the
    // scan reads the str bytes), and — because a bool predicate materialises
    // NOTHING — pulling in NO bump allocator.
    let wat = emit_module(&isalnum_module("is_an"))
        .expect("the s.isalnum() program must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_str_isalnum (param $s i32) (result i32)"),
        "the isalnum helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_isalnum"),
        "$is_an must call the isalnum helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $is_an (param $s i32) (result i32)"),
        "bool return → i32 result, str param → i32:\n{wat}"
    );
    assert!(
        wat.contains("(memory (export \"mem\")"),
        "the scan reads the str payload → memory must be declared:\n{wat}"
    );
    // A bool predicate allocates nothing — no bump allocator, no heap gate.
    assert!(
        !wat.contains("(func $__alloc"),
        "isalnum is non-allocating — it must NOT pull in the bump allocator:\n{wat}"
    );
    // The honest ASCII-only boundary: an undecidable non-ASCII byte traps.
    assert!(
        wat.contains("unreachable"),
        "the helper must trap (unreachable) on an undecidable non-ASCII byte:\n{wat}"
    );
}

#[test]
fn real_isalnum_program_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&isalnum_module("is_an")).expect("is_an program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1195: skipping EXECUTED isalnum witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module \
             (asserted in `isalnum_emits_helper_call_memory_and_no_allocator`); a \
             box with WABT also runs it and asserts the bool == CPython."
        );
        return;
    }
    eprintln!("PMAT-1195: running EXECUTED s.isalnum() witness via WABT");
    let mut ran = 0usize;
    for &(s, want) in CASES {
        let got = exec_case(s).expect("WABT present");
        assert_eq!(
            got, want,
            "executed WASM {s:?}.isalnum() = {got} but CPython = {want}"
        );
        ran += 1;
    }
    assert_eq!(ran, CASES.len());
    eprintln!(
        "PMAT-1195: all {ran} inputs executed in WABT and value-matched CPython \
         (incl. 'abc'/'ABC'/'123'/'a1b'/'Hello2'->True, ''->False (empty), \
         'a b'/'a!b'/'a_b'/'1.5'->False, and the '/'/':'/'@'/'['/'`'/'{{' boundaries \
         just outside the three alphanumeric ranges).\n\
         --- emitted is_an WAT (emit_module over meta-HIR) ---\n{wat}"
    );
}

#[test]
fn non_ascii_all_alnum_prefix_traps() {
    // Honest ASCII-only boundary: `.isalnum()` over a string whose ASCII prefix is
    // ALL alphanumeric, then a non-ASCII byte, is UNDECIDABLE (the trailing code
    // point might be a Unicode letter/digit — CPython "ab\u{00e9}".isalnum() is
    // True) — so it TRAPS (`unreachable`), NEVER a silent wrong bool. "ab\u{00e9}"
    // — é is U+00E9 = 0xC3 0xA9, the first byte >= 0x80, reached with the leading
    // "ab" (both alphanumeric).
    // The program must still lower (asserted structurally in the construct test).
    emit_module(&isalnum_module("is_an")).expect("is_an program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1195: skipping non-ASCII trap witness — WABT absent. The trap \
             (`unreachable`) is asserted structurally in \
             `isalnum_emits_helper_call_memory_and_no_allocator`."
        );
        return;
    }
    let s = "ab\u{00e9}";
    assert!(
        !s.is_ascii(),
        "the trap fixture must carry a non-ASCII byte"
    );
    let trapped = exec_expect_trap(s).expect("WABT present");
    assert!(
        trapped,
        "'{s}'.isalnum() must TRAP on the non-ASCII byte after an all-alphanumeric \
         prefix (honest ASCII-only boundary), not return a bool"
    );
    eprintln!(
        "PMAT-1195: 'ab\\u00e9'.isalnum() correctly TRAPPED on the non-ASCII é byte \
         (0xC3) after the all-alphanumeric 'ab' prefix — undecidable Unicode \
         letter/digit case, never a silent wrong bool."
    );
}

#[test]
fn non_alnum_ascii_before_non_ascii_returns_false_no_trap() {
    // The distinguishing correctness of a PREDICATE (vs the case-fold ops): a
    // definitively non-alphanumeric ASCII byte short-circuits to `0` (False) BEFORE
    // any later non-ASCII byte is examined, so it does NOT trap. "a!\u{00e9}" — '!'
    // (0x21 non-alphanumeric) forces False; the scan returns 0 and NEVER reaches
    // the non-ASCII é. CPython "a!\u{00e9}".isalnum() is also False, so this is an
    // exact match, not a divergence.
    // The program must still lower (asserted structurally in the construct test).
    emit_module(&isalnum_module("is_an")).expect("is_an program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1195: skipping short-circuit witness — WABT absent. The \
             short-circuit-before-trap ordering is structural (the `not-alnum` \
             return precedes the loop's next iteration)."
        );
        return;
    }
    let s = "a!\u{00e9}";
    assert!(
        !s.is_ascii(),
        "the fixture must carry a non-ASCII byte after 'a!'"
    );
    // CPython ground truth: False (the '!' makes it non-alphanumeric regardless of é).
    assert!(
        !py_isalnum_ascii("a!"),
        "the ASCII prefix 'a!' is non-alphanumeric"
    );
    let got = exec_case(s).expect("WABT present");
    assert!(
        !got,
        "'{s}'.isalnum() must return False via short-circuit on the non-alphanumeric \
         '!' BEFORE the non-ASCII é byte (matching CPython) — not trap, not True"
    );
    eprintln!(
        "PMAT-1195: 'a!\\u00e9'.isalnum() correctly returned False (short-circuit on \
         the non-alphanumeric '!' before the non-ASCII é) — a definitive answer \
         never traps, matching CPython."
    );
}
