//! XPILE-WITNESS (Lean lane) — PMAT-1418: the Lean backend REFUSES every
//! source language it was not written for, instead of emitting Python
//! semantics for it and exiting 0.
//!
//! WHAT WAS WRONG. `xpile-lean-codegen`'s own module doc declares the surface
//! it implements: **Python → Lean**. That mapping is faithful only because
//! Python `int` is unbounded (so Lean `Int` is right) and Python `//` floors
//! (so `Int.fdiv` is right). Nothing enforced the premise. `emit_module` READ
//! `module.source_lang` — to interpolate it into the header comment
//! `-- xpile-generated from C module …` — and then emitted Python semantics
//! regardless. So the backend announced the mismatch in prose, in its own
//! output, and lowered it anyway.
//!
//! Measured at `e617d97c` on `crates/xpile/tests/fixtures/c_int_arith.c`,
//! comparing this backend against xpile's OWN C-honest Rust emit of the very
//! same source file:
//!
//! | expression      | `--target rust` (C semantics) | `--target lean` (before) |
//! |-----------------|-------------------------------|--------------------------|
//! | `half(-7)`      | `-3`  (C `/` truncates)       | `-4`  — `Int.fdiv` FLOORS |
//! | `poly(50000)`   | `-1794867295` (i32 wraps)     | `2500100001` — `Int` is unbounded |
//! | `factorial(13)` | `1932053504` (i32 wraps)      | does not elaborate: `fail to show termination`, so Lean falls back to the **`sorry` axiom** |
//!
//! The first two are SILENT: `lean` exits 0 and prints a different number than
//! the C program computes. The third is worse in kind — the PROOF lane emitted
//! a module resting on `sorry`, in a repo whose standing contract claim is zero
//! real `sorry` and zero `axiom`.
//!
//! And all of it was stamped `@[xpile_contract "C-PY-INT-ARITH"]` — the PYTHON
//! contract — on C code, when `contracts/c-int-arith-v1.yaml:10` says in its own
//! description that the C contract is "Distinct from C-PY-INT-ARITH (Python
//! int)". The Rust and Ruchy backends have real C paths and cite
//! `C-C-INT-ARITH`; Lean has no C path at all.
//!
//! WHY IT SURVIVED, and the sharpest part of this finding. `xpile audit … --target
//! lean` over the 11 tracked `.c` files scored them
//! `{"f1_pct":100.0,"f1_status":"OK"}` — a PERFECT citation score, for citing the
//! wrong contract. The audit asks whether a citation is PRESENT, never whether it
//! is the RIGHT one for the source language, so the wrongest lane in the repo
//! carried the highest score and propped the whole-corpus number up from the
//! honest 84.1% to 85.7%. A metric that cannot distinguish "cited correctly" from
//! "cited at all" reports its best number exactly where the defect is worst.
//!
//! Two languages reached the backend and got Python semantics — `C` (and `.h`)
//! and `Wasm` (the WAT lift). `Shell` is refused here too, and that is NOT
//! redundant with the per-`Stmt` shell refusals in the backend: those guard the
//! STATEMENTS, so an item-less shell module (a comment-only or empty `.sh`) had
//! no statement to refuse, fell through the item loop, and emitted a header-only
//! "Lean module" at exit 0. A negative over an empty enumeration passes for free.
//!
//! WHAT THIS ASSERTS:
//!
//!   1. Every tracked `.c` / `.h` file refuses `--target lean`, with a message
//!      that names the source language and redirects to a backend that has a
//!      real path for it. The corpus is derived from `git ls-files` — no count
//!      is hard-coded anywhere — and is asserted NON-EMPTY first, because a
//!      "everything in this set refuses" claim is free on an empty set.
//!   2. POSITIVE CONTROL: Python still emits. Without this, deleting the whole
//!      backend would satisfy assertion 1.
//!   3. The item-less shell module — the empty-enumeration hole specifically —
//!      refuses rather than emitting a header-only module at exit 0.
//!   4. The WAT lift refuses.
//!   5. EXECUTED DIFFERENTIAL: the C semantics the Lean lane could not honour
//!      are pinned by actually compiling and running xpile's C-honest Rust emit.
//!      This is what makes the finding a wrong VALUE rather than a wrong label.
//!   6. The C lane's citations on the backends that DO have a C path name
//!      `C-C-INT-ARITH` and never `C-PY-INT-ARITH`.
//!   7. The guard's `match` carries no `_` arm, so a new `SourceLang` variant
//!      must make an explicit decision instead of silently inheriting Python
//!      semantics — which is exactly how `C` and `Wasm` arrived.
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
        .join("xpile-lean-source-lang-refusal-witness")
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

