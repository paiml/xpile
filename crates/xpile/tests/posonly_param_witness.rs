//! XPILE-POSONLY-001 (PMAT-1389) — a parameter the source declares is either
//! emitted or refused, never silently dropped.
//!
//! ## What was wrong
//!
//! `lower_function` in `crates/depyler-frontend/src/lib.rs` guarded
//! `kwonlyargs` and `kwarg` but never looked at `posonlyargs`, and its
//! parameter loop iterates `f.args.args` alone. Every parameter left of a `/`
//! in a module-level `def` was therefore **deleted from the signature**, and
//! each call site's positional arguments shifted left onto whichever
//! parameters survived. Measured through the shipped CLI at `c1d7db19`:
//!
//! ```text
//! def f(a: int, /, b: int = 2) -> int:      python3 -> 2
//!     return b
//! def run() -> int:
//!     return f(9)
//!
//! xpile transpile m.py --target rust        rc=0, 0 bytes of stderr
//!   pub fn f(b: i64) -> i64 { b }           <- `a` gone
//!   pub fn run() -> i64 { f(9i64) }         <- 9 re-binds to `b`
//! rustc m.rs && ./m                         -> 9      (CPython: 2)
//! ```
//!
//! That is the worst shape in this window's sweep and the cheapest instance of
//! it: exit 0, empty stderr, output that **compiles and executes cleanly**, and
//! is wrong — on the rust, wasm, lean and ruchy lanes at once, from one hole in
//! one frontend.
//!
//! ## What is asserted, and why in this shape
//!
//! The load-bearing test is [`every_declared_parameter_is_emitted_or_refused`].
//! It is not a list of the spellings that must refuse — it is the *relation the
//! defect violated*: for each probe, either the frontend refuses, or every
//! parameter name the `def` line declares appears in the emitted signature. A
//! hand-listed refusal set would go stale the moment `/` support is added;
//! this one stays true either way, because a lane that starts *supporting* `/`
//! satisfies it by emitting the parameter.
//!
//! [`the_control_still_executes_and_agrees_with_cpython`] is the red half. The
//! refusal above is only worth having if the `/`-free spelling of the same
//! program still works end to end — otherwise this slice would have traded a
//! wrong answer for a dead lane. It transpiles, compiles with `rustc`, runs the
//! binary, and compares against CPython. That differential is what would have
//! caught the defect in the first place: it prints `2` on both sides now and
//! printed `9` from the Rust side before the fix.
//!
//! Over-refusal is guarded separately by [`the_neighbouring_parameter_kinds_are_untouched`]:
//! `*args`, ordinary defaults and methods must all still lower. `*args` in
//! particular is handled *correctly* today and was explicitly out of scope.
//!
//! No toolchain is needed for the two frontend tests, so they never skip. The
//! execution half skips loudly (on stderr) when `rustc`/`python3` are absent.
//! Runtimes below were measured, not assumed.

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

