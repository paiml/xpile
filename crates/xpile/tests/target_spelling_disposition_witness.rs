//! XPILE-TARGET-SPELL-001 (PMAT-1435) — the `--target` VOCABULARY is one
//! claim class with four surfaces, and every one of them must name the same
//! set the binary actually accepts.
//!
//! ## The defect this locks out
//!
//! `parse_target` accepted THIRTEEN spellings. Four of them — `wat` (→ `wasm`),
//! `sh` / `bash` (→ `shell`) and `forjar-yaml` (→ `forjar`) — were named by no
//! surface the binary prints. Measured at `8582f9c4` on a force-rebuilt binary:
//!
//! ```text
//! $ xpile transpile p.py --target wat            # exit 0, 1727 bytes of WAT
//! $ xpile transpile p.py --target bashrs
//! Error: unknown target `bashrs`; choose: rust, ruchy, ptx, wgsl, spirv, wasm, lean, shell, forjar
//! ```
//!
//! The refusal — the "what can I use instead" surface, PMAT-1434's shape one
//! flag over — enumerated nine. `xpile transpile --help` enumerated the same
//! nine. `book/src/reference/cli.md` published the CLOSED-WORLD claim *"one of
//! `rust`, `ruchy`, `ptx`, `wgsl`, `spirv`, `wasm`, `lean`, `shell`,
//! `forjar`"*, which was false; `book/src/reference/backends.md` disclosed all
//! four aliases two pages over, so the book contradicted itself and the only
//! true statement in the repo was the one no gate read.
//!
//! ## Why the existing gates could not see it — and actively held it in place
//!
//! Both `--target` gates modelled "what the CLI accepts" as *what `--help`
//! says*:
//!
//! - `cli_docs_drift.rs::the_target_row_names_exactly_the_accepted_target_spellings`
//!   is named for this exact property and checks BOTH directions. With `live`
//!   short by four it could never fire in the omit direction, and in the
//!   advertise direction it FORBADE the row from naming a real spelling.
//! - `backend_docs_drift.rs::every_inline_target_flag_in_the_book_names_a_live_spelling`
//!   likewise. MEASURED: appending ``--target wat`` to `backends.md` red-ed it
//!   with *"the book uses `--target <x>` with value(s) the CLI does not
//!   accept: [(…, \"wat\")]"* — about a value the CLI does accept.
//!
//! `backend_docs_drift.rs`'s own doc comment claimed its executed half "could
//! catch a `--help` string that has drifted from `parse_target`". It could
//! not, in the direction that was wrong: the executed set is
//! `documented ∪ advertised`, both derived from CLAIMS, so a spelling `--help`
//! OMITS is in neither set and is never run. **A both-directions gate is only
//! as honest as the set it compares to**; when both sides of a set equality
//! are claims, agreement is not truth.
//!
//! ## What this file measures instead
//!
//! `target_spelling_help()` now renders the refusal from `TARGET_SPELLINGS`,
//! the single roster `parse_target` matches through, so the message is
//! BEHAVIOUR. Every check below derives its sets from the running binary —
//! there is no roster written down here (PMAT-1396) — and each requires a
//! known anchor so a broken scan cannot pass vacuously (PMAT-1416).
//!
//! The substantive claim `backends.md` makes about an alias is not that it
//! parses but that it is *indistinguishable* from its canonical spelling, so
//! that is asserted on the full observable behaviour (stdout, stderr and exit
//! status), not on exit code alone — an alias that silently emitted something
//! else would be a wrong answer, not a documentation defect.
//!
//! `parse_target` has TWO call sites (`transpile` and `audit`), so the
//! vocabulary is checked at both: a guard written for one argument is not a
//! guard for the function (PMAT-1387).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::{Command, Output};

/// A spelling no roster will ever contain, used both as the trigger for the
/// refusal and as the anti-vacuity control.
const UNKNOWN_TARGET: &str = "__no_such_target__";

fn probe_source(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "xpile-tgtspell-{}-{tag}-{}",
        std::process::id(),
        // Distinct per call site: multi-exec probes sharing one temp dir have
        // raced in this repo before.
        tag.len()
    ));
    std::fs::create_dir_all(&dir).expect("create probe dir");
    let src = dir.join("probe.py");
    std::fs::write(&src, "def add(a: int, b: int) -> int:\n    return a + b\n").expect("write");
    src
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xpile"))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("running `xpile {}`: {e}", args.join(" ")))
}

