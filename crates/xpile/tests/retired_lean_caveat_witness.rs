//! XPILE-RETIRED-001 (PMAT-1473) — the release notes about to ship reported a
//! divergence that had been FIXED, in the section readers trust most.
//!
//! THE DEFECT. `CHANGELOG.md`'s `[Unreleased]` "Known divergences" list said:
//!
//! > **5. The default `--contracts on` Lean emit does not elaborate.** Every
//! > backend cites its contracts in comments except Lean, which emits
//! > `@[xpile_contract "C-PY-INT-ARITH"]` — an attribute no Lean prelude
//! > registers, so `lean` rejects the file.
//!
//! **PMAT-1405 fixed exactly that**, and the list was never updated. Measured on
//! this tree with a forced-fresh build: `xpile transpile add.py --target lean`
//! under the DEFAULT flags emits `/-- xpile-contract: C-PY-INT-ARITH -/` — a doc
//! comment, exactly like every other backend's citation — and `lean` exits **0**.
//! Explicit `--contracts on` likewise exits 0.
//!
//! The same retired caveat was ALSO still enforced by the active sprint plan's
//! "What the note must NOT claim" list, which forbade claiming *"that the default
//! Lean emit elaborates"* — i.e. it forbade Thursday's operator from stating a
//! true, already-announced capability.
//!
//! WHY IT IS WORTH A GATE. **A known-divergences section that reports a FIXED
//! defect is the same class of falsehood as one that omits a live defect**, and
//! it is read by exactly the people deciding what a release may claim. The
//! aged-claim shape has no tell: nothing about the sentence looks wrong, and it
//! was true when written.
//!
//! [[PMAT-1411]] NAMED THIS CLASS IN ITS OWN SUBJECT — *"the sweep found the
//! INVERSE defect, a GREEN test ENFORCING the retired Lean caveat PMAT-1405 had
//! already fixed"* — and swept the **tests**. It never reached the **documents**.
//! After any semantics change, ask what ELSE describes the thing you changed.
//!
//! WHAT THIS FILE PINS, and the design that matters:
//!
//! 1. **The behaviour is MEASURED here, not assumed** — this test runs the
//!    emitter under default flags and, when the toolchain is present, runs
//!    `lean` on the output. Every document rule below is checked against that
//!    measurement.
//! 2. **No release-facing document may assert the retired caveat** while the
//!    measurement contradicts it.
//! 3. **BIDIRECTIONAL**: if the emit ever REGRESSES to `@[xpile_contract`, the
//!    "RETIRED" corrections this slice wrote become the stale claims, and rule 3
//!    reds. A one-directional gate would silently bless the regression.
//!
//! SHIPPED RELEASE SECTIONS ARE OUT OF SCOPE, DELIBERATELY. `## [0.1.617]`
//! carries the identical caveat at `CHANGELOG.md:7930` and it is CORRECT there —
//! it describes what 0.1.617 shipped. A rule that flagged it would be demanding
//! the project falsify its own history. The subject is `[Unreleased]` plus the
//! forward-looking planning documents.

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

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

