//! XPILE-RULESET-DRIFT-001 — the enforcement-truth gate (PMAT-1347).
//!
//! Sibling of `claims_drift.rs` (derived DOC counts) and
//! `roadmap_registration.rs` (ledger registration). Where those pin what the
//! repo says about ITSELF, this one pins what the repo says about **what
//! actually blocks a merge** — the claim that gives every other gate its
//! meaning. A gate nobody is required to pass is a suggestion.
//!
//! ## The drift this exists to catch (it has now happened three times)
//!
//! **2026-07-27 — the SPLIT, and this gate's own false alarm (PMAT-1475).**
//! Enforcement stopped being the property of one ruleset. The org moved
//! `workspace-test` OUT of `13878864` and into a new dedicated ruleset
//! `19814559` ("workspace-test — repos that emit it (aprender, rmedia,
//! xpile)"), created `2026-07-27T13:48:24`. The effective required set for
//! `refs/heads/main` never changed: it was `{gate, workspace-test}` before the
//! split and it is `{gate, workspace-test}` after it.
//!
//! The split was also *correct*, which is the part that should have prevented
//! the misreading. `13878864` is scoped `repository_name: ["~ALL"]` — 244 org
//! repos — and most of them emit no `workspace-test` job. A required context no
//! job produces hangs every PR on "Expected — Waiting for status to be
//! reported" forever, which is precisely the hazard
//! `every_required_context_names_a_real_ci_job` below exists to catch. So the
//! org was *repairing a deadlock*, and this repo read it as an attack on its
//! own enforcement. **Ask what the change would cost if it were what you think
//! it is:** requiring a job across 244 repos that cannot run it is obviously
//! broken, so that was probably not what happened.
//!
//! This gate read `orgs/paiml/rulesets/13878864` and reported a WEAKENING. It
//! was the only failing test in the workspace for two days; it was recorded as
//! the blocker for the v0.1.618 tag cut; and it was escalated as an OWNER
//! DECISION ("re-deriving the snapshot ratifies the weakening") about a
//! weakening that never happened. Downstream, three documents — including the
//! packaged `contracts/README.md` that ships to crates.io, and a `[Unreleased]`
//! CHANGELOG entry — were *edited away from the truth* to agree with it.
//!
//! The defect was not the reading. It was the SUBJECT. The gate asked "what
//! blocks a merge on `main`?" and measured "what does ruleset 13878864
//! contain?" Those are the same number only until someone adds a second
//! ruleset. **The authoritative endpoint is `repos/paiml/xpile/rules/branches/
//! main`** — the aggregation of every active ruleset that applies to the
//! branch — and this gate now reads that, keeping the per-ruleset reads only
//! for metadata the aggregate does not carry. A required context that MOVES
//! between rulesets is now visibly a move; a context that DISAPPEARS is still
//! visibly a weakening; and the two can no longer be confused.
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
//! * **LIVE half** (2 tests). The first reads the EFFECTIVE branch endpoint
//!   `gh api repos/paiml/xpile/rules/branches/main` and compares both the
//!   union (what blocks a merge) and the per-ruleset attribution (which
//!   ruleset supplies each context) against the committed snapshots. That
//!   endpoint is REPO-scoped, so unlike the org read it is answerable by
//!   Actions' default `GITHUB_TOKEN`. The second keeps the per-ruleset ORG
//!   read for the three properties the aggregate omits — `enforcement`,
//!   `strict_required_status_checks_policy` and `updated_at`. Both
//!   skip-with-reason when `gh` is absent or unauthorized, with an
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

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// The branch whose *effective* protection is the subject of this gate. Not a
/// ruleset id — the 2026-07-27 split is exactly what happens when a gate lets a
/// ruleset id stand in for the branch it protects.
const BRANCH_RULES_ENDPOINT: &str = "repos/paiml/xpile/rules/branches/main";
const SNAPSHOT_DIR: &str = "docs/status";
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

