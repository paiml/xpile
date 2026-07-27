//! Derived-counts claims-drift gate (XPILE-CLAIMS-001).
//!
//! Sibling of `lean_pilot_roots.rs`. Where that test pins the Lean pilot
//! COUNT (lakefile roots ⇔ the PROVABILITY-INVENTORY.md PILOT header), this
//! one guards the broader class of DERIVED-COUNT doc claims the architectural
//! review (`docs/specifications/fable-architectural-review.md`, F8) found
//! drifting because nothing sampled them:
//!
//!  (a) README's "N source languages" / "nine backends" are DERIVED — the
//!      live frontend registry (`xpile_core::default_session()`) and the
//!      `Target` enum (`crates/xpile-backend/src/lib.rs`) are the source of
//!      truth, not a hand-typed numeral.
//!      PMAT-1346 (XPILE-FRONTEND-SUBSTANCE-001) hardened the frontend half:
//!      it used to count `register_frontend(Arc::new(` CALL SITES, which
//!      validated the numeral while the substance was hollow — `ruchy-frontend`
//!      was registered, counted, and lowered nothing. It now RUNS each
//!      registered frontend against a real program in its own language and
//!      counts only the ones that lower it, plus asserts that none answers a
//!      real program with an empty `Ok` module.
//!  (b) EVERY module-count literal in `contracts/lean/PROVABILITY-INVENTORY.md`
//!      (the line-11 `lake build` reproduce block INCLUDED) equals the
//!      lakefile root count.
//!  (c) the roadmap's "N machine-checked modules" equals that same count.
//!  (d) a roadmap-consistency lint: a PMAT id cited in a `strategic_goals`
//!      clause that shouts "COMPLETE" must not still be `status: planned`
//!      in the same ledger (the pillar-D "Wasm lift COMPLETE" vs PMAT-952
//!      `status: planned` case).
//!
//! (b)–(d) are pure `std::fs` text parsing. (a) additionally links
//! `xpile-core` — an EXISTING dependency of this crate, not a new one — to
//! probe the live frontend registry: a text scan of registration call sites
//! is exactly what let a hollow frontend pass. Either way the required `gate`
//! job compiles this (`clippy --all-targets`) and `workspace-test` runs it
//! (`cargo test --workspace`), with zero extra CI wiring.

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

/// One probe program per registered frontend, keyed by `Frontend::name()`.
///
/// PMAT-1346: these are deliberately the *smallest program with real content*
/// in each language — not empty files. An empty file may legitimately lower to
/// an empty module, so probing with one would make the substance test vacuous
/// against exactly the bug it exists to catch.
const FRONTEND_PROBES: &[(&str, &str, &str)] = &[
    (
        "python",
        "probe.py",
        "def add(a: int, b: int) -> int:\n    return a + b\n",
    ),
    ("c", "probe.c", "int add(int a, int b) { return a + b; }\n"),
    (
        "ruchy",
        "probe.ruchy",
        "fun add(a: i64, b: i64) -> i64 { a + b }\n",
    ),
    ("bashrs", "probe.sh", "echo hello\n"),
    // The lift is a right-inverse on the `xpile-wasm-codegen` EMIT IMAGE, so
    // the probe is written in that canonical shape (named func, named params)
    // rather than in arbitrary hand-rolled WAT.
    (
        "wasm",
        "probe.wat",
        "(module\n  ;; source module: probe\n  \
         (func $add (param $a i64) (param $b i64) (result i64)\n    \
         local.get $a\n    local.get $b\n    i64.add\n  )\n)\n",
    ),
];

/// What a registered frontend actually did with a real program in its own
/// language.
#[derive(Debug)]
enum Substance {
    /// Lowered it to a module with N items — the frontend reads the language.
    Lowered(usize),
    /// Refused with a reason — honest, but NOT a source language xpile lowers.
    Refused(String),
    /// Returned `Ok` with an EMPTY module: a wrong answer delivered
    /// successfully. This is the shape PMAT-1346 exists to make impossible.
    Hollow,
}

