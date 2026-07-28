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
    //
    // PMAT-1421: this probe used a bare `i64.add` and went STALE by exactly
    // the drift that slice fixed. PMAT-1402 re-routed `+` through
    // `$__wasm_add_i64`, so a bare `i64.add` stopped being in the image the
    // comment above says the probe is written in — the emit produces it only
    // inside the `$__wasm_*` prelude, which the lift skips wholesale. It kept
    // passing because the lift still carried a stale inverse arm for the bare
    // opcode; when that arm was removed (it mis-lifted hand-written WAT to
    // Python semantics and re-emitted to a different running value), THIS
    // GATE is what caught the probe. The probe is now in the current image:
    // the helper is declared so the module is standalone well-formed WAT
    // (`wat2wasm`-checked), and the lift skips the helper by name and inverts
    // the `call` — so this still exercises real lowering, not a weaker claim.
    (
        "wasm",
        "probe.wat",
        "(module\n  ;; source module: probe\n  \
         (func $__wasm_add_i64 (param $x i64) (param $y i64) (result i64)\n    \
         local.get $x\n    local.get $y\n    i64.add\n  )\n  \
         (func $add (param $a i64) (param $b i64) (result i64)\n    \
         local.get $a\n    local.get $b\n    call $__wasm_add_i64\n  )\n)\n",
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

// ── (f) the BOOK is a LIVE claim surface, not a v0.1.0 snapshot ──────

// Sections (a) and (e) point the derived-count machinery at exactly two
// files: `README.md` and `docs/status/CURRENT.md`. `book/src/` — the
// mdBook a `cargo install xpile` user actually reads — was never in scope,
// and drifted exactly the way an ungated claim surface does (PMAT-1417):
//
//   * `introduction.md` opened with "Seven frontends — Python, C, C++,
//     Rust, Ruchy, Lean 4, Shell". Three of those seven have no frontend
//     crate and no registration; `wasm`, which does, was missing. The same
//     paragraph said "seven backends" (nine) and "12 contracts at full
//     quorum" (35 contracts, 9 of them PARTIAL).
//   * `installation.md` printed a `$ xpile --version` → `xpile 0.1.0`
//     transcript, and called the workspace 27 crates "published at v0.1.0".
//   * `reference/frontends.md` marked the C frontend a scaffold (it emits),
//     omitted `wasm`, and listed C++/Rust/Lean 4 as workspace-member
//     frontends under a sentence that said "Two more" before three bullets.
//   * `reference/backends.md` listed 7 of 9 backends, called PTX/WGSL a
//     scaffold and SPIR-V "not yet a crate" (all three emit), and
//     documented the Lean citation as `@[xpile_contract "<ID>"]` — the form
//     PMAT-1405 had removed the day before *because* no Lean prelude
//     registers it, so the default `--target lean` emit did not parse.
//     `tutorials/python-to-lean.md` went further and asserted the attribute
//     "is a real Lean attribute. Not a comment."
//
// The correction is not to retype the numerals. Section (e)'s own lesson is
// that a typed count is drift waiting to happen, so what is enforced here is
// the DERIVE relationship: every claim below is re-derived from the live
// registry, the live emit, or the live tree on each run, and the book is
// required to agree with it or to state the command instead.

/// Every `*.md` under `book/src/`, recursively, as (repo-relative path,
/// contents). Walks the tree rather than naming files, so a page added later
/// is covered the moment it lands — the failure mode being repaired is a
/// claim surface that nothing enumerates.
fn book_pages() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, String)>) {
        let entries =
            fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
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
                let body = fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {rel}: {e}"));
                out.push((rel, body));
            }
        }
    }
    let root = workspace_root();
    let mut out = Vec::new();
    walk(&root.join("book/src"), &root, &mut out);
    out
}

/// REGISTRY KEYS of every registered backend — `Backend::name()`, what
/// `xpile info` prints under `backends (N)`, read from the same
/// `default_session()` the CLI dispatches through. PMAT-1430: this was named
/// `registered_backend_flags` and is NOT the `--target` flag set; `bashrs` is
/// a key with no flag of that spelling.
fn registered_backend_keys() -> Vec<String> {
    xpile_core::default_session()
        .backends
        .iter()
        .map(|b| b.name().to_string())
        .collect()
}

/// Names of every registered frontend, substantive or not.
fn registered_frontend_names() -> Vec<String> {
    xpile_core::default_session()
        .frontends
        .iter()
        .map(|f| f.name().to_string())
        .collect()
}

