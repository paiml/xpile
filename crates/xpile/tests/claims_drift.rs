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
    // ASCII-only lowering, so every byte offset below indexes `text` too:
    // `str::to_ascii_lowercase` rewrites `A..=Z` and copies every other byte
    // through, including the multi-byte `≥` and `—` this corpus is full of.
    // (`str::to_lowercase` is NOT length-preserving — 'İ' becomes two chars —
    // which would slide every offset and misattribute the reported line.)
    let lower = text.to_ascii_lowercase();
    debug_assert_eq!(lower.len(), text.len(), "ASCII lowering must preserve len");
    let mut out = label_form_depths(&lower);
    out.extend(definition_form_depths(&lower));
    out.sort_unstable_by_key(|&(_, at)| at);
    out
}

/// The LABEL form: `depth-N UNIVERSAL`, in any of the four spellings the corpus
/// writes, matched case-insensitively.
///
/// PMAT-1454 — the CASE blindness. `contracts/**` shouts its milestones:
/// `COMPLETES DEPTH-5 UNIVERSAL ACROSS ALL 12 CONTRACTS`. The prefix was
/// matched as the literal lower-case `depth-`, so fifty claims in the
/// normative substrate were invisible to a parser that had already been
/// repaired twice for spelling (PMAT-1448 the range, PMAT-1450 the slash
/// list). Measured control: the pre-PMAT-1454 parser returns `[]` for the
/// upper-case line and this one returns `[5]`. Third spelling axis, third
/// slice running — which is why the definition form below is matched too,
/// rather than waiting for a fourth.
fn label_form_depths(lower: &str) -> Vec<(usize, usize)> {
    const PREFIX: &str = "depth-";
    let text = lower;
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
        // `lower` is already ASCII-lowered by the caller. The qualifier sits
        // AFTER the depth in every spelling PMAT-1448/1450 repaired…
        if text[i..].trim_start().starts_with("universal") {
            out.push((claimed, at));
            continue;
        }
        // …and BEFORE it in `UNIVERSAL Diamond depth-3`, which the substrate
        // writes too — including in one line of `xpile-spec.md` and one of
        // `audit-design.md`, so this spelling was blind in the MARKDOWN half
        // as well as the new one.
        //
        // The qualifier must be ADJACENT (bar the noun it qualifies), and that
        // is measured, not stylistic. Allowing any run of lower-case words
        // between them — the first thing this tried — matched two corpus
        // sentences that assert nothing about depth: the ratchet POLICY
        // "Diamond-depth UNIVERSAL ratchet is frozen at depth-13" and
        // "further UNIVERSAL broadening sweeps beyond depth-13", both of which
        // say what NOT to do. Reporting a policy as a false claim is how a
        // gate becomes un-shippable, and every needle fails in both
        // directions (PMAT-1451). Both shapes are pinned below.
        let head = &text[..at];
        if let Some(k) = head.rfind("universal") {
            let between = &head[k + "universal".len()..];
            if between.is_empty() || between == " diamond " {
                out.push((claimed, at));
            }
        }
    }
    out
}

