//! XPILE-LEDGER-002 — the roadmap-ledger **PARSE** gate (PMAT-1398).
//!
//! ## What this exists to catch
//!
//! `docs/roadmaps/roadmap.yaml` was **invalid YAML** from 968d352d until
//! PMAT-1398, and nothing in the repo noticed for five commits. The PMAT-1388
//! ledger row's `description:` is a single-quoted YAML scalar and three
//! apostrophes inside it were written with one quote instead of the doubled
//! `''` that a single-quoted scalar requires (`xpile's output`,
//! `box's Vulkan adapter`, `crate's own dummy_module()`). The first lone quote
//! *ends* the scalar, so the remaining prose is re-parsed as YAML structure and
//! the document dies at the first line that looks like a mapping key. The same
//! entry escapes `audit''s` correctly four lines earlier — a slip, not a
//! convention, which is exactly the kind of thing a gate is for.
//!
//! The reason it went unseen is the interesting part, and it is the same
//! exit-0-but-false shape as PMAT-1385/1386/1387/1390 — one level up, in the
//! *gate* rather than in the tool. All three assertions of the sibling
//! [`roadmap_registration.rs`] (XPILE-LEDGER-001) are pure **text**: `contains`
//! and line-prefix matching over the raw bytes. `xpile attestations` and
//! `xpile quorum` scan the same file textually too. So the whole toolchain
//! reported the ledger GREEN over a file that no YAML parser on earth could
//! read. A text scan cannot tell the difference between a document and a
//! plausible-looking pile of lines.
//!
//! ## The four assertions
//!
//! 1. [`every_yaml_file_under_docs_roadmaps_loads_in_a_real_parser`] — the
//!    direct fix for the above. The file set is **globbed from the directory**,
//!    never hand-listed, so a `docs/roadmaps/*.yaml` added tomorrow is covered
//!    the day it lands rather than the day someone remembers to add it here.
//! 2. [`the_roadmap_ledger_is_a_sequence_of_unique_id_bearing_mappings`] — the
//!    structural shape that XPILE-LEDGER-001 *assumes* and never checks:
//!    `roadmap:` is a sequence, every element is a mapping carrying a non-empty
//!    scalar `id`, and no id is registered twice.
//! 3. [`the_queue_is_a_sequence_of_id_bearing_mappings`] — the same for
//!    `queue.yaml`, whose `status:` rows drive the next-pick loop.
//! 4. [`the_text_scan_the_registration_gate_uses_agrees_with_the_parser`] — the
//!    load-bearing one. It re-derives the ledger's id set **both ways** — the
//!    byte-identical `- id: ` line scan XPILE-LEDGER-001 uses, and
//!    `serde_yaml`'s view — and demands they be equal. That is a relation
//!    between two live implementations rather than a hand-copied expected
//!    value, so it cannot go stale as the ledger grows, and it fails on the
//!    whole failure *class*: a truncated document, a row swallowed into a
//!    string scalar, an `id` nested one level too deep. Any of those leaves the
//!    text scan happily counting rows the parser does not see.
//!
//! `serde_yaml` is already a dev-dependency of this crate (PMAT-484), so this
//! adds no crate to the graph. Everything here is `std::fs` plus that parser:
//! no git, no network, no runtime linkage, so it can never skip — it runs for
//! real inside the REQUIRED `workspace-test` context even in a shallow clone or
//! an extracted `.crate`.
//!
//! [`roadmap_registration.rs`]: ./roadmap_registration.rs

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn roadmaps_dir() -> PathBuf {
    workspace_root().join("docs").join("roadmaps")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Every `*.yaml` / `*.yml` directly under `docs/roadmaps/`, sorted. Globbed
/// from the directory on purpose: a hand-listed set silently stops covering the
/// file someone adds next week.
fn roadmap_yaml_files() -> Vec<PathBuf> {
    let dir = roadmaps_dir();
    let mut found: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .map(|e| e.expect("dir entry").path())
        .filter(|p| {
            p.is_file()
                && matches!(
                    p.extension().and_then(|e| e.to_str()),
                    Some("yaml") | Some("yml")
                )
        })
        .collect();
    found.sort();
    found
}

/// Parse a YAML document, or return the parser's own message.
fn parse(path: &Path) -> Result<serde_yaml::Value, String> {
    serde_yaml::from_str::<serde_yaml::Value>(&read(path)).map_err(|e| e.to_string())
}

fn parse_or_panic(rel: &str) -> serde_yaml::Value {
    let path = roadmaps_dir().join(rel);
    parse(&path).unwrap_or_else(|e| {
        panic!(
            "docs/roadmaps/{rel} is not valid YAML: {e}\n\n\
             Reproduce: python3 -c \"import yaml; \
             yaml.safe_load(open('docs/roadmaps/{rel}'))\"\n\
             Most likely cause (PMAT-1398): an apostrophe inside a \
             single-quoted YAML scalar written as `'` instead of the doubled \
             `''` the format requires."
        )
    })
}

/// The id rows of a top-level sequence, as `serde_yaml` sees them.
fn parsed_ids(doc: &serde_yaml::Value, key: &str, rel: &str) -> Vec<String> {
    let seq = doc
        .get(key)
        .unwrap_or_else(|| panic!("docs/roadmaps/{rel} has no top-level `{key}:` key"))
        .as_sequence()
        .unwrap_or_else(|| panic!("docs/roadmaps/{rel}: `{key}:` is not a YAML sequence"));

    seq.iter()
        .enumerate()
        .map(|(i, item)| {
            let map = item.as_mapping().unwrap_or_else(|| {
                panic!("docs/roadmaps/{rel}: `{key}[{i}]` is not a mapping — got {item:?}")
            });
            let id = map
                .get(serde_yaml::Value::from("id"))
                .unwrap_or_else(|| panic!("docs/roadmaps/{rel}: `{key}[{i}]` has no `id:` key"));
            let id = id.as_str().unwrap_or_else(|| {
                panic!("docs/roadmaps/{rel}: `{key}[{i}].id` is not a string — got {id:?}")
            });
            assert!(
                !id.trim().is_empty(),
                "docs/roadmaps/{rel}: `{key}[{i}].id` is empty"
            );
            id.trim().to_string()
        })
        .collect()
}

// ── 1. Every roadmap YAML file loads in a real parser ───────────────────────

#[test]
fn every_yaml_file_under_docs_roadmaps_loads_in_a_real_parser() {
    let files = roadmap_yaml_files();
    assert!(
        files.len() >= 2,
        "expected at least roadmap.yaml + queue.yaml under {} — found {}. \
         Either the ledger moved or this glob has gone blind.",
        roadmaps_dir().display(),
        files.len()
    );

    let failures: Vec<String> = files
        .iter()
        .filter_map(|p| parse(p).err().map(|e| format!("  {}: {e}", p.display())))
        .collect();

    assert!(
        failures.is_empty(),
        "{} of {} YAML file(s) under docs/roadmaps/ do NOT parse:\n{}\n\n\
         A text-scanning gate (XPILE-LEDGER-001, `xpile attestations`, \
         `xpile quorum`) will report GREEN over an unparseable ledger — that is \
         precisely why this test exists. Fix the YAML, do not widen the gate.",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}

// ── 2. The ledger's structural shape ────────────────────────────────────────

#[test]
fn the_roadmap_ledger_is_a_sequence_of_unique_id_bearing_mappings() {
    let doc = parse_or_panic("roadmap.yaml");
    let ids = parsed_ids(&doc, "roadmap", "roadmap.yaml");

    assert!(
        ids.len() > 1_000,
        "docs/roadmaps/roadmap.yaml parsed to only {} row(s) — the ledger has \
         been truncated, or its shape changed and this gate has gone blind",
        ids.len()
    );

    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for id in &ids {
        *seen.entry(id.as_str()).or_default() += 1;
    }
    let dupes: Vec<String> = seen
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|(id, n)| format!("{id} ×{n}"))
        .collect();
    assert!(
        dupes.is_empty(),
        "docs/roadmaps/roadmap.yaml registers {} id(s) more than once: {}. \
         A duplicated row makes the registration gate satisfiable by the wrong \
         entry.",
        dupes.len(),
        dupes.join(", ")
    );
}

#[test]
fn the_queue_is_a_sequence_of_id_bearing_mappings() {
    let doc = parse_or_panic("queue.yaml");
    let ids = parsed_ids(&doc, "queue", "queue.yaml");
    assert!(
        ids.len() >= 10,
        "docs/roadmaps/queue.yaml parsed to only {} row(s) — the next-pick \
         source of truth has gone empty or changed shape",
        ids.len()
    );
}

// ── 3. The text scan and the parser must agree ──────────────────────────────

#[test]
fn the_text_scan_the_registration_gate_uses_agrees_with_the_parser() {
    let path = roadmaps_dir().join("roadmap.yaml");

    // Byte-identical to `registered_ids()` in roadmap_registration.rs — this is
    // the view XPILE-LEDGER-001, `xpile attestations` and `xpile quorum` all
    // take of the ledger.
    let by_text: BTreeSet<String> = read(&path)
        .lines()
        .filter_map(|l| l.strip_prefix("- id: "))
        .map(str::trim)
        .filter(|id| id.starts_with("PMAT-"))
        .map(str::to_string)
        .collect();

    let doc = parse_or_panic("roadmap.yaml");
    let by_parser: BTreeSet<String> = parsed_ids(&doc, "roadmap", "roadmap.yaml")
        .into_iter()
        .filter(|id| id.starts_with("PMAT-"))
        .collect();

    // Vacuity guard on BOTH sides: an empty-vs-empty comparison agrees
    // perfectly and proves nothing.
    assert!(
        by_text.len() > 1_000 && by_parser.len() > 1_000,
        "vacuous comparison — text scan saw {} id(s), parser saw {}. Both \
         should be the full ledger.",
        by_text.len(),
        by_parser.len()
    );

    let text_only: Vec<&String> = by_text.difference(&by_parser).collect();
    let parser_only: Vec<&String> = by_parser.difference(&by_text).collect();
    assert!(
        text_only.is_empty() && parser_only.is_empty(),
        "the `- id: ` TEXT scan and the YAML PARSER disagree about \
         docs/roadmaps/roadmap.yaml.\n\
         seen only by the text scan ({}): {:?}\n\
         seen only by the parser   ({}): {:?}\n\n\
         The text scan is what the registration gate and the `xpile \
         attestations` / `xpile quorum` reporters use. When the two views \
         diverge, those tools are counting rows that are not in the document \
         (a row swallowed into a string scalar, a truncated file) or missing \
         rows that are.",
        text_only.len(),
        text_only,
        parser_only.len(),
        parser_only
    );
}
