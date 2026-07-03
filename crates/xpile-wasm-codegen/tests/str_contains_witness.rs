//! PMAT-1127 — EXECUTED string SUBSTRING (`x in s`) witness for the native WASM
//! EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The prefix/suffix slice (`str_prefix_witness.rs`) wired `s.startswith(p)` /
//! `s.endswith(p)` via `$__wasm_str_startswith` / `$__wasm_str_endswith` — byte
//! compares pinned to ONE offset. This slice adds Python's substring test
//! `needle in haystack` (`Expr::StrContains`) via `$__wasm_str_contains` — a
//! non-allocating byte search that SLIDES the needle over every start offset.
//!
//! ## Why a byte search IS Python's `in`
//!
//! CPython's `in` tests a Unicode CODE-POINT substring. Both operands are valid
//! UTF-8, and UTF-8 is a self-synchronising PREFIX code: `needle[0]` is always a
//! LEAD byte (never a `0x80..0xBF` continuation), so ANY matching byte forces the
//! compare to begin on a CHAR boundary in the haystack. Hence a `len(needle)`-byte
//! match is exactly a `needle`-code-point match — no char walk, no split
//! multi-byte char, and no false positive from a SHARED continuation byte
//! straddling a boundary. The witness proves this on MULTI-BYTE fixtures where a
//! naive byte search could diverge:
//!   * `"é" in "héllo"` → True — `é` (0xC3 0xA9) is a genuine multi-byte needle.
//!   * `"©" in "héllo"` → False — `©` (0xC2 0xA9) SHARES the trailing 0xA9 with
//!     `é` (0xC3 0xA9) but its LEAD byte 0xC2 never appears, so no false positive.
//!   * `"ana" in "banana"` → True — matches at offset 1, NOT 0: exercises the
//!     SLIDE that startswith/endswith never do.
//!
//! ## The real program
//!
//! ```python
//! def has(s: str, p: str) -> bool:
//!     return p in s
//! ```
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the helper + call) on a host without WABT. The pinned
//! CPython booleans are cross-checked against Rust's `str::contains` (which
//! equals Python's `in` for valid UTF-8).

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// A single `(haystack, needle)` fixture with its pinned CPython `needle in
/// haystack` result. `python3 -c "print('{needle}' in '{haystack}')"`.
struct Case {
    haystack: &'static str,
    needle: &'static str,
    contained: bool,
}

/// The witness fixtures — ASCII (start / middle / end / absent / longer-than /
/// empty / equal / single-char) and MULTI-BYTE (é / © shared-continuation-byte)
/// pairs, plus a SLIDE fixture (`"ana" in "banana"` matches at offset 1). Each
/// pinned bool is the CPython ground truth (asserted == Rust `str::contains` in
/// `cpython_contains_is_pinned`).
const CASES: &[Case] = &[
    // ── ASCII ────────────────────────────────────────────────────────────
    Case {
        haystack: "hello",
        needle: "he",
        contained: true,
    }, // at start (a prefix)
    Case {
        haystack: "hello",
        needle: "ell",
        contained: true,
    }, // in the middle
    Case {
        haystack: "hello",
        needle: "lo",
        contained: true,
    }, // at end (a suffix)
    Case {
        haystack: "hello",
        needle: "hello",
        contained: true,
    }, // equal
    Case {
        haystack: "hello",
        needle: "l",
        contained: true,
    }, // single char, appears twice
    Case {
        haystack: "hello",
        needle: "xyz",
        contained: false,
    },
    Case {
        haystack: "hello",
        needle: "helloo",
        contained: false,
    }, // needle LONGER than haystack
    Case {
        haystack: "hello",
        needle: "ll0",
        contained: false,
    }, // matches then diverges at the last byte
    Case {
        haystack: "hello",
        needle: "",
        contained: true,
    }, // empty needle → True
    Case {
        haystack: "",
        needle: "",
        contained: true,
    },
    Case {
        haystack: "",
        needle: "a",
        contained: false,
    },
    // The SLIDE fixture — startswith/endswith would MISS this (offset != 0 and
    // != len(s)-len(p)); a naive one-offset compare returns the wrong answer.
    Case {
        haystack: "banana",
        needle: "ana",
        contained: true,
    }, // first match at offset 1
    Case {
        haystack: "banana",
        needle: "nana",
        contained: true,
    }, // at offset 2
    Case {
        haystack: "aaa",
        needle: "aa",
        contained: true,
    }, // overlapping candidate starts
    // ── MULTI-BYTE (é = 0xC3 0xA9, © = 0xC2 0xA9 — a SHARED continuation byte)
    Case {
        haystack: "héllo",
        needle: "é",
        contained: true,
    }, // genuine multi-byte needle
    Case {
        haystack: "héllo",
        needle: "©",
        contained: false,
    }, // NOT a false positive on the shared 0xA9
    Case {
        haystack: "héllo",
        needle: "hé",
        contained: true,
    }, // multi-byte prefix
    Case {
        haystack: "héllo",
        needle: "éllo",
        contained: true,
    }, // multi-byte suffix (offset on a char boundary)
    Case {
        haystack: "héllo",
        needle: "él",
        contained: true,
    }, // straddles é and l (offset 1)
    Case {
        haystack: "café",
        needle: "©",
        contained: false,
    },
    Case {
        haystack: "café",
        needle: "é",
        contained: true,
    },
];

