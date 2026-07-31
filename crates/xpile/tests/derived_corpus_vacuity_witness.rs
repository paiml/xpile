//! XPILE-SKIPGUARD-003 (PMAT-1507, widened by PMAT-1508 and PMAT-1509) — a
//! `for` loop over a corpus DERIVED from the repository must be unable to pass
//! by iterating nothing. THREE derivations are in scope: `git ls-files`, a
//! `read_dir` walk, and a SELECTION out of a document that was read.
//!
//! ## The shape, and where it sits in the family
//!
//! PMAT-1505 closed *a presence probe that cannot succeed skips the whole
//! test*. PMAT-1506 closed the sibling one layer down — *a conditional whose
//! string-literal needle the corpus never contains, so the assertion inside
//! never runs while the test goes on passing*. This file closes the third:
//! **`for x in COLLECTION { assert!(…) }` where `COLLECTION` is empty.** The
//! whole loop is skipped, every assertion inside it is skipped, the test prints
//! `ok`, and the passing-test count is identical — the same immune-to-reading
//! signature, with an even smaller footprint than PMAT-1506's, because there is
//! not even a conditional to read.
//!
//! With the doc-parse arm the class is **CLOSED**: all three arms PMAT-1507
//! specified are now gated.
//!
//! ## THE BOUNDARY IS TWO LISTS, NOT ONE RULE
//!
//! **586** assertion-bearing `for` loops exist across the tracked test corpus
//! (measured by PMAT-1506). A rule demanding a floor from all of them reds
//! hundreds of correct files and gets disabled (PMAT-1500). The subset taken
//! here is the one where **emptiness can only mean the scan missed** — and the
//! third arm is the one that cannot be spelled as a literal, so [`Deriv`] has
//! two constructors:
//!
//! * [`Deriv::Bare`] — the read IS the derivation. `git ls-files`, `read_dir`.
//!   A pathspec matched nothing, a directory was absent or resolved against the
//!   wrong working directory: the loop body never runs and the test still
//!   prints `ok`.
//! * [`Deriv::Selected`] — **reading a document is not a derivation; SELECTING
//!   out of one is.** `let text = read(page)` then `for line in text.lines()`
//!   is out of scope forever, because the read already panics when the file is
//!   absent, so an empty iteration means an empty FILE. But
//!   `lines.filter(is_id).collect()` — or a helper that pushes conditionally —
//!   CAN come back empty because the parser matched nothing.
//!
//! ⛔ **THE NAIVE THIRD ARM WAS MEASURED TWICE AND IT FAILS.** Adding the read
//! as a BARE literal takes the subject class 29 → 54 and leaves **9 unanchored
//! against 0 today** (PMAT-1508, re-measured by PMAT-1509: the same 9). Six are
//! raw `.lines()` walks and one is a negative scan that is *correct* while
//! empty. `selects_at_top_level` + `performs_selection` are what separate them,
//! and `reading_a_file_is_not_a_derivation_unless_something_was_selected_from_
//! it` asserts the boundary in both directions.
//!
//! ⚠️ **`.split(` AND `.lines()` ARE RESHAPES, NOT SELECTIONS** — the one
//! decision the arm rests on, and PMAT-1507's `next_lane` entry guessed it
//! wrong by listing `.split(` as an extraction step. `text.split(sep)` yields
//! at least one element for any non-empty input, so it cannot come back empty
//! because a scan missed. `claims_drift::roadmap_complete_claims_do_not_cite_
//! planned_items` proves the distinction pays: it loops `block.split(';')` and
//! IS in scope — not for the `.split(`, but because `block` came out of a
//! helper that accumulates conditionally.
//!
//! And the scan is DEPTH-AWARE for the same reason.
//! `shell_passthrough_disclosure_witness::paragraphs` contains a `.filter(`,
//! but it sits inside a `.map(` closure and filters the CONTENT of each
//! paragraph, never the number of them. A `contains(".filter(")` predicate
//! would have flagged correct code.
//!
//! ## What was measured, and by EFFECT
//!
//! Every arm was measured the way PMAT-1505 mandates — a test's EFFECT, not its
//! source. Every candidate site was instrumented with an iteration marker and
//! executed.
//!
//! | arm | subject sites | ran zero times |
//! |---|---|---|
//! | `git ls-files` (PMAT-1507, 2026-07-31) | 2 | 0 |
//! | `read_dir` (PMAT-1508, 2026-07-31) | 27 | **0** — 16 test binaries, all exit 0, 2…103 iterations |
//! | selected doc-parse (PMAT-1509, 2026-07-31) | 34 | **0 that are defects** — 4, 5, 80, 165, 3 iterations; the single 0 is correct by design (below) |
//!
//! Union: **44 distinct sites** (63 memberships — a site that walks a directory
//! AND selects out of a read belongs to two arms).
//!
//! **So the finding is again that there is no finding, and that is the result
//! worth recording.** A sweep that reports only refutations reads as a hunt for
//! embarrassments rather than a measurement (PMAT-1505). What the corpus did
//! not have is anything that would notice the next unfloored site — the
//! property was held by convention, and convention is what this repository has
//! repeatedly measured to be a suggestion (`claims_drift.rs`: *"a doc rule with
//! no gate is a suggestion"*). This file is the ratchet.
//!
//! ## PMAT-1509 FOUND ITS DEFECTS IN THE ANCHOR RULES, NOT IN THE CORPUS
//!
//! ⛔ **A FLOOR ABOVE A FILTER IS NOT A FLOOR BELOW IT — the best find here.**
//! Anchor (C) resolves a floor anywhere in the derivation CHAIN, which is what
//! killed four of PMAT-1508's six false positives. Unrestricted it also hands
//! out amnesty ACROSS a selection.
//! `ci_tool_install::every_path_install_curl_fails_on_http_error` iterates
//! `path_install_steps()`, and the only floor in its chain is
//! `workflow_files()`'s `assert!(!files.is_empty())` — which floors the
//! WORKFLOW FILES, upstream of the `.filter(|s| s.body.contains("GITHUB_PATH"))`
//! that produces the loop's actual collection. Every workflow file can be
//! present and the filter still match nothing. (C) now stops at a selection.
//!
//! ⚠️ **AND (B) STILL CARRIED THE HOLE PMAT-1508 CLOSED IN (A).** The counter
//! anchor was `after.contains(&counter) && has_assert(&after)` — two
//! independent tests ANDed, satisfied by an `eprintln!` of the counter plus any
//! unrelated assertion below it. The identical *a print is not a floor* shape,
//! one anchor over, found by looking where the previous slice had just looked.
//! The counter must now be read INSIDE an assertion.
//!
//! ⚠️ **HONEST HALF FOR BOTH: nothing live was vacuous.** `path_install_steps()`
//! returns 5 today and tightening (B) moved no site at all. These are holes
//! closed **before** they were load-bearing, not saves — the same honest
//! reading PMAT-1508 recorded for its own anchor fix, and worth stating plainly
//! rather than letting a repair narrative imply otherwise.
//!
//! ★ **THE ARM SHIPS WITH NO EXEMPTION LIST BECAUSE THE ONE CORRECT ZERO IS
//! ANCHORED BY DERIVATION.** `release_runbook_facts_witness.rs:277` iterates
//! `cited_manifest_lines(&runbook())`, measured at **0** by PMAT-1507 and again
//! here, and it is CORRECT — the runbook carries no live `Cargo.toml:<N>`
//! citation and should not. What keeps that from being a silent pass is
//! `the_citation_detector_can_still_fire`, which drives the same extractor on
//! CONSTRUCTED input and asserts it returns `[43]`. Anchor (E) recognises
//! exactly that, so the site is anchored by a property that **evaporates the
//! moment the control stops flooring the extractor** — strictly better than
//! naming the file in a list. PMAT-1495's exemption trap has fired on eight
//! consecutive slices; this arm gives it nothing to fire on.
//!
//! ## The three repairs
//!
//! Turning the rule on demanded a floor from three live sites, each a check
//! that would have gone silently vacuous if its parser stopped matching:
//!
//! * `book_rust_example_witness::every_compiled_region_is_published_by_the_page_
//!   it_names` — rename either `BOOK-EXAMPLE-BEGIN`/`-END` marker and the region
//!   map empties (measured 4).
//! * `claims_drift::roadmap_complete_claims_do_not_cite_planned_items` — move
//!   the `strategic_goals:` or `roadmap:` key and the block is `""`, whose
//!   `.split(';')` yields one empty clause that matches nothing (measured 80).
//! * `mcp_surface_disclosure_witness::no_unqualified_path_confinement_guarantee`
//!   — an unbalanced ``` fence empties the unquoted-line set and the SECURITY
//!   claim screen scans nothing. Its existing control reads the raw body, so it
//!   would not have noticed (measured 165).
//!
//! ## THE EXTRACTOR WAS WRONG SIX TIMES BEFORE THE CLEAN RESULT WAS BELIEVABLE
//!
//! PMAT-1508's first draft reported **6 unanchored `read_dir` sites**. All six
//! were FALSE POSITIVES, and a gate that cries wolf gets disabled (PMAT-1505),
//! so killing them was the slice. Each one is now a control below:
//!
//! * **A floor one or two CALLS down was invisible.** Anchor (C) resolved only
//!   when the loop iterated `helper()` *directly*; binding the result first —
//!   `let modules = lane_modules(); for m in &modules` — hid `lane_modules()`'s
//!   own `assert!(!out.is_empty())`. `snapshot_required()` needs three hops to
//!   reach `snapshot_rulesets()`'s floor. **A per-site look cannot see a
//!   property that lives in a helper the site never names.**
//! * **A `read_dir` loop usually BUILDS the corpus rather than checking it**,
//!   with the floor under the loop on what it filled. Anchor (B′).
//! * **The floor may be counted into a local first** (`let n = xs.len();
//!   assert!(n >= 30)`), which is how `claims_drift` writes it.
//! * **An existence probe IS a floor**: `xs.iter().find(…).expect(…)` cannot
//!   survive an empty `xs`. Anchor (A′).
//!
//! ⚠️ **AND THE ANCHOR PREDICATE ITSELF WAS HOLLOW.** As shipped it was
//! `body.contains("files.len()")` — which an `eprintln!("scanned {} files",
//! files.len())` satisfies. The length must now be read inside an ASSERTION
//! (`assertion_windows`). PMAT-1509 found the same hole in (B); see above.
//!
//! ## The rule
//!
//! Over every tracked `crates/*/tests/*.rs`: if a function contains a `for`
//! loop that carries an assertion and iterates a value transitively derived
//! from `git ls-files`, a `read_dir` walk, or a SELECTION out of a document
//! read, that collection must be ANCHORED — by
//!
//! * (A) an assertion in the same function reading the collection's own
//!   `is_empty()` / `.len()`, directly or through one `let` binding, or
//! * (A′) an existence probe — `find(…).expect(…)` / `.unwrap()` — over it, or
//! * (B) a counter incremented in the loop that a later ASSERTION reads, or
//! * (B′) a collection the loop FILLS that a later assertion floors, or
//! * (C) a floor in the derivation CHAIN, resolved through calls — **stopping
//!   at a selection**, or
//! * (D) a skip-guard on the collection's own length PAIRED with a counter a
//!   later assertion floors (neither half suffices alone), or
//! * (E) a constructed-input CONTROL that floors the extractor elsewhere in the
//!   file — the anchor for a negative scan whose emptiness is the property.
//!
//! **No exemption list exists here**, and that is deliberate: PMAT-1495's
//! exemption trap has fired on eight consecutive slices, and an exemption
//! nobody has seen fire is the thing it punishes. All three arms land green on
//! the live corpus with nothing carved out.
//!
//! ⛔ **THE RULE'S REACH IS ITS SUBJECT, NOT THE CORPUS IT WALKS.** The scan
//! reads all 278 tracked test files; the sites it governs are the ones that both
//! derive in scope AND assert inside the loop — **44**, and
//! `the_gate_prints_its_subject_cardinality_by_arm` PRINTS the number per arm
//! rather than letting a 278-file corpus imply otherwise.
//!
//! ⛔ **AND THE SUBJECT IS FLOORED PER ARM, NOT OVER THE UNION.** Re-run for
//! PMAT-1509 with the `read_to_string` literal misspelled,
//! `every_derived_assertion_loop_is_anchored` passed again — **fifth
//! consecutive confirmation** — and so did **all eleven constructed red halves**,
//! because they build their fixtures FROM the literal and stay self-consistently
//! green. Only `the_subject_class_is_not_empty` and the cardinality disclosure
//! red. **Only a LIVE per-arm subject floor sees a literal go stale.**
//!
//! ## Self-analysis, and the trap PMAT-1506 hit on CI
//!
//! PMAT-1506's gate went green locally and **red on CI**, because a
//! `git ls-files` corpus cannot see an UNTRACKED file: the new gate did not
//! analyse *itself* until it was committed, and when it did, it flagged its own
//! test fixtures. Both halves are pre-empted here. `this_gate_is_inside_its_own
//! _corpus` asserts this path is in the scanned set, so the self-analysis is
//! proven rather than assumed. And the constructed fixtures below never contain
//! a derivation literal contiguously — they build them with `concat!` — so the
//! detector cannot mistake this file's TEST DATA for live code. That is the
//! repository's own established remedy: **factor so the shape no longer exists,
//! rather than exempt it** (PMAT-1506).
//!
//! ⚠️ **MY OWN BUG, kept:** three of the new controls asserted
//! `found.iter().all(|f| f.anchored)` — **vacuously true on an empty `found`**,
//! which is this file's own defect class inside this file's own new controls.
//! Each is now `!found.is_empty() && …`. Writing a vacuity gate is writing new
//! assertions; run the rule against your own additions.
//!
//! ## Honest scope
//!
//! Three derivations. What is still NOT covered: a collection reshaped rather
//! than selected (by decision, measured above), a selection performed by a
//! helper whose accumulation is neither a depth-0 combinator nor a guarded
//! `push`/`insert`, and anything derived from a source outside the repository
//! (a network fetch, an environment variable). Saying so here rather than
//! letting the file name imply otherwise is the point of the exercise.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;

