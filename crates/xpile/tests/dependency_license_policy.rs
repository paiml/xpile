//! XPILE-LICENSE-001 — the dependency-licence disclosure gate (PMAT-1409).
//!
//! Sibling of `ci_tool_install.rs` (a job that claims to have installed a tool
//! actually has it) and `ruleset_drift.rs` (what actually blocks a merge).
//! This one pins a third shape of the same failure: **configuration that is
//! never executed.**
//!
//! ## The failure this exists to catch
//!
//! `deny.toml` has configured `[licenses]` and `[bans]` since the file was
//! written. `.github/workflows/ci.yml` ran exactly one check kind —
//! `cargo deny check advisories` — so `cargo deny check licenses` had **never
//! executed in this repository's history**. Reading `deny.toml` alone, a
//! reviewer would conclude the licence policy was enforced. It was not
//! enforced, not violated-and-accepted, not waived: it was *unknown*.
//!
//! Its first run exits 4. Rejections split three ways — LGPL-3.0-only
//! (copyleft) reached as a NORMAL dependency of the shipped `xpile` binary via
//! `rustpython-parser -> depyler-frontend -> xpile-core`, plus permissive
//! CC0-1.0 and Zlib crates, one of which is build-script-only. Which of those
//! sentences is true of which crate is the entire point, and it is exactly the
//! kind of claim that rots into prose. So it is enumerated in `NOTICE.md` and
//! re-derived here.
//!
//! ## Stratified, for the same reason `ruleset_drift.rs` is
//!
//! **STATIC half** — `std::fs` only. Cannot skip, so it holds in CI, offline,
//! and inside an extracted `.crate`:
//!
//! 1. Every check kind configured in `deny.toml` is executed by *some* CI job.
//!    This is the root-cause assertion: it reds at the commit that adds a
//!    `[sources]` section without wiring it up, not three weeks later.
//! 2. `NOTICE.md`'s disclosure table is well-formed, every row carries a
//!    linkage classification from a closed set, and every row's licence is
//!    genuinely outside `deny.toml`'s allow-list (a row for an allowed licence
//!    would be padding that inflates the non-vacuity floor).
//! 3. The LGPL rows say `binary`. The legally load-bearing sentence in
//!    `NOTICE.md` is "this copyleft library is linked into what we ship"; a
//!    later edit that quietly downgrades it to `dev-only` reds here.
//! 4. The `license-scan` job exists, runs the check, and its *tripwire* step is
//!    not `continue-on-error`. Its raw-report step deliberately is — see the
//!    job comment — and a gate that let the tripwire go the same way would be
//!    decorative.
//!
//! **LIVE half** — needs `cargo-deny` and `cargo`. Skips with a printed reason
//! when they are absent; `XPILE_REQUIRE_DENY=1` (set on the `license-scan` job)
//! turns the skip into a hard failure, so deleting the install step reds
//! instead of silently returning to skip-green:
//!
//! 5. The set of `(crate, version, licence)` triples `cargo deny check
//!    licenses` rejects **equals** the set `NOTICE.md` enumerates. Two-way: a
//!    new rejection reds, and so does a documented crate that has left the
//!    graph. `NOTICE.md` cannot rot in either direction.
//! 6. Every `linkage:` claim is re-derived from `cargo tree -e normal`. The
//!    discriminator is real: a crate that appears under `-e normal` with
//!    `xpile` at the root is in the artifact users install; one that appears
//!    only under `-e normal,build` ran at build time and is not linked.
//!
//! **Not in scope:** whether the LGPL should be there at all. That is the
//! `lgpl-in-shipped-binary` owner decision in `docs/roadmaps/queue.yaml`. This
//! gate makes the facts undeniable; it does not decide them.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const NOTICE: &str = "NOTICE.md";
const DENY: &str = "deny.toml";
const CI: &str = ".github/workflows/ci.yml";
const BEGIN: &str = "<!-- XPILE-LICENSE-DISCLOSURE-BEGIN -->";
const END: &str = "<!-- XPILE-LICENSE-DISCLOSURE-END -->";

/// The check kinds `cargo deny` knows. A `deny.toml` section outside this set
/// (`[graph]`, `[output]`) configures *how* checks run, not *which*.
const CHECK_KINDS: [&str; 4] = ["advisories", "bans", "licenses", "sources"];