/// Fixed, non-overlapping addresses for the two preloaded str params, below
/// `LITERAL_BASE` (= 512) and the bump heap (>= 1024). Each is a length-prefixed
/// region (i32 BYTE count @ base+0, UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;
const P_ADDR: i32 = 256;

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def has(s: str, p: str) -> bool: return p in s` — i.e. `StrContains {
/// haystack: s, needle: p }` (Python `p in s` tests p as a substring of s).
fn contains_module() -> Module {
    let body = Expr::StrContains {
        haystack: Box::new(Expr::Ident("s".into())),
        needle: Box::new(Expr::Ident("p".into())),
    };
    let f = Function {
        name: "has".into(),
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
        return_type: Type::Bool,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "contains_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// A HEAP-operand module: `def has_lo(s: str) -> bool: return ("l" + "o") in s`.
/// The needle `"l" + "o"` (`Expr::Concat`) materialises a heap string, so this
/// exercises the `expr_has_heap_op` / `collect_expr_literals` StrContains arms
/// (the allocator + literal `(data)` segments must be gated in).
fn heap_needle_module() -> Module {
    let needle = Expr::Concat {
        lhs: Box::new(Expr::LitStr("l".into())),
        rhs: Box::new(Expr::LitStr("o".into())),
    };
    let body = Expr::StrContains {
        haystack: Box::new(Expr::Ident("s".into())),
        needle: Box::new(needle),
    };
    let f = Function {
        name: "has_lo".into(),
        params: vec![Param {
            name: "s".into(),
            ty: Type::Str,
            mutable: false,
        }],
        return_type: Type::Bool,
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
/// (`$has(S_ADDR, P_ADDR)`) onto the emitted module, before its closing `)`.
fn build_witness_wat(kernel_wat: &str, s: &str, p: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1127 witness: preload the two str params (below LITERAL_BASE)\n");
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
        "  (func (export \"run\") (result i32)\n    \
           i32.const {S_ADDR}\n    i32.const {P_ADDR}\n    call $has)\n"
    ));
    wat.push_str(")\n");
    wat
}

/// Parse a `run() => i32:<value>` line from `wasm-interp --run-all-exports`.
fn parse_run_i32(stdout: &str) -> i32 {
    let line = stdout
        .lines()
        .find(|l| l.contains("run() => i32:"))
        .unwrap_or_else(|| panic!("no `run` i32 export in interp output:\n{stdout}"));
    let idx = line.find("=> i32:").unwrap();
    line[idx + "=> i32:".len()..]
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("parse i32 from {line:?}"))
}

/// Lower `p in s`, run it in WABT with `s`/`p` preloaded, return the bool.
/// `None` when WABT is absent (the caller skips the value assertion).
fn exec_contains(s: &str, p: &str) -> Option<bool> {
    let kernel_wat = emit_module(&contains_module()).expect("contains program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, s, p);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-contains-{}-{}",
        std::process::id(),
        s.len() * 131 + p.len() * 7
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("has.wat");
    let wasm_path = dir.join("has.wasm");
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
    Some(parse_run_i32(&stdout) != 0)
}

#[test]
fn cpython_contains_is_pinned() {
    // Rust `str::contains` operates on the byte sequence of valid UTF-8 ==
    // Python's code-point `in`, so it validates every pinned bool.
    for c in CASES {
        assert_eq!(
            c.contained,
            c.haystack.contains(c.needle),
            "contains mismatch for {:?} in {:?}",
            c.needle,
            c.haystack
        );
    }
    // The multi-byte fixtures MUST genuinely exercise a non-ASCII byte, else the
    // "byte substring == code-point substring" claim is untested.
    assert!(CASES
        .iter()
        .any(|c| !c.haystack.is_ascii() || !c.needle.is_ascii()));
    // A shared-continuation-byte NEGATIVE case must be present (the false-
    // positive guard): "©" in "héllo" shares 0xA9 but is False.
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "héllo" && c.needle == "©" && !c.contained));
    // A genuine SLIDE case must be present (match at an interior offset): "ana"
    // in "banana" — startswith/endswith would return the wrong answer.
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "banana" && c.needle == "ana" && c.contained));
}

