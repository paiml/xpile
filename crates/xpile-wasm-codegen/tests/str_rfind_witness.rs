//! PMAT-1143 — EXECUTED string RFIND (`s.rfind(p)`) witness for the native WASM
//! EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The find slice (`str_find_witness.rs`) wired `s.find(p)` via `$__wasm_str_find`
//! — a byte SEARCH that returns the CODE-POINT index of the FIRST match (or `-1`).
//! This slice adds Python's `str.rfind` (`Expr::StrMethod`, op `Rfind`) via
//! `$__wasm_str_rfind` — the reverse-scan sibling: the SAME naive byte match and
//! the SAME byte→code-point-index conversion, but the outer slide runs from the
//! LAST candidate start offset DOWN to `0`, so the first match found is the
//! RIGHTMOST (last) occurrence.
//!
//! ## Why rfind is a genuine increment over find (not a rename)
//!
//! On a string with MORE THAN ONE occurrence, `rfind` and `find` disagree, and
//! the witness pins fixtures where they genuinely differ:
//!   * `"hello".rfind("l")` → 3 (find → 2) — the LAST of two `l`s.
//!   * `"banana".rfind("ana")` → 3 (find → 1) — the overlapping right match.
//!   * `"mississippi".rfind("ss")` → 5 (find → 2).
//!   * `"aaa".rfind("a")` → 2 (find → 0).
//!
//! ## Why the rfind index must be CODE-POINT, not byte
//!
//! Like `find`, Python `str.rfind` returns the position in CODE POINTS, not
//! bytes. So `$__wasm_str_rfind`, on a match at byte offset `start`, converts
//! `start` to a code-point index by counting the non-continuation bytes in
//! `haystack[0..start]` (`(b & 0xC0) != 0x80`). The witness proves it on fixtures
//! where a naive BYTE index would silently diverge:
//!   * `"héllo".rfind("l")` → 3 — the last `l` is at byte offset 4 (`é` is 2
//!     bytes), so a byte-index rfind would wrongly return 4; the char index is 3.
//!   * `"abécdé".rfind("é")` → 5 — the SECOND `é` (char index 5, byte offset 6).
//!   * `"café".rfind("é")` → 3 — a match AT a multi-byte char.
//!
//! ## Why the EMPTY-needle answer diverges from find
//!
//! This is the ONE place `rfind` differs from `find` beyond direction: Python
//! `"…".rfind("")` returns `len(…)` in CODE POINTS (the empty string is found at
//! the END), whereas `"…".find("")` is `0`. So `$__wasm_str_rfind` returns
//! `$__wasm_str_charlen(h)` for an empty needle, NOT `0`:
//!   * `"abc".rfind("")` → 3, `"".rfind("")` → 0.
//!   * `"héllo".rfind("")` → 5 — the CODE-POINT length (5), NOT the byte length
//!     (6). A byte-length answer would silently diverge on non-ASCII.
//!
//! Plus the boundary cases pinned to CPython:
//!   * absent → `-1` (`"hello".rfind("z")`).
//!   * needle LONGER than haystack → `-1`.
//!   * a shared-continuation-byte NEGATIVE (`"café".rfind("©")` → -1).
//!
//! ## The real program
//!
//! ```python
//! def rfnd(s: str, p: str) -> int:
//!     return s.rfind(p)
//! ```
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the helper + call) on a host without WABT. The pinned
//! CPython ints are cross-checked against Rust's byte `str::rfind` converted to a
//! code-point index (`h[..byte].chars().count()`), which equals Python's
//! char-indexed `str.rfind` for valid UTF-8.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// A single `(haystack, needle)` fixture with its pinned CPython `haystack
/// .rfind(needle)` result. `python3 -c "print('{haystack}'.rfind('{needle}'))"`.
struct Case {
    haystack: &'static str,
    needle: &'static str,
    rfind: i64,
}

