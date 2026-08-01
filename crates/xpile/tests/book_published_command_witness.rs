//! XPILE-BOOKTRANSCRIPT-001 arm (a) (PMAT-1511) — **a shell command published
//! in a reader-facing page is an EXECUTABLE claim, and nothing in this
//! repository had ever executed one.**
//!
//! ## Why this arm exists
//!
//! `book/src/quickstart.md` told every reader to run
//! `rustc -O factorial.rs --crate-type lib --emit=metadata -o /dev/null`. That
//! line **exited 1 on every host whose user cannot write to `/dev`** — i.e.
//! essentially every reader's — for 72 days, on the page a first-time reader
//! reaches first. PMAT-1504 found it by *running* it and repaired the text; it
//! could not ship a gate, because the Wed 2026-07-29 freeze barred a new
//! `tests/` file. This is that gate.
//!
//! Screening by READING is what let the defect live: the command is
//! well-formed, the flags are real, and the failure is an *environment* error
//! wearing a compile error's clothes. The only instrument that sees it is
//! execution.
//!
//! ## THE BOUNDARY IS TWO LISTS, NOT ONE RULE
//!
//! Measured 2026-07-31 over `README.md` + `book/src/**`: **76** published
//! command lines. Half of them cannot be executed by a test — `cargo install`
//! mutates the caller's `~/.cargo/bin`, `git clone` needs the network,
//! `cargo run --example` needs a checkout and minutes of build time, and
//! `xpile transpile [OPTIONS] <INPUT>` is a *synopsis*, not an invocation. So
//! the corpus is partitioned by a **closed, header-declared taxonomy**
//! ([`SCREENS`] + [`is_synopsis`]) and [`screened_set_is_logged_not_silent`]
//! prints every screened line with its reason. A silent filter reads as
//! "covered everything" when it covered half (PMAT-1502 (d)).
//!
//! The partition is TOTAL by construction — [`classify`] returns
//! [`Disposition::Execute`] for anything no screen claims — so a newly
//! published command joins the EXECUTED set by default. A command can only
//! leave it by someone adding a screen pattern, which is a visible edit.
//!
//! ## Two properties, and the second one is why a clean arm (a) is not enough
//!
//! **Property A — exit status.** Every executed command's exit status must
//! match what its own page publishes. The expected status is *derived*, not
//! listed: a published transcript whose first non-blank line begins with
//! `Error:` is a REFUSAL and must exit non-zero
//! (`book/src/tutorials/shell-roundtrip.md:60` is the live instance); every
//! other command must exit 0.
//!
//! **Property B — the checkout precondition.** Property A alone would have
//! passed this repository on 2026-07-31 with **zero** findings, and two pages
//! were nevertheless wrong. `xpile diamond` exits **0** in an empty directory —
//! the contract corpus is compiled into the binary — and `README.md` says so
//! twice ("works anywhere", "from any directory"), and the binary's own
//! `quorum` error text says so a third time. But
//! `book/src/installation.md` said a source checkout "is also required" to run
//! `xpile diamond`, and `book/src/quickstart.md` annotated the same command
//! `# if you're in a repo with contracts/`. Both were false, and both were
//! **under**-claims — the shape PMAT-1473 named, where a "what's still broken"
//! note outlives the thing it describes.
//!
//! So Property B **measures** the roster instead of pinning it: every
//! subcommand `xpile --help` advertises is probed bare in an empty scratch
//! directory, and the pages are checked against that measurement in BOTH
//! directions. Regressing `diamond` into needing a checkout reds this file just
//! as loudly as re-introducing the prose would. Nothing here names a
//! subcommand that a future `--help` will not, because the roster IS `--help`.
//!
//! ⚠️ Subcommands that exit **2** bare are excluded from the B roster and
//! logged: clap's usage exit means a required operand is missing, which is not
//! the same claim as "needs a checkout". `transpile` and `hybrid` are the two.
//!
//! ## The stale-artefact trap, closed structurally
//!
//! PMAT-1504's first run of this arm reported five of the book's six
//! `cargo run --example` lines as FAILING; all six exit 0. The cached example
//! binaries in the shared target dir had been built inside a git worktree since
//! removed, and every example resolves its input through
//! `env!("CARGO_MANIFEST_DIR")`, baked at COMPILE time. This file cannot
//! reproduce that: the binary under test is `env!("CARGO_BIN_EXE_xpile")`,
//! which cargo rebuilds as a dependency of this test, and the six
//! `cargo run --example` lines are screened rather than executed.
//!
//! ## Non-vacuity
//!
//! Every quantified rule here floors its own subject **inline**, not by
//! borrowing a sibling's floor — the two enforcement rules each assert their
//! own set size before ranging over it, because a later edit can separate a
//! rule from the test that floors it (PMAT-1510). On top of that:
//! [`corpus_is_derived_and_floored`], [`executed_set_has_a_floor`] (per ARM —
//! each anchor page must contribute, and the deliberate-refusal arm must stay
//! live), [`fixture_resolver_is_non_vacuous`],
//! [`subcommand_roster_is_non_trivial`] (the measured split must be
//! non-degenerate, or property B is unfalsifiable in one direction),
//! [`both_precondition_vocabularies_match_live_prose`],
//! [`detection_vocabularies_are_live_as_groups`],
//! [`every_disclosure_exemption_literal_is_live`], and
//! [`checkout_lane_is_logged_and_bounded`].
//!
//! [`a_command_known_to_fail_is_reported_as_failing`] drives the real executor
//! on a constructed command that exits non-zero — an executor that swallows
//! status reads green forever otherwise.
//!
//! ## What the red halves proved, and one that lied twice
//!
//! Seven perturbations, each asserted to have APPLIED by re-reading the file:
//! restoring the `-o /dev/null` spelling reds property A; restoring either
//! false page reds property B (comment arm and prose arm separately); flipping
//! one tabulated row reds arm B3; neutering the executor, the corpus pathspec
//! or the synopsis screen reds the floors.
//!
//! ⚠️ **The prose arm's red half came back GREEN twice, for two DIFFERENT
//! reasons, and both were bugs in the probe.** First the perturbation deleted
//! the table along with the sentence, so what reddened was arm B3's floor, not
//! the claim rule — a red for the wrong reason reads exactly like a red for the
//! right one. Then the replacement probe passed its backticks through a
//! single-quoted shell string, which delivered `` \`xpile diamond\` `` and no
//! claim parsed. Only the third spelling was a valid instance of the subject
//! class. **A green red-half is a bug in the red half until proven otherwise,
//! and a RED one still has to be red for the stated reason** (PMAT-1477,
//! PMAT-1510's three-arm probe). The fourth arm — the same variant wording
//! applied to `xpile quorum`, where the claim is TRUE — stays green, which is
//! what proves the rule does not red correct pages.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// Corpus
// ---------------------------------------------------------------------------

/// The reader-facing published surfaces. `README.md` is the page a crates.io
/// reader gets without cloning; `book/src/**` is the rendered book.
///
/// `CHANGELOG.md` carries 73 more `$` lines and is deliberately OUT of scope:
/// it is append-only narration of what past releases did, not an instruction to
/// the reader, and gating it would gate the project's record of its own history
/// (PMAT-1477's lesson — measure the false-positive rate of a corpus widening
/// before doing it). `docs/specifications/**` is out for the same reason plus
/// the v0.2.0 pages, which describe commands that deliberately do not exist yet.
///
/// ⚠️ PMAT-1514 WIDENED THIS. The list was `["README.md", "book/src"]`, and the
/// git pathspec `README.md` matches **only the root README** — so four other
/// tracked, reader-facing READMEs publishing ten command lines sat outside a
/// corpus whose own comment defines itself as "the page a crates.io reader gets
/// without cloning". Two of them (`contracts/README.md`, `examples/README.md`)
/// are **packaged into the crate**, so a crates.io reader receives them exactly
/// as they receive the root README. The stated principle was right; the
/// pathspec did not implement it.
const CORPUS_PATHSPECS: &[&str] = &["README.md", "*/README.md", "*/*/README.md", "book/src"];

