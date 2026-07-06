//! PMAT-1327 — EXECUTED STEPPED string-SLICE witness for the native WASM EMIT
//! lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! PMAT-1058 (`str_slice_witness.rs`) shipped the plain half-open `s[lo:hi]`
//! (step 1); this slice adds the GENERAL stepped `s[i:j:k]` with a
//! compile-time-constant, NON-zero step — including the ubiquitous `s[::-1]`
//! reverse idiom (`k == -1`). The stepped form routes through the new
//! `$__wasm_str_slice_step` helper, which reproduces CPython's
//! `PySlice_Unpack` defaults + `PySlice_AdjustIndices` normalisation (per the
//! STEP SIGN) and copies each selected code point char-exactly.
//!
//! ## The real programs
//!
//! ```python
//! def f(s: str) -> str:
//!     return s[i:j:k]        # a stepped, char-exact slice
//! ```
//!
//! for a MATRIX of `(i, j, k)` specs (positive/negative steps, missing bounds,
//! negative-index normalisation, empty selections, out-of-range clamps).
//!
//! ## Char-exactness
//!
//! The fixture `"aβcδe"` carries two 2-byte UTF-8 code points (`β` at char 1,
//! `δ` at char 3). A reverse `s[::-1]` must return `"eδcβa"` (7 bytes:
//! `[101, 206,180, 99, 206,178, 97]`) — a BYTE reversal would split `β`/`δ`
//! into invalid UTF-8. The `$__wasm_str_slice_step` char-walk copies each code
//! point whole.
//!
//! ## Ground truth
//!
//! Every spec's expected string is a LITERAL value pinned from real CPython
//! (`python3 -c "print('aβcδe'[i:j:k])"`). [`pins_reproduce_reference`] proves
//! the in-test [`py_slice`] reference reproduces every CPython pin, so the
//! executed WASM is diffed against a reference that is itself CPython-anchored.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the `$__wasm_str_slice_step` helper + call) on a host
//! without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// The multi-byte str-param fixture: `β` (U+03B2) and `δ` (U+03B4) are each
/// 2-byte UTF-8 code points, so the fixture genuinely exercises CHAR (not byte)
/// stepping. `chars = [a, β, c, δ, e]`, char length 5, byte length 7.
const FIX_S: &str = "aβcδe";

/// Fixed address for the preloaded `s` param, below `LITERAL_BASE` (= 512) and
/// the bump heap (>= 1024). A length-prefixed region (i32 BYTE count @ base+0,
/// UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;

/// One `(lo, hi, k)` spec + its CPython-pinned expected value.
struct Spec {
    lo: Option<i64>,
    hi: Option<i64>,
    k: i64,
    /// `python3 -c "print(repr('aβcδe'[lo:hi:k]))"`.
    cpython: &'static str,
}

/// The matrix — every `cpython` field is a LITERAL value copied from a real
/// `python3` run over `FIX_S` (see the module doc comment).
const SPECS: &[Spec] = &[
    // reverse (the headline idiom).
    Spec {
        lo: None,
        hi: None,
        k: -1,
        cpython: "eδcβa",
    },
    // positive step, missing bounds.
    Spec {
        lo: None,
        hi: None,
        k: 2,
        cpython: "ace",
    },
    // positive step, explicit start.
    Spec {
        lo: Some(1),
        hi: None,
        k: 2,
        cpython: "βδ",
    },
    // step 3 (only two chars land).
    Spec {
        lo: None,
        hi: None,
        k: 3,
        cpython: "aδ",
    },
    // negative step, explicit start > stop.
    Spec {
        lo: Some(4),
        hi: Some(0),
        k: -1,
        cpython: "eδcβ",
    },
    // positive step, both bounds explicit.
    Spec {
        lo: Some(1),
        hi: Some(4),
        k: 2,
        cpython: "βδ",
    },
    // negative-index start with a reverse step.
    Spec {
        lo: Some(-1),
        hi: None,
        k: -1,
        cpython: "eδcβa",
    },
    // positive step, explicit start 0.
    Spec {
        lo: Some(0),
        hi: None,
        k: 2,
        cpython: "ace",
    },
    // negative step, explicit stop 0 (excludes index 0).
    Spec {
        lo: None,
        hi: Some(0),
        k: -2,
        cpython: "ec",
    },
    // EMPTY: step 1, start >= stop.
    Spec {
        lo: Some(3),
        hi: Some(1),
        k: 1,
        cpython: "",
    },
    // EMPTY: negative step, stop > start.
    Spec {
        lo: Some(1),
        hi: Some(3),
        k: -1,
        cpython: "",
    },
    // step 1 explicit, out-of-range hi clamps to len.
    Spec {
        lo: Some(0),
        hi: Some(100),
        k: 1,
        cpython: "aβcδe",
    },
    // both bounds out of range (deep negative + large positive), positive step.
    Spec {
        lo: Some(-100),
        hi: Some(100),
        k: 2,
        cpython: "ace",
    },
    // reverse by 2.
    Spec {
        lo: None,
        hi: None,
        k: -2,
        cpython: "eca",
    },
];