/// Does `needle` occur in some MARKDOWN TABLE ROW of `page` as a whole word?
///
/// Both halves matter. **Table row** (a line starting with `|`) is where a
/// roster page states its roster; an incidental mention in prose is not an
/// entry. **Whole word**, with `-` counted as a word character, is what stops
/// `wasm` from being "found" inside `xpile-wasm-codegen` — the first cut of
/// this gate used a bare `page.contains()` and stayed GREEN when both the
/// `wasm` and `forjar` rows were deleted, because the codegen crate names in
/// neighbouring rows still contained the substrings. A roster gate that
/// survives deleting the row it is supposed to require is not a gate.
fn table_row_names(page: &str, needle: &str) -> bool {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    let hay = page.to_lowercase();
    let needle = needle.to_lowercase();
    hay.lines()
        .filter(|l| l.trim_start().starts_with('|'))
        .any(|row| {
            let mut from = 0;
            while let Some(rel) = row[from..].find(&needle) {
                let at = from + rel;
                let end = at + needle.len();
                let before_ok = row[..at].chars().next_back().is_none_or(|c| !is_word(c));
                let after_ok = row[end..].chars().next().is_none_or(|c| !is_word(c));
                if before_ok && after_ok {
                    return true;
                }
                from = at + 1;
            }
            false
        })
}

/// Anti-vacuity for the whole section: every assertion below is a scan over
/// this corpus, and a scan over nothing passes for free.
#[test]
fn the_book_corpus_is_not_empty() {
    let pages = book_pages();
    assert!(
        pages.len() > 1,
        "book_pages() found {} page(s) under book/src/ — the book moved or \
         the walker broke, and every (f) assertion is now vacuous",
        pages.len()
    );
    for (rel, body) in &pages {
        assert!(
            !body.trim().is_empty(),
            "book page {rel} is empty — it cannot be carrying the claims this \
             section checks"
        );
    }
}

/// The backends reference must name every registered backend's REGISTRY KEY
/// — `Backend::name()`, the string `xpile info` prints. It listed 7 of 9 (no
/// `wasm`, no `forjar`) while both emitted.
///
/// PMAT-1430: this doc comment and the failure message below used to call
/// those values "`--target` flag"s. They are not. `Backend::name()` and the
/// `--target` spelling coincide for eight of the nine backends and differ for
/// the shell backend, whose key is `bashrs` and whose flag is `shell`. That
/// mislabel is what put `bashrs` in backends.md's `--target` column and kept
/// it there: correcting the cell alone would have turned THIS test red, so
/// the gate was enforcing the defect. The page now carries both columns, and
/// the `--target` half is checked against the running binary by
/// `backend_docs_drift.rs` (XPILE-CLIDOCS-002).
#[test]
fn book_backend_reference_names_every_registered_backend() {
    let page = read("book/src/reference/backends.md");
    let keys = registered_backend_keys();
    assert!(
        !keys.is_empty(),
        "default_session() registered zero backends — the registry moved"
    );
    let missing: Vec<&String> = keys.iter().filter(|f| !table_row_names(&page, f)).collect();
    assert!(
        missing.is_empty(),
        "book/src/reference/backends.md does not name registered backend \
         key(s) {missing:?} (the `Name` column — `Backend::name()`, what \
         `xpile info` prints, NOT the `--target` spelling). The page presents \
         itself as the backend roster; a backend the CLI dispatches to and the \
         book omits is a capability the reader cannot discover. Live registry: \
         {keys:?}"
    );
}

/// …and the frontends reference must name every registered frontend. It
/// omitted `wasm` entirely.
#[test]
fn book_frontend_reference_names_every_registered_frontend() {
    let page = read("book/src/reference/frontends.md");
    let names = registered_frontend_names();
    assert!(
        !names.is_empty(),
        "default_session() registered zero frontends — the registry moved"
    );
    let missing: Vec<&String> = names
        .iter()
        .filter(|n| !table_row_names(&page, n))
        .collect();
    assert!(
        missing.is_empty(),
        "book/src/reference/frontends.md does not name registered frontend(s) \
         {missing:?}. Live registry: {names:?}"
    );
}

