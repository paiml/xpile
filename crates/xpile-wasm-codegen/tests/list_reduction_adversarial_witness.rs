//! PMAT-1254 — ADVERSARIAL edge-case EXECUTED witness for the native-WASM
//! list-REDUCTION family (`sum` PMAT-1248/1249, `min`/`max` PMAT-1250,
//! `sorted` PMAT-1252). This is the discipline adversarial-verify pass over
//! the six list-reduction slices shipped since the last list-family fuzz:
//! the per-slice witnesses in `src/tests.rs` each drive ONE happy-path input
//! (a distinct-element non-empty list), so the correctness-critical EDGE
//! paths — the empty-list `unreachable` TRAP, the empty-vs-identity contrast,
//! float-sum fold ORDER, and sort stability over DUPLICATES — have never been
//! executed end-to-end. An emit-shape assertion (`wat.contains("unreachable")`)
//! cannot prove the trap actually FIRES, nor that the fold direction matches
//! CPython; only running the REAL-emitted module can. This file does exactly
//! that, pinning every result against CPython (values verified via python3):
//!
//!   * `min([])` / `max([])` over `list[int]` AND `list[float]` TRAP
//!     (`error: unreachable executed`) — Python raises `ValueError`, and the
//!     helper's `i32.eqz → unreachable` guard is the faithful WASM analogue.
//!     A regression dropping that guard would read `xs[0]` off an empty region
//!     and return garbage SILENTLY; this witness makes that a hard failure.
//!   * `sum([]) == 0` (int) and `== 0.0` (float) — the DIVERGENT contrast:
//!     a fold-from-identity reduction returns the identity on empty, it does
//!     NOT trap. The two empty-list behaviours are witnessed side by side.
//!   * `sum([1e16, -1e16, 1.0]) == 1.0` — a LEFT fold (CPython's `sum`):
//!     `((0 + 1e16) - 1e16) + 1 == 1.0`. A right fold (or any reordering)
//!     would yield `0.0` — so this single input pins the fold DIRECTION.
//!   * `sorted([3,-1,3,-1,2,2])` read at ALL six positions == `[-1,-1,2,2,3,3]`
//!     (asc) and `[3,3,2,2,-1,-1]` (desc) — DUPLICATES + negatives, which the
//!     distinct-element happy-path witness cannot exercise; a sort bug that
//!     drops or duplicates an element on a tie would slip through it.
//!
//! Gated on `wasm_runtime_available()` — a clean skip on a host without WABT
//! (`wat2wasm` / `wasm-interp`), same as every other executed witness.
//!
//! Contracts: C-COMPILE-RUST-TO-WASM (the emit lane under test) +
//! C-WASM-HEAP (the sorted probe allocates a fresh record).

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- meta-HIR builders ------------------------------------------------------

fn param(name: &str, ty: Type) -> Param {
    Param {
        name: name.into(),
        ty,
        mutable: false,
    }
}

fn list_ty(elem: Type) -> Type {
    Type::List(Box::new(elem))
}

fn func(name: &str, ret: Type, params: Vec<Param>, stmts: Vec<Stmt>, tail: Expr) -> Item {
    Item::Function(Function {
        name: name.into(),
        params,
        return_type: ret,
        body: Block {
            stmts,
            trailing_return: tail,
        },
    })
}

/// `def name(xs: list[<elem>]) -> <elem>: return {min|max}(xs)`.
fn minmax_fn(name: &str, elem: Type, is_max: bool, of_float: bool) -> Item {
    func(
        name,
        elem.clone(),
        vec![param("xs", list_ty(elem))],
        vec![],
        Expr::ListMinMax {
            list: Box::new(Expr::Ident("xs".into())),
            is_max,
            of_float,
            of_struct_cmp: false,
            key: None,
            default: None,
        },
    )
}

/// `def name(xs: list[<elem>]) -> <elem>: return sum(xs)`.
fn sum_fn(name: &str, elem: Type, of_float: bool) -> Item {
    func(
        name,
        elem.clone(),
        vec![param("xs", list_ty(elem))],
        vec![],
        Expr::Sum {
            list: Box::new(Expr::Ident("xs".into())),
            of_float,
            start: None,
        },
    )
}

/// `def name(xs: list[int]) -> int: ys = sorted(xs, reverse=<reverse>); return ys[k]`.
fn sorted_at_fn(name: &str, reverse: bool, k: i64) -> Item {
    func(
        name,
        Type::I64,
        vec![param("xs", list_ty(Type::I64))],
        vec![Stmt::Let {
            name: "ys".into(),
            ty: list_ty(Type::I64),
            mutable: false,
            value: Expr::Sorted {
                list: Box::new(Expr::Ident("xs".into())),
                reverse,
                key: None,
                of_float: false,
            },
        }],
        Expr::Index {
            collection: Box::new(Expr::Ident("ys".into())),
            index: Box::new(Expr::LitInt(k)),
        },
    )
}

