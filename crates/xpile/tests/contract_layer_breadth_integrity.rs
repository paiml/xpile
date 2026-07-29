//! XPILE-LAYERBREADTH-001 (PMAT-1463) — a Diamond broadening record's CENSUS
//! must equal the census the substrate's own records give it, as of that
//! record's own slice.
//!
//! PMAT-1462 gated the ORDINAL half of a Diamond broadening record ("the second
//! L2 contract at depth-10") and deliberately left the CARDINAL half alone, on
//! the reading that "a cardinal names no subject contract, so the rank
//! arithmetic does not apply". That is true and irrelevant: a cardinal does not
//! need a subject, it needs a POPULATION — and the population is the same
//! `(depth, as-of)` pair the ordinal gate already computes. The projections a
//! record actually publishes are four:
//!
//! | claim                                    | derived from the population at `(depth, as-of)` |
//! |------------------------------------------|--------------------------------------------------|
//! | `N contracts on M distinct layers`       | `|pop|` and `|{layers}|`                          |
//! | `depth-D across ALL M taxonomy layers`   | `|{layers}|`                                      |
//! | `N contracts at depth-D+`                | `|pop|`                                           |
//! | `N Layer-K contracts at depth-D`         | `|pop restricted to layer K|`                     |
//!
//! ## What was wrong
//!
//! At `e17697ae`, **51** sites under `contracts/` disagreed with that
//! arithmetic. The root cause is the one PMAT-1460/1461/1462 have been
//! unwinding for four slices: `C-BASHRS-POSIX-IDEMPOTENCE` declares
//! `layer: semantics` (Layer 1) and was counted as a Layer **2** contract.
//!
//! The damage here is not a rank — it is the substrate's HEADLINE milestone.
//! Five slices published **"COMPLETES DEPTH-D ACROSS ALL 5 TAXONOMY LAYERS"**
//! for D = 4, 5, 6, 7 and 8. Every one of them is false: at each of those
//! moments the population spanned **four** layers — L1 + L3 + L4 + L5 — and
//! Layer 2 was absent. The fifth layer was Bashrs, counted twice: once
//! correctly as Layer 1 and once again as the Layer 2 it is not.
//!
//! Two further shapes rode on the same mistake:
//!
//!   * **The pair whose true half launders its false half.** "BROADENS DEPTH-4
//!     ACROSS LAYERS to 4 contracts on 4 distinct layers" is *right about the
//!     contracts* — Bashrs really was the fourth contract at depth-4 — and
//!     wrong about the layers, which were three. A reader who checks the count
//!     gets YES and stops. Third run of the PMAT-1458 shape.
//!   * **The milestone was taken from the contract that earned it.** Depth-4
//!     reached all five layers at PMAT-338, depth-5 at PMAT-349, depth-6 at
//!     PMAT-360, depth-7 at PMAT-371, depth-8 at PMAT-382 — every one of them
//!     `C-XLATE-PY-LIST-TO-VEC`, a Layer-2 contract, two slices *after* the
//!     record that claimed it. That contract's own file records its arrival as
//!     the "first POST-UNIVERSAL broadening (post-PMAT-330 … milestone)":
//!     it credits PMAT-330 with the milestone it completed itself. Same victim
//!     PMAT-1462 found demoted in the ordinal class, in a second class.
//!
//! ## A correction that does not delete the claim leaves the claim
//!
//! `contracts/lean/XpileFrontendTrait.lean` carries a PMAT-1461 paragraph that
//! says, in as many words, *"this docstring claimed PMAT-358 completed depth-6
//! ACROSS ALL 5 TAXONOMY LAYERS. It did not."* — sitting **between** the
//! section header that makes the claim and the status line that repeats it, and
//! one docstring above a theorem doc that repeats it twice more, unqualified.
//! Prose annotated as false is not prose repaired. Every restatement was
//! rewritten here; the gate has no "this is a correction" exemption, because an
//! exemption is a laundering route for the next real claim.
//!
//! ## The dating question, MEASURED rather than decided
//!
//! PMAT-1461 filed the substrate-wide depth counts as an open class with "every
//! site UNDERSTATING", and PMAT-1462 recorded that "the dating question is REAL
//! there and must be decided first". Measuring it dissolves it. Against the
//! LIVE substrate 37 of the 45 `N contracts at depth-D+` records disagree —
//! which is what "every site understating" saw. Against the state at each
//! record's own `# PMAT-NNN`, **44 of 45 agree**: they are honest history, and
//! the class was ~98% a non-defect. The one that is not is
//! `xlate-lean-to-rust-v1.yaml` @PMAT-363, which said 8 where the substrate held
//! 10, plus one record whose count was elided to nothing at all. No policy call
//! was needed; the same construction PMAT-1462 used for ordinals answers it, and
//! [`the_as_of_rule_is_load_bearing`] pins that the two readings really do
//! differ so this is a measurement and not an assertion.
//!
//! ## Blind spots, each pinned by a control that PASSES
//!
//!   * SUBJECT — [`the_gate_reports_what_it_reaches`] prints the census per
//!     artifact kind and fails if either corpus goes empty.
//!   * GROUND TRUTH — [`the_reconstruction_matches_the_live_diamond_report`],
//!     cross-checked against `xpile diamond --json` rather than against another
//!     field of the file being parsed.
//!   * ANCHOR — [`the_lean_anchor_prefers_the_block_s_own_id`]. A first draft
//!     took the first owned `PMAT-` id anywhere in the block and mis-dated
//!     `FfiCpythonExt.lean`'s sign-decomposition docstring to PMAT-216 (a
//!     cross-reference two paragraphs up), reporting a TRUE claim as false.
//!     The rule now prefers the `## PMAT-NNN` section head, then the
//!     `discharged at … (PMAT-NNN)` status line, then `**PMAT-NNN`.
//!   * NEEDLE, false-positive direction —
//!     [`a_subset_claim_that_names_its_members_is_not_a_totality_claim`]. 45
//!     sites under `contracts/` say `ACROSS 3 LAYERS` with no `all`, every one
//!     of them enumerating the three it means (`PyIntArith (L1) +
//!     FFI-CPYTHON-EXT (L4) + CompileRustToPtxMma (L5)`). Those are honest
//!     subset claims and the totality needle requires the word `all` so it
//!     cannot reach them.
//!   * NEEDLE, layer-vs-count — [`a_layer_number_is_not_a_contract_count`]. The
//!     substrate-wide needle had to reject `3 L3 contracts at depth-7`, where a
//!     naive read takes the LAYER number as the count.
//!   * VOCABULARY — [`canonical_layer_numbering_is_read_from_the_taxonomy_doc`].
//!
//! Run against `e17697ae`'s `contracts/` this gate reports **51** sites: 20 in
//! `*.yaml` (of 80 claims read) and 31 in `lean/*.lean` (of 46, 0 unanchored).
//! On the repaired tree it reports 0.
//!
//! Out of subject, stated as a measurement rather than assumed: `contracts/`
//! holds no `.md` or `kani/*.rs` site matching any of the four needles (0 hits),
//! and `docs/` is not scanned — the CHANGELOG and roadmap ledgers must quote
//! the falsehoods they correct.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
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
    lean_file: Option<String>,
}

