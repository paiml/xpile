//! XPILE-LAYERLABEL-001 (PMAT-1460) — a contract may not be filed under a
//! layer other than the one it declares.
//!
//! Every contract carries an optional `metadata.xpile.layer` tag, and
//! `docs/specifications/sub/contract-taxonomy.md` numbers the five layers that
//! tag names. Nine of the 35 contracts carry it. Three of those nine —
//! `C-NOTATION-LATEX-MATH-TO-EQUATION`, `C-XLATE-LEAN-TO-RUST` and
//! `C-XLATE-RUST-FN-TO-LEAN-THM` — declare `layer: translation`, i.e. **Layer
//! 2**, and open their own `metadata.description` with the sentence "Layer 2
//! of the xpile contract taxonomy (translation)".
//!
//! On 2026-07-29, at `771f1652`, those same three contracts were written as
//! **Layer 5** at **46** sites under `contracts/` — the number this gate itself
//! reports on that tree — and a further **3** sites called `Notation` **Layer
//! 4**. Eighteen of the repaired lines are `invariants:`/`postconditions:`
//! entries, the NORMATIVE slots PMAT-1454 established have no citation escape.
//! So one contract was filed under three different layers, and the refutation
//! of the wrong two was 600 lines up in the same file, in a machine-readable
//! field.
//!
//! The 3 Layer-4 sites write the layer BEFORE the name and are outside this
//! gate's needle; they were repaired by hand and the hole is pinned by
//! [`the_needle_cannot_see_a_layer_written_before_the_name`] rather than
//! implied to be covered.
//!
//! ## Never-true, not aged
//!
//! `git log -S"layer: translation"` puts the tag in `cdcece9c`, the initial
//! commit. The first `(Layer 5)` landed at PMAT-334 / PMAT-351 / PMAT-352,
//! months later. Every one of those attributions was false the moment it was
//! typed — this is not a count that went stale as the substrate grew, which
//! matters because the two defects have different fixes.
//!
//! ## Why it read as checked, and which direction it ran
//!
//! The layer attributions carried tallies with them — 41 of the repaired lines
//! state a Layer-5-scoped contract count: "8 contracts at depth-5+
//! (**2 Layer 5** contracts at depth-5)", "**3 Layer 5** contracts at
//! depth-5", "**4 Layer 5** contracts at depth-5", "the **fourth L5** contract
//! at depth-7+". Under the substrate's own tags exactly ONE Layer-5 contract
//! is past depth-1 (`C-COMPILE-RUST-TO-PTX-MMA`), so the published Layer-5
//! breadth ran 4× over — and Layer-5 breadth is the headline of the
//! ACROSS-LAYERS Diamond story. Meanwhile the sentence next door got it right:
//! `Notation.lean` writes "PMAT-395 (PyListToVec **L2**), PMAT-396
//! (XlateLeanToRust **L5**)" — the untagged contract correct, the tagged one
//! wrong, in one list.
//!
//! ## What this gate decides, and what it cannot
//!
//! It decides **self-consistency**, which is arithmetic: if a contract
//! declares a layer, no text under `contracts/` may attribute it to a
//! different one. It does NOT decide whether a contract's declared layer is
//! the *right* layer, and it cannot check the 26 contracts that declare no
//! layer at all — so the 127 layer-scoped tallies phrased over untagged
//! contracts ("the second L2 contract at depth-10", "3 L3 contracts at
//! depth-7") are outside its reach and were left alone. That limitation is not
//! a comment: [`the_gate_reports_how_much_of_the_substrate_it_can_reach`]
//! measures it and prints the untagged roster, and the gate's reach grows on
//! its own as contracts get tagged.
//!
//! Also deliberately untouched: the substrate-wide depth counts riding in the
//! same sentences (`8 contracts at depth-5+`, live 13). That is a separate
//! measured class — 112 false sites under `contracts/`, every one UNDERSTATING
//! because the substrate outgrew its own frozen records — with a different
//! ground truth (`xpile diamond --json`'s `depth_N_plus`) and a genuine open
//! question about whether a broadening record may freeze a count. Four
//! sentences here had to be rewritten anyway; rather than restate a number
//! this slice did not gate, the rewrite drops it. The other 108 stand.
//!
//! ## Blind spots, each pinned by a control that PASSES
//!
//!   * SUBJECT — [`the_subject_covers_every_contract_artifact_kind`] asserts
//!     the walk reaches `.yaml`, `.lean`, `.rs` and `.md`, which is the corpus
//!     PMAT-1452 and PMAT-1456 had to widen to twice.
//!   * WRAPPING — [`the_needle_reads_across_a_wrapped_comment`]. Three of the
//!     original sites put the contract name on one line and `(Layer 5)` on the
//!     next; a line-local needle finds neither. Prose runs are flattened, the
//!     way `claims_drift::substrate_blocks` learned to.
//!   * NEEDLE — [`the_needle_reports_a_wrong_layer_and_spares_a_right_one`]
//!     and [`the_needle_does_not_reach_across_a_clause`]. The loose version of
//!     this needle (a 40-character gap) reported two false positives on the
//!     live tree — `CompileRustToPtxMma), PMAT-346 pushes Bashrs (Layer 2)`
//!     and `C-XLATE-RUST-FN-TO-LEAN-THM) are Layer-1/Layer-2 contracts` — so
//!     the gap is capped at three non-alphanumeric characters and both shapes
//!     are pinned as constructed cases that must NOT be reported.
//!   * VOCABULARY — [`canonical_layer_numbering_is_read_from_the_taxonomy_doc`]
//!     derives the name→number map from the spec table instead of hard-coding
//!     it, and fails loudly if a rename ever makes two layers share a word.
//!   * DIRECTION — [`the_needle_cannot_see_a_layer_written_before_the_name`]
//!     pins what this gate does NOT reach, so the 3 hand-repaired sites are a
//!     disclosed hole rather than an assumed catch.
//!
//! Every one of those is load-bearing by measurement, not by argument. Run
//! against the pre-repair contracts the gate reports **46** sites; with the
//! subject narrowed to `.yaml` it reports **23**, with prose flattening removed
//! **37**, and with the derived `Notation` alias suppressed **41**. Each guard
//! is worth the sites it is worth, and none of them is decoration.

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

