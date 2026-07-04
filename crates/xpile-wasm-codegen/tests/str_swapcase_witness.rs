//! PMAT-1201 — EXECUTED `s.swapcase()` witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! `s.swapcase()` materialises a NEW heap string with the case of every ASCII
//! letter flipped BOTH ways (`A`–`Z` → `a`–`z` AND `a`–`z` → `A`–`Z`, in one
//! pass). It joins the allocating string-method family (`removeprefix` /
//! `removesuffix` / `replace` / `zfill` / `upper` / `lower` / `capitalize`) on
//! the WASM lane: an `Expr::StrMethod { op: SwapCase }` in a string position
//! lowers via the allocating `$__wasm_str_swapcase` helper (calls `$__alloc` +
//! `i32.store8`, rides the `needs_heap` gate). It is the both-directions twin of
//! `$__wasm_str_upper_lower`, which flips only ONE direction per its `up` flag.
//!
//! ## The real program
//!
//! ```python
//! def swap(s: str) -> str:
//!     return s.swapcase()
//! ```
//!
//! ## ASCII-only, with the SAME HONEST runtime boundary as `.upper()`/`.lower()`
//!
//! Python's `str.swapcase()` does FULL Unicode case flipping (`"ß".swapcase() ==
//! "SS"`, `"É".swapcase() == "é"`), which needs a case table this scalar lane
//! does not carry. So the helper flips only the ASCII letters and, on the FIRST
//! byte `>= 0x80` (any byte of a non-ASCII code point in valid UTF-8), executes
//! `unreachable` — a TRAP, exactly like the `upper` / `lower` / `capitalize`
//! siblings. It NEVER passes a non-ASCII byte through unchanged, so it never
//! silently diverges from CPython: for pure-ASCII `s` the result is char-exact,
//! and for a non-ASCII `s` it aborts rather than returning a wrongly-flipped
//! string. `cpython_swapcase_ground_truth_is_pinned` documents that ASCII
//! boundary and `non_ascii_swapcase_traps_not_silent` proves the trap.
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel `$swap`
//! takes an `i32` (the `s` param base-pointer, preloaded into a `(data …)` region
//! below `LITERAL_BASE`) and returns the constructed string's `i32` base-pointer.
//! The witness adds only zero-arg wrappers that push the constant `S_ADDR`, call
//! the kernel, and read back the result: `run_len` (the i32 byte-count header @
//! result+0) and a `run_byte_i` family (each re-runs the kernel and `i32.load8_u`s
//! payload byte `i`).
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_str_swapcase` helper + call + heap + trap)
//! on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and
/// the bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0,
/// UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;

/// ASCII `str.swapcase()` reference — flip `'A'`–`'Z'` ↔ `'a'`–`'z'`, every other
/// byte unchanged. Byte-exact against CPython for ASCII inputs (the WASM lane
/// traps on non-ASCII). Used both to PIN the expectations and cross-check them.
fn py_swapcase(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_uppercase() {
                c.to_ascii_lowercase()
            } else if c.is_ascii_lowercase() {
                c.to_ascii_uppercase()
            } else {
                c
            }
        })
        .collect()
}

/// (input, CPython `input.swapcase()`) — pinned to the exact CPython ground truth
/// (verified with python3). ASCII-only inputs, since the WASM lane traps on
/// non-ASCII (see `non_ascii_swapcase_traps_not_silent`).
const CASES: &[(&str, &str)] = &[
    ("hello", "HELLO"),             // all lower -> all upper
    ("WORLD", "world"),             // all upper -> all lower
    ("MixedCase42", "mIXEDcASE42"), // mixed; digits pass through unchanged
    ("", ""),                       // empty -> empty (no payload)
    ("aZ", "Az"),                   // one of each, both directions in one string
    ("Hi There!", "hI tHERE!"),     // space + punctuation unchanged
    ("123-456", "123-456"),         // no letters at all -> plain copy
    ("aA_zZ", "Aa_Zz"),             // '_' (0x5f, between 'Z' and 'a') untouched
    ("gGkK", "GgKk"),               // interior letters, alternating case
];

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def swap(s: str) -> str: return s.swapcase()`.
fn swapcase_module(name: &str) -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::SwapCase,
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
/// `kernel` = the emitted kernel function name (`swap`); `n_out` = the expected
/// result byte length.
fn build_witness_wat(kernel_wat: &str, kernel: &str, s: &str, n_out: usize) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1201 witness: preload the s param (below LITERAL_BASE)\n");
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

