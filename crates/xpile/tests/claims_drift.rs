//! Derived-counts claims-drift gate (XPILE-CLAIMS-001).
//!
//! Sibling of `lean_pilot_roots.rs`. Where that test pins the Lean pilot
//! COUNT (lakefile roots ⇔ the PROVABILITY-INVENTORY.md PILOT header), this
//! one guards the broader class of DERIVED-COUNT doc claims the architectural
//! review (`docs/specifications/fable-architectural-review.md`, F8) found
//! drifting because nothing sampled them:
//!
//!  (a) README's "five source languages" / "nine backends" are DERIVED —
//!      the frontend registry (`crates/xpile-core/src/lib.rs`,
//!      `register_frontend` calls) and the `Target` enum
//!      (`crates/xpile-backend/src/lib.rs`) are the source of truth, not a
//!      hand-typed numeral.
//!  (b) EVERY module-count literal in `contracts/lean/PROVABILITY-INVENTORY.md`
//!      (the line-11 `lake build` reproduce block INCLUDED) equals the
//!      lakefile root count.
//!  (c) the roadmap's "N machine-checked modules" equals that same count.
//!  (d) a roadmap-consistency lint: a PMAT id cited in a `strategic_goals`
//!      clause that shouts "COMPLETE" must not still be `status: planned`
//!      in the same ledger (the pillar-D "Wasm lift COMPLETE" vs PMAT-952
//!      `status: planned` case).
//!
//! Like `lean_pilot_roots.rs` this is pure `std::fs` text parsing — no new
//! dependency, no runtime linkage — so the required `gate` job compiles it
//! (`clippy --all-targets`) and `workspace-test` runs it (`cargo test
//! --workspace`), with zero extra CI wiring.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let path = workspace_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// English cardinal for small counts — the form the README prose uses.
const NUMBER_WORDS: &[&str] = &[
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen",
];

fn word(n: usize) -> &'static str {
    assert!(
        n < NUMBER_WORDS.len(),
        "no spelled-out word for {n}; extend NUMBER_WORDS"
    );
    NUMBER_WORDS[n]
}

/// Count of `lakefile.lean` `roots := #[ … ]` entries — the authoritative
/// pilot size. Mirrors `lean_pilot_roots.rs::lakefile_roots` (count only).
fn lakefile_root_count() -> usize {
    let src = read("contracts/lean/lakefile.lean");
    let mut in_roots = false;
    let mut n = 0;
    for line in src.lines() {
        let t = line.trim_start();
        if t.starts_with("roots :=") {
            in_roots = true;
            continue;
        }
        if !in_roots {
            continue;
        }
        if t.starts_with(']') {
            break;
        }
        if let Some(rest) = t.strip_prefix('`') {
            if rest.starts_with(|c: char| c.is_ascii_alphabetic()) {
                n += 1;
            }
        }
    }
    n
}

