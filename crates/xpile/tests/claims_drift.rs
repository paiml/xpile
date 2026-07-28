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
/// Every depth a piece of prose claims to be UNIVERSAL, in all THREE spellings
/// the corpus uses: the plain `depth-N UNIVERSAL`, the RANGE form
/// `depth-A..B UNIVERSAL`, and the SLASH-LIST form
/// `depth-A/B/C/… UNIVERSAL`.
///
/// A range and a slash list each claim their LARGEST member —
/// "depth-1..13 UNIVERSAL" and "depth-1/2/…/13 UNIVERSAL" both assert that
/// every contract carries at least thirteen Diamond categories, not at least
/// one — so that is what is returned. A trailing `+` (`depth-13+ UNIVERSAL`)
/// is the same claim as `depth-13`.
///
/// PMAT-1448: the caller used `counts_between(.., "depth-", " UNIVERSAL")`,
/// which cannot match a range, and the range is the spelling that falsehood
/// actually used.
///
/// PMAT-1450: the repair was still one spelling short of the corpus. The
/// canonical spec's instance — `xpile-spec.md`'s description of what the
/// Diamond CI gate enforces — is written
/// `depth-1/2/3/4/5/6/7/8/9/10/11/12/13 UNIVERSAL (all 12 contracts)`, and
/// `claimed_universal_depths` scored it ZERO: only the final `13` is followed
/// by ` UNIVERSAL`, and it is not preceded by `depth-`. Measured control, on
/// the tree that carried it: the PMAT-1448 parser returns `[]` for that line
/// and this one returns `[13]`. Ask what the DEFECT spells, every time — the
/// answer has now been a different spelling twice running.
fn claimed_universal_depths(text: &str) -> Vec<usize> {
    claimed_universal_depths_at(text)
        .into_iter()
        .map(|(d, _)| d)
        .collect()
}

/// As [`claimed_universal_depths`], plus the byte offset of each claim's
/// `depth-` prefix — needed to ask whether the claim sits inside a QUOTED span
/// (see [`quoted_spans`]).
fn claimed_universal_depths_at(text: &str) -> Vec<(usize, usize)> {
    const PREFIX: &str = "depth-";
    let bytes = text.as_bytes();
    let digits_at = |i: &mut usize| -> Option<usize> {
        let start = *i;
        while *i < bytes.len() && bytes[*i].is_ascii_digit() {
            *i += 1;
        }
        if *i == start {
            None
        } else {
            text[start..*i].parse().ok()
        }
    };
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = text[from..].find(PREFIX) {
        let at = from + rel;
        let mut i = at + PREFIX.len();
        from = i; // prefix is non-empty ⇒ strictly advances
        let Some(low) = digits_at(&mut i) else {
            continue;
        };
        let mut claimed = low;
        if text[i..].starts_with("..") {
            let mut j = i + 2;
            if let Some(high) = digits_at(&mut j) {
                claimed = high;
                i = j;
            }
        }
        // Slash list: `depth-1/2/…/13`. Consume every `/<digits>` run and keep
        // the largest, so the claim is read at its strongest.
        while text[i..].starts_with('/') {
            let mut j = i + 1;
            let Some(next) = digits_at(&mut j) else { break };
            claimed = claimed.max(next);
            i = j;
        }
        if text[i..].starts_with('+') {
            i += 1;
        }
        if text[i..].trim_start().starts_with("UNIVERSAL") {
            out.push((claimed, at));
        }
    }
    out
}

/// Byte ranges of `"…"` and `` `…` `` spans in `text` — the two ways this
/// corpus quotes a sentence it is reporting rather than making.
///
/// PMAT-1450: PMAT-1448's denial rule is paragraph-scoped, and its comment says
/// "prose may QUOTE a falsehood … but it may not assert one" — but the code
/// only checked that a denial phrase appeared SOMEWHERE in the paragraph, so a
/// paragraph that legitimately quotes a retired claim became permanently exempt
/// and could assert a live one alongside it. Found by running the red half
/// against this slice's OWN repair: the corrected `xpile-spec.md` bullet quotes
/// the retired wording and says "used to", and re-asserting the falsehood in
/// that same bullet left the gate GREEN. A disclosure in front of a false pass
/// is still a false pass.
fn quoted_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    for delim in ['"', '`'] {
        let mut open: Option<usize> = None;
        for (i, c) in text.char_indices() {
            if c != delim {
                continue;
            }
            match open {
                None => open = Some(i),
                Some(start) => {
                    spans.push((start, i + c.len_utf8()));
                    open = None;
                }
            }
        }
    }
    spans
}

/// The marker that declares a documentation SECTION to be a dated record of
/// what was once true rather than a description of the live system.
///
/// PMAT-1450: it is section-scoped, and deliberately not sentence-scoped. The
/// milestone enumerations in `sub/diamond-taxonomy.md` and `xpile-spec.md` §28
/// are 40+ consecutive bullets inside ONE blank-line-delimited paragraph, so a
/// paragraph-scoped denial would have let a single clause whitewash the lot.
/// A heading is also the unit a READER sees before the list, which is the
/// point: the framing has to be visible without scrolling into it.
const HISTORICAL_MARKER: &str = "(historical record)";

/// One blank-line-delimited paragraph of a markdown file.
struct Para {
    /// 1-based line number the paragraph starts on.
    start: usize,
    /// The paragraph's lines joined with a single space — a claim can WRAP
    /// across a line break, so the scan runs over this, not over lines.
    flat: String,
    /// The nearest preceding ATX heading.
    heading: String,
    /// The source lines, so a byte offset into `flat` can be attributed back to
    /// the line that actually carries it.
    lines: Vec<(usize, String)>,
}

impl Para {
    /// The 1-based line number and text of the line containing byte offset
    /// `at` in [`Self::flat`]. `flat` is `lines.join(" ")`, so the mapping is
    /// exact: each line contributes its own length plus one for the join.
    fn line_at(&self, at: usize) -> (usize, &str) {
        let mut consumed = 0usize;
        for (n, l) in &self.lines {
            let end = consumed + l.len();
            if at < end {
                return (*n, l);
            }
            consumed = end + 1; // the joining space
        }
        self.lines
            .last()
            .map(|(n, l)| (*n, l.as_str()))
            .unwrap_or((self.start, ""))
    }
}

