//! XPILE-LAYERORDINAL-001 (PMAT-1462) — a layer-scoped ordinal must equal the
//! rank the substrate's own records give it.
//!
//! The Diamond broadening records say things like "the **second L2 contract at
//! depth-10**" and "the **third Layer 2 contract at depth-11**". Each of those
//! is a rank inside a set: *contracts declaring layer N that had reached
//! depth-D by the slice writing the sentence*. Both halves of that set are
//! machine-readable — the layer from `metadata.xpile.layer` (all 35 contracts
//! carry it since PMAT-1461) and the arrival order from the `# PMAT-NNN:`
//! header on each `*_diamond:` entry — so the rank is arithmetic, not
//! judgement.
//!
//! ## What was wrong
//!
//! At `b31fe0ac`, **29** of the 103 layer-scoped ordinals under `contracts/`
//! disagreed with that arithmetic — 20 in `*.yaml`, 9 in `lean/*.lean` — and
//! every one had a single root cause: `C-BASHRS-POSIX-IDEMPOTENCE` counted as
//! a **Layer 2** contract. It declares `layer: semantics`, i.e. Layer 1.
//!
//! The damage ran in both directions:
//!
//!   * **Bashrs claimed a rank in a layer it is not in.** "Diamond depth-10
//!     broadened: second L2 contract at depth-10" — as a Layer-2 claim it has
//!     no rank at all, because Bashrs is not a Layer-2 contract, and at
//!     PMAT-401 the Layer-2 population at depth-10 was *empty*. The ordinal
//!     `second` was nevertheless **correct** — for Layer **1**, after
//!     `C-PY-INT-ARITH`. That is why it read as checked: a reader verifying
//!     "is Bashrs second?" gets YES, and the half that aged is the half nobody
//!     checks.
//!   * **`C-XLATE-PY-LIST-TO-VEC` was demoted by the phantom.** It was
//!     published as the SECOND (then THIRD) Layer-2 contract at depths 4
//!     through 13. It was the **FIRST**, at every one of them — a real
//!     across-layers milestone, given away to a contract from another layer.
//!     The receipt is in the sentence itself: "Third L2 contract at depth-10
//!     (after PMAT-401 Bashrs and PMAT-400 FfiCpythonExt openers)" ranks a
//!     Layer-1 and a Layer-4 contract inside the Layer-2 set.
//!
//! ## Never-true, and why the previous slice could not see it
//!
//! Not aged: the layer tag predates every one of these sentences, so each was
//! false when typed. PMAT-1460 corrected contracts written under the wrong
//! layer *label*; PMAT-1461 completed the tagging from 9 of 35 to 35 of 35 and
//! repaired 18 sites that spell "Layer 2" beside the name. **A tally computed
//! over a wrong label is not a label** — none of these 29 sites spells a layer
//! beside a contract name, so a label needle cannot reach them. PMAT-1461's own
//! header disclosed them as out of reach "phrased over untagged contracts";
//! that excuse was refuted by its own diff, which left nothing untagged.
//! Completing the tagging is exactly what made this decidable.
//!
//! ## The dating question, answered by construction
//!
//! A broadening record is dated, so a frozen *substrate-wide* count ("8
//! contracts at depth-5+", live 13) raises a real question about whether a
//! historical record may keep a stale number. An **ordinal does not**: the
//! record's own date is derivable from the `# PMAT-NNN` on the block it sits
//! in, so this gate checks each ordinal against the state *at that slice*. No
//! policy call is needed and no live count is frozen. The substrate-wide
//! counts remain a separate, still-open class and are deliberately not touched
//! here.
//!
//! ## Blind spots, each pinned by a control that PASSES
//!
//!   * SUBJECT — [`the_gate_reports_what_it_reaches`] prints the claim census
//!     per artifact kind and fails if either corpus goes empty.
//!   * GROUND TRUTH — [`the_reconstruction_matches_the_live_diamond_report`].
//!     The whole gate rests on parsing every `*_diamond:` entry; a parser that
//!     skipped entries would silently shift every rank and the gate would stay
//!     green while lying. The per-contract count is cross-checked against
//!     `xpile diamond --json`, which reaches the same entries through the
//!     shipped binary's own parser, and every named theorem must exist in its
//!     `lean_file`. (The first draft cross-checked against the `lean_theorem:`
//!     line count in the same file; that is NOT independent of the entry set
//!     and it was also wrong — the tier ladder carries four `lean_theorem:`
//!     lines on non-Diamond entries.)
//!   * DATING — [`unattributed_diamonds_cannot_reach_a_gated_ordinal`]. Eleven
//!     of the 207 entries carry no `# PMAT-NNN`; they are treated as present
//!     from the start, and the test proves that choice cannot move any gated
//!     rank (all 11 sit on contracts that never pass depth-2; no claim in the
//!     corpus is below depth-3).
//!   * NEEDLE — [`the_needle_reports_a_wrong_ordinal_and_spares_a_right_one`]
//!     and [`the_needle_reads_across_a_wrapped_comment`]. Prose flattening is
//!     worth exactly **3 of the 29** sites, measured on `b31fe0ac`: 1 of the 20
//!     yaml sites and 2 of the 29 reachable Lean claims. Small, kept because a
//!     wrapped claim is genuinely invisible line-locally and the wrap point is
//!     arbitrary — but the guard states the measured figure rather than a
//!     flattering one. (It first asserted flattening "more than doubles" the
//!     reach; the live corpus said 62 -> 71 and the assertion was deleted.)
//!   * VOCABULARY — [`canonical_layer_numbering_is_read_from_the_taxonomy_doc`]
//!     derives `word -> layer number` from the spec table rather than
//!     hard-coding it.
//!
//! Run against `b31fe0ac`'s `contracts/` the gate reports **29** sites — 20 in
//! `*.yaml`, 9 in `lean/*.lean`. On the repaired tree it reports 0 over 100
//! ranked claims. Each guard is worth the sites it is worth.

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

