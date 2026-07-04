//! PMAT-1185 — EXECUTED `s.upper()` / `s.lower()` witness for the native WASM
//! EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! `s.upper()` / `s.lower()` materialise a NEW heap string with every ASCII
//! letter case-flipped. They join the allocating string-method family
//! (`removeprefix` / `removesuffix` / `replace` / `zfill`) on the WASM lane: an
//! `Expr::StrMethod { op: Upper | Lower }` in a string position lowers via the
//! allocating `$__wasm_str_upper_lower` helper (calls `$__alloc` + `i32.store8`,
//! rides the `needs_heap` gate). Both ops share the one helper — an `up` i32
//! immediate (1 = upper, 0 = lower) selects the direction.
//!
//! ## The real programs
//!
//! ```python
//! def up(s: str) -> str:
//!     return s.upper()
//!
//! def lo(s: str) -> str:
//!     return s.lower()
//! ```
//!
//! ## ASCII-only, with an HONEST runtime boundary (unlike `.zfill`)
//!
//! Python's `str.upper()` / `str.lower()` do FULL Unicode case folding
//! (`"café".upper() == "CAFÉ"`), which needs a case table this scalar lane does
//! not carry. So the helper case-flips only the ASCII letters and, on the FIRST
//! byte `>= 0x80` (any byte of a non-ASCII code point in valid UTF-8), executes
//! `unreachable` — a TRAP, exactly like the `index` / `rindex` ValueError
//! siblings. It NEVER passes a non-ASCII byte through unchanged, so it never
//! silently diverges from CPython: for pure-ASCII `s` the result is char-exact,
//! and for a non-ASCII `s` it aborts rather than returning a wrong string. The
//! `cpython_case_ground_truth_is_pinned` test documents that ASCII boundary and
//! `non_ascii_upper_traps_not_silent` proves the trap (no silent divergence).
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernels `$up` /
//! `$lo` take an `i32` (the `s` param base-pointer, preloaded into a `(data …)`
//! region below `LITERAL_BASE`) and return the constructed string's `i32`
//! base-pointer. The witness adds only zero-arg wrappers that push the constant
//! `S_ADDR`, call the kernel, and read back the result: `run_len` (the i32
//! byte-count header @ result+0) and a `run_byte_i` family (each re-runs the
//! kernel and `i32.load8_u`s payload byte `i`).
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_str_upper_lower` helper + call) on a host
//! without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and
/// the bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0,
/// UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;

/// (input, CPython `input.upper()`, CPython `input.lower()`) — pinned to the
/// exact CPython ground truth (verified with python3). ASCII-only inputs, since
/// the WASM lane traps on non-ASCII (see `non_ascii_upper_traps_not_silent`).
const CASES: &[(&str, &str, &str)] = &[
    ("hello", "HELLO", "hello"), // the headline: lower -> upper
    ("WORLD", "WORLD", "world"), // upper -> lower
    ("MixedCase42", "MIXEDCASE42", "mixedcase42"), // digits pass through unchanged
    ("", "", ""),                // empty -> empty (no payload)
    ("a", "A", "a"),             // single char, both directions
    ("Z", "Z", "z"),             // boundary letter 'Z'/'z'
    ("hi there!", "HI THERE!", "hi there!"), // space + punctuation unchanged
    ("123-456", "123-456", "123-456"), // no letters at all -> plain copy
    ("aA_zZ", "AA_ZZ", "aa_zz"), // '_' (0x5f, between 'Z' and 'a') untouched
    ("gGkK", "GGKK", "ggkk"),    // interior letters
];

/// Build the meta-HIR `Module` the Python frontend produces for a single
/// `def NAME(s: str) -> str: return s.OP()` (OP = `upper`/`lower`).
fn case_module(name: &str, op: StrMethodOp) -> Module {
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
/// `kernel` = the emitted kernel function name (`up` / `lo`); `n_out` = the
/// expected result byte length.
fn build_witness_wat(kernel_wat: &str, kernel: &str, s: &str, n_out: usize) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1185 witness: preload the s param (below LITERAL_BASE)\n");
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

/// Lower `f(s) = s.OP()`, run it in WABT with `s` preloaded, and reconstruct the
/// case-flipped string. `None` when WABT is absent (caller skips the value
/// assertion). Asserts the WASM byte length matches CPython.
fn exec_case(kernel: &str, op: StrMethodOp, s: &str, expected: &str) -> Option<String> {
    let kernel_wat = emit_module(&case_module(kernel, op)).expect("case program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let n_out = expected.len();
    let wat = build_witness_wat(&kernel_wat, kernel, s, n_out);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-case-{}-{}-{}",
        kernel,
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
        "wat2wasm failed for {s:?}.{kernel}():\n{}\n---WAT---\n{wat}",
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
        "wasm-interp run failed for {s:?}.{kernel}(): stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    let got_len = parse_i32_export(&stdout, "run_len");
    assert_eq!(
        got_len as usize, n_out,
        "{s:?}.{kernel}() byte length: WASM={got_len} CPython={n_out}\nWAT:\n{wat}"
    );
    let mut bytes = Vec::with_capacity(n_out);
    for i in 0..n_out {
        bytes.push(parse_i32_export(&stdout, &format!("run_byte_{i}")) as u8);
    }
    Some(String::from_utf8(bytes).expect("constructed case string bytes are valid UTF-8"))
}

#[test]
fn cpython_case_ground_truth_is_pinned() {
    // The pinned CPython forms the witness value-matches. On ASCII, upper/lower
    // flip only 'A'..'Z' <-> 'a'..'z'; every other byte (digits, '_', space,
    // punctuation) passes through, and the code-point length is unchanged. (These
    // pins were verified vs python3 when this slice landed.)
    for &(s, up, lo) in CASES {
        assert_eq!(s.to_ascii_uppercase(), up, "pinned {s:?}.upper()");
        assert_eq!(s.to_ascii_lowercase(), lo, "pinned {s:?}.lower()");
        // ASCII-only: byte length == char length == unchanged across the flip.
        assert_eq!(s.len(), up.len(), "upper preserves byte length for {s:?}");
        assert_eq!(s.len(), lo.len(), "lower preserves byte length for {s:?}");
        assert!(s.is_ascii(), "witness inputs are ASCII (non-ASCII traps)");
    }
}

#[test]
fn case_emits_upper_lower_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): both programs lower
    // through the production emitter, carrying the shared helper + call + heap.
    for (kernel, op, up_flag) in [("up", StrMethodOp::Upper, 1), ("lo", StrMethodOp::Lower, 0)] {
        let wat = emit_module(&case_module(kernel, op))
            .expect("the s.upper()/s.lower() program must lower through emit_module");
        assert!(
            wat.contains(
                "(func $__wasm_str_upper_lower (param $s i32) (param $up i32) (result i32)"
            ),
            "the upper/lower helper must be emitted:\n{wat}"
        );
        assert!(
            wat.contains("call $__wasm_str_upper_lower"),
            "${kernel} must call the upper/lower helper:\n{wat}"
        );
        assert!(
            wat.contains(&format!(
                "i32.const {up_flag}\n    call $__wasm_str_upper_lower"
            )),
            "${kernel} must pass the `up` flag {up_flag}:\n{wat}"
        );
        assert!(
            wat.contains(&format!("(func ${kernel} (param $s i32) (result i32)")),
            "str return → i32 result (heap pointer), str param → i32:\n{wat}"
        );
        // Materialising a case-flipped string → needs the bump heap.
        assert!(
            wat.contains("(func $__alloc"),
            "upper/lower needs the bump heap:\n{wat}"
        );
        // The honest ASCII-only boundary: a non-ASCII byte traps.
        assert!(
            wat.contains("unreachable"),
            "the helper must trap (unreachable) on a non-ASCII byte:\n{wat}"
        );
    }
}

#[test]
fn real_case_program_executes_in_wasm_and_matches_cpython() {
    let up_wat = emit_module(&case_module("up", StrMethodOp::Upper))
        .expect("upper program lowers through emit_module");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1185: skipping EXECUTED upper/lower witness — WABT (wat2wasm / \
             wasm-interp) absent. Both programs lowered through emit_module \
             (asserted in `case_emits_upper_lower_helper_and_call`); a box with \
             WABT also runs them and asserts the CONSTRUCTED string == CPython."
        );
        return;
    }
    eprintln!("PMAT-1185: running EXECUTED s.upper()/s.lower() witness via WABT");
    let mut ran = 0usize;
    for &(s, up, lo) in CASES {
        let got_up = exec_case("up", StrMethodOp::Upper, s, up).expect("WABT present");
        assert_eq!(
            got_up, up,
            "executed WASM {s:?}.upper() = {got_up:?} but CPython = {up:?}"
        );
        let got_lo = exec_case("lo", StrMethodOp::Lower, s, lo).expect("WABT present");
        assert_eq!(
            got_lo, lo,
            "executed WASM {s:?}.lower() = {got_lo:?} but CPython = {lo:?}"
        );
        ran += 1;
    }
    assert_eq!(ran, CASES.len());
    eprintln!(
        "PMAT-1185: all {ran} inputs executed both directions in WABT and \
         value-matched CPython (incl. 'MixedCase42'->'MIXEDCASE42'/'mixedcase42', \
         digits/'_'/punctuation pass-through, empty ''->'').\n\
         --- emitted up WAT (emit_module over meta-HIR) ---\n{up_wat}"
    );
}

#[test]
fn non_ascii_upper_traps_not_silent() {
    // The honest ASCII-only boundary: `.upper()` / `.lower()` over a string with
    // a non-ASCII byte TRAPS (`unreachable`) rather than silently returning an
    // un-folded string. CPython would fold ("café".upper() == "CAFÉ"), but this
    // scalar lane carries no case table — so it aborts, NEVER a silent divergence.
    let up_wat = emit_module(&case_module("up", StrMethodOp::Upper))
        .expect("upper program lowers through emit_module");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1185: skipping non-ASCII trap witness — WABT absent. The trap \
             (`unreachable`) is asserted structurally in \
             `case_emits_upper_lower_helper_and_call`."
        );
        return;
    }
    // "café" — 'é' is 0xC3 0xA9, the first byte >= 0x80 -> the helper traps.
    let s = "café";
    // Build a witness that just reads run_len (one call is enough to hit the trap
    // on the non-ASCII byte). `n_out` is irrelevant — the run must FAIL.
    let wat = build_witness_wat(&up_wat, "up", s, 1);
    let dir = std::env::temp_dir().join(format!("xpile-wasm-str-case-trap-{}", std::process::id()));
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
    // clean run returning a folded/unchanged string.
    let trapped =
        !run.status.success() || stdout.contains("unreachable") || stderr.contains("unreachable");
    assert!(
        trapped,
        "'{s}'.upper() must TRAP on the non-ASCII byte (honest ASCII-only \
         boundary), not run clean: status={:?} stdout={stdout:?} stderr={stderr:?}",
        run.status
    );
    eprintln!(
        "PMAT-1185: '{s}'.upper() correctly TRAPPED on the non-ASCII 'é' byte \
         (0xC3) — honest ASCII-only boundary, never a silent un-folded string."
    );
}
