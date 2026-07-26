//! XPILE-RULESET-DRIFT-001 — the enforcement-truth gate (PMAT-1347).
//!
//! Sibling of `claims_drift.rs` (derived DOC counts) and
//! `roadmap_registration.rs` (ledger registration). Where those pin what the
//! repo says about ITSELF, this one pins what the repo says about **what
//! actually blocks a merge** — the claim that gives every other gate its
//! meaning. A gate nobody is required to pass is a suggestion.
//!
//! ## The drift this exists to catch (it already happened, twice, opposite ways)
//!
//! On 2026-07-05 the org ruleset `13878864` was flipped from `[gate]` to
//! `[gate, kani, lake-build, workspace-test]`, and `docs/status/
//! ruleset-13878864.json` was committed as the receipt. Six hours later the
//! flip was **reverted** — live `updated_at` 23:50 vs the snapshot's 17:35,
//! same day. The snapshot kept asserting a four-context set for three weeks
//! while looking authoritative, because a committed JSON file has no way to
//! notice the API moved underneath it. Simultaneously `.github/workflows/
//! ci.yml` carried a note claiming the *inverse* error — that `workspace-test`
//! was NOT enforced when it was. Two contradictory false claims, three weeks,
//! zero red tests.
//!
//! ## Why this is not just "diff the JSON in CI"
//!
//! Reading the ORG rulesets endpoint needs a token with org scope. GitHub
//! Actions' default `GITHUB_TOKEN` is repo-scoped, so a live-only check would
//! **skip green** on exactly the runner that matters — the CF-4 skip-as-green
//! shape `XPILE-WITNESS-002` exists to kill. So the gate is stratified:
//!
//! * **STATIC half** (3 tests, `std::fs` only — can never skip, runs in CI, in
//!   a released `.crate`, offline): the committed snapshot must agree with the
//!   machine-readable `XPILE-ENFORCEMENT` marker lines carried by `ci.yml` and
//!   `docs/status/*.md`, the advisory set must be exactly the leftover jobs,
//!   and every required context must name a job that actually exists.
//!   That last one is not theoretical: a required context naming no job leaves
//!   every PR permanently unmergeable rather than failing loudly.
//! * **LIVE half** (1 test): `gh api orgs/paiml/rulesets/13878864` vs the
//!   snapshot. Skips-with-reason when `gh` is absent or unauthorized, with an
//!   `XPILE_REQUIRE_RULESET_CHECK=1` tripwire (mirroring
//!   `XPILE_REQUIRE_WASM_RUNTIME` / `XPILE_REQUIRE_KANI`) so the release
//!   pre-flight can demand a real answer instead of accepting a skip.
//!
//! The static half is what makes the marker lines load-bearing: the prose and
//! the snapshot can no longer disagree silently, so the only way to lie about
//! enforcement is to lie in two files at once, and the live half then catches
//! that on any authorized run.
//!
//! **Not in scope, deliberately:** restoring `kani` + `lake-build` as required
//! contexts. That is an org-admin `PUT` (the repo-level endpoint 404s for an
//! org-sourced ruleset) and is recorded as the `ruleset-reflip` OWNER decision
//! in `docs/roadmaps/queue.yaml`. This gate reports the truth; it does not
//! change the policy.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const RULESET_ID: &str = "13878864";
const SNAPSHOT: &str = "docs/status/ruleset-13878864.json";
const REQUIRED_MARKER: &str = "XPILE-ENFORCEMENT REQUIRED-CONTEXTS:";
const ADVISORY_MARKER: &str = "XPILE-ENFORCEMENT ADVISORY-CONTEXTS:";

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let path = workspace_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The files that are allowed to make an enforcement claim. Any of them may
/// carry marker lines; all markers of a given kind must agree.
fn marker_bearing_files() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files = vec![root.join(".github/workflows/ci.yml")];
    let mut status: Vec<PathBuf> = fs::read_dir(root.join("docs/status"))
        .expect("docs/status/ exists")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    status.sort();
    files.append(&mut status);
    files
}

