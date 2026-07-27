//! PMAT-1383 (XPILE-SHELLX-001): the Python→shell cross-domain lane emitted a
//! script that RAN DIFFERENTLY from its source, and exited 0 while doing it.
//!
//! `BashrsBackend::lower` walked each function body through a `filter` that
//! kept the six renderable `Stmt` kinds — `Cmd` / `Pipeline` / `ShellAssign` /
//! `ShellLoop` / `ShellIf` / `ShellCase` — and DISCARDED the other 35 without a
//! word. The bashrs *frontend* only ever produces renderable statements, so the
//! shell→shell round-trip never saw it; the hole was reachable only from the
//! Python direction, which is exactly the lane `CLAUDE.md`'s workflow item 3
//! advertises (`xpile transpile foo.py --target shell`).
//!
//! SIX EXECUTION-WITNESSED DIVERGENCES, measured through the shipped CLI
//! against live CPython before the fix, not asserted:
//!
//! | source                                  | `python3` | emitted `sh`   |
//! |-----------------------------------------|-----------|----------------|
//! | `print("hello")`                        | `hello`   | *(empty)*      |
//! | `run(a); x = 5; print(x); run(b)`       | `a b 5`   | `a b`          |
//! | `run(before); if 1 < 2: run(guarded)`   | 3 lines   | 2 lines        |
//! | `while i < 2: run(loop)`                | `loop`×2  | *(empty)*      |
//! | `for i in range(2): run(iter)`          | `iter`×2  | *(empty)*      |
//! | `run(before); raise ValueError(...)`    | rc=1      | rc=0           |
//!
//! Every one exited 0. The `if` case is the sharpest: the guarded command
//! disappeared while its unguarded siblings emitted, so the script LOOKED like
//! a faithful translation and silently erased the condition. The loop cases
//! dropped the loop whole, body included, leaving a script whose only content
//! was a comment reading `# (no commands — empty script or parse produced 0
//! Stmt::Cmd)` — itself false, since the parse produced statements.
//!
//! A SECOND, DISJOINT DROP PATH. meta-HIR carries a function's return value as
//! `Block::trailing_return`, an `Expr` OUTSIDE `stmts`, so a walk over the
//! statement list alone leaves it unchecked — and `return 1 + 2` was discarded
//! the same silent way. It is now refused unless it is an integer literal (kept
//! because both canonical cross-domain fixtures end in `return 0`, and
//! DISCLOSED in the emitted script because a script's exit status is its last
//! command's, not that integer).
//!
//! The load-bearing test is [`every_accepted_emit_executes_like_cpython`]: it
//! asserts the PROPERTY `exit 0 ==> the script passes `sh -n` AND its stdout and
//! exit status byte-match CPython` over the WHOLE corpus — the refused sources
//! included. Pinning per-shape refusal messages cannot catch the next shape that
//! leaks, and if a future change starts accepting one of the refused shapes this
//! property immediately checks that it EXECUTES correctly instead of quietly
//! going green. The executing half is not decorative: every one of the scripts
//! above is valid POSIX, so a syntax-only witness stays green through the entire
//! defect.
//!
//! Gated on `sh` + `python3` presence (skips with a reason). `XPILE_REQUIRE_SH=1`
//! turns a missing tool into a FAILURE rather than a skip, so the witness cannot
//! decay to skip-green in CI (PMAT-1375 TRAP 4).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use depyler_frontend::PythonFrontend;
use xpile_backend::{BackendConfig, BackendError, Profile, Target};
use xpile_frontend::Frontend;

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