/// The raw `unknown target …` refusal for a given subcommand.
fn refusal(subcommand: &str) -> String {
    let src = probe_source(subcommand);
    let out = run(&[
        subcommand,
        &src.to_string_lossy(),
        "--target",
        UNKNOWN_TARGET,
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        !out.status.success(),
        "`xpile {subcommand} --target {UNKNOWN_TARGET}` EXITED 0 — an unknown \
         target must refuse, or every check in this file is vacuous"
    );
    assert!(
        stderr.contains("unknown target"),
        "`xpile {subcommand} --target {UNKNOWN_TARGET}` refused without naming \
         the target as the cause:\n{stderr}"
    );
    stderr
}

/// `(canonical, aliases)` as the refusal message publishes them; an alias is
/// carried with the canonical spelling it resolves to.
fn published_vocabulary(stderr: &str) -> (BTreeSet<String>, BTreeSet<(String, String)>) {
    let vocab = stderr
        .split("unknown target")
        .nth(1)
        .unwrap_or_else(|| panic!("no `unknown target` in:\n{stderr}"));
    let section = |k: &str| -> Vec<String> {
        vocab
            .split(k)
            .nth(1)
            .map(|s| {
                s.split(';')
                    .next()
                    .unwrap_or("")
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    let canonical: BTreeSet<String> = section("choose:").into_iter().collect();
    let aliases: BTreeSet<(String, String)> = section("aliases:")
        .into_iter()
        .filter_map(|t| {
            let (spelling, canon) = t.split_once('=')?;
            Some((spelling.trim().to_string(), canon.trim().to_string()))
        })
        .collect();
    (canonical, aliases)
}

/// The `Target backend:` vocabulary `xpile transpile --help` advertises,
/// parsed in the same two halves.
fn help_vocabulary() -> (BTreeSet<String>, BTreeSet<(String, String)>) {
    let out = run(&["transpile", "--help"]);
    assert!(out.status.success(), "`xpile transpile --help` must exit 0");
    let help = String::from_utf8_lossy(&out.stdout).to_string();
    let line = help
        .lines()
        .find(|l| l.contains("Target backend:"))
        .unwrap_or_else(|| panic!("`--help` must describe `Target backend:`:\n{help}"))
        .to_string();
    let body = line
        .split("Target backend:")
        .nth(1)
        .expect("checked above")
        .split("[default")
        .next()
        .unwrap_or("")
        .to_string();
    let canonical: BTreeSet<String> = body
        .split("aliases:")
        .next()
        .unwrap_or("")
        .split('|')
        .map(|t| t.trim().trim_end_matches(';').trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    let aliases: BTreeSet<(String, String)> = body
        .split("aliases:")
        .nth(1)
        .unwrap_or("")
        .split(',')
        .filter_map(|t| {
            let (spelling, canon) = t.trim().split_once('=')?;
            Some((spelling.trim().to_string(), canon.trim().to_string()))
        })
        .collect();
    (canonical, aliases)
}

/// The refusal message must publish a set with real content in both halves.
/// Anchored on spellings that exist for independent reasons, so a scan that
/// silently returns nothing cannot make every other check pass (PMAT-1416).
#[test]
fn the_published_vocabulary_is_derived_and_not_empty() {
    let (canonical, aliases) = published_vocabulary(&refusal("transpile"));
    assert!(
        canonical.len() > 5 && canonical.contains("rust") && canonical.contains("shell"),
        "the refusal's `choose:` half parsed as {canonical:?} — the message \
         shape changed and every check in this file would pass vacuously"
    );
    assert!(
        !aliases.is_empty()
            && aliases
                .iter()
                .any(|(spelling, canon)| spelling == "wat" && canon == "wasm"),
        "the refusal's `aliases:` half parsed as {aliases:?} — it must carry at \
         least the `wat=wasm` alias PMAT-1435 was opened on"
    );
    assert!(
        canonical.is_disjoint(&aliases.iter().map(|(s, _)| s.clone()).collect()),
        "a spelling is published as BOTH canonical and an alias: {canonical:?} \
         vs {aliases:?}"
    );
    for (spelling, canon) in &aliases {
        assert!(
            canonical.contains(canon),
            "alias `{spelling}` resolves to `{canon}`, which the same message \
             does not publish as a canonical spelling ({canonical:?})"
        );
    }
}

/// THE LOAD-BEARING CHECK. Every spelling the refusal publishes is one the
/// running binary ACCEPTS, and a spelling it does not publish is REFUSED.
/// Set equality against BEHAVIOUR, in both directions.
///
/// A backend may legitimately refuse the probe program (`shell` and `forjar`
/// want a shell-origin module; `ptx` wants `--hardware`) — what must never
/// happen is the SPELLING being rejected.
#[test]
fn every_published_spelling_is_accepted_and_the_unpublished_one_is_not() {
    let (canonical, aliases) = published_vocabulary(&refusal("transpile"));
    let src = probe_source("accept");
    let src = src.to_string_lossy().into_owned();

    let mut published: Vec<String> = canonical.iter().cloned().collect();
    published.extend(aliases.iter().map(|(s, _)| s.clone()));
    assert!(
        published.len() > 8,
        "only {published:?} to execute — the scan broke"
    );

    for spelling in &published {
        let out = run(&["transpile", &src, "--target", spelling]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("unknown target"),
            "`--target {spelling}` is published by the binary's own refusal \
             message and REJECTED by the same binary:\n{stderr}"
        );
    }

    // The control. Without it, a `parse_target` that accepted EVERYTHING would
    // satisfy the loop above.
    let out = run(&["transpile", &src, "--target", UNKNOWN_TARGET]);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("unknown target"),
        "`--target {UNKNOWN_TARGET}` was not rejected — the check above proves \
         nothing"
    );
}

/// An alias must be INDISTINGUISHABLE from the canonical spelling it resolves
/// to, which is what `book/src/reference/backends.md` tells the reader. Asserted
/// on the full observable behaviour, not on exit status: an alias that parsed
/// but emitted something else would be a silent wrong answer.
#[test]
fn every_alias_is_byte_identical_to_its_canonical_spelling() {
    let (_, aliases) = published_vocabulary(&refusal("transpile"));
    assert!(
        !aliases.is_empty(),
        "no aliases parsed — this check would range over nothing"
    );
    let src = probe_source("alias");
    let src = src.to_string_lossy().into_owned();

    for (spelling, canon) in &aliases {
        let a = run(&["transpile", &src, "--target", spelling]);
        let c = run(&["transpile", &src, "--target", canon]);
        assert_eq!(
            a.status.code(),
            c.status.code(),
            "`--target {spelling}` exits {:?} but `--target {canon}` exits {:?}",
            a.status.code(),
            c.status.code()
        );
        assert_eq!(
            String::from_utf8_lossy(&a.stdout),
            String::from_utf8_lossy(&c.stdout),
            "`--target {spelling}` and `--target {canon}` emit different stdout, \
             but backends.md calls them the same target"
        );
        assert_eq!(
            String::from_utf8_lossy(&a.stderr),
            String::from_utf8_lossy(&c.stderr),
            "`--target {spelling}` and `--target {canon}` report differently on \
             stderr, but backends.md calls them the same target"
        );
    }
}

/// `--help` and the refusal are two independently authored strings describing
/// one roster. Set equality BOTH ways, both halves. This is the check that was
/// missing: `--help` named nine while `parse_target` took thirteen, and no
/// gate compared it to anything but itself.
#[test]
fn the_help_vocabulary_equals_the_refusal_vocabulary() {
    let (refusal_canonical, refusal_aliases) = published_vocabulary(&refusal("transpile"));
    let (help_canonical, help_aliases) = help_vocabulary();

    assert!(
        help_canonical.len() > 5 && help_canonical.contains("rust"),
        "`--help` canonical half parsed as {help_canonical:?} — shape changed"
    );
    assert_eq!(
        help_canonical, refusal_canonical,
        "`xpile transpile --help` and the `unknown target` refusal disagree \
         about the CANONICAL spellings.\n--help:  {help_canonical:?}\nrefusal: \
         {refusal_canonical:?}"
    );
    assert_eq!(
        help_aliases, refusal_aliases,
        "`xpile transpile --help` and the `unknown target` refusal disagree \
         about the ALIASES.\n--help:  {help_aliases:?}\nrefusal: \
         {refusal_aliases:?}"
    );
}

/// `parse_target` is reached from `transpile` AND from `audit`. Both must
/// publish the same vocabulary — a guard written for one argument is not a
/// guard for the function (PMAT-1387), and `audit --target` carries no
/// spelling list of its own for a reader to fall back on.
#[test]
fn the_audit_subcommand_publishes_the_same_vocabulary() {
    let (t_canonical, t_aliases) = published_vocabulary(&refusal("transpile"));
    let (a_canonical, a_aliases) = published_vocabulary(&refusal("audit"));
    assert_eq!(
        a_canonical, t_canonical,
        "`xpile audit --target` and `xpile transpile --target` publish \
         different canonical spellings"
    );
    assert_eq!(
        a_aliases, t_aliases,
        "`xpile audit --target` and `xpile transpile --target` publish \
         different aliases"
    );

    // …and `audit` really does ACCEPT an alias, not merely name one.
    let src = probe_source("audit");
    let alias = t_aliases
        .iter()
        .next()
        .map(|(s, _)| s.clone())
        .expect("at least one alias");
    let out = run(&[
        "audit",
        &src.to_string_lossy(),
        "--target",
        &alias,
        "--json",
    ]);
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("unknown target"),
        "`xpile audit --target {alias}` rejects a spelling `xpile audit` itself \
         publishes"
    );
}

/// The canonical spellings are one-per-registered-backend. This ties the
/// vocabulary to the REGISTRY rather than to another string (PMAT-1433: a gate
/// that pins doc-to-binary reproduces the binary being wrong), so adding a
/// backend without a `--target` spelling — or a spelling with no backend —
/// fails here rather than in the book.
#[test]
fn the_canonical_spellings_are_one_per_registered_backend() {
    let (canonical, _) = published_vocabulary(&refusal("transpile"));
    let info = run(&["info"]);
    assert!(info.status.success(), "`xpile info` must exit 0");
    let info = String::from_utf8_lossy(&info.stdout).to_string();
    let live: usize = info
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("backends (")
                .and_then(|r| r.split(')').next())
                .and_then(|n| n.trim().parse::<usize>().ok())
        })
        .unwrap_or_else(|| panic!("`xpile info` must print a `backends (N):` header:\n{info}"));
    assert!(
        live > 3,
        "`xpile info` reports {live} backends — registry moved"
    );
    assert_eq!(
        canonical.len(),
        live,
        "the `--target` refusal publishes {} canonical spelling(s) but `xpile \
         info` reports {live} registered backend(s): {canonical:?}",
        canonical.len()
    );
}
