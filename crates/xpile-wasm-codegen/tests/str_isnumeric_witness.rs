//! PMAT-1211 — EXECUTED `s.isnumeric()` witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM`).
//!
//! `s.isnumeric()` is the EIGHTH of the `str` `is*` PREDICATE family to reach the
//! WASM lane, and the first that SHARES an existing helper outright: it lowers via
//! the very same non-allocating `$__wasm_str_isdigit(s) -> i32` byte scan that
//! `s.isdigit()` uses (PMAT-1189), the way `isupper`/`islower` share
//! `$__wasm_str_isupper_islower`. Before this slice `.isnumeric()` fell through to
//! the honest catch-all refusal even though the Rust/Ruchy lanes already emit it
//! (`char::is_numeric()`) — a Rust-vs-WASM lane asymmetry this slice closes.
//!
//! ## The real program
//!
//! ```python
//! def is_num(s: str) -> bool:
//!     return s.isnumeric()
//! ```
//!
//! ## Why the isdigit scan is byte-EXACT for isnumeric (not an approximation)
//!
//! CPython `str.isnumeric()` is a strict SUPERSET of `str.isdigit()`: it is `True`
//! for the whole Unicode numeric span (categories Nd + Nl + No — decimal digits,
//! letter-numerals like Roman `Ⅴ`, and other-numerals like the fraction `½` and
//! superscript `²`), whereas `isdigit` covers only Nd + a few digit-like code
//! points. They DIFFER — but only OUTSIDE the ASCII range:
//!
//!   * On an ASCII byte (`< 0x80`) the ONLY numeric characters are `'0'`–`'9'`
//!     (`0x30`–`0x39`, all category Nd). So over an all-ASCII string `isnumeric`
//!     and `isdigit` compute the IDENTICAL function: `True` iff non-empty and every
//!     byte is `0x30`–`0x39`. The `$__wasm_str_isdigit` scan answers exactly that.
//!   * On a non-ASCII byte the answer is genuinely UNDECIDABLE in this scalar lane
//!     (no Unicode Nd/Nl/No table): `"½".isnumeric()` is `True`, `"½".isdigit()` is
//!     `False`, and this lane can decide neither. The isdigit scan already TRAPS
//!     (`unreachable`) on a non-ASCII byte reached with an all-digit prefix — the
//!     honest ASCII-only boundary — so it refuses rather than answers, correct for
//!     `isnumeric` too.
//!   * A DEFINITIVELY non-digit ASCII byte short-circuits to `0` BEFORE any later
//!     non-ASCII byte is examined, so `"a½".isnumeric()` returns `0` (matching
//!     CPython — the `'a'` makes it non-numeric regardless of the `½`) and NEVER
//!     traps.
//!
//! So the isdigit helper is byte-exact for `isnumeric` on EXACTLY the inputs it is
//! byte-exact for `isdigit` (all-ASCII + the short-circuit-False cases), and traps
//! on exactly the same inputs. Sharing the helper is not a shortcut — it is the
//! honest consequence of the two predicates coinciding on this lane's decidable
//! domain. No duplicate scan is emitted (a `--duplicates` audit stays clean).
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel `$is_num`
//! takes an `i32` (the `s` param base-pointer, preloaded into a `(data …)` region
//! below `LITERAL_BASE`) and returns the i32 bool directly. The witness adds only a
//! zero-arg `run` export that pushes the constant `S_ADDR`, calls the kernel, and
//! returns its i32.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the shared `$__wasm_str_isdigit` helper, declares memory,
//! and pulls in NO bump allocator) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and the
/// bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0, UTF-8
/// bytes @ base+8).
const S_ADDR: i32 = 16;

