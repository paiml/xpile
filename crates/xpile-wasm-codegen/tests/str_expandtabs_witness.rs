//! PMAT-1219 — EXECUTED `s.expandtabs()` / `s.expandtabs(tabsize)` witness for the
//! native WASM EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! `expandtabs` materialises a NEW heap string with each tab (`\t`, `0x09`)
//! replaced by the ASCII spaces (`0x20`) needed to reach the next multiple of
//! `tabsize`, the COLUMN counted in **code points** and reset to `0` after each
//! `\n` (`0x0a`) or `\r` (`0x0d`). An `Expr::StrMethod { op: ExpandTabs }` in a
//! string position lowers via the allocating `$__wasm_str_expandtabs(s, ts)` helper
//! (calls `$__alloc` + `memory.fill` + `memory.copy`, rides the `needs_heap` gate).
//!
//! ## The real programs
//!
//! ```python
//! def et(s: str) -> str:          # bare form — tabsize defaults to 8
//!     return s.expandtabs()
//!
//! def et(s: str, n: int) -> str:  # explicit tabsize
//!     return s.expandtabs(n)
//! ```
//!
//! ## Why it is char-exact (no Unicode trap, unlike `.upper()`/`.title()`)
//!
//! Only the ASCII tab/newline bytes are ever interpreted; every OTHER code point
//! (identified by its UTF-8 lead-byte length, as in `$__wasm_str_reverse`) is copied
//! VERBATIM and counts as ONE column, so a multibyte payload round-trips unchanged
//! (`"é\t".expandtabs(4)` == `"é   "`, `"日本\tx".expandtabs(4)` == `"日本  x"`),
//! matching CPython and the rust/ruchy `.chars()` walk — no Unicode table, no
//! non-ASCII trap. This is the second no-trap payload-MOVING transform after reverse
//! (PMAT-1213). A `tabsize <= 0` drops tabs (0 spaces), matching CPython.
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel `$et` takes
//! an `i32` (the `s` param base-pointer, preloaded into a `(data …)` region below
//! `LITERAL_BASE`) and — for the explicit form only — an `i64` (the tabsize),
//! returning the constructed string's `i32` base-pointer. The witness adds only
//! zero-arg wrappers that push `S_ADDR` (+ the tabsize for the explicit form), call
//! `$et`, and read back `run_len` (the i32 byte-count header @ result+0) and a
//! `run_byte_i` family (each re-runs `$et` and `i32.load8_u`s payload byte `i`).
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT path
//! lowers + carries the `$__wasm_str_expandtabs` helper + call) on a host without
//! WABT. A second gate re-derives every pinned expectation against live `python3`,
//! so the pins cannot silently rot.

use std::process::{Command, Stdio};

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and the
/// bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0, UTF-8
/// bytes @ base+8).
const S_ADDR: i32 = 16;

/// `(input, tabsize, expected)` — the CPython ground truth for the EXPLICIT
/// `s.expandtabs(tabsize)` form, pinned (verified with `python3` when this slice
/// landed AND re-derived live in `pinned_cases_match_live_cpython`). Covers the
/// tab-to-next-multiple fill, the empty result, `tabsize == 0` (tabs dropped), the
/// `tabsize == 1` degenerate, multiple tabs, the `\n`/`\r` column reset, and — the
/// point of a no-trap transform — non-ASCII / astral / combining-mark payloads that
/// are copied verbatim and each count as ONE column.
const CASES: &[(&str, i64, &str)] = &[
    ("\t", 8, "        "),
    ("a\t", 8, "a       "),
    ("ab\tc", 8, "ab      c"),
    ("abcdefgh\t", 8, "abcdefgh        "), // tab at a multiple → a full tabsize run
    ("\t", 0, ""),                         // tabsize 0 → tab dropped
    ("a\tb", 0, "ab"),                     // tabsize 0 → tab dropped, payload kept
    ("a\tb\tc", 4, "a   b   c"),
    ("a\tb", 1, "a b"),
    ("a\n\tb", 8, "a\n        b"),       // LF resets the column
    ("a\r\tb", 8, "a\r        b"),       // CR resets the column
    ("ab\t\tc", 4, "ab      c"),         // two tabs
    ("é\t", 4, "é   "),                  // 2-byte payload, 1 column → 3 spaces (char-exact)
    ("€€€\t", 4, "€€€ "),                // three 3-byte cols → 1 space
    ("日本\tx", 4, "日本  x"),           // two 3-byte cols → 2 spaces, then x
    ("🎉\t", 4, "🎉   "),                // one 4-byte astral col → 3 spaces
    ("e\u{301}\t", 4, "e\u{301}  "),     // e + combining acute = 2 cols → 2 spaces
    ("café no tabs", 8, "café no tabs"), // no tab → verbatim (multibyte untouched)
    ("x\ty\tz", 3, "x  y  z"),
    ("\ta", 2, "  a"),
    ("a\tb\r\nc\td", 4, "a   b\r\nc   d"), // CRLF resets; a tab on each line
];