/// A file split into paragraphs, each carrying the nearest preceding ATX
/// heading. A heading is a hard paragraph break, and is itself yielded so a
/// claim written INTO a heading is still scanned.
///
/// PMAT-1451 — FENCED CODE IS NOT PROSE. A `#` comment inside a ``` fence has
/// the shape of an ATX heading, and this walker used to accept it as one. Both
/// directions are wrong and one of them is unsound: a Python or shell sample
/// containing `# … (historical record)` would have GRANTED the exemption to
/// every paragraph after it, and an ordinary `# real_python.py` sample line
/// silently REVOKES the real heading from everything below the fence. No
/// fenced block in the corpus spells the marker today, so this is a hardening
/// with no current verdict change — pinned by
/// `a_fence_comment_is_not_a_heading`, since a corpus that does not yet
/// contain the shape cannot demonstrate the rule.
fn paragraphs_under_headings(body: &str) -> Vec<Para> {
    let mut out = Vec::new();
    let mut start = 1usize;
    let mut buf: Vec<(usize, String)> = Vec::new();
    let mut heading = String::new();
    let mut fenced = false;
    for (i, line) in body.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
        }
        let is_heading =
            !fenced && t.starts_with('#') && t.trim_start_matches('#').starts_with(' ');
        if line.trim().is_empty() || is_heading {
            if !buf.is_empty() {
                out.push(Para {
                    start,
                    flat: buf
                        .iter()
                        .map(|(_, l)| l.as_str())
                        .collect::<Vec<_>>()
                        .join(" "),
                    heading: heading.clone(),
                    lines: std::mem::take(&mut buf),
                });
            }
            if is_heading {
                heading = t.to_string();
                out.push(Para {
                    start: i + 1,
                    flat: t.to_string(),
                    heading: heading.clone(),
                    lines: vec![(i + 1, t.to_string())],
                });
            }
            continue;
        }
        if buf.is_empty() {
            start = i + 1;
        }
        buf.push((i + 1, line.to_string()));
    }
    if !buf.is_empty() {
        out.push(Para {
            start,
            flat: buf
                .iter()
                .map(|(_, l)| l.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            heading,
            lines: buf,
        });
    }
    out
}

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

// ── (b2) pilot SIZE is a CLAIM CLASS, over the WHOLE tracked corpus ──

/// Modules in `contracts/lean/` that are NOT lakefile roots — the
/// KNOWN-INCOMPLETE population. Derived, not exempted: PMAT-1451's rule is
/// that a carve-out which is not checked is a hole, so the "0 modules"
/// KNOWN-INCOMPLETE claim the anchor gate above deliberately skips is
/// verified here against its own denominator instead of being waved past.
fn known_incomplete_module_count() -> usize {
    let files = lean_module_files("contracts/lean");
    let roots = lakefile_root_count();
    assert!(
        files >= roots,
        "contracts/lean/ has {files} module file(s) but lakefile.lean lists \
         {roots} root(s) — a root names a file that does not exist, or the \
         roots parser is over-counting"
    );
    files - roots
}

/// Modules in the SEPARATE Mathlib lane. Also derived rather than exempted:
/// `contracts/lean-models/` is a different population with a different size,
/// and prose that names it is making a true claim about that lane, not a
/// false one about the pilot.
fn mathlib_lane_module_count() -> usize {
    lean_module_files("contracts/lean-models")
}

/// `*.lean` files under `dir` (recursively) that are not the build script.
fn lean_module_files(dir: &str) -> usize {
    fn walk(d: &std::path::Path, n: &mut usize) {
        let entries = fs::read_dir(d).unwrap_or_else(|e| panic!("read_dir {}: {e}", d.display()));
        for p in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
            if p.is_dir() {
                walk(&p, n);
            } else if p.extension().and_then(|s| s.to_str()) == Some("lean")
                && p.file_name().and_then(|s| s.to_str()) != Some("lakefile.lean")
            {
                *n += 1;
            }
        }
    }
    let mut n = 0;
    walk(&workspace_root().join(dir), &mut n);
    n
}

/// The `contracts/` tree — every contract YAML, every Lean source, every
/// inventory page.
///
/// PMAT-1452: `claim_pages()` is a MARKDOWN corpus (the book, `README.md`,
/// `CLAUDE.md`, `docs/status/INDEX.md`, `docs/specifications/**`). The
/// NORMATIVE artifacts — the contract YAMLs the entire provability claim
/// rests on, and the Lean sources that discharge them — had never been in ANY
/// claims-drift gate's subject, and all three live falsehoods this slice found
/// were sitting in them.
fn provable_artifact_pages() -> Vec<(String, String)> {
    let root = workspace_root();
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, String)>) {
        let entries =
            fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        paths.sort(); // deterministic order ⇒ stable failure message
        for p in paths {
            if p.is_dir() {
                walk(&p, root, out);
            } else if matches!(
                p.extension().and_then(|s| s.to_str()),
                Some("yaml") | Some("lean") | Some("md")
            ) {
                let rel = p
                    .strip_prefix(root)
                    .expect("contract artifact under workspace root")
                    .to_string_lossy()
                    .into_owned();
                let body = fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {rel}: {e}"));
                out.push((rel, body));
            }
        }
    }
    let mut out = Vec::new();
    walk(&root.join("contracts"), &root, &mut out);
    out
}

