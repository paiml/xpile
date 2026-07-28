//! XPILE-FRONTEND-CLAIM-002 (PMAT-1434) — the DISPATCH-FAILURE message is the
//! third surface of the frontend claim class, and it read neither disposition.
//!
//! ## The defect this locks out
//!
//! When no frontend claims a file, `xpile transpile` answers with the one
//! message whose entire job is to say what the reader could use instead.
//! Measured at `da411cef` (2026-07-28) on a force-rebuilt binary:
//!
//! ```text
//! $ xpile transpile notes.txt --target rust
//! Error: no frontend handles `.txt`; known extensions: ["py", "pyi", "c", "h",
//!        "ruchy", "sh", "bash", "zsh", "mk", "wat"]
//! ```
//!
//! Two of those ten spellings refuse EVERY input:
//!
//! ```text
//! $ xpile transpile probe.ruchy --target rust   # exit 1
//! the Ruchy frontend has no parser — Ruchy is an OUTPUT language only
//! $ xpile transpile probe.mk --target rust      # exit 1
//! `probe.mk` is a Makefile, and there is no Makefile dialect
//! ```
//!
//! and the two extensionless spellings `matches_path` claims, `Makefile` and
//! `Dockerfile`, appeared in it not at all. Over-reported and under-reported in
//! one line — PMAT-1433's finding exactly, on the surface PMAT-1433 did not
//! reach.
//!
//! ## Why the list was wrong, and why it is nobody's mistake
//!
//! `extensions()` is a ROUTING table, and keeping a refusing spelling in it is
//! a DELIBERATE decision recorded in three places: `ruchy-frontend`'s module
//! doc ("emptying `extensions()` would make the refusal dead code"),
//! `bashrs-frontend` at the `*.mk` guard, and `Frontend::lowers_input`'s own
//! doc comment, which justifies routing-only registration as getting the file
//! "a specific refusal … instead of the generic `no frontend handles .<ext>`
//! message". That decision is right. It also makes `extensions()` a set of
//! spellings xpile ROUTES, and this message was printing it as the set of
//! spellings xpile READS.
//!
//! ## Why the existing gates did not catch it
//!
//! `frontend_claim_disposition_witness.rs` (XPILE-FRONTEND-CLAIM-001,
//! PMAT-1433) fixed the claim class in the two surfaces it enumerated —
//! `xpile info` and `book/src/reference/frontends.md`. Neither it nor
//! `claims_drift.rs` reads a message on the ERROR path, and no test anywhere
//! asserted on this string: `grep -rn "known extensions"` over the whole tree
//! matched exactly one line, the `format!` that produced it.
//!
//! ## What this file asserts, and what it deliberately does not
//!
//! The two published lists are compared to the REGISTRY (not to `xpile info`,
//! not to the book), in both directions, in the `*.<ext>` vocabulary
//! `refused_claims()` uses. That the registry's declarations match real
//! behaviour at every claimed spelling is XPILE-FRONTEND-CLAIM-001's job and is
//! not restated here — but a purely transitive gate would prove nothing about
//! the message itself, so every advertised spelling is additionally DRIVEN
//! through the shipped binary: each must be genuinely routed (it must not come
//! back with this very message), each advertised refusal must really fail, and
//! at least one advertised lowering spelling must really transpile at exit 0.
//!
//! ## The half this does NOT close, stated so it is not mistaken for covered
//!
//! `published ⊆ routed` is checked by driving every published spelling. The
//! converse, `routed ⊆ published`, is NOT — and cannot be from here, because
//! `Frontend::matches_path` is a PREDICATE and the registry exposes no
//! enumeration of the extensionless names it claims. Today the only override is
//! `bashrs-frontend`'s, and every name it adds is declared in
//! `refused_claims()`, so the union above happens to be complete. A frontend
//! that overrode `matches_path` to claim a spelling it LOWERS would be invisible
//! to both this message and `xpile info`, exactly the way `Makefile` was before
//! PMAT-1433. Closing that needs a declaration of the claim set itself (a
//! `claimed_filenames()` peer to `extensions()`), which is PMAT-1433's standing
//! lead (c) and deliberately not folded in here.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The marker introducing each list in the message under test.
const LOWERS_MARKER: &str = "spellings that LOWER: ";
const REFUSED_MARKER: &str = "ROUTED but REFUSED (no parser): ";

