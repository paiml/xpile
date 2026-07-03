//! PMAT-1163 — EXECUTED start-bounded string FIND (`s.find(p, start)`) witness for
//! the native WASM EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The find slice (`str_find_witness.rs`) wired the 1-arg `s.find(p)` via
//! `$__wasm_str_find` — a byte SEARCH that returns the CODE-POINT index of the
//! FIRST match (or `-1`). This slice adds Python's 2-arg `str.find(sub, start)`
//! (`Expr::StrMethod`, op `Find`, `args.len() == 2`) via `$__wasm_str_find_from` —
//! the start-bounded generalisation: the SAME naive byte match and the SAME
//! byte→code-point-index conversion, but the slide BEGINS at the byte offset of
//! the `start`-th code point, and the reported index is still ABSOLUTE (Python's
//! `find` with a start reports the position in the ORIGINAL string).
//!
//! ## Why the 2-arg form is a genuine increment over the 1-arg form
//!
//! With a `start`, `find` skips earlier matches, so 1-arg and 2-arg genuinely
//! disagree, and the witness pins fixtures where they differ:
//!   * `"abcabc".find("bc", 2)` → 4 (1-arg → 1) — the SECOND `bc`.
//!   * `"aaa".find("a", 1)` → 1 (1-arg → 0).
//!   * `"hello".find("l", 4)` → -1 (1-arg → 2) — no `l` at or after 4.
//!
//! ## Why the full Python start semantics are non-trivial
//!
//! `start` is a CODE-POINT index with sign + overflow rules the witness pins:
//!   * NEGATIVE start counts from the end (`start += len`, clamped to 0):
//!     `"abcabc".find("bc", -3)` → 4; `"abc".find("a", -100)` → 0.
//!   * `start > len` → -1 — INCLUDING the empty needle
//!     (`"abc".find("", 4)` → -1), the reason the `> len` guard precedes the
//!     empty-needle branch in the helper.
//!   * an EMPTY needle → the clamped `start` (`"abc".find("", 2)` → 2,
//!     `"abc".find("", 3)` → 3, `"abc".find("", -1)` → 2).
//!
//! ## Why the index must be CODE-POINT and the start must be CHAR-decoded
//!
//! Python indexes by CODE POINT, so the helper must (a) decode the `start`-th CODE
//! POINT to a byte offset before sliding, and (b) convert the match's byte offset
//! back to a CODE-POINT index. The witness proves both on fixtures where a naive
//! BYTE model would silently diverge:
//!   * `"héllo".find("llo", 1)` → 2 — start char 1 is `é` (byte 1); the match is
//!     at byte 3 but code-point index 2 (`é` is 2 bytes).
//!   * `"héllo".find("l", 3)` → 3 — start char 3 is the second `l`.
//!   * `"abécdé".find("é", 3)` → 5 — the SECOND `é` (char 5, byte 6).
//!
//! ## The real program
//!
//! ```python
//! def ffrom(s: str, p: str, start: int) -> int:
//!     return s.find(p, start)
//! ```
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the helper + call) on a host without WABT. Every pinned
//! CPython int is cross-checked against a Rust reimplementation of Python's 2-arg
//! `str.find` (`rust_char_find_from`, char-decoded start + code-point index) and,
//! when `python3` is present, against CPython itself (a true differential).

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// A single `(haystack, needle, start)` fixture with its pinned CPython
/// `haystack.find(needle, start)` result.
struct Case {
    haystack: &'static str,
    needle: &'static str,
    start: i64,
    find: i64,
}

