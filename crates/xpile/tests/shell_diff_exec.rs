//! Shell-side differential-execution gate (PMAT-043 /
//! XPILE-BASHRS-MERGER-001).
//!
//! This is the bashrs-domain twin of `tests/diff_exec.rs`. For each
//! fixture in this gate's curated set, two pipelines must produce
//! identical stdout:
//!
//!   1. CPython: `exec(open(file).read()); demo()` — the function's
//!      `subprocess.run([...])` calls fire and their stdout flows
//!      through.
//!
//!   2. Shell:   `xpile transpile file --target shell | /bin/sh` —
//!      depyler-frontend recognises each `subprocess.run` and lowers
//!      it to a `Stmt::Cmd`; bashrs-backend emits real POSIX sh;
//!      `/bin/sh` executes the same commands directly.
//!
//! Architectural role: this gate is the **Runtime stratum witness**
//! for `C-BASHRS-POSIX-IDEMPOTENCE`. Pre-PMAT-043 the contract
//! showed UNVERIFIED on `xpile quorum` (no stratum witnesses). Post-
//! this PR, Runtime ≥1 because this gate observes the bashrs-
//! emitted shell actually executing and producing output equivalent
//! to the Python source.
//!
//! Skip behaviour: if `python3` or `/bin/sh` is missing from PATH,
//! the test prints a warning and exits OK — same posture as
//! `tests/diff_exec.rs`.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_xpile"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn have_python_and_sh() -> bool {
    let py = Command::new("python3").arg("--version").output().is_ok();
    let sh = Command::new("/bin/sh")
        .arg("-c")
        .arg("true")
        .output()
        .is_ok();
    py && sh
}