/// Run every registered frontend against its probe. Pure runtime linkage
/// against the live `default_session()` registry — the same table the CLI
/// dispatches through, so this cannot drift from the shipped binary the way a
/// `register_frontend(Arc::new(` call-count text scan could.
///
/// The third tuple element is the frontend's own `lowers_input()`
/// DECLARATION, kept alongside the observed outcome so
/// [`frontend_lowers_input_declaration_matches_behaviour`] can confront one
/// with the other.
fn probe_registered_frontends() -> Vec<(&'static str, Substance)> {
    probe_registered_frontends_with_declarations()
        .into_iter()
        .map(|(n, s, _)| (n, s))
        .collect()
}

fn probe_registered_frontends_with_declarations() -> Vec<(&'static str, Substance, bool)> {
    let session = xpile_core::default_session();
    session
        .frontends
        .iter()
        .map(|f| {
            let name = f.name();
            let declares_lowering = f.lowers_input();
            let (_, file, source) = FRONTEND_PROBES
                .iter()
                .find(|(n, _, _)| *n == name)
                .unwrap_or_else(|| {
                    panic!(
                        "registered frontend `{name}` has no entry in FRONTEND_PROBES. \
                         A new source language must ship a probe program here — \
                         otherwise this gate silently stops covering it."
                    )
                });
            let outcome = match f.parse_and_lower(&PathBuf::from(file), source) {
                Ok(m) if m.items.is_empty() => Substance::Hollow,
                Ok(m) => Substance::Lowered(m.items.len()),
                Err(e) => Substance::Refused(e.to_string()),
            };
            (name, outcome, declares_lowering)
        })
        .collect()
}

/// PMAT-1346: `Frontend::lowers_input()` is a self-report — `xpile info` and
/// the crate docs both read it. A self-report that nothing checks is exactly
/// how a hollow frontend got counted as a source language in the first place,
/// so confront the declaration with what the frontend actually did.
#[test]
fn frontend_lowers_input_declaration_matches_behaviour() {
    for (name, outcome, declared) in probe_registered_frontends_with_declarations() {
        let observed = matches!(outcome, Substance::Lowered(_));
        assert_eq!(
            declared, observed,
            "frontend `{name}` declares lowers_input() == {declared} but a real \
             program in its own language produced {outcome:?}. The declaration \
             is what `xpile info` prints — make it match the behaviour."
        );
    }
}

/// SUBSTANTIVE frontends — the ones that genuinely lower their language.
/// This, not the registration count, is what "N source languages" means.
fn substantive_frontends() -> Vec<&'static str> {
    probe_registered_frontends()
        .into_iter()
        .filter(|(_, s)| matches!(s, Substance::Lowered(_)))
        .map(|(n, _)| n)
        .collect()
}

/// PMAT-1346 (XPILE-FRONTEND-SUBSTANCE-001). The load-bearing assertion of
/// this whole section: no registered frontend may answer a real program with
/// an empty `Ok` module.
///
/// `ruchy-frontend` did exactly that for every `.ruchy` input until PMAT-1346,
/// so `xpile transpile foo.ruchy --target rust` printed a header comment and
/// exited 0 — the only silent-wrong-answer hole in the repo, sitting directly
/// on `README.md`'s lead promise ("it refuses at transpile time with a reason
/// instead of emitting code that silently diverges"). A frontend must lower
/// its language or refuse; "succeed emptily" is not a third option.
#[test]
fn no_registered_frontend_answers_a_real_program_with_an_empty_module() {
    let hollow: Vec<&str> = probe_registered_frontends()
        .into_iter()
        .filter(|(_, s)| matches!(s, Substance::Hollow))
        .map(|(n, _)| n)
        .collect();
    assert!(
        hollow.is_empty(),
        "HOLLOW frontend(s) {hollow:?}: registered, dispatched to, and they \
         return Ok(Module {{ items: [] }}) for a real program in their own \
         language — a silently empty transpile with a ZERO exit code. Lower \
         the language or return a FrontendError naming what is unimplemented."
    );
}

