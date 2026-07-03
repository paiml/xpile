//! PMAT-1166 — EXECUTED `str.format` / `%`-format FOLD witness for the native
//! WASM EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! A Python `"{}-{}".format(x, y)` / `"%s=%d" % (a, b)` reaches the WASM lane
//! as a raw `Expr::StrFormat { fmt, args }` (the frontend folds only an
//! f-string's literal text into a `Concat`; a `.format(...)` / `% (...)`
//! template stays templated). PMAT-1166's pre-pass fold rewrites the SIMPLE
//! bare-`{}` template into the same left-nested `Concat` the lane already
//! lowers — interleaving the literal chunks with the argument expressions —
//! then re-runs the PMAT-1164 int-operand normaliser so an int arg
//! auto-stringifies via `str(int)`. A template carrying a format spec
//! (`"{:>5}"`), a positional (`"{0}"`), a named field (`"{k}"`), or an
//! arg-count mismatch stays a `StrFormat` and refuses honestly at emit.
//!
//! These witnesses build the RAW (pre-fold) `StrFormat` meta-HIR the frontend
//! emits, run it through the PRODUCTION `emit_module` (which applies the fold),
//! execute the result in WABT, reconstruct the produced string, and
//! differential-check it against the pinned CPython ground truth.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path folds + lowers) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ─── module builders — the RAW frontend `StrFormat` shape ───────────────────

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

fn str_param(name: &str) -> Param {
    Param {
        name: name.into(),
        ty: Type::Str,
        mutable: false,
    }
}

fn strformat(fmt: &str, args: Vec<Expr>) -> Expr {
    Expr::StrFormat {
        fmt: fmt.into(),
        args,
    }
}

/// `def two(x: int, y: int) -> str: return "{}-{}".format(x, y)`.
fn two_ints_module() -> Module {
    let body = strformat(
        "{}-{}",
        vec![Expr::Ident("x".into()), Expr::Ident("y".into())],
    );
    str_fn_module("two", vec![int_param("x"), int_param("y")], body)
}

/// `def sum3(a,b,c: int) -> str: return "%d+%d=%d" % (a, b, c)` — the frontend
/// lowers the `%`-template to `fmt="{}+{}={}"`.
fn pct_three_module() -> Module {
    let body = strformat(
        "{}+{}={}",
        vec![
            Expr::Ident("a".into()),
            Expr::Ident("b".into()),
            Expr::Ident("c".into()),
        ],
    );
    str_fn_module(
        "sum3",
        vec![int_param("a"), int_param("b"), int_param("c")],
        body,
    )
}

/// `def esc(n: int) -> str: return "{{}}={}".format(n)` — literal `{}` (via the
/// `{{` / `}}` escape) followed by an interpolated int → `"{}=<n>"`.
fn brace_escape_module() -> Module {
    let body = strformat("{{}}={}", vec![Expr::Ident("n".into())]);
    str_fn_module("esc", vec![int_param("n")], body)
}

/// `def greet(name: str, n: int) -> str: return "{}-{}".format(name, n)` — the
/// HEADLINE mixed str+int case. The str operand stays str-valued; the int
/// operand auto-stringifies via `str(int)`.
fn mixed_module() -> Module {
    let body = strformat(
        "{}-{}",
        vec![Expr::Ident("name".into()), Expr::Ident("n".into())],
    );
    str_fn_module("greet", vec![str_param("name"), int_param("n")], body)
}

// ─── WABT execution harness ─────────────────────────────────────────────────

/// Fixed address for a single preloaded str param, below `LITERAL_BASE` (512)
/// and the bump heap (>= 1024): length-prefixed (i32 byte count @ base+0,
/// UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;

fn i32_data_escape(v: i32) -> String {
    v.to_le_bytes()
        .iter()
        .map(|b| format!("\\{b:02x}"))
        .collect()
}

fn bytes_data_escape(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
}