#[derive(Debug, Clone)]
struct Contract {
    id: String,
    layer: u32,
    yaml: PathBuf,
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
    let mut paths: Vec<PathBuf> = fs::read_dir(root.join("contracts"))
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
            let mut lf = None;
            for probe in lines.iter().skip(i + 1) {
                if is_top_level_key(probe) {
                    break;
                }
                if let Some(v) = quoted_after(probe, "lean_file:") {
                    lf = Some(v);
                }
            }
            entries.push(DiamondEntry {
                pmat,
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
// The arithmetic: the Diamond census at a depth, as of a slice.
// ---------------------------------------------------------------------------

/// Every contract that had reached `depth` by `as_of`.
fn population(all: &[Contract], depth: u32, as_of: u32) -> Vec<&Contract> {
    all.iter()
        .filter(|c| c.reached(depth).is_some_and(|p| p <= as_of))
        .collect()
}

/// `(contracts, distinct layers)` at `(depth, as_of)`.
fn census(all: &[Contract], depth: u32, as_of: u32) -> (u32, u32) {
    let pop = population(all, depth, as_of);
    let layers: BTreeSet<u32> = pop.iter().map(|c| c.layer).collect();
    (pop.len() as u32, layers.len() as u32)
}

/// The contracts of one layer at `(depth, as_of)`.
fn layer_census(all: &[Contract], depth: u32, as_of: u32, layer: u32) -> u32 {
    population(all, depth, as_of)
        .iter()
        .filter(|c| c.layer == layer)
        .count() as u32
}

/// The layers present at `(depth, as_of)`, for a report a human can act on.
fn layers_at(all: &[Contract], depth: u32, as_of: u32) -> Vec<u32> {
    let set: BTreeSet<u32> = population(all, depth, as_of)
        .iter()
        .map(|c| c.layer)
        .collect();
    set.into_iter().collect()
}

// ---------------------------------------------------------------------------
// The needles.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// `N contracts on M distinct layers` — both halves derived.
    Pair { contracts: u32, layers: u32 },
    /// `across ALL M taxonomy layers` — a totality claim over the taxonomy.
    Totality { layers: u32 },
    /// `N contracts at depth-D+` — the substrate-wide count.
    Census { contracts: u32 },
    /// `N Layer-K contracts at depth-D` — the per-layer count.
    LayerCensus { contracts: u32, layer: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Claim {
    kind: Kind,
    depth: u32,
    /// A clause that dates ITSELF — `After PMAT-349 … (substrate at 7 contracts
    /// at depth-5+), PMAT-350 pushes …` — is a claim about the state at that
    /// id, not at the block's. Judging it against the block would report an
    /// honest record as false.
    redated: Option<u32>,
    text: String,
}

/// Collapse a wrapped comment/docstring run into one line.
fn flatten(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    for (i, line) in body.lines().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let t = line.trim_start();
        let t = t
            .strip_prefix('#')
            .or_else(|| t.strip_prefix("--"))
            .unwrap_or(t);
        out.push_str(t.trim());
    }
    out
}

const SMALL_WORDS: [(&str, u32); 10] = [
    ("one", 1),
    ("two", 2),
    ("three", 3),
    ("four", 4),
    ("five", 5),
    ("six", 6),
    ("seven", 7),
    ("eight", 8),
    ("nine", 9),
    ("ten", 10),
];

/// A cardinal at the head of `s`: `12` or `twelve`. Returns `(value, len)`.
fn cardinal(s: &str) -> Option<(u32, usize)> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        return digits.parse().ok().map(|v| (v, digits.len()));
    }
    SMALL_WORDS
        .iter()
        .find(|(w, _)| s.starts_with(w))
        .map(|(w, v)| (*v, w.len()))
}

/// `contract` / `contracts`, at the head of `s`. Returns the consumed length.
fn contract_noun(s: &str) -> Option<usize> {
    let rest = s.strip_prefix("contract")?;
    Some(if rest.starts_with('s') { 9 } else { 8 })
}

/// `layer` / `layers`, allowing `distinct ` / `taxonomy ` qualifiers first.
fn layer_noun(s: &str) -> Option<usize> {
    let mut used = 0;
    let mut rest = s;
    for _ in 0..2 {
        for q in ["distinct ", "taxonomy ", "of the ", "of its "] {
            if let Some(r) = rest.strip_prefix(q) {
                used += q.len();
                rest = r;
            }
        }
    }
    let r = rest.strip_prefix("layer")?;
    used += 5;
    if r.starts_with('s') {
        used += 1;
    }
    Some(used)
}

/// The depth token nearest to `at` in `hay`, in either direction.
fn nearest_depth(hay: &str, at: usize) -> Option<u32> {
    let mut best: Option<(usize, u32)> = None;
    let mut from = 0;
    while let Some(rel) = hay[from..].find("depth-") {
        let i = from + rel;
        from = i + 6;
        let digits: String = hay[i + 6..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let Ok(d) = digits.parse::<u32>() else {
            continue;
        };
        let dist = at.abs_diff(i);
        if best.is_none_or(|(b, _)| dist < b) {
            best = Some((dist, d));
        }
    }
    best.map(|(_, d)| d)
}

/// Every census claim in `hay` (already lowercased for matching by the caller).
///
/// Four shapes, all anchored on the POPULATION NOUN rather than on a
/// quantifier — PMAT-1458's rule, which buys immunity to `all 5 layers` style
/// non-claims for free.
fn claims(hay: &str) -> Vec<Claim> {
    /// A claim's number and its noun may be separated by qualifiers, never by a
    /// clause.
    const GAP: usize = 24;
    let lower = hay.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut out: Vec<Claim> = Vec::new();
    let push = |c: Claim, out: &mut Vec<Claim>| {
        if !out.contains(&c) {
            out.push(c);
        }
    };

    // --- shapes anchored on `<N> contract(s)` --------------------------------
    let mut i = 0;
    while i < lower.len() {
        if !lower.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let Some((n, nlen)) = cardinal(&lower[i..]) else {
            i += 1;
            continue;
        };
        // A cardinal must start a word. `L3` and `Layer-3` fall out here.
        if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'-') {
            i += 1;
            continue;
        }
        // A LAYER number is not a count. `adding a SECOND Layer 3 contract at
        // depth-3` and `(3 Layer 3 contracts at depth-5)` both put a layer
        // number exactly where a substrate-wide count sits; without this the
        // first reads as "3 contracts at depth-3" (an ordinal claim PMAT-1462
        // already owns) and the second double-counts its own parenthetical.
        if lower[..i]
            .trim_end_matches(' ')
            .to_ascii_lowercase()
            .ends_with("layer")
        {
            i += 1;
            continue;
        }
        let after_num = i + nlen;
        let tail = lower[after_num..].trim_start();
        let space = lower[after_num..].len() - tail.len();
        if space == 0 {
            i = after_num;
            continue;
        }
        // `<N> (Layer K|LK) contracts at depth-D`  — the per-layer census.
        let layered = layer_prefix(tail);
        let (subject_layer, used) = match layered {
            Some((k, u)) => (Some(k), u),
            None => (None, 0),
        };
        let rest = &tail[used..];
        let Some(clen) = contract_noun(rest) else {
            i = after_num;
            continue;
        };
        let after_noun = &rest[clen..];
        let noun_at = after_num + space + used;

        // … `on M distinct layers`
        if let Some(m) = after_noun.strip_prefix(" on ").and_then(|r| {
            let r2 = r.strip_prefix("all ").unwrap_or(r);
            let (v, vl) = cardinal(r2)?;
            let t = r2[vl..].trim_start();
            layer_noun(t).map(|_| v)
        }) {
            if subject_layer.is_none() {
                if let Some(depth) = nearest_depth(&lower, noun_at) {
                    push(
                        Claim {
                            kind: Kind::Pair {
                                contracts: n,
                                layers: m,
                            },
                            depth,
                            redated: redating(&lower, i),
                            text: snippet(hay, i, 72),
                        },
                        &mut out,
                    );
                }
                i = after_num;
                continue;
            }
        }

        // … `at depth-D`
        if let Some(rel) = after_noun.find("at depth-") {
            if rel <= GAP {
                let digits: String = after_noun[rel + 9..]
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                // `one contract at depth-5 PER LAYER` distributes the
                // population over the taxonomy; it is not a census of it.
                let tail_after = &after_noun[rel + 9 + digits.len()..];
                let distributed = tail_after.trim_start().starts_with("per ");
                if let (Ok(depth), false) = (digits.parse::<u32>(), distributed) {
                    let kind = match subject_layer.or_else(|| layer_subject_before(&lower, i)) {
                        Some(k) => Kind::LayerCensus {
                            contracts: n,
                            layer: k,
                        },
                        None => Kind::Census { contracts: n },
                    };
                    push(
                        Claim {
                            kind,
                            depth,
                            redated: redating(&lower, i),
                            text: snippet(hay, i, 72),
                        },
                        &mut out,
                    );
                }
            }
        }
        i = after_num;
    }

    // --- the TOTALITY shape: `across ALL M taxonomy layers` ------------------
    // The word `all` is required. Without it the phrase has a live subset
    // reading in this corpus (45 sites say `ACROSS 3 LAYERS` and every one
    // enumerates the three it means), and a needle loose enough to reach those
    // would fabricate findings.
    for lead in ["across all ", "on all ", "spanning all ", "over all "] {
        let mut from = 0;
        while let Some(rel) = lower[from..].find(lead) {
            let start = from + rel;
            from = start + lead.len();
            let r = &lower[from..];
            let Some((m, mlen)) = cardinal(r) else {
                continue;
            };
            let t = r[mlen..].trim_start();
            if layer_noun(t).is_none() {
                continue;
            }
            let Some(depth) = nearest_depth(&lower, start) else {
                continue;
            };
            push(
                Claim {
                    kind: Kind::Totality { layers: m },
                    depth,
                    redated: redating(&lower, start),
                    text: snippet(hay, start, 72),
                },
                &mut out,
            );
        }
    }
    out
}

/// The slice a clause dates ITSELF to: the nearest `after PMAT-NNN` earlier in
/// the same sentence. `contracts/lean/Notation.lean` writes *"After PMAT-349
/// brought XlatePyListToVec (Layer 2) to depth-5 (substrate at 7 contracts at
/// depth-5+), PMAT-350 pushes …"* — the 7 is the state at PMAT-349 and is
/// correct there; against the block's own PMAT-350 it reads as off by one.
fn redating(lower: &str, at: usize) -> Option<u32> {
    const WINDOW: usize = 240;
    let floor = at.saturating_sub(WINDOW);
    let floor = (floor..=at).find(|i| lower.is_char_boundary(*i))?;
    let back = &lower[floor..at];
    // A sentence terminator ends the clause; anything before it is not ours.
    let start = back
        .rfind(". ")
        .map(|i| i + 2)
        .max(back.rfind(".** ").map(|i| i + 4))
        .unwrap_or(0);
    let clause = &back[start..];
    // `after PMAT-330 …` and `post-PMAT-330 …` are the same re-dating. The
    // second spelling is how the contract that ACTUALLY completed each
    // milestone filed its own arrival — "first POST-UNIVERSAL broadening
    // (post-PMAT-330 depth-4 ACROSS ALL 5 LAYERS milestone)" — so leaving it
    // out would have let the clearest evidence of the mis-credit pass.
    let i = clause
        .rfind("after pmat-")
        .into_iter()
        .chain(clause.rfind("post-pmat-"))
        .max()?;
    // `after PMAT-228 L1, PMAT-229 L2, PMAT-230 L4, PMAT-231 L5, PMAT-232 L3 —
    // that completed UNIVERSAL depth-2 across all 5 layers` dates itself to the
    // LAST arrival in the enumeration, not the first. Reading the first is the
    // same one-directional mistake PMAT-1461 found in the label needle, and it
    // reported that TRUE sentence as false.
    let tail = clause[i..].to_ascii_uppercase();
    let mut best = None;
    let mut from = 0;
    while let Some(rel) = tail[from..].find("PMAT-") {
        let at = from + rel;
        from = at + 5;
        if let Some(v) = pmat_id(&tail[at..]) {
            best = Some(best.map_or(v, |b: u32| b.max(v)));
        }
    }
    best
}

/// A layer subject written BEFORE the count: `Layer 3 now has 4 contracts at
/// depth-4`. PMAT-1461 disclosed the layer-before-the-name order as a blind
/// spot for a LABEL needle; for a CENSUS needle it is not silence, it is a
/// wrong answer — the sentence above is a TRUE Layer-3 census that reads as a
/// false substrate-wide one.
fn layer_subject_before(lower: &str, at: usize) -> Option<u32> {
    let back = &lower[..at];
    let back = back.trim_end();
    let back = [
        "now has",
        "has",
        "now holds",
        "holds",
        "now carries",
        "carries",
    ]
    .iter()
    .find_map(|v| back.strip_suffix(v))?
    .trim_end();
    let k = back.chars().last()?.to_digit(10)?;
    if !(1..=5).contains(&k) {
        return None;
    }
    let head = back[..back.len() - 1].trim_end();
    (head.ends_with("layer") || head.ends_with('l')).then_some(k)
}

/// `layer 3 ` / `layer-3 ` / `l3 ` at the head of `s`, as `(layer, consumed)`.
fn layer_prefix(s: &str) -> Option<(u32, usize)> {
    if let Some(r) = s
        .strip_prefix("layer ")
        .or_else(|| s.strip_prefix("layer-"))
    {
        let k = r.chars().next()?.to_digit(10)?;
        if (1..=5).contains(&k) && r[1..].starts_with(' ') {
            return Some((k, 8));
        }
        return None;
    }
    let r = s.strip_prefix('l')?;
    let k = r.chars().next()?.to_digit(10)?;
    if (1..=5).contains(&k) && r[1..].starts_with(' ') {
        return Some((k, 3));
    }
    None
}

fn snippet(hay: &str, at: usize, len: usize) -> String {
    let start = (0..=at)
        .rev()
        .find(|i| hay.is_char_boundary(*i))
        .unwrap_or(0);
    let end = (start + len..=hay.len())
        .find(|i| hay.is_char_boundary(*i))
        .unwrap_or(hay.len());
    hay[start..end].to_string()
}

// ---------------------------------------------------------------------------
// Findings.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Finding {
    file: String,
    as_of: u32,
    claim: Claim,
    truth: String,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} [@PMAT-{} depth-{}] {:?} — derived truth: {}\n      {:?}",
            self.file, self.as_of, self.claim.depth, self.claim.kind, self.truth, self.claim.text
        )
    }
}