/// The literal that marks a corpus as derived by shelling out to git. Built by
/// `concat!` so that no contiguous occurrence exists in this file's own source
/// — the detector therefore reads this gate as deriving nothing, and its
/// fixtures below cannot be mistaken for live derivations (PMAT-1506's CI
/// failure).
fn deriv_literal() -> String {
    concat!("ls-", "files").to_string()
}

/// The literal that marks a corpus as derived by walking a directory. Same
/// `concat!` discipline, same reason.
fn walk_literal() -> String {
    concat!("read_", "dir").to_string()
}

/// The literal that marks a DOCUMENT READ. On its own this is not a derivation
/// at all — see [`Deriv::Selected`] and PMAT-1509's measurement.
fn read_literal() -> String {
    concat!("read_to_", "string").to_string()
}

/// A derivation kind. **TWO LISTS, NOT ONE RULE** — the shape PMAT-1479 had to
/// publish about the shell frontend's acceptance boundary, and the same shape
/// here, because the third arm cannot be spelled as a literal.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Deriv {
    /// The read IS the derivation. `git ls-files`, a `read_dir` walk: emptiness
    /// can only mean the scan missed — a pathspec matched nothing, a directory
    /// was absent or wrong, the working directory was not the repository root.
    Bare(String),
    /// Reading a document is **not** a derivation; SELECTING out of one is.
    /// `let text = read(page)` then `for line in text.lines()` is out of scope
    /// forever: the read already panics when the file is absent, so an empty
    /// iteration means an empty FILE, not a missed SCAN. But
    /// `let ids = lines.filter(is_id).collect()` — or a helper that pushes
    /// conditionally — CAN come back empty because the parser matched nothing,
    /// and that is a silent miss of exactly the kind this file governs.
    Selected(String),
}

impl Deriv {
    fn literal(&self) -> &str {
        match self {
            Deriv::Bare(s) | Deriv::Selected(s) => s.as_str(),
        }
    }
}

/// THE SUBJECT SET'S DEFINITION, in three arms.
///
/// ⛔ **THE NAIVE THIRD ARM WAS MEASURED AND IT FAILS.** Adding
/// `read_to_string` as a `Bare` literal takes the subject class from 29 sites
/// to 54 and leaves **9 unanchored against 0 today** (measured 2026-07-31 by
/// PMAT-1508, re-measured by PMAT-1509, same 9). Six of those nine are raw
/// `for line in text.lines()` loops over a just-read file and one is a negative
/// scan that is *correct* while empty — a rule that reds correct files on day
/// one gets disabled (PMAT-1500), which is worse than no rule. `Selected` is
/// the narrower predicate that separates the two, and it is the whole design of
/// this arm.
fn deriv_literals() -> Vec<Deriv> {
    vec![
        Deriv::Bare(deriv_literal()),
        Deriv::Bare(walk_literal()),
        Deriv::Selected(read_literal()),
    ]
}

/// Combinators that SELECT a subset. Applied at paren depth 0 of a pipeline,
/// each can return fewer elements than it was given — including none — so an
/// empty result means the predicate matched nothing.
const SELECTORS: &[&str] = &[
    ".filter(",
    ".filter_map(",
    ".find(",
    ".retain(",
    ".take_while(",
    ".skip_while(",
    ".position(",
];

/// ⛔ **`.split(` AND `.lines()` ARE RESHAPES, NOT SELECTIONS, and leaving them
/// out is the single decision this arm rests on.** PMAT-1507's `next_lane`
/// entry listed `.split(` as a candidate extraction step; measured, it is not
/// one. `text.split(sep)` over a non-empty `text` yields **at least one**
/// element for every input — it cannot come back empty because a scan missed,
/// only because the file was empty, and the read above it already panicked if
/// the file was absent. Admitting it would drag in every `for line in …` loop
/// in the corpus, which is precisely the residue this predicate exists to
/// exclude.
///
/// `claims_drift::roadmap_complete_claims_do_not_cite_planned_items` is the
/// case that proves the distinction pays: it loops
/// `block.split(';').flat_map(…)` and IS in scope — not because of the
/// `.split(`, but because `block` came out of a helper that accumulates
/// conditionally. The `.split(` is doing no selecting at all.
fn selects_at_top_level(expr: &str) -> bool {
    let bytes: Vec<char> = expr.chars().collect();
    let mut depth: i32 = 0;
    for (i, c) in bytes.iter().enumerate() {
        match c {
            '(' => {
                depth += 1;
                continue;
            }
            ')' => {
                depth -= 1;
                continue;
            }
            '.' => {}
            _ => continue,
        }
        if depth != 0 {
            continue;
        }
        let tail: String = bytes[i..].iter().take(16).collect();
        if SELECTORS.iter().any(|s| tail.starts_with(s)) {
            return true;
        }
    }
    false
}

/// Does this function body SELECT — i.e. can its result be empty because a
/// predicate matched nothing, rather than because its input was empty?
///
/// Two spellings, and the second is the one that matters: four of the five
/// live doc-parse subjects are hand-rolled loops, not combinator pipelines.
/// `unquoted_lines`, `marked_regions`, `strategic_goals_block` and
/// `cited_manifest_lines` all walk their input and **accumulate
/// conditionally** — a `push`/`insert` under an `if`, or past a `continue`.
/// That is a selection written longhand, and a combinator-only predicate would
/// have missed every one of them.
fn performs_selection(body: &str) -> bool {
    if selects_at_top_level(body) {
        return true;
    }
    let accumulates = ["push(", "push_str(", "insert(", "insert_str("]
        .iter()
        .any(|m| body.contains(m));
    let conditionally = body.contains("if ") || body.contains("continue");
    accumulates && conditionally
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

/// Every tracked integration-test source. Derived, never typed — and floored
/// by its own caller, since a gate that scans an empty corpus is the exact
/// defect this file exists to forbid.
fn tracked_test_sources() -> Vec<(String, String)> {
    let root = repo_root();
    let out = Command::new("git")
        .current_dir(&root)
        .args(["ls-files", "crates/*/tests/*.rs"])
        .output()
        .expect("git ls-files runs at the workspace root");
    assert!(out.status.success(), "git ls-files failed: {out:?}");
    let listing = String::from_utf8(out.stdout).expect("git emits utf-8 paths");
    let mut files = Vec::new();
    for rel in listing.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let path = root.join(rel);
        if let Ok(text) = std::fs::read_to_string(&path) {
            files.push((rel.to_string(), text));
        }
    }
    files
}

