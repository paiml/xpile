//! PMAT-952 (runtime-witness half) — executed WASM-runtime DiffExec
//! witness for the native-WASM §29 lane (`C-COMPILE-RUST-TO-WASM`).
//!
//! Sibling of `crates/xpile-ptx-codegen/tests/gpu_witness.rs` (PMAT-949,
//! the CUDA witness) and `crates/xpile-wgsl-codegen/tests/gpu_witness.rs`
//! (PMAT-950, the cross-vendor wgpu/WGSL witness). This one runs the same
//! `out[i] = 2*in[i] + 1` semantics through **WABT** (`wat2wasm` assembles
//! each module; `wasm-interp --run-all-exports` executes every exported
//! function) — the runtime-stratum upgrade of the EMIT-only PMAT-951
//! slice, with NO new Cargo dependency.
//!
//! Graceful-skip posture (mirrors the cc/python3 / nvcc / wgpu differential
//! gates): when WABT is absent (free CI runners have no `wat2wasm` /
//! `wasm-interp`), the engine is never installed, the backend records the
//! benign `NotRun { no-engine }`, and the test asserts that well-behaved
//! fallback and exits OK. On a box with WABT the engine RUNS BOTH emitters'
//! WAT in the wasm runtime and asserts the executed outputs agree → a real
//! `DiffExecResult::Match`.

use std::process::Command;

use xpile_backend::{
    Artifact, Backend, BackendConfig, DiffExecResult, Profile, QuorumStatus, Target,
};
use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available, WasmBackend};

fn kernel_module() -> Module {
    Module {
        name: "saxpy_kernel".into(),
        source_lang: SourceLang::Rust,
        items: Vec::new(),
        ffi_boundaries: Vec::new(),
    }
}

fn wasm_config() -> BackendConfig {
    BackendConfig {
        emit_contracts: true,
        target: Target::Wasm,
        profile: Profile::RustOut,
        hardware: None,
    }
}

#[test]
fn wasm_diffexec_executes_in_runtime_and_matches() {
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-952: skipping executed WASM witness — WABT (wat2wasm / \
             wasm-interp) absent. A box with WABT runs this and produces a \
             real DiffExecResult::Match; free CI records the benign \
             NotRun {{ no-engine }} and stays green."
        );

        // Even with no runtime the backend must stay well-behaved: both
        // real emitters fire and the quorum records NotRun (NOT a crash,
        // NOT a fake Match). This keeps the path under test in CI.
        let backend = WasmBackend::new_wasm_diffexec_witness();
        let artifact: Artifact = backend
            .lower(&kernel_module(), &wasm_config())
            .expect("witness backend lowers");
        match artifact.quorum_status {
            QuorumStatus::Multi {
                emitters,
                diff_exec: Some(DiffExecResult::NotRun { .. }),
            } => {
                assert_eq!(emitters.len(), 2, "both real emitters should fire");
            }
            other => panic!("expected Multi NotRun (no runtime), got {other:?}"),
        }
        return;
    }

    eprintln!("PMAT-952: running executed WASM-runtime witness via WABT");

    let backend = WasmBackend::new_wasm_diffexec_witness();
    let artifact: Artifact = backend
        .lower(&kernel_module(), &wasm_config())
        .expect("witness backend lowers + runs in wasm runtime");

    // The primary emission carries a real WAT module + the contract.
    // PMAT-976: the general side now drives xpile's REAL `emit_module`, which
    // emits a named `(func $eN …)` followed by a separate
    // `(export "eN" (func $eN))` (NOT the inline `(func (export …))` form the
    // old hand-written emitter used). Assert that REAL-emitter shape so a
    // regression back to hand-written WAT would fail here.
    assert!(
        artifact.primary.contains("(module")
            && artifact.primary.contains("(export \"e0\" (func $e0))"),
        "primary should be the REAL emit_module WAT (named func + separate export), got:\n{}",
        artifact.primary
    );
    assert!(
        artifact
            .primary
            .contains(";; xpile-wasm-codegen — native WAT (scalar/control subset)"),
        "primary must carry the real emitter's module banner, got:\n{}",
        artifact.primary
    );
    assert!(
        artifact
            .citations
            .iter()
            .any(|c| c.as_str() == "C-COMPILE-RUST-TO-WASM"),
        "emission must cite C-COMPILE-RUST-TO-WASM"
    );

    match artifact.quorum_status {
        QuorumStatus::Multi {
            emitters,
            diff_exec: Some(DiffExecResult::Match { max_abs_diff }),
        } => {
            assert_eq!(emitters.len(), 2, "general + specialist both ran");
            assert!(
                emitters.iter().any(|e| e == "wasm-saxpy-general"),
                "general emitter must be reported, got {emitters:?}"
            );
            assert!(
                emitters
                    .iter()
                    .any(|e| e == "wasm-saxpy-specialist-doubling"),
                "specialist emitter must be reported, got {emitters:?}"
            );
            // `out = 2*x + 1` is exactly representable for the fixture
            // inputs; the explicit `x*2+1` and the reassociated `(x+x)+1`
            // agree bit-for-bit in IEEE-754 f64 here.
            assert!(
                max_abs_diff <= 1.0e-9,
                "executed WASM outputs diverged: max_abs_diff={max_abs_diff}"
            );
            eprintln!(
                "PMAT-952: EXECUTED WASM-runtime witness PASSED — general \
                 (x*2+1) vs specialist ((x+x)+1) agree (max_abs_diff={max_abs_diff}). \
                 This is the real Run≥1 DiffExecResult::Match upgrading \
                 C-COMPILE-RUST-TO-WASM to the runtime stratum."
            );
        }
        QuorumStatus::Multi {
            diff_exec: Some(DiffExecResult::Divergent { max_abs_diff, .. }),
            ..
        } => panic!("WASM emitters DIVERGED (contract falsified): max_abs_diff={max_abs_diff}"),
        other => panic!("expected an executed Multi Match with WABT present, got {other:?}"),
    }
}

