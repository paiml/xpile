//! PMAT-1207 — a randomized DIFFERENTIAL witness for the WHOLE recent native-WASM
//! string-op family against LIVE CPython (`python3`).
//!
//! Every sibling witness (`str_upper_lower_witness`, `str_title_witness`,
//! `str_strip_witness`, `str_isdigit_witness`, …) pins a HAND-PICKED `CASES`
//! table to CPython. Hand-picked tables catch the cases the author thought of;
//! they do NOT catch a divergence hiding in an input nobody enumerated. This
//! witness closes that gap: it generates a DETERMINISTIC corpus of ASCII strings
//! (curated edges + a fixed-seed LCG walk over a whitespace/case/digit/punctuation
//! alphabet) and, for EVERY string × EVERY op, diffs xpile's REAL emitted WAT
//! (assembled + executed in WABT) against the value `python3` actually returns —
//! `python3` is the literal oracle, so there is zero reimplementation risk.
//!
//! ## The family under test (the PMAT-1185..1205 run)
//!
//! Allocating string→string transforms:
//!   `upper` `lower` `capitalize` `swapcase` `title` `strip` `lstrip` `rstrip`
//! Non-allocating string→bool predicates:
//!   `isdigit` `isnumeric` `isalpha` `isspace` `isalnum` `isupper` `islower`
//!   `isascii`
//!
//! ## Why ASCII-only inputs
//!
//! The corpus is ASCII (every byte `< 0x80`) BY CONSTRUCTION. On ASCII input the
//! whole-string case-fold ops never hit their non-ASCII trap, the strip family's
//! boundary bytes are all decidable (no trap), and the `is*` predicates are fully
//! decidable — so EVERY op runs clean and MUST byte-match / bool-match CPython
//! exactly. A silent divergence (the dangerous class — not a trap, not a refusal)
//! would fail this test. The non-ASCII TRAP posture is already witnessed per-op
//! (`non_ascii_*_traps_not_silent`); this witness is the breadth complement:
//! exhaustive ASCII exact-match, not edge-picked.
//!
//! ## Gating
//!
//! Runs only when BOTH `wat2wasm`/`wasm-interp` (WABT) AND `python3` are present.
//! On free CI (no WABT) it skips cleanly — same posture as every sibling witness —
//! after still exercising the EMIT path for every op. On a box with WABT but no
//! `python3` it skips the value diff with a message.

use std::collections::BTreeSet;
use std::process::{Command, Stdio};

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and the
/// bump heap (>= 1024). Length-prefixed: i32 BYTE count @ base+0, UTF-8 bytes @ base+8.
const S_ADDR: i32 = 16;

/// The allocating string→string transforms: (op, kernel name, CPython method).
const TRANSFORMS: &[(StrMethodOp, &str, &str)] = &[
    (StrMethodOp::Upper, "upper", "upper"),
    (StrMethodOp::Lower, "lower", "lower"),
    (StrMethodOp::Capitalize, "capitalize", "capitalize"),
    (StrMethodOp::SwapCase, "swapcase", "swapcase"),
    (StrMethodOp::Title, "title", "title"),
    (StrMethodOp::Strip, "strip", "strip"),
    (StrMethodOp::LStrip, "lstrip", "lstrip"),
    (StrMethodOp::RStrip, "rstrip", "rstrip"),
];

/// The non-allocating string→bool predicates: (op, kernel name, CPython method).
const PREDICATES: &[(StrMethodOp, &str, &str)] = &[
    (StrMethodOp::IsDigit, "isdigit", "isdigit"),
    // PMAT-1211: isnumeric shares the isdigit byte scan (≡ isdigit over ASCII, both
    // trap on non-ASCII); the ASCII-only corpus exercises the byte-exact domain.
    (StrMethodOp::IsNumeric, "isnumeric", "isnumeric"),
    (StrMethodOp::IsAlpha, "isalpha", "isalpha"),
    (StrMethodOp::IsSpace, "isspace", "isspace"),
    (StrMethodOp::IsAlnum, "isalnum", "isalnum"),
    (StrMethodOp::IsUpper, "isupper", "isupper"),
    (StrMethodOp::IsLower, "islower", "islower"),
    (StrMethodOp::IsAscii, "isascii", "isascii"),
];

