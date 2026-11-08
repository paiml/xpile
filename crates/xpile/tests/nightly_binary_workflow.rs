//! XPILE-NIGHTLY-BIN-001 (xpile#2149) — the nightly binary workflow keeps its
//! three promises. Static (std::fs only), never skips:
//! 1. it runs only on `schedule` / `workflow_dispatch` (LAB, cannot block);
//! 2. it builds only when main moved (SHA stamp compared) and CI is green on
//!    that exact SHA (both required contexts checked);
//! 3. it publishes a sha256 and stamps the release with the SHA, and the
//!    publish step is gated on the decide step, so a red night publishes none;
//! 4. a red night files the night issue as the LAST step.

use std::fs;
use std::path::PathBuf;

fn wf() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/nightly.yml");
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn must(t: &str, needle: &str, why: &str) {
    assert!(t.contains(needle), "nightly.yml lacks `{needle}`: {why}");
}

#[test]
fn triggers_are_night_only() {
    let t = wf();
    must(&t, "schedule:", "a nightly needs a schedule");
    must(&t, "workflow_dispatch:", "manual run");
    for bad in ["pull_request", "merge_group", "\n  push:"] {
        assert!(!t.contains(bad), "nightly.yml must not trigger on {bad:?}");
    }
}

#[test]
fn builds_only_when_main_moved_and_ci_green() {
    let t = wf();
    must(
        &t,
        "built-from: ",
        "SHA stamp read back from the last nightly",
    );
    must(&t, "name==\"gate\"", "gate context checked on the SHA");
    must(
        &t,
        "name==\"workspace-test\"",
        "workspace-test context checked on the SHA",
    );
    let publish = t
        .split("publish the nightly prerelease")
        .nth(1)
        .expect("publish step");
    assert!(
        publish.contains("steps.decide.outputs.build == 'true'"),
        "publish must be gated on the decide step"
    );
    must(&t, "--locked", "build from the lockfile");
}

#[test]
fn publishes_sha256_and_stamps_sha() {
    let t = wf();
    must(&t, "sha256sum", "a sha256 beside the binary");
    must(&t, "--prerelease", "the nightly is a prerelease");
    must(
        &t,
        "--target \"$SHA\"",
        "release stamped with the built SHA",
    );
}

#[test]
fn no_continue_on_error_and_issue_step_is_last() {
    let t = wf();
    assert!(
        !t.contains("continue-on-error"),
        "a night step must be able to fail"
    );
    let last = t.rsplit("      - name:").next().unwrap();
    assert!(
        last.contains("night-issue"),
        "last step must be the night-issue action"
    );
    assert!(last.contains("failure() && github.event_name == 'schedule'"));
    assert!(last.contains("check: nightly"));
}