/// The DEFINITION form: `every contract has ≥N distinct Diamond categories`.
///
/// PMAT-1454 — the blindness that matters most, because it needs no
/// misspelling at all. `sub/diamond-taxonomy.md` DEFINES depth-N UNIVERSAL as
/// "*every* contract has at least N distinct Diamond theorem categories", and
/// the substrate then asserts the claim **in the words of that definition**,
/// with no `depth-` token anywhere in the sentence:
///
/// ```text
///   invariants:
///     - "Substrate milestone: every contract has ≥5 distinct Diamond categories"
/// ```
///
/// A needle keyed on the LABEL cannot see the DEFINITION the label expands to.
/// Twelve such claims were live, up to `≥13`, against a substrate whose live
/// universal depth is 1 — and unlike the label form they carry no marker a
/// reader could grep for either.
///
/// Two guards, each pinned by a unit test in both directions because a
/// carve-out that is not checked is a hole (PMAT-1451):
///   * the clause must be ABOUT Diamond categories, so `every contract has ≥2
///     references` is not a depth claim;
///   * the quantifier must sit within the subject's own clause, so a `≥N`
///     belonging to a later sentence cannot be read onto `every contract`.
fn definition_form_depths(lower: &str) -> Vec<(usize, usize)> {
    const SUBJECT: &str = "every contract";
    const QUANTIFIERS: [&str; 3] = ["≥", ">=", "at least "];
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find(SUBJECT) {
        let at = from + rel;
        from = at + SUBJECT.len(); // non-empty needle ⇒ strictly advances
        let (clo, chi) = clause_bounds(lower, at);
        let clause = &lower[clo..chi];
        if !clause.contains("diamond") {
            continue;
        }
        // Nearest quantifier at or after the subject, inside the same clause.
        let tail = &lower[from..chi.max(from)];
        let Some((qat, q)) = QUANTIFIERS
            .iter()
            .filter_map(|q| tail.find(q).map(|i| (i, *q)))
            .min_by_key(|&(i, _)| i)
        else {
            continue;
        };
        let ds = from + qat + q.len();
        let digits: String = lower[ds..]
            .trim_start_matches(' ')
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if let Ok(n) = digits.parse::<usize>() {
            out.push((n, at));
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

/// The `contracts/` tree — every contract YAML, every Lean source, every Kani
/// harness, every inventory page.
///
/// PMAT-1452: `claim_pages()` is a MARKDOWN corpus (the book, `README.md`,
/// `CLAUDE.md`, `docs/status/INDEX.md`, `docs/specifications/**`). The
/// NORMATIVE artifacts — the contract YAMLs the entire provability claim
/// rests on, and the Lean sources that discharge them — had never been in ANY
/// claims-drift gate's subject, and all three live falsehoods this slice found
/// were sitting in them.
///
/// PMAT-1456 — AND `.rs` WAS STILL MISSING, which is a subject blindness the
/// repository had already written the rule for. [`Block`]'s own doc comment,
/// 30 lines below, says "`.lean`/`.rs`/`.md` under `contracts/` are prose
/// throughout": the TEXT MODEL names `.rs`, and this walk never handed it one.
/// Measured at the time: 41 `.lean`, 35 `.yaml` and 3 `.md` in the subject,
/// and **24 `.rs` outside it** — the files that carry all 95 `#[kani::proof]`
/// harnesses, i.e. the SYMBOLIC stratum's own evidence. Four assertions of
/// TOTAL §14.4 quorum were living there, in the one artifact kind whose
/// absence from the substrate is what puts contracts at PARTIAL.
///
/// Widening an artifact kind means auditing the text model for it too, so
/// [`substrate_blocks`] learned to strip `//!`/`///` here — without that, a
/// claim that WRAPS across two doc-comment lines gets the marker spliced into
/// the middle of it and matches nothing. That was not hypothetical: it is
/// exactly how `contracts/kani/ffi_cpython_ext.rs`'s "every / contract in
/// xpile's substrate is now at QUORUM" hid.
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
                Some("yaml") | Some("lean") | Some("md") | Some("rs")
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

/// One block of a `contracts/**` artifact: a maximal run of lines of the same
/// KIND, flattened, with each line's offset kept so a match can be attributed
/// back to it.
///
/// PMAT-1454 — `paragraphs_under_headings` cannot be used here, and the reason
/// is worth writing down because it looked like it could. It treats any line
/// whose first non-space character is `#` as an ATX heading, and in YAML that
/// is EVERY comment. So a four-line provenance comment became four one-line
/// "headings", which breaks the record in both directions: a claim that WRAPS
/// (`… COMPLETES DEPTH-13` / `UNIVERSAL ACROSS ALL 12 CONTRACTS`) is split
/// across two paragraphs and matches neither, and the `# PMAT-333:` that opens
/// the block falls outside the scope of the claim it cites, so a correctly
/// cited record reads as uncited. Measured: 36 sites reported as uncited that
/// are all, in fact, cited by their own block header.
///
/// The block is the record unit for these artifacts, exactly as the
/// `(historical record)` SECTION is for markdown — and it is a tighter scope
/// than the file, so a citation cannot be borrowed from an unrelated equation.
struct Block {
    /// True if this is provenance (a comment run) rather than a normative
    /// field run. `.lean`/`.rs`/`.md` under `contracts/` are prose throughout.
    record: bool,
    flat: String,
    /// `(offset in `flat`, 1-based line number)`, ascending.
    marks: Vec<(usize, usize)>,
}

impl Block {
    /// The 1-based line number containing byte offset `at` in [`Self::flat`].
    fn line_at(&self, at: usize) -> usize {
        self.marks
            .iter()
            .rev()
            .find(|&&(off, _)| off <= at)
            .map_or(0, |&(_, n)| n)
    }
}

/// Split a `contracts/**` artifact into [`Block`]s.
///
/// YAML alternates between provenance (`#` runs) and normative fields; every
/// other artifact kind under `contracts/` is prose, so its blocks are simply
/// its non-blank runs. Comment markers are stripped when flattening, or a
/// claim that wraps mid-phrase would have a `#` spliced into it and stop
/// matching — which is precisely how the wrapped `DEPTH-13 / UNIVERSAL` sites
/// hid.
///
/// PMAT-1456 — the same rule, one artifact kind over. `.rs` under
/// `contracts/` is a Kani harness: prose lives in `//!` module docs and `///`
/// item docs, and the SAME wrap defeats the SAME needle. Measured on
/// `contracts/kani/ffi_cpython_ext.rs`: stripping the marker takes the blocks
/// containing `every contract` from 1 to 2, and the one that appears is the
/// live falsehood ("every / contract in xpile's substrate is now at QUORUM").
/// `//!` is tried before `///` before `//`, or the two-slash arm would eat the
/// first two characters and leave a stray `!`/`/` at the head of the phrase.
fn substrate_blocks(rel: &str, body: &str) -> Vec<Block> {
    let yaml = rel.ends_with(".yaml");
    let rust = rel.ends_with(".rs");
    let mut out: Vec<Block> = Vec::new();
    let mut cur: Option<Block> = None;
    // Lean `/-! … -/` module docs and `/-- … -/` theorem docs are ONE record
    // that spans blank lines, and the citation sits in the block's `##`
    // header. Splitting on a blank line separates the milestone sentence from
    // the `## PMAT-354 —` that dates it, and ten correctly-cited records read
    // as uncited. Counted rather than boolean because Lean block comments
    // nest.
    let mut doc_depth = 0usize;
    for (i, raw) in body.lines().enumerate() {
        let t = raw.trim();
        let opens = t.matches("/-").count();
        let closes = t.matches("-/").count();
        if t.is_empty() {
            if doc_depth == 0 {
                out.extend(cur.take());
            }
            continue;
        }
        doc_depth = (doc_depth + opens).saturating_sub(closes);
        // In YAML a comment is provenance and anything else is an assertion.
        // Elsewhere under contracts/ there is no assertion syntax at all.
        let record = !yaml || t.starts_with('#');
        let text = if yaml && record {
            t.trim_start_matches('#').trim_start()
        } else if rust {
            ["//!", "///", "//"]
                .iter()
                .find_map(|m| t.strip_prefix(m))
                .unwrap_or(t)
                .trim_start()
        } else {
            t
        };
        let start_new = cur.as_ref().is_none_or(|b| b.record != record);
        if start_new {
            out.extend(cur.take());
            cur = Some(Block {
                record,
                flat: String::new(),
                marks: Vec::new(),
            });
        }
        let b = cur.as_mut().expect("just set");
        if !b.flat.is_empty() {
            b.flat.push(' ');
        }
        b.marks.push((b.flat.len(), i + 1));
        b.flat.push_str(text);
    }
    out.extend(cur);
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
    // XPILE-SKIPGUARD-003 (PMAT-1509): `strategic_goals_block` SELECTS — it
    // accumulates only the lines between the `strategic_goals:` key and the
    // next `roadmap:` key. Rename or reorder either and it returns `""`, whose
    // `.split(';')` yields one empty clause, `shouts_complete("")` is false, and
    // this test passes having examined no claim. Measured 2026-07-31: 80 clauses.
    assert!(
        !block.is_empty(),
        "docs/roadmaps/roadmap.yaml yielded an EMPTY strategic_goals block. The \
         extractor keys on a literal `strategic_goals:` line terminated by a \
         `roadmap:` line; if either key moved, every COMPLETE-claim check below \
         silently stops running while this test still prints `ok`."
    );
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
/// The substrate splitter, and the citation branch that today's corpus cannot
/// demonstrate.
///
/// PMAT-1454: the record-citation rule in
/// `docs_claim_no_universal_depth_the_substrate_does_not_hold` never fires on
/// the live corpus — every provenance block that carries a milestone also
/// carries an id. A rule a corpus cannot exercise has to be pinned somewhere
/// or it decays into a comment (PMAT-1451 shipped its fence-awareness rule the
/// same way, for the same reason). So the two dispositions are demonstrated
/// here on synthetic input instead.
#[test]
fn an_uncited_provenance_block_is_reported() {
    // A YAML comment run is ONE record and a field run is ONE assertion, and
    // the split is on the comment marker, not on the blank line.
    let yaml = "  # PMAT-354: FIFTH Diamond — COMPLETES DEPTH-5\n  \
                # UNIVERSAL ACROSS ALL 12 CONTRACTS.\n  \
                invariants:\n  \
                - \"Substrate milestone: every contract has ≥5 Diamond categories\"\n";
    let blocks = substrate_blocks("contracts/x-v1.yaml", yaml);
    assert_eq!(blocks.len(), 2, "one comment run, one field run");
    assert!(blocks[0].record && !blocks[1].record);

    // The claim WRAPS across two comment lines. Flattening has to strip the
    // second `#`, or `DEPTH-5` and `UNIVERSAL` end up with a `#` between them
    // and the needle sees nothing — which is how these hid.
    assert_eq!(
        claimed_universal_depths(&blocks[0].flat),
        vec![5],
        "a claim wrapped across two comment lines must still be seen"
    );
    assert_eq!(
        blocks[0].line_at(0),
        1,
        "attribution back to the source line"
    );
    assert!(
        !pmat_ids(&blocks[0].flat).is_empty(),
        "this record is cited"
    );
    // …and the assertion half is found in the field run, by its DEFINITION
    // spelling, with no `depth-` token anywhere in it.
    assert_eq!(claimed_universal_depths(&blocks[1].flat), vec![5]);

    // THE BRANCH THE CORPUS CANNOT REACH: same record, no id.
    let uncited = substrate_blocks(
        "contracts/x-v1.yaml",
        "  # FIFTH Diamond — COMPLETES DEPTH-5 UNIVERSAL ACROSS ALL 12.\n",
    );
    assert_eq!(uncited.len(), 1);
    assert_eq!(claimed_universal_depths(&uncited[0].flat), vec![5]);
    assert!(
        pmat_ids(&uncited[0].flat).is_empty(),
        "an uncited milestone record must be reported, or the exemption is \
         unconditional and the gate's record branch is decoration"
    );

    // A Lean `/-! … -/` docstring spans blank lines: one record, not three.
    let lean = "/-! ## PMAT-354 — FIFTH Diamond\n\n\
                **SUBSTRATE MILESTONE: DEPTH-5 UNIVERSAL.**\n\n\
                Tier: DIAMOND. -/\n";
    let blocks = substrate_blocks("contracts/lean/X.lean", lean);
    assert_eq!(
        blocks.len(),
        1,
        "a Lean block comment is ONE record even across blank lines — \
         splitting it separates the milestone from the `## PMAT-` that dates \
         it, and ten correctly-cited records read as uncited"
    );
    assert!(!pmat_ids(&blocks[0].flat).is_empty());
}

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

/// The four sentences that were live in `contracts/kani/ffi_cpython_ext.rs`
/// and `contracts/lean/FfiCpythonExt.lean` on 2026-07-28, verbatim, with the
/// `//!` / `///` markers stripped exactly as [`substrate_blocks`] strips them.
///
/// Quoted here so the two blindness controls below run against the REAL text
/// rather than against a paraphrase that might be easier to catch than the
/// thing that actually shipped.
const RETIRED_TOTALITY_SITES: [&str; 4] = [
    "100% of the substrate at QUORUM (≥1 vote in ≥3 strata)",
    "Zero contracts UNVERIFIED, zero PARTIAL",
    "This is the **TWELFTH and FINAL** Kani harness — every contract in xpile's \
     substrate is now at QUORUM after this lands.",
    "With this landed and PMAT-077 (companion Kani harness) shipped, every \
     contract in xpile's substrate has paired Lean + Kani Bronze-tier discharges.",
];

/// PMAT-1456 — a gate's SUBJECT and its NEEDLE are two independent blind spots,
/// and this establishes BOTH with a control that PASSES rather than by
/// argument.
///
/// PMAT-1451 widened this gate's subject from `book_pages()` to `claim_pages()`
/// and pinned five spellings. Neither half reached the defect:
///
///   * the NEEDLE, run over the text that actually shipped, matches nothing —
///     so widening the subject alone would have left all four published;
///   * the SUBJECT excluded `.rs` entirely, so sharpening the needle alone
///     would have left all four published too.
///
/// Fixing one and calling it done is the failure mode; the pair below is what
/// makes "fixed both" a checked statement.
#[test]
fn total_quorum_needle_and_subject_are_independently_blind() {
    // ── NEEDLE. The five PMAT-1451 spellings, as they were written (a
    //    case-sensitive `contains`, which is how they were applied).
    const PMAT_1451_NEEDLES: [&str; 5] = [
        "full quorum",
        "100% QUORUM",
        "100% §14.4",
        "all at QUORUM",
        "0 PARTIAL",
    ];
    for site in RETIRED_TOTALITY_SITES {
        assert!(
            !PMAT_1451_NEEDLES.iter().any(|n| site.contains(n)),
            "the PMAT-1451 needle set matches {site:?} after all — then this \
             slice's NEEDLE half is not a blindness and the doc comment \
             claiming it is must be rewritten"
        );
        // …and the replacement DOES see it. Without this the assertion above
        // is satisfied by a needle that matches nothing at all.
        assert!(
            !total_quorum_claims(site, 35).is_empty(),
            "the widened needle does not match {site:?}, so it would not have \
             caught the text this slice removed"
        );
        // Each site is a LIVE claim, not one the scoping branch waves past —
        // none of them named the population it was true of.
        assert!(
            total_quorum_claims(site, 35).iter().all(|&(_, s)| !s),
            "{site:?} reads as scoped to a past substrate; it is not, and \
             treating it as one would re-exempt the defect"
        );
    }

    // ── SUBJECT. `.rs` under `contracts/` is now collected; it was not.
    let pages = provable_artifact_pages();
    let rs: Vec<&String> = pages
        .iter()
        .map(|(rel, _)| rel)
        .filter(|rel| rel.ends_with(".rs"))
        .collect();
    assert!(
        rs.len() >= 20,
        "only {} `.rs` file(s) under contracts/ are in the subject",
        rs.len()
    );
    assert!(
        rs.iter()
            .any(|rel| rel.ends_with("kani/ffi_cpython_ext.rs")),
        "the file all four retired sites came from is not in the subject"
    );
}

/// PMAT-1456 — the needle reports a quantifier's OWN PREDICATE, not a token
/// that happens to share its clause.
///
/// All three honest sentences below were reported as drift on this gate's
/// first widened run, and all three are sentences a reader would defend. What
/// spares each is asserted SEPARATELY, because measurement showed they are
/// spared by different branches and an undifferentiated "none of these are
/// reported" would have let the dead proximity guard survive (see
/// [`quorum_token_follows`]).
#[test]
fn total_quorum_needle_reports_a_predicate_not_a_neighbour() {
    // `xpile-spec.md:213` — the quantifier's predicate is `declares a
    // qa_gate`; `§14.4` belongs to the following conjunct. Also SCOPED, by
    // `all 12`, which is the other half of why it is honest.
    let spec = "every contract declares a `qa_gate`), and all 12 are at §14.4 \
                N-of-M QUORUM via paired Lean refinement theorems";
    // `contracts/README.md:29` — true of every contract, and the QUORUM token
    // is in the PRECEDING sentence, which `.**` did not split off.
    let readme = "**35 contracts: 26 at §14.4 QUORUM, 9 PARTIAL, 0 \
                  UNVERIFIED.** Every contract binds a Lean refinement theorem";
    // `README.md:193` — names the SUBSET, which is the repair the gate asks
    // for. Caught only once the scan became case-insensitive.
    let subset = "The mature core sits at full QUORUM while newer contracts \
                  are still accreting stratum votes.";
    for honest in [spec, readme, subset] {
        assert!(
            total_quorum_claims(honest, 35).iter().all(|&(_, s)| s),
            "reported a true sentence as live substrate drift: {honest:?}"
        );
    }
    // The hostile variant: same quantifier, quorum token AS its predicate.
    let hostile = "every contract in the substrate is at QUORUM";
    let hits = total_quorum_claims(hostile, 35);
    assert!(
        !hits.is_empty() && hits.iter().all(|&(_, s)| !s),
        "the needle no longer catches a bare totality claim: {hostile:?}"
    );

    // ── WHICH branch spares each one. Asserted separately, because a blanket
    //    "not reported" is what let a guard that bounded nothing look justified.
    let quantifier_at = |s: &str| {
        s.to_ascii_lowercase()
            .find("every contract")
            .expect("fixture carries the quantifier")
    };
    // `readme` is spared by DIRECTION: no §14.4 token follows the quantifier
    // at all, so scanning the whole clause — which is what the code did before
    // — is exactly what reported it.
    assert!(
        !quorum_token_follows(&readme.to_ascii_lowercase(), quantifier_at(readme)),
        "a token now follows the quantifier in the README fixture, so the \
         direction rule no longer spares it and this control is testing \
         nothing"
    );
    assert!(
        QUORUM_TOKENS
            .iter()
            .any(|t| readme.to_ascii_lowercase().contains(t)),
        "the README fixture carries no §14.4 token anywhere, so it would be \
         spared even without the direction rule"
    );
    // `spec` is spared by SCOPING (`all 12`), NOT by direction — a token does
    // follow its quantifier. This is the assertion whose first version claimed
    // a proximity window did the work; it did not.
    assert!(
        quorum_token_follows(&spec.to_ascii_lowercase(), quantifier_at(spec)),
        "the spec fixture no longer has a token after its quantifier, so it is \
         spared by direction and the scoping branch is untested here"
    );
    assert_eq!(
        clause_denominators(&spec.to_ascii_lowercase()),
        vec![12],
        "the spec fixture must scope itself with `all 12`, or it is spared by \
         something this test does not name"
    );
}

/// PMAT-1456 — widening a gate to a new ARTIFACT KIND means auditing its TEXT
/// MODEL for that kind. The `.rs` doc-comment marker breaks a wrapped claim
/// exactly as the YAML `#` did (PMAT-1454), and the site that hid behind it is
/// the one this slice removed.
#[test]
fn substrate_blocks_strip_rust_doc_markers() {
    let rs = "//! This is the **TWELFTH and FINAL** Kani harness — every\n\
              //! contract in xpile's substrate is now at QUORUM after this\n\
              //! lands.\n";
    // The claim is INVISIBLE in the raw source: it wraps.
    assert!(
        !rs.contains("every contract"),
        "the fixture no longer wraps, so it cannot demonstrate the defect"
    );
    let blocks = substrate_blocks("contracts/kani/x.rs", rs);
    assert_eq!(blocks.len(), 1, "one comment run is one block");
    assert!(
        blocks[0]
            .flat
            .contains("every contract in xpile's substrate is now at QUORUM"),
        "the marker was not stripped, so the wrapped claim still reads as \
         `every //! contract` and matches nothing: {:?}",
        blocks[0].flat
    );
    assert!(
        !blocks[0].flat.contains("//"),
        "a comment marker survived into the flattened text: {:?}",
        blocks[0].flat
    );
    // …and the needle finds it there, which is the only reason the stripping
    // matters.
    assert!(
        !total_quorum_claims(&blocks[0].flat, 35).is_empty(),
        "stripped, but the claim still does not match"
    );
    // The offset must still attribute back to the line the phrase STARTS on.
    let (at, _) = total_quorum_claims(&blocks[0].flat, 35)[0];
    assert_eq!(
        blocks[0].line_at(at),
        1,
        "`every contract` starts on line 1 and must be reported there"
    );
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
/// Every one of these is total ON ITS OWN — "100%", "all", "zero remaining"
/// carry the quantifier inside the phrase.
///
/// PMAT-1456 REMOVED `"full quorum"` from this list, which is a NARROWING and
/// therefore has to be justified. Its subject can be a SUBSET, so on its own it
/// says nothing about totality: `README.md:193` reads "The mature core sits at
/// full QUORUM while newer contracts are still accreting stratum votes" — the
/// honest disclosure, and precisely the "say which SUBSET is discharged" repair
/// this gate's own failure message asks for. It only matched at all once this
/// scan became case-insensitive (the corpus writes `full QUORUM`), so nothing
/// that was previously caught is lost: the phrase is still checked whenever it
/// sits with a totality quantifier, and `has full quorum` over a NAMED contract
/// is `PER_CONTRACT_QUORUM_CLAIMS`' job.
const TOTAL_QUORUM_CLAIMS: [&str; 5] = [
    "100% quorum",
    "100% §14.4",
    "all at quorum",
    "0 partial",
    // PMAT-1456 — the same claim, spelled out. `contracts/kani/
    // ffi_cpython_ext.rs` wrote "Zero contracts UNVERIFIED, zero PARTIAL"
    // and the numeral needle above could not see it.
    "zero partial",
];

/// A §14.4 token counts as a quantifier's claim only if it FOLLOWS it — the
/// quantifier's predicate is what comes after it.
///
/// `contracts/README.md:29` is why: "…9 PARTIAL, 0 UNVERIFIED.** Every contract
/// binds a Lean refinement theorem". Every contract does, and the QUORUM token
/// is in the PRECEDING sentence, which `clauses_with_offsets` never split off
/// because `.**` is not one of its delimiters. Scanning the whole clause
/// reports that true sentence as substrate drift.
///
/// ⚠️ A PROXIMITY window was written here first, and its own red half refuted
/// it. The rule was "the token must follow the quantifier AND sit within 60
/// bytes", justified by a second site — `xpile-spec.md:213`'s "every contract
/// declares a `qa_gate`), and all 12 are at §14.4 N-of-M QUORUM", where the
/// token belongs to the following conjunct. It does not hold up: that gap is
/// **56 bytes**, INSIDE the window, so the window never spared it — the
/// SCOPING branch did, via `all 12`. Setting the constant to `usize::MAX / 4`
/// left the whole gate green, so the window bounded nothing anywhere in the
/// corpus. Keeping it would have meant publishing a guard that does no work
/// and a justification that measurement contradicts, which is the shape this
/// slice exists to remove. Direction is kept because it is load-bearing;
/// proximity is dropped because it is not.
///
/// `total_quorum_needle_reports_a_predicate_not_a_neighbour` pins both — the
/// direction rule by the site that needs it, and the scoping branch by the
/// site that turned out to need THAT instead.
fn quorum_token_follows(lower: &str, at: usize) -> bool {
    QUORUM_TOKENS.iter().any(|t| lower[at..].contains(t))
}

/// Phrases that quantify over the WHOLE contract population. Each needs a
/// §14.4 token in the same clause before it becomes a quorum claim — see
/// [`total_quorum_claims`].
const TOTALITY_QUANTIFIERS: [&str; 4] = [
    "every contract",
    "all contracts",
    "100% of the substrate",
    "12 of 12",
];

/// Tokens that make a quantified clause be about §14.4 DISCHARGE STATE rather
/// than about some other universal property of contracts.
///
/// This is the guard that keeps the needle from over-reaching, and it is
/// load-bearing in a way measurement showed: `xpile-spec.md` says "every
/// contract declares a `qa_gate`" and `audit-design.md` says "every equation
/// has Silver-tier refinement" — both TRUE, both quantified over the whole
/// substrate, neither a quorum claim. A needle without this token requirement
/// reports true sentences as drift.
const QUORUM_TOKENS: [&str; 5] = [
    "quorum",
    "§14.4",
    "paired lean",
    "paired-discharge",
    "paired discharge",
];

/// Every TOTAL §14.4 discharge claim in `clause`, as
/// `(byte offset, scoped_to_a_smaller_past_substrate)`.
///
/// PMAT-1456 — written as a CLASS (quantifier × §14.4 token), not as more
/// literals, because the literal list is what failed. The five spellings
/// PMAT-1451 pinned matched NONE of the four live falsehoods this slice found;
/// measured over the 24 `.rs` files under `contracts/`, they matched nothing
/// at all, while the class needle matches "100% of the substrate at QUORUM",
/// "zero PARTIAL", "every contract … is now at QUORUM" and "every contract …
/// has paired Lean + Kani … discharges".
///
/// ## The scoping rule, and why it is a CHECKED carve-out
///
/// A claim that names its own denominator — "the then-12-contract substrate",
/// "all 12 contracts", "12 of 12" — is true of the substrate it names, and
/// naming a past size IS the date. That is the repair form this repository
/// already uses (`glossary.md`, `audit-design.md`, `book/src/changelog.md`),
/// so it must not be reported.
///
/// It is not an exemption keyword, though — a carve-out that is not checked is
/// a hole (PMAT-1451). The denominator is compared against the LIVE contract
/// count: "all 35 contracts at QUORUM" scopes to the substrate that exists
/// today and stays a live claim that must be true. Only a denominator that
/// DIFFERS from the live total buys the scope.
///
/// ## What this deliberately does NOT match
///
/// "operational across the entire substrate" (`audit-design.md:46`). The
/// predicate there is that the quorum ARCHITECTURE runs substrate-wide, which
/// is true — `xpile quorum` does range over every contract. Folding "the
/// entire substrate" into [`TOTALITY_QUANTIFIERS`] made that line an offence
/// during measurement. The claim class here is quantification over CONTRACTS
/// reaching a discharge state, and one prose site whose subject is the
/// architecture rather than the contracts is repaired by wording, not gated.
fn total_quorum_claims(clause: &str, live_total: usize) -> Vec<(usize, bool)> {
    let lower = clause.to_ascii_lowercase();
    // Scope is decided on the claim's OWN BULLET, not on the whole flattened
    // clause. Fourth slice running in which this exact correction is needed
    // (PMAT-1450 paragraph→line, 1451 line→clause, 1452 paragraph→clause), and
    // this gate's own red half is what surfaced it: restoring the retired
    // `contracts/kani/ffi_cpython_ext.rs` text reported 2 of its 4 sites,
    // because "100% of the substrate at QUORUM" and "Zero contracts
    // UNVERIFIED, zero PARTIAL" are BULLETS in a run whose FIRST bullet reads
    // "12 contracts × 2 strata". A neighbour must not launder a bare claim.
    let scoped_at = |at: usize| {
        let d = clause_denominators(bullet_segment(&lower, at));
        !d.is_empty() && d.iter().all(|&n| n != live_total)
    };

    let predicate_token = |at: usize| quorum_token_follows(&lower, at);
    // A DENIAL is not an assertion. `book/src/concepts/contracts.md` opens a
    // section with "**Not every contract is at quorum**", which the quantifier
    // matches and which is the honest sentence.
    let denied = |at: usize| {
        let lo = (at.saturating_sub(12)..=at)
            .find(|&i| lower.is_char_boundary(i))
            .unwrap_or(at);
        let back = &lower[lo..at];
        back.contains("not ") || back.contains("n't ")
    };

    let mut hits: Vec<(usize, bool)> = Vec::new();
    for needle in TOTAL_QUORUM_CLAIMS {
        let mut from = 0usize;
        while let Some(rel) = lower[from..].find(needle) {
            let at = from + rel;
            from = at + needle.len(); // non-empty needle ⇒ strictly advances
            if !denied(at) {
                hits.push((at, scoped_at(at)));
            }
        }
    }
    for needle in TOTALITY_QUANTIFIERS {
        let mut from = 0usize;
        while let Some(rel) = lower[from..].find(needle) {
            let at = from + rel;
            from = at + needle.len();
            if !denied(at) && predicate_token(at) {
                hits.push((at, scoped_at(at)));
            }
        }
    }
    // `all 12 contracts`, `all 35 contracts` — the quantifier with its
    // denominator spliced in, which no fixed phrase can carry.
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find("all ") {
        let at = from + rel;
        from = at + "all ".len();
        let rest = &lower[at + "all ".len()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty()
            && rest[digits.len()..].starts_with(" contract")
            && !denied(at)
            && predicate_token(at)
        {
            hits.push((at, scoped_at(at)));
        }
    }
    hits.sort_unstable();
    hits.dedup();
    hits
}

/// The list item of `lower` containing byte offset `at`, or the whole slice if
/// it is not a list.
///
/// `substrate_blocks` and `paragraphs_under_headings` both flatten a bullet run
/// into one string, and `clauses_with_offsets` splits only on `. ` and `; ` —
/// so a four-bullet milestone list is ONE clause and any denominator in any
/// bullet scopes all four. Bullet markers are the missing delimiter.
fn bullet_segment(lower: &str, at: usize) -> &str {
    const MARKS: [&str; 3] = [" - ", " * ", " • "];
    let mut lo = 0usize;
    let mut hi = lower.len();
    for m in MARKS {
        if let Some(j) = lower[..at].rfind(m) {
            lo = lo.max(j + m.len());
        }
        if let Some(j) = lower[at..].find(m) {
            hi = hi.min(at + j);
        }
    }
    &lower[lo..hi]
}

/// Every explicit contract-population size named in `lower` (already
/// lowercased). A claim carrying one is ABOUT that population.
///
/// Three forms, all live in the corpus: `12 contracts` / `then-12-contract`
/// (attributive or head noun), `all 12 are …` (the noun elided — this is how
/// `xpile-spec.md:213` scopes itself, and without it that dated line reads as a
/// live claim), and `12 of 12` (`audit-design.md`).
fn clause_denominators(lower: &str) -> Vec<usize> {
    let b = lower.as_bytes();
    let mut out = Vec::new();
    let digits_before = |j: usize| {
        let mut k = j;
        while k > 0 && b[k - 1].is_ascii_digit() {
            k -= 1;
        }
        (k < j).then(|| lower[k..j].parse::<usize>().ok()).flatten()
    };
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find("contract") {
        let at = from + rel;
        from = at + "contract".len();
        let mut j = at;
        while j > 0 && matches!(b[j - 1], b' ' | b'-') {
            j -= 1;
        }
        out.extend(digits_before(j));
    }
    for lead in ["all ", "of "] {
        let mut from = 0usize;
        while let Some(rel) = lower[from..].find(lead) {
            let at = from + rel + lead.len();
            from = at;
            let digits: String = lower[at..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if !digits.is_empty() {
                out.extend(digits.parse::<usize>().ok());
            }
        }
    }
    out
}

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
    // The live denominator, DERIVED from the same `xpile quorum` run that
    // supplies the PARTIAL population — never a literal. It is what decides
    // whether a claim naming "N contracts" is scoped to a past substrate or is
    // a live claim about today's (see `total_quorum_claims`).
    let live_total = status.len();

    let mut offences = Vec::new();
    let mut per_contract = Vec::new();
    let mut scanned = 0usize;
    let mut historical = Vec::new();
    let mut undated = Vec::new();
    let mut scoped: Vec<String> = Vec::new();
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
                {
                    for (off, claim_scoped) in total_quorum_claims(clause, live_total) {
                        let at = base + off;
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
                                undated.push(format!(
                                    "{rel}:{n}: totality claim under {:?}",
                                    para.heading
                                ));
                            } else {
                                historical.push(format!("{rel}:{n}"));
                            }
                            continue;
                        }
                        // A claim that names its own denominator is scoped to
                        // the substrate it names — see `total_quorum_claims`.
                        // Counted, so a later narrowing that kills the branch
                        // cannot go unnoticed.
                        if claim_scoped {
                            scoped.push(format!("{rel}:{n}"));
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
                        let quoted = spans.iter().any(|&(s, e)| at >= s && at < e);
                        if denied && quoted {
                            historical.push(format!("{rel}:{n}"));
                            continue;
                        }
                        offences.push(format!("{rel}:{n}: {}", excerpt(&para.flat, at)));
                    }
                }
            }
        }
    }
    assert!(
        scanned > 1,
        "the exemption swallowed the corpus: only {scanned} page(s) scanned"
    );

    // ── the substrate arm (PMAT-1456) ────────────────────────────────────
    //
    // The SUBJECT half. PMAT-1451 moved this gate from `book_pages()` to
    // `claim_pages()` and stopped there; `claim_pages()` is MARKDOWN, so the
    // artifacts the quorum claim is ABOUT — the contract YAMLs, the Lean
    // sources that discharge them, and the Kani harnesses that are the
    // Symbolic stratum — were never in its subject at all. All four live
    // falsehoods were in `contracts/kani/ffi_cpython_ext.rs`.
    //
    // Both blindnesses are established by a control that PASSES, not by
    // argument (see `total_quorum_needle_and_subject_are_independently_blind`):
    // the OLD needle over the NEW subject finds nothing, and the NEW needle
    // over the OLD subject finds nothing. Fixing either alone leaves the
    // defect published.
    let substrate = provable_artifact_pages();
    let rs_files = substrate.iter().filter(|(r, _)| r.ends_with(".rs")).count();
    assert!(
        rs_files >= 20,
        "provable_artifact_pages() collected only {rs_files} `.rs` file(s) from \
         contracts/ — the Kani harnesses are the artifact kind this arm was \
         added for, and the walk is not reaching them"
    );
    let mut substrate_claims = 0usize;
    for (rel, body) in &substrate {
        for block in substrate_blocks(rel, body) {
            for (clause, base) in clauses_with_offsets(&block.flat) {
                for (off, claim_scoped) in total_quorum_claims(clause, live_total) {
                    let n = block.line_at(base + off);
                    substrate_claims += 1;
                    if claim_scoped {
                        scoped.push(format!("{rel}:{n}"));
                        continue;
                    }
                    if partial == 0 {
                        continue; // the claim would be true
                    }
                    // A NORMATIVE field is what the contract asserts and is
                    // parsed into the contract's meaning; a provenance comment
                    // or docstring narrates. Both are reported — the record
                    // exemption here is the DENOMINATOR, not the comment
                    // marker — but naming which one it is tells the reader
                    // whether they are editing prose or an assertion.
                    let slot = if block.record {
                        "a provenance comment/docstring"
                    } else {
                        "a NORMATIVE field"
                    };
                    offences.push(format!(
                        "{rel}:{n}: {slot} asserts TOTAL §14.4 quorum — {}",
                        excerpt(&block.flat, base + off)
                    ));
                }
            }
        }
    }
    assert!(
        substrate_claims > 0,
        "the substrate arm scanned {} contract artifact(s) and found no quorum \
         claim of any kind, so the needle is matching nothing there and this \
         whole arm passes for free",
        substrate.len()
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
    // ⚠️ DOMINATED, and said so rather than implied. This was written as the
    // anti-vacuity tripwire for the scoping branch, and its red half refuted
    // that: killing the branch (`scoped_at` forced to `false`) reds the
    // OFFENCES assertion above instead, on `audit-design.md:46` and the other
    // honestly-scoped sentences, which is the louder and more useful failure.
    // There is no perturbation in this corpus that reaches this line. It is
    // kept as a cheap forward tripwire for a future narrowing that leaves the
    // branch alive but matching nothing — NOT as evidence the branch is
    // tested, which the red half is. Second guard in this slice whose
    // justification measurement contradicted; see [`quorum_token_follows`] for
    // the first, which was deleted rather than downgraded.
    assert!(
        !scoped.is_empty(),
        "no totality claim named a past denominator anywhere in the corpus, so \
         the scoping branch of `total_quorum_claims` is matching nothing"
    );
}

/// A short excerpt of `flat` around byte offset `at`, for a failure message.
fn excerpt(flat: &str, at: usize) -> String {
    let lo = flat[..at]
        .char_indices()
        .rev()
        .nth(30)
        .map_or(0, |(i, _)| i);
    let hi = flat[at..]
        .char_indices()
        .nth(70)
        .map_or(flat.len(), |(i, _)| at + i);
    format!("…{}…", flat[lo..hi].trim())
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
///
/// PMAT-1454 — THIRD PASS, and both axes were wrong again. PMAT-1452 left the
/// standing lead *"ask of EVERY other claims-drift gate whether its RULE stops
/// at markdown"*; this one did, and the answer was 91 unchecked instances.
///
///   * SUBJECT. `claim_pages()` is MARKDOWN. The 35 contract YAMLs and the 35
///     Lean sources that discharge them — the normative artifacts the whole
///     provability claim rests on — were in no gate for this class at all.
///   * SPELLING, twice more. `contracts/**` shouts: `COMPLETES DEPTH-5
///     UNIVERSAL ACROSS ALL 12 CONTRACTS`, and the prefix was matched as the
///     literal lower-case `depth-` (50 sites). Worse, twelve sites assert the
///     claim in the words of its own DEFINITION — `every contract has ≥13
///     distinct Diamond categories` — with no `depth-` token to match at all.
///     A needle keyed on the LABEL is blind to the DEFINITION the label
///     expands to, and that spelling needs no misspelling to hide.
///
/// ⭐ THE SHARPEST SITES ARE ASSERTIONS, NOT PROSE. Thirty-six of them sit in
/// `invariants:` and `postconditions:` — the fields that say what the contract
/// HOLDS, parsed into `xpile_contract_frontend`'s `invariants: Vec<String>` —
/// stating `"Substrate milestone: every contract has ≥13 distinct Diamond
/// categories"` and `"DEPTH-13 UNIVERSAL achieved: 12 contracts at depth-13+"`.
/// The live substrate is 35 contracts whose shallowest carries ONE Diamond.
/// Every one of those equations already carries the same milestone in an
/// adjacent `# PMAT-NNN:` provenance comment, correctly cited — so the
/// normative copies duplicated a RECORD into an ASSERTION slot, and deleting
/// the duplicate loses nothing but the falsehood.
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
    // PMAT-1454 — the CASE spelling, verbatim from `contracts/`. The parser
    // this slice inherited matched the literal lower-case `depth-`, so every
    // one of these was invisible while sitting in the normative substrate.
    assert_eq!(
        claimed_universal_depths(
            "FIFTH Diamond category — COMPLETES DEPTH-5 UNIVERSAL ACROSS ALL 12 CONTRACTS"
        ),
        vec![5],
        "the UPPER-CASE spelling must be matched"
    );
    assert_eq!(
        claimed_universal_depths("DEPTH-13 UNIVERSAL achieved: 12 contracts at depth-13+"),
        vec![13],
        "the upper-case claim counts once; the trailing `depth-13+` is a \
         population statement, not a universality claim — it is not followed \
         by UNIVERSAL, and that distinction is the whole point of the suffix \
         check"
    );
    // PMAT-1454 — the DEFINITION spelling, which carries no `depth-` token at
    // all. This is the form `sub/diamond-taxonomy.md` uses to DEFINE the term,
    // and the form the contracts use to assert it.
    assert_eq!(
        claimed_universal_depths(
            "Substrate milestone: every contract has ≥5 distinct Diamond categories"
        ),
        vec![5],
        "the DEFINITION spelling must be matched — it is the claim, not a paraphrase"
    );
    assert_eq!(
        claimed_universal_depths("every contract now has at least 9 Diamond categories"),
        vec![9],
        "`at least N` is the same quantifier as `≥N`"
    );
    assert_eq!(
        claimed_universal_depths("every contract carries >=1 wired Diamond"),
        vec![1],
        "`>=N` is the ASCII spelling of the same quantifier"
    );
    // PMAT-1454 — the PREFIX order. `UNIVERSAL` qualifies the depth from the
    // left here, and the corpus writes it both ways within one file.
    assert_eq!(
        claimed_universal_depths("completes UNIVERSAL Diamond depth-3 across all 5 layers"),
        vec![3],
        "the qualifier may PRECEDE the depth"
    );
    // …and the two shapes that made the loose version of that rule
    // un-shippable, verbatim from the corpus. Both are POLICY — they say what
    // not to do — and neither asserts that any depth is universal.
    assert!(
        claimed_universal_depths(
            "**Diamond-depth UNIVERSAL ratchet is frozen at depth-13.** No depth-14+ sweeps"
        )
        .is_empty(),
        "`xpile-spec.md`'s ratchet POLICY asserts no depth; a needle that \
         reports it would be reporting a true sentence as drift"
    );
    assert!(
        claimed_universal_depths(
            "Do **not** run further UNIVERSAL broadening sweeps beyond depth-13"
        )
        .is_empty(),
        "`sub/diamond-taxonomy.md`'s freeze notice asserts no depth either"
    );
    // …and the two guards on that form, each pinned in the direction that
    // would make it over-reach. A carve-out that is not checked is a hole.
    assert!(
        claimed_universal_depths("every contract has ≥2 references and one author").is_empty(),
        "a quantified claim about a NON-Diamond property is not a depth claim"
    );
    assert!(
        claimed_universal_depths("every contract has a Diamond. Kani covers ≥24 of them")
            .is_empty(),
        "a quantifier in a LATER clause must not be read onto `every contract`"
    );

    // A claim of the form "depth-N UNIVERSAL", or the range spelling
    // "depth-A..B UNIVERSAL", for a depth the substrate does not hold.
    // Scanned per PARAGRAPH, not per line, for two reasons: a claim and its
    // denial live a paragraph apart, and the disclosure on the Diamond page
    // WRAPS between `depth-1..13` and `UNIVERSAL`, so a line-oriented scan
    // cannot see it at all and a reflow would silently change the verdict.
    // PMAT-1454 — the SUBSTRATE half of the subject. `claim_pages()` is
    // markdown; the 35 contract YAMLs and the 35 Lean sources that discharge
    // them carried 91 instances of this claim class and were in no gate.
    let substrate = provable_artifact_pages();
    let substrate_files = substrate.len();
    assert!(
        substrate_files >= 30,
        "provable_artifact_pages() collected only {substrate_files} file(s) \
         from contracts/ — the walk is not reaching the substrate, and the \
         half of the subject PMAT-1454 added would be vacuous"
    );
    // The markdown exemption is a HEADING carrying `(historical record)`, and
    // `paragraphs_under_headings` accepts any line whose first non-space
    // character is `#` — which every YAML comment is. So the marker would be
    // grantable from an ordinary contract comment, to every field below it.
    // The substrate arm below therefore does not consult it at all; this
    // asserts the corpus cannot be silently relying on that, rather than
    // leaving the reasoning in a comment (PMAT-1451's fence-comment shape,
    // one artifact kind over).
    for (rel, body) in &substrate {
        assert!(
            !body.contains(HISTORICAL_MARKER),
            "{rel} spells `{HISTORICAL_MARKER}`, but the substrate arm of this \
             gate judges by ARTIFACT STRUCTURE (normative field vs provenance \
             comment) and never reads the marker. A YAML comment is
             indistinguishable from an ATX heading here, so honouring it would \
             let one comment exempt every field beneath it. Decide which rule \
             governs before introducing the marker under contracts/."
        );
    }

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
    // ── the substrate arm (PMAT-1454) ────────────────────────────────────
    //
    // `contracts/**` has no headings, so the markdown record-marker rule has
    // nothing to attach to. The artifact's own structure supplies the
    // distinction instead, and it is a sharper one:
    //
    //   * a NORMATIVE FIELD — `invariants:`, `postconditions:`, anything that
    //     is not a comment — is what the contract ASSERTS. It is parsed into
    //     `xpile_contract_frontend::…::invariants` and is part of the
    //     contract's meaning. It must be live-true; there is no record
    //     exemption, because a record does not belong in an assertion slot.
    //   * a PROVENANCE COMMENT (`# PMAT-354: …`) or a Lean docstring NARRATES
    //     a numbered slice. It is exempt iff it says WHEN — the same citation
    //     rule the markdown arm applies inside a `(historical record)`
    //     section, scoped to the CLAUSE (PMAT-1450 → 1451 → 1452 each had to
    //     make exactly this scope fix; a neighbouring id must not launder a
    //     bare claim).
    let mut substrate_assertions_ok = 0usize;
    let mut substrate_records = 0usize;
    let mut substrate_uncited = Vec::new();
    for (rel, body) in &substrate {
        for block in substrate_blocks(rel, body) {
            for (claimed, at) in claimed_universal_depths_at(&block.flat) {
                let line_no = block.line_at(at);
                if !block.record {
                    if claimed <= universal {
                        substrate_assertions_ok += 1;
                        continue;
                    }
                    offences.push(format!(
                        "{rel}:{line_no}: a contract ASSERTS depth-{claimed} \
                         UNIVERSAL in a normative field"
                    ));
                    continue;
                }
                if claimed <= universal {
                    substrate_records += 1;
                    continue;
                }
                // A record says WHEN, scoped to its own provenance BLOCK —
                // the run of comment lines, or the one `/-! … -/` docstring,
                // that the claim is written in. A neighbouring equation's
                // citation cannot reach it.
                //
                // ⚠️ THIS IS A LOWER BOUND AND IT CURRENTLY BITES NOTHING.
                // Measured, not assumed: every provenance block under
                // `contracts/` that carries a milestone also carries a
                // citation, so the branch below never fires on today's corpus
                // — its red half PASSES, twice over (strip one id from a
                // block, then strip all nine from the visible region: both
                // stayed green, because a Lean module docstring goes on to
                // list all five Diamond categories with their ids).
                //
                // At CLAUSE scope — the tightening PMAT-1450, 1451 and 1452
                // each had to make to their own anti-whitewash checks — it
                // reports 32 sites, all of them Lean docstring sentences that
                // would each need a `(PMAT-NNN)` appended. That is recorded
                // as the follow-up rather than done here, because it is 32
                // cosmetic insertions and none of them is a false claim: the
                // LIVE falsehoods this slice exists for are the assertions
                // above, and they are gated. `an_uncited_provenance_block_is_
                // reported` pins that this branch CAN fire, so it is a
                // hardening with no current verdict change rather than a
                // decoration nobody has tested.
                if !pmat_ids(&block.flat).is_empty() {
                    substrate_records += 1;
                    continue;
                }
                substrate_uncited.push(format!(
                    "{rel}:{line_no}: claims depth-{claimed} UNIVERSAL without \
                     citing the slice it records"
                ));
            }
        }
    }

    assert!(
        pages_scanned > 1,
        "claim_pages() returned {pages_scanned} page(s) — the corpus walk broke"
    );
    // The LIVE FALSEHOOD reports before every hygiene rule below it — a gate
    // that aborts on the tidiness of an exemption and buries a false published
    // claim has its priorities inverted (PMAT-1451 shipped exactly that bug and
    // reported one site of six).
    assert!(
        offences.is_empty(),
        "the docs or the contract substrate claim a UNIVERSAL depth the \
         substrate does not hold — {} — but the shallowest contract carries \
         {universal} Diamond categor{}, so depth-{universal} is the live \
         universal depth. Say which SUBSET is deep, lift the shallow \
         contracts, or — in prose — move the sentence under a \
         `{HISTORICAL_MARKER}` heading if it is a dated record. ({} mention(s) \
         already are.) A normative `invariants:`/`postconditions:` entry has \
         no record exemption: state what the equation establishes about ITS \
         contract and leave the milestone to the provenance comment.",
        offences.join(", "),
        if universal == 1 { "y" } else { "ies" },
        historical.len()
    );
    assert!(
        substrate_uncited.is_empty(),
        "provenance under contracts/ records a UNIVERSAL depth the substrate \
         no longer holds without citing the slice it records — {}. A record of \
         what was once true says WHEN.",
        substrate_uncited.join("\n  ")
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
    // Both arms of the widened subject must be REACHED, and inside the
    // substrate arm both dispositions must be — an unreachable arm is an
    // unchecked arm (PMAT-1452). `substrate_assertions_ok` is the strongest of
    // the three: it is a control that PASSES, driven by
    // `ffi-shell-subprocess-v1.yaml`'s "every contract carries ≥1 wired
    // Diamond (depth-1 UNIVERSAL invariant preserved)" — a normative field
    // asserting the depth the substrate really does hold. If the needle ever
    // stops matching, that count goes to zero and says so, instead of the
    // gate going quietly green.
    assert!(
        substrate_assertions_ok >= 1,
        "no contract asserts a UNIVERSAL depth the substrate DOES hold, so the \
         normative-field pass path is unexercised and this gate could not tell \
         a working needle from a broken one"
    );
    assert!(
        substrate_records >= 1,
        "no cited provenance record of a UNIVERSAL milestone was found under \
         contracts/, so the record exemption is unreachable and untested"
    );
}

// ── PMAT-1455: the substrate's PUBLISHED PROOF VOLUME ────────────────
//
// Every number below is a claim about a file the repository owns, so every
// one of them is derivable. Two populations, and conflating them is the
// first trap: a contract's `kani_harness:` CITATIONS (10 on
// `C-XLATE-LEAN-TO-RUST`) are not the `#[kani::proof]` FUNCTIONS in its
// harness file (13 in `contracts/kani/xlate_lean_to_rust.rs`). The book
// states the first, `sub/kaizen-fleet.md` the second; each arm below is
// derived from the artifact its prose points at.

/// Everything a contract YAML publishes about its own size.
#[derive(Clone, Copy)]
struct ContractParts {
    /// Entries under `equations:`.
    equations: usize,
    /// Equations carrying a `lean_theorem:` citation.
    lean_citations: usize,
    /// Equations carrying a `kani_harness:` citation.
    kani_citations: usize,
}

/// `metadata.id` → the sizes of that contract, parsed with `serde_yaml` (the
/// crate's own loader) rather than counted by line-scan — a `kani_harness:`
/// spelled inside a provenance comment would be counted by a grep and is not
/// a citation.
fn contract_parts() -> HashMap<String, ContractParts> {
    let dir = workspace_root().join("contracts");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("yaml"))
        .collect();
    paths.sort();
    let mut out = HashMap::new();
    for p in paths {
        let body = fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
        let doc: serde_yaml::Value = serde_yaml::from_str(&body)
            .unwrap_or_else(|e| panic!("{} is not valid YAML: {e}", p.display()));
        let Some(id) = doc
            .get("metadata")
            .and_then(|m| m.get("id"))
            .and_then(|v| v.as_str())
        else {
            continue;
        };
        let mut parts = ContractParts {
            equations: 0,
            lean_citations: 0,
            kani_citations: 0,
        };
        if let Some(eqs) = doc.get("equations").and_then(|e| e.as_mapping()) {
            parts.equations = eqs.len();
            for (_, eq) in eqs {
                if eq.get("lean_theorem").is_some() {
                    parts.lean_citations += 1;
                }
                if eq.get("kani_harness").is_some() {
                    parts.kani_citations += 1;
                }
            }
        }
        out.insert(id.to_string(), parts);
    }
    out
}

/// Drop `--` line comments and NESTED `/- … -/` blocks from Lean source,
/// preserving newlines so line attribution survives.
fn strip_lean_comments(src: &str) -> String {
    let b = src.as_bytes();
    let (mut out, mut i, mut depth) = (String::with_capacity(src.len()), 0usize, 0usize);
    while i < b.len() {
        if b[i..].starts_with(b"/-") {
            depth += 1;
            i += 2;
            continue;
        }
        if depth > 0 && b[i..].starts_with(b"-/") {
            depth -= 1;
            i += 2;
            continue;
        }
        if depth == 0 && b[i..].starts_with(b"--") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if depth == 0 || b[i] == b'\n' {
            out.push(b[i] as char);
        }
        i += 1;
    }
    out
}

/// Drop `//` line comments and NESTED `/* … */` blocks from Rust source.
fn strip_rust_comments(src: &str) -> String {
    let b = src.as_bytes();
    let (mut out, mut i, mut depth) = (String::with_capacity(src.len()), 0usize, 0usize);
    while i < b.len() {
        if b[i..].starts_with(b"/*") {
            depth += 1;
            i += 2;
            continue;
        }
        if depth > 0 && b[i..].starts_with(b"*/") {
            depth -= 1;
            i += 2;
            continue;
        }
        if depth == 0 && b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if depth == 0 || b[i] == b'\n' {
            out.push(b[i] as char);
        }
        i += 1;
    }
    out
}

fn files_in(rel: &str, ext: &str) -> Vec<String> {
    let dir = workspace_root().join(rel);
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some(ext))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display())))
        .collect()
}

