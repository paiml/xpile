//! XPILE-WITNESS (WASM lane) — PMAT-1419: the WASM backend REFUSES every
//! source language whose integer semantics it does not implement, instead of
//! emitting Python semantics for it and exiting 0.
//!
//! WHAT WAS WRONG. `xpile-wasm-codegen` implements ONE integer lowering, and
//! it is Python's: `int` is unbounded, `//` floors, and the emitted helper
//! carries the generated comment `;; __wasm_floordiv_i64(a, b) = floor(a / b)
//! (Python //)`. Nothing enforced that premise. Where the Lean backend at
//! least READ `module.source_lang` (to print it into a header comment) before
//! ignoring it — PMAT-1418 — this backend never referenced `source_lang` **at
//! all**, so the mismatch was not even observable in its own output.
//!
//! `decy-frontend` lowers C `int` — a 32-bit, truncating-division, wrapping
//! type — to `Type::I64`, and per that variant's own documentation the Rust
//! backend "narrows it to `i32` on the `SourceLang::C` emit path". There is no
//! such path here, and `source_lang` is the ONLY discriminator: the meta-HIR
//! types C and Python produce are identical. So C reached the Python lowering.
//!
//! Measured at `c0b52d0f` on the tracked `crates/xpile/tests/fixtures/
//! c_int_arith.c`, executing the emitted module under `wasm-interp` against
//! `gcc -O0` on the same file. xpile's own `--target rust` emit agrees with
//! `gcc` on all three, so this is the WASM lane diverging from xpile itself,
//! not merely from a foreign compiler:
//!
//! | expression      | `gcc` / `--target rust`   | `--target wasm` (before)      |
//! |-----------------|---------------------------|-------------------------------|
//! | `half(-7)`      | `-3` (C `/` truncates)    | `-4` — the helper FLOORS      |
//! | `poly(50000)`   | `-1794867295` (i32 wraps) | `2500100001` — i64, no wrap   |
//! | `factorial(13)` | `1932053504` (i32 wraps)  | `6227020800` — i64, no wrap   |
//!
//! Every one is SILENT: `wat2wasm` ACCEPTS the module and `wasm-interp` RUNS
//! it, so the lane returned a different number at exit 0 rather than refusing.
//! The emit was additionally byte-identical to the emit of the corresponding
//! PYTHON source — the C file's semantics left no trace whatsoever in the
//! output — and every emitted function cited `C-COMPILE-RUST-TO-WASM`, a
//! *Rust*-to-WASM compile contract, on C source that `C-C-INT-ARITH` governs.
//!
//! WHY IT SURVIVED. This is the same shape PMAT-1418 fixed one backend at a
//! time; that slice's own guard doc names it — "which is precisely how `C` and
//! `Wasm` arrived" — but swept only the Lean lane. The WASM lane carries ~69 of
//! this release's commits and the executing witnesses in the required CI job.
//! It is NOT that the C lane was untested: PMAT-1395 built an executed C→WASM
//! witness that value-matches against the real C compiler. But every probe in
//! that corpus is a LITERAL RETURN, so the corpus could not observe an
//! arithmetic divergence — the gap was in the corpus's SHAPE, not its rigour.
//! The fix is therefore scoped to arithmetic and leaves that path intact.
//!
//! WHAT THIS ASSERTS:
//!
//!   1. C integer ARITHMETIC refuses `--target wasm`, with a message naming the
//!      divergence and redirecting to a backend that has a real C path —
//!      including the CONSTANT-folded division case, which no runtime value
//!      reveals.
//!   2. OVER-REFUSAL CONTROLS, which is where most of this slice's rework went.
//!      The guard is scoped to C `int` ARITHMETIC — not to the C language, and
//!      not to an instruction pattern. PMAT-1395 built a real C scalar-ABI
//!      RETURN path (value-matched against `cc`), and PMAT-1404 pins that the
//!      non-GPU lanes honour C's DECLARED 64-bit widths. So `long f(long a) {
//!      return a + 1; }` must still lower — i64 is the CORRECT width for C
//!      `long` — while `int f(int a)` must not, and division must refuse at
//!      BOTH widths because C `/` truncates where the helper floors. Two
//!      earlier cuts of this belt failed exactly here and were caught by those
//!      pre-existing witnesses.
//!   3. ANTI-VACUITY / ATTRIBUTION: what now refuses is still ACCEPTED by the C
//!      FRONTEND (`--target rust` emits it). Without this, a frontend that had
//!      simply stopped parsing C would satisfy assertion 1 just as well.
//!   4. POSITIVE CONTROL: Python still emits, and the emitted WAT still
//!      carries the flooring helper — the lowering C was silently receiving.
//!   5. The `Wasm` arm stays ALLOWED: the WAT lift's round-trip fixed point
//!      `emit(lift(emit(M))) == emit(M)` (PMAT-954) still holds byte-for-byte,
//!      so the guard did not over-refuse.
//!   6. EXECUTED DIFFERENTIAL: the ambiguity the guard refuses is real and
//!      still live — `n // 2` at `-7` EXECUTES to `-4` through the WASM lane
//!      while the same source-level `x / 2` in C EXECUTES to `-3` through the
//!      Rust lane. One backend lowering cannot serve both, which is why the
//!      refusal is the honest answer rather than a coercion (PMAT-1395).
//!   7. The guard's `match` carries no `_` arm, so a new `SourceLang` variant
//!      is a COMPILE error rather than a silent inheritance of Python
//!      semantics.
//!
//! Skips with reason when a required tool is absent — never silently green.

