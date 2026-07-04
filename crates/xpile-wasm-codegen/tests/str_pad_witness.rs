//! PMAT-1209 — EXECUTED `s.rjust(w)` / `s.ljust(w)` / `s.center(w)` witness for
//! the native WASM EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The space-pad family materialises a NEW heap string equal to `s` padded with
//! ASCII space (`0x20`) to `width` CODE POINTS. All three share the single
//! allocating `$__wasm_str_pad(s, w, mode)` helper (mode 0 = rjust / left-pad,
//! 1 = ljust / right-pad, 2 = center); an `Expr::StrMethod { op: RJust|LJust|Center
//! }` in a string position lowers via that helper (calls `$__alloc` +
//! `memory.fill` + `memory.copy`, rides the `needs_heap` gate) and uses
//! `$__wasm_str_charlen` for the width math.
//!
//! ## The real programs
//!
//! ```python
//! def pad(s: str, w: int) -> str:
//!     return s.rjust(w)   # / s.ljust(w) / s.center(w)
//! ```
//!
//! ## Why it is char-exact (no Unicode trap, unlike `.upper()`/`.title()`)
//!
//! The total pad is `max(0, width - charlen(s))`. The pad bytes are pure 1-byte
//! ASCII spaces inserted at code-point boundaries (the very start and/or very end),
//! and the rest of `s` is copied byte-for-byte, so NO payload byte is ever
//! inspected or folded — the result is CORRECT for any valid UTF-8
//! (`"café".rjust(6)` == `"  café"`, `"é".center(3)` == `" é "`). Center splits
//! the pad with CPython's exact parity bias `left = pad/2 + (pad & width & 1)`,
//! `right = pad - left` (so `"ab".center(5)` == `"  ab "`, left-heavy on an odd
//! total, NOT Rust `{:^}`'s right-bias).
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel `$pad`
//! takes an `i32` (the `s` param base-pointer, preloaded into a `(data …)` region
//! below `LITERAL_BASE`) and an `i64` (the width), returning the constructed
//! string's `i32` base-pointer. The witness adds only zero-arg wrappers that push
//! the constant `S_ADDR` + `w`, call `$pad`, and read back the result: `run_len`
//! (the i32 byte-count header @ result+0) and a `run_byte_i` family (each re-runs
//! `$pad` and `i32.load8_u`s payload byte `i`).
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_str_pad` helper + call) on a host without
//! WABT.

use std::collections::BTreeSet;
use std::process::{Command, Stdio};

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and the
/// bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0, UTF-8
/// bytes @ base+8).
const S_ADDR: i32 = 16;

/// (op, input, width, CPython `getattr(input, op)(width)`) — pinned to the exact
/// CPython ground truth (verified with python3 when this slice landed). Covers all
/// three pad directions, the width <= len copy, the empty string, non-ASCII
/// char-exactness, negative width, and the center parity bias on both even and odd
/// total pad.
const CASES: &[(StrMethodOp, &str, i64, &str)] = &[
    // rjust — all pad on the LEFT.
    (StrMethodOp::RJust, "ab", 5, "   ab"),
    (StrMethodOp::RJust, "ab", 1, "ab"), // width < len → copy
    (StrMethodOp::RJust, "", 3, "   "),  // empty → three spaces
    (StrMethodOp::RJust, "café", 6, "  café"), // NON-ASCII, char-exact
    (StrMethodOp::RJust, "ab", -1, "ab"), // negative width → copy
    // ljust — all pad on the RIGHT.
    (StrMethodOp::LJust, "ab", 5, "ab   "),
    (StrMethodOp::LJust, "x", 0, "x"),         // width 0 → copy
    (StrMethodOp::LJust, "café", 6, "café  "), // NON-ASCII, char-exact
    // center — CPython parity bias left = pad/2 + (pad & width & 1).
    (StrMethodOp::Center, "ab", 5, "  ab "), // odd pad 3 → 2 left / 1 right
    (StrMethodOp::Center, "abc", 6, " abc  "), // odd pad 3, even width → 1 left / 2 right
    (StrMethodOp::Center, "ab", 4, " ab "),  // even pad 2 → 1 / 1
    (StrMethodOp::Center, "hi", 1, "hi"),    // width < len → copy
    (StrMethodOp::Center, "é", 3, " é "),    // NON-ASCII, char-exact (é is 2 bytes)
    (StrMethodOp::Center, "abcd", 9, "   abcd  "), // odd pad 5, odd width → 3 / 2
    (StrMethodOp::Center, "a", 4, " a  "),   // odd pad 3, even width → 1 / 2
];

fn op_name(op: StrMethodOp) -> &'static str {
    match op {
        StrMethodOp::RJust => "rjust",
        StrMethodOp::LJust => "ljust",
        StrMethodOp::Center => "center",
        _ => unreachable!("witness only exercises the pad family"),
    }
}

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def pad(s: str, w: int) -> str: return s.<op>(w)`.
fn pad_module(op: StrMethodOp) -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op,
        args: vec![Expr::Ident("w".into())],
    };
    let f = Function {
        name: "pad".into(),
        params: vec![
            Param {
                name: "s".into(),
                ty: Type::Str,
                mutable: false,
            },
            Param {
                name: "w".into(),
                ty: Type::I64,
                mutable: false,
            },
        ],
        return_type: Type::Str,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "pad_program".into(),
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
/// `n_out` = the expected result byte length.
fn build_witness_wat(kernel_wat: &str, s: &str, w: i64, n_out: usize) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1209 witness: preload the s param (below LITERAL_BASE)\n");
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
           i32.const {S_ADDR}\n    i64.const {w}\n    call $pad\n    i32.load)\n"
    ));
    for i in 0..n_out {
        wat.push_str(&format!(
            "  (func (export \"run_byte_{i}\") (result i32)\n    \
               i32.const {S_ADDR}\n    i64.const {w}\n    call $pad\n    \
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

/// Lower `pad(s, w) = s.<op>(w)`, run it in WABT with `s` preloaded, and
/// reconstruct the padded string. `None` when WABT is absent (caller skips the
/// value assertion). Asserts the WASM byte length matches CPython.
fn exec_pad(op: StrMethodOp, s: &str, w: i64, expected: &str) -> Option<String> {
    let kernel_wat = emit_module(&pad_module(op)).expect("pad program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let n_out = expected.len();
    let wat = build_witness_wat(&kernel_wat, s, w, n_out);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-pad-{}-{}-{}",
        std::process::id(),
        op_name(op),
        s.len().wrapping_mul(131).wrapping_add(w as usize)
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("pad.wat");
    let wasm_path = dir.join("pad.wasm");
    std::fs::write(&wat_path, &wat).expect("write wat");
    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed for {s:?}.{}({w}):\n{}\n---WAT---\n{wat}",
        op_name(op),
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
        "wasm-interp run failed for {s:?}.{}({w}): stdout={stdout:?} stderr={:?}",
        op_name(op),
        String::from_utf8_lossy(&run.stderr)
    );
    let got_len = parse_i32_export(&stdout, "run_len");
    assert_eq!(
        got_len as usize,
        n_out,
        "{s:?}.{}({w}) byte length: WASM={got_len} CPython={n_out}\nWAT:\n{wat}",
        op_name(op)
    );
    let mut bytes = Vec::with_capacity(n_out);
    for i in 0..n_out {
        bytes.push(parse_i32_export(&stdout, &format!("run_byte_{i}")) as u8);
    }
    Some(String::from_utf8(bytes).expect("constructed pad string bytes are valid UTF-8"))
}

#[test]
fn cpython_pad_ground_truth_is_pinned() {
    // The pinned CPython forms the witness value-matches. rjust/ljust/center pad
    // with ASCII space to `width` code points; a width no larger than the current
    // length (incl. a negative width) is a plain copy. (Rust's `format!` width has
    // a right-bias center that DIVERGES from CPython, so the ground truth is pinned
    // here, verified vs python3 when this slice landed.)
    for &(op, s, w, expected) in CASES {
        let cl = |t: &str| t.chars().count();
        // char length of the result is max(width, char-len(s)).
        assert_eq!(
            cl(expected),
            cl(s).max(if w < 0 { 0 } else { w as usize }),
            "pinned {s:?}.{}({w}) = {expected:?} must have char-len max(width, len(s))",
            op_name(op)
        );
        // the pad is pure ASCII space; the non-space code points come only from s,
        // in order (the pad only prepends/appends spaces, never reorders/edits s).
        assert!(
            expected.trim_matches(' ').contains(s.trim_matches(' '))
                || s.trim_matches(' ').is_empty(),
            "pinned {s:?}.{}({w}) = {expected:?} must contain s unchanged between the space pads",
            op_name(op)
        );
    }
}

#[test]
fn pad_emits_pad_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): each pad program lowers
    // through the production emitter, carrying the shared helper + call + heap.
    for op in [StrMethodOp::RJust, StrMethodOp::LJust, StrMethodOp::Center] {
        let wat = emit_module(&pad_module(op))
            .unwrap_or_else(|e| panic!("the s.{}(w) program must lower: {e:?}", op_name(op)));
        assert!(
            wat.contains(
                "(func $__wasm_str_pad (param $s i32) (param $w i64) (param $mode i32) (result i32)"
            ),
            "the pad helper must be emitted for {}:\n{wat}",
            op_name(op)
        );
        assert!(
            wat.contains("call $__wasm_str_pad"),
            "$pad must call the pad helper for {}:\n{wat}",
            op_name(op)
        );
        assert!(
            wat.contains("(func $pad (param $s i32) (param $w i64) (result i32)"),
            "str return → i32 result (heap pointer), int width → i64 param for {}:\n{wat}",
            op_name(op)
        );
        // Materialising a padded string → needs the bump heap + the char helper.
        assert!(
            wat.contains("(func $__alloc"),
            "pad needs the bump heap for {}:\n{wat}",
            op_name(op)
        );
        assert!(
            wat.contains("call $__wasm_str_charlen"),
            "pad's width math needs the char-count helper for {}:\n{wat}",
            op_name(op)
        );
    }
    // The `mode` selector const distinguishes the three ops at the call site.
    let rjust = emit_module(&pad_module(StrMethodOp::RJust)).unwrap();
    let ljust = emit_module(&pad_module(StrMethodOp::LJust)).unwrap();
    let center = emit_module(&pad_module(StrMethodOp::Center)).unwrap();
    assert!(
        rjust.contains("i32.const 0\n    call $__wasm_str_pad"),
        "rjust must push mode 0:\n{rjust}"
    );
    assert!(
        ljust.contains("i32.const 1\n    call $__wasm_str_pad"),
        "ljust must push mode 1:\n{ljust}"
    );
    assert!(
        center.contains("i32.const 2\n    call $__wasm_str_pad"),
        "center must push mode 2:\n{center}"
    );
}

#[test]
fn two_arg_fill_char_form_is_refused() {
    // PMAT-1209: the shared space-pad helper pads with a fixed ASCII space; the
    // 2-arg fill-char form `s.rjust(w, fill)` is refused honestly rather than
    // silently padding with spaces (which would diverge from a non-space fill).
    for op in [StrMethodOp::RJust, StrMethodOp::LJust, StrMethodOp::Center] {
        let body = Expr::StrMethod {
            recv: Box::new(Expr::Ident("s".into())),
            op,
            args: vec![Expr::Ident("w".into()), Expr::LitStr("*".into())],
        };
        let f = Function {
            name: "pad".into(),
            params: vec![
                Param {
                    name: "s".into(),
                    ty: Type::Str,
                    mutable: false,
                },
                Param {
                    name: "w".into(),
                    ty: Type::I64,
                    mutable: false,
                },
            ],
            return_type: Type::Str,
            body: Block {
                stmts: vec![],
                trailing_return: body,
            },
        };
        let m = Module {
            name: "pad_fill".into(),
            source_lang: SourceLang::Rust,
            items: vec![Item::Function(f)],
            ffi_boundaries: Vec::new(),
        };
        let err = emit_module(&m).expect_err("2-arg fill-char pad must be refused");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("fill-char"),
            "the refusal must name the fill-char form for {}: {msg}",
            op_name(op)
        );
    }
}