/// Lean `theorem`/`lemma` DECLARATIONS under `contracts/lean/`.
///
/// ⚠️ THE COMMENT-STRIP IS THE POINT, not hygiene. `grep -cE '^\s*(theorem|
/// lemma)'` over this directory answers 512; twenty-three of those lines are
/// English inside `/-- … -/` docstrings, e.g. `XlateLeanToRust.lean`'s
/// "Locked-in by the refinement / theorem below — any emitter that …", whose
/// second line begins with the word `theorem` followed by an identifier-shaped
/// word. A naive grep COUNTS PROSE ABOUT THEOREMS AS THEOREMS, and it
/// over-counts in the flattering direction.
fn lean_theorem_declarations() -> usize {
    files_in("contracts/lean", "lean")
        .iter()
        .map(|body| {
            strip_lean_comments(body)
                .lines()
                .filter(|l| {
                    let t = l.trim_start();
                    for kw in ["theorem ", "lemma "] {
                        if let Some(rest) = t.strip_prefix(kw) {
                            return rest
                                .chars()
                                .next()
                                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
                        }
                    }
                    false
                })
                .count()
        })
        .sum()
}

/// `#[kani::proof]` harnesses under `contracts/kani/`.
///
/// Same trap, same direction: the raw string occurs 101 times, six of them
/// inside `//!` module docs explaining what a harness is (`py_int_arith.rs`,
/// `c_int_arith.rs`, `c_float_arith.rs`, `ols_model_uniqueness.rs`). 95 are
/// attributes on a function. The published figure was the grep's.
fn kani_proof_harnesses() -> usize {
    files_in("contracts/kani", "rs")
        .iter()
        .map(|body| strip_rust_comments(body).matches("#[kani::proof]").count())
        .sum()
}

