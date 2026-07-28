//! XPILE-AUDITCLAIM-001 (PMAT-1443) — every user-facing rendering of the
//! frontend registry carries its DISPOSITION, not the raw routing set.
//!
//! ## The defect this locks out
//!
//! `Frontend::extensions()` is a ROUTING set. Two of its entries are in it
//! precisely BECAUSE they have no parser: `.ruchy` (PMAT-1346, emit-only) and
//! `.mk` (PMAT-1420, no Makefile dialect) are registered so a matching file
//! reaches a SPECIFIC refusal instead of the generic dispatch failure. That
//! decision is right, and it is exactly what makes rendering the flat union to
//! a user a false claim about what xpile can read.
//!
//! PMAT-1434 fixed that on the dispatch-failure message and wrote the general
//! rule into its own doc comment. Two more surfaces were still publishing the
//! union at 1e251c70, both MEASURED before this gate was written:
//!
//! | surface | published | actually reads |
//! |---|---|---|
//! | `xpile audit <dir-with-no-source>` | `xpile recognises .bash, .c, .h, .mk, .py, .pyi, .ruchy, .sh, .wat, .zsh` | 8 of those 10 |
//! | `cargo run --example 06_inspect_session` | `ruchy extensions: ["ruchy"]`, `bashrs extensions: [… "mk"]`, under the heading `Frontends (read source → meta-HIR)` | neither `ruchy` nor `mk` |
//!
//! The audit bail is the worse of the two: it fires on the ERROR path, where
//! the reader is asking "what should I point this at?", and one answer in five
//! was wrong. The example is what `book/src/quickstart.md` tells the reader to
//! run to answer "what's registered?" — `xpile info` grew the disposition at
//! PMAT-1428 and this second copy of the same roster did not.
//!
//! ## The two scopes, and why naming the wrong one is also a false claim
//!
//! `matches_path` claims extensionless `Makefile` / `Dockerfile`, but
//! `collect_source_files` walks by EXTENSION and never sees them — verified
//! below, not assumed. So the audit bail must publish the narrower
//! [`SpellingScope::Extensions`] split: reusing the dispatch message's `All`
//! split would advertise two spellings that cannot work at any extension,
//! trading one over-report for another. That is what
//! [`the_bail_never_publishes_a_spelling_the_audit_walk_cannot_collect`] pins.
//!
//! ## Why the existing gates did not catch it
//!
//! `frontend_dispatch_message_witness.rs` (XPILE-FRONTEND-CLAIM-002) is
//! rigorous about the dispatch message and reads no other surface — the
//! PMAT-1417/1438 SCOPE shape, a gate aimed at a SITE rather than at the
//! CLAIM CLASS. `cli_audit_honesty_witness.rs` (XPILE-AUDITHON-001) does spawn
//! this exact bail, and asserts `stderr.contains("no source file")` — the
//! reason clause, never the extension list that follows it.
//!
//! No external toolchain is involved — the subject is the shipped `xpile`
//! binary, the live registry and the tracked examples corpus — so this witness
//! has no skip path and always executes.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use xpile_frontend::render_frontend_roster;

/// A real program in each frontend's own language, keyed by frontend name.
/// Every registered frontend must appear here or [`probe_for`] panics — a new
/// source language cannot silently drop out of this gate's coverage.
const PROBES: &[(&str, &str)] = &[
    (
        "python",
        "def add(a: int, b: int) -> int:\n    return a + b\n",
    ),
    ("c", "int add(int a, int b) { return a + b; }\n"),
    ("ruchy", "fun add(a: i64, b: i64) -> i64 { a + b }\n"),
    ("bashrs", "echo hello\n"),
    (
        "wasm",
        "(module\n  ;; source module: probe\n  \
         (func $__wasm_add_i64 (param $x i64) (param $y i64) (result i64)\n    \
         local.get $x\n    local.get $y\n    i64.add\n  )\n  \
         (func $add (param $a i64) (param $b i64) (result i64)\n    \
         local.get $a\n    local.get $b\n    call $__wasm_add_i64\n  )\n)\n",
    ),
];

