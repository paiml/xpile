//! PMAT-1197 — EXECUTED `s.isupper()` / `s.islower()` witness for the native
//! WASM EMIT lane (`C-COMPILE-RUST-TO-WASM`).
//!
//! `s.isupper()` / `s.islower()` are the FIFTH/SIXTH of the `str` `is*` PREDICATE
//! family to reach the WASM lane (after PMAT-1189 `s.isdigit()`, PMAT-1191
//! `s.isalpha()`, PMAT-1193 `s.isspace()` and PMAT-1195 `s.isalnum()`) and the
//! first PAIR whose truth needs STATE across the scan rather than an
//! "every-char-matches" fold: Python's rule is "at least one CASED char AND no
//! cased char of the OPPOSITE case". Both are a bool (i32 0/1) result from a
//! single left-to-right scan of the payload bytes, so they do NOT ride the
//! `needs_heap` gate and carry no bump allocator. One shared helper
//! `$__wasm_str_isupper_islower(s, want_upper)` serves both directions (a
//! `want_upper` i32 flag picks the wanted/disqualifier ASCII letter ranges),
//! exactly like the `$__wasm_str_upper_lower` case-fold pair. `Expr::StrMethod {
//! op: IsUpper | IsLower }` in a value position lowers via this helper; before
//! this slice both fell through to the honest catch-all refusal.
//!
//! ## The real programs
//!
//! ```python
//! def is_up(s: str) -> bool:
//!     return s.isupper()
//!
//! def is_lo(s: str) -> bool:
//!     return s.islower()
//! ```
//!
//! ## Semantics — at least one cased char AND no opposite-case char
//!
//! Python `str.isupper()` is `True` iff `s` has at least one cased character and
//! every cased character is uppercase; `str.islower()` is the lowercase mirror.
//! For ASCII the cased characters are exactly the letters, so:
//!   * `isupper(s) == has_upper_letter(s) && !has_lower_letter(s)`
//!   * `islower(s) == has_lower_letter(s) && !has_upper_letter(s)`
//!
//! Neither needs an empty guard (unlike the isdigit family): the helper falls
//! through as `$has_cased`, which starts `0`, so `""` / `"123"` (no cased char)
//! return `0` — Python `"".isupper()` and `"123".isupper()` are both `False`.
//!
//! ## ASCII-only, with an honest boundary — but short-circuited on a definitive
//! ## DISQUALIFIER first
//!
//! Python also decides over non-ASCII cased Unicode (`"\u{00c1}".isupper()` is
//! `True`, `"\u{00c1}b".isupper()` is `False`), which needs a case table this
//! scalar lane does not carry. The scan is therefore ordered so a DEFINITIVE `0`
//! never traps:
//!   * a NON-ASCII byte (`>= 0x80`) is reached only when no opposite-case ASCII
//!     letter has appeared yet — the trailing code point might be a same-case,
//!     opposite-case, or uncased Unicode char (all three change the answer), so it
//!     TRAPS (`unreachable`), exactly like `isdigit`/`isalpha`/`isspace`/`isalnum`
//!     (`non_ascii_no_disqualifier_prefix_traps`);
//!   * an OPPOSITE-CASE ASCII letter (a lowercase letter for `isupper`, an
//!     uppercase letter for `islower`) short-circuits to `0` BEFORE any later
//!     non-ASCII byte is examined, so `"a\u{00c1}".isupper()` returns `0`
//!     (Python's answer) and NEVER traps
//!     (`opposite_case_ascii_before_non_ascii_returns_false_no_trap`).
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel
//! (`$is_case`) takes an `i32` (the `s` param base-pointer, preloaded into a
//! `(data …)` region below `LITERAL_BASE`) and returns the i32 bool directly. The
//! witness adds only a zero-arg `run` export that pushes the constant `S_ADDR`,
//! calls the kernel, and returns its i32.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_str_isupper_islower` helper, declares
//! memory, and pulls in NO bump allocator) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and
/// the bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0,
/// UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;

