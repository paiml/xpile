//! XPILE-LEDGER-003 — the roadmap-ledger **VOCABULARY** gate (PMAT-1399).
//!
//! ## What this exists to catch
//!
//! `docs/roadmaps/roadmap.yaml` is not xpile's private file. `pmat` owns its
//! schema, the repo mandates `pmat query` as the code-search path, and
//! `.git/hooks/pre-commit` tells you to run `pmat work start <ID>`. But xpile
//! had invented **eight `item_type` variants and three `status` values** that
//! `pmat` does not accept, so `pmat work list` exited **4** on every version of
//! this ledger — valid YAML or not:
//!
//! ```text
//! item_type: capability(24) correctness(18) fix(13) provability(11)
//!            verification(7) test(4) verify(1)      -> 78 rows
//! status:    obsolete(45) in_progress(3) open(3)    -> 51 rows
//! plus 2 rows with no `status` field at all
//! ```
//!
//! One unknown variant kills the whole parse, so the hook's own advice was
//! dead advice and every `pmat work` command was unusable against this repo.
//!
//! ## Why the sibling gates did not catch it
//!
//! This is the same **exit-0-but-false** shape as PMAT-1385/1386/1387/1390 and
//! PMAT-1398, one more level out. [`roadmap_registration.rs`] (XPILE-LEDGER-001)
//! matches raw text, so it is blind to schema. [`roadmap_ledger_parses.rs`]
//! (XPILE-LEDGER-002) proves the bytes are *YAML* — which they were, once the
//! apostrophes were fixed — but a document can be perfectly well-formed YAML
//! and still be meaningless to the tool that owns it. Parsing is not
//! conforming.
//!
//! ## Why this gate does not shell out to `pmat`
//!
//! The obvious implementation — run `pmat work list` and assert exit 0 — would
//! **skip green wherever `pmat` is absent**, which is every CI runner here.
//! That is the silent-skip class this repo has spent the release cleaning up
//! (XPILE-WITNESS-001/002). So the accepted vocabulary is inlined from
//! `pmat 3.24.2`'s own error text and checked with `serde_yaml` alone: no
//! external binary, no network, no way to skip.
//!
//! The trade is explicit: this gate can go **stale** if pmat widens its enums.
//! Staleness here is safe in the honest direction — it can only reject a value
//! pmat has newly started accepting, which shows up as a failing test with the
//! offending value named, not as a false green.

use std::fs;
use std::path::{Path, PathBuf};

/// `pmat 3.24.2`, verbatim from its parse error:
/// "unknown variant `verification`, expected one of `task`, `epic`, `bug`,
///  `feature`, `enhancement`, `documentation`, `refactor`"
const PMAT_ITEM_TYPES: &[&str] = &[
    "task",
    "epic",
    "bug",
    "feature",
    "enhancement",
    "documentation",
    "refactor",
];

/// `pmat 3.24.2`, verbatim: "unknown status 'obsolete' (did you mean
/// 'completed'?) Valid values: completed, done, inprogress, wip, planned,
/// todo, blocked, review, cancelled"
const PMAT_STATUSES: &[&str] = &[
    "completed",
    "done",
    "inprogress",
    "wip",
    "planned",
    "todo",
    "blocked",
    "review",
    "cancelled",
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn ledger_path() -> PathBuf {
    workspace_root()
        .join("docs")
        .join("roadmaps")
        .join("roadmap.yaml")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The `roadmap:` sequence, as mappings. Panics with the parser's own message
/// if the document does not load — XPILE-LEDGER-002 owns that failure, but a
/// panic here is still better than a silent pass.
fn ledger_rows() -> Vec<serde_yaml::Mapping> {
    let doc: serde_yaml::Value = serde_yaml::from_str(&read(&ledger_path()))
        .unwrap_or_else(|e| panic!("docs/roadmaps/roadmap.yaml is not valid YAML: {e}"));
    doc.get("roadmap")
        .and_then(|r| r.as_sequence())
        .unwrap_or_else(|| {
            panic!("docs/roadmaps/roadmap.yaml has no top-level `roadmap:` sequence")
        })
        .iter()
        .filter_map(|v| v.as_mapping().cloned())
        .collect()
}

fn field<'a>(row: &'a serde_yaml::Mapping, key: &str) -> Option<&'a str> {
    row.get(serde_yaml::Value::String(key.to_string()))
        .and_then(|v| v.as_str())
}

fn id_of(row: &serde_yaml::Mapping) -> String {
    field(row, "id").unwrap_or("<no id>").to_string()
}

