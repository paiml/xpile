//! XPILE-CITEMATRIX-001 (PMAT-1445) — the citation a function carries is
//! TYPE-directed, and the book published it as a constant.
//!
//! THE DEFECT, one sentence in `book/src/reference/frontends.md`, false twice:
//!
//! > Each emitted Rust/Ruchy/Lean function carries a
//! > `// xpile-contract: C-PY-INT-ARITH` citation for the arithmetic contract.
//!
//! **The ID is not constant.** Measured over the CLI, one minimal function per
//! Python type, on every code lane:
//!
//! | Python type | cited |
//! |---|---|
//! | `int` | `C-PY-INT-ARITH` |
//! | `float` | `C-PY-FLOAT-ARITH` |
//! | `str` | `C-XLATE-PY-STR-TO-RUST-STRING` |
//! | `bool` | `C-XLATE-PY-BOOL-TO-RUST-BOOL` |
//!
//! So "each … function carries `C-PY-INT-ARITH`" is right for one type of four,
//! and a reader with a `float` function was pointed at the wrong contract — on
//! the page's only statement about citations.
//!
//! **And `//` is not the Lean form.** The Lean lane emits
//! `/-- xpile-contract: <ID> -/`. That is not an oversight in the emitter:
//! PMAT-1405 changed it deliberately, because a file `lean` must actually parse
//! cannot carry the old `@[xpile_contract "…"]` attribute that nothing
//! registers. The emitter moved and this page did not — so it named the Rust
//! comment syntax for a lane that would not compile with it.
//!
//! WHAT IS ALREADY HONEST, measured and recorded so the next hunt does not
//! re-derive it: every OTHER site in the corpus that shows this citation is
//! correct. `backends.md` gives `// xpile-contract: <ID>` for Rust and
//! `/-- xpile-contract: <ID>[, <ID>]* -/` for Lean, both with placeholders;
//! `cli.md`, `contracts.md` and `adding-a-backend.md` use `<ID>`; and the
//! concrete `C-PY-INT-ARITH` in `quickstart.md`, `python-to-rust.md`,
//! `python-to-lean.md` and `README.md` appears in transcripts of `int`
//! functions, where it is the right answer. **One site universally quantified
//! over a value that varies; the rest quantified over a placeholder.** That is
//! the tell worth carrying forward: a doc that writes `<ID>` cannot go stale
//! this way, and one that writes a literal can.
//!
//! WHAT THIS FILE PINS. Two published tables, both compared to the CLI by
//! EQUALITY, plus the FACTORISATION that makes two small tables legitimate
//! instead of one twelve-cell matrix:
//!
//! - the ID depends on the TYPE and not on the lane, and
//! - the comment form depends on the LANE and not on the type.
//!
//! Checking the factorisation is not decoration. Without it, `lean` could start
//! citing something the other two do not, and each table would still be
//! individually satisfiable by picking a lane to believe. The property that
//! makes the documentation *shaped* the way it is has to be the property under
//! test.
//!
//! NON-VACUITY, three ways, because a citation check is unusually easy to pass
//! over nothing: every cell must actually produce a citation (an emit with none
//! is a failure, not an empty row); the ID set must contain **more than one**
//! distinct value (otherwise the table has collapsed back into the constant
//! this slice removed and would read as fixed again); and the Lean form must
//! DIFFER from the Rust one (the specific falsehood, pinned so a regression to
//! `//` reds by name rather than by a table diff).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

const PAGE: &str = "book/src/reference/frontends.md";

/// (Python type name, a minimal function whose signature is entirely that type)
const TYPES: &[(&str, &str)] = &[
    ("int", "def f(a: int, b: int) -> int:\n    return a + b\n"),
    (
        "float",
        "def f(a: float, b: float) -> float:\n    return a * b\n",
    ),
    ("str", "def f(a: str) -> str:\n    return a\n"),
    ("bool", "def f(a: bool) -> bool:\n    return a\n"),
];

