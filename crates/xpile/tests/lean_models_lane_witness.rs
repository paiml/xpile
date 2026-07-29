//! XPILE-LEANMODELS-001 (PMAT-1472) — the Mathlib lane published a DERIVATION it
//! does not carry, on a file `cargo package` uploads and no test had ever read.
//!
//! THE DEFECT. `contracts/lean-models/README.md`, under a heading that reads
//! "## What's proven", closed its capstone table with:
//!
//! > **Subsumes** the constant model (`k = 1`, `φ₀ ≡ 1`) and simple linear
//! > regression (`k = 2`, `φ₀ ≡ 1`, `φ₁ = x`) as special cases of one general
//! > theorem.
//!
//! and `Models/GeneralLinear.lean`'s own module doc said the same. Measured:
//! `Models/Basic.lean` and `Models/SimpleLinear.lean` **do not import**
//! `Models.GeneralLinear`, and no corollary, `example`, or specialisation
//! anywhere in the lane instantiates `ols_unique`/`ols_strict` at `k = 1` or
//! `k = 2`. A green `lake build` therefore establishes **three independent
//! theorems**, not one general theorem plus two derived ones.
//!
//! WHY THAT IS THE SHARP KIND OF FALSE. This lane's entire value proposition is
//! that `warningAsError := true` makes a green build an un-fakeable "no `sorry`"
//! claim. "Subsumes … as special cases of one general theorem", printed in a
//! table of what is *proven*, is a claim about the SHAPE OF THE PROOF — and the
//! build that certifies everything else on the page certifies nothing about it.
//! This is [[PMAT-1468]]'s lesson one lane over: **a green build is not a
//! discharged claim — read what it is quantified over.** Nothing here would
//! notice if `Basic.lean`'s statement drifted out of agreement with the general
//! one, because nothing connects them.
//!
//! AND IT WAS UNREADABLE BY CONSTRUCTION. `contracts/lean-models/README.md` is
//! uploaded by `cargo package -p xpile` and, before this file, was read by
//! **zero** tests — the [[PMAT-1466]] / [[PMAT-1468]] packaged-surface class,
//! which MEMORY had recorded as "read but NOT measured". Reading a file is not
//! measuring it.
//!
//! WHAT THIS FILE PINS.
//!
//! 1. **Every theorem the README tabulates exists**, in the module the README
//!    files it under. The names are PARSED OUT OF THE README, not typed here, so
//!    the packaged page checks itself against the Lean sources.
//! 2. **Every path the README cites exists.** Also parsed, not listed.
//! 3. **A derivation claim must be backed by an import or disclosed.** If a
//!    document in the lane says one result subsumes another, either the subsumed
//!    module imports the subsuming one, or the sentence carries a disclosure.
//! 4. The lane's module set and the README's account of it agree BOTH ways.
//! 5. `warningAsError := true` — the claim the "un-fakeable" argument rests on.
//!
//! WHAT IS HONEST AND WAS NOT "CORRECTED", measured before touching it: all 12
//! tabulated theorem names exist as `theorem`s in the named files; all six cited
//! companion artifacts exist; the core lane really is Mathlib-free (every
//! `Mathlib` hit under `contracts/lean/` is prose about NOT importing it — a
//! naive grep counts prose about X as X); and `contracts/kani/ols_model_uniqueness.rs`
//! already says "Do not read this harness as proving `ols_unique`". The page was
//! wrong in one place and right in the rest; the fix is scoped to the one place.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = workspace_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

const README: &str = "contracts/lean-models/README.md";
const LANE: &str = "contracts/lean-models";

/// The lane's Lean modules, DISCOVERED from the directory.
fn lane_modules() -> Vec<String> {
    let dir = workspace_root().join(LANE).join("Models");
    let mut out: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {LANE}/Models: {e}"))
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".lean"))
        .collect();
    out.sort();
    assert!(
        !out.is_empty(),
        "{LANE}/Models holds no .lean modules — every rule below would range over nothing"
    );
    out
}

/// `(theorem name, module the README files it under)`, PARSED from the README's
/// tables. The section heading that most recently named a `Models/X.lean` file
/// determines the module, which is how the page itself is organised.
fn tabulated_theorems() -> Vec<(String, String)> {
    let body = read(README);
    let mut current = String::new();
    let mut out = Vec::new();
    for line in body.lines() {
        if line.starts_with('#') {
            if let Some(a) = line.find("Models/") {
                let rest = &line[a + "Models/".len()..];
                if let Some(b) = rest.find(".lean") {
                    current = rest[..b].to_string();
                }
            }
            continue;
        }
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("| `") {
            if let Some(end) = rest.find('`') {
                let name = &rest[..end];
                let looks_like_ident = !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
                if looks_like_ident && !current.is_empty() {
                    out.push((name.to_string(), format!("{current}.lean")));
                }
            }
        }
    }
    assert!(
        out.len() >= 4,
        "parsed only {} theorem rows out of {README}; the tables were restructured and this rule \
         has stopped ranging over them",
        out.len()
    );
    out
}

