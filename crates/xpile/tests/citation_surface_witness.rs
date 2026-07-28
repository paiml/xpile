//! XPILE-CITESURFACE-001 (PMAT-1447): who carries a contract citation, in
//! what spelling, and what enforces it.
//!
//! Thirteen surfaces published, as an unconditional property of emission,
//! that **every emitted function carries a `// xpile-contract: <ID>`
//! citation** — nine in the book and the specs, and FOUR more in the product
//! itself: `xpile transpile --help`'s `--contracts` description ("annotates
//! every emitted construct"), `xpile audit --help` ("the % of emitted
//! functions that carry a citation" — the sentence `cli.md` published
//! verbatim), and two `xpile-backend` doc comments including
//! `BackendConfig::contracts`.
//!
//! **THE LAST FOUR WERE FOUND BY THIS GATE, AFTER THE AUDIT HAD FINISHED.**
//! That is the fourth consecutive slice where scoping the gate to the CLAIM
//! rather than to the SITE turned up members the sweep missed ([[PMAT-1438]],
//! [[PMAT-1440]], [[PMAT-1443]]) — and this time the misses were the
//! user-facing ones, because the audit had been reading `book/src`.
//!
//! It is not true and it was never meant to be: a citation line is emitted per
//! ID returned by [`xpile_meta_hir::Function::applicable_contracts`], which is
//! EMPTY for comparison-only, logical-only, constant-only and call-only
//! bodies. `xpile audit`'s F1 denominator was deliberately narrowed to the
//! non-empty subset at XPILE-FALSIFY-002 / PMAT-023 *because* the universal
//! reading "double-penalised comparison-only … functions that correctly emit
//! no citation by design" — so the repo's own roadmap records the design
//! decision the book contradicted. Measured on this tree at 7c1191f9:
//! `def ident(a: int) -> int: return a` emits ZERO citations from
//! `--target rust`, `--target ruchy` and `--target lean`, at exit 0, and
//! `xpile audit` over a 3-function corpus with one arithmetic function prints
//! `functions emitted : 3 / require citation : 1 / coverage (F1) : 100.0%`.
//!
//! Three further claims in the same class, corrected in the same slice:
//!
//!   * `book/src/reference/contracts.md` — the page every other page links to
//!     for what a contract says — attributed the universal to
//!     `C-XPILE-BACKEND-TRAIT`'s `compile_contract_citation`. That equation's
//!     own `domain` says the opposite ("Pure language-level constructs
//!     (function definitions, structs, arithmetic) do NOT require a
//!     citation") and its invariant says the chain is `Artifact.citations`,
//!     "NOT regex over `Artifact.primary` text" — i.e. it is not about the
//!     comment line at all. PMAT-1437/1438 corrected the *error-path* half of
//!     that same sentence and left the citation half standing.
//!   * `book/src/contributing/adding-a-backend.md` named
//!     `crates/xpile/tests/qa_gate.rs` as "the CI gate [that] parses every
//!     emitted artifact and fails if a citation is missing". That file
//!     contains the string `citation` zero times; it binds contracts'
//!     `qa_gate: required_tests:` names to real `#[test]` fns. The real gate
//!     is `contract_citation_integrity.rs`. PMAT-1391's shape: a doc claiming
//!     a check runs, where the named check is not that check.
//!   * `crates/xpile-rust-codegen` and `crates/xpile-meta-hir` doc comments
//!     still said the Lean lane cites with `@[xpile_contract "<ID>"]`.
//!     PMAT-1405 retired that form and recorded, as its own lesson, that "this
//!     defect was written down in THREE places and gated in ZERO". It was
//!     written down in five.
//!
//! **A CONCURRENT SLICE LANDED PMAT-1445 ON THAT SAME SENTENCE WHILE THIS WAS
//! IN FLIGHT, AND ITS FIX IS AN INSTANCE OF THIS CLASS.** PMAT-1445 (#2043)
//! corrected two of the sentence's three falsehoods — the type-directed ID and
//! the Lean docstring form — with a derived matrix gate
//! (`citation_id_matrix_witness.rs`) that is better than prose on both
//! questions and is kept. Its replacement text restated the third: *"Each
//! emitted function carries a `xpile-contract:` citation naming the contract
//! that governs its own types."* The detector below reds on that wording. A
//! fix scoped to the SITE rewrote the sentence and carried the CLASS forward
//! inside the correction — which is the argument for this file existing,
//! delivered by accident.
//!
//! # What this file pins
//!
//! BEHAVIOUR (derived from the live registry through the CLI, so the subject
//! is the production path a user takes, not a unit-test flag combination):
//! that an uncited-by-design function exists and stays uncited; that the ID
//! tracks the construct rather than being the constant `C-PY-INT-ARITH`; that
//! the Lean code lane's spelling is the docstring; and that F1's denominator
//! is strictly smaller than the emitted-function count on a corpus where both
//! are defined — which is exactly the difference between the two readings of
//! `cli.md`'s sentence (100.0% vs 33.3%).
//!
//! PROSE: no published paragraph may assert the universal. Non-vacuity is by
//! construction (PMAT-1438) — the nine book/spec paragraphs are embedded here
//! VERBATIM as they stood at 7c1191f9 and the detector must flag every one of
//! them, so softening the detector reds even after the corpus is clean. A
//! separate assertion pins that none of the nine carried the disclosure token,
//! so the exemption is not what the rule turns on.
//!
//! The corpus that detector runs over is `book/src` + `docs/specifications` +
//! `README.md` + every `crates/*/src` — which is why it reached the four
//! product-side sites. Three paragraphs it must NOT flag, each verified by
//! hand and each the reason for one refinement above: `bashrs-backend`'s
//! "Every emit carries … `# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE`" is
//! TRUE (that lane cites unconditionally — measured on both entry paths);
//! `xpile-wasm-codegen`'s "every emitted function additionally CITED" is a
//! past-tense narrative of a refusal it shipped; and `audit-design.md`'s
//! capability-vs-contract case study quotes the slogan in order to record it
//! as falsified. A gate that reports those is reporting the repo's honesty as
//! its dishonesty.
//!
//! # What this file does NOT pin
//!
//! Whether any *particular* function is cited — that is
//! `contract_citation_integrity.rs`'s job (every applicable contract is
//! actually cited; every cited ID resolves to an on-disk contract). This file
//! pins the shape of the claim, not the coverage.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use depyler_frontend::PythonFrontend;
use xpile_frontend::Frontend;
use xpile_meta_hir::Item;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