/// A per-CALL unique scratch directory. Per-TEST is not enough — these tests
/// run on parallel threads and `transpile`/`rustc` are separate calls for the
/// same probe, so a shared directory gets wiped mid-compile and `rustc` fails
/// to link its own object files, which reads exactly like an emitter defect.
fn scratch(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join("xpile-posonly").join(format!(
        "{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Run `xpile transpile` for `target`. `Ok(stdout)` when the lane accepts,
/// `Err(stderr)` when it refuses. Contracts off so the emitted text is the
/// signature and nothing else.
fn transpile(src: &str, target: &str, tag: &str) -> Result<String, String> {
    let dir = scratch(tag);
    let py = dir.join("p.py");
    std::fs::write(&py, src).expect("write probe");
    let out = Command::new(xpile_bin())
        .args([
            "transpile",
            py.to_str().unwrap(),
            "--target",
            target,
            "--contracts",
            "off",
        ])
        .output()
        .expect("spawn xpile");
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        assert!(
            out.stdout.is_empty(),
            "a refusal must emit NO artifact, got {} bytes of stdout",
            out.stdout.len()
        );
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// The probe corpus: `(tag, source, parameter names the `def` line declares)`.
///
/// The names are written out rather than parsed back out of the source so the
/// expectation is independent of the thing under test — a parser bug cannot
/// make the assertion agree with itself.
///
/// The parameter names are deliberately unlike anything the backends emit on
/// their own — the WAT lane, for one, always prints an `__wasm_floordiv_i64`
/// helper with `(param $a i64) (param $b i64)`, so a probe that called its
/// parameters `a`/`b` would find them in every emission and pass vacuously.
const PROBES: &[(&str, &str, &[&str])] = &[
    (
        "posonly_with_default",
        // THE reproducer. The defaulted second parameter is what makes the
        // emitted Rust still *compile* after `alpha` is dropped: the call site
        // `f(9)` has exactly as many arguments as the mutilated signature has
        // parameters, so nothing downstream can notice.
        "def f(alpha: int, /, beta: int = 2) -> int:\n    return beta\n",
        &["alpha", "beta"],
    ),
    (
        "posonly_only",
        "def g(alpha: int, beta: int, /) -> int:\n    return alpha\n",
        &["alpha", "beta"],
    ),
    (
        "posonly_then_positional",
        "def h(alpha: int, /, beta: int) -> int:\n    return alpha + beta\n",
        &["alpha", "beta"],
    ),
    (
        "single_posonly",
        "def one(alpha: int, /) -> int:\n    return alpha\n",
        &["alpha"],
    ),
    (
        "plain_positional_control",
        "def f(alpha: int, beta: int = 2) -> int:\n    return beta\n",
        &["alpha", "beta"],
    ),
    (
        "vararg_control",
        "def total(*items: int) -> int:\n    s = 0\n    for it in items:\n        s = s + it\n    return s\n",
        &["items"],
    ),
];

/// Does `emitted` bind a parameter called `name`? One spelling per lane:
/// Rust/Ruchy `alpha: i64`, Lean `(alpha : Int)`, WAT `(param $alpha i64)`.
fn binds_param(emitted: &str, name: &str) -> bool {
    emitted.contains(&format!("{name}:"))
        || emitted.contains(&format!("{name} :"))
        || emitted.contains(&format!("${name} "))
}

/// THE load-bearing invariant, over four lanes.
///
/// For every probe and every lane: refuse, or emit all of the declared
/// parameters. Before PMAT-1389 the four `/` probes were all *accepted* with a
/// parameter missing — 16 violations across rust/wasm/lean/ruchy.
#[test]
fn every_declared_parameter_is_emitted_or_refused() {
    let t0 = std::time::Instant::now();
    let mut accepted = 0usize;
    let mut refused = 0usize;
    let mut violations: Vec<String> = Vec::new();
    for target in ["rust", "wasm", "lean", "ruchy"] {
        for (tag, src, params) in PROBES {
            match transpile(src, target, tag) {
                Err(_) => refused += 1,
                Ok(emitted) => {
                    accepted += 1;
                    for p in *params {
                        if !binds_param(&emitted, p) {
                            violations.push(format!(
                                "{target}/{tag}: parameter `{p}` is declared in the source but \
                                 absent from the emitted signature:\n{emitted}"
                            ));
                        }
                    }
                }
            }
        }
    }
    eprintln!(
        "XPILE-POSONLY-001: {accepted} accepted, {refused} refused across \
         {} probe/lane pairs ({:.2}s)",
        PROBES.len() * 4,
        t0.elapsed().as_secs_f64()
    );
    // Vacuity guard: a frontend that refused everything would satisfy the
    // relation trivially, and that is precisely the failure mode an
    // over-broad `/` guard would introduce.
    //
    // 7, not 8, and the missing one is measured rather than assumed: the two
    // controls across four lanes would be 8, but the Lean lane refuses
    // `vararg_control` for an unrelated pre-existing reason — `for x in xs:`
    // needs a monadic-iteration encoding the Lean backend does not have (that
    // lane is value-functions only). The four `/` probes contribute 0.
    assert!(
        accepted >= 7,
        "only {accepted} probe/lane pairs were accepted — the emission half of \
         this test is near-vacuous (live figure at PMAT-1389 was 7: two controls \
         across four lanes, less the Lean lane's pre-existing refusal of the \
         `for` loop in `vararg_control`)"
    );
    assert!(
        violations.is_empty(),
        "PMAT-1389: {} signature(s) dropped a parameter the source declares:\n\n{}",
        violations.len(),
        violations.join("\n\n")
    );
}

/// The refusal must say WHICH feature it refused. A caller who reads
/// "keyword-only args / **kwargs" for a `/` looks at the wrong half of the
/// signature, which is why this is its own arm in `lower_function` rather than
/// an extra disjunct on the keyword-only guard.
#[test]
fn the_refusal_names_positional_only_parameters() {
    for target in ["rust", "wasm", "lean", "ruchy"] {
        let err = transpile(PROBES[0].1, target, "reason")
            .expect_err("a `/` signature must be refused, not emitted");
        assert!(
            err.contains("positional-only") && err.contains('/'),
            "--target {target}: the refusal must name positional-only parameters and \
             the `/` separator, got:\n{err}"
        );
        assert!(
            !err.contains("keyword-only"),
            "--target {target}: a `/` was reported as a keyword-only/**kwargs problem, \
             which points the reader at the wrong end of the signature:\n{err}"
        );
    }
}

/// Over-refusal guard. `*args` is lowered CORRECTLY today (`def total(*args: int)`
/// → `pub fn total(args: Vec<i64>)`, and both sides sum to 6), ordinary
/// defaults are filled in at the call site, and methods carry `self`. None of
/// them may be caught by the new `/` arm.
#[test]
fn the_neighbouring_parameter_kinds_are_untouched() {
    for (tag, src) in [
        ("vararg", PROBES[5].1),
        ("default", PROBES[4].1),
        (
            "method",
            "class C:\n    def __init__(self, x: int) -> None:\n        self.x = x\n\n    def get(self) -> int:\n        return self.x\n",
        ),
    ] {
        let emitted = transpile(src, "rust", tag)
            .unwrap_or_else(|e| panic!("`{tag}` must still lower after PMAT-1389, got:\n{e}"));
        assert!(
            !emitted.is_empty(),
            "`{tag}` lowered to an empty artifact"
        );
    }
}

/// THE RED HALF, executed. The `/`-free spelling of the reproducer must still
/// transpile, compile under `rustc`, run, and agree with CPython — otherwise
/// PMAT-1389 traded a silent wrong answer for a dead lane.
///
/// This is the differential that would have caught the defect: run against the
/// `/` spelling before the fix it printed `9` where CPython prints `2`.
#[test]
fn the_control_still_executes_and_agrees_with_cpython() {
    if !rustc_present() || !python3_present() {
        eprintln!(
            "warning: rustc/python3 not on PATH; skipping XPILE-POSONLY-001 execution half \
             (the frontend halves above still ran)"
        );
        return;
    }
    let t0 = std::time::Instant::now();
    let py_src =
        "def f(a: int, b: int = 2) -> int:\n    return b\n\ndef run() -> int:\n    return f(9)\n";

    let py = Command::new("python3")
        .arg("-c")
        .arg(format!("{py_src}\nprint(run())\n"))
        .output()
        .expect("spawn python3");
    assert!(py.status.success(), "python3 probe failed");
    let expected = String::from_utf8_lossy(&py.stdout).trim().to_string();
    assert_eq!(
        expected, "2",
        "the probe's own semantics changed — `f(9)` must take the default `b=2`"
    );

    let rust = transpile(py_src, "rust", "exec").expect("the `/`-free control must lower");
    let dir = scratch("exec-build");
    let rs = dir.join("p.rs");
    std::fs::write(
        &rs,
        format!("{rust}\nfn main() {{ println!(\"{{}}\", run()); }}\n"),
    )
    .expect("write rust");
    let bin = dir.join("p");
    let build = Command::new("rustc")
        .arg("--edition=2021")
        .arg("-o")
        .arg(&bin)
        .arg(&rs)
        .output()
        .expect("spawn rustc");
    assert!(
        build.status.success(),
        "the emitted control must compile:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&bin).output().expect("run emitted binary");
    assert!(run.status.success(), "the emitted binary must run");
    let got = String::from_utf8_lossy(&run.stdout).trim().to_string();
    // Print the receipt rather than inferring execution from a green exit
    // (PMAT-1383): both observed values and the wall clock of the rustc build,
    // so a future reader can tell a real differential from a silent skip.
    eprintln!(
        "XPILE-POSONLY-001 execution: cpython={expected} emitted-rust={got} \
         (transpile+rustc+run {:.2}s)",
        t0.elapsed().as_secs_f64()
    );
    assert_eq!(
        got, expected,
        "PMAT-1389: the emitted Rust printed `{got}` where CPython prints `{expected}` \
         — a positional argument bound to the wrong parameter"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
