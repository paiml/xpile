//! XPILE-CLIDOCS-002 (PMAT-1430) — a `--target` spelling published anywhere
//! in the book must be one the BINARY accepts.
//!
//! ## The defect this locks out
//!
//! `book/src/reference/backends.md` publishes a status table whose second
//! column is headed `` `--target` ``. Measured at 66d8c575, the Shell row's
//! cell read `` `bashrs` `` — and the CLI does not accept it:
//!
//! ```text
//! $ xpile transpile script.sh --target bashrs
//! Error: unknown target `bashrs`; choose: rust, ruchy, ptx, wgsl, spirv, wasm, lean, shell, forjar
//! ```
//!
//! The spelling is `shell`. Eight of the nine rows were right; the ninth sent
//! a reader following the reference page's own `--target` column straight into
//! a hard error, on the page whose entire job is to say what to pass.
//!
//! ## Why the value was wrong, and why correcting it alone would have RED-ed
//!
//! `Backend::name()` and the `--target` spelling are two different strings,
//! and for exactly one backend they differ: `xpile info` prints
//! `- bashrs → Shell`, where `bashrs` is the REGISTRY KEY and `Shell` is the
//! `Target`. `parse_target` (`crates/xpile/src/main.rs`) accepts `shell`,
//! `sh` and `bash` for that variant, and `bashrs` for nothing.
//!
//! The conflation was not a typo — it was ENFORCED.
//! `claims_drift.rs::book_backend_reference_names_every_registered_backend`
//! required backends.md to name every `Backend::name()`, and its own doc
//! comment and failure message called those values "`--target` flag"s. So the
//! page passed BECAUSE it published the registry key in the `--target` column,
//! and editing the cell to `shell` on its own would have turned that gate RED.
//! A gate whose premise is false for one member of the set it walks certifies
//! the defect. PMAT-1430 splits the two claims: backends.md now carries a
//! `Name` column for the registry key (the idiom `frontends.md` already used)
//! and a `--target` column for the CLI spelling, `claims_drift` keeps checking
//! the former and stops calling it the latter, and THIS file checks the latter
//! by executing the binary.
//!
//! ## Why cli_docs_drift.rs did not catch it
//!
//! `cli_docs_drift.rs` (PMAT-1429) checks the `--target` row — on
//! `book/src/reference/cli.md`, the one file it reads. That is PMAT-1417's
//! lesson exactly: the fix was scoped to a FILE, and the same claim CLASS on
//! the next page over stayed live. This gate is scoped to the CLASS and walks
//! the WHOLE `book/src` corpus, in both the table-column form and the inline
//! `--target <x>` prose form.
//!
//! ## Both halves are recorded independently
//!
//! The live set is taken from the binary's own `unknown target` REFUSAL, and
//! then every documented spelling is ALSO EXECUTED. Nothing here is a
//! hard-coded roster (PMAT-1396).
//!
//! ## PMAT-1435 — HALF ONE used to be a CLAIM, and it was short by four
//!
//! Until PMAT-1435 the live set came from `xpile transpile --help`, whose
//! sentence named the nine canonical spellings — while `parse_target` also
//! accepted `wat`, `sh`, `bash` and `forjar-yaml`. This file's doc comment
//! asserted the executed half "could catch a `--help` string that has drifted
//! from `parse_target`". It could not, in the direction that was actually
//! wrong: the executed set is `documented ∪ advertised`, both derived from
//! CLAIMS, so a spelling `--help` OMITS is in neither and is never run.
//!
//! The consequence was directional and live. `live` was the nine, so the two
//! `names_a_live_spelling` checks reported a spelling the CLI **does** accept
//! as one it does not — a gate written to keep the book honest about
//! `--target` FORBADE the book from documenting four real spellings, in a
//! failure message that said "value(s) the CLI does not accept". Same shape as
//! the defect this file was created to lock out, one level up: a gate whose
//! premise is false for part of the set it walks certifies the defect.
//!
//! `target_spelling_help()` now renders the refusal from the same
//! `TARGET_SPELLINGS` roster `parse_target` matches through, so this file can
//! model "what the CLI accepts" from BEHAVIOUR.
//! `target_spelling_disposition_witness.rs` (XPILE-TARGET-SPELL-001) holds
//! that message, `--help` and the book to each other in both directions.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn xpile(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xpile"))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("running `xpile {}`: {e}", args.join(" ")))
}

