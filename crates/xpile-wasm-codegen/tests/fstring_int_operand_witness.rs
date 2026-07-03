//! PMAT-1164 — EXECUTED f-string / format INT-OPERAND witness for the native
//! WASM EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! A Python f-string / `str.format` / `%`-format WITH literal text around an
//! interpolated value lowers (in the shared frontend) to a left-nested
//! `Expr::Concat` whose operands are the literal chunks interleaved with the
//! interpolated expressions — `f"count={n}"` → `Concat(LitStr("count="), n)`.
//! For a STRING interpolation the operand is already string-valued; for an INT
//! interpolation (`{n}`, `{a+b}`, `{len(s)}`) the operand is a raw `i64`
//! expression. The Rust backend leans on `format!`'s `Display`; the WASM lane
//! has no `Display`, so PMAT-1164's pre-pass rewrites every int-valued `Concat`
//! operand into an explicit `str(int)` (`ToStr { of_float: false }`) BEFORE the
//! gate scans + emission run. The int→str helper then gates naturally and
//! `emit_concat` sees an ordinary string-valued operand.
//!
//! These witnesses build the RAW (pre-normalisation) meta-HIR the frontend
//! emits, run it through the PRODUCTION `emit_module` (which applies the
//! pre-pass), execute the result in WABT, reconstruct the produced string, and
//! differential-check it against the pinned CPython ground truth.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_int_to_str` helper + call) on a host
//! without WABT.

use std::process::Command;

use xpile_meta_hir::{
    BinOp, Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type,
};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ─── module builders — the RAW frontend shape; `emit_module` normalises ─────

/// Wrap a str-returning body in a `Module`.
fn str_fn_module(name: &str, params: Vec<Param>, body: Expr) -> Module {
    let f = Function {
        name: name.into(),
        params,
        return_type: Type::Str,
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

fn int_param(name: &str) -> Param {
    Param {
        name: name.into(),
        ty: Type::I64,
        mutable: false,
    }
}

/// `def label(n: int) -> str: return f"count={n}"` → `Concat(LitStr, n)`.
fn label_module() -> Module {
    let body = Expr::Concat {
        lhs: Box::new(Expr::LitStr("count=".into())),
        rhs: Box::new(Expr::Ident("n".into())),
    };
    str_fn_module("label", vec![int_param("n")], body)
}

/// `def two(x: int, y: int) -> str: return f"{x},{y}"` →
/// `Concat(Concat(x, ","), y)` — TWO int operands in one concat (proves the
/// dedicated concat-dst / concat-off scratch survive each operand's own
/// `str(int)` re-materialisation).
fn two_module() -> Module {
    let body = Expr::Concat {
        lhs: Box::new(Expr::Concat {
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::LitStr(",".into())),
        }),
        rhs: Box::new(Expr::Ident("y".into())),
    };
    str_fn_module("two", vec![int_param("x"), int_param("y")], body)
}

/// `def arith(a: int, b: int) -> str: return f"sum={a + b}"` →
/// `Concat(LitStr, a + b)` — an int-ARITHMETIC operand, not a bare name.
fn arith_module() -> Module {
    let body = Expr::Concat {
        lhs: Box::new(Expr::LitStr("sum=".into())),
        rhs: Box::new(Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::Ident("a".into())),
            rhs: Box::new(Expr::Ident("b".into())),
        }),
    };
    str_fn_module("arith", vec![int_param("a"), int_param("b")], body)
}

/// `def litlen(s: str) -> str: return f"len={len(s)}"` →
/// `Concat(LitStr, StrMethod { CharCount, recv: s })` — a str param whose
/// CODE-POINT length (not byte length) is interpolated (char-vs-byte witness).
fn litlen_module() -> Module {
    let body = Expr::Concat {
        lhs: Box::new(Expr::LitStr("len=".into())),
        rhs: Box::new(Expr::StrMethod {
            recv: Box::new(Expr::Ident("s".into())),
            op: StrMethodOp::CharCount,
            args: vec![],
        }),
    };
    str_fn_module(
        "litlen",
        vec![Param {
            name: "s".into(),
            ty: Type::Str,
            mutable: false,
        }],
        body,
    )
}

