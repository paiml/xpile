//! PMAT-1203 — EXECUTED `s.title()` witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! `s.title()` materialises a NEW heap string title-cased word-by-word: the FIRST
//! ASCII letter of each word is upper-cased, every REMAINING letter of the word is
//! lower-cased, and any NON-ALPHABETIC character (space, digit, `_`, punctuation)
//! is a WORD BOUNDARY that resets the state. It joins the allocating string-method
//! family (`removeprefix` / `removesuffix` / `replace` / `zfill` / `upper` /
//! `lower` / `capitalize` / `swapcase`) on the WASM lane: an `Expr::StrMethod {
//! op: Title }` in a string position lowers via the allocating `$__wasm_str_title`
//! helper (calls `$__alloc` + `i32.store8`, rides the `needs_heap` gate).
//!
//! ## The real program
//!
//! ```python
//! def title(s: str) -> str:
//!     return s.title()
//! ```
//!
//! ## STATEFUL, unlike the byte-parallel `.swapcase()`
//!
//! Title-casing is NOT a per-byte function of the byte alone — the case a letter
//! receives depends on whether the PREVIOUS character was a cased (ASCII-letter)
//! character. The helper carries a `$prev` flag: a letter at a word START
//! (`$prev == 0`) is upper-cased, a letter MID-word (`$prev == 1`) is lower-cased,
//! and any non-letter passes through unchanged AND clears `$prev`. This is exactly
//! CPython's ASCII `do_title` loop (`is_cased == is_letter` for ASCII), so
//! `"it's".title() == "It'S"` (the un-cased `'` resets the word, re-capitalising
//! the `s`) and `"a1b2c3".title() == "A1B2C3"` (every digit is a boundary, so
//! every letter re-capitalises).
//!
//! ## ASCII-only, with the SAME HONEST runtime boundary as the case-fold siblings
//!
//! Python's `str.title()` does FULL Unicode title mapping, which needs a case
//! table this scalar lane does not carry. So the helper title-cases only the ASCII
//! letters and, on the FIRST byte `>= 0x80` (any byte of a non-ASCII code point in
//! valid UTF-8), executes `unreachable` — a TRAP, exactly like the `upper` /
//! `lower` / `capitalize` / `swapcase` siblings. It NEVER passes a non-ASCII byte
//! through unchanged, so it never silently diverges from CPython: for pure-ASCII
//! `s` the result is char-exact, and for a non-ASCII `s` it aborts.
//! `cpython_title_ground_truth_is_pinned` documents the ASCII boundary and
//! `non_ascii_title_traps_not_silent` proves the trap.
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel `$title`
//! takes an `i32` (the `s` param base-pointer, preloaded into a `(data …)` region
//! below `LITERAL_BASE`) and returns the constructed string's `i32` base-pointer.
//! The witness adds only zero-arg wrappers that push the constant `S_ADDR`, call
//! the kernel, and read back the result: `run_len` (the i32 byte-count header @
//! result+0) and a `run_byte_i` family (each re-runs the kernel and `i32.load8_u`s
//! payload byte `i`).
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_str_title` helper + call + heap + trap) on a
//! host without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and
/// the bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0,
/// UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;

/// ASCII `str.title()` reference — CPython's `do_title` loop specialised to ASCII
/// (`is_cased == is_ascii_alphabetic`): a letter at a word start (previous char
/// not a letter) is upper-cased, a letter mid-word is lower-cased, and any
/// non-letter passes through and resets the "previous was cased" state. Byte-exact
/// against CPython for ASCII inputs (the WASM lane traps on non-ASCII). Used both
/// to PIN the expectations and cross-check them.
fn py_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_cased = false;
    for c in s.chars() {
        if c.is_ascii_alphabetic() {
            if prev_cased {
                out.push(c.to_ascii_lowercase());
            } else {
                out.push(c.to_ascii_uppercase());
            }
            prev_cased = true;
        } else {
            out.push(c);
            prev_cased = false;
        }
    }
    out
}

/// (input, CPython `input.title()`) — pinned to the exact CPython ground truth
/// (verified with python3 when this slice landed). ASCII-only inputs, since the
/// WASM lane traps on non-ASCII (see `non_ascii_title_traps_not_silent`). Chosen to
/// stress the STATEFUL word-boundary logic, not just a whole-string capitalize.
const CASES: &[(&str, &str)] = &[
    ("hello world", "Hello World"), // two words, space boundary
    ("it's", "It'S"),               // apostrophe re-capitalises the trailing 's'
    ("123abc", "123Abc"),           // leading digits, then a fresh word
    ("ABC", "Abc"),                 // all-upper collapses to Title (mid-word lower)
    ("HELLO", "Hello"),             // all-upper single word
    ("a1b2c3", "A1B2C3"),           // EVERY digit is a boundary -> every letter re-caps
    ("mIxEd42cAsE", "Mixed42Case"), // mixed case + a digit run splitting two words
    ("foo_bar", "Foo_Bar"),         // '_' (0x5f) is a non-letter boundary
    ("", ""),                       // empty -> empty (no payload)
    ("aZ", "Az"),                   // single word: first upper, rest LOWER (mid-word)
    (" a ", " A "),                 // leading/trailing space around a 1-letter word
    ("don't stop", "Don'T Stop"),   // apostrophe + space, two re-caps
];

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def title(s: str) -> str: return s.title()`.
fn title_module(name: &str) -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::Title,
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