use std::path::{Path, PathBuf};
use std::process::Command;

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

/// A per-CALL unique directory. Two probes sharing one directory have produced
/// cross-test clobbering in this repo before.
fn probe_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("xpile-wasm-source-lang-refusal-witness")
        .join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir probe dir");
    dir
}

struct Run {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn transpile(src: &Path, target: &str) -> Run {
    let out = Command::new(xpile_bin())
        .args(["transpile", src.to_str().unwrap(), "--target", target])
        .output()
        .expect("spawn xpile");
    Run {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    }
}

fn tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Tracked C-family sources, derived from git rather than a literal list, so a
/// newly-added `.c` fixture is covered the moment it is committed.
fn tracked_c_sources() -> Vec<PathBuf> {
    let root = repo_root();
    let out = Command::new("git")
        .current_dir(&root)
        .args(["ls-files", "*.c", "*.h"])
        .output()
        .expect("spawn git ls-files");
    assert!(out.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| root.join(l.trim()))
        .filter(|p| p.exists())
        .collect()
}

#[test]
fn c_integer_arithmetic_refuses_the_wasm_target() {
    let fixture = repo_root().join("crates/xpile/tests/fixtures/c_int_arith.c");
    assert!(fixture.exists(), "fixture missing: {}", fixture.display());

    let run = transpile(&fixture, "wasm");
    assert!(
        !run.ok,
        "PMAT-1419: `c_int_arith.c` --target wasm must REFUSE. Its `half`/`poly`/\
         `factorial` lower to the Python integer semantics (i64, and a FLOORING \
         `$__wasm_floordiv_i64` where C `/` truncates), and it exited 0 emitting:\n{}",
        run.stdout
    );
    let msg = format!("{}{}", run.stdout, run.stderr);
    assert!(
        msg.contains("does not lower INTEGER ARITHMETIC"),
        "the refusal must come from the integer-semantics belt, naming the \
         divergence — not incidentally from some unsupported construct, which \
         would leave a C file made only of supported constructs still emitting \
         wrong numbers. Got:\n{msg}"
    );
    assert!(
        msg.contains("--target rust"),
        "the refusal must redirect to a backend with a real C path; got:\n{msg}"
    );
}

#[test]
fn constant_folded_c_division_refuses_even_with_no_runtime_value() {
    // The width divergence needs a runtime value to be observable, so the belt
    // conditions on one. DIVISION does NOT: floor-vs-truncate differs on
    // compile-time constants too. A rule that only looked at value-dependent
    // code would emit `-4` for this at exit 0.
    let dir = probe_dir("constant-division");
    let src = dir.join("k.c");
    std::fs::write(&src, "int f(void) { return -7 / 2; }\n").expect("write c");
    let run = transpile(&src, "wasm");
    assert!(
        !run.ok,
        "`-7 / 2` is -3 in C and -4 under the emitted flooring helper, with no \
         runtime value involved — this must refuse, not emit:\n{}",
        run.stdout
    );
}

#[test]
fn the_c_scalar_return_path_that_pmat_1395_witnessed_still_lowers() {
    // OVER-REFUSAL CONTROL, and the reason this guard is scoped to arithmetic
    // rather than to the C language. PMAT-1395 built a REAL C scalar-ABI return
    // path and pinned it with an executed witness that value-matches the output
    // against `cc` compiling the identical source
    // (`xpile-wasm-codegen/tests/c_scalar_abi_witness.rs`). Refusing all C would
    // have deleted a working, witnessed capability — and every probe in that
    // corpus is a LITERAL return, which is exactly why it never observed the
    // arithmetic divergence this slice fixes.
    for (tag, body) in [
        ("literal", "int f(void) { return 2; }\n"),
        // The `i_neg` probe shape. It lowers via `call $__wasm_mul_i64` (by -1),
        // so an instruction-level rule that ignored constant-ness would refuse
        // it — the emitter's own probe corpus catches that regression.
        ("negated-literal", "int f(void) { return -2; }\n"),
        ("unsigned", "unsigned int f(void) { return 4294967295; }\n"),
    ] {
        let dir = probe_dir(&format!("scalar-return-{tag}"));
        let src = dir.join("s.c");
        std::fs::write(&src, body).expect("write c");
        let run = transpile(&src, "wasm");
        assert!(
            run.ok,
            "PMAT-1419 must not over-refuse: the `{tag}` C scalar-return shape is \
             value-matched against the real C compiler by PMAT-1395 and must keep \
             lowering. Got:\n{}",
            run.stderr
        );
    }
}

#[test]
fn the_belt_is_keyed_on_the_declared_c_width_not_on_the_instruction() {
    // The two discriminating cases, and the reason this check reads DECLARED
    // TYPES rather than only emitted instructions. C `long` is genuinely
    // 64-bit, so i64 is the CORRECT width for it and `a + 1` must still lower
    // — an earlier cut refused it purely because the WAT said
    // `call $__wasm_add_i64`, and `c_long_gpu_width_witness` red, since
    // PMAT-1404's WGSL refusal is only defensible while the non-GPU lanes DO
    // honour the declared 64-bit width. DIVISION still diverges at 64 bits
    // though: C `/` truncates, the emitted helper floors.
    for (tag, body, want_ok) in [
        ("long-add", "long f(long a) { return a + 1; }\n", true),
        ("long-div", "long f(long a) { return a / 2; }\n", false),
        // C `int` is 32-bit carried on i64 — the actual divergence.
        ("int-add", "int f(int a) { return a + 1; }\n", false),
    ] {
        let dir = probe_dir(&format!("width-{tag}"));
        let src = dir.join("w.c");
        std::fs::write(&src, body).expect("write c");
        let run = transpile(&src, "wasm");
        assert_eq!(
            run.ok,
            want_ok,
            "`{tag}` ({}) — C `long` is 64-bit so the lane's i64 is correct for it, \
             while C `int` is 32-bit and wraps, and division truncates at BOTH \
             widths. Got exit_ok={}, stderr:\n{}",
            body.trim(),
            run.ok,
            run.stderr
        );
    }
}

#[test]
fn the_c_frontend_still_accepts_what_the_wasm_backend_now_refuses() {
    // ATTRIBUTION. A backend refusal is only meaningful if the FRONTEND still
    // parses the file — otherwise the refusal assertions above would be
    // satisfied by a C frontend that had simply broken, and the lane would read
    // as "guarded" while actually being dead.
    let fixture = repo_root().join("crates/xpile/tests/fixtures/c_int_arith.c");
    let rust = transpile(&fixture, "rust");
    assert!(
        rust.ok,
        "the C frontend must still ACCEPT what the WASM backend refuses — \
         `--target rust` has a real C path and must emit: {}",
        rust.stderr
    );
    assert!(
        rust.stdout.contains("wrapping_"),
        "the Rust C path is what a correct WASM lowering would have to mirror \
         (narrow to i32, wrapping ops); got:\n{}",
        rust.stdout
    );

    // Corpus sweep: report the partition rather than asserting a count, and
    // assert only that the refusing set is NON-EMPTY — a "these all refuse"
    // claim is free on an empty set.
    let sources = tracked_c_sources();
    assert!(!sources.is_empty(), "no tracked .c/.h source found");
    let mut by_belt = 0usize;
    let mut other_refusal = 0usize;
    let mut emitted = 0usize;
    for src in &sources {
        let run = transpile(src, "wasm");
        if run.ok {
            emitted += 1;
        } else if format!("{}{}", run.stdout, run.stderr)
            .contains("does not lower INTEGER ARITHMETIC")
        {
            by_belt += 1;
        } else {
            other_refusal += 1;
        }
    }
    eprintln!(
        "witness[wasm-c-arith]: of {} tracked C sources — {by_belt} refused BY THE BELT, \
         {other_refusal} refused for other reasons, {emitted} still lower (the PMAT-1395 \
         scalar-ABI shapes)",
        sources.len()
    );
    // ATTRIBUTED, not just counted. An earlier cut asserted `refused > 0`, which
    // stayed GREEN with the belt disabled — tracked C files refuse for unrelated
    // reasons (unsupported constructs, header-only files), so a bare refusal
    // count certifies nothing about the belt reaching the corpus.
    assert!(
        by_belt > 0,
        "no tracked C source refuses via the integer-semantics belt — it is not \
         reaching the corpus it was written for. ({other_refusal} refused for other \
         reasons, which is exactly the confusion this assertion exists to avoid.)"
    );
}

#[test]
fn python_still_emits_the_flooring_lowering_through_the_wasm_backend() {
    // POSITIVE CONTROL for the refusals above: without this, deleting the WASM
    // backend outright would satisfy them. It also pins WHAT C was silently
    // receiving — the Python flooring helper, in the emitted text.
    let dir = probe_dir("python-positive-control");
    let src = dir.join("m.py");
    std::fs::write(&src, "def half(n: int) -> int:\n    return n // 2\n").expect("write py");
    let run = transpile(&src, "wasm");
    assert!(
        run.ok,
        "Python is the surface this backend implements and must still emit; got:\n{}",
        run.stderr
    );
    assert!(
        run.stdout.contains("__wasm_floordiv_i64"),
        "expected the Python flooring helper — the lowering a C source was \
         silently getting. Got:\n{}",
        run.stdout
    );
}

#[test]
fn the_wat_lift_round_trip_fixed_point_still_holds() {
    // The `Wasm` arm stays ALLOWED. The lift's image is this backend's OWN emit,
    // so its semantics are faithful by construction — and a guard that refused
    // it would break PMAT-954's fixed point. Asserted, not assumed, because
    // over-refusing is the natural failure mode of a fix like this one.
    let dir = probe_dir("wat-lift-fixed-point");
    let py = dir.join("m.py");
    std::fs::write(&py, "def sq(n: int) -> int:\n    return n * n\n").expect("write py");
    let first = transpile(&py, "wasm");
    assert!(first.ok, "python emit failed: {}", first.stderr);

    let wat = dir.join("m.wat");
    std::fs::write(&wat, &first.stdout).expect("write wat");
    let second = transpile(&wat, "wasm");
    assert!(
        second.ok,
        "PMAT-954's WAT lift must still reach this backend — a `SourceLang::Wasm` \
         module is the lift's round-trip fixed point, not a foreign language. Got:\n{}",
        second.stderr
    );

    // Compare past the header line, which names the source module.
    let strip = |s: &str| s.lines().skip(1).collect::<Vec<_>>().join("\n");
    assert_eq!(
        strip(&first.stdout),
        strip(&second.stdout),
        "emit(lift(emit(M))) == emit(M) must still hold byte-for-byte"
    );
}

#[test]
fn the_ambiguity_the_guard_refuses_is_executed() {
    // EXECUTED DIFFERENTIAL — this is what makes the finding a wrong VALUE and
    // not merely a wrong label, and what rules out "just coerce it" (PMAT-1395:
    // coercing to make output appear is how a silent wrong answer gets
    // installed). The SAME source-level operation — integer division of -7 by 2
    // — has two different correct answers depending on the source language, and
    // this backend has exactly one lowering for it.
    if !tool_available("wat2wasm") || !tool_available("wasm-interp") {
        eprintln!("SKIP the_ambiguity_the_guard_refuses_is_executed: wabt not present");
        return;
    }
    if !tool_available("rustc") {
        eprintln!("SKIP the_ambiguity_the_guard_refuses_is_executed: rustc not present");
        return;
    }
    let dir = probe_dir("ambiguity-differential");

    // ── Python side: `n // 2` at -7, EXECUTED through the WASM lane. ──
    let py = dir.join("m.py");
    std::fs::write(
        &py,
        "def half(n: int) -> int:\n    return n // 2\n\n\
         def probe() -> int:\n    return half(-7)\n",
    )
    .expect("write py");
    let emit = transpile(&py, "wasm");
    assert!(emit.ok, "python emit failed: {}", emit.stderr);
    let wat = dir.join("m.wat");
    std::fs::write(&wat, &emit.stdout).expect("write wat");
    let wasm = dir.join("m.wasm");
    let asm = Command::new("wat2wasm")
        .args([wat.to_str().unwrap(), "-o", wasm.to_str().unwrap()])
        .output()
        .expect("spawn wat2wasm");
    assert!(
        asm.status.success(),
        "wat2wasm must accept the emitted module — that it DOES is exactly why the \
         C defect was silent: {}",
        String::from_utf8_lossy(&asm.stderr)
    );
    let run = Command::new("wasm-interp")
        .args(["--run-all-exports", wasm.to_str().unwrap()])
        .output()
        .expect("spawn wasm-interp");
    let out = String::from_utf8_lossy(&run.stdout);
    let line = out
        .lines()
        .find(|l| l.trim_start().starts_with("probe()"))
        .unwrap_or_else(|| panic!("no `probe()` export in wasm-interp output:\n{out}"));
    // `wasm-interp` prints integer exports UNSIGNED — reinterpret at the
    // declared i64 width before comparing to a negative expectation.
    let raw: u64 = line
        .rsplit("i64:")
        .next()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or_else(|| panic!("unparsable probe line: {line}"));
    let python_answer = raw as i64;
    assert_eq!(
        python_answer, -4,
        "Python `-7 // 2` FLOORS to -4; this is the lowering the WASM backend \
         implements and the one a C source was silently receiving"
    );

    // ── C side: `x / 2` at -7, EXECUTED through the lane that has a real C path. ──
    let fixture = repo_root().join("crates/xpile/tests/fixtures/c_int_arith.c");
    assert!(fixture.exists(), "fixture missing: {}", fixture.display());
    let rust = transpile(&fixture, "rust");
    assert!(
        rust.ok,
        "the Rust lane has a real C path and must emit: {}",
        rust.stderr
    );
    let mut program = rust.stdout.clone();
    program.push_str("fn main() { println!(\"{}\", half(-7)); }\n");
    let rs = dir.join("prog.rs");
    std::fs::write(&rs, &program).expect("write rs");
    let bin = dir.join("prog");
    let build = Command::new("rustc")
        .args(["-O", "-o", bin.to_str().unwrap(), rs.to_str().unwrap()])
        .output()
        .expect("spawn rustc");
    assert!(
        build.status.success(),
        "xpile's C→Rust emit must compile: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let got = Command::new(&bin).output().expect("run compiled C emit");
    let c_answer: i64 = String::from_utf8_lossy(&got.stdout)
        .trim()
        .parse()
        .expect("parse C answer");
    assert_eq!(
        c_answer, -3,
        "C `-7 / 2` TRUNCATES toward zero to -3 (`contracts/c-int-arith-v1.yaml`)"
    );

    assert_ne!(
        python_answer, c_answer,
        "the two languages must still disagree — if they ever agree, the premise of \
         this guard has changed and the refusal should be revisited rather than left \
         standing on a stale measurement"
    );
}

#[test]
fn the_wasm_source_language_guard_has_no_wildcard_arm() {
    // STRUCTURAL. `C` reached this backend because nothing forced a decision
    // when a `SourceLang` variant was added. The guard's match is exhaustive, so
    // the COMPILER now forces that decision — but only for as long as nobody
    // adds a `_` arm, which would silently restore the original defect for every
    // future language. Checked on shape, not on a count.
    let src = repo_root().join("crates/xpile-wasm-codegen/src/lib.rs");
    let text = std::fs::read_to_string(&src).expect("read wasm codegen source");
    let start = text
        .find("fn reject_unsupported_source_lang")
        .expect("guard function must exist — it is what this whole witness pins");
    let body = &text[start..];
    let end = body
        .find("\nfn check_module_binding_names")
        .unwrap_or(body.len());
    let body = &body[..end];
    assert!(
        !body.contains("_ =>"),
        "PMAT-1419: `reject_unsupported_source_lang` must stay exhaustive over \
         SourceLang with no `_` arm, so that adding a variant is a COMPILE error \
         rather than a silent inheritance of Python integer semantics. Found a \
         wildcard arm in:\n{body}"
    );
}
