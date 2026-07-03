//! PMAT-1144 — EXECUTED string INDEX / RINDEX (`s.index(p)` / `s.rindex(p)`)
//! witness for the native WASM EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The find/rfind slices (`str_find_witness.rs` / `str_rfind_witness.rs`) wired
//! `s.find(p)` / `s.rfind(p)` via `$__wasm_str_find` / `$__wasm_str_rfind` — byte
//! searches returning the CODE-POINT index of the FIRST / LAST match, or `-1`
//! when absent. This slice adds Python's `str.index` / `str.rindex`
//! (`Expr::StrMethod`, ops `StrIndex` / `RIndex`) via `$__wasm_str_index` /
//! `$__wasm_str_rindex` — the TRAPPING siblings.
//!
//! ## Why index/rindex are a genuine increment over find/rfind (not a rename)
//!
//! `str.index` is `str.find` and `str.rindex` is `str.rfind` on a PRESENT needle
//! — same CODE-POINT index, same empty-needle answer, same multi-byte
//! correctness. The ONE observable difference: a MISSING needle raises
//! `ValueError` in CPython instead of returning `-1`. The WASM ABI has no
//! exceptions, so the honest analogue of a Python exception is a **trap** — the
//! wrapper `unreachable`s when its wrapped search returns `-1`. The witness pins
//! BOTH halves:
//!   * present → the exact CODE-POINT index (identical to find/rfind);
//!   * absent → a WASM trap (`wasm-interp` reports `run() => error: unreachable
//!     executed`), matching CPython raising `ValueError` — NOT `-1`.
//!
//! ## Why index and rindex genuinely disagree
//!
//! On a string with more than one occurrence they diverge exactly as find/rfind
//! do (index = first, rindex = last):
//!   * `"hello".index("l")` → 2, `"hello".rindex("l")` → 3.
//!   * `"banana".index("ana")` → 1, `"banana".rindex("ana")` → 3.
//!   * `"abécdé".index("é")` → 2, `"abécdé".rindex("é")` → 5.
//!
//! ## Why the index must be CODE-POINT, not byte (inherited from find/rfind)
//!
//! Python `str.index`/`rindex` return CODE-POINT positions. The wrappers add no
//! conversion of their own — they return the wrapped search's already-char-indexed
//! result — so the multi-byte fixtures (where a byte index would silently diverge)
//! ride through unchanged: `"héllo".rindex("l")` → 3 (byte offset 4, é is 2 bytes),
//! `"café".index("é")` → 3.
//!
//! ## Why the EMPTY needle never traps
//!
//! The empty string is always "found" — `"abc".index("")` == 0 and
//! `"abc".rindex("")` == 3 (rindex inherits rfind's charlen-at-END answer, NOT 0).
//! Neither raises `ValueError`, so neither traps. The witness pins the empty-needle
//! rindex answer to the CODE-POINT length (`"héllo".rindex("")` == 5, NOT the byte
//! length 6).
//!
//! ## The trapping boundary cases (Python `ValueError`)
//!
//!   * absent → trap (`"hello".index("z")`, `"café".rindex("©")` — the shared
//!     continuation byte 0xA9 is NOT a substring).
//!   * needle LONGER than haystack → trap (`"hello".index("helloo")` — NOT `-1`).
//!   * absent in the empty haystack → trap (`"".index("a")`).
//!
//! ## The real programs
//!
//! ```python
//! def idx(s: str, p: str) -> int:  return s.index(p)
//! def ridx(s: str, p: str) -> int: return s.rindex(p)
//! ```
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the wrapper + wrapped helper + call) on a host without
//! WABT. The pinned CPython outcomes are cross-checked against Rust's byte
//! `str::find`/`str::rfind` converted to a code-point index (`None` → trap), which
//! equals Python's char-indexed `str.index`/`str.rindex` for valid UTF-8.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// A single `(haystack, needle)` fixture with its pinned CPython `index` /
/// `rindex` outcomes. `None` = CPython raises `ValueError` (the needle is absent)
/// → a WASM trap; `Some(i)` = the CODE-POINT index.
/// `python3 -c "s='…'; print(s.index('…'))"` (ValueError ↔ None).
struct Case {
    haystack: &'static str,
    needle: &'static str,
    index: Option<i64>,
    rindex: Option<i64>,
}

