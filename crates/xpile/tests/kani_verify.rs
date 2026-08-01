//! Run Kani harnesses against the contract files (PMAT-020 / XPILE-QUORUM-002).
//!
//! PMAT-019 / XPILE-QUORUM-001 shipped the *citation gate* —
//! `crates/xpile/tests/kani_harnesses.rs` validates that every YAML
//! `kani_harness:` field references a real file with a real
//! `#[kani::proof] fn <name>` — but it does NOT actually invoke
//! `cargo kani` to verify the proofs discharge. That makes the
//! Symbolic stratum a *claim* rather than a *fact*. This test
//! closes that gap.
//!
//! Mechanism:
//!   1. Walk every `contracts/kani/*.rs` file.
//!   2. For each, materialise a temp Cargo crate (`Cargo.toml` +
//!      copy of the harness file as `lib.rs`).
//!   3. Run `cargo kani` from the temp dir.
//!   4. Assert the verifier exits with status 0 AND its stdout
//!      contains `VERIFICATION:- SUCCESSFUL`.
//!
//! Skip behaviour + anti-vacuity tripwire (XPILE-RULESET-002 / F9):
//! if `cargo-kani` is missing from PATH the test prints a warning and
//! exits OK — same posture as `assert_rustc_runs` and `diff_exec.rs`'s
//! python3/rustc gates — UNLESS the `XPILE_REQUIRE_KANI` env var is
//! set, in which case a missing `cargo-kani` is a hard PANIC. The
//! `kani` CI job (which is now a REQUIRED status context, see
//! `docs/status/enforcement-handoff.md`) installs kani-verifier AND
//! sets `XPILE_REQUIRE_KANI=1`, so if that job ever loses its kani
//! install the required check goes RED instead of passing vacuously.
//!
//! We deliberately do NOT key the tripwire on `CI=true`: GitHub sets
//! `CI=true` in *every* job, including `workspace-test`, which runs
//! `cargo test --workspace` (this test) WITHOUT kani installed by
//! design. Keying on `CI=true` there would red the required
//! `workspace-test` context and wedge every merge. The env-var
//! tripwire mirrors `XPILE_REQUIRE_WASM_RUNTIME` (XPILE-WITNESS-001).
//!
//! Why a workspace test rather than a `build.rs`:
//!   - Cargo doesn't allow running other Cargo invocations from
//!     `build.rs` reliably; the lock file fights.
//!   - Workspace tests already have the pattern (PMAT-014 deadlines,
//!     PMAT-016 SOTA, PMAT-018 diff_exec — all four prior gates
//!     follow this pattern).
//!   - `cargo test --workspace` is the canonical "run our gates"
//!     incantation; layering Kani under it means contributors run
//!     the symbolic stratum by default if their environment supports
//!     it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn have_cargo_kani() -> bool {
    Command::new("cargo")
        .args(["kani", "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn collect_kani_harness_files(root: &Path) -> Vec<PathBuf> {
    let dir = root.join("contracts").join("kani");
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(p);
        }
    }
    out
}