/// Curated ASCII edge strings — the shapes that stress case boundaries, the
/// whitespace set (incl. VT/FF `0x0b`/`0x0c` and FS/US `0x1c`/`0x1f`), digit
/// word-boundaries (title), and all-cased / no-cased predicate branches.
const EDGES: &[&str] = &[
    "",
    " ",
    "  ",
    "   ",
    "a",
    "A",
    "0",
    "z",
    "Z",
    "aA",
    "Aa",
    "aZ",
    "AZ",
    "az",
    "abc",
    "ABC",
    "Abc",
    "aBc",
    "123",
    "007",
    "9",
    "  hi  ",
    "\t\n",
    "\x0b\x0c",
    "\x1cab\x1f",
    "it's",
    "don't stop",
    "hello world",
    "Hello World",
    "HELLO",
    "hello",
    "foo_bar",
    "a1b2c3",
    "mIxEd42cAsE",
    "  spaced out  ",
    "no_ws",
    "xxhelloxx",
    "a b c",
    " a ",
    "gGkK",
    "ABC def",
    "42",
    "_abc",
    "hi there!",
    "\ta\t",
    "MixedCase42",
    "123abc",
    "A B",
];

/// The alphabet the LCG walk samples — a rich ASCII mix: lowercase, uppercase,
/// digits, the full whitespace set (space/tab/LF/CR/VT/FF/FS/US), and the
/// boundary punctuation the title/case/predicate ops care about.
const ALPHABET: &[u8] = b"abczABZ019 \t\n\r\x0b\x0c\x1c\x1f_'!@[`{";

/// A fixed-seed 64-bit LCG (deterministic — no `rand`, so `cargo deny` is
/// unaffected and the corpus is byte-stable run to run).
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

/// Build the deterministic ASCII corpus: the curated edges + LCG-generated
/// strings of length 0..=8 over [`ALPHABET`], de-duplicated (a `BTreeSet` keeps
/// it stable and ordered).
fn corpus() -> Vec<String> {
    let mut set: BTreeSet<String> = EDGES.iter().map(|s| s.to_string()).collect();
    let mut rng = Lcg(0x9E3779B97F4A7C15); // golden-ratio seed
    for _ in 0..24 {
        let len = rng.below(9); // 0..=8
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(ALPHABET[rng.below(ALPHABET.len())]);
        }
        // Every byte is < 0x80 → valid ASCII / UTF-8.
        set.insert(String::from_utf8(bytes).expect("ASCII bytes are valid UTF-8"));
    }
    set.into_iter().collect()
}

/// Build the meta-HIR `Module` for `def <name>(s: str) -> <ret>: return s.<op>()`.
fn op_module(name: &str, op: StrMethodOp, ret: Type) -> Module {
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
        return_type: ret,
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

/// Assemble `wat` + run all exports in WABT, returning stdout. `None` when the run
/// TRAPS or the assembler rejects the module (the caller decides if that is a
/// failure — for an ASCII input it always is).
fn assemble_run(wat: &str, s: &str, kernel: &str) -> Result<String, String> {
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-fuzz-{}-{:016x}",
        std::process::id(),
        case_hash(s, kernel)
    ));
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    let wat_path = dir.join("case.wat");
    let wasm_path = dir.join("case.wasm");
    std::fs::write(&wat_path, wat).map_err(|e| format!("write wat: {e}"))?;
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
            "wasm-interp TRAPPED on ASCII input {s:?}.{kernel}() (must never trap on ASCII): \
             stdout={:?} stderr={:?}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&run.stdout).into_owned())
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

/// Splice the preloaded `s` region + a transform's readback exports (`run_len` +
/// `run_byte_i` for i in 0..`n_out`) onto the emitted module.
fn build_transform_wat(kernel_wat: &str, kernel: &str, s: &str, n_out: usize) -> String {
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
    wat
}

/// Splice the preloaded `s` region + a single `run` export onto the emitted
/// predicate module.
fn build_predicate_wat(kernel_wat: &str, kernel: &str, s: &str) -> String {
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
        "  (func (export \"run\") (result i32)\n    \
           i32.const {S_ADDR}\n    call ${kernel})\n"
    ));
    wat.push_str(")\n");
    wat
}

