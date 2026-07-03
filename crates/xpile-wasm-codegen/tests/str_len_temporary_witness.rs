//! PMAT-1148 — EXECUTED `len(<str TEMPORARY>)` witness for the native WASM
//! EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## What this slice adds
//!
//! `emit_len` gained a NON-name branch: `len()` of a string-VALUED temporary
//! (`Concat` `a+b`, an `s * n` `Repeat`, an `s[lo:hi]` `Slice`, a str-valued
//! `if`/`else` — the `str(bool)` desugar — a `Chr`, an `s[i]`, or a str-returning
//! call) lowers via `emit_str_expr` to an i32 base-pointer to a length-prefixed
//! region, then `$__wasm_str_charlen` (the CODE-POINT count, exact as for a str
//! NAME). Before this slice `len()` of anything but an `Expr::Ident` refused.
//!
//! ## Why the Python frontend makes this `StrMethod`, not `Expr::Len`
//!
//! Python `len(s)` over a `str` counts CODE POINTS, so the frontend synthesises
//! it as `Expr::StrMethod { op: CharCount, recv }` (NOT `Expr::Len`, which is the
//! byte-length node reserved for lists). The value dispatch routes
//! `StrMethod{CharCount}` → `emit_len(recv)`. This witness therefore builds the
//! REAL node a user hits.
//!
//! ## The latent scan gap this slice ALSO closes (the interesting bug)
//!
//! The helper-requirement scans (`expr_has_str_slice` → `$__wasm_str_slice`,
//! `expr_has_int_to_str` → `$__wasm_int_to_str`, `expr_has_str_contains` →
//! `$__wasm_str_contains`, `expr_has_str_eq` → `$__wasm_str_eq`) previously
//! lacked an `Expr::StrMethod` arm, so they never recursed into `recv`. That was
//! HARMLESS only because the old `emit_len` refused a temporary before any WAT
//! was produced. Once `emit_len` accepts a temporary, `len(s[1:4])` /
//! `len(str(n))` emit a `call $__wasm_str_slice` / `$__wasm_int_to_str` against a
//! helper NEVER DECLARED — a hard `wat2wasm` "undefined function" failure. The
//! four scans now carry the `StrMethod` arm; `helpers_are_declared_for_len_of_*`
//! is the regression guard (it fails on the pre-fix code), and the executed
//! witness is the end-to-end backstop.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + declares the callee helpers) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, SourceLang, StrMethodOp, Type};

/// `len(recv)` as the frontend spells it: `StrMethod { CharCount, recv }`.
fn len_of(recv: Expr) -> Expr {
    Expr::StrMethod {
        recv: Box::new(recv),
        op: StrMethodOp::CharCount,
        args: vec![],
    }
}

fn lit_s(s: &str) -> Expr {
    Expr::LitStr(s.into())
}

/// A zero-arg `def <name>() -> int: return <body>` — no params, so
/// `wasm-interp --run-all-exports` invokes it directly.
fn nullary(name: &str, body: Expr) -> Function {
    Function {
        name: name.into(),
        params: vec![],
        return_type: Type::I64,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    }
}

/// Each `(export name, len-recv temporary, CPython len)`. The recv shapes cover
/// every string-VALUED temporary `emit_str_expr` accepts.
fn cases() -> Vec<(&'static str, Expr, i64)> {
    vec![
        // len("café" + "•λ") == 6
        (
            "f_concat",
            len_of(Expr::Concat {
                lhs: Box::new(lit_s("café")),
                rhs: Box::new(lit_s("•λ")),
            }),
            6,
        ),
        // len("café•λ"[1:-1]) == len("afé•") == 4  (SLICE under StrMethod)
        (
            "f_slice",
            len_of(Expr::Slice {
                collection: Box::new(lit_s("café•λ")),
                lo: Some(Box::new(Expr::LitInt(1))),
                hi: Some(Box::new(Expr::LitInt(-1))),
                of_str: true,
                step: None,
            }),
            4,
        ),
        // len(str(-12345)) == len("-12345") == 6  (ToStr under StrMethod)
        (
            "f_strint",
            len_of(Expr::ToStr {
                value: Box::new(Expr::LitInt(-12345)),
                of_float: false,
            }),
            6,
        ),
        // len("λ" * 3) == 3  (Repeat under StrMethod)
        (
            "f_repeat",
            len_of(Expr::Repeat {
                seq: Box::new(lit_s("λ")),
                n: Box::new(Expr::LitInt(3)),
                of_str: true,
            }),
            3,
        ),
        // len(chr(955)) == len("λ") == 1
        (
            "f_chr",
            len_of(Expr::Chr {
                value: Box::new(Expr::LitInt(955)),
            }),
            1,
        ),
        // len("yes" if "b" in "abc" else "n") == 3  (StrContains in an IfExpr
        // cond under StrMethod — guards the expr_has_str_contains arm)
        (
            "f_if_contains",
            len_of(Expr::IfExpr {
                cond: Box::new(Expr::StrContains {
                    haystack: Box::new(lit_s("abc")),
                    needle: Box::new(lit_s("b")),
                }),
                then_expr: Box::new(lit_s("yes")),
                else_expr: Box::new(lit_s("n")),
            }),
            3,
        ),
        // len("longer" if "hi" == "hi" else "z") == 6  (str `==` in an IfExpr
        // cond under StrMethod — guards the expr_has_str_eq arm)
        (
            "f_if_streq",
            len_of(Expr::IfExpr {
                cond: Box::new(Expr::BinOp {
                    op: BinOp::Eq,
                    lhs: Box::new(lit_s("hi")),
                    rhs: Box::new(lit_s("hi")),
                }),
                then_expr: Box::new(lit_s("longer")),
                else_expr: Box::new(lit_s("z")),
            }),
            6,
        ),
    ]
}