/// Pure-ASCII Python `str.isupper()` reference — `True` iff `s` has at least one
/// ASCII uppercase letter (`0x41`–`0x5A`) and no ASCII lowercase letter
/// (`0x61`–`0x7A`). For ASCII inputs this is exactly CPython's `str.isupper()`
/// (which additionally decides over Unicode-cased chars, out of this lane's scope
/// — those TRAP). Used both to PIN the `UPPER_CASES` expectations and as the
/// ground truth the witness value-matches.
fn py_isupper_ascii(s: &str) -> bool {
    let has_upper = s.bytes().any(|b| (0x41..=0x5a).contains(&b));
    let has_lower = s.bytes().any(|b| (0x61..=0x7a).contains(&b));
    has_upper && !has_lower
}

/// Pure-ASCII Python `str.islower()` reference — the lowercase mirror of
/// [`py_isupper_ascii`].
fn py_islower_ascii(s: &str) -> bool {
    let has_upper = s.bytes().any(|b| (0x41..=0x5a).contains(&b));
    let has_lower = s.bytes().any(|b| (0x61..=0x7a).contains(&b));
    has_lower && !has_upper
}

/// (input, CPython `input.isupper()`) — pinned to the exact CPython ground truth.
/// ASCII-only inputs, since the WASM lane traps on a non-ASCII byte reached with
/// no opposite-case letter yet (see `non_ascii_no_disqualifier_prefix_traps`).
const UPPER_CASES: &[(&str, bool)] = &[
    ("ABC", true),    // all uppercase
    ("A1", true),     // uppercase + digit (digit uncased)
    ("A B", true),    // uppercase letters + space (space uncased)
    ("HELLO2", true), // uppercase + digit
    ("A", true),      // 'A' (0x41) — lower boundary of the uppercase range
    ("Z", true),      // 'Z' (0x5a) — upper boundary of the uppercase range
    ("Abc", false),   // has lowercase 'bc'
    ("aB", false),    // leading lowercase 'a' (disqualifier before 'B')
    ("abc", false),   // all lowercase
    ("a", false),     // 'a' (0x61) — a lowercase letter, disqualifier
    ("z", false),     // 'z' (0x7a) — a lowercase letter, disqualifier
    ("", false),      // empty -> False (no cased char; no empty guard needed)
    ("123", false),   // digits only -> False (no cased char)
    (" ", false),     // a lone space -> False (no cased char)
    ("!", false),     // punctuation only -> False (no cased char)
    ("@", false),     // '@' (0x40) just below 'A' — uncased -> False (no cased char)
    ("[", false),     // '[' (0x5b) just above 'Z' — uncased -> False
    ("`", false),     // '`' (0x60) just below 'a' — uncased -> False
    ("{", false),     // '{' (0x7b) just above 'z' — uncased -> False
];

