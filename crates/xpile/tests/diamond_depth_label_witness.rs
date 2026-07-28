//! XPILE-DIAMONDLABEL-001 (PMAT-1448) — `xpile diamond` published a
//! hand-written enumeration of its own COMPUTED classification, on three
//! surfaces, and all three named labels the reporter could not emit.
//!
//! THE DEFECT, on the reporter's own primary output. Every text run printed a
//! legend directly above the column it explains:
//!
//! > `depth: 0 = none, 1 = depth-1 (1 Diamond), 2 = depth-2 (2 Diamonds), 3+ = depth-3+ (3+)`
//!
//! The three rows immediately below it read `depth-21+`, `depth-20`,
//! `depth-13`. The legend's top bucket was a string `depth_label()` could
//! never return — **the line was refuted by the output it headed, on every
//! single run.** `xpile diamond --help` carried a second, differently stale
//! transcription of the same match (`…, depth-8, depth-9+`), and the module
//! header a third (four hard-coded cardinalities, of which "Depth-2 UNIVERSAL:
//! 12/12 contracts" measured 14 of 35 on the tree that carried it).
//!
//! THE MECHANISM, which the unit test recorded without noticing: the
//! classifier was a 22-arm `match`, one arm appended by each numbered slice
//! that first reached that depth (PMAT-286 opened depth-5, …, PMAT-327 opened
//! depth-21+), ending in a saturating `_ => "depth-21+"`. So the cap was not a
//! design — it was however many arms had been typed — and the legend and
//! `--help` were transcriptions of it that stopped being re-typed. The label
//! is now COMPUTED (`none` at zero, `depth-N` otherwise), which is what makes
//! the rule statable in one line and unfalsifiable by growth: there is nothing
//! left to enumerate.
//!
//! WHAT THIS FILE PINS, all derived from the live binary and the live corpus,
//! with no label, count or denominator written down anywhere:
//!
//! 1. **EXACTNESS** — every classification the reporter emits over the live
//!    contract corpus equals its own count column. This is the property that
//!    makes a one-line legend legitimate.
//! 2. **DISJOINT SPELLINGS** — no classification carries a `+`. That spelling
//!    belongs to the totals block's CUMULATIVE buckets, where it means "how
//!    many contracts carry at least N" — a different quantity. Keeping the two
//!    disjoint is what lets rule 3 be decided by a scan.
//! 3. **NO PHANTOM CLASSIFICATION** — no published surface (`--help`, the
//!    legend, the book) may present a `depth-K` token as a classification
//!    unless the reporter can actually produce it.
//! 4. **THE BOOK TRANSCRIPT IS COMPARED TO THE BINARY** — the legend line and
//!    every contract row shown in `diamond-substrate.md` must match the live
//!    report by equality. The previous copy of that transcript OMITTED the
//!    legend — an unmarked elision — which is why the otherwise careful repair
//!    of that page never saw that the legend was where the falsehood lived.
//!
//! NON-VACUITY. A scan for "labels that cannot be produced" passes for free
//! over a corpus that names none, so: the corpus must be non-empty, the report
//! must contain more than one DISTINCT label (otherwise the classification has
//! collapsed back to a constant and would read as fixed), and the transcript
//! comparison must actually find rows to compare.
//!
//! RELATED, and repaired in the same slice rather than here: the
//! `book_claims_no_universal_depth_the_substrate_does_not_hold` gate in
//! `claims_drift.rs` could not match `depth-1..13 UNIVERSAL` — the RANGE
//! spelling its own doc comment quotes as the defect — so a live instance
//! survived in `book/src/reference/cli.md`, four lines above a link to the
//! page that gate protects.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const TRANSCRIPT_PAGE: &str = "book/src/concepts/diamond-substrate.md";
const TRANSCRIPT_MARKER: &str = "DIAMOND-TRANSCRIPT";

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

