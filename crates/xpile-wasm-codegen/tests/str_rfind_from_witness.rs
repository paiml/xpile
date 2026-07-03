//! PMAT-1165 — EXECUTED start-bounded string RFIND (`s.rfind(p, start)`) witness for
//! the native WASM EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The rfind slice (`str_rfind_witness.rs`) wired the 1-arg `s.rfind(p)` via
//! `$__wasm_str_rfind` — a byte SEARCH that returns the CODE-POINT index of the
//! LAST match (or `-1`). This slice adds Python's 2-arg `str.rfind(sub, start)`
//! (`Expr::StrMethod`, op `Rfind`, `args.len() == 2`) via `$__wasm_str_rfind_from` —
//! the start-bounded generalisation, equivalently the reverse-scan sibling of
//! `$__wasm_str_find_from` (PMAT-1163): it SHARES find-from's start machinery (clamp
//! the negative/overflow `start`, then decode the `start`-th code point to a byte
//! offset), but the candidate slide runs DOWN from the last fitting offset to that
//! start byte, so the FIRST match is the RIGHTMOST at or after `start`, and the
//! reported index is still ABSOLUTE (Python's `rfind` with a start reports the
//! position in the ORIGINAL string).
//!
//! ## Why the 2-arg form is a genuine increment over the 1-arg form
//!
//! `rfind` returns the RIGHTMOST match, so a `start` can only RAISE the lower bound:
//! the result is either the SAME as the 1-arg `rfind` (when `start` ≤ that index) or
//! `-1` (when `start` exceeds it — no match can start at or after `start`). It never
//! shifts to a *different* positive index (unlike `find`, where a `start` selects a
//! LATER match). The witness pins that genuine `-1` cutoff:
//!   * `"abcabc".rfind("a", 4)` → -1 (1-arg → 3) — the only `a`s are at 0 and 3.
//!   * `"abcabc".rfind("a", 3)` → 3 — `start` == the 1-arg result, still matched.
//!   * `"héllo".rfind("l", 4)` → -1 (1-arg → 3).
//!   * `"abcabc".rfind("bc", 5)` → -1 (1-arg → 4).
//!
//! It also pins that the rightmost-of-many selection still holds under a start:
//!   * `"aXbXc".rfind("X", 2)` → 3, `"aaa".rfind("a", 1)` → 2.
//!
//! ## Why the full Python start semantics are non-trivial
//!
//! `start` is a CODE-POINT index with sign + overflow rules the witness pins:
//!   * NEGATIVE start counts from the end (`start += len`, clamped to 0):
//!     `"abcabc".rfind("bc", -3)` → 4; `"abc".rfind("a", -100)` → 0.
//!   * `start > len` → -1 — INCLUDING the empty needle
//!     (`"abc".rfind("", 4)` → -1), the reason the `> len` guard precedes the
//!     empty-needle branch in the helper.
//!   * an EMPTY needle → `len` (the code-point length), found at the END and
//!     UNAFFECTED by an in-range start (`"abc".rfind("", 0)` → 3,
//!     `"abc".rfind("", 3)` → 3). This is the ONE place rfind-from diverges from
//!     find-from (whose empty answer is the clamped START).
//!
//! ## Why the index must be CODE-POINT and the start must be CHAR-decoded
//!
//! Python indexes by CODE POINT, so the helper must (a) decode the `start`-th CODE
//! POINT to a byte offset before sliding, and (b) convert the match's byte offset
//! back to a CODE-POINT index. The witness proves both on fixtures where a naive
//! BYTE model would silently diverge:
//!   * `"abécdé".rfind("é", 3)` → 5 — the SECOND `é` is at byte 6 but code-point
//!     index 5 (`é` is 2 bytes); start char 3 is `c` (byte 4).
//!   * `"héllo".rfind("l", 0)` → 3 — last `l` at byte 4, code-point 3.
//!   * `"abécdé".rfind("é", -2)` → 5 — start = 4 (from-end), still the second `é`.
//!
//! ## The real program
//!
//! ```python
//! def rfrom(s: str, p: str, start: int) -> int:
//!     return s.rfind(p, start)
//! ```
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the helper + call) on a host without WABT. Every pinned
//! CPython int is cross-checked against a Rust reimplementation of Python's 2-arg
//! `str.rfind` (`rust_char_rfind_from`, char-decoded start + code-point index) and,
//! when `python3` is present, against CPython itself (a true differential).

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// A single `(haystack, needle, start)` fixture with its pinned CPython
/// `haystack.rfind(needle, start)` result.
struct Case {
    haystack: &'static str,
    needle: &'static str,
    start: i64,
    rfind: i64,
}

