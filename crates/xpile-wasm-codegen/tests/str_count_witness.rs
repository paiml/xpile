//! PMAT-1128 — EXECUTED string OCCURRENCE-COUNT (`s.count(p)`) witness for the
//! native WASM EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The substring slice (`str_contains_witness.rs`) wired `needle in haystack`
//! via `$__wasm_str_contains` — a byte SEARCH that returns at the FIRST match.
//! This slice adds Python's `str.count` (`Expr::StrMethod`, op `Count`) via
//! `$__wasm_str_count` — the counting generalisation: the same byte slide, but
//! it COUNTS non-overlapping matches (advancing the cursor by `len(needle)` on
//! each) and returns the total as an `i64` (a Python `int`).
//!
//! ## Why a byte slide IS Python's `str.count`
//!
//! For a NON-EMPTY needle, the count is a pure NUMBER of matches — identical in
//! byte- or code-point-space. Both operands are valid UTF-8, so `needle[0]` is a
//! LEAD byte (never a `0x80..0xBF` continuation): every byte match lands on a
//! CHAR boundary in the haystack, so a `len(needle)`-byte match is exactly a
//! `needle`-code-point match — no split char, no false positive on a SHARED
//! continuation byte, and the non-overlapping advance-by-`len(needle)` matches
//! CPython's (and Rust's `str::matches`) left-to-right, non-overlapping rule.
//! The EMPTY needle is the ONE case where bytes and code points diverge: Python
//! `s.count("")` is `len(s) + 1` in CODE POINTS, so the helper returns
//! `charlen(s) + 1` (`"héllo".count("")` == 6, not the byte-derived 7).
//!
//! The witness proves this on fixtures where a naive byte count could diverge:
//!   * `"aaa".count("aa")` → 1 — NON-OVERLAPPING (a byte count that re-scanned
//!     from `start+1` would wrongly return 2).
//!   * `"banana".count("ana")` → 1 — the second "ana" overlaps the first, so
//!     non-overlapping counting skips it.
//!   * `"héllo".count("")` → 6 — the CODE-POINT (not byte) empty-needle count.
//!   * `"héllo".count("l")` → 2 — a match past a multi-byte char (`é`), proving
//!     the byte offsets stay char-aligned.
//!
//! ## The real program
//!
//! ```python
//! def cnt(s: str, p: str) -> int:
//!     return s.count(p)
//! ```
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the helper + call) on a host without WABT. The pinned
//! CPython ints are cross-checked against Rust's `str::matches().count()` (which
//! equals Python's `str.count` for valid UTF-8, empty needle included).

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// A single `(haystack, needle)` fixture with its pinned CPython `haystack
/// .count(needle)` result. `python3 -c "print('{haystack}'.count('{needle}'))"`.
struct Case {
    haystack: &'static str,
    needle: &'static str,
    count: i64,
}

/// The witness fixtures — ASCII (single / multi / non-overlapping / absent /
/// equal / longer-than / empty), MULTI-BYTE (é / count past a multi-byte char /
/// empty-needle code-point count), and OVERLAP fixtures (`"aaa".count("aa")`,
/// `"banana".count("ana")`) where naive re-scanning would over-count. Each
/// pinned int is the CPython ground truth (asserted == Rust `str::matches`
/// count in `cpython_count_is_pinned`).
const CASES: &[Case] = &[
    // ── ASCII ────────────────────────────────────────────────────────────
    Case {
        haystack: "hello",
        needle: "l",
        count: 2,
    }, // single char, twice
    Case {
        haystack: "hello",
        needle: "ll",
        count: 1,
    },
    Case {
        haystack: "hello",
        needle: "z",
        count: 0,
    }, // absent
    Case {
        haystack: "hello",
        needle: "hello",
        count: 1,
    }, // equal
    Case {
        haystack: "hello",
        needle: "helloo",
        count: 0,
    }, // needle LONGER than haystack
    Case {
        haystack: "banana",
        needle: "a",
        count: 3,
    },
    Case {
        haystack: "banana",
        needle: "na",
        count: 2,
    },
    Case {
        haystack: "aaaa",
        needle: "aa",
        count: 2,
    }, // NON-overlapping: [0,2), [2,4)
    // ── OVERLAP fixtures — non-overlapping counting is the whole point ────
    Case {
        haystack: "aaa",
        needle: "aa",
        count: 1,
    }, // match at 0 consumes [0,2); "a" left → 1, NOT 2
    Case {
        haystack: "banana",
        needle: "ana",
        count: 1,
    }, // "ana" at 1 consumes [1,4); the 2nd "ana" (at 3) overlaps → skipped
    Case {
        haystack: "aaaaa",
        needle: "aa",
        count: 2,
    }, // [0,2),[2,4); index 4 "a" left → 2
    // ── EMPTY needle — Python s.count("") == len(s) + 1 (CODE POINTS) ─────
    Case {
        haystack: "hello",
        needle: "",
        count: 6,
    },
    Case {
        haystack: "",
        needle: "",
        count: 1,
    },
    Case {
        haystack: "abc",
        needle: "",
        count: 4,
    },
    Case {
        haystack: "",
        needle: "a",
        count: 0,
    },
    // ── MULTI-BYTE (é = 0xC3 0xA9) ───────────────────────────────────────
    Case {
        haystack: "héllo",
        needle: "é",
        count: 1,
    }, // genuine multi-byte needle
    Case {
        haystack: "héllo",
        needle: "l",
        count: 2,
    }, // matches PAST the multi-byte char — byte offsets stay char-aligned
    Case {
        haystack: "héllo",
        needle: "",
        count: 6,
    }, // CODE-POINT count + 1 (byte-count + 1 would be 7)
    Case {
        haystack: "café",
        needle: "é",
        count: 1,
    },
    Case {
        haystack: "café",
        needle: "©",
        count: 0,
    }, // © (0xC2 0xA9) shares the trailing 0xA9 with é — NOT a false positive
    Case {
        haystack: "ababab",
        needle: "ab",
        count: 3,
    },
];