/// The witness fixtures — start-skips-earlier-match (differs from 1-arg find),
/// NEGATIVE / very-negative / overflow starts, EMPTY-needle-at-start (incl. the
/// `start > len` → -1 and negative-start cases), MULTI-BYTE (char-decoded start +
/// code-point index, where a byte model would diverge), plus absent / too-long
/// boundaries. Each pinned int is the CPython ground truth (asserted == the Rust
/// 2-arg-find reimplementation in `cpython_find_from_is_pinned`).
const CASES: &[Case] = &[
    // ── start SKIPS an earlier match — 2-arg genuinely differs from 1-arg ──────
    Case {
        haystack: "abcabc",
        needle: "bc",
        start: 2,
        find: 4,
    }, // 1-arg → 1
    Case {
        haystack: "abcabc",
        needle: "abc",
        start: 1,
        find: 3,
    },
    Case {
        haystack: "aaa",
        needle: "a",
        start: 1,
        find: 1,
    }, // 1-arg → 0
    Case {
        haystack: "aaa",
        needle: "a",
        start: 2,
        find: 2,
    },
    Case {
        haystack: "hello",
        needle: "l",
        start: 0,
        find: 2,
    }, // start 0 == 1-arg
    Case {
        haystack: "hello",
        needle: "l",
        start: 3,
        find: 3,
    }, // the SECOND l
    Case {
        haystack: "hello",
        needle: "l",
        start: 4,
        find: -1,
    }, // no l at/after 4
    Case {
        haystack: "abcabc",
        needle: "ca",
        start: 2,
        find: 2,
    }, // match wraps the start
    Case {
        haystack: "abcabc",
        needle: "bc",
        start: 5,
        find: -1,
    }, // needle can't fit from 5
    // ── NEGATIVE / overflow starts ────────────────────────────────────────────
    Case {
        haystack: "abcabc",
        needle: "bc",
        start: -3,
        find: 4,
    }, // start = 3
    Case {
        haystack: "abcabc",
        needle: "abc",
        start: -3,
        find: 3,
    }, // start = 3
    Case {
        haystack: "abc",
        needle: "a",
        start: -100,
        find: 0,
    }, // clamped to 0
    Case {
        haystack: "abc",
        needle: "a",
        start: 5,
        find: -1,
    }, // start beyond len
    Case {
        haystack: "abc",
        needle: "a",
        start: 1000000000000,
        find: -1,
    }, // huge start
    // ── EMPTY needle ──────────────────────────────────────────────────────────
    Case {
        haystack: "abc",
        needle: "",
        start: 0,
        find: 0,
    },
    Case {
        haystack: "abc",
        needle: "",
        start: 2,
        find: 2,
    },
    Case {
        haystack: "abc",
        needle: "",
        start: 3,
        find: 3,
    }, // start == len
    Case {
        haystack: "abc",
        needle: "",
        start: 4,
        find: -1,
    }, // start > len → -1
    Case {
        haystack: "abc",
        needle: "",
        start: -1,
        find: 2,
    }, // start = 2
    Case {
        haystack: "abc",
        needle: "",
        start: -100,
        find: 0,
    }, // clamped to 0
    // ── absent / needle longer than haystack ──────────────────────────────────
    Case {
        haystack: "hello",
        needle: "z",
        start: 0,
        find: -1,
    },
    Case {
        haystack: "abc",
        needle: "abcd",
        start: 0,
        find: -1,
    },
    // ── MULTI-BYTE (é = 0xC3 0xA9) — char-decoded start + code-point index ─────
    Case {
        haystack: "héllo",
        needle: "llo",
        start: 1,
        find: 2,
    }, // start char 1 = é
    Case {
        haystack: "héllo",
        needle: "l",
        start: 3,
        find: 3,
    }, // start char 3 = 2nd l
    Case {
        haystack: "héllo",
        needle: "é",
        start: 0,
        find: 1,
    }, // find AT the mb char
    Case {
        haystack: "héllo",
        needle: "o",
        start: 2,
        find: 4,
    },
    Case {
        haystack: "abécdé",
        needle: "é",
        start: 0,
        find: 2,
    }, // first é (char 2)
    Case {
        haystack: "abécdé",
        needle: "é",
        start: 3,
        find: 5,
    }, // second é (char 5)
    Case {
        haystack: "abécdé",
        needle: "é",
        start: -2,
        find: 5,
    }, // start = 4 → second é
    Case {
        haystack: "café",
        needle: "©",
        start: 0,
        find: -1,
    }, // shared 0xA9, not a substr
    Case {
        haystack: "naïve café",
        needle: "café",
        start: 0,
        find: 6,
    }, // ï before match
];