/// The witness fixtures — start-drops-to-`-1` (differs from 1-arg rfind),
/// rightmost-of-many under a start, NEGATIVE / very-negative / overflow starts,
/// EMPTY-needle-at-END (incl. the `start > len` → -1 and negative-start cases),
/// MULTI-BYTE (char-decoded start + code-point index, where a byte model would
/// diverge), plus absent / too-long boundaries. Each pinned int is the CPython
/// ground truth (asserted == the Rust 2-arg-rfind reimplementation in
/// `cpython_rfind_from_is_pinned`).
const CASES: &[Case] = &[
    // ── start DROPS the answer to -1 — 2-arg genuinely differs from 1-arg ───────
    Case {
        haystack: "abcabc",
        needle: "a",
        start: 3,
        rfind: 3,
    }, // start == 1-arg result → still matched
    Case {
        haystack: "abcabc",
        needle: "a",
        start: 4,
        rfind: -1,
    }, // 1-arg → 3
    Case {
        haystack: "abcabc",
        needle: "bc",
        start: 4,
        rfind: 4,
    },
    Case {
        haystack: "abcabc",
        needle: "bc",
        start: 5,
        rfind: -1,
    }, // 1-arg → 4
    Case {
        haystack: "hello",
        needle: "l",
        start: 0,
        rfind: 3,
    }, // start 0 == 1-arg (last l)
    Case {
        haystack: "hello",
        needle: "l",
        start: 3,
        rfind: 3,
    }, // start == result
    Case {
        haystack: "hello",
        needle: "l",
        start: 4,
        rfind: -1,
    }, // no l at/after 4
    // ── rightmost-of-many still holds under a start ─────────────────────────────
    Case {
        haystack: "aXbXc",
        needle: "X",
        start: 0,
        rfind: 3,
    },
    Case {
        haystack: "aXbXc",
        needle: "X",
        start: 2,
        rfind: 3,
    },
    Case {
        haystack: "aaa",
        needle: "a",
        start: 0,
        rfind: 2,
    },
    Case {
        haystack: "aaa",
        needle: "a",
        start: 1,
        rfind: 2,
    },
    Case {
        haystack: "abcabc",
        needle: "bc",
        start: 0,
        rfind: 4,
    },
    // ── NEGATIVE / overflow starts ──────────────────────────────────────────────
    Case {
        haystack: "abcabc",
        needle: "bc",
        start: -3,
        rfind: 4,
    }, // start = 3
    Case {
        haystack: "abc",
        needle: "a",
        start: -100,
        rfind: 0,
    }, // clamped to 0
    Case {
        haystack: "abc",
        needle: "a",
        start: 5,
        rfind: -1,
    }, // start beyond len
    Case {
        haystack: "abc",
        needle: "a",
        start: 1000000000000,
        rfind: -1,
    }, // huge start
    // ── EMPTY needle (found at the END → len, unaffected by an in-range start) ───
    Case {
        haystack: "abc",
        needle: "",
        start: 0,
        rfind: 3,
    },
    Case {
        haystack: "abc",
        needle: "",
        start: 2,
        rfind: 3,
    },
    Case {
        haystack: "abc",
        needle: "",
        start: 3,
        rfind: 3,
    }, // start == len
    Case {
        haystack: "abc",
        needle: "",
        start: 4,
        rfind: -1,
    }, // start > len → -1
    Case {
        haystack: "abc",
        needle: "",
        start: -1,
        rfind: 3,
    }, // start = 2, still the END
    Case {
        haystack: "abc",
        needle: "",
        start: -100,
        rfind: 3,
    }, // clamped to 0, still the END
    // ── absent / needle longer than haystack ────────────────────────────────────
    Case {
        haystack: "hello",
        needle: "z",
        start: 0,
        rfind: -1,
    },
    Case {
        haystack: "abc",
        needle: "abcd",
        start: 0,
        rfind: -1,
    },
    // ── MULTI-BYTE (é = 0xC3 0xA9) — char-decoded start + code-point index ───────
    Case {
        haystack: "héllo",
        needle: "l",
        start: 0,
        rfind: 3,
    }, // last l, code-point 3 (byte 4)
    Case {
        haystack: "café",
        needle: "f",
        start: 0,
        rfind: 2,
    },
    Case {
        haystack: "abécdé",
        needle: "é",
        start: 0,
        rfind: 5,
    }, // rightmost é (char 5, byte 6)
    Case {
        haystack: "abécdé",
        needle: "é",
        start: 3,
        rfind: 5,
    }, // start char 3 = c
    Case {
        haystack: "abécdé",
        needle: "é",
        start: -2,
        rfind: 5,
    }, // start = 4 → still second é
    Case {
        haystack: "abécdé",
        needle: "é",
        start: 6,
        rfind: -1,
    }, // start == len; both é < 6
    Case {
        haystack: "café",
        needle: "©",
        start: 0,
        rfind: -1,
    }, // shared 0xA9, not a substr
    Case {
        haystack: "naïve café",
        needle: "café",
        start: 0,
        rfind: 6,
    }, // ï before match
];