/// The witness fixtures — ASCII (last-of-many / at-start / at-end / absent /
/// equal / longer-than / empty), MULTI-BYTE (rfind AT / PAST a multi-byte char,
/// where a byte index would diverge), and a shared-continuation-byte NEGATIVE.
/// Each pinned int is the CPython ground truth (asserted == the Rust
/// byte-rfind→code-point-index conversion in `cpython_rfind_is_pinned`).
const CASES: &[Case] = &[
    // ── ASCII — rfind returns the LAST occurrence (differs from find) ──────
    Case {
        haystack: "hello",
        needle: "l",
        rfind: 3,
    }, // LAST of two l's (find would be 2)
    Case {
        haystack: "hello",
        needle: "ll",
        rfind: 2,
    }, // single occurrence — rfind == find
    Case {
        haystack: "hello",
        needle: "h",
        rfind: 0,
    }, // at the start
    Case {
        haystack: "hello",
        needle: "o",
        rfind: 4,
    }, // at the end
    Case {
        haystack: "hello",
        needle: "z",
        rfind: -1,
    }, // absent
    Case {
        haystack: "hello",
        needle: "hello",
        rfind: 0,
    }, // equal
    Case {
        haystack: "hello",
        needle: "helloo",
        rfind: -1,
    }, // needle LONGER than haystack
    Case {
        haystack: "banana",
        needle: "ana",
        rfind: 3,
    }, // LAST (overlapping) "ana" at 3 (find would be 1)
    Case {
        haystack: "banana",
        needle: "na",
        rfind: 4,
    }, // last "na" (find would be 2)
    Case {
        haystack: "mississippi",
        needle: "ss",
        rfind: 5,
    }, // last "ss" (find would be 2)
    Case {
        haystack: "aaa",
        needle: "a",
        rfind: 2,
    }, // last of three (find would be 0)
    Case {
        haystack: "abcabc",
        needle: "bc",
        rfind: 4,
    }, // last "bc" (find would be 1)
    // ── EMPTY needle — Python "…".rfind("") == len(…) in CODE POINTS ──────
    Case {
        haystack: "hello",
        needle: "",
        rfind: 5,
    }, // charlen (find would be 0)
    Case {
        haystack: "",
        needle: "",
        rfind: 0,
    },
    Case {
        haystack: "abc",
        needle: "",
        rfind: 3,
    },
    Case {
        haystack: "",
        needle: "a",
        rfind: -1,
    },
    // ── MULTI-BYTE (é = 0xC3 0xA9) — CODE-POINT index, not byte offset ────
    Case {
        haystack: "héllo",
        needle: "llo",
        rfind: 2,
    }, // single occurrence; byte offset 3 (é is 2 bytes) — char index 2
    Case {
        haystack: "héllo",
        needle: "l",
        rfind: 3,
    }, // LAST l — byte offset 4, char index 3 (a byte rfind would return 4)
    Case {
        haystack: "héllo",
        needle: "é",
        rfind: 1,
    }, // rfind AT the multi-byte char
    Case {
        haystack: "café",
        needle: "é",
        rfind: 3,
    }, // multi-byte char at the end (char index 3, byte offset 3)
    Case {
        haystack: "café",
        needle: "©",
        rfind: -1,
    }, // © (0xC2 0xA9) shares the trailing 0xA9 with é — NOT a substring
    Case {
        haystack: "naïve café",
        needle: "café",
        rfind: 6,
    }, // two multi-byte chars before the match ("ï" at char 2)
    Case {
        haystack: "abécdé",
        needle: "é",
        rfind: 5,
    }, // SECOND é: char index 5, byte offset 6 (find would be 2)
    // The empty-needle MULTI-BYTE case — charlen (5), NOT byte length (6).
    Case {
        haystack: "héllo",
        needle: "",
        rfind: 5,
    },
];