/// Every `(claimed_count, byte_offset)` in `text` whose HEAD NOUN is
/// `module`/`modules`.
///
/// This is the CLASS, not an enumeration of spellings — which is the whole
/// point. The gate this supplements pinned five hand-written
/// `counts_between(.., prefix, suffix)` anchors, and PMAT-1450 established
/// that a needle spelled one way is blind to the same claim spelled another.
/// So the scan runs the other direction: find the noun, then read the number
/// off it.
///
/// Two forms, because the corpus writes both:
///   * ATTRIBUTIVE — `35-module substrate`, `a (now 35-module) pilot`.
///   * HEAD NOUN — `35 modules`, `35 machine-checked modules`,
///     `35 lakefile-rooted modules` (up to three tokens of qualifier).
///
/// Two shapes are deliberately NOT counts, and each was a false positive
/// during measurement:
///   * `module` as a hyphenated MODIFIER (`all 3 CLI module-construction
///     sites`) — `module` is not the thing being counted.
///   * a digit run that is part of a larger token (`PMAT-361 (MetaHirModule
///     modules side)`, `size-0 modules`, `all 4-byte (module, config) pairs`)
///     — a bare-integer token is required, so an id or a unit cannot pose as
///     a count.
fn module_counts(text: &str) -> Vec<(usize, usize)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = text[from..].find("module") {
        let at = from + rel;
        from = at + "module".len(); // non-empty needle ⇒ strictly advances
        if at > 0 && (b[at - 1] as char).is_ascii_alphanumeric() {
            continue; // `MetaHirModule` — not the word
        }
        let mut end = at + "module".len();
        if text[end..].starts_with('s') {
            end += 1;
        }
        match text[end..].chars().next() {
            // `module-construction`, `module-level` — modifier, not head noun.
            Some(c) if c == '-' || c.is_ascii_alphanumeric() => continue,
            _ => {}
        }
        let mut claimed: Option<usize> = None;
        // ATTRIBUTIVE: digits hyphen-attached to the noun.
        if at > 0 && b[at - 1] == b'-' {
            let j = at - 1;
            let mut k = j;
            while k > 0 && b[k - 1].is_ascii_digit() {
                k -= 1;
            }
            if k < j && (k == 0 || !(b[k - 1] as char).is_ascii_alphanumeric()) {
                claimed = text[k..j].parse::<usize>().ok();
            }
        }
        // HEAD NOUN: the nearest bare-integer token within three.
        if claimed.is_none() {
            for tok in text[..at].split_whitespace().rev().take(3) {
                let t = tok.trim_matches(|c: char| !c.is_ascii_alphanumeric());
                if !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit()) {
                    claimed = t.parse().ok();
                    break;
                }
            }
        }
        if let Some(n) = claimed {
            out.push((n, at));
        }
    }
    out
}

/// The clause of `flat` containing byte offset `at`.
///
/// PMAT-1451: a claim is a CLAUSE, not a line and not a paragraph. It matters
/// here for the same reason it mattered there — `paragraphs_under_headings`
/// flattens a whole markdown TABLE into one paragraph, so scoping the
/// population lookup to the paragraph made `fable-architectural-review.md`'s
/// "Proofs are REAL: 35 modules" row inherit the `contracts/lean-models/` row
/// four lines below it and get judged against the Mathlib lane's size.
fn clause_bounds(flat: &str, at: usize) -> (usize, usize) {
    let (mut lo, mut hi) = (0usize, flat.len());
    for d in ["|", ". ", "; ", "? ", "! "] {
        if let Some(j) = flat[..at].rfind(d) {
            lo = lo.max(j + d.len());
        }
        if let Some(j) = flat[at..].find(d) {
            hi = hi.min(at + j);
        }
    }
    (lo, hi)
}

