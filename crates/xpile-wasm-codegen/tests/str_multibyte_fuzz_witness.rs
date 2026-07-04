//! PMAT-1217 — a randomized DIFFERENTIAL witness for the native-WASM string ops
//! that are CHAR-EXACT with **NO trap arm on non-ASCII**, over a randomized
//! **multibyte UTF-8** corpus, against LIVE CPython (`python3`).
//!
//! ## The gap this closes
//!
//! Two sibling randomized fuzz witnesses already exist, but BOTH are ASCII-bounded
//! by construction:
//!   - `str_family_fuzz_witness` (PMAT-1207) fuzzes upper/lower/…/strip/is* over an
//!     **ASCII-ONLY** corpus ("every byte `< 0x80` BY CONSTRUCTION"), precisely so
//!     the case-fold + predicate families never hit their non-ASCII trap.
//!   - `str_pad_witness` (PMAT-1215) fuzzes the width-arg PAD family, also ASCII.
//!
//! The per-op reverse (`str_reverse_witness`) and strip (`str_strip_witness`)
//! witnesses DO carry multibyte cases — but they are HAND-PICKED (`café`, `日本`,
//! `a€b`, `🎉x`). Hand-picked tables catch the inputs the author enumerated; they do
//! NOT catch a divergence hiding in a multibyte input nobody thought of (a 4-byte
//! astral char adjacent to a 2-byte char, a combining mark, a mixed-width run). This
//! witness closes that gap: a DETERMINISTIC randomized corpus of **valid multibyte
//! UTF-8** strings, diffed for EVERY (string × op) against the value `python3`
//! actually returns.
//!
//! ## The ops under test — the no-trap non-ASCII family
//!
//! Only the ops that are byte-exact WITHOUT a non-ASCII trap belong here:
//!   - `reverse` (`s[::-1]`, PMAT-1213) — the first no-trap payload-MOVING transform.
//!     Reverses by CODE POINT (UTF-8 lead byte gives each code point's length), moving
//!     each multi-byte code point WHOLE. Correct for ANY valid UTF-8 → the corpus is
//!     unconstrained (any mix of 1/2/3/4-byte code points, combining marks included).
//!   - `strip` / `lstrip` / `rstrip` (PMAT-1205) — **BOUNDARY-ONLY** ASCII trap: they
//!     examine only the bytes they must judge (from each end inward until the first
//!     non-whitespace byte), so interior non-ASCII passes through untouched and NEVER
//!     traps (`"a€b".strip() == "a€b"`). The corpus for these is constrained so the
//!     first non-whitespace byte from EACH end is ASCII (an ASCII letter wraps the
//!     multibyte interior), which is exactly the no-trap domain — the multibyte payload
//!     lives strictly in the interior and is copied verbatim.
//!
//! The case-fold family (upper/lower/…) and the `is*` predicates are deliberately NOT
//! here: those TRAP on a non-ASCII byte (they need a Unicode table xpile honestly does
//! not carry), and that trap posture is already witnessed per-op
//! (`non_ascii_*_traps_not_silent`). This witness is the breadth complement for the
//! *no-trap* ops only: exhaustive-ish multibyte exact-match, not edge-picked.
//!
//! ## Gating
//!
//! Runs the EXECUTED differential only when BOTH `wat2wasm`/`wasm-interp` (WABT) AND
//! `python3` are present. On free CI (no WABT) it skips cleanly after still exercising
//! the EMIT path for every op — same posture as every sibling witness.

use std::collections::BTreeSet;
use std::process::{Command, Stdio};

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and the
/// bump heap (>= 1024). Length-prefixed: i32 BYTE count @ base+0, UTF-8 bytes @ base+8.
const S_ADDR: i32 = 16;

/// The reverse op — an unconstrained multibyte corpus (any valid UTF-8). `(op, kernel,
/// CPython-form)`. `reverse` is `s[::-1]`, not a method, so the oracle special-cases it.
const REVERSE: (StrMethodOp, &str, &str) = (StrMethodOp::Reverse, "reverse", "reverse");

/// The boundary-only-trap strip family — an ASCII-boundary/multibyte-interior corpus.
const STRIP_FAMILY: &[(StrMethodOp, &str, &str)] = &[
    (StrMethodOp::Strip, "strip", "strip"),
    (StrMethodOp::LStrip, "lstrip", "lstrip"),
    (StrMethodOp::RStrip, "rstrip", "rstrip"),
];

