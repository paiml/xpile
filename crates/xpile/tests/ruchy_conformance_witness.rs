//! XPILE-RUCHYCONF-001 (PMAT-1446) — "compiles to Rust" was published as a
//! property of the Ruchy lane; it is a property of 8 fixtures in 39.
//!
//! THE DEFECT. `book/src/reference/backends.md`'s Status cell read
//! `✅ **Real emission** (same overflow semantics; compiles to Rust)`, and
//! `README.md`'s architecture diagram read `Ruchy — full emission (compiles to
//! Rust)`. Measured over the repo's own `oracle_fixtures/` with `ruchy` v4.2.1:
//!
//! | stage | fixtures |
//! |---|---|
//! | `xpile … --target ruchy` emits | 39 of 39 |
//! | `ruchy check` (parse) accepts | 18 of 39 |
//! | `ruchy transpile` produces Rust | 16 of 39 |
//! | `rustc` compiles that Rust | **8 of 39** |
//!
//! **21 emitted artifacts do not parse as Ruchy at all** — `Expected
//! RightBrace, found Let`, `… found Match` — so for the majority of the corpus
//! the claim fails at the first step, before Rust is even reached.
//!
//! WHAT WAS ALREADY HONEST, so it is not re-derived: the `✅` itself
//! (PMAT-1440 verified input-dependence across all nine backends), and
//! `README.md`'s *specific* example — `xpile transpile factorial.py --target
//! ruchy # → Ruchy (compiles to Rust)` is **true**, measured end to end:
//! emits, `ruchy check` accepts, `ruchy transpile` produces Rust, `rustc`
//! compiles it. **The falsehood was the universal, not the instance** — the
//! same shape as PMAT-1438's shell-roundtrip transcript, which was also
//! accurate under a false generalisation.
//!
//! NO DENOMINATOR IS WRITTEN DOWN ANYWHERE. The counts are re-derived here from
//! the live fixture directory. That is not tidiness: the sibling figures in
//! `ruchy_exec_witness.rs` read `38/38`, `18/38`, `8/38`, and a 39th fixture
//! landed the morning after they were "re-measured" (PMAT-1427) — so all three
//! denominators were wrong while two numerators stayed right. A count typed
//! into prose has a half-life (PMAT-1396); this file removes the last of them
//! and that header now points here.
//!
//! ⚠️ THE SPLIT THIS FILE MAKES, stated rather than discovered. **`ruchy` is
//! installed in no CI workflow** (`grep -rn ruchy .github/workflows` → nothing),
//! so a gate that needs the toolchain runs only where a developer has it. This
//! file therefore separates two rules:
//!
//! - **the WORDING rule runs everywhere**, needs no toolchain, and is what
//!   actually holds the line in CI: no published page may present "compiles to
//!   Rust" as an unqualified property of the Ruchy lane.
//! - **the COUNTS rule** re-derives the table and compares by equality, and
//!   discloses loudly when the toolchain is absent.
//!
//! A disclosed skip in front of a false pass is still a false pass, so the skip
//! is confined to the half that genuinely cannot run, and the half that can run
//! is the one that would catch the defect being fixed. Reverting the Status
//! cell reds in CI; reverting a *count* only reds locally, and this comment is
//! where that limit is written down.

use std::path::{Path, PathBuf};
use std::process::Command;

const PAGE: &str = "book/src/reference/backends.md";
const MARKER: &str = "XPILE-RUCHYCONF-001";

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

