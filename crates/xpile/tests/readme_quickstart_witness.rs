//! XPILE-README-001 (PMAT-1415) — the README's Quickstart claim about CI,
//! executed.
//!
//! THE DEFECT. `README.md` shows the two-line `factorial.py` from
//! `examples/factorial.py`, prints the `i64` emit it produces — the one whose
//! every arithmetic operation is `checked_*` — and then states:
//!
//! > CI compiles this output with `rustc -O` and asserts
//! > `factorial(10) == 3628800`.
//!
//! CI did not. The only test in the repository asserting that value from an
//! emitted `factorial` is `transpile_e2e.rs::
//! factorial_emitted_rust_computes_correct_values`, and it transpiles
//! `crates/xpile/tests/fixtures/factorial.py` — a DIFFERENT source, annotated
//! `-> BigInt`, whose emit contains **zero** `checked_` calls. Its sibling
//! `bigint_implicit_promotion_factorial_emits_bigint_mode` asserts
//! `!rust.contains("checked_mul")` outright. So the artifact CI compiled shared
//! a file *name* with the README's and nothing else.
//!
//! WHY THAT IS THE EXPENSIVE HALF. The sentence is not decoration; it is the
//! evidence offered for the paragraph above it, whose claim is that an `i64`
//! overflow **panics** rather than wrapping silently. The BigInt test cannot
//! check that even in principle: it compiles against a hand-written
//! `mod xpile_bigint` shim whose `Mul` is a plain `*` on an `i64` field, so
//! under `rustc -O` it *wraps*. The one property the README sells was the one
//! property the cited test structurally could not observe.
//!
//! THE REPAIR IS TO EXECUTE THE CLAIM, NOT TO SOFTEN IT. Measured before this
//! file was written, the README's transcript is *true* — it compiles under
//! `rustc -O`, `factorial(10)` is 3628800, and `factorial(21)` panics naming
//! `C-PY-INT-ARITH`. Nothing was wrong with the emitter. What was wrong was
//! that a reader had no way to tell, because the sentence asserting it was
//! unbacked. So the sentence now names this file, and this file runs it.
//!
//! Everything here is DERIVED FROM `README.md` ITSELF — the Python source and
//! the expected transcript are parsed out of the published fenced blocks, not
//! copied into a constant. A copy would drift the moment either side moved,
//! which is the failure mode being repaired. No count and no emitted line is
//! hard-coded (PMAT-1396's rule).
//!
//! Test 5 pins the *reason* this file exists separately from
//! `transpile_e2e.rs`: the two corpora are genuinely different artifacts. If a
//! later change unifies them, that test reds and says so, rather than leaving
//! two witnesses quietly covering one path.

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_xpile"))
}

/// Workspace root — an integration test's CWD is the PACKAGE root
/// (`crates/xpile`). Same idiom as `packaged_contracts.rs`.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn readme() -> String {
    let p = workspace_root().join("README.md");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// A scratch directory unique per CALL, not per test — two probes in one test
/// must not share a path (the multi-exec landmine).
fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("xpile-readme1415-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap_or_else(|e| panic!("create {}: {e}", d.display()));
    d
}

// ── README extraction ───────────────────────────────────────────────────────

/// Body of the first fenced block whose info string is `lang` and whose body
/// satisfies `pick`. Returns `None` rather than a guess, so a parser that stops
/// matching fails test 1 loudly instead of silently covering nothing.
fn fenced_block(md: &str, lang: &str, pick: impl Fn(&str) -> bool) -> Option<String> {
    let mut in_block = false;
    let mut cur_lang = String::new();
    let mut body = String::new();
    for line in md.lines() {
        if let Some(rest) = line.strip_prefix("```") {
            if in_block {
                if cur_lang == lang && pick(&body) {
                    return Some(body);
                }
                in_block = false;
                body.clear();
            } else {
                in_block = true;
                cur_lang = rest.trim().to_string();
                body.clear();
            }
            continue;
        }
        if in_block {
            body.push_str(line);
            body.push('\n');
        }
    }
    None
}

/// The command line the Quickstart transcript block opens with. The transcript
/// is everything after it.
const QUICKSTART_CMD: &str = "$ xpile transpile factorial.py";

