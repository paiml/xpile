//! XPILE-REFUSECLAIM-001 (PMAT-1438) — a claim class is not a paragraph.
//!
//! THE DEFECT. PMAT-1437 (5e334e95) measured, and corrected, a falsehood the
//! book had published since 2026-05-15: that every backend's refusal "names the
//! governing contract and, where one exists, a better `--target`", attributed
//! to `C-XPILE-BACKEND-TRAIT`. It rewrote `backends.md`'s header blockquote and
//! its guarantee paragraph, and §1 of `adding-a-backend.md`. Its commit message
//! says why the second one mattered:
//!
//! > `adding-a-backend.md` repeated the same list as IMPLEMENTATION
//! > INSTRUCTIONS, so a contributor was told to satisfy a requirement that
//! > neither the contract states nor most shipped backends meet.
//!
//! The identical claim survived in FOUR more places, in three of the same four
//! files, and one of them is 100 lines below the machine-derived table that
//! refutes it:
//!
//! | site | mood |
//! |---|---|
//! | `book/src/reference/backends.md` §Error handling | a numbered normative `must` |
//! | `book/src/reference/backends.md` §Error handling | attribution to a "structural compile-contract citation" invariant |
//! | `book/src/reference/cli.md` body | universal indicative |
//! | `book/src/tutorials/shell-roundtrip.md` | generalisation from one true example + the same attribution |
//! | `book/src/contributing/adding-a-backend.md` §3 | a code comment, i.e. instructions again |
//!
//! So `backends.md` asserted at line 167 what it refuted at line 78, in one
//! commit, and `adding-a-backend.md` fixed the instruction in §1 and reissued
//! it in §3.
//!
//! WHY NO GATE SAW IT. PMAT-1437 built
//! `backend_refusal_disclosure_witness.rs::
//! every_invariant_the_book_attributes_to_a_contract_is_an_equation_key`
//! for exactly this class. It computes its subject as
//!
//! ```text
//! // The page's header blockquote: the first run of `>` lines.
//! let quote: Vec<&str> = body.lines().skip_while(|l| !l.starts_with('>')).collect();
//! ```
//!
//! — a HEADER-ONLY check. Two of the five sites are in page BODIES, so it
//! cannot reach them at any strictness. The other three name no contract ID at
//! all, so no attribution gate of any spelling could reach them: what they
//! spell is the REQUIREMENT, not its attribution.
//!
//! That is the generalisable lesson, and it is PMAT-1437's own lesson turned on
//! its author: **ask what the defect SPELLED.** 1437 learned it once (its first
//! draft checked backticked `equations:` keys, and the original falsehood
//! contained no backticked key), fixed the site in front of it, and shipped a
//! gate whose SUBJECT — the header blockquote — was still narrower than the
//! class. A claim class is not a paragraph, and it is not a file either.
//!
//! WHAT THIS FILE PINS. Two rules, each keyed on what one group of defects
//! spelled, neither reachable by the header-only check:
//!
//! 1. `an_invariant_named_in_quotes_beside_a_contract_id_must_be_an_equation_key`
//!    — anywhere in a page, not just its header. Both body sites name the
//!    invariant with a DOUBLE-QUOTED PROSE PHRASE ("structural compile-contract
//!    citation"), which resembles but is not the key `compile_contract_citation`.
//!    A quoted phrase has no referent to check; a key does.
//!
//! 2. `prose_asserting_what_a_refusal_message_contains_must_disclaim_or_link`
//!    — the three sites that name no contract. Saying a refusal names the
//!    contract or suggests a target is fine where it is true; asserting it
//!    without a disclaimer or a link to the measured table is what was false.
//!
//! HISTORICAL NOTE PARAGRAPHS ARE EXEMPT, deliberately and narrowly. This repo
//! discloses corrections in place ("Through v0.1.617 this line said …"), and a
//! gate that forbade restating the old claim would forbid the disclosure that
//! makes the correction legible. The exemption requires the literal phrase
//! `Through v0.1.` in the same paragraph. NEITHER original defect carried it —
//! verified below against their verbatim text, not asserted — so the exemption
//! does not weaken the gate against the thing it was built for. It can of
//! course be abused by pasting the phrase onto a fresh falsehood; that is true
//! of every disclosure-aware gate and is said out loud here rather than left
//! for someone to discover.
//!
//! NON-VACUITY IS BY CONSTRUCTION, not by a count. Both detectors are run over
//! the offending paragraphs' VERBATIM pre-fix text, embedded below, and must
//! flag them. A future edit that softens either detector into a no-op reds
//! here even if the book corpus has meanwhile been rewritten to contain no
//! instances at all — the failure mode a corpus-only check has (PMAT-1396: a
//! negative over an empty enumeration passes for free).
//!
//! MEASURED, AND HONEST, so it is not re-derived: the shell-roundtrip
//! TRANSCRIPT itself is accurate. `xpile transpile script.sh --target rust`
//! really does emit that message, contract and target and all — `rust` is one
//! of the backends whose refusals do both. The falsehood was the sentence
//! GENERALISING from it, not the example. `qa_gate.rs`, named in
//! `adding-a-backend.md` as the citation gate, also exists.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