/// Run the reporter and return its stdout. Uses the binary this test was built
/// alongside, so the witness measures the CLI a user gets rather than a
/// re-implementation of it.
fn diamond(args: &[&str]) -> String {
    let root = workspace_root();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xpile"));
    cmd.arg("diamond")
        .arg("--contracts-dir")
        .arg(root.join("contracts"))
        .args(args)
        .current_dir(&root);
    let out = cmd.output().expect("spawn xpile diamond");
    assert!(
        out.status.success(),
        "`xpile diamond {}` failed ({}), so every assertion below would be \
         measuring an empty report rather than the reporter. stderr: {}",
        args.join(" "),
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 diamond report")
}

/// `(id, diamond_count, classification)` for every contract, parsed out of the
/// JSON report without a serde shape to keep in sync.
fn rows() -> Vec<(String, usize, String)> {
    let json = diamond(&["--json"]);
    let mut out = Vec::new();
    for chunk in json.split("{\"id\":\"").skip(1) {
        let id = chunk
            .split('"')
            .next()
            .expect("id is quoted")
            .trim()
            .to_string();
        let count: usize = between(chunk, "\"diamond_count\":", ",")
            .unwrap_or_else(|| panic!("no diamond_count for {id}"))
            .parse()
            .unwrap_or_else(|e| panic!("unparseable diamond_count for {id}: {e}"));
        let depth =
            between(chunk, "\"depth\":\"", "\"").unwrap_or_else(|| panic!("no depth for {id}"));
        out.push((id, count, depth));
    }
    assert!(
        !out.is_empty(),
        "parsed 0 contracts out of `xpile diamond --json` — the JSON shape moved. \
         Update this witness rather than letting it pass over an empty corpus:\n{json}"
    );
    out
}

fn between(hay: &str, start: &str, end: &str) -> Option<String> {
    let i = hay.find(start)? + start.len();
    let j = hay[i..].find(end)? + i;
    Some(hay[i..j].to_string())
}

/// The legend line the reporter prints above the table.
fn legend() -> String {
    let text = diamond(&[]);
    text.lines()
        .find(|l| l.starts_with("depth:"))
        .unwrap_or_else(|| {
            panic!(
                "`xpile diamond` printed no legend line. The legend is what states the \
                 classification rule; without it the `depth` column is unexplained and \
                 rule 4 below has nothing to compare.\n{text}"
            )
        })
        .to_string()
}

/// Every `depth-<digits>` token in `text`, with any trailing `+`.
fn depth_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = text[from..].find("depth-") {
        let start = from + rel;
        let mut i = start + "depth-".len();
        from = i;
        let digits_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == digits_start {
            continue; // `depth-N`, a placeholder — inert by construction
        }
        if i < bytes.len() && bytes[i] == b'+' {
            i += 1;
        }
        out.push(text[start..i].to_string());
    }
    out
}

// ─── 1. EXACTNESS ────────────────────────────────────────────────────

#[test]
fn every_classification_equals_its_own_count() {
    let rows = rows();
    for (id, count, depth) in &rows {
        let expected = if *count == 0 {
            "none".to_string()
        } else {
            format!("depth-{count}")
        };
        assert_eq!(
            depth, &expected,
            "{id} carries {count} Diamonds but is classified {depth:?}. The classification \
             must be the count exactly — a bucketed label is what let the legend and \
             `--help` publish classes the reporter could not emit (PMAT-1448)."
        );
    }
    // NON-VACUITY: more than one DISTINCT label, or the classification has
    // collapsed to a constant and every assertion here is satisfiable by one
    // value.
    let distinct: BTreeSet<&String> = rows.iter().map(|(_, _, d)| d).collect();
    assert!(
        distinct.len() > 1,
        "the live report carries only the label(s) {distinct:?}. With a single class this \
         witness proves nothing about a classifier."
    );
}

#[test]
fn no_classification_carries_a_cumulative_plus() {
    for (id, count, depth) in rows() {
        assert!(
            !depth.contains('+'),
            "{id} ({count} Diamonds) is classified {depth:?}. A `+` in this reporter marks a \
             CUMULATIVE bucket in the totals block (`depth-N+: K contracts`), which is a \
             different quantity; a classification carrying one makes the two indistinguishable \
             to any reader or scan."
        );
    }
}

// ─── 2. THE SURFACES MAY NOT NAME A CLASS THAT CANNOT EXIST ──────────

/// Every classification the reporter is capable of emitting, as a predicate:
/// `none`, or `depth-` followed by digits and NO `+`.
fn is_producible_classification(tok: &str) -> bool {
    tok == "none"
        || tok
            .strip_prefix("depth-")
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

#[test]
fn the_legend_names_no_class_the_reporter_cannot_produce() {
    let legend = legend();
    let tokens = depth_tokens(&legend);
    // The legend legitimately MENTIONS the cumulative `depth-N+` spelling in
    // order to distinguish it — but only as a placeholder, which carries no
    // digits and so is not collected here. Any DIGITED token in the legend is
    // being presented as a concrete class.
    for tok in &tokens {
        assert!(
            is_producible_classification(tok),
            "the legend presents {tok:?} as a class, and `depth_label()` cannot return it. \
             Through v0.1.617 this line ended `3+ = depth-3+ (3+)` while the rows beneath it \
             read depth-21, depth-20, depth-13.\nlegend: {legend}"
        );
    }
    // The rule must actually be STATED, not merely free of bad examples: an
    // empty legend would satisfy the loop above.
    assert!(
        legend.contains("depth-N"),
        "the legend must state the classification RULE with a placeholder (`depth-N`), not an \
         enumeration of examples — an enumeration is what went stale.\nlegend: {legend}"
    );
}

#[test]
fn help_text_names_no_class_the_reporter_cannot_produce() {
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .args(["diamond", "--help"])
        .output()
        .expect("spawn xpile diamond --help");
    let help = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        help.contains("depth-N"),
        "`xpile diamond --help` must state the classification rule with a placeholder. \
         Through v0.1.617 it enumerated up to a `depth-9+` bucket the reporter has never \
         emitted as a class.\n{help}"
    );
    for tok in depth_tokens(&help) {
        assert!(
            is_producible_classification(&tok),
            "`xpile diamond --help` presents {tok:?} as a class, and `depth_label()` cannot \
             return it.\n{help}"
        );
    }
}

