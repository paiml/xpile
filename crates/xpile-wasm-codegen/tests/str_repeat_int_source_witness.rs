//! PMAT-1149 — EXECUTED `str(int) * k` witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## The latent gate hole this closes
//!
//! `str(n) * k` composes two already-shipped ops: `str(int)` (PMAT-1060,
//! `Expr::ToStr { of_float: false }` → `$__wasm_int_to_str`) as the SEQ of a
//! string repeat (PMAT-1142, `Expr::Repeat { of_str: true }` → the byte-
//! replication `$__wasm_str_repeat`). `emit_repeat` lowers its seq via
//! `emit_str_expr`, whose `ToStr` arm emits `call $__wasm_int_to_str` — so a
//! repeat-hosted `str(n)` DOES call the int→str helper at run time.
//!
//! But the helper is only DECLARED when `module_needs_int_to_str` finds an
//! int→str site, and its walker `expr_has_int_to_str` was the ONE string-helper
//! scanner missing the `Expr::Repeat` arm every sibling carries
//! (`expr_has_str_slice` / `expr_has_str_contains` / `expr_has_str_repeat` /
//! `expr_uses_str_method` all recurse into a repeat's operands). So a module
//! whose SOLE `str(int)` lives inside a repeat (`return str(n) * k`) emitted a
//! `call $__wasm_int_to_str` against a helper that was never declared — a hard
//! `wat2wasm` failure. PMAT-1149 adds the missing arm; this witness pins it on
//! silicon and guards against regression.
//!
//! ## The real program
//!
//! ```python
//! def rep_i(n: int, k: int) -> str:
//!     return str(n) * k
//! ```
//!
//! ## Why the values are Python-exact
//!
//! `str(n)` is decimal ASCII (sign-aware; `$__wasm_int_to_str` works in the
//! unsigned magnitude so `i64::MIN` is exact), and `s * k` is pure byte
//! replication (`max(k, 0)` copies). Both are 1-byte-ASCII for a decimal int, so
//! the concatenation is byte-for-byte CPython `str(n) * k`. A count `k <= 0`
//! clamps to the empty string (`str(9) * -2 == ""`).
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers AND now carries `$__wasm_int_to_str` — the actual regression
//! guard) on a host without WABT. Expected values are validated independently
//! against Rust `format!("{n}").repeat(k)` in `cpython_str_repeat_is_pinned`.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, Param, SourceLang, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// A single `(n, k)` fixture with its pinned CPython `str(n) * k` result.
/// `python3 -c "print(repr(str(n) * k))"`.
struct Case {
    n: i64,
    k: i64,
    expected: &'static str,
}

/// Fixtures: basic, zero-int (`str(0)`), identity count, NEGATIVE int (sign
/// path through `$__wasm_int_to_str`), zero/negative COUNT clamps, and a
/// multi-digit int replicated. Each `expected` is CPython ground truth
/// (asserted == Rust `format!` + `repeat` in `cpython_str_repeat_is_pinned`).
const CASES: &[Case] = &[
    Case {
        n: 5,
        k: 3,
        expected: "555",
    },
    Case {
        n: 0,
        k: 4,
        expected: "0000",
    }, // str(0) → "0"
    Case {
        n: 42,
        k: 1,
        expected: "42",
    }, // identity count
    Case {
        n: 100,
        k: 2,
        expected: "100100",
    }, // multi-digit
    // ── NEGATIVE int: the sign path through $__wasm_int_to_str ────────────
    Case {
        n: -12,
        k: 2,
        expected: "-12-12",
    },
    Case {
        n: -1,
        k: 3,
        expected: "-1-1-1",
    },
    // ── COUNT clamps: Python str(n) * k == "" for k <= 0 ─────────────────
    Case {
        n: 7,
        k: 0,
        expected: "",
    },
    Case {
        n: 9,
        k: -2,
        expected: "",
    },
];