fn tool_on_path(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn read(rel: &str) -> String {
    let p = workspace_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// Every `oracle_fixtures/*.py`, sorted. The corpus is DISCOVERED, never listed.
fn fixtures() -> Vec<PathBuf> {
    let dir = workspace_root().join("crates/xpile/tests/oracle_fixtures");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir oracle_fixtures: {e}"))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("py"))
        .collect();
    out.sort();
    assert!(
        !out.is_empty(),
        "oracle_fixtures/ holds no .py files — every count below would be 0 of 0, which passes \
         every comparison for the wrong reason"
    );
    out
}

// ---------------------------------------------------------------------------
// Rule 1 — the WORDING. Runs everywhere, needs no toolchain.
// ---------------------------------------------------------------------------

#[test]
fn no_page_presents_compiles_to_rust_as_a_property_of_the_lane() {
    // What the DEFECT spelled, not what the fix spells (PMAT-1437). Both sites
    // put the phrase in a parenthetical beside a capability verdict, with no
    // qualifier and no link. A mention that names a specific example, or that
    // points at the measured table, is fine — that is how the README's
    // factorial line survives, and it survives because it is TRUE.
    // PARAGRAPH granularity, not line. A hard-wrapped sentence puts the phrase
    // on one line and its qualifier on another, and this file's own disclosure
    // ("Through v0.1.617 the Status cell said `compiles to Rust` …") is exactly
    // that shape — the first draft flagged its own correction. PMAT-1430's rule:
    // a doc gate must distinguish USE from MENTION, and the unit that carries
    // the distinction is the paragraph.
    let mut offenders = Vec::new();
    for rel in ["book/src/reference/backends.md", "README.md"] {
        for (line_no, para) in paragraphs(&read(rel)) {
            let p = para.to_ascii_lowercase();
            if !p.contains("compiles to rust") {
                continue;
            }
            let qualified = p.contains(" of 39")
                || p.contains("factorial")
                || p.contains("#how-far")
                || p.contains("see the book")
                || p.contains("through v0.1.")
                || p.contains("pmat-1446");
            if !qualified {
                offenders.push(format!("{rel}:{line_no}: {}", para.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "\n\"compiles to Rust\" is published as an unqualified property of the Ruchy lane:\n  {}\n\
         Measured, it holds for a minority of the fixture corpus — most emitted artifacts do \
         not parse as Ruchy at all. Name the instance, or link the measured table in \
         {PAGE} (#how-far-the-ruchy-lane-actually-gets).",
        offenders.join("\n  ")
    );
}

#[test]
fn the_measured_table_exists_and_names_the_live_corpus_size() {
    // The table must be present and its denominator must equal the CORPUS, not
    // a number someone typed. This runs without the toolchain, so a new fixture
    // reds the page in CI even where the counts themselves cannot be checked —
    // which is exactly how `ruchy_exec_witness.rs`'s `38` went stale unnoticed.
    let body = read(PAGE);
    let begin = format!("<!-- {MARKER}:BEGIN -->");
    assert!(body.contains(&begin), "{PAGE} must carry `{begin}`");
    let n = fixtures().len();
    let table = body
        .split(&begin)
        .nth(1)
        .and_then(|s| s.split(&format!("<!-- {MARKER}:END -->")).next())
        .expect("the marked table is closed");
    let rows: Vec<&str> = table.lines().filter(|l| l.contains(" of ")).collect();
    assert!(
        rows.len() >= 4,
        "{PAGE}'s {MARKER} table has {} count row(s); the chain has four stages",
        rows.len()
    );
    for row in &rows {
        assert!(
            row.contains(&format!("of {n}")),
            "{PAGE} publishes {row:?}, but oracle_fixtures/ holds {n} fixtures. Every row's \
             denominator is the corpus size; a fixture landed and the table did not move."
        );
    }
}

// ---------------------------------------------------------------------------
// Rule 2 — the COUNTS. Needs `ruchy` + `rustc`; discloses when absent.
// ---------------------------------------------------------------------------

#[test]
fn the_published_counts_are_what_the_toolchain_does() {
    if !tool_on_path("ruchy") || !tool_on_path("rustc") {
        eprintln!(
            "warning: `ruchy` and/or `rustc` not on PATH — skipping the COUNTS half of \
             {MARKER}. `ruchy` is installed in no CI workflow, so this half is a \
             developer-machine check by construction; the WORDING half above runs everywhere \
             and is what holds the published claim in CI."
        );
        return;
    }

    let bin = {
        let mut p = std::env::current_exe().expect("test binary path");
        p.pop();
        if p.ends_with("deps") {
            p.pop();
        }
        p.join("xpile")
    };
    assert!(bin.exists(), "the xpile binary is not next to this test");

    let dir = std::env::temp_dir().join(format!("xpile_ruchyconf_{}", std::process::id()));
    let out = dir.join("out");
    std::fs::create_dir_all(&out).expect("create probe dirs");

    let (mut emits, mut parses, mut transpiles, mut compiles) = (0usize, 0, 0, 0);
    for f in fixtures() {
        let stem = f
            .file_stem()
            .expect("fixture stem")
            .to_string_lossy()
            .into_owned();
        let rk = dir.join(format!("{stem}.ruchy"));
        let rs = dir.join(format!("{stem}.rs"));

        let e = Command::new(&bin)
            .args(["transpile", f.to_str().expect("utf-8"), "--target", "ruchy"])
            .output()
            .expect("spawn xpile");
        if !e.status.success() {
            continue;
        }
        emits += 1;
        std::fs::write(&rk, &e.stdout).expect("write .ruchy");

        if !Command::new("ruchy")
            .args(["check", rk.to_str().expect("utf-8")])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            continue;
        }
        parses += 1;

        let t = Command::new("ruchy")
            .args(["transpile", rk.to_str().expect("utf-8")])
            .output()
            .expect("spawn ruchy transpile");
        if !t.status.success() || t.stdout.is_empty() {
            continue;
        }
        transpiles += 1;
        std::fs::write(&rs, &t.stdout).expect("write .rs");

        // `--out-dir`, NOT `-o /dev/null`: rustc creates a temp dir beside its
        // output, and pointing that at /dev/ fails with "couldn't create a temp
        // dir: Permission denied" — which a naive harness reads as a COMPILE
        // ERROR. The first measurement for this slice reported 0 of 39 for
        // exactly that reason, and 0 would have been a fabricated number inside
        // a fix for fabricated numbers.
        if Command::new("rustc")
            .args([
                "--edition=2021",
                "--crate-type=lib",
                "--out-dir",
                out.to_str().expect("utf-8"),
                rs.to_str().expect("utf-8"),
            ])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            compiles += 1;
        }
    }
    let _ = std::fs::remove_dir_all(&dir);

    let n = fixtures().len();
    let measured = [
        ("emits", emits),
        ("parse", parses),
        ("transpile", transpiles),
        ("rustc", compiles),
    ];

    // Non-vacuity BEFORE the comparison: a chain that collapsed to zero at the
    // first stage would make every later row trivially agree with a table of
    // zeros.
    assert_eq!(
        emits, n,
        "xpile emitted for {emits} of {n} fixtures. The published table's first row is the \
         premise for the rest; if emission itself regressed, fix that before reading the \
         downstream counts."
    );

    let body = read(PAGE);
    for (stage, got) in measured {
        assert!(
            body.contains(&format!("| {got} of {n} |")),
            "{PAGE} does not publish `{got} of {n}` for the `{stage}` stage. Measured now: \
             emits={emits}, parse={parses}, transpile={transpiles}, rustc={compiles} (of {n}). \
             The table is the published claim and this is the measurement; they are one set."
        );
    }

    // The claim being repaired, pinned as an inequality so it reds if the lane
    // ever DOES compile everything — at which point the prose should change.
    assert!(
        compiles < n,
        "every fixture now completes the ruchy→rustc chain ({compiles} of {n}). That is a real \
         capability gain: the Status cell may say `compiles to Rust` again, and this assertion \
         should be deleted with it."
    );
    assert!(
        parses < n,
        "`ruchy check` now accepts every emitted artifact ({parses} of {n}); the '21 do not \
         parse' half of this slice's prose is stale."
    );
}

/// A file split into blank-line-delimited paragraphs, flattened, as
/// (starting line number, text). A claim and its qualifier are a paragraph
/// apart, not a line apart.
fn paragraphs(body: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut start = 1usize;
    let mut buf: Vec<&str> = Vec::new();
    for (i, line) in body.lines().enumerate() {
        if line.trim().is_empty() {
            if !buf.is_empty() {
                out.push((start, buf.join(" ")));
                buf.clear();
            }
            start = i + 2;
        } else {
            if buf.is_empty() {
                start = i + 1;
            }
            buf.push(line);
        }
    }
    if !buf.is_empty() {
        out.push((start, buf.join(" ")));
    }
    out
}