/// A population of the substrate a page can put a number in front of.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Part {
    Equations,
    LeanTheorems,
    KaniHarnesses,
    StratumVotes,
}

/// Longest spelling first — `lean refinement theorems` must win over
/// `refinement theorems`, or the walk-back would start inside the phrase.
const PART_SPELLINGS: [(&str, Part); 7] = [
    ("lean refinement theorems", Part::LeanTheorems),
    ("lean theorems", Part::LeanTheorems),
    ("refinement theorems", Part::LeanTheorems),
    ("kani bmc harnesses", Part::KaniHarnesses),
    ("kani harnesses", Part::KaniHarnesses),
    ("stratum-vote artifacts", Part::StratumVotes),
    ("equations", Part::Equations),
];

/// One `<N> <unit>` claim: the number, the population, and where it sits.
struct PartClaim {
    claimed: usize,
    part: Part,
    /// Byte offset of the NUMERAL in the flattened text.
    at: usize,
    /// The word immediately before the numeral, lowercased.
    preceded_by: String,
}

/// Scan `flat` for `<N> <unit>` claims about the substrate.
///
/// Three shapes are REJECTED at the needle, each because the corpus really
/// contains it and each would otherwise be a fabricated finding:
///
///   * `PMAT-353 equations_block_struct_extensionality` — the numeral is the
///     tail of an id and the unit is the head of a snake_case identifier.
///     Rejected by requiring a non-alphanumeric, non-`-` byte before the
///     numeral and a non-identifier byte after the unit. Four sites in
///     `contracts/lean/XpileContractFrontendTrait.lean` alone.
///   * `4/9 equations`, `9/9 equations` — a FRACTION is a coverage record
///     scoped to a slice, not a total. Rejected when the numeral is preceded
///     by `/`.
///   * `3 equations entries` — a compound noun about YAML rows, not a count of
///     the contract's equations (`notation-latex-math-to-equation-v1.yaml`).
///     Rejected by the identifier-boundary rule only if followed by an
///     identifier char, so this one is handled by the caller's routing: it
///     names no contract and no derivation directory, and is left unjudged.
fn part_claims(flat: &str) -> Vec<PartClaim> {
    let lower = flat.to_ascii_lowercase();
    let b = lower.as_bytes();
    let mut out: Vec<PartClaim> = Vec::new();
    for (spelling, part) in PART_SPELLINGS {
        let mut from = 0usize;
        while let Some(rel) = lower[from..].find(spelling) {
            let start = from + rel;
            let end = start + spelling.len();
            from = end;
            // The unit must END at a word boundary: `equations_block` is an
            // identifier, not a count of equations.
            if b.get(end)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
            {
                continue;
            }
            // Walk back over emphasis and space to the numeral.
            let mut i = start;
            while i > 0 && matches!(b[i - 1], b' ' | b'*' | b'_' | b'\t' | b'~') {
                i -= 1;
            }
            let num_end = i;
            while i > 0 && (b[i - 1].is_ascii_digit() || b[i - 1] == b',') {
                i -= 1;
            }
            if i == num_end {
                continue; // no numeral: `carries equations`, or a longer
                          // spelling owns the number (`Lean refinement
                          // theorems` reached via `refinement theorems`).
            }
            // `PMAT-353 equations` / `x4 equations` are not counts.
            if i > 0 && (b[i - 1] == b'-' || b[i - 1].is_ascii_alphanumeric()) {
                continue;
            }
            // `4/9 equations` is a coverage fraction, not a total.
            if i > 0 && b[i - 1] == b'/' {
                continue;
            }
            let digits: String = lower[i..num_end]
                .chars()
                .filter(|c| c.is_ascii_digit())
                .collect();
            let Ok(claimed) = digits.parse::<usize>() else {
                continue;
            };
            let preceded_by = token_before(&lower, i)
                .unwrap_or("")
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_string();
            out.push(PartClaim {
                claimed,
                part,
                at: i,
                preceded_by,
            });
        }
    }
    // Two spellings can report the same numeral (`260 Lean refinement
    // theorems` is also `… refinement theorems`); the walk-back makes the
    // shorter one find no numeral, but dedupe on the offset anyway so a future
    // spelling cannot double-report.
    out.sort_by_key(|c| c.at);
    out.dedup_by_key(|c| c.at);
    out
}

