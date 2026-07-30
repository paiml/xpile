//! XPILE-ENFORCE-PROSE-001 — no document may under-claim what blocks a merge
//! (PMAT-1475).
//!
//! `ruleset_drift.rs` pins the machine-readable half: the committed
//! `docs/status/ruleset-*.json` receipts, the `XPILE-ENFORCEMENT` marker lines,
//! and the live branch-rules endpoint. This file pins the PROSE half, which is
//! what a reader actually reads and which drifted in the opposite direction
//! from every other honesty defect this release.
//!
//! ## Why the prose needs its own gate
//!
//! On 2026-07-27 `workspace-test` was **moved** from org ruleset `13878864`
//! into a new ruleset `19814559`. Effective protection on `main` did not
//! change. But a per-id check reported a weakening, and over the next two days
//! three documents were edited to AGREE with it — including
//! `contracts/README.md`, which is packaged and published to crates.io, and an
//! `[Unreleased]` CHANGELOG entry that was about to ship as release notes.
//!
//! Every one of those edits passed CI. `ruleset_drift.rs` could not see them:
//! its subject is markers and JSON, and none of the three sentences was a
//! marker. So the repo told its readers it enforced less than it did, in files
//! no enforcement gate was quantified over.
//!
//! **The direction is the point.** A sweep that only hunts over-claiming will
//! rewrite a TRUE sentence into a false one and call it a correction.
//!
//! ## What this asserts
//!
//! Over a DISCOVERED corpus — every tracked Markdown file, via `git ls-files`,
//! so a new document is covered the day it lands — no file may state that a
//! context recorded as required is not enforced.
//!
//! Quoted spans are exempt: an entry that corrects itself has to be able to
//! quote what it used to say. That exemption is itself a hole, so it is
//! bounded — inline code and block quotes only — and both the needle and the
//! exemption carry a control that must PASS, because a screen nobody has seen
//! fire is a screen that might match nothing at all.
//!
//! ## The second subject: ATTRIBUTION (PMAT-1476)
//!
//! The tests above check the merge-blocking **set**. They cannot see a document
//! that gets the set right and names the wrong **source** for it — and that is
//! the shape that regenerates the defect, because a reader who follows a wrong
//! source re-derives the wrong set for themselves.
//!
//! This file's own release entry shipped with one:
//!
//! > Only two status checks block a merge: `gate` and `workspace-test`. That is
//! > the live org ruleset 13878864 […] the snapshot is committed at
//! > `docs/status/ruleset-13878864.json`
//!
//! Conclusion true, citation self-falsifying: ruleset `13878864` has required
//! `gate` alone since 2026-07-27, and so does its committed receipt. Following
//! the citation produces "`workspace-test` was dropped" — the exact false
//! inference that cost two days. Under-claim screening is blind to it by
//! construction, since the sentence under-claims nothing.
//!
//! So: a paragraph that names **one** ruleset id while claiming a context that
//! ruleset does not require must also name the other ruleset, or the union
//! endpoint `repos/paiml/xpile/rules/branches/main`. The required sets come from
//! the committed receipts, discovered by glob — a third ruleset is covered the
//! day its receipt lands, with no needle to update.
//!
//! Two deliberate scope choices, each with a control:
//!
//! 1. **Paragraph, not proximity.** A ±300-character window around a ruleset id
//!    flagged 30 sites, nearly all honest — in a document *about* enforcement,
//!    both context names are always nearby. Paragraph scope flagged four, all
//!    real.
//! 2. **`CHANGELOG.md` is narrowed to `[Unreleased]`.** A released section is a
//!    dated record of what was measured then, not a present-tense claim; the
//!    same doctrine `claim_pages()` implements in `book_claims_witness`. The
//!    `[0.1.617]` section correctly recorded `13878864` requiring both contexts,
//!    because on 2026-07-26 it did. `[Unreleased]` is the text that ships next,
//!    so it is the text held to the present tense.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// Sentences that were LIVE and FALSE between 2026-07-27 and 2026-07-29. Each
/// asserts that a required context is not enforced. Matched case-insensitively
/// against text with quoted spans removed.
const UNDER_CLAIMS: &[&str] = &[
    // contracts/README.md — packaged, shipped to crates.io.
    "live org ruleset is currently weaker",
    "weaker than that record",
    // The generic shape: any claim that the required set is `gate` alone.
    "requires only gate",
    "required set is gate alone",
    "only gate blocks",
    "workspace-test was dropped",
    "workspace-test is no longer required",
    "workspace-test no longer blocks",
    // PMAT-1476: the spelling that shipped in this CHANGELOG's PMAT-1416 entry.
    // "`workspace-test` dropped out of the required set, so a PR can merge with
    // it red" — a distinct sentence from the six above, and the only one that
    // spells out the false consequence.
    "dropped out of the required set",
];