/// The multibyte alphabet the reverse corpus samples: a rich mix of every UTF-8 width.
/// 1-byte ASCII, 2-byte (Latin-1 / Greek), 3-byte (CJK / symbols / `€`), 4-byte (astral
/// / emoji), plus a lone COMBINING acute (`\u{301}`) — the adversarial case for
/// code-point (not grapheme) reversal, which CPython `s[::-1]` also splits.
const MB_ALPHABET: &[char] = &[
    'a', 'Z', '0', ' ', '_', // 1-byte
    'é', 'ñ', 'ü', 'α', 'β', 'Ω', '©', // 2-byte
    '€', '日', '本', '中', '→', '♥', // 3-byte
    '🎉', '𝕏', '😀', '𐍈',       // 4-byte
    '\u{301}', // combining acute (2-byte, no base) — code-point-reversal adversary
];

/// The ASCII letters that wrap the strip corpus interior, guaranteeing the first
/// non-whitespace byte from EACH end is ASCII (`< 0x80`) so strip/lstrip/rstrip stay in
/// their no-trap domain. The multibyte payload lives strictly between them.
const STRIP_ASCII_ENDS: &[char] = &['a', 'Z', 'x', '7', '!'];

/// The ASCII whitespace the strip corpus prefixes/suffixes with (the chars CPython
/// `str.strip()` removes AND the xpile isspace set: space/tab/LF/CR/VT/FF + FS..US).
const STRIP_WS: &[char] = &[
    ' ', '\t', '\n', '\r', '\u{0b}', '\u{0c}', '\u{1c}', '\u{1f}',
];

/// Curated multibyte reverse edges — the shapes the per-op witness pins, plus a
/// combining mark and a dense mixed-width run.
const REVERSE_EDGES: &[&str] = &[
    "",
    "a",
    "é",
    "€",
    "🎉",
    "café",
    "日本",
    "a€b",
    "🎉x",
    "αβγ",
    "a€🎉本z",
    "e\u{301}",      // e + combining acute → reversed splits the cluster
    "🎉😀𝕏𐍈",        // four 4-byte astral code points
    "aα本🎉Zé中→♥𐍈", // every width interleaved
];

/// Curated strip edges — ASCII-boundary, multibyte-interior; interior whitespace and
/// interior non-ASCII must survive; leading/trailing ASCII whitespace must be removed.
const STRIP_EDGES: &[&str] = &[
    "a€中b",           // no surrounding ws → unchanged
    "  a€中b  ",       // ASCII ws both ends → trimmed to a€中b
    "\ta本b\t",        // tab ws → trimmed
    "x🎉y",            // astral interior, no ws
    "  a b c  ",       // interior spaces preserved
    "\u{1c}a→b\u{1f}", // FS/US ws boundary
    " a€ 🎉 中b ",     // interior ws + interior multibyte both preserved
    "aéz",             // ASCII ends, 2-byte interior, no ws
];

