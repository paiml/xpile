//! PMAT-2163 (epic #2125 row 5): the quorum honesty pilot.
//!
//! `xpile quorum` counted every `lean_theorem:` ref as a Semantic vote. Most
//! of those theorems are about a Lean re-implementation, so no edit to xpile
//! could turn one red (PMAT-1512). A contract whose `metadata:` says
//! `quorum_semantic: shipped-bound` now counts only obligations that carry a
//! `shipped_binding:`. This file holds that field to something derived:
//!
//! 1. **The bound set is derived from the Lean source, both ways.** Each
//!    pilot contract's Lean file has one pin block (`-- BEGIN … pins (PMAT-N)`)
//!    that a seam test checks against shipped output. The defs the pins reach,
//!    closed over the in-file defs they call, are the pinned model. A theorem
//!    is bound iff its statement names one of them. The YAML's
//!    `shipped_binding:` set must equal that set, and each binding must name a
//!    test file that reads that pin block.
//! 2. **The binary counts it.** A pilot row's Semantic equals its
//!    `shipped_binding:` count; every other row still equals its
//!    `lean_theorem:` count. The text report names the pilot and says the
//!    other contracts are unchanged.
//! 3. **The old count is shown going away.** The same contracts with the
//!    pilot marker stripped report the old count again.
//! 4. **Docs cannot quote a count the binary no longer prints.** Every live
//!    `.md` quote of a quorum row, a totals line, or a row of the pilot table
//!    in `book/src/reference/cli.md` must match the binary.
//!
//! "Bound" means the theorem is about a function whose values are checked
//! against the shipped binary on the pinned inputs. It does not mean the
//! theorem itself was checked against the binary.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

const MARKER: &str = "quorum_semantic: shipped-bound";
const PILOT_TABLE_DOC: &str = "book/src/reference/cli.md";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

// ─── Lean side ──────────────────────────────────────────────────────

fn idents(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '\''))
        .filter(|w| {
            w.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        })
        .map(str::to_string)
        .collect()
}

const DECL_STARTS: &[&str] = &[
    "def ",
    "private def ",
    "abbrev ",
    "theorem ",
    "lemma ",
    "example ",
    "structure ",
    "inductive ",
    "namespace ",
    "section ",
    "end ",
    "instance ",
    "/--",
    "--",
    "@[",
];

fn starts_decl(line: &str) -> bool {
    let t = line.trim_start();
    DECL_STARTS.iter().any(|d| t.starts_with(d)) || t == "end"
}

/// `def` name -> the text of its signature and body.
fn lean_defs(src: &str) -> BTreeMap<String, String> {
    let lines: Vec<&str> = src.lines().collect();
    let mut defs = BTreeMap::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        let Some(rest) = ["def ", "private def ", "abbrev "]
            .iter()
            .find_map(|k| t.strip_prefix(k))
        else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '\'')
            .collect();
        let mut body = rest[name.len()..].to_string();
        for next in &lines[i + 1..] {
            if starts_decl(next) {
                break;
            }
            body.push('\n');
            body.push_str(next);
        }
        defs.insert(name, body);
    }
    defs
}

/// The statement of `theorem name`: everything between its name and `:=`.
fn theorem_statement<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("theorem {name}");
    let mut from = 0;
    while let Some(at) = src[from..].find(&key) {
        let start = from + at + key.len();
        if src[start..].starts_with(|c: char| c.is_whitespace()) {
            let end = src[start..].find(":=")?;
            return Some(&src[start..start + end]);
        }
        from = start;
    }
    None
}

