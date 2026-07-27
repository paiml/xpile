//! XPILE-PACKAGE-001 (PMAT-1407) — the contract corpus reaches an INSTALLED
//! binary, and the mechanism that gets it there cannot silently regress.
//!
//! THE DEFECT. `xpile diamond` / `quorum` / `attestations` resolve
//! `--contracts-dir` (default `contracts`) relative to the process CWD.
//! Measured on the published 0.1.617 crate: 954 packaged entries, 837 of them
//! test fixtures, and ZERO contract YAMLs — so all three exited 1 with
//! `Error: contracts is not a directory` for everyone who ran the README's
//! own `cargo install xpile`, while working perfectly from a git checkout.
//! That asymmetry is why it survived: the development path never exercises
//! the shipped one.
//!
//! WHY THE OBVIOUS FIX IS THE WRONG ONE, and why test 3 below exists. The
//! natural repair is `include = ["contracts/**/*.yaml", ...]` in
//! `crates/xpile/Cargo.toml`. Executed, it does TWO harmful things and zero
//! useful ones:
//!   (a) it matches NOTHING — `include` globs are rooted at the PACKAGE dir
//!       and `contracts/` lives at the WORKSPACE root, one level up. The
//!       `../../contracts/**/*.yaml` spelling matches nothing either;
//!       `cargo package` does not reach outside the package root.
//!   (b) `include` is AUTHORITATIVE. Adding it dropped the package from 954
//!       entries to 7 — all 837 fixtures gone — and exited 0 while doing it.
//! And even had it worked, packaging alone CANNOT fix the bug: the `.crate`
//! only puts files where the COMPILER can see them, and `cargo install`
//! discards the build directory, so a runtime file read still has nothing to
//! open. The corpus has to be in the BINARY.
//!
//! THE MECHANISM, in three parts, one test each:
//!   1. `crates/xpile/contracts` is a symlink to the canonical workspace-root
//!      `contracts/`, which puts the YAMLs inside the package root without
//!      duplicating a single byte in git. `cargo package` dereferences it
//!      into real regular files (verified: `-rw-r--r--` in the tarball,
//!      byte-identical to canonical).
//!   2. `build.rs` enumerates that directory and emits `include_str!` calls,
//!      so the bytes land in the binary. It enumerates rather than reading a
//!      hand-written list, so adding a contract needs no edit anywhere here.
//!   3. No `include` key, per (b) above.
//!
//! Test 4 is the one that measures the PROPERTY rather than the mechanism:
//! the embedded corpus and the on-disk corpus must produce byte-identical
//! reports. It executes the real binary from a directory that is not a
//! checkout — the exact situation of an installed user — so it fails if the
//! corpus is stale, truncated, or empty, no matter which of the three parts
//! broke.
//!
//! NO COUNT IS HARD-CODED ANYWHERE IN THIS FILE. Both sides are re-derived
//! from the filesystem on every run (PMAT-1396's rule: a prose cardinality a
//! policy is conditioned on is a bomb with a fuse).

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_xpile"))
}

/// Workspace root. An integration test's CWD is the PACKAGE root
/// (`crates/xpile`), not the workspace root.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The canonical corpus: `*.yaml` directly under the workspace-root
/// `contracts/`, sorted by file name. Re-derived, never hard-coded.
fn canonical_contract_files() -> Vec<String> {
    let dir = workspace_root().join("contracts");
    let mut out: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("yaml"))
        .filter_map(|p| p.file_name().and_then(|s| s.to_str()).map(String::from))
        .collect();
    out.sort();
    out
}

/// 1. The symlink that puts the corpus inside the package root exists and
///    points at the canonical directory.
///
/// Without it `build.rs` embeds nothing, and every installed user is back to
/// `Error: contracts is not a directory`.
#[test]
fn xpile_crate_contracts_symlink_resolves_to_the_canonical_corpus() {
    let link = crate_root().join("contracts");
    assert!(
        link.exists(),
        "{} is missing. It is the symlink to the workspace-root `contracts/` that puts \
         the contract corpus inside the xpile PACKAGE root, which is what lets build.rs \
         `include_str!` it into the binary and what lets `cargo package` ship it. \
         Recreate with: ln -s ../../contracts crates/xpile/contracts",
        link.display()
    );

    let resolved = link
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize {}: {e}", link.display()));
    let canonical = workspace_root()
        .join("contracts")
        .canonicalize()
        .expect("canonicalize workspace contracts");
    assert_eq!(
        resolved,
        canonical,
        "{} resolves to {} but the canonical corpus is {}. A COPY here would be a drift \
         bomb — the packaged contracts would silently diverge from the ones `pv lint` and \
         every gate read.",
        link.display(),
        resolved.display(),
        canonical.display()
    );

    // Non-vacuity: a symlink pointing at an EMPTY directory would satisfy
    // everything above and embed nothing.
    let n = canonical_contract_files().len();
    assert!(
        n >= 2,
        "only {n} contract YAML(s) found under the canonical corpus — too few for this \
         gate to mean anything. Every assertion here would pass vacuously on an empty \
         corpus."
    );
}