/// Fixed, non-overlapping addresses for the two preloaded str params, below
/// `LITERAL_BASE` (= 512) and the bump heap (>= 1024). Each is a length-prefixed
/// region (i32 BYTE count @ base+0, UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;
const P_ADDR: i32 = 256;

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def cnt(s: str, p: str) -> int: return s.count(p)` — i.e. `StrMethod {
/// recv: s, op: Count, args: [p] }`.
fn count_module() -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::Count,
        args: vec![Expr::Ident("p".into())],
    };
    let f = Function {
        name: "cnt".into(),
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
        name: "count_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// A LITERAL-arg module: `def cnt_l(s: str) -> int: return s.count("l")`. The
/// needle `"l"` is an `Expr::LitStr`, so this exercises the PMAT-1128
/// `collect_expr_literals` StrMethod arm — the "l" literal MUST be laid out as a
/// `(data)` segment (before the fix a literal method arg found no address).
fn literal_needle_module() -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::Count,
        args: vec![Expr::LitStr("l".into())],
    };
    let f = Function {
        name: "cnt_l".into(),
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

/// A HEAP-operand module: `def cnt_lo(s: str) -> int: return s.count("l" + "o")`.
/// The needle `"l" + "o"` (`Expr::Concat`) materialises a heap string, so this
/// exercises the PMAT-1128 `expr_has_heap_op` StrMethod arm (the bump allocator
/// plus the "l"/"o" literal `(data)` segments must be gated in — before the fix
/// a heap method arg emitted `$__alloc` against an undeclared allocator).
fn heap_needle_module() -> Module {
    let needle = Expr::Concat {
        lhs: Box::new(Expr::LitStr("l".into())),
        rhs: Box::new(Expr::LitStr("o".into())),
    };
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::Count,
        args: vec![needle],
    };
    let f = Function {
        name: "cnt_lo".into(),
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
/// (`$cnt(S_ADDR, P_ADDR)`) onto the emitted module, before its closing `)`.
fn build_witness_wat(kernel_wat: &str, s: &str, p: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1128 witness: preload the two str params (below LITERAL_BASE)\n");
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
           i32.const {S_ADDR}\n    i32.const {P_ADDR}\n    call $cnt)\n"
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
    line[idx + "=> i64:".len()..]
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("parse i64 from {line:?}"))
}

/// Lower `s.count(p)`, run it in WABT with `s`/`p` preloaded, return the count.
/// `None` when WABT is absent (the caller skips the value assertion).
fn exec_count(s: &str, p: &str) -> Option<i64> {
    let kernel_wat = emit_module(&count_module()).expect("count program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, s, p);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-count-{}-{}",
        std::process::id(),
        s.len() * 131 + p.len() * 7
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("cnt.wat");
    let wasm_path = dir.join("cnt.wasm");
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

#[test]
fn cpython_count_is_pinned() {
    // Rust `str::matches(p).count()` is non-overlapping, left-to-right, and
    // matches on CHAR boundaries (empty pattern included), so it equals Python's
    // `str.count` for valid UTF-8 — it validates every pinned int.
    for c in CASES {
        assert_eq!(
            c.count,
            c.haystack.matches(c.needle).count() as i64,
            "count mismatch for {:?}.count({:?})",
            c.haystack,
            c.needle
        );
    }
    // The multi-byte fixtures MUST genuinely exercise a non-ASCII byte, else the
    // "byte occurrence count == code-point occurrence count" claim is untested.
    assert!(CASES
        .iter()
        .any(|c| !c.haystack.is_ascii() || !c.needle.is_ascii()));
    // A NON-OVERLAPPING fixture must be present (the whole point vs a naive
    // re-scan): "aaa".count("aa") == 1, not 2.
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "aaa" && c.needle == "aa" && c.count == 1));
    // The CODE-POINT empty-needle count must be present on a multi-byte string
    // (byte-count + 1 would give 7, not 6): "héllo".count("") == 6.
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "héllo" && c.needle.is_empty() && c.count == 6));
    // A shared-continuation-byte NEGATIVE case (no false positive): "©" in "café"
    // shares 0xA9 but count == 0.
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "café" && c.needle == "©" && c.count == 0));
}