/// Does this clause attribute its number to a directory of the substrate?
fn names_derivation_dir(clause_lower: &str) -> bool {
    clause_lower.contains("contracts/lean") || clause_lower.contains("contracts/kani")
}

/// EVERY published count of a NAMED CONTRACT's own parts equals what that
/// contract's YAML holds.
///
/// PMAT-1455 — `book/src/reference/contracts.md` published
/// `C-XLATE-LEAN-TO-RUST` as carrying "40 equations, 33 Lean refinement
/// theorems and 10 Kani harnesses saying so". The contract holds 33 equations.
/// The other two numbers were right, which is what made the sentence readable:
/// 33 lean_theorem citations and 10 kani_harness citations are exactly what it
/// carries — so the page said there are SEVEN MORE EQUATIONS THAN THEOREMS
/// where in fact every equation has one.
///
/// The direction is the finding. That paragraph exists to disclose that
/// nothing implements this contract — "the proofs range over abstract models …
/// so they hold, and they hold of nothing shipped" — and the one number it got
/// wrong INFLATED the proof volume it was disclosing.
///
/// SUBJECT is `claim_pages()`, deliberately not widened to `contracts/**`, and
/// this is a measurement rather than an assumption: every `<N> equations`
/// occurrence under `contracts/` is a slice-scoped provenance record
/// (`remaining 8 equations` / `1/9 to 4/9 equations`, each citing its PMAT id),
/// an identifier (`PMAT-353 equations_block_…`), or a compound noun
/// (`3 equations entries`). No live falsehood of this class is there, and a
/// needle loose enough to reach those shapes reports them as findings.
#[test]
fn published_contract_part_counts_match_the_contract_yaml() {
    let parts = contract_parts();
    assert!(
        parts.len() >= 30,
        "contract_parts() loaded {} contract(s) — the substrate walk broke, \
         and every comparison below would be vacuous",
        parts.len()
    );

    let mut offences = Vec::new();
    let mut agreements: HashMap<Part, usize> = HashMap::new();
    let mut version_numerals = 0usize;

    for (rel, body) in claim_pages() {
        for para in paragraphs_under_headings(&body) {
            if para.heading.contains(HISTORICAL_MARKER) {
                continue;
            }
            let quoted = quoted_spans(&para.flat);
            for claim in part_claims(&para.flat) {
                // Reporting a claim is not making one (PMAT-1450).
                if quoted.iter().any(|&(a, b)| a <= claim.at && claim.at < b) {
                    continue;
                }
                // ⚠️ `Lean 4 refinement theorems` is a VERSION, not a count.
                // Both `README.md` and `book/src/concepts/contracts.md` write
                // it, and without this the gate reports the language version
                // as a drifted theorem tally.
                if claim.preceded_by == "lean" {
                    version_numerals += 1;
                    continue;
                }
                let (clo, chi) = clause_bounds(&para.flat, claim.at);
                let clause = &para.flat[clo..chi];
                let clause_lower = clause.to_ascii_lowercase();
                // A directory-attributed count is about the whole substrate,
                // not about one contract — the other test owns it.
                if names_derivation_dir(&clause_lower) {
                    continue;
                }
                // Subject: the nearest contract id, clause before paragraph
                // before heading. A clause naming two contracts is ambiguous
                // and is left to the reader, not guessed at.
                let subject = [clause, &para.flat, &para.heading]
                    .iter()
                    .map(|scope| contract_ids(scope))
                    .find_map(|ids| {
                        let known: Vec<String> =
                            ids.into_iter().filter(|i| parts.contains_key(i)).collect();
                        (known.len() == 1).then(|| known[0].clone())
                    });
                let Some(id) = subject else {
                    continue;
                };
                let p = parts[&id];
                let (live, what) = match claim.part {
                    Part::Equations => (p.equations, "equations"),
                    Part::LeanTheorems => (p.lean_citations, "`lean_theorem:` citations"),
                    Part::KaniHarnesses => (p.kani_citations, "`kani_harness:` citations"),
                    // An aggregate unit is never a property of one contract.
                    Part::StratumVotes => continue,
                };
                if claim.claimed == live {
                    *agreements.entry(claim.part).or_default() += 1;
                    continue;
                }
                let (line, _) = para.line_at(claim.at);
                offences.push(format!(
                    "{rel}:{line}: says {id} carries {} {what}, and it carries \
                     {live} — \"{}\"",
                    claim.claimed,
                    clause.trim()
                ));
            }
        }
    }

    assert!(
        offences.is_empty(),
        "a published per-contract size does not match the contract YAML. \
         Update the prose, or the contract, whichever is wrong:\n{}",
        offences.join("\n")
    );
    for part in [Part::Equations, Part::LeanTheorems, Part::KaniHarnesses] {
        assert!(
            agreements.contains_key(&part),
            "no AGREEING {part:?} claim was found, so that arm is unexercised \
             and this gate cannot tell a working needle from a broken one. \
             `book/src/reference/contracts.md` states all three for \
             C-XLATE-LEAN-TO-RUST; if it stopped, say so here rather than \
             leaving the arm standing."
        );
    }
    assert!(
        version_numerals >= 1,
        "the `Lean 4` discriminator never saw the shape it exists for, so the \
         corpus stopped writing `Lean 4 refinement theorems` and the rule is \
         ranging over nothing"
    );
}

/// A LANGUAGE VERSION IS NOT A COUNT — `Lean 4 refinement theorems` parses as
/// "4 refinement theorems", and every walk-back over a numeral finds it.
///
/// PMAT-1455 — this guard is a HARDENING WITH NO CURRENT VERDICT CHANGE, and
/// saying so is the point. Deleting it from both gates above leaves the whole
/// suite GREEN: the two live sites (`README.md`, `book/src/concepts/
/// contracts.md`) are unrouted for an unrelated reason — neither clause names
/// a contract or a derivation directory, the second only because
/// `clause_bounds` splits on the `|` of its table row. So
/// `version_numerals >= 1` proves the discriminator SEES the shape; it does
/// NOT prove the guard prevented a report. The red half established that
/// difference; an argument would have concluded the opposite.
///
/// A corpus that does not yet contain the dangerous arrangement cannot
/// demonstrate the rule (PMAT-1451), so the arrangement is constructed here:
/// one clause, naming `contracts/lean/`, therefore ROUTED. Without the guard
/// that text reports "attributes 4 Lean theorem/lemma declarations to the
/// substrate, which holds 489" — a fabricated finding against a version
/// number.
#[test]
fn a_language_version_is_not_a_count() {
    let routed = "the Semantic stratum is Lean 4 refinement theorems recorded in contracts/lean/";
    let claims = part_claims(routed);
    assert_eq!(
        claims.len(),
        1,
        "expected the version numeral to be SEEN (and then discriminated), \
         got {} claim(s)",
        claims.len()
    );
    assert_eq!(
        claims[0].claimed, 4,
        "the numeral parsed is the Lean version"
    );
    assert_eq!(claims[0].part, Part::LeanTheorems);
    assert_eq!(
        claims[0].preceded_by, "lean",
        "the discriminator is the preceding word, and it is what both gates \
         key on; if this stops being `lean` the guard silently stops firing"
    );
    assert!(
        names_derivation_dir(&routed.to_ascii_lowercase()),
        "the constructed clause must ROUTE, or it would not demonstrate the \
         guard is load-bearing for a judged claim"
    );
    // And the shape that IS a count still parses as one.
    let real = part_claims("489 Lean refinement theorems in contracts/lean/");
    assert_eq!(real.len(), 1);
    assert_eq!(real[0].claimed, 489);
    assert_ne!(
        real[0].preceded_by, "lean",
        "a real tally must not be swallowed by the version guard"
    );
}

/// The needle rejections named in [`part_claims`] are the corpus's own shapes,
/// so each is pinned against the text that produced it. Both live under
/// `contracts/`, which this gate's subject deliberately excludes — so unlike
/// the version guard these are not reachable from the judged corpus at all,
/// and this test is the ONLY thing that exercises them.
#[test]
fn the_part_needle_rejects_identifiers_and_fractions() {
    for (text, why) in [
        (
            "- PMAT-353 equations_block_struct_extensionality: inner equations",
            "an id followed by a snake_case identifier is not `<N> equations`",
        ),
        (
            "Brings Silver coverage on this contract from 1/9 to 4/9 equations.",
            "a coverage FRACTION is scoped to a slice, not a total",
        ),
    ] {
        assert!(
            part_claims(text).is_empty(),
            "part_claims fabricated a claim from {text:?} — {why}"
        );
    }
    // The bare form the same file uses IS seen. It is exempted downstream, by
    // routing — not by the needle failing to notice it. A shape the needle
    // cannot see is a shape nobody can reason about.
    let seen = part_claims("Bronze-tier refinement theorems for the remaining 8 equations");
    assert_eq!(seen.len(), 1, "the bare `8 equations` form must be SEEN");
    assert_eq!(seen[0].claimed, 8);
}

/// EVERY published count attributed to `contracts/lean/` or `contracts/kani/`
/// equals what that directory holds.
///
/// PMAT-1455 — `docs/specifications/sub/kaizen-fleet.md`, the sub-spec
/// `xpile-spec.md` §20 points at for fleet membership, published the
/// 2026-05-18 snapshot in the PRESENT TENSE: "The translation contracts
/// produce verifiable kernels by construction: 260 Lean refinement theorems …
/// in `contracts/lean/` + 43 Kani BMC harnesses in `contracts/kani/` = 303
/// stratum-vote artifacts", and "every contract in the substrate (12 of 12)
/// has a Kani BMC harness on every PR". Live: 489, 95, 584, and 24 of 35 — so
/// the tally understated by ~2x while the COVERAGE claim overstated, asserting
/// a harness for eleven contracts that have none.
///
/// ⭐ AND THE REPLACEMENT NUMBER IS ALSO A GREP ARTEFACT IF YOU LET IT BE.
/// `docs/specifications/sub/sprint-6day-2026-07-26.md` — the ratified plan,
/// under the heading "What the note may honestly claim", i.e. the text
/// scheduled to become Friday's release body — said "101 Kani harnesses". That
/// is `grep -c '#\[kani::proof\]'`, and six of those 101 occurrences are
/// inside `//!` doc comments explaining what a harness is. 95 are attributes.
/// The same shape inflates the Lean side by 23. See `lean_theorem_
/// declarations` and `kani_proof_harnesses`: both strip comments, because the
/// naive count is the one that gets published.
#[test]
fn published_substrate_proof_volume_matches_the_contract_tree() {
    let lean = lean_theorem_declarations();
    let kani = kani_proof_harnesses();
    let votes = lean + kani;
    let parts = contract_parts();
    let with_kani = parts.values().filter(|p| p.kani_citations > 0).count();
    let contracts = parts.len();
    assert!(
        lean >= 100 && kani >= 10 && with_kani >= 1 && with_kani < contracts,
        "derived substrate looks wrong (lean {lean}, kani {kani}, \
         {with_kani}/{contracts} contracts with a harness) — a directory \
         moved; fix the derivation, not the prose"
    );

    let mut offences = Vec::new();
    let mut agreements: HashMap<Part, usize> = HashMap::new();
    let mut coverage_agreements = 0usize;

    for (rel, body) in claim_pages() {
        for para in paragraphs_under_headings(&body) {
            if para.heading.contains(HISTORICAL_MARKER) {
                continue;
            }
            let quoted = quoted_spans(&para.flat);
            for claim in part_claims(&para.flat) {
                if quoted.iter().any(|&(a, b)| a <= claim.at && claim.at < b) {
                    continue;
                }
                if claim.preceded_by == "lean" {
                    continue; // the version numeral; pinned by the sibling test
                }
                let (clo, chi) = clause_bounds(&para.flat, claim.at);
                let clause = &para.flat[clo..chi];
                let clause_lower = clause.to_ascii_lowercase();
                if !names_derivation_dir(&clause_lower) {
                    continue;
                }
                let (live, what) = match claim.part {
                    Part::LeanTheorems => (lean, "Lean theorem/lemma declarations"),
                    Part::KaniHarnesses => (kani, "`#[kani::proof]` harnesses"),
                    Part::StratumVotes => (votes, "stratum-vote artifacts"),
                    // `N equations` next to a directory pointer is a
                    // per-contract claim that happens to cite the Lean file;
                    // the sibling test owns it.
                    Part::Equations => continue,
                };
                if claim.claimed == live {
                    *agreements.entry(claim.part).or_default() += 1;
                    continue;
                }
                let (line, _) = para.line_at(claim.at);
                offences.push(format!(
                    "{rel}:{line}: attributes {} {what} to the substrate, which \
                     holds {live} — \"{}\"",
                    claim.claimed,
                    clause.trim()
                ));
            }
            // The COVERAGE half. `12 of 12` and `24/35 contracts` are the same
            // claim in two spellings, and the first one was the dangerous
            // direction: a harness asserted for every contract when eleven
            // have none.
            for (num, den, at) in coverage_fractions(&para.flat) {
                if quoted.iter().any(|&(a, b)| a <= at && at < b) {
                    continue;
                }
                let (clo, chi) = clause_bounds(&para.flat, at);
                let clause = &para.flat[clo..chi];
                let lower = clause.to_ascii_lowercase();
                if !(lower.contains("kani") && lower.contains("contracts/kani")) {
                    continue;
                }
                if num == with_kani && den == contracts {
                    coverage_agreements += 1;
                    continue;
                }
                let (line, _) = para.line_at(at);
                offences.push(format!(
                    "{rel}:{line}: says {num} of {den} contracts carry a Kani \
                     harness; {with_kani} of {contracts} do — \"{}\"",
                    clause.trim()
                ));
            }
        }
    }

    assert!(
        offences.is_empty(),
        "a published substrate tally does not match `contracts/` \
         (contracts/lean: {lean} declarations, contracts/kani: {kani} \
         harnesses over {with_kani} of {contracts} contracts). Update the \
         prose — and if the number came from a grep, strip the comments \
         first:\n{}",
        offences.join("\n")
    );
    for part in [Part::LeanTheorems, Part::KaniHarnesses, Part::StratumVotes] {
        assert!(
            agreements.contains_key(&part),
            "no AGREEING {part:?} tally was found in the corpus, so that arm is \
             unexercised. The substrate's size is worth stating somewhere; if \
             the last statement of it was deleted, this gate is ranging over \
             nothing"
        );
    }
    assert!(
        coverage_agreements >= 1,
        "no `N of M contracts` Kani-coverage claim was found, so the coverage \
         arm — the one that caught `12 of 12` where 24 of 35 hold — is \
         unreachable and untested"
    );
}