/// `(start, end, name)` for every `fn` in the source, by brace matching.
fn functions(src: &str) -> Vec<(usize, usize, String)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        let rest = t
            .strip_prefix("pub ")
            .unwrap_or(t)
            .strip_prefix("fn ")
            .map(str::to_string);
        let Some(rest) = rest else { continue };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        let mut depth: i32 = 0;
        let mut started = false;
        for (j, l) in lines.iter().enumerate().skip(i) {
            depth += l.matches('{').count() as i32 - l.matches('}').count() as i32;
            if l.contains('{') {
                started = true;
            }
            if started && depth <= 0 {
                out.push((i, j, name));
                break;
            }
        }
    }
    out
}

/// The index of the line closing the block opened on `start`.
fn block_end(lines: &[&str], start: usize) -> usize {
    let mut depth: i32 = 0;
    for (j, l) in lines.iter().enumerate().skip(start) {
        depth += l.matches('{').count() as i32 - l.matches('}').count() as i32;
        if depth <= 0 && j > start {
            return j;
        }
    }
    lines.len().saturating_sub(1)
}

/// The text of every assertion (or `panic!`) in `text`, each spanning from the
/// macro name to the line where its parentheses balance.
///
/// PMAT-1508 needs this because the first draft's floor predicate was
/// `body.contains("x.len()")`, and an `eprintln!("scanned {} steps", steps.len())`
/// satisfies it. **A print is not a floor**: the corpus comes back empty, the
/// line prints `0`, every assertion in the loop is skipped, and the test exits
/// 0 — the exact defect this file exists to forbid, admitted through its own
/// anchor rule. Widening anchor (C) to resolve through a call chain made the
/// looseness compound, so the two changes ship together.
fn assertion_windows(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(pos) = ["assert!(", "assert_eq!(", "assert_ne!(", "panic!("]
            .iter()
            .filter_map(|m| line.find(m))
            .min()
        else {
            continue;
        };
        let mut depth: i32 = 0;
        let mut buf = String::new();
        for (j, l) in lines.iter().enumerate().skip(i).take(12) {
            let seg = if j == i { &l[pos..] } else { l };
            buf.push_str(seg);
            buf.push('\n');
            depth += seg.matches('(').count() as i32 - seg.matches(')').count() as i32;
            if depth <= 0 {
                break;
            }
        }
        out.push(buf);
    }
    out
}

/// Does `text` FLOOR `name` — an assertion that reads the collection's own
/// emptiness or length, as opposed to merely mentioning it?
///
/// One level of indirection is followed, because `claims_drift` writes the
/// floor the way a careful author does: `let substrate_files = substrate.len();`
/// and then asserts on the count, with the message quoting the number. The
/// collection is floored; the assertion just does not spell it.
fn floors_named(text: &str, name: &str) -> bool {
    let mut names = vec![name.to_string()];
    for line in text.lines() {
        let t = line.trim_start();
        let Some(rest) = t.strip_prefix("let ") else {
            continue;
        };
        let rest = rest.strip_prefix("mut ").unwrap_or(rest);
        let var: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if var.is_empty() {
            continue;
        }
        if t.contains(&format!("{name}.len()")) || t.contains(&format!("{name}.is_empty()")) {
            names.push(var);
        }
    }

    let windows = assertion_windows(text);
    names.iter().any(|n| {
        let needles = [format!("{n}.is_empty()"), format!("{n}.len()"), n.clone()];
        windows.iter().any(|w| {
            if *n == name {
                needles[..2].iter().any(|x| w.contains(x.as_str()))
            } else {
                // A derived count: the assertion must READ it, and reading it
                // at all is a floor only because the binding is `<coll>.len()`.
                w.contains(n.as_str())
            }
        })
    })
}

/// (A′) AN EXISTENCE PROBE IS A FLOOR. `xs.iter().find(…).expect("…")` cannot
/// survive an empty `xs`, so the loop below it cannot silently iterate nothing.
///
/// `contract_layer_label_integrity::the_short_alias_is_derived_and_stays_
/// unambiguous` is written exactly this way, and calling it unanchored would
/// red a correct file — which is how a gate gets disabled (PMAT-1500).
fn probes_existence(text: &str, name: &str) -> bool {
    text.split(';').any(|stmt| {
        stmt.contains(name)
            && stmt.contains(".find(")
            && (stmt.contains(".expect(") || stmt.contains(".unwrap()"))
    })
}

/// Does `text` floor SOMETHING? Used for anchor (C), where the collection is
/// bound to a local inside the helper (`out`, `found`, …) whose name the call
/// site never sees.
fn floors_anything(text: &str) -> bool {
    assertion_windows(text)
        .iter()
        .any(|w| w.contains(".is_empty()") || w.contains(".len()"))
}

/// Is `counter` READ by an assertion in `text`?
///
/// (B) shipped as `after.contains(counter) && has_assert(after)` — satisfied by
/// an `eprintln!` of the counter plus any unrelated assertion anywhere below.
/// That is PMAT-1508's *a print is not a floor* in the counter anchor, and
/// `a_printed_counter_does_not_anchor_a_derived_loop` now keeps it closed.
fn counter_is_asserted(text: &str, counter: &str) -> bool {
    assertion_windows(text).iter().any(|w| w.contains(counter))
}

fn has_assert(text: &str) -> bool {
    text.contains("assert!(")
        || text.contains("assert_eq!(")
        || text.contains("assert_ne!(")
        || text.contains("assert!")
}

/// One unanchored site: `file:line`, the enclosing function, the collection.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Finding {
    file: String,
    line: usize,
    function: String,
    collection: String,
    anchored: bool,
}