/// The code lanes that emit a citation above each function.
const LANES: &[&str] = &["rust", "ruchy", "lean"];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

fn xpile_bin() -> PathBuf {
    let mut p = std::env::current_exe().expect("test binary path");
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    p.join("xpile")
}

/// Transpile one source at one target and return the citation LINE, verbatim.
///
/// The probe directory is unique PER CALL, not per (type, lane). The tests
/// below each run `measure()` and `cargo test` runs them on concurrent threads
/// in ONE process, so a name keyed on pid + cell is shared state between
/// siblings: the first to finish deletes the directory the second is still
/// reading, and the failure surfaces as a spurious "refused a plain function".
/// That is [[PMAT-1436]]'s shape inside a fresh witness, and it showed up on
/// this file's first run.
fn citation_line(ty: &str, lane: &str, source: &str) -> String {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "xpile_citematrix_{}_{n}_{ty}_{lane}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create probe dir");
    let file = dir.join("probe.py");
    std::fs::write(&file, source).expect("write probe");

    let bin = xpile_bin();
    assert!(
        bin.exists(),
        "the xpile binary is not next to this test at {} — this witness measures the CLI, so a \
         missing binary must fail rather than skip",
        bin.display()
    );
    let out = Command::new(&bin)
        .args([
            "transpile",
            file.to_str().expect("utf-8 path"),
            "--target",
            lane,
        ])
        .output()
        .expect("spawn xpile");
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        out.status.success(),
        "`--target {lane}` refused a plain `{ty}` function, so this cell measures a refusal \
         rather than a citation:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .find(|l| l.contains("xpile-contract:"))
        .unwrap_or_else(|| {
            panic!(
                "`--target {lane}` emitted a `{ty}` function with NO citation at all. The page \
                 says every emitted function carries one; either that is now false or the \
                 default changed.\n{stdout}"
            )
        })
        .trim()
        .to_string()
}

/// `// xpile-contract: C-PY-INT-ARITH` → ("C-PY-INT-ARITH", "// xpile-contract: <ID>")
fn split_citation(line: &str) -> (String, String) {
    let id: String = line
        .split_whitespace()
        .find(|t| t.starts_with("C-"))
        .unwrap_or_else(|| panic!("no contract ID in citation line {line:?}"))
        .trim_end_matches(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit()))
        .to_string();
    (id.clone(), line.replace(&id, "<ID>"))
}

fn page_body() -> String {
    let p = workspace_root().join(PAGE);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {PAGE}: {e}"))
}

/// The two-column rows of a marked table, as (left, right) with backticks
/// stripped.
fn marked_rows(marker: &str) -> Vec<(String, String)> {
    let body = page_body();
    let begin = format!("<!-- {marker}:BEGIN -->");
    let end = format!("<!-- {marker}:END -->");
    let s = body
        .find(&begin)
        .unwrap_or_else(|| panic!("{PAGE} must carry `{begin}`"))
        + begin.len();
    let e = body
        .find(&end)
        .unwrap_or_else(|| panic!("{PAGE} must carry `{end}`"));
    body[s..e]
        .lines()
        .filter(|l| l.trim_start().starts_with('|'))
        .filter_map(|l| {
            let mut cells = l.split('|').skip(1);
            let a = cells.next()?.trim().trim_matches('`').to_string();
            let b = cells.next()?.trim().trim_matches('`').to_string();
            // skip the header and the `|---|---|` rule
            (!a.is_empty() && !a.starts_with("---") && !a.contains("type") && !a.contains("target"))
                .then_some((a, b))
        })
        .collect()
}

/// The whole measurement: (type, lane) → (id, form).
fn measure() -> BTreeMap<(String, String), (String, String)> {
    let mut out = BTreeMap::new();
    for (ty, source) in TYPES {
        for lane in LANES {
            let line = citation_line(ty, lane, source);
            out.insert(
                ((*ty).to_string(), (*lane).to_string()),
                split_citation(&line),
            );
        }
    }
    out
}