/// The one `-- BEGIN … pins (PMAT-N)` … `-- END …` block: (begin line, text).
fn pin_block(src: &str) -> Result<(String, String), String> {
    let begins: Vec<&str> = src
        .lines()
        .filter(|l| l.starts_with("-- BEGIN ") && l.contains("pins (PMAT-"))
        .collect();
    let [begin] = begins.as_slice() else {
        return Err(format!("expected one pin block, found {}", begins.len()));
    };
    let end = begin.replacen("-- BEGIN ", "-- END ", 1);
    let a = src.find(begin).unwrap() + begin.len();
    let b = src[a..]
        .find(&end)
        .ok_or_else(|| format!("no `{end}` after `{begin}`"))?;
    Ok((begin.to_string(), src[a..a + b].to_string()))
}

/// In-file defs the pin block reaches, closed over the defs they call.
fn pinned_model(src: &str) -> Result<BTreeSet<String>, String> {
    let (_, pins) = pin_block(src)?;
    let defs = lean_defs(src);
    let mut closure: BTreeSet<String> = idents(&pins)
        .into_iter()
        .filter(|i| defs.contains_key(i))
        .collect();
    let mut todo: Vec<String> = closure.iter().cloned().collect();
    while let Some(d) = todo.pop() {
        for i in idents(&defs[&d]) {
            if defs.contains_key(&i) && closure.insert(i.clone()) {
                todo.push(i);
            }
        }
    }
    if closure.is_empty() {
        return Err("the pin block names no def in this file".into());
    }
    Ok(closure)
}

// ─── YAML side ──────────────────────────────────────────────────────

fn field(line: &str, key: &str) -> Option<String> {
    let v = line.trim_start().strip_prefix(key)?;
    Some(v.trim().trim_matches('"').to_string())
}

fn is_code(line: &str) -> bool {
    let t = line.trim_start();
    !t.is_empty() && !t.starts_with('#')
}

/// Theorem short name -> its `shipped_binding:` (None if unbound). A
/// `shipped_binding:` must directly follow its obligation's `lean_theorem:`.
fn obligations(yaml: &str) -> Result<BTreeMap<String, Option<String>>, String> {
    let mut out = BTreeMap::new();
    let mut last: Option<String> = None;
    for line in yaml.lines().filter(|l| is_code(l)) {
        if let Some(t) = field(line, "lean_theorem:") {
            let short = t.rsplit('.').next().unwrap_or(&t).to_string();
            out.insert(short.clone(), None);
            last = Some(short);
        } else if let Some(b) = field(line, "shipped_binding:") {
            let t = last
                .take()
                .ok_or_else(|| format!("`shipped_binding: {b}` does not follow a lean_theorem"))?;
            out.insert(t, Some(b));
        } else {
            last = None;
        }
    }
    Ok(out)
}

fn lean_file(yaml: &str) -> Result<String, String> {
    let files: BTreeSet<String> = yaml
        .lines()
        .filter(|l| is_code(l))
        .filter_map(|l| field(l, "lean_file:"))
        .collect();
    let mut it = files.into_iter();
    match (it.next(), it.next()) {
        (Some(f), None) => Ok(f),
        _ => Err("a pilot contract must cite exactly one lean_file".into()),
    }
}

/// Check one pilot contract: the YAML's bound set equals the derived one, and
/// each binding names a test that reads the pin block. Returns the bound count.
fn check_pilot(
    yaml: &str,
    lean_src: &str,
    test_src: &dyn Fn(&str) -> Option<String>,
) -> Result<usize, String> {
    let model = pinned_model(lean_src)?;
    let (begin, _) = pin_block(lean_src)?;
    let obls = obligations(yaml)?;
    let mut derived = BTreeSet::new();
    for t in obls.keys() {
        let stmt = theorem_statement(lean_src, t)
            .ok_or_else(|| format!("theorem {t} is cited but not declared in the Lean file"))?;
        if idents(stmt).iter().any(|i| model.contains(i)) {
            derived.insert(t.clone());
        }
    }
    let declared: BTreeSet<String> = obls
        .iter()
        .filter(|(_, b)| b.is_some())
        .map(|(t, _)| t.clone())
        .collect();
    if derived != declared {
        let over: Vec<_> = declared.difference(&derived).collect();
        let under: Vec<_> = derived.difference(&declared).collect();
        return Err(format!(
            "shipped_binding set disagrees with the Lean pins (model {model:?}): \
             bound but not about a pinned def {over:?}; about a pinned def but unbound {under:?}"
        ));
    }
    if declared.is_empty() {
        return Err("a pilot contract binds no theorem, so its Semantic would be 0".into());
    }
    for b in obls.values().flatten() {
        let src = test_src(b).ok_or_else(|| format!("shipped_binding {b} does not exist"))?;
        if !src.contains(&begin) {
            return Err(format!(
                "shipped_binding {b} does not read the pin block `{begin}`"
            ));
        }
    }
    Ok(declared.len())
}