/// Non-vacuity: at least one frontend must actually lower something, or the
/// hollow check above would pass trivially on a registry of pure refusals.
/// Also checks the two outcomes are *substantive in themselves* — a "lowered"
/// module has items and a refusal carries a reason.
#[test]
fn the_substance_probe_is_not_vacuous() {
    let probed = probe_registered_frontends();
    assert!(
        !probed.is_empty(),
        "default_session() registered zero frontends — the registry moved"
    );
    for (name, outcome) in &probed {
        match outcome {
            Substance::Lowered(items) => assert!(
                *items > 0,
                "frontend `{name}` reported Lowered(0) — the classifier is broken"
            ),
            // A refusal with no reason is only marginally better than a silent
            // empty module: the user still cannot tell what xpile could not do.
            Substance::Refused(reason) => assert!(
                !reason.trim().is_empty(),
                "frontend `{name}` refused with an EMPTY message — a refusal \
                 must name what is unimplemented"
            ),
            Substance::Hollow => {} // reported by the dedicated test above
        }
    }
    assert!(
        !substantive_frontends().is_empty(),
        "no registered frontend lowered its own probe program: {probed:#?}. \
         Either every frontend regressed, or the probe corpus went stale."
    );
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

/// PMAT-1346: the README numeral is derived from SUBSTANTIVE frontends, not
/// from `register_frontend` call sites.
///
/// The previous form counted registrations, which validated the numeral while
/// the substance was hollow: `ruchy-frontend` was registered, counted toward
/// "five source languages", and lowered nothing at all. Counting behaviour
/// instead means the claim can only be satisfied by a frontend that works.
#[test]
fn readme_source_language_count_is_derived_from_substantive_frontends() {
    let substantive = substantive_frontends();
    let n = substantive.len();
    let readme = read("README.md");
    let needle = format!("{} source languages", word(n));
    assert!(
        readme.contains(&needle),
        "README.md must say '{needle}': {n} registered frontends actually \
         lower their own language ({substantive:?}). Update the README \
         numeral to match the behaviour, or make a frontend substantive."
    );
}

/// A registered-but-refusing frontend is honest only if the README says so.
/// Otherwise the docs still advertise an input language that refuses every
/// input — the numeral would be right and the story still wrong.
#[test]
fn registered_frontends_that_refuse_are_disclosed_in_the_readme() {
    let readme = read("README.md").to_lowercase();
    for (name, outcome) in probe_registered_frontends() {
        let Substance::Refused(_) = outcome else {
            continue;
        };
        let disclosed = readme
            .lines()
            .any(|l| l.contains(name) && l.contains("refuse"));
        assert!(
            disclosed,
            "frontend `{name}` is registered but REFUSES every input, and no \
             README line mentions both `{name}` and 'refuse'. A registered \
             language that cannot be read must be disclosed as such."
        );
    }
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

// ── (e) CURRENT.md carries NO bare derived counts ────────────────────

/// Number words this gate recognises as a written-out count, plus the digit
/// case. `word()`'s table is the emit side; this is the detect side, and it
/// deliberately includes the small words prose actually reaches for.
fn is_bare_count(tok: &str) -> bool {
    let t = tok
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    if t.is_empty() {
        return false;
    }
    t.chars().all(|c| c.is_ascii_digit()) || NUMBER_WORDS.contains(&t.as_str())
}

/// The last whitespace-separated token before byte offset `at`.
fn token_before(text: &str, at: usize) -> Option<&str> {
    text[..at].split_whitespace().next_back()
}

/// PMAT-1348: `docs/status/CURRENT.md` is declared a POINTER file — numbers in
/// it must be stated as the command that derives them, never typed inline.
///
/// The 2026-05-18 demotion of this file to "a thin index" was prose only, and
/// counts crept straight back in: `27 workspace crates` (live: 31),
/// `12 contracts` (live: 35), a `12 QUORUM, 0 PARTIAL` line (live: 24/11),
/// `PTX / WGSL / SPIR-V still scaffolded` (all three emit), and a
/// `crates.io: xpile 0.0.1 name reservation` (0.1.616 was live). Five false
/// claims, two months, in the file `docs/status/INDEX.md` calls "the single
/// source of truth".
///
/// A doc rule with no gate is a suggestion — so this is the gate.
#[test]
fn current_md_carries_no_bare_derived_counts() {
    let cur = read("docs/status/CURRENT.md");
    // Noun phrases whose count is DERIVABLE from the tree. A bare integer or
    // number-word immediately before one of these is drift waiting to happen.
    let banned_nouns: &[&str] = &[
        "workspace crates",
        "contracts pass",
        "contracts total",
        "Kani BMC harnesses",
        "Kani harnesses",
        "Lean theorems",
        "machine-checked modules",
        "stratum-vote artifacts",
        "wired Diamond equations",
        "PRs merged",
        "QUORUM",
        "PARTIAL",
        "UNVERIFIED",
    ];
    let mut offences = Vec::new();
    for noun in banned_nouns {
        let mut from = 0;
        while let Some(rel) = cur[from..].find(noun) {
            let at = from + rel;
            if let Some(tok) = token_before(&cur, at) {
                if is_bare_count(tok) {
                    offences.push(format!("`{tok} {noun}`"));
                }
            }
            from = at + noun.len();
        }
    }
    assert!(
        offences.is_empty(),
        "docs/status/CURRENT.md carries bare derived count(s): {}. \
         CURRENT.md is a POINTER file — state the DERIVE COMMAND instead \
         (see its 'Derive the live numbers' table). Every count typed here \
         has gone stale; that is why this gate exists.",
        offences.join(", ")
    );
}

/// Regression pin naming the EXACT strings that were live and false, so the
/// gate above can never be quietly loosened back past them.
#[test]
fn current_md_does_not_carry_the_2026_05_stale_claims() {
    let cur = read("docs/status/CURRENT.md");
    // (needle, what was actually true when it was found stale on 2026-07-26)
    let stale: &[(&str, &str)] = &[
        ("27 workspace crates", "31 crates"),
        ("12 contracts", "35 contracts"),
        ("still scaffolded", "PTX, WGSL and SPIR-V all emit"),
        ("0.0.1", "0.1.616 was published to crates.io"),
    ];
    for &(needle, truth) in stale {
        assert!(
            !cur.contains(needle),
            "docs/status/CURRENT.md re-introduced the stale claim {needle:?} \
             (truth at 2026-07-26: {truth})"
        );
    }
}

/// PMAT-1411 INVERTED THIS TEST, and the inversion is the point.
///
/// It used to REQUIRE CURRENT.md to say the DEFAULT `--target lean` emit does
/// not elaborate and that `--contracts off` is the elaborating form. PMAT-1405
/// then changed the lane's citation form from the unparseable
/// `@[xpile_contract "…"]` attribute to a `/-- xpile-contract: … -/` docstring,
/// so `lean` accepts the default emit — MEASURED 2026-07-27, `lean` exits 0,
/// and `lean_default_emit_witness.rs` holds it there. The caveat became false
/// the moment the defect was fixed, and this gate went on ENFORCING it: a
/// green test demanding that the docs describe a defect that no longer exists.
///
/// Disclosing a caveat that has been retired is the same CLAIMS-001 falsehood
/// as omitting a live one, just pointing the other way. So the assertion is now
/// the NEGATIVE: the retired wording must not come back. The positive claim —
/// that the default emit really does elaborate — is held by an EXECUTION
/// witness (`lean_default_emit_witness.rs` runs `lean`), which is where a claim
/// about the toolchain belongs, not by a substring in a Markdown file.
#[test]
fn current_md_does_not_resurrect_the_retired_lean_contracts_caveat() {
    let cur = read("docs/status/CURRENT.md");
    // Wording that was true through v0.1.617 and false after PMAT-1405. Each
    // needle is scoped tightly enough that the "Superseded:" paragraph
    // documenting the history does not trip it.
    let retired: &[(&str, &str)] = &[
        (
            "not a registered Lean attribute**",
            "the Lean CODE lane cites via a docstring; the attribute form \
             survives only in the never-elaborated contract-rendering lane",
        ),
        (
            "For Lean output you intend to elaborate, use",
            "the DEFAULT emit elaborates; no flag change is needed",
        ),
        (
            "the default does NOT elaborate",
            "`lean` exits 0 on the default emit (PMAT-1405)",
        ),
    ];
    for &(needle, truth) in retired {
        assert!(
            !cur.contains(needle),
            "docs/status/CURRENT.md re-introduced the RETIRED Lean caveat \
             {needle:?}. Truth as of 2026-07-27: {truth}. If the default emit \
             has genuinely regressed, `lean_default_emit_witness.rs` is where \
             that gets caught and fixed — do not re-document the old defect to \
             make the docs match a regression."
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