#[test]
fn count_emits_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): `s.count(p)` lowers
    // through the production emitter, carrying its helper + call, declaring memory
    // (the search reads the str bytes), and NEVER pulling in the bump allocator (a
    // count over two str PARAMS allocates nothing).
    let wat = emit_module(&count_module()).expect("the `s.count(p)` program must lower");
    assert!(
        wat.contains("(func $__wasm_str_count (param $h i32) (param $n i32) (result i64)"),
        "the $__wasm_str_count helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_count"),
        "$cnt must call $__wasm_str_count:\n{wat}"
    );
    // The empty-needle case reads the CODE-POINT count, so the helper must call
    // $__wasm_str_charlen (emitted for any str-touching module).
    assert!(
        wat.contains("(func $__wasm_str_charlen"),
        "the empty-needle count needs $__wasm_str_charlen:\n{wat}"
    );
    assert!(
        wat.contains("(memory"),
        "the occurrence count needs memory declared:\n{wat}"
    );
    assert!(
        !wat.contains("(func $__alloc"),
        "a pure param count module must NOT carry the bump allocator:\n{wat}"
    );
    // A count-only module carries no prefix/suffix/contains helper (no dead
    // helper). Match the helper DEFINITION `(func $__wasm_str_…` — the bare name
    // `$__wasm_str_contains` also appears in the count helper's own doc comment
    // ("the counting generalisation of $__wasm_str_contains"), which is harmless.
    assert!(
        !wat.contains("(func $__wasm_str_startswith")
            && !wat.contains("(func $__wasm_str_endswith")
            && !wat.contains("(func $__wasm_str_contains"),
        "a count-only module carries no startswith/endswith/contains helper:\n{wat}"
    );
}

#[test]
fn literal_arg_lays_out_data() {
    // PMAT-1128 fix (collect_expr_literals StrMethod arm): `s.count("l")` MUST lay
    // out the "l" needle literal as a `(data)` segment — before the fix the
    // literal method arg fell through, so `emit_str_expr` found no address.
    let wat = emit_module(&literal_needle_module()).expect("the literal-needle program must lower");
    assert!(
        wat.contains("call $__wasm_str_count"),
        "the literal-needle module must still call the count helper:\n{wat}"
    );
    // The literal byte 'l' (0x6c) must appear as a (data) segment.
    assert!(
        wat.contains("\\6c"),
        "the \"l\" needle literal must be laid out as a (data) segment:\n{wat}"
    );
    // Still no allocator — a literal needle materialises nothing at runtime.
    assert!(
        !wat.contains("(func $__alloc"),
        "a literal-needle count must NOT carry the bump allocator:\n{wat}"
    );
}

#[test]
fn heap_operand_pulls_allocator_and_literals() {
    // PMAT-1128 fix (expr_has_heap_op StrMethod arm): the `("l" + "o")` needle
    // materialises a heap string, so the module MUST carry the bump allocator and
    // lay out the "l"/"o" literal `(data)` segments — before the fix a heap method
    // arg emitted `$__alloc` against an undeclared allocator (a hard assemble fail).
    let wat = emit_module(&heap_needle_module()).expect("the heap-needle program must lower");
    assert!(
        wat.contains("call $__wasm_str_count"),
        "the heap-needle module must still call the count helper:\n{wat}"
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
fn real_count_programs_execute_in_wasm_and_match_cpython() {
    // Prove the emit path lowers (holds without WABT).
    emit_module(&count_module()).expect("count program lowers");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1128: skipping EXECUTED string count witness — WABT (wat2wasm / \
             wasm-interp) absent. The `s.count(p)` program lowered through \
             emit_module (asserted in `count_emits_helper_and_call`); a box with \
             WABT also runs all {} cases and asserts each == the pinned CPython \
             int. Free CI skips execution and stays green.",
            CASES.len()
        );
        return;
    }

    eprintln!("PMAT-1128: running EXECUTED string count witness via WABT");
    let mut checked = 0usize;
    for c in CASES {
        let got = exec_count(c.haystack, c.needle).expect("WABT present → a value");
        assert_eq!(
            got, c.count,
            "executed WASM `{:?}.count({:?})` = {got} but CPython = {}",
            c.haystack, c.needle, c.count
        );
        checked += 1;
    }
    eprintln!(
        "PMAT-1128: EXECUTED string count witness PASSED — {checked} cases lowered \
         through emit_module and executed in WABT, each value-matching CPython, \
         including the NON-OVERLAPPING fixtures (\"aaa\".count(\"aa\")=1, \
         \"banana\".count(\"ana\")=1), the CODE-POINT empty-needle count \
         (\"héllo\".count(\"\")=6, not the byte-derived 7), and the multi-byte \
         count-past-a-char fixture (\"héllo\".count(\"l\")=2) — byte occurrence \
         count == code-point occurrence count, proven on silicon."
    );
}