/// The witness fixtures — ASCII (first/last-of-many where index and rindex
/// DIVERGE, at-start/at-end, absent → trap, equal, longer-than → trap),
/// EMPTY-needle (never traps; rindex = charlen at END), MULTI-BYTE (index AT / PAST
/// a multi-byte char, where a byte index would diverge), and a
/// shared-continuation-byte NEGATIVE (trap). Each pinned outcome is the CPython
/// ground truth (asserted == the Rust byte-search→code-point-index conversion in
/// `cpython_index_is_pinned`).
const CASES: &[Case] = &[
    // ── ASCII — index=first, rindex=last (they DIVERGE on >1 occurrence) ─────
    Case {
        haystack: "hello",
        needle: "l",
        index: Some(2),
        rindex: Some(3),
    }, // first l=2, last l=3
    Case {
        haystack: "hello",
        needle: "ll",
        index: Some(2),
        rindex: Some(2),
    }, // single occurrence — index == rindex
    Case {
        haystack: "hello",
        needle: "h",
        index: Some(0),
        rindex: Some(0),
    }, // at the start
    Case {
        haystack: "hello",
        needle: "o",
        index: Some(4),
        rindex: Some(4),
    }, // at the end
    Case {
        haystack: "hello",
        needle: "z",
        index: None,
        rindex: None,
    }, // ABSENT → ValueError → trap (NOT -1)
    Case {
        haystack: "hello",
        needle: "hello",
        index: Some(0),
        rindex: Some(0),
    }, // equal
    Case {
        haystack: "hello",
        needle: "helloo",
        index: None,
        rindex: None,
    }, // needle LONGER than haystack → trap (NOT -1)
    Case {
        haystack: "banana",
        needle: "ana",
        index: Some(1),
        rindex: Some(3),
    }, // first "ana"=1, last (overlapping)=3
    Case {
        haystack: "banana",
        needle: "na",
        index: Some(2),
        rindex: Some(4),
    },
    Case {
        haystack: "mississippi",
        needle: "ss",
        index: Some(2),
        rindex: Some(5),
    },
    Case {
        haystack: "aaa",
        needle: "a",
        index: Some(0),
        rindex: Some(2),
    }, // first of three=0, last=2
    // ── EMPTY needle — NEVER traps; rindex = charlen at the END (NOT 0) ──────
    Case {
        haystack: "hello",
        needle: "",
        index: Some(0),
        rindex: Some(5),
    },
    Case {
        haystack: "",
        needle: "",
        index: Some(0),
        rindex: Some(0),
    },
    Case {
        haystack: "abc",
        needle: "",
        index: Some(0),
        rindex: Some(3),
    },
    Case {
        haystack: "",
        needle: "a",
        index: None,
        rindex: None,
    }, // absent in the empty haystack → trap
    // ── MULTI-BYTE (é = 0xC3 0xA9) — CODE-POINT index, not byte offset ───────
    Case {
        haystack: "héllo",
        needle: "llo",
        index: Some(2),
        rindex: Some(2),
    }, // single occurrence; byte offset 3 (é is 2 bytes) — char index 2
    Case {
        haystack: "héllo",
        needle: "l",
        index: Some(2),
        rindex: Some(3),
    }, // first l byte 3/char 2, last l byte 4/char 3 (a byte search would diverge)
    Case {
        haystack: "héllo",
        needle: "é",
        index: Some(1),
        rindex: Some(1),
    }, // AT the multi-byte char
    Case {
        haystack: "café",
        needle: "é",
        index: Some(3),
        rindex: Some(3),
    }, // multi-byte char at the end (char index 3, byte offset 3)
    Case {
        haystack: "café",
        needle: "©",
        index: None,
        rindex: None,
    }, // © (0xC2 0xA9) shares the trailing 0xA9 with é — NOT a substring → trap
    Case {
        haystack: "naïve café",
        needle: "café",
        index: Some(6),
        rindex: Some(6),
    }, // two multi-byte chars before the match ("ï" at char 2)
    Case {
        haystack: "abécdé",
        needle: "é",
        index: Some(2),
        rindex: Some(5),
    }, // first é char 2/byte 2, second é char 5/byte 6 — index and rindex DIVERGE
    // The empty-needle MULTI-BYTE rindex case — charlen (5), NOT byte length (6).
    Case {
        haystack: "héllo",
        needle: "",
        index: Some(0),
        rindex: Some(5),
    },
];