/// Fixed, non-overlapping addresses for the two preloaded str params, below
/// `LITERAL_BASE` (= 512) and the bump heap (>= 1024). Each is a length-prefixed
/// region (i32 BYTE count @ base+0, UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;
const P_ADDR: i32 = 256;

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def rfrom(s: str, p: str, start: int) -> int: return s.rfind(p, start)` — i.e.
/// `StrMethod { recv: s, op: Rfind, args: [p, start] }`.
fn rfind_from_module() -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::Rfind,
        args: vec![Expr::Ident("p".into()), Expr::Ident("start".into())],
    };
    let f = Function {
        name: "rfrom".into(),
        params: vec![
            Param {
                name: "s".into(),
                ty: Type::Str,
                mutable: false,
            },
            Param {
                name: "p".into(),
                ty: Type::Str,
                mutable: false,
            },
            Param {
                name: "start".into(),
                ty: Type::I64,
                mutable: false,
            },
        ],
        return_type: Type::I64,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "rfind_from_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// A LITERAL-needle module: `def rfrom_l(s: str, start: int) -> int:
/// return s.rfind("l", start)`. The needle `"l"` is an `Expr::LitStr`, so this
/// exercises the `collect_expr_literals` StrMethod arm — the "l" literal MUST be
/// laid out as a `(data)` segment (a literal method arg with no address fails to
/// lower).
fn literal_needle_module() -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::Rfind,
        args: vec![Expr::LitStr("l".into()), Expr::Ident("start".into())],
    };
    let f = Function {
        name: "rfrom_l".into(),
        params: vec![
            Param {
                name: "s".into(),
                ty: Type::Str,
                mutable: false,
            },
            Param {
                name: "start".into(),
                ty: Type::I64,
                mutable: false,
            },
        ],
        return_type: Type::I64,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "literal_needle_program".into(),
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

/// Splice the two str-param `(data …)` regions + a zero-arg `run` export
/// (`$rfrom(S_ADDR, P_ADDR, start)`) onto the emitted module, before its closing
/// `)`. `start` is pushed as an i64 const, so each fixture's start is baked into
/// its `run`.
fn build_witness_wat(kernel_wat: &str, s: &str, p: &str, start: i64) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1165 witness: preload the two str params (below LITERAL_BASE)\n");
    for (addr, txt) in [(S_ADDR, s), (P_ADDR, p)] {
        let bytes = txt.as_bytes();
        wat.push_str(&format!(
            "  (data (i32.const {addr}) \"{}\")\n",
            i32_data_escape(bytes.len() as i32)
        ));
        wat.push_str(&format!(
            "  (data (i32.const {}) \"{}\")\n",
            addr + 8,
            bytes_data_escape(bytes)
        ));
    }
    wat.push_str(&format!(
        "  (func (export \"run\") (result i64)\n    \
           i32.const {S_ADDR}\n    i32.const {P_ADDR}\n    i64.const {start}\n    call $rfrom)\n"
    ));
    wat.push_str(")\n");
    wat
}

