//! XPILE-CI-INSTALL-001 — the toolchain-install honesty gate (PMAT-1370).
//!
//! Sibling of `ruleset_drift.rs` (what actually blocks a merge) and
//! `claims_drift.rs` (what the docs claim). Those two pin what the repo *says*.
//! This one pins something narrower and meaner: that a CI job which claims to
//! have installed a tool **actually has it**, and that a failure to install
//! reds where the installer ran rather than somewhere downstream.
//!
//! ## The failure this exists to catch (it happened, on the release SHA)
//!
//! The `wasi` job installed its WASI runtime with
//!
//! ```text
//! curl -fsSL https://wasmtime.dev/install.sh | bash
//! echo "$HOME/.wasmtime/bin" >> "$GITHUB_PATH"
//! ```
//!
//! That upstream script resolves "latest" in `get_latest_release()` by scraping
//! `api.github.com/repos/bytecodealliance/wasmtime/releases/latest` with an
//! **unauthenticated** request. Actions runners share egress IPs, so that call
//! is rate-limited in bursts. The rate-limit body carries no `tag_name`, so the
//! installer's `sed 's/.*tag_name": *"//' | sed 's/".*//'` pipeline degrades the
//! whole JSON document to its first character — the literal `{`. The script then
//! prints
//!
//! ```text
//!   Installing latest version of Wasmtime ({)
//! Error: Could not download Wasmtime version '{'
//! ```
//!
//! **and exits 0.** The install step went green with nothing installed. The job
//! failed one step later with `wasmtime: command not found` (exit 127) — an
//! error that names neither the real cause nor the installer, and that reads
//! like a `$GITHUB_PATH` typo.
//!
//! Observed live: `wasi` FAILURE on `ccb95a04` (the SHA tagged `v0.1.617`) while
//! the same job was SUCCESS on `ef53c281` roughly an hour earlier, with no
//! change to the job in between. That makes it **flaky, not broken**, which is
//! strictly worse — a green run proves nothing about the next one, and the
//! failure is indistinguishable from an unrelated infrastructure hiccup.
//!
//! ## The two properties enforced here
//!
//! 1. **Verify in-step.** Any step that appends to `$GITHUB_PATH` must invoke
//!    the tool it just installed, *in that same step*, by absolute path. In-step
//!    matters twice over: `$GITHUB_PATH` only takes effect in SUBSEQUENT steps
//!    (so a bare `tool --version` in the install step would not even find it),
//!    and a downstream verification would attribute the failure to the wrong
//!    step. This converts "installer printed an error and exited 0" from a
//!    silent green into a red that names the tool.
//!
//! 2. **Pin the artifact.** No workflow may bootstrap a tool through a script
//!    that resolves its own version at run time, and no workflow may fetch
//!    `releases/latest`. Pinned release-asset downloads are not API-rate-limited
//!    and are reproducible; "latest" is neither.
//!
//! Both are static: `std::fs` only, no network, no `gh`, no runner. This test
//! cannot skip, so it holds in CI, offline, and inside an extracted `.crate`.
//!
//! **Not in scope:** whether the pinned version is current. A stale pin is a
//! visible, boring problem; an unpinned fetch is an invisible, flaky one.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Minimum number of `$GITHUB_PATH` install steps that must exist for the first
/// assertion to mean anything. Five exist today (elan ×2, pmat, wasmtime,
/// mdbook); the floor is set below that so ordinary churn doesn't trip it, but
/// above zero so deleting every install step cannot green this gate vacuously.
const MIN_INSTALL_STEPS: usize = 4;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn workflow_files() -> Vec<PathBuf> {
    let dir = workspace_root().join(".github/workflows");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "yml" || x == "yaml"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "XPILE-CI-INSTALL-001: no workflow files under .github/workflows/ — \
         the gate would be vacuous"
    );
    files
}

/// One `- name:`/`- uses:` step, as raw lines.
struct Step {
    file: String,
    name: String,
    body: String,
}

