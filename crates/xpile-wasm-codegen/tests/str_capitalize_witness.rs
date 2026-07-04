//! PMAT-1187 — EXECUTED `s.capitalize()` witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! `s.capitalize()` materialises a NEW heap string whose FIRST ASCII letter is
//! upper-cased and every REMAINING ASCII letter is lower-cased. It joins the
//! allocating string-method family (`removeprefix` / `removesuffix` / `replace` /
//! `zfill` / `upper` / `lower`) on the WASM lane: an `Expr::StrMethod { op:
//! Capitalize }` in a string position lowers via the allocating
//! `$__wasm_str_capitalize` helper (calls `$__alloc` + `i32.store8`, rides the
//! `needs_heap` gate). The helper branches on the byte index — `i == 0`
//! upper-flips an `a`–`z`, `i > 0` lower-flips an `A`–`Z`.
//!
//! ## The real program
//!
//! ```python
//! def cap(s: str) -> str:
//!     return s.capitalize()
//! ```
//!
//! ## ASCII-only, with the SAME HONEST runtime boundary as upper/lower
//!
//! Python's `str.capitalize()` does FULL Unicode case mapping (title-case the
//! first char, lower-fold the rest), which needs a case table this scalar lane
//! does not carry. So the helper case-flips only the ASCII letters and, on the
//! FIRST byte `>= 0x80` (any byte of a non-ASCII code point in valid UTF-8),
//! executes `unreachable` — a TRAP, exactly like the `upper` / `lower` / `index`
//! siblings. It NEVER passes a non-ASCII byte through unchanged, so it never
//! silently diverges from CPython: for pure-ASCII `s` the result is char-exact,
//! and for a non-ASCII `s` it aborts rather than returning a wrongly-mapped
//! string. `non_ascii_capitalize_traps_not_silent` proves the trap fires even
//! when the non-ASCII byte is in the LOWER-cased tail (`"café"`), not just at the
//! first char.
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel `$cap`
//! takes an `i32` (the `s` param base-pointer, preloaded into a `(data …)` region
//! below `LITERAL_BASE`) and returns the constructed string's `i32` base-pointer.
//! The witness adds only zero-arg wrappers that push the constant `S_ADDR`, call
//! the kernel, and read back the result: `run_len` (the i32 byte-count header @
//! result+0) and a `run_byte_i` family (each re-runs the kernel and `i32.load8_u`s
//! payload byte `i`).
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_str_capitalize` helper + call) on a host
//! without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and
/// the bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0,
/// UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;

/// Pure-ASCII Python `str.capitalize()` reference — first char upper-cased, every
/// remaining char lower-cased (the empty string maps to `""`). Used both to PIN
/// the `CASES` expectations and as the ground truth the witness value-matches.
/// (For ASCII inputs this is byte-identical to CPython's `str.capitalize()`.)
fn py_capitalize_ascii(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let mut out = String::new();
            out.push(first.to_ascii_uppercase());
            out.push_str(&chars.as_str().to_ascii_lowercase());
            out
        }
    }
}

/// (input, CPython `input.capitalize()`) — pinned to the exact CPython ground
/// truth (verified with python3). ASCII-only inputs, since the WASM lane traps on
/// non-ASCII (see `non_ascii_capitalize_traps_not_silent`).
const CASES: &[(&str, &str)] = &[
    ("hello", "Hello"),         // the headline: first up, rest already lower
    ("WORLD", "World"),         // first stays up, rest lowered
    ("heLLo", "Hello"),         // interior caps lowered, first upper-flipped
    ("", ""),                   // empty -> empty (no payload)
    ("a", "A"),                 // single char upper-flipped
    ("Z", "Z"),                 // single upper char stays, no tail
    ("aBC dEF", "Abc def"),     // space is NOT a word boundary — whole tail lowered
    ("123abc", "123abc"),       // first char '1' non-letter, tail already lower
    ("42", "42"),               // no letters at all -> plain copy
    ("gGkK", "Ggkk"),           // first 'g'->'G', tail "GkK"->"gkk"
    ("hi there!", "Hi there!"), // punctuation + space pass through
    ("_abc", "_abc"),           // '_' (0x5f) first, non-letter, tail already lower
];

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def cap(s: str) -> str: return s.capitalize()`.
fn cap_module(name: &str) -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::Capitalize,
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
/// `kernel` = the emitted kernel function name (`cap`); `n_out` = the expected
/// result byte length.
fn build_witness_wat(kernel_wat: &str, kernel: &str, s: &str, n_out: usize) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1187 witness: preload the s param (below LITERAL_BASE)\n");
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