/// A faithful reimplementation of CPython stepped slicing over CODE POINTS —
/// `PySlice_Unpack` defaults (`k > 0` → `0` / `len`; `k < 0` → `len-1` / `-1`)
/// followed by `PySlice_AdjustIndices` (negative bound `+= n`; clamp per step
/// sign). This is the same arithmetic `$__wasm_str_slice_step` performs, so it
/// pins the WASM output to CPython (proven by `pins_reproduce_reference`).
fn py_slice(s: &str, lo: Option<i64>, hi: Option<i64>, k: i64) -> String {
    assert!(k != 0, "zero step is refused, never diffed");
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len() as i64;
    let mut start = lo.unwrap_or(if k < 0 { i64::MAX } else { 0 });
    let mut stop = hi.unwrap_or(if k < 0 { i64::MIN } else { i64::MAX });
    let adjust = |mut idx: i64| -> i64 {
        if idx < 0 {
            idx = idx.wrapping_add(n);
            if idx < 0 {
                idx = if k < 0 { -1 } else { 0 };
            }
        } else if idx >= n {
            idx = if k < 0 { n - 1 } else { n };
        }
        idx
    };
    start = adjust(start);
    stop = adjust(stop);
    let mut out = String::new();
    let mut i = start;
    if k > 0 {
        while i < stop {
            out.push(chars[i as usize]);
            i += k;
        }
    } else {
        while i > stop {
            out.push(chars[i as usize]);
            i += k;
        }
    }
    out
}

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def f(s: str) -> str: return s[lo:hi:k]`.
fn slice_module(lo: Option<i64>, hi: Option<i64>, k: i64) -> Module {
    let body = Expr::Slice {
        collection: Box::new(Expr::Ident("s".into())),
        lo: lo.map(|v| Box::new(Expr::LitInt(v))),
        hi: hi.map(|v| Box::new(Expr::LitInt(v))),
        of_str: true,
        step: Some(k),
    };
    let f = Function {
        name: "f".into(),
        params: vec![Param {
            name: "s".into(),
            ty: Type::Str,
            mutable: false,
        }],
        return_type: Type::Str,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "slice_step_program".into(),
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

/// Splice the `s` param `(data …)` region + `run_len` + per-byte readers onto
/// the emitted module, before its closing `)`. `n_out` = expected result bytes.
fn build_witness_wat(kernel_wat: &str, n_out: usize) -> String {
    let s = FIX_S.as_bytes();
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1327 witness: preload the str param (below LITERAL_BASE)\n");
    wat.push_str(&format!(
        "  (data (i32.const {S_ADDR}) \"{}\")\n",
        i32_data_escape(s.len() as i32)
    ));
    wat.push_str(&format!(
        "  (data (i32.const {}) \"{}\")\n",
        S_ADDR + 8,
        bytes_data_escape(s)
    ));
    wat.push_str(&format!(
        "  (func (export \"run_len\") (result i32)\n    i32.const {S_ADDR}\n    call $f\n    i32.load)\n"
    ));
    // run_byte_i: byte i of the constructed result (each export re-runs $f under
    // a fresh bump heap via --run-all-exports).
    for i in 0..n_out {
        wat.push_str(&format!(
            "  (func (export \"run_byte_{i}\") (result i32)\n    \
               i32.const {S_ADDR}\n    call $f\n    \
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

/// Run one emitted stepped-slice module through WABT and return the constructed
/// result string. `idx` disambiguates the per-spec temp dir (parallel libtest).
fn run_wasm_slice(kernel_wat: &str, n_out: usize, idx: usize) -> String {
    let wat = build_witness_wat(kernel_wat, n_out);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-slice-step-{}-{idx}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("f.wat");
    let wasm_path = dir.join("f.wasm");
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

    let got_len = parse_i32_export(&stdout, "run_len");
    assert_eq!(
        got_len as usize, n_out,
        "constructed result byte length: WASM={got_len} expected={n_out}\nWAT:\n{wat}"
    );
    let mut bytes = Vec::with_capacity(n_out);
    for i in 0..n_out {
        bytes.push(parse_i32_export(&stdout, &format!("run_byte_{i}")) as u8);
    }
    String::from_utf8(bytes).expect("constructed result bytes are valid UTF-8")
}

#[test]
fn pins_reproduce_reference() {
    // The in-test CPython-mirroring reference reproduces every pinned CPython
    // value — so a WASM diff against `py_slice` is a diff against CPython.
    assert!(!FIX_S.is_ascii(), "fixture must be multi-byte");
    assert_eq!(FIX_S.chars().count(), 5);
    assert_eq!(FIX_S.len(), 7, "two 2-byte code points → 7 bytes");
    for sp in SPECS {
        assert_eq!(
            py_slice(FIX_S, sp.lo, sp.hi, sp.k),
            sp.cpython,
            "py_slice reference must match the CPython pin for lo={:?} hi={:?} k={}",
            sp.lo,
            sp.hi,
            sp.k
        );
    }
}

#[test]
fn stepped_slice_emits_step_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): a stepped slice lowers
    // through the production emitter, carrying the STEP helper + call, and NOT
    // the plain-slice helper (a stepped-only module → no dead plain helper).
    let wat = emit_module(&slice_module(None, None, -1))
        .expect("the s[::-1] reverse program must lower through emit_module");
    assert!(
        wat.contains(
            "(func $__wasm_str_slice_step (param $s i32) (param $lo i64) (param $hi i64) (param $k i64)"
        ),
        "the stepped-slice helper must be emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_slice_step"),
        "$f must call the stepped-slice helper:\n{wat}"
    );
    assert!(
        !wat.contains("call $__wasm_str_slice\n") && !wat.contains("(func $__wasm_str_slice "),
        "a stepped-only module must carry NO plain-slice helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $f (param $s i32) (result i32)"),
        "str return → i32 result (heap pointer):\n{wat}"
    );
    assert!(
        wat.contains("(func $__alloc") && wat.contains("(func $__wasm_str_charlen"),
        "stepped slice needs the bump heap + char-walk helpers:\n{wat}"
    );
    // The reverse idiom passes k = -1 with the k<0 default sentinels.
    assert!(
        wat.contains("i64.const -1\n    call $__wasm_str_slice_step"),
        "s[::-1] must pass k = -1 to the stepped helper:\n{wat}"
    );
}

#[test]
fn zero_step_refuses() {
    // `s[::0]` is a Python ValueError — refused honestly at lowering, never a
    // trap or miscompile.
    let err = emit_module(&slice_module(None, None, 0)).expect_err("a zero step must refuse");
    let msg = format!("{err}");
    assert!(
        msg.contains("zero step") && msg.contains("ValueError"),
        "zero-step refusal must name the Python ValueError, got: {msg}"
    );
}

#[test]
fn stepped_slice_executes_in_wasm_and_matches_cpython() {
    // CONSTRUCT half always runs (every spec lowers through the real emitter).
    let kernels: Vec<(String, usize, &Spec)> = SPECS
        .iter()
        .map(|sp| {
            let wat = emit_module(&slice_module(sp.lo, sp.hi, sp.k)).unwrap_or_else(|e| {
                panic!(
                    "spec lo={:?} hi={:?} k={} must lower: {e}",
                    sp.lo, sp.hi, sp.k
                )
            });
            assert_eq!(
                py_slice(FIX_S, sp.lo, sp.hi, sp.k),
                sp.cpython,
                "reference sanity for lo={:?} hi={:?} k={}",
                sp.lo,
                sp.hi,
                sp.k
            );
            (wat, sp.cpython.len(), sp)
        })
        .collect();

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1327: skipping EXECUTED stepped-slice witness — WABT \
             (wat2wasm / wasm-interp) absent. All {} specs lowered through \
             emit_module (asserted above + in `stepped_slice_emits_step_helper_and_call`); \
             a box with WABT also runs each and asserts the CONSTRUCTED result \
             == its CPython pin. Free CI skips execution and stays green.",
            SPECS.len()
        );
        return;
    }

    eprintln!(
        "PMAT-1327: running EXECUTED stepped-slice witness over {} specs via WABT",
        SPECS.len()
    );
    for (idx, (kernel_wat, n_out, sp)) in kernels.iter().enumerate() {
        let got = run_wasm_slice(kernel_wat, *n_out, idx);
        assert_eq!(
            &got, sp.cpython,
            "executed WASM s[{:?}:{:?}:{}] over {FIX_S:?} = {got:?} but CPython = {:?}",
            sp.lo, sp.hi, sp.k, sp.cpython
        );
    }
    eprintln!(
        "PMAT-1327: EXECUTED stepped-slice witness PASSED — all {} `s[i:j:k]` \
         specs (incl. s[::-1] = {:?}) lowered through emit_module and executed \
         in WABT char-exactly to their CPython pins over the MULTI-BYTE fixture \
         {FIX_S:?}.",
        SPECS.len(),
        SPECS[0].cpython,
    );
}