#[test]
fn contains_emits_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): `p in s` lowers through
    // the production emitter, carrying its helper + call, declaring memory (the
    // search reads the str bytes), and NEVER pulling in the bump allocator (a
    // bool predicate over two str PARAMS allocates nothing).
    let wat = emit_module(&contains_module()).expect("the `p in s` program must lower");
    assert!(
        wat.contains("(func $__wasm_str_contains (param $h i32) (param $n i32) (result i32)"),
        "the $__wasm_str_contains helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_contains"),
        "$has must call $__wasm_str_contains:\n{wat}"
    );
    assert!(
        wat.contains("(memory"),
        "the substring test needs memory declared:\n{wat}"
    );
    assert!(
        !wat.contains("(func $__alloc"),
        "a pure param predicate module must NOT carry the bump allocator:\n{wat}"
    );
    // The contains helper is independent of the prefix/suffix helpers — a
    // contains-only module carries neither (no dead helper).
    assert!(
        !wat.contains("$__wasm_str_startswith") && !wat.contains("$__wasm_str_endswith"),
        "a contains-only module carries no startswith/endswith helper:\n{wat}"
    );
}

#[test]
fn heap_operand_pulls_allocator_and_literals() {
    // The `("l" + "o") in s` needle materialises a heap string, so the module
    // MUST carry the bump allocator (the `expr_has_heap_op` StrContains arm) and
    // lay out the "l"/"o" literal `(data)` segments (the `collect_expr_literals`
    // arm). A MISS in either would fail to assemble.
    let wat = emit_module(&heap_needle_module()).expect("the heap-needle program must lower");
    assert!(
        wat.contains("call $__wasm_str_contains"),
        "the heap-needle module must still call the contains helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $__alloc"),
        "a heap-constructed needle must pull in the bump allocator:\n{wat}"
    );
    assert!(
        wat.contains("$__wasm_concat_dst"),
        "the `\"l\" + \"o\"` needle must lower via the inline concat path (its \
         dedicated $__wasm_concat_dst scratch):\n{wat}"
    );
    // The literal bytes 'l' (0x6c) and 'o' (0x6f) must appear as (data) segments.
    assert!(
        wat.contains("\\6c") && wat.contains("\\6f"),
        "the \"l\"/\"o\" needle literals must be laid out as (data) segments:\n{wat}"
    );
}

#[test]
fn real_contains_programs_execute_in_wasm_and_match_cpython() {
    // Prove the emit path lowers (holds without WABT).
    emit_module(&contains_module()).expect("contains program lowers");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1127: skipping EXECUTED string substring witness — WABT \
             (wat2wasm / wasm-interp) absent. The `p in s` program lowered through \
             emit_module (asserted in `contains_emits_helper_and_call`); a box with \
             WABT also runs all {} cases and asserts each == the pinned CPython \
             bool. Free CI skips execution and stays green.",
            CASES.len()
        );
        return;
    }

    eprintln!("PMAT-1127: running EXECUTED string substring witness via WABT");
    let mut checked = 0usize;
    for c in CASES {
        let got = exec_contains(c.haystack, c.needle).expect("WABT present → a value");
        assert_eq!(
            got, c.contained,
            "executed WASM `{:?} in {:?}` = {got} but CPython = {}",
            c.needle, c.haystack, c.contained
        );
        checked += 1;
    }
    eprintln!(
        "PMAT-1127: EXECUTED string substring witness PASSED — {checked} cases \
         lowered through emit_module and executed in WABT, each value-matching \
         CPython, including the SLIDE fixtures (\"ana\" in \"banana\"=True at \
         offset 1) and the MULTI-BYTE fixtures (\"é\" in \"héllo\"=True, \"©\" in \
         \"héllo\"=False on a shared 0xA9 — byte substring == code-point \
         substring, proven on silicon, never a split char or false positive)."
    );
}
