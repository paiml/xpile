//! XPILE-SKIPGUARD-002 (PMAT-1506) — a conditional assertion whose ANTECEDENT
//! is a string literal the corpus never contains. The assertion is written, the
//! test is green, and the property has never been checked.
//!
//! THE DEFECT, measured 2026-07-31 on the live tree.
//! `release_runbook_facts_witness.rs` carried two of them:
//!
//! * `the_runbook_does_not_type_an_advisory_roster` opened with
//!   `if let Some(at) = n.find("are ADVISORY")`. The corrected runbook spells it
//!   *"state the ADVISORY set AS DERIVED"*, so `find` returned `None` and the
//!   assertion inside — the entire subject of the test, the rule that the
//!   release runbook may not type an advisory roster — **had never executed**.
//!   The test passed on a sibling `assert!(n.contains("AS DERIVED"))`, which is
//!   precisely why the shape reads as covered: there is a passing assertion in
//!   the function, just not the one the function is named for.
//! * `the_runbook_does_not_describe_the_version_bump_as_one_line` looped
//!   `for pat in ["single-sourced at Cargo.toml", "one line +"]`. The first is
//!   present at offset 505; the second is **absent (-1)** and its arm had never
//!   run.
//!
//! Neither is a false claim — the runbook really does defer to the derivation,
//! and the version text really is corrected. **The defect is that nothing was
//! measuring it**, which is PMAT-1505's hollow-witness shape one layer down: a
//! guard that cannot succeed skips a whole test, a needle that cannot match
//! skips one assertion inside a test that goes on passing. The second is
//! harder to see, because the passing-test count and the assertion count are
//! both unchanged.
//!
//! WHAT THIS FILE PINS. Over every tracked `crates/*/tests/*.rs`: a conditional
//! block that contains an assertion and whose antecedent tests a STRING-LITERAL
//! NEEDLE (`find` / `contains` / `starts_with` / `ends_with`, including a needle
//! taken from a `for … in ["…", "…"]` literal array) must be ANCHORED — either
//!
//! * (A) it has an `else` branch that also asserts, so one disposition always
//!   executes; or
//! * (B) the block increments a counter that a later assertion in the same
//!   function reads, so an empty scan reds.
//!
//! ⛔ SCOPE, HONESTLY. This is the NEEDLE arm only. The general vacuous-loop
//! class — `for x in COLLECTION { assert!(…) }` where `COLLECTION` may be empty
//! — is **not** covered: measured 2026-07-31, **586** such loops exist across
//! **275** tracked test files, and a rule demanding a floor from all of them
//! would red hundreds of correct files, which is how a gate gets disabled
//! (PMAT-1500). `queue.yaml` `next_lane` carries that arm with its exemption
//! (`diamond_depth_label_witness.rs`'s two collections are empty BY DESIGN)
//! already pre-registered. Nor does this file cover DEAD GUARDS — the
//! `XPILE_REQUIRE_*` tripwires and `python3`-absent skips are a third shape.
//!
//! THE MEASUREMENT WAS BY EFFECT, NOT BY READING (PMAT-1505's mandate). The
//! whole corpus holds **nine** such sites; each was probed by instrumenting the
//! block and running the test, not by reading the guard:
//!
//! | site | hits |
//! |---|---|
//! | `dict_boundary_fuzz_witness.rs` `_eqm` / `_eqn` | 12 each |
//! | `range_bool_bound.rs` loop-header fingerprint | 7 |
//! | `lean_models_lane_witness.rs` `Models/*.lean` | ≥3, already floored |
//! | `mcp_surface_disclosure_witness.rs:230` `mcp` | `else` arm, always runs |
//! | `release_runbook_facts_witness.rs` `single-sourced at Cargo.toml` | 1 |
//! | `release_runbook_facts_witness.rs` `one line +` | **0** |
//! | `release_runbook_facts_witness.rs` `are ADVISORY` | **0** |
//! | `mcp_surface_disclosure_witness.rs` spelled/digit tool count | **0** |
//!
//! Three arms execute nothing, and they are **not the same finding**. The two
//! runbook ones are DEFECTS: the property each names is simply unchecked. The
//! cardinality family is a NEGATIVE REGRESSION SCAN over every spelling
//! `four`…`nine` and `0`…`9`, and zero matches is its CORRECT reading — the
//! thing it hunts (PMAT-1499's *"Six initial tools"* against seven rows) was
//! fixed. A rule that cannot tell those apart demands a floor from a correct
//! file and gets disabled, so the remedy differs: the runbook rules were
//! re-keyed and floored, the cardinality scan was factored into a helper with a
//! CONTROL that drives it on constructed input (`the_published_cardinality_
//! scan_can_still_fire`), which is this repo's own established fix — see
//! `the_citation_detector_can_still_fire` (PMAT-1505). Factoring it out removes
//! it from this gate's corpus by construction, which is why no exemption list
//! exists here: there is nothing on the tree it would hold.
//!
//! The two live-but-unfloored sites got floors rather than exemptions, because
//! a floor there is worth having: if the oracle stops emitting `_eqm` names or
//! the emitter stops spelling `let __forstop`, those assertions go quiet
//! exactly the way the runbook's did.
//!
//! ⚠️ **AND THIS GATE'S FIRST RUN AGAINST ITSELF WAS ON CI, NOT LOCALLY.** The
//! corpus is `git ls-files`, which cannot see an **untracked** file — so while
//! the slice was being written the scan covered every test file except the one
//! being added. It went green locally and red on the first CI run, on two false
//! positives of its own making: `line.contains("assert")` matched the identifier
//! `Anchor::ElseAsserts` in the classifier's own body, and the controls' fixture
//! sources, written as multi-line string literals, began exactly the way live
//! code does. Both are fixed (`has_assertion`, `fixture`) and
//! `the_scan_reaches_the_shapes_it_claims_to_cover` now asserts this file is in
//! its own corpus. **A new gate does not analyse itself until it is committed.**

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The needle-bearing predicates. A conditional whose antecedent calls one of
/// these on a string literal is testing for the PRESENCE of that literal.
const SEARCHES: [&str; 4] = [".find(", ".contains(", ".starts_with(", ".ends_with("];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