/// A fixed-seed 64-bit LCG (deterministic — no `rand`, so `cargo deny` is unaffected and
/// the corpus is byte-stable run to run).
struct Lcg(u64);
impl Lcg {
    fn next_u64(&mut self) -> u64 {
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

/// Build the deterministic multibyte reverse corpus: curated edges + LCG-generated
/// strings of 0..=7 code points over [`MB_ALPHABET`], de-duplicated.
fn reverse_corpus() -> Vec<String> {
    let mut set: BTreeSet<String> = REVERSE_EDGES.iter().map(|s| s.to_string()).collect();
    let mut rng = Lcg(0xD1B54A32D192ED03); // distinct seed from PMAT-1207
    for _ in 0..40 {
        let len = rng.below(8); // 0..=7 code points
        let mut s = String::new();
        for _ in 0..len {
            s.push(MB_ALPHABET[rng.below(MB_ALPHABET.len())]);
        }
        set.insert(s);
    }
    set.into_iter().collect()
}

/// Build the deterministic strip corpus: curated edges + LCG-generated
/// `ws* ASCII (multibyte-interior) ASCII ws*` strings, de-duplicated. The ASCII letters
/// bracketing the multibyte interior guarantee the no-trap domain (first non-ws byte from
/// each end is `< 0x80`); the multibyte payload is strictly interior.
fn strip_corpus() -> Vec<String> {
    let mut set: BTreeSet<String> = STRIP_EDGES.iter().map(|s| s.to_string()).collect();
    let mut rng = Lcg(0x2545F4914F6CDD1D); // distinct seed
    for _ in 0..40 {
        let mut s = String::new();
        // 0..=3 leading ASCII whitespace.
        for _ in 0..rng.below(4) {
            s.push(STRIP_WS[rng.below(STRIP_WS.len())]);
        }
        // ASCII left boundary.
        s.push(STRIP_ASCII_ENDS[rng.below(STRIP_ASCII_ENDS.len())]);
        // 0..=5 multibyte interior chars (may include interior whitespace, which strip
        // must PRESERVE since it is not at a boundary).
        for _ in 0..rng.below(6) {
            s.push(MB_ALPHABET[rng.below(MB_ALPHABET.len())]);
        }
        // ASCII right boundary.
        s.push(STRIP_ASCII_ENDS[rng.below(STRIP_ASCII_ENDS.len())]);
        // 0..=3 trailing ASCII whitespace.
        for _ in 0..rng.below(4) {
            s.push(STRIP_WS[rng.below(STRIP_WS.len())]);
        }
        set.insert(s);
    }
    set.into_iter().collect()
}

/// Build the meta-HIR `Module` for `def <name>(s: str) -> str: return s.<op>()`.
fn op_module(name: &str, op: StrMethodOp) -> Module {
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

/// A stable per-(input, op) hash so distinct cases get distinct temp dirs.
fn case_hash(s: &str, kernel: &str) -> u64 {
    let mut h: u64 = 1469598103934665603; // FNV-1a offset basis
    for &b in s.as_bytes().iter().chain(b"|").chain(kernel.as_bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

/// Splice the preloaded `s` region + a transform's readback exports (`run_len` +
/// `run_byte_i` for i in 0..`n_out`) onto the emitted module, then assemble + run in
/// WABT and reconstruct the result string. `expected` supplies the expected byte length
/// (so a length divergence is caught even though only `n_out` readback exports exist).
fn wasm_transform(
    kernel_wat: &str,
    kernel: &str,
    s: &str,
    expected: &str,
) -> Result<String, String> {
    let n_out = expected.len();
    let close = kernel_wat.rfind(')').expect("module close paren");
    let mut wat = String::from(&kernel_wat[..close]);
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

    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-mb-fuzz-{}-{:016x}",
        std::process::id(),
        case_hash(s, kernel)
    ));
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    let wat_path = dir.join("case.wat");
    let wasm_path = dir.join("case.wasm");
    std::fs::write(&wat_path, &wat).map_err(|e| format!("write wat: {e}"))?;
    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .map_err(|e| format!("spawn wat2wasm: {e}"))?;
    if !assemble.status.success() {
        return Err(format!(
            "wat2wasm FAILED for {s:?}.{kernel}():\n{}\n---WAT---\n{wat}",
            String::from_utf8_lossy(&assemble.stderr)
        ));
    }
    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .map_err(|e| format!("spawn wasm-interp: {e}"))?;
    if !run.status.success() {
        return Err(format!(
            "wasm-interp TRAPPED on {s:?}.{kernel}() (must never trap on the no-trap domain): \
             stdout={:?} stderr={:?}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    // `wasm-interp --run-all-exports` reports a per-export trap as
    // `run_len() => error: unreachable executed` and still exits 0, so the status
    // check above will not catch it — detect the trap here and report it as a clean
    // Err (a trap on the no-trap domain is a real mismatch, not a parse failure).
    if stdout.contains("=> error:") {
        return Err(format!(
            "wasm-interp TRAPPED on {s:?}.{kernel}() (must never trap on the no-trap domain):\n{stdout}"
        ));
    }
    let got_len = parse_i32(&stdout, "run_len") as usize;
    if got_len != n_out {
        return Err(format!(
            "{s:?}.{kernel}() WASM byte-length {got_len} != CPython {n_out}"
        ));
    }
    let mut out = Vec::with_capacity(n_out);
    for i in 0..n_out {
        out.push(parse_i32(&stdout, &format!("run_byte_{i}")) as u8);
    }
    String::from_utf8(out).map_err(|e| format!("{s:?}.{kernel}() bytes not UTF-8: {e}"))
}

/// Parse a `name() => i32:<value>` line from `wasm-interp --run-all-exports`.
fn parse_i32(stdout: &str, name: &str) -> i32 {
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

/// `true` iff `python3` is invocable.
fn python3_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Lowercase hex of a byte slice — the wire format for the `python3` oracle (ASCII-safe
/// for the multibyte / control bytes that would break a `-c` arg or a line).
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

/// The `python3` ground-truth oracle. Feeds one task per line (`<op> <hex>`) to a single
/// `python3` process; each result comes back as `OK:<hex>`. `python3` decodes each hex →
/// UTF-8 `str`, applies the op (`s[::-1]` for `reverse`, else `getattr(s, op)()`), and
/// re-encodes to UTF-8 hex — so there is zero reimplementation risk.
fn python_oracle(tasks: &[(&str, String)]) -> Vec<String> {
    let script = r#"
import sys
out = []
for ln in sys.stdin.read().split('\n'):
    if not ln:
        continue
    parts = ln.split(' ')
    op = parts[0]
    h = parts[1] if len(parts) > 1 else ''
    s = bytes.fromhex(h).decode('utf-8')
    m = s[::-1] if op == 'reverse' else getattr(s, op)()
    out.append('OK:' + m.encode('utf-8').hex())
sys.stdout.write('\n'.join(out))
"#;
    let mut input = String::new();
    for (op, h) in tasks {
        input.push_str(&format!("{op} {h}\n"));
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
        .expect("run python3 oracle");
    assert!(
        out.status.success(),
        "python3 oracle failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The oracle joins with '\n' and every result is a non-empty `OK:` line, so a plain
    // line split recovers exactly one result per task (an empty transform result is
    // `OK:` with empty hex, still non-empty).
    String::from_utf8_lossy(&out.stdout)
        .split('\n')
        .map(|l| l.to_string())
        .collect()
}

#[test]
fn no_trap_ops_match_cpython_over_random_multibyte_corpus() {
    // EMIT path must lower for every op regardless of WABT/python3 (holds on free CI).
    emit_module(&op_module(REVERSE.1, REVERSE.0))
        .unwrap_or_else(|e| panic!("reverse must lower: {e:?}"));
    for &(op, kernel, _) in STRIP_FAMILY {
        emit_module(&op_module(kernel, op))
            .unwrap_or_else(|e| panic!("{kernel} must lower: {e:?}"));
    }

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1217: skipping EXECUTED multibyte fuzz — WABT (wat2wasm / wasm-interp) \
             absent. Every op lowered through emit_module above; a box with WABT + python3 \
             also runs the multibyte corpus and value-matches CPython."
        );
        return;
    }
    if !python3_available() {
        eprintln!("PMAT-1217: skipping multibyte fuzz — python3 (the oracle) absent.");
        return;
    }

    let rev_inputs = reverse_corpus();
    let strip_inputs = strip_corpus();
    eprintln!(
        "PMAT-1217: multibyte differential fuzz — reverse over {} inputs, strip family over \
         {} inputs (× {} ops), vs live python3",
        rev_inputs.len(),
        strip_inputs.len(),
        STRIP_FAMILY.len()
    );

    // 1) One python3 call for ALL ground truth (reverse tasks then strip-family tasks).
    let mut tasks: Vec<(&str, String)> = Vec::new();
    for s in &rev_inputs {
        tasks.push((REVERSE.2, hex(s.as_bytes())));
    }
    for s in &strip_inputs {
        for &(_, _, py) in STRIP_FAMILY {
            tasks.push((py, hex(s.as_bytes())));
        }
    }
    let oracle = python_oracle(&tasks);
    assert_eq!(
        oracle.len(),
        tasks.len(),
        "python3 oracle returned {} results for {} tasks",
        oracle.len(),
        tasks.len()
    );

    // 2) Emit each kernel once (input lives in the data section, so the kernel WAT is
    //    input-independent) and reuse across the corpus.
    let reverse_wat = emit_module(&op_module(REVERSE.1, REVERSE.0)).unwrap();
    let strip_wat: Vec<String> = STRIP_FAMILY
        .iter()
        .map(|&(op, kernel, _)| emit_module(&op_module(kernel, op)).unwrap())
        .collect();

    let mut mismatches: Vec<String> = Vec::new();
    let mut ran = 0usize;

    // 3a) reverse over the unconstrained multibyte corpus.
    for (i, s) in rev_inputs.iter().enumerate() {
        let want_hex = oracle[i]
            .strip_prefix("OK:")
            .expect("reverse oracle line has OK: prefix");
        let want = String::from_utf8(unhex(want_hex)).expect("CPython UTF-8 result");
        match wasm_transform(&reverse_wat, REVERSE.1, s, &want) {
            Ok(got) if got == want => ran += 1,
            Ok(got) => mismatches.push(format!("{s:?}.reverse(): WASM={got:?} CPython={want:?}")),
            Err(e) => mismatches.push(e),
        }
    }

    // 3b) strip family over the ASCII-boundary / multibyte-interior corpus.
    let strip_base = rev_inputs.len();
    for (si, s) in strip_inputs.iter().enumerate() {
        for (ki, &(_, kernel, _)) in STRIP_FAMILY.iter().enumerate() {
            let idx = strip_base + si * STRIP_FAMILY.len() + ki;
            let want_hex = oracle[idx]
                .strip_prefix("OK:")
                .expect("strip oracle line has OK: prefix");
            let want = String::from_utf8(unhex(want_hex)).expect("CPython UTF-8 result");
            match wasm_transform(&strip_wat[ki], kernel, s, &want) {
                Ok(got) if got == want => ran += 1,
                Ok(got) => {
                    mismatches.push(format!("{s:?}.{kernel}(): WASM={got:?} CPython={want:?}"))
                }
                Err(e) => mismatches.push(e),
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "PMAT-1217: {} WASM/CPython divergence(s) over the multibyte corpus:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    eprintln!(
        "PMAT-1217: all {ran} (input × op) pairs executed in WABT and matched live python3 — \
         no silent divergence for s[::-1] over arbitrary UTF-8 nor for strip/lstrip/rstrip \
         over ASCII-boundary multibyte-interior inputs (interior payload copied verbatim, \
         no non-ASCII boundary trap)."
    );
}

#[test]
fn corpora_are_deterministic_and_cover_all_utf8_widths() {
    // Reverse corpus: deterministic, contains all four UTF-8 widths + a combining mark,
    // and (unlike PMAT-1207) is NOT ASCII-only — that non-ASCII content is the point.
    let a = reverse_corpus();
    let b = reverse_corpus();
    assert_eq!(
        a, b,
        "reverse corpus must be deterministic (fixed-seed LCG)"
    );
    assert!(
        a.iter().any(|s| !s.is_ascii()),
        "reverse corpus must contain non-ASCII (multibyte) input"
    );
    assert!(a.iter().any(|s| s.contains('é')), "a 2-byte code point");
    assert!(a.iter().any(|s| s.contains('日')), "a 3-byte code point");
    assert!(a.iter().any(|s| s.contains('🎉')), "a 4-byte code point");
    assert!(
        a.iter().any(|s| s.contains('\u{301}')),
        "a combining mark (code-point-reversal adversary)"
    );
    assert!(a.iter().any(|s| s.is_empty()), "the empty string");

    // Strip corpus: deterministic, and every input's first/last NON-whitespace char is
    // ASCII (the no-trap precondition) while at least one carries multibyte interior.
    let c = strip_corpus();
    let d = strip_corpus();
    assert_eq!(c, d, "strip corpus must be deterministic");
    for s in &c {
        let stripped = s.trim_matches(|ch: char| STRIP_WS.contains(&ch));
        if let Some(first) = stripped.chars().next() {
            assert!(
                first.is_ascii(),
                "strip corpus first non-ws char must be ASCII (no-trap domain): {s:?}"
            );
            assert!(
                stripped.chars().next_back().unwrap().is_ascii(),
                "strip corpus last non-ws char must be ASCII (no-trap domain): {s:?}"
            );
        }
    }
    assert!(
        c.iter().any(|s| !s.is_ascii()),
        "strip corpus must carry multibyte interior payload"
    );
}