/// The detector. Returns every SUBJECT site — an assertion-bearing `for` loop
/// over a `git ls-files`-derived collection — each tagged with whether it is
/// anchored.
///
/// The rule below filters this to the unanchored ones. It returns the whole
/// subject class rather than only the violations on purpose: a rule quantified
/// over a set it also computes passes for free when that set comes back empty,
/// which is the defect this very file exists to forbid. `the_subject_class_is_
/// not_empty` floors it.
fn subject_sites(rel: &str, src: &str, derivs: &[Deriv]) -> Vec<Finding> {
    let lines: Vec<&str> = src.lines().collect();
    let fns = functions(src);
    let spells = |text: &str| derivs.iter().any(|d| text.contains(d.literal()));
    let spells_bare = |text: &str| {
        derivs
            .iter()
            .any(|d| matches!(d, Deriv::Bare(_)) && text.contains(d.literal()))
    };

    // Which helper functions are themselves derivations? Fixpoint, so a helper
    // that calls a helper that shells out is still a derivation.
    let mut deriv_fns: BTreeSet<String> = BTreeSet::new();
    let mut bare_fns: BTreeSet<String> = BTreeSet::new();
    for _ in 0..4 {
        for (a, b, name) in &fns {
            let body = lines[*a..=*b].join("\n");
            let indirect = |set: &BTreeSet<String>| {
                set.iter()
                    .any(|d| body.contains(&format!("{d}(")) && d != name)
            };
            if spells(&body) || indirect(&deriv_fns) {
                deriv_fns.insert(name.clone());
            }
            if spells_bare(&body) || indirect(&bare_fns) {
                bare_fns.insert(name.clone());
            }
        }
    }

    // Which helpers SELECT? Transitive, because a wrapper that returns a
    // selecting helper's result returns a selection. `sel_fns` is what makes
    // the third arm a real predicate rather than a literal, and it is also what
    // anchor (C) is restricted to below.
    let mut sel_fns: BTreeSet<String> = BTreeSet::new();
    for _ in 0..4 {
        for (a, b, name) in &fns {
            let body = lines[*a..=*b].join("\n");
            let calls_sel = sel_fns
                .iter()
                .any(|d| body.contains(&format!("{d}(")) && d != name);
            if performs_selection(&body) || calls_sel {
                sel_fns.insert(name.clone());
            }
        }
    }

    // The call graph restricted to derivation helpers. PMAT-1508 (e): a floor
    // can live one or two calls further down than the collection's own
    // spelling, and a per-function look cannot see it. `snapshot_required()`
    // → `snapshot_required_by_ruleset()` → `snapshot_rulesets()` is three hops
    // to the `assert!(!out.is_empty())` that actually protects the corpus.
    let mut calls: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (a, b, name) in &fns {
        let body = lines[*a..=*b].join("\n");
        let callees: BTreeSet<String> = deriv_fns
            .iter()
            .filter(|d| d.as_str() != name && body.contains(&format!("{d}(")))
            .cloned()
            .collect();
        calls.insert(name.clone(), callees);
    }

    let mut found = Vec::new();
    for (a, b, fname) in &fns {
        let body: Vec<&str> = lines[*a..=*b].to_vec();
        let whole = body.join("\n");

        // Locals bound to a derived value, transitively — and, for each, the
        // derivation helpers its right-hand side went through.
        //
        // `bare` and `selected` are what split the third arm from the first
        // two: a local is a SUBJECT when it reached a bare derivation, or when
        // it reached a document read AND a selection was applied on the way.
        let mut derived: BTreeSet<String> = BTreeSet::new();
        let mut bare: BTreeSet<String> = BTreeSet::new();
        let mut selected: BTreeSet<String> = BTreeSet::new();
        let mut whence: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut sel_whence: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for _ in 0..3 {
            for (k, line) in body.iter().enumerate() {
                let t = line.trim_start();
                let Some(rest) = t.strip_prefix("let ") else {
                    continue;
                };
                let rest = rest.strip_prefix("mut ").unwrap_or(rest);
                let var: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if var.is_empty() {
                    continue;
                }
                // The binding's RHS: this line through the next `;`.
                let mut chunk = String::new();
                for l in body.iter().skip(k).take(40) {
                    chunk.push_str(l);
                    chunk.push('\n');
                    if l.trim_end().ends_with(';') {
                        break;
                    }
                }
                let rhs = chunk.split_once('=').map(|(_, r)| r).unwrap_or(&chunk);
                let via: BTreeSet<String> = deriv_fns
                    .iter()
                    .filter(|d| rhs.contains(&format!("{d}(")))
                    .cloned()
                    .collect();
                let upstream_vars: Vec<&String> = derived
                    .iter()
                    .filter(|v| {
                        rhs.contains(&format!("{v}."))
                            || rhs.contains(&format!("&{v}"))
                            || rhs.contains(&format!(" {v} "))
                    })
                    .collect();
                let upstream: BTreeSet<String> = upstream_vars
                    .iter()
                    .flat_map(|v| whence.get(*v).cloned().unwrap_or_default())
                    .collect();
                let from_deriv = spells(rhs) || !via.is_empty() || !upstream_vars.is_empty();
                if from_deriv {
                    let entry = whence.entry(var.clone()).or_default();
                    entry.extend(via);
                    entry.extend(upstream);
                    let via_bare = bare_fns
                        .iter()
                        .any(|d| rhs.contains(&format!("{d}(")) && *d != var);
                    if spells_bare(rhs)
                        || via_bare
                        || upstream_vars.iter().any(|v| bare.contains(*v))
                    {
                        bare.insert(var.clone());
                    }
                    let sel_via: BTreeSet<String> = sel_fns
                        .iter()
                        .filter(|d| rhs.contains(&format!("{d}(")) && **d != var)
                        .cloned()
                        .collect();
                    let sel_up: BTreeSet<String> = upstream_vars
                        .iter()
                        .flat_map(|v| sel_whence.get(*v).cloned().unwrap_or_default())
                        .collect();
                    if selects_at_top_level(rhs)
                        || !sel_via.is_empty()
                        || upstream_vars.iter().any(|v| selected.contains(*v))
                    {
                        let e = sel_whence.entry(var.clone()).or_default();
                        e.extend(sel_via);
                        e.extend(sel_up);
                        selected.insert(var.clone());
                    }
                    derived.insert(var);
                }
            }
        }

        for (k, line) in body.iter().enumerate() {
            let t = line.trim_start();
            if !t.starts_with("for ") {
                continue;
            }
            // PMAT-1510: requiring the SAME line to end in `{` made every
            // rustfmt-wrapped loop header invisible — not reported as anchored,
            // not reported at all. A long header breaks as
            //     for entry in some::long::call(&a, &b)
            //         .filter(..)
            //     {
            // and neither line satisfies both halves. Rejoin the header up to
            // its opening brace before parsing it.
            let mut header = t.trim_end().to_string();
            let mut k_end = k;
            while !header.ends_with('{') && k_end + 1 < body.len() && k_end - k < 6 {
                k_end += 1;
                let next = body[k_end].trim();
                // No separator before a method-chain continuation: rustfmt
                // breaks `files.iter()` as `files` / `.iter()`, and joining
                // those with a space yields `files .iter()`, which the
                // collection matcher below does not recognise. The first draft
                // of this fix did exactly that — the three-arm probe (same
                // violation, single-line header REDS, wrapped header PASSES)
                // is what caught it.
                if !next.starts_with('.') && !header.ends_with('.') {
                    header.push(' ');
                }
                header.push_str(next);
                header = header.trim_end().to_string();
            }
            if !header.ends_with('{') {
                continue;
            }
            let t: &str = &header;
            let Some(iter_expr) = t.split_once(" in ").map(|(_, r)| r) else {
                continue;
            };
            let resolved: Option<(String, bool, bool)> = derived
                .iter()
                .find(|v| {
                    iter_expr.contains(&format!("&{v}"))
                        || iter_expr.contains(&format!("{v}."))
                        || iter_expr
                            .split_whitespace()
                            .any(|w| w.trim_matches(&['&', '{'][..]) == v.as_str())
                })
                .map(|v| (v.clone(), bare.contains(v), selected.contains(v)))
                .or_else(|| {
                    deriv_fns
                        .iter()
                        .find(|d| iter_expr.contains(&format!("{d}(")))
                        .map(|d| (format!("{d}()"), bare_fns.contains(d), sel_fns.contains(d)))
                })
                .or_else(|| {
                    spells(iter_expr).then(|| {
                        (
                            "<inline>".to_string(),
                            spells_bare(iter_expr),
                            selects_at_top_level(iter_expr),
                        )
                    })
                });
            let Some((collection, is_bare, is_selected)) = resolved else {
                continue;
            };
            // ⛔ THE THIRD ARM'S WHOLE POINT. A document read alone is not a
            // derivation this gate governs; only a SELECTION out of one is.
            if !is_bare && !is_selected {
                continue;
            }

            // Brace matching must start at the line carrying the `{`, which is
            // `k_end` for a wrapped header and `k` for a single-line one. Using
            // `k` unconditionally meant a wrapped loop's extent was computed
            // from a line with no brace on it, so its body — and the assertion
            // inside — was never found (PMAT-1510).
            let end = block_end(&body, k_end);
            let inner = body[k..=end].join("\n");
            if !has_assert(&inner) {
                continue;
            }

            // (A) a floor on the collection in this function.
            let base = collection.replace("()", "");
            let mut anchored = floors_named(&whole, &base)
                || floors_named(&whole, &collection)
                || probes_existence(&whole, &base);

            // (B) a counter incremented in the loop and read after it.
            let after = body[end..].join("\n");
            if !anchored {
                for (idx, _) in inner.match_indices("+= 1") {
                    let head = &inner[..idx];
                    let counter: String = head
                        .chars()
                        .rev()
                        .skip_while(|c| c.is_whitespace())
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    if !counter.is_empty() && counter_is_asserted(&after, &counter) {
                        anchored = true;
                    }
                }
            }

            // (B′) THE BUILDER FORM, and it is what a `read_dir` walk usually
            // looks like: the loop does not check the corpus, it BUILDS it, and
            // the floor sits under the loop on the collection it filled. That
            // is the same protection a counter gives — if the walk finds
            // nothing, the assertion after the loop reds — so refusing to
            // recognise it would red `ruleset_drift::snapshot_rulesets`, which
            // carries a textbook `assert!(!out.is_empty(), …)` three lines past
            // its own `for`.
            if !anchored {
                for marker in ["insert(", "push(", "extend(", "insert_str("] {
                    for (idx, _) in inner.match_indices(marker) {
                        let head = inner[..idx].trim_end();
                        let head = head.strip_suffix('.').unwrap_or(head);
                        let filled: String = head
                            .chars()
                            .rev()
                            .take_while(|c| c.is_alphanumeric() || *c == '_')
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();
                        if !filled.is_empty() && floors_named(&after, &filled) {
                            anchored = true;
                        }
                    }
                }
            }

            // (D) A SKIP-GUARD IS NOT A FLOOR ON ITS OWN — but a skip-guard
            // PAIRED with a counter a later assertion floors is one.
            //
            // `cli_spirv_input_fidelity_witness` writes it this way:
            // `if outs.len() < 2 { continue; }` then `emitting_targets += 1;`
            // then the loop, then `assert!(emitting_targets >= 4)`. The guard
            // alone is the PMAT-1505 defect verbatim (a skip that reports
            // success); the counter alone does not say the loop had anything to
            // iterate. Together they do: at least four targets got past a guard
            // that requires a non-empty collection. Both halves are required,
            // and `a_skip_guard_alone_does_not_anchor_a_selected_loop` keeps it
            // that way.
            if !anchored {
                let wl: Vec<&str> = whole.lines().collect();
                let guarded = wl.iter().enumerate().any(|(i, l)| {
                    let t = l.trim_start();
                    let window = wl[i..]
                        .iter()
                        .take(3)
                        .copied()
                        .collect::<Vec<_>>()
                        .join(" ");
                    t.starts_with("if ")
                        && (t.contains(&format!("{base}.len()"))
                            || t.contains(&format!("{base}.is_empty()")))
                        && (window.contains("continue") || window.contains("return"))
                });
                if guarded {
                    for (idx, _) in whole.match_indices("+= 1") {
                        let head = &whole[..idx];
                        let counter: String = head
                            .chars()
                            .rev()
                            .skip_while(|c| c.is_whitespace())
                            .take_while(|c| c.is_alphanumeric() || *c == '_')
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();
                        if !counter.is_empty() && counter_is_asserted(&after, &counter) {
                            anchored = true;
                        }
                    }
                }
            }

            // (E) A NEGATIVE SCAN WITH A CONSTRUCTED-INPUT CONTROL IS ANCHORED,
            // and this is what lets the gate ship with NO exemption list.
            //
            // `release_runbook_facts_witness.rs:277` iterates
            // `cited_manifest_lines(&runbook())`, which PMAT-1507 measured at
            // ZERO and recorded as CORRECT: the runbook carries no live
            // `Cargo.toml:<N>` citation, and it should not. Its emptiness is the
            // property, not a miss. What keeps that from being a silent pass is
            // `the_citation_detector_can_still_fire`, which drives the SAME
            // extractor on CONSTRUCTED input and asserts it returns `[43]`.
            //
            // That is this repository's own established remedy — factor the scan
            // into a helper and control it on constructed input (PMAT-1506) —
            // so recognising it is strictly better than exempting the file by
            // name. An exemption nobody has seen fire is what PMAT-1495's trap
            // punishes; this anchor is DERIVED, and it evaporates the moment the
            // control stops floring the extractor.
            if !anchored && !base.is_empty() {
                let mut extractors: BTreeSet<String> = BTreeSet::new();
                extractors.insert(base.clone());
                if let Some(hs) = sel_whence.get(&collection) {
                    extractors.extend(hs.iter().cloned());
                }
                let windows = assertion_windows(src);
                anchored = extractors.iter().any(|h| {
                    let call = format!("{h}(");
                    windows
                        .iter()
                        .any(|w| w.contains(&call) && !w.contains(".is_empty()"))
                });
            }

            // (C) a floor inside the derivation CHAIN — not just the helper the
            // loop names. PMAT-1508: resolving only the named helper missed
            // `lane_modules()`'s own `assert!(!out.is_empty())` whenever the
            // loop iterated a LOCAL bound to it, and missed
            // `snapshot_rulesets()`'s floor two calls below `snapshot_required`.
            // A per-site look cannot see a property that lives in a helper the
            // site never names.
            //
            // ⛔ **AND IT STOPS AT A SELECTION.** PMAT-1509: resolving the whole
            // chain unconditionally hands out amnesty ACROSS a filter.
            // `ci_tool_install::every_path_install_curl_fails_on_http_error`
            // iterates `path_install_steps()`, whose only floor lives in
            // `workflow_files()` — `assert!(!files.is_empty())` — which floors
            // the WORKFLOW FILES, upstream of the
            // `.filter(|s| s.body.contains("GITHUB_PATH"))` that produces the
            // loop's actual collection. **A floor above a filter is not a floor
            // below it**: every workflow file can be present and the filter
            // still match nothing. So for a selected site the chain is
            // restricted to helpers that themselves select — the ones whose
            // output the floor would actually be about.
            if !anchored {
                let mut chain: BTreeSet<String> = BTreeSet::new();
                let base = collection.replace("()", "");
                for (_, _, hname) in &fns {
                    if *hname == base {
                        chain.insert(hname.clone());
                    }
                }
                if let Some(hs) = whence.get(&collection) {
                    chain.extend(hs.iter().cloned());
                }
                for _ in 0..4 {
                    let grown: BTreeSet<String> = chain
                        .iter()
                        .filter_map(|h| calls.get(h))
                        .flatten()
                        .cloned()
                        .collect();
                    chain.extend(grown);
                }
                for (ha, hb, hname) in &fns {
                    if !chain.contains(hname) {
                        continue;
                    }
                    if is_selected && !sel_fns.contains(hname) {
                        continue; // upstream of the selection — see above.
                    }
                    if floors_anything(&lines[*ha..=*hb].join("\n")) {
                        anchored = true;
                    }
                }
            }

            found.push(Finding {
                file: rel.to_string(),
                line: a + k + 1,
                function: fname.clone(),
                collection,
                anchored,
            });
        }
    }
    found
}