/// Fixed, non-overlapping addresses for the two preloaded str params, below
/// `LITERAL_BASE` (= 512) and the bump heap (>= 1024). Each is a length-prefixed
/// region (i32 BYTE count @ base+0, UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;
const P_ADDR: i32 = 256;

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def idx(s: str, p: str) -> int: return s.<op>(p)` — i.e. `StrMethod { recv:
/// s, op, args: [p] }`. `op` is `StrIndex` (`.index`) or `RIndex` (`.rindex`).
fn method_module(name: &str, op: StrMethodOp) -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op,
        args: vec![Expr::Ident("p".into())],
    };
    let f = Function {
        name: name.into(),
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
        name: format!("{name}_program"),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

fn index_module() -> Module {
    method_module("idx", StrMethodOp::StrIndex)
}

fn rindex_module() -> Module {
    method_module("ridx", StrMethodOp::RIndex)
}

/// A LITERAL-arg module: `def idx_l(s: str) -> int: return s.index("l")`. The
/// needle `"l"` is an `Expr::LitStr`, so this exercises the shared
/// `collect_expr_literals` StrMethod arm — the "l" literal MUST be laid out as a
/// `(data)` segment (a literal method arg with no address fails to lower).
fn literal_needle_module() -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::StrIndex,
        args: vec![Expr::LitStr("l".into())],
    };
    let f = Function {
        name: "idx_l".into(),
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

/// A HEAP-operand module: `def idx_lo(s: str) -> int: return s.index("l" + "o")`.
/// The needle `"l" + "o"` (`Expr::Concat`) materialises a heap string, so this
/// exercises the shared `expr_has_heap_op` StrMethod arm (the bump allocator plus
/// the "l"/"o" literal `(data)` segments must be gated in).
fn heap_needle_module() -> Module {
    let needle = Expr::Concat {
        lhs: Box::new(Expr::LitStr("l".into())),
        rhs: Box::new(Expr::LitStr("o".into())),
    };
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op: StrMethodOp::StrIndex,
        args: vec![needle],
    };
    let f = Function {
        name: "idx_lo".into(),
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
/// (`$<fn>(S_ADDR, P_ADDR)`) onto the emitted module, before its closing `)`.
fn build_witness_wat(kernel_wat: &str, fn_name: &str, s: &str, p: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1144 witness: preload the two str params (below LITERAL_BASE)\n");
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
           i32.const {S_ADDR}\n    i32.const {P_ADDR}\n    call ${fn_name})\n"
    ));
    wat.push_str(")\n");
    wat
}

/// Parse the `run() => …` line from `wasm-interp --run-all-exports`. Returns
/// `Some(i)` for `run() => i64:<i>` (a present-needle index), or `None` for
/// `run() => error: unreachable executed` (an absent needle → the wrapper trapped,
/// == CPython `ValueError`). A missing `run` line is a harness bug (panic).
fn parse_run_outcome(stdout: &str) -> Option<i64> {
    let line = stdout
        .lines()
        .find(|l| l.contains("run() =>"))
        .unwrap_or_else(|| panic!("no `run` export in interp output:\n{stdout}"));
    if let Some(idx) = line.find("=> i64:") {
        let raw = line[idx + "=> i64:".len()..].trim();
        // wasm-interp prints an i64 as UNSIGNED decimal; index/rindex never
        // return a negative (absent traps instead), so the value is a small
        // non-negative that round-trips through u64 → i64 unchanged.
        return Some(
            raw.parse::<u64>()
                .map(|u| u as i64)
                .unwrap_or_else(|_| panic!("parse i64 from {line:?}")),
        );
    }
    assert!(
        line.contains("=> error:"),
        "unexpected `run` outcome line (neither i64 nor a trap): {line:?}"
    );
    // A trap (`unreachable executed`) == the Python `ValueError` for an absent
    // needle.
    None
}

