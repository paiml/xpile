//! XPILE-FRONTEND-CLAIM-001 (PMAT-1433) — a frontend may lower only SOME of
//! the path spellings it claims, and every report said it lowered all of them.
//!
//! ## The defect this locks out
//!
//! `Frontend::lowers_input()` (PMAT-1346) is a WHOLE-FRONTEND boolean over a
//! per-CLAIM property. `bashrs-frontend` earns `true` on `.sh`, and PMAT-1420
//! then made `*.mk`, `Makefile` and `Dockerfile` — all three ROUTED into it by
//! `matches_path` — refuse unconditionally. Nothing in the reporting had the
//! granularity to say so, so both published surfaces read as full support:
//!
//! | surface | said | true of |
//! |---|---|---|
//! | `xpile info` | `- bashrs (sh, bash, zsh, mk)`, unannotated, counted among "4 lowering" | `sh`, `bash`, `zsh` |
//! | `book/src/reference/frontends.md` | Extensions `` `.sh`, `.bash`, `.zsh`, `.mk` `` under Status "✅ **Real POSIX parser**" | `sh`, `bash`, `zsh` |
//!
//! MEASURED against the live registry at abd65d84 (2026-07-28), one probe per
//! frontend driven at every spelling that frontend claims:
//!
//! | claim | disposition |
//! |---|---|
//! | `probe.py`, `probe.pyi`, `probe.c`, `probe.h`, `probe.sh`, `probe.bash`, `probe.zsh`, `probe.wat` | LOWERED |
//! | `probe.mk`, `Makefile`, `Dockerfile`, `probe.ruchy` | REFUSED |
//!
//! `.mk` sits in `extensions()` — the very list both reports print — beside
//! three spellings that work, and `Makefile` / `Dockerfile` are claimed by
//! `matches_path` alone and so appeared in NEITHER report. The surface was
//! over-reported in one direction and under-reported in the other at once.
//!
//! ## Why the existing gates did not catch it
//!
//! `claims_drift.rs`'s `frontend_lowers_input_declaration_matches_behaviour`
//! confronts the declaration with behaviour, which is the right shape — but
//! `FRONTEND_PROBES` carries ONE file name per frontend (`probe.sh` for
//! bashrs), so the confrontation samples one of four claimed extensions and
//! can never reach `.mk`. Its book checks assert the frontends.md table NAMES
//! every registered frontend; no rule read the Extensions cell.
//! `makefile_dialect_refusal_witness.rs` (PMAT-1420) proves the three build
//! drivers refuse — it is about the REFUSAL, not about who says otherwise.
//!
//! This gate is over the CLAIM CLASS, not over `.mk`: every frontend, every
//! spelling it claims, both directions of set equality. A spelling that
//! refuses without being declared reds, and a declared-refused spelling that
//! starts lowering reds too — so implementing the Makefile dialect forces the
//! disclosure to move instead of going quietly stale (PMAT-1431's
//! `notation_surface` shape).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use xpile_frontend::Frontend;

/// A real program in each frontend's own language. Deliberately a SECOND
/// table rather than a reuse of `claims_drift.rs`'s `FRONTEND_PROBES`: that
/// one answers "does this frontend read its language at all", this one answers
/// "does the answer depend on the path spelling", and the anti-vacuity test
/// below needs the SAME bytes to reach both a lowering and a refusing
/// spelling. Every registered frontend must appear here or
/// [`disposition_matrix`] panics.
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

/// What a frontend did with its own probe at one claimed path spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    Lowers,
    Refuses,
    /// `Ok` with an EMPTY module — a wrong answer delivered successfully.
    /// Never a valid declaration; it fails whichever way it is declared.
    Hollow,
}