// ---------------------------------------------------------------------------
// The canonical layer vocabulary, derived rather than declared.
// ---------------------------------------------------------------------------

/// `word → layer number` for every word appearing in a layer's name.
///
/// `metadata.xpile.layer` spells a layer by ONE word of its taxonomy name
/// (`translation` for "Layer 2: Translation", `compile` for "Layer 5:
/// Compile-time / IR", `semantics` for "Layer 1: Language semantics"), so the
/// map is keyed on words, not on the whole name. Hyphenated and slashed words
/// are split, and each fragment's leading alphabetic run is the key.
fn canonical_layer_words() -> BTreeMap<String, u32> {
    let doc = workspace_root().join(TAXONOMY_DOC);
    let body = fs::read_to_string(&doc).unwrap_or_else(|e| panic!("read {TAXONOMY_DOC}: {e}"));
    let mut out: BTreeMap<String, u32> = BTreeMap::new();
    let mut collisions: Vec<String> = Vec::new();
    for line in body.lines() {
        let t = line.trim();
        // `| **Layer 2: Translation** | `kernel` | … |`
        let Some(rest) = t.strip_prefix("| **Layer ") else {
            continue;
        };
        let Some((num, name)) = rest.split_once(':') else {
            continue;
        };
        let Ok(n) = num.trim().parse::<u32>() else {
            continue;
        };
        let name = name.split("**").next().unwrap_or("");
        for frag in name.split(['-', '/', ' ']) {
            let word: String = frag
                .chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .flat_map(|c| c.to_lowercase())
                .collect();
            if word.len() < 2 {
                continue;
            }
            if let Some(prev) = out.insert(word.clone(), n) {
                if prev != n {
                    collisions.push(format!("{word:?} names both Layer {prev} and Layer {n}"));
                }
            }
        }
    }
    assert!(
        collisions.is_empty(),
        "{TAXONOMY_DOC} no longer names the five layers unambiguously, so a \
         `metadata.xpile.layer` tag cannot be resolved to a number: {collisions:?}. \
         Rename the layer or teach this gate the new spelling — do not guess."
    );
    out
}

