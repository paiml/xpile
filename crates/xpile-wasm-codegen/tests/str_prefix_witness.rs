//! PMAT-1126 — EXECUTED string PREFIX/SUFFIX witness for the native WASM EMIT
//! lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The ordering slice (`str_ordering_witness.rs`) wired `a < b` / `<=` / `>` /
//! `>=` via `$__wasm_str_cmp`; this slice adds the two Python string PREDICATES
//! `s.startswith(p)` / `s.endswith(p)` via `$__wasm_str_startswith` /
//! `$__wasm_str_endswith` — non-allocating BYTE prefix/suffix compares.
//!
//! ## Why a byte compare IS Python's startswith/endswith
//!
//! CPython's `startswith`/`endswith` compare Unicode CODE POINTS. Both operands
//! are valid UTF-8, and UTF-8 is a self-synchronising PREFIX code: `p[0]` is
//! always a LEAD byte (never a `0x80..0xBF` continuation), so a matching byte
//! forces the compare to begin on a CHAR boundary in `s`. Hence a `len(p)`-byte
//! prefix/suffix match is exactly a `p`-code-point match — no char walk, no
//! split multi-byte char, and no false positive from a SHARED continuation byte.
//! The witness proves this on MULTI-BYTE fixtures where a naive byte compare
//! could diverge:
//!   * `"héllo".startswith("hé")` → True — `hé` is 3 bytes (`h`, then `é` =
//!     0xC3 0xA9), a genuine multi-byte prefix.
//!   * `"héllo".startswith("h©")` → False — `©` (0xC2 0xA9) SHARES the trailing
//!     continuation byte 0xA9 with `é` (0xC3 0xA9) but differs in the LEAD byte,
//!     so the compare stops at byte 1 (never a false positive on 0xA9).
//!   * `"héllo".endswith("éllo")` → True — the suffix start offset `6-5 = 1`
//!     lands exactly on `é`'s lead byte (a char boundary), so the byte-suffix
//!     match IS a code-point-suffix match.
//!   * `"café".endswith("©")` → False, `"café".endswith("é")` → True — the
//!     suffix-offset lead byte disambiguates 0xC3 (é) from 0xC2 (©).
//!
//! ## The real program
//!
//! ```python
//! def pred(s: str, p: str) -> bool:
//!     return s.startswith(p)      # (and s.endswith(p))
//! ```
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the helper + call) on a host without WABT. The pinned
//! CPython booleans are cross-checked against Rust's `str::starts_with` /
//! `str::ends_with` (which equal Python's for valid UTF-8).

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, StrMethodOp, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// A single `(s, p)` fixture with its two pinned CPython predicate results.
/// `python3 -c "s,p='{s}','{p}'; print(s.startswith(p), s.endswith(p))"`.
struct Case {
    s: &'static str,
    p: &'static str,
    starts: bool,
    ends: bool,
}

/// The witness fixtures — ASCII (prefix, suffix, equal, longer-than, empty) and
/// MULTI-BYTE (é / © shared-continuation-byte) pairs. Each pinned bool is the
/// CPython ground truth (also asserted == Rust `str` methods in
/// `cpython_prefix_is_pinned`).
const CASES: &[Case] = &[
    // ── ASCII ────────────────────────────────────────────────────────────
    Case {
        s: "hello",
        p: "he",
        starts: true,
        ends: false,
    },
    Case {
        s: "hello",
        p: "hello",
        starts: true,
        ends: true,
    }, // equal
    Case {
        s: "hello",
        p: "hi",
        starts: false,
        ends: false,
    }, // same-len prefix differs
    Case {
        s: "hello",
        p: "xyz",
        starts: false,
        ends: false,
    },
    Case {
        s: "hi",
        p: "hello",
        starts: false,
        ends: false,
    }, // needle longer than haystack
    Case {
        s: "hello",
        p: "",
        starts: true,
        ends: true,
    }, // empty needle
    Case {
        s: "",
        p: "",
        starts: true,
        ends: true,
    },
    Case {
        s: "",
        p: "a",
        starts: false,
        ends: false,
    },
    Case {
        s: "hello",
        p: "lo",
        starts: false,
        ends: true,
    },
    Case {
        s: "hello",
        p: "xo",
        starts: false,
        ends: false,
    }, // same-len suffix differs
    Case {
        s: "hello",
        p: "world",
        starts: false,
        ends: false,
    },
    // ── MULTI-BYTE (é = 0xC3 0xA9, © = 0xC2 0xA9 — a SHARED continuation byte)
    Case {
        s: "héllo",
        p: "hé",
        starts: true,
        ends: false,
    }, // genuine multi-byte prefix
    Case {
        s: "héllo",
        p: "h©",
        starts: false,
        ends: false,
    }, // NOT a false positive on 0xA9
    Case {
        s: "héllo",
        p: "llo",
        starts: false,
        ends: true,
    },
    Case {
        s: "héllo",
        p: "éllo",
        starts: false,
        ends: true,
    }, // suffix offset on a char boundary
    Case {
        s: "café",
        p: "©",
        starts: false,
        ends: false,
    },
    Case {
        s: "café",
        p: "é",
        starts: false,
        ends: true,
    },
];