/// EVERY published present-tense size of the Lean proof pilot equals the live
/// `lakefile.lean` root count — anywhere in the corpus, in any spelling.
///
/// PMAT-1452 — the rule was already written down and enforced on TWO files in
/// SIX spellings. Both bounds were wrong, on the two independent axes PMAT-1450
/// named, and each blindness is established below by a control that PASSES:
///
///   * SUBJECT. `provability_inventory_module_counts_match_lakefile` reads
///     `contracts/lean/PROVABILITY-INVENTORY.md`; `roadmap_lean_module_count_
///     matches_lakefile` reads one `strategic_goals` clause. Nothing else in
///     the repository was checked, and `claim_pages()` — the corpus every
///     other claims-drift gate ranges over — is MARKDOWN ONLY, so
///     `contracts/**` has never been in any gate's subject at all. All three
///     live falsehoods were there.
///
///   * NEEDLE. The anchors are six literal `prefix<n>suffix` spellings. The
///     corpus writes the same claim as `the 23 modules that elaborate` and
///     `the whole 28-module pilot`, neither of which any anchor matches.
///
/// ⭐ THE SHARPEST SITE IS THE DERIVATION ITSELF. `contracts/lean/lakefile.lean`
/// opened with "PILOT = the 23 modules that elaborate clean under bare core"
/// twenty-six lines above a `roots := #[…]` array holding 35 — and
/// `lakefile_root_count()`, the ground truth BOTH older gates measure against,
/// parses that array out of that file and never reads the prose above it. The
/// authoritative file contradicted itself, and the half nothing enforced is
/// the half that was false. The other two sites, `PyExceptAllowlist.lean` and
/// its contract `py-except-allowlist-v1.yaml`, both said 28.
///
/// The three OTHER populations prose legitimately counts are DERIVED, not
/// exempted, because a carve-out that is not checked is a hole (PMAT-1451):
/// the KNOWN-INCOMPLETE remainder and the separate `contracts/lean-models/`
/// Mathlib lane each get their own denominator, and the test asserts every
/// population actually got dispatched — an unreachable arm is an unchecked
/// arm.
#[test]
fn lean_pilot_size_claims_match_the_lakefile() {
    let pilot = lakefile_root_count();
    let incomplete = known_incomplete_module_count();
    let mathlib = mathlib_lane_module_count();
    assert!(
        pilot >= 1 && mathlib >= 1,
        "derived populations look empty (pilot {pilot}, mathlib lane \
         {mathlib}) — a directory moved; fix the derivation, not the prose"
    );

    // Words that make a paragraph be ABOUT the Lean proof lane. Without this
    // the scan would range over every `module` in the repository (WASM
    // modules, Python modules, CPython extension modules).
    const LANE: [&str; 8] = [
        "pilot",
        "lakefile",
        "lake build",
        "machine-checked",
        "contracts/lean",
        "lean_proof",
        "elaborat",
        "substrate",
    ];

    let mut pages = claim_pages();
    let contract_pages = provable_artifact_pages();
    let contract_page_count = contract_pages.len();
    pages.extend(contract_pages);
    assert!(
        contract_page_count >= 30,
        "provable_artifact_pages() collected only {contract_page_count} file(s) \
         from contracts/ — the walk is not reaching the contract corpus, and \
         this gate's whole subject widening is vacuous"
    );

    let mut offences = Vec::new();
    let mut agree_in_contracts = 0usize;
    let mut agree_elsewhere = 0usize;
    let mut by_population: HashMap<&str, usize> = HashMap::new();
    let mut records = 0usize;

    for (rel, body) in &pages {
        for para in paragraphs_under_headings(body) {
            let lower = para.flat.to_ascii_lowercase();
            if !LANE.iter().any(|k| lower.contains(k)) {
                continue;
            }
            if para.heading.contains(HISTORICAL_MARKER) {
                continue;
            }
            let quoted = quoted_spans(&para.flat);
            for (claimed, at) in module_counts(&para.flat) {
                // Reporting a claim is not making one (PMAT-1450).
                if quoted.iter().any(|&(a, b)| a <= at && at < b) {
                    continue;
                }
                let (clo, chi) = clause_bounds(&para.flat, at);
                let clause_text = &para.flat[clo..chi];
                let clause = clause_text.to_ascii_lowercase();
                let (population, live) =
                    if clause.contains("lean-models") || clause.contains("mathlib lane") {
                        ("the separate Mathlib lane", mathlib)
                    } else if clause.contains("known-incomplete") {
                        ("the KNOWN-INCOMPLETE remainder", incomplete)
                    } else {
                        ("the lake pilot", pilot)
                    };
                if claimed == live {
                    *by_population.entry(population).or_default() += 1;
                    if rel.starts_with("contracts/") {
                        agree_in_contracts += 1;
                    } else {
                        agree_elsewhere += 1;
                    }
                    continue;
                }
                // A GROWTH RECORD (`22 -> 23 modules`, `→ an 11-module pilot`)
                // states what a named slice changed, not what is true now. It
                // must cite the slice, so the shape cannot be used to launder
                // a bare stale numeral into an exemption.
                //
                // BOTH halves are scoped to the CLAUSE, and the second half is
                // why: written against the PARAGRAPH, this exemption's own red
                // half came back GREEN. `audit-design.md`'s bullet opens
                // "(PMAT-903/904, 2026-06-24 sprint)" and carries the growth
                // record four sentences later, so stripping the citation FROM
                // THE RECORD still left a `PMAT-` in the paragraph and the
                // stale numeral stayed exempt. That is the same
                // paragraph-vs-clause defect PMAT-1450 fixed in the
                // `(historical record)` guard and PMAT-1451 fixed in the
                // quorum enumeration — third slice running in which the
                // correction restated the defect it was correcting.
                let back = &para.flat[clo..at];
                if (back.contains('→') || back.contains("->")) && clause_text.contains("PMAT-") {
                    records += 1;
                    continue;
                }
                let line = para
                    .lines
                    .iter()
                    .find(|(_, l)| l.contains(&format!("{claimed}")))
                    .map_or(para.start, |(n, _)| *n);
                offences.push(format!(
                    "{rel}:{line}: claims {claimed} for {population}, which \
                     currently has {live} — \"{}\"",
                    clause_text.trim()
                ));
            }
        }
    }

    assert!(
        offences.is_empty(),
        "published Lean-pilot size(s) do not match the live derivation \
         (lakefile roots {pilot}, KNOWN-INCOMPLETE {incomplete}, Mathlib lane \
         {mathlib}). Update the prose, or the lakefile, whichever is wrong:\n{}",
        offences.join("\n")
    );

    // ── anti-vacuity: the scan must actually be finding claims, on BOTH
    // halves of the widened subject and in EVERY population arm ──
    assert!(
        agree_in_contracts >= 1,
        "no matching pilot-size claim was found anywhere under contracts/, so \
         the half of the subject PMAT-1452 added is not being exercised and \
         this gate would stay green if the needle stopped working"
    );
    assert!(
        agree_elsewhere >= 1,
        "no matching pilot-size claim was found outside contracts/, so the \
         pre-existing markdown half of the subject is not being exercised"
    );
    for population in [
        "the lake pilot",
        "the KNOWN-INCOMPLETE remainder",
        "the separate Mathlib lane",
    ] {
        assert!(
            by_population.contains_key(population),
            "no claim was judged against `{population}`, so that arm is \
             unreachable — an unchecked arm is exactly the hole this gate \
             derives populations to avoid. Either the corpus stopped stating \
             it, or the clause needle no longer routes it."
        );
    }
    assert!(
        records >= 1,
        "no growth-record clause (`22 -> 23 modules`, `→ an 11-module pilot`) \
         was seen, so the one exemption this gate grants is unreachable and \
         untested. If the corpus really stopped carrying one, delete the \
         exemption rather than leaving it standing unexercised."
    );
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

/// `text` split into clauses at `". "` and `"; "`, each paired with its byte
/// offset in `text` so a match can be attributed back to the line that carries
/// it. The split characters are ASCII, so every boundary is a char boundary.
fn clauses_with_offsets(text: &str) -> Vec<(&str, usize)> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    while i + 1 < b.len() {
        if (b[i] == b'.' || b[i] == b';') && b[i + 1] == b' ' {
            out.push((&text[start..i], start));
            start = i + 2;
            i += 2;
            continue;
        }
        i += 1;
    }
    out.push((&text[start..], start));
    out
}