// ---------------------------------------------------------------------------
// What each contract declares about itself.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Declared {
    id: String,
    layer: u32,
    /// The spellings this contract goes by in substrate prose.
    names: Vec<String>,
}

/// `C-XLATE-LEAN-TO-RUST` → `XlateLeanToRust`.
fn camel_of(id: &str) -> String {
    id.trim_start_matches("C-")
        .split('-')
        .map(|seg| {
            let mut cs = seg.chars();
            match cs.next() {
                None => String::new(),
                Some(f) => f
                    .to_uppercase()
                    .chain(cs.flat_map(|c| c.to_lowercase()))
                    .collect(),
            }
        })
        .collect()
}

/// Every contract that declares a layer, with the names it is written by.
///
/// The short alias (`Notation` for `NotationLatexMathToEquation`) is DERIVED,
/// not listed: a contract's first identifier segment is an alias only when it
/// is at least six characters and is not a substring of any other contract's
/// full name. That keeps `Compile` (five contracts) and `Xlate` (nine) out
/// while admitting `Notation`, which is how the substrate actually writes it.
fn declared_layers() -> Vec<Declared> {
    let root = workspace_root();
    let words = canonical_layer_words();
    let mut all_camels: Vec<String> = Vec::new();
    let mut raw: Vec<(String, String)> = Vec::new(); // (id, tag)

    let dir = root.join("contracts");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir contracts: {e}"))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("yaml"))
        .collect();
    paths.sort();

    for p in &paths {
        let rel = p
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let body = fs::read_to_string(p).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let doc: serde_yaml::Value =
            serde_yaml::from_str(&body).unwrap_or_else(|e| panic!("parse {rel}: {e}"));
        let md = &doc["metadata"];
        let id = md["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{rel}: metadata.id"))
            .to_string();
        all_camels.push(camel_of(&id));
        if let Some(tag) = md["xpile"]["layer"].as_str() {
            raw.push((id, tag.to_string()));
        }
    }

    raw.into_iter()
        .map(|(id, tag)| {
            let layer = *words.get(tag.as_str()).unwrap_or_else(|| {
                panic!(
                    "{id}: `metadata.xpile.layer: {tag}` is not a layer named by \
                     {TAXONOMY_DOC} (known: {:?})",
                    words.keys().collect::<Vec<_>>()
                )
            });
            let camel = camel_of(&id);
            let mut names = vec![camel.clone()];
            if let Some(head) = first_segment(&camel) {
                let unique = !all_camels.iter().any(|c| *c != camel && c.contains(&head));
                if head.len() >= 6 && unique {
                    names.push(head);
                }
            }
            Declared { id, layer, names }
        })
        .collect()
}

