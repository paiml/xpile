//! PMAT-1142 — EXECUTED string-REPEAT (`s * n`) witness for the native WASM
//! EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The slice witness (`str_slice_witness.rs`) shipped `s[lo:hi]` as a char-exact
//! heap SUBSTRING; the concat witness (`concat_chr_witness.rs`) shipped `a + b`.
//! This slice adds Python's sequence repetition `s * n` (`Expr::Repeat { of_str:
//! true }`) via `$__wasm_str_repeat` — an ALLOCATING op that materialises a NEW
//! heap string = the source UTF-8 payload replicated `max(n, 0)` times.
//!
//! ## Why a byte replication IS Python's `str * n`
//!
//! Unlike `.upper()` / `.lower()` (which need a full Unicode case table to match
//! CPython on non-ASCII), string repetition is a PURE byte operation: `s * n`
//! copies the exact bytes of `s`, `n` times, with NO code-point transform. UTF-8
//! is self-synchronising, so replicating the whole byte payload replicates the
//! whole code-point sequence — a multi-byte char is copied intact each pass.
//! Therefore `$__wasm_str_repeat` is byte-for-byte identical to CPython `str *
//! int` for ANY string, ASCII or not. A count `n <= 0` clamps to the empty
//! string (Python `"x" * -1 == ""`).
//!
//! The witness proves this on fixtures where a naive implementation could
//! diverge:
//!   * `"é" * 3` → `"ééé"` — a MULTI-BYTE char (`é` = 0xC3 0xA9) replicated: the
//!     result is 6 bytes `[0xC3,0xA9]×3`, proving char-exactness (a per-code-
//!     point transform is NOT needed — the bytes just repeat).
//!   * `"🎉" * 2` → 8 bytes — a 4-byte code point replicated whole.
//!   * `"ab" * -2` → `""` — the NEGATIVE-count clamp (Python semantics).
//!   * `"ab" * 0` / `"" * 5` → `""` — empty result via a zero count / source.
//!
//! ## The real program
//!
//! ```python
//! def rep(s: str, n: int) -> str:
//!     return s * n
//! ```
//!
//! ## Witness shape
//!
//! `wasm-interp --run-all-exports` invokes zero-arg exports. The kernel `$rep`
//! takes an `i32` base-pointer (`s`) and an `i64` count (`n`) and RETURNS an
//! `i32` (the constructed string's base-pointer). The witness adds, per case:
//!   1. one length-prefixed `(data …)` segment preloading `s` at a fixed address
//!      (below `LITERAL_BASE` = 512, so it never overlaps the bump heap);
//!   2. a `run_len` export returning the result's i32 byte count (header @ +0);
//!   3. a `run_byte_i` family — each re-runs `$rep(S_ADDR, n)`, adds `8 + i`, and
//!      `i32.load8_u`s that payload byte of the CONSTRUCTED result.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_str_repeat` helper + call) on a host
//! without WABT. The pinned CPython results are cross-checked against Rust's
//! `str::repeat` (which equals Python `str * n` for a non-negative count; a
//! negative count clamps to the empty string).

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// A single `(source, count)` fixture with its pinned CPython `source * count`
/// result. `python3 -c "print(repr('{source}' * {count}))"`.
struct Case {
    source: &'static str,
    count: i64,
    expected: &'static str,
}