// ─── WABT execution harness ─────────────────────────────────────────────────

/// Fixed address for the single preloaded str param, below `LITERAL_BASE`
/// (= 512) and the bump heap (>= 1024): length-prefixed (i32 byte count @
/// base+0, UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;

/// Escape an `i32` as a little-endian WAT `(data …)` string-literal.
fn i32_data_escape(v: i32) -> String {
    v.to_le_bytes()
        .iter()
        .map(|b| format!("\\{b:02x}"))
        .collect()
}

/// Escape raw bytes as a WAT `(data …)` string-literal.
fn bytes_data_escape(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
}

/// Splice `run_len` + `run_byte_i` readers that push the given prelude (the
/// argument-setup instructions and `call $kernel`), read the returned string's
/// header + payload bytes. `out_len` is the expected produced byte length.
fn build_reader_wat(kernel_wat: &str, prelude: &str, out_len: usize, data: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1164 witness: run the kernel, read back the string\n");
    wat.push_str(data);
    wat.push_str(&format!(
        "  (func (export \"run_len\") (result i32)\n{prelude}    i32.load)\n"
    ));
    for i in 0..out_len {
        wat.push_str(&format!(
            "  (func (export \"run_byte_{i}\") (result i32)\n{prelude}    \
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

/// Assemble + run a witness WAT, reconstruct the produced string. Returns `None`
/// only when WABT is absent (checked by the caller before building the WAT).
fn run_and_reconstruct(tag: &str, wat: &str, out_len: usize) -> String {
    let dir = std::env::temp_dir().join(format!("xpile-wasm-fstr-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("k.wat");
    let wasm_path = dir.join("k.wasm");
    std::fs::write(&wat_path, wat).expect("write wat");
    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed for {tag}:\n{}\n---WAT---\n{wat}",
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
        "wasm-interp run failed for {tag}: stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    let got_len = parse_i32_export(&stdout, "run_len") as usize;
    assert_eq!(
        got_len, out_len,
        "{tag}: produced byte length WASM={got_len} CPython={out_len}\nWAT:\n{wat}"
    );
    let mut bytes = Vec::with_capacity(out_len);
    for i in 0..out_len {
        bytes.push(parse_i32_export(&stdout, &format!("run_byte_{i}")) as u8);
    }
    String::from_utf8(bytes).expect("produced string bytes are valid UTF-8")
}

/// Execute an INT-param kernel (`args` pushed as i64 consts) and reconstruct.
fn exec_int_kernel(module: &Module, kernel: &str, args: &[i64], expected: &str) -> Option<String> {
    let kernel_wat = emit_module(module).expect("f-string int-operand program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let mut prelude = String::new();
    for a in args {
        prelude.push_str(&format!("    i64.const {a}\n"));
    }
    prelude.push_str(&format!("    call ${kernel}\n"));
    let wat = build_reader_wat(&kernel_wat, &prelude, expected.len(), "");
    let tag = format!(
        "{kernel}-{}",
        args.iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join("_")
    );
    Some(run_and_reconstruct(&tag, &wat, expected.len()))
}

/// Execute the single-str-param `litlen` kernel over `s` (preloaded at
/// `S_ADDR`) and reconstruct.
fn exec_litlen(s: &str, expected: &str) -> Option<String> {
    let kernel_wat = emit_module(&litlen_module()).expect("litlen program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let bytes = s.as_bytes();
    let data = format!(
        "  (data (i32.const {S_ADDR}) \"{}\")\n  (data (i32.const {}) \"{}\")\n",
        i32_data_escape(bytes.len() as i32),
        S_ADDR + 8,
        bytes_data_escape(bytes),
    );
    let prelude = format!("    i32.const {S_ADDR}\n    call $litlen\n");
    let wat = build_reader_wat(&kernel_wat, &prelude, expected.len(), &data);
    Some(run_and_reconstruct(
        &format!("litlen-{}", bytes.len()),
        &wat,
        expected.len(),
    ))
}

// ─── CPython ground-truth pin ───────────────────────────────────────────────

#[test]
fn cpython_fstring_int_values_are_pinned() {
    let n = 42;
    assert_eq!(format!("count={n}"), "count=42");
    let n = -42;
    assert_eq!(format!("count={n}"), "count=-42");
    let n = 0;
    assert_eq!(format!("count={n}"), "count=0");
    let (x, y) = (3, 7);
    assert_eq!(format!("{x},{y}"), "3,7");
    let (x, y) = (-1, 20);
    assert_eq!(format!("{x},{y}"), "-1,20");
    let (a, b) = (5, 8);
    assert_eq!(format!("sum={}", a + b), "sum=13");
    // "café" — 4 code points, 5 UTF-8 bytes. Python `len` counts code points.
    assert_eq!("café".chars().count(), 4);
    assert_eq!("café".len(), 5);
    assert_eq!(format!("len={}", "café".chars().count()), "len=4");
}

// ─── CONSTRUCT assertions (hold with or without WABT) ───────────────────────

#[test]
fn label_emits_int_to_str_helper_and_call() {
    let wat = emit_module(&label_module()).expect("f\"count={n}\" must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_int_to_str (param $n i64) (result i32)"),
        "the int→str helper must be emitted for a format int operand:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_int_to_str"),
        "the concat must call the int→str helper on its int operand:\n{wat}"
    );
    assert!(
        wat.contains("(func $label (param $n i64) (result i32)"),
        "str return → i32 result (heap pointer):\n{wat}"
    );
    assert!(
        wat.contains("(func $__alloc"),
        "materialising the decimal string needs the bump heap:\n{wat}"
    );
}

#[test]
fn helper_emitted_exactly_once_for_two_operands() {
    // TWO int operands in one function must still gate a SINGLE helper DEFINITION
    // (no called-but-undeclared / duplicate-def gate hole).
    let wat = emit_module(&two_module()).expect("f\"{x},{y}\" lowers");
    assert_eq!(
        wat.matches("(func $__wasm_int_to_str (param $n i64) (result i32)")
            .count(),
        1,
        "the int→str helper must be DEFINED exactly once:\n{wat}"
    );
    // Both operands ARE stringified. `emit_concat` re-evaluates each operand
    // across its length / header / copy passes (the accepted heap-waste pattern
    // every materialising concat operand uses), so the call COUNT is a multiple
    // of the operand count, not exactly one-per-operand — assert only that both
    // operands materialise (>= 2 calls).
    assert!(
        wat.matches("call $__wasm_int_to_str").count() >= 2,
        "both int operands must materialise via the helper:\n{wat}"
    );
}

#[test]
fn str_only_concat_is_untouched_by_the_pre_pass() {
    // `f"{a}{b}"` over STR params must NOT gain a spurious int→str helper — the
    // classifier only wraps positively-int operands.
    let body = Expr::Concat {
        lhs: Box::new(Expr::Ident("a".into())),
        rhs: Box::new(Expr::Ident("b".into())),
    };
    let m = str_fn_module(
        "cat",
        vec![
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
        body,
    );
    let wat = emit_module(&m).expect("str-only concat lowers");
    assert!(
        !wat.contains("call $__wasm_int_to_str"),
        "a str-only concat must never call the int→str helper:\n{wat}"
    );
}

#[test]
fn strformat_and_formatspec_refused_honestly() {
    // PMAT-1166: a bare-`{}` template now FOLDS (see `str_format_fold_witness`);
    // a template carrying a FORMAT SPEC still refuses (its width / alignment is
    // not modelled on the WASM lane).
    let strfmt = str_fn_module(
        "sf",
        vec![int_param("x")],
        Expr::StrFormat {
            fmt: "v={:>5}".into(),
            args: vec![Expr::Ident("x".into())],
        },
    );
    let err = emit_module(&strfmt).expect_err("a spec'd StrFormat template must refuse");
    assert!(
        format!("{err}").contains("str.format"),
        "StrFormat refusal must name the unfolded template:\n{err}"
    );
    // A bare single-interpolation f-string lowers to a `FormatSpec`.
    let fspec = str_fn_module(
        "fs",
        vec![int_param("n")],
        Expr::FormatSpec {
            value: Box::new(Expr::Ident("n".into())),
            rust_spec: String::new(),
            of_float: false,
        },
    );
    let err = emit_module(&fspec).expect_err("a bare FormatSpec must refuse");
    assert!(
        format!("{err}").contains("bare single-interpolation"),
        "FormatSpec refusal must name the bare interpolation:\n{err}"
    );
}

// ─── EXECUTED differential witnesses ────────────────────────────────────────

#[test]
fn label_executes_and_matches_cpython() {
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1164: skipping EXECUTED f-string int-operand witness — WABT \
             absent. The label program lowered through emit_module (asserted in \
             `label_emits_int_to_str_helper_and_call`)."
        );
        // Still exercise the EMIT path.
        let _ = emit_module(&label_module()).expect("label lowers");
        return;
    }
    eprintln!("PMAT-1164: running EXECUTED f-string int-operand witnesses via WABT");
    for (n, expected) in [(42_i64, "count=42"), (-42, "count=-42"), (0, "count=0")] {
        let got = exec_int_kernel(&label_module(), "label", &[n], expected).expect("WABT present");
        assert_eq!(
            got, expected,
            "executed WASM f\"count={{{n}}}\" = {got:?} but CPython = {expected:?}"
        );
    }
}

#[test]
fn two_ints_execute_and_match_cpython() {
    if !wasm_runtime_available() {
        let _ = emit_module(&two_module()).expect("two lowers");
        return;
    }
    for (x, y, expected) in [(3_i64, 7_i64, "3,7"), (-1, 20, "-1,20")] {
        let got = exec_int_kernel(&two_module(), "two", &[x, y], expected).expect("WABT present");
        assert_eq!(
            got, expected,
            "executed WASM f\"{{{x}}},{{{y}}}\" = {got:?} but CPython = {expected:?}"
        );
    }
}

#[test]
fn int_arith_operand_executes_and_matches_cpython() {
    if !wasm_runtime_available() {
        let _ = emit_module(&arith_module()).expect("arith lowers");
        return;
    }
    let got = exec_int_kernel(&arith_module(), "arith", &[5, 8], "sum=13").expect("WABT present");
    assert_eq!(got, "sum=13", "executed WASM f\"sum={{5+8}}\" = {got:?}");
}

#[test]
fn charcount_operand_executes_and_matches_cpython() {
    if !wasm_runtime_available() {
        let _ = emit_module(&litlen_module()).expect("litlen lowers");
        return;
    }
    // "café" — 4 code points, 5 bytes. `len(s)` interpolates the CODE-POINT
    // count, so the produced string is "len=4", NOT "len=5".
    let got = exec_litlen("café", "len=4").expect("WABT present");
    assert_eq!(
        got, "len=4",
        "executed WASM f\"len={{len('café')}}\" must count code points (4), not bytes (5): {got:?}"
    );
    // A pure-ASCII control: len("hello") = 5.
    let got = exec_litlen("hello", "len=5").expect("WABT present");
    assert_eq!(
        got, "len=5",
        "executed WASM f\"len={{len('hello')}}\" = {got:?}"
    );
}