fn contract_yamls() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::fs::read_dir(root().join("contracts"))
        .expect("contracts/")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "yaml"))
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap();
            (
                format!("contracts/{}", p.file_name().unwrap().to_string_lossy()),
                text,
            )
        })
        .collect();
    v.sort();
    v
}

fn metadata_id(yaml: &str) -> String {
    yaml.lines()
        .skip_while(|l| !l.starts_with("metadata:"))
        .find_map(|l| field(l, "id:"))
        .expect("metadata.id")
}

fn is_pilot(yaml: &str) -> bool {
    yaml.lines()
        .skip_while(|l| !l.starts_with("metadata:"))
        .skip(1)
        .take_while(|l| l.is_empty() || l.starts_with(' ') || l.starts_with('#'))
        .any(|l| l.trim() == MARKER)
}

fn count(yaml: &str, key: &str) -> u64 {
    yaml.lines()
        .filter(|l| is_code(l) && l.trim_start().starts_with(key))
        .count() as u64
}

fn live_test_src(rel: &str) -> Option<String> {
    std::fs::read_to_string(root().join(rel)).ok()
}

// ─── binary side ────────────────────────────────────────────────────

fn quorum(contracts: &Path, json: bool) -> String {
    let r = root();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xpile"));
    cmd.current_dir(&r).arg("quorum");
    if json {
        cmd.arg("--json");
    }
    let out = cmd
        .args(["--contracts-dir", contracts.to_str().unwrap()])
        .output()
        .expect("run xpile quorum");
    assert!(
        out.status.success(),
        "xpile quorum failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn rows(json: &str) -> BTreeMap<String, serde_json::Value> {
    let v: serde_json::Value = serde_json::from_str(json).expect("quorum --json");
    v["contracts"]
        .as_array()
        .expect("contracts array")
        .iter()
        .map(|r| (r["id"].as_str().unwrap().to_string(), r.clone()))
        .collect()
}

// ─── 1. the bound set is derived ────────────────────────────────────

#[test]
fn every_shipped_binding_is_derived_from_the_lean_pins() {
    let pilots: Vec<_> = contract_yamls()
        .into_iter()
        .filter(|(_, y)| is_pilot(y))
        .collect();
    assert!(
        pilots.len() >= 2,
        "the pilot shrank to {} contracts",
        pilots.len()
    );
    for (path, yaml) in &pilots {
        let lean = read(&lean_file(yaml).unwrap());
        let n = check_pilot(yaml, &lean, &live_test_src).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(n >= 1, "{path}");
    }
    // Outside the pilot, `shipped_binding:` has no effect, so it must not appear.
    for (path, yaml) in contract_yamls().iter().filter(|(_, y)| !is_pilot(y)) {
        assert_eq!(
            count(yaml, "shipped_binding:"),
            0,
            "{path} binds without the marker"
        );
    }
}

#[test]
fn red_a_binding_on_a_reimplementation_theorem_is_refused() {
    let yaml = read("contracts/py-int-arith-v1.yaml");
    let lean = read("contracts/lean/PyIntArith.lean");
    let key = "lean_theorem: \"XpileContracts.CPyIntArith.division_algorithm_diamond\"\n";
    let at = yaml.find(key).expect("division_algorithm_diamond is cited") + key.len();
    let mut forged = yaml.clone();
    forged.insert_str(
        at,
        "    shipped_binding: \"crates/xpile/tests/lean_shipped_pilot_witness.rs\"\n",
    );
    let err = check_pilot(&forged, &lean, &live_test_src).unwrap_err();
    assert!(err.contains("division_algorithm_diamond"), "{err}");
}

#[test]
fn red_an_unbound_pinned_theorem_and_a_binding_that_skips_the_pins_are_refused() {
    let yaml = read("contracts/py-int-arith-v1.yaml");
    let lean = read("contracts/lean/PyIntArith.lean");
    let line = "shipped_binding: \"crates/xpile/tests/lean_shipped_pilot_witness.rs\"\n";
    let at = yaml.find(line).expect("a binding");
    let start = yaml[..at].rfind('\n').unwrap() + 1;
    let dropped = format!("{}{}", &yaml[..start], &yaml[at + line.len()..]);
    let err = check_pilot(&dropped, &lean, &live_test_src).unwrap_err();
    assert!(err.contains("about a pinned def but unbound"), "{err}");

    let other = |_: &str| Some("fn main() {}".to_string());
    let err = check_pilot(&yaml, &lean, &other).unwrap_err();
    assert!(err.contains("does not read the pin block"), "{err}");
}

// ─── 2. the binary counts it, and says so ───────────────────────────

#[test]
fn the_binary_counts_only_bound_theorems_on_the_pilot_and_nothing_else_changes() {
    let yamls: BTreeMap<String, String> = contract_yamls()
        .into_iter()
        .map(|(_, y)| (metadata_id(&y), y))
        .collect();
    let live = rows(&quorum(&root().join("contracts"), true));
    assert_eq!(live.len(), yamls.len(), "one row per contract");
    let mut pilots = 0;
    for (id, yaml) in &yamls {
        let r = &live[id];
        let (key, rule) = if is_pilot(yaml) {
            pilots += 1;
            ("shipped_binding:", "shipped-bound")
        } else {
            ("lean_theorem:", "every-lean-theorem")
        };
        assert_eq!(r["semantic"], count(yaml, key), "{id} Semantic");
        assert_eq!(r["semantic_rule"], rule, "{id} semantic_rule");
    }
    assert!(pilots >= 2, "pilot rows: {pilots}");

    let text = quorum(&root().join("contracts"), false);
    let disclosure = text
        .lines()
        .find(|l| l.starts_with("Semantic, pilot (PMAT-2163): "))
        .expect("the text report discloses the pilot");
    for (id, yaml) in &yamls {
        assert_eq!(
            disclosure.contains(id.as_str()),
            is_pilot(yaml),
            "{id}: {disclosure}"
        );
    }
    let others = format!("the other {} contracts count every", yamls.len() - pilots);
    assert!(disclosure.contains(&others), "{disclosure}");
}

// ─── 3. the old count, shown going away ─────────────────────────────

#[test]
fn red_stripping_the_marker_restores_the_old_reimplementation_count() {
    let scratch =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("quorum-pilot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    for (path, yaml) in contract_yamls() {
        let name = Path::new(&path).file_name().unwrap();
        let stripped: String = yaml
            .lines()
            .filter(|l| l.trim() != MARKER)
            .map(|l| format!("{l}\n"))
            .collect();
        std::fs::write(scratch.join(name), stripped).unwrap();
    }
    let old = rows(&quorum(&scratch, true));
    let new = rows(&quorum(&root().join("contracts"), true));
    let _ = std::fs::remove_dir_all(&scratch);

    let yaml = read("contracts/py-int-arith-v1.yaml");
    let before = count(&yaml, "lean_theorem:");
    let after = count(&yaml, "shipped_binding:");
    assert!(before > after, "{before} -> {after}");
    assert_eq!(old["C-PY-INT-ARITH"]["semantic"], before);
    assert_eq!(new["C-PY-INT-ARITH"]["semantic"], after);
    assert_eq!(old["C-PY-INT-ARITH"]["semantic_rule"], "every-lean-theorem");
    let changed: Vec<&String> = new
        .keys()
        .filter(|id| old[*id]["semantic"] != new[*id]["semantic"])
        .collect();
    let pilot_ids: Vec<String> = contract_yamls()
        .iter()
        .filter(|(_, y)| is_pilot(y))
        .map(|(_, y)| metadata_id(y))
        .collect();
    assert_eq!(
        changed.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        {
            let mut p = pilot_ids;
            p.sort();
            p
        },
        "only pilot rows change"
    );
}

// ─── 4. docs cannot quote a count the binary no longer prints ───────

#[derive(Debug, PartialEq)]
enum Quote {
    Row(String, Vec<String>),
    Totals(String),
    Pilot(String, u64, u64),
}

fn quotes(doc: &str) -> Vec<Quote> {
    let mut out = Vec::new();
    for line in doc.lines() {
        let t = line.trim();
        let w: Vec<&str> = t.split_whitespace().collect();
        if w.len() == 6
            && w[0].starts_with("C-")
            && w[1..5].iter().all(|n| n.parse::<u64>().is_ok())
            && ["QUORUM", "PARTIAL", "UNVERIFIED"].contains(&w[5])
        {
            out.push(Quote::Row(
                w[0].into(),
                w[1..].iter().map(|s| s.to_string()).collect(),
            ));
        } else if let Some(i) = t.find("totals: ") {
            // `totals: <N> QUORUM, …` is a placeholder, not a quote.
            let n = &t[i + "totals: ".len()..];
            if n.starts_with(|c: char| c.is_ascii_digit()) && n.contains(" QUORUM, ") {
                out.push(Quote::Totals(t[i..].to_string()));
            }
        } else if t.starts_with("| `C-") {
            let cells: Vec<&str> = t.trim_matches('|').split('|').map(str::trim).collect();
            if let [id, a, b] = cells.as_slice() {
                if let (Ok(a), Ok(b)) = (a.parse(), b.parse()) {
                    out.push(Quote::Pilot(id.trim_matches('`').into(), a, b));
                }
            }
        }
    }
    out
}

fn check_quote(
    q: &Quote,
    text: &str,
    live: &BTreeMap<String, serde_json::Value>,
    yamls: &BTreeMap<String, String>,
) -> Result<(), String> {
    match q {
        Quote::Row(id, cells) => {
            let printed = text
                .lines()
                .find(|l| l.split_whitespace().next() == Some(id.as_str()))
                .ok_or_else(|| format!("{id}: the binary prints no such row"))?;
            let p: Vec<&str> = printed.split_whitespace().skip(1).collect();
            (p == cells.iter().map(String::as_str).collect::<Vec<_>>())
                .then_some(())
                .ok_or_else(|| format!("{id}: quoted {cells:?}, the binary prints {p:?}"))
        }
        Quote::Totals(t) => text
            .lines()
            .any(|l| l.trim() == t)
            .then_some(())
            .ok_or_else(|| format!("quoted `{t}`, the binary prints another totals line")),
        Quote::Pilot(id, pilot, every) => {
            let r = live
                .get(id)
                .ok_or_else(|| format!("{id}: no such contract"))?;
            let y = &yamls[id];
            let want = (r["semantic"].as_u64().unwrap(), count(y, "lean_theorem:"));
            (r["semantic_rule"] == "shipped-bound" && want == (*pilot, *every))
                .then_some(())
                .ok_or_else(|| {
                    format!(
                        "{id}: quoted ({pilot}, {every}), live {want:?} rule {}",
                        r["semantic_rule"]
                    )
                })
        }
    }
}

fn is_dated(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() > 11
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit)
        && b[10] == b'-'
}