fn module(name: &str, items: Vec<Item>) -> Module {
    Module {
        name: name.into(),
        source_lang: SourceLang::Rust,
        items,
        ffi_boundaries: Vec::new(),
    }
}

// ---- data-image + WABT harness ----------------------------------------------

const MEM_LINE: &str = "  (memory (export \"mem\") 1)\n";

/// Lay a length-prefixed `list[int]`/`list[float]` record (i32 count @ base+0,
/// 8-byte elements @ base+8) into `image` at `base`. Both element kinds share
/// the 8-byte stride; a float element is written as its raw IEEE-754 bits.
fn write_list_i64(image: &mut [u8], base: usize, elems: &[i64]) {
    image[base..base + 4].copy_from_slice(&(elems.len() as i32).to_le_bytes());
    for (i, &e) in elems.iter().enumerate() {
        let off = base + 8 + i * 8;
        image[off..off + 8].copy_from_slice(&e.to_le_bytes());
    }
}

fn write_list_f64(image: &mut [u8], base: usize, elems: &[f64]) {
    image[base..base + 4].copy_from_slice(&(elems.len() as i32).to_le_bytes());
    for (i, &e) in elems.iter().enumerate() {
        let off = base + 8 + i * 8;
        image[off..off + 8].copy_from_slice(&e.to_le_bytes());
    }
}

/// WAT `(data)` string escape — each byte as `\HH`.
fn wat_data_escape(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 4);
    for &b in bytes {
        s.push('\\');
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Splice a `(data)` segment + driver exports into the REAL-emitted module,
/// right after its `(memory …)` declaration.
fn inject(module_wat: &str, data_image: &[u8], drivers: &str) -> String {
    assert!(
        module_wat.contains(MEM_LINE),
        "emitted module exports memory:\n{module_wat}"
    );
    let data = format!(
        "  (data (i32.const 0) \"{}\")\n{drivers}",
        wat_data_escape(data_image)
    );
    module_wat.replacen(MEM_LINE, &format!("{MEM_LINE}{data}"), 1)
}

/// Assemble + run under WABT. Returns `(stdout, assemble_ok)`; a runtime trap
/// is reported IN stdout as `error: unreachable executed` with a ZERO exit
/// code (verified empirically — `wasm-interp` does not fail the process on a
/// trap), so callers inspect stdout, not the exit status.
fn assemble_and_run(tag: &str, wat: &str) -> (String, bool) {
    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-listadv-{}-{}", tag, std::process::id()));
    std::fs::create_dir_all(&dir).expect("work dir");
    let wat_path = dir.join(format!("{tag}.wat"));
    let wasm_path = dir.join(format!("{tag}.wasm"));
    std::fs::write(&wat_path, wat).expect("write wat");
    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm rejected the REAL-emitted module for {tag}:\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&assemble.stderr)
    );
    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    (
        String::from_utf8_lossy(&run.stdout).into_owned(),
        run.status.success(),
    )
}

/// Parse every `eN() => f64:<value>` line, in export order.
fn parse_f64_exports(stdout: &str) -> Vec<f64> {
    stdout
        .lines()
        .filter_map(|l| l.split_once("=> f64:").map(|(_, v)| v.trim()))
        .map(|tok| {
            tok.parse::<f64>()
                .unwrap_or_else(|e| panic!("parse f64 `{tok}`: {e}"))
        })
        .collect()
}

// ---- EMIT-shape assertions (always run, no WABT) ----------------------------

#[test]
fn minmax_helpers_emit_the_empty_list_trap_guard() {
    // Both the int and float min/max helpers guard the empty list with an
    // `i32.eqz → unreachable` BEFORE loading `xs[0]`. This is the static
    // half; `*_empty_*_traps` below proves the guard actually fires.
    let wat_i = emit_module(&module(
        "mm_i",
        vec![minmax_fn("f", Type::I64, false, false)],
    ))
    .expect("int min lowers");
    assert!(
        wat_i.contains("$__wasm_list_minmax_i64") && wat_i.contains("unreachable"),
        "int min/max helper carries the empty-list trap:\n{wat_i}"
    );
    let wat_f = emit_module(&module(
        "mm_f",
        vec![minmax_fn("f", Type::F64, false, true)],
    ))
    .expect("float min lowers");
    assert!(
        wat_f.contains("$__wasm_list_minmax_f64") && wat_f.contains("unreachable"),
        "float min/max helper carries the empty-list trap:\n{wat_f}"
    );
}

// ---- EXECUTED adversarial witnesses (gated on WABT) -------------------------