/// Fence languages whose contents are shell.
const SHELL_FENCES: &[&str] = &["bash", "sh", "console", "shell"];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/xpile → repo root")
        .to_path_buf()
}

/// Tracked `.md` files under [`CORPUS_PATHSPECS`], DERIVED from `git ls-files`
/// so a new book page joins the corpus without an edit here.
fn corpus() -> Vec<PathBuf> {
    let root = repo_root();
    let out = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("ls-files")
        .args(CORPUS_PATHSPECS)
        .output()
        .expect("git ls-files must run — this gate's whole corpus is derived from it");
    assert!(out.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.ends_with(".md"))
        .map(|l| root.join(l))
        .collect()
}

fn rel(p: &Path) -> String {
    p.strip_prefix(repo_root())
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// One published command line, with the transcript the page prints under it.
#[derive(Debug, Clone)]
struct PublishedCmd {
    file: String,
    line: usize,
    /// The command as published, `$ ` stripped, trailing `# comment` INTACT.
    cmd: String,
    /// The trailing `# …` comment, if any — Property B reads this.
    comment: Option<String>,
    /// Lines the page prints under this command, up to the next `$ ` or the
    /// closing fence.
    transcript: Vec<String>,
    /// Set when an earlier line in the SAME fence is a `cd` — this command
    /// cannot be run standalone, because its working directory was established
    /// by a predecessor the taxonomy screens (PMAT-1514).
    chained: bool,
    /// Which fence this came from, so sequential commands share a scratch dir.
    fence: usize,
    /// Was the line `$ `-prefixed? Bare lines in a fence that HAS `$` lines are
    /// transcript output and get dropped in the second pass.
    dollar: bool,
}

/// Extract every published command from one page.
///
/// A `$ `-prefixed line inside a shell fence is always a command. A bare line
/// is a command **only in a fence that has no `$` lines at all** — in a fence
/// that does, bare lines are the transcript of the command above them, and
/// executing them would run the emitted Rust as if it were shell.
fn published_commands(path: &Path) -> Vec<PublishedCmd> {
    let text = std::fs::read_to_string(path).expect("read corpus page");
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();

    let mut in_fence = false;
    let mut lang = String::new();
    let mut fence_id = 0usize;
    // Lines already absorbed into a multi-line command above (PMAT-1514).
    let mut skip_until = 0usize;

    for (idx, raw) in lines.iter().enumerate() {
        let s = raw.trim();
        if let Some(tag) = s.strip_prefix("```") {
            if in_fence {
                in_fence = false;
                lang.clear();
            } else {
                in_fence = true;
                lang = tag.trim().to_string();
                fence_id += 1;
            }
            continue;
        }
        if !in_fence || !SHELL_FENCES.contains(&lang.as_str()) {
            continue;
        }
        if idx <= skip_until && skip_until > 0 {
            continue;
        }
        let dollar = s.starts_with("$ ");
        let mut cmd = if dollar {
            s[2..].trim().to_string()
        } else if s.is_empty() || s.starts_with('#') {
            continue;
        } else {
            s.to_string()
        };

        // PMAT-1514: a published command may span lines, and executing the
        // pieces separately is not a measurement of it.
        //
        // `crates/xpile/examples/README.md:20-23` publishes a `for … do …
        // done` loop with `\` continuations. Line-split, the fragments are
        // `sh: 1: Syntax error: end of file unexpected` — three offences
        // reported against a page that is CORRECT. The command is the whole
        // construct, so join it before judging it. Once joined, this one
        // contains `cargo run --example` and the existing screen claims it,
        // which is the right outcome by the taxonomy that was already there.
        let mut consumed = 0usize;
        while cmd.trim_end().ends_with('\\') || (starts_shell_block(&cmd) && !cmd.contains("done"))
        {
            let Some(next) = lines.get(idx + consumed + 1) else {
                break;
            };
            let t = next.trim();
            if t.starts_with("```") || t.starts_with("$ ") {
                break;
            }
            // A `\` continuation is one logical line and joins with a SPACE.
            // A new line inside a `for … do … done` body is a separate
            // statement and joins with a NEWLINE — joining it with a space
            // yields `echo "…" cargo run …`, which is a different command
            // and a syntax error, i.e. another false accusation.
            let was_continuation = cmd.trim_end().ends_with('\\');
            let joined = cmd.trim_end().trim_end_matches('\\').trim_end().to_string();
            cmd = if was_continuation {
                format!("{joined} {t}")
            } else {
                format!("{joined}\n{t}")
            };
            consumed += 1;
            if consumed > 20 {
                break;
            }
        }
        skip_until = idx + consumed;

        let mut transcript = Vec::new();
        for follow in &lines[idx + 1..] {
            let t = follow.trim();
            if t.starts_with("```") || t.starts_with("$ ") {
                break;
            }
            transcript.push(t.to_string());
        }

        out.push(PublishedCmd {
            file: rel(path),
            line: idx + 1,
            cmd: cmd.clone(),
            comment: trailing_comment(&cmd),
            transcript,
            fence: fence_id,
            chained: false,
            dollar,
        });
    }

    // Second pass: in a fence that HAS `$` lines, the bare lines are the
    // transcript of the command above them — executing them would run the
    // emitted Rust as if it were shell.
    let dollar_fences: BTreeSet<usize> = out.iter().filter(|c| c.dollar).map(|c| c.fence).collect();
    out.retain(|c| c.dollar || !dollar_fences.contains(&c.fence));

    // Third pass (PMAT-1514): a `cd` screens the REST OF ITS BLOCK, not just
    // its own line.
    //
    // The `cd ` screen's reason has always read "CHAINED_DIRCHANGE — depends on
    // a screened predecessor", and nothing implemented the *depends on* half:
    // only the `cd` line itself was screened, and every later line in the same
    // fence was executed from the wrong directory. A stated scope the code does
    // not implement is the defect class this file exists to catch, sitting
    // inside this file.
    //
    // It was live, and the corpus widening above exposed it.
    // `contracts/lean-models/README.md` publishes
    //     cd contracts/lean-models
    //     lake exe cache get
    // and the second line, run without the first, was reported as a page
    // publishing a succeeding command that exits 1 — a false accusation against
    // a correct document. Run WITH the `cd`, as a reader would, it exits 0 and
    // downloads 5.3 GB of Mathlib, which is its own reason a test must never
    // execute this block.
    let mut first_cd: BTreeMap<usize, usize> = BTreeMap::new();
    for c in &out {
        if bare_command(c).trim_start().starts_with("cd ") {
            let e = first_cd.entry(c.fence).or_insert(c.line);
            if c.line < *e {
                *e = c.line;
            }
        }
    }
    for c in &mut out {
        if first_cd.get(&c.fence).is_some_and(|at| c.line > *at) {
            c.chained = true;
        }
    }
    out
}

/// Does this line open a multi-line shell construct whose body follows?
///
/// Only the block openers that actually appear in this corpus — a broader
/// grammar would be guessing. Anything else is judged as the single line it is.
fn starts_shell_block(cmd: &str) -> bool {
    let t = cmd.trim_start();
    t.starts_with("for ") || t.starts_with("while ") || t.starts_with("until ")
}

/// The trailing `# …` of a command line, if the `#` is not inside quotes.
fn trailing_comment(line: &str) -> Option<String> {
    let mut in_s = false;
    let mut in_d = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '\'' if !in_d => in_s = !in_s,
            '"' if !in_s => in_d = !in_d,
            '#' if !in_s && !in_d && i > 0 => {
                return Some(line[i..].to_string());
            }
            _ => {}
        }
    }
    None
}

/// The command with its trailing comment removed — what actually runs.
fn bare_command(c: &PublishedCmd) -> String {
    match &c.comment {
        Some(cm) => c.cmd[..c.cmd.len() - cm.len()].trim_end().to_string(),
        None => c.cmd.clone(),
    }
}