/// How far past a match to look for the word that would make it honest.
const ACQUITTAL_WINDOW: usize = 48;

/// Remove the spans a document is allowed to quote, and ONLY those.
///
/// Disclosed quotation means a **block quote** (`> …`): an entry that corrects
/// itself has to be able to restate what it used to say. Inline code is NOT a
/// quotation — it is formatting, and treating it as one is a hole wide enough
/// to drive the original defect through. The first draft of this gate blanked
/// backticked spans, and `docs/RELEASE.md`'s live-and-false
/// "the live ruleset requires only `gate`" went UNDETECTED because the one word
/// that made it a claim sat inside backticks. So backticks are stripped as
/// *characters* while their contents stay in the haystack. This is the same
/// needle-blindness that let PMAT-1449's gate miss `contracts/README.md`.
fn strip_quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        if line.trim_start().starts_with('>') {
            out.push('\n');
            continue;
        }
        // A span inside double quotes is also disclosed quotation — that is how
        // a prose paragraph cites the wording it is correcting when a block
        // quote would break the sentence.
        let mut in_quote = false;
        for ch in line.chars() {
            match ch {
                '`' => {}
                '"' => {
                    in_quote = !in_quote;
                    out.push(' ');
                }
                _ => out.push(if in_quote { ' ' } else { ch }),
            }
        }
        out.push('\n');
    }
    out
}

/// Every tracked Markdown file. Discovered, not listed — the corpus is the
/// blind spot that let the packaged `contracts/README.md` sit outside
/// PMAT-1449's two-file corpus.
fn tracked_markdown() -> Vec<String> {
    let out = Command::new("git")
        .args(["ls-files", "*.md"])
        .current_dir(workspace_root())
        .output()
        .expect("`git ls-files` runs");
    assert!(out.status.success(), "`git ls-files` failed");
    let files: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    assert!(
        files.len() >= 20,
        "discovered only {} tracked .md files — the corpus query is broken, not \
         the repo",
        files.len()
    );
    files
}

/// Collapse everything that is not a word character, a hyphen or a slash to a
/// single space.
///
/// **This is the third time punctuation has hidden this exact claim** (PMAT-1476).
/// PMAT-1449's needle was `requires only [` — bracket-only — and missed
/// `contracts/README.md`, which spells it with backticks and "and". This file's
/// first draft therefore dropped the brackets… and missed
/// `**It requires only `[gate]`.**` in `[Unreleased]` — four thousand lines
/// above its own entry, and the sentence that *originated* the false model.
/// Stripping backticks as characters was not enough, because the brackets
/// survived: `requires only [gate]` does not contain `requires only gate`.
///
/// Choosing between the two spellings is the mistake. Normalising the haystack
/// and the needle the same way removes the choice, and no future spelling of
/// the separator — `(gate)`, `"gate"`, `: gate` — reopens it. Hyphens survive
/// because `workspace-test` is one token; slashes survive because
/// `rules/branches/main` is a path the acquittals key on.
fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '/' {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.push(ch);
        } else {
            pending_space = true;
        }
    }
    out
}

/// A needle is an offence only if `workspace-test` does not appear right after
/// it. "requires only gate **and workspace-test**" is an honest sentence that
/// happens to contain the words of a dishonest one, and the first draft of this
/// gate flagged two of them — including a correct line in a shipped release
/// section. A screen that cries wolf gets loosened by the next person to hit it.
fn offences_in(text: &str) -> BTreeSet<String> {
    let hay = normalise(&strip_quoted(text)).to_lowercase();
    let mut found = BTreeSet::new();
    for needle in UNDER_CLAIMS {
        // The needle goes through the same normalisation as the haystack, so a
        // needle can never be written in a spelling the haystack cannot hold.
        let lower = normalise(&needle.to_lowercase());
        let mut from = 0;
        while let Some(rel) = hay[from..].find(&lower) {
            let at = from + rel;
            let end = at + lower.len();
            let window = &hay[end..hay.len().min(end + ACQUITTAL_WINDOW)];
            if !window.contains("workspace-test") {
                found.insert((*needle).to_string());
                break;
            }
            from = end;
        }
    }
    found
}