/// Legal values of the `linkage` column, ordered by how much they matter.
const LINKAGES: [&str; 3] = ["binary", "build-only", "dev-only"];

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

// ── parsing ────────────────────────────────────────────────────────────────

/// Top-level `[section]` headers of `deny.toml` that name a check kind.
fn configured_check_kinds() -> BTreeSet<String> {
    read(DENY)
        .lines()
        .map(str::trim_end)
        .filter_map(|l| l.strip_prefix('[').and_then(|l| l.strip_suffix(']')))
        .filter(|s| CHECK_KINDS.contains(s))
        .map(str::to_string)
        .collect()
}

/// Every check kind named on a `cargo deny check …` command line in a
/// workflow, mapped to the workflow lines that run it.
fn executed_check_kinds() -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in read(CI).lines() {
        let Some((_, rest)) = line.split_once("cargo deny check ") else {
            continue;
        };
        for word in rest.split_whitespace() {
            if CHECK_KINDS.contains(&word) {
                out.entry(word.to_string())
                    .or_default()
                    .push(line.trim().to_string());
            }
        }
    }
    out
}

/// `[licenses] allow = [...]` — the SPDX ids that are policy-clean, so a
/// disclosure row naming one of them would be noise.
fn allowed_licenses() -> BTreeSet<String> {
    let text = read(DENY);
    let after = text
        .split_once("[licenses]")
        .map(|(_, rest)| rest)
        .unwrap_or("");
    let block = after
        .split_once("allow = [")
        .map(|(_, rest)| rest.split_once(']').map(|(inner, _)| inner).unwrap_or(""))
        .unwrap_or("");
    block
        .split(',')
        .map(|s| s.trim().trim_matches('"').trim())
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Row {
    krate: String,
    version: String,
    license: String,
    linkage: String,
    via: String,
}

impl Row {
    fn spec(&self) -> String {
        format!("{}@{}", self.krate, self.version)
    }
    /// The identity compared against `cargo deny`, deliberately excluding the
    /// prose columns: those are for humans, these three are the claim.
    fn triple(&self) -> (String, String, String) {
        (
            self.krate.clone(),
            self.version.clone(),
            self.license.clone(),
        )
    }
}

/// The fenced Markdown table in `NOTICE.md`. The header and separator rows are
/// dropped by requiring a version column that starts with a digit — a rule that
/// also rejects a half-written row rather than silently parsing it as data.
fn disclosure_rows() -> Vec<Row> {
    let text = read(NOTICE);
    let (_, rest) = text
        .split_once(BEGIN)
        .unwrap_or_else(|| panic!("{NOTICE} has no `{BEGIN}` fence"));
    let (block, _) = rest
        .split_once(END)
        .unwrap_or_else(|| panic!("{NOTICE} has no `{END}` fence"));

    let mut rows = Vec::new();
    for line in block.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect::<Vec<_>>();
        if cells.len() != 5 {
            continue;
        }
        if !cells[1].starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        rows.push(Row {
            krate: cells[0].to_string(),
            version: cells[1].to_string(),
            license: cells[2].to_string(),
            linkage: cells[3].to_string(),
            via: cells[4].to_string(),
        });
    }
    rows
}

// ── STATIC half ────────────────────────────────────────────────────────────

/// The root-cause assertion. Configuring a check and never running it is not a
/// policy — it is the *appearance* of one, which is worse than no file at all
/// because it reads as settled.
#[test]
fn every_configured_deny_check_kind_is_executed_by_some_ci_job() {
    let configured = configured_check_kinds();
    let executed = executed_check_kinds();

    // Non-vacuity, both sides. A parser that finds nothing asserts nothing.
    assert!(
        configured.len() >= 2,
        "parsed only {} check-kind section(s) from {DENY} ({configured:?}) — the \
         parser is broken, not the config",
        configured.len()
    );
    assert!(
        !executed.is_empty(),
        "parsed no `cargo deny check …` invocation from {CI} — the parser is \
         broken, or nothing runs cargo-deny at all"
    );

    let missing: Vec<&String> = configured
        .iter()
        .filter(|k| !executed.contains_key(*k))
        .collect();
    assert!(
        missing.is_empty(),
        "{DENY} configures check kind(s) {missing:?} that NO job in {CI} \
         executes. That is the PMAT-1409 defect verbatim: `[licenses]` was \
         configured from the start and `cargo deny check licenses` had never \
         once run, so the repo looked like it had a licence policy while the \
         real answer was unknown. Either run the kind (advisory job is fine — \
         see `license-scan`) or delete the section. Executed today: {:?}",
        executed.keys().collect::<Vec<_>>()
    );
}