/// Lower `s.<op>(p)`, run it in WABT with `s`/`p` preloaded. Returns
/// `Some(Some(i))` = index `i`, `Some(None)` = the module TRAPPED (ValueError),
/// `None` = WABT absent (the caller skips the value assertion).
fn exec_method(module: &Module, fn_name: &str, s: &str, p: &str) -> Option<Option<i64>> {
    let kernel_wat = emit_module(module).expect("index/rindex program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, fn_name, s, p);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-index-{}-{}-{}",
        std::process::id(),
        fn_name,
        s.len() * 131 + p.len() * 7
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join(format!("{fn_name}.wat"));
    let wasm_path = dir.join(format!("{fn_name}.wasm"));
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
    // wasm-interp exits 0 even on a trap (it reports the trap per-export on
    // stdout), so parse the outcome line rather than the process status.
    assert!(
        run.status.success(),
        "wasm-interp run failed: stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    Some(parse_run_outcome(&stdout))
}

/// Python `str.index` = the CODE-POINT index of the FIRST occurrence, or a
/// `ValueError` (→ `None`) when absent. Rust's `str::find` returns a BYTE offset;
/// convert it to a code-point index by counting the chars in the prefix — exactly
/// Python's char-indexed `str.index` for valid UTF-8. `None` (absent) is the
/// `ValueError`.
fn rust_char_index(h: &str, n: &str) -> Option<i64> {
    h.find(n).map(|byte| h[..byte].chars().count() as i64)
}

/// Python `str.rindex` = the CODE-POINT index of the LAST occurrence, or a
/// `ValueError` (→ `None`). (For an empty needle Rust's `rfind` returns
/// `Some(h.len())`, so the conversion yields `h.chars().count()` = the charlen
/// answer, never a `ValueError`.)
fn rust_char_rindex(h: &str, n: &str) -> Option<i64> {
    h.rfind(n).map(|byte| h[..byte].chars().count() as i64)
}

#[test]
fn cpython_index_is_pinned() {
    for c in CASES {
        assert_eq!(
            c.index,
            rust_char_index(c.haystack, c.needle),
            "index mismatch for {:?}.index({:?})",
            c.haystack,
            c.needle
        );
        assert_eq!(
            c.rindex,
            rust_char_rindex(c.haystack, c.needle),
            "rindex mismatch for {:?}.rindex({:?})",
            c.haystack,
            c.needle
        );
    }
    // The multi-byte fixtures MUST genuinely exercise a non-ASCII byte, else the
    // "byte offset → code-point index" conversion is untested.
    assert!(CASES
        .iter()
        .any(|c| !c.haystack.is_ascii() || !c.needle.is_ascii()));
    // An ABSENT fixture must be pinned to a TRAP (None), NOT -1 — this is the ONE
    // thing index/rindex add over find/rfind.
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "hello" && c.needle == "z" && c.index.is_none()));
    // A needle-LONGER-than-haystack fixture must also trap (Python raises, never
    // returns -1).
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "hello" && c.needle == "helloo" && c.index.is_none()));
    // A fixture where index and rindex genuinely DISAGREE (a true first vs last
    // occurrence): "hello".index("l") == 2 but "hello".rindex("l") == 3.
    assert!(CASES.iter().any(|c| c.haystack == "hello"
        && c.needle == "l"
        && c.index == Some(2)
        && c.rindex == Some(3)));
    // A fixture where the CODE-POINT index differs from the BYTE offset:
    // "héllo".rindex("l") == 3 (code points), but the byte offset is 4.
    assert!(CASES.iter().any(|c| {
        c.haystack == "héllo"
            && c.needle == "l"
            && c.rindex == Some(3)
            && c.haystack.rfind(c.needle) == Some(4) // byte offset genuinely differs
    }));
    // The EMPTY needle must NEVER trap and rindex must be the CODE-POINT length
    // (found at the END), NOT 0 and NOT the byte length: "héllo" is 6 bytes / 5
    // code points → rindex 5, index 0.
    assert!(CASES.iter().any(|c| {
        c.haystack == "héllo"
            && c.needle.is_empty()
            && c.index == Some(0)
            && c.rindex == Some(5)
            && c.haystack.len() == 6 // byte length genuinely differs from char length
    }));
    // A shared-continuation-byte NEGATIVE must trap (no false positive): "©" in
    // "café" shares 0xA9 but is not a substring → ValueError.
    assert!(CASES
        .iter()
        .any(|c| c.haystack == "café" && c.needle == "©" && c.index.is_none()));
}