/// No tracked document may claim the repo enforces less than its receipts record.
#[test]
fn no_document_under_claims_the_merge_blocking_set() {
    let root = workspace_root();
    let mut offences = Vec::new();
    for rel in tracked_markdown() {
        let Ok(text) = std::fs::read_to_string(root.join(&rel)) else {
            continue;
        };
        for needle in offences_in(&text) {
            offences.push(format!("{rel}: \"{needle}\""));
        }
    }
    assert!(
        offences.is_empty(),
        "document(s) claim a required context is not enforced:\n  {}\n\n\
         Before changing anything, run `gh api repos/paiml/xpile/rules/branches/main`. \
         That is the UNION over every ruleset protecting main, and it is the only \
         thing that answers \"what blocks a merge\". A single ruleset losing a \
         context is not evidence the context stopped being required — on \
         2026-07-27 `workspace-test` MOVED to ruleset 19814559 and three \
         documents were edited to claim less enforcement than the repo has. \
         If the enforcement really did weaken, re-derive the receipts under \
         docs/status/ first; the prose follows the receipts, never the reverse.",
        offences.join("\n  ")
    );
}

/// The needle works. Without this, a typo in `UNDER_CLAIMS` turns the gate above
/// into a test that reads every Markdown file and asserts nothing.
#[test]
fn the_under_claim_screen_actually_fires() {
    let live_and_false =
        "Do not read the merge-blocking set here. The live org ruleset is currently \
         weaker than that record, and the gap is an open owner decision.";
    let found = offences_in(live_and_false);
    assert!(
        found.contains("live org ruleset is currently weaker"),
        "the screen did not fire on the exact sentence that shipped to crates.io \
         in v0.1.617's contracts/README.md: {found:?}"
    );

    // Two-way control: honest prose must NOT trip it.
    let honest = "The merge-blocking set is the union over every ruleset protecting \
                  main: gate and workspace-test.";
    assert!(
        offences_in(honest).is_empty(),
        "the screen fires on honest prose — it would force documents to be wrong"
    );

    // The acquittal window. This exact sentence ships in a released CHANGELOG
    // section and is TRUE; the first draft of this gate flagged it. A screen
    // that cries wolf on correct prose gets deleted by whoever hits it next.
    let true_but_similar = "the live org ruleset requires only `gate` and `workspace-test`";
    assert!(
        offences_in(true_but_similar).is_empty(),
        "the screen flagged a TRUE sentence — \"requires only gate AND \
         workspace-test\" names both required contexts: {:?}",
        offences_in(true_but_similar)
    );

    // PMAT-1476. The two spellings that each evaded a previous revision of this
    // screen. Both must fire now, and they fire for the same reason: the
    // haystack and the needle are normalised identically, so the separator
    // between "only" and "gate" cannot decide the outcome.
    for evaded in [
        // Shipped in `[Unreleased]`; missed because `[` sat between the words.
        "**It requires only `[gate]`.** Measured against the live API.",
        // PMAT-1449's bracket-only needle missed this one for the mirror reason.
        "the live org ruleset requires only `gate`, an open owner decision",
        // Spelled the consequence out; matched none of the six original needles.
        "`workspace-test` dropped out of the required set, so a PR can merge \
         with it red.",
    ] {
        assert!(
            !offences_in(evaded).is_empty(),
            "a spelling that shipped in this repository still evades the \
             screen: {evaded:?}"
        );
    }
}

/// The committed receipts under `docs/status/`, keyed by ruleset id.
///
/// Discovered by glob, not listed: the whole defect this file exists for was a
/// second ruleset appearing where one gate assumed there was only ever one.
fn receipts() -> BTreeMap<String, BTreeSet<String>> {
    let dir = workspace_root().join("docs/status");
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).expect("docs/status is readable") {
        let path = entry.expect("dir entry").path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if !(name.starts_with("ruleset-") && name.ends_with(".json")) {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("receipt is readable");
        let json: serde_json::Value = serde_json::from_str(&text).expect("receipt is JSON");
        let id = json["id"].to_string().trim_matches('"').to_string();
        let mut contexts = BTreeSet::new();
        for rule in json["rules"].as_array().into_iter().flatten() {
            if rule["type"] != "required_status_checks" {
                continue;
            }
            for check in rule["parameters"]["required_status_checks"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if let Some(c) = check["context"].as_str() {
                    contexts.insert(c.to_string());
                }
            }
        }
        out.insert(id, contexts);
    }
    assert!(
        out.len() >= 2,
        "discovered {} ruleset receipts under docs/status — this gate's entire \
         subject is that more than one ruleset protects `main`; with fewer than \
         two receipts it asserts nothing",
        out.len()
    );
    out
}

