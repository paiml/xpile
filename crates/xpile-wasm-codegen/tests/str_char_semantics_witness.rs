//! PMAT-1032 — witness for CHAR-oriented string semantics on the WASM lane,
//! the sweep-#11 (PMAT-1031 finding 2) non-ASCII divergence cluster.
//!
//! CPython strings are sequences of Unicode CODE POINTS; the WASM str ABI is
//! length-prefixed UTF-8 BYTES. Before PMAT-1032 every Python-visible read was
//! byte-oriented and SILENTLY diverged on non-ASCII input:
//!   * `len("héllo")` returned 6 (bytes), CPython 5 (chars) — b07;
//!   * `for ch in "abé"` iterated 4 times, CPython 3 — b08;
//!   * `ord("é")` trapped on the byte-count!=1 guard, CPython 233 — b09;
//!   * `s[-1]` trapped, CPython indexes from the end — b05;
//!   * `chr(233)` emitted the lone byte 0xE9 (not valid UTF-8, internally
//!     inconsistent with the 2-byte literal encoding of the same char), and
//!     `chr(n)` for ANY n > 255 silently truncated to the low byte.
//!
//! These kernels drive the REAL emit (string literals need no host preload)
//! through wat2wasm + wasm-interp and assert the executed value VALUE-MATCHES
//! CPython, including the IndexError / TypeError / ValueError trap analogues.
//! Gated on `wasm_runtime_available()`.

use std::process::Command;

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

fn lit(s: &str) -> Expr {
    Expr::LitStr(s.into())
}
fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn let_str(n: &str, v: Expr) -> Stmt {
    Stmt::Let {
        name: n.into(),
        ty: Type::Str,
        value: v,
        mutable: false,
    }
}
fn let_int(n: &str, v: i64) -> Stmt {
    Stmt::Let {
        name: n.into(),
        ty: Type::I64,
        value: Expr::LitInt(v),
        mutable: true,
    }
}
fn ord_of(e: Expr) -> Expr {
    Expr::Ord { value: Box::new(e) }
}
fn char_at(s: &str, i: i64) -> Expr {
    Expr::StrCharAt {
        string: Box::new(ident(s)),
        index: Box::new(Expr::LitInt(i)),
    }
}
fn chr_of(n: i64) -> Expr {
    Expr::Chr {
        value: Box::new(Expr::LitInt(n)),
    }
}
fn add(l: Expr, r: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Add,
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}

/// A zero-arg `run() -> int` kernel — string literals need no host preload,
/// so `wasm-interp --run-all-exports` drives it directly.
fn kernel(stmts: Vec<Stmt>, tail: Expr) -> Module {
    Module {
        name: "sc".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(Function {
            name: "run".into(),
            params: vec![],
            return_type: Type::I64,
            body: Block {
                stmts,
                trailing_return: tail,
            },
        })],
        ffi_boundaries: Vec::new(),
    }
}

/// `for ch in s: <body>` over the str local `s`.
fn for_ch_in(s: &str, body: Vec<Stmt>) -> Stmt {
    Stmt::ForEach {
        var: "ch".into(),
        iter: Expr::StrChars {
            string: Box::new(ident(s)),
        },
        elem_ty: Type::Str,
        body,
        over_keys: false,
        dict_guard: None,
        mutate_elems: false,
    }
}

/// Run a kernel; `Ok(value)` or `Err(())` on a trap. Files are numbered by
/// an atomic counter — the tests run as PARALLEL threads of one process, so
/// a pid-keyed shared path would race (kernel A's wasm read as kernel B's).
fn run(m: &Module) -> Result<i64, ()> {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let k = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let wat = emit_module(m).expect("kernel lowers");
    let dir = std::env::temp_dir().join(format!("xpile-strchar-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wp = dir.join(format!("p{k}.wat"));
    let bp = dir.join(format!("p{k}.wasm"));
    std::fs::write(&wp, &wat).unwrap();
    let a = Command::new("wat2wasm")
        .arg(&wp)
        .arg("-o")
        .arg(&bp)
        .output()
        .unwrap();
    assert!(
        a.status.success(),
        "wat2wasm:\n{}\n{wat}",
        String::from_utf8_lossy(&a.stderr)
    );
    let r = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&bp)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&r.stdout);
    let line = stdout
        .lines()
        .find(|l| l.starts_with("run()"))
        .unwrap_or("");
    if line.contains("unreachable executed") || line.is_empty() {
        return Err(());
    }
    let v: u64 = line
        .rsplit_once(':')
        .expect("scalar")
        .1
        .trim()
        .parse()
        .expect("u64");
    Ok(v as i64)
}