/// `(input, expected)` — the CPython ground truth for the BARE `s.expandtabs()` form
/// (tabsize defaults to 8). Confirms the 0-arg default injection (`i64.const 8`).
const DEFAULT_CASES: &[(&str, &str)] = &[
    ("\t", "        "),
    ("a\tb", "a       b"),
    ("ab\tcd\te", "ab      cd      e"),
    ("café\tx", "café    x"), // multibyte payload under the default tabsize
];

/// Build the meta-HIR `Module` the Python frontend produces. `tabsize == Some(n)` is
/// the explicit `def et(s, n): return s.expandtabs(n)` (an `n: int` param); `None` is
/// the bare `def et(s): return s.expandtabs()` (no extra param, the emit injects the
/// default `i64.const 8`).
fn et_module(tabsize: Option<i64>) -> Module {
    let args = if tabsize.is_some() {
        vec![Expr::Ident("n".into())]
    } else {
        vec![]
    };
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::ExpandTabs,
        args,
    };
    let mut params = vec![Param {
        name: "s".into(),
        ty: Type::Str,
        mutable: false,
    }];
    if tabsize.is_some() {
        params.push(Param {
            name: "n".into(),
            ty: Type::I64,
            mutable: false,
        });
    }
    let f = Function {
        name: "et".into(),
        params,
        return_type: Type::Str,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "et_program".into(),
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
/// (`run_len` / `run_byte_i`) onto the emitted module, before its closing `)`. The
/// call sequence pushes `S_ADDR` and — for the explicit form (`tabsize == Some`) —
/// the tabsize `i64`, matching the emitted `$et` arity. `n_out` = expected byte len.
fn build_witness_wat(kernel_wat: &str, s: &str, tabsize: Option<i64>, n_out: usize) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let arg = match tabsize {
        Some(n) => format!("\n    i64.const {n}"),
        None => String::new(),
    };
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1219 witness: preload the s param (below LITERAL_BASE)\n");
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
           i32.const {S_ADDR}{arg}\n    call $et\n    i32.load)\n"
    ));
    for i in 0..n_out {
        wat.push_str(&format!(
            "  (func (export \"run_byte_{i}\") (result i32)\n    \
               i32.const {S_ADDR}{arg}\n    call $et\n    \
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

/// Lower `et(s[, n]) = s.expandtabs([n])`, run it in WABT with `s` preloaded, and
/// reconstruct the expanded string. `None` when WABT is absent (caller skips the
/// value assertion). Asserts the WASM byte length matches CPython.
fn exec_expandtabs(s: &str, tabsize: Option<i64>, expected: &str) -> Option<String> {
    let kernel_wat = emit_module(&et_module(tabsize)).expect("expandtabs program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let n_out = expected.len();
    let wat = build_witness_wat(&kernel_wat, s, tabsize, n_out);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-expandtabs-{}-{:016x}",
        std::process::id(),
        case_hash(s, tabsize)
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("et.wat");
    let wasm_path = dir.join("et.wasm");
    std::fs::write(&wat_path, &wat).expect("write wat");
    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed for {s:?}.expandtabs({tabsize:?}):\n{}\n---WAT---\n{wat}",
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
        "wasm-interp run failed for {s:?}.expandtabs({tabsize:?}): stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    // A per-export trap prints `run_len() => error: …` yet exits 0 — a no-trap op
    // must never do that, so treat it as a hard failure.
    assert!(
        !stdout.contains("=> error:"),
        "wasm-interp TRAPPED on {s:?}.expandtabs({tabsize:?}) (no-trap op):\n{stdout}"
    );
    let got_len = parse_i32_export(&stdout, "run_len");
    assert_eq!(
        got_len as usize, n_out,
        "{s:?}.expandtabs({tabsize:?}) byte length: WASM={got_len} CPython={n_out}\nWAT:\n{wat}",
    );
    let mut bytes = Vec::with_capacity(n_out);
    for i in 0..n_out {
        bytes.push(parse_i32_export(&stdout, &format!("run_byte_{i}")) as u8);
    }
    Some(String::from_utf8(bytes).expect("constructed expandtabs string bytes are valid UTF-8"))
}

/// A stable per-(input, tabsize) hash so distinct cases get distinct temp dirs.
fn case_hash(s: &str, tabsize: Option<i64>) -> u64 {
    let mut h: u64 = 1469598103934665603; // FNV-1a offset basis
    let tag = tabsize.map(|n| n.to_le_bytes()).unwrap_or([0xff; 8]);
    for &b in s.as_bytes().iter().chain(tag.iter()) {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

/// Ask live `python3` for `s.expandtabs([n])`, returned as a hex-encoded UTF-8
/// string, or `None` when python3 is absent. Hex avoids any shell/quoting hazard.
fn python_expandtabs(s: &str, tabsize: Option<i64>) -> Option<String> {
    let in_hex: String = s.bytes().map(|b| format!("{b:02x}")).collect();
    let call = match tabsize {
        Some(n) => format!("s.expandtabs({n})"),
        None => "s.expandtabs()".to_string(),
    };
    let script = format!(
        "import binascii,sys\n\
         s=binascii.unhexlify(sys.argv[1]).decode('utf-8')\n\
         sys.stdout.write(({call}).encode('utf-8').hex())\n"
    );
    let out = Command::new("python3")
        .arg("-c")
        .arg(&script)
        .arg(&in_hex)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn python3_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[test]
fn expandtabs_emits_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): both forms lower through the
    // production emitter, carrying the helper + call + heap.
    for tabsize in [Some(4i64), None] {
        let wat = emit_module(&et_module(tabsize))
            .unwrap_or_else(|e| panic!("the s.expandtabs({tabsize:?}) program must lower: {e:?}"));
        assert!(
            wat.contains(
                "(func $__wasm_str_expandtabs (param $s i32) (param $ts i64) (result i32)"
            ),
            "the expandtabs helper must be emitted for {tabsize:?}:\n{wat}"
        );
        assert!(
            wat.contains("call $__wasm_str_expandtabs"),
            "$et must call the expandtabs helper for {tabsize:?}:\n{wat}"
        );
        // Materialising a heap string → needs the bump allocator.
        assert!(
            wat.contains("(func $__alloc"),
            "expandtabs needs the bump heap for {tabsize:?}:\n{wat}"
        );
    }
    // The bare form injects the default tabsize (`i64.const 8`) at the call; the
    // explicit form pushes the `n` param instead.
    let bare = emit_module(&et_module(None)).unwrap();
    assert!(
        bare.contains("i64.const 8\n    call $__wasm_str_expandtabs"),
        "the bare `.expandtabs()` must default the tabsize to 8:\n{bare}"
    );
    assert!(
        bare.contains("(func $et (param $s i32) (result i32)"),
        "the bare form takes only the s param:\n{bare}"
    );
    let explicit = emit_module(&et_module(Some(4))).unwrap();
    assert!(
        explicit.contains("(func $et (param $s i32) (param $n i64) (result i32)"),
        "the explicit form takes s + the tabsize:\n{explicit}"
    );
}

#[test]
fn pinned_cases_match_live_cpython() {
    // The pins in CASES / DEFAULT_CASES must equal what live python3 returns — so a
    // future CPython or an author typo cannot silently rot the ground truth. Skips
    // cleanly when python3 is absent (free CI).
    if !python3_available() {
        eprintln!("PMAT-1219: skipping the live-CPython pin check — python3 absent.");
        return;
    }
    for &(s, ts, expected) in CASES {
        let got = python_expandtabs(s, Some(ts)).expect("python3 expandtabs");
        assert_eq!(
            got,
            hex(expected.as_bytes()),
            "pinned {s:?}.expandtabs({ts}) = {expected:?} disagrees with live python3"
        );
    }
    for &(s, expected) in DEFAULT_CASES {
        let got = python_expandtabs(s, None).expect("python3 expandtabs");
        assert_eq!(
            got,
            hex(expected.as_bytes()),
            "pinned bare {s:?}.expandtabs() = {expected:?} disagrees with live python3"
        );
    }
}

#[test]
fn wasm_expandtabs_matches_cpython() {
    // EMIT path exercised above regardless of WABT. The EXECUTED differential runs
    // only with WABT present.
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1219: skipping EXECUTED expandtabs witness — WABT (wat2wasm / \
             wasm-interp) absent. Every case lowered through emit_module; a box with \
             WABT value-matches CPython byte-for-byte."
        );
        // Still assert the EMIT path lowers for every case.
        for &(s, ts, expected) in CASES {
            assert!(exec_expandtabs(s, Some(ts), expected).is_none());
        }
        return;
    }
    let mut ran = 0usize;
    for &(s, ts, expected) in CASES {
        let got = exec_expandtabs(s, Some(ts), expected)
            .expect("WABT present → executed result available");
        assert_eq!(
            got, expected,
            "{s:?}.expandtabs({ts}): WASM={got:?} CPython={expected:?}"
        );
        ran += 1;
    }
    for &(s, expected) in DEFAULT_CASES {
        let got =
            exec_expandtabs(s, None, expected).expect("WABT present → executed result available");
        assert_eq!(
            got, expected,
            "bare {s:?}.expandtabs(): WASM={got:?} CPython={expected:?}"
        );
        ran += 1;
    }
    eprintln!(
        "PMAT-1219: all {ran} expandtabs (input × tabsize) cases executed in WABT and \
         matched CPython byte-for-byte — tabs expand to the next code-point-column \
         multiple, the column resets on \\n/\\r, tabsize<=0 drops tabs, and any \
         multibyte payload (incl. astral + combining marks) is copied verbatim with NO \
         non-ASCII trap."
    );
}