/// The endpoint that answers "what blocks a merge", and the only acquittal a
/// single-ruleset citation gets other than naming the other ruleset.
const UNION_ENDPOINT: &str = "repos/paiml/xpile/rules/branches/main";

/// A phrase that turns a mention into a claim about enforcement. Without one of
/// these a paragraph is merely naming a ruleset, which is not an attribution.
const ENFORCEMENT_VERBS: &[&str] = &[
    "block a merge",
    "blocks a merge",
    "merge-blocking",
    "required status check",
    "required_status_check",
    "are required",
    "is required",
    "requires",
];

/// `[Unreleased]` only, for `CHANGELOG.md`; the whole file otherwise.
///
/// A released section is a dated record — `[0.1.617]` says ruleset `13878864`
/// required both contexts, which was true when it was measured on 2026-07-26 and
/// is exactly the kind of frozen numeral this repo deliberately does not rewrite.
/// `[Unreleased]` is the text that ships next.
fn present_tense_surface(rel: &str, text: &str) -> String {
    if rel != "CHANGELOG.md" {
        return text.to_string();
    }
    // ANCHORED at a line start. An unanchored `find("## [Unreleased]")` matched a
    // PROSE MENTION of the literal, in backticks, 8,700 lines down in a
    // `[0.1.617]` entry, once the release roll removed the real heading — so this
    // narrowing silently selected the wrong release's section (PMAT-1496). When
    // `[Unreleased]` is absent the leading RELEASED section is the active region.
    let start = text
        .find("\n## [Unreleased]")
        .map(|i| i + 1)
        .or_else(|| text.find("\n## [").map(|i| i + 1))
        .expect("CHANGELOG.md has no `## [...]` heading at a line start");
    let after = &text[start..];
    let end = after[1..]
        .find("\n## [")
        .map(|i| i + 1)
        .unwrap_or(after.len());
    after[..end].to_string()
}