/// A file extension no registered frontend claims. Asserted to be unclaimed by
/// [`the_probe_extension_is_unclaimed_and_the_lists_are_not_empty`] rather than
/// assumed — the whole file is a scan of a message that only appears when
/// dispatch fails, so an extension that quietly became claimed would make every
/// test here fail to obtain its subject.
const UNCLAIMED_EXT: &str = "xpileprobe";

/// Bytes that LOWER at `*.sh` (see the path-vs-program control below), so a
/// refusal at a bashrs-claimed spelling is a fact about the PATH.
const SHELL_PROBE: &str = "echo hello\n";

/// A per-CALL temp directory. Not per-test: several checks below spawn the
/// binary many times, and a shared directory would let one call's leftovers
/// satisfy another's assertion.
fn scratch(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("xpile-claim002-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("create {}: {e}", dir.display()));
    dir
}

/// `xpile transpile <dir>/<file> --target <target>` → (exit ok, stderr).
fn transpile(file: &str, contents: &str, target: &str, tag: &str) -> (bool, String) {
    let dir = scratch(tag);
    let path = dir.join(file);
    std::fs::write(&path, contents).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .args([
            "transpile",
            path.to_str().expect("utf-8 path"),
            "--target",
            target,
        ])
        .output()
        .unwrap_or_else(|e| panic!("running xpile transpile {file}: {e}"));
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let ok = out.status.success();
    let _ = std::fs::remove_dir_all(&dir);
    (ok, stderr)
}

/// The dispatch-failure message for a file `name` that no frontend claims.
fn dispatch_failure_message(name: &str) -> String {
    let (ok, stderr) = transpile(name, "irrelevant bytes\n", "rust", "msg");
    assert!(
        !ok,
        "`xpile transpile {name}` SUCCEEDED — some frontend now claims it, so \
         the dispatch-failure message could not be obtained and every check in \
         this file would have nothing to read"
    );
    assert!(
        stderr.contains("no frontend handles"),
        "`xpile transpile {name}` failed for a different reason than dispatch; \
         this file is checking the wrong string.\n--- stderr ---\n{stderr}"
    );
    stderr
}

/// Read the `["a", "b", …]` debug list that follows `marker` in `msg`.
/// Returns `None` when the marker is absent (an empty half is omitted).
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

/// What the REGISTRY declares, in the message's vocabulary: `(lowers, refused)`.
///
/// Recomputed here from `extensions()` + `refused_claims()` rather than calling
/// into the binary's own helper, so the message is checked against the registry
/// and not against itself.
fn registry_split() -> (BTreeSet<String>, BTreeSet<String>) {
    let session = xpile_core::default_session();
    assert!(
        !session.frontends.is_empty(),
        "default_session() registered zero frontends — the registry moved and \
         every set comparison below would hold over two empty sets"
    );
    let mut lowers = BTreeSet::new();
    let mut refused = BTreeSet::new();
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
        refused.extend(declared.iter().map(|c| (*c).to_string()));
    }
    (lowers, refused)
}

/// Every spelling the registry claims — the union the message must partition.
fn claimed_spellings() -> BTreeSet<String> {
    let (lowers, refused) = registry_split();
    lowers.union(&refused).cloned().collect()
}

/// A claim spelling as a concrete file name: `*.<ext>` → `probe.<ext>`,
/// anything else verbatim. Mirrors the vocabulary in
/// `frontend_claim_disposition_witness.rs`.
fn file_for(claim: &str) -> String {
    match claim.strip_prefix("*.") {
        Some(ext) => format!("probe.{ext}"),
        None => claim.to_string(),
    }
}

// ---------------------------------------------------------------------------
// The message vs the registry — both directions.
// ---------------------------------------------------------------------------