/// Fixed, non-overlapping addresses for the two preloaded str params, below
/// `LITERAL_BASE` (= 512) and the bump heap (>= 1024). Each is a length-prefixed
/// region (i32 BYTE count @ base+0, UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;
const P_ADDR: i32 = 256;

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def ffrom(s: str, p: str, start: int) -> int: return s.find(p, start)` — i.e.
/// `StrMethod { recv: s, op: Find, args: [p, start] }`.
fn find_from_module() -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::Find,
        args: vec![Expr::Ident("p".into()), Expr::Ident("start".into())],
    };
    let f = Function {
        name: "ffrom".into(),
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
        name: "find_from_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// A LITERAL-needle module: `def ffrom_l(s: str, start: int) -> int:
/// return s.find("l", start)`. The needle `"l"` is an `Expr::LitStr`, so this
/// exercises the `collect_expr_literals` StrMethod arm — the "l" literal MUST be
/// laid out as a `(data)` segment (a literal method arg with no address fails to
/// lower).
fn literal_needle_module() -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::Find,
        args: vec![Expr::LitStr("l".into()), Expr::Ident("start".into())],
    };
    let f = Function {
        name: "ffrom_l".into(),
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
/// (`$ffrom(S_ADDR, P_ADDR, start)`) onto the emitted module, before its closing
/// `)`. `start` is pushed as an i64 const, so each fixture's start is baked into
/// its `run`.
fn build_witness_wat(kernel_wat: &str, s: &str, p: &str, start: i64) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1163 witness: preload the two str params (below LITERAL_BASE)\n");
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
           i32.const {S_ADDR}\n    i32.const {P_ADDR}\n    i64.const {start}\n    call $ffrom)\n"
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
    // wasm-interp prints an i64 result as UNSIGNED decimal, so `find`'s -1 comes
    // back as 18446744073709551615 (u64::MAX). Parse the bits as u64 and
    // reinterpret them as i64 (identity for the small non-negative indices).
    raw.parse::<u64>()
        .map(|u| u as i64)
        .unwrap_or_else(|_| panic!("parse i64 from {line:?}"))
}