/// A per-CALL unique directory. Two probes sharing one directory have produced
/// cross-test clobbering in this repo before (PMAT-1427), and a witness that
/// identifies its workspace by a glob over a shared namespace measures the
/// neighbourhood rather than the run (PMAT-1436). The counter is not
/// decoration: the first cut of this file gave `applicable_contracts_of` a
/// FIXED tag and two concurrently-running tests raced on it, producing a
/// failure that reproduced only when the whole binary ran.
fn probe_dir(tag: &str) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir()
        .join("xpile-citation-surface-witness")
        .join(format!("{tag}-{n}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir probe dir");
    dir
}

/// Emit `py` for `target` with the CLI's DEFAULT flags — no `--contracts`
/// argument. Citations are ON by default; passing the flag would certify a
/// combination the default invocation never takes, which is precisely the
/// hole PMAT-1405 recorded in the Lean lane's own semantic witness.
fn emit(tag: &str, py: &str, target: &str) -> String {
    let dir = probe_dir(tag);
    let src = dir.join("probe.py");
    std::fs::write(&src, py).expect("write probe");
    let out = Command::new(xpile_bin())
        .args(["transpile", src.to_str().unwrap(), "--target", target])
        .output()
        .expect("spawn xpile");
    assert!(
        out.status.success(),
        "probe `{tag}` must lower for --target {target} (an uncited function is \
         the subject here, so a REFUSAL would satisfy the assertion for the \
         wrong reason): {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Any of the citation spellings this repo emits, in any lane.
fn cites_anything(emitted: &str) -> bool {
    emitted.contains("xpile-contract") || emitted.contains("xpile_contract")
}

// ---------------------------------------------------------------------------
// Probes. `ident` and `pick` are the two shapes the roadmap names by name in
// XPILE-FALSIFY-002 as correctly emitting no citation; `add` and `scale` are
// the anti-vacuity controls that must cite in the SAME lane, so a probe that
// simply failed to lower, or a lane that stopped citing altogether, cannot
// satisfy the uncited assertions for the wrong reason.
// ---------------------------------------------------------------------------

const IDENT_PY: &str = "def ident(a: int) -> int:\n    return a\n";
const PICK_PY: &str =
    "def pick(a: int, b: int) -> int:\n    if a > b:\n        return a\n    return b\n";
const ADD_PY: &str = "def add(a: int, b: int) -> int:\n    return a + b\n";
const SCALE_PY: &str = "def scale(a: float, b: float) -> float:\n    return a * b\n";

fn applicable_contracts_of(py: &str) -> Vec<&'static str> {
    let dir = probe_dir("hir");
    let src = dir.join("probe.py");
    std::fs::write(&src, py).expect("write probe");
    let module = PythonFrontend
        .parse_and_lower(&src, py)
        .expect("probe lowers to meta-HIR");
    let mut out = Vec::new();
    for item in &module.items {
        if let Item::Function(f) = item {
            out.extend(f.applicable_contracts());
        }
    }
    out
}

/// The uncited-by-design property, in the three lanes the book named. WHY THE
/// CONTROL IS LOAD-BEARING: without `add` citing in the same lane, this test is
/// equally satisfied by a backend that has stopped emitting citations at all —
/// the failure mode a bare absence check cannot distinguish (PMAT-1442's
/// hold-the-path-vary-the-content control, one dimension over).
#[test]
fn a_body_with_no_governing_construct_emits_no_citation_in_rust_ruchy_or_lean() {
    assert!(
        applicable_contracts_of(IDENT_PY).is_empty(),
        "the DERIVATION must be empty for a call-free, operator-free body — if \
         this ever becomes non-empty the emitted-citation assertions below stop \
         measuring what they claim"
    );
    assert!(
        applicable_contracts_of(PICK_PY).is_empty(),
        "comparison-only bodies have no governing contract (XPILE-FALSIFY-002)"
    );
    assert!(
        !applicable_contracts_of(ADD_PY).is_empty(),
        "the control must have a governing contract, or every row below agrees \
         for the wrong reason"
    );

    for target in ["rust", "ruchy", "lean"] {
        let uncited = emit(&format!("ident-{target}"), IDENT_PY, target);
        assert!(
            !cites_anything(&uncited),
            "--target {target} emitted a citation for a body with no applicable \
             contract. Either the emitter over-cites (the `over_citations` bug \
             `xpile audit` counts), or `applicable_contracts()` widened — in \
             which case the book pages corrected by PMAT-1447 need to move \
             back. Emitted:\n{uncited}"
        );
        let cited = emit(&format!("add-{target}"), ADD_PY, target);
        assert!(
            cites_anything(&cited),
            "CONTROL FAILED: --target {target} emitted no citation for an \
             arithmetic body either, so the assertion above proves nothing \
             about applicability. Emitted:\n{cited}"
        );
    }

    // `pick` refuses on the Lean lane for an unrelated reason (Lean has no
    // statement-form if/else), so it is measured only where it lowers. Stated
    // rather than silently dropped: a skipped row that looks like a pass is
    // the shape this whole file exists to remove.
    for target in ["rust", "ruchy"] {
        let emitted = emit(&format!("pick-{target}"), PICK_PY, target);
        assert!(
            !cites_anything(&emitted),
            "--target {target} cited a comparison-only function:\n{emitted}"
        );
    }
}

/// The published spelling was the constant `C-PY-INT-ARITH`. The ID is
/// construct-directed, so an int probe and a float probe must disagree.
#[test]
fn the_cited_id_tracks_the_construct_not_a_fixed_string() {
    let int_ids = applicable_contracts_of(ADD_PY);
    let float_ids = applicable_contracts_of(SCALE_PY);
    assert!(!int_ids.is_empty() && !float_ids.is_empty());
    assert_ne!(
        int_ids, float_ids,
        "if every contract-bearing body cited the same ID, `frontends.md`'s \
         `C-PY-INT-ARITH` spelling would have been merely imprecise rather \
         than wrong"
    );
    let rust = emit("scale-rust", SCALE_PY, "rust");
    assert!(
        rust.contains(float_ids[0]) && !rust.contains("C-PY-INT-ARITH"),
        "the float lane must cite {} and not the int contract:\n{rust}",
        float_ids[0]
    );
}

/// PMAT-1405 moved the Lean CODE lane to a docstring. `lean_default_emit_witness.rs`
/// pins the EMIT; this pins that no `crates/*/src` doc comment still describes
/// the retired form as the code lane's — the half of PMAT-1405's own recorded
/// lesson ("written down in THREE places and gated in ZERO") that stayed open.
#[test]
fn the_lean_code_lane_cites_with_a_docstring_not_a_line_comment_or_attribute() {
    let lean = emit("add-lean-spelling", ADD_PY, "lean");
    assert!(
        lean.contains("/-- xpile-contract:"),
        "the Lean code lane must cite with a docstring:\n{lean}"
    );
    assert!(
        !lean.contains("// xpile-contract:") && !lean.contains("@[xpile_contract"),
        "the Lean code lane must use neither the Rust comment form nor the \
         retired attribute:\n{lean}"
    );

    for (path, text) in crate_src_files() {
        // STRUCTURAL exemption: the PROOF-lane crates — `*-contract-backend`
        // (rendering) and `*-contract-frontend` (parsing) — are the lane that
        // legitimately produces and reads the attribute, so their own docs
        // describing it are correct by construction. Keyed on the crate the
        // file lives in, not on a keyword in the sentence: the first cut
        // guessed at words like "theorem" and reported
        // `xpile-contract-backend`'s format grid (`@[xpile_contract ...]`
        // (Lean)) and then `xpile-contract-frontend`'s list of constructs it
        // PARSES. Both are exactly right; a keyword exemption was never going
        // to converge on that.
        //
        // The crate name is taken by RELATIVISING against `crates/` and reading
        // the first component — not by scanning the whole path. The first cut
        // did the latter and `book_rust_example_witness`'s PMAT-1444 tripwire
        // reddened on it by name: an absolute path carries every ancestor's
        // name, so a checkout under, say, `~/work/xpile-contract-stuff/` would
        // have exempted the entire tree. That tripwire exists because exactly
        // this shape made a gate red where it was authored and green in CI.
        let crate_name = path
            .strip_prefix(workspace_root().join("crates"))
            .ok()
            .and_then(|rel| rel.components().next())
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .unwrap_or_default();
        if crate_name.contains("-contract-") {
            continue;
        }
        for para in paragraphs_of(&path, &text) {
            let flat = normalise(&para);
            if !flat.contains("@[xpile_contract") {
                continue;
            }
            // Honest if it scopes the attribute to the contract-RENDERING
            // lane, which legitimately keeps it — or if it names the code
            // lane's docstring in the same breath, which is what
            // distinguishing the two eras looks like.
            // Honest if it scopes the attribute to the proof lane, names the
            // code lane's docstring in the same breath, or speaks of the
            // attribute as a PAST state. The last one is the same tense
            // distinction the universality detector makes: `xpile-lean-codegen`
            // says "All three WERE STAMPED `@[xpile_contract …]`" while
            // narrating a defect it fixed, and `xpile-wasm-codegen` says "every
            // emitted function additionally CITED". A history is not a claim.
            let is_rendering_lane = flat.contains("contract-rendering")
                || flat.contains("contract-backend")
                || flat.contains("contract backend")
                || flat.contains("leantheorem")
                || flat.contains("theorem")
                || flat.contains("docstring")
                || flat.contains("through v0.1.")
                || flat.contains("were stamped")
                || flat.contains("was stamped")
                || flat.contains("previously")
                || flat.contains("retired")
                || flat.contains("no longer")
                || flat.contains("pmat-1405");
            assert!(
                is_rendering_lane,
                "{}: a doc comment names `@[xpile_contract …]` without scoping \
                 it to the contract-RENDERING lane. The CODE lane retired that \
                 form at PMAT-1405; saying otherwise here is the same claim the \
                 book carried.\n{para}",
                path.display()
            );
        }
    }
}

/// `cli.md` described F1's denominator as "emitted functions". The two
/// readings are different numbers, and this proves it on a corpus rather than
/// restating the sentence: `require citation` must be strictly smaller than
/// `functions emitted`, with both non-zero.
#[test]
fn f1s_denominator_is_strictly_smaller_than_the_emitted_function_count() {
    let dir = probe_dir("audit-corpus");
    std::fs::write(dir.join("add.py"), ADD_PY).unwrap();
    std::fs::write(dir.join("pick.py"), PICK_PY).unwrap();
    std::fs::write(dir.join("ident.py"), IDENT_PY).unwrap();

    let out = Command::new(xpile_bin())
        .args(["audit", dir.to_str().unwrap(), "--json"])
        .output()
        .expect("spawn xpile audit");
    assert!(
        out.status.success(),
        "xpile audit must exit 0 on this corpus"
    );
    let json = String::from_utf8_lossy(&out.stdout).to_string();

    let field = |k: &str| -> f64 {
        let needle = format!("\"{k}\":");
        let start = json.find(&needle).unwrap_or_else(|| {
            panic!("`{k}` missing from audit --json payload: {json}");
        }) + needle.len();
        let rest = &json[start..];
        let end = rest.find([',', '}']).unwrap_or(rest.len());
        rest[..end].trim().parse().unwrap_or_else(|e| {
            panic!("`{k}` is not numeric in {json}: {e}");
        })
    };

    let emitted = field("functions_emitted");
    let requiring = field("functions_requiring_citation");
    let f1 = field("f1_pct");
    assert!(
        requiring > 0.0 && emitted > requiring,
        "this corpus must contain BOTH a contract-bearing function and at least \
         one that correctly needs no citation, or the two denominators are not \
         distinguishable here: emitted={emitted} requiring={requiring}"
    );
    assert_eq!(
        f1,
        100.0,
        "F1 must be 100% here — every function that requires a citation has \
         one. Under `cli.md`'s pre-PMAT-1447 wording (% of EMITTED functions) \
         the same run reads {:.1}%, which is the whole point: {json}",
        100.0 * field("functions_with_citation") / emitted
    );
    assert_eq!(
        field("over_citations"),
        0.0,
        "an over-citation means the emitter cited a function with no applicable \
         contract, which would make the corrected book text wrong in the other \
         direction: {json}"
    );
}

// ---------------------------------------------------------------------------
// The prose half.
// ---------------------------------------------------------------------------

/// The universal, in the spellings the DEFECT used — not the spellings the
/// correction uses (PMAT-1437). Family A needs a present-tense carry verb, so
/// an honest PAST-TENSE narrative of a fixed defect ("every emitted function
/// additionally cited …", `xpile-wasm-codegen`) is not swept in.
const UNIVERSAL_QUANTIFIERS: &[&str] = &[
    "each emitted",
    "every emitted",
    "emitted functions that",
    "all emitted functions",
];

const CARRY_VERBS: &[&str] = &["carries", "carry", "requires", "must carry", "cited"];

/// How far after the quantifier the noun and verb must appear for the sentence
/// to be ASSERTING the universal rather than merely containing both words.
///
/// ADJACENCY IS LOAD-BEARING, and the paragraph that proved it is
/// `audit-design.md`'s capability-vs-contract case study — the most honest
/// prose in the repo, which quotes the slogan *"every emitted construct is
/// under a provable contract"* in order to record it as **falsified in
/// practice**. An unbounded scan flagged it because the word `requires`
/// appears 400 characters later in a clause about `diamond_coverage.rs`.
/// Reporting the file that documents the defect as an instance of the defect
/// is [[PMAT-1430]]'s use-vs-mention, and a window is the cheap structural
/// answer to it.
const ADJACENCY: usize = 90;

/// Family B: spellings that assert the universal with no carry verb at all.
/// Every one of these is a literal taken from a site this slice corrected.
const VERBLESS_UNIVERSALS: &[&str] = &[
    "citation per function",
    "citation above each emitted function",
    "citation on every emitted function",
    "emission must carry a structural contract citation",
];

/// `every emit carries` needs a second condition, and finding out why is worth
/// recording. `crates/bashrs-backend` says "Every emit carries a `#!/bin/sh`
/// shebang and a `# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE` citation
/// line" — and that is TRUE, measured on both entry paths (`--target shell`
/// from Python, and a `.sh` round-trip): the shell lane cites its lane
/// contract unconditionally, as do wasm, wgsl and spirv. Correcting it would
/// have traded a false universal for a false particular. The DEFECT's spelling
/// carried the generic PLACEHOLDER — "Every emit carries `// xpile-contract:
/// <ID>`", a claim about all backends — so the placeholder is the condition.
const PLACEHOLDER_UNIVERSAL: (&str, &str) = ("every emit carries", "<id>");

const CITATION_TOKENS: &[&str] = &["xpile-contract", "xpile_contract", "citation"];

/// A paragraph is honest if it discloses the correction in this repo's
/// established idiom, or if it names the actual condition. Deliberately two
/// tokens, both principled: naming `applicable_contracts` IS the disclosure.
const DISCLOSURE_TOKENS: &[&str] = &["through v0.1.", "applicable_contracts"];

/// Lower-cased, comment markers removed, whitespace collapsed.
///
/// The `//` strip is not cosmetic (PMAT-1438): `adding-a-backend.md` makes the
/// claim inside a wrapped `//` comment, and a needle written from the prose
/// sites silently misses the one site that is INSTRUCTIONS TO CONTRIBUTORS.
///
/// Blockquote markers are stripped PER LINE, not globally. A global
/// `replace('>', " ")` also eats the `>` of the `<ID>` PLACEHOLDER — which is
/// the exact token that distinguishes the generic all-backends claim from
/// `bashrs-backend`'s true lane-specific one, so the detector went blind to
/// the table-row defect it was written for and only the embedded-verbatim
/// assertion caught it.
fn normalise(paragraph: &str) -> String {
    let stripped: Vec<String> = paragraph
        .lines()
        .map(|l| l.trim_start().trim_start_matches('>').to_string())
        .collect();
    stripped
        .join("\n")
        .replace("///", " ")
        .replace("//!", " ")
        .replace("//", " ")
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Paragraph = the unit a claim is made in.
///
/// THIS SPLITTER IS LOAD-BEARING AND ITS FIRST CUT WAS WRONG. "A run of
/// consecutive non-blank lines" merges every bullet of a markdown list into
/// one blob, and in Rust it merges a whole `#[derive(Subcommand)]` body — 200
/// lines of doc comments, attributes and fields — into a single "paragraph".
/// A merged blob is a claim about the NEIGHBOURHOOD: the run reported
/// `audit-design.md`'s F6 dossier bullet and `bashrs-backend`'s `render_arg`
/// line as offenders, because some OTHER bullet 40 lines away carried the
/// needle. PMAT-1436's shape in a text scanner.
///
/// So: a `.rs` paragraph is a run of consecutive COMMENT lines, broken by any
/// line of code; a `.md` paragraph additionally breaks at a heading, a list
/// item and a table row, each of which is its own claim.
fn paragraphs_of(path: &Path, text: &str) -> Vec<String> {
    let is_rs = path.extension().and_then(|s| s.to_str()) == Some("rs");
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let flush = |cur: &mut Vec<&str>, out: &mut Vec<String>| {
        if !cur.is_empty() {
            out.push(cur.join("\n"));
            cur.clear();
        }
    };
    for line in text.lines() {
        let t = line.trim();
        let comment = t.starts_with("///") || t.starts_with("//!") || t.starts_with("//");
        let blank = t.is_empty() || t == "///" || t == "//!" || t == "//";
        let breaks_here = if is_rs {
            blank || !comment
        } else {
            blank
                || t.starts_with('#')
                || t.starts_with("- ")
                || t.starts_with("* ")
                || t.starts_with("| ")
                || t.starts_with("```")
                || t.chars().next().is_some_and(|c| c.is_ascii_digit()) && t.contains(". ")
        };
        if breaks_here {
            flush(&mut cur, &mut out);
            // A heading / bullet / row is itself the start of a paragraph.
            if !blank && !is_rs {
                cur.push(line);
            }
        } else {
            cur.push(line);
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// Does this paragraph assert the universal?
fn asserts_the_universal(paragraph: &str) -> bool {
    let flat = normalise(paragraph);
    if !CITATION_TOKENS.iter().any(|t| flat.contains(t)) {
        return false;
    }
    if VERBLESS_UNIVERSALS.iter().any(|n| flat.contains(n)) {
        return true;
    }
    if flat.contains(PLACEHOLDER_UNIVERSAL.0) && flat.contains(PLACEHOLDER_UNIVERSAL.1) {
        return true;
    }
    // The quantified noun is not always `function`. `xpile transpile --help`
    // and `xpile_backend::strip_contract_citations` both said "every emitted
    // CONSTRUCT is cited", and while the paragraph splitter was over-merging,
    // the word `function` happened to be in range from a neighbouring
    // declaration — so the first cut of this predicate reached them by
    // accident and lost them the moment the splitter got more precise.
    const NOUNS: &[&str] = &["function", "construct", "emission", "artifact"];
    for q in UNIVERSAL_QUANTIFIERS {
        let mut from = 0;
        while let Some(at) = flat[from..].find(q) {
            let start = from + at;
            let mut end = (start + ADJACENCY).min(flat.len());
            while end > start && !flat.is_char_boundary(end) {
                end -= 1;
            }
            let window = &flat[start..end];
            if NOUNS.iter().any(|n| window.contains(n))
                && CARRY_VERBS.iter().any(|v| window.contains(v))
            {
                return true;
            }
            from = start + q.len();
        }
    }
    false
}

fn discloses(paragraph: &str) -> bool {
    let flat = normalise(paragraph);
    DISCLOSURE_TOKENS.iter().any(|t| flat.contains(t))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk(&p, ext, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some(ext) {
            out.push(p);
        }
    }
}

/// Every `crates/<member>/src` tree, built by JOINING `src` onto each crate
/// directory rather than by testing path components. PMAT-1444: a
/// `components().any(|c| c == "src")` filter over an absolute path is a no-op
/// in a checkout that lives under a `src/` directory, so the location of the
/// checkout decided the verdict. Constructing the roots cannot drift that way,
/// and it keeps this file (which lives in `crates/xpile/tests/` and quotes
/// every defect verbatim) out of its own corpus by construction.
fn crate_src_files() -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    let crates = workspace_root().join("crates");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&crates)
        .expect("crates/ readable")
        .flatten()
        .map(|e| e.path().join("src"))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in &dirs {
        let mut rs = Vec::new();
        walk(dir, "rs", &mut rs);
        rs.sort();
        for p in rs {
            // STRUCTURAL statement of the subject, not a second cardinality
            // (PMAT-1444): every path read lies under some `crates/<x>/src`.
            // A count would drift; this cannot.
            assert!(
                p.starts_with(dir),
                "{} escaped the crate-src root {}",
                p.display(),
                dir.display()
            );
            let text = read(&p);
            files.push((p, text));
        }
    }
    assert!(
        !dirs.is_empty() && files.len() > 30,
        "the crates/*/src sweep found {} files across {} crate roots — a broken \
         root derivation reads as a clean corpus. (The live figure is 43; the \
         floor is deliberately slack, because the STRUCTURAL assertion above is \
         what states the subject.)",
        files.len(),
        dirs.len()
    );
    files
}

/// Book pages, specs and the README: everything that TELLS a reader or a
/// contributor what the citation rule is. CHANGELOG.md is excluded on purpose
/// — it is the historical record and quotes every defect it fixed.
fn published_prose() -> Vec<(PathBuf, String)> {
    let root = workspace_root();
    let mut paths = Vec::new();
    walk(&root.join("book/src"), "md", &mut paths);
    walk(&root.join("docs/specifications"), "md", &mut paths);
    paths.push(root.join("README.md"));
    paths.sort();
    let out: Vec<(PathBuf, String)> = paths.into_iter().map(|p| (p.clone(), read(&p))).collect();
    assert!(
        out.len() > 20,
        "published-prose sweep found only {} files",
        out.len()
    );
    out
}

#[test]
fn no_published_paragraph_claims_a_citation_on_every_emitted_function() {
    let mut offenders = Vec::new();
    let corpus = published_prose()
        .into_iter()
        .chain(crate_src_files())
        .collect::<Vec<_>>();
    for (path, text) in &corpus {
        for para in paragraphs_of(path, text) {
            if asserts_the_universal(&para) && !discloses(&para) {
                offenders.push(format!("{}\n    {}", path.display(), para.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a citation is emitted per ID in `applicable_contracts()`, which is \
         EMPTY for comparison-only / logical-only / constant-only / call-only \
         bodies — so no page may state it as a property of every emitted \
         function. Say what the condition is, or disclose the correction with \
         `Through v0.1.…`. Offending paragraphs ({}):\n\n{}",
        offenders.len(),
        offenders.join("\n\n")
    );
}

/// The nine offending paragraphs, VERBATIM as they stood at 7c1191f9.
/// Non-vacuity is established against these rather than against a corpus
/// count, so softening a detector reds here even after the corpus is clean
/// (PMAT-1438).
const PRE_FIX_PARAGRAPHS: &[(&str, &str)] = &[
    (
        "book/src/reference/frontends.md",
        "Each emitted Rust/Ruchy/Lean function carries a\n`// xpile-contract: C-PY-INT-ARITH` citation for the arithmetic\ncontract.",
    ),
    (
        "book/src/reference/backends.md",
        "- A `// xpile-contract: <ID>` citation above each emitted function",
    ),
    (
        "book/src/contributing/adding-a-backend.md (code comment)",
        "        // 2. Emit a `// xpile-contract: <ID>` citation per function.",
    ),
    (
        "book/src/contributing/adding-a-backend.md (table row)",
        "| Citation | Every emit carries `// xpile-contract: <ID>` | `tests/qa_gate.rs` (already enforced) |",
    ),
    (
        "book/src/tutorials/python-to-rust.md",
        "   [`C-XPILE-BACKEND-TRAIT`](../reference/contracts.md#c-xpile-backend-trait)\n   contract requires every emitted function to carry such a citation.",
    ),
    (
        "book/src/reference/cli.md",
        "Reports falsifier F1 (Layer-1 contract citation coverage) for a\ncorpus. Walks the given path, transpiles every recognised source\nfile, and reports the % of emitted functions that carry a\n`// xpile-contract: <ID>` citation.",
    ),
    (
        "book/src/reference/contracts.md",
        "Every `Backend` emission must carry a structural contract citation\n(`// xpile-contract: <ID>`) — that is `compile_contract_citation`, and\nlike all twenty of this contract's equations it quantifies over the\n**emitted** artifact. The contract says nothing about error paths.",
    ),
    (
        "docs/specifications/xpile-spec.md",
        "Both transpile cleanly via `xpile transpile` → Rust + Ruchy + Lean for the Python case; → Rust for the C case. Each emitted function carries a `// xpile-contract: <ID>` citation referencing the appropriate v0.2.0 contract.",
    ),
    (
        "docs/specifications/sub/v0.2.0-decy-merger.md",
        "2. Citation on every emitted function: `// xpile-contract:\n   C-C-INT-ARITH` (not C-PY-INT-ARITH).",
    ),
];

#[test]
fn the_detector_flags_every_verbatim_pre_fix_paragraph() {
    let missed: Vec<&str> = PRE_FIX_PARAGRAPHS
        .iter()
        .filter(|(_, para)| !asserts_the_universal(para))
        .map(|(site, _)| *site)
        .collect();
    assert!(
        missed.is_empty(),
        "the detector no longer sees {} of the paragraphs it was written for: \
         {missed:?}. A needle set written from the CORRECTED text reaches the \
         prose sites and misses the code-comment and table spellings — that is \
         PMAT-1438's recorded miss, and this assertion is what stops it \
         recurring.",
        missed.len()
    );
}

/// If the originals had carried the disclosure token, the exemption — not the
/// detector — would be what the rule turns on, and nobody would know.
#[test]
fn none_of_the_pre_fix_paragraphs_carried_the_disclosure_token() {
    let exempted: Vec<&str> = PRE_FIX_PARAGRAPHS
        .iter()
        .filter(|(_, para)| discloses(para))
        .map(|(site, _)| *site)
        .collect();
    assert!(
        exempted.is_empty(),
        "these defects would have been waved through by the disclosure \
         exemption rather than detected: {exempted:?}"
    );
}

/// PMAT-1391's shape: a doc that says a gate enforces something must name a
/// file that is that gate. `adding-a-backend.md` named `qa_gate.rs`, which
/// mentions citations zero times.
#[test]
fn a_paragraph_naming_the_citation_gate_names_a_file_that_contains_citation_logic() {
    let root = workspace_root();
    let mut checked: BTreeSet<String> = BTreeSet::new();
    let mut offenders = Vec::new();

    // SUBJECT: paragraphs about the EMITTED-citation requirement, not about
    // citation in any of its other senses. The bare word `citation` names a
    // Lean-theorem citation gate, a Kani citation gate and a prose citation
    // graph elsewhere in these files, and keying on it made this test's first
    // run report five spec tables that assert nothing of the kind. The needles
    // below are the two spellings the DEFECT used (PMAT-1437): the table row
    // said "Every emit carries `// xpile-contract: <ID>`" and the paragraph
    // said "The citation requirement is non-negotiable … fails if a citation
    // is missing".
    // Narrowed AGAIN on the first run: `xpile-contract` alone still swept in
    // `audit-design.md`'s Citation-Bridge bullet, which names `kani_harnesses.rs`
    // for a KANI gate in a clause 500 characters from where it discusses the
    // Lean docstring. These four are the spellings the two defect sites used —
    // "The citation requirement is non-negotiable … fails if a citation is
    // missing" and "`tests/qa_gate.rs` (already enforced)". Deliberately
    // narrow, and narrow in a way the file states: a differently-worded future
    // site escapes, which is the price of not reporting five true paragraphs.
    const EMITTED_CITATION_NEEDLES: &[&str] = &[
        "citation requirement",
        "citation is missing",
        "already enforced",
    ];

    for (path, text) in published_prose() {
        for para in paragraphs_of(&path, &text) {
            let flat = normalise(&para);
            // NOT `discloses()`. That exemption exists for the universality
            // rule, and one of its two tokens is `applicable_contracts` —
            // which the CORRECTED paragraph naming the real gate contains, so
            // reusing it here would skip the very paragraph the anchor below
            // needs to see. The only exemption a gate-NAME claim earns is the
            // repo's correction idiom, which is how the page records that
            // `qa_gate.rs` never was this gate.
            let quoting_the_correction = flat.contains("through v0.1.");
            if !EMITTED_CITATION_NEEDLES.iter().any(|n| flat.contains(n)) || quoting_the_correction
            {
                continue;
            }
            for token in flat.split_whitespace() {
                let cleaned = token.trim_matches(|c: char| !c.is_ascii_graphic() || c == '`');
                let Some(stem) = cleaned.strip_suffix(".rs") else {
                    continue;
                };
                let Some(file) = stem.rsplit('/').next() else {
                    continue;
                };
                let candidate = root.join("crates/xpile/tests").join(format!("{file}.rs"));
                if !candidate.exists() {
                    continue;
                }
                checked.insert(file.to_string());
                let body = read(&candidate);
                if !body.contains("xpile-contract") && !body.contains("xpile_contract") {
                    offenders.push(format!(
                        "{}: names `{file}.rs` as the gate for a citation claim, but that \
                         file never mentions a citation.\n    {}",
                        path.display(),
                        para.trim()
                    ));
                }
            }
        }
    }

    assert!(offenders.is_empty(), "{}", offenders.join("\n\n"));
    assert!(
        checked.contains("contract_citation_integrity"),
        "ANCHOR: the book must still send a contributor to the real citation \
         gate. Checked: {checked:?}"
    );

    // The gate the book now names must contain the two assertions it names.
    let gate = read(&root.join("crates/xpile/tests/contract_citation_integrity.rs"));
    for f in [
        "every_emitted_citation_resolves_to_an_on_disk_contract",
        "every_applicable_contract_is_actually_cited",
    ] {
        assert!(
            gate.contains(&format!("fn {f}(")),
            "adding-a-backend.md names `{f}` — a name check is not an API check \
             (PMAT-1439), so this asserts the fn exists in the file the page \
             sends the reader to"
        );
    }

    // And the file the book USED to name must still be citation-free, so the
    // correction cannot silently stop being a correction.
    let qa = read(&root.join("crates/xpile/tests/qa_gate.rs"));
    assert!(
        !qa.is_empty() && !qa.contains("xpile-contract"),
        "qa_gate.rs has grown citation logic — if it is now a citation gate, \
         `adding-a-backend.md`'s disclosure that it never was needs to move"
    );
}