fn module_of_all() -> Module {
    Module {
        name: "len_temporary".into(),
        source_lang: SourceLang::Rust,
        items: cases()
            .into_iter()
            .map(|(n, body, _)| Item::Function(nullary(n, body)))
            .collect(),
        ffi_boundaries: Vec::new(),
    }
}

/// The pre-fix bug: `len(<slice>)` / `len(str(n))` CALLED a helper the scan never
/// declared. Assert every callee helper is DEFINED, not just called — this fails
/// on the pre-PMAT-1148 scans (missing `(func $__wasm_str_slice …)` etc.), with
/// no WABT needed.
#[test]
fn helpers_are_declared_for_len_of_temporaries() {
    let wat = xpile_wasm_codegen::emit_module(&module_of_all()).expect("len(<temporary>) lowers");
    // len itself is always a code-point count.
    assert!(
        wat.contains("call $__wasm_str_charlen") && wat.contains("(func $__wasm_str_charlen"),
        "len is a code-point count via a DEFINED charlen helper:\n{wat}"
    );
    for helper in [
        "$__wasm_str_slice",    // len("…"[1:-1])
        "$__wasm_int_to_str",   // len(str(n))
        "$__wasm_str_repeat",   // len("λ" * 3)
        "$__wasm_str_contains", // len("yes" if "b" in s else "n")
        "$__wasm_str_eq",       // len("…" if s == t else "…")
    ] {
        assert!(
            wat.contains(&format!("call {helper}")),
            "expected a `call {helper}` (the temporary uses it):\n{wat}"
        );
        assert!(
            wat.contains(&format!("(func {helper}")),
            "REGRESSION: `{helper}` is CALLED but never DEFINED — the scan lost \
             the `StrMethod` recv recursion (the PMAT-1148 latent gap):\n{wat}"
        );
    }
}

#[test]
fn refuses_len_of_a_non_string_temporary() {
    // len([1, 2, 3]) — a list literal carries no length header and is not a
    // string-valued temporary; still refused honestly (no silent miscompile).
    let m = Module {
        name: "bad".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(nullary(
            "bad",
            Expr::Len(Box::new(Expr::ListLit(vec![
                Expr::LitInt(1),
                Expr::LitInt(2),
                Expr::LitInt(3),
            ]))),
        ))],
        ffi_boundaries: Vec::new(),
    };
    let err = xpile_wasm_codegen::emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("unsupported") && err.contains("len()"),
        "len of a list literal refuses honestly: {err}"
    );
}

#[test]
fn cpython_lens_are_pinned() {
    // Ground truth recomputed here so the pinned constants in `cases()` cannot
    // drift from CPython silently.
    assert_eq!("café•λ"[..].chars().count(), 6);
    assert_eq!("café•λ".chars().collect::<Vec<_>>()[1..5].len(), 4); // "afé•"
    assert_eq!((-12345_i64).to_string().chars().count(), 6);
    assert_eq!("λ".repeat(3).chars().count(), 3);
    assert_eq!(char::from_u32(955).unwrap().to_string().chars().count(), 1);
}

#[test]
fn real_len_temporary_programs_execute_in_wasm_and_match_cpython() {
    let cs = cases();
    let wat =
        xpile_wasm_codegen::emit_module(&module_of_all()).expect("len(<temporary>) module lowers");
    if !xpile_wasm_codegen::wasm_runtime_available() {
        eprintln!(
            "PMAT-1148: skipping EXECUTED len(<str temporary>) witness — WABT \
             (wat2wasm / wasm-interp) absent. The module lowered and DECLARED its \
             callee helpers (asserted in helpers_are_declared_for_len_of_temporaries); \
             the pinned outcomes {:?} are the CPython ground truth.",
            cs.iter().map(|(n, _, v)| (*n, *v)).collect::<Vec<_>>()
        );
        return;
    }

    let dir = std::env::temp_dir().join(format!("xpile-len-temp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wp = dir.join("p.wat");
    let bp = dir.join("p.wasm");
    std::fs::write(&wp, &wat).unwrap();

    let a = Command::new("wat2wasm")
        .arg(&wp)
        .arg("-o")
        .arg(&bp)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        a.status.success(),
        "wat2wasm failed (a called-but-undeclared helper is the classic cause):\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&a.stderr)
    );
    let r = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&bp)
        .output()
        .expect("spawn wasm-interp");
    assert!(r.status.success(), "wasm-interp: {:?}", r);
    let stdout = String::from_utf8_lossy(&r.stdout);

    for (name, _, expect) in &cs {
        let needle = format!("{name}() => i64:{expect}");
        assert!(
            stdout.contains(&needle),
            "len temporary `{name}` must execute to {expect} (== CPython), got:\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1148: len(<str temporary>) witness PASSED — concat/slice/str(int)/\
         repeat/chr/str-valued-if all lower + execute to the CPython code-point \
         length over the UTF-8 ABI; the StrMethod-recv scan gap is closed."
    );
}