/// True for a line that opens a new step, e.g. `      - name: Install pmat`.
///
/// Deliberately not a YAML parse: pulling a YAML crate in to read four
/// workflows would make this gate depend on a parser's opinion about anchors
/// and block scalars. The shape being matched is a list item under `steps:`,
/// which is unambiguous at the line level.
fn is_step_start(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("- name:") || t.starts_with("- uses:")
}

fn steps_of(path: &Path) -> Vec<Step> {
    let file = path
        .strip_prefix(workspace_root())
        .unwrap_or(path)
        .display()
        .to_string();
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {file}: {e}"));

    let mut steps: Vec<Step> = Vec::new();
    for line in text.lines() {
        if is_step_start(line) {
            let name = line
                .trim_start()
                .strip_prefix("- name:")
                .map(|n| n.trim().to_string())
                .unwrap_or_default();
            steps.push(Step {
                file: file.clone(),
                name,
                body: String::new(),
            });
        }
        if let Some(step) = steps.last_mut() {
            step.body.push_str(line);
            step.body.push('\n');
        }
    }
    steps
}

fn path_install_steps() -> Vec<Step> {
    workflow_files()
        .iter()
        .flat_map(|p| steps_of(p))
        .filter(|s| s.body.contains("GITHUB_PATH"))
        .collect()
}

/// `Install wasmtime (pinned prebuilt binary)` -> `wasmtime`.
fn installed_tool(step: &Step) -> String {
    let rest = step.name.strip_prefix("Install ").unwrap_or_else(|| {
        panic!(
            "XPILE-CI-INSTALL-001: {} step {:?} appends to $GITHUB_PATH but its name \
             does not start with `Install ` — the gate identifies the installed tool \
             from the step name, so name it `Install <tool> [(note)]`.",
            step.file, step.name
        )
    });
    rest.split_whitespace()
        .next()
        .expect("step name has a tool token")
        .to_string()
}

/// Every `$GITHUB_PATH` install step must run the tool it installed, in-step.
#[test]
fn every_path_install_verifies_the_binary_in_step() {
    let steps = path_install_steps();
    assert!(
        steps.len() >= MIN_INSTALL_STEPS,
        "XPILE-CI-INSTALL-001 is vacuous: found {} $GITHUB_PATH install step(s), \
         floor is {MIN_INSTALL_STEPS}. If installs were genuinely removed, lower the \
         floor deliberately; do not let the gate pass on an empty set.",
        steps.len()
    );

    let mut verified: BTreeSet<String> = BTreeSet::new();
    for step in &steps {
        let tool = installed_tool(step);
        let ok = step
            .body
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .any(|l| l.contains(&tool) && l.contains("--version"));
        assert!(
            ok,
            "XPILE-CI-INSTALL-001: {} step {:?} puts `{tool}` on $GITHUB_PATH but never \
             invokes it in that step.\n\n\
             Add a verification line INSIDE the step, by absolute path:\n\
             \x20   \"$HOME/.../{tool}\" --version\n\n\
             Why in-step and why absolute: $GITHUB_PATH only takes effect in SUBSEQUENT \
             steps, so a bare `{tool} --version` here would not resolve; and a downstream \
             check blames the wrong step. This is the PMAT-1370 shape — the wasmtime \
             installer printed an error, exited 0, and the job died a step later on a bare \
             `command not found` that named neither the installer nor the cause.",
            step.file, step.name
        );
        verified.insert(tool);
    }
    eprintln!(
        "XPILE-CI-INSTALL-001: {} $GITHUB_PATH install step(s) verified in-step: {}",
        steps.len(),
        verified.into_iter().collect::<Vec<_>>().join(", ")
    );
}