/// Pure-ASCII Python `str.isnumeric()` reference — `True` iff `s` is non-empty and
/// every byte is an ASCII decimal digit `'0'`–`'9'`. For ASCII inputs this is
/// exactly CPython's `str.isnumeric()` (which additionally accepts non-ASCII
/// Unicode numerics — `½`, `Ⅴ`, `²` — out of this lane's scope; those TRAP). Used
/// both to PIN the `CASES` expectations and as the ground truth the witness
/// value-matches. Identical to the isdigit reference on ASCII, by construction.
fn py_isnumeric_ascii(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// (input, CPython `input.isnumeric()`) — pinned to the exact CPython ground truth.
/// ASCII-only inputs, since the WASM lane traps on a non-ASCII byte reached with an
/// all-digit prefix (see `non_ascii_all_digit_prefix_traps`).
const CASES: &[(&str, bool)] = &[
    ("123", true),   // the headline: all ASCII digits (Nd)
    ("0", true),     // single digit
    ("00", true),    // leading zeros are numeric
    ("42", true),    // ordinary number
    ("9", true),     // boundary digit '9' (0x39)
    ("", false),     // empty -> False (Python's vacuous-all is still False)
    ("12a", false),  // trailing non-numeric ASCII letter
    ("a12", false),  // leading non-numeric ASCII letter
    ("abc", false),  // no numerics at all
    (" 12", false),  // leading space (0x20, < '0')
    ("12 ", false),  // trailing space
    ("-12", false),  // '-' (0x2d, < '0') is NOT numeric in Python (a sign)
    ("+5", false),   // '+' (0x2b) is NOT numeric
    ("1.5", false),  // '.' (0x2e, < '0') is NOT numeric (isnumeric, not isfloat)
    ("3/4", false),  // ASCII '/' is NOT numeric (unlike a Unicode fraction char)
    ("1_2", false),  // '_' (0x5f, > '9') is NOT numeric
    ("12\t", false), // tab (0x09, < '0')
    (":", false),    // ':' (0x3a) is just past '9' (0x39) — the upper boundary
    ("/", false),    // '/' (0x2f) is just below '0' (0x30) — the lower boundary
];

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def is_num(s: str) -> bool: return s.isnumeric()`.
fn isnumeric_module(name: &str) -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::IsNumeric,
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
/// (`$is_num(S_ADDR)`) onto the emitted module, before its closing `)`.
fn build_witness_wat(kernel_wat: &str, kernel: &str, s: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1211 witness: preload the s param (below LITERAL_BASE)\n");
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

/// Lower `is_num(s) = s.isnumeric()`, run it in WABT with `s` preloaded, return the
/// bool. `None` when WABT is absent (caller skips the value assertion).
fn exec_case(s: &str) -> Option<bool> {
    let kernel_wat = emit_module(&isnumeric_module("is_num")).expect("is_num program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, "is_num", s);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-isnumeric-{}-{}",
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
        "wat2wasm failed for {s:?}.isnumeric():\n{}\n---WAT---\n{wat}",
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
        "wasm-interp run failed for {s:?}.isnumeric(): stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    Some(parse_run_i32(&stdout) != 0)
}

/// Assemble + run a witness expected to TRAP, returning whether the run trapped.
/// `None` when WABT is absent.
fn exec_expect_trap(s: &str) -> Option<bool> {
    let kernel_wat = emit_module(&isnumeric_module("is_num")).expect("is_num program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, "is_num", s);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-isnumeric-trap-{}-{}",
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
fn cpython_isnumeric_ground_truth_is_pinned() {
    // Every pin equals the ASCII `str.isnumeric()` reference (non-empty AND all
    // bytes '0'..'9'). On ASCII this coincides with str.isdigit(); both were
    // verified vs python3 when this slice landed.
    for &(s, want) in CASES {
        assert_eq!(py_isnumeric_ascii(s), want, "pinned {s:?}.isnumeric()");
        assert!(
            s.is_ascii(),
            "witness inputs are ASCII (non-ASCII may trap)"
        );
    }
    // The empty-string False must be present (the vacuous-all guard) …
    assert!(CASES.iter().any(|&(s, want)| s.is_empty() && !want));
    // … a True must be present (else the predicate could be a constant `false`) …
    assert!(CASES.iter().any(|&(_, want)| want));
    // … and BOTH boundary non-digits ('/' just below '0', ':' just above '9') must
    // be pinned False (the off-by-one guard on the 0x30..0x39 range) …
    assert!(CASES.iter().any(|&(s, want)| s == "/" && !want));
    assert!(CASES.iter().any(|&(s, want)| s == ":" && !want));
    // … and the ASCII fraction slash '3/4' must be False — the isnumeric-specific
    // pin (a Unicode fraction like '¾' is True in Python, but the ASCII '/' is not,
    // and this lane never sees the Unicode form without trapping).
    assert!(CASES.iter().any(|&(s, want)| s == "3/4" && !want));
}

#[test]
fn isnumeric_shares_isdigit_helper_call_memory_and_no_allocator() {
    // CONSTRUCT assertion (holds with or without WABT): the program lowers through
    // the production emitter, REUSING the isdigit helper (no separate isnumeric
    // scan), declaring memory, and — because a bool predicate materialises NOTHING
    // — pulling in NO bump allocator.
    let wat = emit_module(&isnumeric_module("is_num"))
        .expect("the s.isnumeric() program must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_str_isdigit (param $s i32) (result i32)"),
        "isnumeric must reuse the shared isdigit helper:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_isdigit"),
        "$is_num must call the shared isdigit helper:\n{wat}"
    );
    // No dedicated isnumeric helper is emitted — the reuse is the whole point (a
    // byte-for-byte duplicate scan would trip a --duplicates audit).
    assert!(
        !wat.contains("$__wasm_str_isnumeric"),
        "isnumeric must NOT emit a duplicate helper — it shares isdigit's scan:\n{wat}"
    );
    assert!(
        wat.contains("(func $is_num (param $s i32) (result i32)"),
        "bool return → i32 result, str param → i32:\n{wat}"
    );
    assert!(
        wat.contains("(memory (export \"mem\")"),
        "the scan reads the str payload → memory must be declared:\n{wat}"
    );
    // A bool predicate allocates nothing — no bump allocator, no heap gate.
    assert!(
        !wat.contains("(func $__alloc"),
        "isnumeric is non-allocating — it must NOT pull in the bump allocator:\n{wat}"
    );
    // The honest ASCII-only boundary: an undecidable non-ASCII byte traps.
    assert!(
        wat.contains("unreachable"),
        "the helper must trap (unreachable) on an undecidable non-ASCII byte:\n{wat}"
    );
}

#[test]
fn real_isnumeric_program_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&isnumeric_module("is_num")).expect("is_num program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1211: skipping EXECUTED isnumeric witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module (asserted \
             in `isnumeric_shares_isdigit_helper_call_memory_and_no_allocator`); a \
             box with WABT also runs it and asserts the bool == CPython."
        );
        return;
    }
    eprintln!("PMAT-1211: running EXECUTED s.isnumeric() witness via WABT");
    let mut ran = 0usize;
    for &(s, want) in CASES {
        let got = exec_case(s).expect("WABT present");
        assert_eq!(
            got, want,
            "executed WASM {s:?}.isnumeric() = {got} but CPython = {want}"
        );
        ran += 1;
    }
    assert_eq!(ran, CASES.len());
    eprintln!(
        "PMAT-1211: all {ran} inputs executed in WABT and value-matched CPython \
         (incl. '123'->True, ''->False (empty), '-12'/'1.5'/'3/4'/' 12'->False, and \
         the '/'/':' range boundaries) — isnumeric reusing the isdigit scan is \
         byte-exact on the ASCII-decidable domain.\n\
         --- emitted is_num WAT (emit_module over meta-HIR) ---\n{wat}"
    );
}

#[test]
fn non_ascii_all_digit_prefix_traps() {
    // Honest ASCII-only boundary: `.isnumeric()` over a string whose ASCII prefix is
    // ALL digits, then a non-ASCII byte, is UNDECIDABLE (the trailing code point
    // might be a Unicode numeric — CPython "12½".isnumeric() is True) — so it TRAPS
    // (`unreachable`), NEVER a silent wrong bool. "12½" — '½' is U+00BD = 0xC2 0xBD,
    // the first byte >= 0x80, reached with "12" all digits.
    emit_module(&isnumeric_module("is_num")).expect("is_num program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1211: skipping non-ASCII trap witness — WABT absent. The trap \
             (`unreachable`) is asserted structurally in \
             `isnumeric_shares_isdigit_helper_call_memory_and_no_allocator`."
        );
        return;
    }
    let s = "12½";
    assert!(
        !s.is_ascii(),
        "the trap fixture must carry a non-ASCII byte"
    );
    let trapped = exec_expect_trap(s).expect("WABT present");
    assert!(
        trapped,
        "'{s}'.isnumeric() must TRAP on the non-ASCII byte after an all-digit prefix \
         (honest ASCII-only boundary — '½' is a Unicode numeric this lane can't \
         decide), not return a bool"
    );
    eprintln!(
        "PMAT-1211: '{s}'.isnumeric() correctly TRAPPED on the non-ASCII '½' byte \
         (0xC2) after the all-digit '12' prefix — undecidable Unicode-numeric case, \
         never a silent wrong bool."
    );
}

#[test]
fn non_numeric_ascii_before_non_ascii_returns_false_no_trap() {
    // The distinguishing correctness of a PREDICATE: a definitively non-numeric
    // ASCII byte short-circuits to `0` (False) BEFORE any later non-ASCII byte is
    // examined, so it does NOT trap. "1a½" — '1' is a digit, then 'a' (0x61 > '9')
    // forces False; the scan returns 0 and NEVER reaches the non-ASCII '½'. CPython
    // "1a½".isnumeric() is also False, so this is an exact match, not a divergence.
    emit_module(&isnumeric_module("is_num")).expect("is_num program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1211: skipping short-circuit witness — WABT absent. The \
             short-circuit-before-trap ordering is structural (the shared isdigit \
             scan's `< '0'` / `> '9'` returns precede the loop's next iteration)."
        );
        return;
    }
    let s = "1a½";
    assert!(
        !s.is_ascii(),
        "the fixture must carry a non-ASCII byte after 'a'"
    );
    // CPython ground truth: False (the 'a' makes it non-numeric regardless of '½').
    assert!(
        !py_isnumeric_ascii("1a"),
        "the ASCII prefix '1a' is non-numeric"
    );
    let got = exec_case(s).expect("WABT present");
    assert!(
        !got,
        "'{s}'.isnumeric() must return False via short-circuit on the non-numeric \
         'a' BEFORE the non-ASCII '½' byte (matching CPython) — not trap, not True"
    );
    eprintln!(
        "PMAT-1211: '{s}'.isnumeric() correctly returned False (short-circuit on the \
         non-numeric 'a' before the non-ASCII '½') — a definitive answer never traps, \
         matching CPython."
    );
}