// ─── PMAT-966: executed witness for the FIRST aggregate ─────────────────
//
// The scalar/control witness above proves a saxpy kernel runs in WABT. This
// one proves the new `list[float]` indexing path: the emitter lowers
// `xs[i]` over a `list[float]` PARAM to `base + i*8` + `f64.load` into
// linear memory; the witness pre-populates that memory with a known fixture
// vector and asserts each `xs[i]` read back from the executed WASM equals
// the CPython-equivalent `fixture[i]`.

/// The fixture list `[10.5, -3.25, 0.0, 42.0, 7.125, -100.0]` — the
/// elements pre-loaded into linear memory and read back by index. Values
/// chosen exactly representable in f64 so the executed read is bit-exact.
const LIST_FIXTURE: &[f64] = &[10.5, -3.25, 0.0, 42.0, 7.125, -100.0];

/// `def get_f(xs: list[float], i: int) -> float: return xs[i]`
fn list_index_kernel_module() -> Module {
    let f = Function {
        name: "get_f".into(),
        params: vec![
            Param {
                name: "xs".into(),
                ty: Type::List(Box::new(Type::F64)),
                mutable: false,
            },
            Param {
                name: "i".into(),
                ty: Type::I64,
                mutable: false,
            },
        ],
        return_type: Type::F64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Index {
                collection: Box::new(Expr::Ident("xs".into())),
                index: Box::new(Expr::Ident("i".into())),
            },
        },
    };
    Module {
        name: "list_index_kernel".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// Encode `vals` as a WAT `(data …)` string-literal of little-endian f64
/// bytes (the WASM memory layout `f64.load` reads). Each byte is escaped
/// as `\HH`.
fn f64_data_escape(vals: &[f64]) -> String {
    let mut s = String::new();
    for v in vals {
        for b in v.to_le_bytes() {
            s.push_str(&format!("\\{b:02x}"));
        }
    }
    s
}

/// Encode an `i32` as a WAT `(data …)` string-literal of little-endian
/// bytes (the WASM `i32.load` reads this header at `base+0`).
fn i32_data_escape(v: i32) -> String {
    let mut s = String::new();
    for b in v.to_le_bytes() {
        s.push_str(&format!("\\{b:02x}"));
    }
    s
}

/// Splice a length-prefixed `(data …)` region pre-loading `LIST_FIXTURE`
/// (PMAT-968 layout: an `i32` element-count header at base+0, then the
/// packed f64 elements from base+8) plus one zero-arg `eK` wrapper export
/// per fixture index — each calling the emitted `$get_f` kernel with
/// base-pointer 0 and index K — into the emitter's real module text, right
/// before its closing `)`. This lets `wasm-interp --run-all-exports` (which
/// calls only zero-arg exports) drive the parametric kernel over the whole
/// fixture.
fn build_list_witness_wat(kernel_wat: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-968 witness: preload the length-prefixed fixture list\n");
    // i32 element-count header at base+0 (PMAT-968).
    wat.push_str(&format!(
        "  (data (i32.const 0) \"{}\")\n",
        i32_data_escape(LIST_FIXTURE.len() as i32)
    ));
    // f64 elements at base+8 (PMAT-968 LIST_ELEMS_OFFSET).
    wat.push_str(&format!(
        "  (data (i32.const 8) \"{}\")\n",
        f64_data_escape(LIST_FIXTURE)
    ));
    for k in 0..LIST_FIXTURE.len() {
        wat.push_str(&format!(
            "  (func (export \"e{k}\") (result f64)\n    \
             i32.const 0\n    i64.const {k}\n    call $get_f)\n"
        ));
    }
    wat.push_str(")\n");
    wat
}

#[test]
fn wasm_list_index_executes_and_matches_cpython() {
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-966: skipping executed list-index witness — WABT \
             (wat2wasm / wasm-interp) absent. A box with WABT runs this and \
             asserts each xs[i] read back from executed WASM == fixture[i]; \
             free CI skips and stays green."
        );
        // Even with no runtime, the EMIT path must still produce a real
        // module with the memory + load shape, and the PMAT-968 bounds
        // guard (keeps the emitter under test in CI without WABT).
        let wat = emit_module(&list_index_kernel_module()).expect("emit list kernel");
        assert!(wat.contains("(memory (export \"mem\") 1)"));
        assert!(wat.contains("f64.load"));
        assert!(wat.contains("unreachable"), "PMAT-968 bounds trap: {wat}");
        return;
    }

    eprintln!("PMAT-966: running executed list-index witness via WABT");

    let kernel_wat = emit_module(&list_index_kernel_module()).expect("emit list kernel");
    // Sanity on the emitted shape before we assemble.
    assert!(
        kernel_wat.contains("(param $xs i32)") && kernel_wat.contains("f64.load"),
        "list param → i32 base + f64.load:\n{kernel_wat}"
    );
    let wat = build_list_witness_wat(&kernel_wat);

    // Assemble + run via WABT, parse the f64 vector (one per eK export).
    let dir = std::env::temp_dir().join(format!("xpile-wasm-list-witness-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("list_index.wat");
    let wasm_path = dir.join("list_index.wasm");
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

    // Each `eK() => f64:<value>` line is xs[K]; collect in index order.
    let mut got: Vec<f64> = Vec::new();
    for line in stdout.lines() {
        if let Some(idx) = line.find("=> f64:") {
            let tok = line[idx + "=> f64:".len()..].trim();
            got.push(tok.parse::<f64>().expect("parse f64 from interp output"));
        }
    }
    assert_eq!(
        got.len(),
        LIST_FIXTURE.len(),
        "one executed read per fixture index; interp output:\n{stdout}"
    );
    // Executed `xs[i]` must equal CPython `fixture[i]` bit-for-bit (the
    // fixture values are exactly representable in f64).
    for (k, (g, &want)) in got.iter().zip(LIST_FIXTURE.iter()).enumerate() {
        assert_eq!(
            *g, want,
            "executed xs[{k}]={g} but CPython fixture[{k}]={want}\nWAT:\n{wat}"
        );
    }

    eprintln!(
        "PMAT-966: EXECUTED list-index witness PASSED — xs[i] over a \
         list[float] param read back {got:?} from WASM linear memory, \
         bit-matching the CPython fixture {LIST_FIXTURE:?}. First aggregate \
         (read-only list[scalar] indexing) executes correctly."
    );
}

// ─── PMAT-968: executed witness for variable-index BOUNDS-CHECKING ───────
//
// PMAT-966 let an out-of-range index silently mis-read (or only trap on an
// unmapped page). PMAT-968 stores the element count in an i32 header at
// base+0 and the `Index` lowering emits `i < 0 || i >= len → unreachable`.
// This witness proves the trap fires: it builds a module that wraps the
// emitted `$get_f` kernel with one IN-BOUNDS export per fixture index plus
// one explicit OUT-OF-BOUNDS export (index == len, the smallest OOB), runs
// it in WABT, and asserts the in-bounds reads bit-match CPython
// `fixture[i]` while the OOB export traps (`error: unreachable executed`) —
// the WASM IndexError analogue, never a silent mis-read.

/// Like [`build_list_witness_wat`] but also adds an `oob` export that calls
/// `$get_f` with `index == len(fixture)` — the first out-of-range index,
/// which the PMAT-968 bounds guard must trap on.
fn build_list_bounds_witness_wat(kernel_wat: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let oob = LIST_FIXTURE.len();
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-968 bounds witness: length-prefixed fixture + OOB probe\n");
    wat.push_str(&format!(
        "  (data (i32.const 0) \"{}\")\n",
        i32_data_escape(LIST_FIXTURE.len() as i32)
    ));
    wat.push_str(&format!(
        "  (data (i32.const 8) \"{}\")\n",
        f64_data_escape(LIST_FIXTURE)
    ));
    for k in 0..LIST_FIXTURE.len() {
        wat.push_str(&format!(
            "  (func (export \"e{k}\") (result f64)\n    \
             i32.const 0\n    i64.const {k}\n    call $get_f)\n"
        ));
    }
    // The out-of-bounds probe: index == len, must trap.
    wat.push_str(&format!(
        "  (func (export \"oob\") (result f64)\n    \
         i32.const 0\n    i64.const {oob}\n    call $get_f)\n"
    ));
    wat.push_str(")\n");
    wat
}

#[test]
fn wasm_list_index_bounds_check_traps_on_oob() {
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-968: skipping executed bounds-check witness — WABT absent. \
             EMIT path is still asserted below; free CI stays green."
        );
        // EMIT path: the bounds guard + header load must be present.
        let wat = emit_module(&list_index_kernel_module()).expect("emit list kernel");
        assert!(wat.contains("unreachable"), "bounds trap emitted: {wat}");
        assert!(
            wat.contains("i64.extend_i32_u"),
            "header length extended for the compare: {wat}"
        );
        assert!(wat.contains("i32.const 8"), "elements offset by 8: {wat}");
        return;
    }

    eprintln!("PMAT-968: running executed bounds-check witness via WABT");

    let kernel_wat = emit_module(&list_index_kernel_module()).expect("emit list kernel");
    assert!(
        kernel_wat.contains("unreachable") && kernel_wat.contains("f64.load"),
        "bounds-checked f64 index:\n{kernel_wat}"
    );
    let wat = build_list_bounds_witness_wat(&kernel_wat);

    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-bounds-witness-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("bounds.wat");
    let wasm_path = dir.join("bounds.wasm");
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

    // In-bounds reads: each `eK() => f64:<value>` must bit-match fixture[K].
    let mut got: Vec<f64> = Vec::new();
    for line in stdout.lines() {
        if line.trim_start().starts_with('e') && line.contains("=> f64:") {
            let idx = line.find("=> f64:").unwrap();
            let tok = line[idx + "=> f64:".len()..].trim();
            got.push(tok.parse::<f64>().expect("parse f64"));
        }
    }
    assert_eq!(
        got.len(),
        LIST_FIXTURE.len(),
        "one in-bounds read per fixture index; output:\n{stdout}"
    );
    for (k, (g, &want)) in got.iter().zip(LIST_FIXTURE.iter()).enumerate() {
        assert_eq!(*g, want, "in-bounds xs[{k}]={g} != fixture[{k}]={want}");
    }
    // The OOB probe (index == len) must TRAP — wasm-interp prints
    // `oob() => error: unreachable executed`. This is the Python IndexError
    // analogue: a deterministic trap, never a silent mis-read.
    assert!(
        stdout.contains("oob() => error: unreachable executed"),
        "OOB index must trap via the PMAT-968 bounds guard; interp output:\n{stdout}"
    );

    eprintln!(
        "PMAT-968: EXECUTED bounds-check witness PASSED — in-bounds xs[i] \
         read {got:?} (bit-matching CPython {LIST_FIXTURE:?}) AND the \
         out-of-range index trapped (`unreachable`), the WASM IndexError \
         analogue."
    );
}

