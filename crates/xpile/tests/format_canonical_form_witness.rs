//! XPILE-FORMATFORM-001 (PMAT-1442) — a claimed EXTENSION is a claim about a
//! grammar, and the book published it as a claim about a FILE FORMAT.
//!
//! THE DEFECT. `book/src/reference/frontends.md` lists `.pyi` under **Extensions
//! that LOWER** for the Python frontend and `.h` for the C frontend, both with
//! an empty **Routed → REFUSED** cell. `xpile info` prints `- python (py, pyi)`
//! and `- c (c, h)` unannotated and counts both frontends among the "4
//! lowering"; the dispatch-failure message lists `"*.pyi"` and `"*.h"` under
//! `spellings that LOWER`. Three surfaces, one reading: *these formats are
//! supported.*
//!
//! **No file in the canonical form of either format lowers.** Measured:
//!
//! | source | verdict |
//! |---|---|
//! | `def add(a: int, b: int) -> int: ...` at `probe.pyi` | `lowering error: function \`add\` does not end with \`return expr\` — required at v0.1.0` |
//! | `#ifndef H_` / `#define H_` / `int add(int, int);` / `#endif` at `probe.h` | `lowering error: unexpected character \`#\` in C source` |
//! | `int add(int a, int b);` at `probe.h` | `lowering error: expected LBrace, found Some(Semi)` |
//!
//! That is not an accident of the probe. A `.pyi` stub is **by definition**
//! bodiless — that is what a stub file IS — and the frontend requires every
//! function to end in `return expr`. A `.h` header is **by definition** an
//! include guard plus prototypes, and the C frontend has **no preprocessor at
//! all**: it refuses on the first `#` character, and refuses a prototype for
//! want of a body. The two formats' defining content is exactly what the two
//! grammars reject.
//!
//! WHAT IS TRUE, and why the table above is not a contradiction. Put a
//! `.py`-shaped DEFINITION in a `.pyi`, or a `.c`-shaped definition in a `.h`,
//! and it lowers at exit 0. The extension really is routed; the grammar really
//! is applied. "Lowers" was a claim about the GRAMMAR that read as a claim
//! about the FORMAT.
//!
//! WHY THE DISPOSITION GATE COULD NOT CATCH IT — and it says so itself.
//! `frontend_claim_disposition_witness.rs` (PMAT-1433) does drive every claimed
//! spelling through its frontend, which is the right shape, and its own doc
//! comment states its subject exactly:
//!
//! > that one answers "does this frontend read its language at all", this one
//! > answers "does the answer depend on the path spelling"
//!
//! So `PROBES` carries **one program per FRONTEND** and `probe_path` writes
//! those same bytes to every spelling that frontend claims — the Python
//! function-with-a-body goes to `probe.py` AND `probe.pyi`, the C definition to
//! `probe.c` AND `probe.h`. By construction it can only ever report `Lowers`
//! for all four. **PMAT-1433 generalised the PATHS a probe reaches and left the
//! CONTENT fixed**, so its own lesson — *one probe per subject samples one of
//! its N claims* — recurred one dimension over, inside the gate written to
//! settle it. The gate is not wrong; the surfaces over-read it.
//!
//! WHAT THIS FILE PINS. The measured table lives in `frontends.md` between
//! markers and is compared by EQUALITY, and each row carries **both** halves:
//!
//! 1. the format's canonical form REFUSES at that spelling, and
//! 2. the same frontend's DEFINITION form LOWERS at that same spelling.
//!
//! Half (2) is the anti-vacuity control, and it is not optional: without it a
//! row could be satisfied by a probe that was simply malformed, or by the
//! extension quietly ceasing to be routed at all — the refusal would look
//! identical. It is [[PMAT-1433]]'s "the SAME BYTES must lower elsewhere" rule,
//! turned to hold the same PATH and vary the CONTENT.
//!
//! IMPLEMENTING A STUB PARSER REDS THIS PAGE, which is the point (PMAT-1431 §4):
//! the disclosure has to move when the behaviour does.
//!
//! DISCLOSED, NOT FIXED: `xpile info` still prints both spellings unannotated.
//! Saying more there needs a per-INPUT granularity that `refused_claims()` does
//! not have — it is per-CLAIM, which is exactly the granularity PMAT-1433 added
//! and exactly one level too coarse for this. Recorded in the book and here
//! rather than left for a reader to discover.

use std::path::{Path, PathBuf};
use std::process::Command;

const PAGE: &str = "book/src/reference/frontends.md";
const MARKER: &str = "XPILE-FORMATFORM-001";