/// Every `*.md` under `book/src/`, recursively, as (repo-relative path, body).
fn book_pages(root: &Path) -> Vec<(String, String)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            if p.is_dir() {
                walk(&p, root, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("md") {
                let rel = p
                    .strip_prefix(root)
                    .expect("book page under workspace root")
                    .to_string_lossy()
                    .into_owned();
                let body =
                    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {rel}: {e}"));
                out.push((rel, body));
            }
        }
    }
    let mut out = Vec::new();
    walk(&root.join("book/src"), root, &mut out);
    out
}

/// A page split into paragraphs, each flattened to one line.
///
/// Flattening matters: the claims this file pins are sentences, and a sentence
/// in a hard-wrapped Markdown file straddles line breaks, so a line-oriented
/// scan silently misses every claim that happens to wrap.
///
/// A LIST ITEM IS ITS OWN PARAGRAPH, and that is load-bearing in both
/// directions. Bullets carry no blank line between them, so joining a whole
/// list into one blob (a) merges a claim with its neighbours' vocabulary — the
/// first draft flagged a "What's next" bullet in `python-to-lean.md` because
/// the bullet BELOW it mentioned "invariants" — and (b) hides the
/// `2. Names the governing contract` item this file exists for inside a blob
/// whose other items are innocuous.
fn paragraphs(body: &str) -> Vec<(usize, String)> {
    fn starts_item(line: &str) -> bool {
        let t = line.trim_start();
        // A bullet, a table row, or an ordered item (`2. `) — the form the
        // `must` list this file exists for was written in.
        let ordered = t
            .split_once(". ")
            .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        t.starts_with("- ") || t.starts_with("* ") || t.starts_with("| ") || ordered
    }

    let mut out = Vec::new();
    let mut start = 1usize;
    let mut buf: Vec<&str> = Vec::new();
    let flush = |buf: &mut Vec<&str>, start: usize, out: &mut Vec<(usize, String)>| {
        if !buf.is_empty() {
            out.push((start, buf.join(" ")));
            buf.clear();
        }
    };
    for (i, line) in body.lines().enumerate() {
        if line.trim().is_empty() {
            flush(&mut buf, start, &mut out);
            start = i + 2;
        } else {
            if starts_item(line) {
                flush(&mut buf, start, &mut out);
            }
            if buf.is_empty() {
                start = i + 1;
            }
            buf.push(line);
        }
    }
    flush(&mut buf, start, &mut out);
    out
}