/// A stable per-input hash so distinct cases get distinct temp dirs (title cases
/// share byte lengths, so a length-only key would collide).
fn input_hash(s: &str) -> u64 {
    let mut h: u64 = 1469598103934665603; // FNV-1a offset basis
    for &b in s.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

/// Splice the preloaded `s` `(data …)` region + zero-arg read-back exports
/// (`run_len` / `run_byte_i`) onto the emitted module, before its closing `)`.
/// `kernel` = the emitted kernel function name (`title`); `n_out` = the expected
/// result byte length.
fn build_witness_wat(kernel_wat: &str, kernel: &str, s: &str, n_out: usize) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1203 witness: preload the s param (below LITERAL_BASE)\n");
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

/// Lower `title(s) = s.title()`, run it in WABT with `s` preloaded, and
/// reconstruct the title-cased string. `None` when WABT is absent (caller skips
/// the value assertion). Asserts the WASM byte length matches CPython.
fn exec_case(s: &str, expected: &str) -> Option<String> {
    let kernel = "title";
    let kernel_wat = emit_module(&title_module(kernel)).expect("title program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let n_out = expected.len();
    let wat = build_witness_wat(&kernel_wat, kernel, s, n_out);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-title-{}-{:016x}",
        std::process::id(),
        input_hash(s)
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
        "wat2wasm failed for {s:?}.title():\n{}\n---WAT---\n{wat}",
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
        "wasm-interp run failed for {s:?}.title(): stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    let got_len = parse_i32_export(&stdout, "run_len");
    assert_eq!(
        got_len as usize, n_out,
        "{s:?}.title() byte length: WASM={got_len} CPython={n_out}\nWAT:\n{wat}"
    );
    let mut bytes = Vec::with_capacity(n_out);
    for i in 0..n_out {
        bytes.push(parse_i32_export(&stdout, &format!("run_byte_{i}")) as u8);
    }
    Some(String::from_utf8(bytes).expect("constructed title string bytes are valid UTF-8"))
}

#[test]
fn cpython_title_ground_truth_is_pinned() {
    // Every pin equals the ASCII `str.title()` reference (word-start letters
    // upper-cased, mid-word letters lower-cased, non-letters passed through and
    // resetting the word state). Verified vs python3 when this slice landed.
    for &(s, want) in CASES {
        assert_eq!(py_title(s), want, "pinned {s:?}.title()");
        // ASCII-only: byte length == char length == unchanged across the flip.
        assert_eq!(s.len(), want.len(), "title preserves byte length for {s:?}");
        assert!(s.is_ascii(), "witness inputs are ASCII (non-ASCII traps)");
    }
    // The fixture must EXERCISE the statefulness (else the op could be a plain
    // whole-string capitalize masquerading as title):
    //   * a MID-WORD lowercasing ("aZ" -> "Az": the 'Z' is lowered because it is
    //     not the first letter of its word).
    assert!(CASES.iter().any(|&(s, w)| s == "aZ" && w == "Az"));
    //   * a non-letter re-capitalising a NEW word: the apostrophe in "it's".
    assert!(CASES.iter().any(|&(s, w)| s == "it's" && w == "It'S"));
    //   * a DIGIT acting as a word boundary, re-capitalising every letter after
    //     it ("a1b2c3" -> "A1B2C3") — the strongest statefulness pin, and the case
    //     a whole-string capitalize would get wrong ("A1b2c3").
    assert!(CASES.iter().any(|&(s, w)| s == "a1b2c3" && w == "A1B2C3"));
    // And more than one word must appear (a boundary must actually fire).
    assert!(CASES
        .iter()
        .any(|&(s, w)| s == "hello world" && w == "Hello World"));
}

#[test]
fn title_emits_helper_call_heap_and_trap() {
    // CONSTRUCT assertion (holds with or without WABT): the program lowers through
    // the production emitter, carrying the helper + call + heap, and — the honest
    // ASCII-only boundary — a trap on a non-ASCII byte.
    let wat = emit_module(&title_module("title")).expect("the s.title() program must lower");
    assert!(
        wat.contains("(func $__wasm_str_title (param $s i32) (result i32)"),
        "the title helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_title"),
        "$title must call the title helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $title (param $s i32) (result i32)"),
        "str return → i32 result (heap pointer), str param → i32:\n{wat}"
    );
    // Materialising a title-cased string → needs the bump heap.
    assert!(
        wat.contains("(func $__alloc"),
        "title needs the bump heap:\n{wat}"
    );
    // The honest ASCII-only boundary: a non-ASCII byte traps.
    assert!(
        wat.contains("unreachable"),
        "the helper must trap (unreachable) on a non-ASCII byte:\n{wat}"
    );
    // Statefulness marker: the helper carries the `$prev` word-boundary local
    // (title-casing is not a per-byte function; a byte-parallel emit would omit it).
    assert!(
        wat.contains("(local $prev i32)"),
        "the title helper must carry the stateful `$prev` word-boundary flag:\n{wat}"
    );
}

