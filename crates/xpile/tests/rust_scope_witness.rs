//! PMAT-1381 (XPILE-RUSTSCOPE-001): the Rust lane's BLOCK-SCOPE ESCAPE class.
//!
//! Python binds at FUNCTION scope: a name first assigned inside an `if` branch
//! leaks past the statement, so `if c: y = 5` then `print(y)` is legal Python.
//! The emitted `Stmt::If` is a Rust BLOCK, so the `let` dies at the closing
//! brace. Through v0.1.617 the frontend left the name in scope anyway, and
//! `xpile transpile --target rust` EXITED 0 emitting Rust that `rustc` REJECTS
//! with E0425 — the accept-then-fail shape PMAT-1378 closed for the WASM lane,
//! measured here across if-only, else-only, elif-without-else, nested-if and
//! multi-statement if/else shapes.
//!
//! The load-bearing test is [`transpiled_rust_either_refuses_or_rustc_accepts_it`]:
//! it asserts the PROPERTY `Ok(rust) ==> rustc accepts it` over the corpus,
//! rather than pinning one message. A per-shape refusal assertion cannot catch
//! the NEXT shape that leaks; the property can.
//!
//! Two loop-side residuals of the SAME class survive and are PINNED, not hidden
//! (see the `known_residual` tests): the PMAT-1038 hoist that rescues the
//! straight-line loop shape declines a `for ... else` and a dict/set-valued
//! loop local, and where it DOES fire it seeds a default that survives the
//! zero-iteration path on which CPython raises `UnboundLocalError`.
//!
//! Gated on `python3` + `rustc` presence (skips with a reason, like the oracle).

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

fn rustc_present() -> bool {
    Command::new("rustc")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn python3_present() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Mirror of the oracle harness's indexmap-rlib discovery so dict-typed probes
/// (which emit `indexmap::IndexMap`) link under bare `rustc`.
fn indexmap_rustc_args() -> Vec<OsString> {
    let Ok(exe) = std::env::current_exe() else {
        return Vec::new();
    };
    let Some(deps) = exe.parent() else {
        return Vec::new();
    };
    let mut rlib = None;
    if let Ok(rd) = std::fs::read_dir(deps) {
        for entry in rd.flatten() {
            let p = entry.path();
            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                if name.starts_with("libindexmap-") && name.ends_with(".rlib") {
                    rlib = Some(p);
                    break;
                }
            }
        }
    }
    let Some(rlib) = rlib else {
        return Vec::new();
    };
    let mut dep = OsString::from("dependency=");
    dep.push(deps);
    let mut ext = OsString::from("indexmap=");
    ext.push(rlib);
    vec!["-L".into(), dep, "--extern".into(), ext]
}