/// The contract IDs named in a string — `C-` followed by upper-case segments.
fn contract_ids(s: &str) -> BTreeSet<String> {
    let bytes: Vec<char> = s.chars().collect();
    let mut out = BTreeSet::new();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == 'C' && bytes[i + 1] == '-' && (i == 0 || !bytes[i - 1].is_alphanumeric()) {
            let mut j = i + 2;
            while j < bytes.len()
                && (bytes[j].is_ascii_uppercase() || bytes[j].is_ascii_digit() || bytes[j] == '-')
            {
                j += 1;
            }
            let id: String = bytes[i..j].iter().collect();
            let id = id.trim_end_matches('-').to_string();
            // `C-` plus at least two dash-separated segments; this rejects
            // incidental matches like a bare `C-` or `C-1`.
            if id.matches('-').count() >= 2 {
                out.insert(id);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// The double-quoted phrases in a string, straight quotes only.
fn double_quoted(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(open) = rest.find('"') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else { break };
        let phrase = after[..close].trim().to_string();
        if !phrase.is_empty() {
            out.push(phrase);
        }
        rest = &after[close + 1..];
    }
    out
}

/// A paragraph that states, in the present tense, that a contract pins a named
/// invariant — where the name is given as a quoted prose phrase rather than as
/// an `equations:` key. Returns (contract id, quoted phrase) pairs.
///
/// This is the detector. It is a free function so the tests below can run it
/// over the pre-fix text as well as over the live corpus.
fn quoted_invariant_attributions(paragraph: &str) -> Vec<(String, String)> {
    if !paragraph.contains("invariant") {
        return Vec::new();
    }
    // The repo discloses superseded claims in place. A historical note is not
    // an attribution.
    if paragraph.contains("Through v0.1.") {
        return Vec::new();
    }
    let ids = contract_ids(paragraph);
    if ids.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for phrase in double_quoted(paragraph) {
        // A quoted phrase is being used as an invariant's NAME only if it
        // reads like one: several lower-case words, no sentence punctuation.
        let wordish = phrase
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_');
        if !wordish || phrase.split_whitespace().count() < 2 {
            continue;
        }
        for id in &ids {
            out.push((id.clone(), phrase.clone()));
        }
    }
    out
}

/// The `equations:` keys of a contract, by ID. The YAML corpus is scanned for
/// the file whose `metadata.id` matches, so a rename cannot silently turn the
/// check into a no-op.
fn equation_keys(root: &Path, id: &str) -> BTreeSet<String> {
    #[derive(serde::Deserialize)]
    struct Doc {
        metadata: Meta,
        #[serde(default)]
        equations: BTreeMap<String, serde_yaml::Value>,
    }
    #[derive(serde::Deserialize)]
    struct Meta {
        id: String,
    }

    let dir = root.join("contracts");
    let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("read_dir contracts: {e}"));
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let body = std::fs::read_to_string(&path).unwrap_or_default();
        let Ok(doc) = serde_yaml::from_str::<Doc>(&body) else {
            continue;
        };
        if doc.metadata.id == id {
            return doc.equations.into_keys().collect();
        }
    }
    BTreeSet::new()
}

