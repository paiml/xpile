//! XPILE-ATTESTTALLY-001 — `xpile attestations` reports a work-item tally it
//! actually measured, in a payload that parses (PMAT-1390).
//!
//! `attestations` is the Extrinsic-stratum scoreboard of the ruchy 5.0 §14.4
//! quorum. Two defects shipped in it, both at exit 0:
//!
//!   (a) PHANTOM WORK ITEM. `scan_roadmap_for_id` seeded the enclosing work
//!       item as `String::new()` and only reassigned on a column-0 `- id: `
//!       line. `docs/roadmaps/roadmap.yaml` opens with ~189 lines of
//!       `strategic_goals:` prose (the `roadmap:` key is line 190, the first
//!       `- id:` is 191) that mention contract ids freely, so every preamble
//!       mention was attributed to a work item whose id is the empty string —
//!       and both printers folded that into the unique-work-item set. Live:
//!
//!           C-PY-INT-ARITH   87 mentions across 69 work item(s)
//!                 - <a nameless bullet>
//!
//!       against 68 by a real YAML parse. Same +1 on C-PY-FLOAT-ARITH (11 vs
//!       10) and C-XLATE-PY-CLASS-TO-STRUCT (5 vs 4); three nameless bullets
//!       in the text report; `"work_items":[""]` in the JSON.
//!
//!   (b) INVALID JSON. Of the six strings in the hand-rolled `--json` payload
//!       only `snippet` was routed through `escape_json`. No exotic filename
//!       was needed to break it — a plain YAML-quoted work-item id (`- id:
//!       "P"`, taken verbatim, unlike the sibling `extract_metadata_id` which
//!       strips quotes) emitted `"work_item":""P""`, which `json.load`
//!       rejects.
//!
//! The properties held here:
//!
//!   (1) a mention in the preamble adds NO work item, prints no nameless
//!       bullet, and is DISCLOSED rather than dropped — dropping it would
//!       lower the mention COUNT that `xpile quorum` scores the Extrinsic
//!       stratum from (main.rs, `row.extrinsic = scan_roadmap_for_id(..).len()`),
//!       trading one wrong number for another;
//!   (2) a quoted work-item id survives as data and the payload parses;
//!   (3) on the LIVE corpus, every contract's reported work-item tally equals
//!       an INDEPENDENT reference computed with a real YAML parser
//!       (`serde_yaml`), not with the line-scan under test. Pre-fix this
//!       disagreed on 3 of 34 contracts.
//!
//! Property (3) carries a vacuity guard on both sides: the live corpus must
//! yield ≥10 contracts and some contract must have ≥10 attesting work items,
//! so a future change that empties the report cannot pass this file.
//!
//! No external toolchain is involved — the subject is the shipped `xpile`
//! binary and two dev-dependencies already in the graph — so this witness has
//! no skip path and always executes.

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_xpile"))
}

/// An integration test's CWD is the PACKAGE root (`crates/xpile`), not the
/// workspace root — the confusion that left four `audit` tests reading a
/// nonexistent fixture path until PMAT-1385.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

/// A scratch corpus unique to the CALL, not to the test — two probes in one
/// test would otherwise share a directory and race.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "xpile-attest-tally-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("contracts")).expect("create scratch corpus");
    std::fs::write(
        dir.join("contracts/foo.yaml"),
        "metadata:\n  id: C-FOO\n  version: \"1.0\"\n",
    )
    .expect("write contract");
    dir
}

