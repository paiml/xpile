//! XPILE-LEDGER-003 (PMAT-1477) — the ledgers had a duplicate work-item id, and
//! the only thing checking for one was a human procedure.
//!
//! THE DEFECT. `docs/roadmaps/queue.yaml` carried **two** `- id: PMAT-1351`
//! entries with **divergent titles** for the same slice — two lanes each appended
//! their own record and a merge kept both. `serde_yaml`/`yaml.safe_load` accept
//! duplicate ids inside a *sequence* without complaint, so every existing check
//! passed: the file parsed, the schema held, `roadmap_registration` found the id
//! registered (twice), and the duplicate rode `main` unnoticed.
//!
//! WHY IT MATTERS MORE THAN TIDINESS. A work-item id is the join key between the
//! commit subject, the CHANGELOG heading, the queue row and the roadmap row.
//! Duplicate it and `git log --grep=PMAT-1351` no longer resolves to one slice,
//! two different titles both claim to describe it, and a reader cannot tell which
//! is current. The divergent-title case is the dangerous one: identical copies
//! are noise, but two *different* descriptions of one id are a contradiction the
//! ledger asserts about itself.
//!
//! THE ROOT CAUSE IS THAT THE CHECK WAS A HABIT, NOT A GATE. This exact failure
//! is written down in the project's own operating notes — *"queue.yaml can
//! auto-merge CLEANLY AND WRONGLY … a duplicated mapping in a YAML sequence is
//! not a parse error … check row COUNT, UNIQUENESS and ORDER every time"* — and
//! has been performed by hand after every ledger conflict since 2026-07-28. It
//! was performed correctly on those occasions and skipped on the one that
//! mattered. **A rule that lives in a procedure is enforced exactly as often as
//! someone remembers it**, which is the same lesson [[PMAT-1470]] found in prose
//! and [[PMAT-1475]] found in a gate that measured the wrong subject.
//!
//! WHAT THIS FILE PINS.
//!
//! 1. **Every work-item id appears at most once in each ledger.** The assertion
//!    is over the PARSED structure, not the text, because the text form is what
//!    fooled every prior check.
//! 2. **Every entry carries the fields the ledger is read for** (`id`, `status`,
//!    `title`), so the "keep both blocks" merge shape — which silently absorbs
//!    one entry's field block into another — reds here rather than in a release
//!    runbook.
//! 3. **An id in `queue.yaml` is registered in `roadmap.yaml`.** The queue is the
//!    working set; the roadmap is the permanent record.
//!
//! The corpus is the two ledgers named by nothing but their path — there are only
//! two, they are the project's ledgers by definition, and a discovered glob over
//! `docs/roadmaps/*.yaml` would silently widen onto any future data file dropped
//! there. Both are asserted to exist and to be non-trivially sized, so this gate
//! cannot pass over an empty or renamed ledger.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

const QUEUE: &str = "docs/roadmaps/queue.yaml";
const ROADMAP: &str = "docs/roadmaps/roadmap.yaml";

/// The entry sequence of a ledger, however the file happens to wrap it.
fn entries(rel: &str) -> Vec<serde_yaml::Mapping> {
    let path = workspace_root().join(rel);
    let body = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    let doc: serde_yaml::Value =
        serde_yaml::from_str(&body).unwrap_or_else(|e| panic!("{rel} is not valid YAML: {e}"));

    let seq = match &doc {
        serde_yaml::Value::Sequence(s) => s.clone(),
        serde_yaml::Value::Mapping(m) => {
            // `queue:` / `items:` / `roadmap:` — take the longest sequence value,
            // so a renamed key does not silently empty this gate.
            let mut best: Vec<serde_yaml::Value> = Vec::new();
            for (_, v) in m {
                if let serde_yaml::Value::Sequence(s) = v {
                    if s.len() > best.len() {
                        best = s.clone();
                    }
                }
            }
            best
        }
        other => panic!("{rel} is a {other:?}, not a ledger"),
    };

    let out: Vec<serde_yaml::Mapping> = seq
        .into_iter()
        .filter_map(|v| match v {
            serde_yaml::Value::Mapping(m) => Some(m),
            _ => None,
        })
        .filter(|m| m.contains_key(serde_yaml::Value::from("id")))
        .collect();

    // NON-VACUITY. A gate that ranges over an empty ledger asserts nothing and
    // goes on passing — which is how the duplicate survived every other check.
    assert!(
        out.len() > 50,
        "{rel} yielded only {} id-bearing entries; the ledger shape changed and every assertion \
         below has stopped ranging over it",
        out.len()
    );
    out
}

