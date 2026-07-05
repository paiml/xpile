//! PMAT-1001 — regression witness for Python negative list-index normalization
//! on the WASM lane, found by the second adversarial CPython-differential sweep.
//!
//! CPython wraps a negative index to the tail (`xs[-1]` == `xs[len-1]`). The
//! WASM list lane previously only handled the READ-side LITERAL case (the
//! frontend folds `xs[-1]` to `xs[len-1]`) and TRAPPED on everything else — a
//! RUNTIME-negative read (`xs[len(xs)-5]`) and ANY store-side negative
//! (`xs[-1] = v`, which the frontend does not fold). The fix normalizes at
//! RUNTIME in the shared bounds-checked address emit: `if i < 0 { i += len }`
//! before the guard, uniform across read + store; a still-negative result
//! (`i < -len`) is caught by the guard (Python IndexError).
//!
//! PMAT-1289 UPDATE: the `len(xs) - k` HIR shape is ambiguous (the frontend
//! folds the literal `xs[-k]` to it; a user can also write it verbatim), and
//! the two readings DIVERGE in CPython when `n < k ≤ 2n`. The emit now unwraps
//! the shape to the raw `-k` (the literal reading) so the runtime normalise
//! applies exactly once — `xs[-2]` on a 1-element list previously READ slot 0
//! where CPython raises IndexError (a silent miscompile, found by the
//! PMAT-1289 probe sweep). The user-written verbatim form now traps on that
//! corner, exactly where the Rust lane's `(len - k) as usize` panics.
//!
//! This witness drives int-list kernels over a preloaded fixture and asserts the
//! executed value VALUE-MATCHES CPython, incl. the out-of-range + too-negative
//! IndexError traps. Gated on `wasm_runtime_available()`.

use std::process::Command;

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, Param, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// The preloaded fixture list `[10, 20, 30, 40]`.
const FIXTURE: &[i64] = &[10, 20, 30, 40];

fn xs_param() -> Vec<Param> {
    vec![Param {
        name: "xs".into(),
        ty: Type::List(Box::new(Type::I64)),
        mutable: false,
    }]
}
fn idx(collection: &str, index: Expr) -> Expr {
    Expr::Index {
        collection: Box::new(Expr::Ident(collection.into())),
        index: Box::new(index),
    }
}
fn sub(l: Expr, r: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Sub,
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}
fn len(name: &str) -> Expr {
    Expr::Len(Box::new(Expr::Ident(name.into())))
}

fn kernel(name: &str, stmts: Vec<Stmt>, tail: Expr) -> Module {
    Module {
        name: name.into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(Function {
            name: "kernel".into(),
            params: xs_param(),
            return_type: Type::I64,
            body: Block {
                stmts,
                trailing_return: tail,
            },
        })],
        ffi_boundaries: Vec::new(),
    }
}

/// Splice a driver preloading FIXTURE at address 0 (i32 count @ 0, i64 elems @ 8)
/// and a zero-arg `run` export calling `kernel(0)`.
fn drive(kernel_wat: &str) -> String {
    let close = kernel_wat.rfind(')').expect("closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    let le = |v: i64| {
        v.to_le_bytes()
            .iter()
            .map(|b| format!("\\{b:02x}"))
            .collect::<String>()
    };
    wat.push_str(&format!(
        "  (data (i32.const 0) \"{}\")\n",
        (FIXTURE.len() as i32)
            .to_le_bytes()
            .iter()
            .map(|b| format!("\\{b:02x}"))
            .collect::<String>()
    ));
    for (k, v) in FIXTURE.iter().enumerate() {
        wat.push_str(&format!(
            "  (data (i32.const {}) \"{}\")\n",
            8 + k * 8,
            le(*v)
        ));
    }
    wat.push_str("  (func (export \"run\") (result i64)\n    i32.const 0\n    call $kernel)\n)\n");
    wat
}

/// Run a driven kernel; returns `Ok(value)` or `Err(())` on a trap.
fn run(kernel_wat: &str) -> Result<i64, ()> {
    let wat = drive(kernel_wat);
    let dir = std::env::temp_dir().join(format!("xpile-negidx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wp = dir.join("p.wat");
    let bp = dir.join("p.wasm");
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
    if line.contains("unreachable executed") {
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
fn negative_list_index_normalizes_and_matches_cpython() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1001: skipping negative-index witness — WABT absent");
        return;
    }

    // The `len(xs) - k` shape — PMAT-1289 POSTURE FLIP. This HIR shape is
    // AMBIGUOUS: the frontend folds the literal `xs[-5]` to it (PMAT-570), and
    // a user can write `xs[len(xs) - 5]` verbatim; on a 4-element list CPython
    // raises IndexError for the first and reads 40 for the second. The emit
    // must pick ONE reading — it now unwraps the shape back to the raw `-5`
    // (the literal reading), because (a) the literal form dominates real code,
    // (b) the OLD wrap-to-40 behaviour silently READ where `xs[-5]` raises —
    // a miscompile, the worse failure mode — and (c) a trap here is exactly
    // where the RUST lane's `(len - 5) as usize` panics, keeping the two
    // backends consistent on the ambiguous corner. So: -5 → += 4 → still
    // negative → IndexError trap.
    let c1 = kernel("c1", vec![], idx("xs", sub(len("xs"), Expr::LitInt(5))));
    assert_eq!(
        run(&emit_module(&c1).unwrap()),
        Err(()),
        "xs[len-5] (the folded xs[-5]) must trap (IndexError) on a 4-list"
    );

    // The same shape IN RANGE is unambiguous — both readings agree:
    // xs[len(xs)-1] == xs[-1] == 40 (unwrap → -1 → += 4 → 3).
    let c1b = kernel("c1b", vec![], idx("xs", sub(len("xs"), Expr::LitInt(1))));
    assert_eq!(
        run(&emit_module(&c1b).unwrap()),
        Ok(40),
        "xs[len-1] (== the folded xs[-1]) should read 40"
    );

    // Store negative LITERAL then read: xs[-1] = 7; return xs[3]  → 7.
    let c2 = kernel(
        "c2",
        vec![Stmt::IndexAssign {
            list_name: "xs".into(),
            indices: vec![Expr::LitInt(-1)],
            value: Expr::LitInt(7),
        }],
        idx("xs", Expr::LitInt(3)),
    );
    assert_eq!(
        run(&emit_module(&c2).unwrap()),
        Ok(7),
        "xs[-1]=7 should write xs[3]"
    );

    // Positive index (regression): xs[2] = 30.
    let p = kernel("p", vec![], idx("xs", Expr::LitInt(2)));
    assert_eq!(run(&emit_module(&p).unwrap()), Ok(30), "xs[2] regression");

    // Out-of-range (>= len): xs[10] → IndexError trap.
    let oor = kernel("oor", vec![], idx("xs", Expr::LitInt(10)));
    assert_eq!(
        run(&emit_module(&oor).unwrap()),
        Err(()),
        "xs[10] must trap (IndexError)"
    );

    // Too-negative (< -len): xs[-10] on a 4-list → still negative after +len → trap.
    let tn = kernel("tn", vec![], idx("xs", Expr::LitInt(-10)));
    assert_eq!(
        run(&emit_module(&tn).unwrap()),
        Err(()),
        "xs[-10] must trap (IndexError)"
    );

    eprintln!(
        "PMAT-1001: negative-index witness PASSED — runtime + store negative list \
         indices wrap to the tail (== CPython), while out-of-range and \
         too-negative still trap (IndexError). Executed in WABT over [10,20,30,40]."
    );
}