/// Lower `cap(s) = s.capitalize()`, run it in WABT with `s` preloaded, and
/// reconstruct the case-mapped string. `None` when WABT is absent (caller skips
/// the value assertion). Asserts the WASM byte length matches CPython.
fn exec_case(s: &str, expected: &str) -> Option<String> {
    let kernel_wat = emit_module(&cap_module("cap")).expect("cap program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let n_out = expected.len();
    let wat = build_witness_wat(&kernel_wat, "cap", s, n_out);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-capitalize-{}-{}",
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
        "wat2wasm failed for {s:?}.capitalize():\n{}\n---WAT---\n{wat}",
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
        "wasm-interp run failed for {s:?}.capitalize(): stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    let got_len = parse_i32_export(&stdout, "run_len");
    assert_eq!(
        got_len as usize, n_out,
        "{s:?}.capitalize() byte length: WASM={got_len} CPython={n_out}\nWAT:\n{wat}"
    );
    let mut bytes = Vec::with_capacity(n_out);
    for i in 0..n_out {
        bytes.push(parse_i32_export(&stdout, &format!("run_byte_{i}")) as u8);
    }
    Some(String::from_utf8(bytes).expect("constructed capitalize string bytes are valid UTF-8"))
}

#[test]
fn cpython_capitalize_ground_truth_is_pinned() {
    // The pinned CPython forms the witness value-matches. On ASCII, capitalize
    // upper-flips the first 'a'..'z' and lower-flips every remaining 'A'..'Z';
    // every other byte (digits, '_', space, punctuation) passes through, and the
    // code-point length is unchanged. (These pins were verified vs python3 when
    // this slice landed.)
    for &(s, cap) in CASES {
        assert_eq!(py_capitalize_ascii(s), cap, "pinned {s:?}.capitalize()");
        // ASCII-only: byte length == char length == unchanged across the flip.
        assert_eq!(
            s.len(),
            cap.len(),
            "capitalize preserves byte length for {s:?}"
        );
        assert!(s.is_ascii(), "witness inputs are ASCII (non-ASCII traps)");
    }
}

#[test]
fn capitalize_emits_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): the program lowers through
    // the production emitter, carrying the helper + call + heap + trap.
    let wat = emit_module(&cap_module("cap"))
        .expect("the s.capitalize() program must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_str_capitalize (param $s i32) (result i32)"),
        "the capitalize helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_capitalize"),
        "$cap must call the capitalize helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $cap (param $s i32) (result i32)"),
        "str return → i32 result (heap pointer), str param → i32:\n{wat}"
    );
    // Materialising a case-mapped string → needs the bump heap.
    assert!(
        wat.contains("(func $__alloc"),
        "capitalize needs the bump heap:\n{wat}"
    );
    // The honest ASCII-only boundary: a non-ASCII byte traps.
    assert!(
        wat.contains("unreachable"),
        "the helper must trap (unreachable) on a non-ASCII byte:\n{wat}"
    );
    // The first-char branch: an `i32.eqz` on the loop index selects upper-flip.
    assert!(
        wat.contains("i32.eqz"),
        "the helper must branch on i == 0 (i32.eqz) for the first-char upper-flip:\n{wat}"
    );
}

#[test]
fn real_capitalize_program_executes_in_wasm_and_matches_cpython() {
    let cap_wat = emit_module(&cap_module("cap")).expect("cap program lowers through emit_module");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1187: skipping EXECUTED capitalize witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module \
             (asserted in `capitalize_emits_helper_and_call`); a box with WABT \
             also runs it and asserts the CONSTRUCTED string == CPython."
        );
        return;
    }
    eprintln!("PMAT-1187: running EXECUTED s.capitalize() witness via WABT");
    let mut ran = 0usize;
    for &(s, cap) in CASES {
        let got = exec_case(s, cap).expect("WABT present");
        assert_eq!(
            got, cap,
            "executed WASM {s:?}.capitalize() = {got:?} but CPython = {cap:?}"
        );
        ran += 1;
    }
    assert_eq!(ran, CASES.len());
    eprintln!(
        "PMAT-1187: all {ran} inputs executed in WABT and value-matched CPython \
         (incl. 'heLLo'->'Hello', 'aBC dEF'->'Abc def' (space is not a boundary), \
         digits/'_'/punctuation pass-through, empty ''->'').\n\
         --- emitted cap WAT (emit_module over meta-HIR) ---\n{cap_wat}"
    );
}

#[test]
fn non_ascii_capitalize_traps_not_silent() {
    // The honest ASCII-only boundary: `.capitalize()` over a string with a
    // non-ASCII byte TRAPS (`unreachable`) rather than silently returning a
    // wrongly-mapped string. CPython would map ("café".capitalize() == "Café"),
    // but this scalar lane carries no case table — so it aborts, NEVER a silent
    // divergence. The non-ASCII byte here is in the LOWER-cased TAIL (index 2),
    // proving the trap fires beyond the first char.
    let cap_wat = emit_module(&cap_module("cap")).expect("cap program lowers through emit_module");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1187: skipping non-ASCII trap witness — WABT absent. The trap \
             (`unreachable`) is asserted structurally in \
             `capitalize_emits_helper_and_call`."
        );
        return;
    }
    // "café" — 'é' is 0xC3 0xA9 at byte index 3, the first byte >= 0x80 -> trap.
    let s = "café";
    // Build a witness that just reads run_len (one call is enough to hit the trap
    // on the non-ASCII byte). `n_out` is irrelevant — the run must FAIL.
    let wat = build_witness_wat(&cap_wat, "cap", s, 1);
    let dir = std::env::temp_dir().join(format!("xpile-wasm-str-cap-trap-{}", std::process::id()));
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
        "wat2wasm failed for the trap witness:\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&assemble.stderr)
    );
    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    // The non-ASCII byte must drive the `unreachable` trap — either a non-zero
    // exit or an explicit "unreachable executed" in the interp output. NEVER a
    // clean run returning a mapped/unchanged string.
    let trapped =
        !run.status.success() || stdout.contains("unreachable") || stderr.contains("unreachable");
    assert!(
        trapped,
        "'{s}'.capitalize() must TRAP on the non-ASCII byte (honest ASCII-only \
         boundary), not run clean: status={:?} stdout={stdout:?} stderr={stderr:?}",
        run.status
    );
    eprintln!(
        "PMAT-1187: '{s}'.capitalize() correctly TRAPPED on the non-ASCII 'é' byte \
         (0xC3) in the lower-cased tail — honest ASCII-only boundary, never a \
         silent wrongly-mapped string."
    );
}