/// 2. `build.rs` derives the embedded set from the directory instead of a
///    hand-maintained list, and refuses to produce an empty corpus.
///
/// A hand-written list would make "add a contract" a two-place edit, and the
/// second place is the one nobody remembers — the same mechanical root cause
/// PMAT-1345 fixed for roadmap registration.
#[test]
fn build_script_enumerates_the_corpus_and_refuses_an_empty_one() {
    let build_rs = crate_root().join("build.rs");
    let src = std::fs::read_to_string(&build_rs)
        .unwrap_or_else(|e| panic!("read {}: {e}", build_rs.display()));

    assert!(
        src.contains("read_dir"),
        "build.rs must ENUMERATE the contracts directory. A hard-coded list silently \
         stops embedding new contracts."
    );
    assert!(
        src.contains("include_str!"),
        "build.rs must emit `include_str!` calls — packaging the YAMLs into the .crate \
         only makes them visible to the COMPILER. `cargo install` discards the build \
         directory, so a runtime file read still finds nothing; the bytes have to be in \
         the binary."
    );
    assert!(
        src.contains("entries.is_empty()"),
        "build.rs must FAIL on an empty corpus. Embedding nothing would leave the \
         reporters printing `no contract IDs discovered` — a truthful message about \
         entirely the wrong cause."
    );
    assert!(
        src.contains("cargo:rerun-if-changed=contracts"),
        "build.rs must re-run when the corpus changes, or an added contract is embedded \
         only after an unrelated clean rebuild."
    );
}

/// 3. `crates/xpile/Cargo.toml` declares no `include` key.
///
/// This is the assertion that catches the plausible-but-wrong fix. `include`
/// is AUTHORITATIVE: adding one to ship the contracts dropped the package
/// from 954 entries to 7 — every test fixture gone — at exit 0.
#[test]
fn xpile_manifest_declares_no_authoritative_include_key() {
    let manifest = crate_root().join("Cargo.toml");
    let src = std::fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("read {}: {e}", manifest.display()));

    for line in src.lines() {
        let t = line.trim();
        if t.starts_with('#') {
            continue;
        }
        assert!(
            !t.starts_with("include"),
            "crates/xpile/Cargo.toml declares `{t}`. `include` is AUTHORITATIVE — it \
             replaces cargo's default file set, and measured on this crate it silently \
             cut the package from 954 entries to 7, dropping all 837 test fixtures at \
             exit 0. It also cannot reach the workspace-root `contracts/` at all, since \
             include globs are rooted at the package dir. Use the \
             `crates/xpile/contracts -> ../../contracts` symlink instead."
        );
    }
}