/// `min([])` over a `list[int]` TRAPS — CPython raises `ValueError`, the helper
/// executes `unreachable`. (min and max share the one `i32.eqz → unreachable`
/// guard, so witnessing `min` locks the path for both directions.)
#[test]
fn min_empty_int_traps() {
    if !wasm_runtime_available() {
        eprintln!("SKIP min_empty_int_traps: WABT not installed");
        return;
    }
    let module_wat = emit_module(&module(
        "min_e_i",
        vec![minmax_fn("f", Type::I64, false, false)],
    ))
    .unwrap();
    // An EMPTY list at base 0 (count 0), no elements.
    let mut image = vec![0u8; 64];
    write_list_i64(&mut image, 0, &[]);
    let drivers = "  (func (export \"e0\") (result f64)\n    \
         i32.const 0\n    call $f\n    f64.convert_i64_s)\n";
    let wat = inject(&module_wat, &image, drivers);
    let (stdout, _ok) = assemble_and_run("min_empty_int", &wat);
    assert!(
        stdout.contains("unreachable executed"),
        "min([]) over list[int] must TRAP (CPython ValueError parity), got:\n{stdout}\n---WAT---\n{wat}"
    );
    eprintln!("=== PMAT-1254: min([]) list[int] traps (ValueError parity) ===\n{stdout}");
}

/// `min([])` over a `list[float]` TRAPS — the float-helper twin of the guard.
#[test]
fn min_empty_float_traps() {
    if !wasm_runtime_available() {
        eprintln!("SKIP min_empty_float_traps: WABT not installed");
        return;
    }
    let module_wat = emit_module(&module(
        "min_e_f",
        vec![minmax_fn("f", Type::F64, false, true)],
    ))
    .unwrap();
    let mut image = vec![0u8; 64];
    write_list_f64(&mut image, 0, &[]);
    // The float helper already returns f64 — no convert wrapper.
    let drivers = "  (func (export \"e0\") (result f64)\n    \
         i32.const 0\n    call $f)\n";
    let wat = inject(&module_wat, &image, drivers);
    let (stdout, _ok) = assemble_and_run("min_empty_float", &wat);
    assert!(
        stdout.contains("unreachable executed"),
        "min([]) over list[float] must TRAP (CPython ValueError parity), got:\n{stdout}\n---WAT---\n{wat}"
    );
    eprintln!("=== PMAT-1254: min([]) list[float] traps (ValueError parity) ===\n{stdout}");
}

/// The DIVERGENT contrast: `sum([])` does NOT trap — it returns the fold
/// identity (`0` for int, `0.0` for float), exactly like CPython. Witnessed
/// side by side with the min/max trap above: the two empty-list behaviours
/// are genuinely different and both are now pinned.
#[test]
fn sum_empty_is_identity_not_a_trap() {
    if !wasm_runtime_available() {
        eprintln!("SKIP sum_empty_is_identity_not_a_trap: WABT not installed");
        return;
    }
    let module_wat = emit_module(&module(
        "sum_e",
        vec![
            sum_fn("si", Type::I64, false),
            sum_fn("sf", Type::F64, true),
        ],
    ))
    .unwrap();
    // Empty int list at base 0, empty float list at base 128 (both count 0).
    let mut image = vec![0u8; 256];
    write_list_i64(&mut image, 0, &[]);
    write_list_f64(&mut image, 128, &[]);
    let drivers = "  (func (export \"e0\") (result f64)\n    \
         i32.const 0\n    call $si\n    f64.convert_i64_s)\n  \
         (func (export \"e1\") (result f64)\n    \
         i32.const 128\n    call $sf)\n";
    let wat = inject(&module_wat, &image, drivers);
    let (stdout, ok) = assemble_and_run("sum_empty", &wat);
    assert!(
        ok && !stdout.contains("unreachable"),
        "sum([]) must NOT trap (fold identity, unlike min/max):\n{stdout}"
    );
    let out = parse_f64_exports(&stdout);
    assert_eq!(out.len(), 2, "two exports (int sum, float sum): {out:?}");
    assert_eq!(out[0], 0.0, "sum([]) over list[int] == 0 (CPython)");
    assert_eq!(out[1], 0.0, "sum([]) over list[float] == 0.0 (CPython)");
    eprintln!("=== PMAT-1254: sum([])==0 / 0.0 — fold identity, NOT a trap ===\n{stdout}");
}