/// THE LOAD-BEARING CHECK. The list the reader is told to choose from must be
/// exactly the spellings that lower, and the refusals must be exactly the
/// spellings that are routed and then refused.
#[test]
fn dispatch_failure_message_matches_the_registry_disposition_split() {
    let msg = dispatch_failure_message(&format!("probe.{UNCLAIMED_EXT}"));
    let (lowers, refused) = registry_split();

    let published_lowers = list_after(&msg, LOWERS_MARKER).unwrap_or_else(|| {
        panic!("the dispatch-failure message carries no {LOWERS_MARKER:?} list:\n{msg}")
    });
    assert_eq!(
        published_lowers, lowers,
        "the dispatch-failure message's LOWER list disagrees with the registry. \
         It is the answer to \"what can I use instead\"; a spelling in it that \
         refuses every input sends the reader into a hard error, and one missing \
         from it hides a language xpile reads.\n--- message ---\n{msg}"
    );

    let published_refused = list_after(&msg, REFUSED_MARKER).unwrap_or_default();
    assert_eq!(
        published_refused, refused,
        "the dispatch-failure message's REFUSED list disagrees with \
         `Frontend::refused_claims()`. Do not edit this gate: if a spelling \
         started lowering, take it out of `refused_claims()` (and out of the \
         book's `Routed → REFUSED` cell) — the message is derived from there.\
         \n--- message ---\n{msg}"
    );
}

/// The extensionless dispatch failure is a SECOND branch of the same
/// `with_context` closure (`filename \`Makefile\`` vs `` `.mk` ``), and a fix
/// applied to one branch of a function is not a fix to the function
/// (PMAT-1387). It must carry the identical lists.
#[test]
fn the_extensionless_dispatch_failure_carries_the_same_lists() {
    let by_ext = dispatch_failure_message(&format!("probe.{UNCLAIMED_EXT}"));
    let by_name = dispatch_failure_message("xpile-probe-with-no-extension");
    assert!(
        by_name.contains("filename `xpile-probe-with-no-extension`"),
        "the extensionless branch did not produce its own label — this test is \
         reading the same branch twice:\n{by_name}"
    );
    for marker in [LOWERS_MARKER, REFUSED_MARKER] {
        assert_eq!(
            list_after(&by_ext, marker),
            list_after(&by_name, marker),
            "the {marker:?} list differs between the extension and the \
             extensionless dispatch-failure branches"
        );
    }
}

/// Structural, so no roster is written down anywhere (PMAT-1396): the two
/// published lists must PARTITION the claimed spellings. A spelling in both is
/// a contradiction; one in neither is the under-reporting half of the defect.
#[test]
fn the_two_published_lists_partition_every_claimed_spelling() {
    let msg = dispatch_failure_message(&format!("probe.{UNCLAIMED_EXT}"));
    let published_lowers = list_after(&msg, LOWERS_MARKER).unwrap_or_default();
    let published_refused = list_after(&msg, REFUSED_MARKER).unwrap_or_default();

    let both: Vec<&String> = published_lowers.intersection(&published_refused).collect();
    assert!(
        both.is_empty(),
        "spelling(s) {both:?} are published as BOTH lowering and refused"
    );

    let published: BTreeSet<String> = published_lowers
        .union(&published_refused)
        .cloned()
        .collect();
    assert_eq!(
        published,
        claimed_spellings(),
        "the two published lists do not cover exactly the spellings the \
         registry claims. `Makefile` and `Dockerfile` are claimed by \
         `matches_path` alone and were in neither list until PMAT-1434.\
         \n--- message ---\n{msg}"
    );
}

// ---------------------------------------------------------------------------
// The message vs the shipped binary — so this file is not purely transitive
// through XPILE-FRONTEND-CLAIM-001.
// ---------------------------------------------------------------------------

/// Every spelling the message publishes must actually be ROUTED: driving the
/// path its own text denotes must NOT come back with this same message. This
/// is what makes the message's VOCABULARY load-bearing — before PMAT-1434 the
/// list read `["py", "pyi", …]`, bare extension names, which denote a file
/// literally called `py` that no frontend claims.
#[test]
fn every_published_spelling_is_actually_routed() {
    let msg = dispatch_failure_message(&format!("probe.{UNCLAIMED_EXT}"));
    let published: BTreeSet<String> = list_after(&msg, LOWERS_MARKER)
        .unwrap_or_default()
        .union(&list_after(&msg, REFUSED_MARKER).unwrap_or_default())
        .cloned()
        .collect();
    assert!(
        published.len() > 1,
        "the message publishes {} spelling(s) — nothing to drive:\n{msg}",
        published.len()
    );
    let mut unrouted = Vec::new();
    for claim in &published {
        let (_, stderr) = transpile(&file_for(claim), SHELL_PROBE, "rust", "routed");
        if stderr.contains("no frontend handles") {
            unrouted.push(claim.clone());
        }
    }
    assert!(
        unrouted.is_empty(),
        "spelling(s) {unrouted:?} are published by the dispatch-failure message \
         but fall through to that very message when driven — as written they \
         reach no frontend at all.\n--- message ---\n{msg}"
    );
}