#[test]
fn real_pad_program_executes_in_wasm_and_matches_cpython() {
    // Lower each op once (for the emit-path skip note) so the CONSTRUCT path is
    // exercised even without WABT.
    let _ = emit_module(&pad_module(StrMethodOp::RJust)).expect("pad program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1209: skipping EXECUTED pad witness — WABT (wat2wasm / \
             wasm-interp) absent. The pad programs lowered through emit_module \
             (asserted in `pad_emits_pad_helper_and_call`); a box with WABT also \
             runs them and asserts the CONSTRUCTED string == CPython."
        );
        return;
    }
    eprintln!("PMAT-1209: running EXECUTED s.rjust/ljust/center(w) witness via WABT");
    let mut ran = 0usize;
    for &(op, s, w, expected) in CASES {
        let got = exec_pad(op, s, w, expected).expect("WABT present");
        assert_eq!(
            got,
            expected,
            "executed WASM {s:?}.{}({w}) = {got:?} but CPython = {expected:?}",
            op_name(op)
        );
        ran += 1;
    }
    assert_eq!(ran, CASES.len());
    eprintln!(
        "PMAT-1209: all {ran} pad cases (rjust/ljust/center) executed in WABT and \
         value-matched CPython (incl. non-ASCII 'café'->'  café' char-exact, empty \
         ''->'   ', negative width copy, and the center parity bias \
         'ab'.center(5)->'  ab ' / 'abcd'.center(9)->'   abcd  ')."
    );
}