/// `NotationLatexMathToEquation` → `Notation`.
fn first_segment(camel: &str) -> Option<String> {
    let mut cs = camel.char_indices();
    cs.next()?;
    for (i, c) in cs {
        if c.is_uppercase() {
            return Some(camel[..i].to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// The subject: every artifact under `contracts/`.
// ---------------------------------------------------------------------------

fn substrate_files() -> Vec<(String, String)> {
    let root = workspace_root();
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
        let mut paths: Vec<PathBuf> = fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        paths.sort();
        for p in paths {
            if p.is_dir() {
                walk(&p, root, out);
            } else if matches!(
                p.extension().and_then(|s| s.to_str()),
                Some("yaml") | Some("lean") | Some("rs") | Some("md")
            ) {
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                let body = fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {rel}: {e}"));
                out.push((rel, body));
            }
        }
    }
    let mut out = Vec::new();
    walk(&root.join("contracts"), &root, &mut out);
    out
}

// ---------------------------------------------------------------------------
// Flattening: a claim that wraps must not be split down the middle.
// ---------------------------------------------------------------------------

/// One flattened run of prose, with each line's start offset kept so a match
/// can be attributed back to a line number.
struct Run {
    flat: String,
    marks: Vec<(usize, usize)>, // (offset in `flat`, 1-based line number)
}

impl Run {
    fn line_at(&self, at: usize) -> usize {
        self.marks
            .iter()
            .rev()
            .find(|&&(off, _)| off <= at)
            .map_or(0, |&(_, n)| n)
    }
}

fn strip_marker(t: &str) -> &str {
    for m in ["/-!", "/--", "///", "//!", "//", "--", "#"] {
        if let Some(rest) = t.strip_prefix(m) {
            return rest.trim_start();
        }
    }
    t
}

/// Split a substrate artifact into flattened runs of consecutive non-blank
/// lines, comment markers removed.
///
/// PMAT-1460: three of the original mislabels wrote the contract name at the
/// end of one comment line and `(Layer 5)` at the start of the next. Scanning
/// line by line finds neither half, and splicing a `#` into the middle of the
/// phrase (what a naive join does) finds neither either — the same failure
/// `claims_drift::substrate_blocks` was taught to avoid.
fn runs_of(body: &str) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    let mut cur: Option<Run> = None;
    for (i, raw) in body.lines().enumerate() {
        let t = raw.trim();
        if t.is_empty() {
            out.extend(cur.take());
            continue;
        }
        let text = strip_marker(t);
        let run = cur.get_or_insert_with(|| Run {
            flat: String::new(),
            marks: Vec::new(),
        });
        if !run.flat.is_empty() {
            run.flat.push(' ');
        }
        run.marks.push((run.flat.len(), i + 1));
        run.flat.push_str(text);
    }
    out.extend(cur);
    out
}

// ---------------------------------------------------------------------------
// The needle: a contract name immediately followed by a layer token.
// ---------------------------------------------------------------------------

/// Read `(Layer 5)` / `Layer-5` / `L5` starting at `cs[i]`, allowing at most
/// [`MAX_GAP`] non-alphanumeric filler characters before it.
///
/// The gap cap is the whole discriminator. A 40-character gap — the first
/// version of this needle — reported `CompileRustToPtxMma), PMAT-346 pushes
/// Bashrs (Layer 2)` and `C-XLATE-RUST-FN-TO-LEAN-THM) are Layer-1/Layer-2
/// contracts` as mislabels; both are correct sentences whose next clause is
/// about a different contract. Three characters admits `X (Layer 2)`, `X
/// L2)` and `X — Layer 2`, and stops at the first word that is not the token.
const MAX_GAP: usize = 3;

fn layer_token_after(cs: &[char], mut i: usize) -> Option<u32> {
    let mut gap = 0usize;
    while i < cs.len() && !cs[i].is_alphanumeric() {
        if gap == MAX_GAP {
            return None;
        }
        gap += 1;
        i += 1;
    }
    if i >= cs.len() {
        return None;
    }
    // `Layer` [sep] digit   |   `L` digit
    let word: String = cs[i..].iter().take(5).collect();
    let mut j = if word.eq_ignore_ascii_case("layer") {
        let mut k = i + 5;
        if k < cs.len() && (cs[k] == ' ' || cs[k] == '-') {
            k += 1;
        }
        k
    } else if cs[i] == 'L' {
        i + 1
    } else {
        return None;
    };
    let start = j;
    while j < cs.len() && cs[j].is_ascii_digit() {
        j += 1;
    }
    if j == start {
        return None;
    }
    // A word boundary after the number, or `L2x` reads as a layer.
    if cs.get(j).is_some_and(|c| c.is_alphanumeric()) {
        return None;
    }
    cs[start..j].iter().collect::<String>().parse().ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Attribution {
    id: String,
    declared: u32,
    written: u32,
    line: usize,
}

/// Every layer attributed to `who` inside one flattened run.
fn attributions_in(run: &Run, who: &[Declared]) -> Vec<Attribution> {
    let cs: Vec<char> = run.flat.chars().collect();
    // char index → byte offset, so a hit maps back to a line.
    let mut byte_of: Vec<usize> = Vec::with_capacity(cs.len() + 1);
    let mut b = 0usize;
    for c in &cs {
        byte_of.push(b);
        b += c.len_utf8();
    }
    byte_of.push(b);

    let mut out = Vec::new();
    for d in who {
        for name in &d.names {
            let nlen = name.chars().count();
            let ncs: Vec<char> = name.chars().collect();
            for i in 0..cs.len().saturating_sub(nlen.saturating_sub(1)) {
                if cs[i..].len() < nlen || cs[i..i + nlen] != ncs[..] {
                    continue;
                }
                // Word boundary at the head, or `XNotation` matches `Notation`.
                if i > 0 && (cs[i - 1].is_alphanumeric() || cs[i - 1] == '-') {
                    continue;
                }
                if let Some(written) = layer_token_after(&cs, i + nlen) {
                    out.push(Attribution {
                        id: d.id.clone(),
                        declared: d.layer,
                        written,
                        line: run.line_at(byte_of[i]),
                    });
                }
            }
        }
    }
    out
}

/// Every attribution under `contracts/`, wrong ones and right ones alike.
fn all_attributions() -> Vec<(String, Attribution)> {
    let who = declared_layers();
    let mut out = Vec::new();
    for (rel, body) in substrate_files() {
        for run in runs_of(&body) {
            for a in attributions_in(&run, &who) {
                out.push((rel.clone(), a));
            }
        }
    }
    out
}

// ===========================================================================
// The gate.
// ===========================================================================

#[test]
fn no_contract_is_attributed_to_a_layer_other_than_its_declared_one() {
    let offences: Vec<_> = all_attributions()
        .into_iter()
        .filter(|(_, a)| a.written != a.declared)
        .collect();
    assert!(
        offences.is_empty(),
        "{} site(s) under `contracts/` file a contract under a layer it does not \
         declare. `metadata.xpile.layer` and {TAXONOMY_DOC} are the substrate's own \
         answer; prose that disagrees with them is wrong, not an alternative \
         numbering:\n{}",
        offences.len(),
        offences
            .iter()
            .map(|(rel, a)| format!(
                "  {rel}:{} — {} declares Layer {} and is written Layer {}",
                a.line, a.id, a.declared, a.written
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn the_gate_reports_how_much_of_the_substrate_it_can_reach() {
    let root = workspace_root();
    let mut total = 0usize;
    let mut untagged: Vec<String> = Vec::new();
    let mut paths: Vec<PathBuf> = fs::read_dir(root.join("contracts"))
        .expect("read_dir contracts")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("yaml"))
        .collect();
    paths.sort();
    for p in &paths {
        let doc: serde_yaml::Value =
            serde_yaml::from_str(&fs::read_to_string(p).expect("read")).expect("parse");
        total += 1;
        if doc["metadata"]["xpile"]["layer"].as_str().is_none() {
            untagged.push(doc["metadata"]["id"].as_str().unwrap_or("?").to_string());
        }
    }
    let tagged = total - untagged.len();

    // This is a DISCLOSURE, machine-checked so it cannot rot into prose: the
    // gate above decides only the tagged contracts, and the roster of what it
    // cannot decide is printed by the assertion that keeps this honest.
    assert!(
        tagged >= 1 && total >= 1,
        "no contract declares `metadata.xpile.layer`, so \
         `no_contract_is_attributed_to_a_layer_other_than_its_declared_one` \
         decides nothing at all"
    );
    assert_eq!(
        tagged + untagged.len(),
        total,
        "every contract is either tagged or untagged"
    );
    // The reach is a measurement, not a target: this prints on failure of the
    // partition-complete branch below, which is the state we WANT to reach.
    if untagged.is_empty() {
        // Partition complete — the 127 layer-scoped tallies PMAT-1460 could not
        // decide ("the second L2 contract at depth-10") become checkable, and
        // this gate should grow a numeric arm for them.
        panic!(
            "all {total} contracts now declare `metadata.xpile.layer`. The layer \
             partition is complete, so the layer-scoped depth tallies under \
             `contracts/` are now decidable — extend this gate rather than \
             deleting this branch (PMAT-1460 standing lead)."
        );
    }
    eprintln!(
        "XPILE-LAYERLABEL-001 reach: {tagged} of {total} contracts declare a layer; \
         {} do not and are outside this gate's subject: {untagged:?}",
        untagged.len()
    );
}

// ---------------------------------------------------------------------------
// Blind-spot pins. Each establishes a capability with a control that PASSES.
// ---------------------------------------------------------------------------

#[test]
fn canonical_layer_numbering_is_read_from_the_taxonomy_doc() {
    let words = canonical_layer_words();
    let layers: BTreeSet<u32> = words.values().copied().collect();
    assert_eq!(
        layers,
        BTreeSet::from([1, 2, 3, 4, 5]),
        "{TAXONOMY_DOC} must number exactly five layers; parsed {layers:?} from \
         its `| **Layer N: …**` table. If the table moved, this gate is reading \
         nothing and every attribution below is unchecked."
    );
    // The two the finding turned on. Without these the parse could be a table
    // of the right shape and the wrong content.
    assert_eq!(
        words.get("translation"),
        Some(&2),
        "`translation` is Layer 2"
    );
    assert_eq!(words.get("compile"), Some(&5), "`compile` is Layer 5");
}

#[test]
fn every_declared_layer_tag_resolves_and_at_least_one_contract_declares_one() {
    let who = declared_layers(); // panics if a tag does not resolve
    assert!(
        !who.is_empty(),
        "no contract declares `metadata.xpile.layer`; the gate's subject is empty"
    );
    // The three that carried the defect, so a metadata regression that drops
    // the tag cannot silently empty the subject.
    let by_id: BTreeMap<&str, u32> = who.iter().map(|d| (d.id.as_str(), d.layer)).collect();
    for id in [
        "C-NOTATION-LATEX-MATH-TO-EQUATION",
        "C-XLATE-LEAN-TO-RUST",
        "C-XLATE-RUST-FN-TO-LEAN-THM",
    ] {
        assert_eq!(
            by_id.get(id),
            Some(&2),
            "{id} declares Layer 2 (translation)"
        );
    }
    assert_eq!(
        by_id.get("C-COMPILE-RUST-TO-PTX-MMA"),
        Some(&5),
        "C-COMPILE-RUST-TO-PTX-MMA declares Layer 5 (compile)"
    );
}

#[test]
fn the_short_alias_is_derived_and_stays_unambiguous() {
    let who = declared_layers();
    let notation = who
        .iter()
        .find(|d| d.id == "C-NOTATION-LATEX-MATH-TO-EQUATION")
        .expect("notation contract declares a layer");
    assert!(
        notation.names.contains(&"Notation".to_string()),
        "`Notation` is how the substrate writes C-NOTATION-LATEX-MATH-TO-EQUATION \
         in over half the sites this gate must reach; aliases are {:?}",
        notation.names
    );
    // …and the ambiguous heads stay out, or one wrong `Compile (Layer 2)`
    // would be charged to five contracts at once.
    for d in &who {
        assert!(
            !d.names.iter().any(|n| n == "Compile" || n == "Xlate"),
            "{} took an ambiguous alias: {:?}",
            d.id,
            d.names
        );
    }
}

#[test]
fn the_subject_covers_every_contract_artifact_kind() {
    let files = substrate_files();
    for ext in ["yaml", "lean", "rs", "md"] {
        assert!(
            files
                .iter()
                .any(|(rel, _)| rel.ends_with(&format!(".{ext}"))),
            "the walk found no `.{ext}` under contracts/ — PMAT-1452 had to widen \
             this corpus to the YAMLs and PMAT-1456 to the Kani harnesses, and \
             both times the live falsehoods were in the kind that was missing"
        );
    }
    assert!(
        files.len() >= 90,
        "only {} artifacts under contracts/; the walk is not reaching the tree",
        files.len()
    );
}

#[test]
fn the_needle_finds_the_live_correct_attributions() {
    // Anti-vacuity: the gate above passes because the attributions AGREE, not
    // because the needle matches nothing. The substrate is full of correct
    // `CompileRustToPtxMma (Layer 5)` sites and they must be seen.
    let all = all_attributions();
    assert!(
        all.len() >= 5,
        "the needle found only {} layer attribution(s) under contracts/; a gate \
         that sees nothing reports nothing",
        all.len()
    );
    assert!(
        all.iter()
            .any(|(_, a)| a.id == "C-COMPILE-RUST-TO-PTX-MMA" && a.written == 5),
        "the needle did not find a single `CompileRustToPtxMma (Layer 5)`, which \
         the substrate writes repeatedly — it is matching the wrong shape"
    );
}

#[test]
fn the_needle_reads_across_a_wrapped_comment() {
    // The exact shape of contracts/notation-…-v1.yaml:602-603 before PMAT-1460:
    // name at the end of one comment line, layer at the start of the next.
    let body = "    # from 9 to 10 contracts. Pushes NotationLatexMathToEquation\n\
                \x20   # (Layer 5) from depth-2 to depth-3. Seventh substrate-wide\n";
    let who = vec![Declared {
        id: "C-NOTATION-LATEX-MATH-TO-EQUATION".into(),
        layer: 2,
        names: vec!["NotationLatexMathToEquation".into(), "Notation".into()],
    }];
    let hits: Vec<_> = runs_of(body)
        .iter()
        .flat_map(|r| attributions_in(r, &who))
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "a wrapped attribution must be read as one claim; got {hits:?}"
    );
    assert_eq!(hits[0].written, 5);
    assert_eq!(hits[0].declared, 2);
    assert_eq!(hits[0].line, 1, "reported against the line the NAME is on");

    // Control that PASSES: the repaired spelling of the same two lines.
    let fixed = body.replace("(Layer 5)", "(Layer 2)");
    let ok: Vec<_> = runs_of(&fixed)
        .iter()
        .flat_map(|r| attributions_in(r, &who))
        .collect();
    assert_eq!(ok.len(), 1, "the repaired form is still SEEN…");
    assert_eq!(ok[0].written, 2, "…and is not an offence");
}

#[test]
fn the_needle_reports_a_wrong_layer_and_spares_a_right_one() {
    let who = vec![
        Declared {
            id: "C-XLATE-LEAN-TO-RUST".into(),
            layer: 2,
            names: vec!["XlateLeanToRust".into()],
        },
        Declared {
            id: "C-COMPILE-RUST-TO-PTX-MMA".into(),
            layer: 5,
            names: vec!["CompileRustToPtxMma".into()],
        },
    ];
    // The real Notation.lean:1516 sentence: one list, one right, one wrong.
    let body = "PMAT-395 (PyListToVec L2), PMAT-396 (XlateLeanToRust L5), and\n\
                PMAT-297 (CompileRustToPtxMma L5).\n";
    let hits: Vec<_> = runs_of(body)
        .iter()
        .flat_map(|r| attributions_in(r, &who))
        .collect();
    let wrong: Vec<_> = hits.iter().filter(|a| a.written != a.declared).collect();
    assert_eq!(wrong.len(), 1, "exactly one offence in {hits:?}");
    assert_eq!(wrong[0].id, "C-XLATE-LEAN-TO-RUST");
    assert!(
        hits.iter()
            .any(|a| a.id == "C-COMPILE-RUST-TO-PTX-MMA" && a.written == 5),
        "the correct sibling in the SAME sentence must be seen and spared"
    );
}

#[test]
fn the_needle_does_not_reach_across_a_clause() {
    // Both of these are live, correct sentences that the 40-character-gap
    // version of this needle reported as mislabels. They must stay silent.
    let who = vec![
        Declared {
            id: "C-COMPILE-RUST-TO-PTX-MMA".into(),
            layer: 5,
            names: vec!["CompileRustToPtxMma".into()],
        },
        Declared {
            id: "C-XLATE-RUST-FN-TO-LEAN-THM".into(),
            layer: 2,
            names: vec!["XlateRustFnToLeanThm".into()],
        },
    ];
    for body in [
        // contracts/lean/Bashrs.lean:596 — next clause is about Bashrs.
        "L5 CompileRustToPtxMma), PMAT-346 pushes Bashrs (Layer 2) from\n\
         depth-4 to depth-5.\n",
        // contracts/lean/XpileContractBackendTrait.lean:42 — a RANGE, and the
        // contract named is inside it.
        "C-XLATE-RUST-FN-TO-LEAN-THM) are Layer-1/Layer-2 contracts\n\
         with concrete equation domains.\n",
    ] {
        let hits: Vec<_> = runs_of(body)
            .iter()
            .flat_map(|r| attributions_in(r, &who))
            .collect();
        assert!(
            hits.iter().all(|a| a.written == a.declared),
            "false positive on a correct sentence: {hits:?}\n{body}"
        );
    }
}

#[test]
fn the_needle_requires_a_word_boundary_on_both_sides() {
    let who = vec![Declared {
        id: "C-NOTATION-LATEX-MATH-TO-EQUATION".into(),
        layer: 2,
        names: vec!["Notation".into()],
    }];
    // Head: `XNotation (Layer 5)` is not this contract.
    let head = "the XNotation (Layer 5) helper\n";
    assert!(
        runs_of(head)
            .iter()
            .flat_map(|r| attributions_in(r, &who))
            .next()
            .is_none(),
        "matched a name that is only a suffix of a longer word"
    );
    // Tail: `Notation L5x` is not a layer token.
    let tail = "Notation L5x is an identifier\n";
    assert!(
        runs_of(tail)
            .iter()
            .flat_map(|r| attributions_in(r, &who))
            .next()
            .is_none(),
        "matched a layer number that runs into the next word"
    );
    // Control that PASSES: the same shapes, boundaries intact, ARE seen.
    let live = "Notation L5 was written here\n";
    let hits: Vec<_> = runs_of(live)
        .iter()
        .flat_map(|r| attributions_in(r, &who))
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "the boundary guard rejected a real hit: {hits:?}"
    );
    assert_eq!(hits[0].written, 5);
}

#[test]
fn the_needle_cannot_see_a_layer_written_before_the_name() {
    // A DISCLOSED HOLE, pinned so nobody reads this gate as total coverage.
    //
    // The needle is directional: it reads a layer token that FOLLOWS a
    // contract name. The substrate also writes the other order — "extends
    // depth-2 coverage to a SECOND Layer-4 contract (Notation joins
    // FfiCpython …)" — and there the token is eleven characters and a noun
    // ahead of the name, which is past [`MAX_GAP`] by design (widening the gap
    // is what produced the false positives in
    // [`the_needle_does_not_reach_across_a_clause`]).
    //
    // Measured at 771f1652: 46 sites were in the name-then-layer shape and the
    // gate reports every one of them; THREE were in this shape
    // (notation-latex-math-to-equation-v1.yaml:568, Notation.lean:1088 and
    // :1089, all calling Notation "Layer-4") and PMAT-1460 repaired those by
    // hand. Deciding this shape needs a sentence-level subject, not a wider
    // gap — filed, not guessed at.
    let who = vec![Declared {
        id: "C-NOTATION-LATEX-MATH-TO-EQUATION".into(),
        layer: 2,
        names: vec!["NotationLatexMathToEquation".into(), "Notation".into()],
    }];
    let body = "    extends depth-2 coverage to a SECOND Layer-4 contract\n\
                \x20   (Notation joins FfiCpython as the second Layer-4 contract\n";
    let hits: Vec<_> = runs_of(body)
        .iter()
        .flat_map(|r| attributions_in(r, &who))
        .collect();
    assert!(
        hits.is_empty(),
        "this gate is now seeing the layer-then-name shape ({hits:?}). That is an \
         improvement, not a failure — delete this test, re-measure the corpus, and \
         say what the new needle costs in false positives."
    );
}

#[test]
fn the_gap_cap_is_load_bearing() {
    // Directly: at MAX_GAP the token is read, one past it the needle stops.
    let cs: Vec<char> = "X   (Layer 5)".chars().collect();
    assert_eq!(
        layer_token_after(&cs, 1),
        None,
        "4 filler chars is past the cap"
    );
    let cs: Vec<char> = "X  (Layer 5)".chars().collect();
    assert_eq!(
        layer_token_after(&cs, 1),
        Some(5),
        "3 filler chars is inside it"
    );
    let cs: Vec<char> = "X — Layer 2".chars().collect();
    assert_eq!(
        layer_token_after(&cs, 1),
        Some(2),
        "an em dash separator still reads"
    );
    assert_eq!(
        MAX_GAP, 3,
        "changing the cap changes what this gate can see — re-measure"
    );
}