/// Contract ids mentioned in a clause: `C-` followed by upper-case ASCII,
/// digits and hyphens. Trailing hyphens are trimmed so `C-FOO-` in prose does
/// not become a distinct id, and a one-character tail (`C-X`) is rejected —
/// the shortest live id is `C-ENUM-TRANSLATION`.
fn contract_ids(clause: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let bytes: Vec<char> = clause.chars().collect();
    let mut i = 0usize;
    while i + 2 < bytes.len() {
        let boundary = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if boundary && bytes[i] == 'C' && bytes[i + 1] == '-' {
            let mut j = i + 2;
            while j < bytes.len()
                && (bytes[j].is_ascii_uppercase() || bytes[j].is_ascii_digit() || bytes[j] == '-')
            {
                j += 1;
            }
            let id: String = bytes[i..j].iter().collect();
            let id = id.trim_end_matches('-').to_string();
            if id.len() > 3 {
                ids.push(id);
            }
            i = j.max(i + 1);
            continue;
        }
        i += 1;
    }
    ids.sort();
    ids.dedup();
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

/// Every prose page whose subject is the system AS IT IS NOW: the book, the
/// canonical specification set, the status index, and the two root-level files
/// a reader or an agent session opens first.
///
/// PMAT-1450: the universal-depth gate's subject was `book_pages()` alone, and
/// the falsehood it exists to kill was published in `docs/specifications/` —
/// which `CLAUDE.md` names as the canonical design. A CLAIM CLASS IS NOT A
/// DIRECTORY (PMAT-1438), and the book is not the only publisher (PMAT-1447).
///
/// What is deliberately OUT, and why each is a record rather than a claim:
///   * `CHANGELOG.md` and `docs/status/2026-*.md` — dated release/status logs.
///     "v0.1.0 — 12 contracts at depth-13 UNIVERSAL" was true of v0.1.0 and
///     stays true of it. Same justification as the `book/src/changelog.md`
///     exemption below, which is checked structurally there.
///   * `docs/roadmaps/*.yaml` — work-item ledgers; every entry is scoped to the
///     slice that wrote it.
///   * `contracts/**` — per-Diamond provenance comments naming the PMAT id that
///     added that equation.
///
/// Prose that describes the CURRENT substrate belongs in the set above; if a
/// page moves out of it to dodge this gate, that is the drift, not the fix.
fn claim_pages() -> Vec<(String, String)> {
    let root = workspace_root();
    let mut out = book_pages();
    for rel in ["README.md", "CLAUDE.md", "docs/status/INDEX.md"] {
        let body = fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        out.push((rel.to_string(), body));
    }
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
                    .expect("spec page under workspace root")
                    .to_string_lossy()
                    .into_owned();
                let body = fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {rel}: {e}"));
                out.push((rel, body));
            }
        }
    }
    walk(&root.join("docs/specifications"), &root, &mut out);
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

/// PMAT-1451 — a `#` comment inside a fenced block is CODE, not a heading.
/// The corpus does not spell this shape today, so the rule cannot be
/// demonstrated from the corpus and is pinned here instead. Both directions
/// matter: the fence must not GRANT a `(historical record)` exemption to the
/// prose after it, and must not REVOKE the real heading either.
#[test]
fn a_fence_comment_is_not_a_heading() {
    let doc = "## Live section\n\n```python\n# helper (historical record)\nx = 1\n```\n\nall 12 contracts are at 100% QUORUM.\n";
    let paras = paragraphs_under_headings(doc);
    let claim = paras
        .iter()
        .find(|p| p.flat.contains("100% QUORUM"))
        .expect("the claim paragraph is in the split");
    assert_eq!(
        claim.heading, "## Live section",
        "a `#` comment inside a ``` fence was taken for an ATX heading, so the \
         fenced text decided which section the prose below it belongs to"
    );
    assert!(
        !claim.heading.contains(HISTORICAL_MARKER),
        "a fenced code comment granted the historical-record exemption"
    );
    // The control: outside a fence the very same line IS a heading, so the
    // fence check cannot pass by disabling heading detection altogether.
    let bare = paragraphs_under_headings("# helper (historical record)\n\nclaim.\n");
    assert_eq!(
        bare.last().expect("a paragraph").heading,
        "# helper (historical record)"
    );
}

/// The needle split is the half PMAT-1451 added, so pin both sides of it:
/// a substrate-totality phrase on a line naming no contract is drift; the
/// same phrase naming one contract is a per-contract claim.
#[test]
fn contract_ids_reads_the_spellings_the_corpus_uses() {
    assert_eq!(
        contract_ids("covered by `C-XPILE-FRONTEND-TRAIT` at full §14.4 QUORUM"),
        vec!["C-XPILE-FRONTEND-TRAIT"]
    );
    assert_eq!(
        contract_ids("asserts C-PY-INT-ARITH has full quorum"),
        vec!["C-PY-INT-ARITH"]
    );
    // Two on one line — `meta-hir.md:111` names both trait contracts.
    assert_eq!(
        contract_ids("`C-XPILE-FRONTEND-TRAIT`; same via `C-XPILE-BACKEND-TRAIT`"),
        vec!["C-XPILE-BACKEND-TRAIT", "C-XPILE-FRONTEND-TRAIT"]
    );
    // Prose that merely contains a capital C is not an id.
    assert!(contract_ids("C code, and a C-like syntax").is_empty());
    assert!(contract_ids("all 12 contracts reach QUORUM").is_empty());
}

/// The live §14.4 table, read from the shipped binary: the `totals:` line and
/// every contract's status keyed by id.
///
/// PMAT-1451: the per-contract half exists so a carve-out can be a CHECK. A
/// line naming `C-FOO` and saying "at full §14.4 QUORUM" is not a claim about
/// the substrate and must not be scored as one — but it IS a claim, so it is
/// verified against this map rather than waved past.
fn live_quorum() -> (String, HashMap<String, String>) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_xpile"))
        .args(["quorum", "--contracts-dir"])
        .arg(workspace_root().join("contracts"))
        .current_dir(workspace_root())
        .output()
        .expect("run xpile quorum");
    assert!(
        out.status.success(),
        "`xpile quorum` failed ({}), so the live §14.4 table could not be \
         derived and every gate over it would be vacuous. stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let report = String::from_utf8(out.stdout).expect("utf-8 quorum report");
    let totals = report
        .lines()
        .find(|l| l.trim_start().starts_with("totals:"))
        .unwrap_or_else(|| panic!("`xpile quorum` printed no totals line:\n{report}"))
        .trim()
        .to_string();
    let mut status = HashMap::new();
    for line in report.lines() {
        let toks: Vec<&str> = line.split_whitespace().collect();
        let (Some(id), Some(last)) = (toks.first(), toks.last()) else {
            continue;
        };
        if id.starts_with("C-") && matches!(*last, "QUORUM" | "PARTIAL" | "UNVERIFIED") {
            status.insert((*id).to_string(), (*last).to_string());
        }
    }
    // Anti-vacuity, as a RELATION between two things the same report prints:
    // the row count must equal the `(N contracts total)` the totals line
    // states. A parser that silently drops rows would otherwise make every
    // per-contract claim below pass for want of an entry.
    let declared = counts_between(&totals, "(", " contracts total)")
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no `(N contracts total)` in totals line {totals:?}"));
    assert_eq!(
        status.len(),
        declared,
        "parsed {} contract row(s) from `xpile quorum` but its own totals line \
         declares {declared}. The table shape moved; fix this parser rather \
         than letting per-contract claims pass on a missing entry.\n{report}",
        status.len()
    );
    (totals, status)
}

