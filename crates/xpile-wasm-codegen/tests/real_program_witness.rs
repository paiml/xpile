//! PMAT-981 — END-TO-END REAL-PROGRAM composition witness for the native
//! WASM EMIT lane (`C-COMPILE-RUST-TO-WASM`).
//!
//! The prior WASM witnesses (`wasm_witness.rs`) each exercise ONE construct
//! in isolation: PMAT-952 a saxpy scalar kernel, PMAT-966 a bare `xs[i]`
//! read, PMAT-968 `len(xs)` + the bounds trap, PMAT-978 a `xs[i] = v`
//! write. This slice proves those constructs COMPOSE on a genuine Python
//! numerical program — a running weighted-sum reduction — not just on
//! single-construct probes.
//!
//! ## The real program
//!
//! ```python
//! def weighted_sum(xs: list[float]) -> float:
//!     total = 0.0       # f64 accumulator
//!     w = 1.0           # running f64 weight (1.0, 2.0, 3.0, …)
//!     i = 0             # i64 loop counter
//!     n = len(xs)       # i64 — len() over the list param
//!     while i < n:      # i64 comparison drives the loop
//!         total = total + xs[i] * w   # indexing + f64 mul + f64 add
//!         w = w + 1.0                 # advance the weight
//!         i = i + 1                   # advance the counter (i64 arith)
//!     return total      # scalar f64 return
//! ```
//!
//! This composes EVERY listed construct in ONE function: the `list[float]`
//! **parameter** (PMAT-966 i32 base-pointer into linear memory), `len(xs)`
//! (PMAT-968 header read), bounds-checked **indexing** `xs[i]` (PMAT-966/968),
//! a `while` loop with an **i64 comparison** condition, **f64 arithmetic**
//! (`*`, `+`) interleaved with **i64 arithmetic** (the counter), three
//! local accumulators (`Let` + `Assign`), and a **scalar f64 return**. It is
//! the `range`/`while` running-computation kernel the task asks for, written
//! as the meta-HIR `Module` the Python frontend WOULD produce for the source
//! above.
//!
//! ## Witness shape (mirrors PMAT-966/968 in `wasm_witness.rs`)
//!
//! `wasm-interp --run-all-exports` only invokes zero-arg exports, and the
//! kernel takes an `(i32 base-pointer)` argument. So the test lowers the
//! real program through the REAL `emit_module`, then splices a self-contained
//! driver onto the emitted module: a length-prefixed `(data …)` segment
//! pre-loads the fixture list at base 0 (an `i32` count header at base+0, the
//! packed `f64` elements at base+8 — the exact PMAT-968 ABI), and one zero-arg
//! `run` export calls `$weighted_sum` with base-pointer `0`. WABT assembles
//! (`wat2wasm`) and executes (`wasm-interp`) it; the executed `f64` result is
//! asserted to VALUE-MATCH the CPython result of the identical program.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (asserting the EMIT
//! path still lowers + carries the composed shape) on a host without WABT,
//! so free CI stays green.

use std::process::Command;

use xpile_meta_hir::{
    BinOp, Block, Expr, FloatOp, Function, Item, Module, Param, SourceLang, Stmt, Type,
};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// The fixture list the real program runs over —
/// `[10.5, -3.25, 0.0, 42.0, 7.125, -100.0]`. Every value is exactly
/// representable in `f64`, and the running weighted sum
/// `Σ xs[i]*(i+1)` is `-392.375` (also exact), so the executed WASM result
/// and the CPython result agree bit-for-bit.
const LIST_FIXTURE: &[f64] = &[10.5, -3.25, 0.0, 42.0, 7.125, -100.0];

/// The CPython reference value of `weighted_sum(LIST_FIXTURE)`. Computed
/// independently (`python3`) and pinned here:
///   10.5*1 + (-3.25)*2 + 0.0*3 + 42.0*4 + 7.125*5 + (-100.0)*6 = -392.375
const CPYTHON_RESULT: f64 = -392.375;

