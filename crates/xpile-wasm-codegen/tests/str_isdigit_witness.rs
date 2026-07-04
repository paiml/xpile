//! PMAT-1189 — EXECUTED `s.isdigit()` witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM`).
//!
//! `s.isdigit()` is the FIRST of the `str` `is*` PREDICATE family to reach the
//! WASM lane. Unlike the case-fold ops (`upper` / `lower` / `capitalize`), it
//! materialises NOTHING — it is a bool (i32 0/1) result from a single
//! left-to-right scan of the payload bytes, so it does NOT ride the `needs_heap`
//! gate and carries no bump allocator. `Expr::StrMethod { op: IsDigit }` in a
//! value position lowers via the non-allocating `$__wasm_str_isdigit` helper;
//! before this slice it fell through to the honest catch-all refusal.
//!
//! ## The real program
//!
//! ```python
//! def is_dig(s: str) -> bool:
//!     return s.isdigit()
//! ```
//!
//! ## Semantics — non-empty AND every code point an ASCII digit
//!
//! Python `str.isdigit()` is `True` iff the string is NON-EMPTY and every char
//! is a decimal digit. The empty string is `False` (a vacuous "all" is still
//! `False` here), so the helper returns `0` before the loop when `len == 0`.
//!
//! ## ASCII-only, with an honest boundary — but short-circuited on a definitive
//! ## answer first (the distinguishing correctness of a predicate)
//!
//! Python also accepts Unicode digit code points (`"²".isdigit()` is `True`),
//! which need a Unicode table this scalar lane does not carry. The scan is
//! therefore ordered so a DEFINITIVE answer never traps:
//!   * a NON-ASCII byte (`>= 0x80`) is examined only when every prior byte was an
//!     ASCII digit — the result is then genuinely undecidable, so it TRAPS
//!     (`unreachable`), exactly like the case-fold siblings, rather than
//!     returning a wrong bool (`non_ascii_all_digit_prefix_traps`);
//!   * a DEFINITIVELY non-digit ASCII byte (`< '0'` or `> '9'`) short-circuits to
//!     `0` BEFORE any later non-ASCII byte is examined, so `"1a²".isdigit()`
//!     returns `0` (Python's answer) and NEVER traps
//!     (`non_digit_ascii_before_non_ascii_returns_false_no_trap`).
//!
//! So a pure-ASCII `s` is answer-exact; a non-ASCII `s` whose ASCII prefix is all
//! digits aborts; a non-ASCII `s` with an earlier non-digit ASCII byte returns
//! `0`. It never passes an unmapped non-ASCII byte off as a wrong `True`/`False`.
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel `$is_dig`
//! takes an `i32` (the `s` param base-pointer, preloaded into a `(data …)` region
//! below `LITERAL_BASE`) and returns the i32 bool directly. The witness adds only
//! a zero-arg `run` export that pushes the constant `S_ADDR`, calls the kernel,
//! and returns its i32.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_str_isdigit` helper, declares memory, and
//! pulls in NO bump allocator) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and
/// the bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0,
/// UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;