/// Turn a declared claim spelling into the path it denotes.
///
/// `*.<ext>` → `probe.<ext>`; anything else is an exact extensionless
/// filename and is used verbatim.
fn path_for(claim: &str) -> PathBuf {
    match claim.strip_prefix("*.") {
        Some(ext) => PathBuf::from(format!("probe.{ext}")),
        None => PathBuf::from(claim),
    }
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

/// Every spelling a frontend CLAIMS: one `*.<ext>` per declared extension,
/// plus any extensionless name it declares as refused (`matches_path` is
/// asserted below to actually claim each one, so the union cannot invent a
/// claim that routing does not make).
fn claimed_spellings(f: &dyn Frontend) -> Vec<String> {
    let mut out: BTreeSet<String> = f.extensions().iter().map(|e| format!("*.{e}")).collect();
    out.extend(f.refused_claims().iter().map(|c| c.to_string()));
    out.into_iter().collect()
}

fn observe(f: &dyn Frontend, path: &Path, source: &str) -> Disposition {
    match f.parse_and_lower(path, source) {
        Ok(m) if m.items.is_empty() => Disposition::Hollow,
        Ok(_) => Disposition::Lowers,
        Err(_) => Disposition::Refuses,
    }
}

/// `(frontend name, claim spelling, declared, observed)` for the whole live
/// registry — the same table `xpile info` dispatches through, so this cannot
/// drift from the shipped binary.
fn disposition_matrix() -> Vec<(&'static str, String, Disposition, Disposition)> {
    let session = xpile_core::default_session();
    assert!(
        !session.frontends.is_empty(),
        "default_session() registered zero frontends — the registry moved and \
         every check in this file would pass over an empty set"
    );
    let mut rows = Vec::new();
    for f in &session.frontends {
        let name = f.name();
        let source = probe_for(name);
        let refused: BTreeSet<&str> = f.refused_claims().iter().copied().collect();
        for claim in claimed_spellings(f.as_ref()) {
            let declared = if refused.contains(claim.as_str()) {
                Disposition::Refuses
            } else {
                Disposition::Lowers
            };
            let observed = observe(f.as_ref(), &path_for(&claim), source);
            rows.push((name, claim, declared, observed));
        }
    }
    assert!(
        rows.len() >= session.frontends.len(),
        "fewer claim rows than frontends — claimed_spellings() returned nothing \
         for at least one frontend"
    );
    rows
}

/// THE LOAD-BEARING CHECK. Every claimed spelling, driven, both directions.
#[test]
fn every_claimed_path_spelling_has_the_declared_disposition() {
    let rows = disposition_matrix();
    let mismatched: Vec<String> = rows
        .iter()
        .filter(|(_, _, declared, observed)| declared != observed)
        .map(|(name, claim, declared, observed)| {
            format!("  {name}: `{claim}` declared {declared:?}, observed {observed:?}")
        })
        .collect();
    assert!(
        mismatched.is_empty(),
        "frontend claim disposition disagrees with behaviour:\n{}\n\n\
         `Frontend::refused_claims()` is what `xpile info` and \
         `book/src/reference/frontends.md` print. If a spelling started \
         lowering, REMOVE it from `refused_claims()` (and from the book's \
         `Routed → REFUSED` cell); if one started refusing, ADD it. Do not \
         change this gate.",
        mismatched.join("\n")
    );
}

/// A declared refusal must be a claim ROUTING actually makes. A phantom entry
/// would inflate the disclosure — the book would name a spelling that never
/// reaches this frontend at all — and, because the row above drives whatever
/// is declared, it would also pass that check for free.
#[test]
fn refused_claims_are_claimed_by_routing_and_well_formed() {
    let session = xpile_core::default_session();
    let mut problems: Vec<String> = Vec::new();
    for f in &session.frontends {
        let exts: BTreeSet<&str> = f.extensions().iter().copied().collect();
        for claim in f.refused_claims() {
            let p = path_for(claim);
            if !f.matches_path(&p) {
                problems.push(format!(
                    "  {}: `{claim}` is declared refused but matches_path({}) is false — \
                     this frontend never sees it",
                    f.name(),
                    p.display()
                ));
            }
            match claim.strip_prefix("*.") {
                Some(ext) if !exts.contains(ext) => problems.push(format!(
                    "  {}: `{claim}` names extension `{ext}`, absent from extensions()",
                    f.name()
                )),
                None if p.extension().is_some() => problems.push(format!(
                    "  {}: `{claim}` is neither a `*.<ext>` glob nor an extensionless \
                     filename",
                    f.name()
                )),
                _ => {}
            }
        }
    }
    assert!(
        problems.is_empty(),
        "malformed `refused_claims()` entries:\n{}",
        problems.join("\n")
    );
}

/// The frontend-level boolean and the per-claim list must be ONE fact.
/// `lowers_input() == false` means "routing only", which is exactly "every
/// claim I make is refused" — stated twice, so tie them together rather than
/// letting a future frontend disclose a partial refusal in one place and a
/// total one in the other.
#[test]
fn lowers_input_agrees_with_the_claim_set() {
    let session = xpile_core::default_session();
    for f in &session.frontends {
        let claims = claimed_spellings(f.as_ref());
        let refused: BTreeSet<&str> = f.refused_claims().iter().copied().collect();
        let all_refused = claims.iter().all(|c| refused.contains(c.as_str()));
        assert_eq!(
            !all_refused,
            f.lowers_input(),
            "frontend `{}` declares lowers_input() == {} but {} of its {} claimed \
             spellings are declared refused. A frontend that refuses everything is \
             routing-only; one that refuses some of what it claims is not.",
            f.name(),
            f.lowers_input(),
            refused.len(),
            claims.len()
        );
    }
}

/// ANTI-VACUITY. A refusal is only interesting if the SAME BYTES lower at a
/// spelling that is declared to lower — otherwise the probe program was
/// simply invalid and every row above would agree for the wrong reason. This
/// is the library-level twin of `makefile_dialect_refusal_witness.rs`'s
/// `anti_vacuity_the_same_bytes_at_a_shell_path_still_emit_on_both_backends`,
/// generalised over the registry rather than pinned to bashrs.
#[test]
fn the_same_bytes_lower_at_a_lowering_spelling() {
    let session = xpile_core::default_session();
    let mut exercised = 0usize;
    for f in &session.frontends {
        let refused: BTreeSet<&str> = f.refused_claims().iter().copied().collect();
        if refused.is_empty() || !f.lowers_input() {
            continue;
        }
        let source = probe_for(f.name());
        let lowering: Vec<String> = claimed_spellings(f.as_ref())
            .into_iter()
            .filter(|c| !refused.contains(c.as_str()))
            .collect();
        assert!(
            !lowering.is_empty(),
            "frontend `{}` declares lowers_input() == true with no lowering claim",
            f.name()
        );
        for claim in &refused {
            assert_eq!(
                observe(f.as_ref(), &path_for(claim), source),
                Disposition::Refuses,
                "`{claim}` was expected to refuse for frontend `{}`",
                f.name()
            );
        }
        for claim in &lowering {
            assert_eq!(
                observe(f.as_ref(), &path_for(claim), source),
                Disposition::Lowers,
                "the identical bytes that `{}` refuses at its declared-refused \
                 spellings must LOWER at `{claim}` — otherwise the refusal is \
                 about the PROGRAM, not the path, and this whole file is vacuous",
                f.name()
            );
            exercised += 1;
        }
    }
    assert!(
        exercised > 0,
        "no frontend declares both a refused claim and a lowering claim, so the \
         path-vs-program distinction was never exercised. If the partial-refusal \
         case genuinely disappeared, delete this test in the same commit that \
         removes it — do not let it skip green."
    );
}

// ---------------------------------------------------------------------------
// The two published surfaces. Both are compared to the REGISTRY, not to each
// other, and both directions are asserted.
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/xpile → repo root")
        .to_path_buf()
}