/// `24 of 35 contracts` / `24/35 contracts` → `(24, 35, offset of the first
/// numeral)`. Only the two spellings the corpus uses; a bare `24 contracts` is
/// deliberately not a coverage claim.
fn coverage_fractions(flat: &str) -> Vec<(usize, usize, usize)> {
    /// Trailing decimal run of `s`, as `(value, byte offset it starts at)`.
    fn trailing_number(s: &str) -> Option<(usize, usize)> {
        let head = s.trim_end_matches(|c: char| c.is_ascii_digit());
        if head.len() == s.len() {
            return None;
        }
        s[head.len()..].parse().ok().map(|v| (v, head.len()))
    }
    const PAD: [char; 4] = [' ', '*', '_', '`'];
    let lower = flat.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find("contracts") {
        let at = from + rel;
        from = at + "contracts".len();
        // `24 of the 35` / `24 of 35` / `24/35` / `24 out of 35`, reading
        // right-to-left from the noun.
        let head = lower[..at].trim_end_matches(PAD);
        let Some((den, den_at)) = trailing_number(head) else {
            continue;
        };
        let sep = head[..den_at].trim_end_matches(PAD);
        let Some(sep) = ["of the", "out of", "of", "/"]
            .iter()
            .find_map(|c| sep.strip_suffix(c))
            .map(|s| s.trim_end_matches(PAD))
        else {
            continue;
        };
        let Some((num, num_at)) = trailing_number(sep) else {
            continue;
        };
        out.push((num, den, num_at));
    }
    out
}

// ── PMAT-1457: ORDINAL-FINALITY claims about substrate populations ───
//
// PMAT-1456 repaired four "TWELFTH and FINAL" sentences by hand and wrote
// its own lesson down: *"FINAL is a claim about the FUTURE and a citation
// cannot date it. Superlatives need a live count, not a date. Still ungated
// — standing lead."* It then EDITED `contracts/lean/FfiCpythonExt.lean` (22
// lines in 06d71142) and walked past the `**TWELFTH and FINAL**` sitting 120
// lines further into that same file. NINE more ordinal-finality claims stood
// elsewhere under `contracts/`, and `cargo test -p xpile --test claims_drift`
// was GREEN on all 32 tests at 06d71142 with every one of them published.
// Same family as PMAT-1443's "a gate's SUBJECT can be narrower than its
// RULE", one level up: the SWEEP was narrower than the rule it wrote down.
//
// The class is checkable, and that is the point. **"The Nth and FINAL X" is
// honest exactly when N equals the LIVE size of X.** Nothing about the future
// is required: the claim dates itself the moment an (N+1)th lands, and a gate
// that re-derives |X| on every run is what turns an unfalsifiable boast into
// a checked one. A record exemption keyed on citing a PMAT id would launder
// every one of these, because they all DO cite one.
//
// ⭐ THE FINDING IS THE MIXED SENTENCE. Four of the ten live claims are TRUE
// and stay untouched — and two of the true ones share a SENTENCE with a false
// totality clause. `contracts/lean/FfiCpythonExt.lean` says "SIXTH and FINAL
// Silver theorem on C-FFI-CPYTHON-EXT" (live: 6 — correct) and in the next
// breath "With this landed, every equation in C-FFI-CPYTHON-EXT has
// Silver-tier coverage" (live: `manifest_completeness` still binds a Bronze
// theorem, 1 of 22). **The ordinal froze the NUMERATOR while the substrate
// grew the DENOMINATOR** — C-FFI-CPYTHON-EXT went 6 equations → 22 and
// C-PY-INT-ARITH went 9 → 42 — so a coverage claim flipped from true to false
// with nobody editing it, laundered by a correct ordinal beside it.
//
// FOUR POPULATIONS, each DERIVED from the artifact the claim's own words
// point at (PMAT-1455's routing rule), and each carrying at least one live
// claim that PASSES — so this is a measurement, not a keyword ban:
//
//   Path α                  10  ← `TENTH and final` PASSES, `FIFTH` reds ×2
//   trait-idempotency stubs  4  ← `FOURTH and FINAL` PASSES
//   Silver eqs on FFI        6  ← `SIXTH and FINAL` PASSES ×2
//   contracts Lean-refined  35  ← `TWELFTH and FINAL` reds
//   multi-eq at full Silver  0  ← `SIXTH and FINAL` reds ×3
//
// There is deliberately NO quotation/denial exemption. The repairs this
// slice writes DESCRIBE the retired boast ("this line used to assert
// finality") instead of reproducing the phrase, so the branch does not exist
// and cannot rot untested — PMAT-1456 lost two guards to their own red
// halves for exactly that reason. A future editor who needs to quote one
// must add the branch AND its red half.

/// Written-out ordinals; `ORDINALS[i]` names position `i + 1`.
const ORDINALS: [&str; 13] = [
    "first",
    "second",
    "third",
    "fourth",
    "fifth",
    "sixth",
    "seventh",
    "eighth",
    "ninth",
    "tenth",
    "eleventh",
    "twelfth",
    "thirteenth",
];

/// Emphasis and code marks the corpus writes both AROUND the phrase and
/// BETWEEN its words (`**TWELFTH and FINAL**`), so they are padding here, not
/// delimiters. Dropping them is what lets the Lean corpus — which bolds every
/// one of these — be scanned at all.
const EMPHASIS: [char; 3] = ['*', '_', '`'];

/// The next word of `s`, plus the remainder. A word is a run of alphanumerics
/// and `-`; leading spaces and [`EMPHASIS`] are skipped, and trailing
/// punctuation (`final**`, `final,`) is not part of it.
fn word_after(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start_matches(|c: char| c == ' ' || EMPHASIS.contains(&c));
    let end = s
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .unwrap_or(s.len());
    if end == 0 {
        return None;
    }
    Some((&s[..end], &s[end..]))
}

/// Every `(position, byte offset of the ordinal)` in `clause` written as an
/// ORDINAL-FINALITY claim: an ordinal followed by `and final` / `and the
/// final` / `and last`.
///
/// ADJACENCY IS THE SHAPE, not a proximity heuristic — the corpus writes the
/// conjunction literally, and PMAT-1456 deleted a distance-window guard that
/// turned out to bound nothing. So there is no window parameter to tune: two
/// words, checked by name.
///
/// A bare ordinal is NOT a claim of this class and must not be reported. Both
/// of these are real text in the corpus and each is honest:
///   * `becomes the first contract in the substrate at FULL Silver tier` —
///     an ordinal with no finality conjunction.
///   * `Path α, fourth contract` — a position with no claim about the last.
fn ordinal_finality_claims(clause: &str) -> Vec<(usize, usize)> {
    let lower = clause.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut out = Vec::new();
    for (i, ord) in ORDINALS.iter().enumerate() {
        let mut from = 0usize;
        while let Some(rel) = lower[from..].find(ord) {
            let at = from + rel;
            from = at + ord.len();
            // Word boundary on the head, so `seconds`/`fourthly` cannot pose
            // as an ordinal.
            // ⚠️ THE JUSTIFICATION HERE IS NOT THE OBVIOUS ONE, and the red
            // half is what established that. This does NOT reject `seconds`
            // — the TAIL scan does, because the word after `second` is `s`,
            // not `and`. What it rejects is a COMPOUND ordinal:
            // `twenty-first and final` would otherwise be scored as position
            // 1 and reported against a series it names no position in.
            // `ORDINALS` stops at thirteenth precisely because compounds are
            // not representable, so refusing to read the unit part of one is
            // correctness, not hygiene. Pinned by the `twenty-first` case in
            // `the_ordinal_finality_needle_reads_the_spellings_the_corpus_writes`,
            // which reds when this is removed. Removing it changed NOTHING
            // in the live corpus.
            if at > 0 && (bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'-') {
                continue;
            }
            let Some((conj, rest)) = word_after(&lower[at + ord.len()..]) else {
                continue;
            };
            if conj != "and" {
                continue;
            }
            let Some((mut tail, rest)) = word_after(rest) else {
                continue;
            };
            if tail == "the" {
                let Some((t, _)) = word_after(rest) else {
                    continue;
                };
                tail = t;
            }
            if tail == "final" || tail == "last" {
                out.push((i + 1, at));
            }
        }
    }
    out.sort_unstable();
    out
}

/// The SERIES an ordinal-finality claim counts itself within.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Series {
    /// "Path α": the Silver-tier property-specific Kani harness programme
    /// that closes `audit-design.md` §4's byte-identity-placeholder caveat.
    PathAlpha,
    /// Primary idempotency stubs of the 2×2 trait-determinism matrix.
    TraitIdempotency,
    /// Multi-equation contracts discharged at Silver tier or better on EVERY
    /// equation.
    FullSilverMultiEq,
    /// Silver-tier equations on the contract the clause names.
    SilverOn(String),
    /// Contracts that have received a Lean refinement theorem.
    LeanRefined,
}

/// Route a claim to its series by the words the CLAUSE ITSELF uses.
///
/// `multi-eq` is tested before `silver theorem` so a claim carrying both is
/// scored against the substrate-wide population rather than one contract's
/// own tally. ⚠️ THAT ORDER BOUNDS NOTHING IN TODAY'S CORPUS, and the red
/// half is what established it: swapping the two rules leaves the whole suite
/// green, because the live `multi-eq` clause never spells "silver theorem" so
/// the earlier rule has nothing to steal. It is kept as a forward tripwire
/// and pinned by a CONSTRUCTED clause in
/// `the_finality_router_sends_each_live_claim_to_its_own_series` — the
/// arrangement the corpus lacks — rather than claimed to be load-bearing
/// today. Second guard in this slice whose obvious justification its own red
/// half refuted; see the head-boundary check in
/// [`ordinal_finality_claims`] for the first.
///
/// `multi-eq` additionally requires `silver`, or the phrase alone would
/// capture any future claim about multi-equation contracts.
fn series_of(clause: &str) -> Option<Series> {
    let lower = clause.to_ascii_lowercase();
    if lower.contains("path α") || lower.contains("path alpha") {
        return Some(Series::PathAlpha);
    }
    if lower.contains("trait-idempotency") {
        return Some(Series::TraitIdempotency);
    }
    if lower.contains("multi-eq") && lower.contains("silver") {
        return Some(Series::FullSilverMultiEq);
    }
    if lower.contains("silver theorem") {
        return contract_ids(clause)
            .into_iter()
            .next()
            .map(Series::SilverOn);
    }
    if lower.contains("refinement theorem") {
        return Some(Series::LeanRefined);
    }
    None
}

/// Live sizes of every series this gate can derive. Re-derived on each run
/// from the contract tree — never a literal, so an eleventh Path α harness
/// reds the `TENTH and final` claim the moment it lands.
struct LiveSeries {
    path_alpha: usize,
    trait_stubs: usize,
    full_silver_multi_eq: usize,
    silver: HashMap<String, usize>,
    lean_refined: usize,
}

/// A `lean_theorem:` naming a discharge at Silver tier or better.
///
/// The generous reading, deliberately: a Gold/Platinum/Diamond theorem
/// SUBSUMES Silver, so scoring "full Silver coverage" against `_silver`
/// suffixes alone would report a contract as short of Silver because it went
/// PAST it. Measured both ways while writing this — `full_silver_multi_eq` is
/// 0 under either — and the generous one is the one that cannot fabricate a
/// finding.
fn is_silver_or_better(theorem: &str) -> bool {
    ["_silver", "_gold", "_platinum", "_diamond"]
        .iter()
        .any(|suffix| theorem.ends_with(suffix))
}

fn live_series() -> LiveSeries {
    // Path α membership is the BLOCK HEADER each member carries, not the
    // words "Path α" — five of the ten files never spell the programme's
    // name. Counted per FILE: a member that grew a second Silver block is
    // still one contract.
    let dir = workspace_root().join("contracts/kani");
    let mut path_alpha = std::collections::HashSet::new();
    let mut entries: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("rs"))
        .collect();
    entries.sort();
    for p in &entries {
        let body = fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
        if body.lines().any(|l| {
            l.contains("PMAT-") && l.contains("Silver-tier property-specific Kani harness")
        }) {
            path_alpha.insert(p.file_name().expect("file").to_string_lossy().into_owned());
        }
    }

    let dir = workspace_root().join("contracts");
    let mut yamls: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("yaml"))
        .collect();
    yamls.sort();
    let (mut trait_stubs, mut full_silver_multi_eq, mut lean_refined) = (0usize, 0usize, 0usize);
    let mut silver: HashMap<String, usize> = HashMap::new();
    for p in &yamls {
        let body = fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
        let doc: serde_yaml::Value = serde_yaml::from_str(&body)
            .unwrap_or_else(|e| panic!("{} is not valid YAML: {e}", p.display()));
        let Some(id) = doc
            .get("metadata")
            .and_then(|m| m.get("id"))
            .and_then(|v| v.as_str())
        else {
            continue;
        };
        // The 2×2 trait-determinism matrix is {plain, Contract} ×
        // {Frontend, Backend}, one primary idempotency stub each. Derived
        // from the contract ids so a fifth trait contract reds the claim.
        if id.starts_with("C-XPILE-") && id.ends_with("-TRAIT") {
            trait_stubs += 1;
        }
        let Some(eqs) = doc.get("equations").and_then(|e| e.as_mapping()) else {
            continue;
        };
        let theorems: Vec<&str> = eqs
            .iter()
            .filter_map(|(_, eq)| eq.get("lean_theorem").and_then(|v| v.as_str()))
            .collect();
        if !theorems.is_empty() {
            lean_refined += 1;
        }
        silver.insert(
            id.to_string(),
            theorems.iter().filter(|t| t.ends_with("_silver")).count(),
        );
        if eqs.len() > 1
            && theorems.len() == eqs.len()
            && theorems.iter().all(|t| is_silver_or_better(t))
        {
            full_silver_multi_eq += 1;
        }
    }
    LiveSeries {
        path_alpha: path_alpha.len(),
        trait_stubs,
        full_silver_multi_eq,
        silver,
        lean_refined,
    }
}

impl Series {
    /// `(live size, how it was derived)` — the second half goes into the
    /// failure message so a reader can check the arithmetic without reading
    /// this file.
    fn live(&self, live: &LiveSeries) -> Option<(usize, String)> {
        match self {
            Series::PathAlpha => Some((
                live.path_alpha,
                "contracts/kani/*.rs carrying a `PMAT-NNN: Silver-tier \
                 property-specific Kani harness` block header"
                    .to_string(),
            )),
            Series::TraitIdempotency => Some((
                live.trait_stubs,
                "`C-XPILE-*-TRAIT` contracts (the trait-determinism matrix)".to_string(),
            )),
            Series::FullSilverMultiEq => Some((
                live.full_silver_multi_eq,
                "multi-equation contracts whose EVERY equation binds a \
                 Silver-or-better `lean_theorem:`"
                    .to_string(),
            )),
            Series::SilverOn(id) => live
                .silver
                .get(id)
                .map(|n| (*n, format!("`{id}` equations binding a `…_silver` theorem"))),
            Series::LeanRefined => Some((
                live.lean_refined,
                "contracts binding at least one `lean_theorem:`".to_string(),
            )),
        }
    }
}

