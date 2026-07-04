//! PMAT-1223 — skeptic pass #8: an EXHAUSTIVE all-128-ASCII-byte DIFFERENTIAL
//! witness for the whole recent native-WASM string-op family against LIVE
//! CPython (`python3`).
//!
//! ## Why this exists on top of the PMAT-1207 fuzz
//!
//! [`str_family_fuzz_witness`] already diffs the family against live `python3`,
//! but its corpus is a *probabilistic* LCG walk over a fixed ~24-byte
//! `ALPHABET` (`b"abczABZ019 \t\n\r\x0b\x0c\x1c\x1f_'!@[`{"`) plus curated edges.
//! A probabilistic walk over a 24-byte alphabet does NOT deterministically visit
//! every ASCII *boundary* byte — and the boundary bytes are exactly where an
//! off-by-one in an ASCII range check hides:
//!
//!   - `'/'` (0x2f) just below `'0'` (0x30) / `'9'` (0x39) just below `':'`
//!     (0x3a) — a `isdigit`/`isnumeric`/`isalnum` digit-range edge,
//!   - `'@'` (0x40) just below `'A'` (0x41) / `'Z'` (0x5a) just below `'['`
//!     (0x5b) — the UPPER-range edge (`upper`/`lower`/`swapcase`/`isupper`/…),
//!   - `` '`' `` (0x60) just below `'a'` (0x61) / `'z'` (0x7a) just below `'{'`
//!     (0x7b) — the lower-range edge,
//!   - `0x08` just below `'\t'` (0x09) / `0x0e` just above `'\r'` (0x0d) /
//!     `0x1b` just below `0x1c` / `0x21` just above `' '` (0x20) — the
//!     whitespace-set edges (`strip`/`lstrip`/`rstrip`/`isspace`).
//!
//! Some of those (`@`, `[`, `` ` ``, `{`, `_`) ARE in the fuzz alphabet, but
//! `/`, `:`, `;`, `0x08`, `0x0e`, `0x1b`, `0x21`, `\`, `]`, `^`, `|`, `}`, `~`
//! are NOT — so a range off-by-one on one of those would slip the probabilistic
//! walk. This witness closes that gap by construction: it tests **every** ASCII
//! byte `0x00..=0x7f` as a 1-char input, so every boundary is visited
//! deterministically, plus a curated multichar set (title's digit/apostrophe
//! word-boundaries, whitespace runs, mixed case). `python3` is the literal
//! oracle — zero reimplementation risk.
//!
//! Result of the pass (2026-07-04): xpile's REAL emitted WAT byte-/bool-matches
//! CPython on ALL 128 ASCII bytes × 16 ops. No divergence. This witness pins
//! that so it can never silently regress.
//!
//! ## Harness-isolation lesson baked in (the trap this pass first hit)
//!
//! A first throwaway probe BATCHED many inputs into one module at
//! `addr = 16 + i*stride`. For `i` large enough the input `(data …)` region
//! landed at an address `>= __HEAP_BASE` (1024), where the bump allocator writes
//! each transform's RESULT — so the allocation clobbered the still-preloaded
//! input and produced garbage that looked like an emitter bug (it was not). The
//! allocation-free predicates were unaffected, which is the tell. The fix — and
//! the invariant every witness here upholds — is **one input per module**, with
//! the single input pinned at `S_ADDR = 16` (below `LITERAL_BASE` = 512 and far
//! below the heap at 1024), exactly as [`str_family_fuzz_witness`] /
//! [`str_upper_lower_witness`] do. Reading the result NEVER moves the input.
//!
//! ## Non-vacuity guard
//!
//! A silent "0 cases actually ran" is the failure mode of a hand-rolled
//! differential (cf. PMAT-1141). This witness counts every executed (op, input)
//! diff and asserts the total equals `corpus.len() * 16` before it is allowed to
//! pass — an absent/skipped case fails the test instead of hiding.
//!
//! ## Gating
//!
//! Runs only when BOTH WABT (`wat2wasm`/`wasm-interp`) AND `python3` are present.
//! On free CI (no WABT) it skips cleanly after still exercising the EMIT path for
//! every op — same posture as every sibling witness.

use std::process::{Command, Stdio};

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and the
/// bump heap (>= 1024). Length-prefixed: i32 BYTE count @ base+0, UTF-8 bytes @
/// base+8. ONE input per module lives here (see the harness-isolation lesson in
/// the module doc).
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
    (StrMethodOp::IsNumeric, "isnumeric", "isnumeric"),
    (StrMethodOp::IsAlpha, "isalpha", "isalpha"),
    (StrMethodOp::IsSpace, "isspace", "isspace"),
    (StrMethodOp::IsAlnum, "isalnum", "isalnum"),
    (StrMethodOp::IsUpper, "isupper", "isupper"),
    (StrMethodOp::IsLower, "islower", "islower"),
    (StrMethodOp::IsAscii, "isascii", "isascii"),
];

/// Curated multichar ASCII inputs on top of the exhaustive single-byte sweep —
/// the stateful / multi-token cases a per-byte sweep cannot reach: `title`'s
/// digit- and apostrophe-delimited word boundaries, whitespace runs at both
/// ends, mixed case, and the boundary bytes packed next to letters.
const MULTICHAR: &[&str] = &[
    "",
    "abc",
    "ABC",
    "Abc",
    "aBC",
    "123abc",
    "ab2cd",
    "it's",
    "don't stop",
    "  hi  ",
    "\tx\n",
    "\x1cA\x1f",
    "foo_bar",
    "a@b[c`d{e",
    "mIxEd42",
    "HELLO world",
    "9lives",
    "AB CD",
    "a b c",
    "_A_",
    "[a]",
    "`z`",
    "{Z}",
    "/0:9;",
    "\x08\t\x0d\x0e",
    "  spaced out  ",
];