/// Phrases that assert TOTAL §14.4 discharge across the whole substrate. Each
/// was live in the corpus while PARTIAL was non-zero.
const TOTAL_QUORUM_CLAIMS: [&str; 5] = [
    "full quorum",
    "100% QUORUM",
    "100% §14.4",
    "all at QUORUM",
    "0 PARTIAL",
];

/// Spellings that assert a NAMED contract has reached quorum.
const PER_CONTRACT_QUORUM_CLAIMS: [&str; 5] = [
    "at full §14.4 QUORUM",
    "at full QUORUM",
    "has full quorum",
    "at QUORUM",
    "reaches QUORUM",
];

/// Docs that say "full quorum" while contracts sit at PARTIAL make the one
/// claim this repository exists to be trustworthy about. Both sides derived:
/// the PARTIAL population from `xpile quorum` over the contract YAMLs, the
/// claim from the prose.
///
/// PMAT-1451 — WHAT THIS GATE COULD NOT SEE, on the same two independent axes
/// PMAT-1450 found for the universal-depth gate one slice earlier:
///
///   * SUBJECT. It ranged over `book_pages()`. Live: 26 QUORUM, **9 PARTIAL**,
///     0 UNVERIFIED over 35 contracts — and five assertions of TOTAL discharge
///     sat outside the book, four of them in the canonical specification set.
///     The worst is `xpile-spec.md`'s "`xpile quorum` → 16 (or 15) QUORUM,
///     **0 PARTIAL**, 0 UNVERIFIED" under `### Exit criterion` — a pasted
///     derived transcript, the exact shape
///     `book_pastes_no_derived_totals_transcript` forbids in the book, sitting
///     1269 lines BELOW the same file's own `**Status:**` line, which says
///     coverage is "**partial, not total**". The canonical spec contradicted
///     itself, and the honest half is the one nothing enforced.
///
///   * NEEDLE, in the FALSE-POSITIVE direction — the axis the previous slices
///     did not hit. `"full quorum"` also matches
///     `sub/provability-roadmap.md`'s "`tests/quorum.rs` asserts
///     C-PY-INT-ARITH has full quorum", which is a claim about ONE contract
///     and is TRUE. Widening the subject without splitting the needle would
///     have reported a true sentence as drift. A carve-out that is not checked
///     is a hole, so per-contract claims are verified against the live table
///     instead of exempted.
#[test]
fn docs_claim_no_total_quorum_while_any_contract_is_partial() {
    let (totals, status) = live_quorum();
    let partial = counts_between(&totals, ", ", " PARTIAL")
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("could not parse a PARTIAL count from {totals:?}"));

    let mut offences = Vec::new();
    let mut per_contract = Vec::new();
    let mut scanned = 0usize;
    let mut historical = Vec::new();
    let mut undated = Vec::new();
    for (rel, body) in claim_pages() {
        // `book/src/changelog.md` is a DATED release log — the same role
        // `CHANGELOG.md` and `docs/status/2026-*.md` play, which is why
        // `claim_pages()` excludes those outright. "v0.1.0 — 12 contracts at
        // 100% QUORUM" was true of v0.1.0 and stays true of it. Exempt by
        // ROLE, and checked to really be that role, so it cannot quietly
        // widen into "the book is exempt".
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
        for para in paragraphs_under_headings(&body) {
            let in_record = para.heading.contains(HISTORICAL_MARKER);
            // Scanned over the FLATTENED paragraph, with byte offsets
            // attributed back to the line that carries them. A claim — and a
            // quotation of one — WRAPS across a line break: the repair written
            // for `xpile-spec.md` opens its backtick on one line and closes it
            // on the next, so a line-scoped span scan sees one lone backtick,
            // finds no closed span, and reports the quotation as an assertion.
            // Same reason `docs_claim_no_universal_depth_the_substrate_does_not_hold`
            // works over `flat`.
            let spans = quoted_spans(&para.flat);
            let lower = para.flat.to_lowercase();
            // Paragraph-scoped denial, per PMAT-1448. It is NOT sufficient on
            // its own — see the quoted-span requirement below.
            let denied = lower.contains("used to")
                || lower.contains("no longer")
                || lower.contains("this line said")
                || lower.contains("does not hold")
                || lower.contains("was false");
            // Scanned per CLAUSE. A claim is a clause — not a line, and not a
            // paragraph. PMAT-1450 moved this family from paragraph to line
            // because a 20-bullet paragraph let a neighbour whitewash a claim;
            // the same argument goes one level finer here, and this gate's own
            // first run proved it: `sub/provability-roadmap.md:103` is a
            // single run-on line carrying BOTH "`tests/quorum.rs` asserts
            // C-PY-INT-ARITH has full quorum" (a TRUE per-contract claim) and,
            // 500 characters earlier, the threshold definition "… = PARTIAL,
            // 0 = UNVERIFIED". A line-scoped enumeration test saw the word
            // PARTIAL, declined to treat the line as a per-contract claim, and
            // reported a true sentence as substrate drift.
            for (clause, base) in clauses_with_offsets(&para.flat) {
                // A clause naming contracts is a PER-CONTRACT claim, checked
                // against the live table rather than against substrate
                // totality. `PARTIAL`/`UNVERIFIED` in the clause makes it an
                // enumeration of the possible states ("is at QUORUM, PARTIAL,
                // or …"), and an arrow makes it a transition ("Bronze→QUORUM")
                // — neither asserts a current state.
                let ids = contract_ids(clause);
                let asserts_per_contract = PER_CONTRACT_QUORUM_CLAIMS
                    .iter()
                    .any(|c| clause.contains(c))
                    && !clause.contains("PARTIAL")
                    && !clause.contains("UNVERIFIED")
                    && !clause.contains("→");
                if !ids.is_empty() && asserts_per_contract {
                    let (n, _) = para.line_at(base);
                    for id in &ids {
                        match status.get(id) {
                            Some(s) if s == "QUORUM" => per_contract.push(format!("{rel}:{n}")),
                            Some(s) => offences
                                .push(format!("{rel}:{n}: says `{id}` is at QUORUM; it is {s}")),
                            // An id the live table does not carry is a separate
                            // defect class (PMAT-1435) and is not this gate's
                            // subject — but it must not read as a satisfied
                            // claim either.
                            None => {}
                        }
                    }
                    continue;
                }
                for b in TOTAL_QUORUM_CLAIMS {
                    let mut from = 0usize;
                    while let Some(hit) = clause[from..].find(b) {
                        let at = base + from + hit;
                        from += hit + b.len();
                        let (n, line) = para.line_at(at);
                        if in_record {
                            // A dated record names WHEN. Checked on the
                            // claim's OWN LINE, not its paragraph: these
                            // sections are 20+ consecutive bullets in one
                            // paragraph, so a paragraph-scoped check is
                            // satisfied by a neighbour and bites nothing
                            // (PMAT-1450 measured exactly that).
                            //
                            // COLLECTED, not asserted in place: an `assert!`
                            // here aborts the walk at the first undated record
                            // and hides every substrate-totality offence below
                            // it — which is exactly what it did on this gate's
                            // own first run, reporting one site of six.
                            if pmat_ids(line).is_empty() && !line.contains("v0.1.") {
                                undated.push(format!("{rel}:{n}: `{b}` under {:?}", para.heading));
                            } else {
                                historical.push(format!("{rel}:{n}"));
                            }
                            continue;
                        }
                        if partial == 0 {
                            continue; // the claim would be true
                        }
                        // Prose may QUOTE a retired falsehood; it may not
                        // ASSERT one. Same structural rule PMAT-1450 settled
                        // for the universal-depth gate, and this slice needed
                        // it for the same reason: the repair written for
                        // `xpile-spec.md` records what that line USED to pin,
                        // and the retired numerals include `0 PARTIAL`. The red
                        // half caught the correction reproducing the defect
                        // (PMAT-1447's shape) on its first otherwise-green run.
                        //
                        // BOTH halves are required. Denial alone is
                        // paragraph-scoped and would exempt anything newly
                        // asserted in the same paragraph; the quoted-span
                        // requirement is what makes it a quotation.
                        let quoted = spans.iter().any(|&(s, e)| at >= s && at + b.len() <= e);
                        if denied && quoted {
                            historical.push(format!("{rel}:{n}"));
                            continue;
                        }
                        offences.push(format!("{rel}:{n}: `{b}`"));
                    }
                }
            }
        }
    }
    assert!(
        scanned > 1,
        "the exemption swallowed the corpus: only {scanned} page(s) scanned"
    );
    // Both derived sets must be non-empty, or a later narrowing of either
    // needle would leave this ranging over nothing and passing for free
    // (PMAT-1396). `C-PY-INT-ARITH` is the anchor: the substrate's oldest
    // contract, claimed at quorum by name in `sub/provability-roadmap.md`.
    assert!(
        !per_contract.is_empty(),
        "no per-contract QUORUM claim was found anywhere in the corpus, so \
         the PER_CONTRACT_QUORUM_CLAIMS needle is matching nothing and every \
         such claim is now unchecked"
    );
    // The live falsehoods first — they are the defect this gate exists for.
    // The undated-record check below is a hygiene rule on the EXEMPTION, and
    // reporting it ahead of a false published claim would bury the headline.
    assert!(
        offences.is_empty(),
        "the docs claim TOTAL §14.4 quorum, or claim a named contract is at \
         QUORUM when it is not — {} — but `xpile quorum` reports {partial} \
         contract(s) at PARTIAL. Totals line: {totals}. Say which SUBSET is \
         discharged, or move the sentence under a `{HISTORICAL_MARKER}` \
         heading if it is a dated record. ({} mention(s) already are.)",
        offences.join(", "),
        historical.len()
    );
    assert!(
        undated.is_empty(),
        "totality claim(s) sit under a `{HISTORICAL_MARKER}` heading without \
         citing a PMAT id or a version on their own line — {}. A record of \
         what was once true says WHEN; otherwise write it as a live claim and \
         make it true.",
        undated.join(", ")
    );
    assert!(
        !historical.is_empty(),
        "no `{HISTORICAL_MARKER}` totality claim was found, so that branch is \
         dead and the exemption is untested"
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
///
/// PMAT-1450 — WHAT THIS GATE COULD NOT SEE, on two independent axes:
///   * SUBJECT. It ranged over `book_pages()`. The canonical specification set
///     — the documents `CLAUDE.md` names as the design of record — was not in
///     it, and carried 73 instances: `xpile-spec.md` §23's status list
///     ("Eleven UNIVERSAL Diamond milestones depth-3..13 … 171 wired Diamond
///     theorems across 12 contracts", under a ✅), §28's coverage table (12
///     rows of "12/12 contracts (UNIVERSAL)"), and
///     `sub/diamond-taxonomy.md`'s "CI enforcement" section, which listed
///     twelve UNIVERSAL assertions as things the gate enforces.
///   * SPELLING. `xpile-spec.md`'s description of the CI gate is written
///     `depth-1/2/3/4/5/6/7/8/9/10/11/12/13 UNIVERSAL (all 12 contracts)`,
///     which the PMAT-1448 parser scored at ZERO claims — the same failure
///     PMAT-1448 was itself written to repair, in a third spelling.
///
/// The published claims were false three ways at once: the substrate is 35
/// contracts, not 12; its live universal depth is 1 (21 contracts carry a
/// single Diamond); and PMAT-475 REPLACED the aggregate depth-2..13 gates with
/// a floor over a named 13-contract cohort, so no gate has enforced depth-2
/// universally since. PMAT-1448 corrected `diamond_coverage.rs`'s own module
/// header to say exactly that — and left both published descriptions of that
/// same file untouched, 130 and 500 lines away in other trees. A fix scoped to
/// the site carries the class forward.
#[test]
fn docs_claim_no_universal_depth_the_substrate_does_not_hold() {
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

    // PMAT-1448 — NON-VACUITY, and the reason this gate needed repairing at
    // all: the spelling quoted in the doc comment above as THE defect must be
    // one the parser can actually match. It was not. The old scan was
    // `counts_between(line, "depth-", " UNIVERSAL")`, which requires the digits
    // to be followed immediately by ` UNIVERSAL`; in `depth-1..13 UNIVERSAL`
    // they are followed by `..13`, so the gate scored ZERO claims on the exact
    // string it was written to kill, and a live instance sat in
    // `book/src/reference/cli.md` — four lines above a link to the page this
    // gate protects. Ask what the DEFECT spelled, not what the fix spells.
    assert_eq!(
        claimed_universal_depths("depth-1..13 UNIVERSAL"),
        vec![13],
        "the RANGE spelling must be matched, and must claim its UPPER end"
    );
    assert_eq!(claimed_universal_depths("depth-7 UNIVERSAL"), vec![7]);
    assert_eq!(claimed_universal_depths("depth-13+ UNIVERSAL"), vec![13]);
    assert!(
        claimed_universal_depths("depth-13 coverage over the core").is_empty(),
        "`depth-N` without the word UNIVERSAL is not a universality claim"
    );
    // PMAT-1450 — the SLASH-LIST spelling, verbatim from the line in
    // `xpile-spec.md` §29 that this repair was written against. The PMAT-1448
    // parser returned `[]` here: only the trailing `13` precedes ` UNIVERSAL`,
    // and `depth-` precedes the `1`.
    assert_eq!(
        claimed_universal_depths(
            "depth-1/2/3/4/5/6/7/8/9/10/11/12/13 UNIVERSAL (all 12 contracts)"
        ),
        vec![13],
        "the SLASH-LIST spelling must be matched, and must claim its LARGEST member"
    );
    assert_eq!(claimed_universal_depths("depth-3/1/2 UNIVERSAL"), vec![3]);
    assert!(
        claimed_universal_depths("depth-1/2/3 categories per contract").is_empty(),
        "a slash list without the word UNIVERSAL is not a universality claim"
    );

    // A claim of the form "depth-N UNIVERSAL", or the range spelling
    // "depth-A..B UNIVERSAL", for a depth the substrate does not hold.
    // Scanned per PARAGRAPH, not per line, for two reasons: a claim and its
    // denial live a paragraph apart, and the disclosure on the Diamond page
    // WRAPS between `depth-1..13` and `UNIVERSAL`, so a line-oriented scan
    // cannot see it at all and a reflow would silently change the verdict.
    let mut offences = Vec::new();
    let mut undated_mentions = 0usize;
    let mut historical = Vec::new();
    let mut pages_scanned = 0usize;
    for (rel, body) in claim_pages() {
        pages_scanned += 1;
        for para in paragraphs_under_headings(&body) {
            let in_record = para.heading.contains(HISTORICAL_MARKER);
            let spans = quoted_spans(&para.flat);
            for (claimed, at) in claimed_universal_depths_at(&para.flat) {
                let (line_no, line) = para.line_at(at);
                if in_record {
                    // A dated record names the slice it records. Checked on the
                    // claim's OWN LINE, not its paragraph: these lists are 20+
                    // consecutive bullets in ONE paragraph, so a paragraph-scoped
                    // check is satisfied by a neighbour and bites nothing. Its
                    // red half proved exactly that before it was tightened.
                    assert!(
                        !pmat_ids(line).is_empty(),
                        "{rel}:{line_no}: sits under a `{HISTORICAL_MARKER}` heading \
                         ({:?}) and claims depth-{claimed} UNIVERSAL without citing a \
                         PMAT id on its own line. A record of what was once true says \
                         WHEN; otherwise write it as a live claim and make it true.\n\
                         line: {line}",
                        para.heading
                    );
                    historical.push(format!("{rel}:{line_no}"));
                    continue;
                }
                undated_mentions += 1;
                if claimed <= universal {
                    continue;
                }
                // A mention is honest iff its own paragraph marks the claim as
                // superseded AND the claim itself sits inside a quoted span.
                // Prose may QUOTE a falsehood — the Diamond page does, on
                // purpose — but it may not assert one, and PMAT-1450 found that
                // the denial phrase alone exempted the whole paragraph
                // including anything newly asserted in it.
                let lower = para.flat.to_lowercase();
                let denied = lower.contains("this page said")
                    || lower.contains("used to")
                    || lower.contains("no longer")
                    || lower.contains("stopped describing")
                    || lower.contains("does not hold");
                let quoted = spans.iter().any(|&(s, e)| at >= s && at < e);
                if !(denied && quoted) {
                    offences.push(format!("{rel}:{line_no}: claims depth-{claimed} UNIVERSAL"));
                }
            }
        }
    }
    assert!(
        pages_scanned > 1,
        "claim_pages() returned {pages_scanned} page(s) — the corpus walk broke"
    );
    assert!(
        undated_mentions > 0,
        "every `depth-N UNIVERSAL` mention in the corpus now sits under a \
         `{HISTORICAL_MARKER}` heading ({} of them), so this gate is ranging over \
         nothing (PMAT-1396: a negative over an empty enumeration passes for free). \
         The marker has swallowed the subject — a live statement of the substrate's \
         actual universal depth belongs somewhere in the docs.",
        historical.len()
    );
    assert!(
        offences.is_empty(),
        "the docs claim a UNIVERSAL depth the substrate does not hold — {} — \
         but the shallowest contract carries {universal} Diamond \
         categor{}, so depth-{universal} is the live universal depth. Say \
         which SUBSET is deep, lift the shallow contracts, or move the sentence \
         under a `{HISTORICAL_MARKER}` heading if it is a dated record. \
         ({} mention(s) already are.)",
        offences.join(", "),
        if universal == 1 { "y" } else { "ies" },
        historical.len()
    );
}