/// Every tracked integration-test source. `git ls-files`, so the corpus cannot
/// drift away from the tree the way a hand-written list does (PMAT-1396).
fn test_sources() -> Vec<String> {
    let out = Command::new("git")
        .args(["ls-files", "crates/*/tests/*.rs"])
        .current_dir(workspace_root())
        .output()
        .expect("git ls-files runs at the workspace root");
    assert!(out.status.success(), "git ls-files failed: {out:?}");
    String::from_utf8(out.stdout)
        .expect("git ls-files emits utf-8 paths")
        .lines()
        .map(str::to_string)
        .collect()
}

/// How a site proves it is not vacuous.
#[derive(Debug, PartialEq, Eq)]
enum Anchor {
    /// An `else` arm that also asserts — one disposition always runs.
    ElseAsserts,
    /// A counter incremented in the block and read by a later assertion.
    Counter(String),
    /// Nothing. This is the defect.
    None,
}

#[derive(Debug)]
struct Site {
    file: String,
    line: usize,
    needles: Vec<String>,
    anchor: Anchor,
}

/// An assert MACRO call — not any identifier that merely contains `assert`.
///
/// PMAT-1506: the first draft tested `line.contains("assert")` and reported THIS
/// FILE at the `} else` classifier, because the body it was scanning assigns
/// `Anchor::ElseAsserts`. A detector that matches its own vocabulary cries wolf
/// on the file it lives in, which is how a gate gets disabled (PMAT-1500).
fn has_assertion(l: &str) -> bool {
    ["assert!(", "assert_eq!(", "assert_ne!(", "assert_matches!("]
        .iter()
        .any(|m| l.contains(m))
}

fn indent_of(l: &str) -> usize {
    l.len() - l.trim_start().len()
}

/// String literals in `s`, escapes ignored (no test needle here uses one).
fn literals(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(a) = rest.find('"') {
        let tail = &rest[a + 1..];
        match tail.find('"') {
            Some(b) => {
                out.push(tail[..b].to_string());
                rest = &tail[b + 1..];
            }
            None => break,
        }
    }
    out
}