/// Lower `s.find(p, start)`, run it in WABT with `s`/`p`/`start` bound, return the
/// index. `None` when WABT is absent (the caller skips the value assertion).
fn exec_find_from(s: &str, p: &str, start: i64) -> Option<i64> {
    let kernel_wat = emit_module(&find_from_module()).expect("find-from program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, s, p, start);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-find-from-{}-{}",
        std::process::id(),
        (s.len() * 131 + p.len() * 7) as i64 + start
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("ffrom.wat");
    let wasm_path = dir.join("ffrom.wasm");
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

/// A faithful reimplementation of Python's 2-arg `str.find(sub, start)`: `start`
/// is a CODE-POINT index (negative counts from the end, clamped to 0; `> len` →
/// -1), the search runs over `h[start:]`, and the result is the ABSOLUTE
/// code-point index (or -1). This mirrors the WASM helper's contract exactly and
/// validates every pinned int independently of CPython.
fn rust_char_find_from(h: &str, n: &str, start: i64) -> i64 {
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
    match h[start_byte..].find(n) {
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

/// Ask CPython itself: `haystack.find(needle, start)`. Strings are passed as argv
/// (no shell quoting), so multi-byte/special content is safe.
fn cpython_find_from(h: &str, n: &str, start: i64) -> i64 {
    let out = Command::new("python3")
        .arg("-c")
        .arg("import sys; print(sys.argv[1].find(sys.argv[2], int(sys.argv[3])))")
        .arg(h)
        .arg(n)
        .arg(start.to_string())
        .output()
        .expect("spawn python3");
    assert!(
        out.status.success(),
        "python3 find failed: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<i64>()
        .expect("python3 printed an int")
}

#[test]
fn cpython_find_from_is_pinned() {
    // Every pinned int == the Rust 2-arg-find reimplementation (always runs).
    for c in CASES {
        assert_eq!(
            c.find,
            rust_char_find_from(c.haystack, c.needle, c.start),
            "find mismatch for {:?}.find({:?}, {})",
            c.haystack,
            c.needle,
            c.start
        );
    }
    // …and, when python3 is present, == CPython itself (a true differential).
    if python3_available() {
        for c in CASES {
            assert_eq!(
                c.find,
                cpython_find_from(c.haystack, c.needle, c.start),
                "CPython disagrees for {:?}.find({:?}, {})",
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
    // A fixture where the 2-arg result differs from the CODE-POINT index of a
    // BYTE-decoded start would be present: "héllo".find("llo", 1) == 2, but a byte
    // model that treated start as a BYTE offset (1 = mid-'h'…here byte 1 = start of
    // é) would still slide from byte 1 and could diverge on other inputs. The clear
    // char-vs-byte separator: "héllo".find("l", 3) == 3 (start char 3 = 2nd l), yet
    // byte offset 3 is the FIRST l — a byte-start model returns 2, not 3.
    assert!(CASES
        .iter()
        .any(|c| { c.haystack == "héllo" && c.needle == "l" && c.start == 3 && c.find == 3 }));
    // A NEGATIVE start must be pinned (from-end clamp): "abcabc".find("bc", -3) == 4.
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "abcabc" && c.needle == "bc" && c.start == -3 && c.find == 4));
    // The empty-needle `start > len` → -1 case must be present (the guard that
    // precedes the empty-needle branch): "abc".find("", 4) == -1.
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "abc" && c.needle.is_empty() && c.start == 4 && c.find == -1));
    // A fixture where 2-arg and 1-arg find genuinely DISAGREE (a true start-skip):
    // "abcabc".find("bc", 2) == 4 but "abcabc".find("bc") == 1.
    assert!(CASES.iter().any(|c| {
        c.haystack == "abcabc"
            && c.needle == "bc"
            && c.start == 2
            && c.find == 4
            && c.haystack.find("bc") == Some(1) // byte find == char find here (ASCII)
    }));
}

#[test]
fn find_from_emits_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): `s.find(p, start)` lowers
    // through the production emitter, carrying the start-bounded helper + its call,
    // declaring memory (the search reads the str bytes), and NEVER pulling in the
    // bump allocator (a find over two str PARAMS allocates nothing).
    let wat = emit_module(&find_from_module()).expect("the `s.find(p, start)` program must lower");
    assert!(
        wat.contains(
            "(func $__wasm_str_find_from (param $h i32) (param $n i32) (param $startc i64) (result i64)"
        ),
        "the $__wasm_str_find_from helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_find_from"),
        "$ffrom must call $__wasm_str_find_from:\n{wat}"
    );
    // The clamp/empty-needle path calls charlen — so the char-semantics family must
    // be co-emitted (module_touches_str).
    assert!(
        wat.contains("(func $__wasm_str_charlen"),
        "find-from's start clamp needs $__wasm_str_charlen co-emitted:\n{wat}"
    );
    assert!(
        wat.contains("(memory"),
        "the byte search needs memory declared:\n{wat}"
    );
    assert!(
        !wat.contains("(func $__alloc"),
        "a pure param find-from module must NOT carry the bump allocator:\n{wat}"
    );
    // No UNRELATED str-op helper (no dead cross-family helper): a find-from module
    // carries no rfind/startswith/endswith/contains/count helper.
    assert!(
        !wat.contains("(func $__wasm_str_rfind")
            && !wat.contains("(func $__wasm_str_startswith")
            && !wat.contains("(func $__wasm_str_endswith")
            && !wat.contains("(func $__wasm_str_contains")
            && !wat.contains("(func $__wasm_str_count"),
        "a find-from module carries no rfind/startswith/endswith/contains/count helper:\n{wat}"
    );
}

#[test]
fn one_arg_find_module_has_no_find_from_helper() {
    // The precise gate (`module_uses_str_find2`) must keep a plain 1-arg `.find(p)`
    // module free of the (dead) start-bounded helper — the common case stays lean.
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::Find,
        args: vec![Expr::Ident("p".into())],
    };
    let f = Function {
        name: "fnd".into(),
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
        name: "one_arg_find_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    };
    let wat = emit_module(&m).expect("the 1-arg `s.find(p)` program must lower");
    assert!(
        wat.contains("call $__wasm_str_find\n") || wat.contains("call $__wasm_str_find "),
        "the 1-arg module must still call the 1-arg find helper:\n{wat}"
    );
    assert!(
        !wat.contains("$__wasm_str_find_from"),
        "a 1-arg `.find(p)` module must NOT carry the start-bounded helper:\n{wat}"
    );
}

#[test]
fn literal_arg_lays_out_data() {
    // `s.find("l", start)` MUST lay out the "l" needle literal as a `(data)` segment.
    let wat = emit_module(&literal_needle_module()).expect("the literal-needle program must lower");
    assert!(
        wat.contains("call $__wasm_str_find_from"),
        "the literal-needle module must still call the find-from helper:\n{wat}"
    );
    // The literal byte 'l' (0x6c) must appear as a (data) segment.
    assert!(
        wat.contains("\\6c"),
        "the \"l\" needle literal must be laid out as a (data) segment:\n{wat}"
    );
    // Still no allocator — a literal needle materialises nothing at runtime.
    assert!(
        !wat.contains("(func $__alloc"),
        "a literal-needle find-from must NOT carry the bump allocator:\n{wat}"
    );
}

#[test]
fn real_find_from_programs_execute_in_wasm_and_match_cpython() {
    // Prove the emit path lowers (holds without WABT).
    emit_module(&find_from_module()).expect("find-from program lowers");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1163: skipping EXECUTED start-bounded find witness — WABT (wat2wasm \
             / wasm-interp) absent. The `s.find(p, start)` program lowered through \
             emit_module (asserted in `find_from_emits_helper_and_call`); a box with \
             WABT also runs all {} cases and asserts each == the pinned CPython int. \
             Free CI skips execution and stays green.",
            CASES.len()
        );
        return;
    }

    eprintln!("PMAT-1163: running EXECUTED start-bounded find witness via WABT");
    let mut checked = 0usize;
    for c in CASES {
        let got = exec_find_from(c.haystack, c.needle, c.start).expect("WABT present → a value");
        assert_eq!(
            got, c.find,
            "executed WASM `{:?}.find({:?}, {})` = {got} but CPython = {}",
            c.haystack, c.needle, c.start, c.find
        );
        checked += 1;
    }
    eprintln!(
        "PMAT-1163: EXECUTED start-bounded find witness PASSED — {checked} cases \
         lowered through emit_module and executed in WABT, each value-matching \
         CPython, including the start-skip fixtures where 2-arg diverges from 1-arg \
         (\"abcabc\".find(\"bc\", 2)=4, \"aaa\".find(\"a\", 1)=1), the NEGATIVE / \
         overflow starts (\"abcabc\".find(\"bc\", -3)=4, \"abc\".find(\"a\", 5)=-1), \
         the empty-needle-at-start cases (\"abc\".find(\"\", 3)=3, \
         \"abc\".find(\"\", 4)=-1), and the MULTI-BYTE char-decoded-start + \
         code-point-index fixtures (\"héllo\".find(\"l\", 3)=3, \
         \"abécdé\".find(\"é\", 3)=5) — start-bounded byte search + char/byte \
         conversion, proven on silicon."
    );
}