/// What the scan found, split so each half can be asserted on separately.
#[derive(Default)]
struct FinalityReport {
    /// `Nth and FINAL` where N ≠ the live size.
    offences: Vec<String>,
    /// `Nth and FINAL` where N == the live size — the claims that PASS.
    agreements: Vec<(Series, String)>,
    /// A finality claim naming a series this gate cannot derive.
    unrouted: Vec<String>,
}

/// Scan `pages` for ordinal-finality claims and score each against `live`.
///
/// Pure in `live` so the behaviour half can hand it a perturbed population
/// and prove this gate tracks the SUBSTRATE rather than the text.
fn finality_report(pages: &[(String, String)], live: &LiveSeries) -> FinalityReport {
    let mut report = FinalityReport::default();
    for (rel, body) in pages {
        for block in substrate_blocks(rel, body) {
            for (clause, base) in clauses_with_offsets(&block.flat) {
                for (position, off) in ordinal_finality_claims(clause) {
                    let n = block.line_at(base + off);
                    let Some(series) = series_of(clause) else {
                        report
                            .unrouted
                            .push(format!("{rel}:{n}: {}", excerpt(&block.flat, base + off)));
                        continue;
                    };
                    let Some((size, how)) = series.live(live) else {
                        report
                            .unrouted
                            .push(format!("{rel}:{n}: {}", excerpt(&block.flat, base + off)));
                        continue;
                    };
                    if position == size {
                        report.agreements.push((series, format!("{rel}:{n}")));
                    } else {
                        report.offences.push(format!(
                            "{rel}:{n}: claims position {position} is the LAST of {how}, \
                             which live holds {size} — {}",
                            excerpt(&block.flat, base + off)
                        ));
                    }
                }
            }
        }
    }
    report
}

#[test]
fn ordinal_finality_claims_match_the_live_population() {
    let live = live_series();
    // Anti-vacuity on every derivation BEFORE it is used to judge anything:
    // a narrowing that takes one of these to 0 would otherwise convert this
    // gate into a silent pass for the series it can no longer see.
    assert!(
        live.path_alpha >= 5 && live.trait_stubs == 4 && live.lean_refined >= 12,
        "a series derivation collapsed: Path α = {}, trait stubs = {}, \
         Lean-refined contracts = {} — re-check the derivations before \
         trusting this gate's verdict",
        live.path_alpha,
        live.trait_stubs,
        live.lean_refined
    );
    let pages = provable_artifact_pages();
    let report = finality_report(&pages, &live);

    assert!(
        report.offences.is_empty(),
        "{} ordinal-finality claim(s) under `contracts/` name a position that \
         is no longer the last of their own series — {}. \"The Nth and FINAL \
         X\" is honest exactly when N is the live |X|; write the ordinal \
         without the finality, or say which past substrate it was final in.",
        report.offences.len(),
        report.offences.join("; ")
    );
    assert!(
        report.unrouted.is_empty(),
        "ordinal-finality claim(s) name a series this gate cannot derive — \
         {}. A finality claim that nothing counts is exactly the shape \
         PMAT-1457 exists to stop: add a `Series` arm with a derivation, or \
         do not claim finality.",
        report.unrouted.join("; ")
    );

    // The claims that PASS are the proof this is a measurement and not a ban
    // on the word FINAL. All four derivable series must be exercised by a
    // LIVE agreeing claim, or a series is being scored against nothing.
    for want in [
        Series::PathAlpha,
        Series::TraitIdempotency,
        Series::SilverOn("C-FFI-CPYTHON-EXT".to_string()),
    ] {
        assert!(
            report.agreements.iter().any(|(s, _)| *s == want),
            "no live ordinal-finality claim agrees with the {want:?} series, \
             so that arm is unreachable and its derivation is untested. \
             Agreements found: {:?}",
            report.agreements
        );
    }
}

#[test]
fn the_finality_gate_tracks_the_population_not_the_text() {
    // THE BEHAVIOUR HALF. Perturb the DERIVED population, not the prose: an
    // eleventh Path α harness must red `TENTH and final`, and a seventh
    // Silver equation on C-FFI-CPYTHON-EXT must red `SIXTH and FINAL`. If
    // this stayed green, the gate would be reading the text and agreeing
    // with itself.
    let pages = provable_artifact_pages();
    let mut live = live_series();
    assert!(
        finality_report(&pages, &live).offences.is_empty(),
        "control: the corpus must be clean at the LIVE population before a \
         perturbation means anything"
    );

    live.path_alpha += 1;
    let grown = finality_report(&pages, &live);
    assert!(
        grown
            .offences
            .iter()
            .any(|o| o.contains("xpile_contract_backend_trait.rs")),
        "an eleventh Path α harness left the `TENTH and final` claim green — \
         the gate is not tracking the population. Offences: {:?}",
        grown.offences
    );
    live.path_alpha -= 1;

    *live
        .silver
        .get_mut("C-FFI-CPYTHON-EXT")
        .expect("C-FFI-CPYTHON-EXT is in the live table") += 1;
    let grown = finality_report(&pages, &live);
    assert!(
        grown.offences.iter().any(|o| o.contains("FfiCpythonExt")),
        "a seventh Silver equation on C-FFI-CPYTHON-EXT left the `SIXTH and \
         FINAL Silver theorem` claim green. Offences: {:?}",
        grown.offences
    );
}

#[test]
fn the_ordinal_finality_needle_reads_the_spellings_the_corpus_writes() {
    for (text, want, why) in [
        (
            "This is the **TWELFTH and FINAL** contract to receive a refinement theorem",
            vec![12usize],
            "the Lean corpus bolds the whole phrase, so `*` must be padding",
        ),
        (
            "Path α extension to a TENTH and final trait-Kani contract",
            vec![10],
            "shouted ordinal, lower-case `final`",
        ),
        (
            "the FOURTH and FINAL primary trait-idempotency stub",
            vec![4],
            "the live trait-matrix claim",
        ),
        (
            "the sixth and the last of them",
            vec![6],
            "`and the last` is the same claim",
        ),
        (
            "becomes the first contract in the substrate at FULL Silver tier",
            vec![],
            "an ordinal with no finality conjunction is a POSITION, not a \
             claim about the last — and this exact sentence is live in \
             contracts/ffi-cpython-ext-v1.yaml",
        ),
        (
            "Property-specific Silver-tier Kani harnesses (Path α, fourth contract)",
            vec![],
            "a bare position; the corpus writes six of these and none is a \
             finality claim",
        ),
        (
            "the FINAL FIVE equations on C-XLATE-LEAN-TO-RUST",
            vec![],
            "`FINAL` without an ordinal before `and` is a different claim \
             class and is live in contracts/lean/XlateLeanToRust.lean",
        ),
        (
            "twenty seconds and final cleanup",
            vec![],
            "`seconds` is not the ordinal `second`. Rejected by the TAIL scan \
             (the next word is `s`, not `and`), NOT by the head boundary — \
             measured, because the obvious reading is the wrong one",
        ),
        (
            "the twenty-first and final entry",
            vec![],
            "a COMPOUND ordinal must not be read as its unit part: scoring \
             this as position 1 would report a wrong position against a \
             series it names no position in. THIS is what the head boundary \
             check buys, and this case reds when it is removed",
        ),
        (
            "the catch-all is the final else",
            vec![],
            "`final` as an ordinary adjective; live in bashrs-frontend",
        ),
    ] {
        let got: Vec<usize> = ordinal_finality_claims(text)
            .into_iter()
            .map(|(p, _)| p)
            .collect();
        assert_eq!(got, want, "{why} — on {text:?}");
    }
}

#[test]
fn the_finality_router_sends_each_live_claim_to_its_own_series() {
    // Routing is by the CLAUSE's own words, and the order of the rules is
    // what keeps two Silver-flavoured series apart. Each string below is the
    // live clause, so a re-ordering that breaks one shows up here rather
    // than as a silently mis-scored claim.
    for (clause, want, why) in [
        (
            "Path α extension to a TENTH and final trait-Kani contract",
            Some(Series::PathAlpha),
            "names the programme",
        ),
        (
            "and the FOURTH and FINAL primary trait-idempotency stub, completing the 2×2 \
             trait-determinism matrix",
            Some(Series::TraitIdempotency),
            "the trait matrix, which also says nothing about Silver",
        ),
        (
            "COMPLETES Silver coverage on C-PY-INT-ARITH (9/9) — SIXTH and FINAL multi-eq \
             contract at full Silver",
            Some(Series::FullSilverMultiEq),
            "carries a contract id AND the word Silver — but NOT the phrase \
             `silver theorem`, so it does not on its own exercise the rule \
             order; the constructed clause below does",
        ),
        (
            "the SIXTH and FINAL multi-eq contract at full Silver, matching the Silver \
             theorem on C-FFI-CPYTHON-EXT",
            Some(Series::FullSilverMultiEq),
            "CONSTRUCTED — the corpus writes no clause carrying both routes, \
             so without this the rule order is untested and swapping it \
             leaves the suite green (measured). A claim about the \
             substrate-wide population must not be scored against one \
             contract's own Silver tally",
        ),
        (
            "SIXTH and FINAL Silver theorem on C-FFI-CPYTHON-EXT — wires the last \
             previously-unwired equation",
            Some(Series::SilverOn("C-FFI-CPYTHON-EXT".to_string())),
            "per-contract Silver tally, read off the id the clause names",
        ),
        (
            "This is the **TWELFTH and FINAL** contract to receive a refinement theorem",
            Some(Series::LeanRefined),
            "the substrate-wide Lean population",
        ),
        (
            "the NINTH and FINAL widget in the shed",
            None,
            "an underivable series must be REPORTED, never scored",
        ),
    ] {
        assert_eq!(series_of(clause), want, "{why} — on {clause:?}");
    }
}

// ── PMAT-1458: the SUBSTRATE SIZE named inside a totality claim ───────
//
// PMAT-1457 closed the ordinal class and named the next one in its own
// notes: *"the per-contract COVERAGE FRACTION — 'every equation at tier T',
// 'N/N' — is derivable exactly the way the ordinal now is, and three live
// sites were repaired BY HAND here."* This is that class, taken at the level
// where it is decidable without arguing about tier semantics.
//
// ⭐ THE NUMBER THAT FROZE IS THE DENOMINATOR, AND IT IS THE POPULATION.
// PMAT-1457's finding was that an ordinal froze the NUMERATOR while the
// substrate grew the DENOMINATOR. One level up, the substrate writes the
// denominator down: `12 contracts`. It has thirty-five. Measured at
// b4b9fc6e, with `cargo test -p xpile --test claims_drift` GREEN on all 35
// tests, this needle finds 64 census claims under `contracts/`: **62 of them
// name a 12-contract substrate**, and 2 name 35. Three of the 62 sit in
// NORMATIVE `invariants:`/`postconditions:` slots, where PMAT-1454
// established there is no record exemption at all. The other 59 are cited
// provenance and are honest records of a smaller substrate — they stay.
//
// ⚠️ AND EVERY ONE OF THE THREE IS TRUE IN ITS TOTALITY HALF. That is what
// made them survive sixty-odd sentences' worth of re-reading:
//
//   "every one of the 12 contracts in the substrate now has at least one
//    Diamond theorem"        → live: `xpile diamond --json` min = 1 over 35
//   "ALL 12 contracts … have at least one Gold-tier refinement theorem"
//                            → live: 35 of 35 bind a Gold-or-better theorem
//   "Gold-tier coverage is now universal across the substrate (12/12)"
//
// A reader checks the CLAIM — *is it universal?* — gets `yes`, and never
// checks the CENSUS riding along inside it. The true half laundered the
// stale half, exactly as a correct ordinal laundered a false coverage clause
// in PMAT-1457. The repairs below keep all three claims: two drop the census
// (an `invariants:` entry is about its equation, not about how big the
// substrate is), and one states the LIVE fraction so this gate maintains it.
//
// THE RULE. **A number that names |substrate| must equal the live
// |substrate|.** Three spellings, because the corpus writes three:
//   * `all N contracts` / `every one of the N contracts` — the totality;
//   * `N/M contracts` — the fraction, M is the census;
//   * `N of M contracts` — the same fraction, spelled out.
// A BARE `N contracts` is deliberately NOT in the class: `2 contracts at
// depth-5+` is a subset count, not a census, and reading it as one would
// report true sentences as drift.
//
// EXEMPTION — the same one PMAT-1454 established for this corpus, not a new
// one: a NORMATIVE field must be live-true with no way out, and a PROVENANCE
// block is exempt iff it cites the slice it narrates. `PMAT-336 … ACROSS ALL
// 12 CONTRACTS` is an honest record of a 12-contract substrate. This gate
// does NOT rewrite history, and it must not: 59 cited records name 12 and
// stay exactly as they are.
//
// ⚠️ THE UNCITED BRANCH BITES NOTHING TODAY — measured, not assumed: every
// provenance block under `contracts/` that names a census also carries a
// `PMAT-NNN`, so it reports 0. It is a lower bound kept for the next
// uncited record, and `an_uncited_census_record_is_reported_and_a_cited_one_
// is_not` pins that it CAN fire in BOTH directions rather than leaving an
// untested branch to rot (PMAT-1451's rule: a carve-out that is not checked
// is a hole).
//
// TWO LIVE CLAIMS AGREE and they are the proof this measures rather than
// bans, one per spelling — both in `contracts/README.md`, neither planted:
//   `covering 24 of 35 contracts`   (fraction)
//   `All 35 contracts sit at Bronze` (totality)
//
// ⚠️ SCOPE, MEASURED RATHER THAN ASSUMED. The corpus also writes `ALL 5
// LAYERS` 71 times and it is TRUE — but `layer` is not a field of a
// contract. It is a sentence inside `metadata.description`, and only 6 of
// the 35 descriptions spell it, so the population is not derivable and a
// gate that guessed at it would be worse than none. Taxonomy-layer census
// claims are therefore OUT of this gate's subject, said here rather than
// left for a reader to infer from a needle that happens not to match.

/// The live number of contracts in the substrate: `contracts/*.yaml`
/// carrying a `metadata.id`.
///
/// Re-derived on every run — never a literal — so the thirty-sixth contract
/// reds every sentence that still says thirty-five.
fn live_contract_count() -> usize {
    let dir = workspace_root().join("contracts");
    let mut n = 0usize;
    let mut entries: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("yaml"))
        .collect();
    entries.sort();
    for p in &entries {
        let body = fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
        let doc: serde_yaml::Value = serde_yaml::from_str(&body)
            .unwrap_or_else(|e| panic!("{} is not valid YAML: {e}", p.display()));
        if doc
            .get("metadata")
            .and_then(|m| m.get("id"))
            .and_then(|v| v.as_str())
            .is_some()
        {
            n += 1;
        }
    }
    n
}

/// True iff the population noun ending at `at + len` is not glued to a word
/// character or a hyphen on its RIGHT — so `contracts` matches `contracts.`
/// and `CONTRACTS**` but `contract-lane` is a different population.
///
/// ⚠️ TRAILING ONLY, AND THAT IS A MEASUREMENT. This was written with a
/// symmetric leading check too, on the obvious reasoning that `subcontracts`
/// and `then-12-contract` must not match. Its red half came back GREEN: the
/// backward walk in [`substrate_size_claims`] requires a space or emphasis
/// and THEN digits, so a noun glued to a word character can never reach a
/// number anyway and the leading branch was unreachable code dressed as a
/// guard. Deleted rather than downgraded. The trailing half IS load-bearing
/// and is pinned by the `contract-lane` case in
/// `the_census_needle_reads_the_spellings_the_corpus_writes`, which reds when
/// it goes — the third guard this slice measured and the second whose obvious
/// justification was the wrong one.
fn noun_ends_cleanly(bytes: &[u8], at: usize, len: usize) -> bool {
    let end = at + len;
    end >= bytes.len() || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'-')
}