/// The identifier passed to a `SEARCHES` predicate, when it is not a literal —
/// e.g. `n.find(pat)` yields `pat`. Used to reach a needle that arrives through
/// a `for … in ["…"]` loop variable.
fn search_argument_ident(cond: &str) -> Option<String> {
    for s in SEARCHES {
        if let Some(at) = cond.find(s) {
            let arg = &cond[at + s.len()..];
            let arg = arg.split(')').next().unwrap_or("").trim();
            if !arg.is_empty()
                && arg
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '&')
            {
                return Some(arg.trim_start_matches('&').to_string());
            }
        }
    }
    None
}

/// Analyse one source. Returns every needle-antecedent conditional that guards
/// an assertion, with the anchor it does or does not have.
fn sites_in(file: &str, src: &str) -> Vec<Site> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for (i, raw) in lines.iter().enumerate() {
        let l = raw.trim();
        if !l.starts_with("if ") || !l.ends_with('{') {
            continue;
        }
        let cond = l
            .trim_start_matches("if ")
            .trim_end_matches('{')
            .trim()
            .trim_start_matches("let Some(");
        let cond = match cond.split_once(") = ") {
            Some((_, r)) => r,
            None => cond,
        };
        if !SEARCHES.iter().any(|s| cond.contains(s)) {
            continue;
        }

        // Enclosing function: the nearest `fn` at column 0 above, through the
        // next one. Anchors must live inside it, not in a neighbour.
        let fn_start = (0..i)
            .rev()
            .find(|&j| lines[j].starts_with("fn ") || lines[j].starts_with("pub fn "))
            .unwrap_or(0);
        let fn_end = ((i + 1)..lines.len())
            .find(|&j| lines[j] == "}")
            .unwrap_or(lines.len() - 1);

        let mut needles = literals(cond);
        if needles.is_empty() {
            // `n.find(pat)` — resolve `pat` against a literal-array `for` head.
            let ident = match search_argument_ident(cond) {
                Some(id) => id,
                None => continue,
            };
            let head = format!("for {ident} in [");
            match lines[fn_start..i].iter().rev().find(|l| l.contains(&head)) {
                Some(h) => needles = literals(h),
                None => continue,
            }
        }
        if needles.is_empty() {
            continue;
        }

        // The block body, to the matching close at the `if`'s own indent.
        let ind = indent_of(raw);
        let close = ((i + 1)..lines.len())
            .find(|&j| indent_of(lines[j]) == ind && lines[j].trim().starts_with('}'))
            .unwrap_or(fn_end);
        let body = &lines[i + 1..close];
        if !body.iter().any(|b| has_assertion(b)) {
            continue;
        }

        let mut anchor = Anchor::None;
        if lines[close].trim().starts_with("} else") {
            let else_close = ((close + 1)..lines.len())
                .find(|&j| indent_of(lines[j]) == ind && lines[j].trim().starts_with('}'))
                .unwrap_or(fn_end);
            if lines[close + 1..else_close]
                .iter()
                .any(|b| has_assertion(b))
            {
                anchor = Anchor::ElseAsserts;
            }
        }
        if anchor == Anchor::None {
            for b in body {
                if let Some((lhs, _)) = b.trim().split_once(" += 1") {
                    let id = lhs.trim();
                    if !id.is_empty()
                        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        && lines[close..fn_end]
                            .iter()
                            .any(|a| has_assertion(a) || a.contains(id))
                        && lines[close..fn_end]
                            .iter()
                            .skip_while(|a| !has_assertion(a))
                            .any(|a| a.contains(id))
                    {
                        anchor = Anchor::Counter(id.to_string());
                        break;
                    }
                }
            }
        }

        out.push(Site {
            file: file.to_string(),
            line: i + 1,
            needles,
            anchor,
        });
    }
    out
}

fn all_sites() -> Vec<Site> {
    let root = workspace_root();
    let mut out = Vec::new();
    for f in test_sources() {
        let src =
            std::fs::read_to_string(root.join(&f)).unwrap_or_else(|e| panic!("read {f}: {e}"));
        out.extend(sites_in(&f, &src));
    }
    out
}