#[test]
fn index_emits_wrapper_wrapped_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): `s.index(p)` lowers
    // through the production emitter, carrying its TRAPPING wrapper AND the wrapped
    // search helper (the `needs_find |= needs_index` fold), declaring memory, and
    // NEVER pulling in the bump allocator (an index over two str PARAMS allocates
    // nothing).
    let wat = emit_module(&index_module()).expect("the `s.index(p)` program must lower");
    assert!(
        wat.contains("(func $__wasm_str_index (param $h i32) (param $n i32) (result i64)"),
        "the $__wasm_str_index wrapper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_index"),
        "$idx must call $__wasm_str_index:\n{wat}"
    );
    // The wrapper calls $__wasm_str_find — so the wrapped search helper MUST be
    // co-emitted even though the module never calls `.find` directly.
    assert!(
        wat.contains("(func $__wasm_str_find (param $h i32) (param $n i32) (result i64)"),
        "the wrapped $__wasm_str_find helper must be co-emitted (needs_find fold):\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_find"),
        "the $__wasm_str_index wrapper body must call $__wasm_str_find:\n{wat}"
    );
    // The trap is a WASM `unreachable` — the honest analogue of Python ValueError.
    assert!(
        wat.contains("unreachable"),
        "an absent needle must lower to a WASM trap (unreachable):\n{wat}"
    );
    assert!(
        wat.contains("(memory"),
        "the byte search needs memory declared:\n{wat}"
    );
    assert!(
        !wat.contains("(func $__alloc"),
        "a pure param index module must NOT carry the bump allocator:\n{wat}"
    );
    // An index-only module carries NO rfind/rindex helper (no dead helper), and no
    // prefix/suffix/contains/count helper either. Match the helper DEFINITION.
    assert!(
        !wat.contains("(func $__wasm_str_rfind ")
            && !wat.contains("(func $__wasm_str_rindex ")
            && !wat.contains("(func $__wasm_str_startswith")
            && !wat.contains("(func $__wasm_str_endswith")
            && !wat.contains("(func $__wasm_str_contains")
            && !wat.contains("(func $__wasm_str_count"),
        "an index-only module carries no rfind/rindex/startswith/endswith/contains/count helper:\n{wat}"
    );
}

#[test]
fn rindex_emits_wrapper_wrapped_helper_and_call() {
    // Symmetric to index: `s.rindex(p)` carries $__wasm_str_rindex AND the wrapped
    // $__wasm_str_rfind (the `needs_rfind |= needs_rindex` fold), plus charlen (the
    // empty-needle case), a trap, and NO allocator / NO find/index helper.
    let wat = emit_module(&rindex_module()).expect("the `s.rindex(p)` program must lower");
    assert!(
        wat.contains("(func $__wasm_str_rindex (param $h i32) (param $n i32) (result i64)"),
        "the $__wasm_str_rindex wrapper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_rindex"),
        "$ridx must call $__wasm_str_rindex:\n{wat}"
    );
    assert!(
        wat.contains("(func $__wasm_str_rfind (param $h i32) (param $n i32) (result i64)")
            && wat.contains("call $__wasm_str_rfind"),
        "the wrapped $__wasm_str_rfind helper must be co-emitted + called (needs_rfind fold):\n{wat}"
    );
    // rindex's empty-needle answer flows through rfind → charlen must be co-emitted.
    assert!(
        wat.contains("(func $__wasm_str_charlen"),
        "rindex's empty-needle case needs $__wasm_str_charlen co-emitted:\n{wat}"
    );
    assert!(
        wat.contains("unreachable"),
        "an absent needle must lower to a WASM trap (unreachable):\n{wat}"
    );
    assert!(
        !wat.contains("(func $__alloc"),
        "a pure param rindex module must NOT carry the bump allocator:\n{wat}"
    );
    assert!(
        !wat.contains("(func $__wasm_str_find ") && !wat.contains("(func $__wasm_str_index "),
        "a rindex-only module carries no find/index helper:\n{wat}"
    );
}

