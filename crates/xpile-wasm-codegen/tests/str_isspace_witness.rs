//! PMAT-1193 — EXECUTED `s.isspace()` witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM`).
//!
//! `s.isspace()` is the THIRD of the `str` `is*` PREDICATE family to reach the
//! WASM lane (after PMAT-1189 `s.isdigit()` and PMAT-1191 `s.isalpha()`) and
//! their direct twin: a bool (i32 0/1) result from a single left-to-right scan of
//! the payload bytes, so it does NOT ride the `needs_heap` gate and carries no
//! bump allocator. It differs from `isdigit`/`isalpha` only in the per-byte
//! ASCII-membership test — the ASCII WHITESPACE set, which is two CONTIGUOUS
//! ranges: `0x09`–`0x0D` (`\t \n \v \f \r`) and `0x1C`–`0x20` (FS GS RS US and
//! the space `0x20`). Those four separators `0x1C`–`0x1F` ARE whitespace to
//! CPython's `str.isspace()` (verified vs python3). `Expr::StrMethod { op:
//! IsSpace }` in a value position lowers via the non-allocating
//! `$__wasm_str_isspace` helper; before this slice it fell through to the honest
//! catch-all refusal.
//!
//! ## The real program
//!
//! ```python
//! def is_sp(s: str) -> bool:
//!     return s.isspace()
//! ```
//!
//! ## Semantics — non-empty AND every code point ASCII whitespace
//!
//! Python `str.isspace()` is `True` iff the string is NON-EMPTY and every char
//! is whitespace. The empty string is `False` (a vacuous "all" is still `False`
//! here), so the helper returns `0` before the loop when `len == 0`.
//!
//! ## ASCII-only, with an honest boundary — but short-circuited on a definitive
//! ## answer first (the distinguishing correctness of a predicate)
//!
//! Python also accepts non-ASCII Unicode whitespace (`"\u{00a0}".isspace()` — a
//! NBSP — is `True`), which needs a Unicode table this scalar lane does not
//! carry. The scan is therefore ordered so a DEFINITIVE answer never traps:
//!   * a NON-ASCII byte (`>= 0x80`) is examined only when every prior byte was
//!     ASCII whitespace — the result is then genuinely undecidable, so it TRAPS
//!     (`unreachable`), exactly like `isdigit` / `isalpha`, rather than returning
//!     a wrong bool (`non_ascii_all_space_prefix_traps`);
//!   * a DEFINITIVELY non-whitespace ASCII byte (any byte `< 0x80` outside the
//!     two whitespace ranges) short-circuits to `0` BEFORE any later non-ASCII
//!     byte is examined, so `"a\u{00a0}".isspace()` returns `0` (Python's answer)
//!     and NEVER traps (`non_space_ascii_before_non_ascii_returns_false_no_trap`).
//!
//! So a pure-ASCII `s` is answer-exact; a non-ASCII `s` whose ASCII prefix is all
//! whitespace aborts; a non-ASCII `s` with an earlier non-whitespace ASCII byte
//! returns `0`. It never passes an unmapped non-ASCII byte off as a wrong bool.
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel `$is_sp`
//! takes an `i32` (the `s` param base-pointer, preloaded into a `(data …)` region
//! below `LITERAL_BASE`) and returns the i32 bool directly. The witness adds only
//! a zero-arg `run` export that pushes the constant `S_ADDR`, calls the kernel,
//! and returns its i32.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_str_isspace` helper, declares memory, and
//! pulls in NO bump allocator) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and
/// the bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0,
/// UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;

/// Pure-ASCII Python `str.isspace()` reference — `True` iff `s` is non-empty and
/// every byte is ASCII whitespace (the two contiguous ranges `0x09`–`0x0D` and
/// `0x1C`–`0x20`). For ASCII inputs this is exactly CPython's `str.isspace()`
/// (which additionally accepts Unicode whitespace, out of this lane's scope —
/// those TRAP). Used both to PIN the `CASES` expectations and as the ground truth
/// the witness value-matches.
fn py_isspace_ascii(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| (0x09..=0x0d).contains(&b) || (0x1c..=0x20).contains(&b))
}

