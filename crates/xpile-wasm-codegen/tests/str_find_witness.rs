//! PMAT-1136 — EXECUTED string FIND (`s.find(p)`) witness for the native WASM
//! EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The substring slice (`str_contains_witness.rs`) wired `needle in haystack`
//! via `$__wasm_str_contains` — a byte SEARCH that returns a BOOL at the first
//! match. This slice adds Python's `str.find` (`Expr::StrMethod`, op `Find`) via
//! `$__wasm_str_find` — the index-returning sibling: the same byte slide, but it
//! returns WHERE the first match starts (or `-1` if absent) as an `i64`.
//!
//! ## Why the find index must be CODE-POINT, not byte
//!
//! `contains`/`count` return a bool / a match count — answers that are IDENTICAL
//! in byte- or code-point-space (a byte-substring match IS a code-point match for
//! valid UTF-8, since `needle[0]` is a LEAD byte). `find` is different: it returns
//! a POSITION, and Python `str.find` returns the position in CODE POINTS, not
//! bytes. So `$__wasm_str_find`, on a match at byte offset `start`, converts
//! `start` to a code-point index by counting the non-continuation bytes in
//! `haystack[0..start]` (`(b & 0xC0) != 0x80`). This is the ONE search op that
//! must diverge from the byte world. The witness proves it on fixtures where a
//! naive BYTE index would silently diverge:
//!   * `"héllo".find("llo")` → 2 — the byte offset is 3 (`é` is 2 bytes), so a
//!     byte-index find would wrongly return 3.
//!   * `"café".find("é")` → 3 — a match AT a multi-byte char (byte offset 3).
//!   * `"héllo".find("l")` → 2 — a match PAST a multi-byte char.
//!
//! Plus the boundary cases pinned to CPython:
//!   * absent → `-1` (`"hello".find("z")`).
//!   * needle LONGER than haystack → `-1`.
//!   * EMPTY needle → `0` (`"abc".find("")` and `"".find("")` are both 0).
//!   * a shared-continuation-byte NEGATIVE (`"café".find("©")` → -1: `©`
//!     (0xC2 0xA9) shares the trailing 0xA9 with `é` but is not a substring).
//!
//! ## The real program
//!
//! ```python
//! def fnd(s: str, p: str) -> int:
//!     return s.find(p)
//! ```
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the helper + call) on a host without WABT. The pinned
//! CPython ints are cross-checked against Rust's byte `str::find` converted to a
//! code-point index (`h[..byte].chars().count()`), which equals Python's
//! char-indexed `str.find` for valid UTF-8.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// A single `(haystack, needle)` fixture with its pinned CPython `haystack
/// .find(needle)` result. `python3 -c "print('{haystack}'.find('{needle}'))"`.
struct Case {
    haystack: &'static str,
    needle: &'static str,
    find: i64,
}

/// The witness fixtures — ASCII (present / absent / at-start / at-end / equal /
/// longer-than / empty), MULTI-BYTE (find AT / PAST a multi-byte char, where a
/// byte index would diverge), and a shared-continuation-byte NEGATIVE. Each
/// pinned int is the CPython ground truth (asserted == the Rust
/// byte-find→code-point-index conversion in `cpython_find_is_pinned`).
const CASES: &[Case] = &[
    // ── ASCII ────────────────────────────────────────────────────────────
    Case {
        haystack: "hello",
        needle: "l",
        find: 2,
    }, // first of two
    Case {
        haystack: "hello",
        needle: "ll",
        find: 2,
    },
    Case {
        haystack: "hello",
        needle: "h",
        find: 0,
    }, // at the start
    Case {
        haystack: "hello",
        needle: "o",
        find: 4,
    }, // at the end
    Case {
        haystack: "hello",
        needle: "z",
        find: -1,
    }, // absent
    Case {
        haystack: "hello",
        needle: "hello",
        find: 0,
    }, // equal
    Case {
        haystack: "hello",
        needle: "helloo",
        find: -1,
    }, // needle LONGER than haystack
    Case {
        haystack: "banana",
        needle: "ana",
        find: 1,
    }, // first "ana" (at 1), not the overlapping one
    Case {
        haystack: "banana",
        needle: "na",
        find: 2,
    },
    // ── EMPTY needle — Python "…".find("") == 0 ──────────────────────────
    Case {
        haystack: "hello",
        needle: "",
        find: 0,
    },
    Case {
        haystack: "",
        needle: "",
        find: 0,
    },
    Case {
        haystack: "abc",
        needle: "",
        find: 0,
    },
    Case {
        haystack: "",
        needle: "a",
        find: -1,
    },
    // ── MULTI-BYTE (é = 0xC3 0xA9) — CODE-POINT index, not byte offset ────
    Case {
        haystack: "héllo",
        needle: "llo",
        find: 2,
    }, // byte offset is 3 (é is 2 bytes) — char index is 2
    Case {
        haystack: "héllo",
        needle: "l",
        find: 2,
    }, // match PAST the multi-byte char
    Case {
        haystack: "héllo",
        needle: "é",
        find: 1,
    }, // match AT the multi-byte char
    Case {
        haystack: "café",
        needle: "é",
        find: 3,
    }, // multi-byte char at the end (char index 3, byte offset 3)
    Case {
        haystack: "café",
        needle: "©",
        find: -1,
    }, // © (0xC2 0xA9) shares the trailing 0xA9 with é — NOT a substring
    Case {
        haystack: "naïve café",
        needle: "café",
        find: 6,
    }, // two multi-byte chars before the match ("ï" at char 2)
    Case {
        haystack: "abécdé",
        needle: "dé",
        find: 4,
    }, // second é; char index 4, byte offset 5
];