#[test]
fn the_published_id_table_is_what_the_binary_cites() {
    let m = measure();
    let published: Vec<(String, String)> = marked_rows("XPILE-CITEMATRIX-001:IDS");
    let measured: Vec<(String, String)> = TYPES
        .iter()
        .map(|(ty, _)| {
            let id = m
                .get(&((*ty).to_string(), LANES[0].to_string()))
                .expect("every (type, lane) cell was measured")
                .0
                .clone();
            ((*ty).to_string(), id)
        })
        .collect();
    assert_eq!(
        published, measured,
        "\n{PAGE}'s IDS table is not what the binary cites.\n  published: {published:?}\n  \
         measured:  {measured:?}"
    );
}

#[test]
fn the_published_syntax_table_is_what_the_binary_emits() {
    let m = measure();
    let published: Vec<(String, String)> = marked_rows("XPILE-CITEMATRIX-001:SYNTAX");
    let measured: Vec<(String, String)> = LANES
        .iter()
        .map(|lane| {
            let form = m
                .get(&(TYPES[0].0.to_string(), (*lane).to_string()))
                .expect("every (type, lane) cell was measured")
                .1
                .clone();
            ((*lane).to_string(), form)
        })
        .collect();
    assert_eq!(
        published, measured,
        "\n{PAGE}'s SYNTAX table is not what the binary emits.\n  published: {published:?}\n  \
         measured:  {measured:?}"
    );
}

#[test]
fn the_id_depends_on_the_type_and_not_on_the_lane() {
    // The property that makes two small tables legitimate instead of one
    // twelve-cell matrix. Without it, a lane could start citing something the
    // other two do not and each table would still be individually satisfiable
    // by picking a lane to believe.
    let m = measure();
    for (ty, _) in TYPES {
        let ids: BTreeSet<&String> = LANES
            .iter()
            .map(|lane| &m[&((*ty).to_string(), (*lane).to_string())].0)
            .collect();
        assert_eq!(
            ids.len(),
            1,
            "the `{ty}` citation is not the same on every lane: {ids:?}. The published table has \
             ONE row per type because the ID is lane-independent; if that has stopped being true \
             the table's shape is wrong, not just its contents."
        );
    }
}

#[test]
fn the_comment_form_depends_on_the_lane_and_not_on_the_type() {
    let m = measure();
    for lane in LANES {
        let forms: BTreeSet<&String> = TYPES
            .iter()
            .map(|(ty, _)| &m[&((*ty).to_string(), (*lane).to_string())].1)
            .collect();
        assert_eq!(
            forms.len(),
            1,
            "`--target {lane}` uses more than one citation form across types: {forms:?}. The \
             published table has ONE row per lane because the form is type-independent."
        );
    }
}

#[test]
fn the_citation_is_not_a_constant_and_lean_is_not_a_slash_comment() {
    // NON-VACUITY, pinned to the two things the old sentence got wrong, so a
    // regression reds BY NAME rather than as an opaque table diff.
    let m = measure();

    let ids: BTreeSet<&String> = m.values().map(|(id, _)| id).collect();
    assert!(
        ids.len() > 1,
        "every measured citation is the same ID ({ids:?}). The published table would then be a \
         constant in table form — which is exactly the claim PMAT-1445 removed — so this must \
         fail rather than pass over a collapsed matrix."
    );

    let rust = &m[&("int".to_string(), "rust".to_string())].1;
    let lean = &m[&("int".to_string(), "lean".to_string())].1;
    assert_ne!(
        rust, lean,
        "the Lean lane now uses the Rust citation form ({rust:?}). PMAT-1405 made it a \
         `/-- … -/` docstring deliberately, because a file `lean` must parse cannot carry the \
         old attribute; if that was reverted, `lean_default_emit_witness.rs` is the gate to \
         look at, and this page has to move with it."
    );
    assert!(
        lean.starts_with("/--"),
        "the Lean citation form is {lean:?}, which is not a Lean docstring"
    );
}