fn tool_present(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// The retired attribute form. If this string is emitted again, PMAT-1405 has
/// regressed and every "RETIRED" note this slice wrote is itself false.
const RETIRED_FORM: &str = "@[xpile_contract";

/// Emit `add.py` with the DEFAULT flags — no `--contracts`, so the CLI default
/// applies, which is the exact invocation PMAT-1405 was about.
fn default_lean_emit() -> String {
    // PER-CALL directory. Tests in one binary run in PARALLEL, and three of them
    // call this; a pid-only path let one test's cleanup delete another's input
    // mid-run ("No such file or directory" on add.py). That is [[PMAT-1436]]'s
    // shared-state lesson, re-learned here by the red half.
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("xpile_retired_caveat_{}_{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let src = dir.join("add.py");
    std::fs::write(&src, "def add(a: int, b: int) -> int:\n    return a + b\n").expect("write py");
    let out_file = dir.join("P.lean");
    let out = Command::new(xpile_bin())
        .args(["transpile"])
        .arg(&src)
        .args(["--target", "lean", "--out"])
        .arg(&out_file)
        .output()
        .expect("spawn xpile");
    assert!(
        out.status.success(),
        "`xpile transpile --target lean` failed under DEFAULT flags:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let emitted = std::fs::read_to_string(&out_file).expect("read emitted Lean");
    let _ = std::fs::remove_dir_all(&dir);
    emitted
}

#[test]
fn the_default_lean_emit_does_not_use_the_retired_attribute_form() {
    // THE MEASUREMENT. Structural half — runs with or without a Lean toolchain,
    // so this never degrades into a skip.
    let emitted = default_lean_emit();
    assert!(
        !emitted.contains(RETIRED_FORM),
        "the DEFAULT `--target lean` emit uses {RETIRED_FORM}…, which no Lean prelude registers. \
         PMAT-1405 has REGRESSED, and the `RETIRED` notes PMAT-1473 wrote into CHANGELOG.md and \
         the sprint plan are now themselves false — revert them.\n--- emitted ---\n{emitted}"
    );
    assert!(
        emitted.contains("xpile-contract:"),
        "the default Lean emit no longer carries a `xpile-contract:` citation at all; the citation \
         may have been dropped rather than reformatted.\n--- emitted ---\n{emitted}"
    );
}

#[test]
fn lean_accepts_the_default_emit() {
    // THE BEHAVIOUR HALF. Skips loudly without a toolchain — but the structural
    // half above still runs, so a missing `lean` cannot make this file vacuous.
    if !tool_present("lean") {
        eprintln!(
            "warning: `lean` not on PATH; skipping the elaboration half of \
             XPILE-RETIRED-001. The structural half still ran."
        );
        return;
    }
    let dir = std::env::temp_dir().join(format!("xpile_retired_elab_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let f = dir.join("P.lean");
    std::fs::write(&f, default_lean_emit()).expect("write lean");
    let out = Command::new("lean").arg(&f).output().expect("spawn lean");
    let ok = out.status.success();
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        ok,
        "`lean` REJECTED the default `--target lean` emit. The known-divergence entry PMAT-1473 \
         retired has become true again:\n{stdout}"
    );
}

/// `[Unreleased]` only — shipped sections describe what they shipped.
fn unreleased_section() -> String {
    let body = read("CHANGELOG.md");
    let a = body
        .find("## [Unreleased]")
        .expect("CHANGELOG.md has an [Unreleased] section");
    let rest = &body[a + "## [Unreleased]".len()..];
    let b = rest.find("\n## [").map(|i| i + 1).unwrap_or(rest.len());
    rest[..b].to_string()
}

/// The NORMATIVE blocks — the passages that tell a reader what is true *now*,
/// as opposed to narrative that recounts what was once true.
///
/// Scoping this to whole documents was wrong and the red half proved it: the
/// `[Unreleased]` section carries 77 arc entries, and EIGHT of them legitimately
/// discuss `@[xpile_contract` in the past tense — including PMAT-1405's own
/// entry, which exists precisely to record the removal. Flagging those would
/// demand the project stop narrating its own fixes. **The defect is not a
/// mention; it is an assertion in a section a reader consults for current
/// truth.**
fn normative_blocks() -> Vec<(String, String)> {
    let mut out = Vec::new();

    // CHANGELOG [Unreleased]: only the trailing status sections.
    let un = unreleased_section();
    let mut cur: Option<(String, usize)> = None;
    let lines: Vec<&str> = un.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if let Some(h) = line.strip_prefix("### ") {
            let hl = h.to_ascii_lowercase();
            let normative = hl.contains("known divergence")
                || hl.contains("still refuses")
                || hl.contains("not merge-blocking");
            if let Some((name, start)) = cur.take() {
                out.push((name, lines[start..i].join("\n")));
            }
            if normative {
                cur = Some((format!("CHANGELOG.md#[Unreleased]/{}", h.trim()), i + 1));
            }
        }
    }
    if let Some((name, start)) = cur {
        out.push((name, lines[start..].join("\n")));
    }

    // The active plan's release-note mandate paragraphs.
    let queue = read("docs/roadmaps/queue.yaml");
    let doc: serde_yaml::Value = serde_yaml::from_str(&queue).expect("queue.yaml parses");
    if let Some(plan) = doc
        .get("sprint")
        .and_then(|s| s.get("plan"))
        .and_then(|v| v.as_str())
    {
        if workspace_root().join(plan).is_file() {
            let body = read(plan);
            for (line, par) in paragraphs(&body) {
                let l = par.to_ascii_lowercase();
                if l.contains("must not claim") || l.contains("may honestly claim") {
                    out.push((format!("{plan}:{line}"), par));
                }
            }
        }
    }

    assert!(
        out.len() >= 2,
        "the NORMATIVE corpus collapsed to {:?}; the rule would range over almost nothing. \
         Expected the [Unreleased] status sections plus the plan's release-note mandate.",
        out.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );
    assert!(
        out.iter().any(|(n, _)| n.contains("divergence")),
        "no `Known divergences` section was found in [Unreleased]; that is the section this file \
         exists to police. Sections found: {:?}",
        out.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );
    out
}

/// A paragraph is EXEMPT when it reports the caveat as retired or historical.
/// Positive markers only — a negation screen would pass anything phrased oddly.
fn reports_it_as_past(par: &str) -> bool {
    let l = par.to_ascii_lowercase();
    // Found by the red half: PMAT-1468's own CHANGELOG paragraph reports this
    // correctly with a lowercase "PMAT-1405 retired that", which an uppercase
    // `RETIRED` check flagged as an offender. Case-insensitive, and the verb
    // list covers report/fix/retire rather than one spelling of one of them.
    par.contains("~~")
        || l.contains("retired")
        || l.contains("through v0.1.")
        || l.contains("used to")
        || l.contains("has not been emitted since")
        || l.contains("pmat-1405 fixed")
        || l.contains("pmat-1473")
        || l.contains("no longer")
}

fn paragraphs(body: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let (mut start, mut n) = (1usize, 1usize);
    let mut buf: Vec<&str> = Vec::new();
    for line in body.lines() {
        if line.trim().is_empty() {
            if !buf.is_empty() {
                out.push((start, buf.join("\n")));
                buf.clear();
            }
        } else {
            if buf.is_empty() {
                start = n;
            }
            buf.push(line);
        }
        n += 1;
    }
    if !buf.is_empty() {
        out.push((start, buf.join("\n")));
    }
    out
}

/// Does this text assert the retired caveat as a CURRENT fact?
fn asserts_the_retired_caveat(par: &str) -> bool {
    let l = par.to_ascii_lowercase();
    let says_no_elaborate = l.contains("lean emit does not elaborate")
        || l.contains("default lean emit elaborates")
        || (l.contains("lean") && l.contains("rejects the file"));
    let names_the_attribute = par.contains(RETIRED_FORM);
    says_no_elaborate || names_the_attribute
}

#[test]
fn no_release_facing_document_asserts_the_retired_caveat() {
    // Checked against the MEASUREMENT, not against a hard-coded expectation:
    // if the emitter regressed, the first test reds instead and this one is moot.
    let emitted = default_lean_emit();
    if emitted.contains(RETIRED_FORM) {
        return; // regression — `the_default_lean_emit_…` owns that failure
    }
    let mut offenders = Vec::new();
    for (name, body) in normative_blocks() {
        for (line, par) in paragraphs(&body) {
            if asserts_the_retired_caveat(&par) && !reports_it_as_past(&par) {
                offenders.push(format!(
                    "{name}:{line}: {}",
                    par.chars().take(140).collect::<String>().replace('\n', " ")
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "\na release-facing document asserts the RETIRED Lean caveat as current:\n  {}\n\n\
         Measured on this tree: the default `--target lean` emit is\n{}\n\
         and `lean` accepts it. PMAT-1405 fixed this; a known-divergences section that reports a \
         FIXED defect is the same class of falsehood as one that omits a live one.",
        offenders.join("\n  "),
        emitted.trim(),
    );
}

#[test]
fn the_shipped_release_section_keeps_its_historical_caveat() {
    // THE CONTROL THAT MUST STAY GREEN. `## [0.1.617]` describes what 0.1.617
    // shipped and is CORRECT there. If a future broadening of the rule starts
    // demanding the project falsify its own history, this reds first.
    let body = read("CHANGELOG.md");
    let shipped_start = body
        .find("\n## [0.1.617]")
        .expect("CHANGELOG.md has a [0.1.617] section");
    let shipped = &body[shipped_start..];
    assert!(
        shipped.contains("Lean emit does not elaborate"),
        "the historical caveat has been removed from `## [0.1.617]`. That section records what \
         0.1.617 SHIPPED and the caveat was true of it — deleting it rewrites history rather than \
         correcting a claim."
    );
    // …and it must be outside the subject this file gates.
    assert!(
        !unreleased_section().contains("Lean emit does not elaborate")
            || reports_it_as_past(&unreleased_section()),
        "[Unreleased] states the caveat without reporting it as retired"
    );
}

#[test]
fn the_pre_fix_text_reds_the_document_rule() {
    // NON-VACUITY. The verbatim sentences this slice replaced.
    const PRE_FIX_CHANGELOG: &str = "**5. The default `--contracts on` Lean emit does not \
         elaborate.** Every backend cites its contracts in comments except Lean, which emits \
         `@[xpile_contract \"C-PY-INT-ARITH\"]` — an attribute no Lean prelude registers, so \
         `lean` rejects the file.";
    const PRE_FIX_PLAN: &str = "that the proof lane is merge-blocking; that the default Lean \
         emit elaborates; that xpile lowers five *input* languages without the Ruchy caveat;";

    for (what, text) in [
        ("CHANGELOG divergence 5", PRE_FIX_CHANGELOG),
        ("sprint plan must-NOT-claim list", PRE_FIX_PLAN),
    ] {
        assert!(
            asserts_the_retired_caveat(text),
            "{what} is not recognised as asserting the caveat — the rule is vacuous against the \
             real defect"
        );
        assert!(
            !reports_it_as_past(text),
            "{what} would be exempted as historical, so the rule could not have caught it"
        );
    }

    // The corrected text must, by contrast, be exempt — otherwise the fix would
    // red its own gate and the next author would "fix" it by deleting the note.
    for (name, body) in normative_blocks() {
        for (line, par) in paragraphs(&body) {
            if asserts_the_retired_caveat(&par) {
                assert!(
                    reports_it_as_past(&par),
                    "{name}:{line} mentions the caveat but is not recognised as reporting it as \
                     past; the corrected wording needs a positive marker"
                );
            }
        }
    }
}
