//! XPILE-SKIPGUARD-003 (PMAT-1507, widened by PMAT-1508) — a `for` loop over a
//! corpus DERIVED from the repository must be unable to pass by iterating
//! nothing. Two derivations are in scope: `git ls-files` and a `read_dir` walk.
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
//! ## Why the scope is two literals and not every loop
//!
//! **586** assertion-bearing `for` loops exist across the tracked test corpus
//! (measured by PMAT-1506). A rule demanding a floor from all of them reds
//! hundreds of correct files and gets disabled, which is PMAT-1500's lesson and
//! the reason its `next_lane` entry made the scope a *constraint* rather than a
//! preference. The subset taken here is the one where **emptiness can only mean
//! the scan missed**: a pathspec matched nothing, a directory was absent or
//! empty or resolved against the wrong working directory, `git` was not there.
//! In every one of those cases the property the test is named for went
//! unchecked while the test reported success. A literal-array loop is a
//! different animal (the elements are right there in the source) and is the
//! needle arm's business (`skip_guard_vacuity_witness.rs`).
//!
//! ⛔ **`read_to_string` was MEASURED and DECLINED, and the decline is an
//! assertion, not a comment** (`reading_a_file_is_not_a_derivation_this_gate_
//! governs`). MEASURED 2026-07-31 with this detector: adding it takes the
//! subject class from **29 sites to 54**, and **9 of the 54 come back
//! unanchored** against 0 today. The residue is dominated by
//! `for line in text.lines()` over a file that was just read — where an empty
//! iteration means an empty FILE, not a missed SCAN, because the read itself
//! already panics when the file is absent — and one member of it
//! (`release_runbook_facts_witness.rs:277`) PMAT-1507 already measured at zero
//! iterations and recorded as CORRECT. Shipping the naive widening would red
//! correct files on day one. PMAT-1507's `next_lane` entry pre-registered this
//! exclusion; PMAT-1508 checked it rather than inheriting it.
//!
//! ## What was measured, and by EFFECT
//!
//! Both arms were measured the way PMAT-1505 mandates — a test's EFFECT, not
//! its source. Every subject site was instrumented with an iteration marker and
//! executed.
//!
//! | arm | subject sites | ran zero times |
//! |---|---|---|
//! | `git ls-files` (PMAT-1507, 2026-07-31) | 2 | 0 |
//! | `read_dir` (PMAT-1508, 2026-07-31) | 27 | **0** — 16 test binaries, all exit 0, 2…103 iterations |
//!
//! **So the finding is again that there is no finding, and that is the result
//! worth recording.** A sweep that reports only refutations reads as a hunt for
//! embarrassments rather than a measurement (PMAT-1505). What the corpus did
//! not have is anything that would notice if the twenty-eighth site arrived
//! unfloored — the property was held by convention, and convention is what this
//! repository has repeatedly measured to be a suggestion (`claims_drift.rs`:
//! *"a doc rule with no gate is a suggestion"*). This file is the ratchet.
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
//!   property that lives in a helper the site never names** — PMAT-1507's
//!   instrument lesson, one layer in.
//! * **A `read_dir` loop usually BUILDS the corpus rather than checking it**,
//!   with the floor under the loop on what it filled. Anchor (B′).
//! * **The floor may be counted into a local first** (`let n = xs.len();
//!   assert!(n >= 30)`), which is how `claims_drift` writes it.
//! * **An existence probe IS a floor**: `xs.iter().find(…).expect(…)` cannot
//!   survive an empty `xs`. Anchor (A′).
//!
//! ⚠️ **AND THE ANCHOR PREDICATE ITSELF WAS HOLLOW.** As shipped it was
//! `body.contains("files.len()")` — which an `eprintln!("scanned {} files",
//! files.len())` satisfies. **A print is not a floor**: the corpus comes back
//! empty, the line prints `0`, every assertion in the loop is skipped, and the
//! test exits 0. A gate written against hollow checks was admitting one through
//! its own anchor rule. The length must now be read inside an ASSERTION
//! (`assertion_windows`), and `a_printed_length_does_not_anchor_a_derived_loop`
//! keeps it that way. Nothing live relied on the loose reading — tightening it
//! moved the `git ls-files` arm not at all — so this is a hole closed before it
//! was load-bearing, recorded rather than quietly fixed.
//!
//! ## The rule
//!
//! Over every tracked `crates/*/tests/*.rs`: if a function contains a `for`
//! loop that carries an assertion and iterates a value transitively derived
//! from `git ls-files` or a `read_dir` walk, that collection must be ANCHORED —
//! by
//!
//! * (A) an assertion in the same function reading the collection's own
//!   `is_empty()` / `.len()`, directly or through one `let` binding, or
//! * (A′) an existence probe — `find(…).expect(…)` / `.unwrap()` — over it, or
//! * (B) a counter incremented in the loop that a later assertion reads, or
//! * (B′) a collection the loop FILLS that a later assertion floors, or
//! * (C) a floor anywhere in the derivation CHAIN, resolved through calls.
//!
//! **No exemption list exists here**, and that is deliberate: PMAT-1495's
//! exemption trap has fired on seven consecutive slices, and an exemption
//! nobody has seen fire is the thing it punishes. Both arms land green on the
//! live corpus with nothing carved out.
//!
//! ⛔ **THE RULE'S REACH IS ITS SUBJECT, NOT THE CORPUS IT WALKS.** The scan
//! reads all 278 tracked test files; the sites it governs are the ones that both
//! derive in scope AND assert inside the loop — **29**, and this file prints the
//! number rather than letting a 278-file corpus imply otherwise.
//!
//! ⛔ **AND THE SUBJECT IS FLOORED PER ARM, NOT OVER THE UNION.** PMAT-1507
//! proved by neutering that the live rule stays GREEN when the detector dies;
//! re-run for PMAT-1508 with `read_dir` misspelled, `every_derived_assertion_
//! loop_is_anchored` passed again — **fourth consecutive confirmation** — while
//! `the_subject_class_is_not_empty` red at 0 sites against a floor of 20. A
//! union floor would have inherited the hole: the `git ls-files` arm alone keeps
//! the union non-empty, so the new arm could die unnoticed exactly where it is
//! new. ★ The constructed red halves cannot catch it either — they build their
//! fixtures FROM the literal, so they stay self-consistently green. **Only the
//! live subject floor sees a literal go stale.**
//!
//! ## Self-analysis, and the trap PMAT-1506 hit on CI
//!
//! PMAT-1506's gate went green locally and **red on CI**, because a
//! `git ls-files` corpus cannot see an UNTRACKED file: the new gate did not
//! analyse *itself* until it was committed, and when it did, it flagged its own
//! test fixtures. Both halves are pre-empted here. `this_gate_is_inside_its_own
//! _corpus` asserts this path is in the scanned set, so the self-analysis is
//! proven rather than assumed. And the constructed fixtures below never contain
//! either derivation literal contiguously — they build them with `concat!` — so
//! the detector cannot mistake this file's TEST DATA for live code. That is the
//! repository's own established remedy: **factor so the shape no longer exists,
//! rather than exempt it** (PMAT-1506).
//!
//! ## Honest scope
//!
//! Two derivations, not the class. A collection parsed out of a DOCUMENT — the
//! other half of PMAT-1507's `next_lane` entry — is the same shape and is NOT
//! covered, for the reason measured above: the naive spelling of it does not
//! survive the constraint that emptiness must mean the scan missed. It needs a
//! narrower predicate (a collection built by an EXTRACTION step, not raw
//! `.lines()`), and `queue.yaml` `next_lane` carries it with this measurement
//! attached. Saying so here rather than letting the file name imply otherwise is
//! the point of the exercise.

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