/// A per-CALL temp directory. Not per-test: these checks spawn the binary once
/// per spelling, and a shared directory would let one probe's file satisfy the
/// next probe's scan — the corpus under audit IS the subject here.
fn scratch(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("xpile-auditclaim-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("create {}: {e}", dir.display()));
    dir
}

/// `xpile audit <dir>` → (exit ok, stdout, stderr).
fn audit(dir: &Path) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .args(["audit", dir.to_str().expect("utf-8 path")])
        .output()
        .unwrap_or_else(|e| panic!("running xpile audit {}: {e}", dir.display()));
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// The no-source bail, obtained from a directory that really holds no source.
fn no_source_bail() -> String {
    let dir = scratch("bail");
    let (ok, stdout, stderr) = audit(&dir);
    assert!(
        !ok,
        "`xpile audit` over an EMPTY directory SUCCEEDED — the bail could not \
         be obtained and every check in this file would read an empty \
         string.\n--- stdout ---\n{stdout}"
    );
    assert!(
        stderr.contains("no source file"),
        "`xpile audit` over an empty directory failed for a DIFFERENT reason \
         than having no source; this file is checking the wrong \
         string.\n--- stderr ---\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    stderr
}

/// Read the `["a", "b", …]` debug list that follows `marker` in `msg`.
/// `None` when the marker is absent — an empty half is omitted entirely.
fn list_after(msg: &str, marker: &str) -> Option<BTreeSet<String>> {
    let rest = &msg[msg.find(marker)? + marker.len()..];
    let open = rest.find('[')?;
    let close = rest[open..].find(']')? + open;
    Some(
        rest[open + 1..close]
            .split('"')
            .skip(1)
            .step_by(2)
            .map(str::to_string)
            .collect(),
    )
}

const LOWERS_MARKER: &str = "spellings that LOWER:";
const REFUSED_MARKER: &str = "ROUTED but REFUSED";
const UNCOLLECTABLE_MARKER: &str = "NOT collected at all";

/// What the REGISTRY declares, in the message's vocabulary, at BOTH scopes:
/// `(extension_lowers, extension_refused, extensionless_refused)`.
///
/// Recomputed here from `extensions()` + `refused_claims()` rather than by
/// calling `Frontend::spellings_by_disposition`, so the message is checked
/// against the registry and not against the helper that renders it.
fn registry_split() -> (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>) {
    let session = xpile_core::default_session();
    assert!(
        !session.frontends.is_empty(),
        "default_session() registered zero frontends — the registry moved and \
         every set comparison below would hold over two empty sets"
    );
    let (mut lowers, mut refused, mut extensionless) =
        (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for f in &session.frontends {
        let declared: BTreeSet<&str> = f.refused_claims().iter().copied().collect();
        for ext in f.extensions() {
            let claim = format!("*.{ext}");
            if declared.contains(claim.as_str()) {
                refused.insert(claim);
            } else {
                lowers.insert(claim);
            }
        }
        extensionless.extend(
            declared
                .iter()
                .filter(|c| !c.starts_with("*."))
                .map(|c| (*c).to_string()),
        );
    }
    (lowers, refused, extensionless)
}

fn probe_for(name: &str) -> &'static str {
    PROBES
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| {
            panic!(
                "registered frontend `{name}` has no entry in PROBES. A new \
                 source language must ship a probe program here — otherwise \
                 this gate silently stops covering its claims."
            )
        })
        .1
}

/// The frontend that claims `*.<ext>`, and its probe program.
fn probe_for_extension(ext: &str) -> &'static str {
    let session = xpile_core::default_session();
    let owner = session
        .frontends
        .iter()
        .find(|f| f.extensions().contains(&ext))
        .unwrap_or_else(|| panic!("no registered frontend claims extension `{ext}`"));
    probe_for(owner.name())
}

/// `files scanned : N` from an audit report.
fn files_scanned(stdout: &str) -> usize {
    stdout
        .lines()
        .find(|l| l.contains("files scanned"))
        .and_then(|l| l.rsplit(':').next())
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or_else(|| panic!("no `files scanned` line in audit report:\n{stdout}"))
}

// ---------------------------------------------------------------------------
// 1. The bail vs the registry — both directions, at the collector's scope.
// ---------------------------------------------------------------------------

