//! PMAT-1427 (XPILE-IDXAPPEND-001): `xs[i].append(e)` / `d[k].append(e)` —
//! the last member of the subscript family still emitting a RAW narrowing
//! coercion.
//!
//! `Stmt::IndexAppend` lowered to `base[(i) as usize].push(e);` for a list base
//! and `base.get_mut(&(k)).unwrap().push(e);` for a dict base. Every SIBLING
//! path had already been corrected — the read (`Expr::Index`, PMAT-639/744),
//! the store (`Stmt::IndexAssign`, PMAT-640/641/863), `del` (PMAT-712/1351),
//! `insert` (PMAT-590), `pop` — each wrapping a negative index like Python and
//! panicking with an `xpile:`-TAGGED message. This one was missed, on BOTH the
//! rust and the ruchy lane, and `xpile transpile` exited 0 for all of it.
//!
//! MEASURED against live CPython through the shipped CLI at `ba31f119`
//! (pre-fix), every row exit-0 on emit and then wrong at RUN time:
//!
//! | source                              | CPython           | pre-fix emit                       |
//! |-------------------------------------|-------------------|------------------------------------|
//! | `a[-1].append(99)`                  | appends to `a[2]` | panic, index `18446744073709551615` |
//! | `i = -1; a[i].append(99)`           | appends to `a[2]` | panic, index `18446744073709551615` |
//! | `try: a[5].append(9) / except IndexError` | caught      | UNTAGGED panic, NOT caught          |
//! | `try: d["zz"].append(9) / except KeyError` | caught      | `Option::unwrap()` on `None`, NOT caught |
//!
//! The third and fourth rows are the PMAT-731 shape: typed-`except`
//! discrimination only re-raises panics tagged `xpile: <KnownExc>:`, so a
//! native Rust panic escapes the `except` that Python would have caught. The
//! `.unwrap()` one is doubly notable because the HIR variant's own doc comment
//! CLAIMED "KeyError-on-absent parity with Python" — the claim was written, the
//! tag was not.
//!
//! FIXED FORWARD rather than refused (PMAT-1426 lesson 5): the semantics are
//! exactly spellable with the idioms the sibling paths already use — the
//! PMAT-639/863 wrap-plus-tagged-bounds block for the list base and the
//! PMAT-1089 `key_error_panic()` for the dict base. No new runtime, no
//! signature change, no capability removed.
//!
//! WHAT THIS FILE PINS, and what it deliberately does NOT:
//!   * The EMIT SHAPE on both lanes, keyed on the raw pre-fix spellings
//!     (`) as usize].push(` and `.unwrap().push(`) being GONE — so a revert
//!     reds here even if no toolchain is installed.
//!   * CROSS-LANE AGREEMENT: the two emitters are twins (ruchy compiles to
//!     Rust) and must emit the byte-identical statement. A later one-lane-only
//!     fix reds this.
//!   * ONE executed row (rustc), so the file stands alone.
//!   * The VALUE half over all ten rows — the wrap, both tagged exceptions, the
//!     unchanged non-negative fast path, and the boundary rows where a wrap and
//!     a clamp-to-zero coincide — lives in
//!     `tests/oracle_fixtures/index_append_index.py`, which the differential
//!     oracle diffs against live CPython. That fixture is the reason this is a
//!     measurement and not an assertion (PMAT-1426 lesson 2: run the property
//!     AND the value).
//!
//! NOT IN SCOPE, confirmed pre-existing and IDENTICAL on both binaries: the
//! ruchy lane emits `fun add(&self, …)` where the rust lane emits
//! `fn add(&mut self, …)` for a method whose body is `self.rows[i].append(v)`
//! (PMAT-1052's struct-field base), so that ONE shape does not compile on the
//! ruchy lane (rustc E0596) — before this fix or after it. That is the mutable
//! pre-walk's field-receiver recognition, a different construct; it is recorded
//! as a standing lead, not silently folded in here.

use depyler_frontend::PythonFrontend;
use std::path::Path;
use std::process::Command;
use xpile_frontend::Frontend;
use xpile_meta_hir::Stmt;

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