// ---------------------------------------------------------------------------
// PMAT-1215 — RANDOMIZED DIFFERENTIAL fuzz for the width-arg pad family vs LIVE
// python3. The pinned `CASES` above nail the headline forms, but a hand-picked
// table only catches the inputs the author enumerated. This fuzz drives
// rjust/ljust/center over a deterministic corpus of ASCII *and* MULTIBYTE
// strings × a systematic width sweep straddling the width-vs-charlen boundary
// and every center parity combination (even/odd pad × even/odd width),
// comparing each EXECUTED WASM result byte-for-byte against the value python3
// actually returns.
//
// Why this is the missing witness: the pad family is char-exact with NO trap
// arm, so multibyte inputs are IN SCOPE (unlike the ASCII-only case-fold fuzz in
// `str_family_fuzz_witness.rs` (PMAT-1207), whose ops trap on non-ASCII, and
// which never took a width arg). So this is the FIRST randomized *multibyte*
// width-math differential — exactly where a code-point-vs-byte width bug or a
// center parity off-by-one would surface on a width the 15 pinned cases never
// touch. python3 is the literal oracle → zero reimplementation risk.
// ---------------------------------------------------------------------------

/// Deterministic 64-bit LCG (no `rand` → `cargo deny` clean, byte-stable corpus).
struct Lcg(u64);
impl Lcg {
    fn next_u64(&mut self) -> u64 {
        // Numerical-Recipes / PCG-style multiplier + odd increment.
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() >> 33) as usize % n
    }
}