/// Build the meta-HIR `Module` the Python frontend would produce for the
/// `weighted_sum` program in the file doc-comment. Built in-crate (the
/// Python→meta-HIR frontend is not reachable from this codegen crate without
/// pulling depyler-frontend), exactly as the PMAT-966/968 list kernels are.
fn weighted_sum_module() -> Module {
    // total = total + xs[i] * w   (f64 + (f64 index * f64))
    let acc_step = Stmt::Assign {
        name: "total".into(),
        value: Expr::FloatBinOp {
            op: FloatOp::Add,
            lhs: Box::new(Expr::Ident("total".into())),
            rhs: Box::new(Expr::FloatBinOp {
                op: FloatOp::Mul,
                lhs: Box::new(Expr::Index {
                    collection: Box::new(Expr::Ident("xs".into())),
                    index: Box::new(Expr::Ident("i".into())),
                }),
                rhs: Box::new(Expr::Ident("w".into())),
            }),
        },
    };
    // w = w + 1.0   (advance the running weight)
    let weight_step = Stmt::Assign {
        name: "w".into(),
        value: Expr::FloatBinOp {
            op: FloatOp::Add,
            lhs: Box::new(Expr::Ident("w".into())),
            rhs: Box::new(Expr::LitFloat(1.0)),
        },
    };
    // i = i + 1   (advance the i64 counter)
    let counter_step = Stmt::Assign {
        name: "i".into(),
        value: Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::Ident("i".into())),
            rhs: Box::new(Expr::LitInt(1)),
        },
    };

    let f = Function {
        name: "weighted_sum".into(),
        params: vec![Param {
            name: "xs".into(),
            ty: Type::List(Box::new(Type::F64)),
            mutable: false,
        }],
        return_type: Type::F64,
        body: Block {
            stmts: vec![
                // total = 0.0
                Stmt::Let {
                    name: "total".into(),
                    ty: Type::F64,
                    value: Expr::LitFloat(0.0),
                    mutable: true,
                },
                // w = 1.0
                Stmt::Let {
                    name: "w".into(),
                    ty: Type::F64,
                    value: Expr::LitFloat(1.0),
                    mutable: true,
                },
                // i = 0
                Stmt::Let {
                    name: "i".into(),
                    ty: Type::I64,
                    value: Expr::LitInt(0),
                    mutable: true,
                },
                // n = len(xs)
                Stmt::Let {
                    name: "n".into(),
                    ty: Type::I64,
                    value: Expr::Len(Box::new(Expr::Ident("xs".into()))),
                    mutable: false,
                },
                // while i < n: { total += xs[i]*w; w += 1.0; i += 1 }
                Stmt::While {
                    cond: Expr::BinOp {
                        op: BinOp::Lt,
                        lhs: Box::new(Expr::Ident("i".into())),
                        rhs: Box::new(Expr::Ident("n".into())),
                    },
                    body: vec![acc_step, weight_step, counter_step],
                },
            ],
            // return total
            trailing_return: Expr::Ident("total".into()),
        },
    };

    Module {
        name: "weighted_sum_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// Encode `vals` as a WAT `(data …)` string-literal of little-endian f64
/// bytes (the layout `f64.load` reads).
fn f64_data_escape(vals: &[f64]) -> String {
    let mut s = String::new();
    for v in vals {
        for b in v.to_le_bytes() {
            s.push_str(&format!("\\{b:02x}"));
        }
    }
    s
}

/// Encode an `i32` as a little-endian WAT `(data …)` string-literal (the
/// PMAT-968 element-count header at base+0).
fn i32_data_escape(v: i32) -> String {
    let mut s = String::new();
    for b in v.to_le_bytes() {
        s.push_str(&format!("\\{b:02x}"));
    }
    s
}

/// Splice the length-prefixed fixture `(data …)` region + a zero-arg `run`
/// export (calling `$weighted_sum` with base-pointer 0) onto the emitted
/// module text, just before its closing `)`. Lets
/// `wasm-interp --run-all-exports` drive the list-taking kernel.
fn build_witness_wat(kernel_wat: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-981 witness: preload the length-prefixed fixture list\n");
    // i32 element-count header at base+0.
    wat.push_str(&format!(
        "  (data (i32.const 0) \"{}\")\n",
        i32_data_escape(LIST_FIXTURE.len() as i32)
    ));
    // f64 elements at base+8 (LIST_ELEMS_OFFSET).
    wat.push_str(&format!(
        "  (data (i32.const 8) \"{}\")\n",
        f64_data_escape(LIST_FIXTURE)
    ));
    // Zero-arg driver: weighted_sum(base-pointer 0).
    wat.push_str(
        "  (func (export \"run\") (result f64)\n    i32.const 0\n    call $weighted_sum)\n",
    );
    wat.push_str(")\n");
    wat
}

#[test]
fn real_weighted_sum_program_executes_in_wasm_and_matches_cpython() {
    // CONSTRUCT-COMPOSITION assertion holds with or without WABT: the real
    // program must LOWER through the production emitter, exercising the
    // list-param + len + index + while + mixed arithmetic combo in one
    // function. (A regression that breaks the composition fails here even on
    // free CI.)
    let kernel_wat = emit_module(&weighted_sum_module())
        .expect("the real weighted-sum program must lower through emit_module");
    assert!(
        kernel_wat.contains("(param $xs i32)"),
        "list[float] param → i32 base-pointer:\n{kernel_wat}"
    );
    assert!(
        kernel_wat.contains("f64.load"),
        "xs[i] read → f64.load:\n{kernel_wat}"
    );
    assert!(
        kernel_wat.contains("i32.load") && kernel_wat.contains("i64.extend_i32_u"),
        "len(xs) → header i32.load + i64 extend:\n{kernel_wat}"
    );
    assert!(
        kernel_wat.contains("(loop $cont") && kernel_wat.contains("i64.lt_s"),
        "while i < n → loop + i64 comparison:\n{kernel_wat}"
    );
    assert!(
        kernel_wat.contains("f64.mul") && kernel_wat.contains("f64.add"),
        "xs[i]*w + total → f64 arithmetic:\n{kernel_wat}"
    );
    // PMAT-1402: this asserted `i64.add` until `+` started routing through
    // `$__wasm_add_i64`. Left alone it would still have PASSED — the helper's
    // own body contains `i64.add` — so it would have gone on reporting
    // "i = i + 1 lowered" while matching bytes the kernel never emitted.
    // Assert the CALL SITE, which only the kernel can produce.
    assert!(
        kernel_wat.contains("call $__wasm_add_i64"),
        "i = i + 1 → checked i64 arithmetic:\n{kernel_wat}"
    );
    assert!(
        kernel_wat.contains("unreachable"),
        "PMAT-968 bounds guard on the indexing:\n{kernel_wat}"
    );

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-981: skipping EXECUTED real-program witness — WABT \
             (wat2wasm / wasm-interp) absent. The composed program lowered \
             through emit_module (asserted above); a box with WABT also runs \
             it and asserts the WASM result == CPython {CPYTHON_RESULT}. Free \
             CI skips the execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-981: running EXECUTED real-program (weighted_sum) witness via WABT");

    let wat = build_witness_wat(&kernel_wat);

    let dir = std::env::temp_dir().join(format!("xpile-wasm-real-program-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("weighted_sum.wat");
    let wasm_path = dir.join("weighted_sum.wasm");
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

    // `run() => f64:<value>` — the single executed result of the program.
    let line = stdout
        .lines()
        .find(|l| l.contains("=> f64:"))
        .unwrap_or_else(|| panic!("no f64 export in interp output:\n{stdout}"));
    let idx = line.find("=> f64:").unwrap();
    let got: f64 = line[idx + "=> f64:".len()..]
        .trim()
        .parse()
        .expect("parse f64 program result");

    assert_eq!(
        got, CPYTHON_RESULT,
        "executed WASM weighted_sum={got} but CPython weighted_sum={CPYTHON_RESULT}\nWAT:\n{wat}"
    );

    eprintln!(
        "PMAT-981: EXECUTED real-program witness PASSED — the composed \
         weighted_sum program (list[float] param + len + xs[i] + while + \
         mixed f64/i64 arithmetic + scalar return) executed in WABT to \
         {got}, bit-matching the CPython result {CPYTHON_RESULT}. The \
         PMAT-951/966/968/978 constructs COMPOSE on a real program, not just \
         single-construct witnesses."
    );
}