/// The canonical taxonomy page: `| **Layer N: Some Name** | … |`.
const TAXONOMY_DOC: &str = "docs/specifications/sub/contract-taxonomy.md";

/// No claim in the corpus is phrased below this depth; asserted, not assumed.
const SHALLOWEST_CLAIM_DEPTH: u32 = 3;

// ---------------------------------------------------------------------------
// Layer vocabulary, derived from the taxonomy table.
// ---------------------------------------------------------------------------

/// `word -> layer number` for every word in a layer's taxonomy name.
fn canonical_layer_words() -> BTreeMap<String, u32> {
    let doc = workspace_root().join(TAXONOMY_DOC);
    let body = fs::read_to_string(&doc).unwrap_or_else(|e| panic!("read {TAXONOMY_DOC}: {e}"));
    let mut out: BTreeMap<String, u32> = BTreeMap::new();
    for line in body.lines() {
        let Some(rest) = line.trim().strip_prefix("| **Layer ") else {
            continue;
        };
        let Some((num, name)) = rest.split_once(':') else {
            continue;
        };
        let Ok(n) = num.trim().parse::<u32>() else {
            continue;
        };
        let name = name.split("**").next().unwrap_or_default();
        for frag in name.split(['-', '/', ' ']) {
            let word: String = frag
                .chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .flat_map(char::to_lowercase)
                .collect();
            if word.len() >= 4 {
                out.insert(word, n);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The substrate: every contract, its declared layer, and its Diamond arrivals.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct DiamondEntry {
    /// The `PMAT-NNN` on the entry's header comment; `0` when absent.
    pmat: u32,
    lean_theorem: Option<String>,
    lean_file: Option<String>,
}

#[derive(Debug, Clone)]
struct Contract {
    id: String,
    layer: u32,
    yaml: PathBuf,
    /// Arrival ids, ascending. `entries[D - 1]` is the id that reached depth-D.
    entries: Vec<DiamondEntry>,
}

impl Contract {
    /// The `PMAT-NNN` at which this contract reached `depth`, if it ever did.
    fn reached(&self, depth: u32) -> Option<u32> {
        let mut ids: Vec<u32> = self.entries.iter().map(|e| e.pmat).collect();
        ids.sort_unstable();
        ids.get(depth as usize - 1).copied()
    }
}

fn parse_contracts() -> Vec<Contract> {
    let root = workspace_root();
    let words = canonical_layer_words();
    let mut out = Vec::new();
    let dir = root.join("contracts");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("read contracts/")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "yaml"))
        .collect();
    paths.sort();
    for p in paths {
        let body = fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
        let lines: Vec<&str> = body.lines().collect();
        let mut id = None;
        let mut layer = None;
        for l in &lines {
            let t = l.trim();
            if id.is_none() {
                if let Some(v) = t.strip_prefix("id: ") {
                    if v.starts_with("C-") {
                        id = Some(v.trim().to_string());
                    }
                }
            }
            if layer.is_none() {
                if let Some(v) = t.strip_prefix("layer: ") {
                    layer = words.get(v.trim()).copied();
                }
            }
        }
        let (Some(id), Some(layer)) = (id, layer) else {
            panic!(
                "{} declares no contract id or no recognised metadata.xpile.layer \
                 (PMAT-1461 made the tag universal; a new contract must carry one)",
                p.display()
            );
        };
        let mut entries = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            if !is_diamond_key(l) {
                continue;
            }
            let mut pmat = 0;
            for probe in lines.iter().skip(i + 1).take(3) {
                if let Some(n) = pmat_id(probe) {
                    pmat = n;
                    break;
                }
            }
            let (mut lt, mut lf) = (None, None);
            for probe in lines.iter().skip(i + 1) {
                if is_top_level_key(probe) {
                    break;
                }
                if let Some(v) = quoted_after(probe, "lean_theorem:") {
                    lt = Some(v.rsplit('.').next().unwrap_or(&v).to_string());
                }
                if let Some(v) = quoted_after(probe, "lean_file:") {
                    lf = Some(v);
                }
            }
            entries.push(DiamondEntry {
                pmat,
                lean_theorem: lt,
                lean_file: lf,
            });
        }
        out.push(Contract {
            id,
            layer,
            yaml: p,
            entries,
        });
    }
    assert!(!out.is_empty(), "no contracts parsed");
    out
}

/// `  some_name_diamond:` at exactly two spaces of indent.
fn is_diamond_key(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("  ") else {
        return false;
    };
    if rest.starts_with(' ') {
        return false;
    }
    let Some(name) = rest.strip_suffix(':') else {
        return false;
    };
    name.contains("diamond") && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_top_level_key(line: &str) -> bool {
    line.strip_prefix("  ").is_some_and(|r| {
        !r.starts_with(' ') && r.ends_with(':') && !r.trim_end_matches(':').contains(' ')
    })
}

fn pmat_id(line: &str) -> Option<u32> {
    let i = line.find("PMAT-")?;
    let digits: String = line[i + 5..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn quoted_after(line: &str, key: &str) -> Option<String> {
    let t = line.trim_start();
    let rest = t.strip_prefix(key)?.trim();
    let inner = rest.strip_prefix('"')?;
    Some(inner.split('"').next()?.to_string())
}

// ---------------------------------------------------------------------------
// The needle.
// ---------------------------------------------------------------------------

const ORDINALS: [&str; 8] = [
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Claim {
    rank: u32,
    layer: u32,
    depth: u32,
    text: String,
}

/// Collapse a wrapped comment/docstring run into one line so a claim split
/// across two `#` lines is still one sentence. Most sites are wrapped.
fn flatten(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    for (i, line) in body.lines().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let t = line.trim_start();
        let t = t
            .strip_prefix("#")
            .or_else(|| t.strip_prefix("--"))
            .unwrap_or(t);
        out.push_str(t.trim());
    }
    out
}

/// Find every `<ordinal> (Layer N|LN) … contract(s) at depth-D` in `hay`.
///
/// The gap between the layer token and `contract` is capped so the needle
/// cannot reach across a clause into an unrelated sentence.
fn claims(hay: &str) -> Vec<Claim> {
    const GAP: usize = 45;
    let lower = hay.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut out = Vec::new();
    for (oi, ord) in ORDINALS.iter().enumerate() {
        let mut from = 0;
        while let Some(rel) = lower[from..].find(ord) {
            let start = from + rel;
            from = start + ord.len();
            if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
                continue;
            }
            let after = &lower[from..];
            if !after.starts_with(' ') {
                continue;
            }
            let after = after.trim_start();
            let consumed = lower[from..].len() - after.len();
            // `layer N`, `layer-N` or `LN`
            let (layer, used) = if let Some(r) = after
                .strip_prefix("layer ")
                .or_else(|| after.strip_prefix("layer-"))
            {
                match r.chars().next().and_then(|c| c.to_digit(10)) {
                    Some(n) if (1..=5).contains(&n) => (n, 6 + 1),
                    _ => continue,
                }
            } else if let Some(r) = after.strip_prefix('l') {
                match r.chars().next().and_then(|c| c.to_digit(10)) {
                    Some(n) if (1..=5).contains(&n) => (n, 2),
                    _ => continue,
                }
            } else {
                continue;
            };
            let tail_at = from + consumed + used;
            let tail = &lower[tail_at..];
            let Some(ci) = tail.find("contract") else {
                continue;
            };
            if ci > GAP {
                continue;
            }
            let rest = &tail[ci..];
            let Some(di) = rest.find("at depth-") else {
                continue;
            };
            if di > GAP {
                continue;
            }
            let digits: String = rest[di + 9..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            let Ok(depth) = digits.parse::<u32>() else {
                continue;
            };
            let end = tail_at + ci + di + 9 + digits.len();
            out.push(Claim {
                rank: oi as u32 + 1,
                layer,
                depth,
                text: hay[start..end.min(hay.len())].to_string(),
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The arithmetic.
// ---------------------------------------------------------------------------

/// Rank of `who` among the layer-`layer` contracts that had reached `depth` by
/// `as_of`, in arrival order. `None` when `who` is not in that set at all.
fn rank_at(all: &[Contract], who: &str, layer: u32, depth: u32, as_of: u32) -> Option<u32> {
    let mut arrivals: Vec<(u32, &str)> = all
        .iter()
        .filter(|c| c.layer == layer)
        .filter_map(|c| c.reached(depth).map(|p| (p, c.id.as_str())))
        .filter(|(p, _)| *p <= as_of)
        .collect();
    arrivals.sort();
    arrivals
        .iter()
        .position(|(_, id)| *id == who)
        .map(|i| i as u32 + 1)
}

#[derive(Debug)]
struct Finding {
    file: String,
    contract: String,
    as_of: u32,
    claim: Claim,
    truth: Option<u32>,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let truth = match self.truth {
            Some(n) => format!("rank {n}"),
            None => format!(
                "NO RANK — {} is not a Layer-{} contract at depth-{}",
                self.contract, self.claim.layer, self.claim.depth
            ),
        };
        write!(
            f,
            "{}: [{} @ PMAT-{}] says rank {} — derived truth: {}\n      {:?}",
            self.file, self.contract, self.as_of, self.claim.rank, truth, self.claim.text
        )
    }
}

/// Every ordinal claim in a `contracts/*.yaml` Diamond block, block-scoped to
/// the `# PMAT-NNN` that introduced it.
fn yaml_findings(all: &[Contract], flatten_prose: bool) -> (usize, Vec<Finding>) {
    let mut checked = 0;
    let mut bad = Vec::new();
    for c in all {
        let body = fs::read_to_string(&c.yaml).expect("read contract");
        let lines: Vec<&str> = body.lines().collect();
        let mut as_of: Option<u32> = None;
        let mut seen: BTreeSet<(String, u32)> = BTreeSet::new();
        for (i, l) in lines.iter().enumerate() {
            if is_top_level_key(l) {
                as_of = lines
                    .iter()
                    .skip(i + 1)
                    .take(3)
                    .find_map(|p| pmat_id(p))
                    .filter(|_| is_diamond_key(l));
            }
            let Some(p) = as_of else { continue };
            let window: Vec<&str> = lines.iter().skip(i).take(3).copied().collect();
            let hay = if flatten_prose {
                flatten(&window.join("\n"))
            } else {
                l.trim().to_string()
            };
            for claim in claims(&hay) {
                if !seen.insert((claim.text.clone(), p)) {
                    continue;
                }
                checked += 1;
                let truth = rank_at(all, &c.id, claim.layer, claim.depth, p);
                if truth != Some(claim.rank) {
                    bad.push(Finding {
                        file: format!("{}:{}", rel(&c.yaml), i + 1),
                        contract: c.id.clone(),
                        as_of: p,
                        claim,
                        truth,
                    });
                }
            }
        }
    }
    (checked, bad)
}

/// Every ordinal claim in a `/-- … -/` docstring of a contract's Lean module,
/// scoped by the `**PMAT-NNN` the block opens with. A block whose id is not one
/// of the owning contract's own Diamond ids is a cross-reference, not a record,
/// and is reported as unanchored rather than judged.
fn lean_findings(all: &[Contract]) -> (usize, Vec<Finding>, Vec<String>) {
    let root = workspace_root();
    let mut checked = 0;
    let mut bad = Vec::new();
    let mut unanchored = Vec::new();
    for c in all {
        let mut files: BTreeSet<String> = BTreeSet::new();
        for e in &c.entries {
            if let Some(f) = &e.lean_file {
                files.insert(f.clone());
            }
        }
        let owned: BTreeSet<u32> = c.entries.iter().map(|e| e.pmat).collect();
        for f in files {
            let path = root.join(&f);
            let body = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {f}: {e}"));
            for (start, block) in docstrings(&body) {
                let hay = flatten(block);
                let found = claims(&hay);
                if found.is_empty() {
                    continue;
                }
                let line = body[..start].lines().count();
                let id = hay.find("**PMAT-").and_then(|i| pmat_id(&hay[i..]));
                let Some(id) = id.filter(|i| owned.contains(i)) else {
                    unanchored.push(format!("{f}:{line} {:?}", found[0].text));
                    continue;
                };
                for claim in found {
                    checked += 1;
                    let truth = rank_at(all, &c.id, claim.layer, claim.depth, id);
                    if truth != Some(claim.rank) {
                        bad.push(Finding {
                            file: format!("{f}:{line}"),
                            contract: c.id.clone(),
                            as_of: id,
                            claim,
                            truth,
                        });
                    }
                }
            }
        }
    }
    (checked, bad, unanchored)
}

fn docstrings(body: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = body[from..].find("/--") {
        let open = from + rel;
        let Some(rel_end) = body[open + 3..].find("-/") else {
            break;
        };
        let close = open + 3 + rel_end;
        out.push((open, &body[open + 3..close]));
        from = close + 2;
    }
    out
}

fn rel(p: &Path) -> String {
    p.strip_prefix(workspace_root())
        .unwrap_or(p)
        .display()
        .to_string()
}

/// The live Diamond census, straight from the shipped binary.
fn run_diamond_json() -> String {
    let root = workspace_root();
    let out = std::process::Command::new(PathBuf::from(env!("CARGO_BIN_EXE_xpile")))
        .args([
            "diamond",
            "--json",
            "--contracts-dir",
            root.join("contracts").to_str().unwrap(),
        ])
        .output()
        .expect("run xpile diamond");
    assert!(
        out.status.success(),
        "xpile diamond failed:\n  stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// `{"id":"C-…","diamond_count":N,…}` — pulled by id so field order cannot
/// silently re-associate a count with the wrong contract.
fn live_diamond_count(json: &str, id: &str) -> Option<u64> {
    let needle = format!("\"id\":\"{id}\",\"diamond_count\":");
    let at = json.find(&needle)? + needle.len();
    let digits: String = json[at..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

// ---------------------------------------------------------------------------
// The invariants.
// ---------------------------------------------------------------------------

#[test]
fn every_layer_scoped_ordinal_in_a_contract_yaml_matches_the_derived_rank() {
    let all = parse_contracts();
    let (checked, bad) = yaml_findings(&all, true);
    assert!(
        checked >= 60,
        "the needle found only {checked} ordinal claims under contracts/*.yaml; \
         it read 71 when written, so a drop this large means the needle stopped \
         matching, not that the substrate stopped claiming"
    );
    assert!(
        bad.is_empty(),
        "{} layer-scoped ordinal(s) in contracts/*.yaml disagree with the rank \
         derived from the substrate's own layer tags and Diamond arrival order \
         ({checked} checked):\n  {}",
        bad.len(),
        bad.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn every_layer_scoped_ordinal_in_a_lean_docstring_matches_the_derived_rank() {
    let all = parse_contracts();
    let (checked, bad, unanchored) = lean_findings(&all);
    assert!(
        checked >= 24,
        "the needle found only {checked} ordinal claims in contract Lean \
         docstrings; it read 29 when written"
    );
    assert!(
        bad.is_empty(),
        "{} layer-scoped ordinal(s) in contracts/lean/*.lean disagree with the \
         derived rank ({checked} checked, {} unanchored):\n  {}",
        bad.len(),
        unanchored.len(),
        bad.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// GROUND TRUTH. Every rank in this gate is computed from the parsed Diamond
/// entries; a parser that skipped one would shift every later rank by one and
/// the gate would stay green while lying. The per-contract count is
/// cross-checked against `xpile diamond --json` — the shipped binary reaching
/// the same entries through its own parser — and every named theorem must exist
/// in the module it names.
#[test]
fn the_reconstruction_matches_the_live_diamond_report() {
    let all = parse_contracts();
    let root = workspace_root();
    let json = run_diamond_json();
    let mut total = 0usize;
    for c in &all {
        let live = live_diamond_count(&json, &c.id).unwrap_or_else(|| {
            panic!("`xpile diamond --json` does not report {}", c.id);
        });
        assert_eq!(
            c.entries.len() as u64,
            live,
            "{}: this gate parsed {} `*_diamond:` entries, `xpile diamond` \
             counts {live}. The Diamond parse is the ground truth for every \
             rank checked here, so a disagreement invalidates all of them.",
            c.id,
            c.entries.len()
        );
        for e in &c.entries {
            let (Some(t), Some(f)) = (&e.lean_theorem, &e.lean_file) else {
                panic!("{}: a Diamond entry names no lean_theorem/lean_file", c.id);
            };
            let body = fs::read_to_string(root.join(f)).unwrap_or_else(|e| panic!("read {f}: {e}"));
            assert!(
                body.lines().any(|l| {
                    ["theorem ", "def ", "lemma ", "instance "]
                        .iter()
                        .any(|k| l.strip_prefix(k).is_some_and(|r| r.starts_with(t.as_str())))
                }),
                "{}: {f} declares no `{t}` — the entry this gate dates cannot be \
                 located in the module it names",
                c.id
            );
        }
        total += c.entries.len();
    }
    assert!(
        total >= 200,
        "only {total} Diamond entries parsed across {} contracts; the substrate \
         held 207 when this gate was written",
        all.len()
    );
}

/// DATING. Eleven entries carry no `# PMAT-NNN` header and are treated as
/// present from the start. That choice is only safe if it cannot move a rank
/// this gate checks — proven here, not argued: every unattributed entry sits on
/// a contract that never passes depth-2, and no claim in the corpus is phrased
/// below depth-3.
#[test]
fn unattributed_diamonds_cannot_reach_a_gated_ordinal() {
    let all = parse_contracts();
    let mut roster = Vec::new();
    for c in &all {
        let n = c.entries.iter().filter(|e| e.pmat == 0).count();
        if n == 0 {
            continue;
        }
        roster.push(format!("{} ({n} of {})", c.id, c.entries.len()));
        assert!(
            (c.entries.len() as u32) < SHALLOWEST_CLAIM_DEPTH,
            "{} carries {n} undated Diamond entr(ies) AND reaches depth-{} — an \
             undated entry on a contract deep enough to be ranked would silently \
             shift that rank. Give the entry a `# PMAT-NNN:` header.",
            c.id,
            c.entries.len()
        );
    }
    let (yaml_n, _) = yaml_findings(&all, true);
    let (lean_n, _, _) = lean_findings(&all);
    let shallowest = all
        .iter()
        .flat_map(|c| {
            let body = fs::read_to_string(&c.yaml).unwrap_or_default();
            claims(&flatten(&body)).into_iter().map(|x| x.depth)
        })
        .min()
        .unwrap_or(u32::MAX);
    assert!(
        shallowest >= SHALLOWEST_CLAIM_DEPTH,
        "a claim is phrased at depth-{shallowest}, below the depth-\
         {SHALLOWEST_CLAIM_DEPTH} floor this exemption relies on"
    );
    eprintln!(
        "XPILE-LAYERORDINAL-001 dating: {} undated entr(ies) on {} \
         never-deeper-than-depth-2 contract(s): {}. Shallowest claim in the \
         corpus: depth-{shallowest}. Ranks checked: {yaml_n} yaml + {lean_n} lean.",
        roster.len(),
        roster.len(),
        roster.join(", ")
    );
}

/// SUBJECT. The gate prints what it reaches, so a corpus that quietly stops
/// being scanned shows up as a number rather than as silence.
#[test]
fn the_gate_reports_what_it_reaches() {
    let all = parse_contracts();
    let (yaml_n, _) = yaml_findings(&all, true);
    let (lean_n, _, unanchored) = lean_findings(&all);
    let tagged = all.iter().filter(|c| (1..=5).contains(&c.layer)).count();
    eprintln!(
        "XPILE-LAYERORDINAL-001 reach: {tagged}/{} contracts carry a layer tag; \
         {yaml_n} ordinal claims under contracts/*.yaml and {lean_n} in contract \
         Lean docstrings are ranked; {} Lean docstring block(s) name a PMAT id \
         that is not one of their own contract's Diamond ids and are NOT judged: \
         {:?}",
        all.len(),
        unanchored.len(),
        unanchored
    );
    assert_eq!(tagged, all.len(), "a contract lost its layer tag");
    assert!(yaml_n > 0 && lean_n > 0, "a corpus went unscanned");
}

/// NEEDLE, red half. A wrong ordinal must be reported and a right one must not.
#[test]
fn the_needle_reports_a_wrong_ordinal_and_spares_a_right_one() {
    let wrong = claims("Diamond depth-10 broadened: second L2 contract at depth-10");
    assert_eq!(wrong.len(), 1, "the needle missed the shipped defect shape");
    assert_eq!((wrong[0].rank, wrong[0].layer, wrong[0].depth), (2, 2, 10));

    let long =
        claims("first L3 contract, and separately a great many other things happened, at depth-9");
    assert!(
        long.is_empty(),
        "the needle reached across a clause: {long:?}"
    );

    let all = parse_contracts();
    // C-BASHRS-POSIX-IDEMPOTENCE is the second Layer-1 contract at depth-10,
    // after C-PY-INT-ARITH — the true reading of the sentence above.
    assert_eq!(
        rank_at(&all, "C-BASHRS-POSIX-IDEMPOTENCE", 1, 10, 401),
        Some(2),
        "the corrected reading must hold, or the repair was wrong too"
    );
    assert_eq!(
        rank_at(&all, "C-BASHRS-POSIX-IDEMPOTENCE", 2, 10, 401),
        None,
        "a Layer-1 contract must have no rank inside the Layer-2 set"
    );
    // C-XLATE-PY-LIST-TO-VEC was FIRST, not second/third, at every depth 4..13.
    for depth in 4..=13 {
        let p = all
            .iter()
            .find(|c| c.id == "C-XLATE-PY-LIST-TO-VEC")
            .and_then(|c| c.reached(depth))
            .expect("pylist reached this depth");
        assert_eq!(
            rank_at(&all, "C-XLATE-PY-LIST-TO-VEC", 2, depth, p),
            Some(1),
            "PyListToVec's Layer-2 rank at depth-{depth}"
        );
    }
}

/// NEEDLE, wrapping. Most sites split the claim over two comment lines.
#[test]
fn the_needle_reads_across_a_wrapped_comment() {
    let wrapped = "    # to depth-6 as the SECOND L2 contract at depth-6+ (Bashrs was\n    # first via PMAT-357).";
    assert!(
        claims(wrapped.lines().next().unwrap()).len() == 1,
        "sanity: this particular site happens to fit on one line"
    );
    let split = "    # depth-4 to depth-5, adding a SECOND Layer 2 contract at\n    # depth-5 (Bashrs was first via PMAT-346).";
    assert!(
        claims(split.lines().next().unwrap()).is_empty(),
        "the line-local read must NOT see the wrapped claim, or this control \
         proves nothing"
    );
    assert_eq!(
        claims(&flatten(split)).len(),
        1,
        "flattening must recover the wrapped claim"
    );

    // And it is load-bearing on the live corpus — modestly, by MEASUREMENT.
    // On `b31fe0ac` flattening reported 20 defective yaml sites where the
    // line-local read reported 19, and reached 29 Lean docstring claims where
    // the line-local read reached 27: 3 of 29 sites. That is the honest figure;
    // an earlier draft of this guard asserted "more than doubles" and was
    // deleted when the corpus said 62 -> 71.
    let all = parse_contracts();
    let (flat_n, _) = yaml_findings(&all, true);
    let (line_n, _) = yaml_findings(&all, false);
    assert!(
        flat_n > line_n,
        "prose flattening now reaches no more claims than a line-local read \
         (line-local {line_n}, flattened {flat_n}) — either the corpus stopped \
         wrapping its claims or the flattening broke; re-measure before \
         trusting it"
    );
    eprintln!(
        "XPILE-LAYERORDINAL-001 flattening: line-local {line_n} claims, \
         flattened {flat_n}."
    );
}

/// VOCABULARY. The layer numbering is read from the spec, not hard-coded.
#[test]
fn canonical_layer_numbering_is_read_from_the_taxonomy_doc() {
    let words = canonical_layer_words();
    for (w, n) in [
        ("semantics", 1),
        ("translation", 2),
        ("architectural", 3),
        ("hybrid", 4),
        ("compile", 5),
    ] {
        assert_eq!(
            words.get(w),
            Some(&n),
            "{TAXONOMY_DOC} no longer numbers `{w}` as Layer {n}; every rank in \
             this gate is keyed on that table ({words:?})"
        );
    }
}