// ─── 3. THE BOOK TRANSCRIPT IS COMPARED TO THE BINARY ────────────────

fn transcript() -> Vec<String> {
    let path = workspace_root().join(TRANSCRIPT_PAGE);
    let body =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {TRANSCRIPT_PAGE}: {e}"));
    let begin = format!("<!-- {TRANSCRIPT_MARKER}:BEGIN -->");
    let end = format!("<!-- {TRANSCRIPT_MARKER}:END -->");
    let s = body
        .find(&begin)
        .unwrap_or_else(|| panic!("{TRANSCRIPT_PAGE} must carry `{begin}`"))
        + begin.len();
    let e = body
        .find(&end)
        .unwrap_or_else(|| panic!("{TRANSCRIPT_PAGE} must carry `{end}`"));
    body[s..e].lines().map(|l| l.to_string()).collect()
}

#[test]
fn the_book_transcript_carries_the_live_legend_verbatim() {
    let live = legend();
    let shown = transcript();
    assert!(
        shown.iter().any(|l| l.trim() == live.trim()),
        "{TRANSCRIPT_PAGE}'s `xpile diamond` transcript does not carry the legend the binary \
         prints.\n  live: {live}\nThe previous copy of this transcript simply OMITTED the \
         legend — an unmarked elision — and that is precisely why the legend was the one part \
         of this reporter's output nobody had checked (PMAT-1448)."
    );
}

#[test]
fn every_contract_row_in_the_book_transcript_matches_the_live_report() {
    let live = rows();
    let mut compared = 0usize;
    for line in transcript() {
        let f: Vec<&str> = line.split_whitespace().collect();
        // A contract row is `<C-ID> <count> <label>`.
        if f.len() != 3 || !f[0].starts_with("C-") {
            continue;
        }
        let (id, shown_count, shown_depth) = (f[0], f[1], f[2]);
        let (_, count, depth) = live
            .iter()
            .find(|(live_id, _, _)| live_id == id)
            .unwrap_or_else(|| {
                panic!(
                    "{TRANSCRIPT_PAGE} shows a row for {id}, which the live report does not \
                     contain. Either the contract was renamed or the transcript is stale."
                )
            });
        assert_eq!(
            (shown_count, shown_depth),
            (count.to_string().as_str(), depth.as_str()),
            "{TRANSCRIPT_PAGE} shows `{id} {shown_count} {shown_depth}`; the live report says \
             `{id} {count} {depth}`."
        );
        compared += 1;
    }
    // NON-VACUITY: a transcript whose rows stopped being recognised would pass
    // this test by comparing nothing.
    assert!(
        compared > 1,
        "only {compared} contract row(s) in {TRANSCRIPT_PAGE} were recognised and compared. \
         The row shape moved, so this comparison is over (almost) nothing."
    );
}

// ─── 4. THE TOTALS BLOCK MUST NOT TRUNCATE ───────────────────────────

#[test]
fn the_totals_block_reaches_the_deepest_contract() {
    let deepest = rows()
        .iter()
        .map(|(_, c, _)| *c)
        .max()
        .expect("non-empty corpus");
    let text = diamond(&[]);
    let totals = text
        .lines()
        .find(|l| l.trim_start().starts_with("depth-1+:"))
        .unwrap_or_else(|| panic!("no cumulative totals line in:\n{text}"));
    let highest = depth_tokens(totals)
        .iter()
        .filter_map(|t| {
            t.trim_start_matches("depth-")
                .trim_end_matches('+')
                .parse::<usize>()
                .ok()
        })
        .max()
        .expect("totals line names at least one bucket");
    assert!(
        highest >= deepest,
        "the deepest contract carries {deepest} Diamonds but the totals block stops at \
         depth-{highest}+, so the deepest contracts are folded into a bucket with nothing \
         above it. Through v0.1.617 this block was 21 hand-written bindings and the deepest \
         contract sat at exactly 21 — one Diamond from silently truncating."
    );
}