fn xpile_info() -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("info")
        .output()
        .unwrap_or_else(|e| panic!("running `xpile info`: {e}"));
    assert!(out.status.success(), "`xpile info` exited {:?}", out.status);
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// `xpile info` must name every refused claim on the frontend's own line, and
/// must name none on a frontend that has none.
#[test]
fn xpile_info_names_exactly_the_refused_claims() {
    let info = xpile_info();
    let session = xpile_core::default_session();
    for f in &session.frontends {
        let prefix = format!("    - {} (", f.name());
        let line = info
            .lines()
            .find(|l| l.starts_with(&prefix))
            .unwrap_or_else(|| {
                panic!(
                    "`xpile info` prints no line for registered frontend `{}`.\n\
                     --- info ---\n{info}",
                    f.name()
                )
            });
        // A routing-only frontend keeps the PMAT-1346 suffix; the per-claim
        // bracket is the PARTIAL case only, so skip the total one here — it is
        // tied to the claim set by `lowers_input_agrees_with_the_claim_set`.
        if !f.lowers_input() {
            assert!(
                line.contains("[routing only"),
                "`{line}` — a routing-only frontend must keep its PMAT-1346 suffix"
            );
            continue;
        }
        let refused = f.refused_claims();
        if refused.is_empty() {
            assert!(
                !line.contains("claims REFUSED"),
                "`{line}` — frontend `{}` declares no refused claim but the info \
                 line carries the bracket",
                f.name()
            );
            continue;
        }
        let expected = format!("[claims REFUSED — no parser: {}]", refused.join(", "));
        assert!(
            line.contains(&expected),
            "`xpile info` line for `{}` does not carry the exact refused-claim \
             disclosure.\n  line:     {line}\n  expected: …{expected}",
            f.name()
        );
    }
}