/// Group offenders as `value -> [ids]` so the failure names the fix, not just
/// the first casualty.
fn report(offenders: &[(String, String)], field_name: &str, accepted: &[&str]) -> String {
    let mut by_value: std::collections::BTreeMap<&str, Vec<&str>> = Default::default();
    for (value, id) in offenders {
        by_value.entry(value).or_default().push(id);
    }
    let mut out = String::new();
    for (value, ids) in &by_value {
        let shown: Vec<&str> = ids.iter().take(4).copied().collect();
        let more = ids.len().saturating_sub(shown.len());
        out.push_str(&format!(
            "  {field_name}: {value}  ({} row(s), e.g. {}{})\n",
            ids.len(),
            shown.join(", "),
            if more > 0 {
                format!(", +{more} more")
            } else {
                String::new()
            }
        ));
    }
    out.push_str(&format!("\npmat accepts only: {}\n", accepted.join(", ")));
    out
}

#[test]
fn every_item_type_is_one_pmat_accepts() {
    let offenders: Vec<(String, String)> = ledger_rows()
        .iter()
        .filter_map(|row| {
            let it = field(row, "item_type")?;
            (!PMAT_ITEM_TYPES.contains(&it)).then(|| (it.to_string(), id_of(row)))
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "docs/roadmaps/roadmap.yaml uses {} item_type value(s) pmat cannot parse.\n\
         ONE unknown variant makes `pmat work list` exit 4 over the WHOLE ledger,\n\
         so every `pmat work` command — including the `pmat work start <ID>` the\n\
         pre-commit hook tells you to run — stops working against this repo.\n\n\
         {}\n\
         The mapping used by PMAT-1399: capability->feature, correctness->bug,\n\
         fix->bug, provability->enhancement, verification/verify/test->task.\n\
         The finer distinction is NOT lost: it is already carried, more richly,\n\
         by the bracketed tag in the title (e.g. `[rust/capability]`,\n\
         `[wasm/verification/dict]`). Put new dimensions there, not in a field\n\
         whose schema pmat owns.\n\n\
         Reproduce: cargo run -q -p xpile -- --version >/dev/null; pmat work list; echo $?",
        offenders.len(),
        report(&offenders, "item_type", PMAT_ITEM_TYPES),
    );
}

#[test]
fn every_status_is_one_pmat_accepts() {
    let offenders: Vec<(String, String)> = ledger_rows()
        .iter()
        .filter_map(|row| {
            let st = field(row, "status")?;
            (!PMAT_STATUSES.contains(&st)).then(|| (st.to_string(), id_of(row)))
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "docs/roadmaps/roadmap.yaml uses {} status value(s) pmat cannot parse.\n\n\
         {}\n\
         The mapping used by PMAT-1399: obsolete->cancelled, in_progress->inprogress,\n\
         open->todo.\n\n\
         Reproduce: pmat work list; echo $?",
        offenders.len(),
        report(&offenders, "status", PMAT_STATUSES),
    );
}

/// `status` is not optional to pmat — a row without it fails the whole parse
/// with `missing field \`status\``. Two rows were appended without it
/// (PMAT-1348, PMAT-1351) and nothing noticed, because the textual gate does
/// not know the schema and the parse gate only checks that the bytes are YAML.
#[test]
fn every_row_carries_the_fields_pmat_requires() {
    let missing: Vec<String> = ledger_rows()
        .iter()
        .filter(|row| field(row, "status").is_none())
        .map(id_of)
        .collect();

    assert!(
        missing.is_empty(),
        "{} roadmap row(s) have no `status` field: {}\n\n\
         pmat treats `status` as REQUIRED and fails the entire ledger with\n\
         `missing field \\`status\\`` — one incomplete row breaks every other.\n\
         When appending an entry, copy the full field set from an existing row\n\
         (github_issue, item_type, title, status, priority, created,\n\
         acceptance_criteria, phases, subtasks, estimated_effort, labels, notes).",
        missing.len(),
        missing.join(", "),
    );
}

/// Non-vacuity: if the ledger ever stops having rows, the three assertions
/// above pass trivially. Pin a floor so an empty or truncated file reds.
#[test]
fn the_ledger_is_not_empty() {
    let n = ledger_rows().len();
    assert!(
        n > 1_000,
        "docs/roadmaps/roadmap.yaml parsed to only {n} rows; the other tests in \
         this file pass vacuously over an empty ledger. Expected >1000 \
         (there were 1334 at PMAT-1399)."
    );
}