/// THE LOAD-BEARING CHECK. The list the reader is told to choose from must be
/// exactly the EXTENSION spellings that lower, and the refusals exactly the
/// extension spellings that are routed and then refused.
#[test]
fn audit_no_source_bail_publishes_the_registry_split_at_the_extension_scope() {
    let msg = no_source_bail();
    let (want_lowers, want_refused, _) = registry_split();

    let got_lowers = list_after(&msg, LOWERS_MARKER).unwrap_or_else(|| {
        panic!(
            "the audit no-source bail publishes no `{LOWERS_MARKER}` list. Before \
             PMAT-1443 it published the flat `extensions()` union under the verb \
             \"recognises\", which named `.mk` and `.ruchy` — both of which refuse \
             every input — as things xpile reads.\n--- stderr ---\n{msg}"
        )
    });
    assert_eq!(
        got_lowers, want_lowers,
        "the spellings the audit bail advertises as LOWERING disagree with the \
         registry.\n  published: {got_lowers:?}\n  registry : \
         {want_lowers:?}\n--- stderr ---\n{msg}"
    );

    let got_refused = list_after(&msg, REFUSED_MARKER).unwrap_or_default();
    assert_eq!(
        got_refused, want_refused,
        "the spellings the audit bail marks ROUTED-but-REFUSED disagree with the \
         registry.\n  published: {got_refused:?}\n  registry : \
         {want_refused:?}\n--- stderr ---\n{msg}"
    );
}

/// Anti-vacuity for the check above, and the direct statement of the defect:
/// the two halves must be a real SPLIT, not the flat union under a new label.
#[test]
fn the_published_halves_are_disjoint_and_the_refused_half_is_not_empty() {
    let msg = no_source_bail();
    let lowers = list_after(&msg, LOWERS_MARKER).expect("LOWERS list");
    let refused = list_after(&msg, REFUSED_MARKER).unwrap_or_default();

    assert!(
        !refused.is_empty(),
        "the audit bail published NO routed-but-refused spellings. Either the \
         registry no longer routes anything it cannot parse — in which case \
         delete this gate deliberately — or the message went back to printing \
         one flat list.\n--- stderr ---\n{msg}"
    );
    assert!(
        !lowers.is_empty(),
        "the audit bail published no lowering spellings at all:\n{msg}"
    );
    let both: Vec<&String> = lowers.intersection(&refused).collect();
    assert!(
        both.is_empty(),
        "spellings published in BOTH halves at once: {both:?}\n{msg}"
    );

    // Known anchors. Without these the two set comparisons above could hold
    // over a registry that had quietly lost its interesting members.
    assert!(
        lowers.contains("*.py"),
        "`*.py` is not published as lowering — the anchor moved:\n{msg}"
    );
    assert!(
        refused.contains("*.ruchy"),
        "`*.ruchy` is not published as routed-but-refused, though \
         `ruchy-frontend` declares `lowers_input() == false`:\n{msg}"
    );
}

// ---------------------------------------------------------------------------
// 2. The bail vs what the collector actually does.
// ---------------------------------------------------------------------------

