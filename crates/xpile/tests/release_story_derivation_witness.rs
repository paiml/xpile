//! XPILE-RELSTORY-001 (PMAT-1470) — the release plan mandated a CHANGELOG story
//! describing the range the PREVIOUS tag had already closed.
//!
//! THE DEFECT. `docs/specifications/sub/sprint-6day-2026-07-26.md` §5 told
//! Thursday's operator what the release is:
//!
//! > **CHANGELOG story — 101 commits / ~76 unique PMAT ids / 136+ files /
//! > +53,940 lines, in 8 arcs**
//!
//! and then enumerated those eight arcs by hand. Measured at `ac0b28fe` on
//! 2026-07-29, `v0.1.617..HEAD` was **109** commits, **173** unique PMAT ids,
//! **250** files and **+77,645** insertions, and `[Unreleased]` already carried
//! **76** arc headings. All four figures are wrong, and the arc list is one
//! whole release behind: of the 15 PMAT ids the eight arcs cite, **14 are not in
//! `v0.1.617..HEAD` at all** — they are v0.1.617 content, already published
//! under `## [0.1.617]`.
//!
//! THE DOCUMENT STATES THE RULE AND BREAKS IT FIVE LINES LATER. The very next
//! paragraph says of the WASM witness count: *"do not type it here (it read
//! `795` on 2026-07-26 and is 857 today; a machine-derived number copied into
//! prose is the PMAT-1445 shape)"*. The banned shape is committed four times in
//! the paragraph immediately above that sentence. **A rule written into prose is
//! not a gate** — which is the whole reason this file exists.
//!
//! THE TRAP THAT KEPT IT ALIVE IS AN ACCIDENTAL CONFIRMATION. By 2026-07-29 the
//! live `[Unreleased]` arc-heading count (`grep -c '^### '`) had reached **76** —
//! numerically equal to the plan's *"~76 unique PMAT ids"*. A reader
//! spot-checking that figure against the CHANGELOG gets a **match** and stops,
//! while the quantity the number actually names stood at **173**. True in the
//! half a reader checks — the [[PMAT-1458]] shape, reached here by coincidence
//! rather than by construction, which is worse: nobody chose it, so nobody knew
//! to disclose it.
//!
//! A CLAIM CLASS IS NOT A SECTION. The same falsehood sat 75 lines below §5 in
//! §"Also sacrificed" — *"This is a release of 101 commits that were already
//! written"* — with no range attribution. Fixing only the headline would have
//! been [[PMAT-1438]] again, so the rules below range over the whole document.
//!
//! WHAT THIS FILE PINS.
//!
//! 1. **The active plan may not type an aggregate count of the release range.**
//!    The range moves with every merge — this slice's own commit took it from
//!    109/173 to 110/174 — so any typed figure is stale before it is read.
//! 2. **The active plan may not hand-enumerate an arc roster.** `[Unreleased]`
//!    is written arc-first as the sprint runs and is therefore the register of
//!    what shipped; a transcription can only drift from it.
//! 3. **The derivation commands the plan names are EXECUTED here** and must
//!    agree with an independent measurement. A cited command that does not
//!    support the claim is the [[PMAT-1453]] shape; this is the behaviour half.
//!
//! THE EXEMPTION IS A POSITIVE MARKER, NOT A NEGATION SCREEN. §2 legitimately
//! reports *"the cron produced all 101 commits"* and *"72 of 101 commits touched
//! `roadmap.yaml`"* — both attribute the count on the same line to an explicit,
//! already-closed range (`v0.1.616..origin/main`, `v0.1.616..HEAD`). A count
//! carrying such a citation is a historical report and passes; a count presented
//! as a property of *this* release does not. `historical_counts_…` below is a
//! control that must stay GREEN, because a rule whose false positives are never
//! measured is a rule that will red someone else's PR (PMAT-1466).
//!
//! THE SUBJECT IS DERIVED, NOT LISTED: `queue.yaml`'s `sprint.plan` names the
//! plan the sprint is executing. Superseded plans (`sprint-10day-2026-06-23.md`)
//! describe ranges that are legitimately closed and are correctly out of scope —
//! scoping this rule to "every planning document" would have flagged them.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = workspace_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn git(args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(workspace_root())
        .output()
        .unwrap_or_else(|e| panic!("spawn git {args:?}: {e}"));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The plan the sprint is currently executing — DERIVED from the queue, so a
