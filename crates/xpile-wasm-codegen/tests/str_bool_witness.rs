//! PMAT-1147 — EXECUTED string-valued CONDITIONAL (`x if c else y`) witness for
//! the native WASM EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## What this slice adds
//!
//! `emit_str_expr` (the string-position lowering) gained an `Expr::IfExpr` arm:
//! a string-valued ternary `x if c else y` lowers to a WASM
//! `(if (result i32) <cond> (then <ptr>) (else <ptr>))` choosing between the two
//! arms' i32 base-pointers. Before this slice an `IfExpr` in a string position
//! fell through to the honest catch-all refusal.
//!
//! ## Why it matters — `str(bool)` now reaches the WASM lane
//!
//! Python `str(b)` over a `bool` is NOT a 0/1 decimal (that would be the wrong
//! answer). The frontend desugars it to `"True" if b else "False"` (an `IfExpr`
//! with string-literal arms, PMAT-502ae) — NOT to `Expr::ToStr` over a bool
//! (which the WASM lane still refuses as an honest type mismatch, since it would
//! mis-convert the 0/1). Before this slice `str(b)` on `--target wasm` failed
//! ("<container/aggregate/builtin expression> in a string position"); now the
//! desugared conditional lowers and executes.
//!
//! ## Why the lowering is correct-by-construction
//!
//! Both arms lower via `emit_str_expr`, so each is an already-correct pointer to
//! a length-prefixed UTF-8 string. The `if` merely selects one pointer — there
//! is NO byte/code-point reasoning here (unlike the byte-search ops), so a
//! multi-byte arm (`"café"`) rides through unchanged. A non-string arm refuses
//! honestly through the recursion, never a silent miscompile.
//!
//! ## The real programs
//!
//! ```python
//! def b2s(b: bool) -> str:    return str(b)          # → "True" if b else "False"
//! def pick(c: bool) -> str:   return "yes" if c else "nope"
//! def tag(c: bool) -> str:    return "café" if c else "z"
//! ```
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `if (result i32)`) on a host without WABT. The
//! pinned outcomes are the CPython ground truth (`str(True)` == "True",
//! `str(False)` == "False", cross-checked in `cpython_str_bool_is_pinned`).

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// Build `def <name>(b: bool) -> str: return <then> if b else <else>` — a
/// string-valued conditional whose arms are string literals. This is exactly the
/// shape the frontend's `str(bool)` desugar produces (`then`/`else` = "True"/
/// "False"), generalised so the witness can also pin arbitrary / multi-byte /
/// different-length arms.
fn ternary_module(name: &str, then_s: &str, else_s: &str) -> Module {
    let body = Expr::IfExpr {
        cond: Box::new(Expr::Ident("b".into())),
        then_expr: Box::new(Expr::LitStr(then_s.into())),
        else_expr: Box::new(Expr::LitStr(else_s.into())),
    };
    let f = Function {
        name: name.into(),
        params: vec![Param {
            name: "b".into(),
            ty: Type::Bool,
            mutable: false,
        }],
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

/// Splice zero-arg wrappers that invoke `$KERNEL(b)` for a constant `bool` (an
/// i32 0/1) and read back the returned string (its i32 length header + each
/// payload byte at `ptr + 8 + i`) onto the emitted module, before its closing
/// `)`. `n_out` = the expected UTF-8 byte length.
fn build_witness_wat(kernel_wat: &str, kernel: &str, b: i32, n_out: usize) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1147 witness: run the kernel for a constant bool, read back\n");
    wat.push_str(&format!(
        "  (func (export \"run_len\") (result i32)\n    i32.const {b}\n    call ${kernel}\n    i32.load)\n"
    ));
    for i in 0..n_out {
        wat.push_str(&format!(
            "  (func (export \"run_byte_{i}\") (result i32)\n    \
               i32.const {b}\n    call ${kernel}\n    \
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

/// Lower a `<then> if b else <else>` kernel, run it in WABT for the given
/// `bool`, and reconstruct the selected arm string. Returns `None` when WABT is
/// absent (the caller skips the value assertion). `expected` is the CPython
/// ground truth for this `b`.
fn exec_ternary(name: &str, then_s: &str, else_s: &str, b: bool, expected: &str) -> Option<String> {
    let kernel_wat =
        emit_module(&ternary_module(name, then_s, else_s)).expect("string ternary lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let n_out = expected.len();
    let wat = build_witness_wat(&kernel_wat, name, i32::from(b), n_out);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-bool-{}-{name}-{}",
        std::process::id(),
        i32::from(b)
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("k.wat");
    let wasm_path = dir.join("k.wasm");
    std::fs::write(&wat_path, &wat).expect("write wat");
    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed for {name}({b}):\n{}\n---WAT---\n{wat}",
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
        "wasm-interp run failed for {name}({b}): stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    let got_len = parse_i32_export(&stdout, "run_len");
    assert_eq!(
        got_len as usize, n_out,
        "{name}({b}) byte length: WASM={got_len} CPython={n_out}\nWAT:\n{wat}"
    );
    let mut bytes = Vec::with_capacity(n_out);
    for i in 0..n_out {
        bytes.push(parse_i32_export(&stdout, &format!("run_byte_{i}")) as u8);
    }
    Some(String::from_utf8(bytes).expect("selected arm bytes are valid UTF-8"))
}

#[test]
fn cpython_str_bool_is_pinned() {
    // The CPython ground truth this witness value-matches. `str(bool)` is the
    // WORD "True"/"False", NOT the 0/1 an int-style conversion would give.
    assert_eq!(format!("{}", true), "true"); // Rust differs — hence the desugar
    assert_eq!(str_bool(true), "True");
    assert_eq!(str_bool(false), "False");
    // A general ternary and a multi-byte arm (CPython semantics).
    assert_eq!(if true { "yes" } else { "nope" }, "yes");
    assert_eq!(if false { "café" } else { "z" }, "z");
    // "café" is 5 UTF-8 bytes (é is 2) — a byte length a naive char count misses.
    assert_eq!("café".len(), 5);
}

/// The Python `str(bool)` answer (the desugar target), spelled out for the pin.
fn str_bool(b: bool) -> &'static str {
    if b {
        "True"
    } else {
        "False"
    }
}

#[test]
fn str_bool_desugar_lowers_to_if_result_i32() {
    // CONSTRUCT assertion (holds with or without WABT): the desugared str(bool)
    // conditional lowers through the production emitter, carrying a typed
    // `if (result i32)` that selects between the two literal pointers, and lays
    // out both arm literals into static data.
    let wat = emit_module(&ternary_module("b2s", "True", "False"))
        .expect("the str(bool) desugar must lower through emit_module");
    assert!(
        wat.contains("(func $b2s (param $b i32) (result i32)"),
        "str return → i32 result (heap pointer), bool param → i32:\n{wat}"
    );
    assert!(
        wat.contains("if (result i32)"),
        "the conditional must lower to a typed `if (result i32)`:\n{wat}"
    );
    // Both arm literals are laid out (collect_expr_literals recurses into IfExpr).
    assert!(
        wat.contains("True") && wat.contains("False"),
        "both arm literals must be laid out into static data:\n{wat}"
    );
}

#[test]
fn str_bool_executes_in_wasm_and_matches_cpython() {
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1147: skipping EXECUTED str(bool) witness — WABT (wat2wasm / \
             wasm-interp) absent. The desugar still lowered above."
        );
        // Still exercise the EMIT path so a lowering regression is caught.
        let _ = emit_module(&ternary_module("b2s", "True", "False")).expect("lowers");
        return;
    }
    // str(True) → "True", str(False) → "False" — the whole point (not 1/0).
    assert_eq!(
        exec_ternary("b2s", "True", "False", true, "True").as_deref(),
        Some("True")
    );
    assert_eq!(
        exec_ternary("b2s", "True", "False", false, "False").as_deref(),
        Some("False")
    );
}

#[test]
fn general_and_multibyte_string_ternary_execute_and_match_cpython() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1147: skipping EXECUTED general-ternary witness — WABT absent.");
        return;
    }
    // Different-length arms (3 vs 4 bytes) — the selected arm's own length header
    // is read back, so an arm-length difference is handled.
    assert_eq!(
        exec_ternary("pick", "yes", "nope", true, "yes").as_deref(),
        Some("yes")
    );
    assert_eq!(
        exec_ternary("pick", "yes", "nope", false, "nope").as_deref(),
        Some("nope")
    );
    // A MULTI-BYTE arm ("café" = 5 UTF-8 bytes) rides through as an opaque
    // pointer — no byte/code-point reasoning in the `if` lowering.
    assert_eq!(
        exec_ternary("tag", "café", "z", true, "café").as_deref(),
        Some("café")
    );
    assert_eq!(
        exec_ternary("tag", "café", "z", false, "z").as_deref(),
        Some("z")
    );
}

#[test]
fn non_string_arm_in_string_position_refuses_honestly() {
    // A str-position `IfExpr` whose `else` arm is a non-string (an int) must
    // refuse through the recursion — never a silent miscompile. (The frontend's
    // type checker would reject this program; the emitter refuses it too as a
    // defence in depth.)
    let body = Expr::IfExpr {
        cond: Box::new(Expr::Ident("b".into())),
        then_expr: Box::new(Expr::LitStr("ok".into())),
        else_expr: Box::new(Expr::LitInt(0)),
    };
    let f = Function {
        name: "bad".into(),
        params: vec![Param {
            name: "b".into(),
            ty: Type::Bool,
            mutable: false,
        }],
        return_type: Type::Str,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    let module = Module {
        name: "bad_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    };
    let err = emit_module(&module).expect_err("a non-string ternary arm must refuse");
    assert!(
        err.to_string().contains("string position"),
        "the refusal must name the string-position mismatch: {err}"
    );
}