/// `None` when the claim agrees with the substrate; otherwise the truth.
fn judge(all: &[Contract], claim: &Claim, as_of: u32) -> Option<String> {
    let (n, l) = census(all, claim.depth, as_of);
    let spread = layers_at(all, claim.depth, as_of);
    match claim.kind {
        Kind::Pair { contracts, layers } => (contracts != n || layers != l)
            .then(|| format!("{n} contract(s) on {l} layer(s) {spread:?}")),
        Kind::Totality { layers } => {
            (layers != l).then(|| format!("{l} layer(s) {spread:?}, over {n} contract(s)"))
        }
        Kind::Census { contracts } => (contracts != n).then(|| format!("{n} contract(s)")),
        Kind::LayerCensus { contracts, layer } => {
            let t = layer_census(all, claim.depth, as_of, layer);
            (contracts != t).then(|| format!("{t} Layer-{layer} contract(s)"))
        }
    }
}

/// Every census claim in a `contracts/*.yaml` Diamond block, block-scoped to the
/// `# PMAT-NNN` that introduced it.
fn yaml_findings(all: &[Contract]) -> (usize, Vec<Finding>) {
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
            let hay = flatten(&window.join("\n"));
            for claim in claims(&hay) {
                if !seen.insert((
                    format!("{:?}{}{:?}", claim.kind, claim.depth, claim.redated),
                    p,
                )) {
                    continue;
                }
                checked += 1;
                let dated = claim.redated.unwrap_or(p);
                if let Some(truth) = judge(all, &claim, dated) {
                    bad.push(Finding {
                        file: format!("{}:{}", rel(&c.yaml), i + 1),
                        as_of: dated,
                        claim,
                        truth,
                    });
                }
            }
        }
    }
    (checked, bad)
}