/// Every integer `N` such that `{prefix}{N}{suffix}` occurs in `text`.
/// Anchored substring parsing (no `regex` dep), the same style as the
/// lakefile parser.
fn counts_between(text: &str, prefix: &str, suffix: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find(prefix) {
        let num_start = from + rel + prefix.len();
        let digits: String = text[num_start..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        from = num_start; // prefix is non-empty ⇒ strictly advances
        if digits.is_empty() {
            continue;
        }
        let after = num_start + digits.len();
        if text[after..].starts_with(suffix) {
            if let Ok(n) = digits.parse::<usize>() {
                out.push(n);
            }
        }
    }
    out
}

// ── (a) README counts are DERIVED from code ──────────────────────────

/// Registered code-lane frontends = `register_frontend(Arc::new(` call sites
/// in `xpile-core::default_session`. (The `pub fn register_frontend(&mut …`
/// definition does not contain `(Arc::new(`, so it is not counted.)
fn registered_frontend_count() -> usize {
    read("crates/xpile-core/src/lib.rs")
        .matches("register_frontend(Arc::new(")
        .count()
}

/// Variants of the `Target` enum in `xpile-backend`.
fn target_variant_count() -> usize {
    let src = read("crates/xpile-backend/src/lib.rs");
    let mut in_enum = false;
    let mut n = 0;
    for line in src.lines() {
        let t = line.trim_start();
        if !in_enum {
            if t.starts_with("pub enum Target {") {
                in_enum = true;
            }
            continue;
        }
        if t == "}" {
            break;
        }
        // A unit-variant line starts with an uppercase ASCII ident char;
        // doc-comments (`///`) and attributes (`#[…]`) never do.
        if matches!(t.chars().next(), Some(c) if c.is_ascii_uppercase()) {
            n += 1;
        }
    }
    n
}

#[test]
fn readme_source_language_count_is_derived_from_frontend_registry() {
    let frontends = registered_frontend_count();
    assert!(
        frontends >= 1,
        "parsed 0 frontends from crates/xpile-core/src/lib.rs — the \
         `register_frontend(Arc::new(` anchor moved; update this gate"
    );
    let readme = read("README.md");
    let needle = format!("{} source languages", word(frontends));
    assert!(
        readme.contains(&needle),
        "README.md must say '{needle}': {frontends} frontends are registered \
         in crates/xpile-core/src/lib.rs. Update the README numeral to match \
         the registry, or the registry to match the claim."
    );
}

#[test]
fn readme_backend_count_is_derived_from_target_enum() {
    let backends = target_variant_count();
    assert!(
        backends >= 1,
        "parsed 0 Target variants from crates/xpile-backend/src/lib.rs — the \
         `pub enum Target {{` anchor moved; update this gate"
    );
    let readme = read("README.md");
    let needle = format!("{} backends", word(backends));
    assert!(
        readme.contains(&needle),
        "README.md must say '{needle}': the Target enum in \
         crates/xpile-backend/src/lib.rs has {backends} variants. Update the \
         README numeral to match the enum, or the enum to match the claim."
    );
}

// ── (b) every PROVABILITY-INVENTORY module count == lakefile roots ───

#[test]
fn provability_inventory_module_counts_match_lakefile() {
    let n = lakefile_root_count();
    let inv = read("contracts/lean/PROVABILITY-INVENTORY.md");
    // (prefix, suffix, site) — each anchor is a *current-pilot-size* claim.
    // NOTE: the "— 0 modules" KNOWN-INCOMPLETE count is intentionally NOT an
    // anchor (it is the count of non-elaborating modules, not the pilot).
    let claims: &[(&str, &str, &str)] = &[
        ("green ⇔ all ", " elaborate", "the line-11 reproduce block"),
        ("machine-checked (", " module", "the PILOT header"),
        (
            "green ⇔ all ",
            " modules still do",
            "the KNOWN-INCOMPLETE invariant",
        ),
        ("whole ", "-module substrate", "the substrate summary"),
        ("(now ", "-module", "the audit-design relationship note"),
    ];
    for &(prefix, suffix, site) in claims {
        let found = counts_between(&inv, prefix, suffix);
        assert!(
            !found.is_empty(),
            "PROVABILITY-INVENTORY.md: expected a module-count claim \
             '{prefix}<n>{suffix}' ({site}) — the anchor moved; update this gate"
        );
        for c in found {
            assert_eq!(
                c, n,
                "PROVABILITY-INVENTORY.md module-count DRIFT ({site}): claim \
                 '{prefix}{c}{suffix}' but lakefile.lean has {n} roots"
            );
        }
    }
}

// ── (c) + (d) roadmap consistency ────────────────────────────────────

/// Fold the `strategic_goals:` block (col-0 key → next col-0 key) into one
/// whitespace-collapsed string, mirroring YAML `>-` newline folding so a
/// claim spanning wrapped lines stays one clause.
fn strategic_goals_block(roadmap: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in roadmap.lines() {
        if line.starts_with("strategic_goals:") {
            inside = true;
            continue;
        }
        if inside && line.starts_with("roadmap:") {
            break;
        }
        if inside {
            out.push_str(line.trim());
            out.push(' ');
        }
    }
    out
}

/// Map every ledger `- id: PMAT-NNN` to its first following `status:`.
fn ledger_statuses(roadmap: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut current: Option<String> = None;
    for line in roadmap.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("- id: ") {
            current = Some(rest.trim().to_string());
        } else if let Some(rest) = t.strip_prefix("status: ") {
            if let Some(id) = current.take() {
                map.entry(id).or_insert_with(|| rest.trim().to_string());
            }
        }
    }
    map
}

/// PMAT ids mentioned in a clause (`PMAT-` + digits).
fn pmat_ids(clause: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut from = 0;
    while let Some(rel) = clause[from..].find("PMAT-") {
        let start = from + rel + "PMAT-".len();
        let digits: String = clause[start..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        from = start;
        if !digits.is_empty() {
            ids.push(format!("PMAT-{digits}"));
        }
    }
    ids
}

/// True iff the clause contains "COMPLETE" as a standalone word (so
/// "INCOMPLETE" / "COMPLETED" do not trip it).
fn shouts_complete(clause: &str) -> bool {
    let word = "COMPLETE";
    let mut from = 0;
    while let Some(rel) = clause[from..].find(word) {
        let at = from + rel;
        let before_ok = at == 0
            || !clause[..at]
                .chars()
                .next_back()
                .unwrap()
                .is_ascii_alphabetic();
        let after = at + word.len();
        let after_ok = match clause[after..].chars().next() {
            Some(c) => !c.is_ascii_alphabetic(),
            None => true,
        };
        if before_ok && after_ok {
            return true;
        }
        from = after;
    }
    false
}

#[test]
fn roadmap_lean_module_count_matches_lakefile() {
    let n = lakefile_root_count();
    let block = strategic_goals_block(&read("docs/roadmaps/roadmap.yaml"));
    let found = counts_between(&block, "Lean pilot now ", " machine-checked modules");
    assert!(
        !found.is_empty(),
        "roadmap.yaml strategic_goals: expected 'Lean pilot now <n> \
         machine-checked modules' — the anchor moved; update this gate"
    );
    for c in found {
        assert_eq!(
            c, n,
            "roadmap.yaml strategic_goals claims '{c} machine-checked modules' \
             but lakefile.lean has {n} roots"
        );
    }
}

#[test]
fn roadmap_complete_claims_do_not_cite_planned_items() {
    let roadmap = read("docs/roadmaps/roadmap.yaml");
    let statuses = ledger_statuses(&roadmap);
    let block = strategic_goals_block(&roadmap);
    // Clause = semicolon / sentence segment. Split on ';' then ". " — NOT on
    // a bare '.' (that would sever version literals like `v4.15.0`).
    for clause in block.split(';').flat_map(|s| s.split(". ")) {
        if !shouts_complete(clause) {
            continue;
        }
        for id in pmat_ids(clause) {
            if let Some(status) = statuses.get(&id) {
                assert_ne!(
                    status.as_str(),
                    "planned",
                    "roadmap.yaml strategic_goals shouts COMPLETE in a clause \
                     citing {id}, but its ledger entry is `status: planned`. \
                     Cite the id that actually shipped, or update the status. \
                     Clause: {clause:?}"
                );
            }
        }
    }
}