/// Every `curl` inside such a step must use `-f`, so an HTTP error is curl's
/// exit code rather than an error page piped into `tar`/`sh`.
#[test]
fn every_path_install_curl_fails_on_http_error() {
    for step in path_install_steps() {
        for line in step.body.lines() {
            let t = line.trim_start();
            if t.starts_with('#') || !t.contains("curl ") {
                continue;
            }
            let flags: Vec<&str> = t
                .split_whitespace()
                .filter(|w| w.starts_with('-') && !w.starts_with("--"))
                .collect();
            assert!(
                flags.iter().any(|f| f.contains('f')),
                "XPILE-CI-INSTALL-001: {} step {:?} curls without `-f`:\n  {t}\n\n\
                 Without `-f`, curl exits 0 on an HTTP 404/429/5xx and pipes the error \
                 BODY into the next stage. `-f` makes the HTTP status the exit status.",
                step.file,
                step.name
            );
        }
    }
}

/// No workflow may resolve a tool version at run time.
#[test]
fn no_workflow_resolves_a_tool_version_at_run_time() {
    // (needle, why it is banned)
    const BANNED: &[(&str, &str)] = &[
        (
            "wasmtime.dev/install.sh",
            "the wasmtime bootstrap script scrapes the UNAUTHENTICATED GitHub releases \
             API for its version; when that call is rate-limited on a shared runner IP it \
             resolves the version to the literal `{`, prints an error, and EXITS 0 \
             (PMAT-1370, observed red on the v0.1.617 release SHA). Download the pinned \
             release asset instead — asset downloads are not API-rate-limited.",
        ),
        (
            "releases/latest",
            "`releases/latest` is an unpinned, rate-limited API read. Pin the version in \
             an `env:` block and interpolate it into the asset URL, as the `docs` job does \
             with PMAT_VERSION and the `wasi` job does with WASMTIME_VERSION.",
        ),
    ];

    for path in workflow_files() {
        let rel = path
            .strip_prefix(workspace_root())
            .unwrap_or(&path)
            .display()
            .to_string();
        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for (line_no, line) in text.lines().enumerate() {
            if line.trim_start().starts_with('#') {
                continue; // the ban is on what RUNS, not on prose explaining it
            }
            for (needle, why) in BANNED {
                assert!(
                    !line.contains(needle),
                    "XPILE-CI-INSTALL-001: {rel}:{} uses `{needle}`.\n\n{why}",
                    line_no + 1
                );
            }
        }
    }
}

/// Regression pin for the exact `wasi` job shape, so the fix cannot be reverted
/// to an unpinned bootstrap without reddening a named test.
#[test]
fn the_wasi_job_pins_its_wasmtime_version() {
    let text =
        fs::read_to_string(workspace_root().join(".github/workflows/ci.yml")).expect("read ci.yml");
    let live: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect();

    let pin = live
        .iter()
        .find(|l| l.trim_start().starts_with("WASMTIME_VERSION:"))
        .unwrap_or_else(|| {
            panic!(
                "XPILE-CI-INSTALL-001: ci.yml has no `WASMTIME_VERSION:` pin. The wasi \
                 job must pin its runtime in an `env:` block (PMAT-1370)."
            )
        });
    let version = pin
        .split_once(':')
        .map(|(_, v)| v.trim())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| panic!("XPILE-CI-INSTALL-001: WASMTIME_VERSION pin is empty"));
    assert!(
        version.starts_with('v') && version[1..].starts_with(|c: char| c.is_ascii_digit()),
        "XPILE-CI-INSTALL-001: WASMTIME_VERSION is {version:?}; expected a concrete tag \
         like `v47.0.2`. A non-literal pin re-opens the run-time-resolution hole."
    );

    assert!(
        live.iter().any(|l| {
            l.contains("bytecodealliance/wasmtime/releases/download/")
                && l.contains("${WASMTIME_VERSION}")
        }),
        "XPILE-CI-INSTALL-001: the wasi job pins WASMTIME_VERSION but no download URL \
         interpolates it — the pin would be decorative."
    );
    eprintln!("XPILE-CI-INSTALL-001: wasi job pins wasmtime {version}");
}