#[test]
fn len_counts_chars_not_bytes() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1032: skipping char-semantics witness — WABT absent");
        return;
    }
    // b07: len("héllo") == 5 (CPython), was 6 (bytes).
    let m = kernel(
        vec![let_str("s", lit("héllo"))],
        Expr::Len(Box::new(ident("s"))),
    );
    assert_eq!(run(&m), Ok(5), "len(\"héllo\") must be the CHAR count 5");
    // 3-byte chars: len("中文") == 2 (6 bytes).
    let m = kernel(
        vec![let_str("s", lit("中文"))],
        Expr::Len(Box::new(ident("s"))),
    );
    assert_eq!(run(&m), Ok(2), "len(\"中文\") must be the CHAR count 2");
}

#[test]
fn for_ch_iterates_chars_and_checksums_code_points() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1032: skipping char-semantics witness — WABT absent");
        return;
    }
    // b08: `for ch in "abé"` iterates 3 times (was 4), and the checksum is
    // the CODE-POINT sum 97 + 98 + 233 = 428 — the PMAT-1030 desugar rides
    // the char-oriented len + s[i] verbatim.
    let m = kernel(
        vec![
            let_str("s", lit("abé")),
            let_int("n", 0),
            let_int("t", 0),
            for_ch_in(
                "s",
                vec![
                    Stmt::Assign {
                        name: "n".into(),
                        value: add(ident("n"), Expr::LitInt(1)),
                    },
                    Stmt::Assign {
                        name: "t".into(),
                        value: add(ident("t"), ord_of(ident("ch"))),
                    },
                ],
            ),
        ],
        add(
            Expr::BinOp {
                op: BinOp::Mul,
                lhs: Box::new(ident("n")),
                rhs: Box::new(Expr::LitInt(1000)),
            },
            ident("t"),
        ),
    );
    // CPython: 3 iterations, checksum 428 → 3*1000 + 428 = 3428.
    assert_eq!(run(&m), Ok(3428), "for-ch over \"abé\" == CPython 3428");
    // Mixed widths incl. a 3-byte char: "aé中" → 97 + 233 + 20013 = 20343.
    let m = kernel(
        vec![
            let_str("s", lit("aé中")),
            let_int("t", 0),
            for_ch_in(
                "s",
                vec![Stmt::Assign {
                    name: "t".into(),
                    value: add(ident("t"), ord_of(ident("ch"))),
                }],
            ),
        ],
        ident("t"),
    );
    assert_eq!(
        run(&m),
        Ok(20343),
        "checksum over \"aé中\" == CPython 20343"
    );
}

#[test]
fn ord_decodes_multibyte_chars() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1032: skipping char-semantics witness — WABT absent");
        return;
    }
    // b09: ord("é") == 233 (was a byte_count!=1 trap).
    let m = kernel(vec![let_str("s", lit("é"))], ord_of(ident("s")));
    assert_eq!(run(&m), Ok(233), "ord(\"é\") == CPython 233");
    // 3-byte: ord("中") == 20013.
    let m = kernel(vec![let_str("s", lit("中"))], ord_of(ident("s")));
    assert_eq!(run(&m), Ok(20013), "ord(\"中\") == CPython 20013");
    // Indexed decode past earlier multi-byte chars: ord("héllo"[1]) == 233.
    let m = kernel(vec![let_str("s", lit("héllo"))], ord_of(char_at("s", 1)));
    assert_eq!(run(&m), Ok(233), "ord(s[1]) over \"héllo\" == CPython 233");
    // ord of a MULTI-char string still traps (Python TypeError analogue) —
    // now keyed on CHAR count, so 2 chars in 3 bytes traps too.
    let m = kernel(vec![let_str("s", lit("aé"))], ord_of(ident("s")));
    assert_eq!(run(&m), Err(()), "ord(\"aé\") must trap (TypeError)");
}