/// Every committed ruleset receipt, keyed by ruleset id, DISCOVERED from
/// `docs/status/ruleset-*.json` rather than listed.
///
/// Discovery is the point. A hard-coded id is precisely what let the 2026-07-27
/// split read as a weakening: adding `ruleset-19814559.json` has to be enough to
/// teach every assertion below about a new source of enforcement, because the
/// person who adds it is not going to find the other call sites.
fn snapshot_rulesets() -> BTreeMap<String, serde_json::Value> {
    let dir = workspace_root().join(SNAPSHOT_DIR);
    let mut out = BTreeMap::new();
    for entry in fs::read_dir(&dir).expect("docs/status/ exists").flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(id) = name
            .strip_prefix("ruleset-")
            .and_then(|r| r.strip_suffix(".json"))
        else {
            continue;
        };
        let rel = format!("{SNAPSHOT_DIR}/{name}");
        let json: serde_json::Value = serde_json::from_str(&read(&rel))
            .unwrap_or_else(|e| panic!("{rel}: invalid JSON: {e}"));
        // The filename is used as the id everywhere below; if the payload
        // disagrees, every per-ruleset assertion is comparing two different
        // rulesets while looking correct.
        assert_eq!(
            json["id"].as_i64().map(|i| i.to_string()).as_deref(),
            Some(id),
            "{rel}: filename says ruleset {id} but the payload's `id` is {:?}",
            json["id"]
        );
        out.insert(id.to_string(), json);
    }
    assert!(
        !out.is_empty(),
        "no `{SNAPSHOT_DIR}/ruleset-*.json` receipts found — this gate would \
         assert nothing about enforcement at all"
    );
    out
}