/// THE SUBJECT SET'S DEFINITION. A collection is repository-derived — and so
/// in this gate's scope — when it comes from one of these, transitively.
///
/// Both share the property that makes the rule safe to demand: **emptiness can
/// only mean the scan missed.** A pathspec matched nothing, a directory was
/// absent or wrong, the working directory was not the repository root. In every
/// one of those cases the loop body never runs and the test still prints `ok`.
///
/// ⛔ `read_to_string` is deliberately NOT here, and PMAT-1508 measured why
/// rather than assuming: adding it takes the subject class from 27 sites to 44
/// and the unanchored residue is dominated by `for line in text.lines()` over a
/// file that was just read. That is a different animal — the read already
/// panics when the file is absent, so an empty iteration means an empty file,
/// not a missed scan — and PMAT-1507's `next_lane` entry pre-registered exactly
/// this exclusion. A rule that reds hundreds of correct files gets disabled
/// (PMAT-1500), which is worse than no rule.
fn deriv_literals() -> Vec<String> {
    vec![deriv_literal(), walk_literal()]
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
fn subject_sites(rel: &str, src: &str, derivs: &[String]) -> Vec<Finding> {
    let lines: Vec<&str> = src.lines().collect();
    let fns = functions(src);
    let spells = |text: &str| derivs.iter().any(|d| text.contains(d.as_str()));

    // Which helper functions are themselves derivations? Fixpoint, so a helper
    // that calls a helper that shells out is still a derivation.
    let mut deriv_fns: BTreeSet<String> = BTreeSet::new();
    for _ in 0..4 {
        for (a, b, name) in &fns {
            let body = lines[*a..=*b].join("\n");
            let direct = spells(&body);
            let indirect = deriv_fns
                .iter()
                .any(|d| body.contains(&format!("{d}(")) && d != name);
            if direct || indirect {
                deriv_fns.insert(name.clone());
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
        let mut derived: BTreeSet<String> = BTreeSet::new();
        let mut whence: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
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
                .or_else(|| spells(iter_expr).then(|| "<inline>".to_string()));
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
                    if !counter.is_empty() && after.contains(&counter) && has_assert(&after) {
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

            // (C) a floor inside the derivation CHAIN — not just the helper the
            // loop names. PMAT-1508: resolving only the named helper missed
            // `lane_modules()`'s own `assert!(!out.is_empty())` whenever the
            // loop iterated a LOCAL bound to it, and missed
            // `snapshot_rulesets()`'s floor two calls below `snapshot_required`.
            // A per-site look cannot see a property that lives in a helper the
            // site never names.
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
fn live_subjects(derivs: &[String]) -> Vec<Finding> {
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
            .map(|d| format!("`{d}`"))
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
/// Measured 2026-07-31: `git ls-files` → 2 sites, `read_dir` → 27.
#[test]
fn the_subject_class_is_not_empty() {
    let arms: [(String, usize, [&str; 2]); 2] = [
        (
            deriv_literal(),
            2,
            [
                "lean_source_lang_refusal_witness.rs",
                "build_script_path_independence.rs",
            ],
        ),
        (walk_literal(), 20, ["ruleset_drift.rs", "claims_drift.rs"]),
    ];

    for (deriv, floor, anchors) in arms {
        let derivs = vec![deriv.clone()];
        let mut subjects = Vec::new();
        for (rel, src) in &tracked_test_sources() {
            subjects.extend(subject_sites(rel, src, &derivs));
        }

        assert!(
            subjects.len() >= floor,
            "the detector found {} assertion-bearing loop(s) over a `{deriv}` corpus \
             anywhere in the tracked test tree; {floor} is the floor measured when this \
             arm was written. The rule above is quantified over a set this file also \
             computes, so a detector that stops matching makes it pass having checked \
             nothing — fix the extractor rather than trusting its green.",
            subjects.len()
        );

        for anchor in anchors {
            assert!(
                subjects.iter().any(|f| f.file.ends_with(anchor)),
                "`{anchor}` carries an assertion loop over a `{deriv}` corpus and the \
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

/// RED HALF 4 — THE SCOPE BOUNDARY, asserted rather than described.
///
/// `read_to_string` is out of scope by decision, and a decision recorded only in
/// a doc comment is a suggestion. If a later slice widens `deriv_literals()` to
/// include it without re-doing PMAT-1508's measurement, this reds and says so.
#[test]
fn reading_a_file_is_not_a_derivation_this_gate_governs() {
    let read = concat!("read_to_", "string");
    assert!(
        !deriv_literals().iter().any(|d| d == read),
        "`{read}` was added to the derivation set. That widening was MEASURED and \
         declined by PMAT-1508: it takes the subject class from 29 sites to 54 and \
         leaves 9 of them UNANCHORED against 0 today, so the widening reds correct \
         files on day one. The residue is dominated by `for line in text.lines()` over \
         a file that was just read — where an empty iteration means an empty FILE, not \
         a missed SCAN, because the read itself already panics when the file is \
         absent. A narrower predicate (a collection built by an EXTRACTION step, not \
         raw `.lines()`) is the open work. Re-do the measurement and rewrite this test \
         before shipping the widening; do not delete it."
    );

    // And the boundary is live, not hypothetical: the shape it excludes exists.
    let fixture = format!(
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
        subject_sites("FIXTURE.rs", &fixture, &deriv_literals()).is_empty(),
        "a `for line in text.lines()` loop over a file that was just read is outside \
         this gate's subject, and the detector must not reach it through the widened set"
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

    for lit in deriv_literals() {
        assert!(
            !src.contains(&lit) || src.contains("concat!"),
            "this file must not spell `{lit}` contiguously — its fixtures would then \
             read as live derivations, which is precisely how PMAT-1506's gate flagged \
             its own test data on CI"
        );
    }

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