/// Pure-ASCII Python `str.isdigit()` reference — `True` iff `s` is non-empty and
/// every byte is an ASCII decimal digit `'0'`–`'9'`. For ASCII inputs this is
/// exactly CPython's `str.isdigit()` (which additionally accepts Unicode digits,
/// out of this lane's scope — those TRAP). Used both to PIN the `CASES`
/// expectations and as the ground truth the witness value-matches.
fn py_isdigit_ascii(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// (input, CPython `input.isdigit()`) — pinned to the exact CPython ground truth.
/// ASCII-only inputs, since the WASM lane traps on a non-ASCII byte reached with
/// an all-digit prefix (see `non_ascii_all_digit_prefix_traps`).
const CASES: &[(&str, bool)] = &[
    ("123", true),   // the headline: all ASCII digits
    ("0", true),     // single digit
    ("00", true),    // leading zeros are digits
    ("007", true),   // more leading zeros
    ("9", true),     // boundary digit '9' (0x39)
    ("", false),     // empty -> False (Python's vacuous-all is still False)
    ("12a", false),  // trailing non-digit ASCII letter
    ("a12", false),  // leading non-digit ASCII letter
    ("abc", false),  // no digits at all
    (" 12", false),  // leading space (0x20, < '0')
    ("12 ", false),  // trailing space
    ("-12", false),  // '-' (0x2d, < '0') is NOT a digit in Python
    ("+5", false),   // '+' (0x2b) is NOT a digit
    ("1.5", false),  // '.' (0x2e, < '0') is NOT a digit
    ("1_2", false),  // '_' (0x5f, > '9') is NOT a digit
    ("12\t", false), // tab (0x09, < '0')
    (":", false),    // ':' (0x3a) is just past '9' (0x39) — the upper boundary
    ("/", false),    // '/' (0x2f) is just below '0' (0x30) — the lower boundary
];

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def is_dig(s: str) -> bool: return s.isdigit()`.
fn isdigit_module(name: &str) -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::IsDigit,
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
/// (`$is_dig(S_ADDR)`) onto the emitted module, before its closing `)`.
fn build_witness_wat(kernel_wat: &str, kernel: &str, s: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1189 witness: preload the s param (below LITERAL_BASE)\n");
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

/// Lower `is_dig(s) = s.isdigit()`, run it in WABT with `s` preloaded, return the
/// bool. `None` when WABT is absent (caller skips the value assertion).
fn exec_case(s: &str) -> Option<bool> {
    let kernel_wat = emit_module(&isdigit_module("is_dig")).expect("is_dig program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, "is_dig", s);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-isdigit-{}-{}",
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
        "wat2wasm failed for {s:?}.isdigit():\n{}\n---WAT---\n{wat}",
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
        "wasm-interp run failed for {s:?}.isdigit(): stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    Some(parse_run_i32(&stdout) != 0)
}

/// Assemble + run a witness expected to TRAP, returning whether the run trapped.
/// `None` when WABT is absent.
fn exec_expect_trap(s: &str) -> Option<bool> {
    let kernel_wat = emit_module(&isdigit_module("is_dig")).expect("is_dig program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, "is_dig", s);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-isdigit-trap-{}-{}",
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
fn cpython_isdigit_ground_truth_is_pinned() {
    // Every pin equals the ASCII `str.isdigit()` reference (non-empty AND all
    // bytes '0'..'9'). These were verified vs python3 when this slice landed.
    for &(s, want) in CASES {
        assert_eq!(py_isdigit_ascii(s), want, "pinned {s:?}.isdigit()");
        assert!(
            s.is_ascii(),
            "witness inputs are ASCII (non-ASCII may trap)"
        );
    }
    // The empty-string False must be present (the vacuous-all guard) …
    assert!(CASES.iter().any(|&(s, want)| s.is_empty() && !want));
    // … a True must be present (else the predicate could be a constant `false`) …
    assert!(CASES.iter().any(|&(_, want)| want));
    // … and BOTH boundary non-digits ('/' just below '0', ':' just above '9')
    // must be pinned False (the off-by-one guard on the 0x30..0x39 range).
    assert!(CASES.iter().any(|&(s, want)| s == "/" && !want));
    assert!(CASES.iter().any(|&(s, want)| s == ":" && !want));
}

#[test]
fn isdigit_emits_helper_call_memory_and_no_allocator() {
    // CONSTRUCT assertion (holds with or without WABT): the program lowers through
    // the production emitter, carrying the helper + call, declaring memory (the
    // scan reads the str bytes), and — because a bool predicate materialises
    // NOTHING — pulling in NO bump allocator.
    let wat = emit_module(&isdigit_module("is_dig"))
        .expect("the s.isdigit() program must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_str_isdigit (param $s i32) (result i32)"),
        "the isdigit helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_isdigit"),
        "$is_dig must call the isdigit helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $is_dig (param $s i32) (result i32)"),
        "bool return → i32 result, str param → i32:\n{wat}"
    );
    assert!(
        wat.contains("(memory (export \"mem\")"),
        "the scan reads the str payload → memory must be declared:\n{wat}"
    );
    // A bool predicate allocates nothing — no bump allocator, no heap gate.
    assert!(
        !wat.contains("(func $__alloc"),
        "isdigit is non-allocating — it must NOT pull in the bump allocator:\n{wat}"
    );
    // The honest ASCII-only boundary: an undecidable non-ASCII byte traps.
    assert!(
        wat.contains("unreachable"),
        "the helper must trap (unreachable) on an undecidable non-ASCII byte:\n{wat}"
    );
}

#[test]
fn real_isdigit_program_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&isdigit_module("is_dig")).expect("is_dig program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1189: skipping EXECUTED isdigit witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module \
             (asserted in `isdigit_emits_helper_call_memory_and_no_allocator`); a \
             box with WABT also runs it and asserts the bool == CPython."
        );
        return;
    }
    eprintln!("PMAT-1189: running EXECUTED s.isdigit() witness via WABT");
    let mut ran = 0usize;
    for &(s, want) in CASES {
        let got = exec_case(s).expect("WABT present");
        assert_eq!(
            got, want,
            "executed WASM {s:?}.isdigit() = {got} but CPython = {want}"
        );
        ran += 1;
    }
    assert_eq!(ran, CASES.len());
    eprintln!(
        "PMAT-1189: all {ran} inputs executed in WABT and value-matched CPython \
         (incl. '123'->True, ''->False (empty), '-12'/'1.5'/' 12'->False, and the \
         '/'/':' range boundaries).\n\
         --- emitted is_dig WAT (emit_module over meta-HIR) ---\n{wat}"
    );
}

#[test]
fn non_ascii_all_digit_prefix_traps() {
    // Honest ASCII-only boundary: `.isdigit()` over a string whose ASCII prefix is
    // ALL digits, then a non-ASCII byte, is UNDECIDABLE (the trailing code point
    // might be a Unicode digit — CPython "12²".isdigit() is True) — so it TRAPS
    // (`unreachable`), NEVER a silent wrong bool. "12²" — '²' is U+00B2 = 0xC2 0xA2,
    // the first byte >= 0x80, reached with "12" all digits.
    // The program must still lower (asserted structurally in the construct test).
    emit_module(&isdigit_module("is_dig")).expect("is_dig program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1189: skipping non-ASCII trap witness — WABT absent. The trap \
             (`unreachable`) is asserted structurally in \
             `isdigit_emits_helper_call_memory_and_no_allocator`."
        );
        return;
    }
    let s = "12²";
    assert!(
        !s.is_ascii(),
        "the trap fixture must carry a non-ASCII byte"
    );
    let trapped = exec_expect_trap(s).expect("WABT present");
    assert!(
        trapped,
        "'{s}'.isdigit() must TRAP on the non-ASCII byte after an all-digit prefix \
         (honest ASCII-only boundary), not return a bool"
    );
    eprintln!(
        "PMAT-1189: '{s}'.isdigit() correctly TRAPPED on the non-ASCII '²' byte \
         (0xC2) after the all-digit '12' prefix — undecidable Unicode-digit case, \
         never a silent wrong bool."
    );
}

#[test]
fn non_digit_ascii_before_non_ascii_returns_false_no_trap() {
    // The distinguishing correctness of a PREDICATE (vs the case-fold ops): a
    // definitively non-digit ASCII byte short-circuits to `0` (False) BEFORE any
    // later non-ASCII byte is examined, so it does NOT trap. "1a²" — '1' is a
    // digit, then 'a' (0x61 > '9') forces False; the scan returns 0 and NEVER
    // reaches the non-ASCII '²'. CPython "1a²".isdigit() is also False, so this is
    // an exact match, not a divergence.
    // The program must still lower (asserted structurally in the construct test).
    emit_module(&isdigit_module("is_dig")).expect("is_dig program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1189: skipping short-circuit witness — WABT absent. The \
             short-circuit-before-trap ordering is structural (the `< '0'` / `> \
             '9'` returns precede the loop's next iteration)."
        );
        return;
    }
    let s = "1a²";
    assert!(
        !s.is_ascii(),
        "the fixture must carry a non-ASCII byte after 'a'"
    );
    // CPython ground truth: False (the 'a' makes it non-numeric regardless of '²').
    assert!(
        !py_isdigit_ascii("1a"),
        "the ASCII prefix '1a' is non-digit"
    );
    let got = exec_case(s).expect("WABT present");
    assert!(
        !got,
        "'{s}'.isdigit() must return False via short-circuit on the non-digit 'a' \
         BEFORE the non-ASCII '²' byte (matching CPython) — not trap, not True"
    );
    eprintln!(
        "PMAT-1189: '{s}'.isdigit() correctly returned False (short-circuit on the \
         non-digit 'a' before the non-ASCII '²') — a definitive answer never traps, \
         matching CPython."
    );
}