/// Fixed, non-overlapping addresses for the two preloaded str params, below
/// `LITERAL_BASE` (= 512) and the bump heap (>= 1024). Each is a length-prefixed
/// region (i32 BYTE count @ base+0, UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;
const P_ADDR: i32 = 256;

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def rfnd(s: str, p: str) -> int: return s.rfind(p)` — i.e. `StrMethod {
/// recv: s, op: Rfind, args: [p] }`.
fn rfind_module() -> Module {
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
    Module {
        name: "rfind_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// A LITERAL-arg module: `def rfnd_l(s: str) -> int: return s.rfind("l")`. The
/// needle `"l"` is an `Expr::LitStr`, so this exercises the `collect_expr_literals`
/// StrMethod arm — the "l" literal MUST be laid out as a `(data)` segment (a
/// literal method arg with no address fails to lower).
fn literal_needle_module() -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::Rfind,
        args: vec![Expr::LitStr("l".into())],
    };
    let f = Function {
        name: "rfnd_l".into(),
        params: vec![Param {
            name: "s".into(),
            ty: Type::Str,
            mutable: false,
        }],
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

/// A HEAP-operand module: `def rfnd_lo(s: str) -> int: return s.rfind("l" + "o")`.
/// The needle `"l" + "o"` (`Expr::Concat`) materialises a heap string, so this
/// exercises the `expr_has_heap_op` StrMethod arm (the bump allocator plus the
/// "l"/"o" literal `(data)` segments must be gated in — a heap method arg would
/// otherwise emit `$__alloc` against an undeclared allocator).
fn heap_needle_module() -> Module {
    let needle = Expr::Concat {
        lhs: Box::new(Expr::LitStr("l".into())),
        rhs: Box::new(Expr::LitStr("o".into())),
    };
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::Rfind,
        args: vec![needle],
    };
    let f = Function {
        name: "rfnd_lo".into(),
        params: vec![Param {
            name: "s".into(),
            ty: Type::Str,
            mutable: false,
        }],
        return_type: Type::I64,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "heap_needle_program".into(),
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
/// (`$rfnd(S_ADDR, P_ADDR)`) onto the emitted module, before its closing `)`.
fn build_witness_wat(kernel_wat: &str, s: &str, p: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1143 witness: preload the two str params (below LITERAL_BASE)\n");
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
           i32.const {S_ADDR}\n    i32.const {P_ADDR}\n    call $rfnd)\n"
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

/// Lower `s.rfind(p)`, run it in WABT with `s`/`p` preloaded, return the index.
/// `None` when WABT is absent (the caller skips the value assertion).
fn exec_rfind(s: &str, p: &str) -> Option<i64> {
    let kernel_wat = emit_module(&rfind_module()).expect("rfind program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, s, p);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-rfind-{}-{}",
        std::process::id(),
        s.len() * 131 + p.len() * 7
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("rfnd.wat");
    let wasm_path = dir.join("rfnd.wasm");
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

/// Python `str.rfind` = the CODE-POINT index of the LAST occurrence, or -1.
/// Rust's `str::rfind` returns a BYTE offset, so convert it to a code-point index
/// by counting the chars in the prefix — this is exactly Python's char-indexed
/// rfind for valid UTF-8, and validates every pinned int independently. (For an
/// empty needle Rust's `rfind` returns `Some(h.len())`, so the conversion yields
/// `h.chars().count()` = Python's charlen answer.)
fn rust_char_rfind(h: &str, n: &str) -> i64 {
    match h.rfind(n) {
        Some(byte) => h[..byte].chars().count() as i64,
        None => -1,
    }
}

#[test]
fn cpython_rfind_is_pinned() {
    for c in CASES {
        assert_eq!(
            c.rfind,
            rust_char_rfind(c.haystack, c.needle),
            "rfind mismatch for {:?}.rfind({:?})",
            c.haystack,
            c.needle
        );
    }
    // The multi-byte fixtures MUST genuinely exercise a non-ASCII byte, else the
    // "byte offset → code-point index" conversion is untested.
    assert!(CASES
        .iter()
        .any(|c| !c.haystack.is_ascii() || !c.needle.is_ascii()));
    // A fixture where rfind's CODE-POINT index differs from the BYTE offset must
    // be present: "héllo".rfind("l") == 3 (code points), but the byte offset is 4
    // (é is 2 bytes).
    assert!(CASES.iter().any(|c| {
        c.haystack == "héllo"
            && c.needle == "l"
            && c.rfind == 3
            && c.haystack.rfind(c.needle) == Some(4) // byte offset genuinely differs
    }));
    // A fixture where rfind and find genuinely DISAGREE (a true last-occurrence,
    // not an aliased find): "hello".rfind("l") == 3 but "hello".find("l") == 2.
    assert!(CASES.iter().any(|c| {
        c.haystack == "hello" && c.needle == "l" && c.rfind == 3 && c.haystack.find("l") == Some(2)
    }));
    // The empty-needle answer must be pinned to the CODE-POINT length (Python
    // "…".rfind("") == len(…) in code points), NOT the byte length: "héllo" is 6
    // bytes but 5 code points → 5.
    assert!(CASES.iter().any(|c| {
        c.haystack == "héllo" && c.needle.is_empty() && c.rfind == 5 && c.haystack.len() == 6
        // byte length genuinely differs from the char length
    }));
    // A shared-continuation-byte NEGATIVE (no false positive): "©" in "café"
    // shares 0xA9 but rfind == -1.
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "café" && c.needle == "©" && c.rfind == -1));
    // An ABSENT fixture must return -1 (not a bogus non-negative index).
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "hello" && c.needle == "z" && c.rfind == -1));
}

#[test]
fn rfind_emits_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): `s.rfind(p)` lowers
    // through the production emitter, carrying its helper + call, declaring memory
    // (the search reads the str bytes), and NEVER pulling in the bump allocator (a
    // rfind over two str PARAMS allocates nothing).
    let wat = emit_module(&rfind_module()).expect("the `s.rfind(p)` program must lower");
    assert!(
        wat.contains("(func $__wasm_str_rfind (param $h i32) (param $n i32) (result i64)"),
        "the $__wasm_str_rfind helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_rfind"),
        "$rfnd must call $__wasm_str_rfind:\n{wat}"
    );
    // The empty-needle answer calls charlen — so the char-semantics family must be
    // co-emitted (module_touches_str).
    assert!(
        wat.contains("(func $__wasm_str_charlen"),
        "rfind's empty-needle case needs $__wasm_str_charlen co-emitted:\n{wat}"
    );
    assert!(
        wat.contains("(memory"),
        "the byte search needs memory declared:\n{wat}"
    );
    assert!(
        !wat.contains("(func $__alloc"),
        "a pure param rfind module must NOT carry the bump allocator:\n{wat}"
    );
    // An rfind-only module carries no find/prefix/suffix/contains/count helper (no
    // dead helper). Match the helper DEFINITION `(func $__wasm_str_…`.
    assert!(
        !wat.contains("(func $__wasm_str_find ")
            && !wat.contains("(func $__wasm_str_startswith")
            && !wat.contains("(func $__wasm_str_endswith")
            && !wat.contains("(func $__wasm_str_contains")
            && !wat.contains("(func $__wasm_str_count"),
        "an rfind-only module carries no find/startswith/endswith/contains/count helper:\n{wat}"
    );
}