/// The witness fixtures — ASCII (basic / identity / zero), NEGATIVE-count clamp,
/// EMPTY source, and MULTI-BYTE (`é` 2-byte, `🎉` 4-byte, mixed) — each pinned
/// result is the CPython ground truth (asserted == Rust `str::repeat` in
/// `cpython_repeat_is_pinned`).
const CASES: &[Case] = &[
    // ── ASCII ────────────────────────────────────────────────────────────
    Case {
        source: "ab",
        count: 3,
        expected: "ababab",
    },
    Case {
        source: "ab",
        count: 1,
        expected: "ab",
    }, // identity
    Case {
        source: "ab",
        count: 0,
        expected: "",
    }, // zero count → empty
    Case {
        source: "x",
        count: 5,
        expected: "xxxxx",
    },
    // ── NEGATIVE count — Python "ab" * -2 == "" (clamp), NOT a trap ───────
    Case {
        source: "ab",
        count: -2,
        expected: "",
    },
    Case {
        source: "ab",
        count: -1,
        expected: "",
    },
    // ── EMPTY source ─────────────────────────────────────────────────────
    Case {
        source: "",
        count: 5,
        expected: "",
    },
    // ── MULTI-BYTE — byte replication is char-exact for UTF-8 ─────────────
    Case {
        source: "é",
        count: 3,
        expected: "ééé",
    }, // é = 0xC3 0xA9; result 6 bytes
    Case {
        source: "aé",
        count: 2,
        expected: "aéaé",
    }, // mixed ASCII + multi-byte
    Case {
        source: "🎉",
        count: 2,
        expected: "🎉🎉",
    }, // 4-byte code point ×2 = 8 bytes
    Case {
        source: "héllo",
        count: 2,
        expected: "héllohéllo",
    },
];

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and
/// the bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0,
/// UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def rep(s: str, n: int) -> str: return s * n` — i.e. `Repeat { seq: s, n,
/// of_str: true }`.
fn rep_module() -> Module {
    let body = Expr::Repeat {
        seq: Box::new(Expr::Ident("s".into())),
        n: Box::new(Expr::Ident("n".into())),
        of_str: true,
    };
    let f = Function {
        name: "rep".into(),
        params: vec![
            Param {
                name: "s".into(),
                ty: Type::Str,
                mutable: false,
            },
            Param {
                name: "n".into(),
                ty: Type::I64,
                mutable: false,
            },
        ],
        return_type: Type::Str,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "repeat_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// A LITERAL-source module: `def rep_l(n: int) -> str: return "ab" * n`. The
/// repeated `"ab"` is an `Expr::LitStr`, so this exercises the PMAT-1142
/// `collect_expr_literals` Repeat arm — the "ab" literal MUST be laid out as a
/// `(data)` segment (else `emit_str_expr` finds no source address).
fn literal_source_module() -> Module {
    let body = Expr::Repeat {
        seq: Box::new(Expr::LitStr("ab".into())),
        n: Box::new(Expr::Ident("n".into())),
        of_str: true,
    };
    let f = Function {
        name: "rep_l".into(),
        params: vec![Param {
            name: "n".into(),
            ty: Type::I64,
            mutable: false,
        }],
        return_type: Type::Str,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "literal_source_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// A HEAP-source module: `def rep_h(n: int) -> str: return ("a" + "b") * n`. The
/// `"a" + "b"` source (`Expr::Concat`) materialises a heap string, so this
/// exercises the PMAT-1142 `expr_has_heap_op` Repeat arm (the bump allocator +
/// the "a"/"b" literal `(data)` segments must be gated in) and the concat path
/// feeding the repeat.
fn heap_source_module() -> Module {
    let source = Expr::Concat {
        lhs: Box::new(Expr::LitStr("a".into())),
        rhs: Box::new(Expr::LitStr("b".into())),
    };
    let body = Expr::Repeat {
        seq: Box::new(source),
        n: Box::new(Expr::Ident("n".into())),
        of_str: true,
    };
    let f = Function {
        name: "rep_h".into(),
        params: vec![Param {
            name: "n".into(),
            ty: Type::I64,
            mutable: false,
        }],
        return_type: Type::Str,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "heap_source_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// A LIST-repeat probe reaching `emit_repeat`'s `of_str: false` arm directly:
/// `Expr::Repeat { seq: [1], of_str: false }` in a value position (the return
/// type is `int` so the return-type gate passes and the body lowering hits the
/// repeat arm — a `list[int]` return would refuse at the return-type gate first,
/// never exercising `emit_repeat`). A list repeat MUST refuse honestly (the WASM
/// list subset has no growth/replication op), never miscompile.
fn list_repeat_module() -> Module {
    let body = Expr::Repeat {
        seq: Box::new(Expr::ListLit(vec![Expr::LitInt(1)])),
        n: Box::new(Expr::Ident("n".into())),
        of_str: false,
    };
    let f = Function {
        name: "rep_list".into(),
        params: vec![Param {
            name: "n".into(),
            ty: Type::I64,
            mutable: false,
        }],
        return_type: Type::I64,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "list_repeat_program".into(),
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

/// Splice the `s` param `(data …)` region + a `run_len` + per-byte readers onto
/// the emitted module, before its closing `)`. Each reader re-runs `$rep(S_ADDR,
/// count)` (a fresh bump-heap result per invocation under `--run-all-exports`).
fn build_witness_wat(kernel_wat: &str, s: &str, count: i64, n_out: usize) -> String {
    let sb = s.as_bytes();
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1142 witness: preload the str source param (below LITERAL_BASE)\n");
    // s @ S_ADDR (length-prefixed: i32 BYTE count header + UTF-8 bytes).
    wat.push_str(&format!(
        "  (data (i32.const {S_ADDR}) \"{}\")\n",
        i32_data_escape(sb.len() as i32)
    ));
    if !sb.is_empty() {
        wat.push_str(&format!(
            "  (data (i32.const {}) \"{}\")\n",
            S_ADDR + 8,
            bytes_data_escape(sb)
        ));
    }
    // run_len: the result's i32 byte count (header at result+0).
    wat.push_str(&format!(
        "  (func (export \"run_len\") (result i32)\n    \
           i32.const {S_ADDR}\n    i64.const {count}\n    call $rep\n    i32.load)\n"
    ));
    // run_byte_i: byte i of the constructed result. Each export re-runs rep
    // (fresh bump heap per invocation under --run-all-exports).
    for i in 0..n_out {
        wat.push_str(&format!(
            "  (func (export \"run_byte_{i}\") (result i32)\n    \
               i32.const {S_ADDR}\n    i64.const {count}\n    call $rep\n    \
               i32.const {off}\n    i32.add\n    i32.load8_u)\n",
            off = 8 + i
        ));
    }
    wat.push_str(")\n");
    wat
}

/// Parse a `name() => i32:<value>` line for a given export name.
fn parse_i32_export(stdout: &str, name: &str) -> i32 {
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

/// Lower `s * n`, run it in WABT with `s` preloaded + the count baked, and read
/// back the CONSTRUCTED result's bytes. Returns `(len, bytes)`.
fn exec_repeat(kernel_wat: &str, s: &str, count: i64, n_out: usize) -> (i32, Vec<u8>) {
    let wat = build_witness_wat(kernel_wat, s, count, n_out);
    // A per-case-unique work dir. `count` may be negative, so format the raw
    // values (never arithmetic that could overflow a usize on a negative count).
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-repeat-{}-{}-{}-{}",
        std::process::id(),
        s.len(),
        count,
        n_out
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("rep.wat");
    let wasm_path = dir.join("rep.wasm");
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
    let len = parse_i32_export(&stdout, "run_len");
    let bytes = (0..n_out)
        .map(|i| parse_i32_export(&stdout, &format!("run_byte_{i}")) as u8)
        .collect();
    (len, bytes)
}

#[test]
fn cpython_repeat_is_pinned() {
    // Rust `str::repeat(k)` for k >= 0 is byte replication == Python `s * k`; a
    // negative count clamps to the empty string. This validates every pinned
    // `expected` against the language semantics independently of the emitter.
    for c in CASES {
        let rust = if c.count < 0 {
            String::new()
        } else {
            c.source.repeat(c.count as usize)
        };
        assert_eq!(
            rust, c.expected,
            "repeat mismatch for {:?} * {}",
            c.source, c.count
        );
    }
    // A MULTI-BYTE fixture must be present, else the "byte replication == code-
    // point replication" claim is untested.
    assert!(CASES.iter().any(|c| !c.source.is_ascii()));
    // `"é" * 3` must be 6 bytes ([0xC3,0xA9] × 3) — a byte-exact multi-byte repeat.
    let eee = CASES
        .iter()
        .find(|c| c.source == "é" && c.count == 3)
        .expect("the é*3 fixture is present");
    assert_eq!(
        eee.expected.as_bytes(),
        &[0xC3, 0xA9, 0xC3, 0xA9, 0xC3, 0xA9]
    );
    // A NEGATIVE-count clamp fixture must be present (the whole point vs a trap):
    // "ab" * -2 == "".
    assert!(CASES
        .iter()
        .any(|c| c.source == "ab" && c.count == -2 && c.expected.is_empty()));
}

#[test]
fn rep_emits_repeat_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): the repeat program lowers
    // through the production emitter, carrying the repeat helper + call, the bump
    // allocator (a repeat MATERIALISES a new string), and memory.
    let wat = emit_module(&rep_module())
        .expect("the `s * n` repeat program must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_str_repeat (param $s i32) (param $k i64) (result i32)"),
        "the $__wasm_str_repeat helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_repeat"),
        "$rep must call the repeat helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $rep (param $s i32) (param $n i64) (result i32)"),
        "str return → i32 result (heap pointer), int count → i64 param:\n{wat}"
    );
    // The repeat materialises a new string → needs the bump heap + memory.
    assert!(
        wat.contains("(func $__alloc") && wat.contains("(memory"),
        "repeat needs the bump allocator + memory:\n{wat}"
    );
    // A pure param repeat needs NO char-walk / slice / concat helpers (byte copy
    // only) — no dead helpers.
    assert!(
        !wat.contains("(func $__wasm_str_slice") && !wat.contains("(func $__wasm_str_count"),
        "a pure str*n module carries no slice/count helper:\n{wat}"
    );
}

#[test]
fn repeat_only_module_carries_no_dead_helper() {
    // A repeat-only module must NOT carry the repeat helper's SIBLINGS (find /
    // contains / startswith) — the "no dead helper" gate discipline.
    let wat = emit_module(&rep_module()).expect("repeat lowers");
    assert!(
        !wat.contains("(func $__wasm_str_find")
            && !wat.contains("(func $__wasm_str_contains")
            && !wat.contains("(func $__wasm_str_startswith"),
        "a repeat-only module carries no find/contains/startswith helper:\n{wat}"
    );
    // And a module with NO repeat carries no repeat helper (gate is real).
    let concat = Module {
        name: "concat_only".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(Function {
            name: "j".into(),
            params: vec![
                Param {
                    name: "a".into(),
                    ty: Type::Str,
                    mutable: false,
                },
                Param {
                    name: "b".into(),
                    ty: Type::Str,
                    mutable: false,
                },
            ],
            return_type: Type::Str,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::Concat {
                    lhs: Box::new(Expr::Ident("a".into())),
                    rhs: Box::new(Expr::Ident("b".into())),
                },
            },
        })],
        ffi_boundaries: Vec::new(),
    };
    let cwat = emit_module(&concat).expect("concat lowers");
    assert!(
        !cwat.contains("$__wasm_str_repeat"),
        "a concat-only (no repeat) module must NOT carry the repeat helper:\n{cwat}"
    );
}

#[test]
fn literal_source_lays_out_data() {
    // PMAT-1142 (collect_expr_literals Repeat arm): `"ab" * n` MUST lay out the
    // "ab" source literal as a `(data)` segment — else `emit_str_expr` finds no
    // source address for the repeat.
    let wat = emit_module(&literal_source_module()).expect("the literal-source program must lower");
    assert!(
        wat.contains("call $__wasm_str_repeat"),
        "the literal-source module must still call the repeat helper:\n{wat}"
    );
    // The literal bytes 'a' (0x61) and 'b' (0x62) must appear as a (data) segment.
    assert!(
        wat.contains("\\61") && wat.contains("\\62"),
        "the \"ab\" source literal must be laid out as a (data) segment:\n{wat}"
    );
}

#[test]
fn heap_source_pulls_allocator_and_concat() {
    // PMAT-1142 (expr_has_heap_op Repeat arm + emit_str_expr Concat source): the
    // `("a" + "b") * n` source materialises a heap string, so the module carries
    // the bump allocator, the concat path, and the "a"/"b" literal segments.
    let wat = emit_module(&heap_source_module()).expect("the heap-source program must lower");
    assert!(
        wat.contains("call $__wasm_str_repeat"),
        "the heap-source module must still call the repeat helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $__alloc"),
        "a heap-constructed source must pull in the bump allocator:\n{wat}"
    );
    assert!(
        wat.contains("$__wasm_concat_dst"),
        "the `\"a\" + \"b\"` source must lower via the inline concat path:\n{wat}"
    );
    assert!(
        wat.contains("\\61") && wat.contains("\\62"),
        "the \"a\"/\"b\" source literals must be laid out as (data) segments:\n{wat}"
    );
}

#[test]
fn list_repeat_is_refused() {
    // A LIST repeat (`[1] * n`, Expr::Repeat of_str: false) is NOT in the WASM
    // list subset — it must refuse honestly, never miscompile.
    let err = emit_module(&list_repeat_module())
        .expect_err("a list repeat must be refused by the WASM lane");
    let msg = err.to_string();
    assert!(
        msg.contains("list repeat") || (msg.contains("unsupported") && msg.contains("repeat")),
        "the list-repeat refusal must name the op honestly: {msg}"
    );
}

#[test]
fn real_repeat_programs_execute_in_wasm_and_match_cpython() {
    // Prove the emit path lowers (holds without WABT).
    let kernel_wat = emit_module(&rep_module()).expect("repeat program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1142: skipping EXECUTED string-repeat witness — WABT (wat2wasm / \
             wasm-interp) absent. The `s * n` program lowered through emit_module \
             (asserted in `rep_emits_repeat_helper_and_call`); a box with WABT also \
             runs all {} cases and asserts each CONSTRUCTED result == the pinned \
             CPython string. Free CI skips execution and stays green.",
            CASES.len()
        );
        return;
    }

    eprintln!("PMAT-1142: running EXECUTED string-repeat witness via WABT");
    let mut checked = 0usize;
    for c in CASES {
        let exp = c.expected.as_bytes();
        let (len, bytes) = exec_repeat(&kernel_wat, c.source, c.count, exp.len());
        assert_eq!(
            len as usize,
            exp.len(),
            "executed WASM len({:?} * {}) = {len} but CPython byte-len = {}",
            c.source,
            c.count,
            exp.len()
        );
        assert_eq!(
            bytes,
            exp,
            "executed WASM ({:?} * {}) = {:?} but CPython = {:?}",
            c.source,
            c.count,
            String::from_utf8_lossy(&bytes),
            c.expected
        );
        checked += 1;
    }
    eprintln!(
        "PMAT-1142: EXECUTED string-repeat witness PASSED — {checked} cases lowered \
         through emit_module and executed in WABT, each byte-matching CPython, \
         including the NEGATIVE-count clamp (\"ab\" * -2 == \"\"), the empty source/ \
         count cases, and the MULTI-BYTE repeats (\"é\" * 3 == \"ééé\" = 6 bytes, \
         \"🎉\" * 2 = 8 bytes) — byte replication == code-point replication, proven \
         on silicon."
    );
}