/// Extract `MARKER: a, b, c` payloads from a file, one set per occurrence.
///
/// Markers live in YAML comments (`# MARKER: …`) and in Markdown HTML comments
/// (`<!-- MARKER: … -->`), so the trailing `-->` is stripped before splitting —
/// otherwise the last context in a Markdown marker reads as `workspace-test -->`.
fn markers(text: &str, marker: &str) -> Vec<BTreeSet<String>> {
    text.lines()
        .filter_map(|line| line.split_once(marker))
        .map(|(_, rest)| {
            rest.trim()
                .trim_end_matches("-->")
                .split(',')
                .map(|c| c.trim().trim_matches('`').to_string())
                .filter(|c| !c.is_empty())
                .collect()
        })
        .collect()
}

/// The required-status-check contexts recorded in the committed snapshot.
fn snapshot_required() -> BTreeSet<String> {
    let json: serde_json::Value =
        serde_json::from_str(&read(SNAPSHOT)).expect("snapshot is valid JSON");
    required_from_ruleset(&json, SNAPSHOT)
}

/// Pull `rules[].parameters.required_status_checks[].context` out of a ruleset
/// payload. Shared by the snapshot and the live API so the two can never be
/// compared through different readers.
fn required_from_ruleset(json: &serde_json::Value, whence: &str) -> BTreeSet<String> {
    let rules = json["rules"]
        .as_array()
        .unwrap_or_else(|| panic!("{whence}: `rules` is not an array"));
    let checks = rules
        .iter()
        .find(|r| r["type"] == "required_status_checks")
        .unwrap_or_else(|| {
            panic!(
                "{whence}: no `required_status_checks` rule — the ruleset no longer \
                 requires ANY status check, which means nothing in this repo is \
                 merge-blocking. That is either a catastrophic regression or an \
                 intentional policy change that must be re-recorded here."
            )
        });
    checks["parameters"]["required_status_checks"]
        .as_array()
        .unwrap_or_else(|| panic!("{whence}: required_status_checks is not an array"))
        .iter()
        .map(|c| {
            c["context"]
                .as_str()
                .unwrap_or_else(|| panic!("{whence}: a required check has no `context`"))
                .to_string()
        })
        .collect()
}

/// Every `name:` under `jobs:` across the CI workflows. These are the strings
/// GitHub matches a required context against.
fn ci_job_names(rel: &str) -> BTreeSet<String> {
    // A job `name:` sits at exactly 4-space indent under `jobs:` → `<id>:`.
    // The workflow's own top-level `name:` is at column 0, and step names are
    // deeper and carry a `- ` list marker.
    read(rel)
        .lines()
        .filter(|line| line.starts_with("    name: "))
        .filter_map(|line| line.trim_start().strip_prefix("name: "))
        .map(|n| n.trim().trim_matches('"').to_string())
        .collect()
}

// ── STATIC half — no network, no tooling, can never skip ────────────────────