/// Parse a `run() => i64:<value>` line from `wasm-interp --run-all-exports`.
fn parse_run_i64(stdout: &str) -> i64 {
    let line = stdout
        .lines()
        .find(|l| l.contains("run() => i64:"))
        .unwrap_or_else(|| panic!("no `run` i64 export in interp output:\n{stdout}"));
    let idx = line.find("=> i64:").unwrap();
    let raw = line[idx + "=> i64:".len()..].trim();
    // wasm-interp prints an i64 result as UNSIGNED decimal, so `rfind`'s -1 comes
    // back as 18446744073709551615 (u64::MAX). Parse the bits as u64 and
    // reinterpret them as i64 (identity for the small non-negative indices).
    raw.parse::<u64>()
        .map(|u| u as i64)
        .unwrap_or_else(|_| panic!("parse i64 from {line:?}"))
}

/// Lower `s.rfind(p, start)`, run it in WABT with `s`/`p`/`start` bound, return the
/// index. `None` when WABT is absent (the caller skips the value assertion).
fn exec_rfind_from(s: &str, p: &str, start: i64) -> Option<i64> {
    let kernel_wat = emit_module(&rfind_from_module()).expect("rfind-from program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, s, p, start);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-rfind-from-{}-{}",
        std::process::id(),
        (s.len() * 131 + p.len() * 7) as i64 + start
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("rfrom.wat");
    let wasm_path = dir.join("rfrom.wasm");
    std::fs::write(&wat_path, &wat).expect("write wat");
    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed:\n{}\n---WAT---\n{wat}",
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
        "wasm-interp run failed: stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    Some(parse_run_i64(&stdout))
}

/// A faithful reimplementation of Python's 2-arg `str.rfind(sub, start)`: `start`
/// is a CODE-POINT index (negative counts from the end, clamped to 0; `> len` →
/// -1), the search runs over `h[start:]`, and the result is the ABSOLUTE
/// code-point index of the RIGHTMOST match (or -1). This mirrors the WASM helper's
/// contract exactly and validates every pinned int independently of CPython.
fn rust_char_rfind_from(h: &str, n: &str, start: i64) -> i64 {
    let len = h.chars().count() as i64;
    // clamp start (Python: negative → from end, then floor at 0)
    let mut s = start;
    if s < 0 {
        s += len;
        if s < 0 {
            s = 0;
        }
    }
    if s > len {
        return -1;
    }
    let s = s as usize;
    // byte offset of the s-th code point (s <= len, so this lands on a boundary)
    let start_byte: usize = h.chars().take(s).map(|c| c.len_utf8()).sum();
    // rightmost byte match in the suffix (n[0] is a lead byte, so every match lands
    // on a code-point boundary); the empty needle rfinds at the suffix's END.
    match h[start_byte..].rfind(n) {
        Some(rel_byte) => {
            let abs_byte = start_byte + rel_byte;
            h[..abs_byte].chars().count() as i64
        }
        None => -1,
    }
}

/// Whether `python3` is on PATH (the true-CPython differential is gated on it).
fn python3_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Ask CPython itself: `haystack.rfind(needle, start)`. Strings are passed as argv
/// (no shell quoting), so multi-byte/special content is safe.
fn cpython_rfind_from(h: &str, n: &str, start: i64) -> i64 {
    let out = Command::new("python3")
        .arg("-c")
        .arg("import sys; print(sys.argv[1].rfind(sys.argv[2], int(sys.argv[3])))")
        .arg(h)
        .arg(n)
        .arg(start.to_string())
        .output()
        .expect("spawn python3");
    assert!(
        out.status.success(),
        "python3 rfind failed: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<i64>()
        .expect("python3 printed an int")
}

#[test]
fn cpython_rfind_from_is_pinned() {
    // Every pinned int == the Rust 2-arg-rfind reimplementation (always runs).
    for c in CASES {
        assert_eq!(
            c.rfind,
            rust_char_rfind_from(c.haystack, c.needle, c.start),
            "rfind mismatch for {:?}.rfind({:?}, {})",
            c.haystack,
            c.needle,
            c.start
        );
    }
    // …and, when python3 is present, == CPython itself (a true differential).
    if python3_available() {
        for c in CASES {
            assert_eq!(
                c.rfind,
                cpython_rfind_from(c.haystack, c.needle, c.start),
                "CPython disagrees for {:?}.rfind({:?}, {})",
                c.haystack,
                c.needle,
                c.start
            );
        }
    }
    // The multi-byte fixtures MUST genuinely exercise a non-ASCII byte, else the
    // char-decoded start + code-point-index conversion is untested.
    assert!(CASES
        .iter()
        .any(|c| !c.haystack.is_ascii() || !c.needle.is_ascii()));
    // A char-vs-byte separator: "abécdé".rfind("é", 3) == 5 (code-point index of the
    // SECOND é), yet its byte offset is 6 — a byte-index model returns 6, not 5.
    assert!(CASES
        .iter()
        .any(|c| { c.haystack == "abécdé" && c.needle == "é" && c.start == 3 && c.rfind == 5 }));
    // A NEGATIVE start must be pinned (from-end clamp): "abcabc".rfind("bc", -3) == 4.
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "abcabc" && c.needle == "bc" && c.start == -3 && c.rfind == 4));
    // The empty-needle `start > len` → -1 case must be present (the guard that
    // precedes the empty-needle branch): "abc".rfind("", 4) == -1.
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "abc" && c.needle.is_empty() && c.start == 4 && c.rfind == -1));
    // The empty-needle END semantics (rfind ≠ find): "abc".rfind("", 0) == 3 (the
    // END), where find-from would give the START (0).
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "abc" && c.needle.is_empty() && c.start == 0 && c.rfind == 3));
    // A fixture where 2-arg and 1-arg rfind genuinely DISAGREE (a true start cutoff
    // to -1): "abcabc".rfind("a", 4) == -1 but "abcabc".rfind("a") == 3.
    assert!(CASES.iter().any(|c| {
        c.haystack == "abcabc"
            && c.needle == "a"
            && c.start == 4
            && c.rfind == -1
            && c.haystack.rfind("a") == Some(3) // byte rfind == char rfind here (ASCII)
    }));
}