/// retarget moves the subject with it and no path is typed here.
fn active_plan() -> String {
    let body = read("docs/roadmaps/queue.yaml");
    let doc: serde_yaml::Value = serde_yaml::from_str(&body).expect("queue.yaml is valid YAML");
    let rel = doc
        .get("sprint")
        .and_then(|s| s.get("plan"))
        .and_then(|v| v.as_str())
        .expect("queue.yaml declares sprint.plan")
        .to_string();
    assert!(
        workspace_root().join(&rel).is_file(),
        "queue.yaml's sprint.plan names {rel:?}, which does not exist — every rule in this file \
         would then range over an unreadable subject"
    );
    rel
}

/// The tag the release range starts at. DERIVED — no version literal here.
fn prior_tag() -> String {
    let t = git(&["describe", "--tags", "--abbrev=0"])
        .trim()
        .to_string();
    assert!(
        !t.is_empty(),
        "`git describe --tags --abbrev=0` returned nothing — this checkout has no tags, so every \
         range measurement below would be taken over the whole history and silently pass"
    );
    t
}

/// Unique PMAT ids in `<prior>..HEAD`, exactly as §5's derivation computes them.
fn ids_in_range() -> Vec<String> {
    let log = git(&["log", &format!("{}..HEAD", prior_tag()), "--format=%s%n%b"]);
    let mut ids: Vec<String> = log
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .filter(|t| {
            t.starts_with("PMAT-") && t.len() > 5 && t[5..].bytes().all(|b| b.is_ascii_digit())
        })
        .map(str::to_string)
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// A count is a HISTORICAL REPORT when the same paragraph attributes it to an
/// explicit commit range (`vA.B.C..<something>`). This is a POSITIVE marker: the
/// text must carry the citation to earn the exemption. A negation screen ("does
/// not say 'this release'") would have let the §"Also sacrificed" site through.
fn cites_an_explicit_range(par: &str) -> bool {
    let b = par.as_bytes();
    let mut i = 0usize;
    while let Some(r) = par[i..].find("..") {
        let at = i + r;
        // walk back over a version-ish token and require it to start `v<digit>`
        let mut j = at;
        while j > 0 && (b[j - 1].is_ascii_digit() || b[j - 1] == b'.') {
            j -= 1;
        }
        if j > 0 && b[j - 1] == b'v' && at > j && b[j].is_ascii_digit() {
            return true;
        }
        i = at + 2;
    }
    false
}

/// A disclosed quotation of the old text — a reporting lead plus a quote.
fn is_disclosed_mention(par: &str) -> bool {
    let quoted = par.contains('"') || par.contains('“');
    let reported = par.contains("Through v0.1.")
        || par.contains("instead read")
        || par.contains("used to say")
        || par.contains("this block said");
    quoted && reported
}

/// Aggregate-shape phrases: a number bound to a quantity that is a property of
/// the release range. Returns the offending phrase for the failure message.
fn aggregate_claims(par: &str) -> Vec<String> {
    const UNITS: [&str; 5] = [" commits", " unique PMAT ids", " files", " lines", " arcs"];
    let mut hits = Vec::new();
    for unit in UNITS {
        let mut base = 0usize;
        while let Some(r) = par[base..].find(unit) {
            let at = base + r;
            // require a bare number immediately before the unit
            let lead = &par[..at];
            let digits: String = lead
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '+' || *c == '~')
                .collect();
            if digits.chars().any(|c| c.is_ascii_digit()) {
                let shown: String = digits.chars().rev().collect();
                hits.push(format!("{shown}{unit}"));
            }
            base = at + unit.len();
        }
    }
    hits
}

/// The claim FRAME — text presenting a figure as the identity of the release
/// being prepared. This is what discriminates, not the unit: the plan is full of
/// honest counts ("~5 lines", "931 lines", a day table's "60 lines") that are
/// commit and file sizes, and the first cut of this rule flagged nine of them.
/// A number is only an offender when the sentence around it says the number IS
/// the release.
fn frames_as_this_release(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    [
        "changelog story",
        "release story",
        "this is a release of",
        "this release is",
        "the release is",
        "release comprises",
        "release totals",
    ]
    .iter()
    .any(|f| l.contains(f))
}