// ---------------------------------------------------------------------------
// Screening — the DECLARED, LOGGED taxonomy
// ---------------------------------------------------------------------------

/// A screen: a substring that disqualifies a published command from execution,
/// and the reason it does. First match wins; the order is the file order.
///
/// ⛔ Every entry is a command that a test genuinely must not run. This is not
/// a place to park a command that merely fails.
const SCREENS: &[(&str, &str)] = &[
    // Mutates state outside the scratch dir.
    ("cargo install", "MUTATES_GLOBAL — writes ~/.cargo/bin"),
    (
        "cargo new",
        "MUTATES_GLOBAL — scaffolds into the caller's workspace",
    ),
    // Needs the network.
    ("git clone", "NETWORK"),
    ("cargo deny", "NETWORK — fetches the advisory database"),
    // Needs a checkout AND minutes of build time.
    (
        "cargo run --example",
        "NEEDS_CHECKOUT_SLOW — builds an example binary",
    ),
    ("cargo fmt", "NEEDS_CHECKOUT_SLOW"),
    ("cargo check", "NEEDS_CHECKOUT_SLOW"),
    ("cargo clippy", "NEEDS_CHECKOUT_SLOW"),
    ("cargo build", "NEEDS_CHECKOUT_SLOW"),
    // Tools this repository does not ship and CI does not install.
    ("pv lint", "NEEDS_EXTERNAL_TOOL — provable-contracts CLI"),
    ("wasmtime", "NEEDS_EXTERNAL_TOOL"),
    // Writes outside the scratch dir, or depends on a screened predecessor.
    (
        "--emit-crate /tmp/",
        "WRITES_OUTSIDE_SCRATCH — absolute output path",
    ),
    (
        "cd ",
        "CHAINED_DIRCHANGE — depends on a screened predecessor",
    ),
    // Illustrative roster entries whose operand the page never defines.
    (
        "model.py",
        "PLACEHOLDER_OPERAND — no page declares model.py",
    ),
    (
        "./project",
        "PLACEHOLDER_OPERAND — no page declares ./project",
    ),
];