/// Tracked C-family sources, derived from git rather than from a literal list,
/// so a newly-added `.c` fixture is covered the moment it is committed.
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
fn every_tracked_c_source_refuses_the_lean_target() {
    let sources = tracked_c_sources();
    // ANTI-VACUITY. "every member of this set refuses" is satisfied for free by
    // an empty set; assert the set is real before believing the negative.
    assert!(
        !sources.is_empty(),
        "no tracked .c/.h source found — the refusal claim below would pass vacuously"
    );

    for src in &sources {
        let run = transpile(src, "lean");
        assert!(
            !run.ok,
            "PMAT-1418: `{}` --target lean must REFUSE (it has no C path and would emit \
             Python integer semantics: unbounded `Int`, and `Int.fdiv` where C `/` \
             truncates). It exited 0 emitting:\n{}",
            src.display(),
            run.stdout
        );
        let msg = format!("{}{}", run.stdout, run.stderr);
        assert!(
            msg.contains("Lean backend does not lower a"),
            "refusal for `{}` must come from the source-language guard, not \
             incidentally from an unsupported construct — otherwise a C file made \
             only of supported constructs would still emit. Got:\n{msg}",
            src.display()
        );
        assert!(
            msg.contains("--target rust"),
            "the refusal must redirect to a backend with a real C path; got:\n{msg}"
        );
    }
}

#[test]
fn python_still_emits_through_the_lean_backend() {
    // POSITIVE CONTROL for the test above: without this, deleting the Lean
    // backend outright would satisfy `every_tracked_c_source_refuses…`.
    let dir = probe_dir("python-positive-control");
    let src = dir.join("m.py");
    std::fs::write(&src, "def sq(n: int) -> int:\n    return n * n\n").expect("write py");
    let run = transpile(&src, "lean");
    assert!(
        run.ok,
        "Python is the surface this backend implements and must still emit; got:\n{}",
        run.stderr
    );
    assert!(
        run.stdout.contains("def sq"),
        "expected a Lean `def`, got:\n{}",
        run.stdout
    );
}

#[test]
fn item_less_shell_module_refuses_instead_of_emitting_an_empty_lean_module() {
    // The empty-enumeration hole, specifically. The backend's per-`Stmt` shell
    // refusals are exhaustive over STATEMENTS, so a shell module with no
    // statements had nothing to refuse: it fell through the item loop and
    // emitted a header-only "Lean module" at exit 0.
    for (tag, body) in [("comment-only", "# just a comment\n"), ("empty", "")] {
        let dir = probe_dir(&format!("shell-{tag}"));
        let src = dir.join("s.sh");
        std::fs::write(&src, body).expect("write sh");
        let run = transpile(&src, "lean");
        assert!(
            !run.ok,
            "PMAT-1418: an item-less ({tag}) shell module must refuse, not emit a \
             header-only Lean module at exit 0. Got:\n{}",
            run.stdout
        );
        assert!(
            run.stderr.contains("Lean backend does not lower a")
                || run.stdout.contains("Lean backend does not lower a"),
            "the refusal must come from the source-language guard; got:\n{}{}",
            run.stdout,
            run.stderr
        );
    }
}

#[test]
fn wasm_lift_refuses_the_lean_target() {
    // The second language that reached the backend and got Python semantics.
    // WASM `i32`/`i64` wrap; Lean `Int` does not. Build the WAT with xpile's own
    // emit so the probe rides the canonical round-trip shape the lift accepts.
    let dir = probe_dir("wasm-lift");
    let py = dir.join("m.py");
    std::fs::write(&py, "def sq(n: int) -> int:\n    return n * n\n").expect("write py");
    let emitted = transpile(&py, "wasm");
    if !emitted.ok {
        eprintln!("SKIP wasm_lift_refuses_the_lean_target: --target wasm did not emit");
        return;
    }
    let wat = dir.join("m.wat");
    std::fs::write(&wat, &emitted.stdout).expect("write wat");

    let lifted = transpile(&wat, "lean");
    assert!(
        !lifted.ok,
        "PMAT-1418: a lifted WASM module must refuse --target lean (i32/i64 WRAP, \
         Lean `Int` does not), got exit 0 emitting:\n{}",
        lifted.stdout
    );
}