/// Fixed, non-overlapping addresses for the two preloaded str params, below
/// `LITERAL_BASE` (= 512) and the bump heap (>= 1024). Each is a length-prefixed
/// region (i32 BYTE count @ base+0, UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;
const P_ADDR: i32 = 256;

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def fnd(s: str, p: str) -> int: return s.find(p)` — i.e. `StrMethod {
/// recv: s, op: Find, args: [p] }`.
fn find_module() -> Module {
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
    Module {
        name: "find_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// A LITERAL-arg module: `def fnd_l(s: str) -> int: return s.find("l")`. The
/// needle `"l"` is an `Expr::LitStr`, so this exercises the PMAT-1128
/// `collect_expr_literals` StrMethod arm — the "l" literal MUST be laid out as a
/// `(data)` segment (a literal method arg with no address fails to lower).
fn literal_needle_module() -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::Find,
        args: vec![Expr::LitStr("l".into())],
    };
    let f = Function {
        name: "fnd_l".into(),
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

/// A HEAP-operand module: `def fnd_lo(s: str) -> int: return s.find("l" + "o")`.
/// The needle `"l" + "o"` (`Expr::Concat`) materialises a heap string, so this
/// exercises the PMAT-1128 `expr_has_heap_op` StrMethod arm (the bump allocator
/// plus the "l"/"o" literal `(data)` segments must be gated in — a heap method
/// arg would otherwise emit `$__alloc` against an undeclared allocator).
fn heap_needle_module() -> Module {
    let needle = Expr::Concat {
        lhs: Box::new(Expr::LitStr("l".into())),
        rhs: Box::new(Expr::LitStr("o".into())),
    };
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::Find,
        args: vec![needle],
    };
    let f = Function {
        name: "fnd_lo".into(),
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
/// (`$fnd(S_ADDR, P_ADDR)`) onto the emitted module, before its closing `)`.
fn build_witness_wat(kernel_wat: &str, s: &str, p: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1136 witness: preload the two str params (below LITERAL_BASE)\n");
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
           i32.const {S_ADDR}\n    i32.const {P_ADDR}\n    call $fnd)\n"
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

/// Lower `s.find(p)`, run it in WABT with `s`/`p` preloaded, return the index.
/// `None` when WABT is absent (the caller skips the value assertion).
fn exec_find(s: &str, p: &str) -> Option<i64> {
    let kernel_wat = emit_module(&find_module()).expect("find program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, s, p);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-find-{}-{}",
        std::process::id(),
        s.len() * 131 + p.len() * 7
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("fnd.wat");
    let wasm_path = dir.join("fnd.wasm");
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

/// Python `str.find` = the CODE-POINT index of the first occurrence, or -1.
/// Rust's `str::find` returns a BYTE offset, so convert it to a code-point index
/// by counting the chars in the prefix — this is exactly Python's char-indexed
/// find for valid UTF-8, and validates every pinned int independently.
fn rust_char_find(h: &str, n: &str) -> i64 {
    match h.find(n) {
        Some(byte) => h[..byte].chars().count() as i64,
        None => -1,
    }
}

#[test]
fn cpython_find_is_pinned() {
    for c in CASES {
        assert_eq!(
            c.find,
            rust_char_find(c.haystack, c.needle),
            "find mismatch for {:?}.find({:?})",
            c.haystack,
            c.needle
        );
    }
    // The multi-byte fixtures MUST genuinely exercise a non-ASCII byte, else the
    // "byte offset → code-point index" conversion is untested.
    assert!(CASES
        .iter()
        .any(|c| !c.haystack.is_ascii() || !c.needle.is_ascii()));
    // A fixture where the CODE-POINT index differs from the BYTE offset must be
    // present (the whole point vs a naive byte find): "héllo".find("llo") == 2
    // (code points), but the byte offset is 3 (é is 2 bytes).
    assert!(CASES.iter().any(|c| {
        c.haystack == "héllo"
            && c.needle == "llo"
            && c.find == 2
            && c.haystack.find(c.needle) == Some(3) // byte offset genuinely differs
    }));
    // The empty-needle answer must be pinned to 0 (Python "…".find("") == 0).
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "abc" && c.needle.is_empty() && c.find == 0));
    // A shared-continuation-byte NEGATIVE (no false positive): "©" in "café"
    // shares 0xA9 but find == -1.
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "café" && c.needle == "©" && c.find == -1));
    // An ABSENT fixture must return -1 (not a bogus non-negative index).
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "hello" && c.needle == "z" && c.find == -1));
}