/// Every enforcement marker in the repo lists exactly the snapshot's required
/// set. This is the assertion that would have caught the 2026-07-05 revert on
/// the very next commit: the snapshot said four, `ci.yml` said one, and no
/// single file was internally inconsistent.
#[test]
fn enforcement_markers_match_the_committed_snapshot() {
    let expected = snapshot_required();
    assert!(
        !expected.is_empty(),
        "{SNAPSHOT} records ZERO required contexts — nothing would block a merge"
    );

    let mut seen = 0usize;
    for file in marker_bearing_files() {
        let rel = file
            .strip_prefix(workspace_root())
            .unwrap_or(&file)
            .display()
            .to_string();
        let text = fs::read_to_string(&file).unwrap_or_default();
        for found in markers(&text, REQUIRED_MARKER) {
            seen += 1;
            assert_eq!(
                found, expected,
                "enforcement drift in {rel}: its `{REQUIRED_MARKER}` marker lists \
                 {found:?} but {SNAPSHOT} records {expected:?}. Exactly one of the \
                 two is a lie about what blocks a merge. Re-derive the snapshot \
                 with `gh api orgs/paiml/rulesets/{RULESET_ID} | jq . > {SNAPSHOT}` \
                 and make every marker match it."
            );
        }
    }

    // Non-vacuity: a gate that finds no markers asserts nothing. Deleting the
    // marker lines must red this test, not silently disarm it.
    assert!(
        seen >= 2,
        "expected at least 2 `{REQUIRED_MARKER}` markers (ci.yml + the status \
         hand-off), found {seen}. The marker lines are the machine-readable half \
         of the enforcement note — removing them disarms XPILE-RULESET-DRIFT-001."
    );
}

/// The advisory set is exactly the CI jobs that are NOT required. Adding a job
/// to `ci.yml` without classifying it reds here — which is how "we added a
/// proof lane and everyone assumed it was blocking" gets caught at the commit
/// that introduces it rather than three weeks later.
#[test]
fn advisory_markers_are_exactly_the_unrequired_ci_jobs() {
    let required = snapshot_required();
    let jobs = ci_job_names(".github/workflows/ci.yml");
    assert!(
        jobs.len() >= 4,
        "parsed only {} job names from ci.yml — the parser is broken, not the \
         workflow: {jobs:?}",
        jobs.len()
    );

    let expected_advisory: BTreeSet<String> = jobs.difference(&required).cloned().collect();

    let mut seen = 0usize;
    for file in marker_bearing_files() {
        let rel = file
            .strip_prefix(workspace_root())
            .unwrap_or(&file)
            .display()
            .to_string();
        let text = fs::read_to_string(&file).unwrap_or_default();
        for found in markers(&text, ADVISORY_MARKER) {
            seen += 1;
            assert_eq!(
                found, expected_advisory,
                "advisory-set drift in {rel}: marker lists {found:?} but the CI jobs \
                 that are not required are {expected_advisory:?} (jobs {jobs:?} minus \
                 required {required:?}). A job that is neither required nor disclosed \
                 as advisory is a job whose enforcement status nobody has decided."
            );
        }
    }
    assert!(
        seen >= 1,
        "no `{ADVISORY_MARKER}` marker found — the advisory lane is undisclosed"
    );
}

/// A required context that names no job is the worst failure mode available:
/// GitHub reports it forever `Expected — Waiting for status to be reported`, so
/// every PR becomes permanently unmergeable and nothing goes red to explain it.
/// `docs/status/enforcement-handoff.md` §1 records this exact hazard.
#[test]
fn every_required_context_names_a_real_ci_job() {
    let required = snapshot_required();
    let mut all_jobs = ci_job_names(".github/workflows/ci.yml");
    all_jobs.extend(ci_job_names(".github/workflows/release.yml"));
    all_jobs.extend(ci_job_names(".github/workflows/book.yml"));

    for ctx in &required {
        assert!(
            all_jobs.contains(ctx),
            "required context `{ctx}` matches no job `name:` in .github/workflows/ \
             (live jobs: {all_jobs:?}). GitHub never reports a status for a context \
             no job produces, so every PR would hang unmergeable rather than fail."
        );
    }
}

// ── LIVE half — skips-with-reason, tripwire-armed ───────────────────────────

