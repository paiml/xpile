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
//!    none is left on the pull-request path; and (PMAT-2177) no night job,
//!    nor any step in one, carries `continue-on-error` either, so a red
//!    night step fails its job and reaches the issue step of assertion 6.
//! 5. the workflow triggers on `schedule` and on `workflow_dispatch`, so the
//!    night condition can come true: a night job in a workflow with no night
//!    trigger never runs, and that is a check turned off, not moved (#2167
//!    adds both triggers; this branch merges after it).
//! 6. (PMAT-2174) the red has somewhere to go: the LAST step of every night
//!    job is the `night-issue` action, under exactly [`ISSUE_IF`], with
//!    `check:` set to its own job id and `issues: write` on the job; no other
//!    job (required or not) calls it; the action names exactly one owner.
//! 7. (PMAT-2174) the action's script, EXECUTED against a stub `gh` (no
//!    network): it refuses every event but `schedule` before calling gh; it
//!    comments on an open issue whose title is exactly
//!    `night: <check> red on main` and creates nothing; it creates the issue,
//!    assigned to the one owner, when only a near-miss title exists; and a
//!    failed lookup fails the step without creating anything.
//!
//! RED arm, recorded in the PR: on the tree before PMAT-2170 (no `if:` on the
//! seven jobs) assertion 1 fails for all seven and assertion 4 fails for
//! `license-scan`'s raw report step. On the tree before PMAT-2177 (850ffff6)
//! `no_step_in_a_night_job_has_continue_on_error` fails, naming exactly
//! `license-scan`'s `cargo deny check licenses bans sources` step.

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