/// Run one transform op over `s` in WABT and reconstruct the result string. The
/// caller passes CPython's expected result so the WASM byte-length is checked
/// against it FIRST (a length divergence is caught even though only `n_out`
/// readback exports exist).
fn wasm_transform(
    kernel_wat: &str,
    kernel: &str,
    s: &str,
    expected: &str,
) -> Result<String, String> {
    let n_out = expected.len();
    let wat = build_transform_wat(kernel_wat, kernel, s, n_out);
    let stdout = assemble_run(&wat, s, kernel)?;
    let got_len = parse_i32(&stdout, "run_len") as usize;
    if got_len != n_out {
        return Err(format!(
            "{s:?}.{kernel}() WASM byte-length {got_len} != CPython {n_out}"
        ));
    }
    let mut bytes = Vec::with_capacity(n_out);
    for i in 0..n_out {
        bytes.push(parse_i32(&stdout, &format!("run_byte_{i}")) as u8);
    }
    String::from_utf8(bytes).map_err(|e| format!("{s:?}.{kernel}() bytes not UTF-8: {e}"))
}

/// Run one predicate op over `s` in WABT, returning the bool.
fn wasm_predicate(kernel_wat: &str, kernel: &str, s: &str) -> Result<bool, String> {
    let wat = build_predicate_wat(kernel_wat, kernel, s);
    let stdout = assemble_run(&wat, s, kernel)?;
    Ok(parse_i32(&stdout, "run") != 0)
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

/// Lowercase hex of a byte slice — the wire format for the `python3` oracle
/// (ASCII-safe for NUL / control bytes that would break a `-c` arg or a line).
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

/// The `python3` ground-truth oracle. Feeds one task per line (`<T|P> <method>
/// <hex>`) to a single `python3` process and returns the results in order.
/// Transform results come back as `OK:<hex>`; predicates as `OK:0` / `OK:1`.
/// `python3` decodes each hex → ASCII `str`, applies the method, and re-encodes.
fn python_oracle(tasks: &[(char, &str, String)]) -> Vec<String> {
    let script = r#"
import sys
out = []
for ln in sys.stdin.read().splitlines():
    if not ln:
        continue
    parts = ln.split(' ')
    kind, op = parts[0], parts[1]
    h = parts[2] if len(parts) > 2 else ''
    s = bytes.fromhex(h).decode('ascii')
    m = getattr(s, op)()
    if kind == 'T':
        out.append('OK:' + m.encode('ascii').hex())
    else:
        out.append('OK:' + ('1' if m else '0'))
sys.stdout.write('\n'.join(out))
"#;
    let mut input = String::new();
    for (kind, op, h) in tasks {
        // Always emit the hex token (empty string for the empty input) so the
        // python `split(' ')` layout is stable.
        input.push_str(&format!("{kind} {op} {h}\n"));
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
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.to_string())
        .collect()
}

#[test]
fn str_family_matches_cpython_over_random_ascii_corpus() {
    // The EMIT path must lower for every op regardless of WABT/python3 (holds on
    // free CI too) — a construct smoke over the whole family.
    for &(op, kernel, _) in TRANSFORMS {
        emit_module(&op_module(kernel, op, Type::Str))
            .unwrap_or_else(|e| panic!("transform {kernel} must lower: {e:?}"));
    }
    for &(op, kernel, _) in PREDICATES {
        emit_module(&op_module(kernel, op, Type::Bool))
            .unwrap_or_else(|e| panic!("predicate {kernel} must lower: {e:?}"));
    }

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1207: skipping EXECUTED differential fuzz — WABT (wat2wasm / \
             wasm-interp) absent. Every op lowered through emit_module above; a \
             box with WABT + python3 also runs the corpus and value-matches CPython."
        );
        return;
    }
    if !python3_available() {
        eprintln!("PMAT-1207: skipping differential fuzz — python3 (the oracle) absent.");
        return;
    }

    let inputs = corpus();
    eprintln!(
        "PMAT-1207: differential fuzz over {} ASCII inputs × {} ops (vs live python3)",
        inputs.len(),
        TRANSFORMS.len() + PREDICATES.len()
    );

    // 1) Gather CPython ground truth for every (op, input) in ONE python3 call.
    let mut tasks: Vec<(char, &str, String)> = Vec::new();
    for s in &inputs {
        let h = hex(s.as_bytes());
        for &(_, _, py) in TRANSFORMS {
            tasks.push(('T', py, h.clone()));
        }
        for &(_, _, py) in PREDICATES {
            tasks.push(('P', py, h.clone()));
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

    // 2) Emit each op's kernel ONCE (input lives in the data section, so the
    //    kernel WAT is input-independent) and reuse across the corpus.
    let transform_wat: Vec<String> = TRANSFORMS
        .iter()
        .map(|&(op, kernel, _)| emit_module(&op_module(kernel, op, Type::Str)).unwrap())
        .collect();
    let predicate_wat: Vec<String> = PREDICATES
        .iter()
        .map(|&(op, kernel, _)| emit_module(&op_module(kernel, op, Type::Bool)).unwrap())
        .collect();

    // 3) Walk the corpus, diffing WASM vs CPython. `mismatches` collects every
    //    divergence so a failure reports the FULL set, not just the first.
    let mut mismatches: Vec<String> = Vec::new();
    let mut ran = 0usize;
    let per_input = TRANSFORMS.len() + PREDICATES.len();
    for (si, s) in inputs.iter().enumerate() {
        let base = si * per_input;
        for (ti, &(_, kernel, _)) in TRANSFORMS.iter().enumerate() {
            let want_hex = oracle[base + ti]
                .strip_prefix("OK:")
                .expect("transform oracle line has OK: prefix");
            let want = String::from_utf8(unhex(want_hex)).expect("CPython ASCII result");
            match wasm_transform(&transform_wat[ti], kernel, s, &want) {
                Ok(got) if got == want => ran += 1,
                Ok(got) => {
                    mismatches.push(format!("{s:?}.{kernel}(): WASM={got:?} CPython={want:?}"))
                }
                Err(e) => mismatches.push(e),
            }
        }
        for (pi, &(_, kernel, _)) in PREDICATES.iter().enumerate() {
            let want = oracle[base + TRANSFORMS.len() + pi]
                .strip_prefix("OK:")
                .expect("predicate oracle line has OK: prefix")
                == "1";
            match wasm_predicate(&predicate_wat[pi], kernel, s) {
                Ok(got) if got == want => ran += 1,
                Ok(got) => mismatches.push(format!("{s:?}.{kernel}(): WASM={got} CPython={want}")),
                Err(e) => mismatches.push(e),
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "PMAT-1207: {} WASM/CPython divergence(s) over the ASCII corpus:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    eprintln!(
        "PMAT-1207: all {ran} (input × op) pairs executed in WABT and matched live \
         python3 — no silent divergence across upper/lower/capitalize/swapcase/title/\
         strip/lstrip/rstrip + isdigit/isnumeric/isalpha/isspace/isalnum/isupper/\
         islower/isascii."
    );
}

#[test]
fn corpus_is_deterministic_ascii_and_covers_predicate_true_branches() {
    // The corpus must be pure ASCII (else the transforms would trap and this
    // becomes a trap test, not an exact-match test) and stable run-to-run.
    let a = corpus();
    let b = corpus();
    assert_eq!(a, b, "corpus must be deterministic (fixed-seed LCG)");
    assert!(
        a.iter().all(|s| s.is_ascii()),
        "corpus must be ASCII-only so the ops never trap"
    );
    // It must contain inputs that drive each predicate's TRUE branch — else the
    // fuzz could pass while only ever exercising the (easy) False path.
    assert!(
        a.iter().any(|s| s == "123"),
        "an all-digit input (isdigit True)"
    );
    assert!(
        a.iter().any(|s| s == "abc"),
        "an all-lower input (isalpha/islower True)"
    );
    assert!(
        a.iter().any(|s| s == "ABC"),
        "an all-upper input (isupper True)"
    );
    assert!(
        a.iter().any(|s| s == "   "),
        "an all-space input (isspace True)"
    );
    assert!(
        a.iter().any(|s| s == "a1b2c3"),
        "an alnum input (isalnum True)"
    );
    // And the degenerate edges the per-op witnesses each pin individually.
    assert!(a.iter().any(|s| s.is_empty()), "the empty string");
    assert!(
        a.iter().any(|s| s == "it's"),
        "the title apostrophe boundary"
    );
}