#[test]
fn literal_arg_lays_out_data() {
    // `s.index("l")` MUST lay out the "l" needle literal as a `(data)` segment.
    let wat = emit_module(&literal_needle_module()).expect("the literal-needle program must lower");
    assert!(
        wat.contains("call $__wasm_str_index"),
        "the literal-needle module must still call the index wrapper:\n{wat}"
    );
    assert!(
        wat.contains("\\6c"),
        "the \"l\" needle literal must be laid out as a (data) segment:\n{wat}"
    );
    assert!(
        !wat.contains("(func $__alloc"),
        "a literal-needle index must NOT carry the bump allocator:\n{wat}"
    );
}

#[test]
fn heap_operand_pulls_allocator_and_literals() {
    // The `("l" + "o")` needle materialises a heap string, so the module MUST
    // carry the bump allocator and lay out the "l"/"o" literal `(data)` segments.
    let wat = emit_module(&heap_needle_module()).expect("the heap-needle program must lower");
    assert!(
        wat.contains("call $__wasm_str_index"),
        "the heap-needle module must still call the index wrapper:\n{wat}"
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
fn real_index_programs_execute_in_wasm_and_match_cpython() {
    // Prove both emit paths lower (holds without WABT).
    emit_module(&index_module()).expect("index program lowers");
    emit_module(&rindex_module()).expect("rindex program lowers");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1144: skipping EXECUTED string index/rindex witness — WABT \
             (wat2wasm / wasm-interp) absent. Both programs lowered through \
             emit_module (asserted in `*_emits_wrapper_wrapped_helper_and_call`); a \
             box with WABT also runs all {} cases (× 2 methods) and asserts each \
             present index == the pinned CPython int and each absent needle TRAPS \
             (== ValueError). Free CI skips execution and stays green.",
            CASES.len()
        );
        return;
    }

    eprintln!("PMAT-1144: running EXECUTED string index/rindex witness via WABT");
    let idx_mod = index_module();
    let ridx_mod = rindex_module();
    let mut checked = 0usize;
    let mut traps = 0usize;
    for c in CASES {
        let got_idx = exec_method(&idx_mod, "idx", c.haystack, c.needle).expect("WABT present");
        assert_eq!(
            got_idx, c.index,
            "executed WASM `{:?}.index({:?})` = {got_idx:?} but CPython = {:?} \
             (None = ValueError/trap)",
            c.haystack, c.needle, c.index
        );
        let got_ridx = exec_method(&ridx_mod, "ridx", c.haystack, c.needle).expect("WABT present");
        assert_eq!(
            got_ridx, c.rindex,
            "executed WASM `{:?}.rindex({:?})` = {got_ridx:?} but CPython = {:?} \
             (None = ValueError/trap)",
            c.haystack, c.needle, c.rindex
        );
        if c.index.is_none() {
            traps += 1;
        }
        checked += 1;
    }
    // The witness must genuinely exercise the trapping path, else it only re-proves
    // find/rfind.
    assert!(
        traps >= 3,
        "the index/rindex witness must execute at least 3 trapping (absent) cases, \
         got {traps}"
    );
    eprintln!(
        "PMAT-1144: EXECUTED string index/rindex witness PASSED — {checked} cases \
         (× index + rindex) lowered through emit_module and executed in WABT, each \
         value-matching CPython, including the {traps} ABSENT cases that TRAPPED \
         (== Python ValueError, NOT -1: \"hello\".index(\"z\"), \
         \"hello\".index(\"helloo\") needle-too-long, \"café\".rindex(\"©\") \
         shared-byte), the first-vs-last divergences (\"hello\".index(\"l\")=2 / \
         rindex=3; \"abécdé\".index(\"é\")=2 / rindex=5), the CODE-POINT-index \
         fixtures where a byte search would diverge (\"héllo\".rindex(\"l\")=3 at \
         byte offset 4), and the empty-needle NON-trap (\"héllo\".index(\"\")=0, \
         rindex=5 the code-point length not byte length 6) — trapping search over \
         the wrapped find/rfind helpers, proven on silicon."
    );
}