/// Attribution: naming one ruleset as the source of a context it does not
/// require. See the module header — the set can be right while the citation
/// falsifies it, and that citation is what regenerates the wrong set downstream.
#[test]
fn no_document_attributes_a_context_to_a_ruleset_that_does_not_require_it() {
    let root = workspace_root();
    let receipts = receipts();
    let union: BTreeSet<String> = receipts.values().flatten().cloned().collect();
    let mut offences = Vec::new();

    for rel in tracked_markdown() {
        let Ok(raw) = std::fs::read_to_string(root.join(&rel)) else {
            continue;
        };
        let scoped = present_tense_surface(&rel, &raw);
        for hit in attribution_offences(&scoped, &receipts, &union) {
            offences.push(format!("{rel}: {hit}"));
        }
    }

    assert!(
        offences.is_empty(),
        "document(s) attribute a required context to a ruleset whose committed \
         receipt does not contain it:\n  {}\n\n\
         The merge-blocking set is the UNION over every ruleset protecting \
         `main`. A paragraph that cites one ruleset id as the source of that set \
         is self-falsifying: a reader who runs the citation gets a smaller set \
         and concludes a context was dropped. That is not hypothetical — it is \
         PMAT-1475, and it cost two days, an owner decision that did not exist, \
         and a falsehood published to crates.io. Either name `{UNION_ENDPOINT}`, \
         or attribute per ruleset ({}).",
        offences.join("\n  "),
        receipts
            .iter()
            .map(|(id, cs)| format!("{id} -> {cs:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

/// The attribution screen fires, does not fire on the honest forms, and reaches
/// a corpus big enough to matter. Without this the test above is a loop that
/// reads every Markdown file and asserts nothing.
#[test]
fn the_attribution_screen_fires_and_spares_the_honest_forms() {
    let receipts = receipts();
    let union: BTreeSet<String> = receipts.values().flatten().cloned().collect();
    assert!(
        union.len() >= 2 && receipts.values().all(|c| c.len() < union.len()),
        "the premise of this gate no longer holds: every receipt would have to \
         be a strict subset of the union for a single-ruleset citation to be \
         wrong. receipts={receipts:?} union={union:?}"
    );

    // Pick a real (ruleset, absent-context) pair from the receipts rather than
    // hard-coding ids — a hard-coded pair is the failure mode this gate is for.
    let (id, absent) = receipts
        .iter()
        .find_map(|(id, cs)| union.iter().find(|c| !cs.contains(*c)).map(|c| (id, c)))
        .expect("some receipt lacks some context in the union");

    let offending = format!(
        "Only two status checks block a merge: {}. That is the live org ruleset \
         {id}; the snapshot is committed at docs/status/ruleset-{id}.json.",
        union.iter().cloned().collect::<Vec<_>>().join(" and ")
    );
    assert!(
        !attribution_offences(&offending, &receipts, &union).is_empty(),
        "the screen did not fire on the sentence that shipped in this \
         CHANGELOG's own `What is NOT merge-blocking` section: {offending:?} \
         (absent from {id}: {absent})"
    );

    // Honest form 1 — cites the union endpoint.
    let via_union = format!(
        "A merge is blocked by {}, the union read with `gh api {UNION_ENDPOINT}`; \
         ruleset {id} is one contributor.",
        union.iter().cloned().collect::<Vec<_>>().join(" and ")
    );
    assert!(
        attribution_offences(&via_union, &receipts, &union).is_empty(),
        "the screen fires on prose that names the union endpoint — the acquittal \
         it exists to grant"
    );

    // Honest form 2 — attributes per ruleset, naming every id.
    let per_ruleset = receipts
        .iter()
        .map(|(i, cs)| {
            format!(
                "ruleset {i} requires {}",
                cs.iter().cloned().collect::<Vec<_>>().join(" and ")
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        attribution_offences(&per_ruleset, &receipts, &union).is_empty(),
        "the screen fires on a correct per-ruleset attribution: {per_ruleset:?}"
    );

    // The `[Unreleased]` scoper actually finds something to read.
    let raw = std::fs::read_to_string(workspace_root().join("CHANGELOG.md"))
        .expect("CHANGELOG.md is readable");
    let scoped = present_tense_surface("CHANGELOG.md", &raw);
    assert!(
        scoped.len() > 20_000 && scoped.len() < raw.len(),
        "the [Unreleased] scoper is degenerate: {} of {} bytes",
        scoped.len(),
        raw.len()
    );
}

/// The attribution rule, factored out so its controls exercise the same code the
/// corpus test does.
fn attribution_offences(
    text: &str,
    receipts: &BTreeMap<String, BTreeSet<String>>,
    union: &BTreeSet<String>,
) -> Vec<String> {
    let body = strip_quoted(text).to_lowercase();
    let mut out = Vec::new();
    for para in body.split("\n\n") {
        let named: Vec<&String> = receipts
            .keys()
            .filter(|id| para.contains(id.as_str()))
            .collect();
        if named.len() != 1 {
            continue;
        }
        let id = named[0];
        let absent: Vec<&String> = union
            .iter()
            .filter(|c| !receipts[id].contains(*c) && para.contains(c.as_str()))
            .collect();
        if absent.is_empty() || !ENFORCEMENT_VERBS.iter().any(|v| para.contains(v)) {
            continue;
        }
        if para.contains(UNION_ENDPOINT) {
            continue;
        }
        out.push(format!(
            "names ONLY ruleset {id} while claiming {absent:?} — that ruleset \
             requires {:?}",
            receipts[id]
        ));
    }
    out
}

/// The quotation exemption works, and is bounded. A CHANGELOG entry has to be
/// able to quote the falsehood it is correcting; it must not be able to launder
/// a live claim by indenting it.
#[test]
fn the_quotation_exemption_is_bounded() {
    // Disclosed quotation — exempt.
    let disclosed = "> The live org ruleset is currently weaker than that record.\n\n\
                     That sentence was false; the context had moved rulesets.";
    assert!(
        offences_in(disclosed).is_empty(),
        "a block-quoted disclosure was flagged — an entry could not describe its \
         own correction"
    );

    // Same words, asserted in the document's own voice — NOT exempt.
    let asserted = "The live org ruleset is currently weaker than that record.";
    assert!(
        !offences_in(asserted).is_empty(),
        "the exemption swallowed an unquoted claim — every offender could evade \
         this gate by never using a quote character"
    );

    // Inline code is formatting, not quotation. This is the exact sentence the
    // first draft of this gate let through in docs/RELEASE.md.
    let backticked = "As of 2026-07-27 it is RED — the live ruleset requires only \
                      `gate` while the committed snapshot records both.";
    assert!(
        !offences_in(backticked).is_empty(),
        "a claim survived by putting the load-bearing word in backticks — the \
         inline-code exemption is a hole, not a feature"
    );
}