/// The full deterministic corpus: every ASCII byte `0x00..=0x7f` as a 1-char
/// string, then the curated multichar set. Every element is valid ASCII (< 0x80)
/// by construction, so every op runs clean (no non-ASCII trap) and MUST match
/// CPython exactly.
fn corpus() -> Vec<String> {
    let mut v: Vec<String> = (0u8..=0x7f)
        .map(|b| String::from_utf8(vec![b]).expect("single ASCII byte is valid UTF-8"))
        .collect();
    v.extend(MULTICHAR.iter().map(|s| s.to_string()));
    v
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

/// A stable per-(input, op) hash so distinct cases get distinct temp dirs — the
/// artifact-isolation discipline every witness follows (unique dir per case).
fn case_hash(s: &str, kernel: &str) -> u64 {
    let mut h: u64 = 1469598103934665603; // FNV-1a offset basis
    for &b in s.as_bytes().iter().chain(b"|").chain(kernel.as_bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

/// Assemble `wat` + run all exports in WABT, returning stdout. `Err` when the run
/// TRAPS or the assembler rejects the module — for an ASCII input that is always a
/// failure (a gate-hole calling an undeclared helper fails wat2wasm HERE; a trap
/// on ASCII means a range check wrongly hit the non-ASCII `unreachable` arm).
fn assemble_run(wat: &str, s: &str, kernel: &str) -> Result<String, String> {
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-exhaustive-{}-{:016x}",
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
            "wat2wasm FAILED for {s:?}.{kernel}():\n{}",
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

/// Splice the preloaded `s` region (at `S_ADDR`, below the heap) + a transform's
/// readback exports (`run_len` + `run_byte_i` for i in 0..`n_out`) onto the
/// emitted module. ONE input per module — reading the result never moves it.
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
fn str_family_matches_cpython_over_exhaustive_ascii() {
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
            "PMAT-1223: skipping EXHAUSTIVE differential — WABT (wat2wasm / \
             wasm-interp) absent. Every op lowered through emit_module above; a \
             box with WABT + python3 also runs all 128 ASCII bytes × 16 ops and \
             value-matches CPython."
        );
        return;
    }
    if !python3_available() {
        eprintln!("PMAT-1223: skipping EXHAUSTIVE differential — python3 (the oracle) absent.");
        return;
    }

    let inputs = corpus();
    let per_input = TRANSFORMS.len() + PREDICATES.len();
    eprintln!(
        "PMAT-1223: EXHAUSTIVE differential over {} ASCII inputs (all 128 bytes + \
         {} multichar) × {} ops (vs live python3)",
        inputs.len(),
        MULTICHAR.len(),
        per_input
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
    //    divergence so a failure reports the FULL set; `ran` is the non-vacuity
    //    counter asserted at the end.
    let mut mismatches: Vec<String> = Vec::new();
    let mut ran = 0usize;
    for (si, s) in inputs.iter().enumerate() {
        let base = si * per_input;
        for (ti, &(_, kernel, _)) in TRANSFORMS.iter().enumerate() {
            let want_hex = oracle[base + ti]
                .strip_prefix("OK:")
                .expect("transform oracle line has OK: prefix");
            let want = String::from_utf8(unhex(want_hex)).expect("CPython ASCII result");
            match wasm_transform(&transform_wat[ti], kernel, s, &want) {
                Ok(got) if got == want => {}
                Ok(got) => {
                    mismatches.push(format!("{s:?}.{kernel}() = {got:?} but CPython = {want:?}"))
                }
                Err(e) => mismatches.push(format!("{s:?}.{kernel}() run error: {e}")),
            }
            ran += 1;
        }
        for (pi, &(_, kernel, _)) in PREDICATES.iter().enumerate() {
            let want = oracle[base + TRANSFORMS.len() + pi]
                .strip_prefix("OK:")
                .expect("predicate oracle line has OK: prefix")
                == "1";
            match wasm_predicate(&predicate_wat[pi], kernel, s) {
                Ok(got) if got == want => {}
                Ok(got) => {
                    mismatches.push(format!("{s:?}.{kernel}() = {got} but CPython = {want}"))
                }
                Err(e) => mismatches.push(format!("{s:?}.{kernel}() run error: {e}")),
            }
            ran += 1;
        }
    }

    assert!(
        mismatches.is_empty(),
        "PMAT-1223: {} WASM-vs-CPython divergence(s) over the exhaustive ASCII corpus:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    // Non-vacuity guard: every (input, op) pair MUST have executed. A silent
    // "0 ran" (cf. PMAT-1141) fails here instead of passing empty.
    assert_eq!(
        ran,
        inputs.len() * per_input,
        "non-vacuity: executed {ran} diffs, expected {}",
        inputs.len() * per_input
    );
    eprintln!(
        "PMAT-1223: all {ran} (input, op) diffs matched live python3 — every ASCII \
         byte 0x00..=0x7f (incl. the boundary bytes '/'.':'.';'.'@'.'['.'`'.'{{' \
         the probabilistic fuzz alphabet omits) × {} transforms + {} predicates, \
         plus {} curated multichar cases. No divergence.",
        TRANSFORMS.len(),
        PREDICATES.len(),
        MULTICHAR.len()
    );
}