/// (spelling, canonical-form source, definition-form source, why it refuses)
///
/// The canonical sources are what the format IS, not a construction chosen to
/// fail: a `.pyi` stub with `...` bodies, and a `.h` with an include guard and
/// prototypes. The definition sources are the anti-vacuity control.
const FORMATS: &[(&str, &str, &str)] = &[
    (
        "*.pyi",
        "def add(a: int, b: int) -> int: ...\n",
        "def add(a: int, b: int) -> int:\n    return a + b\n",
    ),
    (
        "*.h",
        "#ifndef PROBE_H_\n#define PROBE_H_\nint add(int a, int b);\n#endif\n",
        "int add(int a, int b) { return a + b; }\n",
    ),
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

fn xpile_bin() -> PathBuf {
    // The integration-test binary sits next to the CLI cargo just built.
    let mut p = std::env::current_exe().expect("test binary path");
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    p.join("xpile")
}

/// Run the CLI over `source` written at `probe.<ext>` in a private directory.
/// Returns (exit-was-success, combined output).
fn transpile(tag: &str, spelling: &str, source: &str) -> (bool, String) {
    let ext = spelling
        .strip_prefix("*.")
        .unwrap_or_else(|| panic!("{spelling} is not a `*.<ext>` spelling"));
    let dir = std::env::temp_dir().join(format!(
        "xpile_formatform_{}_{}_{}",
        std::process::id(),
        tag,
        ext
    ));
    std::fs::create_dir_all(&dir).expect("create probe dir");
    let file = dir.join(format!("probe.{ext}"));
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
            "rust",
        ])
        .output()
        .expect("spawn xpile");

    let _ = std::fs::remove_dir_all(&dir);
    let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), combined)
}

fn page_body() -> String {
    let p = workspace_root().join(PAGE);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {PAGE}: {e}"))
}

/// The spellings the published table marks as refusing in canonical form.
fn published_spellings() -> Vec<String> {
    let body = page_body();
    let begin = format!("<!-- {MARKER}:BEGIN -->");
    let end = format!("<!-- {MARKER}:END -->");
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
            let cell = l.split('|').nth(1)?.trim();
            let sp = cell.trim_matches('`');
            (sp.starts_with("*.")).then(|| sp.to_string())
        })
        .collect()
}

#[test]
fn the_canonical_form_of_each_claimed_format_refuses() {
    for (spelling, canonical, _) in FORMATS {
        let (ok, out) = transpile("canon", spelling, canonical);
        assert!(
            !ok,
            "a file in the CANONICAL form of `{spelling}` now LOWERS. That is a real capability \
             gain — move the row out of the {MARKER} table in {PAGE} and say so.\n{out}"
        );
        assert!(
            out.contains("lowering error") || out.contains("parse_and_lower failed"),
            "`{spelling}` refused, but not from the frontend — the refusal must reach the \
             frontend's own parse, or this row is measuring dispatch rather than the \
             grammar.\n{out}"
        );
    }
}

#[test]
fn the_definition_form_lowers_at_the_same_spelling() {
    // ANTI-VACUITY, and the half that makes the other one mean something.
    // Without it, a row is equally satisfied by a malformed probe or by the
    // extension quietly ceasing to be routed — both look like a refusal.
    for (spelling, _, definition) in FORMATS {
        let (ok, out) = transpile("defn", spelling, definition);
        assert!(
            ok,
            "`{spelling}` no longer lowers even a plain DEFINITION, so the refusal above is not \
             about the format's canonical form — the spelling may have stopped being routed \
             altogether, which is a different (and larger) claim than this page makes.\n{out}"
        );
    }
}

#[test]
fn the_published_table_is_exactly_the_measured_set() {
    // Equality, both directions: a row for a format that in fact lowers, and a
    // measured refusal with no row, both red. A floor would let the page shrink
    // silently (PMAT-1431 §4).
    let published: Vec<String> = published_spellings();
    let measured: Vec<String> = FORMATS.iter().map(|(s, ..)| (*s).to_string()).collect();

    assert_eq!(
        published, measured,
        "\n{PAGE}'s {MARKER} table lists {published:?} but this witness measures {measured:?}.\n\
         The table is the published claim and this list is the measurement; they are one set."
    );
    assert!(
        !published.is_empty(),
        "{PAGE}'s {MARKER} table is empty, so every comparison above ranged over nothing \
         (PMAT-1396: a negative over an empty enumeration passes for free)"
    );
}

#[test]
fn every_spelling_in_the_table_is_one_the_registry_actually_claims() {
    // The table may only discuss spellings that are really routed. A row for an
    // extension no frontend claims would be a caveat about nothing — and would
    // read to a user as though the spelling were supported enough to warn about.
    let session = xpile_core::default_session();
    let claimed: Vec<String> = session
        .frontends
        .iter()
        .flat_map(|f| f.extensions().iter().map(|e| format!("*.{e}")))
        .collect();
    assert!(
        !claimed.is_empty(),
        "default_session() claims no extensions at all — the registry moved"
    );
    for (spelling, ..) in FORMATS {
        assert!(
            claimed.contains(&(*spelling).to_string()),
            "`{spelling}` is in this witness's table but no registered frontend claims it \
             (claimed: {claimed:?})"
        );
    }
}

#[test]
fn the_page_discloses_that_xpile_info_is_still_unannotated() {
    // A gap this file cannot close must stay SAID. `xpile info` prints both
    // spellings unannotated because `refused_claims()` is per-CLAIM and this
    // distinction is per-INPUT. If someone gives it that granularity, this test
    // reds and the disclosure has to move — a disclosed gap must not be able to
    // decay into a stale caveat (PMAT-1411's inversion).
    let body = page_body();
    assert!(
        body.contains("`xpile info` still prints"),
        "{PAGE} no longer discloses that `xpile info` reports these spellings unannotated. If \
         that was fixed, delete this test with the caveat; if it was not, restore the caveat."
    );
}