fn tool_present(tool: &str, flag: &str) -> bool {
    Command::new(tool)
        .arg(flag)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// PMAT-1375 TRAP 4: scoped to its own env var, never `CI=true` — a runner
/// without `python3` must still be able to skip by design.
fn require_sh() -> bool {
    std::env::var("XPILE_REQUIRE_SH").is_ok_and(|v| v == "1")
}

/// Both halves of the differential need a shell and a CPython.
///
/// The probe is `sh -c true`, NOT `sh --version`: POSIX `sh` has no
/// `--version` (dash exits 2, and so does a bare `sh -c` with no operand).
/// The first draft of this witness used one of those and skipped GREEN on a
/// machine with a perfectly good `/bin/sh` — the exact skip-as-green failure
/// XPILE-WITNESS-002 exists to kill, reproduced inside the witness written to
/// prevent it. Verified by running with `--nocapture` and reading for the SKIP
/// line rather than trusting the green.
fn toolchain_ready() -> bool {
    let ok = Command::new("sh")
        .args(["-c", "true"])
        .output()
        .is_ok_and(|o| o.status.success())
        && tool_present("python3", "--version");
    if !ok {
        assert!(
            !require_sh(),
            "XPILE_REQUIRE_SH=1 but `sh` and/or `python3` is unavailable — \
             refusing to skip green"
        );
        eprintln!("SKIP: `sh` and/or `python3` unavailable — shell differential skipped");
    }
    ok
}

/// A per-CALL unique scratch directory. Keying only on (tag, pid) is not
/// enough: several calls share a tag and the tests run on parallel threads, so
/// a shared directory gets wiped mid-run and the failure reads like an emitter
/// defect (the lesson the WASM multi-exec witnesses recorded).
fn scratch(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join("xpile-shellx").join(format!(
        "{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// One corpus row: a tag, the Python source, and whether the shell lane is
/// expected to emit it today. `expect_emit == false` rows are the witnessed
/// divergences — they must REFUSE, and the property test still checks them in
/// case a future change starts accepting one.
struct Case {
    tag: &'static str,
    src: &'static str,
    expect_emit: bool,
}

const fn c(tag: &'static str, src: &'static str, expect_emit: bool) -> Case {
    Case {
        tag,
        src,
        expect_emit,
    }
}

/// Every source defines `build()` and every observable effect is a
/// `subprocess.run(["echo", ...])`, so CPython's stdout and the emitted
/// script's stdout are directly comparable. `ls` / `pwd` are deliberately
/// absent — their output depends on the working directory, which would make a
/// divergence unattributable.
const CORPUS: &[Case] = &[
    // ---- accepted: the straight-line command sequence the lane models ----
    c(
        "single_cmd",
        "import subprocess\ndef build() -> None:\n    subprocess.run([\"echo\", \"one\"])\n",
        true,
    ),
    c(
        "multi_cmd",
        "import subprocess\ndef build() -> None:\n    subprocess.run([\"echo\", \"a\"])\n    \
         subprocess.run([\"echo\", \"b\"])\n    subprocess.run([\"echo\", \"c\"])\n",
        true,
    ),
    // The canonical cross-domain shape: `subprocess_demo.py` and
    // `bashrs_diff_demo.py` both end in `return 0`. Accepted, and the ignored
    // return value is disclosed in the emitted script.
    c(
        "cmd_then_return_zero",
        "import subprocess\ndef build() -> int:\n    subprocess.run([\"echo\", \"start\"])\n    \
         subprocess.run([\"echo\", \"end\"])\n    return 0\n",
        true,
    ),
    c(
        "cmd_with_flag_args",
        "import subprocess\ndef build() -> int:\n    subprocess.run([\"echo\", \"-n\", \"x\"])\n    \
         subprocess.run([\"echo\", \"y\"])\n    return 0\n",
        true,
    ),
    // ---- refused: each row was a silent wrong answer through v0.1.617 ----
    c(
        "print_only",
        "def build() -> None:\n    print(\"hello\")\n",
        false,
    ),
    c(
        "assign_between_cmds",
        "import subprocess\ndef build() -> None:\n    subprocess.run([\"echo\", \"a\"])\n    \
         x = 5\n    print(x)\n    subprocess.run([\"echo\", \"b\"])\n",
        false,
    ),
    c(
        "if_guarded_cmd",
        "import subprocess\ndef build() -> None:\n    subprocess.run([\"echo\", \"before\"])\n    \
         if 1 < 2:\n        subprocess.run([\"echo\", \"guarded\"])\n    \
         subprocess.run([\"echo\", \"after\"])\n",
        false,
    ),
    c(
        "while_loop_cmds",
        "import subprocess\ndef build() -> None:\n    i = 0\n    while i < 2:\n        \
         subprocess.run([\"echo\", \"loop\"])\n        i = i + 1\n",
        false,
    ),
    c(
        "for_range_cmds",
        "import subprocess\ndef build() -> None:\n    for i in range(2):\n        \
         subprocess.run([\"echo\", \"iter\"])\n",
        false,
    ),
    c(
        "raise_after_cmd",
        "import subprocess\ndef build() -> None:\n    subprocess.run([\"echo\", \"before\"])\n    \
         raise ValueError(\"boom\")\n",
        false,
    ),
    c(
        "assert_before_cmd",
        "import subprocess\ndef build() -> None:\n    assert 1 == 1\n    \
         subprocess.run([\"echo\", \"ok\"])\n",
        false,
    ),
    // The disjoint `Block::trailing_return` path — a computed return value.
    c(
        "computed_return",
        "import subprocess\ndef build() -> int:\n    subprocess.run([\"echo\", \"x\"])\n    \
         return 1 + 2\n",
        false,
    ),
];

/// Run the shipped CLI. Returns `Some(emitted_shell)` on exit 0.
fn transpile_to_shell(case: &Case) -> Result<String, String> {
    let dir = scratch(case.tag);
    let py = dir.join(format!("{}.py", case.tag));
    std::fs::write(&py, case.src).expect("write py");
    let out = Command::new(xpile_bin())
        .args(["transpile", py.to_str().unwrap(), "--target", "shell"])
        .output()
        .expect("run xpile");
    let _ = std::fs::remove_dir_all(&dir);
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// `(stdout, exit-status)` of `python3 <src>; build()`.
fn cpython_run(case: &Case) -> (String, i32) {
    let dir = scratch(case.tag);
    let py = dir.join("run.py");
    std::fs::write(&py, format!("{}\nbuild()\n", case.src)).expect("write py");
    let out = Command::new("python3")
        .arg(&py)
        .current_dir(&dir)
        .output()
        .expect("run python3");
    let _ = std::fs::remove_dir_all(&dir);
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

/// `(stdout, exit-status)` of `sh <emitted>`.
fn sh_run(tag: &str, shell: &str) -> (String, i32) {
    let dir = scratch(tag);
    let sh = dir.join("emit.sh");
    std::fs::write(&sh, shell).expect("write sh");
    let out = Command::new("sh")
        .arg(&sh)
        .current_dir(&dir)
        .output()
        .expect("run sh");
    let _ = std::fs::remove_dir_all(&dir);
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

/// `sh -n <emitted>` — POSIX syntax check, no execution.
fn sh_syntax_ok(tag: &str, shell: &str) -> Result<(), String> {
    let dir = scratch(tag);
    let sh = dir.join("syntax.sh");
    std::fs::write(&sh, shell).expect("write sh");
    let out = Command::new("sh")
        .args(["-n", sh.to_str().unwrap()])
        .output()
        .expect("run sh -n");
    let _ = std::fs::remove_dir_all(&dir);
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// THE LOAD-BEARING PROPERTY, asserted over the WHOLE corpus rather than the
/// accepted rows: **if the shell lane exits 0, the script it emitted must pass
/// `sh -n` and must execute exactly like CPython** — same stdout, same exit
/// status. Every pre-fix emit passed `sh -n`, so the execution half is what
/// makes this non-vacuous.
#[test]
fn every_accepted_emit_executes_like_cpython() {
    if !toolchain_ready() {
        return;
    }
    let mut checked = 0usize;
    for case in CORPUS {
        let Ok(shell) = transpile_to_shell(case) else {
            continue; // refused — covered by the refusal test below
        };
        sh_syntax_ok(case.tag, &shell).unwrap_or_else(|e| {
            panic!("`{}`: emitted shell fails `sh -n`: {e}\n{shell}", case.tag)
        });

        let (py_out, py_rc) = cpython_run(case);
        let (sh_out, sh_rc) = sh_run(case.tag, &shell);
        assert_eq!(
            py_out, sh_out,
            "`{}`: CPython stdout != emitted-shell stdout\n--- emitted ---\n{shell}",
            case.tag
        );
        assert_eq!(
            py_rc, sh_rc,
            "`{}`: CPython exit status != emitted-shell exit status\n--- emitted ---\n{shell}",
            case.tag
        );
        checked += 1;
    }
    let expected = CORPUS.iter().filter(|c| c.expect_emit).count();
    assert!(
        checked >= expected,
        "only {checked} of {expected} expected-emitting sources actually emitted — \
         the property test is measuring less than the corpus claims"
    );
}

/// The corpus rows that must REFUSE do so, and — critically — they refuse at
/// the BACKEND, not because the frontend stopped reading them. A refusal test
/// that only asserts `Err(_)` goes vacuous the moment an unrelated frontend
/// change starts rejecting the source, so this asserts the frontend still
/// LOWERS each one before pinning the backend stage.
#[test]
fn unrenderable_python_refuses_at_the_backend_stage() {
    let session = xpile_core::default_session();
    let backend = session
        .backends
        .iter()
        .find(|b| b.targets().contains(&Target::Shell))
        .expect("shell backend registered");
    let config = BackendConfig {
        emit_contracts: true,
        target: Target::Shell,
        profile: Profile::RustOut,
        hardware: None,
    };
    let mut refused = 0usize;
    for case in CORPUS.iter().filter(|c| !c.expect_emit) {
        let path = PathBuf::from(format!("{}.py", case.tag));
        let module = PythonFrontend
            .parse_and_lower(Path::new(&path), case.src)
            .unwrap_or_else(|e| {
                panic!(
                    "`{}`: the PYTHON FRONTEND now rejects this source ({e}) — the backend \
                     refusal below would be untested. Move the row or fix the frontend.",
                    case.tag
                )
            });
        match backend.lower(&module, &config) {
            Err(BackendError::Lower(msg)) => {
                assert!(
                    msg.contains("bashrs-backend cannot render"),
                    "`{}`: refused, but not with the shell lane's diagnostic: {msg}",
                    case.tag
                );
                refused += 1;
            }
            Err(other) => panic!(
                "`{}`: refused at the wrong stage — expected BackendError::Lower, got {other:?}",
                case.tag
            ),
            Ok(art) => panic!(
                "`{}`: SILENTLY EMITTED instead of refusing — this is the PMAT-1383 defect:\n{}",
                case.tag, art.primary
            ),
        }
    }
    assert_eq!(
        refused,
        CORPUS.iter().filter(|c| !c.expect_emit).count(),
        "every non-emitting row must have been exercised"
    );
}

/// The diagnostic must name the construct and where it is, so a user can act on
/// it. A refusal that says only "unsupported" moves the silent wrong answer into
/// a silent dead end.
#[test]
fn the_refusal_names_the_construct_and_its_position() {
    let case = CORPUS
        .iter()
        .find(|c| c.tag == "if_guarded_cmd")
        .expect("row present");
    let err = transpile_to_shell(case).expect_err("the `if` row refuses");
    for needle in ["`if`", "statement 2", "function `build`"] {
        assert!(
            err.contains(needle),
            "refusal should name {needle}; got:\n{err}"
        );
    }
    let ret = CORPUS
        .iter()
        .find(|c| c.tag == "computed_return")
        .expect("row present");
    let err = transpile_to_shell(ret).expect_err("the computed-return row refuses");
    assert!(
        err.contains("return value of function `build`"),
        "the trailing_return refusal should name the return value; got:\n{err}"
    );
}

/// An accepted `return <int>` is DISCLOSED rather than silently dropped: the
/// script's exit status is its last command's, not that integer. Emitted as a
/// comment, so it cannot change stdout or kill a `.`-sourcing parent the way an
/// injected `exit` would.
#[test]
fn an_accepted_integer_return_is_disclosed_in_the_emitted_script() {
    let case = CORPUS
        .iter()
        .find(|c| c.tag == "cmd_then_return_zero")
        .expect("row present");
    let shell = transpile_to_shell(case).expect("the canonical shape emits");
    assert!(
        shell.contains("# note: `build` ends in `return 0`, which is NOT modelled"),
        "the ignored return value must be disclosed; got:\n{shell}"
    );
}

/// The shell→shell round-trip must NOT gain that note. bashrs-frontend wraps a
/// script in a SYNTHETIC `main` whose `trailing_return` is a structural
/// `LitInt(0)` — there is no `return` in the user's script at all, so
/// disclosing one would print a claim the source does not make, and would put a
/// spurious line in the round-trip the `C-BASHRS-POSIX-IDEMPOTENCE` contract
/// covers.
#[test]
fn the_shell_lane_gains_no_return_note() {
    if !toolchain_ready() {
        return;
    }
    let dir = scratch("roundtrip");
    let sh = dir.join("rt.sh");
    std::fs::write(&sh, "echo hi\nfor x in a b; do\n  echo $x\ndone\n").expect("write sh");
    let out = Command::new(xpile_bin())
        .args(["transpile", sh.to_str().unwrap(), "--target", "shell"])
        .output()
        .expect("run xpile");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "shell round-trip must still emit");
    let emitted = String::from_utf8_lossy(&out.stdout);
    assert!(
        !emitted.contains("# note:"),
        "the shell lane has no source `return` to disclose; got:\n{emitted}"
    );
    assert!(
        emitted.contains("for x in a b; do"),
        "the round-trip must still carry the loop; got:\n{emitted}"
    );
}