/// Live docs: every tracked `.md` except the dated history.
fn live_docs() -> Vec<String> {
    let out = Command::new("git")
        .current_dir(root())
        .args(["ls-files", "*.md"])
        .output()
        .expect("git ls-files");
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .filter(|p| !p.ends_with("CHANGELOG.md") && *p != "book/src/changelog.md")
        // A dated snapshot (`docs/status/2026-05-18-…md`) records what the
        // binary printed that day; it is history, not a live claim.
        .filter(|p| !is_dated(p.rsplit('/').next().unwrap_or(p)))
        .map(str::to_string)
        .collect()
}

#[test]
fn every_quorum_count_a_live_doc_quotes_is_what_the_binary_prints() {
    let contracts = root().join("contracts");
    let text = quorum(&contracts, false);
    let live = rows(&quorum(&contracts, true));
    let yamls: BTreeMap<String, String> = contract_yamls()
        .into_iter()
        .map(|(_, y)| (metadata_id(&y), y))
        .collect();
    let docs = live_docs();
    assert!(docs.len() > 50, "git ls-files found {} docs", docs.len());
    let mut bad = Vec::new();
    let mut pilot_quotes = BTreeSet::new();
    for doc in &docs {
        for q in quotes(&read(doc)) {
            if let Quote::Pilot(id, ..) = &q {
                if doc == PILOT_TABLE_DOC {
                    pilot_quotes.insert(id.clone());
                }
            }
            if let Err(e) = check_quote(&q, &text, &live, &yamls) {
                bad.push(format!("{doc}: {e}"));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "stale quorum quotes:\n  {}",
        bad.join("\n  ")
    );
    let pilot_ids: BTreeSet<String> = yamls
        .iter()
        .filter(|(_, y)| is_pilot(y))
        .map(|(id, _)| id.clone())
        .collect();
    assert!(!pilot_ids.is_empty());
    assert_eq!(
        pilot_quotes, pilot_ids,
        "{PILOT_TABLE_DOC}'s pilot table must list exactly the pilot"
    );
}

#[test]
fn red_a_doc_quote_the_binary_no_longer_prints_is_refused() {
    let contracts = root().join("contracts");
    let text = quorum(&contracts, false);
    let live = rows(&quorum(&contracts, true));
    let yamls: BTreeMap<String, String> = contract_yamls()
        .into_iter()
        .map(|(_, y)| (metadata_id(&y), y))
        .collect();
    let every = count(&yamls["C-PY-INT-ARITH"], "lean_theorem:");
    let pilot = live["C-PY-INT-ARITH"]["semantic"].as_u64().unwrap();
    let row = text
        .lines()
        .find(|l| l.trim_start().starts_with("C-PY-INT-ARITH "))
        .unwrap()
        .to_string();
    let totals = text
        .lines()
        .find(|l| l.starts_with("totals: "))
        .unwrap()
        .to_string();

    let good = format!("{row}\n{totals}\n| `C-PY-INT-ARITH` | {pilot} | {every} |\n");
    let q = quotes(&good);
    assert_eq!(q.len(), 3, "{q:?}");
    for x in &q {
        check_quote(x, &text, &live, &yamls).unwrap();
    }
    // The pre-pilot Semantic in the row, the pilot table, and a totals line.
    let stale_row = row.replacen(&format!(" {pilot} "), &format!(" {every} "), 1);
    let stale = format!(
        "{stale_row}\n| `C-PY-INT-ARITH` | {every} | {every} |\n{}\n",
        totals.replacen("totals: ", "totals: 9", 1)
    );
    let q = quotes(&stale);
    assert_eq!(q.len(), 3, "{q:?}");
    for x in &q {
        assert!(
            check_quote(x, &text, &live, &yamls).is_err(),
            "{x:?} passed"
        );
    }
}