/// Build the meta-HIR `Module` the Python frontend produces for
/// `def rep_i(n: int, k: int) -> str: return str(n) * k` — i.e.
/// `Repeat { seq: ToStr(n), n: k, of_str: true }`. The `str(n)` seq is the SOLE
/// int→str site, so it exercises the PMAT-1149 gate arm.
fn rep_i_module() -> Module {
    let body = Expr::Repeat {
        seq: Box::new(Expr::ToStr {
            value: Box::new(Expr::Ident("n".into())),
            of_float: false,
        }),
        n: Box::new(Expr::Ident("k".into())),
        of_str: true,
    };
    let f = Function {
        name: "rep_i".into(),
        params: vec![
            Param {
                name: "n".into(),
                ty: Type::I64,
                mutable: false,
            },
            Param {
                name: "k".into(),
                ty: Type::I64,
                mutable: false,
            },
        ],
        return_type: Type::Str,
        body: Block {
            stmts: vec![],
            trailing_return: body,
        },
    };
    Module {
        name: "str_repeat_int_source_program".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// Splice a `run_len` + per-byte readers onto the emitted module, before its
/// closing `)`. Each reader re-runs `$rep_i(n, k)` (a fresh bump-heap result per
/// invocation under `--run-all-exports`) — no preloaded data segment is needed
/// (the source string is CONSTRUCTED from the int `n`, not a str param).
fn build_witness_wat(kernel_wat: &str, n: i64, k: i64, n_out: usize) -> String {
    let close = kernel_wat
        .rfind(')')
        .expect("emitted module has a closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel_wat[..close]);
    // run_len: the result's i32 byte count (header at result+0).
    wat.push_str(&format!(
        "  (func (export \"run_len\") (result i32)\n    \
           i64.const {n}\n    i64.const {k}\n    call $rep_i\n    i32.load)\n"
    ));
    // run_byte_i: byte i of the constructed result. Each export re-runs rep_i
    // (fresh bump heap per invocation under --run-all-exports).
    for i in 0..n_out {
        wat.push_str(&format!(
            "  (func (export \"run_byte_{i}\") (result i32)\n    \
               i64.const {n}\n    i64.const {k}\n    call $rep_i\n    \
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

/// Lower `str(n) * k`, run it in WABT with `n`/`k` baked, and read back the
/// CONSTRUCTED result's bytes. Returns `(len, bytes)`.
fn exec_str_repeat(kernel_wat: &str, n: i64, k: i64, n_out: usize) -> (i32, Vec<u8>) {
    let wat = build_witness_wat(kernel_wat, n, k, n_out);
    // A per-case-unique work dir. `n`/`k` may be negative, so format the raw
    // values (never arithmetic that could overflow a usize on a negative value).
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-str-repeat-int-{}-{}-{}-{}",
        std::process::id(),
        n,
        k,
        n_out
    ));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("rep_i.wat");
    let wasm_path = dir.join("rep_i.wasm");
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
    let len = parse_i32_export(&stdout, "run_len");
    let bytes = (0..n_out)
        .map(|i| parse_i32_export(&stdout, &format!("run_byte_{i}")) as u8)
        .collect();
    (len, bytes)
}

#[test]
fn cpython_str_repeat_is_pinned() {
    // Rust `format!("{n}")` is decimal ASCII == CPython `str(n)`, and `.repeat(k)`
    // for k >= 0 is byte replication == Python `s * k`; a negative count clamps to
    // the empty string. Validates every pinned `expected` against the language
    // semantics independently of the emitter.
    for c in CASES {
        let rust = if c.k < 0 {
            String::new()
        } else {
            format!("{}", c.n).repeat(c.k as usize)
        };
        assert_eq!(rust, c.expected, "str({}) * {} mismatch", c.n, c.k);
    }
    // A NEGATIVE-int fixture must exist (else the sign path is untested).
    assert!(CASES.iter().any(|c| c.n < 0 && !c.expected.is_empty()));
    // A NEGATIVE-count clamp fixture must exist (`str(9) * -2 == ""`).
    assert!(CASES.iter().any(|c| c.k < 0 && c.expected.is_empty()));
    // A str(0) fixture must exist (the "at least one digit" path → "0").
    assert!(CASES
        .iter()
        .any(|c| c.n == 0 && c.expected.starts_with('0')));
}

#[test]
fn str_repeat_int_source_emits_int_to_str_helper() {
    // PMAT-1149 REGRESSION GUARD (holds with or without WABT): the `str(n) * k`
    // program lowers AND carries BOTH helpers. Before the fix, `expr_has_int_to_str`
    // missed the `Expr::Repeat` arm, so `$__wasm_int_to_str` was NEVER declared
    // even though `emit_repeat`→`emit_str_expr` emits `call $__wasm_int_to_str` —
    // a hard wat2wasm failure. This assertion FAILS pre-fix.
    let wat = emit_module(&rep_i_module())
        .expect("the `str(n) * k` program must lower through emit_module");
    assert!(
        wat.contains("(func $__wasm_int_to_str (param $n i64) (result i32)"),
        "the int→str helper MUST be declared for `str(n) * k` (the PMAT-1149 gate \
         hole): its call site is emitted by emit_repeat's seq lowering:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_int_to_str"),
        "the repeat's `str(n)` seq must call the int→str helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $__wasm_str_repeat (param $s i32) (param $k i64) (result i32)"),
        "the repeat helper must also be declared:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_repeat"),
        "$rep_i must call the repeat helper:\n{wat}"
    );
    // Every emitted `call $__wasm_*` must have a matching declared helper — the
    // exact invariant the gate hole broke. A str*int module needs int_to_str,
    // str_repeat, and the bump allocator; nothing else.
    assert!(
        wat.contains("(func $__alloc") && wat.contains("(memory"),
        "str(n) * k materialises heap strings → needs the bump allocator + memory:\n{wat}"
    );
    // No dead sibling helpers (the "no dead helper" discipline).
    assert!(
        !wat.contains("(func $__wasm_str_find")
            && !wat.contains("(func $__wasm_str_contains")
            && !wat.contains("(func $__wasm_str_slice"),
        "a str(n)*k module carries no find/contains/slice helper:\n{wat}"
    );
}

#[test]
fn real_str_repeat_int_programs_execute_in_wasm_and_match_cpython() {
    // Prove the emit path lowers (holds without WABT).
    let kernel_wat =
        emit_module(&rep_i_module()).expect("str(n) * k program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1149: skipping EXECUTED `str(int) * k` witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module AND carries \
             `$__wasm_int_to_str` (asserted in `str_repeat_int_source_emits_int_to_str_helper` \
             — the actual gate-hole regression guard); a box with WABT also runs all {} \
             cases and asserts each CONSTRUCTED result == the pinned CPython string. \
             Free CI skips execution and stays green.",
            CASES.len()
        );
        return;
    }

    eprintln!("PMAT-1149: running EXECUTED `str(int) * k` witness via WABT");
    let mut checked = 0usize;
    for c in CASES {
        let exp = c.expected.as_bytes();
        let (len, bytes) = exec_str_repeat(&kernel_wat, c.n, c.k, exp.len());
        assert_eq!(
            len as usize,
            exp.len(),
            "executed WASM len(str({}) * {}) = {len} but CPython byte-len = {}",
            c.n,
            c.k,
            exp.len()
        );
        assert_eq!(
            bytes,
            exp,
            "executed WASM (str({}) * {}) = {:?} but CPython = {:?}",
            c.n,
            c.k,
            String::from_utf8_lossy(&bytes),
            c.expected
        );
        checked += 1;
    }
    eprintln!(
        "PMAT-1149: EXECUTED `str(int) * k` witness PASSED — {checked} cases lowered \
         through emit_module and executed in WABT, each byte-matching CPython, \
         including the NEGATIVE int sign path (str(-12) * 2 == \"-12-12\"), the str(0) \
         \"at least one digit\" path, and the NEGATIVE-count clamp (str(9) * -2 == \"\"). \
         The int→str helper — previously undeclared for a repeat-hosted str(int) — is \
         proven declared AND correct on silicon."
    );
}