/// The published refusals are real, and they are about the PATH, not the
/// program: the SAME bytes that every one of them rejects must transpile at
/// exit 0 at a shell spelling. Without that control, a refusal everywhere would
/// satisfy the check for the wrong reason (the PMAT-1410 vacuity shape).
#[test]
fn published_refusals_really_refuse_while_the_same_bytes_lower_at_a_shell_path() {
    let msg = dispatch_failure_message(&format!("probe.{UNCLAIMED_EXT}"));
    let refused = list_after(&msg, REFUSED_MARKER).unwrap_or_default();
    let (_, declared_refused) = registry_split();
    assert!(
        !declared_refused.is_empty(),
        "the registry declares no refused claim, so the REFUSED half of the \
         message is empty and this check covers nothing. If partial refusal \
         genuinely disappeared, delete this test in the commit that removes it \
         rather than letting it skip green."
    );
    assert!(
        !refused.is_empty(),
        "the registry declares {} refused claim(s) and the dispatch-failure \
         message publishes NONE — the reader is told what to choose from and \
         not that {} of the spellings routed by xpile refuse every input.\
         \n--- message ---\n{msg}",
        declared_refused.len(),
        declared_refused.len()
    );
    for claim in &refused {
        let (ok, stderr) = transpile(&file_for(claim), SHELL_PROBE, "shell", "refuse");
        assert!(
            !ok,
            "`{claim}` is published as REFUSED but `xpile transpile` exited 0 \
             for it. Take it out of `refused_claims()` — the message, \
             `xpile info` and the book all read that one declaration."
        );
        assert!(
            !stderr.contains("no frontend handles"),
            "`{claim}` is published as REFUSED but is not routed anywhere:\n{stderr}"
        );
    }
    let (ok, stderr) = transpile("probe.sh", SHELL_PROBE, "shell", "control");
    assert!(
        ok,
        "the identical bytes every published refusal rejects must LOWER at \
         `probe.sh` — otherwise the refusals above are about the PROGRAM and \
         this test is vacuous.\n{stderr}"
    );
}

/// Anti-vacuity for the end-to-end direction: at least one published lowering
/// spelling must really transpile at exit 0, so the LOWER list cannot be a set
/// of things that all fail further down.
#[test]
fn a_published_lowering_spelling_transpiles_end_to_end() {
    let (lowers, _) = registry_split();
    assert!(
        lowers.contains("*.py"),
        "the registry no longer claims `*.py`; pick another end-to-end anchor \
         rather than deleting this check. Live LOWER set: {lowers:?}"
    );
    let (ok, stderr) = transpile(
        "probe.py",
        "def add(a: int, b: int) -> int:\n    return a + b\n",
        "rust",
        "e2e",
    );
    assert!(
        ok,
        "`xpile transpile probe.py --target rust` failed, so the LOWER list is \
         published without a single demonstrated member:\n{stderr}"
    );
}

/// Anti-vacuity for the whole file. Every test above obtains its subject by
/// making dispatch FAIL, so an extension that quietly became claimed, or an
/// empty registry, would take the subject away.
#[test]
fn the_probe_extension_is_unclaimed_and_the_lists_are_not_empty() {
    let session = xpile_core::default_session();
    let probe = PathBuf::from(format!("probe.{UNCLAIMED_EXT}"));
    let claimants: Vec<&str> = session
        .frontends
        .iter()
        .filter(|f| f.matches_path(&probe))
        .map(|f| f.name())
        .collect();
    assert!(
        claimants.is_empty(),
        "frontend(s) {claimants:?} now claim `.{UNCLAIMED_EXT}` — choose a \
         different unclaimed extension for this file's probe"
    );
    let (lowers, refused) = registry_split();
    assert!(
        !lowers.is_empty() && !refused.is_empty(),
        "registry split is degenerate (lowers: {lowers:?}, refused: {refused:?}) \
         — the partition and both-directions checks would hold trivially"
    );
}