/// The inverse direction, and the one that caught C++/Rust/Lean 4: the book
/// may not advertise a frontend CRATE that does not exist in the tree. The
/// page listed `crates/cpp-frontend`-shaped claims in prose ("C++ — planned",
/// "Rust — scaffold", "Lean 4 — scaffold") as *workspace members*, which they
/// have never been.
#[test]
fn book_frontend_reference_names_no_frontend_crate_that_does_not_exist() {
    let page = read("book/src/reference/frontends.md");
    let crates_dir = workspace_root().join("crates");
    let existing: Vec<String> = fs::read_dir(&crates_dir)
        .unwrap_or_else(|e| panic!("read_dir crates/: {e}"))
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        existing.iter().any(|c| c.ends_with("-frontend")),
        "no `*-frontend` crate found under crates/ — the layout moved and \
         this gate would pass vacuously"
    );
    // Every `<word>-frontend` token the page cites IN CODE FONT must be a real
    // directory. The backtick requirement is what distinguishes a crate
    // citation (the table's Crate column) from a prose link target such as
    // `contributing/adding-a-frontend.md`, which is a page, not a crate.
    let mut cited = Vec::new();
    for span in page.split('`').skip(1).step_by(2) {
        for tok in span.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
            if tok.ends_with("-frontend") && tok.len() > "-frontend".len() {
                cited.push(tok.to_string());
            }
        }
    }
    assert!(
        !cited.is_empty(),
        "book/src/reference/frontends.md cites no `*-frontend` crate at all — \
         the table lost its Crate column and this gate stopped covering it"
    );
    let phantom: Vec<&String> = cited.iter().filter(|c| !existing.contains(c)).collect();
    assert!(
        phantom.is_empty(),
        "book/src/reference/frontends.md advertises frontend crate(s) \
         {phantom:?} that do not exist under crates/. A source language the \
         book lists and the tree does not implement is the exact claim this \
         section exists to stop. Existing: {existing:?}"
    );
}

/// A `--version` transcript is a numeral, and a numeral in prose is not
/// re-derived when the workspace is published. `installation.md` printed
/// `xpile 0.1.0` under a "Verify:" heading for every release after v0.1.0.
///
/// The rule is REMOVAL, not correction (section (e)'s rule): no book page may
/// pin a version transcript at all — not even the current one, which would
/// simply red this gate at the next release bump and teach the next author to
/// retype it.
#[test]
fn book_pins_no_version_transcript() {
    let live = env!("CARGO_PKG_VERSION");
    let mut offences = Vec::new();
    for (rel, body) in book_pages() {
        for (n, line) in body.lines().enumerate() {
            // The exact shape `xpile --version` prints: `xpile <semver>` with
            // nothing before it on the line.
            let Some(rest) = line.trim_start().strip_prefix("xpile ") else {
                continue;
            };
            let ver = rest.trim();
            let parts: Vec<&str> = ver.split('.').collect();
            if parts.len() == 3
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
            {
                offences.push(format!("{rel}:{}: `{}`", n + 1, line.trim()));
            }
        }
    }
    assert!(
        offences.is_empty(),
        "book page(s) pin a `xpile <version>` transcript: {}. The live \
         version is {live} and it changes every release, so a pinned \
         transcript is stale by construction — describe the output instead \
         of reproducing it.",
        offences.join(", ")
    );
}

/// Same rule for the other transcript the book pasted: `xpile quorum`'s
/// totals line. `concepts/contracts.md` carried
/// `totals: 12 QUORUM, 0 PARTIAL, 0 UNVERIFIED (12 contracts total)` while
/// the live totals had moved on and PARTIAL was no longer zero — a pasted
/// transcript asserting completeness the substrate had not kept.
#[test]
fn book_pastes_no_derived_totals_transcript() {
    let mut offences = Vec::new();
    for (rel, body) in book_pages() {
        for (n, line) in body.lines().enumerate() {
            let t = line.trim();
            if !t.starts_with("totals:") {
                continue;
            }
            // A placeholder form (`<N> QUORUM`) is the encouraged spelling and
            // cannot go stale; only digits are drift.
            if t.chars().any(|c| c.is_ascii_digit()) {
                offences.push(format!("{rel}:{}: `{t}`", n + 1));
            }
        }
    }
    assert!(
        offences.is_empty(),
        "book page(s) paste a derived `totals:` transcript with literal \
         numerals: {}. Run the command in the text and show the SHAPE \
         (`totals: <N> QUORUM, …`); the numbers move every sprint.",
        offences.join(", ")
    );
}