/// A usage synopsis (`xpile transpile [OPTIONS] <INPUT>`) is a description of
/// the interface, not an invocation of it.
fn is_synopsis(cmd: &str) -> bool {
    for (i, ch) in cmd.char_indices() {
        if ch == '<' {
            // `<INPUT>` / `<PATH>` — a metavariable, not the `<` redirect.
            if cmd[i + 1..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
            {
                return true;
            }
        }
        if ch == '[' {
            // `[OPTIONS]`, `[--json]`, `[PATH]`, `[SUBCOMMAND]` — but NOT the
            // `[ … ]` test bracket, which is lowercase or an operator.
            let rest = &cmd[i + 1..];
            if rest.starts_with("--") || rest.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            {
                return true;
            }
        }
    }
    false
}

#[derive(Debug, PartialEq, Eq)]
enum Disposition {
    Execute,
    Screened(&'static str),
}

/// TOTAL: a command no screen claims is EXECUTED. There is no third bucket, so
/// a newly published command joins the executed set by default.
fn classify(cmd: &str) -> Disposition {
    if is_synopsis(cmd) {
        return Disposition::Screened("SYNOPSIS — a usage line, not an invocation");
    }
    for (pat, reason) in SCREENS {
        if cmd.contains(pat) {
            return Disposition::Screened(reason);
        }
    }
    // PMAT-1514: `SCREENS` matches CONTIGUOUS substrings, and a real invocation
    // interleaves flags. `crates/xpile/examples/README.md` publishes
    // `cargo run --quiet --example "$ex" -p xpile`, which the
    // `"cargo run --example"` pattern does not contain — so the command was
    // executed, failed for want of a `Cargo.toml`, and was reported as a page
    // publishing a broken command. The screen's INTENT is "building an example
    // needs a checkout and minutes"; that intent is about the two tokens, not
    // about their adjacency.
    if cmd.contains("cargo run") && cmd.contains("--example") {
        return Disposition::Screened("NEEDS_CHECKOUT_SLOW — builds an example binary");
    }
    Disposition::Execute
}

// ---------------------------------------------------------------------------
// Fixtures — resolved FROM the pages, not typed here
// ---------------------------------------------------------------------------

/// Input files the pages instruct the reader to create, keyed by filename.
///
/// Three conventions are in use and all three are live:
///
/// * prose BEFORE the block — `quickstart.md`: "Save the following as
///   `factorial.py`:"
/// * prose AFTER the block — `shell-roundtrip.md`: "Save this as `script.sh`."
/// * a first-line comment INSIDE the block — `README.md` and
///   `python-to-rust.md`: `# factorial.py`
///
/// Only `.py` and `.sh` are collected: those are the source languages xpile
/// consumes. A `toml` block captioned `# Cargo.toml` is a dependency snippet,
/// not a transpile input, and materialising it would change how `cargo`
/// behaves in the scratch dir.
fn declared_fixtures() -> BTreeMap<String, String> {
    declared_fixtures_with_conventions().0
}

/// The fixtures plus WHICH conventions actually resolved one, so a convention
/// that has silently stopped working is visible (PMAT-1515).
fn declared_fixtures_with_conventions() -> (BTreeMap<String, String>, BTreeSet<FixtureConvention>) {
    let mut out = BTreeMap::new();
    let mut conventions: BTreeSet<FixtureConvention> = BTreeSet::new();
    for page in corpus() {
        let text = std::fs::read_to_string(&page).expect("read corpus page");
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0usize;
        while i < lines.len() {
            let s = lines[i].trim();
            if !s.starts_with("```") {
                i += 1;
                continue;
            }
            let lang = s[3..].trim().to_string();
            let start = i;
            let mut end = i + 1;
            while end < lines.len() && !lines[end].trim().starts_with("```") {
                end += 1;
            }
            if matches!(lang.as_str(), "python" | "py" | "sh" | "shell") {
                let body: Vec<&str> = lines[start + 1..end.min(lines.len())].to_vec();
                if let Some((name, how)) = fixture_name(&lines, start, end, &body) {
                    conventions.insert(how);
                    out.entry(name).or_insert_with(|| {
                        let mut b = body.join("\n");
                        b.push('\n');
                        b
                    });
                }
            }
            i = end + 1;
        }
    }
    (out, conventions)
}

/// The filename a fenced block declares itself to be, under the three
/// conventions the corpus uses.
/// Which of the three declaration conventions resolved a fixture (PMAT-1515).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FixtureConvention {
    /// (c) a first-line `# name.py` comment inside the fence.
    FirstLineComment,
    /// (a) prose in the lines BEFORE the fence.
    ProseBefore,
    /// (b) prose in the lines AFTER the fence.
    ProseAfter,
}

/// The prose spellings that declare the following (or preceding) block a
/// fixture.
///
/// ⚠️ PMAT-1515 HOISTED THESE OUT OF `fixture_name`. They were three string
/// literals inline in the function body, and this file holds every OTHER
/// vocabulary to a liveness standard — `every_disclosure_exemption_literal_is_live`
/// reds on a stale `DISCLOSURE_MARKERS` entry, `detection_vocabularies_are_live_as_groups`
/// floors the checkout vocabularies. Being inline is precisely how a dead
/// literal escaped both: `"Save as"` matched **nothing** in the corpus — 0
/// occurrences across the pages, present in exactly one tracked file in the
/// whole repository, this test itself — and no gate could see it.
///
/// **A literal that is not in a named const is invisible to the liveness gate
/// standing next to it.**
const FIXTURE_DECLARATION_MARKERS: &[&str] = &["Save the following as", "Save this as"];

fn fixture_name(
    lines: &[&str],
    start: usize,
    end: usize,
    body: &[&str],
) -> Option<(String, FixtureConvention)> {
    // (c) first-line comment inside the block.
    if let Some(first) = body.first() {
        let t = first.trim();
        if let Some(rest) = t.strip_prefix('#') {
            if let Some(n) = as_fixture_filename(rest.trim()) {
                return Some((n, FixtureConvention::FirstLineComment));
            }
        }
    }
    // (a) prose in the three lines BEFORE the fence, (b) the three AFTER.
    let before = lines[start.saturating_sub(3)..start].join(" ");
    let after = lines[(end + 1).min(lines.len())..(end + 4).min(lines.len())].join(" ");
    for (window, how) in [
        (before, FixtureConvention::ProseBefore),
        (after, FixtureConvention::ProseAfter),
    ] {
        if !FIXTURE_DECLARATION_MARKERS
            .iter()
            .any(|m| window.contains(m))
        {
            continue;
        }
        for tok in window.split('`') {
            if let Some(n) = as_fixture_filename(tok) {
                return Some((n, how));
            }
        }
    }
    None
}

fn as_fixture_filename(tok: &str) -> Option<String> {
    let t = tok.trim().trim_end_matches([':', '.', ',']);
    if (t.ends_with(".py") || t.ends_with(".sh"))
        && !t.contains('/')
        && !t.contains(' ')
        && t.len() > 3
    {
        Some(t.to_string())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

/// A per-CALL unique scratch directory. Per-TEST is not enough: these tests run
/// on parallel threads and a shared directory gets wiped mid-run, which reads
/// exactly like a command failure (PMAT-1436, re-learned by PMAT-1473).
fn scratch(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join("xpile-book-cmd").join(format!(
        "{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Run one published command in `dir`. `xpile` resolves to
/// `env!("CARGO_BIN_EXE_xpile")` — the binary cargo rebuilt for this test —
/// so a stale artefact cannot masquerade as a documentation defect.
fn run_published(cmd: &str, dir: &Path) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_xpile");
    let script = if let Some(rest) = cmd.strip_prefix("xpile ") {
        format!("{bin} {rest}")
    } else if cmd.trim() == "xpile" {
        bin.to_string()
    } else {
        cmd.to_string()
    };
    Command::new("sh")
        .arg("-c")
        .arg(&script)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("spawn `sh -c` for `{cmd}`: {e}"))
}

/// The exit status the page itself publishes for a command: a transcript whose
/// first non-blank line begins with `Error:` is a REFUSAL the page is showing
/// off on purpose. DERIVED from the page, never listed here.
fn page_expects_success(c: &PublishedCmd) -> bool {
    !c.transcript
        .iter()
        .find(|l| !l.is_empty())
        .is_some_and(|l| l.starts_with("Error:"))
}

/// Where a published command is run.
///
/// Most commands run in an empty scratch dir, which is the environment an
/// installed-from-crates.io reader has. But three of the four analysis
/// subcommands are *documented* as needing a checkout and genuinely do, and
/// `ls contracts/*.yaml` names a repo-relative path — running those in a
/// scratch dir would report the page as broken when it is the harness that is
/// wrong. Both membership tests are DERIVED, never listed: one from
/// [`measured_checkout_freedom`], the other from whether the path's first
/// segment exists at the repo root.
///
/// The lane is LOGGED by [`checkout_lane_is_logged_and_bounded`], because a
/// command silently moved into it escapes the "works for an installed reader"
/// half of property A.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Lane {
    Scratch,
    Checkout,
}

fn lane(bare: &str, measured: &BTreeMap<String, bool>) -> Lane {
    if let Some(sub) = bare
        .strip_prefix("xpile ")
        .and_then(|r| r.split_whitespace().next())
    {
        if measured.get(sub) == Some(&false) {
            return Lane::Checkout;
        }
    }
    for tok in bare.split_whitespace() {
        if tok.starts_with('-') || !tok.contains('/') {
            continue;
        }
        let first = tok.split('/').next().unwrap_or("");
        if !first.is_empty() && repo_root().join(first).is_dir() {
            return Lane::Checkout;
        }
    }
    Lane::Scratch
}

fn all_commands() -> Vec<PublishedCmd> {
    corpus()
        .iter()
        .flat_map(|p| published_commands(p))
        .collect()
}

fn executed_commands() -> Vec<PublishedCmd> {
    all_commands()
        .into_iter()
        .filter(|c| !c.chained && classify(&bare_command(c)) == Disposition::Execute)
        .collect()
}

// ===========================================================================
// PROPERTY A — every published command's exit status matches its own page
// ===========================================================================

#[test]
fn every_published_command_exits_as_its_page_publishes() {
    let cmds = executed_commands();

    // ⛔ A rule quantified over a set it also computes must floor THAT set
    // itself. Borrowing `executed_set_has_a_floor`'s floor would let a future
    // edit separate them, leaving this rule silently ranging over nothing
    // (PMAT-1510).
    assert!(
        cmds.len() >= 30,
        "only {} published command(s) survive extraction+screening — this rule \
         would pass by executing almost nothing",
        cmds.len()
    );

    // Commands in one fence run in ONE scratch dir, in order: `quickstart.md`
    // writes `factorial.rs` on line 48 and compiles it on line 49. Running each
    // in a fresh directory reports line 49 as broken — a harness error wearing a
    // defect's clothes, which is the exact trap this file exists to close.
    let mut by_fence: BTreeMap<(String, usize), Vec<PublishedCmd>> = BTreeMap::new();
    for c in cmds {
        by_fence
            .entry((c.file.clone(), c.fence))
            .or_default()
            .push(c);
    }

    let fixtures = declared_fixtures();
    let measured = measured_checkout_freedom();
    let root = repo_root();
    let mut offences = Vec::new();

    for ((file, fence), group) in &by_fence {
        let dir = scratch(&format!("{}-{fence}", file.replace(['/', '.'], "_")));
        for (name, body) in &fixtures {
            std::fs::write(dir.join(name), body).expect("seed fixture");
        }
        for c in group {
            let bare = bare_command(c);
            let cwd = match lane(&bare, &measured) {
                Lane::Scratch => dir.as_path(),
                Lane::Checkout => root.as_path(),
            };
            let out = run_published(&bare, cwd);
            let ok = out.status.success();
            if ok != page_expects_success(c) {
                let head = String::from_utf8_lossy(&out.stderr)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(180)
                    .collect::<String>();
                offences.push(format!(
                    "{}:{} published as {} but exited {:?}\n      $ {}\n      {}",
                    c.file,
                    c.line,
                    if page_expects_success(c) {
                        "SUCCEEDING"
                    } else {
                        "an Error: transcript"
                    },
                    out.status.code(),
                    bare,
                    head
                ));
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    assert!(
        offences.is_empty(),
        "XPILE-BOOKTRANSCRIPT-001 arm (a): {} published command(s) do not exit the way \
         their own page says they do. A `$` line in a reader-facing page is an \
         instruction, not decoration — either fix the command or publish the \
         `Error:` transcript it actually produces.\n  {}",
        offences.len(),
        offences.join("\n  ")
    );
}

/// NON-VACUITY for Property A: the executor must REPORT a failing command.
///
/// This drives the real [`run_published`] on a constructed command known to
/// exit non-zero, through the real [`page_expects_success`]. Without it an
/// executor that swallowed the status — or a `page_expects_success` that
/// returned `false` for everything — would read green forever over an entirely
/// broken book.
#[test]
fn a_command_known_to_fail_is_reported_as_failing() {
    let dir = scratch("nonvacuity");

    let failing = PublishedCmd {
        file: "constructed".into(),
        line: 0,
        cmd: "xpile transpile definitely-not-a-real-file.py".into(),
        comment: None,
        transcript: vec!["// xpile-generated".into()],
        fence: 0,
        chained: false,
        dollar: true,
    };
    let out = run_published(&bare_command(&failing), &dir);
    assert!(
        !out.status.success(),
        "the control command must exit non-zero, got {:?}",
        out.status.code()
    );
    assert!(
        page_expects_success(&failing),
        "a transcript that is not an `Error:` block must expect success"
    );
    // …so the two together are an OFFENCE, which is what the rule detects.

    let refusal = PublishedCmd {
        transcript: vec!["Error: backend `rust` failed".into()],
        ..failing.clone()
    };
    assert!(
        !page_expects_success(&refusal),
        "an `Error:` transcript must expect a non-zero exit — otherwise \
         shell-roundtrip.md:60's deliberate refusal reds the gate"
    );

    let succeeding = run_published("xpile info", &dir);
    assert!(
        succeeding.status.success(),
        "`xpile info` must exit 0 in a scratch dir, else the executor itself is broken: {}",
        String::from_utf8_lossy(&succeeding.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ===========================================================================
// PROPERTY B — a page's checkout precondition must match the measurement
// ===========================================================================

/// ⚠️ **DETECT THE PROPERTY, NOT THE LITERAL.** The first draft of this rule was
/// a list of fixed phrases including `"is required"`, and the live defect it
/// was written for said *"A source checkout is **also** required"* — three
/// characters away, and the red half came back GREEN. A phrase list is a needle
/// set and it goes stale against the prose it hunts (PMAT-1506). What a
/// checkout-requirement claim actually IS: a **checkout noun** and a
/// **requirement verb** in the same sentence.
///
/// These are DETECTION vocabularies, so breadth is the point: an individual
/// literal that matches nothing today costs nothing and catches tomorrow's
/// phrasing. [`detection_vocabularies_are_live_as_groups`] holds them to the
/// weaker but checkable standard — each GROUP must match live prose, and the
/// per-literal liveness is printed so a wholly dead group is visible. The
/// EXEMPTION vocabulary below is held to the stricter one.
const CHECKOUT_NOUNS: &[&str] = &["checkout", "source tree", "a clone", "the clone"];
const REQUIREMENT_VERBS: &[&str] = &["requir", "need", "must", "only work"];

/// Conditional spellings that carry the requirement without a verb —
/// *"if you're in a repo with contracts/"* asserts the precondition by naming
/// the environment rather than by demanding it.
const NEEDS_CHECKOUT_PHRASES: &[&str] = &[
    "if you're in a repo",
    "if you are in a repo",
    "in an xpile checkout",
    "from a checkout",
    "from an xpile checkout",
    "run those from",
    "run it from",
];

/// Phrases by which a page claims a command needs NO checkout.
const WORKS_ANYWHERE_MARKERS: &[&str] = &[
    "anywhere",
    "any directory",
    "no checkout",
    "without a checkout",
];

/// Every subcommand `xpile --help` advertises. DERIVED, so a new subcommand
/// joins the rule for free and nothing here can name one that no longer exists.
fn advertised_subcommands() -> Vec<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("--help")
        .output()
        .expect("xpile --help");
    assert!(out.status.success(), "`xpile --help` must exit 0");
    let text = String::from_utf8_lossy(&out.stdout);
    let mut in_cmds = false;
    let mut subs = Vec::new();
    for line in text.lines() {
        if line.starts_with("Commands:") {
            in_cmds = true;
            continue;
        }
        if in_cmds {
            if line.trim().is_empty() || !line.starts_with("  ") {
                if !line.starts_with("  ") && !line.trim().is_empty() {
                    break;
                }
                continue;
            }
            // Continuation lines of a long description are indented further.
            if line.starts_with("    ") {
                continue;
            }
            if let Some(name) = line.split_whitespace().next() {
                subs.push(name.to_string());
            }
        }
    }
    subs
}

/// `true` when the subcommand exits 0 with NO checkout in sight.
///
/// Subcommands that exit 2 are excluded by the caller: clap's usage exit means
/// a required operand is missing, which is a different claim entirely.
fn probe_bare(sub: &str) -> Option<bool> {
    let dir = scratch(&format!("probe-{sub}"));
    let out = run_published(&format!("xpile {sub}"), &dir);
    let code = out.status.code();
    let _ = std::fs::remove_dir_all(&dir);
    match code {
        Some(2) => None, // required operand missing — not a checkout claim
        Some(0) => Some(true),
        _ => Some(false),
    }
}

/// The measured split: subcommand → works-without-a-checkout.
fn measured_checkout_freedom() -> BTreeMap<String, bool> {
    advertised_subcommands()
        .into_iter()
        .filter_map(|s| probe_bare(&s).map(|b| (s, b)))
        .collect()
}

/// Sentences and command-line comments in the corpus that make a
/// checkout-precondition claim about a named subcommand.
///
/// Two arms, because the claim is published in two shapes:
///
/// * a trailing comment on a `$ xpile <sub>` line — the subject is unambiguous;
/// * a prose sentence naming one or more backticked `xpile <sub>` commands.
fn precondition_claims() -> Vec<(String, usize, String, String, bool)> {
    let mut claims = Vec::new();

    // Arm B1 — trailing comments on published `xpile` commands.
    for c in all_commands() {
        let Some(comment) = &c.comment else { continue };
        let low = comment.to_lowercase();
        let Some(sub) = bare_command(&c)
            .strip_prefix("xpile ")
            .and_then(|r| r.split_whitespace().next())
            .map(str::to_string)
        else {
            continue;
        };
        if let Some(needs) = verdict_of(&low) {
            claims.push((c.file.clone(), c.line, sub, comment.clone(), needs));
        }
    }

    // Arm B2 — prose sentences naming backticked `xpile <sub>` commands.
    for page in corpus() {
        let text = std::fs::read_to_string(&page).expect("read corpus page");
        for unit in prose_units(&text) {
            for sentence in unit.split(". ") {
                let low = sentence.to_lowercase();
                let Some(needs) = verdict_of(&low) else {
                    continue;
                };
                for tok in sentence.split('`') {
                    let Some(rest) = tok.trim().strip_prefix("xpile ") else {
                        continue;
                    };
                    let sub = rest.split_whitespace().next().unwrap_or("").to_string();
                    if sub.is_empty() || sub.starts_with('-') {
                        continue;
                    }
                    let line = line_of(&text, sentence.split_whitespace().next().unwrap_or(""));
                    claims.push((
                        rel(&page),
                        line,
                        sub,
                        sentence.trim().chars().take(140).collect::<String>(),
                        needs,
                    ));
                }
            }
        }
    }
    claims
}

/// Split a page into the units a claim can live in.
///
/// ⚠️ **The joining rule cuts BOTH ways.** Markdown prose is hard-wrapped, so a
/// line-wise scan of it is wrong by default (PMAT-1510) and consecutive prose
/// lines must be joined. But a **table row is already one semantic unit**, and
/// joining it to its neighbours is equally wrong: it welds the header cell
/// *"outside a checkout | what it needs"* onto the first data row, which names
/// `xpile diamond` — a false positive on a correct table, found by running this
/// against the very table this slice wrote.
///
/// Fenced blocks are excluded: their contents are commands (arm B1's subject),
/// not prose.
fn prose_units(text: &str) -> Vec<String> {
    let mut units = Vec::new();
    let mut buf: Vec<&str> = Vec::new();
    let mut in_fence = false;
    let flush = |buf: &mut Vec<&str>, units: &mut Vec<String>| {
        if !buf.is_empty() {
            units.push(buf.join(" "));
            buf.clear();
        }
    };
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            in_fence = !in_fence;
            flush(&mut buf, &mut units);
            continue;
        }
        if in_fence {
            continue;
        }
        if t.is_empty() {
            flush(&mut buf, &mut units);
            continue;
        }
        if t.starts_with('|') {
            flush(&mut buf, &mut units);
            units.push(t.to_string());
            continue;
        }
        buf.push(t);
    }
    flush(&mut buf, &mut units);
    units
}

/// Arm B3 — a markdown table row of the shape
/// `| `xpile <sub>` | … exits <N> … |` states the verdict directly, and the
/// row is the most authoritative form the claim takes. Without this arm the
/// per-command table `installation.md` publishes would be prose that no rule
/// can see, which is the shape this whole file exists to close.
fn tabulated_exit_claims() -> Vec<(String, usize, String, bool)> {
    let mut out = Vec::new();
    for page in corpus() {
        let text = std::fs::read_to_string(&page).expect("read corpus page");
        for (i, line) in text.lines().enumerate() {
            let t = line.trim();
            if !t.starts_with('|') {
                continue;
            }
            let cells: Vec<&str> = t.trim_matches('|').split('|').map(str::trim).collect();
            let Some(first) = cells.first() else { continue };
            let Some(sub) = first
                .trim_matches('`')
                .strip_prefix("xpile ")
                .map(|s| s.trim().to_string())
            else {
                continue;
            };
            let rest = cells[1..].join(" ").to_lowercase();
            let zero = rest.contains("exits 0");
            let nonzero = rest.contains("exits 1") || rest.contains("exits non-zero");
            if zero ^ nonzero {
                out.push((rel(&page), i + 1, sub, zero));
            }
        }
    }
    out
}

/// ⛔ THE DISCLOSURE EXEMPTION, and it is the one that will red a CORRECT page.
///
/// A sentence that NARRATES a past error — "through v0.1.618 this page said a
/// checkout was required" — restates the falsehood in order to record its
/// repair. Gating those demands the project stop narrating its own fixes, which
/// is exactly what whole-document scoping did to PMAT-1473 (eight honest
/// narrative arcs flagged) and what PMAT-1477 correctly declined to do to the
/// append-only ledgers. The markers are POSITIVE and past-tense: a claim earns
/// exemption by dating itself, never by carrying a negation.
///
/// [`the_disclosure_exemption_fires_in_both_directions`] is the mandatory
/// control — an exemption nobody has seen fire may match everything.
/// ⛔ An EXEMPTION is held to a stricter standard than a detector: every
/// literal here must be LIVE in the corpus, because an escape hatch nobody has
/// seen fire may be matching everything. `"used to say"` and `"previously
/// said"` were in the first draft and matched nothing; they are deleted rather
/// than kept as anticipatory (PMAT-1510 — delete a check that cannot fire, do
/// not replace it with another one).
/// [`every_disclosure_exemption_literal_is_live`] enforces this.
const DISCLOSURE_MARKERS: &[&str] = &[
    "through v0.1.",
    "this page said",
    "this page came to claim",
    "was false",
    "wrong about",
];

/// `Some(true)` = "needs a checkout", `Some(false)` = "works anywhere",
/// `None` = the text makes no precondition claim. A text carrying BOTH
/// vocabularies is ambiguous and makes no claim; a text narrating a past
/// correction is exempt.
fn verdict_of(low: &str) -> Option<bool> {
    if DISCLOSURE_MARKERS.iter().any(|m| low.contains(m)) {
        return None;
    }
    let needs = NEEDS_CHECKOUT_PHRASES.iter().any(|m| low.contains(m))
        || (CHECKOUT_NOUNS.iter().any(|n| low.contains(n))
            && REQUIREMENT_VERBS.iter().any(|v| low.contains(v)));
    let anywhere = WORKS_ANYWHERE_MARKERS.iter().any(|m| low.contains(m));
    match (needs, anywhere) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    }
}

fn line_of(text: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    text.lines()
        .position(|l| l.contains(needle))
        .map(|i| i + 1)
        .unwrap_or(0)
}

#[test]
fn no_page_may_claim_a_checkout_precondition_the_binary_does_not_have() {
    let measured = measured_checkout_freedom();
    let claims = precondition_claims();

    // Own floor, not `both_precondition_vocabularies_match_live_prose`'s.
    assert!(
        claims.len() >= 6,
        "only {} precondition claim(s) extracted — this rule would pass by \
         checking almost nothing",
        claims.len()
    );

    let mut offences = Vec::new();
    for (file, line, sub, quote, claims_needs_checkout) in &claims {
        let Some(&works_anywhere) = measured.get(sub) else {
            continue; // not a probeable subcommand (needs an operand), or not a subcommand
        };
        let actually_needs_checkout = !works_anywhere;
        if *claims_needs_checkout != actually_needs_checkout {
            offences.push(format!(
                "{file}:{line} says `xpile {sub}` {} — MEASURED in an empty directory it {}\n      \"{quote}\"",
                if *claims_needs_checkout { "NEEDS a checkout" } else { "works ANYWHERE" },
                if actually_needs_checkout { "exits non-zero" } else { "exits 0" },
            ));
        }
    }

    // Arm B3 — tabulated exit-status rows.
    let tabulated = tabulated_exit_claims();
    for (file, line, sub, claims_exit_zero) in &tabulated {
        let Some(&works_anywhere) = measured.get(sub) else {
            continue;
        };
        if *claims_exit_zero != works_anywhere {
            offences.push(format!(
                "{file}:{line} tabulates `xpile {sub}` as exiting {} outside a \
                 checkout — MEASURED it exits {}",
                if *claims_exit_zero { "0" } else { "non-zero" },
                if works_anywhere { "0" } else { "non-zero" },
            ));
        }
    }
    assert!(
        tabulated.len() >= 3,
        "arm B3 found {} tabulated exit-status row(s) — installation.md \
         publishes a four-row per-command table, so this arm is quantified over \
         nothing and the table is ungated",
        tabulated.len()
    );

    assert!(
        offences.is_empty(),
        "XPILE-BOOKTRANSCRIPT-001 arm (a), property B: {} page(s) publish a \
         checkout precondition that the shipped binary contradicts. The roster \
         is DERIVED from `xpile --help` and each verdict is MEASURED by running \
         the subcommand in an empty scratch dir, so this fires equally when the \
         prose rots and when the binary regresses.\n  {}",
        offences.len(),
        offences.join("\n  ")
    );
}

// ===========================================================================
// Non-vacuity floors — one per arm, per PMAT-1507
// ===========================================================================

#[test]
fn corpus_is_derived_and_floored() {
    let files = corpus();
    assert!(
        files.len() >= 15,
        "corpus collapsed to {} page(s) — `git ls-files {:?}` matched almost \
         nothing and every rule in this file would pass by iterating nothing",
        files.len(),
        CORPUS_PATHSPECS
    );
    let names: BTreeSet<String> = files.iter().map(|p| rel(p)).collect();
    for anchor in [
        "README.md",
        "book/src/quickstart.md",
        "book/src/installation.md",
    ] {
        assert!(
            names.contains(anchor),
            "{anchor} is not in the derived corpus — the pathspecs are wrong"
        );
    }

    // PMAT-1514: the corpus must contain EVERY tracked README, re-derived here
    // independently of `CORPUS_PATHSPECS`.
    //
    // Without this the widening is unpinned: reverting the pathspec list to
    // `["README.md", "book/src"]` drops four reader-facing pages and every
    // property in this file still passes, which is how they were missing in the
    // first place. A rule and the constant it depends on must be tied together
    // by something that fails when they part company.
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root())
        .args(["ls-files", "*README.md"])
        .output()
        .expect("git ls-files");
    let tracked: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .filter(|p| p.matches('/').count() <= 2)
        .collect();
    assert!(
        tracked.len() >= 4,
        "only {} tracked README(s) found; the independent re-derivation broke and \
         this floor is asserting nothing",
        tracked.len()
    );
    let missing: Vec<&String> = tracked.iter().filter(|p| !names.contains(*p)).collect();
    assert!(
        missing.is_empty(),
        "these tracked READMEs are reader-facing — two of them ship INSIDE the \
         packaged crate — but the corpus does not reach them, so nothing executes \
         the commands they publish: {missing:?}"
    );
}

#[test]
fn executed_set_has_a_floor() {
    let all = all_commands();
    let exec = executed_commands();
    assert!(
        all.len() >= 60,
        "only {} published command(s) extracted — the fence parser matched \
         almost nothing (76 were live on 2026-07-31)",
        all.len()
    );
    assert!(
        exec.len() >= 30,
        "only {} of {} published commands survive screening. The screens are \
         supposed to be a short, declared list of things a test must not run; \
         if they now swallow most of the corpus this gate has become decoration",
        exec.len(),
        all.len()
    );
    // The two anchor pages must each contribute executed commands, per ARM —
    // a union floor lets one page's arm die unnoticed (PMAT-1507).
    for anchor in ["README.md", "book/src/quickstart.md"] {
        let n = exec.iter().filter(|c| c.file == anchor).count();
        assert!(n >= 3, "{anchor} contributes only {n} executed command(s)");
    }
    // …and the deliberate-refusal arm must stay live, or `page_expects_success`
    // is never exercised in its `false` branch over real corpus text.
    let refusals = exec.iter().filter(|c| !page_expects_success(c)).count();
    assert!(
        refusals >= 1,
        "no published `Error:` transcript survives extraction — \
         book/src/tutorials/shell-roundtrip.md:60 is the live instance"
    );
}

#[test]
fn screened_set_is_logged_not_silent() {
    let mut by_reason: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for c in all_commands() {
        if let Disposition::Screened(reason) = classify(&bare_command(&c)) {
            by_reason
                .entry(reason)
                .or_default()
                .push(format!("{}:{}  $ {}", c.file, c.line, c.cmd));
        }
    }
    let total: usize = by_reason.values().map(Vec::len).sum();
    eprintln!(
        "XPILE-BOOKTRANSCRIPT-001 arm (a): {total} published command(s) SCREENED, \
         by declared reason:"
    );
    for (reason, sites) in &by_reason {
        eprintln!("  [{}] x{}", reason, sites.len());
        for s in sites {
            eprintln!("      {s}");
        }
    }
    assert!(
        total >= 1,
        "no command screened at all — the taxonomy matches nothing, which means \
         either the corpus is empty or `classify` is broken"
    );
    // Each declared screen family must still match something. A screen that
    // matches nothing is a rule nobody has seen fire, and it rots silently.
    for family in [
        "MUTATES_GLOBAL",
        "NETWORK",
        "NEEDS_CHECKOUT_SLOW",
        "NEEDS_EXTERNAL_TOOL",
        "SYNOPSIS",
    ] {
        assert!(
            by_reason.keys().any(|r| r.contains(family)),
            "screen family {family} matched no published command — either the \
             corpus changed or the pattern is dead"
        );
    }
}

/// The checkout lane is the one escape hatch in property A, so it is logged and
/// floored in BOTH directions: it must stay a minority, and it must not be
/// empty (an empty lane means `lane()` has stopped resolving and four commands
/// are being run in the wrong environment).
#[test]
fn checkout_lane_is_logged_and_bounded() {
    let measured = measured_checkout_freedom();
    let exec = executed_commands();
    let (checkout, scratch_lane): (Vec<_>, Vec<_>) = exec
        .iter()
        .partition(|c| lane(&bare_command(c), &measured) == Lane::Checkout);

    eprintln!(
        "XPILE-BOOKTRANSCRIPT-001 arm (a): {} executed command(s) run in the \
         CHECKOUT lane (the rest run in an empty scratch dir):",
        checkout.len()
    );
    for c in &checkout {
        eprintln!("      {}:{}  $ {}", c.file, c.line, c.cmd);
    }

    assert!(
        !checkout.is_empty(),
        "the checkout lane resolved to nothing — `xpile quorum` is measured as \
         needing a checkout and three pages publish it, so this lane cannot be \
         empty unless `lane()` is broken"
    );
    assert!(
        checkout.len() * 3 <= scratch_lane.len(),
        "{} of {} executed commands are in the checkout lane. That lane skips \
         the 'works for an installed reader' half of property A, so it must \
         stay a small minority — if it has grown, something is being routed \
         there to make it pass",
        checkout.len(),
        exec.len()
    );
}

#[test]
fn fixture_resolver_is_non_vacuous() {
    let (fx, conventions) = declared_fixtures_with_conventions();

    // PMAT-1515 — THIS IS THE ASSERTION THE COMMENT ALWAYS PROMISED.
    //
    // The comment below has always read "All three declaration conventions must
    // be live", and the assertions under it could only ever detect the death of
    // ONE. `factorial.py` is declared by convention (a) AND convention (c), so
    // either alone satisfies `contains_key("factorial.py")`; an audit killed (a)
    // and then (c) and the file stayed 16/16 green both times, while the number
    // of resolved declarations silently fell from 5 to 2 and `factorial.py`'s
    // CONTENT changed which page it came from.
    //
    // Detecting that needs the resolver to report WHICH convention answered,
    // which is why `fixture_name` now returns one.
    for how in [
        FixtureConvention::FirstLineComment,
        FixtureConvention::ProseBefore,
        FixtureConvention::ProseAfter,
    ] {
        assert!(
            conventions.contains(&how),
            "no fixture in the corpus is declared by {how:?} — that convention has \
             gone dead and the resolver has degraded to the ones that still work. \
             Live conventions: {conventions:?}"
        );
    }

    // All three declaration conventions must be live, or the resolver has
    // silently degraded to whichever one still works.
    assert!(
        fx.contains_key("factorial.py"),
        "factorial.py was not resolved out of any page — README.md declares it \
         with a first-line `# factorial.py` comment and quickstart.md with \
         \"Save the following as `factorial.py`:\". Resolved: {:?}",
        fx.keys().collect::<Vec<_>>()
    );
    assert!(
        fx.contains_key("script.sh"),
        "script.sh was not resolved — shell-roundtrip.md declares it with prose \
         AFTER the block (\"Save this as `script.sh`.\"). Resolved: {:?}",
        fx.keys().collect::<Vec<_>>()
    );
    assert!(
        fx["factorial.py"].contains("def factorial"),
        "the resolved factorial.py is not the page's Python block"
    );
    assert!(
        fx["script.sh"].contains("mkdir -p"),
        "the resolved script.sh is not the page's shell block"
    );
    // And the filter must REJECT a non-input block, or a `# Cargo.toml`
    // dependency snippet lands in the scratch dir and changes how cargo behaves.
    assert_eq!(
        as_fixture_filename("Cargo.toml"),
        None,
        "only .py/.sh blocks are transpile inputs"
    );
    assert_eq!(
        as_fixture_filename("factorial.py"),
        Some("factorial.py".into())
    );
}

#[test]
fn subcommand_roster_is_non_trivial() {
    let subs = advertised_subcommands();
    assert!(
        subs.len() >= 6,
        "`xpile --help` parsed to {} subcommand(s) — the Commands: block parser \
         is broken and property B would quantify over nothing: {subs:?}",
        subs.len()
    );
    for anchor in ["diamond", "quorum", "info"] {
        assert!(
            subs.iter().any(|s| s == anchor),
            "`{anchor}` missing from the parsed roster {subs:?}"
        );
    }
    let measured = measured_checkout_freedom();
    let free = measured.values().filter(|v| **v).count();
    let bound = measured.values().filter(|v| !**v).count();
    assert!(
        free >= 1 && bound >= 1,
        "the measured split is degenerate ({free} work anywhere, {bound} need a \
         checkout) — a probe that returns one verdict for everything makes \
         property B unfalsifiable in one direction. Measured: {measured:?}"
    );
    // The two anchors of the defect this arm found, asserted as a MEASUREMENT
    // rather than as a pinned expectation of the prose.
    assert_eq!(
        measured.get("diamond"),
        Some(&true),
        "`xpile diamond` no longer works outside a checkout. That is a real \
         behaviour change, not a test bug: README.md publishes \"works \
         anywhere\" and `xpile quorum`'s own error text says so too. Update \
         BOTH, then this assertion."
    );
    assert_eq!(
        measured.get("quorum"),
        Some(&false),
        "`xpile quorum` now works outside a checkout — README.md:291 and \
         installation.md still say it needs one"
    );
}

#[test]
fn both_precondition_vocabularies_match_live_prose() {
    let claims = precondition_claims();
    let needs = claims.iter().filter(|c| c.4).count();
    let anywhere = claims.iter().filter(|c| !c.4).count();
    eprintln!(
        "XPILE-BOOKTRANSCRIPT-001 property B: {} precondition claim(s):",
        claims.len()
    );
    for (f, l, sub, quote, n) in &claims {
        eprintln!(
            "  {f}:{l} `xpile {sub}` → {}  \"{quote}\"",
            if *n {
                "NEEDS CHECKOUT"
            } else {
                "works anywhere"
            }
        );
    }
    assert!(
        needs >= 1,
        "no live sentence matches NEEDS_CHECKOUT_MARKERS — the needle set has \
         gone stale against the prose and half of property B can never fire \
         (PMAT-1506's shape: an antecedent the corpus does not contain)"
    );
    assert!(
        anywhere >= 1,
        "no live sentence matches WORKS_ANYWHERE_MARKERS — the other half of \
         property B can never fire"
    );
    // The claims must resolve onto PROBED subcommands, or the rule compares
    // nothing: a vocabulary that only ever matches `transpile`/`hybrid` (both
    // excluded by the exit-2 screen) is a rule quantified over the empty set.
    let measured = measured_checkout_freedom();
    let resolved = claims
        .iter()
        .filter(|c| measured.contains_key(&c.2))
        .count();
    assert!(
        resolved >= 3,
        "only {resolved} precondition claim(s) name a probeable subcommand — \
         property B is quantified over almost nothing"
    );
}

/// Lowercased text of the whole corpus, for the vocabulary-liveness rules.
fn corpus_blob() -> String {
    corpus()
        .iter()
        .map(|p| {
            std::fs::read_to_string(p)
                .expect("read corpus page")
                .to_lowercase()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Each DETECTION group must match live prose. A group that matches nothing is
/// a rule quantified over the empty set no matter how many literals it holds.
#[test]
fn detection_vocabularies_are_live_as_groups() {
    let blob = corpus_blob();
    for (name, group) in [
        ("CHECKOUT_NOUNS", CHECKOUT_NOUNS),
        ("REQUIREMENT_VERBS", REQUIREMENT_VERBS),
        ("NEEDS_CHECKOUT_PHRASES", NEEDS_CHECKOUT_PHRASES),
        ("WORKS_ANYWHERE_MARKERS", WORKS_ANYWHERE_MARKERS),
    ] {
        let live: Vec<&&str> = group.iter().filter(|m| blob.contains(**m)).collect();
        eprintln!(
            "{name}: {}/{} literal(s) live — {live:?}",
            live.len(),
            group.len()
        );
        assert!(
            !live.is_empty(),
            "{name} matches nothing in the live corpus: every literal has gone \
             stale and the half of property B it feeds can never fire"
        );
    }
}

/// The stricter rule for the EXEMPTION vocabulary: every literal must be live.
#[test]
fn every_disclosure_exemption_literal_is_live() {
    let blob = corpus_blob();
    let dead: Vec<&&str> = DISCLOSURE_MARKERS
        .iter()
        .filter(|m| !blob.contains(**m))
        .collect();
    assert!(
        dead.is_empty(),
        "{dead:?} exempt sentences from property B but match nothing in the \
         corpus. An exemption nobody has seen fire may match everything — \
         delete it rather than carrying it as anticipatory"
    );
}

/// PMAT-1515 — the fixture-declaration spellings are held to the same standard,
/// which they escaped for as long as they were inline literals.
///
/// `"Save as"` was one of them and matched **nothing**: zero occurrences across
/// the corpus, and present in exactly one tracked file in the repository — this
/// test. It is deleted rather than carried, because a matcher nobody has seen
/// fire is indistinguishable from one that fires on everything.
#[test]
fn every_fixture_declaration_marker_is_live() {
    // `corpus_blob` lower-cases, and these markers are capitalised because
    // `fixture_name` matches the RAW page text. Comparing them without
    // lower-casing reports every marker dead — which is what the first draft of
    // this test did, and it would have "proved" a defect that is not there.
    let blob = corpus_blob();
    let dead: Vec<&&str> = FIXTURE_DECLARATION_MARKERS
        .iter()
        .filter(|m| !blob.contains(&m.to_lowercase()))
        .collect();
    assert!(
        dead.is_empty(),
        "{dead:?} declare a fenced block to be a fixture but match nothing in the \
         corpus — delete them rather than carrying them as anticipatory. This is \
         the gate the inline spelling escaped: `\"Save as\"` was dead on arrival \
         and nothing could see it (PMAT-1515)"
    );
    assert!(
        !FIXTURE_DECLARATION_MARKERS.is_empty(),
        "the marker table is empty, so the prose conventions resolve nothing and \
         this property asserts nothing"
    );
}

/// MANDATORY CONTROL for [`DISCLOSURE_MARKERS`]: the exemption must swallow a
/// dated narration of a past error AND must NOT swallow the live claim that
/// carries the same words. Without this, an exemption that matched everything
/// would make property B unfalsifiable and nothing would notice.
#[test]
fn the_disclosure_exemption_fires_in_both_directions() {
    // A live claim — must still be read as a claim.
    assert_eq!(
        verdict_of("run those from a checkout, or point them at one with --roadmap"),
        Some(true)
    );
    assert_eq!(
        verdict_of("xpile diamond reports on the release you installed from any directory"),
        Some(false)
    );
    // The SAME words, dated as a past error — must be exempt.
    assert_eq!(
        verdict_of("through v0.1.618 this page said a checkout was required for all four"),
        None,
        "a dated narration of a past error must not be gated as a live claim"
    );
    assert_eq!(
        verdict_of("wrong about xpile diamond, which works anywhere"),
        None
    );
    // Ambiguity: both vocabularies present is not a claim either.
    assert_eq!(
        verdict_of("it needs a checkout, except when it works anywhere"),
        None
    );
    // And a sentence with neither vocabulary is not a claim.
    assert_eq!(
        verdict_of("xpile diamond prints a per-contract table"),
        None
    );
}

/// The synopsis screen is the one most likely to swallow real invocations, so
/// it gets a two-way control: metavariable spellings the corpus actually uses
/// must screen, and every real invocation shape must NOT.
#[test]
fn synopsis_screen_fires_in_both_directions() {
    for syn in [
        "xpile transpile [OPTIONS] <INPUT>",
        "xpile hybrid [OPTIONS] <PATH>",
        "xpile diamond [--contracts-dir <DIR>] [--json]",
        "xpile help [SUBCOMMAND]",
        "xpile audit <PATH>",
    ] {
        assert!(is_synopsis(syn), "`{syn}` must screen as a synopsis");
    }
    for real in [
        "xpile transpile factorial.py",
        "xpile transpile factorial.py --target lean",
        "xpile diamond",
        "xpile info",
        "rustc -O factorial.rs --crate-type lib --emit=metadata --out-dir .",
        "git clone https://github.com/paiml/xpile && cd xpile",
    ] {
        assert!(
            !is_synopsis(real),
            "`{real}` is a real invocation and must NOT screen as a synopsis"
        );
    }
}

/// `classify` is TOTAL and its default is EXECUTE. A newly published command
/// must join the executed set without anyone remembering to add it.
#[test]
fn classification_defaults_to_execute() {
    assert_eq!(
        classify("xpile transpile factorial.py --target wasm"),
        Disposition::Execute
    );
    assert_eq!(classify("xpile diamond"), Disposition::Execute);
    assert!(matches!(
        classify("cargo install xpile"),
        Disposition::Screened(_)
    ));
    assert!(matches!(
        classify("git clone https://github.com/paiml/xpile"),
        Disposition::Screened(_)
    ));
}

/// The trailing-comment parser is what property B's B1 arm reads, and a `#`
/// inside quotes is not a comment.
#[test]
fn trailing_comment_parser_respects_quotes() {
    assert_eq!(
        trailing_comment("xpile diamond   # works anywhere"),
        Some("# works anywhere".to_string())
    );
    assert_eq!(trailing_comment("xpile info"), None);
    assert_eq!(
        trailing_comment(r#"echo "a # b""#),
        None,
        "a `#` inside double quotes is not a comment"
    );
    assert_eq!(
        trailing_comment("echo 'a # b'"),
        None,
        "a `#` inside single quotes is not a comment"
    );
}