/// A human-readable list of the receipts, for assertion messages.
fn snapshot_names() -> String {
    snapshot_rulesets()
        .keys()
        .map(|id| format!("{SNAPSHOT_DIR}/ruleset-{id}.json"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// What each committed receipt requires, keyed by ruleset id.
fn snapshot_required_by_ruleset() -> BTreeMap<String, BTreeSet<String>> {
    snapshot_rulesets()
        .into_iter()
        .map(|(id, json)| {
            let whence = format!("{SNAPSHOT_DIR}/ruleset-{id}.json");
            (id, required_from_ruleset(&json, &whence))
        })
        .collect()
}

/// The contexts that block a merge on `main`, per the committed receipts — the
/// UNION across every ruleset that applies, not the contents of any one of them.
fn snapshot_required() -> BTreeSet<String> {
    snapshot_required_by_ruleset()
        .into_values()
        .flatten()
        .collect()
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
        "the committed receipts ({}) record ZERO required contexts — nothing \
         would block a merge",
        snapshot_names()
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
                found,
                expected,
                "enforcement drift in {rel}: its `{REQUIRED_MARKER}` marker lists \
                 {found:?} but the committed receipts ({}) record {expected:?} in \
                 total. Exactly one of the two is a lie about what blocks a merge. \
                 Confirm the truth with `gh api {BRANCH_RULES_ENDPOINT}` FIRST — \
                 that is the union over every ruleset protecting the branch, and \
                 the only thing a marker is allowed to describe.",
                snapshot_names()
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

fn gh_api(path: &str) -> Result<serde_json::Value, String> {
    let out = Command::new("gh")
        .args(["api", path])
        .output()
        .map_err(|e| format!("`gh` not invocable: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "`gh api {path}` failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("gh returned non-JSON: {e}"))
}

/// Honour the anti-vacuity tripwire, then skip with a reason.
fn skip_or_fail(why: &str, half: &str) {
    assert!(
        std::env::var_os("XPILE_REQUIRE_RULESET_CHECK").is_none(),
        "XPILE_REQUIRE_RULESET_CHECK is set but the {half} could not be read \
         ({why}). The enforcement claim must not pass vacuously on a run that \
         demanded a real answer. Authenticate with `gh auth login` and re-run."
    );
    eprintln!(
        "warning: skipping XPILE-RULESET-DRIFT-001 {half} — {why}.\n\
         The STATIC half (snapshots ⇔ markers ⇔ job names) still ran.\n\
         To run this half locally: `gh auth login`, then\n\
         `XPILE_REQUIRE_RULESET_CHECK=1 cargo test -p xpile --test ruleset_drift`."
    );
}

/// The contexts GitHub will actually enforce on `main`, keyed by the ruleset
/// that supplies each — read from the branch-rules aggregate.
fn live_required_by_ruleset(rules: &serde_json::Value) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for rule in rules
        .as_array()
        .expect("branch rules endpoint returns an array")
        .iter()
        .filter(|r| r["type"] == "required_status_checks")
    {
        let id = rule["ruleset_id"]
            .as_i64()
            .expect("every applied rule names its ruleset_id")
            .to_string();
        let contexts = rule["parameters"]["required_status_checks"]
            .as_array()
            .expect("required_status_checks is an array")
            .iter()
            .map(|c| {
                c["context"]
                    .as_str()
                    .expect("context is a string")
                    .to_string()
            });
        out.entry(id).or_default().extend(contexts);
    }
    out
}

/// **The authoritative check.** What blocks a merge on `main` is the union over
/// every active ruleset that applies to the branch — never the contents of one
/// ruleset, which is the mistake that made this gate cry wolf for two days
/// (PMAT-1475).
#[test]
fn live_effective_enforcement_matches_the_committed_snapshots() {
    let rules = match gh_api(BRANCH_RULES_ENDPOINT) {
        Ok(v) => v,
        Err(why) => return skip_or_fail(&why, "effective-enforcement half"),
    };

    let live_by_ruleset = live_required_by_ruleset(&rules);
    let snap_by_ruleset = snapshot_required_by_ruleset();

    let live_union: BTreeSet<String> = live_by_ruleset.values().flatten().cloned().collect();
    let snap_union = snapshot_required();

    // Union first: this is the sentence every document in the repo repeats, and
    // it is the one a reader acts on.
    assert_eq!(
        live_union,
        snap_union,
        "ENFORCEMENT DRIFT: `gh api {BRANCH_RULES_ENDPOINT}` says a merge to main \
         is blocked by {live_union:?}, but the committed receipts ({}) record \
         {snap_union:?}. Something genuinely changed about what blocks a merge. \
         Re-derive EVERY receipt, and update the `{REQUIRED_MARKER}` markers.",
        snapshot_names()
    );

    // Attribution second: the union can be right while the receipts describe the
    // wrong rulesets — which is exactly the 2026-07-27 state, and is how a gate
    // keyed to one id reports a weakening that did not happen.
    assert_eq!(
        live_by_ruleset, snap_by_ruleset,
        "RULESET ATTRIBUTION DRIFT: main is protected by {live_by_ruleset:?} but \
         the committed receipts record {snap_by_ruleset:?}. NOTE THE UNION ABOVE \
         AGREED, so the set of merge-blocking contexts did NOT change — a context \
         MOVED between rulesets (or a ruleset was added/removed). This is a SPLIT, \
         not a weakening: do not \"fix\" it by editing documents to claim less \
         enforcement. Add or re-derive the matching \
         `{SNAPSHOT_DIR}/ruleset-<id>.json` receipt and leave the prose alone."
    );

    // Non-vacuity: an endpoint that returned no status-check rules at all would
    // satisfy neither assertion by agreeing with an empty snapshot only if the
    // receipts were ALSO empty — which snapshot_rulesets() already refuses.
    assert!(
        !live_union.is_empty(),
        "the live branch-rules endpoint reports NO required status checks for \
         main — nothing blocks a merge"
    );
}

/// Per-ruleset metadata the branch aggregate does not carry: whether the ruleset
/// is `active` at all, whether `strict` is set, and when it last moved.
#[test]
fn live_ruleset_metadata_matches_the_committed_snapshots() {
    for (id, snapshot) in snapshot_rulesets() {
        let rel = format!("{SNAPSHOT_DIR}/ruleset-{id}.json");
        let live = match gh_api(&format!("orgs/paiml/rulesets/{id}")) {
            Ok(v) => v,
            // Reading an ORG ruleset needs org scope, which Actions'
            // repo-scoped GITHUB_TOKEN does not have.
            Err(why) => return skip_or_fail(&why, "ruleset-metadata half"),
        };

        assert_eq!(
            live["enforcement"], snapshot["enforcement"],
            "ruleset {id} enforcement mode drifted: live {:?} vs {rel} {:?}. An \
             `active` ruleset flipped to `evaluate` or `disabled` enforces \
             NOTHING while its required-context list still reads correctly.",
            live["enforcement"], snapshot["enforcement"]
        );

        // `strict` false means a PR may merge on checks run against a stale base —
        // load-bearing for release abort rule A1b, so it is pinned rather than
        // assumed.
        let live_strict = strict_policy(&live);
        let snap_strict = strict_policy(&snapshot);
        assert_eq!(
            live_strict, snap_strict,
            "ruleset {id}: strict_required_status_checks_policy drifted, live \
             {live_strict} vs {rel} {snap_strict}. With `false`, green checks do \
             not prove the merged combination was ever tested together (release \
             abort rule A1b)."
        );

        // The snapshot going stale-but-plausible is the specific way this failed
        // before: it was regenerated BEFORE the revert, so it looked authoritative
        // and was six hours out of date.
        assert_eq!(
            live["updated_at"], snapshot["updated_at"],
            "ruleset {id} was edited at {:?} but {rel} was captured at {:?}. The \
             required set still matches, so nothing is broken YET — but a snapshot \
             that lags the API is exactly how the 2026-07-05 revert hid for three \
             weeks. Re-capture it.",
            live["updated_at"], snapshot["updated_at"]
        );
    }
}

fn strict_policy(ruleset: &serde_json::Value) -> bool {
    ruleset["rules"]
        .as_array()
        .and_then(|rules| rules.iter().find(|r| r["type"] == "required_status_checks"))
        .and_then(|r| r["parameters"]["strict_required_status_checks_policy"].as_bool())
        .unwrap_or(false)
}
