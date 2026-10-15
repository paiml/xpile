//! XPILE-NIGHT-CHECKS-001 — a check that cannot block a merge does not run on
//! the pull-request path (PMAT-2170, row C3 of
//! `docs/specifications/sub/release-v0.1.619-plan.md`, epic #2124).
//!
//! A check has two states. A GATE can block a merge or a release; on `main`
//! the gates are the `XPILE-ENFORCEMENT REQUIRED-CONTEXTS:` set in `ci.yml`
//! (`ruleset_drift.rs` pins that set to the live branch rules). Every other
//! `ci.yml` job is listed in the `XPILE-ENFORCEMENT ADVISORY-CONTEXTS:` marker
//! and cannot block anything. Before this gate, all of them still ran on every
//! pull request: a red there held nothing, and a reader of the PR saw a red
//! that meant nothing. Those jobs now run only on the nightly `schedule:` and
//! on `workflow_dispatch`; a red opens or updates one issue with one owner.
//!
//! The set of night jobs is READ from the advisory marker, not restated here,
//! so a job added to `ci.yml` without a decision is caught twice: by
//! `ruleset_drift.rs` (it must be listed) and by this file (once listed, it
//! must be night-only).
//!
//! What this file asserts, all from `ci.yml` alone (no network, never skips):
//! 1. every advisory job carries exactly the job-level `if:` [`NIGHT_IF`];
//! 2. no required job carries a job-level `if:`, and the workflow still runs
//!    on `pull_request` and on `push` to `main`, so the gates are unchanged;
//! 3. `gate` and `workspace-test` do not `needs:` a night job (a skipped need
//!    would skip the gate);
//! 4. every `continue-on-error` in the workflow sits inside a job that carries
//!    [`NIGHT_IF`] (being listed as advisory is not enough), so
//!    none is left on the pull-request path.
//! 5. the workflow triggers on `schedule` and on `workflow_dispatch`, so the
//!    night condition can come true: a night job in a workflow with no night
//!    trigger never runs, and that is a check turned off, not moved (#2167
//!    adds both triggers; this branch merges after it).
//!
//! RED arm, recorded in the PR: on the tree before PMAT-2170 (no `if:` on the
//! seven jobs) assertion 1 fails for all seven and assertion 4 fails for
//! `license-scan`'s raw report step.

use std::collections::BTreeSet;
use std::path::PathBuf;

const CI: &str = ".github/workflows/ci.yml";
const REQUIRED_MARKER: &str = "XPILE-ENFORCEMENT REQUIRED-CONTEXTS:";
const ADVISORY_MARKER: &str = "XPILE-ENFORCEMENT ADVISORY-CONTEXTS:";

/// The one job-level condition a night job carries. Exact string: a second
/// spelling would be a second rule.
const NIGHT_IF: &str =
    "github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'";

fn ci_text() -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(CI)).unwrap_or_else(|e| panic!("read {CI}: {e}"))
}

fn ci_yaml() -> serde_yaml::Value {
    serde_yaml::from_str(&ci_text()).unwrap_or_else(|e| panic!("parse {CI}: {e}"))
}

/// The single marker line's comma-separated set. Exactly one line per marker.
fn marker_set(marker: &str) -> BTreeSet<String> {
    let text = ci_text();
    let lines: Vec<&str> = text
        .lines()
        .filter_map(|l| l.split_once(marker).map(|(_, rest)| rest))
        .collect();
    assert_eq!(
        lines.len(),
        1,
        "{CI} must carry exactly one `{marker}` line"
    );
    let set: BTreeSet<String> = lines[0]
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    assert!(!set.is_empty(), "`{marker}` in {CI} lists nothing");
    set
}

fn job<'a>(y: &'a serde_yaml::Value, id: &str) -> &'a serde_yaml::Value {
    let j = &y["jobs"][id];
    assert!(j.is_mapping(), "{CI} has no job `{id}` (named by a marker)");
    j
}