#[test]
fn rfind_from_emits_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): `s.rfind(p, start)` lowers
    // through the production emitter, carrying the start-bounded helper + its call,
    // declaring memory (the search reads the str bytes), and NEVER pulling in the
    // bump allocator (an rfind over two str PARAMS allocates nothing).
    let wat =
        emit_module(&rfind_from_module()).expect("the `s.rfind(p, start)` program must lower");
    assert!(
        wat.contains(
            "(func $__wasm_str_rfind_from (param $h i32) (param $n i32) (param $startc i64) (result i64)"
        ),
        "the $__wasm_str_rfind_from helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_rfind_from"),
        "$rfrom must call $__wasm_str_rfind_from:\n{wat}"
    );
    // The clamp/empty-needle path calls charlen — so the char-semantics family must
    // be co-emitted (module_touches_str).
    assert!(
        wat.contains("(func $__wasm_str_charlen"),
        "rfind-from's start clamp needs $__wasm_str_charlen co-emitted:\n{wat}"
    );
    assert!(
        wat.contains("(memory"),
        "the byte search needs memory declared:\n{wat}"
    );
    assert!(
        !wat.contains("(func $__alloc"),
        "a pure param rfind-from module must NOT carry the bump allocator:\n{wat}"
    );
    // No UNRELATED str-op helper (no dead cross-family helper): an rfind-from module
    // carries no find_from/startswith/endswith/contains/count helper. (The 1-arg
    // $__wasm_str_rfind IS co-emitted — module_uses_str_method matches op Rfind
    // regardless of arg count — but that is harmless/valid and mirrors find-from.)
    assert!(
        !wat.contains("(func $__wasm_str_find_from")
            && !wat.contains("(func $__wasm_str_startswith")
            && !wat.contains("(func $__wasm_str_endswith")
            && !wat.contains("(func $__wasm_str_contains")
            && !wat.contains("(func $__wasm_str_count"),
        "an rfind-from module carries no find_from/startswith/endswith/contains/count helper:\n{wat}"
    );
}