/// The two predicate ops, each with its meta-HIR `StrMethodOp` and the WAT
/// helper the emitter must produce + call.
const OPS: &[(StrMethodOp, &str)] = &[
    (StrMethodOp::StartsWith, "$__wasm_str_startswith"),
    (StrMethodOp::EndsWith, "$__wasm_str_endswith"),
];

/// Fixed, non-overlapping addresses for the two preloaded str params, below
/// `LITERAL_BASE` (= 512) and the bump heap (>= 1024). Each is a length-prefixed
/// region (i32 BYTE count @ base+0, UTF-8 bytes @ base+8).
const S_ADDR: i32 = 16;
const P_ADDR: i32 = 256;

/// The pinned expected for a given case + op.
fn expected(c: &Case, op: StrMethodOp) -> bool {
    match op {
        StrMethodOp::StartsWith => c.starts,
        StrMethodOp::EndsWith => c.ends,
        _ => unreachable!("only prefix/suffix ops in OPS"),
    }
}

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def pred(s: str, p: str) -> bool: return s.<op>(p)`.
fn pred_module(op: StrMethodOp) -> Module {
    let body = Expr::StrMethod {
        recv: Box::new(Expr::Ident("s".into())),
        op,
        args: vec![Expr::Ident("p".into())],
    };
    let f = Function {
        name: "pred".into(),
        params: vec![
            Param {
                name: "s".into(),
                ty: Type::Str,
                mutable: false,
            },
            Param {
                name: "p".into(),
                ty: Type::Str,
                mutable: false,
            },
        ],
        return_type: Type::Bool,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "pred_program".into(),
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

/// Splice the two str-param `(data …)` regions + a zero-arg `run` export
/// (`$pred(S_ADDR, P_ADDR)`) onto the emitted module, before its closing `)`.
fn build_witness_wat(kernel_wat: &str, s: &str, p: &str) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    wat.push_str("  ;; PMAT-1126 witness: preload the two str params (below LITERAL_BASE)\n");
    for (addr, txt) in [(S_ADDR, s), (P_ADDR, p)] {
        let bytes = txt.as_bytes();
        wat.push_str(&format!(
            "  (data (i32.const {addr}) \"{}\")\n",
            i32_data_escape(bytes.len() as i32)
        ));
        wat.push_str(&format!(
            "  (data (i32.const {}) \"{}\")\n",
            addr + 8,
            bytes_data_escape(bytes)
        ));
    }
    wat.push_str(&format!(
        "  (func (export \"run\") (result i32)\n    \
           i32.const {S_ADDR}\n    i32.const {P_ADDR}\n    call $pred)\n"
    ));
    wat.push_str(")\n");
    wat
}

/// Parse a `run() => i32:<value>` line from `wasm-interp --run-all-exports`.
fn parse_run_i32(stdout: &str) -> i32 {
    let line = stdout
        .lines()
        .find(|l| l.contains("run() => i32:"))
        .unwrap_or_else(|| panic!("no `run` i32 export in interp output:\n{stdout}"));
    let idx = line.find("=> i32:").unwrap();
    line[idx + "=> i32:".len()..]
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("parse i32 from {line:?}"))
}

/// Lower `s.<op>(p)`, run it in WABT with `s`/`p` preloaded, return the bool.
/// `None` when WABT is absent (the caller skips the value assertion).
fn exec_pred(op: StrMethodOp, s: &str, p: &str) -> Option<bool> {
    let kernel_wat = emit_module(&pred_module(op)).expect("pred program lowers");
    if !wasm_runtime_available() {
        return None;
    }
    let wat = build_witness_wat(&kernel_wat, s, p);
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-pred-{}-{op:?}-{}",
        std::process::id(),
        s.len() * 31 + p.len()
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("pred.wat");
    let wasm_path = dir.join("pred.wasm");
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
    Some(parse_run_i32(&stdout) != 0)
}

#[test]
fn cpython_prefix_is_pinned() {
    // Rust `str::starts_with`/`ends_with` operate on the byte sequence of valid
    // UTF-8 == Python's code-point predicate, so they validate every pinned bool.
    for c in CASES {
        assert_eq!(
            c.starts,
            c.s.starts_with(c.p),
            "starts mismatch for {:?}.startswith({:?})",
            c.s,
            c.p
        );
        assert_eq!(
            c.ends,
            c.s.ends_with(c.p),
            "ends mismatch for {:?}.endswith({:?})",
            c.s,
            c.p
        );
    }
    // The multi-byte fixtures MUST genuinely exercise a non-ASCII byte, else the
    // "byte prefix/suffix == code-point prefix/suffix" claim is untested.
    assert!(CASES.iter().any(|c| !c.s.is_ascii() || !c.p.is_ascii()));
    // And a shared-continuation-byte NEGATIVE case must be present (the false-
    // positive guard): "héllo".startswith("h©") shares 0xA9 but is False.
    assert!(CASES
        .iter()
        .any(|c| c.s == "héllo" && c.p == "h©" && !c.starts));
}

#[test]
fn pred_emits_helper_and_call() {
    // CONSTRUCT assertion (holds with or without WABT): each predicate lowers
    // through the production emitter, carrying its helper + call, declaring
    // memory (the compare reads the str bytes), and NEVER pulling in the bump
    // allocator (a bool predicate allocates nothing).
    for (op, helper) in OPS {
        let wat = emit_module(&pred_module(*op))
            .unwrap_or_else(|e| panic!("the s.{op:?}(p) program must lower: {e:?}"));
        assert!(
            wat.contains(&format!(
                "(func {helper} (param $s i32) (param $p i32) (result i32)"
            )),
            "the {helper} helper must be emitted for {op:?}:\n{wat}"
        );
        assert!(
            wat.contains(&format!("call {helper}")),
            "$pred must call {helper} for {op:?}:\n{wat}"
        );
        assert!(
            wat.contains("(memory"),
            "the predicate needs memory declared:\n{wat}"
        );
        assert!(
            !wat.contains("(func $__alloc"),
            "a pure predicate module must NOT carry the bump allocator:\n{wat}"
        );
    }
    // A module using ONLY startswith must NOT carry the endswith helper (each is
    // gated separately — no dead helper).
    let sw_only = emit_module(&pred_module(StrMethodOp::StartsWith)).unwrap();
    assert!(
        sw_only.contains("$__wasm_str_startswith") && !sw_only.contains("$__wasm_str_endswith"),
        "startswith-only module carries no endswith helper:\n{sw_only}"
    );
}

#[test]
fn unwired_str_method_still_refused() {
    // A string method the WASM lane does NOT wire must still be a hard refusal,
    // never wrong WAT — the honesty guard the new ops preserve.
    //
    // In a VALUE (non-str) position the emit_expr `StrMethod { op, .. }`
    // catch-all fires: an `.upper()` in a bool-returning slot (the emitter does
    // not re-type-check the body) hits that arm, which names the op + the
    // supported set. This exercises exactly the new dispatch's fall-through.
    let f = Function {
        name: "bad".into(),
        params: vec![Param {
            name: "s".into(),
            ty: Type::Str,
            mutable: false,
        }],
        return_type: Type::Bool,
        body: Block {
            stmts: vec![],
            trailing_return: Expr::StrMethod {
                recv: Box::new(Expr::Ident("s".into())),
                op: StrMethodOp::Upper,
                args: vec![],
            },
        },
    };
    let m = Module {
        name: "bad_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    };
    let err = emit_module(&m).expect_err(".upper() must be refused on the WASM lane");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("Upper") && msg.contains("startswith"),
        "the refusal must name the op + the supported set: {msg}"
    );
}

#[test]
fn real_prefix_programs_execute_in_wasm_and_match_cpython() {
    // Prove the emit path lowers for both ops (holds without WABT).
    for (op, _) in OPS {
        emit_module(&pred_module(*op)).expect("predicate program lowers");
    }

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1126: skipping EXECUTED string prefix/suffix witness — WABT \
             (wat2wasm / wasm-interp) absent. The pred programs lowered through \
             emit_module (asserted in `pred_emits_helper_and_call`); a box with \
             WABT also runs all {} case×op pairs and asserts each == the pinned \
             CPython bool. Free CI skips execution and stays green.",
            CASES.len() * OPS.len()
        );
        return;
    }

    eprintln!("PMAT-1126: running EXECUTED string prefix/suffix witness via WABT");
    let mut checked = 0usize;
    for c in CASES {
        for (op, _) in OPS {
            let want = expected(c, *op);
            let got = exec_pred(*op, c.s, c.p).expect("WABT present → a value");
            assert_eq!(
                got, want,
                "executed WASM `{:?}.{op:?}({:?})` = {got} but CPython = {want}",
                c.s, c.p
            );
            checked += 1;
        }
    }
    eprintln!(
        "PMAT-1126: EXECUTED string prefix/suffix witness PASSED — {checked} \
         (case × op) pairs lowered through emit_module and executed in WABT, \
         each value-matching CPython, including the MULTI-BYTE fixtures \
         (\"héllo\".startswith(\"hé\")=True, \"héllo\".startswith(\"h©\")=False \
         on a shared 0xA9, \"héllo\".endswith(\"éllo\")=True — byte prefix/suffix \
         == code-point prefix/suffix, proven on silicon, never a split char or \
         false positive)."
    );
}