#[test]
fn every_theorem_the_readme_tabulates_exists_in_the_module_it_names() {
    // The packaged page checks itself against the Lean sources. Names are parsed
    // out of the README, so adding a row to the table extends the rule for free.
    let rows = tabulated_theorems();
    let mut missing = Vec::new();
    for (name, module) in &rows {
        let src = read(&format!("{LANE}/Models/{module}"));
        let declared = src.lines().any(|l| {
            let l = l.trim_start();
            (l.starts_with("theorem ") || l.starts_with("lemma "))
                && l.split_whitespace().nth(1).is_some_and(|n| {
                    n.trim_end_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_') == name
                })
        });
        if !declared {
            missing.push(format!("{name} (README files it under Models/{module})"));
        }
    }
    assert!(
        missing.is_empty(),
        "\n{README} tabulates theorems that do not exist where it says they do:\n  {}\n\
         This page is uploaded to crates.io by `cargo package -p xpile`.",
        missing.join("\n  ")
    );
    assert!(
        rows.len() >= 12,
        "the README tabulated {} theorems; it published 12 when this rule was written, so rows \
         have been REMOVED — check that the lane did not shrink silently",
        rows.len()
    );
}

#[test]
fn every_path_the_readme_cites_exists() {
    // Parsed, not listed: any backticked token that looks like a repo path.
    let body = read(README);
    let root = workspace_root();
    let mut missing = Vec::new();
    let mut checked = 0usize;
    for span in body.split('`').skip(1).step_by(2) {
        let cand = span.trim();
        let is_path = (cand.ends_with(".lean")
            || cand.ends_with(".rs")
            || cand.ends_with(".py")
            || cand.ends_with(".yaml"))
            && !cand.contains(' ')
            && !cand.contains('(');
        if !is_path {
            continue;
        }
        // README paths are written relative to the lane or to the repo root.
        let rel = cand.trim_start_matches("./");
        let candidates = [
            root.join(rel),
            root.join(LANE).join(rel),
            root.join(LANE).join(rel.trim_start_matches("../")),
            root.join(rel.trim_start_matches("../")),
            // bare module names — the tables write `Basic.lean`, not the full path
            root.join(LANE).join("Models").join(rel),
            // bare fixture names — `ols_model.py` is named without its directory
            root.join("crates/xpile/tests/fixtures").join(rel),
        ];
        checked += 1;
        if !candidates.iter().any(|p| p.is_file()) {
            missing.push(cand.to_string());
        }
    }
    assert!(
        checked >= 6,
        "only {checked} path-shaped citations were found in {README}; the parser has stopped \
         matching and this rule ranges over almost nothing"
    );
    assert!(
        missing.is_empty(),
        "\n{README} cites paths that do not exist:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn a_derivation_claim_is_backed_by_an_import_or_disclosed() {
    // THE RULE THAT CATCHES THE DEFECT. "A subsumes B" is a claim about the shape
    // of the PROOF. The lane may make it only if B's module actually imports A —
    // i.e. the derivation exists — or if the sentence discloses that it does not.
    let mut docs: Vec<(String, String)> = vec![(README.to_string(), read(README))];
    for m in lane_modules() {
        let rel = format!("{LANE}/Models/{m}");
        let body = read(&rel);
        docs.push((rel, body));
    }

    // Does anything import the capstone? Derived, not assumed.
    let importers: Vec<String> = lane_modules()
        .into_iter()
        .filter(|m| m != "GeneralLinear.lean")
        .filter(|m| {
            read(&format!("{LANE}/Models/{m}"))
                .lines()
                .any(|l| l.trim_start().starts_with("import") && l.contains("GeneralLinear"))
        })
        .collect();

    let mut offenders = Vec::new();
    for (rel, body) in &docs {
        for (idx, line) in body.lines().enumerate() {
            let l = line.to_ascii_lowercase();
            let claims_derivation = l.contains("subsumes")
                || l.contains("as special cases of")
                || l.contains("as a special case of");
            if !claims_derivation {
                continue;
            }
            // Disclosure may sit BEFORE or AFTER the sentence: the README puts a
            // blockquote beneath the claim, and that blockquote itself QUOTES the
            // old wording, so a forward-only window flags the disclosure as an
            // offender. Found by the red half, not by reasoning.
            let lo = idx.saturating_sub(14);
            let window: String = body
                .lines()
                .skip(lo)
                .take(idx - lo + 14)
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_lowercase();
            let disclosed = window.contains("does not derive")
                || window.contains("not derived")
                || window.contains("independent theorems")
                || window.contains("no corollary");
            if !disclosed && importers.is_empty() {
                offenders.push(format!("{rel}:{}: {}", idx + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "\nthe Mathlib lane claims a DERIVATION that its modules do not carry:\n  {}\n\n\
         No module under {LANE}/Models imports GeneralLinear, so nothing instantiates \
         `ols_unique` at k=1 or k=2 — a green `lake build` proves {} INDEPENDENT theorems. \
         Either add the corollaries (making the claim true) or disclose that the derivation \
         is mathematical rather than formalised.",
        offenders.join("\n  "),
        lane_modules().len(),
    );
}

#[test]
fn the_readme_and_the_lane_agree_on_the_module_set_both_ways() {
    let body = read(README);
    let modules = lane_modules();
    for m in &modules {
        let stem = m.trim_end_matches(".lean");
        assert!(
            body.contains(stem),
            "{LANE}/Models/{m} exists and {README} never mentions it — the packaged page \
             under-reports the lane"
        );
    }
    // And the other way: every `Models/X.lean` the README names must exist.
    let mut named = 0usize;
    for span in body.split('`').skip(1).step_by(2) {
        if let Some(rest) = span.trim().strip_prefix("Models/") {
            if rest.ends_with(".lean") {
                named += 1;
                assert!(
                    modules.iter().any(|m| m == rest),
                    "{README} names Models/{rest}, which is not in the lane: {modules:?}"
                );
            }
        }
    }
    assert!(
        named >= 3,
        "{README} names {named} lane modules; it named 3 when this rule was written"
    );
}

#[test]
fn the_unfakeable_build_claim_rests_on_a_setting_that_is_actually_set() {
    // The README's argument for trusting a green build is `warningAsError := true`.
    // If that ever stops being set, every "proven" claim on the page weakens and
    // nothing else would notice.
    let body = read(README);
    assert!(
        body.contains("warningAsError"),
        "{README} no longer rests its argument on warningAsError; re-point this rule"
    );
    let lakefile = read(&format!("{LANE}/lakefile.lean"));
    let set = lakefile
        .lines()
        .any(|l| l.contains("warningAsError") && l.contains("true") && !l.trim().starts_with("--"));
    assert!(
        set,
        "{README} claims `warningAsError := true` makes a green build an un-fakeable \
         no-`sorry` claim, and {LANE}/lakefile.lean does not set it"
    );
}

#[test]
fn the_pre_fix_subsumption_sentence_reds_the_derivation_rule() {
    // NON-VACUITY. The verbatim sentences this slice replaced, in both files.
    const PRE_FIX_README: &str = "**Subsumes** the constant model (`k = 1`, `φ₀ ≡ 1`) and simple \
         linear regression (`k = 2`, `φ₀ ≡ 1`, `φ₁ = x`) as special cases of one general theorem.";
    const PRE_FIX_LEAN: &str = "Subsumes the constant model (`k = 1`, `φ₀ ≡ 1`) and simple linear \
         regression (`k = 2`, `φ₀ ≡ 1`, `φ₁ = x`).";

    for (what, text) in [
        ("README", PRE_FIX_README),
        ("GeneralLinear.lean", PRE_FIX_LEAN),
    ] {
        let l = text.to_ascii_lowercase();
        assert!(
            l.contains("subsumes") || l.contains("as special cases of"),
            "{what}'s pre-fix sentence is not recognised as a derivation claim, so the rule \
             could not have caught the defect it was written for"
        );
        let disclosed = l.contains("does not derive")
            || l.contains("not derived")
            || l.contains("independent theorems")
            || l.contains("no corollary");
        assert!(
            !disclosed,
            "{what}'s pre-fix sentence would have counted as disclosed — the rule is vacuous"
        );
    }

    // And the premise really does hold: nothing imports the capstone. If someone
    // adds the corollaries, THIS assertion reds and the account above must be
    // rewritten — which is the correct outcome, not a false alarm.
    let importers = lane_modules()
        .into_iter()
        .filter(|m| m != "GeneralLinear.lean")
        .filter(|m| read(&format!("{LANE}/Models/{m}")).contains("import Models.GeneralLinear"))
        .count();
    assert_eq!(
        importers, 0,
        "a lane module now imports GeneralLinear — the derivation may now exist, so the \
         disclosure added by PMAT-1472 has itself become the stale claim and must be revisited"
    );
}