#[test]
fn disclosure_rows_are_well_formed_and_outside_the_allow_list() {
    let rows = disclosure_rows();
    // Floor, not a ratchet: high enough that a red can never be fixed by
    // deleting rows, low enough that dropping a dependency is not a failure.
    assert!(
        rows.len() >= 5,
        "{NOTICE} discloses only {} row(s) — the table parser is broken or the \
         disclosure has been gutted. A red here is NOT fixable by deleting rows.",
        rows.len()
    );

    let allowed = allowed_licenses();
    assert!(
        allowed.len() >= 4,
        "parsed only {} entries from {DENY}'s `[licenses] allow` list \
         ({allowed:?}) — the parser is broken",
        allowed.len()
    );

    for row in &rows {
        assert!(
            LINKAGES.contains(&row.linkage.as_str()),
            "{NOTICE} row {} has linkage {:?}, which is not one of {LINKAGES:?}. \
             The linkage column is the whole reason this file is useful: \
             \"copyleft in the shipped binary\" and \"copyleft in a build \
             script\" are different facts.",
            row.spec(),
            row.linkage
        );
        assert!(
            !allowed.contains(&row.license),
            "{NOTICE} discloses {} under {:?}, which IS on {DENY}'s allow-list. \
             Rows for allowed licences are padding — they inflate the \
             non-vacuity floor without disclosing anything.",
            row.spec(),
            row.license
        );
        assert!(
            row.via.len() >= 10 && row.via.contains("->") || row.via.contains("build"),
            "{NOTICE} row {} has no usable `reached via` path ({:?}) — a \
             disclosure that does not say HOW the crate arrives cannot be acted on",
            row.spec(),
            row.via
        );
    }

    let specs: BTreeSet<String> = rows.iter().map(Row::spec).collect();
    assert_eq!(
        specs.len(),
        rows.len(),
        "{NOTICE} lists a duplicate crate@version row"
    );
}

/// The sentence that matters. `malachite` is copyleft and it is in the binary
/// every `cargo install xpile` produces; if that ever reads `dev-only`, either
/// the dependency graph changed (update the row, and the live half will agree)
/// or someone made the problem disappear by retyping it.
#[test]
fn every_lgpl_row_is_disclosed_as_linked_into_the_shipped_binary() {
    let rows = disclosure_rows();
    let lgpl: Vec<&Row> = rows
        .iter()
        .filter(|r| r.license.to_uppercase().starts_with("LGPL"))
        .collect();
    assert!(
        !lgpl.is_empty(),
        "{NOTICE} discloses no LGPL row. If the copyleft dependency really is \
         gone, that is excellent news and this test should be deleted in the \
         same commit that proves it — but it must not be deleted to make a red \
         go away."
    );
    for row in lgpl {
        assert_eq!(
            row.linkage,
            "binary",
            "{NOTICE} discloses LGPL crate {} as {:?}. It reaches the shipped \
             binary as a NORMAL dependency; re-derive with `cargo tree -p xpile \
             -e normal --all-features -i {}` before changing this column.",
            row.spec(),
            row.linkage,
            row.spec()
        );
    }

    let text = read(NOTICE);
    assert!(
        text.contains("lgpl-in-shipped-binary"),
        "{NOTICE} must point at the `lgpl-in-shipped-binary` owner decision. \
         Enumerating a copyleft dependency without naming who decides what to \
         do about it turns disclosure into a shrug."
    );
}