fn gh_ruleset() -> Result<serde_json::Value, String> {
    let out = Command::new("gh")
        .args(["api", &format!("orgs/paiml/rulesets/{RULESET_ID}")])
        .output()
        .map_err(|e| format!("`gh` not invocable: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "`gh api orgs/paiml/rulesets/{RULESET_ID}` failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("gh returned non-JSON: {e}"))
}

/// The snapshot is a claim about a live system. Verify it against that system
/// whenever we are authorized to ask.
#[test]
fn live_ruleset_matches_the_committed_snapshot() {
    let live = match gh_ruleset() {
        Ok(v) => v,
        Err(why) => {
            // Anti-vacuity tripwire. Reading an ORG ruleset needs a token with
            // org scope, which Actions' repo-scoped GITHUB_TOKEN does not have —
            // so this test legitimately skips in CI and the STATIC half above is
            // what runs there. The release pre-flight (docs/RELEASE.md) sets
            // XPILE_REQUIRE_RULESET_CHECK=1 to refuse the skip.
            assert!(
                std::env::var_os("XPILE_REQUIRE_RULESET_CHECK").is_none(),
                "XPILE_REQUIRE_RULESET_CHECK is set but the live ruleset could not \
                 be read ({why}). The enforcement claim must not pass vacuously on \
                 a run that demanded a real answer. Authenticate with `gh auth login` \
                 (org read scope) and re-run."
            );
            eprintln!(
                "warning: skipping XPILE-RULESET-DRIFT-001 live check — {why}.\n\
                 The STATIC half (snapshot ⇔ markers ⇔ job names) still ran.\n\
                 To run this half locally: `gh auth login` with org read scope, then\n\
                 `XPILE_REQUIRE_RULESET_CHECK=1 cargo test -p xpile --test ruleset_drift`."
            );
            return;
        }
    };

    let live_required = required_from_ruleset(&live, "live ruleset");
    let snap_required = snapshot_required();
    assert_eq!(
        live_required, snap_required,
        "ENFORCEMENT DRIFT: the live org ruleset {RULESET_ID} requires \
         {live_required:?} but {SNAPSHOT} records {snap_required:?}. This is the \
         2026-07-05 failure recurring — someone changed branch protection and the \
         repo's committed receipt did not follow. Re-derive with \
         `gh api orgs/paiml/rulesets/{RULESET_ID} | jq . > {SNAPSHOT}` and update \
         every `{REQUIRED_MARKER}` marker to match."
    );

    let snapshot: serde_json::Value =
        serde_json::from_str(&read(SNAPSHOT)).expect("snapshot is valid JSON");

    assert_eq!(
        live["enforcement"], snapshot["enforcement"],
        "ruleset enforcement mode drifted: live {:?} vs snapshot {:?}. An `active` \
         ruleset flipped to `evaluate` or `disabled` enforces NOTHING while its \
         required-context list still reads correctly.",
        live["enforcement"], snapshot["enforcement"]
    );

    // `strict` false means a PR may merge on checks run against a stale base —
    // load-bearing for release abort rule A1b, so it is pinned rather than
    // assumed.
    let live_strict = strict_policy(&live);
    let snap_strict = strict_policy(&snapshot);
    assert_eq!(
        live_strict, snap_strict,
        "strict_required_status_checks_policy drifted: live {live_strict} vs \
         snapshot {snap_strict}. With `false`, green checks do not prove the merged \
         combination was ever tested together (release abort rule A1b)."
    );

    // The snapshot going stale-but-plausible is the specific way this failed
    // before: it was regenerated BEFORE the revert, so it looked authoritative
    // and was six hours out of date.
    assert_eq!(
        live["updated_at"], snapshot["updated_at"],
        "the ruleset was edited at {:?} but {SNAPSHOT} was captured at {:?}. The \
         required set still matches, so nothing is broken YET — but a snapshot \
         that lags the API is exactly how the 2026-07-05 revert hid for three \
         weeks. Re-capture it.",
        live["updated_at"], snapshot["updated_at"]
    );
}

fn strict_policy(ruleset: &serde_json::Value) -> bool {
    ruleset["rules"]
        .as_array()
        .and_then(|rules| rules.iter().find(|r| r["type"] == "required_status_checks"))
        .and_then(|r| r["parameters"]["strict_required_status_checks_policy"].as_bool())
        .unwrap_or(false)
}
