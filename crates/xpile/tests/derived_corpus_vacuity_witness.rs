//! XPILE-SKIPGUARD-003 (PMAT-1507) — a `for` loop over a corpus DERIVED from
//! `git ls-files` must be unable to pass by iterating nothing.
//!
//! ## The shape, and where it sits in the family
//!
//! PMAT-1505 closed *a presence probe that cannot succeed skips the whole
//! test*. PMAT-1506 closed the sibling one layer down — *a conditional whose
//! string-literal needle the corpus never contains, so the assertion inside
//! never runs while the test goes on passing*. Its `next_lane` entry named the
//! third shape and did not ship it: **`for x in COLLECTION { assert!(…) }`
//! where `COLLECTION` is empty.** The whole loop is skipped, every assertion
//! inside it is skipped, the test prints `ok`, and the passing-test count is
//! identical — the same immune-to-reading signature, with an even smaller
//! footprint than PMAT-1506's, because there is not even a conditional to read.
//!
//! ## Why this file is scoped to `git ls-files` and not to every loop
//!
//! **586** assertion-bearing `for` loops exist across the tracked test corpus
//! (measured by PMAT-1506). A rule demanding a floor from all of them reds
//! hundreds of correct files and gets disabled, which is PMAT-1500's lesson and
//! the reason its `next_lane` entry made the scope a *constraint* rather than a
//! preference. The subset this file takes is the one where **emptiness can only
//! mean the scan missed**: a corpus derived by shelling out to `git ls-files`.
//! When such a loop runs zero times, the pathspec matched nothing, or `git`
//! was not there, or the working directory was wrong — and in every one of
//! those cases the property the test is named for went unchecked while the test
//! reported success. A literal-array loop is a different animal (the elements
//! are right there in the source) and is the shipped needle arm's business.
//!
//! ## What was measured, 2026-07-31, before this file was written
//!
//! The whole tracked test corpus was scanned for assertion-bearing loops over
//! collections transitively derived from the repository, and **every one of the
//! 28 unfloored sites was then instrumented with an iteration counter and
//! executed** — PMAT-1505's mandate that a test's EFFECT, not its source, is
//! the measurement. 19 test binaries, all at exit 0. Twenty-six ran non-empty.
//! **Two ran zero times, and neither is a defect:**
//!
//! | site | iterations | reading |
//! |---|---|---|
//! | `mcp_surface_disclosure_witness.rs:310` | **0** | correct — negative regression scan, PMAT-1499 fixed the claim it hunts; factored + controlled by PMAT-1506 |
//! | `release_runbook_facts_witness.rs:277` | **0** | correct — the live runbook carries no `Cargo.toml:<N>` citation; `the_citation_detector_can_still_fire` is its control |
//! | the other 26 | 2 … 178 | live |
//!
//! **So the finding is that there is no finding, and that is the result worth
//! recording.** A sweep that reports only refutations reads as a hunt for
//! embarrassments rather than a measurement (PMAT-1505). All ten files that
//! derive a corpus from `git ls-files` already floor it. What none of them had
//! is anything that would notice if the eleventh did not — the property was
//! held by convention, and convention is what this repository has repeatedly
//! measured to be a suggestion (`claims_drift.rs`: *"a doc rule with no gate is
//! a suggestion"*). This file is the ratchet, not a repair.
//!
//! ## The rule
//!
//! Over every tracked `crates/*/tests/*.rs`: if a function contains a `for`
//! loop that carries an assertion and iterates a value transitively derived
//! from `git ls-files`, then that collection must be ANCHORED — by
//!
//! * (A) a non-vacuity assertion in the same function (`is_empty` / `.len()`
//!   against the collection), or
//! * (B) a counter incremented in the loop that a later assertion reads, or
//! * (C) a floor inside the derivation helper itself, which is where several of
//!   the live files correctly put it.
//!
//! **No exemption list exists here**, and that is deliberate: PMAT-1495's
//! exemption trap has fired on seven consecutive slices, and an exemption
//! nobody has seen fire is the thing it punishes. The rule lands green on the
//! live corpus with nothing carved out.
//!
//! ⛔ **AND THE RULE'S REACH IS ITS SUBJECT, NOT THE CORPUS IT WALKS.** The scan
//! reads all 278 tracked test files; the sites it actually governs are the ones
//! that both derive from `git ls-files` AND assert inside the loop, and measured
//! 2026-07-31 there are **exactly two** — `lean_source_lang_refusal_witness.rs`
//! and `build_script_path_independence.rs`. Ten files derive such a corpus, but
//! most of them assert AFTER the loop over counters it fills, which is anchor
//! (B) and a different shape. Two is a thin subject and this file says so
//! rather than letting a 278-file corpus imply otherwise; `the_subject_class_is
//! _not_empty` pins it, so a detector that stops matching reds instead of
//! reporting a clean sweep over nothing.
//!
//! ## Self-analysis, and the trap PMAT-1506 hit on CI
//!
//! PMAT-1506's gate went green locally and **red on CI**, because a
//! `git ls-files` corpus cannot see an UNTRACKED file: the new gate did not
//! analyse *itself* until it was committed, and when it did, it flagged its own
//! test fixtures. Both halves are pre-empted here. `this_gate_is_inside_its_own
//! _corpus` asserts this path is in the scanned set, so the self-analysis is
//! proven rather than assumed. And the constructed fixtures below never contain
//! the derivation literal contiguously — they build it with `concat!` — so the
//! detector cannot mistake this file's TEST DATA for live code. That is the
//! repository's own established remedy: **factor so the shape no longer exists,
//! rather than exempt it** (PMAT-1506).
//!
//! ## Honest scope
//!
//! This covers `git ls-files`-derived corpora only. Collections derived from a
//! `read_dir` walk or from parsing a document are the same class and are NOT
//! covered — measured above, they are clean today, and `queue.yaml`
//! `next_lane` carries them. Saying so here rather than letting the file name
//! imply otherwise is the point of the exercise.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