/// Parse the pipe-delimited row of `frontends.md` whose Name cell is
/// `` `name` ``. Returns the cells, trimmed. Panics if the table or the row is
/// missing, so a deleted table cannot make this gate pass by having nothing to
/// disagree with (PMAT-1417).
fn frontends_md_row(page: &str, name: &str) -> Vec<String> {
    let needle = format!("`{name}`");
    for line in page.lines() {
        if !line.trim_start().starts_with('|') {
            continue;
        }
        let cells: Vec<String> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim().to_string())
            .collect();
        if cells.len() >= 2 && cells[1] == needle {
            return cells;
        }
    }
    panic!(
        "book/src/reference/frontends.md has no table row whose Name cell is \
         {needle} — the frontend table moved, and this gate must not pass over \
         a page it can no longer read"
    );
}

/// Backticked tokens in a table cell, normalised to claim spellings: a leading
/// `.` becomes `*.` so `` `.sh` `` and `` `*.mk` `` compare against the same
/// vocabulary the registry uses. An em-dash cell is the empty set.
fn cell_claims(cell: &str) -> BTreeSet<String> {
    if cell.trim() == "—" {
        return BTreeSet::new();
    }
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(|t| {
            let t = t.trim();
            match t.strip_prefix('.') {
                Some(ext) => format!("*.{ext}"),
                None => t.to_string(),
            }
        })
        .collect()
}

/// The published table's two path columns must EQUAL what the registry
/// declares. This is the check that would have failed on the `.mk` row.
#[test]
fn book_frontends_table_path_columns_match_the_registry() {
    let p = repo_root().join("book/src/reference/frontends.md");
    let page =
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("reading {}: {e}", p.display()));
    let session = xpile_core::default_session();
    for f in &session.frontends {
        let cells = frontends_md_row(&page, f.name());
        assert!(
            cells.len() >= 4,
            "frontends.md row for `{}` has {} cells; expected at least 4 \
             (Frontend | Name | Extensions that LOWER | Routed → REFUSED | …)",
            f.name(),
            cells.len()
        );
        let refused: BTreeSet<String> = f.refused_claims().iter().map(|c| c.to_string()).collect();
        let lowering: BTreeSet<String> = claimed_spellings(f.as_ref())
            .into_iter()
            .filter(|c| !refused.contains(c))
            .collect();
        assert_eq!(
            cell_claims(&cells[2]),
            lowering,
            "frontends.md `Extensions that LOWER` cell for `{}` disagrees with \
             the registry (cell: {:?})",
            f.name(),
            cells[2]
        );
        assert_eq!(
            cell_claims(&cells[3]),
            refused,
            "frontends.md `Routed → REFUSED` cell for `{}` disagrees with \
             `refused_claims()` (cell: {:?})",
            f.name(),
            cells[3]
        );
    }
}