/// `structural compile-contract citation` → `structural_compile_contract_citation`.
fn as_key(phrase: &str) -> String {
    phrase
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

// ---------------------------------------------------------------------------
// The two paragraphs, VERBATIM, as they stood at 5e334e95. Non-vacuity is
// established against these rather than against a corpus count, so softening a
// detector reds here even if the corpus no longer contains an instance.
// ---------------------------------------------------------------------------

const PREFIX_BACKENDS_ATTRIBUTION: &str = "This is the `C-XPILE-BACKEND-TRAIT` \"structural compile-contract citation\" invariant — see the [contract reference](contracts.md#c-xpile-backend-trait).";

const PREFIX_SHELL_ATTRIBUTION: &str = "The error message **names the governing contract** and **suggests the correct target**. This is the [`C-XPILE-BACKEND-TRAIT`](../reference/contracts.md#c-xpile-backend-trait) contract's \"structural compile-contract citation\" invariant in action.";

const PREFIX_BACKENDS_MUST: &str =
    "When a backend cannot lower a particular construct it must fail with an error message that:";

const PREFIX_BACKENDS_MUST_ITEM_2: &str =
    "2. Names the governing contract (e.g. `C-BASHRS-POSIX-IDEMPOTENCE`).";

const PREFIX_BACKENDS_MUST_ITEM_3: &str =
    "3. Suggests the correct target if one exists (`use --target shell`).";

const PREFIX_CLI_BODY: &str = "If a backend cannot lower a particular construct, the error message names the governing contract and suggests the correct target — see the [shell-roundtrip tutorial](../tutorials/shell-roundtrip.md) for an example.";

const PREFIX_CONTRIBUTING_COMMENT: &str = "// 4. On unsupported constructs, return an error naming the //    construct, the governing contract, and the suggested //    target.";

/// `book/src/reference/contracts.md`, the page every other page LINKS TO for
/// what a contract says, under its own `## C-XPILE-BACKEND-TRAIT` heading.
/// PMAT-1437 corrected three pages' descriptions of this contract and left the
/// authority they all cite asserting the error-path requirement outright.
const PREFIX_CONTRACT_REFERENCE: &str = "Every `Backend` emission must carry a structural contract citation (`// xpile-contract: <ID>`). Error paths must name the governing contract.";

/// Prose asserting what a refusal MESSAGE contains. These are the spellings the
/// five defects used — including the code-comment spelling, which no
/// "names the governing contract" needle reaches, and which is exactly the
/// miss this file's whole lesson is about.
const REQUIREMENT_NEEDLES: &[&str] = &[
    "names the governing contract",
    "name the governing contract",
    "naming the governing contract",
    "the governing contract, and the suggested target",
    "suggests the correct target",
    "suggest the correct target",
    "suggesting a better",
    "target-suggestion",
];

/// A paragraph carrying a REQUIREMENT_NEEDLE is honest if it also says the
/// thing is not guaranteed, or sends the reader to the measured table.
const DISCLAIMER_TOKENS: &[&str] = &[
    "house style",
    "not an invariant",
    "not a guarantee",
    "not a requirement",
    "Through v0.1.",
    "backends.md#status",
    "(#status)",
];

/// Lower-case, with code-comment continuation markers removed and runs of
/// whitespace collapsed.
///
/// The `//` strip is not cosmetic. `adding-a-backend.md` §3 makes the claim
/// inside a wrapped `//` comment, so the flattened text reads
/// `… the governing contract, and the suggested //    target.` — a needle
/// written from the prose sites matches four of the five and silently misses
/// the one that is INSTRUCTIONS TO CONTRIBUTORS, which is the site PMAT-1437's
/// commit message singled out as the expensive one.
fn normalise(paragraph: &str) -> String {
    let stripped = paragraph.replace("//", " ");
    stripped
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn requirement_hits(paragraph: &str) -> Vec<&'static str> {
    let hay = normalise(paragraph);
    REQUIREMENT_NEEDLES
        .iter()
        .copied()
        .filter(|n| hay.contains(&normalise(n)))
        .collect()
}

fn is_disclaimed(paragraph: &str) -> bool {
    DISCLAIMER_TOKENS.iter().any(|t| paragraph.contains(t))
}

// ---------------------------------------------------------------------------
// Rule 1 — attribution, anywhere on the page.
// ---------------------------------------------------------------------------

#[test]
fn an_invariant_named_in_quotes_beside_a_contract_id_must_be_an_equation_key() {
    let root = workspace_root();
    let mut offenders: Vec<String> = Vec::new();

    for (rel, body) in book_pages(&root) {
        for (line, para) in paragraphs(&body) {
            for (id, phrase) in quoted_invariant_attributions(&para) {
                let known = equation_keys(&root, &id);
                if known.is_empty() {
                    offenders.push(format!(
                        "{rel}:{line}: names {id}, which no contract in contracts/ declares"
                    ));
                    continue;
                }
                if !known.contains(&as_key(&phrase)) {
                    offenders.push(format!(
                        "{rel}:{line}: calls {phrase:?} an invariant of {id}, but {id} has no \
                         such `equations:` key (closest by name: {:?})",
                        known
                            .iter()
                            .filter(|k| phrase
                                .split_whitespace()
                                .any(|w| k.contains(&w.to_ascii_lowercase())))
                            .collect::<Vec<_>>()
                    ));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "\na book page names an invariant of a contract with a quoted prose phrase that is not \
         one of its `equations:` keys:\n  {}\n\
         A quoted phrase has no referent that can be checked. Name a key that exists, or say \
         plainly that the contract does not pin it.",
        offenders.join("\n  ")
    );
}

#[test]
fn the_attribution_detector_flags_the_paragraphs_it_was_built_for() {
    // NON-VACUITY. Both pre-fix paragraphs must be flagged, and the phrase they
    // used must genuinely fail to resolve — otherwise this whole rule is
    // decoration over a corpus that happens to be clean today.
    let root = workspace_root();
    for (label, text) in [
        ("backends.md §Error handling", PREFIX_BACKENDS_ATTRIBUTION),
        ("shell-roundtrip.md §3", PREFIX_SHELL_ATTRIBUTION),
    ] {
        let hits = quoted_invariant_attributions(text);
        assert!(
            !hits.is_empty(),
            "the detector no longer flags the {label} attribution this rule exists for:\n{text}"
        );
        for (id, phrase) in hits {
            let known = equation_keys(&root, &id);
            assert!(
                !known.is_empty(),
                "{label}: {id} vanished from contracts/, so this non-vacuity check is testing \
                 nothing"
            );
            assert!(
                !known.contains(&as_key(&phrase)),
                "{label}: {phrase:?} now resolves to an `equations:` key of {id}. If the contract \
                 grew that equation the fix is to say so; if this normalisation got looser, the \
                 rule stopped working."
            );
        }
    }
}

#[test]
fn a_historical_note_is_exempt_but_neither_original_defect_was() {
    // The exemption is narrow ON PURPOSE, and this test is the proof: adding
    // the disclosure phrase exempts a paragraph, and NEITHER paragraph this
    // rule was built for carried it. If the exemption were what let the
    // defects through, this test says so.
    for text in [PREFIX_BACKENDS_ATTRIBUTION, PREFIX_SHELL_ATTRIBUTION] {
        assert!(
            !text.contains("Through v0.1."),
            "the pre-fix text already carried the disclosure phrase, so the exemption — not the \
             detector — is what this rule turns on:\n{text}"
        );
        let disclosed = format!("Through v0.1.617 the page said: {text}");
        assert!(
            quoted_invariant_attributions(&disclosed).is_empty(),
            "a disclosed historical note is still flagged; correcting a claim in place would be \
             impossible under this rule"
        );
    }
}

// ---------------------------------------------------------------------------
// Rule 2 — the requirement itself, which names no contract.
// ---------------------------------------------------------------------------

#[test]
fn prose_asserting_what_a_refusal_message_contains_must_disclaim_or_link() {
    let root = workspace_root();
    let mut offenders: Vec<String> = Vec::new();
    let mut carriers: BTreeSet<String> = BTreeSet::new();

    for (rel, body) in book_pages(&root) {
        for (line, para) in paragraphs(&body) {
            let hits = requirement_hits(&para);
            if hits.is_empty() {
                continue;
            }
            carriers.insert(rel.clone());
            if !is_disclaimed(&para) {
                offenders.push(format!(
                    "{rel}:{line}: says {hits:?} with no disclaimer or link"
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "\na book page states what a backend's refusal MESSAGE contains without saying it is not \
         guaranteed and without linking the measured table:\n  {}\n\
         Measured over a fixed corpus, most refusals name neither a contract nor a better \
         `--target`, and `C-XPILE-BACKEND-TRAIT` requires neither — see \
         `backend_refusal_disclosure_witness.rs` (XPILE-BACKENDREFUSE-001) and PMAT-1437.",
        offenders.join("\n  ")
    );

    // NON-VACUITY by ANCHOR, not by count (PMAT-1396: a negative over an empty
    // enumeration passes for free, and a hard-coded total re-rots the moment
    // the corpus moves). These two pages are the ones the class lives on; if
    // the scan stops finding them, the needles have drifted away from the prose.
    for anchor in [
        "book/src/reference/backends.md",
        "book/src/contributing/adding-a-backend.md",
    ] {
        assert!(
            carriers.contains(anchor),
            "{anchor} no longer matches any REQUIREMENT_NEEDLE, so this scan is not reaching the \
             prose it was written for. Found on: {carriers:?}"
        );
    }
}

#[test]
fn the_requirement_scan_flags_every_paragraph_it_was_built_for() {
    // NON-VACUITY, per site rather than per suite (PMAT-1410's lesson: a
    // refusal corpus checked in aggregate hides the entry that passes for an
    // unrelated reason). Each of the four pre-fix paragraphs must be caught,
    // and each must be caught for want of a DISCLAIMER, not by accident.
    for (label, text) in [
        ("backends.md must-preamble", PREFIX_BACKENDS_MUST),
        ("backends.md must-item 2", PREFIX_BACKENDS_MUST_ITEM_2),
        ("backends.md must-item 3", PREFIX_BACKENDS_MUST_ITEM_3),
        ("cli.md body", PREFIX_CLI_BODY),
        (
            "adding-a-backend.md §3 comment",
            PREFIX_CONTRIBUTING_COMMENT,
        ),
        (
            "contracts.md C-XPILE-BACKEND-TRAIT",
            PREFIX_CONTRACT_REFERENCE,
        ),
    ] {
        let hits = requirement_hits(text);
        // The preamble alone carries no needle — it is the numbered ITEMS that
        // make the claim, and each is its own paragraph. Recorded rather than
        // papered over: a scan keyed on the requirement vocabulary reaches the
        // items, not the sentence introducing them, which is why the fix had to
        // rewrite the whole section rather than the flagged lines.
        if label == "backends.md must-preamble" {
            assert!(
                hits.is_empty(),
                "{label}: expected the preamble to carry no needle of its own; it now does, so \
                 this note is stale"
            );
            continue;
        }
        assert!(
            !hits.is_empty(),
            "{label}: no REQUIREMENT_NEEDLE matches the text this rule exists for. Ask what the \
             DEFECT spelled, not what the fix spells:\n{text}"
        );
        assert!(
            !is_disclaimed(text),
            "{label}: the pre-fix paragraph counts as disclaimed, so the disclaimer — not the \
             absence of one — is what this rule turns on:\n{text}"
        );
    }
}

#[test]
fn the_code_comment_spelling_is_reachable_and_the_obvious_needle_misses_it() {
    // PMAT-1437's lesson, made executable. Its gate checked backticked
    // `equations:` keys and the falsehood contained none; the first draft of
    // THIS file's scan used only "names/naming the governing contract" and the
    // §3 code comment spells it "naming the construct, the governing contract,
    // and the suggested target" — so the needle written from the fixed text
    // would have missed the one site that is INSTRUCTIONS to contributors.
    let naive = [
        "names the governing contract",
        "naming the governing contract",
    ];
    assert!(
        !naive
            .iter()
            .any(|n| PREFIX_CONTRIBUTING_COMMENT.to_ascii_lowercase().contains(n)),
        "the code-comment site now matches the naive needle, so this cautionary test is stale — \
         re-derive which spellings the live corpus actually uses"
    );
    assert!(
        !requirement_hits(PREFIX_CONTRIBUTING_COMMENT).is_empty(),
        "the full needle set no longer reaches the code-comment spelling"
    );
}