/// (input, CPython `input.isspace()`) — pinned to the exact CPython ground truth.
/// ASCII-only inputs, since the WASM lane traps on a non-ASCII byte reached with
/// an all-whitespace prefix (see `non_ascii_all_space_prefix_traps`).
const CASES: &[(&str, bool)] = &[
    (" ", true),       // the headline: a single space (0x20)
    ("  ", true),      // two spaces
    ("\t", true),      // tab (0x09) — lower boundary of range 1
    ("\r", true),      // CR (0x0d) — upper boundary of range 1
    ("\n", true),      // LF (0x0a)
    ("\u{0b}", true),  // VT (0x0b)
    ("\u{0c}", true),  // FF (0x0c)
    ("\u{1c}", true),  // FS (0x1c) — lower boundary of range 2
    ("\u{1f}", true),  // US (0x1f) — interior of range 2
    (" \t\n\r", true), // a mix of whitespace
    ("", false),       // empty -> False (Python's vacuous-all is still False)
    ("a", false),      // a letter — no whitespace at all
    ("1", false),      // a digit
    ("a ", false),     // leading non-whitespace
    (" a", false),     // trailing non-whitespace
    (" a ", false),    // interior non-whitespace (scan advances a space, then False)
    ("\u{08}", false), // backspace (0x08) just below tab (0x09) — range-1 lower boundary
    ("\u{0e}", false), // SO (0x0e) just above CR (0x0d) — the inter-range gap
    ("\u{1b}", false), // ESC (0x1b) just below FS (0x1c) — the inter-range gap upper end
    ("!", false),      // '!' (0x21) just above space (0x20) — range-2 upper boundary
];

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def is_sp(s: str) -> bool: return s.isspace()`.
fn isspace_module(name: &str) -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::IsSpace,
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
/// (`$is_sp(S_ADDR)`) onto the emitted module, before its closing `)`.
fn build_witness_wat(kernel_wat: &str, kernel: &str, s: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1193 witness: preload the s param (below LITERAL_BASE)\n");
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

/// Lower `is_sp(s) = s.isspace()`, run it in WABT with `s` preloaded, return the
/// bool. `None` when WABT is absent (caller skips the value assertion).
fn exec_case(s: &str) -> Option<bool> {
    let kernel_wat = emit_module(&isspace_module("is_sp")).expect("is_sp program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, "is_sp", s);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-isspace-{}-{}",
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
        "wat2wasm failed for {s:?}.isspace():\n{}\n---WAT---\n{wat}",
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
        "wasm-interp run failed for {s:?}.isspace(): stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    Some(parse_run_i32(&stdout) != 0)
}

/// Assemble + run a witness expected to TRAP, returning whether the run trapped.
/// `None` when WABT is absent.
fn exec_expect_trap(s: &str) -> Option<bool> {
    let kernel_wat = emit_module(&isspace_module("is_sp")).expect("is_sp program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, "is_sp", s);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-isspace-trap-{}-{}",
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
fn cpython_isspace_ground_truth_is_pinned() {
    // Every pin equals the ASCII `str.isspace()` reference (non-empty AND all
    // bytes in 0x09..0x0d / 0x1c..0x20). These were verified vs python3 when this
    // slice landed.
    for &(s, want) in CASES {
        assert_eq!(py_isspace_ascii(s), want, "pinned {s:?}.isspace()");
        assert!(
            s.is_ascii(),
            "witness inputs are ASCII (non-ASCII may trap)"
        );
    }
    // The empty-string False must be present (the vacuous-all guard) …
    assert!(CASES.iter().any(|&(s, want)| s.is_empty() && !want));
    // … a True must be present (else the predicate could be a constant `false`) …
    assert!(CASES.iter().any(|&(_, want)| want));
    // … and ALL FOUR boundaries just outside the two whitespace ranges must be
    // pinned False (the off-by-one guards on 0x09..0x0d and 0x1c..0x20):
    //   0x08 below tab, 0x0e above CR, 0x1b below FS, 0x21('!') above space.
    assert!(CASES.iter().any(|&(s, want)| s == "\u{08}" && !want));
    assert!(CASES.iter().any(|&(s, want)| s == "\u{0e}" && !want));
    assert!(CASES.iter().any(|&(s, want)| s == "\u{1b}" && !want));
    assert!(CASES.iter().any(|&(s, want)| s == "!" && !want));
    // … and both ends of BOTH whitespace ranges must be pinned True (the
    // inclusive boundaries 0x09/0x0d and 0x1c/0x20):
    assert!(CASES.iter().any(|&(s, want)| s == "\t" && want)); // 0x09
    assert!(CASES.iter().any(|&(s, want)| s == "\r" && want)); // 0x0d
    assert!(CASES.iter().any(|&(s, want)| s == "\u{1c}" && want)); // 0x1c
    assert!(CASES.iter().any(|&(s, want)| s == " " && want)); // 0x20
}

#[test]
fn isspace_emits_helper_call_memory_and_no_allocator() {
    // CONSTRUCT assertion (holds with or without WABT): the program lowers through
    // the production emitter, carrying the helper + call, declaring memory (the
    // scan reads the str bytes), and — because a bool predicate materialises
    // NOTHING — pulling in NO bump allocator.
    let wat = emit_module(&isspace_module("is_sp"))
        .expect("the s.isspace() program must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_str_isspace (param $s i32) (result i32)"),
        "the isspace helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_isspace"),
        "$is_sp must call the isspace helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $is_sp (param $s i32) (result i32)"),
        "bool return → i32 result, str param → i32:\n{wat}"
    );
    assert!(
        wat.contains("(memory (export \"mem\")"),
        "the scan reads the str payload → memory must be declared:\n{wat}"
    );
    // A bool predicate allocates nothing — no bump allocator, no heap gate.
    assert!(
        !wat.contains("(func $__alloc"),
        "isspace is non-allocating — it must NOT pull in the bump allocator:\n{wat}"
    );
    // The honest ASCII-only boundary: an undecidable non-ASCII byte traps.
    assert!(
        wat.contains("unreachable"),
        "the helper must trap (unreachable) on an undecidable non-ASCII byte:\n{wat}"
    );
}

#[test]
fn real_isspace_program_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&isspace_module("is_sp")).expect("is_sp program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1193: skipping EXECUTED isspace witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module \
             (asserted in `isspace_emits_helper_call_memory_and_no_allocator`); a \
             box with WABT also runs it and asserts the bool == CPython."
        );
        return;
    }
    eprintln!("PMAT-1193: running EXECUTED s.isspace() witness via WABT");
    let mut ran = 0usize;
    for &(s, want) in CASES {
        let got = exec_case(s).expect("WABT present");
        assert_eq!(
            got, want,
            "executed WASM {s:?}.isspace() = {got} but CPython = {want}"
        );
        ran += 1;
    }
    assert_eq!(ran, CASES.len());
    eprintln!(
        "PMAT-1193: all {ran} inputs executed in WABT and value-matched CPython \
         (incl. ' '/'\\t'/'\\r'/'\\x1c'/'\\x1f'/mix->True, ''->False (empty), \
         'a'/'1'/'a '/' a'->False, and the 0x08/0x0e/0x1b/'!' boundaries just \
         outside the two whitespace ranges).\n\
         --- emitted is_sp WAT (emit_module over meta-HIR) ---\n{wat}"
    );
}

#[test]
fn non_ascii_all_space_prefix_traps() {
    // Honest ASCII-only boundary: `.isspace()` over a string whose ASCII prefix is
    // ALL whitespace, then a non-ASCII byte, is UNDECIDABLE (the trailing code
    // point might be Unicode whitespace — CPython " \u{00a0}".isspace() is True) —
    // so it TRAPS (`unreachable`), NEVER a silent wrong bool. " \u{00a0}" — the
    // NBSP is U+00A0 = 0xC2 0xA0, the first byte >= 0x80, reached with the leading
    // space (whitespace).
    // The program must still lower (asserted structurally in the construct test).
    emit_module(&isspace_module("is_sp")).expect("is_sp program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1193: skipping non-ASCII trap witness — WABT absent. The trap \
             (`unreachable`) is asserted structurally in \
             `isspace_emits_helper_call_memory_and_no_allocator`."
        );
        return;
    }
    let s = " \u{00a0}";
    assert!(
        !s.is_ascii(),
        "the trap fixture must carry a non-ASCII byte"
    );
    let trapped = exec_expect_trap(s).expect("WABT present");
    assert!(
        trapped,
        "'{s}'.isspace() must TRAP on the non-ASCII byte after an all-whitespace \
         prefix (honest ASCII-only boundary), not return a bool"
    );
    eprintln!(
        "PMAT-1193: ' \\u00a0'.isspace() correctly TRAPPED on the non-ASCII NBSP \
         byte (0xC2) after the all-whitespace ' ' prefix — undecidable \
         Unicode-whitespace case, never a silent wrong bool."
    );
}

#[test]
fn non_space_ascii_before_non_ascii_returns_false_no_trap() {
    // The distinguishing correctness of a PREDICATE (vs the case-fold ops): a
    // definitively non-whitespace ASCII byte short-circuits to `0` (False) BEFORE
    // any later non-ASCII byte is examined, so it does NOT trap. "a\u{00a0}" — 'a'
    // (0x61 non-whitespace) forces False; the scan returns 0 and NEVER reaches the
    // non-ASCII NBSP. CPython "a\u{00a0}".isspace() is also False, so this is an
    // exact match, not a divergence.
    // The program must still lower (asserted structurally in the construct test).
    emit_module(&isspace_module("is_sp")).expect("is_sp program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1193: skipping short-circuit witness — WABT absent. The \
             short-circuit-before-trap ordering is structural (the `not-ws` return \
             precedes the loop's next iteration)."
        );
        return;
    }
    let s = "a\u{00a0}";
    assert!(
        !s.is_ascii(),
        "the fixture must carry a non-ASCII byte after 'a'"
    );
    // CPython ground truth: False (the 'a' makes it non-whitespace regardless of NBSP).
    assert!(
        !py_isspace_ascii("a"),
        "the ASCII prefix 'a' is non-whitespace"
    );
    let got = exec_case(s).expect("WABT present");
    assert!(
        !got,
        "'{s}'.isspace() must return False via short-circuit on the non-whitespace \
         'a' BEFORE the non-ASCII NBSP byte (matching CPython) — not trap, not True"
    );
    eprintln!(
        "PMAT-1193: 'a\\u00a0'.isspace() correctly returned False (short-circuit on \
         the non-whitespace 'a' before the non-ASCII NBSP) — a definitive answer \
         never traps, matching CPython."
    );
}