/// Every subject site in the tracked test corpus, for the given derivation set.
fn live_subjects(derivs: &[Deriv]) -> Vec<Finding> {
    let files = tracked_test_sources();
    assert!(
        files.len() > 100,
        "the corpus derivation returned {} file(s); this gate would pass by scanning \
         nothing, which is the exact defect it exists to forbid",
        files.len()
    );
    let mut findings = Vec::new();
    for (rel, src) in &files {
        findings.extend(subject_sites(rel, src, derivs));
    }
    findings.sort();
    findings
}

/// THE RULE. Every repository-derived assertion loop is anchored — over BOTH
/// derivation literals, `git ls-files` and `read_dir`, as one set.
#[test]
fn every_derived_assertion_loop_is_anchored() {
    let derivs = deriv_literals();
    let findings = live_subjects(&derivs);
    let unanchored: Vec<&Finding> = findings.iter().filter(|f| !f.anchored).collect();

    assert!(
        unanchored.is_empty(),
        "XPILE-SKIPGUARD-003: {} loop(s) iterate a corpus derived from {} and carry an \
         assertion, with nothing that reds if the corpus comes back EMPTY. A pathspec \
         that matches nothing, a directory that is absent or wrong, a missing `git`, or \
         a wrong working directory then skips every assertion in the loop and the test \
         still prints `ok`. Add a non-vacuity floor on the collection (a counter a later \
         assertion reads, or a floor inside the derivation helper, both count). Sites:\n{}",
        unanchored.len(),
        derivs
            .iter()
            .map(|d| format!("`{}`", d.literal()))
            .collect::<Vec<_>>()
            .join(" / "),
        unanchored
            .iter()
            .map(|f| format!(
                "  {}:{} in `{}` over `{}`",
                f.file, f.line, f.function, f.collection
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// NON-VACUITY. The extractor still finds the derivations it is known to find.
///
/// Without this, a regex drifting out from under the scan reports a clean sweep
/// — the failure mode every derived gate in this repository has to defend
/// against, and the one this file is named for.
/// EACH ARM SEPARATELY, because a floor over the union hides an arm that dies.
#[test]
fn the_extractor_still_finds_the_known_derivations() {
    let files = tracked_test_sources();

    // (literal, floor, files whose derivation is load-bearing and long-lived)
    let arms: [(String, usize, [&str; 2]); 2] = [
        (
            deriv_literal(),
            8,
            [
                "shell_artifact_policy_witness.rs",
                "skip_guard_vacuity_witness.rs",
            ],
        ),
        (
            walk_literal(),
            20,
            ["ruleset_drift.rs", "lean_models_lane_witness.rs"],
        ),
    ];

    for (deriv, floor, anchors) in arms {
        let deriving: Vec<&str> = files
            .iter()
            .filter(|(_, src)| src.contains(&deriv))
            .map(|(rel, _)| rel.as_str())
            .collect();

        assert!(
            deriving.len() >= floor,
            "only {} tracked test file(s) derive a corpus from `{}`; 10 did for \
             `{}` and 24 for `{}` when this arm was written (2026-07-31). Either \
             the extractor is broken or the corpus moved — check which before \
             lowering this floor.",
            deriving.len(),
            deriv,
            deriv_literal(),
            walk_literal()
        );

        for anchor in anchors {
            assert!(
                deriving.iter().any(|f| f.ends_with(anchor)),
                "`{anchor}` derives its corpus from `{deriv}` and the extractor no \
                 longer sees it; the scan has drifted off its own subject"
            );
        }
    }
}

/// NON-VACUITY, THE ONE THAT MATTERS. The SUBJECT CLASS is not empty.
///
/// `every_ls_files_derived_assertion_loop_is_anchored` is a universal over a
/// set this file also computes. If the loop detector silently stops matching —
/// a `for` spelling it does not parse, a derivation it no longer resolves —
/// the subject class collapses to nothing and the rule passes forever, having
/// checked no loop at all. **That is the precise defect this file was written
/// to forbid, one level up**, and it is the shape PMAT-1506 found inside a gate
/// built for exactly its own defect class.
///
/// ⛔ **AND IT IS FLOORED PER ARM, NOT OVER THE UNION.** PMAT-1507 proved by
/// neutering the detector that the live rule stays GREEN when the subject class
/// dies; only the floor reds. A union floor inherits that hole one level in: the
/// `git ls-files` arm alone keeps the union non-empty, so the `read_dir` arm
/// could stop matching entirely and nothing here would notice — unfalsifiable
/// exactly where this slice is new.
///
/// Measured 2026-07-31: `git ls-files` → 2 sites, `read_dir` → 27, and the
/// SELECTED document-parse arm → 34 (union 44; the arms overlap wherever one
/// site derives two ways).
///
/// ⚠️ The third arm's floor is the one that needs the NAMED anchors most. Its
/// subject is not "files containing a literal" but "files where a SELECTION was
/// applied to something read", and that predicate has four moving parts — a
/// depth-0 combinator scan, a conditional-accumulation heuristic, a transitive
/// helper closure and an upstream-variable walk. Any one of them silently
/// failing takes the arm to zero, and only a live floor with named members sees
/// it (PMAT-1508: the constructed red halves build their fixtures FROM the
/// predicate and stay self-consistently green).
#[test]
fn the_subject_class_is_not_empty() {
    let arms: [(Deriv, usize, [&str; 2]); 3] = [
        (
            Deriv::Bare(deriv_literal()),
            2,
            [
                "lean_source_lang_refusal_witness.rs",
                "build_script_path_independence.rs",
            ],
        ),
        (
            Deriv::Bare(walk_literal()),
            20,
            ["ruleset_drift.rs", "claims_drift.rs"],
        ),
        (
            Deriv::Selected(read_literal()),
            25,
            [
                "mcp_surface_disclosure_witness.rs",
                "book_rust_example_witness.rs",
            ],
        ),
    ];

    for (deriv, floor, anchors) in arms {
        let lit = deriv.literal().to_string();
        let derivs = vec![deriv.clone()];
        let mut subjects = Vec::new();
        for (rel, src) in &tracked_test_sources() {
            subjects.extend(subject_sites(rel, src, &derivs));
        }

        assert!(
            subjects.len() >= floor,
            "the detector found {} assertion-bearing loop(s) over a `{lit}` corpus \
             anywhere in the tracked test tree; {floor} is the floor measured when this \
             arm was written. The rule above is quantified over a set this file also \
             computes, so a detector that stops matching makes it pass having checked \
             nothing — fix the extractor rather than trusting its green.",
            subjects.len()
        );

        for anchor in anchors {
            assert!(
                subjects.iter().any(|f| f.file.ends_with(anchor)),
                "`{anchor}` carries an assertion loop over a `{lit}` corpus and the \
                 detector no longer sees it; the subject class has drifted off its own \
                 subject. Live subjects: {:?}",
                subjects.iter().map(|f| &f.file).collect::<Vec<_>>()
            );
        }
    }
}

/// RED HALF 1. The detector fires on an unanchored derived loop.
///
/// Driven on constructed input, so the control cannot go stale when the live
/// corpus changes — and built with `concat!` so this fixture is not itself a
/// derivation site when the gate scans its own file.
#[test]
fn the_detector_fires_on_an_unanchored_derived_loop() {
    let deriv = deriv_literal();
    let fixture = format!(
        r#"
fn corpus() -> Vec<String> {{
    let out = Command::new("git").args(["{deriv}", "*.md"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect()
}}

fn the_property_nobody_checks() {{
    for path in corpus() {{
        assert!(path.ends_with(".md"), "not markdown");
    }}
}}
"#
    );

    let found: Vec<Finding> = subject_sites("FIXTURE.rs", &fixture, &deriv_literals())
        .into_iter()
        .filter(|f| !f.anchored)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "the detector must flag an assertion loop over a `git {deriv}` corpus with no \
         floor; it reported {found:?}"
    );
    assert_eq!(found[0].function, "the_property_nobody_checks");
}

/// RED HALF 1b — THE NEW ARM, and it exists because RED HALF 1 does not cover
/// it. A control written against `git ls-files` says nothing about whether the
/// `read_dir` literal is wired in: delete `walk_literal()` from the set and
/// every fixture above still passes. This one reds.
#[test]
fn the_detector_fires_on_an_unanchored_directory_walk() {
    let walk = walk_literal();
    let fixture = format!(
        r#"
fn lane_files() -> Vec<String> {{
    std::fs::{walk}("contracts/lean-models/Models")
        .expect("the lane exists")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}}

fn every_module_is_registered() {{
    for m in lane_files() {{
        assert!(m.ends_with(".lean"), "stray file in the lane: {{m}}");
    }}
}}
"#
    );

    let found: Vec<Finding> = subject_sites("FIXTURE.rs", &fixture, &deriv_literals())
        .into_iter()
        .filter(|f| !f.anchored)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "the detector must flag an assertion loop over a `{walk}` corpus with no floor. \
         A directory that is absent, empty, or resolved against the wrong working \
         directory then skips every assertion in the loop while the test prints `ok`. \
         It reported {found:?}"
    );
    assert_eq!(found[0].function, "every_module_is_registered");
}

/// RED HALF 1c. The BUILDER form (B′) is not a blanket amnesty: a loop that
/// fills a collection nothing later floors is still unanchored.
///
/// Without this, (B′) could be read as "any `push` anywhere anchors the site",
/// which would quietly exempt the majority of the new arm.
#[test]
fn the_builder_form_still_requires_the_floor_it_is_named_for() {
    let walk = walk_literal();
    let fixture = format!(
        r#"
fn every_receipt_parses() {{
    let mut out = Vec::new();
    for entry in std::fs::{walk}("docs/status").expect("dir").flatten() {{
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(!name.is_empty(), "unnamed entry");
        out.push(name);
    }}
    eprintln!("scanned {{}} receipt(s)", out.len());
}}
"#
    );

    let found: Vec<Finding> = subject_sites("FIXTURE.rs", &fixture, &deriv_literals())
        .into_iter()
        .filter(|f| !f.anchored)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "a builder loop whose filled collection is only PRINTED after the loop — never \
         asserted on — must still be flagged; an `eprintln!` of `out.len()` is not a \
         floor. Got {found:?}"
    );
}

/// RED HALF 2, the other direction. An anchored loop is NOT flagged.
///
/// A detector that fires on everything is as useless as one that fires on
/// nothing, and it is the version that gets disabled.
#[test]
fn the_detector_accepts_an_anchored_derived_loop() {
    let deriv = deriv_literal();
    let fixture = format!(
        r#"
fn corpus() -> Vec<String> {{
    let out = Command::new("git").args(["{deriv}", "*.md"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect()
}}

fn the_property_that_is_checked() {{
    let paths = corpus();
    assert!(!paths.is_empty(), "the corpus came back empty");
    for path in &paths {{
        assert!(path.ends_with(".md"), "not markdown");
    }}
}}
"#
    );

    let found: Vec<Finding> = subject_sites("FIXTURE.rs", &fixture, &deriv_literals())
        .into_iter()
        .filter(|f| !f.anchored)
        .collect();
    assert!(
        found.is_empty(),
        "a loop whose corpus is floored must not be flagged, or the gate reds correct \
         files and gets disabled (PMAT-1500); got {found:?}"
    );
}

/// RED HALF 2b. The floor may live one or two CALLS below the collection the
/// loop names, and the detector must resolve through them.
///
/// This is PMAT-1508's own extractor bug, pinned. The first draft resolved
/// anchor (C) only when the loop iterated `helper()` *directly*, so binding the
/// result to a local first — `let modules = lane_modules(); for m in &modules`
/// — hid a textbook `assert!(!out.is_empty())` and reported a false positive on
/// a correct file. `snapshot_required()` needs three hops. A gate that cries
/// wolf gets disabled (PMAT-1505).
#[test]
fn the_detector_resolves_a_floor_through_the_call_chain() {
    let walk = walk_literal();
    let fixture = format!(
        r#"
fn lane_files() -> Vec<String> {{
    let out: Vec<String> = std::fs::{walk}("contracts/lean-models/Models")
        .expect("the lane exists")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!out.is_empty(), "the lane holds no modules");
    out
}}

fn lean_modules() -> Vec<String> {{
    lane_files().into_iter().filter(|n| n.ends_with(".lean")).collect()
}}

fn every_module_is_registered() {{
    let modules = lean_modules();
    for m in &modules {{
        assert!(m.ends_with(".lean"), "stray file in the lane: {{m}}");
    }}
}}
"#
    );

    let found: Vec<Finding> = subject_sites("FIXTURE.rs", &fixture, &deriv_literals())
        .into_iter()
        .filter(|f| !f.anchored)
        .collect();
    assert!(
        found.is_empty(),
        "the floor is two calls below the collection the loop names, and it protects \
         the loop exactly as if it were written in place. Flagging this reds a correct \
         file — and a per-site look that cannot see through a call is the instrument \
         error PMAT-1507 recorded. Got {found:?}"
    );
}

/// RED HALF 3. A loop with no assertion is not the subject.
///
/// An empty collection is only a defect when something was supposed to be
/// CHECKED for each element. Iterating nothing to build a list is ordinary.
#[test]
fn the_detector_ignores_a_derived_loop_that_asserts_nothing() {
    let deriv = deriv_literal();
    let fixture = format!(
        r#"
fn corpus() -> Vec<String> {{
    let out = Command::new("git").args(["{deriv}"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect()
}}

fn collect_them() -> usize {{
    let mut n = 0;
    for _path in corpus() {{
        n += 1;
    }}
    n
}}
"#
    );

    assert!(
        subject_sites("FIXTURE.rs", &fixture, &deriv_literals()).is_empty(),
        "a derived loop carrying no assertion is not this gate's subject"
    );
}

/// RED HALF 4 — THE SCOPE BOUNDARY, asserted rather than described, in BOTH
/// directions. This is PMAT-1508's `reading_a_file_is_not_a_derivation_this_
/// gate_governs` REWRITTEN for the widening rather than deleted by it, which is
/// the instruction its own failure message carried.
///
/// PMAT-1508 measured the naive widening and declined it: adding the read as a
/// BARE literal takes the subject class from 29 to 54 and leaves 9 unanchored
/// against 0 today. PMAT-1509 re-measured (same 9) and shipped the narrower
/// predicate instead. So the decline stands in its original form — the read is
/// never a bare derivation — and what changed is that a SELECTION out of a read
/// now is.
#[test]
fn reading_a_file_is_not_a_derivation_unless_something_was_selected_from_it() {
    let read = read_literal();
    assert!(
        !deriv_literals()
            .iter()
            .any(|d| matches!(d, Deriv::Bare(s) if *s == read)),
        "`{read}` was promoted to a BARE derivation. That widening was MEASURED and \
         declined TWICE (PMAT-1508, re-measured by PMAT-1509): it takes the subject \
         class from 29 sites to 54 and leaves 9 of them UNANCHORED against 0 today, so \
         it reds correct files on day one and the gate gets disabled (PMAT-1500). Six \
         of the nine are `for line in text.lines()` over a just-read file — an empty \
         iteration there means an empty FILE, not a missed SCAN, because the read \
         already panics when the file is absent. Keep it `Deriv::Selected`."
    );

    // (i) The excluded shape is LIVE, not hypothetical: a raw `.lines()` walk
    //     over a just-read file must stay out of scope.
    let raw = format!(
        r#"
fn no_page_overclaims() {{
    let text = std::fs::{read}("docs/README.md").expect("the page exists");
    for line in text.lines() {{
        assert!(!line.contains("ALL POSIX"), "overclaim: {{line}}");
    }}
}}
"#
    );
    assert!(
        subject_sites("FIXTURE.rs", &raw, &deriv_literals()).is_empty(),
        "a `for line in text.lines()` loop over a file that was just read is outside \
         this gate's subject; admitting it is the widening that was measured and declined"
    );

    // (ii) And the INCLUDED shape is live too, or the arm is decorative. One
    //      `.filter(` is the whole difference: the collection can now come back
    //      empty because the predicate matched nothing.
    let selected = format!(
        r#"
fn every_declared_id_is_registered() {{
    let text = std::fs::{read}("docs/roadmaps/roadmap.yaml").expect("the ledger exists");
    let ids: Vec<&str> = text.lines().filter(|l| l.contains("- id:")).collect();
    for id in &ids {{
        assert!(id.contains("PMAT-"), "malformed id: {{id}}");
    }}
}}
"#
    );
    let found: Vec<Finding> = subject_sites("FIXTURE.rs", &selected, &deriv_literals())
        .into_iter()
        .filter(|f| !f.anchored)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "one `.filter(` separates the two fixtures above, and it is the difference \
         between an empty FILE and a parser that matched NOTHING. If this is not \
         flagged, the third arm governs nothing at all. Got {found:?}"
    );
}

/// RED HALF 4b. `.split(` AND `.lines()` ARE RESHAPES, NOT SELECTIONS — the one
/// decision the whole third arm rests on, pinned so a later slice cannot widen
/// it by reflex.
///
/// PMAT-1507's `next_lane` entry listed `.split(` among the candidate extraction
/// steps. It is not one: `text.split(sep)` yields at least one element for every
/// non-empty input, so it cannot come back empty because a scan missed. Admitting
/// it drags in the six raw-`.lines()` residue sites measured above.
///
/// `shell_passthrough_disclosure_witness::paragraphs` is the live case, and it is
/// the reason the scan has to be DEPTH-AWARE: its pipeline is
/// `text.split("\n\n").map(|p| p.lines().filter(…).collect().join(" "))` — there
/// IS a `.filter(` in it, but it sits inside the `.map(` closure and filters the
/// CONTENT of each paragraph, never the number of them. A `contains(".filter(")`
/// predicate would have called that a selection and flagged a correct file.
#[test]
fn splitting_a_document_is_a_reshape_not_a_selection() {
    assert!(
        !selects_at_top_level(r#"text.split("\n\n").map(|p| p.trim()).collect()"#),
        "`.split(` reshapes; it cannot return fewer than one element for a non-empty \
         input, so its emptiness is an empty FILE and not a missed scan"
    );
    assert!(
        !selects_at_top_level(
            r#"text.split("\n\n").map(|p| p.lines().filter(|l| !l.starts_with('>')).collect::<Vec<_>>().join(" ")).collect()"#
        ),
        "the `.filter(` here is INSIDE the `.map(` closure — it selects lines within \
         each paragraph, never paragraphs. Reading it as a selection flags \
         `shell_passthrough_disclosure_witness::paragraphs`, which is correct code"
    );
    assert!(
        selects_at_top_level(r#"lines.iter().filter(|l| l.starts_with("- id:")).collect()"#),
        "a depth-0 `.filter(` IS a selection — it can match nothing"
    );

    // And the live consequence, end to end.
    let read = read_literal();
    let fixture = format!(
        r#"
fn paragraphs(text: &str) -> Vec<String> {{
    text.split("\n\n")
        .map(|p| p.lines().filter(|l| !l.starts_with('>')).collect::<Vec<_>>().join(" "))
        .collect()
}}

fn no_document_makes_a_universal_claim() {{
    let text = std::fs::{read}("CLAUDE.md").expect("the page exists");
    for para in paragraphs(&text) {{
        assert!(!para.contains("everything else refuses"), "universal: {{para}}");
    }}
}}
"#
    );
    assert!(
        subject_sites("FIXTURE.rs", &fixture, &deriv_literals()).is_empty(),
        "a paragraph RESHAPE of a just-read document is not a selection, and flagging \
         it reds a correct file"
    );
}

/// PMAT-1506's lesson, made an assertion: a `git ls-files` corpus cannot see an
/// UNTRACKED file, so a new gate does not analyse ITSELF until it is committed.
/// That is how PMAT-1506's gate went green locally and red on CI. This asserts
/// the self-analysis is real, and that it comes back clean.
#[test]
fn this_gate_is_inside_its_own_corpus() {
    let deriv = deriv_literal();
    let files = tracked_test_sources();
    let me = "derived_corpus_vacuity_witness.rs";

    let mine = files.iter().find(|(rel, _)| rel.ends_with(me));
    let Some((rel, src)) = mine else {
        panic!(
            "{me} is not in its own `git {deriv}` corpus. Until this file is COMMITTED \
             it cannot analyse itself, and a gate that has never been run against its \
             own source is the one shape it is least able to detect (PMAT-1506)."
        );
    };

    // PMAT-1510 — an assertion stood here that could not fail, stating a
    // property that was already false.
    //
    // It read `!src.contains(&lit) || src.contains("concat!")`. This file uses
    // `concat!` to split its fixture literals, so the right disjunct is
    // UNCONDITIONALLY TRUE and the assertion had no failing input. Its message
    // claimed the file "must not spell `ls-files` contiguously" — measured, the
    // file spells `ls-files` 16 times and `read_dir` 11 times contiguously, in
    // its own doc comment and code. So a check that could never fire was also
    // asserting something untrue about the file it was reading.
    //
    // Nothing replaces it, deliberately. The property that actually matters —
    // that this gate's own source contains no unanchored derived loop — is the
    // assertion immediately below, which is quantified over `subject_sites` of
    // this very file and CAN fail. Splitting fixture literals remains a real
    // convention; it is enforced by that check finding no live site here, not
    // by a substring ban that never fired.

    let self_findings: Vec<Finding> = subject_sites(rel, src, &deriv_literals())
        .into_iter()
        .filter(|f| !f.anchored)
        .collect();
    assert!(
        self_findings.is_empty(),
        "this gate reports {} unanchored site(s) in its own source: {self_findings:?}",
        self_findings.len()
    );
}

/// RED HALF 5. A PRINT IS NOT A FLOOR — the anchor rule's own hollow-check
/// hole, pinned.
///
/// PMAT-1507 shipped `whole.contains("files.len()")` as anchor (A), and an
/// `eprintln!("scanned {} files", files.len())` satisfies it: the corpus comes
/// back empty, the line prints `0`, every assertion in the loop is skipped, and
/// the test exits 0. A gate against hollow checks admitting one through its own
/// anchor rule is the shape PMAT-1506 found and PMAT-1504 named. The predicate
/// now requires the length to be read INSIDE an assertion, and this control is
/// what keeps it that way.
///
/// Nothing in the live corpus relied on the loose reading — measured 2026-07-31,
/// tightening it moved the `git ls-files` arm not at all — so this is a hole
/// closed before it was load-bearing, not a repair of a live failure.
#[test]
fn a_printed_length_does_not_anchor_a_derived_loop() {
    let walk = walk_literal();
    let fixture = format!(
        r#"
fn every_receipt_is_named_for_its_ruleset() {{
    let files: Vec<String> = std::fs::{walk}("docs/status")
        .expect("dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    eprintln!("XPILE-RULESET-001: scanned {{}} receipt(s)", files.len());
    for f in &files {{
        assert!(f.starts_with("ruleset-"), "stray receipt: {{f}}");
    }}
}}
"#
    );

    let found: Vec<Finding> = subject_sites("FIXTURE.rs", &fixture, &deriv_literals())
        .into_iter()
        .filter(|f| !f.anchored)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "an `eprintln!` of the corpus length is a DISCLOSURE, not a floor — it cannot \
         red when the walk comes back empty. It must not anchor the loop. Got {found:?}"
    );
}

/// RED HALF 6. The counted-into-a-local floor is accepted, and it must be,
/// because `claims_drift` is written that way.
///
/// The pair with RED HALF 5: one indirection through `let n = xs.len();` is
/// still a floor when an ASSERTION reads `n`, and is still not one when only a
/// print does. Without both arms, tightening the predicate looks like a free
/// win instead of a boundary that had to be placed.
#[test]
fn a_length_bound_to_a_local_and_asserted_still_anchors() {
    let walk = walk_literal();
    let fixture = format!(
        r#"
fn every_receipt_is_named_for_its_ruleset() {{
    let files: Vec<String> = std::fs::{walk}("docs/status")
        .expect("dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let scanned = files.len();
    assert!(scanned >= 2, "the walk reached {{scanned}} receipt(s); 2 exist");
    for f in &files {{
        assert!(f.starts_with("ruleset-"), "stray receipt: {{f}}");
    }}
}}
"#
    );

    let found: Vec<Finding> = subject_sites("FIXTURE.rs", &fixture, &deriv_literals())
        .into_iter()
        .filter(|f| !f.anchored)
        .collect();
    assert!(
        found.is_empty(),
        "`let scanned = files.len();` followed by an assertion on `scanned` floors the \
         corpus as surely as asserting on `files.len()` in place; flagging it reds a \
         correct file. Got {found:?}"
    );
}

/// RED HALF 7. An existence probe anchors — and only when it is one.
///
/// `xs.iter().find(…).expect("…")` cannot survive an empty `xs`, so a loop
/// under it cannot silently iterate nothing. `find(…)` alone can: it yields
/// `None` and, if nothing unwraps it, the empty corpus passes straight through.
#[test]
fn an_existence_probe_anchors_but_a_bare_find_does_not() {
    let walk = walk_literal();
    let body = |tail: &str| {
        format!(
            r#"
fn declared_layers() -> Vec<String> {{
    std::fs::{walk}("contracts")
        .expect("contracts/ exists")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}}

fn the_short_alias_stays_unambiguous() {{
    let who = declared_layers();
    {tail}
    for d in &who {{
        assert!(!d.contains("Xlate"), "{{d}} took an ambiguous alias");
    }}
}}
"#
        )
    };

    let probed = body(
        r#"let notation = who.iter().find(|d| d.starts_with("C-NOTATION")).expect("declared");
    assert!(notation.ends_with(".yaml"), "not a contract file");"#,
    );
    let found: Vec<Finding> = subject_sites("FIXTURE.rs", &probed, &deriv_literals())
        .into_iter()
        .filter(|f| !f.anchored)
        .collect();
    assert!(
        found.is_empty(),
        "`.find(…).expect(…)` panics on an empty corpus, so the loop below it cannot \
         iterate nothing unnoticed; flagging it reds a correct file. Got {found:?}"
    );

    let unprobed = body(
        r#"let notation = who.iter().find(|d| d.starts_with("C-NOTATION"));
    assert!(notation.is_none() || notation.is_some(), "always true");"#,
    );
    let found: Vec<Finding> = subject_sites("FIXTURE.rs", &unprobed, &deriv_literals())
        .into_iter()
        .filter(|f| !f.anchored)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "a bare `find(…)` yields `None` on an empty corpus and nothing unwraps it, so \
         the loop below still iterates nothing at exit 0. It must not anchor. \
         Got {found:?}"
    );
}

/// RED HALF 8 — **A FLOOR ABOVE A FILTER IS NOT A FLOOR BELOW IT**, and this is
/// PMAT-1509's own finding about the anchor rule PMAT-1508 shipped.
///
/// Anchor (C) resolves a floor anywhere in the derivation CHAIN, which is what
/// killed four of PMAT-1508's six false positives. Unrestricted, it also hands
/// out amnesty ACROSS a selection.
/// `ci_tool_install::every_path_install_curl_fails_on_http_error` is the live
/// case: it iterates `path_install_steps()`, and the only floor in its chain is
/// `workflow_files()`'s `assert!(!files.is_empty())` — which floors the WORKFLOW
/// FILES, *upstream* of the `.filter(|s| s.body.contains("GITHUB_PATH"))` that
/// produces the loop's collection. Every workflow file can be present and the
/// filter still match nothing.
///
/// ⚠️ **HONEST HALF: nothing was vacuous.** `path_install_steps()` returns 5
/// today (measured by instrumentation, 2026-07-31). This is a hole in the anchor
/// rule closed before it was load-bearing — the same shape and the same honest
/// reading as PMAT-1508's `a_printed_length_does_not_anchor_a_derived_loop` —
/// not a save. The site is floored in this slice regardless, because the rule
/// now demands it.
#[test]
fn a_floor_above_a_filter_does_not_anchor_below_it() {
    let read = read_literal();
    let body = |floor_in_selector: &str| {
        format!(
            r#"
fn workflow_files() -> Vec<String> {{
    let files: Vec<String> = std::fs::{read}(".github/workflows/ci.yml")
        .expect("ci.yml exists")
        .lines()
        .map(str::to_string)
        .collect();
    assert!(!files.is_empty(), "no workflow files");
    files
}}

fn path_install_steps() -> Vec<String> {{
    let steps: Vec<String> = workflow_files()
        .into_iter()
        .filter(|s| s.contains("GITHUB_PATH"))
        .collect();
    {floor_in_selector}
    steps
}}

fn every_path_install_curl_fails_on_http_error() {{
    for step in path_install_steps() {{
        assert!(step.contains("-f"), "curl without -f: {{step}}");
    }}
}}
"#
        )
    };

    let upstream_only = subject_sites("FIXTURE.rs", &body(""), &deriv_literals());
    let unanchored: Vec<&Finding> = upstream_only.iter().filter(|f| !f.anchored).collect();
    assert_eq!(
        unanchored.len(),
        1,
        "the only floor sits ABOVE the `.filter(` that builds the loop's collection. \
         It says the workflow files are present; it says nothing about whether the \
         filter matched. Accepting it is amnesty across a selection. Got {upstream_only:?}"
    );

    // The dual: a floor BELOW the selection does anchor, or the rule is just
    // "selected sites can never be anchored through a helper" — useless.
    let with_floor = subject_sites(
        "FIXTURE.rs",
        &body(r#"assert!(!steps.is_empty(), "no $GITHUB_PATH install steps");"#),
        &deriv_literals(),
    );
    assert!(
        !with_floor.is_empty() && with_floor.iter().all(|f| f.anchored),
        "a floor on the SELECTED collection, inside the selecting helper, anchors the \
         loop exactly as if it were written at the call site. Got {with_floor:?}"
    );
}

/// RED HALF 9 — A SKIP-GUARD IS NOT A FLOOR, BUT A SKIP-GUARD PLUS A COUNTER IS.
/// Anchor (D), both directions.
///
/// `cli_spirv_input_fidelity_witness` writes the pattern that forced this:
/// `if outs.len() < 2 { continue; }` / `emitting_targets += 1;` / the loop /
/// `assert!(emitting_targets >= 4)`. The guard ALONE is PMAT-1505's defect
/// verbatim — a skip that reports success — and must not anchor. The pair does:
/// four targets got past a guard that requires a non-empty collection, so the
/// loop demonstrably had something to iterate.
#[test]
fn a_skip_guard_alone_does_not_anchor_a_selected_loop() {
    let read = read_literal();
    let body = |counter: &str, floor: &str| {
        format!(
            r#"
fn no_target_emits_the_same_artifact_for_different_programs() {{
    let src = std::fs::{read}("fixtures/list.txt").expect("the list exists");
    let mut reached = 0usize;
    for t in ["rust", "wasm"] {{
        let outs: Vec<String> = src.lines()
            .filter_map(|n| emit(n, t))
            .collect();
        if outs.len() < 2 {{
            continue;
        }}
        {counter}
        for i in 0..outs.len() {{
            assert_ne!(outs[i], outs[0], "identical artifacts");
        }}
    }}
    {floor}
}}
"#
        )
    };

    let guard_only = subject_sites("FIXTURE.rs", &body("", ""), &deriv_literals());
    let unanchored: Vec<&Finding> = guard_only.iter().filter(|f| !f.anchored).collect();
    assert_eq!(
        unanchored.len(),
        1,
        "`if outs.len() < 2 {{ continue; }}` SKIPS on an empty collection — it is the \
         PMAT-1505 defect, not a floor. Nothing here reds when every target refuses. \
         Got {guard_only:?}"
    );

    let paired = subject_sites(
        "FIXTURE.rs",
        &body(
            "reached += 1;",
            r#"assert!(reached >= 1, "no target emitted for the sample");"#,
        ),
        &deriv_literals(),
    );
    assert!(
        !paired.is_empty() && paired.iter().all(|f| f.anchored),
        "guard + counter + a later floor on the counter DOES anchor: the assertion \
         cannot pass unless something got past a guard that requires a non-empty \
         collection. Flagging it reds a correct file. Got {paired:?}"
    );
}

/// RED HALF 10 — A CORRECTLY-EMPTY NEGATIVE SCAN IS ANCHORED BY ITS
/// CONSTRUCTED-INPUT CONTROL, and this is what lets the arm ship with **no
/// exemption list**.
///
/// `release_runbook_facts_witness.rs:277` iterates `cited_manifest_lines(&
/// runbook())`, measured at **0** iterations by PMAT-1507 and again by
/// PMAT-1509, and it is CORRECT: the runbook carries no live `Cargo.toml:<N>`
/// citation and should not. `the_citation_detector_can_still_fire` is what keeps
/// that from being a silent pass — it drives the same extractor on constructed
/// input and asserts it returns `[43]`.
///
/// Recognising that is strictly better than naming the file in an exemption
/// list: the anchor is DERIVED, so it evaporates the moment the control stops
/// flooring the extractor. PMAT-1495's exemption trap has fired on eight
/// consecutive slices; this arm gives it nothing to fire on.
#[test]
fn a_constructed_input_control_anchors_a_correctly_empty_scan() {
    let read = read_literal();
    let body = |control: &str| {
        format!(
            r#"
fn cited_lines(note: &str) -> Vec<usize> {{
    let mut cited = Vec::new();
    for (i, l) in note.lines().enumerate() {{
        if l.contains("Cargo.toml:") {{
            cited.push(i);
        }}
    }}
    cited
}}

fn runbook() -> String {{
    std::fs::{read}("docs/RELEASE.md").expect("the runbook exists")
}}

{control}

fn a_citation_points_at_what_it_claims() {{
    let n = runbook();
    let cited = cited_lines(&n);
    for line_no in cited {{
        assert!(line_no > 0, "line 0 cannot be cited");
    }}
}}
"#
        )
    };

    let controlled = subject_sites(
        "FIXTURE.rs",
        &body(
            r#"
fn the_citation_detector_can_still_fire() {
    assert_eq!(cited_lines("see Cargo.toml:43 for it"), vec![0], "detector is dead");
}
"#,
        ),
        &deriv_literals(),
    );
    assert!(
        !controlled.is_empty() && controlled.iter().all(|f| f.anchored),
        "the extractor is driven on CONSTRUCTED input by an assertion that floors its \
         output, so its emptiness on the live corpus is a measured property rather than \
         a silent miss. Flagging it forces an exemption list, which is the thing this \
         file refuses to have. Got {controlled:?}"
    );

    // The dual, and it is the one that keeps (E) from being a blanket amnesty:
    // a "control" that only ever asserts the result is EMPTY proves nothing —
    // a detector that always returns nothing satisfies it.
    let empty_control = subject_sites(
        "FIXTURE.rs",
        &body(
            r#"
fn the_runbook_cites_nothing() {
    assert!(cited_lines(&runbook()).is_empty(), "a live citation appeared");
}
"#,
        ),
        &deriv_literals(),
    );
    let unanchored: Vec<&Finding> = empty_control.iter().filter(|f| !f.anchored).collect();
    assert_eq!(
        unanchored.len(),
        1,
        "a control that only asserts EMPTINESS is satisfied by a detector that has \
         died — it is the tautology shape PMAT-1505 found inside `pins_are_not_vacuous`. \
         Only a control that floors the extractor on constructed input anchors. \
         Got {empty_control:?}"
    );
}

/// RED HALF 11 — A PRINTED COUNTER IS NOT A FLOOR EITHER. PMAT-1508 closed this
/// hole in anchor (A); PMAT-1509 found the same one still open in (B).
///
/// (B) shipped as `after.contains(&counter) && has_assert(&after)` — two
/// independent tests ANDed: *the counter's name appears somewhere below* and
/// *some assertion exists somewhere below*. An `eprintln!("scanned {n} steps")`
/// followed by any unrelated assertion satisfies both, and the counter is never
/// actually floored. It is the identical shape to the `eprintln!(… files.len())`
/// that PMAT-1508 found in (A), one anchor over.
///
/// ⚠️ **HONEST HALF, measured:** tightening it to *the counter must be read
/// inside an assertion* moved **no live site** — all 54 subjects keep the
/// anchoring they had. Another hole closed before it was load-bearing, not a
/// save. Recording that is the point; a sweep that reports only refutations
/// reads as a hunt for embarrassments (PMAT-1505).
#[test]
fn a_printed_counter_does_not_anchor_a_derived_loop() {
    let walk = walk_literal();
    let body = |tail: &str| {
        format!(
            r#"
fn every_receipt_is_named_for_its_ruleset() {{
    let files: Vec<String> = std::fs::{walk}("docs/status")
        .expect("dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let mut seen = 0usize;
    for f in &files {{
        seen += 1;
        assert!(f.starts_with("ruleset-"), "stray receipt: {{f}}");
    }}
    {tail}
}}
"#
        )
    };

    let printed = body(
        r#"eprintln!("XPILE-RULESET-001: {seen} receipt(s)");
    assert!(true, "an unrelated assertion");"#,
    );
    let found: Vec<Finding> = subject_sites("FIXTURE.rs", &printed, &deriv_literals())
        .into_iter()
        .filter(|f| !f.anchored)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "an `eprintln!` of the counter cannot red when the walk comes back empty, and \
         an unrelated assertion below it says nothing about the counter. ANDing \
         `contains(counter)` with `has_assert(anything)` is two checks that never meet. \
         Got {found:?}"
    );

    let asserted = body(r#"assert!(seen >= 2, "the walk reached {seen} receipt(s)");"#);
    let found: Vec<Finding> = subject_sites("FIXTURE.rs", &asserted, &deriv_literals())
        .into_iter()
        .filter(|f| !f.anchored)
        .collect();
    assert!(
        found.is_empty(),
        "a counter READ BY an assertion is a floor — it is the anchor (B) this file has \
         accepted since PMAT-1507, and flagging it reds correct files. Got {found:?}"
    );
}

/// THE REACH, PRINTED. PMAT-1507's lesson — *a rule's reach is its SUBJECT, not
/// the corpus it walks* — kept honest by making the cardinality an assertion
/// with a disclosure rather than a sentence in the header.
///
/// The scan reads all tracked `crates/*/tests/*.rs`; the sites it GOVERNS are
/// the ones that both derive in scope AND assert inside the loop.
#[test]
fn the_gate_prints_its_subject_cardinality_by_arm() {
    let mut total = 0usize;
    for arm in deriv_literals() {
        let one = vec![arm.clone()];
        let mut n = 0usize;
        for (rel, src) in &tracked_test_sources() {
            n += subject_sites(rel, src, &one).len();
        }
        eprintln!(
            "XPILE-SKIPGUARD-003: arm `{}` governs {n} site(s)",
            arm.literal()
        );
        total += n;
    }
    let union = live_subjects(&deriv_literals()).len();
    eprintln!("XPILE-SKIPGUARD-003: union subject class = {union} site(s)");
    assert!(
        union >= 40 && total >= union,
        "the union subject class is {union} site(s) against a floor of 40 measured \
         2026-07-31: the arms govern 2 (`ls-files`) + 27 (`read_dir`) + 34 (selected \
         doc-parse) = 63 memberships over 44 distinct sites, because a site that walks \
         a directory AND selects out of a read belongs to two arms. A collapse here \
         means the detector stopped matching, not that the corpus got safer."
    );
}