/// Every line of `body` that states the release's aggregate shape. LINE
/// granularity, because a markdown table is a single blank-line paragraph and
/// paragraph scoping evaluated the exemption over the whole day-plan at once.
fn aggregate_offenders(body: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (idx, line) in body.lines().enumerate() {
        if !frames_as_this_release(line) {
            continue;
        }
        if cites_an_explicit_range(line) || is_disclosed_mention(line) {
            continue;
        }
        for hit in aggregate_claims(line) {
            out.push((idx + 1, hit));
        }
    }
    out
}

#[test]
fn the_active_plan_types_no_aggregate_of_the_release_range() {
    let rel = active_plan();
    let body = read(&rel);
    let offenders: Vec<String> = aggregate_offenders(&body)
        .into_iter()
        .map(|(line, hit)| format!("{rel}:{line}: types {hit:?}"))
        .collect();
    let prior = prior_tag();
    assert!(
        offenders.is_empty(),
        "\nthe active release plan types an aggregate count of the release range:\n  {}\n\n\
         The range moves with EVERY merge — right now `{prior}..HEAD` is {} commits and {} unique \
         PMAT ids — so a typed figure is stale before the operator reads it. State the derivation, \
         not the number, or attribute the count to an explicit closed range (`{prior}..<tag>`) on \
         the same paragraph if it is a historical report.",
        offenders.join("\n  "),
        git(&["log", "--oneline", &format!("{prior}..HEAD")])
            .lines()
            .count(),
        ids_in_range().len(),
    );
}

