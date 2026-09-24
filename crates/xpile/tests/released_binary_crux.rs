//! XPILE-CRUX-001 — the known-answer gate on the RELEASED binary (xpile#2131).
//!
//! Every other witness in this crate runs the `xpile` built from this tree.
//! A release ships something else: the bytes `cargo install xpile` produces
//! from crates.io. This gate runs THAT binary, named by `XPILE_BIN`, against
//! a pinned known answer:
//!
//! 1. `$XPILE_BIN --version` must print `xpile <XPILE_CRUX_VERSION>` (default:
//!    this crate's version);
//! 2. the three published examples (`examples/{factorial,gcd,word_count}.py`,
//!    the files the README tells a reader to transpile), plus a fixed
//!    `main()` driver, are transpiled with `--emit-crate`, built with
//!    `cargo`, and run;
//! 3. the program's stdout must equal [`KNOWN_ANSWER`] byte for byte, AND
//!    `python3` running the same source must print the same bytes.
//!
//! Three-way, because each pair alone can be satisfied wrongly: a pinned table
//! alone could be wrong about Python, and a python3 comparison alone would pass
//! if both sides drifted together (e.g. an example edited to something
//! trivial).
//!
//! ```console
//! $ cargo install xpile --version 0.1.618 --locked --root /tmp/x
//! $ XPILE_BIN=/tmp/x/bin/xpile XPILE_CRUX_VERSION=0.1.618 \
//!     cargo test -p xpile --test released_binary_crux -- --ignored
//! ```
//!
//! ## Positive controls (each must go RED)
//!
//! - `XPILE_CRUX_SELF_TEST=flip` flips one line of the pinned answer.
//! - `XPILE_BIN=/bin/echo` is a stub binary: GNU echo answers `--version`
//!   with `echo (GNU coreutils) …`, which is not the expected version.
//!
//! The comparison logic is also exercised by the ordinary (non-ignored) tests
//! at the bottom, so a refactor that makes it vacuous reds in `workspace-test`
//! without anyone installing anything.
//!
//! `#[ignore]` because it needs an installed binary, `python3`, `cargo` and
//! the crates.io index. It never skips once asked to run: an unset
//! `XPILE_BIN` or a missing tool is a panic, not a pass.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What the driver prints, verified against CPython 3 on 2026-09-23.
/// `gcd(-7, 3) == 1` pins Python floor-mod: with Rust's truncating `%` the
/// same loop returns -1. (`gcd(-48, 18)` looks like it would, and does not:
/// both semantics reach 6.)
const KNOWN_ANSWER: &str = "\
2432902008176640000
21
1
the 3
cat 1
and 2
hat 1
bat 1
";

/// The `XPILE_CRUX_SELF_TEST=flip` control: replace this line of the pinned
/// answer with a wrong one.
const FLIP: (&str, &str) = ("2432902008176640000\n", "2432902008176640001\n");

/// Appended to the concatenated examples: a `main()` that calls into each
/// example (`gcd` twice, the second call to pin floor-mod) and prints the
/// results, so the emitted crate is a runnable binary.
const DRIVER: &str = r#"

def main() -> None:
    print(factorial(20))
    print(gcd(1071, 462))
    print(gcd(-7, 3))
    for word, n in word_count("the cat and the hat and the bat").items():
        print(word, n)
"#;

const EXAMPLES: [&str; 3] = [
    "examples/factorial.py",
    "examples/gcd.py",
    "examples/word_count.py",
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

/// `Ok` when `stdout` of `xpile --version` names exactly `expected`.
fn check_version(stdout: &str, expected: &str) -> Result<(), String> {
    let first = stdout.lines().next().unwrap_or("").trim();
    if first == format!("xpile {expected}") {
        Ok(())
    } else {
        Err(format!(
            "`--version` printed {first:?}, expected \"xpile {expected}\""
        ))
    }
}

/// `Ok` when the transpiled program, CPython and the pinned table all agree.
fn check_answers(known: &str, python: &str, xpile: &str) -> Result<(), String> {
    let mut errs = Vec::new();
    if python != known {
        errs.push(format!(
            "python3 disagrees with the pinned answer:\n--- pinned\n{known}--- python3\n{python}"
        ));
    }
    if xpile != known {
        errs.push(format!(
            "the transpiled program disagrees with the pinned answer:\n--- pinned\n{known}--- xpile\n{xpile}"
        ));
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs.join("\n"))
    }
}