/// 4. THE PROPERTY, executed: a binary run OUTSIDE any checkout reports the
///    same corpus as one run inside it.
///
/// This is the user-facing repair. `diamond` is the reporter whose every
/// column derives from contract YAML text alone — no roadmap, no fixtures, no
/// witness dirs — so it is fully correct from the embedded corpus and is the
/// honest thing to assert exits 0. `quorum` and `attestations` additionally
/// read the development ledger and still refuse without a checkout; test 5
/// pins that they refuse for the RIGHT reason.
#[test]
fn diamond_reports_identically_from_an_installed_binary_and_a_checkout() {
    // A directory that is emphatically not an xpile checkout.
    let scratch = std::env::temp_dir().join(format!(
        "xpile-pkg1407-{}-{}",
        std::process::id(),
        "installed"
    ));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("create scratch");
    assert!(
        !scratch.join("contracts").exists(),
        "scratch dir must not contain a contracts/ dir or the fallback is never exercised"
    );

    let from_install = Command::new(bin())
        .arg("diamond")
        .arg("--json")
        .current_dir(&scratch)
        .output()
        .expect("spawn xpile in scratch");
    let from_checkout = Command::new(bin())
        .arg("diamond")
        .arg("--json")
        .current_dir(workspace_root())
        .output()
        .expect("spawn xpile in checkout");

    assert!(
        from_install.status.success(),
        "`xpile diamond` must exit 0 from a directory that is not a checkout — that is \
         the whole point of embedding the corpus, and it is what every `cargo install \
         xpile` user gets. stderr:\n{}",
        String::from_utf8_lossy(&from_install.stderr)
    );
    assert!(
        from_checkout.status.success(),
        "`xpile diamond` regressed inside a checkout. stderr:\n{}",
        String::from_utf8_lossy(&from_checkout.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&from_install.stdout),
        String::from_utf8_lossy(&from_checkout.stdout),
        "the EMBEDDED corpus and the ON-DISK corpus produced different Diamond reports. \
         The embedded copy is stale, truncated, or empty — rebuild so build.rs re-runs, \
         and check the crates/xpile/contracts symlink."
    );

    // Non-vacuity: two identical EMPTY reports would satisfy the equality
    // above. The report keys on `metadata.id`, which is not derivable from
    // the file name in general, so assert on corpus SIZE rather than on
    // per-file names.
    let stdout = String::from_utf8_lossy(&from_install.stdout);
    let rows = stdout.matches("\"id\"").count();
    assert!(
        rows >= canonical_contract_files().len(),
        "the embedded report has {rows} row(s) but the canonical corpus has {} file(s) — \
         the embedded copy is truncated",
        canonical_contract_files().len()
    );

    // The fallback must ANNOUNCE itself. Substituting a different corpus
    // silently is how a report becomes a wrong answer nobody can see.
    let err = String::from_utf8_lossy(&from_install.stderr);
    assert!(
        err.contains("embedded in this binary"),
        "the embedded fallback must say so on stderr; got:\n{err}"
    );
    let _ = std::fs::remove_dir_all(&scratch);
}

/// 5. The reporters that genuinely NEED a checkout refuse for the right
///    reason, and say which evidence is missing.
///
/// Before PMAT-1407 both died on `contracts is not a directory` — blaming a
/// path the user never supplied and implying a broken install rather than a
/// checkout-scoped command. The refusal itself is deliberate and must stay:
/// PMAT-1386 established that scoring an unreadable stratum 0 turns a report
/// into a silent wrong answer (702 roadmap mentions collapsed to 0; 10 of 35
/// contracts fell QUORUM -> PARTIAL, at exit 0).
#[test]
fn ledger_scoped_reporters_refuse_naming_the_real_missing_evidence() {
    let scratch =
        std::env::temp_dir().join(format!("xpile-pkg1407-{}-{}", std::process::id(), "ledger"));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("create scratch");

    for sub in ["quorum", "attestations"] {
        let out = Command::new(bin())
            .arg(sub)
            .current_dir(&scratch)
            .output()
            .unwrap_or_else(|e| panic!("spawn xpile {sub}: {e}"));
        assert!(
            !out.status.success(),
            "`xpile {sub}` must still refuse without a checkout — it tallies strata out \
             of the development ledger, and reporting those as 0 would be a silent wrong \
             answer"
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("roadmap.yaml"),
            "`xpile {sub}` must name the roadmap as the missing evidence, not the \
             contracts dir. Got:\n{err}"
        );
        assert!(
            !err.contains("contracts is not a directory"),
            "`xpile {sub}` still blames the contracts dir — the embedded corpus should \
             have satisfied that read. Got:\n{err}"
        );
    }
    let _ = std::fs::remove_dir_all(&scratch);
}

/// 6. An EXPLICIT `--contracts-dir` that does not exist is still an error.
///
/// The embedded fallback is offered only for the DEFAULT. A user who typed a
/// path wants that path; quietly reporting on a different corpus would be a
/// wrong answer at exit 0 — the shape this whole sprint exists to kill.
#[test]
fn an_explicit_missing_contracts_dir_is_still_an_error() {
    let missing = std::env::temp_dir().join("xpile-pkg1407-definitely-not-here");
    let _ = std::fs::remove_dir_all(&missing);
    let out = Command::new(bin())
        .arg("diamond")
        .arg("--contracts-dir")
        .arg(&missing)
        .output()
        .expect("spawn xpile");
    assert!(
        !out.status.success(),
        "an explicitly-supplied missing --contracts-dir must NOT silently fall back to \
         the embedded corpus"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("is not a directory"),
        "expected the explicit-path error; got:\n{err}"
    );
    assert!(
        !err.contains("embedded in this binary"),
        "the embedded fallback must not engage for an explicit path; got:\n{err}"
    );
}