/// The literal that marks a corpus as repository-derived. Built by `concat!`
/// so that no contiguous occurrence exists in this file's own source — the
/// detector therefore reads this gate as deriving nothing, and its fixtures
/// below cannot be mistaken for live derivations (PMAT-1506's CI failure).
fn deriv_literal() -> String {
    concat!("ls-", "files").to_string()
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
fn subject_sites(rel: &str, src: &str, deriv: &str) -> Vec<Finding> {
    let lines: Vec<&str> = src.lines().collect();
    let fns = functions(src);

    // Which helper functions are themselves derivations? Fixpoint, so a helper
    // that calls a helper that shells out is still a derivation.
    let mut deriv_fns: BTreeSet<String> = BTreeSet::new();
    for _ in 0..4 {
        for (a, b, name) in &fns {
            let body = lines[*a..=*b].join("\n");
            let direct = body.contains(deriv);
            let indirect = deriv_fns
                .iter()
                .any(|d| body.contains(&format!("{d}(")) && d != name);
            if direct || indirect {
                deriv_fns.insert(name.clone());
            }
        }
    }

    let mut found = Vec::new();
    for (a, b, fname) in &fns {
        let body: Vec<&str> = lines[*a..=*b].to_vec();
        let whole = body.join("\n");

        // Locals bound to a derived value, transitively.
        let mut derived: BTreeSet<String> = BTreeSet::new();
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
                let from_deriv = rhs.contains(deriv)
                    || deriv_fns.iter().any(|d| rhs.contains(&format!("{d}(")))
                    || derived.iter().any(|v| {
                        rhs.contains(&format!("{v}."))
                            || rhs.contains(&format!("&{v}"))
                            || rhs.contains(&format!(" {v} "))
                    });
                if from_deriv {
                    derived.insert(var);
                }
            }
        }

        for (k, line) in body.iter().enumerate() {
            let t = line.trim_start();
            if !t.starts_with("for ") || !t.trim_end().ends_with('{') {
                continue;
            }
            let Some(iter_expr) = t.split_once(" in ").map(|(_, r)| r) else {
                continue;
            };
            let collection = derived
                .iter()
                .find(|v| {
                    iter_expr.contains(&format!("&{v}"))
                        || iter_expr.contains(&format!("{v}."))
                        || iter_expr
                            .split_whitespace()
                            .any(|w| w.trim_matches(&['&', '{'][..]) == v.as_str())
                })
                .cloned()
                .or_else(|| {
                    deriv_fns
                        .iter()
                        .find(|d| iter_expr.contains(&format!("{d}(")))
                        .map(|d| format!("{d}()"))
                })
                .or_else(|| iter_expr.contains(deriv).then(|| "<inline>".to_string()));
            let Some(collection) = collection else {
                continue;
            };

            let end = block_end(&body, k);
            let inner = body[k..=end].join("\n");
            if !has_assert(&inner) {
                continue;
            }

            // (A) a floor on the collection in this function.
            let base = collection.replace("()", "");
            let mut anchored = whole.contains(&format!("{base}.is_empty()"))
                || whole.contains(&format!("{base}.len()"))
                || whole.contains(&format!("!{base}.is_empty()"))
                || whole.contains(&format!("{base}().is_empty()"))
                || whole.contains(&format!("{base}().len()"));

            // (B) a counter incremented in the loop and read after it.
            if !anchored {
                let after = body[end..].join("\n");
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
                    if !counter.is_empty() && after.contains(&counter) && has_assert(&after) {
                        anchored = true;
                    }
                }
            }

            // (C) a floor inside the derivation helper itself.
            if !anchored {
                for (ha, hb, hname) in &fns {
                    if !collection.starts_with(hname.as_str()) && !derived.contains(hname) {
                        continue;
                    }
                    let hbody = lines[*ha..=*hb].join("\n");
                    if hbody.contains("is_empty()") || hbody.contains(".len()") {
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

/// THE RULE. Every `git ls-files`-derived assertion loop is anchored.
#[test]
fn every_ls_files_derived_assertion_loop_is_anchored() {
    let deriv = deriv_literal();
    let files = tracked_test_sources();
    assert!(
        files.len() > 100,
        "the corpus derivation returned {} file(s); this gate would pass by scanning \
         nothing, which is the exact defect it exists to forbid",
        files.len()
    );

    let mut findings = Vec::new();
    for (rel, src) in &files {
        findings.extend(subject_sites(rel, src, &deriv));
    }
    findings.sort();
    let unanchored: Vec<&Finding> = findings.iter().filter(|f| !f.anchored).collect();

    assert!(
        unanchored.is_empty(),
        "XPILE-SKIPGUARD-003: {} loop(s) iterate a corpus derived from `git {}` and \
         carry an assertion, with nothing that reds if the corpus comes back EMPTY. \
         A pathspec that matches nothing, a missing `git`, or a wrong working \
         directory then skips every assertion in the loop and the test still prints \
         `ok`. Add a non-vacuity floor on the collection (or a counter a later \
         assertion reads). Sites:\n{}",
        unanchored.len(),
        deriv,
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
#[test]
fn the_extractor_still_finds_the_known_derivations() {
    let deriv = deriv_literal();
    let files = tracked_test_sources();

    let deriving: Vec<&str> = files
        .iter()
        .filter(|(_, src)| src.contains(&deriv))
        .map(|(rel, _)| rel.as_str())
        .collect();

    assert!(
        deriving.len() >= 8,
        "only {} tracked test file(s) derive a corpus from `git {}`; ten did when \
         this gate was written (2026-07-31). Either the extractor is broken or the \
         corpus moved — check which before lowering this floor.",
        deriving.len(),
        deriv
    );

    // Anchors: files whose derivation is load-bearing and long-lived.
    for anchor in [
        "shell_artifact_policy_witness.rs",
        "skip_guard_vacuity_witness.rs",
    ] {
        assert!(
            deriving.iter().any(|f| f.ends_with(anchor)),
            "`{anchor}` derives its corpus from `git {deriv}` and the extractor no \
             longer sees it; the scan has drifted off its own subject"
        );
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
/// Measured 2026-07-31: the live subject class is exactly TWO sites, both
/// anchored. Two is small, and saying so is the point — the rule's reach is
/// its subject, not the 278-file corpus it walks.
#[test]
fn the_subject_class_is_not_empty() {
    let deriv = deriv_literal();
    let mut subjects = Vec::new();
    for (rel, src) in &tracked_test_sources() {
        subjects.extend(subject_sites(rel, src, &deriv));
    }

    assert!(
        !subjects.is_empty(),
        "the detector found NO assertion-bearing loop over a `git {deriv}` corpus \
         anywhere in the tracked test tree. Two existed when this gate was written. \
         The rule above is therefore quantified over the empty set and cannot fail — \
         fix the extractor rather than trusting its green."
    );

    for anchor in [
        "lean_source_lang_refusal_witness.rs",
        "build_script_path_independence.rs",
    ] {
        assert!(
            subjects.iter().any(|f| f.file.ends_with(anchor)),
            "`{anchor}` carries an assertion loop over a `git {deriv}` corpus and the \
             detector no longer sees it; the subject class has drifted off its own \
             subject. Live subjects: {:?}",
            subjects.iter().map(|f| &f.file).collect::<Vec<_>>()
        );
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

    let found: Vec<Finding> = subject_sites("FIXTURE.rs", &fixture, &deriv)
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

    let found: Vec<Finding> = subject_sites("FIXTURE.rs", &fixture, &deriv)
        .into_iter()
        .filter(|f| !f.anchored)
        .collect();
    assert!(
        found.is_empty(),
        "a loop whose corpus is floored must not be flagged, or the gate reds correct \
         files and gets disabled (PMAT-1500); got {found:?}"
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
        subject_sites("FIXTURE.rs", &fixture, &deriv).is_empty(),
        "a derived loop carrying no assertion is not this gate's subject"
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

    assert!(
        !src.contains(&deriv) || src.contains("concat!"),
        "this file must not spell the derivation literal contiguously — its fixtures \
         would then read as live derivations, which is precisely how PMAT-1506's gate \
         flagged its own test data on CI"
    );

    let self_findings: Vec<Finding> = subject_sites(rel, src, &deriv)
        .into_iter()
        .filter(|f| !f.anchored)
        .collect();
    assert!(
        self_findings.is_empty(),
        "this gate reports {} unanchored site(s) in its own source: {self_findings:?}",
        self_findings.len()
    );
}