/// PMAT-1348's rule, applied to this file: a count typed into prose is stale
/// the next time a dependency moves. `NOTICE.md` states DERIVE COMMANDS.
#[test]
fn the_notice_states_derive_commands_and_no_bare_counts() {
    let text = read(NOTICE);
    for needle in [
        "cargo deny check licenses",
        "cargo tree -p xpile -e normal",
        "cargo test -p xpile --test dependency_license_policy",
    ] {
        assert!(
            text.contains(needle),
            "{NOTICE} must state the derive command `{needle}` so a reader can \
             re-check it instead of trusting the table"
        );
    }

    // Bare cardinalities about the disclosure itself. Numbers inside a version
    // (`0.4.22`) or a command are untouched; this looks only for a digit or an
    // English cardinal immediately qualifying a noun the table already counts.
    let cardinals = [
        "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    ];
    let nouns = ["rejection", "rejections", "offending crates", "violations"];
    let lower = text.to_lowercase();
    let mut found = Vec::new();
    for noun in nouns {
        for (idx, _) in lower.match_indices(noun) {
            let prefix: String = lower[..idx].chars().rev().take(12).collect::<String>();
            let prefix: String = prefix.chars().rev().collect();
            let last_word = prefix.split_whitespace().next_back().unwrap_or("");
            if last_word.chars().all(|c| c.is_ascii_digit()) && !last_word.is_empty()
                || cardinals.contains(&last_word)
            {
                found.push(format!("{last_word} {noun}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "{NOTICE} carries bare derived count(s): {found:?}. State the derive \
         command instead — `docs/status/CURRENT.md` accumulated five \
         simultaneous false counts exactly this way (PMAT-1348)."
    );
}

/// A job that reports and a job whose verdict means something are different
/// things. The raw `cargo deny` step is `continue-on-error` on purpose (it
/// exits 4 today and a permanently-red advisory job is invisible — `wasi` was
/// RED on the SHA tagged v0.1.617 and the release was cut anyway). The drift
/// tripwire must NOT be, or the whole job is decorative.
#[test]
fn the_license_scan_job_runs_the_check_and_its_tripwire_can_fail() {
    let ci = read(CI);
    assert!(
        ci.contains("\n    name: license-scan\n"),
        "{CI} has no `license-scan` job — nothing executes the licence check"
    );
    assert!(
        ci.contains("cargo deny check licenses"),
        "{CI} never runs `cargo deny check licenses`"
    );
    assert!(
        ci.contains("XPILE_REQUIRE_DENY"),
        "{CI} does not set XPILE_REQUIRE_DENY, so the live half of this gate \
         would skip-green on a runner with no cargo-deny — the skip-as-green \
         shape XPILE-WITNESS-002 exists to kill"
    );

    // Locate the tripwire step and assert `continue-on-error` is not attached
    // to it. Steps are separated by `      - name:` at six-space indent.
    let job = ci
        .split_once("  license-scan:")
        .map(|(_, rest)| rest)
        .expect("license-scan job body");
    let tripwire = job
        .split("      - name:")
        .find(|s| s.contains("--test dependency_license_policy"))
        .expect("license-scan runs the dependency_license_policy tripwire");
    assert!(
        !tripwire.contains("continue-on-error"),
        "the `license-scan` tripwire step is `continue-on-error`, which makes \
         the whole job decorative: it would stay green through any drift it is \
         supposed to catch. Only the RAW report step may ignore its exit code."
    );
}

// ── LIVE half ──────────────────────────────────────────────────────────────

/// `Some(reason)` when the live half cannot run. `XPILE_REQUIRE_DENY=1` (set on
/// the `license-scan` job) converts that into a hard failure.
fn live_blocked() -> Option<String> {
    let required = std::env::var("XPILE_REQUIRE_DENY").is_ok_and(|v| v == "1");
    for (bin, args) in [("cargo-deny", ["--version"]), ("cargo", ["--version"])] {
        let ok = Command::new(bin)
            .args(args)
            .output()
            .is_ok_and(|o| o.status.success());
        if !ok {
            let reason = format!("`{bin}` is not available");
            assert!(
                !required,
                "XPILE_REQUIRE_DENY=1 but {reason}. This host DECLARED it would \
                 execute the licence tripwire; skipping green here is the exact \
                 failure the flag exists to prevent."
            );
            return Some(reason);
        }
    }
    None
}

/// Every `(crate, version, licence)` cargo-deny rejects, from the JSON output.
///
/// Shape (verified against cargo-deny's `--format json`): one diagnostic per
/// line, the rejected crate at `fields.graphs[0].Krate`, the SPDX id at
/// `fields.labels[0].span`. Parsed from JSON rather than the human output
/// because the human output prints `rejected` twice per diagnostic — once in
/// the headline and once in the label — which is how the queue entry that
/// scheduled this slice came to claim double the real number of rejections.
fn live_rejections() -> BTreeSet<(String, String, String)> {
    let out = Command::new("cargo")
        .args(["deny", "--format", "json", "check", "licenses"])
        .current_dir(workspace_root())
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .expect("run cargo deny");
    let text = String::from_utf8_lossy(&out.stderr);

    let mut set = BTreeSet::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let f = &v["fields"];
        if f["code"] != "rejected" {
            continue;
        }
        let krate = &f["graphs"][0]["Krate"];
        let (Some(name), Some(version), Some(license)) = (
            krate["name"].as_str(),
            krate["version"].as_str(),
            f["labels"][0]["span"].as_str(),
        ) else {
            panic!("cargo-deny JSON shape changed — cannot read a rejection: {line}");
        };
        set.insert((name.to_string(), version.to_string(), license.to_string()));
    }
    set
}

#[test]
fn the_notice_enumerates_exactly_the_live_rejections() {
    if let Some(reason) = live_blocked() {
        eprintln!("SKIP the_notice_enumerates_exactly_the_live_rejections: {reason}");
        return;
    }

    let live = live_rejections();
    assert!(
        !live.is_empty(),
        "cargo-deny reported NO licence rejection. Either the dependency graph \
         got dramatically cleaner (delete the {NOTICE} rows in the same commit \
         that proves it) or the JSON parser above is silently matching nothing \
         — check by running `cargo deny check licenses` by hand."
    );

    let documented: BTreeSet<(String, String, String)> =
        disclosure_rows().iter().map(Row::triple).collect();

    let undisclosed: Vec<_> = live.difference(&documented).collect();
    let stale: Vec<_> = documented.difference(&live).collect();

    assert!(
        undisclosed.is_empty(),
        "cargo-deny rejects {undisclosed:?}, which {NOTICE} does not disclose. \
         A new non-permissive dependency entered the graph — add a row with its \
         linkage (derive it, do not guess: `cargo tree -p xpile -e normal \
         --all-features -i <crate>@<version>`)."
    );
    assert!(
        stale.is_empty(),
        "{NOTICE} discloses {stale:?}, which cargo-deny no longer rejects. \
         Either the crate left the graph (delete the row) or its licence \
         changed. Both directions are checked so this file cannot rot."
    );
}

#[test]
fn every_linkage_claim_is_re_derived_from_cargo_tree() {
    if let Some(reason) = live_blocked() {
        eprintln!("SKIP every_linkage_claim_is_re_derived_from_cargo_tree: {reason}");
        return;
    }

    let rows = disclosure_rows();
    let mut checked = 0usize;
    for row in &rows {
        let normal = cargo_tree(&row.spec(), "normal");
        let linked = reaches_the_xpile_binary(&normal);

        match row.linkage.as_str() {
            "binary" => {
                assert!(
                    linked,
                    "{NOTICE} claims {} is linked into the shipped binary, but \
                     `cargo tree -p xpile -e normal -i {}` does not reach the \
                     `xpile` package. Output:\n{normal}",
                    row.spec(),
                    row.spec()
                );
            }
            other => {
                assert!(
                    !linked,
                    "{NOTICE} claims {} is {other}, but it IS reachable over \
                     NORMAL edges from the `xpile` binary — i.e. it ships. \
                     Output:\n{normal}",
                    row.spec()
                );
                // …and it must still be in the graph SOMEHOW, or the row names
                // a crate nothing depends on and the negative passes for free.
                let with_build = cargo_tree(&row.spec(), "normal,build");
                assert!(
                    reaches_the_xpile_binary(&with_build),
                    "{NOTICE} claims {} is {other}, but it is not reachable \
                     from `xpile` over normal+build edges either — the negative \
                     above passed for free. Output:\n{with_build}",
                    row.spec()
                );
            }
        }
        checked += 1;
    }
    assert_eq!(
        checked,
        rows.len(),
        "linkage re-derivation skipped a row — the loop is broken"
    );
    assert!(checked >= 5, "only {checked} linkage claim(s) re-derived");
}

fn cargo_tree(spec: &str, edges: &str) -> String {
    let out = Command::new("cargo")
        .args([
            "tree",
            "-p",
            "xpile",
            "-e",
            edges,
            "--all-features",
            "--locked",
            "-i",
            spec,
        ])
        .current_dir(workspace_root())
        // Pin the CHILD's environment rather than inheriting it. `ci.yml` sets
        // `CARGO_TERM_COLOR: always` workflow-wide, so on a runner `cargo tree`
        // wraps every glyph in ANSI escapes while locally it emits none (cargo
        // auto-disables colour when stdout is not a tty). The first CI run of
        // this job failed exactly there, on output that visibly DID reach
        // `xpile v0.1.617` — a green local run proved nothing about the runner.
        // The parser below strips escapes anyway; this makes its input
        // deterministic instead of environment-dependent.
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .unwrap_or_else(|e| panic!("cargo tree -i {spec}: {e}"));
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Strip ANSI CSI escape sequences (`ESC [ … m`). `cargo tree` colours its
/// glyphs when `CARGO_TERM_COLOR=always`, which `ci.yml` sets workflow-wide, so
/// a runner sees `\x1b[2m└──\x1b[0m xpile v0.1.617` where a local pipe sees
/// `└── xpile v0.1.617`.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // Consume the CSI sequence up to and including its final byte.
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Does an inverted `cargo tree` reach the root `xpile` package? The trailing
/// space in `"xpile v"` is load-bearing — without it every row would "reach"
/// via `xpile-core`, and `xpile-core` is a library, not the thing users run.
fn reaches_the_xpile_binary(tree: &str) -> bool {
    tree.lines().any(|line| {
        strip_ansi(line)
            .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '└' | '├' | '─' | '│'))
            .starts_with("xpile v")
    })
}

/// The regression this file paid for in CI. `every_linkage_claim_…` shells out
/// to cargo, and a test that parses a subprocess's output is only as good as
/// its assumptions about that subprocess's environment: the first CI run of
/// `license-scan` failed on a tree that visibly DID contain
/// `└── xpile v0.1.617 (…)`, because the runner's `CARGO_TERM_COLOR: always`
/// wrapped every glyph in escapes the parser walked straight past. Locally,
/// cargo emitted no colour and the same test passed. Both spellings are pinned
/// here so the fix cannot regress silently, together with the negatives that
/// keep the matcher from becoming "any line mentioning xpile".
#[test]
fn the_tree_matcher_reads_coloured_and_plain_output_alike() {
    let plain = "malachite v0.4.22\n└── malachite-bigint v0.2.3\n    └── xpile v0.1.617 (/w/crates/xpile)\n";
    let coloured = "malachite v0.4.22\n\u{1b}[2m└──\u{1b}[0m malachite-bigint v0.2.3\n\u{1b}[2m \u{1b}[0m   \u{1b}[2m└──\u{1b}[0m xpile v0.1.617 (/w/crates/xpile)\n";
    assert!(reaches_the_xpile_binary(plain), "plain tree must match");
    assert!(
        reaches_the_xpile_binary(coloured),
        "ANSI-coloured tree must match — this is the exact form the runner emits"
    );

    // Negatives: reaching a LIBRARY is not reaching the shipped binary, and an
    // empty inverted tree (cargo tree's answer for an unreachable crate) is not
    // a match. Without these the matcher could pass by being permissive.
    assert!(
        !reaches_the_xpile_binary(
            "hexf-parse v0.2.1\n└── xpile-core v0.1.617 (/w/crates/xpile-core)\n"
        ),
        "`xpile-core` is a library — the trailing space in \"xpile v\" is what keeps them apart"
    );
    assert!(
        !reaches_the_xpile_binary("warning: nothing to print.\n"),
        "an empty inverted tree must not read as reachable"
    );
    assert!(
        !reaches_the_xpile_binary("\u{1b}[2m└──\u{1b}[0m xpile-wgsl-codegen v0.1.617\n"),
        "a coloured LIBRARY line must not match either"
    );
}