/// Every spelling advertised as LOWERING must really be collected by the audit
/// walk and really lower — checked by running the audit on a one-file corpus
/// per spelling, with a real program in that spelling's own language.
///
/// PINNED TO THE FRONTEND STAGE, deliberately. "Lowers" is a claim about
/// `parse_and_lower`, and `xpile audit` runs a whole pipeline: its default
/// `--target rust` cannot emit `Stmt::Cmd`, so a perfectly-lowered `probe.bash`
/// still lands in the error list as `backend: lowering error … use --target
/// shell`. That is an honest downstream refusal about a TARGET, not a false
/// claim about the spelling. Asserting on `errors (…)` alone conflated the two
/// and failed here on the first run; the assertion keys on the `parse_and_lower`
/// stage prefix instead, so a real frontend refusal still reds.
#[test]
fn every_spelling_published_as_lowering_is_collected_and_lowers() {
    let msg = no_source_bail();
    let lowers = list_after(&msg, LOWERS_MARKER).expect("LOWERS list");
    assert!(!lowers.is_empty(), "vacuous: nothing published as lowering");

    let mut measurable = 0usize;
    for claim in &lowers {
        let ext = claim
            .strip_prefix("*.")
            .unwrap_or_else(|| panic!("`{claim}` in the LOWERS half is not a `*.<ext>` glob"));
        let dir = scratch(&format!("lower-{ext}"));
        std::fs::write(dir.join(format!("probe.{ext}")), probe_for_extension(ext))
            .expect("write probe");
        let (ok, stdout, stderr) = audit(&dir);
        assert!(
            ok,
            "`xpile audit` REFUSED a corpus holding one `{claim}` file, though \
             the bail advertises `{claim}` as a spelling that \
             lowers.\n--- stderr ---\n{stderr}"
        );
        assert_eq!(
            files_scanned(&stdout),
            1,
            "the audit walk did not collect `probe.{ext}`, though `{claim}` is \
             advertised as lowering.\n{stdout}"
        );
        assert!(
            !stdout.contains("parse_and_lower"),
            "`probe.{ext}` is advertised as LOWERING but the audit reported a \
             FRONTEND-stage failure for it.\n{stdout}"
        );
        if stdout.contains('%') && !stdout.contains("VACUOUS") {
            measurable += 1;
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    // Anti-vacuity: an "audit did not error" assertion passes for free over a
    // corpus that lowered to nothing measurable. At least one advertised
    // spelling must carry the whole way through to a real F1 denominator.
    assert!(
        measurable > 0,
        "not one spelling advertised as lowering produced a MEASURABLE audit — \
         every probe reported VACUOUS, so `lowers` was never confirmed to mean \
         anything past parsing"
    );
}

/// The justification for the message's wording, not a test of the fix: a
/// routed-but-refused extension is NOT "unrecognised". It IS collected, it IS
/// counted in `files scanned`, and it lands in the error list — which is why
/// folding it in with `*.py` under one verb was the falsehood.
///
/// This measures the COLLECTOR and is green on both sides of PMAT-1443. It is
/// here so that a future change making refused extensions uncollectable reds
/// the sentence that describes them rather than leaving it stale.
#[test]
fn a_refused_extension_is_collected_and_reported_as_an_error_not_as_absent() {
    let (_, want_refused, _) = registry_split();
    assert!(
        !want_refused.is_empty(),
        "the registry declares no refusing extension — this check would range \
         over an empty set"
    );
    for claim in &want_refused {
        let ext = claim.strip_prefix("*.").expect("`*.<ext>` glob");
        let dir = scratch(&format!("refused-{ext}"));
        std::fs::write(dir.join(format!("probe.{ext}")), probe_for_extension(ext))
            .expect("write probe");
        let (ok, stdout, stderr) = audit(&dir);
        assert!(
            ok,
            "`xpile audit` over a corpus of one `{claim}` file REFUSED. audit is \
             a reporter: a real corpus it could not lower is a measurement \
             OUTCOME (VACUOUS + errors), not an input error.\n{stderr}"
        );
        assert_eq!(
            files_scanned(&stdout),
            1,
            "`probe.{ext}` was NOT collected, so the bail's claim that such a \
             file `IS collected and reported as an error` is now false.\n{stdout}"
        );
        assert!(
            stdout.contains(&format!("probe.{ext}")) && stdout.contains("errors"),
            "`probe.{ext}` was collected but does not appear in the audit's \
             error list.\n{stdout}"
        );
        assert!(
            !stdout.contains("[OK]"),
            "a corpus of one unparseable `{claim}` file reported a passing \
             coverage grade.\n{stdout}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// THE ANTI-OVER-CORRECTION PIN. `matches_path` claims extensionless
/// `Makefile` / `Dockerfile`; the audit walk is extension-only and cannot
/// reach them. Reusing the dispatch message's `All` split here would advertise
/// two spellings that cannot work at any extension — a different over-report,
/// not a fix. The bail must therefore EXCLUDE them from both published halves
/// and DISCLOSE the exclusion.
#[test]
fn the_bail_never_publishes_a_spelling_the_audit_walk_cannot_collect() {
    let (_, _, extensionless) = registry_split();
    assert!(
        !extensionless.is_empty(),
        "no frontend claims an extensionless spelling any more — this check \
         would range over an empty set and pass for free"
    );

    let msg = no_source_bail();
    let lowers = list_after(&msg, LOWERS_MARKER).expect("LOWERS list");
    let refused = list_after(&msg, REFUSED_MARKER).unwrap_or_default();
    let disclosed = list_after(&msg, UNCOLLECTABLE_MARKER).unwrap_or_else(|| {
        panic!(
            "the audit bail does not disclose the spellings its walk cannot \
             reach ({extensionless:?}). Omitting them silently is the same \
             under-report in a smaller font.\n--- stderr ---\n{msg}"
        )
    });

    for claim in &extensionless {
        assert!(
            !lowers.contains(claim),
            "`{claim}` is published as a spelling `xpile audit` can be pointed \
             at, but the audit walk collects by extension and never sees \
             it.\n{msg}"
        );
        assert!(
            !refused.contains(claim),
            "`{claim}` is published in the audit bail's routed-but-refused \
             half, which reads as `audit scans it, then errors`. It is not \
             scanned at all.\n{msg}"
        );
        assert!(
            disclosed.contains(claim),
            "`{claim}` is claimed by `matches_path` but the bail does not say \
             the audit walk cannot reach it.\n{msg}"
        );
    }

    // …and the behaviour the disclosure describes. A corpus holding ONLY the
    // extensionless spellings must still bail, or the sentence is false.
    let dir = scratch("extensionless");
    for claim in &extensionless {
        std::fs::write(dir.join(claim), "irrelevant bytes\n").expect("write probe");
    }
    let (ok, stdout, _) = audit(&dir);
    assert!(
        !ok,
        "a corpus holding only {extensionless:?} was SCANNED — the audit walk \
         now reaches the extensionless spellings, so the bail's disclosure that \
         it cannot is stale.\n{stdout}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// 3. The other surface in the class: the roster the book tells readers to run.
// ---------------------------------------------------------------------------

/// The shared renderer must present each frontend's claims split by
/// disposition — never a refusing spelling in the lowering half.
#[test]
fn the_shared_roster_renders_every_claim_under_its_own_disposition() {
    let session = xpile_core::default_session();
    let roster = render_frontend_roster(&session.frontends);
    assert!(
        !roster.trim().is_empty(),
        "render_frontend_roster produced nothing for a non-empty registry"
    );

    for f in &session.frontends {
        let line = roster
            .lines()
            .find(|l| l.split_whitespace().nth(1) == Some(f.name()))
            .unwrap_or_else(|| {
                panic!(
                    "no roster line for registered frontend `{}`:\n{roster}",
                    f.name()
                )
            });
        // Split the line at the refusal marker: everything before it is what
        // the reader is told this frontend READS.
        let (lowering_half, refused_half) = match line.find(REFUSED_MARKER) {
            Some(i) => (&line[..i], &line[i..]),
            None => (line, ""),
        };
        let declared: BTreeSet<&str> = f.refused_claims().iter().copied().collect();
        for ext in f.extensions() {
            let claim = format!("*.{ext}");
            if declared.contains(claim.as_str()) {
                assert!(
                    !lowering_half.contains(&claim),
                    "`{claim}` refuses every input but the roster presents it as \
                     something `{}` reads:\n{line}",
                    f.name()
                );
                assert!(
                    refused_half.contains(&claim),
                    "`{claim}` is declared refused but the roster does not say \
                     so:\n{line}"
                );
            } else {
                assert!(
                    lowering_half.contains(&claim),
                    "`{claim}` lowers but does not appear in `{}`'s lowering \
                     half:\n{line}",
                    f.name()
                );
            }
        }
        for claim in declared.iter().filter(|c| !c.starts_with("*.")) {
            assert!(
                refused_half.contains(claim),
                "extensionless claim `{claim}` is missing from the roster:\n{line}"
            );
        }
    }

    // Anti-vacuity: at least one frontend must actually exercise the refusal
    // half, or every assertion above is over an empty `declared`.
    assert!(
        roster.contains(REFUSED_MARKER),
        "the roster names no refusing claim at all, so the split is untested:\n{roster}"
    );
}

/// The rule over the DERIVED examples corpus, not over the one file the defect
/// was found in (PMAT-1417's scope lesson). An example is a published,
/// runnable surface — `book/src/quickstart.md` points at `06_inspect_session`
/// by name — so none of them may render the raw routing set. The shared
/// renderer is the supported way to show the roster.
#[test]
fn no_example_renders_the_raw_routing_set() {
    let examples = Path::new("examples");
    let mut seen = Vec::new();
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(examples)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", examples.display()))
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let body = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}: {e}"));
        if body.contains(".extensions()") {
            offenders.push(name.clone());
        }
        seen.push(name);
    }

    assert!(
        !seen.is_empty(),
        "no example sources found under {} — this rule would pass over an \
         empty corpus",
        examples.display()
    );
    let anchor = "06_inspect_session.rs";
    assert!(
        seen.iter().any(|n| n == anchor),
        "the anchor example `{anchor}` is not in the corpus this rule walks \
         ({seen:?}); the rule may be looking in the wrong place"
    );
    assert!(
        offenders.is_empty(),
        "these examples render `Frontend::extensions()` — the ROUTING set, \
         which includes spellings that refuse every input — instead of calling \
         `xpile_frontend::render_frontend_roster`: {offenders:?}"
    );

    let anchor_body = std::fs::read_to_string(examples.join(anchor))
        .unwrap_or_else(|e| panic!("read {anchor}: {e}"));
    assert!(
        anchor_body.contains("render_frontend_roster"),
        "`{anchor}` no longer calls the shared roster renderer, so the property \
         asserted by `the_shared_roster_renders_every_claim_under_its_own_\
         disposition` no longer says anything about what this example prints"
    );
}