fn run_attestations(roadmap: &Path, contracts: &Path, json: bool) -> String {
    let mut args = vec![
        "attestations".to_string(),
        "--roadmap".to_string(),
        roadmap.display().to_string(),
        "--contracts-dir".to_string(),
        contracts.display().to_string(),
    ];
    if json {
        args.push("--json".to_string());
    }
    let out = Command::new(bin())
        .args(&args)
        .output()
        .expect("run xpile attestations");
    assert!(
        out.status.success(),
        "xpile attestations {args:?} failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Property (1), text half.
#[test]
fn a_preamble_mention_adds_no_work_item_and_prints_no_nameless_bullet() {
    let dir = scratch("preamble");
    let roadmap = dir.join("roadmap.yaml");
    std::fs::write(
        &roadmap,
        "strategic_goals:\n  note: mentions C-FOO in the preamble\n\
         roadmap:\n- id: PMAT-1\n  title: unrelated\n",
    )
    .expect("write roadmap");

    let text = run_attestations(&roadmap, &dir.join("contracts"), false);
    assert!(
        text.contains("0 work item(s)"),
        "ZERO work items mention C-FOO; the preamble is not one. Got:\n{text}"
    );
    // The nameless bullet the phantom printed. `      - ` with nothing after
    // it is the exact line shape; match it as a whole line.
    assert!(
        !text.lines().any(|l| l.trim_end() == "      -"),
        "a nameless work-item bullet was printed:\n{text}"
    );
    // Disclosed, not dropped.
    assert!(
        text.contains("outside every work item"),
        "the preamble mention must be disclosed rather than silently \
         discarded — `xpile quorum` scores the Extrinsic stratum from the \
         mention COUNT, so dropping it would move a different number. \
         Got:\n{text}"
    );
    assert!(
        text.contains("1 total mention(s)"),
        "the mention itself is real and must still be counted:\n{text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Property (1), JSON half: the phantom reported as `null`, never `""`.
#[test]
fn a_preamble_mention_is_reported_as_null_not_empty_string() {
    let dir = scratch("preamble-json");
    let roadmap = dir.join("roadmap.yaml");
    std::fs::write(
        &roadmap,
        "strategic_goals:\n  note: mentions C-FOO in the preamble\n\
         roadmap:\n- id: PMAT-1\n  title: unrelated\n",
    )
    .expect("write roadmap");

    let raw = run_attestations(&roadmap, &dir.join("contracts"), true);
    let v: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("--json payload is not JSON: {e}\n{raw}"));
    let c = &v["contracts"][0];
    assert_eq!(c["id"], "C-FOO");
    assert_eq!(c["mention_count"], 1, "the mention is retained");
    assert_eq!(
        c["work_items"],
        serde_json::json!([]),
        "no work item attests C-FOO; pre-PMAT-1390 this was [\"\"]"
    );
    assert_eq!(c["preamble_mentions"], 1);
    assert!(
        c["mentions"][0]["work_item"].is_null(),
        "a mention with no enclosing work item reports null, not \"\""
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Property (2). A YAML-quoted id is the minimal input that broke the payload
/// — no unusual path, no unusual snippet.
#[test]
fn a_quoted_work_item_id_still_yields_parseable_json() {
    let dir = scratch("quoted-id");
    let contracts = dir.join("contracts");
    for (tag, body) in [
        ("dq", "roadmap:\n- id: \"P\"\n  c: C-FOO\n"),
        ("sq", "roadmap:\n- id: 'P'\n  c: C-FOO\n"),
    ] {
        let roadmap = dir.join(format!("roadmap-{tag}.yaml"));
        std::fs::write(&roadmap, body).expect("write roadmap");
        let raw = run_attestations(&roadmap, &contracts, true);
        let v: serde_json::Value = serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("--json payload is not JSON ({tag}): {e}\n{raw}"));
        assert_eq!(
            v["contracts"][0]["work_items"],
            serde_json::json!(["P"]),
            "the quotes are YAML syntax, not part of the id ({tag})"
        );
        assert_eq!(v["contracts"][0]["mentions"][0]["work_item"], "P");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Property (2), the escape sweep: a `"` and a `\` reaching each remaining
/// hand-interpolated string must not terminate it.
#[test]
fn quotes_and_backslashes_in_the_corpus_do_not_break_the_payload() {
    let dir = scratch("escapes");
    let roadmap = dir.join("roadmap.yaml");
    std::fs::write(
        &roadmap,
        "roadmap:\n- id: PMAT-1\n  title: 'a \\ backslash and a \" quote near C-FOO'\n",
    )
    .expect("write roadmap");
    // A second contract whose ID ITSELF carries a quote, and which is never
    // mentioned so it lands in `unattested`. That reaches the two remaining
    // raw-interpolated strings (`contracts[].id` and `unattested[]`); a
    // fixture whose specials appear only in the snippet would pass even
    // pre-fix, since `snippet` was the one field already escaped.
    std::fs::write(
        dir.join("contracts/quo.yaml"),
        "metadata:\n  id: 'C-QUO\"TE'\n",
    )
    .expect("write contract");

    let raw = run_attestations(&roadmap, &dir.join("contracts"), true);
    let v: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("--json payload is not JSON: {e}\n{raw}"));
    assert_eq!(
        v["unattested"],
        serde_json::json!(["C-QUO\"TE"]),
        "the quote is part of the id and must survive as data"
    );
    let snippet = v["contracts"][0]["mentions"][0]["snippet"]
        .as_str()
        .expect("snippet is a string");
    assert!(
        snippet.contains('"') && snippet.contains('\\'),
        "both characters must survive as DATA, not be stripped: {snippet}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Property (3). The reference walks the parsed YAML tree rather than
/// re-serializing it: re-serialization can fold a long scalar across lines and
/// split an id, which would make the reference agree with the line-scan for
/// the wrong reason.
fn value_mentions(v: &serde_yaml::Value, needle: &str) -> bool {
    match v {
        serde_yaml::Value::String(s) => s.contains(needle),
        serde_yaml::Value::Sequence(xs) => xs.iter().any(|x| value_mentions(x, needle)),
        serde_yaml::Value::Mapping(m) => m
            .iter()
            .any(|(k, val)| value_mentions(k, needle) || value_mentions(val, needle)),
        _ => false,
    }
}

#[test]
fn the_live_tally_equals_an_independent_yaml_parse() {
    let root = workspace_root();
    let roadmap_path = root.join("docs/roadmaps/roadmap.yaml");
    let contracts = root.join("contracts");

    let raw = run_attestations(&roadmap_path, &contracts, true);
    let report: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("live --json payload is not JSON: {e}\n{raw}"));

    let doc: serde_yaml::Value =
        serde_yaml::from_str(&std::fs::read_to_string(&roadmap_path).expect("read roadmap"))
            .expect(
                "roadmap.yaml must parse — crates/xpile/tests/roadmap_ledger_parses.rs owns this",
            );
    let items = doc
        .get("roadmap")
        .and_then(|r| r.as_sequence())
        .expect("roadmap.yaml has a `roadmap:` sequence");

    let contracts_json = report["contracts"]
        .as_array()
        .expect("contracts is an array");
    assert!(
        contracts_json.len() >= 10,
        "vacuity guard: the live corpus attested only {} contracts; this \
         test proves nothing over an empty report",
        contracts_json.len()
    );

    let mut max_items = 0usize;
    for c in contracts_json {
        let id = c["id"].as_str().expect("contract id");
        let reported = c["work_items"]
            .as_array()
            .expect("work_items is an array")
            .len();
        let reference = items.iter().filter(|it| value_mentions(it, id)).count();
        assert_eq!(
            reported, reference,
            "{id}: `xpile attestations` reports {reported} attesting work \
             item(s); a real YAML parse of the same file finds {reference}. \
             Pre-PMAT-1390 this over-counted by exactly one for every \
             contract also named in the strategic_goals preamble."
        );
        max_items = max_items.max(reported);
    }
    assert!(
        max_items >= 10,
        "vacuity guard: the busiest contract has only {max_items} attesting \
         work item(s), so the equality above is near-trivially satisfiable"
    );

    // The nameless bullet, on the corpus that actually produced three of them.
    let text = run_attestations(&roadmap_path, &contracts, false);
    assert!(
        !text.lines().any(|l| l.trim_end() == "      -"),
        "the live text report still prints a nameless work-item bullet"
    );
}