#[test]
fn real_title_program_executes_in_wasm_and_matches_cpython() {
    let wat =
        emit_module(&title_module("title")).expect("title program lowers through emit_module");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1203: skipping EXECUTED title witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module (asserted \
             in `title_emits_helper_call_heap_and_trap`); a box with WABT also runs \
             it and asserts the CONSTRUCTED string == CPython."
        );
        return;
    }
    eprintln!("PMAT-1203: running EXECUTED s.title() witness via WABT");
    let mut ran = 0usize;
    for &(s, want) in CASES {
        let got = exec_case(s, want).expect("WABT present");
        assert_eq!(
            got, want,
            "executed WASM {s:?}.title() = {got:?} but CPython = {want:?}"
        );
        ran += 1;
    }
    assert_eq!(ran, CASES.len());
    eprintln!(
        "PMAT-1203: all {ran} inputs executed in WABT and value-matched CPython \
         (statefulness pinned: mid-word lower 'aZ'->'Az', apostrophe re-cap \
         'it's'->'It'S', digit-boundary re-cap 'a1b2c3'->'A1B2C3', empty ''->'').\n\
         --- emitted title WAT (emit_module over meta-HIR) ---\n{wat}"
    );
}

#[test]
fn non_ascii_title_traps_not_silent() {
    // The honest ASCII-only boundary: `.title()` over a string with a non-ASCII
    // byte TRAPS (`unreachable`) rather than silently returning a wrongly-cased
    // string. CPython would title-map ("cafÉ au lait".title() == "Café Au Lait"),
    // but this scalar lane carries no case table — so it aborts, NEVER a silent
    // divergence.
    let wat =
        emit_module(&title_module("title")).expect("title program lowers through emit_module");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1203: skipping non-ASCII trap witness — WABT absent. The trap \
             (`unreachable`) is asserted structurally in \
             `title_emits_helper_call_heap_and_trap`."
        );
        return;
    }
    // "Café" — 'é' is 0xC3 0xA9, the first byte >= 0x80 -> the helper traps. (The
    // ASCII prefix "Caf" is title-cased in place first; the trap fires on the 'é'.)
    let s = "Café";
    let witness = build_witness_wat(&wat, "title", s, 1);
    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-str-title-trap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("trap.wat");
    let wasm_path = dir.join("trap.wasm");
    std::fs::write(&wat_path, &witness).expect("write wat");
    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed for the trap witness:\n{}\n---WAT---\n{witness}",
        String::from_utf8_lossy(&assemble.stderr)
    );
    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    // The non-ASCII byte must drive the `unreachable` trap — either a non-zero exit
    // or an explicit "unreachable executed" in the interp output. NEVER a clean run
    // returning a folded/unchanged string.
    let trapped =
        !run.status.success() || stdout.contains("unreachable") || stderr.contains("unreachable");
    assert!(
        trapped,
        "'{s}'.title() must TRAP on the non-ASCII byte (honest ASCII-only \
         boundary), not run clean: status={:?} stdout={stdout:?} stderr={stderr:?}",
        run.status
    );
    eprintln!(
        "PMAT-1203: '{s}'.title() correctly TRAPPED on the non-ASCII 'é' byte \
         (0xC3) — honest ASCII-only boundary, never a silent wrongly-cased string."
    );
}