#[test]
fn one_arg_rfind_module_has_no_rfind_from_helper() {
    // The precise gate (`module_uses_str_rfind2`) must keep a plain 1-arg `.rfind(p)`
    // module free of the (dead) start-bounded helper — the common case stays lean.
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::Rfind,
        args: vec![Expr::Ident("p".into())],
    };
    let f = Function {
        name: "rfnd".into(),
        params: vec![
            Param {
                name: "s".into(),
                ty: Type::Str,
                mutable: false,
            },
            Param {
                name: "p".into(),
                ty: Type::Str,
                mutable: false,
            },
        ],
        return_type: Type::I64,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    let m = Module {
        name: "one_arg_rfind_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    };
    let wat = emit_module(&m).expect("the 1-arg `s.rfind(p)` program must lower");
    assert!(
        wat.contains("call $__wasm_str_rfind\n") || wat.contains("call $__wasm_str_rfind "),
        "the 1-arg module must still call the 1-arg rfind helper:\n{wat}"
    );
    assert!(
        !wat.contains("$__wasm_str_rfind_from"),
        "a 1-arg `.rfind(p)` module must NOT carry the start-bounded helper:\n{wat}"
    );
}

#[test]
fn literal_arg_lays_out_data() {
    // `s.rfind("l", start)` MUST lay out the "l" needle literal as a `(data)` segment.
    let wat = emit_module(&literal_needle_module()).expect("the literal-needle program must lower");
    assert!(
        wat.contains("call $__wasm_str_rfind_from"),
        "the literal-needle module must still call the rfind-from helper:\n{wat}"
    );
    // The literal byte 'l' (0x6c) must appear as a (data) segment.
    assert!(
        wat.contains("\\6c"),
        "the \"l\" needle literal must be laid out as a (data) segment:\n{wat}"
    );
    // Still no allocator — a literal needle materialises nothing at runtime.
    assert!(
        !wat.contains("(func $__alloc"),
        "a literal-needle rfind-from must NOT carry the bump allocator:\n{wat}"
    );
}

#[test]
fn real_rfind_from_programs_execute_in_wasm_and_match_cpython() {
    // Prove the emit path lowers (holds without WABT).
    emit_module(&rfind_from_module()).expect("rfind-from program lowers");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1165: skipping EXECUTED start-bounded rfind witness — WABT (wat2wasm \
             / wasm-interp) absent. The `s.rfind(p, start)` program lowered through \
             emit_module (asserted in `rfind_from_emits_helper_and_call`); a box with \
             WABT also runs all {} cases and asserts each == the pinned CPython int. \
             Free CI skips execution and stays green.",
            CASES.len()
        );
        return;
    }

    eprintln!("PMAT-1165: running EXECUTED start-bounded rfind witness via WABT");
    let mut checked = 0usize;
    for c in CASES {
        let got = exec_rfind_from(c.haystack, c.needle, c.start).expect("WABT present → a value");
        assert_eq!(
            got, c.rfind,
            "executed WASM `{:?}.rfind({:?}, {})` = {got} but CPython = {}",
            c.haystack, c.needle, c.start, c.rfind
        );
        checked += 1;
    }
    eprintln!(
        "PMAT-1165: EXECUTED start-bounded rfind witness PASSED — {checked} cases \
         lowered through emit_module and executed in WABT, each value-matching \
         CPython, including the start-cutoff fixtures where 2-arg drops to -1 \
         (\"abcabc\".rfind(\"a\", 4)=-1 vs 1-arg 3), the rightmost-of-many selection \
         (\"aXbXc\".rfind(\"X\", 2)=3), the NEGATIVE / overflow starts \
         (\"abcabc\".rfind(\"bc\", -3)=4, \"abc\".rfind(\"a\", 5)=-1), the \
         empty-needle-at-END cases (\"abc\".rfind(\"\", 0)=3, \"abc\".rfind(\"\", 4)=-1), \
         and the MULTI-BYTE char-decoded-start + code-point-index fixtures \
         (\"héllo\".rfind(\"l\", 0)=3, \"abécdé\".rfind(\"é\", 3)=5) — start-bounded \
         reverse byte search + char/byte conversion, proven on silicon."
    );
}