/// Fuzz corpus for the pad family: curated ASCII edges of both char-len
/// parities, multibyte code points of every UTF-8 width (2-byte é/Greek, 3-byte
/// €/CJK, 4-byte astral emoji), interior/edge/all-space cases, and a handful of
/// deterministic LCG-built ASCII strings. Deduped + ordered (BTreeSet) so the
/// corpus is byte-stable run to run.
fn pad_corpus() -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    for s in [
        "",
        "a",
        "ab",
        "abc",
        "abcd",
        "abcde", // ASCII, char-len 0..5 (both parities)
        "café",
        "é",
        "αβ",
        "αβγ", // 2-byte code points (é / Greek)
        "€",
        "a€b",
        "日本",
        "日本語", // 3-byte code points (€ / CJK)
        "🎉",
        "🎉x",
        "x🎉y", // 4-byte astral emoji
        "π=3",
        " a ",
        "  ", // mixed + interior/edge/all-space
    ] {
        set.insert(s.to_string());
    }
    // A few deterministic ASCII strings so the corpus is not purely curated.
    const ALPHABET: &[u8] = b"abAB01 _";
    let mut rng = Lcg(0xD1B54A32D192ED03); // arbitrary fixed seed
    for _ in 0..6 {
        let len = rng.below(6); // 0..=5
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(ALPHABET[rng.below(ALPHABET.len())]);
        }
        set.insert(String::from_utf8(bytes).expect("ASCII bytes are valid UTF-8"));
    }
    set.into_iter().collect()
}

/// The width sweep for a string of char-length `l`: `-1` (negative → copy), `l`
/// (width == charlen boundary → no pad), `l+1..=l+3` (walk the center parity
/// bias through every (pad-parity × width-parity) combination across even/odd-
/// length strings), and `l+6` (a larger pad). Crucially the sweep is anchored to
/// the CODE-POINT count, so a byte-length width bug on a multibyte input lands
/// off-by-`(byte_len - char_len)` and is caught.
fn width_sweep(l: usize) -> Vec<i64> {
    let l = l as i64;
    vec![-1, l, l + 1, l + 2, l + 3, l + 6]
}

/// `true` iff `python3` is invocable (the differential oracle).
fn python3_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Lowercase hex of a byte slice — ASCII-safe wire format for the oracle (a
/// space / control byte in the payload would otherwise break the line protocol).
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Decode lowercase hex to bytes.
fn unhex(h: &str) -> Vec<u8> {
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).expect("valid hex pair"))
        .collect()
}