/// Run the Python source via CPython by `exec`ing the file and then
/// invoking the named entry function. Returns stdout trimmed of
/// trailing whitespace (so a stray newline at the end doesn't trip
/// the byte-for-byte compare).
fn run_cpython(fixture_path: &std::path::Path, entry: &str) -> Result<String, String> {
    let src = fixture_path.to_str().ok_or("non-utf8 fixture path")?;
    // The fixture intentionally omits `import subprocess` because
    // depyler-frontend only accepts `def` + `from __future__ import
    // annotations` at top level. We inject the import on the
    // CPython side so the fixture's subprocess.run calls resolve.
    // The shell side doesn't need any import — bashrs-backend
    // emits the commands as bare shell statements.
    //
    // Note: we deliberately discard the return value of `entry()` —
    // we only care about its side-effect stdout. Matches the shell
    // side's behaviour (shell scripts produce stdout, not return
    // values to the calling process).
    let prog = format!("import subprocess; exec(open(r'{src}').read()); {entry}()");
    let out = Command::new("python3")
        .args(["-c", &prog])
        .output()
        .map_err(|e| format!("spawn python3: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "python3 exited non-zero: stderr={}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// Transpile the Python source to shell and pipe through /bin/sh.
/// Captures stdout the same way as `run_cpython`.
fn run_shell(fixture_path: &std::path::Path) -> Result<String, String> {
    // Stage 1: xpile transpile <file> --target shell → stdout = sh source
    let transpile = Command::new(bin())
        .args([
            "transpile",
            fixture_path.to_str().unwrap(),
            "--target",
            "shell",
        ])
        .output()
        .map_err(|e| format!("spawn xpile: {e}"))?;
    if !transpile.status.success() {
        return Err(format!(
            "xpile transpile failed: stderr={}",
            String::from_utf8_lossy(&transpile.stderr)
        ));
    }
    let shell_source = String::from_utf8_lossy(&transpile.stdout).to_string();

    // Stage 2: /bin/sh -c <shell-source> → stdout = run result
    let run = Command::new("/bin/sh")
        .arg("-c")
        .arg(&shell_source)
        .output()
        .map_err(|e| format!("spawn /bin/sh: {e}"))?;
    if !run.status.success() {
        return Err(format!(
            "/bin/sh exited non-zero (script ran but commands failed?):\n\
             === stderr ===\n{}\n\
             === transpiled source ===\n{shell_source}",
            String::from_utf8_lossy(&run.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&run.stdout).trim_end().to_string())
}

/// PMAT-052 expected output of `bashrs_realistic_demo.sh` after
/// transpilation + /bin/sh execution. Byte-for-byte deterministic.
const REALISTIC_DEMO_EXPECTED: &str = "hello world\nhow are you\nHi, Noah Gift\nstarted zero done";

/// PMAT-1268 expected output of `bashrs_for_loop_demo.sh` after
/// transpilation + /bin/sh execution. Byte-for-byte deterministic:
/// the single-line loop prints `item 1..3`, the multi-line loop
/// prints `hello`/`bye` for `alice` then `bob`.
const FOR_LOOP_DEMO_EXPECTED: &str =
    "item 1\nitem 2\nitem 3\nhello alice\nbye alice\nhello bob\nbye bob";

/// PMAT-1276 expected output of `bashrs_while_loop_demo.sh` after
/// transpilation + /bin/sh execution. The `while` counts up (3 ticks),
/// the `until` counts down (3 lines). Byte-for-byte deterministic.
const WHILE_LOOP_DEMO_EXPECTED: &str = "tick 0\ntick 1\ntick 2\ndown 3\ndown 2\ndown 1";

/// PMAT-1281 expected output of `bashrs_nested_loop_demo.sh`. The
/// for-in-for block prints the 4 `cell` cells; the while-wrapping-for
/// block prints the 4 `mix` cells. Byte-for-byte deterministic.
const NESTED_LOOP_DEMO_EXPECTED: &str = "cell 1 a\ncell 1 b\ncell 2 a\ncell 2 b\n\
     mix 2 x\nmix 2 y\nmix 1 x\nmix 1 y";

/// PMAT-1283/1284 expected output of `bashrs_if_demo.sh`: the if/then/fi
/// prints `big` (true); the if/then/else/fi takes the else (`not-three`);
/// the if-inside-for picks `i == 2`; the elif chain takes its second arm
/// (`grade-b`). Byte-for-byte deterministic.
const IF_DEMO_EXPECTED: &str = "big\nnot-three\npicked 2\ngrade-b";

/// PMAT-1285 expected output of `bashrs_case_demo.sh`: `fruit=c` matches
/// the `b|c)` multi-pattern arm (`bee-or-cee`); `mode=go` runs the
/// nested loop in the `go)` arm (`step 1`/`step 2`). Byte-for-byte
/// deterministic.
const CASE_DEMO_EXPECTED: &str = "bee-or-cee\nstep 1\nstep 2";

/// PMAT-1377 expected output of `bashrs_arith_shift_demo.sh`: `1 << 2` and
/// the tight `1<<2` are both 4; `x=$((3<<3))` is 24; `8 >> 1` is 4; the
/// quoted `<<` stays literal text. Byte-for-byte deterministic.
const ARITH_SHIFT_DEMO_EXPECTED: &str = "4\n4\n24\n4\na << b";

/// PMAT-1377: the here-doc spellings that walked PAST the PMAT-1371 guard.
/// Each is a real here-document; each MUST refuse. `(label, source)`.
///
/// The guard shipped at PMAT-1371 matched a `Bare` token that *starts with*
/// `<<`, which is only the space-separated `cat <<EOF` spelling. The v0.1.0
/// tokenizer splits on WHITESPACE alone — it does not split an operator off
/// a word — so anything glued to the operator hid it inside a longer token.
const HEREDOC_EVASION_SPELLINGS: &[(&str, &str)] = &[
    // No space between the command word and the operator.
    (
        "attached `cat<<EOF`",
        "cat<<EOF\n  keep  me\n\nafter blank\nEOF\n",
    ),
    // An explicit file descriptor number in front of the operator.
    (
        "fd-prefixed `cat 0<<EOF`",
        "cat 0<<EOF\n  keep  me\n\nafter blank\nEOF\n",
    ),
    ("fd-prefixed `cat 1<<EOF`", "cat 1<<EOF\nx\nEOF\n"),
    // Both at once, with the tab-stripping `<<-` variant.
    ("attached tab-strip `cat<<-EOF`", "cat<<-EOF\n\tx\n\tEOF\n"),
    // Nested in a loop body — a DIFFERENT parser route (`parse_segment_seq`),
    // and a worse failure: `indent_body` tab-prefixed the terminator, so the
    // emitted script did not even parse.
    (
        "attached, nested in a for body",
        "for f in a b; do\n  cat<<EOF\n  x  y\nEOF\ndone\n",
    ),
    (
        "fd-prefixed, nested in an if body",
        "if true; then\n  cat 0<<EOF\n  x  y\nEOF\nfi\n",
    ),
];

#[test]
fn shell_diff_demo_realistic_shell_input_round_trip() {
    // PMAT-052: a `.sh` fixture that exercises every Layer B
    // construct flows through bashrs-frontend → bashrs-backend →
    // /bin/sh and produces the deterministic expected output.
    //
    // This is the bashrs-side analogue of
    // `shell_diff_demo_cpython_vs_bashrs_emit_agree` — that test
    // verifies CPython ≡ bashrs-emit on the Python fixture; this
    // test verifies the bashrs lane works end-to-end without going
    // through Python at all.
    if !have_python_and_sh() {
        eprintln!(
            "warning: skipping PMAT-052 — /bin/sh not on PATH. CI environments \
             with /bin/sh will still run this gate."
        );
        return;
    }
    let sh_path = fixture("bashrs_realistic_demo.sh");
    let actual = run_shell(&sh_path).expect("shell run");
    assert_eq!(
        actual, REALISTIC_DEMO_EXPECTED,
        "bashrs realistic demo output diverged. The transpiled .sh \
         emit produced a different stdout than expected. Likely cause: \
         one of the Layer B parser / renderer paths regressed.\n\
         === expected ===\n{REALISTIC_DEMO_EXPECTED}\n\
         === actual  ===\n{actual}"
    );
}

#[test]
fn shell_diff_demo_for_loop_round_trip() {
    // PMAT-1268: a `.sh` fixture containing real `for` loops flows
    // through bashrs-frontend's new loop parser → mHIR
    // (`Stmt::ShellLoop` / `LoopKind::For`) → bashrs-backend
    // (`render_shell_loop`) → /bin/sh, producing deterministic
    // output. This is the FIRST shell control-flow construct to
    // round-trip end-to-end in xpile.
    //
    // Load-bearing anti-regression: before PMAT-1268 the frontend
    // REFUSED every loop (PMAT-989 — refuse rather than shred into
    // barewords). A real execution witness (the loop body actually
    // runs and its stdout is compared) is what proves the loop is
    // parsed + emitted faithfully, not silently dropped — the exact
    // failure mode (`do : # pending` placeholder that ate the body)
    // that motivated the refusal.
    if !have_python_and_sh() {
        eprintln!(
            "warning: skipping PMAT-1268 for-loop round-trip — /bin/sh not on PATH. \
             CI environments with /bin/sh will still run this gate."
        );
        return;
    }
    let sh_path = fixture("bashrs_for_loop_demo.sh");
    let actual = run_shell(&sh_path).expect("shell run");
    assert_eq!(
        actual, FOR_LOOP_DEMO_EXPECTED,
        "bashrs for-loop demo output diverged. The frontend loop parser or the \
         backend loop renderer regressed (or the loop body was dropped/shredded).\n\
         === expected ===\n{FOR_LOOP_DEMO_EXPECTED}\n\
         === actual  ===\n{actual}"
    );
    // Anchor the content so a future fixture edit can't make this
    // test pass on empty / degenerate output (a shredded loop would
    // print nothing here).
    assert!(
        actual.contains("item 1") && actual.contains("bye bob"),
        "expected both loops' output present; the loop body may have been dropped: {actual}"
    );
}

#[test]
fn shell_diff_demo_while_until_loop_round_trip() {
    // PMAT-1276: a `.sh` fixture with real `while` and `until` loops
    // flows through bashrs-frontend's loop parser → mHIR
    // (`LoopKind::While` / `Until`) → bashrs-backend → /bin/sh,
    // producing deterministic output. The loop CONDITION round-trips as
    // an opaque LitStr and its `$VAR` refs expand at shell run time.
    //
    // Anti-regression: before PMAT-1276 the frontend REFUSED
    // while/until (only `for` was handled since PMAT-1268). A real
    // execution witness (the loop bodies actually run and their stdout
    // is compared) proves the loops are parsed + emitted faithfully,
    // not shredded. Both loops terminate by construction (count to a
    // bound), so the gate never hangs.
    if !have_python_and_sh() {
        eprintln!(
            "warning: skipping PMAT-1276 while/until round-trip — /bin/sh not on PATH. \
             CI environments with /bin/sh will still run this gate."
        );
        return;
    }
    let sh_path = fixture("bashrs_while_loop_demo.sh");
    let actual = run_shell(&sh_path).expect("shell run");
    assert_eq!(
        actual, WHILE_LOOP_DEMO_EXPECTED,
        "bashrs while/until demo output diverged. The frontend loop parser or the \
         backend loop renderer regressed (or a loop body/condition was dropped).\n\
         === expected ===\n{WHILE_LOOP_DEMO_EXPECTED}\n\
         === actual  ===\n{actual}"
    );
    // Anchor content so a future edit can't make this pass on empty
    // output (a shredded loop would print nothing).
    assert!(
        actual.contains("tick 0") && actual.contains("down 1"),
        "expected both loops' output present; a loop body may have been dropped: {actual}"
    );
}

#[test]
fn shell_diff_demo_nested_loop_round_trip() {
    // PMAT-1281: a `.sh` fixture with NESTED loops (for-in-for +
    // while-wrapping-for) flows through the frontend's recursive loop
    // parser → mHIR (nested `Stmt::ShellLoop`) → bashrs-backend
    // (recursive `render_shell_loop`) → /bin/sh, producing deterministic
    // output. The inner loop bodies actually run, proving the nested
    // shape is emitted faithfully (not flattened or shredded). Both
    // loops terminate by construction, so the gate never hangs.
    if !have_python_and_sh() {
        eprintln!(
            "warning: skipping PMAT-1281 nested-loop round-trip — /bin/sh not on PATH. \
             CI environments with /bin/sh will still run this gate."
        );
        return;
    }
    let sh_path = fixture("bashrs_nested_loop_demo.sh");
    let actual = run_shell(&sh_path).expect("shell run");
    assert_eq!(
        actual, NESTED_LOOP_DEMO_EXPECTED,
        "bashrs nested-loop demo output diverged. The frontend recursive loop parser or \
         the backend recursive renderer regressed (or a nested body was flattened/dropped).\n\
         === expected ===\n{NESTED_LOOP_DEMO_EXPECTED}\n\
         === actual  ===\n{actual}"
    );
    // Anchor both nesting flavours so this can't pass on partial output.
    assert!(
        actual.contains("cell 2 b") && actual.contains("mix 1 y"),
        "expected both nested blocks' inner output; a nested body may have been dropped: {actual}"
    );
}

#[test]
fn shell_diff_demo_if_round_trip() {
    // PMAT-1283: a `.sh` fixture with `if`/`then`/`else`/`fi`
    // conditionals (including an if nested in a for loop) flows through
    // the frontend's `Stmt::ShellIf` parser → mHIR → bashrs-backend
    // (`render_shell_if`) → /bin/sh, producing deterministic output.
    // The taken branches actually run, proving the conditional is
    // emitted faithfully (not flattened, dropped, or shredded).
    if !have_python_and_sh() {
        eprintln!(
            "warning: skipping PMAT-1283 if round-trip — /bin/sh not on PATH. \
             CI environments with /bin/sh will still run this gate."
        );
        return;
    }
    let sh_path = fixture("bashrs_if_demo.sh");
    let actual = run_shell(&sh_path).expect("shell run");
    assert_eq!(
        actual, IF_DEMO_EXPECTED,
        "bashrs if demo output diverged. The frontend if-parser or the backend \
         render_shell_if regressed (or a branch was mis-taken/dropped).\n\
         === expected ===\n{IF_DEMO_EXPECTED}\n\
         === actual  ===\n{actual}"
    );
    // Anchor the else-arm, nested-if, and elif-chain outputs so this
    // can't pass on partial output.
    assert!(
        actual.contains("not-three")
            && actual.contains("picked 2")
            && actual.contains("grade-b"),
        "expected the else-arm, nested-if, and elif outputs; a branch may have been dropped: {actual}"
    );
}

#[test]
fn shell_diff_demo_case_round_trip() {
    // PMAT-1285: a `.sh` fixture with `case … in … esac` (single- and
    // multi-pattern arms, `*` default, and a nested loop in an arm)
    // flows through the frontend's `Stmt::ShellCase` parser → mHIR →
    // bashrs-backend (`render_shell_case`) → /bin/sh, producing
    // deterministic output. The matched arm's body actually runs,
    // proving the case is emitted faithfully (not flattened/dropped).
    if !have_python_and_sh() {
        eprintln!(
            "warning: skipping PMAT-1285 case round-trip — /bin/sh not on PATH. \
             CI environments with /bin/sh will still run this gate."
        );
        return;
    }
    let sh_path = fixture("bashrs_case_demo.sh");
    let actual = run_shell(&sh_path).expect("shell run");
    assert_eq!(
        actual, CASE_DEMO_EXPECTED,
        "bashrs case demo output diverged. The frontend case parser or the backend \
         render_shell_case regressed (or an arm was mis-matched/dropped).\n\
         === expected ===\n{CASE_DEMO_EXPECTED}\n\
         === actual  ===\n{actual}"
    );
    // Anchor the multi-pattern match + nested-loop-arm output.
    assert!(
        actual.contains("bee-or-cee") && actual.contains("step 2"),
        "expected the multi-pattern arm and nested-loop-arm output: {actual}"
    );
}

#[test]
fn shell_diff_demo_cpython_vs_bashrs_emit_agree() {
    if !have_python_and_sh() {
        eprintln!(
            "warning: skipping PMAT-043 shell_diff_exec — python3 and/or /bin/sh \
             not on PATH. CI environments with both will still run this gate."
        );
        return;
    }
    let py_path = fixture("bashrs_diff_demo.py");
    let py_out = run_cpython(&py_path, "demo").expect("CPython run");
    let sh_out = run_shell(&py_path).expect("shell run");
    assert_eq!(
        py_out, sh_out,
        "CPython and bashrs-emitted shell diverge on bashrs_diff_demo.py:\n\
         === CPython ===\n{py_out}\n\
         === Shell  ===\n{sh_out}\n\
         The Runtime-stratum witness for C-BASHRS-POSIX-IDEMPOTENCE has \
         broken. Either depyler-frontend's subprocess.run lowering or \
         bashrs-backend's emit changed in a way that no longer matches \
         the Python observable behaviour."
    );
    // Anchor the *content* too so a future change to the fixture
    // can't accidentally make this test pass on no-output input.
    assert!(
        py_out.contains("starting") && py_out.contains("done"),
        "expected `starting` and `done` lines in output; got: {py_out}"
    );
}

// ---------------------------------------------------------------------------
// PMAT-1377 — the here-doc refusal actually covers the here-docs people write.
// ---------------------------------------------------------------------------

/// EXECUTION witness that the arithmetic LEFT SHIFT survives the widened
/// here-doc guard.
///
/// PMAT-1377 changed the operator scan from `starts_with("<<")` to
/// `contains("<<")`. PMAT-090 captures a whole `$((…))` arithmetic expansion
/// as ONE `Bare` token, so that widening put every left shift in the blast
/// radius — a naive `contains` would have refused `$((1 << 2))` outright.
///
/// This is asserted by RUNNING the round-tripped script, not by inspecting the
/// emitted text: an emit-only check would still pass if the operator were
/// mangled into something the shell then evaluated differently.
#[test]
fn shell_diff_demo_arith_shift_round_trip() {
    if !have_python_and_sh() {
        eprintln!(
            "warning: skipping PMAT-1377 arith-shift round-trip — /bin/sh not on \
             PATH. CI environments with /bin/sh will still run this gate."
        );
        return;
    }
    let sh_path = fixture("bashrs_arith_shift_demo.sh");
    let actual = run_shell(&sh_path).expect("shell run");
    assert_eq!(
        actual, ARITH_SHIFT_DEMO_EXPECTED,
        "bashrs arithmetic-shift demo output diverged. Either the widened \
         here-doc guard is now over-refusing `$((…))`, or the shift operator \
         stopped round-tripping verbatim.\n\
         === expected ===\n{ARITH_SHIFT_DEMO_EXPECTED}\n\
         === actual  ===\n{actual}"
    );
    // Anti-vacuity: pin the computed values, so a fixture that silently
    // stopped evaluating its arithmetic cannot pass on degenerate output.
    assert!(
        actual.contains("24") && actual.contains("a << b"),
        "expected the assignment-form shift (24) and the quoted literal to \
         survive; got: {actual}"
    );
}

/// Every here-doc spelling that walked PAST the PMAT-1371 guard now refuses —
/// asserted through the SHIPPED CLI, so it pins the binary a user runs rather
/// than a library entry point.
///
/// Before this slice each of these exited **0**. Flat, the emitted script
/// passed `bash -n` clean and executed DIFFERENTLY (a body of `"  keep  me"` /
/// `""` / `"after blank"` came back as `"keep me"` / `"after blank"` — leading
/// and internal whitespace collapsed, blank line deleted). Nested in a
/// `for`/`if` body it was worse: the terminator was tab-prefixed, so the
/// emitted script did not parse at all.
#[test]
fn shell_heredoc_evasion_spellings_all_refuse() {
    let dir = std::env::temp_dir().join(format!("xpile-heredoc-evasion-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    for (i, (label, src)) in HEREDOC_EVASION_SPELLINGS.iter().enumerate() {
        let path = dir.join(format!("evade{i}.sh"));
        std::fs::write(&path, src).expect("write fixture");
        let out = Command::new(bin())
            .args(["transpile", path.to_str().unwrap(), "--target", "shell"])
            .output()
            .expect("spawn xpile");
        assert!(
            !out.status.success(),
            "{label} must be REFUSED, but `xpile transpile` exited 0 and emitted:\n{}",
            String::from_utf8_lossy(&out.stdout)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("here-document"),
            "{label} must refuse AS A HERE-DOCUMENT (a generic parse failure would \
             not tell the user what to do), got: {stderr}"
        );
    }
}

/// The guard must not have become "refuse anything containing `<<`". Each of
/// these carries a `<<` that is NOT a redirection and must still transpile.
/// This is the RED half of the widening: it is what a `line.contains("<<")`
/// implementation would fail.
#[test]
fn shell_non_redirection_double_angle_still_transpiles() {
    let dir = std::env::temp_dir().join(format!("xpile-heredoc-ok-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    for (i, (label, src)) in [
        ("double-quoted `<<`", "echo \"a << b\"\n"),
        ("single-quoted `<<`", "echo 'a << b'\n"),
        ("spaced arithmetic shift", "echo $((1 << 2))\n"),
        ("tight arithmetic shift", "echo $((1<<2))\n"),
        ("assignment-form shift", "x=$((1<<2))\necho $x\n"),
        ("escaped angles", "echo a\\<\\<b\n"),
    ]
    .iter()
    .enumerate()
    {
        let path = dir.join(format!("ok{i}.sh"));
        std::fs::write(&path, src).expect("write fixture");
        let out = Command::new(bin())
            .args(["transpile", path.to_str().unwrap(), "--target", "shell"])
            .output()
            .expect("spawn xpile");
        assert!(
            out.status.success(),
            "{label} is NOT a here-document and must still transpile; the widened \
             guard is over-refusing. stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