/// True iff `head` ends in a decimal number that is not glued to a word
/// character or a hyphen on ITS left.
///
/// ⚠️ THIS IS THE GUARD THAT MADE THE FRACTION SPELLING SHIPPABLE, and it was
/// a live false positive rather than a hypothetical: `xpile-backend-trait-v1
/// .yaml:241` says *"language's Layer-1/2 contracts via meta-HIR"*, and
/// without this the `1/2` reads as a fraction and the gate reports a
/// two-contract substrate in a normative field. Same shape kills `depth-3/4
/// contracts` and `PMAT-194/195 contracts`. Pinned by the `Layer-1/2` case in
/// `the_census_needle_reads_the_spellings_the_corpus_writes`, which reds when
/// this returns a bare `true`.
fn trailing_number_is_free(head: &str) -> bool {
    let digits = head.len() - head.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return false;
    }
    let before = &head[..head.len() - digits];
    !before.ends_with(|c: char| c.is_ascii_alphanumeric() || c == '-')
}

/// True iff `head` ends with `word` as a whole word — so `install` does not
/// end with the quantifier `all`.
fn ends_with_word(head: &str, word: &str) -> bool {
    head.strip_suffix(word)
        .is_some_and(|before| !before.ends_with(|c: char| c.is_ascii_alphanumeric()))
}

/// Every `(named census, byte offset)` in `clause` that states HOW MANY
/// contracts the substrate holds.
///
/// The offset returned is the number's, so a failure message points at the
/// digits a repair has to change.
///
/// [`EMPHASIS`] is skipped where it can separate the words of a phrase, so
/// offsets stay TRUE offsets into `clause` — the caller needs them to name a
/// line, and PMAT-1457's habit of stripping the marks first would shift every
/// one of them.
fn substrate_size_claims(clause: &str) -> Vec<(usize, usize)> {
    let lower = clause.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut out = Vec::new();
    // Every occurrence of the population noun, scanned BACKWARDS for one of
    // the three census shapes. Anchoring on the NOUN rather than on `all` is
    // what keeps `all 5 layers` and `all four strata` out with no stop-list:
    // a shape that does not end in `contract(s)` is not a claim about this
    // population, by construction.
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find("contract") {
        let noun = from + rel;
        from = noun + "contract".len();
        let len = if lower[noun..].starts_with("contracts") {
            "contracts".len()
        } else {
            "contract".len()
        };
        if !noun_ends_cleanly(bytes, noun, len) {
            continue;
        }
        // Walk left over spaces and emphasis to the number qualifying it.
        let mut i = noun;
        while i > 0 && (bytes[i - 1] == b' ' || EMPHASIS.contains(&(bytes[i - 1] as char))) {
            i -= 1;
        }
        let mut j = i;
        while j > 0 && bytes[j - 1].is_ascii_digit() {
            j -= 1;
        }
        if j == i {
            continue; // no number in front of the noun at all
        }
        let Ok(census) = lower[j..i].parse::<usize>() else {
            continue;
        };
        // Suffix tests only — offsets do not matter past this point, so the
        // marks can simply go.
        let cleaned: String = lower[..j]
            .chars()
            .filter(|c| !EMPHASIS.contains(c))
            .collect();
        let head = cleaned.trim_end();
        // `N/M contracts` and `N of M contracts` — M is the census. The
        // NUMERATOR is deliberately NOT scored: "Silver coverage" has two
        // defensible readings (an exactly-`_silver` theorem, or any tier that
        // subsumes it — see `is_silver_or_better`), and a numerator scored
        // under the wrong one would fabricate a finding. A denominator that
        // names a population has exactly one reading, and it is the half that
        // actually froze.
        let fraction = head
            .strip_suffix('/')
            .is_some_and(|n| trailing_number_is_free(n.trim_end()));
        let spelled = head
            .strip_suffix("the")
            .map_or(head, str::trim_end)
            .strip_suffix(" of")
            .is_some_and(|n| trailing_number_is_free(n.trim_end()));
        // The totality, where N IS the census.
        let totality = ends_with_word(head, "all")
            || head.ends_with("all of the")
            || head.ends_with("every one of the")
            || head.ends_with("each of the");
        if fraction || spelled || totality {
            out.push((census, j));
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// What the census scan found, split so each half can be asserted on
/// separately — the live falsehood before any hygiene rule (PMAT-1451).
#[derive(Default)]
struct SizeReport {
    /// A NORMATIVE field naming a census that is not the live one.
    offences: Vec<String>,
    /// A PROVENANCE block naming a stale census without citing its slice.
    uncited: Vec<String>,
    /// A census that matches live — the claims that PASS.
    agreements: Vec<String>,
}

/// Scan `pages` for substrate-census claims and score each against `live`.
///
/// Pure in `live` so the behaviour half can hand it a perturbed population
/// and prove this tracks the SUBSTRATE rather than agreeing with the text.
fn size_report(pages: &[(String, String)], live: usize) -> SizeReport {
    let mut report = SizeReport::default();
    for (rel, body) in pages {
        for block in substrate_blocks(rel, body) {
            for (census, at) in substrate_size_claims(&block.flat) {
                let n = block.line_at(at);
                if census == live {
                    report.agreements.push(format!("{rel}:{n}"));
                    continue;
                }
                let what = format!(
                    "{rel}:{n}: names a {census}-contract substrate; live is \
                     {live} — {}",
                    excerpt(&block.flat, at)
                );
                if block.record {
                    if pmat_ids(&block.flat).is_empty() {
                        report.uncited.push(what);
                    }
                } else {
                    report.offences.push(what);
                }
            }
        }
    }
    report
}

#[test]
fn substrate_census_claims_name_the_live_contract_count() {
    let live = live_contract_count();
    // Anti-vacuity BEFORE the derivation judges anything: a walk that broke
    // would silently agree with every sentence in the corpus.
    assert!(
        live >= 12,
        "live_contract_count() derived {live} contract(s) from contracts/*.yaml \
         — the walk is broken and this gate would be scoring against nothing"
    );
    let pages = provable_artifact_pages();
    let report = size_report(&pages, live);

    assert!(
        report.offences.is_empty(),
        "{} normative field(s) under `contracts/` state the substrate's SIZE \
         as a number it no longer holds — {}. A normative \
         `invariants:`/`postconditions:` entry has no record exemption \
         (PMAT-1454): drop the census and assert what the equation \
         establishes, or write the LIVE fraction and let this gate maintain \
         it. A universal claim stays true while the census inside it goes \
         stale, which is exactly why nobody re-reads it.",
        report.offences.len(),
        report.offences.join("; ")
    );
    assert!(
        report.uncited.is_empty(),
        "provenance under `contracts/` records a {live}-contract substrate as \
         some other number without citing the slice it records — {}. A record \
         of a smaller substrate is honest and stays; it just has to say WHEN.",
        report.uncited.join("; ")
    );
    // The claims that PASS, one per spelling — proof this is a measurement of
    // the substrate and not a ban on writing numbers next to `contracts`. If
    // a repair ever deletes both, the gate keeps passing while scoring
    // nothing, and that is the failure mode this asserts against.
    assert!(
        report.agreements.len() >= 2,
        "fewer than two live census claims agree with the substrate's actual \
         size of {live}; without an agreeing claim this gate cannot \
         distinguish `measures the corpus` from `matches nothing`. \
         Agreements: {:?}",
        report.agreements
    );
}

#[test]
fn the_census_gate_tracks_the_substrate_not_the_text() {
    // THE BEHAVIOUR HALF. Perturb the DERIVED population, not the prose. The
    // two live agreements say `35`; at a 36-contract substrate BOTH must
    // become findings, and the cited records that say `12` must STAY exempt
    // — a gate that flipped those too would be rewriting history rather than
    // catching drift.
    let pages = provable_artifact_pages();
    let live = live_contract_count();
    let clean = size_report(&pages, live);
    assert!(
        clean.offences.is_empty() && clean.uncited.is_empty(),
        "control: the corpus must be clean at the LIVE census before a \
         perturbation means anything. offences: {:?} uncited: {:?}",
        clean.offences,
        clean.uncited
    );

    let grown = size_report(&pages, live + 1);
    assert!(
        grown.agreements.is_empty(),
        "a thirty-sixth contract left {} census claim(s) reading as agreed — \
         the gate is comparing the text against itself: {:?}",
        grown.agreements.len(),
        grown.agreements
    );
    assert!(
        grown
            .uncited
            .iter()
            .chain(grown.offences.iter())
            .any(|o| o.contains("README.md")),
        "growing the substrate by one left `contracts/README.md`'s `All 35 \
         contracts` and `24 of 35 contracts` unreported, so the needle is not \
         reading the live count at all. offences: {:?} uncited: {:?}",
        grown.offences,
        grown.uncited
    );
}

#[test]
fn the_census_needle_reads_the_spellings_the_corpus_writes() {
    for (text, want, why) in [
        (
            "with PMAT-197 landed, ALL 12 contracts in the substrate have at least one \
             Gold-tier refinement theorem",
            vec![12usize],
            "the totality spelling, verbatim from a normative `invariants:` entry",
        ),
        (
            "Gold-tier coverage is now universal across the substrate (12/12 contracts)",
            vec![12],
            "the FRACTION spelling — the denominator is the census; the equal \
             numerator is not double-counted",
        ),
        (
            "every one of the 12 contracts in the substrate now has at least one Diamond theorem",
            vec![12],
            "`every one of the N` is the same totality",
        ),
        (
            "Platinum coverage now spans 9 of 12 contracts across all 5 layers",
            vec![12],
            "the SPELLED-OUT fraction: `9 of 12` names the same census as `9/12`, \
             and this is live in XpileFrontendTrait.lean",
        ),
        (
            "runs cargo kani over all 101 harnesses (24 files, covering 24 of 35 contracts)",
            vec![35],
            "the live agreeing claim; `all 101 harnesses` is a different \
             population and must not be read as a contract census",
        ),
        (
            "All 35 contracts sit at Bronze.",
            vec![35],
            "the other live agreeing claim, and the reason a trailing `.` cannot \
             be part of the noun",
        ),
        (
            "Diamond depth-3 broadened: 11 contracts at depth-3+",
            vec![],
            "a BARE `N contracts` is a SUBSET count, not a census. Forty-odd of \
             these are live and every one is honest; reading them as censuses \
             would report true sentences as drift",
        ),
        (
            "12 contracts × 2 strata (Sem + Sym) = 24 paired discharges",
            vec![],
            "same — an arithmetic operand, live in contracts/kani/ffi_cpython_ext.rs",
        ),
        (
            "With both landed, of the then-12-contract substrate:",
            vec![],
            "the PAST-SCOPED spelling PMAT-1456 repaired that same file INTO. A \
             needle that flagged its own house style would push the next repair \
             back toward the unscoped form",
        ),
        (
            "5 layers of the contract taxonomy fully covered",
            vec![],
            "`layers` is a different population and is out of subject — see the \
             block comment. `contract` here is an adjective and carries no number",
        ),
        (
            "depth-2 Diamonds on all 5 layers (1, 2, 3, 4, 5)",
            vec![],
            "the 71 live `ALL 5 LAYERS` claims are TRUE and must stay unreported",
        ),
        (
            "language's Layer-1/2 contracts via meta-HIR",
            vec![],
            "THE LIVE FALSE POSITIVE, verbatim from xpile-backend-trait-v1.yaml:241. \
             `Layer-1/2` is a layer PAIR; read as a fraction it reports a \
             two-contract substrate in a normative field. This case reds when \
             `trailing_number_is_free` is weakened",
        ),
        (
            "extends it across all 12 contract-lane theorems",
            vec![],
            "a HYPHENATED compound is a different population: `contract-lane` \
             theorems are not contracts, and scoring twelve of them as a \
             substrate census would mis-scope the claim. This is what the \
             noun's trailing word boundary buys — measured, because the \
             `--contracts-dir` case that looked like the obvious justification \
             reds NOTHING (its backward walk finds no number at all)",
        ),
        (
            "the tool will install 12 contracts",
            vec![],
            "`install` ends in the letters `all`, and reading it as the \
             quantifier would turn a bare subset count into a census. This case \
             reds when `ends_with_word` is relaxed to `ends_with`",
        ),
        (
            "**SUBSTRATE MILESTONE: DEPTH-5 UNIVERSAL ACROSS ALL 12 CONTRACTS.**",
            vec![12],
            "the corpus's COMMONEST spelling, bolded, with a trailing `.` — the \
             Lean substrate writes it this way about a hundred times",
        ),
        (
            "completes coverage across all **12** contracts",
            vec![12],
            "CONSTRUCTED — emphasis BETWEEN the words of the phrase rather than \
             around it. ⚠️ Measured, not assumed: removing the emphasis skip \
             from the backward walk leaves the LIVE verdict completely \
             unchanged, because the corpus bolds the whole phrase every time. \
             It is kept as a forward tripwire and pinned HERE rather than \
             claimed to be load-bearing today",
        ),
    ] {
        let got: Vec<usize> = substrate_size_claims(text)
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(got, want, "{why} — on {text:?}");
    }
}

#[test]
fn an_uncited_census_record_is_reported_and_a_cited_one_is_not() {
    // BOTH DIRECTIONS on the exemption, on synthetic pages so the assertion
    // does not depend on the corpus staying arranged as it is today. The
    // `.lean` extension makes `substrate_blocks` treat the body as prose,
    // which is what every real provenance record here is.
    let stale = "Coverage now spans all 12 contracts.";
    let uncited = vec![("contracts/lean/Fake.lean".to_string(), stale.to_string())];
    let report = size_report(&uncited, 35);
    assert_eq!(
        report.uncited.len(),
        1,
        "an uncited provenance record naming a stale census must be REPORTED, \
         or the branch is decoration. Got: {report:?}",
        report = report.uncited
    );
    assert!(
        report.offences.is_empty(),
        "prose under contracts/ is provenance, not a normative field — it must \
         not be reported as an assertion: {:?}",
        report.offences
    );

    let cited = vec![(
        "contracts/lean/Fake.lean".to_string(),
        format!("## PMAT-336 — {stale}"),
    )];
    let report = size_report(&cited, 35);
    assert!(
        report.uncited.is_empty(),
        "a record that says WHEN is honest and must stay exempt — this gate \
         does not rewrite history: {:?}",
        report.uncited
    );

    // …and the same sentence in a NORMATIVE slot has no way out, citation or
    // not. This is the asymmetry PMAT-1454 established, re-pinned for the
    // census class: a record does not belong in an assertion slot.
    let normative = vec![(
        "contracts/fake-v1.yaml".to_string(),
        format!("    invariants:\n      - \"PMAT-336: {stale}\""),
    )];
    let report = size_report(&normative, 35);
    assert_eq!(
        report.offences.len(),
        1,
        "a normative field naming a stale census must be reported even though \
         it cites a slice — otherwise a `PMAT-` prefix laundered every one of \
         the three live offences this slice repaired. Got: {:?}",
        report.offences
    );
}