/// `/-- … -/` and `/-! … -/` blocks, with their byte offsets.
fn docblocks(body: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while from < body.len() {
        let Some(rel) = body[from..]
            .find("/--")
            .into_iter()
            .chain(body[from..].find("/-!"))
            .min()
        else {
            break;
        };
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

/// The block's OWN slice id. Preference order matters: a block routinely cites
/// other slices, and the first owned id in reading order is not reliably its
/// own — that draft mis-dated a true claim by two hundred slices.
fn block_anchor(hay: &str, owned: &BTreeSet<u32>) -> Option<u32> {
    let heads = ["## pmat-", "discharged at", "**pmat-"];
    let lower = hay.to_ascii_lowercase();
    for head in heads {
        let mut from = 0;
        while let Some(rel) = lower[from..].find(head) {
            let at = from + rel;
            from = at + head.len();
            let id = if head == "discharged at" {
                lower[at..]
                    .split_once('(')
                    .and_then(|(_, r)| pmat_id(&r.to_ascii_uppercase()))
            } else {
                pmat_id(&lower[at..].to_ascii_uppercase())
            };
            if let Some(id) = id.filter(|i| owned.contains(i)) {
                return Some(id);
            }
        }
    }
    None
}

fn lean_findings(all: &[Contract]) -> (usize, Vec<Finding>, Vec<String>) {
    let root = workspace_root();
    let mut checked = 0;
    let mut bad = Vec::new();
    let mut unanchored = Vec::new();
    for c in all {
        let files: BTreeSet<String> = c
            .entries
            .iter()
            .filter_map(|e| e.lean_file.clone())
            .collect();
        let owned: BTreeSet<u32> = c.entries.iter().map(|e| e.pmat).collect();
        for f in files {
            let path = root.join(&f);
            let body = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {f}: {e}"));
            for (start, block) in docblocks(&body) {
                let hay = flatten(block);
                let found = claims(&hay);
                if found.is_empty() {
                    continue;
                }
                let line = body[..start].lines().count();
                let Some(id) = block_anchor(&hay, &owned) else {
                    unanchored.push(format!("{f}:{line} {:?}", found[0].text));
                    continue;
                };
                for claim in found {
                    checked += 1;
                    let dated = claim.redated.unwrap_or(id);
                    if let Some(truth) = judge(all, &claim, dated) {
                        bad.push(Finding {
                            file: format!("{f}:{line}"),
                            as_of: dated,
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

fn rel(p: &Path) -> String {
    p.strip_prefix(workspace_root())
        .unwrap_or(p)
        .display()
        .to_string()
}

fn report(bad: &[Finding]) -> String {
    let mut s = String::new();
    for f in bad {
        let _ = writeln!(s, "  {f}");
    }
    s
}

// ---------------------------------------------------------------------------
// The invariants.
// ---------------------------------------------------------------------------

#[test]
fn every_diamond_census_claim_in_a_contract_yaml_matches_the_substrate() {
    let all = parse_contracts();
    let (checked, bad) = yaml_findings(&all);
    assert!(
        checked >= 55,
        "the needle found only {checked} census claims under contracts/*.yaml; \
         it read 68 when written, so a drop this large means the needle stopped \
         matching, not that the substrate stopped claiming"
    );
    assert!(
        bad.is_empty(),
        "{} Diamond census claim(s) in contracts/*.yaml disagree with the census \
         derived from the substrate's own layer tags and Diamond arrival order, \
         as of each record's own slice ({checked} checked):\n{}",
        bad.len(),
        report(&bad)
    );
}

#[test]
fn every_diamond_census_claim_in_a_lean_docstring_matches_the_substrate() {
    let all = parse_contracts();
    let (checked, bad, unanchored) = lean_findings(&all);
    assert!(
        checked >= 32,
        "the needle found only {checked} census claims in contract Lean \
         docstrings; it read 39 when written"
    );
    assert!(
        bad.is_empty(),
        "{} Diamond census claim(s) in contracts/lean/*.lean disagree with the \
         derived census ({checked} checked, {} unanchored):\n{}",
        bad.len(),
        unanchored.len(),
        report(&bad)
    );
}

/// GROUND TRUTH. Every census here is computed from the parsed Diamond entries;
/// a parser that skipped one would shift the arrival dates and the gate would
/// stay green while lying. Cross-checked against `xpile diamond --json` — the
/// shipped binary reaching the same entries through its own parser — rather
/// than against another field of the file being parsed.
#[test]
fn the_reconstruction_matches_the_live_diamond_report() {
    let all = parse_contracts();
    let json = run_diamond_json();
    let mut total = 0usize;
    for c in &all {
        let live = live_diamond_count(&json, &c.id)
            .unwrap_or_else(|| panic!("`xpile diamond --json` does not report {}", c.id));
        assert_eq!(
            c.entries.len() as u64,
            live,
            "{}: this gate parsed {} `*_diamond:` entries, `xpile diamond` counts \
             {live}. Every census checked here rests on that parse.",
            c.id,
            c.entries.len()
        );
        total += c.entries.len();
    }
    assert!(
        total >= 200,
        "only {total} Diamond entries parsed across {} contracts; the substrate \
         held 207 when this gate was written",
        all.len()
    );
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

fn live_diamond_count(json: &str, id: &str) -> Option<u64> {
    let needle = format!("\"id\":\"{id}\",\"diamond_count\":");
    let at = json.find(&needle)? + needle.len();
    let digits: String = json[at..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// DATING, measured. The whole class turns on reading each record against the
/// state at its own slice rather than against the live substrate. That choice
/// is only interesting if the two readings differ — proven here rather than
/// argued, so the "as of" rule cannot quietly become a no-op.
#[test]
fn the_as_of_rule_is_load_bearing() {
    let all = parse_contracts();
    let now = all
        .iter()
        .flat_map(|c| c.entries.iter().map(|e| e.pmat))
        .max()
        .expect("some entry");
    let (checked, bad) = yaml_findings(&all);
    assert!(bad.is_empty(), "the as-of reading must be clean first");

    // Re-judge every yaml claim against the LIVE substrate instead.
    let mut live_bad = 0usize;
    for c in &all {
        let body = fs::read_to_string(&c.yaml).expect("read contract");
        let lines: Vec<&str> = body.lines().collect();
        let mut as_of: Option<u32> = None;
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for (i, l) in lines.iter().enumerate() {
            if is_top_level_key(l) {
                as_of = lines
                    .iter()
                    .skip(i + 1)
                    .take(3)
                    .find_map(|p| pmat_id(p))
                    .filter(|_| is_diamond_key(l));
            }
            if as_of.is_none() {
                continue;
            }
            let window: Vec<&str> = lines.iter().skip(i).take(3).copied().collect();
            for claim in claims(&flatten(&window.join("\n"))) {
                if seen.insert(format!("{:?}{}", claim.kind, claim.depth))
                    && judge(all.as_slice(), &claim, now).is_some()
                {
                    live_bad += 1;
                }
            }
        }
    }
    assert!(
        live_bad > 20,
        "reading these {checked} records against the LIVE substrate now flags \
         only {live_bad} of them. The two readings have converged, so \"as of \
         the record's own slice\" is no longer the thing keeping this gate \
         honest — re-derive the doctrine before trusting the header."
    );
    eprintln!(
        "XPILE-LAYERBREADTH-001 dating: {checked} yaml census claims. As of each \
         record's own slice: 0 disagree. Against the live substrate (PMAT-{now}): \
         {live_bad} disagree. The second number is what PMAT-1461 saw when it \
         filed this class as \"every site UNDERSTATING\"."
    );
}

/// SUBJECT. The gate prints what it reaches.
#[test]
fn the_gate_reports_what_it_reaches() {
    let all = parse_contracts();
    let (yaml_n, _) = yaml_findings(&all);
    let (lean_n, _, unanchored) = lean_findings(&all);
    let tagged = all.iter().filter(|c| (1..=5).contains(&c.layer)).count();
    eprintln!(
        "XPILE-LAYERBREADTH-001 reach: {tagged}/{} contracts carry a layer tag; \
         {yaml_n} census claims under contracts/*.yaml and {lean_n} in contract \
         Lean docstrings are judged; {} Lean block(s) name no PMAT id of their \
         own contract and are NOT judged: {:?}",
        all.len(),
        unanchored.len(),
        unanchored
    );
    assert_eq!(tagged, all.len(), "a contract lost its layer tag");
    assert!(yaml_n > 0 && lean_n > 0, "a corpus went unscanned");
}

/// NEEDLE, false-positive direction. `contracts/` carries 45 sites of the form
/// `DEPTH-6 ACROSS 3 LAYERS opened: PyIntArith (L1) + FFI-CPYTHON-EXT (L4) +
/// CompileRustToPtxMma (L5)`. Those name the three layers they mean; at
/// PMAT-368 the substrate already had depth-6 on five. They are honest subset
/// claims and must never be reported. Requiring the word `all` is what keeps
/// them out — this control fails if that requirement is relaxed.
#[test]
fn a_subset_claim_that_names_its_members_is_not_a_totality_claim() {
    let subset = "Diamond depth-6 ACROSS 3 LAYERS opened: PyIntArith (L1) + \
                  FFI-CPYTHON-EXT (L4) + CompileRustToPtxMma (L5)";
    assert!(
        claims(subset).is_empty(),
        "the needle read a subset claim as a totality claim: {:?}",
        claims(subset)
    );
    let totality = "SIXTH Diamond category — COMPLETES DEPTH-6 ACROSS ALL 5 TAXONOMY LAYERS";
    let got = claims(totality);
    assert_eq!(
        got.len(),
        1,
        "the needle missed the shipped defect shape: {got:?}"
    );
    assert_eq!(got[0].kind, Kind::Totality { layers: 5 });
    assert_eq!(got[0].depth, 6);

    // And the shape really is live in the corpus, so the exclusion is not free.
    let root = workspace_root();
    let mut subset_sites = 0usize;
    for p in fs::read_dir(root.join("contracts")).expect("contracts/") {
        let p = p.expect("entry").path();
        if p.extension().is_none_or(|x| x != "yaml") {
            continue;
        }
        let body = fs::read_to_string(&p).expect("read");
        subset_sites += body
            .lines()
            .filter(|l| {
                let u = l.to_ascii_uppercase();
                u.contains("ACROSS 3 LAYERS") && !u.contains("ALL")
            })
            .count();
    }
    assert!(
        subset_sites >= 10,
        "only {subset_sites} subset-phrased sites remain; this carve-out was \
         measured against 32 in contracts/*.yaml and is no longer load-bearing"
    );
    eprintln!(
        "XPILE-LAYERBREADTH-001 subset carve-out: {subset_sites} `ACROSS N \
         LAYERS` site(s) in contracts/*.yaml are excluded as subset claims."
    );
}

/// NEEDLE. A layer number sitting where a count would sit is the corpus's own
/// non-claim: `11 contracts at depth-5+ (3 Layer 3 contracts at depth-5)` holds
/// a substrate count of 11 and a Layer-3 count of 3, and a naive read takes the
/// `3` for a second substrate count.
#[test]
fn a_layer_number_is_not_a_contract_count() {
    let both =
        "Diamond depth-5 broadened: 11 contracts at depth-5+ (3 Layer 3 contracts at depth-5)";
    let got = claims(both);
    assert!(
        got.contains(&Claim {
            kind: Kind::Census { contracts: 11 },
            depth: 5,
            redated: None,
            text: got
                .iter()
                .find(|c| matches!(c.kind, Kind::Census { .. }))
                .map(|c| c.text.clone())
                .unwrap_or_default(),
        }),
        "the substrate-wide count was not read: {got:?}"
    );
    assert!(
        got.iter().any(|c| c.kind
            == Kind::LayerCensus {
                contracts: 3,
                layer: 3
            }),
        "the per-layer count was not read: {got:?}"
    );
    assert_eq!(
        got.iter()
            .filter(|c| matches!(c.kind, Kind::Census { .. }))
            .count(),
        1,
        "a layer number was read as a second substrate-wide count: {got:?}"
    );

    // The shipped defect: the same phrase with the wrong per-layer number.
    let all = parse_contracts();
    assert_eq!(
        layer_census(&all, 5, 349, 2),
        1,
        "at PMAT-349 exactly one Layer-2 contract had reached depth-5 \
         (C-XLATE-PY-LIST-TO-VEC); the site said two, counting Bashrs"
    );
    assert_eq!(
        layer_census(&all, 5, 349, 1),
        2,
        "and the two Layer-1 contracts at depth-5 are where the phantom came from"
    );
}

/// The five milestone slices claimed a taxonomy-complete depth two slices
/// early, and the contract that actually completed each one is the same one
/// PMAT-1462 found demoted in the ordinal class.
#[test]
fn the_five_taxonomy_milestones_were_completed_by_a_layer_two_contract() {
    let all = parse_contracts();
    let claimed_at = [(4u32, 330u32), (5, 347), (6, 358), (7, 369), (8, 380)];
    let mut roster = Vec::new();
    for (depth, claimed) in claimed_at {
        let (_, layers_then) = census(&all, depth, claimed);
        assert_eq!(
            layers_then,
            4,
            "depth-{depth} at PMAT-{claimed} spanned {layers_then} layers, not \
             four: {:?}",
            layers_at(&all, depth, claimed)
        );
        // The first slice at which this depth actually spans all five.
        let all_ids: BTreeSet<u32> = all
            .iter()
            .filter_map(|c| c.reached(depth))
            .filter(|p| *p > 0)
            .collect();
        let completed = all_ids
            .into_iter()
            .find(|p| census(&all, depth, *p).1 == 5)
            .unwrap_or_else(|| panic!("depth-{depth} never spans all five layers"));
        assert!(
            completed > claimed,
            "depth-{depth} spanned all five layers at PMAT-{completed}, which is \
             not after the PMAT-{claimed} record that claimed it"
        );
        let who: Vec<&str> = all
            .iter()
            .filter(|c| c.reached(depth) == Some(completed))
            .map(|c| c.id.as_str())
            .collect();
        assert!(
            who.contains(&"C-XLATE-PY-LIST-TO-VEC"),
            "depth-{depth} was completed at PMAT-{completed} by {who:?}"
        );
        roster.push(format!(
            "depth-{depth}: claimed at PMAT-{claimed}, completed at PMAT-{completed}"
        ));
    }
    eprintln!("XPILE-LAYERBREADTH-001 milestones: {}", roster.join("; "));
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
            "{TAXONOMY_DOC} no longer numbers `{w}` as Layer {n}; every census in \
             this gate is keyed on that table ({words:?})"
        );
    }
    assert_eq!(
        words.len(),
        canonical_layer_words().len(),
        "the taxonomy read is not deterministic"
    );
}

/// ANCHOR. A Lean block routinely cites other slices; taking the first owned
/// id in reading order dated `FfiCpythonExt.lean`'s sign-decomposition
/// docstring — whose status line says PMAT-328 — to PMAT-216, a cross-reference
/// two paragraphs above, and reported its TRUE census claim as false.
#[test]
fn the_lean_anchor_prefers_the_block_s_own_id() {
    let owned: BTreeSet<u32> = [216, 288, 328].into_iter().collect();
    let cross = "behavior (PMAT-216) or its inverse-existence (PMAT-288). \
                 Status: discharged at v0.1.0 (PMAT-328). Tier: DIAMOND. \
                 Broadens DEPTH-5 ACROSS LAYERS to 3 contracts on 3 layers.";
    assert_eq!(
        block_anchor(cross, &owned),
        Some(328),
        "the anchor took a cross-reference for the block's own id"
    );
    let head = "## PMAT-348 — FIFTH Diamond on C-XPILE-BACKEND-TRAIT. After \
                PMAT-347 achieved depth-5, the substrate had six contracts.";
    assert_eq!(
        block_anchor(head, &[347, 348].into_iter().collect()),
        Some(348),
        "a `## PMAT-NNN` section head must win over a later citation"
    );
    assert_eq!(
        block_anchor("no ids here at all", &owned),
        None,
        "an unanchored block must be reported, not judged"
    );

    // And the mis-dating really would have fabricated a finding.
    let all = parse_contracts();
    assert_eq!(
        census(&all, 5, 328),
        (3, 3),
        "the FfiCpythonExt claim is TRUE at its own slice"
    );
    assert_eq!(
        census(&all, 5, 216),
        (0, 0),
        "and false at the id the broken anchor picked — which is the false \
         accusation this rule exists to prevent"
    );
}