/// (input, CPython `input.islower()`) — the lowercase mirror of `UPPER_CASES`.
const LOWER_CASES: &[(&str, bool)] = &[
    ("abc", true),    // all lowercase
    ("a1", true),     // lowercase + digit
    ("a b", true),    // lowercase letters + space
    ("hello2", true), // lowercase + digit
    ("a", true),      // 'a' (0x61) — lower boundary of the lowercase range
    ("z", true),      // 'z' (0x7a) — upper boundary of the lowercase range
    ("aBc", false),   // has uppercase 'B'
    ("Ab", false),    // leading uppercase 'A' (disqualifier before 'b')
    ("ABC", false),   // all uppercase
    ("A", false),     // 'A' (0x41) — an uppercase letter, disqualifier
    ("Z", false),     // 'Z' (0x5a) — an uppercase letter, disqualifier
    ("", false),      // empty -> False (no cased char)
    ("123", false),   // digits only -> False (no cased char)
    (" ", false),     // a lone space -> False (no cased char)
    ("!", false),     // punctuation only -> False (no cased char)
    ("@", false),     // '@' (0x40) uncased -> False
    ("[", false),     // '[' (0x5b) uncased -> False
    ("`", false),     // '`' (0x60) uncased -> False
    ("{", false),     // '{' (0x7b) uncased -> False
];

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def <name>(s: str) -> bool: return s.isupper()` (or `.islower()`), selected
/// by `op`.
fn iscase_module(name: &str, op: StrMethodOp) -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op,
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
    wat.push_str("  ;; PMAT-1197 witness: preload the s param (below LITERAL_BASE)\n");
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

/// A short tag for the `op` used in temp-dir names / kernel names.
fn op_tag(op: StrMethodOp) -> &'static str {
    match op {
        StrMethodOp::IsUpper => "isupper",
        StrMethodOp::IsLower => "islower",
        _ => "iscase",
    }
}

/// Lower `<kernel>(s) = s.isupper()/.islower()`, run it in WABT with `s`
/// preloaded, return the bool. `None` when WABT is absent (caller skips the value
/// assertion).
fn exec_case(op: StrMethodOp, s: &str) -> Option<bool> {
    let kernel = "is_case";
    let kernel_wat = emit_module(&iscase_module(kernel, op)).expect("iscase program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, kernel, s);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-{}-{}-{}",
        op_tag(op),
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
        "wat2wasm failed for {s:?}.{}():\n{}\n---WAT---\n{wat}",
        op_tag(op),
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
        "wasm-interp run failed for {s:?}.{}(): stdout={stdout:?} stderr={:?}",
        op_tag(op),
        String::from_utf8_lossy(&run.stderr)
    );
    Some(parse_run_i32(&stdout) != 0)
}

/// Assemble + run a witness expected to TRAP, returning whether the run trapped.
/// `None` when WABT is absent.
fn exec_expect_trap(op: StrMethodOp, s: &str) -> Option<bool> {
    let kernel = "is_case";
    let kernel_wat = emit_module(&iscase_module(kernel, op)).expect("iscase program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, kernel, s);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-{}-trap-{}-{}",
        op_tag(op),
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
        "wat2wasm failed for the {s:?} {} trap witness:\n{}\n---WAT---\n{wat}",
        op_tag(op),
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
fn cpython_isupper_islower_ground_truth_is_pinned() {
    // Every isupper pin equals the ASCII `str.isupper()` reference (has an ASCII
    // uppercase letter AND no ASCII lowercase letter). Verified vs python3 when
    // this slice landed.
    for &(s, want) in UPPER_CASES {
        assert_eq!(py_isupper_ascii(s), want, "pinned {s:?}.isupper()");
        assert!(
            s.is_ascii(),
            "witness inputs are ASCII (non-ASCII may trap)"
        );
    }
    for &(s, want) in LOWER_CASES {
        assert_eq!(py_islower_ascii(s), want, "pinned {s:?}.islower()");
        assert!(
            s.is_ascii(),
            "witness inputs are ASCII (non-ASCII may trap)"
        );
    }
    // The empty-string False must be present in BOTH — the distinguishing
    // no-empty-guard case (falls through $has_cased == 0, no special-case) …
    assert!(UPPER_CASES.iter().any(|&(s, want)| s.is_empty() && !want));
    assert!(LOWER_CASES.iter().any(|&(s, want)| s.is_empty() && !want));
    // … the "no cased char" False must be present (a digits-only string) — the
    // key contrast with the every-char-fold predicates (`"123".isdigit()` is
    // True, but `"123".isupper()`/`.islower()` are False) …
    assert!(UPPER_CASES.iter().any(|&(s, want)| s == "123" && !want));
    assert!(LOWER_CASES.iter().any(|&(s, want)| s == "123" && !want));
    // … a True must be present in each (else the predicate could be a constant
    // `false`) …
    assert!(UPPER_CASES.iter().any(|&(_, want)| want));
    assert!(LOWER_CASES.iter().any(|&(_, want)| want));
    // … the opposite-case disqualifier must be exercised (a mixed-case string
    // that is False because of an opposite-case letter) …
    assert!(UPPER_CASES.iter().any(|&(s, want)| s == "aB" && !want));
    assert!(LOWER_CASES.iter().any(|&(s, want)| s == "Ab" && !want));
    // … and both inclusive ends of the wanted case range must be pinned True:
    assert!(UPPER_CASES.iter().any(|&(s, want)| s == "A" && want)); // 0x41
    assert!(UPPER_CASES.iter().any(|&(s, want)| s == "Z" && want)); // 0x5a
    assert!(LOWER_CASES.iter().any(|&(s, want)| s == "a" && want)); // 0x61
    assert!(LOWER_CASES.iter().any(|&(s, want)| s == "z" && want)); // 0x7a
}

#[test]
fn isupper_islower_emit_shared_helper_call_memory_and_no_allocator() {
    // CONSTRUCT assertion (holds with or without WABT): both programs lower
    // through the production emitter, carrying the SHARED helper + call, declaring
    // memory (the scan reads the str bytes), and — because a bool predicate
    // materialises NOTHING — pulling in NO bump allocator.
    for (op, want_upper_flag) in [
        (StrMethodOp::IsUpper, "i32.const 1"),
        (StrMethodOp::IsLower, "i32.const 0"),
    ] {
        let wat = emit_module(&iscase_module("is_case", op))
            .unwrap_or_else(|_| panic!("the s.{}() program must lower", op_tag(op)));
        assert!(
            wat.contains(
                "(func $__wasm_str_isupper_islower (param $s i32) (param $want_upper i32) (result i32)"
            ),
            "the shared isupper/islower helper must be emitted for {}:\n{wat}",
            op_tag(op)
        );
        assert!(
            wat.contains("call $__wasm_str_isupper_islower"),
            "$is_case must call the shared helper for {}:\n{wat}",
            op_tag(op)
        );
        // The direction flag is pushed immediately before the call.
        assert!(
            wat.contains(&format!(
                "{want_upper_flag}\n    call $__wasm_str_isupper_islower"
            )),
            "the {} kernel must push the {want_upper_flag} direction flag:\n{wat}",
            op_tag(op)
        );
        assert!(
            wat.contains("(func $is_case (param $s i32) (result i32)"),
            "bool return → i32 result, str param → i32 for {}:\n{wat}",
            op_tag(op)
        );
        assert!(
            wat.contains("(memory (export \"mem\")"),
            "the scan reads the str payload → memory must be declared for {}:\n{wat}",
            op_tag(op)
        );
        // A bool predicate allocates nothing — no bump allocator, no heap gate.
        assert!(
            !wat.contains("(func $__alloc"),
            "{} is non-allocating — it must NOT pull in the bump allocator:\n{wat}",
            op_tag(op)
        );
        // The honest ASCII-only boundary: an undecidable non-ASCII byte traps.
        assert!(
            wat.contains("unreachable"),
            "the helper must trap (unreachable) on an undecidable non-ASCII byte for {}:\n{wat}",
            op_tag(op)
        );
    }
}

#[test]
fn real_isupper_islower_programs_execute_in_wasm_and_match_cpython() {
    if !wasm_runtime_available() {
        // Still exercise the lowering (asserted structurally elsewhere).
        emit_module(&iscase_module("is_case", StrMethodOp::IsUpper)).expect("isupper lowers");
        emit_module(&iscase_module("is_case", StrMethodOp::IsLower)).expect("islower lowers");
        eprintln!(
            "PMAT-1197: skipping EXECUTED isupper/islower witness — WABT (wat2wasm \
             / wasm-interp) absent. The programs lowered through emit_module \
             (asserted in `isupper_islower_emit_shared_helper_call_memory_and_no_allocator`); \
             a box with WABT also runs them and asserts the bool == CPython."
        );
        return;
    }
    eprintln!("PMAT-1197: running EXECUTED s.isupper()/s.islower() witnesses via WABT");
    let mut ran = 0usize;
    for &(s, want) in UPPER_CASES {
        let got = exec_case(StrMethodOp::IsUpper, s).expect("WABT present");
        assert_eq!(
            got, want,
            "executed WASM {s:?}.isupper() = {got} but CPython = {want}"
        );
        ran += 1;
    }
    for &(s, want) in LOWER_CASES {
        let got = exec_case(StrMethodOp::IsLower, s).expect("WABT present");
        assert_eq!(
            got, want,
            "executed WASM {s:?}.islower() = {got} but CPython = {want}"
        );
        ran += 1;
    }
    assert_eq!(ran, UPPER_CASES.len() + LOWER_CASES.len());
    eprintln!(
        "PMAT-1197: all {ran} inputs executed in WABT and value-matched CPython \
         (isupper: 'ABC'/'A1'/'A B'/'A'/'Z'->True, 'Abc'/'aB'/'abc'/'a'/'z'->False, \
         ''/'123'/' '->False (no cased char); islower: the mirror). Empty and \
         digits-only correctly return False WITHOUT an empty guard."
    );
}

#[test]
fn non_ascii_no_disqualifier_prefix_traps() {
    // Honest ASCII-only boundary: `.isupper()` over a string whose ASCII prefix
    // has NO opposite-case (lowercase) letter, then a non-ASCII byte, is
    // UNDECIDABLE (the trailing code point might be a same-case, opposite-case, or
    // uncased Unicode char — CPython "A\u{00c1}".isupper() is True) — so it TRAPS
    // (`unreachable`), NEVER a silent wrong bool. "A\u{00c1}" — Á is U+00C1 =
    // 0xC3 0x81, the first byte >= 0x80, reached after the uppercase 'A' with no
    // lowercase seen. The islower mirror: "a\u{00e1}" (á = U+00E1 = 0xC3 0xA1).
    for (op, s) in [
        (StrMethodOp::IsUpper, "A\u{00c1}"),
        (StrMethodOp::IsLower, "a\u{00e1}"),
    ] {
        emit_module(&iscase_module("is_case", op)).expect("iscase program lowers");
        if !wasm_runtime_available() {
            eprintln!(
                "PMAT-1197: skipping non-ASCII trap witness ({}) — WABT absent. The \
                 trap (`unreachable`) is asserted structurally in \
                 `isupper_islower_emit_shared_helper_call_memory_and_no_allocator`.",
                op_tag(op)
            );
            continue;
        }
        assert!(
            !s.is_ascii(),
            "the trap fixture must carry a non-ASCII byte"
        );
        let trapped = exec_expect_trap(op, s).expect("WABT present");
        assert!(
            trapped,
            "'{s}'.{}() must TRAP on the non-ASCII byte after a prefix with no \
             opposite-case letter (honest ASCII-only boundary), not return a bool",
            op_tag(op)
        );
        eprintln!(
            "PMAT-1197: '{s}'.{}() correctly TRAPPED on the non-ASCII byte (0xC3) \
             after a same-case ASCII prefix — undecidable Unicode-cased case, never \
             a silent wrong bool.",
            op_tag(op)
        );
    }
}

#[test]
fn opposite_case_ascii_before_non_ascii_returns_false_no_trap() {
    // The distinguishing correctness of a PREDICATE (vs the case-fold ops): an
    // opposite-case ASCII letter short-circuits to `0` (False) BEFORE any later
    // non-ASCII byte is examined, so it does NOT trap. isupper "a\u{00c1}" — the
    // leading lowercase 'a' forces False; the scan returns 0 and NEVER reaches the
    // non-ASCII Á. CPython "a\u{00c1}".isupper() is also False, an exact match.
    // islower mirror: "A\u{00e1}" — leading 'A' forces False before á.
    for (op, s, ascii_prefix, py) in [
        (
            StrMethodOp::IsUpper,
            "a\u{00c1}",
            "a",
            py_isupper_ascii as fn(&str) -> bool,
        ),
        (
            StrMethodOp::IsLower,
            "A\u{00e1}",
            "A",
            py_islower_ascii as fn(&str) -> bool,
        ),
    ] {
        emit_module(&iscase_module("is_case", op)).expect("iscase program lowers");
        if !wasm_runtime_available() {
            eprintln!(
                "PMAT-1197: skipping short-circuit witness ({}) — WABT absent. The \
                 short-circuit-before-trap ordering is structural (the disqualifier \
                 `return` precedes the loop's next iteration).",
                op_tag(op)
            );
            continue;
        }
        assert!(
            !s.is_ascii(),
            "the fixture must carry a non-ASCII byte after the disqualifier"
        );
        // CPython ground truth: False (the opposite-case ASCII letter forces it,
        // regardless of the trailing non-ASCII char).
        assert!(
            !py(ascii_prefix),
            "the ASCII prefix {ascii_prefix:?} is already the answer (False)"
        );
        let got = exec_case(op, s).expect("WABT present");
        assert!(
            !got,
            "'{s}'.{}() must return False via short-circuit on the opposite-case \
             '{ascii_prefix}' BEFORE the non-ASCII byte (matching CPython) — not trap, not True",
            op_tag(op)
        );
        eprintln!(
            "PMAT-1197: '{s}'.{}() correctly returned False (short-circuit on the \
             opposite-case '{ascii_prefix}' before the non-ASCII byte) — a definitive \
             answer never traps, matching CPython.",
            op_tag(op)
        );
    }
}