/// `sum([1e16, -1e16, 1.0]) == 1.0` — a LEFT fold, matching CPython's `sum`.
/// `((0 + 1e16) - 1e16) + 1 == 1.0`; a right fold would collapse the `1.0` and
/// yield `0.0`. This single input pins the fold DIRECTION end-to-end.
#[test]
fn float_sum_is_a_left_fold() {
    if !wasm_runtime_available() {
        eprintln!("SKIP float_sum_is_a_left_fold: WABT not installed");
        return;
    }
    let elems = [1e16_f64, -1e16_f64, 1.0_f64];
    // CPython: sum([1e16, -1e16, 1.0]) == 1.0 (verified via python3).
    let expected = 1.0_f64;
    let module_wat = emit_module(&module("sum_fold", vec![sum_fn("sf", Type::F64, true)])).unwrap();
    let mut image = vec![0u8; 128];
    write_list_f64(&mut image, 0, &elems);
    let drivers = "  (func (export \"e0\") (result f64)\n    \
         i32.const 0\n    call $sf)\n";
    let wat = inject(&module_wat, &image, drivers);
    let (stdout, ok) = assemble_and_run("sum_fold", &wat);
    assert!(
        ok && !stdout.contains("unreachable"),
        "sum runs cleanly:\n{stdout}"
    );
    let out = parse_f64_exports(&stdout);
    assert_eq!(out.len(), 1, "one export: {out:?}");
    assert!(
        (out[0] - expected).abs() <= 1.0e-9,
        "sum({elems:?}) executed {}, expected (CPython, left fold) {expected} \
         — a right fold would give 0.0",
        out[0]
    );
    eprintln!("=== PMAT-1254: float sum is a LEFT fold — sum([1e16,-1e16,1.0])==1.0 ===\n{stdout}");
}

/// `sorted([3,-1,3,-1,2,2])` read at ALL six positions == `[-1,-1,2,2,3,3]`
/// (asc) and `[3,3,2,2,-1,-1]` (desc). DUPLICATES + negatives — a sort bug on
/// equal keys (a dropped or doubled element) would produce a wrong full order
/// the distinct-element happy-path witness cannot see.
#[test]
fn sorted_over_duplicates_full_order() {
    if !wasm_runtime_available() {
        eprintln!("SKIP sorted_over_duplicates_full_order: WABT not installed");
        return;
    }
    let elems: [i64; 6] = [3, -1, 3, -1, 2, 2];
    let mut asc = elems.to_vec();
    asc.sort();
    let mut desc = asc.clone();
    desc.reverse();
    // CPython: asc == [-1,-1,2,2,3,3], desc == [3,3,2,2,-1,-1] (verified).
    assert_eq!(asc, vec![-1, -1, 2, 2, 3, 3]);
    assert_eq!(desc, vec![3, 3, 2, 2, -1, -1]);

    // Twelve functions: a0..a5 read sorted(asc)[k], d0..d5 read sorted(desc)[k].
    let mut items = Vec::new();
    for k in 0..6i64 {
        items.push(sorted_at_fn(&format!("a{k}"), false, k));
    }
    for k in 0..6i64 {
        items.push(sorted_at_fn(&format!("d{k}"), true, k));
    }
    let module_wat = emit_module(&module("sorted_dup", items)).unwrap();

    let mut image = vec![0u8; 128];
    write_list_i64(&mut image, 0, &elems);
    // e0..e5 = ascending positions, e6..e11 = descending positions.
    let mut drivers = String::new();
    for k in 0..6 {
        drivers.push_str(&format!(
            "  (func (export \"e{k}\") (result f64)\n    \
             i32.const 0\n    call $a{k}\n    f64.convert_i64_s)\n"
        ));
    }
    for k in 0..6 {
        drivers.push_str(&format!(
            "  (func (export \"e{}\") (result f64)\n    \
             i32.const 0\n    call $d{k}\n    f64.convert_i64_s)\n",
            k + 6
        ));
    }
    let wat = inject(&module_wat, &image, &drivers);
    let (stdout, ok) = assemble_and_run("sorted_dup", &wat);
    assert!(
        ok && !stdout.contains("unreachable"),
        "sorted runs cleanly:\n{stdout}"
    );
    let out = parse_f64_exports(&stdout);
    assert_eq!(out.len(), 12, "twelve exports (6 asc, 6 desc): {out:?}");
    for (k, &e) in asc.iter().enumerate() {
        assert!(
            (out[k] - e as f64).abs() <= 1.0e-9,
            "sorted(asc)[{k}] executed {}, expected (CPython) {e}",
            out[k]
        );
    }
    for (k, &e) in desc.iter().enumerate() {
        assert!(
            (out[6 + k] - e as f64).abs() <= 1.0e-9,
            "sorted(desc)[{k}] executed {}, expected (CPython) {e}",
            out[6 + k]
        );
    }
    eprintln!(
        "=== PMAT-1254: sorted([3,-1,3,-1,2,2]) full order over DUPLICATES ===\n\
         asc={asc:?} desc={desc:?}; all 12 positions match CPython\n{stdout}"
    );
}