/// PMAT-2177: moving a `continue-on-error` step to the night does not make it
/// two-state. It still runs and cannot fail, so a red there neither fails the
/// night job nor reaches the `night-issue` step (`failure()` never comes
/// true). Every step of a night job can fail its job; so can the job itself.
#[test]
fn no_step_in_a_night_job_has_continue_on_error() {
    let y = ci_yaml();
    let jobs = y["jobs"].as_mapping().expect("ci.yml has a jobs: mapping");
    let night = marker_set(ADVISORY_MARKER);
    let mut found = Vec::new();
    for (id, j) in jobs {
        let id = id.as_str().unwrap_or_default().to_string();
        // A night job is one listed as advisory OR one carrying the night
        // condition; either way its red is meant to file the one issue.
        let is_night = night.contains(&id) || j["if"].as_str().map(str::trim) == Some(NIGHT_IF);
        if !is_night {
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
        "`continue-on-error` in a night job is a third state (C324 #6): the step \
         runs, cannot fail its job, and so never files the night issue; remove it:\n  {}",
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

// ── PMAT-2174: a red night opens or updates its one issue ──────────────────

const ACTION: &str = ".github/actions/night-issue/action.yml";
const ACTION_USES: &str = "./.github/actions/night-issue";
/// The one step-level condition on the issue step. `failure()` means it runs
/// only after its job has already failed, so it can never turn a job green or
/// red; `schedule` alone means a pull request, a merge group, a push and a
/// manual dispatch file nothing. Exact string, like [`NIGHT_IF`].
const ISSUE_IF: &str = "failure() && github.event_name == 'schedule'";

fn action_yaml() -> serde_yaml::Value {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text =
        std::fs::read_to_string(root.join(ACTION)).unwrap_or_else(|e| panic!("read {ACTION}: {e}"));
    serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("parse {ACTION}: {e}"))
}

fn calls_the_action(step: &serde_yaml::Value) -> bool {
    step["uses"].as_str().map(str::trim) == Some(ACTION_USES)
}

#[test]
fn every_night_job_ends_by_filing_its_one_issue() {
    let y = ci_yaml();
    let mut wrong = Vec::new();
    for id in marker_set(ADVISORY_MARKER) {
        let j = job(&y, &id);
        let steps = j["steps"].as_sequence().cloned().unwrap_or_default();
        let calls: Vec<&serde_yaml::Value> = steps.iter().filter(|s| calls_the_action(s)).collect();
        if calls.len() != 1 {
            wrong.push(format!(
                "{id}: calls {ACTION_USES} {} times, not once",
                calls.len()
            ));
            continue;
        }
        if !steps.last().is_some_and(calls_the_action) {
            wrong.push(format!(
                "{id}: the issue step is not the LAST step, so failure() cannot see the steps after it"
            ));
        }
        let s = calls[0];
        if s["if"].as_str().map(str::trim) != Some(ISSUE_IF) {
            wrong.push(format!(
                "{id}: issue step if = {:?}, not `{ISSUE_IF}`",
                s["if"]
            ));
        }
        if !s["continue-on-error"].is_null() {
            wrong.push(format!("{id}: issue step carries continue-on-error"));
        }
        if s["with"]["check"].as_str() != Some(id.as_str()) {
            wrong.push(format!(
                "{id}: issue step check = {:?}, not its own job id",
                s["with"]["check"]
            ));
        }
        if j["permissions"]["issues"].as_str() != Some("write") {
            wrong.push(format!("{id}: job lacks `permissions: issues: write`"));
        }
    }
    assert!(
        wrong.is_empty(),
        "a red night check must open or update its one issue (PMAT-2174):\n  {}",
        wrong.join("\n  ")
    );
}

#[test]
fn only_night_jobs_file_issues_and_the_action_names_one_owner() {
    let y = ci_yaml();
    let night = marker_set(ADVISORY_MARKER);
    let jobs = y["jobs"].as_mapping().expect("ci.yml has a jobs: mapping");
    let outside: Vec<String> = jobs
        .iter()
        .filter_map(|(id, j)| {
            let id = id.as_str().unwrap_or_default().to_string();
            let calls = j["steps"]
                .as_sequence()
                .into_iter()
                .flatten()
                .any(calls_the_action);
            (calls && !night.contains(&id)).then_some(id)
        })
        .collect();
    assert!(
        outside.is_empty(),
        "job(s) {outside:?} call {ACTION_USES} but are not night jobs; a gate's red is the \
         gate's own verdict, not a ticket"
    );
    let a = action_yaml();
    let owner = a["inputs"]["owner"]["default"].as_str().unwrap_or_default();
    assert!(
        !owner.is_empty() && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "{ACTION} must name exactly ONE owner login as `inputs.owner.default`, got {owner:?}"
    );
    let steps = a["runs"]["steps"]
        .as_sequence()
        .expect("composite action has steps");
    assert_eq!(steps.len(), 1, "{ACTION} is one script step");
    let run = steps[0]["run"]
        .as_str()
        .expect("the step has a run: script");
    assert!(
        !run.contains("${{"),
        "{ACTION}'s script interpolates an expression; inputs reach it through `env:` only"
    );
}

/// The action's script, run by bash against a stub `gh` that records every
/// call. Returns (exit code, the recorded gh calls).
fn run_action(event: &str, listed: &str, list_rc: i32) -> (i32, Vec<String>) {
    let a = action_yaml();
    let run = a["runs"]["steps"][0]["run"]
        .as_str()
        .expect("script")
        .to_string();
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("xpile-night-issue-{}-{seq}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("stub dir");
    let log = dir.join("gh.log");
    let stub = dir.join("gh");
    std::fs::write(
        &stub,
        "#!/bin/bash\nprintf '%s\\n' \"$*\" >> \"$STUB_LOG\"\n\
         if [ \"$1 $2\" = \"issue list\" ]; then printf '%b' \"$STUB_LIST\"; exit \"$STUB_LIST_RC\"; fi\n\
         exit 0\n",
    )
    .expect("write stub gh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let status = std::process::Command::new("bash")
        .arg("-c")
        .arg(&run)
        .env("PATH", path)
        .env("STUB_LOG", &log)
        .env("STUB_LIST", listed)
        .env("STUB_LIST_RC", list_rc.to_string())
        .env("GITHUB_EVENT_NAME", event)
        .env("CHECK", "kani")
        .env("OWNER", "the-one-owner")
        .env("GITHUB_REPOSITORY", "paiml/xpile")
        .env("GITHUB_SERVER_URL", "https://github.com")
        .env("GITHUB_RUN_ID", "4242")
        .env("GITHUB_SHA", "0123abcd")
        .env_remove("GH_TOKEN")
        .status()
        .expect("bash runs");
    let calls = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    (status.code().unwrap_or(-1), calls)
}

#[test]
fn the_issue_script_refuses_every_event_but_schedule() {
    for event in [
        "pull_request",
        "merge_group",
        "push",
        "workflow_dispatch",
        "",
    ] {
        let (rc, calls) = run_action(event, "", 0);
        assert_ne!(
            rc, 0,
            "event {event:?}: the night-issue script must refuse, exited 0"
        );
        assert!(
            calls.is_empty(),
            "event {event:?}: gh was called before the refusal: {calls:?}"
        );
    }
}

#[test]
fn an_open_issue_with_the_exact_title_is_commented_not_duplicated() {
    let listed = "7\tnight: kani red on main (old)\n41\tnight: kani red on main\n";
    let (rc, calls) = run_action("schedule", listed, 0);
    assert_eq!(rc, 0, "calls: {calls:?}");
    assert!(
        calls.iter().any(|c| c.starts_with("issue comment 41 ")),
        "no comment on #41: {calls:?}"
    );
    assert!(
        !calls.iter().any(|c| c.starts_with("issue create")),
        "created a second issue although #41 is open: {calls:?}"
    );
    assert!(
        calls.iter().any(|c| c.contains("/actions/runs/4242")),
        "run url not posted: {calls:?}"
    );
}

#[test]
fn a_near_miss_title_is_not_the_issue_so_one_is_created_for_the_one_owner() {
    let (rc, calls) = run_action("schedule", "7\tnight: kani red on main (old)\n", 0);
    assert_eq!(rc, 0, "calls: {calls:?}");
    let created: Vec<&String> = calls
        .iter()
        .filter(|c| c.starts_with("issue create"))
        .collect();
    assert_eq!(created.len(), 1, "expected exactly one create: {calls:?}");
    assert!(
        created[0].contains("--title night: kani red on main --assignee the-one-owner "),
        "create is not titled/assigned as specified: {}",
        created[0]
    );
    assert!(
        !calls.iter().any(|c| c.starts_with("issue comment")),
        "{calls:?}"
    );
}

#[test]
fn a_failed_lookup_fails_the_step_and_creates_nothing() {
    let (rc, calls) = run_action("schedule", "", 1);
    assert_ne!(
        rc, 0,
        "a failed `gh issue list` must fail the step: {calls:?}"
    );
    assert!(
        !calls
            .iter()
            .any(|c| c.starts_with("issue create") || c.starts_with("issue comment")),
        "acted on a lookup that failed: {calls:?}"
    );
}