#[test]
fn literal_arg_lays_out_data() {
    // `s.rfind("l")` MUST lay out the "l" needle literal as a `(data)` segment.
    let wat = emit_module(&literal_needle_module()).expect("the literal-needle program must lower");
    assert!(
        wat.contains("call $__wasm_str_rfind"),
        "the literal-needle module must still call the rfind helper:\n{wat}"
    );
    // The literal byte 'l' (0x6c) must appear as a (data) segment.
    assert!(
        wat.contains("\\6c"),
        "the \"l\" needle literal must be laid out as a (data) segment:\n{wat}"
    );
    // Still no allocator — a literal needle materialises nothing at runtime.
    assert!(
        !wat.contains("(func $__alloc"),
        "a literal-needle rfind must NOT carry the bump allocator:\n{wat}"
    );
}

#[test]
fn heap_operand_pulls_allocator_and_literals() {
    // The `("l" + "o")` needle materialises a heap string, so the module MUST
    // carry the bump allocator and lay out the "l"/"o" literal `(data)` segments.
    let wat = emit_module(&heap_needle_module()).expect("the heap-needle program must lower");
    assert!(
        wat.contains("call $__wasm_str_rfind"),
        "the heap-needle module must still call the rfind helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $__alloc"),
        "a heap-constructed needle must pull in the bump allocator:\n{wat}"
    );
    assert!(
        wat.contains("$__wasm_concat_dst"),
        "the `\"l\" + \"o\"` needle must lower via the inline concat path:\n{wat}"
    );
    assert!(
        wat.contains("\\6c") && wat.contains("\\6f"),
        "the \"l\"/\"o\" needle literals must be laid out as (data) segments:\n{wat}"
    );
}

#[test]
fn real_rfind_programs_execute_in_wasm_and_match_cpython() {
    // Prove the emit path lowers (holds without WABT).
    emit_module(&rfind_module()).expect("rfind program lowers");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1143: skipping EXECUTED string rfind witness — WABT (wat2wasm / \
             wasm-interp) absent. The `s.rfind(p)` program lowered through \
             emit_module (asserted in `rfind_emits_helper_and_call`); a box with \
             WABT also runs all {} cases and asserts each == the pinned CPython \
             int. Free CI skips execution and stays green.",
            CASES.len()
        );
        return;
    }

    eprintln!("PMAT-1143: running EXECUTED string rfind witness via WABT");
    let mut checked = 0usize;
    for c in CASES {
        let got = exec_rfind(c.haystack, c.needle).expect("WABT present → a value");
        assert_eq!(
            got, c.rfind,
            "executed WASM `{:?}.rfind({:?})` = {got} but CPython = {}",
            c.haystack, c.needle, c.rfind
        );
        checked += 1;
    }
    eprintln!(
        "PMAT-1143: EXECUTED string rfind witness PASSED — {checked} cases lowered \
         through emit_module and executed in WABT, each value-matching CPython, \
         including the LAST-occurrence fixtures where rfind diverges from find \
         (\"hello\".rfind(\"l\")=3, \"banana\".rfind(\"ana\")=3, \
         \"mississippi\".rfind(\"ss\")=5), the CODE-POINT-index fixtures where a \
         byte rfind would diverge (\"héllo\".rfind(\"l\")=3 at byte offset 4; \
         \"abécdé\".rfind(\"é\")=5), the -1 absent/too-long/shared-byte cases, and \
         the empty-needle charlen answer (\"héllo\".rfind(\"\")=5, the code-point \
         length, NOT the byte length 6) — reverse byte search + byte→char-index \
         conversion, proven on silicon."
    );
}