fn id_of(m: &serde_yaml::Mapping) -> String {
    m.get(serde_yaml::Value::from("id"))
        .and_then(|v| v.as_str())
        .expect("entry has a string id")
        .to_string()
}

fn title_of(m: &serde_yaml::Mapping) -> String {
    m.get(serde_yaml::Value::from("title"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

#[test]
fn no_ledger_declares_the_same_work_item_id_twice() {
    // THE RULE THAT CATCHES THE DEFECT. Asserted on the PARSED sequence: the
    // duplicate that shipped was invisible to `serde_yaml`'s validity check, to
    // the schema check, and to `roadmap_registration`.
    for rel in [QUEUE, ROADMAP] {
        let es = entries(rel);
        let mut seen: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for e in &es {
            seen.entry(id_of(e)).or_default().push(title_of(e));
        }
        let dups: Vec<(&String, &Vec<String>)> =
            seen.iter().filter(|(_, ts)| ts.len() > 1).collect();
        assert!(
            dups.is_empty(),
            "\n{rel} declares {} work-item id(s) more than once:\n{}\n\n\
             A duplicate id inside a YAML *sequence* is not a parse error, so \
             `serde_yaml` accepts it and every existing ledger check passes. An id is the join \
             key between the commit subject, the CHANGELOG heading and both ledger rows — \
             duplicated, `git log --grep=<id>` no longer resolves to one slice. Merge the entries \
             (keep the title that matches the other ledger) rather than deleting either \
             blindly.",
            dups.len(),
            dups.iter()
                .map(|(id, titles)| format!(
                    "  {id} x{}:\n{}",
                    titles.len(),
                    titles
                        .iter()
                        .map(|t| format!("     - {}", t.chars().take(110).collect::<String>()))
                        .collect::<Vec<_>>()
                        .join("\n")
                ))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

#[test]
fn every_ledger_entry_carries_the_fields_the_ledger_is_read_for() {
    // The "keep both blocks" merge shape absorbs one entry's trailing field block
    // into its neighbour, leaving a row with an `id` and nothing else. That has
    // happened repeatedly during concurrent-lane conflicts; it should red here.
    for rel in [QUEUE, ROADMAP] {
        let mut bad = Vec::new();
        for e in entries(rel) {
            for field in ["status", "title"] {
                if !e.contains_key(serde_yaml::Value::from(field)) {
                    bad.push(format!("{}: missing `{field}`", id_of(&e)));
                }
            }
        }
        assert!(
            bad.is_empty(),
            "\n{rel} has entries missing a load-bearing field:\n  {}\n\
             This is the signature of a conflict resolution that kept two `- id:` lines and let \
             one entry's field block be absorbed by the other.",
            bad.join("\n  ")
        );
    }
}

fn status_of(m: &serde_yaml::Mapping) -> String {
    m.get(serde_yaml::Value::from("status"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

#[test]
fn every_done_queue_id_is_registered_in_the_roadmap() {
    // The queue is the WORKING SET; the roadmap is the permanent record. A slice
    // that has LANDED must be registered, or it leaves no history.
    //
    // Scoped to `status: done` because the first cut of this rule required every
    // queue id to be registered and flagged ELEVEN — all of them legitimately
    // unregistered: 6 `superseded` (work that never shipped), 3 `open` (including
    // the Thursday/Friday release commits, not yet done) and 2 `deferred` (the
    // 0.1.619 lane). Registration happens when a slice lands, so a forward-looking
    // queue is not an orphan list. Found by running the rule, not by reasoning
    // about it.
    let registered: std::collections::BTreeSet<String> =
        entries(ROADMAP).iter().map(id_of).collect();
    let orphans: Vec<String> = entries(QUEUE)
        .iter()
        .filter(|e| status_of(e) == "done")
        .map(id_of)
        .filter(|id| !registered.contains(id))
        .collect();
    assert!(
        orphans.is_empty(),
        "\n{} queue id(s) marked `status: done` are not registered in {ROADMAP}: {orphans:?}\n\
         A landed slice must have a permanent record.",
        orphans.len()
    );
}

#[test]
fn unlanded_queue_statuses_are_not_required_to_be_registered() {
    // GREEN CONTROL for the rule above — the false-positive half. If the status
    // scoping is ever dropped, this reds first and names the statuses that would
    // be wrongly demanded, instead of the release runbook discovering it.
    let registered: std::collections::BTreeSet<String> =
        entries(ROADMAP).iter().map(id_of).collect();
    let unlanded: Vec<(String, String)> = entries(QUEUE)
        .iter()
        .filter(|e| status_of(e) != "done")
        .map(|e| (id_of(e), status_of(e)))
        .filter(|(id, _)| !registered.contains(id))
        .collect();
    assert!(
        !unlanded.is_empty(),
        "no unlanded-and-unregistered queue entries exist any more, so this control no longer \
         demonstrates that the `status: done` scoping is load-bearing — re-point or delete it"
    );
    for (id, st) in &unlanded {
        assert!(
            ["open", "deferred", "superseded", "planned", "blocked"].contains(&st.as_str()),
            "{id} is unregistered with status {st:?}, which is not a recognised unlanded status; \
             either it is a real orphan or the ledger has a new status this control must learn"
        );
    }
}

#[test]
fn the_duplicate_this_slice_merged_stays_merged() {
    // NON-VACUITY, anchored on the real defect. PMAT-1351 was declared twice with
    // DIVERGENT titles. If it ever returns, this names it directly rather than
    // relying on the generic rule's message.
    let es = entries(QUEUE);
    let n = es.iter().filter(|e| id_of(e) == "PMAT-1351").count();
    assert_eq!(
        n, 1,
        "PMAT-1351 appears {n} times in {QUEUE}; PMAT-1477 merged two divergent-titled copies \
         into one and recorded why in that entry's `dedup_note`"
    );
    let e = es
        .iter()
        .find(|e| id_of(e) == "PMAT-1351")
        .expect("PMAT-1351 is in the queue");
    assert!(
        e.contains_key(serde_yaml::Value::from("dedup_note")),
        "the retained PMAT-1351 entry lost its `dedup_note`, which is the only record that two \
         copies once existed and which title was chosen"
    );
}

#[test]
fn the_rule_would_actually_fire_on_a_duplicate() {
    // RED-HALF-IN-THE-GATE. The assertions above pass over the repaired ledger,
    // which proves nothing about whether they CAN fail. This constructs the
    // defect in memory and checks the detection logic reports it — so the rule is
    // never silently satisfied by a shape it cannot see.
    let mut synthetic: BTreeMap<String, Vec<String>> = BTreeMap::new();
    synthetic
        .entry("PMAT-0001".to_string())
        .or_default()
        .push("first description".to_string());
    synthetic
        .entry("PMAT-0001".to_string())
        .or_default()
        .push("a DIFFERENT description of the same id".to_string());
    let dups: Vec<_> = synthetic.iter().filter(|(_, ts)| ts.len() > 1).collect();
    assert_eq!(
        dups.len(),
        1,
        "the duplicate-detection logic did not flag a constructed duplicate; the real rule above \
         is therefore vacuous"
    );

    // And a ledger with distinct ids must NOT be flagged — the false-positive half.
    let mut clean: BTreeMap<String, Vec<String>> = BTreeMap::new();
    clean
        .entry("PMAT-0001".into())
        .or_default()
        .push("a".into());
    clean
        .entry("PMAT-0002".into())
        .or_default()
        .push("b".into());
    assert!(
        clean.values().all(|ts| ts.len() == 1),
        "the detection logic flags distinct ids as duplicates"
    );
}