/// The list-base probe: a literal negative, a runtime negative, and a
/// non-negative literal, so one emit carries every list arm.
const LIST_SRC: &str = r#"def main() -> None:
    a: list[list[int]] = [[1], [2], [3]]
    a[-1].append(99)
    i: int = -2
    a[i].append(88)
    a[0].append(7)
    print(a[2][1], a[1][1], a[0][1])
"#;

/// The dict-base probe.
const DICT_SRC: &str = r#"def main() -> None:
    d: dict[str, list[int]] = {"a": [1]}
    d["a"].append(5)
    print(d["a"][1])
"#;

/// Transpile `src` through the SHIPPED binary for `target`, returning stdout.
fn emit(label: &str, target: &str, src: &str) -> String {
    // PMAT-1429: the probe dir must be unique per CALL, not per
    // (label, target). Several tests in this file emit the same
    // (label, target) pair, and `cargo test` runs them concurrently — so
    // one test's `remove_dir_all` raced another's `create_dir_all`, and the
    // `fs::write` below failed with NotFound. Intermittent RED on
    // `workspace-test`, a REQUIRED context, from a witness that passes
    // whenever it is run alone (`--test-threads=1`).
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir()
        .join("xpile-index-append-witness")
        .join(format!("{label}-{target}-{}-{nonce}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create probe dir");
    let py = dir.join("p.py");
    std::fs::write(&py, src).expect("write probe source");
    let out = Command::new(xpile_bin())
        .args(["transpile", py.to_str().unwrap(), "--target", target])
        .args(["--contracts", "off"])
        .output()
        .expect("run xpile");
    assert!(
        out.status.success(),
        "`--target {target}` must still ACCEPT {label} (this is a fix-forward, \
not a refusal): {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 emit")
}

/// PREMISE. Both probes must actually lower to `Stmt::IndexAppend` — otherwise
/// every emit assertion below would be about some other construct and would
/// pass for a reason that has nothing to do with this fix.
#[test]
fn both_probes_lower_to_index_append() {
    for (label, src, want_dict) in [("list", LIST_SRC, false), ("dict", DICT_SRC, true)] {
        let module = PythonFrontend
            .parse_and_lower(Path::new("t.py"), src)
            .unwrap_or_else(|e| panic!("{label} probe must lower: {e}"));
        let mut seen = 0usize;
        for item in &module.items {
            let xpile_meta_hir::Item::Function(f) = item else {
                continue;
            };
            for stmt in &f.body.stmts {
                if let Stmt::IndexAppend { base_is_dict, .. } = stmt {
                    assert_eq!(
                        *base_is_dict, want_dict,
                        "{label} probe: wrong container flavour"
                    );
                    seen += 1;
                }
            }
        }
        assert!(
            seen > 0,
            "{label} probe must produce at least one Stmt::IndexAppend"
        );
    }
}

/// The list base WRAPS a negative index and bounds-checks it with the
/// `xpile: IndexError:` tag, on both lanes. Keyed on the raw pre-fix spelling
/// `) as usize].push(` being GONE — the post-fix form is
/// `[__iax as usize].push(`, so the two are distinguishable by the character
/// before ` as usize].push(` and a revert reds this without any toolchain.
#[test]
fn list_base_wraps_and_tags_bounds_on_both_lanes() {
    for target in ["rust", "ruchy"] {
        let out = emit("list", target, LIST_SRC);
        assert!(
            !out.contains(") as usize].push("),
            "`--target {target}`: the RAW narrowing coercion `base[(i) as usize].push(..)` \
is back — `(-1i64) as usize` is usize::MAX, which panics where CPython appends \
to the last sub-list:\n{out}"
        );
        assert!(
            out.contains("let __ia: i64 = ("),
            "`--target {target}`: the index must bind before the wrap:\n{out}"
        );
        assert!(
            out.contains("let __iax = if __ia < 0 {"),
            "`--target {target}`: a negative index must wrap to `len + i`:\n{out}"
        );
        assert!(
            out.contains("panic!(\"xpile: IndexError: list index out of range\")"),
            "`--target {target}`: an out-of-range index must panic with the \
`xpile: IndexError:` TAG — the typed-`except` filter (PMAT-731) only re-raises \
tagged panics, so an untagged one escapes `except IndexError`:\n{out}"
        );
        assert!(
            out.contains("[__iax as usize].push("),
            "`--target {target}`: the push must index through the WRAPPED index:\n{out}"
        );
    }
}

/// The dict base panics with the CPython-shaped `xpile: KeyError:` tag rather
/// than `Option::unwrap`'s untagged native message, on both lanes.
#[test]
fn dict_base_tags_key_error_on_both_lanes() {
    for target in ["rust", "ruchy"] {
        let out = emit("dict", target, DICT_SRC);
        assert!(
            !out.contains(".unwrap().push("),
            "`--target {target}`: the untagged `get_mut(&k).unwrap().push(..)` is back — \
`except KeyError` cannot catch a native `Option::unwrap()` panic:\n{out}"
        );
        assert!(
            out.contains("get_mut(__k).unwrap_or_else(|| panic!(\"xpile: KeyError: {}\""),
            "`--target {target}`: an absent key must panic with the CPython-shaped \
`xpile: KeyError:` tag (PMAT-1089's `key_error_panic`):\n{out}"
        );
    }
}

/// CROSS-LANE AGREEMENT. The ruchy emitter is the rust emitter's twin (ruchy
/// compiles to Rust), so the `IndexAppend` statement itself must be
/// byte-identical. This is what catches a later fix applied to ONE lane — the
/// asymmetry that PMAT-1425/1426 both named as the tell.
#[test]
fn both_lanes_emit_the_same_index_append_statement() {
    for (label, src, needle) in [
        ("list", LIST_SRC, "let __ia: i64 = ("),
        ("dict", DICT_SRC, "let __k = &("),
    ] {
        let pick = |target: &str| -> Vec<String> {
            emit(label, target, src)
                .lines()
                .filter(|l| l.contains(needle))
                .map(|l| l.trim().to_string())
                .collect()
        };
        let rust = pick("rust");
        let ruchy = pick("ruchy");
        assert!(
            !rust.is_empty(),
            "{label}: the rust lane must emit the statement at all"
        );
        assert_eq!(
            rust, ruchy,
            "{label}: the rust and ruchy lanes must emit the SAME IndexAppend \
statement — a one-lane-only fix is the cross-lane asymmetry this gate exists \
to catch"
        );
    }
}

/// One EXECUTED row so the file does not rest entirely on text. Compiles the
/// list probe's rust emit with `rustc` and checks the value CPython produces.
/// Skips with a printed reason when `rustc` is absent, the same posture as the
/// e2e harness.
#[test]
fn negative_index_append_executes_to_the_cpython_value() {
    if Command::new("rustc").arg("--version").output().is_err() {
        eprintln!("SKIP negative_index_append_executes_to_the_cpython_value: rustc absent");
        return;
    }
    let out = emit("list-exec", "rust", LIST_SRC);
    let dir = std::env::temp_dir().join("xpile-index-append-witness/exec");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create exec dir");
    let rs = dir.join("m.rs");
    let bin = dir.join("m");
    std::fs::write(&rs, &out).expect("write emit");
    let c = Command::new("rustc")
        .args(["-O", rs.to_str().unwrap(), "-o", bin.to_str().unwrap()])
        .output()
        .expect("run rustc");
    assert!(
        c.status.success(),
        "the emit must COMPILE (accept-then-fail-rustc is the disposition this \
sprint removes):\n{}\n--- emit ---\n{out}",
        String::from_utf8_lossy(&c.stderr)
    );
    let r = Command::new(&bin).output().expect("run emitted binary");
    let got = String::from_utf8_lossy(&r.stdout);
    assert_eq!(
        got.trim(),
        "99 88 7",
        "CPython prints `99 88 7` for the list probe — `a[-1]` and `a[i=-2]` \
append to the LAST and second-to-last sub-lists, `a[0]` to the first. \
stderr:\n{}",
        String::from_utf8_lossy(&r.stderr)
    );
}