#[test]
fn c_integer_semantics_the_lean_lane_could_not_honour_are_executed() {
    // EXECUTED DIFFERENTIAL — this is what makes the finding a wrong VALUE and
    // not merely a wrong label. `--target rust` has a real C path, so it is the
    // oracle for what the C source actually means. The three expressions below
    // are exactly the three the Lean emit got wrong.
    let rustc_ok = Command::new("rustc")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !rustc_ok {
        eprintln!("SKIP c_integer_semantics…: rustc not present");
        return;
    }

    let fixture = repo_root().join("crates/xpile/tests/fixtures/c_int_arith.c");
    assert!(fixture.exists(), "fixture missing: {}", fixture.display());

    // DEFAULT flags deliberately — no `--contracts` argument. The citations are
    // Rust line comments, so they cost this differential nothing, and riding the
    // default keeps the witness on the path users actually take. (An earlier cut
    // passed `--contracts off` and `default_flag_witness.rs` correctly reds that:
    // a witness that runs a flag set nobody ships certifies a path nobody ships,
    // which is how PMAT-1405 slipped past this lane's own semantic oracle.)
    let rust = Command::new(xpile_bin())
        .args(["transpile", fixture.to_str().unwrap(), "--target", "rust"])
        .output()
        .expect("spawn xpile");
    assert!(
        rust.status.success(),
        "the Rust lane has a real C path and must emit: {}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let mut program = String::from_utf8_lossy(&rust.stdout).to_string();
    program.push_str(
        "fn main() {\n\
         \x20   println!(\"{} {} {}\", half(-7), poly(50000), factorial(13));\n\
         }\n",
    );

    let dir = probe_dir("c-semantics-differential");
    let src = dir.join("prog.rs");
    std::fs::write(&src, &program).expect("write rs");
    let bin = dir.join("prog");
    let build = Command::new("rustc")
        .args(["-O", "-o", bin.to_str().unwrap(), src.to_str().unwrap()])
        .output()
        .expect("spawn rustc");
    assert!(
        build.status.success(),
        "xpile's C→Rust emit must compile: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&bin).output().expect("run compiled C emit");
    let got = String::from_utf8_lossy(&run.stdout).trim().to_string();

    // C `/` truncates toward zero: -7/2 == -3. Lean's `Int.fdiv` FLOORS: -4.
    // C `int` is 32 bits and wraps: poly(50000) and factorial(13) both overflow.
    // Lean `Int` is unbounded: 2500100001 and 6227020800.
    assert_eq!(
        got, "-3 -1794867295 1932053504",
        "C integer semantics drifted. These are the values the Lean lane silently \
         disagreed with (-4 / 2500100001 / no-elaboration-via-`sorry`), and the \
         reason `--target lean` now refuses a C source module."
    );
}

#[test]
fn c_sources_cite_the_c_contract_on_backends_that_have_a_c_path() {
    // The wrong-contract class, gated beyond Lean. Rust and Ruchy DO have real C
    // paths, so a C source must cite the C integer contract there and must never
    // carry the Python one — the citation the Lean lane was emitting.
    let fixture = repo_root().join("crates/xpile/tests/fixtures/c_int_arith.c");
    assert!(fixture.exists(), "fixture missing: {}", fixture.display());

    for target in ["rust", "ruchy"] {
        let run = transpile(&fixture, target);
        assert!(
            run.ok,
            "`{target}` has a real C path and must emit: {}",
            run.stderr
        );
        assert!(
            run.stdout.contains("C-C-INT-ARITH"),
            "C source on `{target}` must cite C-C-INT-ARITH, got:\n{}",
            run.stdout
        );
        assert!(
            !run.stdout.contains("C-PY-INT-ARITH"),
            "C source on `{target}` must NEVER cite the PYTHON contract \
             C-PY-INT-ARITH — contracts/c-int-arith-v1.yaml calls the two \
             \"Distinct\". Got:\n{}",
            run.stdout
        );
    }
}

#[test]
fn the_source_language_guard_has_no_wildcard_arm() {
    // STRUCTURAL. `C` and `Wasm` reached this backend because nothing forced a
    // decision when a `SourceLang` variant was added. The guard's match is
    // exhaustive, so the COMPILER now forces that decision — but only for as
    // long as nobody adds a `_` arm, which would silently restore the original
    // defect for every future language. Checked on shape, not on a count.
    let src = repo_root().join("crates/xpile-lean-codegen/src/lib.rs");
    let text = std::fs::read_to_string(&src).expect("read lean codegen source");
    let start = text
        .find("fn reject_non_python_source")
        .expect("guard function must exist — it is what this whole witness pins");
    let body = &text[start..];
    let end = body.find("\npub fn emit_module").unwrap_or(body.len());
    let body = &body[..end];
    assert!(
        !body.contains("_ =>"),
        "PMAT-1418: `reject_non_python_source` must stay exhaustive over SourceLang \
         with no `_` arm, so that adding a variant is a COMPILE error rather than a \
         silent inheritance of Python integer semantics. Found a wildcard arm in:\n{body}"
    );
}