/// The Lean citation form the book shows must be the form the backend
/// EMITS — derived by running the shipped binary, not typed here.
///
/// PMAT-1405 replaced `@[xpile_contract "<ID>"]` with a
/// `/-- xpile-contract: <ID> -/` docstring because no Lean prelude registers
/// that attribute, so the DEFAULT `--target lean` emit did not parse. The
/// book kept showing the attribute in two `--target lean` transcripts and
/// asserted in prose that it "is a real Lean attribute. Not a comment."
///
/// Deriving both sides means a revert of PMAT-1405 flips this gate
/// automatically rather than leaving the book pinned to whichever form
/// happened to be true when someone last read it.
#[test]
fn book_lean_transcripts_carry_the_live_citation_form() {
    let dir = std::env::temp_dir().join(format!("xpile-claims1417-lean-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("create {}: {e}", dir.display()));
    let src = dir.join("probe.py");
    fs::write(&src, "def probe(n: int) -> int:\n    return n + 1\n").expect("write probe");

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_xpile"))
        .args([
            "transpile",
            src.to_str().expect("utf-8 path"),
            "--target",
            "lean",
        ])
        .output()
        .expect("run xpile transpile --target lean");
    assert!(
        out.status.success(),
        "`xpile transpile probe.py --target lean` failed ({}), so the live \
         citation form could not be derived and this gate would be vacuous. \
         stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let emit = String::from_utf8(out.stdout).expect("utf-8 emit");
    let cite = emit
        .lines()
        .find(|l| l.contains("xpile-contract") || l.contains("xpile_contract"))
        .unwrap_or_else(|| {
            panic!("the default `--target lean` emit carries NO contract citation:\n{emit}")
        });

    // Which spelling does the live emitter use? Derived, never assumed.
    let emits_docstring = cite.contains("/--") && cite.contains("xpile-contract");
    let emits_attribute = cite.contains("@[xpile_contract");
    assert!(
        emits_docstring ^ emits_attribute,
        "cannot classify the live Lean citation form from {cite:?} — update \
         this gate rather than letting it guess"
    );

    // Book pages that reproduce a `--target lean` transcript. Identified by
    // the command itself so a new tutorial is covered automatically.
    let mut checked = 0usize;
    let mut offences = Vec::new();
    for (rel, body) in book_pages() {
        let lines: Vec<&str> = body.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.contains("--target lean") {
                continue;
            }
            // The transcript is the rest of this fenced block.
            let mut block = Vec::new();
            for l in &lines[i + 1..] {
                if l.trim_start().starts_with("```") {
                    break;
                }
                block.push(*l);
            }
            if block.is_empty() {
                continue;
            }
            checked += 1;
            let text = block.join("\n");
            if !text.contains("xpile-contract") && !text.contains("xpile_contract") {
                continue; // a transcript that shows no citation claims nothing
            }
            let shows_attribute = text.contains("@[xpile_contract");
            let shows_docstring = text.contains("/-- xpile-contract");
            if emits_docstring && shows_attribute {
                offences.push(format!(
                    "{rel}:{}: transcript shows `@[xpile_contract …]` but the \
                     backend emits {cite:?}",
                    i + 1
                ));
            }
            if emits_attribute && shows_docstring {
                offences.push(format!(
                    "{rel}:{}: transcript shows a docstring but the backend \
                     emits {cite:?}",
                    i + 1
                ));
            }
        }
    }
    assert!(
        checked > 0,
        "no book page reproduces a `--target lean` transcript — either the \
         tutorials were removed or the fence scanner broke; either way this \
         gate is covering nothing"
    );
    assert!(
        offences.is_empty(),
        "book Lean transcript(s) show a citation form the backend does not \
         emit: {}. The live emit line is {cite:?}.",
        offences.join(", ")
    );
    let _ = fs::remove_dir_all(&dir);
}

/// A book that says "full quorum" while contracts sit at PARTIAL is making
/// the one claim this repository exists to be trustworthy about.
///
/// Both sides derived: the PARTIAL population from the contract YAMLs' own
/// stratum references, the claim from the book text.
#[test]
fn book_claims_no_total_quorum_while_any_contract_is_partial() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_xpile"))
        .args(["quorum", "--contracts-dir"])
        .arg(workspace_root().join("contracts"))
        .current_dir(workspace_root())
        .output()
        .expect("run xpile quorum");
    assert!(
        out.status.success(),
        "`xpile quorum` failed ({}), so the live PARTIAL count could not be \
         derived and this gate would be vacuous. stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let report = String::from_utf8(out.stdout).expect("utf-8 quorum report");
    let totals = report
        .lines()
        .find(|l| l.trim_start().starts_with("totals:"))
        .unwrap_or_else(|| panic!("`xpile quorum` printed no totals line:\n{report}"));
    let partial = counts_between(totals, ", ", " PARTIAL")
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("could not parse a PARTIAL count from {totals:?}"));
    if partial == 0 {
        return; // the claim would be true; nothing to forbid
    }
    // Phrases that assert TOTAL discharge. Each was live in the book while
    // PARTIAL was non-zero.
    let banned = [
        "full quorum",
        "100% QUORUM",
        "100% §14.4",
        "all at QUORUM",
        "0 PARTIAL",
    ];
    let mut offences = Vec::new();
    let mut scanned = 0usize;
    for (rel, body) in book_pages() {
        // `book/src/changelog.md` is a DATED release log: "v0.1.0 — 12
        // contracts at 100% QUORUM" was true of v0.1.0 and stays true of it.
        // The exemption is one file and is checked to really be that file, so
        // it cannot quietly widen into "the book is exempt".
        if rel == "book/src/changelog.md" {
            assert!(
                body.contains("## v0.1.0"),
                "book/src/changelog.md is exempted here as a dated release \
                 log, but it no longer carries a dated `## v0.1.0` heading — \
                 re-justify the exemption or drop it"
            );
            continue;
        }
        scanned += 1;
        for (n, line) in body.lines().enumerate() {
            for b in banned {
                if line.contains(b) {
                    offences.push(format!("{rel}:{}: `{b}`", n + 1));
                }
            }
        }
    }
    assert!(
        scanned > 1,
        "the exemption swallowed the corpus: only {scanned} page(s) scanned"
    );
    assert!(
        offences.is_empty(),
        "the book claims TOTAL §14.4 quorum — {} — but `xpile quorum` reports \
         {partial} contract(s) at PARTIAL. Totals line: {}",
        offences.join(", "),
        totals.trim()
    );
}

/// The Diamond page defines **depth-N UNIVERSAL** as "*every* contract has at
/// least N distinct Diamond theorem categories" and then claimed
/// "depth-1..13 UNIVERSAL — all 12 contracts have ≥13 Diamond categories".
///
/// By the page's own definition that had stopped being true: new contracts
/// join at depth-1+ (`diamond_coverage.rs` grandfathers the depth-13 gate on
/// purpose), so the universal depth over the WHOLE population is set by the
/// shallowest contract, not by the deep core. The claim was a statement about
/// all contracts backed by a property of thirteen of them.
///
/// Derived from `xpile diamond --json`, so it tracks the substrate instead of
/// pinning a milestone.
#[test]
fn book_claims_no_universal_depth_the_substrate_does_not_hold() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_xpile"))
        .args(["diamond", "--json", "--contracts-dir"])
        .arg(workspace_root().join("contracts"))
        .current_dir(workspace_root())
        .output()
        .expect("run xpile diamond --json");
    assert!(
        out.status.success(),
        "`xpile diamond --json` failed ({}), so the live universal depth could \
         not be derived and this gate would be vacuous. stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let report = String::from_utf8(out.stdout).expect("utf-8 diamond report");
    // Minimal extraction, no serde_json shape to keep in sync: every contract
    // row carries a `"diamond_count":<N>`, and the universal depth is their
    // minimum.
    let counts = counts_between(&report, "\"diamond_count\":", ",");
    assert!(
        !counts.is_empty(),
        "parsed 0 `diamond_count` fields out of `xpile diamond --json` — the \
         JSON shape moved; update this gate rather than letting it pass on \
         an empty set:\n{report}"
    );
    let universal = *counts.iter().min().expect("non-empty");

    // A claim of the form "depth-N UNIVERSAL" for N > the live universal depth.
    let mut offences = Vec::new();
    for (rel, body) in book_pages() {
        for (n, line) in body.lines().enumerate() {
            for claimed in counts_between(line, "depth-", " UNIVERSAL") {
                if claimed > universal {
                    offences.push(format!("{rel}:{}: claims depth-{claimed} UNIVERSAL", n + 1));
                }
            }
        }
    }
    assert!(
        offences.is_empty(),
        "the book claims a UNIVERSAL depth the substrate does not hold — {} — \
         but the shallowest contract carries {universal} Diamond \
         categor{}, so depth-{universal} is the live universal depth. Say \
         which SUBSET is deep, or lift the shallow contracts.",
        offences.join(", "),
        if universal == 1 { "y" } else { "ies" }
    );
}