fn xpile_stdout(args: &[&str]) -> String {
    let out = xpile(args);
    assert!(
        out.status.success(),
        "`xpile {}` exited {:?}",
        args.join(" "),
        out.status.code()
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Every `*.md` under `book/src/`, recursively, as (repo-relative path, body).
/// Walked rather than enumerated: a page added later is covered the moment it
/// lands, which is the failure mode being repaired.
fn book_pages() -> Vec<(String, String)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        paths.sort(); // deterministic order, so a failure message is stable
        for p in paths {
            if p.is_dir() {
                walk(&p, root, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("md") {
                let rel = p
                    .strip_prefix(root)
                    .expect("book page under workspace root")
                    .to_string_lossy()
                    .into_owned();
                let body =
                    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {rel}: {e}"));
                out.push((rel, body));
            }
        }
    }
    let root = workspace_root();
    let mut out = Vec::new();
    walk(&root.join("book/src"), &root, &mut out);
    out
}

/// Every `--target` spelling the RUNNING BINARY accepts. HALF ONE.
///
/// PMAT-1435: this was derived from `xpile transpile --help` and was SHORT BY
/// FOUR. `parse_target` accepted `wat`, `sh`, `bash` and `forjar-yaml`; the
/// help string named only the nine canonical spellings. So the two checks
/// below — whose whole job is to keep the book honest about `--target` —
/// reported a spelling the CLI DOES accept as one it does not, and forbade
/// the book from documenting it truthfully. (Measured: appending
/// ``--target wat`` to `backends.md` red-ed
/// `every_inline_target_flag_in_the_book_names_a_live_spelling` with "value(s)
/// the CLI does not accept: [(…, \"wat\")]".)
///
/// The set now comes from the REFUSAL MESSAGE, which `target_spelling_help`
/// renders from the same `TARGET_SPELLINGS` roster `parse_target` matches
/// through — behaviour, not a second prose claim.
/// `target_spelling_disposition_witness.rs` (XPILE-TARGET-SPELL-001) holds
/// the message to the roster in both directions and against `--help`.
fn accepted_targets() -> BTreeSet<String> {
    // `transpile` reads the input BEFORE parsing `--target`, so the vocabulary
    // is only reachable with a readable file in hand.
    let dir = std::env::temp_dir().join(format!("xpile-tgtvocab-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create probe dir");
    let src = dir.join("probe.py");
    std::fs::write(&src, "def add(a: int, b: int) -> int:\n    return a + b\n").expect("write");
    let out = xpile(&[
        "transpile",
        &src.to_string_lossy(),
        "--target",
        "__no_such_target__",
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let vocab = stderr
        .split("unknown target")
        .nth(1)
        .unwrap_or_else(|| panic!("`--target __no_such_target__` must refuse:\n{stderr}"));
    let after = |k: &str| -> Vec<String> {
        vocab
            .split(k)
            .nth(1)
            .map(|s| {
                s.split(';')
                    .next()
                    .unwrap_or("")
                    .split(',')
                    .map(|t| t.trim().split('=').next().unwrap_or("").trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut set: BTreeSet<String> = after("choose:").into_iter().collect();
    set.extend(after("aliases:"));
    assert!(
        set.len() > 3,
        "parsed only {set:?} target spellings from the refusal message — the \
         message shape changed and every check below would pass vacuously:\n{stderr}"
    );
    set
}

/// Split a markdown table row into trimmed cells (leading/trailing `|` dropped).
fn cells(row: &str) -> Vec<String> {
    let t = row.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_separator_row(row: &str) -> bool {
    cells(row)
        .iter()
        .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'))
}

/// Backticked tokens inside a cell, e.g. ``` `shell` ``` → `["shell"]`.
fn backticked(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Every (page, spelling) published in a markdown-table column headed
/// `--target`, across the whole book corpus.
fn documented_target_cells() -> Vec<(String, String)> {
    let mut found = Vec::new();
    for (rel, body) in book_pages() {
        let lines: Vec<&str> = body.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            if !lines[i].trim_start().starts_with('|') {
                i += 1;
                continue;
            }
            // Header row of a table; the next row must be the separator.
            let header = cells(lines[i]);
            let col = header.iter().position(|c| c.contains("--target"));
            if col.is_none() || i + 1 >= lines.len() || !is_separator_row(lines[i + 1]) {
                i += 1;
                continue;
            }
            let col = col.expect("checked above");
            let mut j = i + 2;
            while j < lines.len() && lines[j].trim_start().starts_with('|') {
                let row = cells(lines[j]);
                if let Some(cell) = row.get(col) {
                    for tok in backticked(cell) {
                        found.push((rel.clone(), tok));
                    }
                }
                j += 1;
            }
            i = j;
        }
    }
    found
}

/// Every inline `--target <x>` INVOCATION in the book.
///
/// Only the invocation form counts — `--target` followed by exactly one space
/// and a lowercase token. The bare flag named in prose (`` `--target` `` — a
/// closing backtick immediately after the flag, then an English sentence) is
/// not a spelling, and neither is a `<TARGET>` placeholder. The first cut of
/// this scanner stripped backticks before reading the token and duly reported
/// that the book publishes the targets `are`, `is` and `also`.
fn documented_inline_targets() -> Vec<(String, String)> {
    let mut found = Vec::new();
    for (rel, body) in book_pages() {
        for line in body.lines() {
            let mut from = 0;
            while let Some(rel_at) = line[from..].find("--target") {
                let at = from + rel_at;
                let rest = &line[at + "--target".len()..];
                from = at + "--target".len();
                let Some(rest) = rest.strip_prefix(' ') else {
                    continue; // `--target`, --targets, end of line — not an invocation
                };
                let tok: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
                    .collect();
                if !tok.is_empty() {
                    found.push((rel.clone(), tok));
                }
            }
        }
    }
    found
}

/// THE LOAD-BEARING CHECK. Every `--target` spelling the book publishes in a
/// `--target` table column is one `xpile transpile --help` advertises.
#[test]
fn every_target_column_cell_in_the_book_names_a_live_spelling() {
    let live = accepted_targets();
    let cells = documented_target_cells();
    assert!(
        cells.len() > 3,
        "found only {} `--target` table cell(s) across book/src — the column \
         header or table shape changed and this gate would pass vacuously",
        cells.len()
    );
    let bad: Vec<&(String, String)> = cells.iter().filter(|(_, t)| !live.contains(t)).collect();
    assert!(
        bad.is_empty(),
        "the book publishes `--target` value(s) the CLI does not accept: {bad:?}\n\
         live spellings: {live:?}\n\
         NOTE: `Backend::name()` is NOT the `--target` spelling — they differ \
         for the shell backend (`bashrs` → `--target shell`). Put the registry \
         key in the `Name` column, not this one."
    );
}

/// Same claim class, prose form: `--target wasm` in a code block or sentence.
#[test]
fn every_inline_target_flag_in_the_book_names_a_live_spelling() {
    let live = accepted_targets();
    let inline = documented_inline_targets();
    assert!(
        inline.len() > 8,
        "found only {} inline `--target <x>` occurrence(s) across book/src — \
         the scan broke and this gate would pass vacuously",
        inline.len()
    );
    let bad: Vec<&(String, String)> = inline.iter().filter(|(_, t)| !live.contains(t)).collect();
    assert!(
        bad.is_empty(),
        "the book uses `--target <x>` with value(s) the CLI does not accept: \
         {bad:?}\nlive spellings: {live:?}"
    );
}

/// HALF TWO, EXECUTED. `--help` is itself a claim; run the binary on every
/// documented spelling and require that it is not rejected as unknown. This
/// catches a `--help` string that has drifted from `parse_target` — which the
/// column check alone could not see, because it trusts `--help`.
#[test]
fn every_documented_target_spelling_is_accepted_by_the_running_binary() {
    let dir = std::env::temp_dir().join(format!("xpile-bedocs1430-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create probe dir");
    let src = dir.join("probe.py");
    std::fs::write(&src, "def add(a: int, b: int) -> int:\n    return a + b\n").expect("write");
    let src = src.to_string_lossy().into_owned();

    let mut spellings: BTreeSet<String> = documented_target_cells()
        .into_iter()
        .map(|(_, t)| t)
        .collect();
    spellings.extend(documented_inline_targets().into_iter().map(|(_, t)| t));
    spellings.extend(accepted_targets());
    assert!(
        spellings.len() > 5,
        "only {spellings:?} to execute — the corpus scan broke"
    );

    for t in &spellings {
        let out = xpile(&["transpile", &src, "--target", t]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        // A backend may legitimately REFUSE this probe program (`shell` and
        // `forjar` want a shell-origin module; `ptx` wants `--hardware`).
        // What must never happen is the target spelling itself being unknown.
        assert!(
            !stderr.contains("unknown target"),
            "`xpile transpile --target {t}` is rejected by the running binary, \
             but that spelling is published in the book and/or `--help`:\n{stderr}"
        );
    }
}

/// The status table's row count must equal the live backend count, so a
/// backend cannot ship undocumented. `claims_drift` checks the registry NAMES
/// appear somewhere in a row; this checks the table's ARITY, which a row that
/// mentions two backends would otherwise satisfy.
#[test]
fn the_backends_page_status_table_has_one_row_per_registered_backend() {
    let info = xpile_stdout(&["info"]);
    let live: usize = info
        .lines()
        .find_map(|l| {
            let l = l.trim();
            l.strip_prefix("backends (")
                .and_then(|r| r.split(')').next())
                .and_then(|n| n.trim().parse::<usize>().ok())
        })
        .expect("`xpile info` must print a `backends (N):` header");
    assert!(
        live > 3,
        "`xpile info` reports {live} backends — registry moved"
    );

    let page = std::fs::read_to_string(workspace_root().join("book/src/reference/backends.md"))
        .expect("read backends.md");
    let lines: Vec<&str> = page.lines().collect();
    let hdr = lines
        .iter()
        .position(|l| l.trim_start().starts_with('|') && cells(l).iter().any(|c| c == "`--target`"))
        .expect("backends.md must publish a status table with a `--target` column");
    let rows = lines[hdr + 2..]
        .iter()
        .take_while(|l| l.trim_start().starts_with('|'))
        .count();
    assert_eq!(
        rows, live,
        "book/src/reference/backends.md's status table has {rows} row(s) but \
         `xpile info` reports {live} registered backend(s)."
    );
}

/// The proof-lane sentence on backends.md must disclose the scaffolds exactly
/// when the binary does. RED IN BOTH DIRECTIONS: an undisclosed scaffold is
/// PMAT-1429's defect one file over, and a disclosure that outlives the
/// scaffold turns the honest fix into a permanent understatement.
#[test]
fn the_backends_page_discloses_the_proof_lane_scaffolds_iff_the_binary_does() {
    let info = xpile_stdout(&["info"]);
    let binary_says_scaffold = info
        .lines()
        .skip_while(|l| !l.trim_start().starts_with("contract_backends ("))
        .skip(1)
        .take_while(|l| l.trim_start().starts_with("- "))
        .any(|l| l.contains("[scaffold"));

    let page = std::fs::read_to_string(workspace_root().join("book/src/reference/backends.md"))
        .expect("read backends.md");
    let sentence = page
        .lines()
        .find(|l| l.contains("contract backend"))
        .unwrap_or_else(|| {
            panic!(
                "backends.md says nothing about the proof lane's contract \
                 backends — this gate cannot pass vacuously"
            )
        });
    // The sentence may wrap; take the paragraph it starts.
    let para: String = page
        .split("\n\n")
        .find(|p| p.contains(sentence))
        .unwrap_or(sentence)
        .to_string();
    let page_says_scaffold = para.to_lowercase().contains("scaffold");

    assert_eq!(
        page_says_scaffold, binary_says_scaffold,
        "book/src/reference/backends.md and the binary disagree about whether \
         the proof-lane contract backends actually render a contract.\n\
         binary (`xpile info`) marks a scaffold: {binary_says_scaffold}\n\
         page discloses a scaffold:            {page_says_scaffold}\n\
         paragraph:\n{para}\n\
         If a scaffold became real, DROP the disclosure here as well as in \
         `xpile info` — an understatement is as false as an overstatement."
    );
}
