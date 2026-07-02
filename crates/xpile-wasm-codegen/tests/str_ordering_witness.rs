//! PMAT-1059 — EXECUTED string-ORDERING witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The equality slices (`literal_string_witness.rs`) wired `a == b` / `a != b`
//! via `$__wasm_str_eq`; this slice adds the four ORDERING ops `<` / `<=` /
//! `>` / `>=` via `$__wasm_str_cmp`, a byte-wise lexicographic 3-way compare.
//!
//! ## Why a byte compare IS Python's ordering
//!
//! CPython compares strings by Unicode CODE POINT. UTF-8 is designed so that
//! byte-lexicographic order EQUALS code-point-lexicographic order — so a plain
//! byte compare over the length-prefixed UTF-8 payload reproduces CPython
//! EXACTLY, with NO char walk (unlike len / index / slice, which must count
//! code points). The witness proves this on MULTI-BYTE fixtures where a naive
//! signed-byte or char-miscount would diverge:
//!   * `"abé" > "abz"` — `é` (U+00E9, lead byte 0xC3=195) vs `z` (0x7A=122):
//!     the first differing byte is 195 > 122, and code point 233 > 122, so both
//!     orders agree → `>`. A SIGNED byte read would see 0xC3 as −61 < 122 and
//!     wrongly return `<`; `$__wasm_str_cmp` uses `i32.load8_u`.
//!   * `"café" > "cafe"` — differs at the 4th byte (`é` lead 0xC3 vs `e` 0x65),
//!     found BEFORE either string ends, so length is irrelevant here.
//!   * `"app" < "apple"` — a common prefix; shorter-is-less (`len(a)-len(b)`).
//!   * `"" < "a"`, `"" <= ""` — the empty-string edges.
//!
//! ## The real program
//!
//! ```python
//! def cmp(a: str, b: str) -> bool:
//!     return a < b          # (and <=, >, >=)
//! ```
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_str_cmp` helper + call) on a host without
//! WABT. The pinned CPython booleans are cross-checked against Rust's `&str`
//! ordering (which is byte-lexicographic == Python's for valid UTF-8).

use std::process::Command;

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, Param, SourceLang, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// A single `(a, b)` fixture with its four pinned CPython ordering results.
/// `python3 -c "a,b='{a}','{b}'; print(a<b, a<=b, a>b, a>=b)"`.
struct Case {
    a: &'static str,
    b: &'static str,
    lt: bool,
    le: bool,
    gt: bool,
    ge: bool,
}

/// The witness fixtures — ASCII, prefix, equal, and MULTI-BYTE (é) pairs, plus
/// the empty-string edges. Each pinned bool is the CPython ground truth (also
/// asserted == Rust `&str` ordering in `cpython_ordering_is_pinned`).
const CASES: &[Case] = &[
    // a < b (first byte differs: 'a' 97 < 'b' 98)
    Case {
        a: "apple",
        b: "banana",
        lt: true,
        le: true,
        gt: false,
        ge: false,
    },
    // a < b via shorter-is-less on a common prefix
    Case {
        a: "app",
        b: "apple",
        lt: true,
        le: true,
        gt: false,
        ge: false,
    },
    // equal
    Case {
        a: "abc",
        b: "abc",
        lt: false,
        le: true,
        gt: false,
        ge: true,
    },
    // a > b
    Case {
        a: "banana",
        b: "apple",
        lt: false,
        le: false,
        gt: true,
        ge: true,
    },
    // MULTI-BYTE: 'é' (233, lead byte 0xC3) > 'z' (122) → "abé" > "abz"
    Case {
        a: "abé",
        b: "abz",
        lt: false,
        le: false,
        gt: true,
        ge: true,
    },
    // MULTI-BYTE: differ at the 4th byte ('é' vs 'e') → "café" > "cafe"
    Case {
        a: "café",
        b: "cafe",
        lt: false,
        le: false,
        gt: true,
        ge: true,
    },
    // empty-string edges
    Case {
        a: "",
        b: "a",
        lt: true,
        le: true,
        gt: false,
        ge: false,
    },
    Case {
        a: "",
        b: "",
        lt: false,
        le: true,
        gt: false,
        ge: true,
    },
];

/// The four ordering ops, each with its meta-HIR `BinOp` and the WAT compare
/// instruction the emitter must produce against the `$__wasm_str_cmp` result.
const OPS: &[(BinOp, &str)] = &[
    (BinOp::Lt, "i32.lt_s"),
    (BinOp::LtEq, "i32.le_s"),
    (BinOp::Gt, "i32.gt_s"),
    (BinOp::GtEq, "i32.ge_s"),
];

/// Fixed, non-overlapping addresses for the two preloaded str params, below
/// `LITERAL_BASE` (= 512) and the bump heap (>= 1024). Each is a length-prefixed
/// region (i32 BYTE count @ base+0, UTF-8 bytes @ base+8).
const A_ADDR: i32 = 16;
const B_ADDR: i32 = 256;

/// The pinned expected for a given case + op.
fn expected(c: &Case, op: BinOp) -> bool {
    match op {
        BinOp::Lt => c.lt,
        BinOp::LtEq => c.le,
        BinOp::Gt => c.gt,
        BinOp::GtEq => c.ge,
        _ => unreachable!("only ordering ops in OPS"),
    }
}

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def cmp(a: str, b: str) -> bool: return a <op> b`.
fn cmp_module(op: BinOp) -> Module {
    let body = Expr::BinOp {
        op,
        lhs: Box::new(Expr::Ident("a".into())),
        rhs: Box::new(Expr::Ident("b".into())),
    };
    let f = Function {
        name: "cmp".into(),
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
        return_type: Type::Bool,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "cmp_program".into(),
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
/// (`$cmp(A_ADDR, B_ADDR)`) onto the emitted module, before its closing `)`.
fn build_witness_wat(kernel_wat: &str, a: &str, b: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1059 witness: preload the two str params (below LITERAL_BASE)\n");
    for (addr, s) in [(A_ADDR, a), (B_ADDR, b)] {
        let bytes = s.as_bytes();
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
           i32.const {A_ADDR}\n    i32.const {B_ADDR}\n    call $cmp)\n"
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

/// Lower `a <op> b`, run it in WABT with `a`/`b` preloaded, return the bool.
/// `None` when WABT is absent (the caller skips the value assertion).
fn exec_cmp(op: BinOp, a: &str, b: &str) -> Option<bool> {
    let kernel_wat = emit_module(&cmp_module(op)).expect("cmp program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, a, b);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-cmp-{}-{op:?}-{}",
        std::process::id(),
        a.len() * 31 + b.len()
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("cmp.wat");
    let wasm_path = dir.join("cmp.wasm");
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
fn cpython_ordering_is_pinned() {
    // Rust `&str` ordering is byte-lexicographic == Python's code-point order
    // for valid UTF-8, so it validates every pinned CPython bool.
    for c in CASES {
        assert_eq!(c.lt, c.a < c.b, "lt mismatch for {:?} < {:?}", c.a, c.b);
        assert_eq!(c.le, c.a <= c.b, "le mismatch for {:?} <= {:?}", c.a, c.b);
        assert_eq!(c.gt, c.a > c.b, "gt mismatch for {:?} > {:?}", c.a, c.b);
        assert_eq!(c.ge, c.a >= c.b, "ge mismatch for {:?} >= {:?}", c.a, c.b);
    }
    // The multi-byte fixtures MUST genuinely exercise a non-ASCII byte, else
    // the "byte order == code-point order" claim is untested.
    assert!(CASES.iter().any(|c| !c.a.is_ascii() || !c.b.is_ascii()));
}

#[test]
fn cmp_emits_str_cmp_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): each ordering op lowers
    // through the production emitter, carrying the cmp helper + call + the right
    // signed compare, and NEVER a bare base-pointer `i32.lt_s` on the pointers.
    for (op, cmp_instr) in OPS {
        let wat = emit_module(&cmp_module(*op))
            .unwrap_or_else(|e| panic!("the a {op:?} b program must lower: {e:?}"));
        assert!(
            wat.contains("(func $__wasm_str_cmp (param $a i32) (param $b i32) (result i32)"),
            "the cmp helper must be emitted for {op:?}:\n{wat}"
        );
        assert!(
            wat.contains("call $__wasm_str_cmp"),
            "$cmp must call the cmp helper for {op:?}:\n{wat}"
        );
        // the result is compared against 0 with the matching signed op
        assert!(
            wat.contains("i32.const 0") && wat.contains(cmp_instr),
            "{op:?} must compare the cmp result against 0 with {cmp_instr}:\n{wat}"
        );
        // memory is declared (the compare reads the str bytes) but NO allocator
        // is pulled in (ordering allocates nothing).
        assert!(
            wat.contains("(memory"),
            "ordering needs memory declared:\n{wat}"
        );
        assert!(
            !wat.contains("(func $__alloc"),
            "a pure ordering module must NOT carry the bump allocator:\n{wat}"
        );
    }
}

#[test]
fn mixed_str_nonstr_ordering_is_refused() {
    // `str < int` is a hard refusal (never a wrong compare) — mirror of the
    // equality mixed-operand guard.
    let body = Expr::BinOp {
        op: BinOp::Lt,
        lhs: Box::new(Expr::Ident("a".into())),
        rhs: Box::new(Expr::LitInt(3)),
    };
    let f = Function {
        name: "bad".into(),
        params: vec![Param {
            name: "a".into(),
            ty: Type::Str,
            mutable: false,
        }],
        return_type: Type::Bool,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    let m = Module {
        name: "bad_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    };
    let err = emit_module(&m).expect_err("str < int must be refused, not miscompiled");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("mixing a `str` operand with a non-`str` operand"),
        "the refusal must name the mixed-operand cause: {msg}"
    );
}

#[test]
fn real_ordering_programs_execute_in_wasm_and_match_cpython() {
    // Prove the emit path lowers for every op (holds without WABT).
    for (op, _) in OPS {
        emit_module(&cmp_module(*op)).expect("ordering program lowers");
    }

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1059: skipping EXECUTED string-ordering witness — WABT \
             (wat2wasm / wasm-interp) absent. The cmp programs lowered through \
             emit_module (asserted in `cmp_emits_str_cmp_helper_and_call`); a \
             box with WABT also runs all {} case×op pairs and asserts each == \
             the pinned CPython bool. Free CI skips execution and stays green.",
            CASES.len() * OPS.len()
        );
        return;
    }

    eprintln!("PMAT-1059: running EXECUTED string-ordering witness via WABT");
    let mut checked = 0usize;
    for c in CASES {
        for (op, _) in OPS {
            let want = expected(c, *op);
            let got = exec_cmp(*op, c.a, c.b).expect("WABT present → a value");
            assert_eq!(
                got, want,
                "executed WASM `{:?} {op:?} {:?}` = {got} but CPython = {want}",
                c.a, c.b
            );
            checked += 1;
        }
    }
    eprintln!(
        "PMAT-1059: EXECUTED string-ordering witness PASSED — {checked} \
         (case × op) pairs lowered through emit_module and executed in WABT, \
         each value-matching CPython, including the MULTI-BYTE fixtures \
         (\"abé\" > \"abz\", \"café\" > \"cafe\" — byte order == code-point \
         order, proven on silicon, never a signed-byte or base-pointer compare)."
    );
}