/// The python3 pad oracle: one task per line (`<op> <hex> <width>`) fed to a
/// single python3 process; returns `getattr(s, op)(int(width))` per line as
/// lowercase `<hex>` (UTF-8-encoded, since a padded result may carry a multibyte
/// payload). The empty input keeps a stable 3-token layout (`op  width`, the
/// middle hex empty), so `split(' ')` is always exactly three fields.
fn python_pad_oracle(tasks: &[(&str, String, i64)]) -> Vec<String> {
    let script = r#"
import sys
out = []
for ln in sys.stdin.read().splitlines():
    if not ln:
        continue
    op, h, w = ln.split(' ')
    s = bytes.fromhex(h).decode('utf-8')
    r = getattr(s, op)(int(w))
    out.append(r.encode('utf-8').hex())
sys.stdout.write('\n'.join(out))
"#;
    let mut input = String::new();
    for (op, h, w) in tasks {
        input.push_str(&format!("{op} {h} {w}\n"));
    }
    let out = Command::new("python3")
        .arg("-c")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .take()
                .expect("python3 stdin")
                .write_all(input.as_bytes())?;
            child.wait_with_output()
        })
        .expect("run python3 pad oracle");
    assert!(
        out.status.success(),
        "python3 pad oracle failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.to_string())
        .collect()
}

fn op_from_name(n: &str) -> StrMethodOp {
    match n {
        "rjust" => StrMethodOp::RJust,
        "ljust" => StrMethodOp::LJust,
        "center" => StrMethodOp::Center,
        _ => unreachable!("pad fuzz only exercises rjust/ljust/center"),
    }
}

#[test]
fn pad_family_matches_cpython_over_randomized_multibyte_corpus() {
    // The EMIT path must lower for every pad op regardless of WABT/python3 (holds
    // on free CI too) — a construct smoke.
    for op in [StrMethodOp::RJust, StrMethodOp::LJust, StrMethodOp::Center] {
        emit_module(&pad_module(op))
            .unwrap_or_else(|e| panic!("pad {} must lower: {e:?}", op_name(op)));
    }
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1215: skipping EXECUTED pad fuzz — WABT (wat2wasm / wasm-interp) \
             absent. The pad programs lowered through emit_module above; a box with \
             WABT + python3 also runs the corpus and value-matches CPython."
        );
        return;
    }
    if !python3_available() {
        eprintln!("PMAT-1215: skipping pad fuzz — python3 (the oracle) absent.");
        return;
    }

    // Build the (op, string, width) case set, deduped + ordered for stability.
    let corpus = pad_corpus();
    let mut cases: BTreeSet<(&str, String, i64)> = BTreeSet::new();
    for s in &corpus {
        let l = s.chars().count();
        for w in width_sweep(l) {
            for op in ["rjust", "ljust", "center"] {
                cases.insert((op, s.clone(), w));
            }
        }
    }
    let cases: Vec<(&str, String, i64)> = cases.into_iter().collect();
    eprintln!(
        "PMAT-1215: randomized pad differential over {} inputs → {} (op, input, width) \
         cases vs live python3",
        corpus.len(),
        cases.len()
    );

    // 1) One batched python3 call for the whole ground truth.
    let tasks: Vec<(&str, String, i64)> = cases
        .iter()
        .map(|(op, s, w)| (*op, hex(s.as_bytes()), *w))
        .collect();
    let oracle = python_pad_oracle(&tasks);
    assert_eq!(
        oracle.len(),
        cases.len(),
        "python3 pad oracle returned {} results for {} tasks",
        oracle.len(),
        cases.len()
    );

    // 2) Execute each case in WABT and diff byte-for-byte against CPython.
    let mut ran = 0usize;
    for ((op_name_s, s, w), want_hex) in cases.iter().zip(oracle.iter()) {
        let expected =
            String::from_utf8(unhex(want_hex)).expect("python pad result is valid UTF-8");
        let op = op_from_name(op_name_s);
        let got = exec_pad(op, s, *w, &expected).expect("WABT present");
        assert_eq!(
            got, expected,
            "executed WASM {s:?}.{op_name_s}({w}) = {got:?} but CPython = {expected:?}"
        );
        ran += 1;
    }
    assert_eq!(ran, cases.len());
    eprintln!(
        "PMAT-1215: all {ran} (op, input, width) pad cases executed in WABT and \
         value-matched live python3 — no silent divergence in the pad family width \
         math (incl. multibyte char-vs-byte width + center parity across all widths)."
    );
}