/// A per-CALL unique scratch directory. Keying it on (tag, pid) is NOT enough:
/// `transpile` and `rustc_accepts` are separate calls for the same tag, and the
/// tests run on parallel threads — a shared directory gets wiped mid-compile
/// and `rustc` fails to LINK its own object files, which reads exactly like an
/// emitter defect. The atomic counter is what makes each call disjoint.
fn scratch(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join("xpile-rust-scope").join(format!(
        "{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// `Ok(rust_source)` when the frontend accepts, `Err(stderr)` when it refuses.
fn transpile(src: &str, tag: &str) -> Result<String, String> {
    let dir = scratch(tag);
    let py = dir.join("p.py");
    std::fs::write(&py, src).expect("write probe");
    let out = Command::new(xpile_bin())
        .args(["transpile", py.to_str().unwrap(), "--target", "rust"])
        .output()
        .expect("spawn xpile");
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// `Ok(())` when `rustc` accepts the emitted Rust, `Err(stderr)` when it rejects.
fn rustc_accepts(rust: &str, tag: &str) -> Result<PathBuf, String> {
    let dir = scratch(tag);
    let rs = dir.join("p.rs");
    std::fs::write(&rs, rust).expect("write rust");
    let bin = dir.join("p");
    let out = Command::new("rustc")
        .arg("--edition=2021")
        .arg("-O")
        .arg("-o")
        .arg(&bin)
        .arg(&rs)
        .args(indexmap_rustc_args())
        .output()
        .expect("spawn rustc");
    if out.status.success() {
        Ok(bin)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

fn run_python(src: &str) -> Result<String, String> {
    let out = Command::new("python3")
        .arg("-c")
        .arg(format!("{src}\nmain()\n"))
        .output()
        .expect("spawn python3");
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

// ---------------------------------------------------------------------------
// The corpus. Every entry is a self-contained module defining `main()`.
// ---------------------------------------------------------------------------

/// Shapes whose FIRST binding of the read name is inside an `if`/`elif`/`else`
/// branch. Every one of these emitted E0425-rejected Rust through v0.1.617.
const IF_ESCAPE: &[(&str, &str)] = &[
    (
        "if_only_annotated",
        "def main() -> None:\n    c: bool = True\n    if c:\n        y: int = 5\n    print(y)\n",
    ),
    (
        "if_only_bare",
        "def main() -> None:\n    c: bool = True\n    if c:\n        y = 5\n    print(y)\n",
    ),
    (
        "if_only_str",
        "def main() -> None:\n    c: bool = True\n    if c:\n        s: str = \"hi\"\n    print(s)\n",
    ),
    (
        "if_only_list",
        "def main() -> None:\n    c: bool = True\n    if c:\n        xs: list[int] = [1, 2]\n    print(xs)\n",
    ),
    (
        "else_only",
        "def main() -> None:\n    c: bool = False\n    if c:\n        print(\"x\")\n    else:\n        y: int = 6\n    print(y)\n",
    ),
    (
        "elif_without_else",
        "def main() -> None:\n    n: int = 2\n    if n == 1:\n        y: int = 1\n    elif n == 2:\n        y: int = 2\n    print(y)\n",
    ),
    (
        "nested_if_in_if",
        "def main() -> None:\n    c: bool = True\n    if c:\n        if c:\n            y: int = 7\n    print(y)\n",
    ),
    (
        "if_inside_for",
        "def main() -> None:\n    for i in range(3):\n        if i == 1:\n            y: int = 42\n    print(y)\n",
    ),
    (
        "for_inside_if",
        "def main() -> None:\n    c: bool = True\n    if c:\n        for i in range(2):\n            y: int = i\n    print(y)\n",
    ),
    (
        "read_in_a_later_block",
        "def main() -> None:\n    c: bool = True\n    if c:\n        y: int = 5\n    if c:\n        print(y)\n",
    ),
    (
        "read_in_an_expression",
        "def main() -> None:\n    c: bool = True\n    if c:\n        y: int = 5\n    print(y + 1)\n",
    ),
    (
        "read_after_return_shape",
        "def f(c: bool) -> int:\n    if c:\n        y: int = 5\n    return y\n\ndef main() -> None:\n    print(f(True))\n",
    ),
    // Bound on EVERY path in Python, but a multi-statement branch takes the
    // general `Stmt::If` path rather than the `let y = if c {…} else {…}` one,
    // so the binding is still block-scoped. Refusing is the honest answer:
    // through v0.1.617 this shape also exited 0 into E0425.
    (
        "ifelse_multi_statement_then",
        "def main() -> None:\n    c: bool = True\n    if c:\n        print(\"x\")\n        y: int = 5\n    else:\n        y: int = 6\n    print(y)\n",
    ),
    (
        "ifelse_multi_statement_both",
        "def main() -> None:\n    c: bool = True\n    if c:\n        print(\"x\")\n        y: int = 5\n    else:\n        print(\"z\")\n        y: int = 6\n    print(y)\n",
    ),
];

/// Shapes that MUST keep working — the emitter already gives each a
/// function-scope binding. These are the non-regression half: a refusal that
/// over-fires would take these with it.
const STILL_ACCEPTED: &[(&str, &str)] = &[
    // The if-as-let path: single-statement branches, every branch binds.
    (
        "ifelse_single_statement",
        "def main() -> None:\n    c: bool = True\n    if c:\n        y: int = 5\n    else:\n        y: int = 6\n    print(y)\n",
    ),
    (
        "elif_chain_with_else",
        "def main() -> None:\n    n: int = 2\n    if n == 1:\n        y: int = 1\n    elif n == 2:\n        y: int = 2\n    else:\n        y: int = 3\n    print(y)\n",
    ),
    // Pre-bound before the `if`, reassigned inside — the documented workaround
    // the refusal message tells the user to reach for.
    (
        "prebound_then_conditionally_reassigned",
        "def main() -> None:\n    y: int = 0\n    c: bool = True\n    if c:\n        y = 5\n    print(y)\n",
    ),
    // Read only INSIDE the binding branch: never escapes, never poisoned.
    (
        "read_inside_the_branch_only",
        "def main() -> None:\n    c: bool = True\n    if c:\n        y: int = 5\n        print(y)\n",
    ),
    // Bound in a branch but never read afterwards.
    (
        "never_read_after_the_if",
        "def main() -> None:\n    c: bool = True\n    if c:\n        y: int = 5\n    print(\"done\")\n",
    ),
    // The PMAT-1038 loop hoist: a straight-line loop-body local read after the
    // loop is pre-declared at function scope.
    (
        "loop_body_local_read_after_for",
        "def main() -> None:\n    for i in range(3):\n        y: int = i * 2\n    print(y)\n",
    ),
    (
        "loop_body_local_read_after_while",
        "def main() -> None:\n    i: int = 0\n    while i < 3:\n        y: int = i\n        i = i + 1\n    print(y)\n",
    ),
    // The loop TARGET itself leaks in Python and must keep leaking here — the
    // withdrawal must not take it (`for i in …` then `print(i)`).
    (
        "loop_target_read_after_loop",
        "def main() -> None:\n    for i in range(3):\n        pass\n    print(i)\n",
    ),
    // PMAT-1381 made this WORK: the branch binding is withdrawn, so the
    // post-`if` assignment emits a fresh function-scope `let`.
    (
        "rebound_after_the_if",
        "def main() -> None:\n    c: bool = True\n    if c:\n        y: int = 5\n    y = 9\n    print(y)\n",
    ),
];

// ---------------------------------------------------------------------------
// The load-bearing property.
// ---------------------------------------------------------------------------

/// XPILE-RUSTSCOPE-001. `Ok(rust) ==> rustc accepts it`, over BOTH corpora.
///
/// This is the assertion that generalises: it does not care WHICH shapes refuse
/// and which compile, only that the CLI never exits 0 on Rust the compiler
/// rejects. Through v0.1.617 fourteen of these sources failed it.
#[test]
fn transpiled_rust_either_refuses_or_rustc_accepts_it() {
    if !rustc_present() {
        eprintln!("warning: rustc not on PATH; skipping XPILE-RUSTSCOPE-001 property");
        return;
    }
    let mut accept_then_fail: Vec<String> = Vec::new();
    let mut refused = 0usize;
    let mut compiled = 0usize;
    for (tag, src) in IF_ESCAPE.iter().chain(STILL_ACCEPTED.iter()) {
        match transpile(src, tag) {
            Err(_) => refused += 1,
            Ok(rust) => match rustc_accepts(&rust, tag) {
                Ok(_) => compiled += 1,
                Err(stderr) => accept_then_fail.push(format!(
                    "{tag}: xpile exited 0 but rustc REJECTED the emitted Rust:\n{}\n--- emitted ---\n{rust}",
                    stderr.trim()
                )),
            },
        }
    }
    eprintln!(
        "XPILE-RUSTSCOPE-001: {} sources — {refused} refused, {compiled} compiled, {} accept-then-fail",
        IF_ESCAPE.len() + STILL_ACCEPTED.len(),
        accept_then_fail.len()
    );
    assert!(
        accept_then_fail.is_empty(),
        "{} source(s) transpiled to Rust that rustc rejects:\n\n{}",
        accept_then_fail.len(),
        accept_then_fail.join("\n\n")
    );
}

/// Every `IF_ESCAPE` shape refuses, and the refusal NAMES the escaping binding
/// and the workaround — an opaque "unsupported" would leave the user guessing.
#[test]
fn if_branch_binding_read_after_the_if_refuses_with_the_scoping_truth() {
    for (tag, src) in IF_ESCAPE {
        let stderr = transpile(src, tag).expect_err(&format!("{tag} must refuse"));
        assert!(
            stderr.contains("is first bound inside an `if`/`elif`/`else` branch"),
            "{tag}: refusal does not name the block-scope cause:\n{stderr}"
        );
        assert!(
            stderr.contains("Bind") && stderr.contains("before the `if`"),
            "{tag}: refusal does not offer the pre-bind workaround:\n{stderr}"
        );
    }
}

/// The non-regression half, EXECUTED: every `STILL_ACCEPTED` shape compiles and
/// its stdout matches CPython byte for byte. Pinning "it still transpiles"
/// alone would not catch a refusal that silently changed the emitted VALUE.
#[test]
fn still_accepted_shapes_execute_and_match_cpython() {
    if !rustc_present() || !python3_present() {
        eprintln!(
            "warning: rustc/python3 not on PATH; skipping XPILE-RUSTSCOPE-001 execution half"
        );
        return;
    }
    for (tag, src) in STILL_ACCEPTED {
        let rust =
            transpile(src, tag).unwrap_or_else(|e| panic!("{tag} must still transpile: {e}"));
        let bin = rustc_accepts(&rust, tag)
            .unwrap_or_else(|e| panic!("{tag}: rustc rejected the emitted Rust: {e}"));
        let actual = Command::new(&bin).output().expect("run probe binary");
        assert!(
            actual.status.success(),
            "{tag}: emitted binary exited {}: {}",
            actual.status,
            String::from_utf8_lossy(&actual.stderr)
        );
        let actual = String::from_utf8_lossy(&actual.stdout)
            .trim_end_matches('\n')
            .to_string();
        let expected = run_python(src).unwrap_or_else(|e| panic!("{tag}: CPython reference: {e}"));
        assert_eq!(actual, expected, "{tag}: diverges from CPython");
    }
}

/// The shape PMAT-1381 turned from E0425 into working code: withdrawing the
/// branch binding lets the post-`if` assignment emit a fresh function-scope
/// `let`. Executed against CPython so "it compiles" cannot stand in for "it is
/// right".
#[test]
fn rebinding_after_the_if_now_compiles_and_prints_the_rebound_value() {
    if !rustc_present() {
        eprintln!("warning: rustc not on PATH; skipping rebind witness");
        return;
    }
    let src = "def main() -> None:\n    c: bool = True\n    if c:\n        y: int = 5\n    y = 9\n    print(y)\n";
    let rust = transpile(src, "rebind").expect("rebound shape transpiles");
    let bin = rustc_accepts(&rust, "rebind").expect("rebound shape compiles");
    let out = Command::new(&bin).output().expect("run rebind probe");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "9",
        "the post-`if` rebinding is what Python prints"
    );
}

// ---------------------------------------------------------------------------
// PINNED RESIDUALS — the loop half of the same class, NOT fixed by PMAT-1381.
// These tests assert TODAY'S (wrong) behaviour on purpose, so the lane cannot
// read as fully honest about block-scope escape and so a later fix trips them
// loudly instead of leaving a stale claim behind.
// ---------------------------------------------------------------------------

/// Loop-side residual #1: the PMAT-1038 hoist declines a loop carrying an
/// `else` clause, so the body binding stays block-scoped and the CLI still
/// exits 0 into E0425. Deliberately NOT in the property corpus above.
#[test]
fn for_else_body_binding_is_a_known_accept_then_fail_residual() {
    if !rustc_present() {
        eprintln!("warning: rustc not on PATH; skipping for-else residual pin");
        return;
    }
    let src = "def main() -> None:\n    for i in range(2):\n        y: int = i\n    else:\n        print(\"done\")\n    print(y)\n";
    let rust = transpile(src, "for_else_residual")
        .expect("RESIDUAL: the frontend still ACCEPTS this (that is the defect)");
    let err = rustc_accepts(&rust, "for_else_residual")
        .expect_err("RESIDUAL: rustc still rejects the emitted Rust");
    assert!(
        err.contains("E0425"),
        "residual should still be the block-scope escape, got:\n{err}"
    );
}

/// Loop-side residual #2: the hoist covers list/primitive-typed loop locals
/// only, so a dict/set-valued one keeps the same accept-then-fail.
#[test]
fn dict_valued_loop_local_is_a_known_accept_then_fail_residual() {
    if !rustc_present() {
        eprintln!("warning: rustc not on PATH; skipping dict-local residual pin");
        return;
    }
    let src = "def main() -> None:\n    for i in range(2):\n        d: dict[str, int] = {\"a\": i}\n    print(len(d))\n";
    let rust = transpile(src, "dict_local_residual")
        .expect("RESIDUAL: the frontend still ACCEPTS this (that is the defect)");
    let err = rustc_accepts(&rust, "dict_local_residual")
        .expect_err("RESIDUAL: rustc still rejects the emitted Rust");
    assert!(
        err.contains("E0425"),
        "residual should still be the block-scope escape, got:\n{err}"
    );
}

/// Loop-side residual #3, and the worse one: where the hoist DOES fire it seeds
/// a primitive default, which survives the ZERO-ITERATION path. CPython raises
/// `UnboundLocalError` there; the emitted binary prints the default. That is a
/// SILENT WRONG ANSWER, not a refusal — pinned here so the disclosure in the
/// release notes has an executing witness behind it.
#[test]
fn empty_iteration_loop_hoist_default_is_a_known_silent_divergence() {
    if !rustc_present() || !python3_present() {
        eprintln!("warning: rustc/python3 not on PATH; skipping empty-iteration residual pin");
        return;
    }
    let src = "def main() -> None:\n    n: int = 0\n    while n > 0:\n        y: int = n\n        n = n - 1\n    print(y)\n";
    let py = run_python(src).expect_err("RESIDUAL: CPython raises on the zero-iteration path");
    assert!(
        py.contains("UnboundLocalError"),
        "CPython reference should be UnboundLocalError, got:\n{py}"
    );
    let rust = transpile(src, "empty_iter_residual").expect("RESIDUAL: the frontend accepts this");
    let bin = rustc_accepts(&rust, "empty_iter_residual").expect("RESIDUAL: it compiles");
    let out = Command::new(&bin)
        .output()
        .expect("run empty-iteration probe");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "0",
        "RESIDUAL: the hoisted default is printed where CPython raises UnboundLocalError"
    );
}