/// The pinned answer, with its first line flipped under the self-test.
fn known_answer() -> String {
    match std::env::var("XPILE_CRUX_SELF_TEST").as_deref() {
        Ok("flip") => KNOWN_ANSWER.replacen(FLIP.0, FLIP.1, 1),
        Ok(other) => panic!("XPILE_CRUX_SELF_TEST={other:?}: the only control is `flip`"),
        Err(_) => KNOWN_ANSWER.to_string(),
    }
}

fn run(cmd: &mut Command, what: &str) -> String {
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("XPILE-CRUX-001: could not start {what}: {e}"));
    assert!(
        out.status.success(),
        "XPILE-CRUX-001: {what} failed ({}):\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 stdout")
}

#[test]
#[ignore = "needs an installed release binary in XPILE_BIN, python3 and cargo"]
fn released_binary_matches_the_known_answer() {
    let bin = std::env::var("XPILE_BIN").unwrap_or_else(|_| {
        panic!(
            "XPILE-CRUX-001: set XPILE_BIN to the installed release binary \
             (`cargo install xpile --version <X> --locked --root <dir>`). This \
             gate does not fall back to the tree's own build."
        )
    });
    let expected =
        std::env::var("XPILE_CRUX_VERSION").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").into());

    let version = run(Command::new(&bin).arg("--version"), "xpile --version");
    if let Err(e) = check_version(&version, &expected) {
        panic!("XPILE-CRUX-001: {bin}: {e}");
    }

    let scratch = std::env::temp_dir().join(format!("xpile-crux-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).expect("create scratch dir");

    let mut source = String::new();
    for ex in EXAMPLES {
        source.push_str(&fs::read_to_string(workspace_root().join(ex)).expect("read example"));
        source.push('\n');
    }
    source.push_str(DRIVER);
    let py = scratch.join("crux.py");
    fs::write(&py, &source).expect("write crux.py");

    let python = run(
        Command::new("python3").arg("-c").arg(format!(
            "import runpy; runpy.run_path({:?})['main']()",
            py.display().to_string()
        )),
        "python3",
    );

    let krate = scratch.join("crate");
    run(
        Command::new(&bin)
            .arg("transpile")
            .arg(&py)
            .arg("--emit-crate")
            .arg(&krate),
        "xpile transpile --emit-crate",
    );
    let xpile = run(
        Command::new("cargo")
            .args(["run", "--quiet", "--release"])
            .current_dir(&krate)
            .env("CARGO_TARGET_DIR", scratch.join("target")),
        "cargo run (emitted crate)",
    );

    let result = check_answers(&known_answer(), &python, &xpile);
    let _ = fs::remove_dir_all(&scratch);
    if let Err(e) = result {
        panic!("XPILE-CRUX-001 ({bin}, {expected}): {e}");
    }
    eprintln!("XPILE-CRUX-001: {bin} ({expected}) matches the known answer and CPython");
}

// ── The comparison logic, exercised without an installed binary ──────────

#[test]
fn the_version_check_rejects_a_stub_and_a_wrong_version() {
    assert!(check_version("xpile 0.1.618\n", "0.1.618").is_ok());
    assert!(check_version("xpile 0.1.617\n", "0.1.618").is_err());
    // What `XPILE_BIN=/bin/echo` printed for `--version` (GNU coreutils).
    assert!(check_version("echo (GNU coreutils) 8.32\n", "0.1.618").is_err());
    assert!(check_version("--version\n", "0.1.618").is_err());
    assert!(check_version("", "0.1.618").is_err());
}

#[test]
fn the_answer_check_rejects_each_single_side_drift() {
    let k = KNOWN_ANSWER;
    assert!(check_answers(k, k, k).is_ok());
    let flipped = k.replacen("21\n", "22\n", 1);
    assert!(check_answers(k, &flipped, k).is_err(), "python drift");
    assert!(check_answers(k, k, &flipped).is_err(), "xpile drift");
    // Both drifting together still disagrees with the pinned table.
    assert!(check_answers(k, &flipped, &flipped).is_err(), "joint drift");
}

#[test]
fn the_flip_control_changes_the_pinned_answer() {
    assert!(
        KNOWN_ANSWER.contains(FLIP.0) && FLIP.0 != FLIP.1,
        "the flip control no longer matches a line of the pinned answer, so \
         XPILE_CRUX_SELF_TEST=flip would be a no-op"
    );
}