/// Workspace crates a harness asks to be built against, declared in its own
/// header as `//! kani-deps: xpile-meta-hir, xpile-backend` (PMAT-1512).
///
/// Before this, every temp crate was emitted with NO `[dependencies]` section,
/// so a harness could not reference xpile even in principle and no proof in the
/// repository could be turned red by a wrong lowering. The declaration lives in
/// the harness rather than in a table here so the two cannot drift.
fn declared_deps(harness_src: &str) -> Vec<String> {
    harness_src
        .lines()
        .take_while(|l| l.starts_with("//!") || l.trim().is_empty())
        .filter_map(|l| {
            l.trim_start_matches("//!")
                .trim()
                .strip_prefix("kani-deps:")
        })
        .flat_map(|list| {
            list.split(',')
                .map(|d| d.trim().to_string())
                .filter(|d| !d.is_empty())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn materialise_temp_crate(harness_src: &Path, temp_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(temp_dir).map_err(|e| format!("create temp dir: {e}"))?;
    let crate_name = harness_src
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("non-utf8 harness filename")?;
    let harness_text = fs::read_to_string(harness_src).map_err(|e| format!("read harness: {e}"))?;
    let root = workspace_root();
    let mut deps = String::new();
    for d in declared_deps(&harness_text) {
        let crate_dir = root.join("crates").join(&d);
        if !crate_dir.join("Cargo.toml").is_file() {
            return Err(format!(
                "harness {} declares `kani-deps: {d}` but {} has no Cargo.toml. A proof \
                 that cannot build against the crate it names would silently fall back \
                 to verifying nothing.",
                harness_src.display(),
                crate_dir.display()
            ));
        }
        deps.push_str(&format!("{d} = {{ path = {:?} }}\n", crate_dir.display()));
    }
    let cargo_toml = format!(
        r#"[package]
name = "kani_verify_{crate_name}"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "lib.rs"

[dependencies]
{deps}
[workspace]
"#
    );
    fs::write(temp_dir.join("Cargo.toml"), cargo_toml).map_err(|e| format!("write toml: {e}"))?;
    fs::write(temp_dir.join("lib.rs"), harness_text).map_err(|e| format!("write lib: {e}"))?;
    Ok(())
}

fn run_kani(crate_dir: &Path) -> Result<String, String> {
    // PMAT-151 lesson: a slow harness can hang `cargo kani` for hours
    // (observed 105+ min on a multiplication symbolic state explosion
    // before manual intervention). Cap each invocation at 3 minutes
    // via `-Z unstable-options --harness-timeout 180s` — Kani returns
    // a clear error message and we move on rather than hang the CI
    // job indefinitely. The unstable-options flag is documented in
    // `cargo kani --help` and gated behind the `-Z` Cargo flag.
    let out = Command::new("cargo")
        .arg("kani")
        .arg("-Z")
        .arg("unstable-options")
        .arg("--harness-timeout")
        .arg("180s")
        .current_dir(crate_dir)
        .output()
        .map_err(|e| format!("spawn cargo kani: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        return Err(format!(
            "cargo kani exited non-zero\n=== stdout ===\n{stdout}\n=== stderr ===\n{stderr}"
        ));
    }
    Ok(stdout)
}

// The live CI gate. For every contracts/kani/*.rs harness file, run
// `cargo kani` against a temp crate that mounts it as `lib.rs`. The
// verifier must exit with status 0 and emit a "VERIFICATION:-
// SUCCESSFUL" line — which we grep for to catch silent SAT-checker
// regressions (Kani has historically swallowed errors when the
// solver crashed).
#[test]
fn every_kani_harness_discharges() {
    if !have_cargo_kani() {
        // Anti-vacuity tripwire (XPILE-RULESET-002 / F9): now that `kani`
        // is a REQUIRED CI context, a runner that lost its kani install
        // must FAIL, not skip-green. The `kani` job sets XPILE_REQUIRE_KANI=1
        // after installing kani-verifier; if this test runs there without
        // `cargo kani` on PATH, refuse. See the module docs for why this is
        // NOT keyed on `CI=true` (that would wedge the required workspace-test).
        assert!(
            std::env::var_os("XPILE_REQUIRE_KANI").is_none(),
            "XPILE_REQUIRE_KANI is set but `cargo kani` is not invocable — the \
             required `kani` context must not pass vacuously (F9). The kani CI \
             job must `cargo install --locked kani-verifier` before running this \
             gate. Refusing to skip."
        );
        eprintln!(
            "warning: skipping XPILE-QUORUM-002 — `cargo kani` not on PATH. \
             To run this gate locally:\n  \
             cargo install --locked kani-verifier && cargo kani-setup\n\
             Then re-run `cargo test --workspace`."
        );
        return;
    }

    let root = workspace_root();
    let harnesses = collect_kani_harness_files(&root);
    assert!(
        !harnesses.is_empty(),
        "expected at least one harness under contracts/kani/; \
         PMAT-019 was supposed to plant py_int_arith.rs"
    );

    let base_tmp = std::env::temp_dir().join("xpile-kani-verify");
    let _ = fs::remove_dir_all(&base_tmp);
    fs::create_dir_all(&base_tmp).expect("create base temp dir");

    let mut failures: Vec<(PathBuf, String)> = Vec::new();
    for harness in &harnesses {
        let stem = harness
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("harness");
        let crate_dir = base_tmp.join(stem);
        if let Err(e) = materialise_temp_crate(harness, &crate_dir) {
            failures.push((harness.clone(), format!("materialise: {e}")));
            continue;
        }
        match run_kani(&crate_dir) {
            Ok(stdout) => {
                if !stdout.contains("VERIFICATION:- SUCCESSFUL") {
                    failures.push((
                        harness.clone(),
                        format!(
                            "Kani exited 0 but stdout lacks `VERIFICATION:- SUCCESSFUL`. Output:\n{stdout}"
                        ),
                    ));
                }
            }
            Err(e) => failures.push((harness.clone(), e)),
        }
    }

    if !failures.is_empty() {
        let mut msg = format!(
            "Kani verification failed on {} harness file(s):\n\n",
            failures.len()
        );
        for (path, err) in &failures {
            msg.push_str(&format!("--- {} ---\n{err}\n\n", path.display()));
        }
        panic!("{msg}");
    }

    eprintln!(
        "XPILE-QUORUM-002: verified {} Kani harness file(s) — Symbolic stratum discharged.",
        harnesses.len()
    );
}

// Anti-vacuity self-test (XPILE-RULESET-002 / F9), mirroring
// XPILE-WITNESS-001's `required_runtime_is_present_when_declared`. When a
// runner DECLARES kani is required (`XPILE_REQUIRE_KANI` set — done by the
// `kani` CI job), `cargo kani` must actually be invocable; otherwise the
// required `kani` context would pass vacuously. Fast: does not run any
// harness. No-op locally / in `workspace-test` where the var is unset.
#[test]
fn required_kani_is_present_when_declared() {
    if std::env::var_os("XPILE_REQUIRE_KANI").is_some() {
        assert!(
            have_cargo_kani(),
            "XPILE_REQUIRE_KANI is set but `cargo kani` is not invocable — the \
             required kani CI context would pass vacuously (F9). Install \
             kani-verifier in the job that sets XPILE_REQUIRE_KANI."
        );
    }
}

// Self-test: harness collection logic finds the expected file. If a
// future refactor moves contracts/kani/ this test fires before the
// quieter integration test below noticed nothing was checked.
#[test]
fn collect_finds_at_least_the_py_int_arith_harness() {
    let root = workspace_root();
    let harnesses = collect_kani_harness_files(&root);
    let names: Vec<_> = harnesses
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert!(
        names.iter().any(|n| n == "py_int_arith.rs"),
        "expected `py_int_arith.rs` under contracts/kani/, found: {names:?}"
    );
}