#[test]
fn the_active_plan_enumerates_no_arc_roster() {
    // A hand-transcribed roster can only drift from `[Unreleased]`, which is
    // written arc-first as the sprint runs. Detected as >=3 CONSECUTIVE numbered
    // items each citing a PMAT id outside a code span — the done-checklist items
    // that name `PMAT-9999` and a `grep -oE 'PMAT-[0-9]+'` recipe are neither
    // consecutive nor bare, and are correctly not a roster.
    let rel = active_plan();
    let body = read(&rel);
    let in_range = ids_in_range();

    let mut run: Vec<(usize, String)> = Vec::new();
    let mut worst: Vec<(usize, String)> = Vec::new();
    for (idx, line) in body.lines().enumerate() {
        let numbered = line
            .split_once(". ")
            .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        let bare_id = strip_code_spans(line)
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .any(|t| {
                t.starts_with("PMAT-") && t.len() > 5 && t[5..].bytes().all(|b| b.is_ascii_digit())
            });
        if numbered && bare_id {
            run.push((idx + 1, line.to_string()));
        } else if !numbered {
            if run.len() > worst.len() {
                worst = run.clone();
            }
            run.clear();
        }
    }
    if run.len() > worst.len() {
        worst = run;
    }

    if worst.len() >= 3 {
        let stale: Vec<String> = worst
            .iter()
            .flat_map(|(_, l)| {
                strip_code_spans(l)
                    .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
                    .filter(|t| {
                        t.starts_with("PMAT-")
                            && t.len() > 5
                            && t[5..].bytes().all(|b| b.is_ascii_digit())
                    })
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|id| !in_range.contains(id))
            .collect();
        panic!(
            "\nthe active release plan hand-enumerates an arc roster ({} consecutive items from \
             {rel}:{}):\n  {}\n\n\
             {} of the PMAT ids it cites are NOT in `{}..HEAD`: {:?}\n\
             `[Unreleased]` in CHANGELOG.md is written arc-first as the sprint runs and is the \
             register of what shipped; a roster typed here can only drift from it.",
            worst.len(),
            worst[0].0,
            worst
                .iter()
                .map(|(n, l)| format!("{n}: {}", &l.chars().take(90).collect::<String>()))
                .collect::<Vec<_>>()
                .join("\n  "),
            stale.len(),
            prior_tag(),
            stale,
        );
    }
}

fn strip_code_spans(line: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in line.chars() {
        if c == '`' {
            inside = !inside;
        } else if !inside {
            out.push(c);
        }
    }
    out
}

#[test]
fn the_derivation_commands_the_plan_names_actually_run() {
    // THE BEHAVIOUR HALF. The plan replaced four numbers with four commands;
    // this RUNS them and checks each against an independent measurement. A cited
    // command that does not support the claim is the PMAT-1453 shape.
    let rel = active_plan();
    let body = read(&rel);
    let fence = body
        .split("```bash")
        .nth(1)
        .and_then(|r| r.split("```").next())
        .unwrap_or_else(|| {
            panic!("{rel} names no ```bash derivation block — §5 must state the derivation")
        });
    for needle in [
        "git describe --tags --abbrev=0",
        "git log",
        "git diff --shortstat",
        "CHANGELOG.md",
    ] {
        assert!(
            fence.contains(needle),
            "{rel}'s derivation block does not mention {needle:?}; it reads:\n{fence}"
        );
    }

    let prior = prior_tag();
    let root = workspace_root();
    let run = |script: &str| -> String {
        let out = Command::new("bash")
            .arg("-c")
            .arg(script)
            .current_dir(&root)
            .output()
            .expect("spawn bash");
        assert!(
            out.status.success(),
            "the plan's derivation command failed: {script}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // Command 1: the prior tag is derived, not typed.
    assert_eq!(
        run("git describe --tags --abbrev=0"),
        prior,
        "the plan's prior-tag derivation disagrees with this test's"
    );

    // Command 2: the id count agrees with an independent extraction.
    let counted: usize = run(&format!(
        "git log {prior}..HEAD --format='%s%n%b' | grep -oE 'PMAT-[0-9]+' | sort -u | wc -l"
    ))
    .parse()
    .expect("id count is a number");
    assert_eq!(
        counted,
        ids_in_range().len(),
        "the plan's PMAT-id derivation and this test's extraction disagree over {prior}..HEAD"
    );

    // Command 3: the arc count is derivable and non-zero, so the plan's pointer
    // at `[Unreleased]` names something a reader can actually count.
    let arcs: usize = run(r"awk '/## \[Unreleased\]/,/^## \[0/' CHANGELOG.md | grep -c '^### '")
        .parse()
        .expect("arc count is a number");
    assert!(
        arcs > 0,
        "the plan points Thursday's operator at `[Unreleased]` for the arc roster, and the \
         awk/grep recipe it names finds ZERO arc headings there"
    );
}

#[test]
fn historical_counts_attributed_to_a_shipped_range_are_not_offenders() {
    // GREEN CONTROL — the false-positive half. §2 legitimately reports counts for
    // ranges that are closed. If the exemption ever stops recognising them, this
    // test reds BEFORE the rule reds someone else's honest paragraph.
    let rel = active_plan();
    let body = read(&rel);
    let offenders = aggregate_offenders(&body);

    // The plan carries many honest counts — §2's assessment of a closed range,
    // per-commit sizes, a day table's line budgets. The FIRST cut of this rule
    // flagged NINE of them; that is why this control exists and why it names the
    // survivors explicitly rather than asserting "some paragraph is exempt".
    let honest: Vec<(usize, &str)> = body
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l))
        .filter(|(_, l)| !aggregate_claims(l).is_empty() && !l.contains("DERIVE IT AT TAG TIME"))
        .collect();
    assert!(
        honest.len() >= 5,
        "{rel} now carries fewer than five counted quantities ({} found); this control can no \
         longer demonstrate that the rule discriminates",
        honest.len()
    );
    for (line, text) in &honest {
        if offenders.iter().any(|(l, _)| l == line) {
            continue; // a genuine offender is allowed to be flagged
        }
        assert!(
            !frames_as_this_release(text)
                || cites_an_explicit_range(text)
                || is_disclosed_mention(text),
            "{rel}:{line} would be flagged, but it is an honest count:\n{}",
            text.chars().take(240).collect::<String>()
        );
    }
    assert!(
        offenders.is_empty(),
        "{rel} still states the release aggregate at {offenders:?}"
    );

    // And the exemption must not be a rubber stamp: a bare claim with no range
    // citation and no disclosure is still an offender.
    assert!(
        !cites_an_explicit_range("This is a release of 101 commits that were already written."),
        "the range-citation exemption matches text carrying no range at all"
    );
    assert!(
        !is_disclosed_mention("This is a release of 101 commits that were already written."),
        "the disclosure exemption matches text that discloses nothing"
    );
}

#[test]
fn the_pre_fix_story_block_reds_every_rule() {
    // NON-VACUITY. The verbatim text this slice removed, embedded so the rules
    // are proven to fire on the real defect and not merely on the empty set.
    const PRE_FIX_HEADLINE: &str = "**CHANGELOG story — 101 commits / ~76 unique PMAT ids / \
         136+ files / +53,940 lines, in 8 arcs**, written as ~8 arc-sized commits of 30–65 lines \
         across Days 2–4 (matching the repo's own demonstrated cadence, not the 0.1.616 release \
         commit's +46):";
    const PRE_FIX_VELOCITY: &str = "**Also sacrificed: velocity optics.** This is a release of \
         101 commits that were already written, wrapped in truth work.";

    for (what, text) in [
        ("§5 headline", PRE_FIX_HEADLINE),
        ("§'Also sacrificed'", PRE_FIX_VELOCITY),
    ] {
        assert!(
            !cites_an_explicit_range(text) && !is_disclosed_mention(text),
            "{what} would be exempted, so the rule could not have caught it"
        );
        assert!(
            frames_as_this_release(text),
            "{what} is not recognised as framing a figure as the release, so the rule would walk \
             past the very text it was written for"
        );
        assert!(
            !aggregate_offenders(text).is_empty(),
            "{what} produced no offender — the rule is vacuous against the real defect"
        );
    }

    // The headline must red on all four of its figures, not just the first.
    let hits = aggregate_claims(PRE_FIX_HEADLINE);
    for unit in [" commits", " unique PMAT ids", " files", " lines", " arcs"] {
        assert!(
            hits.iter().any(|h| h.ends_with(unit)),
            "the pre-fix headline typed a {unit} figure and the rule did not flag it; hits={hits:?}"
        );
    }

    // And the arcs it enumerated really are out of range — the reason the rule
    // exists, re-measured rather than asserted.
    let in_range = ids_in_range();
    let arc_ids = [
        "PMAT-1254",
        "PMAT-1290",
        "PMAT-1330",
        "PMAT-1332",
        "PMAT-1337",
        "PMAT-1338",
        "PMAT-1341",
        "PMAT-1327",
        "PMAT-1331",
        "PMAT-1268",
        "PMAT-1285",
        "PMAT-1269",
        "PMAT-1282",
        "PMAT-482",
    ];
    let stale = arc_ids
        .iter()
        .filter(|id| !in_range.contains(&id.to_string()))
        .count();
    assert_eq!(
        stale,
        arc_ids.len(),
        "the pre-fix arc roster cited {} ids that ARE in {}..HEAD; the roster has become \
         current and this file's account of the defect needs re-measuring",
        arc_ids.len() - stale,
        prior_tag()
    );
}

#[test]
fn the_subject_resolves_to_the_plan_the_queue_is_executing() {
    // NON-VACUITY OF THE SUBJECT. If `sprint.plan` stops resolving, every rule
    // above ranges over the wrong document and goes on passing (PMAT-1396).
    let rel = active_plan();
    assert!(
        rel.starts_with("docs/") && rel.ends_with(".md"),
        "sprint.plan resolved to {rel:?}, which is not a markdown document under docs/"
    );
    let body = read(&rel);
    assert!(
        body.contains("CHANGELOG story"),
        "{rel} carries no `CHANGELOG story` block, so the aggregate and derivation rules have \
         nothing to range over — the section was renamed or removed"
    );
    // Superseded plans are deliberately OUT of scope: they describe ranges that
    // are legitimately closed. Assert at least one exists, so the choice to scope
    // by `sprint.plan` is a live distinction rather than a vacuous one.
    let others = std::fs::read_dir(workspace_root().join("docs/specifications/sub"))
        .expect("sub/ exists")
        .filter_map(|e| e.ok())
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.contains("sprint") && n.ends_with(".md") && !rel.ends_with(&n)
        })
        .count();
    assert!(
        others > 0,
        "no superseded sprint plan was found, so scoping by queue.sprint.plan is indistinguishable \
         from scoping over every planning document — re-check that the distinction still matters"
    );
}