#[test]
fn negative_index_wraps_pythonically() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1032: skipping char-semantics witness — WABT absent");
        return;
    }
    // b05: s[-1] indexes from the END (was a trap). ord(s[-1]) over "abé"
    // == 233 — negative AND multi-byte at once.
    let m = kernel(vec![let_str("s", lit("abé"))], ord_of(char_at("s", -1)));
    assert_eq!(run(&m), Ok(233), "ord(s[-1]) over \"abé\" == CPython 233");
    // s[-4] over "héllo" → index 1 → é.
    let m = kernel(vec![let_str("s", lit("héllo"))], ord_of(char_at("s", -4)));
    assert_eq!(run(&m), Ok(233), "ord(s[-4]) over \"héllo\" == CPython 233");
    // Too-negative still traps (Python IndexError): "ab"[-3].
    let m = kernel(vec![let_str("s", lit("ab"))], ord_of(char_at("s", -3)));
    assert_eq!(run(&m), Err(()), "s[-3] over \"ab\" must trap (IndexError)");
    // Out-of-range positive still traps: "ab"[5].
    let m = kernel(vec![let_str("s", lit("ab"))], ord_of(char_at("s", 5)));
    assert_eq!(run(&m), Err(()), "s[5] over \"ab\" must trap (IndexError)");
}

#[test]
fn chr_encodes_utf8_and_round_trips() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1032: skipping char-semantics witness — WABT absent");
        return;
    }
    // chr(233) is ONE char (was the invalid lone byte 0xE9): len == 1 and it
    // content-equals the literal "é" (identical UTF-8 bytes — the internal
    // inconsistency the sweep flagged is gone).
    let m = kernel(
        vec![let_str("c", chr_of(233))],
        Expr::Len(Box::new(ident("c"))),
    );
    assert_eq!(run(&m), Ok(1), "len(chr(233)) == CPython 1");
    let m = kernel(
        vec![let_str("c", chr_of(233))],
        Expr::IfExpr {
            cond: Box::new(Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(ident("c")),
                rhs: Box::new(lit("é")),
            }),
            then_expr: Box::new(Expr::LitInt(1)),
            else_expr: Box::new(Expr::LitInt(0)),
        },
    );
    assert_eq!(run(&m), Ok(1), "chr(233) == \"é\" (content equality)");
    // Round-trips across every encoded width: 1-byte (65), 2-byte (233),
    // 3-byte (20013), 4-byte (128169 = U+1F4A9).
    for cp in [65_i64, 233, 20013, 128_169] {
        let m = kernel(vec![let_str("c", chr_of(cp))], ord_of(ident("c")));
        assert_eq!(run(&m), Ok(cp), "ord(chr({cp})) must round-trip");
    }
    // Range guard (Python ValueError analogue): chr(0x110000) and chr(-1)
    // trap — the old lowering silently masked both to a single byte.
    let m = kernel(vec![let_str("c", chr_of(0x110000))], ord_of(ident("c")));
    assert_eq!(run(&m), Err(()), "chr(0x110000) must trap (ValueError)");
    let m = kernel(vec![let_str("c", chr_of(-1))], ord_of(ident("c")));
    assert_eq!(run(&m), Err(()), "chr(-1) must trap (ValueError)");
    eprintln!(
        "PMAT-1032: char-semantics witness PASSED — len/iter/ord/chr/s[-k] \
         are code-point-exact vs CPython across 1..4-byte UTF-8"
    );
}