fn needs(j: &serde_yaml::Value) -> BTreeSet<String> {
    match &j["needs"] {
        serde_yaml::Value::String(s) => [s.clone()].into(),
        serde_yaml::Value::Sequence(v) => v
            .iter()
            .filter_map(|n| n.as_str().map(str::to_string))
            .collect(),
        _ => BTreeSet::new(),
    }
}

#[test]
fn every_advisory_job_runs_only_at_night() {
    let y = ci_yaml();
    let night = marker_set(ADVISORY_MARKER);
    let wrong: Vec<String> = night
        .iter()
        .filter(|id| job(&y, id)["if"].as_str().map(str::trim) != Some(NIGHT_IF))
        .map(|id| format!("{id}: if = {:?}", job(&y, id)["if"]))
        .collect();
    assert!(
        wrong.is_empty(),
        "these jobs cannot block a merge but can still run on a pull request \
         or a push; each needs the job-level `if: {NIGHT_IF}`:\n  {}",
        wrong.join("\n  ")
    );
}

#[test]
fn the_gates_still_run_on_every_pull_request_and_push() {
    let y = ci_yaml();
    let on = y["on"].as_mapping().expect("ci.yml has an `on:` mapping");
    for trigger in ["pull_request", "push"] {
        assert!(
            on.contains_key(trigger),
            "{CI} no longer triggers on `{trigger}`: the required gates would not report there"
        );
    }
    let night = marker_set(ADVISORY_MARKER);
    for id in marker_set(REQUIRED_MARKER) {
        let j = job(&y, &id);
        assert!(
            j["if"].is_null(),
            "required job `{id}` carries a job-level `if:` ({:?}); a gate that \
             can be skipped is not a gate",
            j["if"]
        );
        let bad: Vec<String> = needs(j).intersection(&night).cloned().collect();
        assert!(
            bad.is_empty(),
            "required job `{id}` needs night job(s) {bad:?}, which are skipped \
             on a pull request, so `{id}` would be skipped too"
        );
    }
}

#[test]
fn no_continue_on_error_is_left_on_the_pull_request_path() {
    let y = ci_yaml();
    let jobs = y["jobs"].as_mapping().expect("ci.yml has a jobs: mapping");
    let mut found = Vec::new();
    for (id, j) in jobs {
        let id = id.as_str().unwrap_or_default().to_string();
        // Off the pull-request path only if the job actually carries the night
        // condition; being listed in the advisory marker is not enough.
        if j["if"].as_str().map(str::trim) == Some(NIGHT_IF) {
            continue;
        }
        if !j["continue-on-error"].is_null() {
            found.push(format!("{id} (job level)"));
        }
        for step in j["steps"].as_sequence().into_iter().flatten() {
            if !step["continue-on-error"].is_null() {
                let name = step["name"].as_str().unwrap_or("<unnamed step>");
                found.push(format!("{id}: {name}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "`continue-on-error` on the pull-request path is a third state (a check \
         that runs and cannot fail); move it to a night job or remove it:\n  {}",
        found.join("\n  ")
    );
}

#[test]
fn the_night_jobs_have_a_night_to_run_in() {
    let y = ci_yaml();
    let on = y["on"].as_mapping().expect("ci.yml has an `on:` mapping");
    let missing: Vec<&str> = ["schedule", "workflow_dispatch"]
        .into_iter()
        .filter(|t| !on.contains_key(*t))
        .collect();
    assert!(
        missing.is_empty(),
        "{CI} gates its night jobs on `{NIGHT_IF}` but has no {missing:?} \
         trigger, so those jobs never run anywhere"
    );
    assert!(
        on["schedule"]
            .as_sequence()
            .is_some_and(|s| s.iter().any(|e| e["cron"].as_str().is_some())),
        "{CI} `schedule:` carries no `cron:` entry, so the night never comes"
    );
}