#[test]
fn find_emits_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): `s.find(p)` lowers
    // through the production emitter, carrying its helper + call, declaring memory
    // (the search reads the str bytes), and NEVER pulling in the bump allocator (a
    // find over two str PARAMS allocates nothing).
    let wat = emit_module(&find_module()).expect("the `s.find(p)` program must lower");
    assert!(
        wat.contains("(func $__wasm_str_find (param $h i32) (param $n i32) (result i64)"),
        "the $__wasm_str_find helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_find"),
        "$fnd must call $__wasm_str_find:\n{wat}"
    );
    assert!(
        wat.contains("(memory"),
        "the byte search needs memory declared:\n{wat}"
    );
    assert!(
        !wat.contains("(func $__alloc"),
        "a pure param find module must NOT carry the bump allocator:\n{wat}"
    );
    // A find-only module carries no prefix/suffix/contains/count helper (no dead
    // helper). Match the helper DEFINITION `(func $__wasm_str_…`.
    assert!(
        !wat.contains("(func $__wasm_str_startswith")
            && !wat.contains("(func $__wasm_str_endswith")
            && !wat.contains("(func $__wasm_str_contains")
            && !wat.contains("(func $__wasm_str_count"),
        "a find-only module carries no startswith/endswith/contains/count helper:\n{wat}"
    );
}

#[test]
fn literal_arg_lays_out_data() {
    // PMAT-1128 fix (collect_expr_literals StrMethod arm), find edition: `s.find("l")`
    // MUST lay out the "l" needle literal as a `(data)` segment.
    let wat = emit_module(&literal_needle_module()).expect("the literal-needle program must lower");
    assert!(
        wat.contains("call $__wasm_str_find"),
        "the literal-needle module must still call the find helper:\n{wat}"
    );
    // The literal byte 'l' (0x6c) must appear as a (data) segment.
    assert!(
        wat.contains("\\6c"),
        "the \"l\" needle literal must be laid out as a (data) segment:\n{wat}"
    );
    // Still no allocator — a literal needle materialises nothing at runtime.
    assert!(
        !wat.contains("(func $__alloc"),
        "a literal-needle find must NOT carry the bump allocator:\n{wat}"
    );
}

#[test]
fn heap_operand_pulls_allocator_and_literals() {
    // PMAT-1128 fix (expr_has_heap_op StrMethod arm), find edition: the `("l" + "o")`
    // needle materialises a heap string, so the module MUST carry the bump
    // allocator and lay out the "l"/"o" literal `(data)` segments.
    let wat = emit_module(&heap_needle_module()).expect("the heap-needle program must lower");
    assert!(
        wat.contains("call $__wasm_str_find"),
        "the heap-needle module must still call the find helper:\n{wat}"
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
fn real_find_programs_execute_in_wasm_and_match_cpython() {
    // Prove the emit path lowers (holds without WABT).
    emit_module(&find_module()).expect("find program lowers");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1136: skipping EXECUTED string find witness — WABT (wat2wasm / \
             wasm-interp) absent. The `s.find(p)` program lowered through \
             emit_module (asserted in `find_emits_helper_and_call`); a box with \
             WABT also runs all {} cases and asserts each == the pinned CPython \
             int. Free CI skips execution and stays green.",
            CASES.len()
        );
        return;
    }

    eprintln!("PMAT-1136: running EXECUTED string find witness via WABT");
    let mut checked = 0usize;
    for c in CASES {
        let got = exec_find(c.haystack, c.needle).expect("WABT present → a value");
        assert_eq!(
            got, c.find,
            "executed WASM `{:?}.find({:?})` = {got} but CPython = {}",
            c.haystack, c.needle, c.find
        );
        checked += 1;
    }
    eprintln!(
        "PMAT-1136: EXECUTED string find witness PASSED — {checked} cases lowered \
         through emit_module and executed in WABT, each value-matching CPython, \
         including the CODE-POINT-index fixtures where a byte find would diverge \
         (\"héllo\".find(\"llo\")=2 at byte offset 3; \"café\".find(\"é\")=3; \
         \"naïve café\".find(\"café\")=6), the -1 absent/too-long/shared-byte \
         cases, and the empty-needle 0 — byte search + byte→char-index \
         conversion, proven on silicon."
    );
}