/// Splice `run_len` + `run_byte_i` readers onto the emitted module: run the
/// `prelude` (arg setup + `call $kernel`), then read the returned string's
/// header + `out_len` payload bytes.
fn build_reader_wat(kernel_wat: &str, prelude: &str, out_len: usize, data: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1166 witness: run the kernel, read back the string\n");
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

fn run_and_reconstruct(tag: &str, wat: &str, out_len: usize) -> String {
    let dir = std::env::temp_dir().join(format!("xpile-wasm-sfmt-{}-{tag}", std::process::id()));
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

/// Execute an all-int-param kernel (`args` as i64 consts) and reconstruct.
fn exec_int_kernel(module: &Module, kernel: &str, args: &[i64], expected: &str) -> Option<String> {
    let kernel_wat = emit_module(module).expect("StrFormat program folds + lowers");
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

/// Execute the mixed `greet(name: str, n: int)` kernel: `name` preloaded at
/// `S_ADDR`, `n` passed as an i64 const.
fn exec_mixed(name: &str, n: i64, expected: &str) -> Option<String> {
    let kernel_wat = emit_module(&mixed_module()).expect("mixed StrFormat folds + lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let bytes = name.as_bytes();
    let data = format!(
        "  (data (i32.const {S_ADDR}) \"{}\")\n  (data (i32.const {}) \"{}\")\n",
        i32_data_escape(bytes.len() as i32),
        S_ADDR + 8,
        bytes_data_escape(bytes),
    );
    let prelude = format!("    i32.const {S_ADDR}\n    i64.const {n}\n    call $greet\n");
    let wat = build_reader_wat(&kernel_wat, &prelude, expected.len(), &data);
    Some(run_and_reconstruct(
        &format!("greet-{}-{n}", bytes.len()),
        &wat,
        expected.len(),
    ))
}

// ─── CPython ground-truth pin ───────────────────────────────────────────────

#[test]
fn cpython_str_format_values_are_pinned() {
    let (x, y) = (3, 7);
    assert_eq!(format!("{x}-{y}"), "3-7");
    let (x, y) = (-1, 20);
    assert_eq!(format!("{x}-{y}"), "-1-20");
    let (a, b, c) = (2, 3, 5);
    assert_eq!(format!("{a}+{b}={c}"), "2+3=5");
    let n = 5;
    assert_eq!(format!("{{}}={n}"), "{}=5");
    // Mixed str + int.
    let (name, n) = ("bob", 42);
    assert_eq!(format!("{name}-{n}"), "bob-42");
    let (name, n) = ("café", -7);
    assert_eq!(format!("{name}-{n}"), "café--7");
}

// ─── CONSTRUCT assertions (hold with or without WABT) ───────────────────────

#[test]
fn bare_strformat_folds_and_gates_int_helper() {
    // `"{}-{}".format(x, y)` over two ints must FOLD (no refusal) and gate the
    // int→str helper (both operands auto-stringify).
    let wat = emit_module(&two_ints_module()).expect("bare `{}` StrFormat must fold");
    assert!(
        wat.contains("(func $__wasm_int_to_str (param $n i64) (result i32)"),
        "the int→str helper must gate for a folded int-arg format:\n{wat}"
    );
    assert!(
        wat.contains("(func $__alloc"),
        "the folded Concat materialises a heap string:\n{wat}"
    );
    assert!(
        wat.matches("call $__wasm_int_to_str").count() >= 2,
        "both folded int operands must materialise via the helper:\n{wat}"
    );
}

#[test]
fn mixed_strformat_folds_str_operand_untouched() {
    // The str operand of `"{}-{}".format(name, n)` must NOT be wrapped in
    // `str(int)` — only the positively-int operand is.
    let wat = emit_module(&mixed_module()).expect("mixed StrFormat folds");
    assert!(
        wat.contains("(func $greet (param $name i32) (param $n i64) (result i32)"),
        "greet takes (str ptr, i64) and returns an i32 heap pointer:\n{wat}"
    );
    // Exactly ONE int operand → the helper still appears, but the str operand
    // is copied verbatim (no double-stringify).
    assert!(
        wat.contains("call $__wasm_int_to_str"),
        "the int operand must stringify:\n{wat}"
    );
}

#[test]
fn spec_positional_named_and_mismatch_refuse() {
    // A format SPEC.
    for fmt in ["v={:>5}", "{:04}"] {
        let m = str_fn_module(
            "f",
            vec![int_param("x")],
            strformat(fmt, vec![Expr::Ident("x".into())]),
        );
        let err = emit_module(&m).unwrap_err();
        assert!(
            format!("{err}").contains("str.format"),
            "spec template {fmt:?} must refuse:\n{err}"
        );
    }
    // A POSITIONAL field.
    let m = str_fn_module(
        "f",
        vec![int_param("x"), int_param("y")],
        strformat(
            "{0}-{1}",
            vec![Expr::Ident("x".into()), Expr::Ident("y".into())],
        ),
    );
    assert!(
        format!("{}", emit_module(&m).unwrap_err()).contains("str.format"),
        "a positional `{{0}}` template must refuse"
    );
    // A NAMED field.
    let m = str_fn_module(
        "f",
        vec![int_param("x")],
        strformat("{k}", vec![Expr::Ident("x".into())]),
    );
    assert!(
        format!("{}", emit_module(&m).unwrap_err()).contains("str.format"),
        "a named `{{k}}` template must refuse"
    );
    // An arg-COUNT mismatch (2 fields, 1 arg) must not fold (would drop an arg).
    let m = str_fn_module(
        "f",
        vec![int_param("x")],
        strformat("{}-{}", vec![Expr::Ident("x".into())]),
    );
    assert!(
        format!("{}", emit_module(&m).unwrap_err()).contains("str.format"),
        "an arg-count mismatch must refuse, not silently drop a field"
    );
}

// ─── EXECUTED differential witnesses ────────────────────────────────────────

#[test]
fn two_ints_execute_and_match_cpython() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1166: skipping EXECUTED str.format fold witness — WABT absent");
        let _ = emit_module(&two_ints_module()).expect("two folds + lowers");
        return;
    }
    eprintln!("PMAT-1166: running EXECUTED str.format fold witnesses via WABT");
    for (x, y, expected) in [(3_i64, 7_i64, "3-7"), (-1, 20, "-1-20"), (0, 0, "0-0")] {
        let got = exec_int_kernel(&two_ints_module(), "two", &[x, y], expected).expect("WABT");
        assert_eq!(
            got, expected,
            "executed WASM \"{{}}-{{}}\".format({x}, {y}) = {got:?} but CPython = {expected:?}"
        );
    }
}

#[test]
fn pct_three_ints_execute_and_match_cpython() {
    if !wasm_runtime_available() {
        let _ = emit_module(&pct_three_module()).expect("sum3 folds + lowers");
        return;
    }
    let got = exec_int_kernel(&pct_three_module(), "sum3", &[2, 3, 5], "2+3=5").expect("WABT");
    assert_eq!(
        got, "2+3=5",
        "executed WASM \"%d+%d=%d\" % (2,3,5) = {got:?}"
    );
    let got =
        exec_int_kernel(&pct_three_module(), "sum3", &[-1, -2, -3], "-1+-2=-3").expect("WABT");
    assert_eq!(
        got, "-1+-2=-3",
        "executed WASM negative `%`-format = {got:?}"
    );
}

#[test]
fn brace_escape_executes_and_matches_cpython() {
    if !wasm_runtime_available() {
        let _ = emit_module(&brace_escape_module()).expect("esc folds + lowers");
        return;
    }
    // "{{}}={}".format(5) → the `{{` / `}}` decode to literal `{` / `}`, so the
    // produced string is "{}=5", NOT a re-interpolation.
    let got = exec_int_kernel(&brace_escape_module(), "esc", &[5], "{}=5").expect("WABT");
    assert_eq!(got, "{}=5", "executed WASM brace-escape fold = {got:?}");
}

#[test]
fn mixed_str_int_executes_and_matches_cpython() {
    if !wasm_runtime_available() {
        let _ = emit_module(&mixed_module()).expect("greet folds + lowers");
        return;
    }
    let got = exec_mixed("bob", 42, "bob-42").expect("WABT");
    assert_eq!(
        got, "bob-42",
        "executed WASM \"{{}}-{{}}\".format('bob', 42) = {got:?}"
    );
    // "café" — a multibyte str operand copied verbatim, then "-" + str(-7).
    let got = exec_mixed("café", -7, "café--7").expect("WABT");
    assert_eq!(
        got, "café--7",
        "executed WASM mixed fold with a multibyte str operand = {got:?}"
    );
}