/// Lower `swap(s) = s.swapcase()`, run it in WABT with `s` preloaded, and
/// reconstruct the case-flipped string. `None` when WABT is absent (caller skips
/// the value assertion). Asserts the WASM byte length matches CPython.
fn exec_case(s: &str, expected: &str) -> Option<String> {
    let kernel = "swap";
    let kernel_wat = emit_module(&swapcase_module(kernel)).expect("swapcase program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let n_out = expected.len();
    let wat = build_witness_wat(&kernel_wat, kernel, s, n_out);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-swapcase-{}-{}",
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
        "wat2wasm failed for {s:?}.swapcase():\n{}\n---WAT---\n{wat}",
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
        "wasm-interp run failed for {s:?}.swapcase(): stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    let got_len = parse_i32_export(&stdout, "run_len");
    assert_eq!(
        got_len as usize, n_out,
        "{s:?}.swapcase() byte length: WASM={got_len} CPython={n_out}\nWAT:\n{wat}"
    );
    let mut bytes = Vec::with_capacity(n_out);
    for i in 0..n_out {
        bytes.push(parse_i32_export(&stdout, &format!("run_byte_{i}")) as u8);
    }
    Some(String::from_utf8(bytes).expect("constructed swapcase string bytes are valid UTF-8"))
}

#[test]
fn cpython_swapcase_ground_truth_is_pinned() {
    // Every pin equals the ASCII `str.swapcase()` reference (flip 'A'..'Z' <->
    // 'a'..'z'; every other byte — digits, '_', space, punctuation — passes
    // through, and the code-point length is unchanged). Verified vs python3 when
    // this slice landed.
    for &(s, want) in CASES {
        assert_eq!(py_swapcase(s), want, "pinned {s:?}.swapcase()");
        // ASCII-only: byte length == char length == unchanged across the flip.
        assert_eq!(
            s.len(),
            want.len(),
            "swapcase preserves byte length for {s:?}"
        );
        assert!(s.is_ascii(), "witness inputs are ASCII (non-ASCII traps)");
    }
    // Both directions must be exercised in the fixture (else the op could be a
    // one-way fold masquerading as swapcase): at least one lower->upper flip AND
    // one upper->lower flip must appear.
    assert!(CASES.iter().any(|&(s, w)| s == "hello" && w == "HELLO"));
    assert!(CASES.iter().any(|&(s, w)| s == "WORLD" && w == "world"));
    // A single string carrying BOTH directions at once ("aZ" -> "Az") pins that
    // the flip is per-byte, not a whole-string upper/lower.
    assert!(CASES.iter().any(|&(s, w)| s == "aZ" && w == "Az"));
}

#[test]
fn swapcase_emits_helper_call_heap_and_trap() {
    // CONSTRUCT assertion (holds with or without WABT): the program lowers through
    // the production emitter, carrying the helper + call + heap, and — the honest
    // ASCII-only boundary — a trap on a non-ASCII byte.
    let wat = emit_module(&swapcase_module("swap")).expect("the s.swapcase() program must lower");
    assert!(
        wat.contains("(func $__wasm_str_swapcase (param $s i32) (result i32)"),
        "the swapcase helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_swapcase"),
        "$swap must call the swapcase helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $swap (param $s i32) (result i32)"),
        "str return → i32 result (heap pointer), str param → i32:\n{wat}"
    );
    // Materialising a case-flipped string → needs the bump heap.
    assert!(
        wat.contains("(func $__alloc"),
        "swapcase needs the bump heap:\n{wat}"
    );
    // The honest ASCII-only boundary: a non-ASCII byte traps.
    assert!(
        wat.contains("unreachable"),
        "the helper must trap (unreachable) on a non-ASCII byte:\n{wat}"
    );
}

#[test]
fn real_swapcase_program_executes_in_wasm_and_matches_cpython() {
    let wat =
        emit_module(&swapcase_module("swap")).expect("swapcase program lowers through emit_module");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1201: skipping EXECUTED swapcase witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module (asserted \
             in `swapcase_emits_helper_call_heap_and_trap`); a box with WABT also \
             runs it and asserts the CONSTRUCTED string == CPython."
        );
        return;
    }
    eprintln!("PMAT-1201: running EXECUTED s.swapcase() witness via WABT");
    let mut ran = 0usize;
    for &(s, want) in CASES {
        let got = exec_case(s, want).expect("WABT present");
        assert_eq!(
            got, want,
            "executed WASM {s:?}.swapcase() = {got:?} but CPython = {want:?}"
        );
        ran += 1;
    }
    assert_eq!(ran, CASES.len());
    eprintln!(
        "PMAT-1201: all {ran} inputs executed in WABT and value-matched CPython \
         (both directions: 'hello'->'HELLO', 'WORLD'->'world', per-byte 'aZ'->'Az', \
         digits/'_'/punctuation pass-through, empty ''->'').\n\
         --- emitted swap WAT (emit_module over meta-HIR) ---\n{wat}"
    );
}

#[test]
fn non_ascii_swapcase_traps_not_silent() {
    // The honest ASCII-only boundary: `.swapcase()` over a string with a non-ASCII
    // byte TRAPS (`unreachable`) rather than silently returning a wrongly-flipped
    // string. CPython would fold ("Café".swapcase() == "cAFÉ"), but this scalar
    // lane carries no case table — so it aborts, NEVER a silent divergence.
    let wat =
        emit_module(&swapcase_module("swap")).expect("swapcase program lowers through emit_module");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1201: skipping non-ASCII trap witness — WABT absent. The trap \
             (`unreachable`) is asserted structurally in \
             `swapcase_emits_helper_call_heap_and_trap`."
        );
        return;
    }
    // "Café" — 'é' is 0xC3 0xA9, the first byte >= 0x80 -> the helper traps. (The
    // ASCII prefix "Caf" is flipped in-place first; the trap fires on the 'é'.)
    let s = "Café";
    // One call is enough to hit the trap on the non-ASCII byte. `n_out` is
    // irrelevant — the run must FAIL.
    let witness = build_witness_wat(&wat, "swap", s, 1);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-swapcase-trap-{}",
        std::process::id()
    ));
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
    // The non-ASCII byte must drive the `unreachable` trap — either a non-zero
    // exit or an explicit "unreachable executed" in the interp output. NEVER a
    // clean run returning a folded/unchanged string.
    let trapped =
        !run.status.success() || stdout.contains("unreachable") || stderr.contains("unreachable");
    assert!(
        trapped,
        "'{s}'.swapcase() must TRAP on the non-ASCII byte (honest ASCII-only \
         boundary), not run clean: status={:?} stdout={stdout:?} stderr={stderr:?}",
        run.status
    );
    eprintln!(
        "PMAT-1201: '{s}'.swapcase() correctly TRAPPED on the non-ASCII 'é' byte \
         (0xC3) — honest ASCII-only boundary, never a silent wrongly-flipped string."
    );
}