/// THE RULE. Every needle-antecedent conditional assertion is anchored.
#[test]
fn every_needle_guarded_assertion_can_be_shown_to_run() {
    let unanchored: Vec<String> = all_sites()
        .into_iter()
        .filter(|s| s.anchor == Anchor::None)
        .map(|s| format!("{}:{} needles {:?}", s.file, s.line, s.needles))
        .collect();
    assert!(
        unanchored.is_empty(),
        "these conditional assertions are guarded by a string-literal needle and nothing shows \
         the needle is ever found, so the assertion may never execute and the test passes \
         anyway:\n  {}\n\nGive the block an `else` arm that also asserts, or count the \
         iterations and assert the count after the scan (`lean_models_lane_witness.rs`'s \
         `named >= 3` is the template). PMAT-1506: two such assertions had never run.",
        unanchored.join("\n  ")
    );
}

/// NON-VACUITY of the SCAN itself. A parser that matches nothing satisfies the
/// rule above for free (PMAT-1396), and this file's whole subject is checks
/// that pass having measured nothing.
#[test]
fn the_scan_reaches_the_shapes_it_claims_to_cover() {
    let sites = all_sites();
    assert!(
        sites.len() >= 5,
        "the needle scan found {} site(s) across {} tracked test files. It found 8 when this rule \
         was written; a parser that has stopped matching reports zero offences forever.",
        sites.len(),
        test_sources().len()
    );
    // Both anchor kinds must be live, or a branch of the classifier could be
    // dead without anything saying so.
    assert!(
        sites.iter().any(|s| s.anchor == Anchor::ElseAsserts),
        "no site is anchored by an `else` arm — that classifier branch is unexercised"
    );
    assert!(
        sites.iter().any(|s| matches!(s.anchor, Anchor::Counter(_))),
        "no site is anchored by a counter — that classifier branch is unexercised"
    );
    let files: BTreeSet<&str> = sites.iter().map(|s| s.file.as_str()).collect();
    assert!(
        files.len() >= 3,
        "every needle site now lives in {files:?}; the scan was measured across 4 files"
    );

    // THE GATE MUST BE INSIDE ITS OWN CORPUS, and this assertion exists because
    // it was not. `git ls-files` cannot see an UNTRACKED file, so while this
    // slice was being written the scan ran over every test file EXCEPT the one
    // being added — green locally, red on CI the moment the commit made it
    // visible, on two false positives of its own making. A defect-class gate
    // that skips the file it lives in is PMAT-1501's blind spot with a new
    // cause: not "nobody pointed it at that file" but "the file had not arrived
    // yet".
    const SELF: &str = "crates/xpile/tests/skip_guard_vacuity_witness.rs";
    assert!(
        test_sources().iter().any(|f| f == SELF),
        "{SELF} is not in its own corpus — either it is untracked (so this gate has never \
         analysed itself and cannot until it is committed) or the `git ls-files` pathspec has \
         stopped matching"
    );
}

/// A constructed Rust source, assembled LINE BY LINE.
///
/// PMAT-1506, and this one only showed up in CI: the controls below embed Rust
/// source as fixtures, and the first draft wrote each as one multi-line string
/// literal — so every fixture line began, in THIS file, exactly the way live code
/// does, and the scan reported its own test data as four defects. One literal per
/// line means each of those lines starts with `"`, which no live conditional does.
///
/// ⚠️ It went green locally and red on CI for a reason worth remembering: the
/// corpus is `git ls-files`, so an UNTRACKED new file is invisible to it. **A new
/// gate does not analyse itself until it is committed.**
fn fixture(lines: &[&str]) -> String {
    lines.join("\n") + "\n"
}