// ─── PMAT-968: executed witness for `len(xs)` over a list param ──────────

/// `def length(xs: list[float]) -> int: return len(xs)`
fn list_len_kernel_module() -> Module {
    let f = Function {
        name: "length".into(),
        params: vec![Param {
            name: "xs".into(),
            ty: Type::List(Box::new(Type::F64)),
            mutable: false,
        }],
        return_type: Type::I64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Len(Box::new(Expr::Ident("xs".into()))),
        },
    };
    Module {
        name: "list_len_kernel".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

#[test]
fn wasm_list_len_executes_and_matches_cpython() {
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-968: skipping executed len() witness — WABT absent. EMIT \
             path asserted below; free CI stays green."
        );
        let wat = emit_module(&list_len_kernel_module()).expect("emit len kernel");
        assert!(wat.contains("i32.load"), "header length load: {wat}");
        assert!(
            wat.contains("i64.extend_i32_u"),
            "len extended to i64: {wat}"
        );
        return;
    }

    eprintln!("PMAT-968: running executed len() witness via WABT");

    let kernel_wat = emit_module(&list_len_kernel_module()).expect("emit len kernel");
    assert!(
        kernel_wat.contains("i32.load") && kernel_wat.contains("i64.extend_i32_u"),
        "len → header load + extend:\n{kernel_wat}"
    );
    // Splice the length-prefixed fixture + one zero-arg `len_e` i64 export.
    let close = kernel_wat.rfind(')').expect("closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str(&format!(
        "  (data (i32.const 0) \"{}\")\n",
        i32_data_escape(LIST_FIXTURE.len() as i32)
    ));
    wat.push_str(&format!(
        "  (data (i32.const 8) \"{}\")\n",
        f64_data_escape(LIST_FIXTURE)
    ));
    wat.push_str("  (func (export \"len_e\") (result i64)\n    i32.const 0\n    call $length)\n");
    wat.push_str(")\n");

    let dir = std::env::temp_dir().join(format!("xpile-wasm-len-witness-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("len.wat");
    let wasm_path = dir.join("len.wasm");
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
        "wasm-interp failed: stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );

    // `len_e() => i64:<n>` must equal CPython `len(fixture)`.
    let want = LIST_FIXTURE.len() as i64;
    let line = stdout
        .lines()
        .find(|l| l.contains("=> i64:"))
        .unwrap_or_else(|| panic!("no i64 export in interp output:\n{stdout}"));
    let idx = line.find("=> i64:").unwrap();
    let got: i64 = line[idx + "=> i64:".len()..]
        .trim()
        .parse()
        .expect("parse i64 len");
    assert_eq!(
        got, want,
        "executed len(xs)={got} but CPython len(fixture)={want}\nWAT:\n{wat}"
    );

    eprintln!(
        "PMAT-968: EXECUTED len() witness PASSED — len(xs) over a list[float] \
         param read {got} from the i32 header, matching CPython len={want}."
    );
}