fn quickstart_python(md: &str) -> String {
    fenced_block(md, "python", |b| b.contains("def factorial"))
        .expect("README.md has a ```python block defining `factorial` (the Quickstart source)")
}

fn quickstart_transcript(md: &str) -> String {
    let block = fenced_block(md, "bash", |b| {
        b.lines().next().map(str::trim) == Some(QUICKSTART_CMD)
    })
    .unwrap_or_else(|| {
        panic!("README.md has a ```bash block whose first line is exactly `{QUICKSTART_CMD}`")
    });
    block
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

// ── elision-aware transcript comparison ─────────────────────────────────────

/// The marker a README transcript uses to stand for elided output.
const ELISION: char = '…';

fn strip_ws(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Does `shown` (a README transcript, possibly containing `…`) faithfully
/// describe `real` (the binary's actual stdout)?
///
/// Whitespace is ignored on both sides — a README reflows one very long emitted
/// line for readability, and that is the only cosmetic licence taken. Every
/// other byte must appear, in order. With no `…` present this degenerates to
/// equality-modulo-whitespace, which is what the Quickstart block wants: it
/// publishes the emit verbatim.
fn elision_match(shown: &str, real: &str) -> Result<(), String> {
    let shown_n = strip_ws(shown);
    let real_n = strip_ws(real);
    let segs: Vec<&str> = shown_n.split(ELISION).collect();
    let last = segs.len() - 1;
    let mut pos = 0usize;

    for (i, seg) in segs.iter().enumerate() {
        if seg.is_empty() {
            continue; // a leading/trailing/adjacent `…` constrains nothing here
        }
        let hay = &real_n[pos..];
        let at = if i == 0 {
            // No leading `…`: the transcript claims to start at the start.
            if hay.starts_with(seg) {
                0
            } else {
                return Err(format!(
                    "README transcript does not begin the way the emit does.\n  \
                     README starts: {}\n  actual starts: {}",
                    &seg[..seg.len().min(80)],
                    &hay[..hay.len().min(80)],
                ));
            }
        } else {
            hay.find(seg).ok_or_else(|| {
                format!(
                    "README transcript segment not found in the emit (in order):\n  \
                     missing: {}\n  remaining emit: {}",
                    &seg[..seg.len().min(120)],
                    &hay[..hay.len().min(200)],
                )
            })?
        };
        pos += at + seg.len();
        if i == last {
            // No trailing `…`: the transcript claims to be complete.
            if pos != real_n.len() {
                return Err(format!(
                    "the emit continues past the end of the README transcript, and no \
                     `{ELISION}` marks the cut:\n  unshown tail: {}",
                    &real_n[pos..real_n.len().min(pos + 200)],
                ));
            }
        }
    }
    Ok(())
}

// ── toolchain helpers ───────────────────────────────────────────────────────

fn transpile(src_py: &Path) -> String {
    let out = Command::new(bin())
        .arg("transpile")
        .arg(src_py)
        .output()
        .expect("spawn xpile transpile");
    assert!(
        out.status.success(),
        "`xpile transpile {}` must exit 0 — it is the README's first command. stderr:\n{}",
        src_py.display(),
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8(out.stdout).expect("stdout is UTF-8")
}

/// `rustc -O` the emitted Rust plus a driver, run it, and hand back the exit
/// status and stderr. Compilation failure is always fatal; a *runtime* failure
/// is returned, because test 4's whole subject is a deliberate panic.
fn rustc_o_and_run(dir: &Path, name: &str, rust: &str, driver: &str) -> std::process::Output {
    assert!(
        Command::new("rustc").arg("--version").output().is_ok(),
        "rustc is not on PATH. This witness must not skip: the README's claim is \
         specifically that `rustc -O` compiles the emit, and a skipped probe in front \
         of that claim is indistinguishable from a passing one."
    );
    let file = dir.join(format!("{name}.rs"));
    let merged = format!("{rust}\n\n{driver}\n");
    std::fs::write(&file, &merged).expect("write merged rust");
    let exe = dir.join(name);
    let compile = Command::new("rustc")
        .arg("--edition=2021")
        .arg("-O")
        .arg("-o")
        .arg(&exe)
        .arg(&file)
        .output()
        .expect("spawn rustc");
    assert!(
        compile.status.success(),
        "`rustc -O` REJECTED the README's own published output — the lead promise of the \
         project ('transpile-success means the output compiles') fails on the first \
         example a reader runs:\n=== source ===\n{merged}\n=== rustc stderr ===\n{}",
        String::from_utf8_lossy(&compile.stderr),
    );
    Command::new(&exe).output().expect("spawn compiled binary")
}

// ── 1. the extraction itself is non-vacuous ─────────────────────────────────

/// Both Quickstart blocks are findable and carry content.
///
/// Every assertion below is conditioned on this parse. A `fenced_block` that
/// matched nothing would make them all pass over an empty string, so the parse
/// is asserted first and separately (PMAT-1396: a negative over an enumeration
/// passes for free on an EMPTY enumeration).
#[test]
fn the_quickstart_blocks_are_extractable_from_the_readme() {
    let md = readme();
    let py = quickstart_python(&md);
    let shown = quickstart_transcript(&md);

    assert!(
        py.contains("def factorial") && py.lines().count() >= 2,
        "the Quickstart Python block is not the factorial program:\n{py}"
    );
    assert!(
        shown.contains("pub fn factorial") && shown.contains("xpile-contract:"),
        "the Quickstart transcript block does not look like an emitted Rust module. \
         If the Quickstart moved to a different example, re-point this witness at it \
         rather than deleting it:\n{shown}"
    );
    // The elision matcher must be able to fail. A matcher that returned Ok
    // unconditionally would make test 2 certify nothing.
    assert!(
        elision_match("pub fn factorial", "pub fn something_else").is_err(),
        "elision_match accepted a transcript that does not describe the emit"
    );
    assert!(
        elision_match("pub fn …", "pub fn factorial(n: i64) -> i64").is_ok(),
        "elision_match rejected a correctly-elided transcript"
    );
    assert!(
        elision_match("pub fn factorial", "pub fn factorial(n: i64)").is_err(),
        "elision_match accepted a transcript that silently drops the emit's tail"
    );
}

// ── 2. the published transcript is what the binary actually prints ───────────

/// The README's transcript equals the live emit (modulo reflowing, and modulo
/// any `…` the README marks explicitly).
///
/// Without this, tests 3 and 4 would verify a program the README does not
/// show — exactly the substitution this slice exists to remove.
#[test]
fn the_readme_transcript_is_the_binarys_actual_emit() {
    let md = readme();
    let dir = scratch("transcript");
    let py = dir.join("factorial.py");
    std::fs::write(&py, quickstart_python(&md)).expect("write factorial.py");

    let real = transpile(&py);
    let shown = quickstart_transcript(&md);

    if let Err(why) = elision_match(&shown, real.trim()) {
        panic!(
            "README.md's Quickstart transcript no longer matches `xpile transpile \
             factorial.py`. A published transcript that has rotted is a wrong answer at \
             exit 0 — update the README, or mark the omission with `{ELISION}`.\n{why}\n\
             === actual emit ===\n{real}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ── 3. THE SENTENCE, EXECUTED ───────────────────────────────────────────────

/// `rustc -O` compiles the Quickstart output and `factorial(10) == 3628800`.
///
/// This is the assertion `README.md` has been attributing to CI. It runs on the
/// README's OWN source and the README's OWN emit — no fixture, no shim, no
/// paraphrase.
#[test]
fn the_readme_output_compiles_under_rustc_o_and_computes_3628800() {
    let md = readme();
    let dir = scratch("compute");
    let py = dir.join("factorial.py");
    std::fs::write(&py, quickstart_python(&md)).expect("write factorial.py");
    let rust = transpile(&py);

    // The driver asserts the README's exact number, and a couple of small
    // values so an emit that returned a constant 3628800 could not pass.
    let driver = r#"
fn main() {
    assert_eq!(factorial(0), 1, "0! == 1");
    assert_eq!(factorial(1), 1, "1! == 1");
    assert_eq!(factorial(5), 120, "5! == 120");
    assert_eq!(factorial(10), 3628800, "README.md: factorial(10) == 3628800");
}
"#;
    let run = rustc_o_and_run(&dir, "readme_factorial", &rust, driver);
    assert!(
        run.status.success(),
        "the README's own emitted output computed the WRONG VALUES under `rustc -O`. \
         stderr:\n{}",
        String::from_utf8_lossy(&run.stderr),
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ── 4. the property the paragraph actually sells ────────────────────────────

/// An `i64` overflow panics, naming the governing contract, rather than
/// wrapping.
///
/// This is the claim the paragraph above the CI sentence makes, and it is the
/// one the pre-existing BigInt test cannot make: that test's shim implements
/// `Mul` as a plain `*` on an `i64` field, which under `rustc -O` wraps
/// silently. `factorial(21)` overflows `i64` for certain (20! is the largest
/// that fits), so this is a total, not a probabilistic, probe.
#[test]
fn the_readme_overflow_claim_panics_naming_the_contract() {
    let md = readme();
    let dir = scratch("overflow");
    let py = dir.join("factorial.py");
    std::fs::write(&py, quickstart_python(&md)).expect("write factorial.py");
    let rust = transpile(&py);

    let driver = r#"
fn main() {
    // 20! is the largest factorial that fits in i64; 21! cannot.
    println!("{}", factorial(21));
}
"#;
    let run = rustc_o_and_run(&dir, "readme_factorial_overflow", &rust, driver);
    assert!(
        !run.status.success(),
        "`factorial(21)` OVERFLOWED i64 SILENTLY and exited 0 — the README's central \
         honesty claim ('panics with a pointer to the unimplemented bigint path rather \
         than wrapping silently') is false, and CPython would have returned the exact \
         answer. stdout:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
    let err = String::from_utf8_lossy(&run.stderr);
    assert!(
        err.contains("C-PY-INT-ARITH"),
        "the overflow panic must NAME the governing contract — 'a pointer to the \
         unimplemented bigint path' is the README's phrase for it. Got:\n{err}"
    );
    assert!(
        err.contains("bigint"),
        "the overflow panic must point at the bigint promotion path. Got:\n{err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ── 5. why this file is not redundant with transpile_e2e.rs ─────────────────

/// The pre-existing e2e factorial test covers a genuinely DIFFERENT artifact.
///
/// `transpile_e2e.rs::factorial_emitted_rust_computes_correct_values` reads
/// `crates/xpile/tests/fixtures/factorial.py`, which is annotated `-> BigInt`
/// and emits no `checked_` arithmetic at all. Pinning that here means a future
/// reader cannot conclude the two witnesses are duplicates and delete one; and
/// if the fixture is ever changed to the `i64` shape, this test reds and points
/// at the docs that would then need re-checking.
#[test]
fn the_e2e_fixture_is_a_different_artifact_from_the_readmes() {
    let md = readme();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/factorial.py")
        .canonicalize()
        .expect("tests/fixtures/factorial.py exists");

    let fixture_src = std::fs::read_to_string(&fixture).expect("read fixture");
    let readme_src = quickstart_python(&md);
    assert_ne!(
        strip_ws(&fixture_src),
        strip_ws(&readme_src),
        "the e2e fixture and the README's Quickstart source have become identical. \
         If that is intentional, this witness and \
         transpile_e2e.rs::factorial_emitted_rust_computes_correct_values now cover the \
         same path — merge them deliberately rather than leaving two."
    );

    let fixture_emit = transpile(&fixture);
    assert!(
        !fixture_emit.contains("checked_"),
        "the e2e fixture now emits `checked_` arithmetic, so it may finally cover the \
         README's `i64` shape. Re-read README.md's Quickstart paragraph and this file's \
         header before changing anything — the split they describe would no longer \
         hold:\n{fixture_emit}"
    );

    let dir = scratch("readme_emit");
    let py = dir.join("factorial.py");
    std::fs::write(&py, &readme_src).expect("write factorial.py");
    let readme_emit = transpile(&py);
    assert!(
        readme_emit.contains("checked_mul") && readme_emit.contains("checked_sub"),
        "the README's Quickstart source stopped emitting `checked_*` arithmetic. The \
         Quickstart prose is built entirely on those wrappers; it is now \
         wrong:\n{readme_emit}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
