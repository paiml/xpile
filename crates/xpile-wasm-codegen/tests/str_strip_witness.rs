//! PMAT-1205 — EXECUTED `s.strip()` / `s.lstrip()` / `s.rstrip()` witness for the
//! native WASM EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! Each op materialises a NEW heap string with the leading (`strip`/`lstrip`)
//! and/or trailing (`strip`/`rstrip`) run of ASCII whitespace removed, the retained
//! byte range copied VERBATIM. They join the allocating string-method family
//! (`removeprefix` / `removesuffix` / `replace` / `zfill` / `upper` / `lower` /
//! `capitalize` / `swapcase` / `title`) on the WASM lane: an `Expr::StrMethod {
//! op: Strip | LStrip | RStrip }` in a string position lowers via the single
//! allocating `$__wasm_str_strip` helper (calls `$__alloc` + `memory.copy`, rides
//! the `needs_heap` gate). All three share it — `left` / `right` i32 flags select
//! which ends to trim, exactly like the `$__wasm_str_upper_lower` `up` flag.
//!
//! ## The real programs
//!
//! ```python
//! def strip(s: str) -> str:  return s.strip()
//! def lstrip(s: str) -> str: return s.lstrip()
//! def rstrip(s: str) -> str: return s.rstrip()
//! ```
//!
//! ## Whitespace set = the isspace-family ASCII set
//!
//! `(0x09..=0x0D) | (0x1C..=0x20)` — tab/LF/VT/FF/CR, FS/GS/RS/US, space. CPython's
//! `str.strip()` and `str.isspace()` share `Py_UNICODE_ISSPACE`, and the Rust/Ruchy
//! lanes emit the same set (`char::is_whitespace() || '\u{1c}'..='\u{1f}'`), so this
//! is byte-exact against CPython for ASCII (verified vs python3 when this slice
//! landed — incl. the FS/GS/RS/US `0x1c`–`0x1f` chars, which ARE stripped).
//!
//! ## Boundary-only ASCII trap — MORE capable than the whole-string case-fold posture
//!
//! Unlike `.upper()`/`.title()` (which scan and trap on EVERY non-ASCII byte), strip
//! only READS the leading/trailing bytes whose whitespace-ness it must decide. A
//! read byte `< 0x80` that is not whitespace is a definitive CONTENT boundary (stop,
//! correct). A read byte `>= 0x80` is UNDECIDABLE (could be a Unicode-whitespace lead
//! CPython would strip, or a non-whitespace char it would keep) — so it `unreachable`
//! TRAPS, never a silent wrong answer. INTERIOR bytes are copied verbatim and never
//! examined, so an interior non-ASCII char with ASCII ends does NOT trap
//! (`"a€b".strip() == "a€b"` is byte-exact — the `interior_non_ascii_*` case proves
//! it). `non_ascii_boundary_strip_traps_not_silent` proves the trap.
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel (`strip` /
//! `lstrip` / `rstrip`) takes an `i32` (the `s` param base-pointer, preloaded into a
//! `(data …)` region below `LITERAL_BASE`) and returns the constructed string's
//! `i32` base-pointer. The witness adds zero-arg wrappers that push the constant
//! `S_ADDR`, call the kernel, and read back the result: `run_len` (the i32
//! byte-count header @ result+0) and a `run_byte_i` family (each re-runs the kernel
//! and `i32.load8_u`s payload byte `i`).
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT path
//! lowers + carries the `$__wasm_str_strip` helper + call + heap + trap) on a host
//! without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and the
/// bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0, UTF-8
/// bytes @ base+8).
const S_ADDR: i32 = 16;

/// `true` iff `b` is an ASCII whitespace byte for `str.strip()` — CPython's
/// `Py_UNICODE_ISSPACE` restricted to ASCII: `0x09`–`0x0D` (tab/LF/VT/FF/CR),
/// `0x1C`–`0x1F` (FS/GS/RS/US), `0x20` (space).
fn is_ascii_strip_ws(b: u8) -> bool {
    (0x09..=0x0d).contains(&b) || (0x1c..=0x20).contains(&b)
}

/// ASCII `str.strip()` / `.lstrip()` / `.rstrip()` reference (`left`/`right` pick the
/// ends). Byte-exact against CPython for ASCII-BOUNDARY inputs (interior bytes are
/// copied verbatim; the WASM lane traps on a non-ASCII BOUNDARY byte). Used both to
/// PIN the expectations and cross-check them.
fn py_strip(s: &str, left: bool, right: bool) -> String {
    let bytes = s.as_bytes();
    let mut start = 0usize;
    let mut end = bytes.len();
    if left {
        while start < end && is_ascii_strip_ws(bytes[start]) {
            start += 1;
        }
    }
    if right {
        while end > start && is_ascii_strip_ws(bytes[end - 1]) {
            end -= 1;
        }
    }
    String::from_utf8(bytes[start..end].to_vec()).expect("ASCII-boundary slice is valid UTF-8")
}

/// The three strip ops and their kernel names / trim-end flags.
const OPS: &[(StrMethodOp, &str, bool, bool)] = &[
    (StrMethodOp::Strip, "strip", true, true),
    (StrMethodOp::LStrip, "lstrip", true, false),
    (StrMethodOp::RStrip, "rstrip", false, true),
];

/// (input, op, CPython `input.<op>()`) — pinned to the exact CPython ground truth
/// (verified with python3 when this slice landed). ASCII-BOUNDARY inputs (interior
/// may be non-ASCII, e.g. `"a€b"` — those bytes are copied, never examined). Chosen
/// to stress: distinct left/right/both trimming, the empty + all-whitespace edges,
/// the FS/GS/RS/US + VT/FF whitespace chars, interior whitespace preserved,
/// non-whitespace ASCII NOT stripped, and interior non-ASCII passing through.
const CASES: &[(&str, StrMethodOp, &str)] = &[
    ("  hello  ", StrMethodOp::Strip, "hello"),    // both ends
    ("  hello  ", StrMethodOp::LStrip, "hello  "), // left only — trailing ws kept
    ("  hello  ", StrMethodOp::RStrip, "  hello"), // right only — leading ws kept
    ("\t\n hi \r\n", StrMethodOp::Strip, "hi"),    // mixed tab/LF/CR whitespace
    ("   ", StrMethodOp::Strip, ""),               // all whitespace -> empty
    ("", StrMethodOp::Strip, ""),                  // empty -> empty (no payload)
    ("no_ws", StrMethodOp::Strip, "no_ws"),        // nothing to strip
    ("\x1c\x1dab\x1e\x1f", StrMethodOp::Strip, "ab"), // FS/GS/RS/US ARE whitespace
    ("\x0b\x0chi\x0b\x0c", StrMethodOp::Strip, "hi"), // VT/FF ARE whitespace
    (" a b c ", StrMethodOp::Strip, "a b c"),      // INTERIOR spaces preserved
    ("xxhelloxx", StrMethodOp::Strip, "xxhelloxx"), // 'x' is not whitespace
    ("\ta\t", StrMethodOp::LStrip, "a\t"),         // lstrip keeps the trailing tab
    ("\ta\t", StrMethodOp::RStrip, "\ta"),         // rstrip keeps the leading tab
    ("a€b", StrMethodOp::Strip, "a€b"),            // INTERIOR non-ASCII, ASCII ends -> no trap
];

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def <name>(s: str) -> str: return s.<op>()`.
fn strip_module(name: &str, op: StrMethodOp) -> Module {
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

/// A stable per-(input, op) hash so distinct cases get distinct temp dirs (the same
/// input under strip/lstrip/rstrip must not collide).
fn case_hash(s: &str, kernel: &str) -> u64 {
    let mut h: u64 = 1469598103934665603; // FNV-1a offset basis
    for &b in s.as_bytes().iter().chain(b"|").chain(kernel.as_bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

/// Splice the preloaded `s` `(data …)` region + zero-arg read-back exports
/// (`run_len` / `run_byte_i`) onto the emitted module, before its closing `)`.
/// `kernel` = the emitted kernel function name; `n_out` = the expected result byte
/// length.
fn build_witness_wat(kernel_wat: &str, kernel: &str, s: &str, n_out: usize) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1205 witness: preload the s param (below LITERAL_BASE)\n");
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

/// Lower `<kernel>(s) = s.<op>()`, run it in WABT with `s` preloaded, and
/// reconstruct the stripped string. `None` when WABT is absent (caller skips the
/// value assertion). Asserts the WASM byte length matches CPython.
fn exec_case(s: &str, op: StrMethodOp, kernel: &str, expected: &str) -> Option<String> {
    let kernel_wat = emit_module(&strip_module(kernel, op)).expect("strip program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let n_out = expected.len();
    let wat = build_witness_wat(&kernel_wat, kernel, s, n_out);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-strip-{}-{:016x}",
        std::process::id(),
        case_hash(s, kernel)
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
    Some(String::from_utf8(bytes).expect("constructed stripped string bytes are valid UTF-8"))
}

#[test]
fn cpython_strip_ground_truth_is_pinned() {
    // Every pin equals the ASCII strip reference for its op. Verified vs python3 when
    // this slice landed.
    for &(s, op, want) in CASES {
        let (_, kernel, left, right) = OPS.iter().find(|o| o.0 == op).expect("known op");
        assert_eq!(py_strip(s, *left, *right), want, "pinned {s:?}.{kernel}()");
        // The result must be a byte-range of the input (strip removes ends only,
        // never rewrites — unlike the case-fold family).
        assert!(
            s.as_bytes()
                .windows(want.len().max(1))
                .any(|w| w == want.as_bytes())
                || want.is_empty(),
            "{want:?} must be a contiguous byte-slice of {s:?}"
        );
    }
    // The fixture must EXERCISE all three trim directions asymmetrically on the SAME
    // input (else strip could masquerade as lstrip or rstrip):
    assert!(CASES
        .iter()
        .any(|&(s, op, w)| s == "  hello  " && op == StrMethodOp::Strip && w == "hello"));
    assert!(CASES
        .iter()
        .any(|&(s, op, w)| s == "  hello  " && op == StrMethodOp::LStrip && w == "hello  "));
    assert!(CASES
        .iter()
        .any(|&(s, op, w)| s == "  hello  " && op == StrMethodOp::RStrip && w == "  hello"));
    // The all-whitespace -> empty edge (result length 0, no payload).
    assert!(CASES
        .iter()
        .any(|&(s, op, w)| s == "   " && op == StrMethodOp::Strip && w.is_empty()));
    // The FS/GS/RS/US (0x1c-0x1f) whitespace chars ARE stripped (a set narrower than
    // this would get "\x1c\x1dab\x1e\x1f".strip() wrong).
    assert!(CASES
        .iter()
        .any(|&(s, op, w)| s == "\x1c\x1dab\x1e\x1f" && op == StrMethodOp::Strip && w == "ab"));
    // Interior non-ASCII with ASCII ends passes through byte-exact (proves
    // boundary-only, not whole-string, trapping).
    assert!(CASES
        .iter()
        .any(|&(s, op, w)| s == "a€b" && op == StrMethodOp::Strip && w == "a€b"));
}

#[test]
fn strip_emits_helper_call_heap_and_trap() {
    // CONSTRUCT assertion (holds with or without WABT): each of the three programs
    // lowers through the production emitter, carrying the SHARED helper + call +
    // heap + the honest ASCII-only trap. The helper's two flag params distinguish it
    // from the single-param case-fold helpers.
    for &(op, kernel, _, _) in OPS {
        let wat = emit_module(&strip_module(kernel, op))
            .unwrap_or_else(|_| panic!("the s.{kernel}() program must lower"));
        assert!(
            wat.contains(
                "(func $__wasm_str_strip (param $s i32) (param $left i32) (param $right i32) (result i32)"
            ),
            "the shared strip helper (with left/right flags) must be emitted for {kernel}:\n{wat}"
        );
        assert!(
            wat.contains("call $__wasm_str_strip"),
            "${kernel} must call the strip helper:\n{wat}"
        );
        assert!(
            wat.contains(&format!("(func ${kernel} (param $s i32) (result i32)")),
            "str return → i32 result (heap pointer), str param → i32 for {kernel}:\n{wat}"
        );
        // Materialising a trimmed string → needs the bump heap.
        assert!(
            wat.contains("(func $__alloc"),
            "{kernel} needs the bump heap:\n{wat}"
        );
        // The honest ASCII-only boundary: a non-ASCII boundary byte traps.
        assert!(
            wat.contains("unreachable"),
            "the strip helper must trap (unreachable) on a non-ASCII boundary byte for {kernel}:\n{wat}"
        );
    }
    // The flag consts must differ per op: strip=(1,1), lstrip=(1,0), rstrip=(0,1).
    // Each lowering pushes `left` then `right` immediately before the call.
    let strip_wat = emit_module(&strip_module("strip", StrMethodOp::Strip)).unwrap();
    assert!(
        strip_wat.contains("i32.const 1\n    i32.const 1\n    call $__wasm_str_strip"),
        "strip must push left=1, right=1:\n{strip_wat}"
    );
    let lstrip_wat = emit_module(&strip_module("lstrip", StrMethodOp::LStrip)).unwrap();
    assert!(
        lstrip_wat.contains("i32.const 1\n    i32.const 0\n    call $__wasm_str_strip"),
        "lstrip must push left=1, right=0:\n{lstrip_wat}"
    );
    let rstrip_wat = emit_module(&strip_module("rstrip", StrMethodOp::RStrip)).unwrap();
    assert!(
        rstrip_wat.contains("i32.const 0\n    i32.const 1\n    call $__wasm_str_strip"),
        "rstrip must push left=0, right=1:\n{rstrip_wat}"
    );
}

#[test]
fn real_strip_program_executes_in_wasm_and_matches_cpython() {
    if !wasm_runtime_available() {
        // Still exercise the EMIT path for every op.
        for &(op, kernel, _, _) in OPS {
            emit_module(&strip_module(kernel, op)).expect("strip program lowers");
        }
        eprintln!(
            "PMAT-1205: skipping EXECUTED strip witness — WABT (wat2wasm / \
             wasm-interp) absent. The programs lowered through emit_module (asserted \
             in `strip_emits_helper_call_heap_and_trap`); a box with WABT also runs \
             them and asserts the CONSTRUCTED string == CPython."
        );
        return;
    }
    eprintln!("PMAT-1205: running EXECUTED s.strip()/.lstrip()/.rstrip() witness via WABT");
    let mut ran = 0usize;
    for &(s, op, want) in CASES {
        let (_, kernel, _, _) = OPS.iter().find(|o| o.0 == op).expect("known op");
        let got = exec_case(s, op, kernel, want).expect("WABT present");
        assert_eq!(
            got, want,
            "executed WASM {s:?}.{kernel}() = {got:?} but CPython = {want:?}"
        );
        ran += 1;
    }
    assert_eq!(ran, CASES.len());
    eprintln!(
        "PMAT-1205: all {ran} inputs executed in WABT and value-matched CPython \
         (all three trim directions on '  hello  '; all-ws->empty; FS/GS/RS/US + \
         VT/FF whitespace; interior spaces preserved; interior non-ASCII 'a€b' \
         byte-exact with NO trap)."
    );
}

#[test]
fn non_ascii_boundary_strip_traps_not_silent() {
    // The honest ASCII-only boundary: `.strip()` over a string whose content
    // BOUNDARY (after the leading whitespace) is a non-ASCII byte TRAPS
    // (`unreachable`) rather than silently returning a wrong run. CPython would strip
    // the surrounding spaces (" é ".strip() == "é"), but this scalar lane cannot
    // decide whether the 0xC3 lead byte begins a Unicode-whitespace char it should
    // strip or a content char it should keep — so it aborts, NEVER a silent divergence.
    let wat = emit_module(&strip_module("strip", StrMethodOp::Strip))
        .expect("strip program lowers through emit_module");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1205: skipping non-ASCII trap witness — WABT absent. The trap \
             (`unreachable`) is asserted structurally in \
             `strip_emits_helper_call_heap_and_trap`."
        );
        return;
    }
    // " é " — the leading space is stripped, then the scan reaches 'é' (0xC3 0xA9);
    // the 0xC3 lead byte (>= 0x80) is undecidable -> the helper traps.
    let s = " é ";
    let witness = build_witness_wat(&wat, "strip", s, 1);
    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-str-strip-trap-{}", std::process::id()));
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
    let trapped =
        !run.status.success() || stdout.contains("unreachable") || stderr.contains("unreachable");
    assert!(
        trapped,
        "'{s}'.strip() must TRAP on the non-ASCII boundary byte (honest ASCII-only \
         boundary), not run clean: status={:?} stdout={stdout:?} stderr={stderr:?}",
        run.status
    );
    eprintln!(
        "PMAT-1205: '{s}'.strip() correctly TRAPPED on the non-ASCII 'é' boundary \
         byte (0xC3) after stripping the leading space — honest ASCII-only boundary, \
         never a silent wrong run."
    );
}