/// POSITIVE CONTROL — the detector fires on a constructed unanchored site.
/// Without this, `every_needle_guarded_assertion_can_be_shown_to_run` passing is
/// indistinguishable from a classifier that anchors everything (PMAT-1505's red
/// arm R7: a negative detector cannot notice its own death).
#[test]
fn the_detector_fires_on_a_constructed_unanchored_needle() {
    let src = fixture(&[
        "fn probe() {",
        "    let n = corpus();",
        "    if let Some(at) = n.find(\"a needle nothing contains\") {",
        "        assert!(is_fine(at), \"never runs\");",
        "    }",
        "}",
    ]);
    let src = src.as_str();
    let sites = sites_in("probe.rs", src);
    assert_eq!(
        sites.len(),
        1,
        "the constructed site must be FOUND: {sites:?}"
    );
    assert_eq!(
        sites[0].anchor,
        Anchor::None,
        "a bare `if let Some(_) = _.find(LITERAL)` around an assertion has no anchor"
    );
    assert_eq!(sites[0].needles, vec!["a needle nothing contains"]);
}

/// POSITIVE CONTROL, loop form — the shape
/// `release_runbook_facts_witness.rs` actually carried, where the needle
/// reaches the predicate through a literal array.
#[test]
fn the_detector_resolves_a_needle_through_a_literal_array_loop() {
    let src = fixture(&[
        "fn probe() {",
        "    let n = corpus();",
        "    for pat in [\"present spelling\", \"retired spelling\"] {",
        "        if let Some(at) = n.find(pat) {",
        "            assert!(is_mention(&n, at), \"never runs for the retired one\");",
        "        }",
        "    }",
        "}",
    ]);
    let src = src.as_str();
    let sites = sites_in("probe.rs", src);
    assert_eq!(
        sites.len(),
        1,
        "the loop-borne needle must be FOUND: {sites:?}"
    );
    assert_eq!(sites[0].anchor, Anchor::None);
    assert_eq!(
        sites[0].needles,
        vec!["present spelling", "retired spelling"],
        "both literals of the array are needles — one of them being present is exactly how the \
         live defect stayed invisible"
    );
}

/// NEGATIVE CONTROLS — the two anchored forms must NOT be reported, or the rule
/// reds correct files and gets disabled (PMAT-1500's cried-wolf lesson).
#[test]
fn the_detector_accepts_both_anchored_forms() {
    let with_else = fixture(&[
        "fn probe() {",
        "    if registered.contains(\"mcp\") {",
        "        assert!(present.is_empty(), \"shipped\");",
        "    } else {",
        "        assert!(!present.is_empty(), \"not shipped\");",
        "    }",
        "}",
    ]);
    let with_else = with_else.as_str();
    assert_eq!(
        sites_in("probe.rs", with_else)[0].anchor,
        Anchor::ElseAsserts,
        "an if/else where BOTH arms assert always executes one of them"
    );

    let with_counter = fixture(&[
        "fn probe() {",
        "    let mut named = 0usize;",
        "    for span in body.split('`') {",
        "        if span.ends_with(\".lean\") {",
        "            named += 1;",
        "            assert!(modules.contains(span), \"named but absent\");",
        "        }",
        "    }",
        "    assert!(named >= 3, \"the scan found nothing\");",
        "}",
    ]);
    let with_counter = with_counter.as_str();
    assert_eq!(
        sites_in("probe.rs", with_counter)[0].anchor,
        Anchor::Counter("named".to_string()),
        "a counter read by a later assertion reds when the scan comes up empty"
    );
}

/// The counter anchor must require the LATER ASSERTION, not merely a counter.
/// An increment nobody reads proves nothing, and accepting it would have
/// silently exempted the real defects.
#[test]
fn a_counter_nobody_asserts_on_is_not_an_anchor() {
    let src = fixture(&[
        "fn probe() {",
        "    let mut seen = 0usize;",
        "    for pat in [\"absent needle\"] {",
        "        if let Some(at) = n.find(pat) {",
        "            seen += 1;",
        "            assert!(ok(at), \"never runs\");",
        "        }",
        "    }",
        "    eprintln!(\"{seen} seen\");",
        "}",
    ]);
    let src = src.as_str();
    assert_eq!(
        sites_in("probe.rs", src)[0].anchor,
        Anchor::None,
        "an unread counter is not a floor"
    );
}
