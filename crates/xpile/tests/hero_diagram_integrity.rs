//! XPILE-HERODIAGRAM-001 (PMAT-1464) — the picture at the top of the README is
//! checked against the registry, the same way the book's ASCII roster is.
//!
//! ## THE DEFECT
//!
//! `docs/assets/hero.svg` is the first thing in `README.md` and therefore the
//! first thing on the GitHub landing page. It landed in `1a4c798a` on
//! 2026-05-15 and, at `647ba346` — **75 days and one commit later** — still drew
//! this, measured against `xpile_core::default_session()`:
//!
//! | drawn | truth |
//! |---|---|
//! | frontends `C++`, `Rust`, `Lean 4` | **no frontend exists for any of them**, then or now |
//! | frontend `Ruchy` | registered for ROUTING ONLY — `lowers_input()` is `false` and `.ruchy` INPUT refuses (PMAT-1346) |
//! | frontends `Shell`, `WebAssembly` | registered and lowering, **absent from the diagram** |
//! | hub subtitle `6 source langs` | **4** frontends lower; 5 are registered. Not 6 either way |
//! | backends `WebAssembly`, `Shell`, `forjar` | registered, **absent** (3 of 9 missing) |
//! | proof lane `mdBook`, drawn as BOTH a source and an output | **no mdBook contract frontend or backend exists** — `README.md` says so outright |
//! | proof lane outputs, drawn with solid bidirectional arrows | both contract backends return a fixed `_scaffold` payload; `xpile info` prints `2 registered, 0 rendering` |
//!
//! Of the six languages drawn flowing INTO meta-HIR, **two were real**.
//!
//! **NEVER-TRUE, NOT AGED — and that is a different defect.** Every prior slice
//! in this arc found a number that had drifted. This one was false on the day
//! it was written: at `1a4c798a` itself `default_session()` registered three
//! frontends (`python`, `c`, `ruchy`) and five backends (no `spirv`), so `C++`,
//! `Rust`, `Lean 4` and `SPIR-V` were all drawn before anything registered
//! them, and `mdBook` never has been. The file was a ROADMAP published as an
//! ARCHITECTURE DIAGRAM, and nothing since has re-read it.
//!
//! ## WHY IT WAS GREEN — and this is the sharp part
//!
//! **PMAT-1440 found this exact roster wrong in `book/src/concepts/two-lanes.md`
//! and fixed a file.** `lane_roster_witness.rs` wrote down, in as many words,
//! *"A regression pin written against a FILE does not protect a CLAIM"* — and
//! then keyed its registry comparison to `const PAGE: &str =
//! "book/src/concepts/two-lanes.md"`, and scoped its phantom needle to a corpus
//! of `book/src/**/*.md` plus `README.md`. Its own comment names *"the hero
//! image's alt-text"* as a third site it repaired. It repaired the **alt text**
//! and never opened the **image**: `c++` and `mdBook` are on that gate's
//! phantom list, both were live in `docs/assets/hero.svg`, and the corpus
//! collected `.md` files only — so the phantom sat inside an artifact embedded
//! by a file the gate was already reading. Same family as PMAT-1457 ("the sweep
//! was narrower than the rule it wrote") and PMAT-1456 ("a gate's text model
//! can name an artifact kind its walk never collects"), in the slice that had
//! just written the rule down.
//!
//! **AND THE NUMERAL HAD A GATED TWIN.** `crates/xpile-core/src/lib.rs` says of
//! the routing-only Ruchy registration: *"It does NOT count toward the README's
//! substantive source-language numeral — `claims_drift.rs` derives that by
//! RUNNING each registered frontend against a real program."* That numeral —
//! `README.md`'s "four source languages" — has been gated and correct for
//! months. The identical numeral in the image the same README embeds said
//! **six**, ungated. One claim, two homes, one gate.
//!
//! ## THE RULE
//!
//! A diagram that ENUMERATES lanes is a claim about the registry, so it is
//! compared to the registry **both directions and in registration order**: an
//! id drawn that nothing registers reds, a registered lane not drawn reds, and
//! a reordering reds. Nothing here is hard-coded — the diagram declares each
//! lane's registry id in a `data-frontend` / `data-backend` /
//! `data-contract-frontend` / `data-contract-backend` attribute, and the
//! expected sets come from `default_session()`, the registry the CLI itself
//! dispatches through. Adding a backend reds this image until the picture
//! moves. That is PMAT-1431 §4's both-directions idiom and PMAT-1440's rule,
//! applied to the artifact PMAT-1440 could not see.
//!
//! ## WHAT THIS GATE DOES NOT DO, stated as a measurement rather than assumed
//!
//!   * It does not judge whether `Shell` is a good human name for the `bashrs`
//!     frontend. It checks the ID against the registry and then checks that the
//!     LABEL is spelled identically in the image, in the SVG's own `<desc>` and
//!     in the README's `alt=` — which is exactly where the drift was: the alt
//!     text named 5 sources and 9 backends while the image drew 6 and 6, so a
//!     screen-reader user and a sighted reader were handed two different
//!     architectures for one image.
//!   * It does not check pixel geometry. A lane could be drawn off-canvas and
//!     this would pass; `sh -n` for SVG does not exist. What is checked is that
//!     the roster is complete, correct, ordered, and consistent across the
//!     three places it is written down.
//!   * `docs/assets/` holds exactly one `.svg` today ([`the_gate_reports_what_it_reaches`]
//!     prints the count and reds if it goes to zero), so "the hero image" and
//!     "the asset corpus" coincide; the phantom half in `lane_roster_witness.rs`
//!     now walks the directory rather than the filename.
//!
//! ## RED HALF
//!
//! Run against `647ba346`'s `docs/assets/hero.svg` the shipped gate reports the
//! frontend roster as `[cpp, rust, lean]` unregistered / `[bashrs, wasm]`
//! undrawn, the backend roster as `[wasm, bashrs, forjar]` undrawn, the hub
//! numeral as `6` against a live `4`, and the alt-vs-desc rosters as
//! disagreeing. Figures in this header are from running this file against the
//! tree it ships with, not from the notes that preceded it.

use std::path::{Path, PathBuf};

const HERO: &str = "docs/assets/hero.svg";
const README: &str = "README.md";
const ASSETS: &str = "docs/assets";

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = workspace_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// Every `<text … {attr}="<id>">Label</text>` in document order, as
/// `(registry id, visible label)`.
///
/// Deliberately a scan and not an XML parse: the assertion is about the two
/// strings, and a dependency-free reader keeps this test buildable in the same
/// place the rest of the witness corpus is.
fn drawn(svg: &str, attr: &str) -> Vec<(String, String)> {
    let needle = format!("{attr}=\"");
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(i) = rest.find(&needle) {
        let after = &rest[i + needle.len()..];
        let Some(q) = after.find('"') else { break };
        let id = after[..q].to_string();
        let tail = &after[q + 1..];
        // The label is the text node of the element carrying the attribute.
        let label = match (tail.find('>'), tail.find('<')) {
            (Some(gt), _) => {
                let body = &tail[gt + 1..];
                body.find('<').map(|lt| body[..lt].trim().to_string())
            }
            _ => None,
        }
        .unwrap_or_default();
        out.push((id, label));
        rest = tail;
    }
    out
}

/// The two-directional comparison, factored out so a constructed control can
/// exercise it on arrangements the corpus does not contain.
///
/// Returns one finding per disagreement: an id drawn that is not registered, a
/// registered id not drawn, or the same set in a different order.
fn roster_findings(kind: &str, drawn: &[(String, String)], expected: &[String]) -> Vec<String> {
    let ids: Vec<String> = drawn.iter().map(|(id, _)| id.clone()).collect();
    let mut findings = Vec::new();
    for id in &ids {
        if !expected.contains(id) {
            findings.push(format!("{kind}: `{id}` is drawn but nothing registers it"));
        }
    }
    for id in expected {
        if !ids.contains(id) {
            findings.push(format!("{kind}: `{id}` is registered but is not drawn"));
        }
    }
    if findings.is_empty() && ids != expected {
        findings.push(format!(
            "{kind}: drawn in {ids:?} but registered in {expected:?} — the diagram is \
             ordered by registration so that a new lane lands in the right row"
        ));
    }
    findings
}

fn frontend_ids_that_lower(session: &xpile_core::TranspileSession) -> Vec<String> {
    session
        .frontends
        .iter()
        .filter(|f| f.lowers_input())
        .map(|f| f.name().to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// The gate.
// ---------------------------------------------------------------------------

#[test]
fn every_lane_the_hero_draws_is_registered_and_every_registered_lane_is_drawn() {
    let svg = read(HERO);
    let session = xpile_core::default_session();

    let backends: Vec<String> = session
        .backends
        .iter()
        .map(|b| b.name().to_string())
        .collect();
    let contract_frontends: Vec<String> = session
        .contract_frontends
        .iter()
        .map(|f| f.name().to_string())
        .collect();
    let contract_backends: Vec<String> = session
        .contract_backends
        .iter()
        .map(|b| b.name().to_string())
        .collect();

    let mut findings = Vec::new();
    findings.extend(roster_findings(
        "code-lane frontend",
        &drawn(&svg, "data-frontend"),
        &frontend_ids_that_lower(&session),
    ));
    findings.extend(roster_findings(
        "code-lane backend",
        &drawn(&svg, "data-backend"),
        &backends,
    ));
    findings.extend(roster_findings(
        "proof-lane contract frontend",
        &drawn(&svg, "data-contract-frontend"),
        &contract_frontends,
    ));
    findings.extend(roster_findings(
        "proof-lane contract backend",
        &drawn(&svg, "data-contract-backend"),
        &contract_backends,
    ));

    assert!(
        findings.is_empty(),
        "\n{HERO} is the first thing in README.md and it disagrees with \
         `xpile_core::default_session()`:\n  {}\n\n\
         The diagram declares each lane's registry id in a `data-*` attribute; fix the \
         picture, not this test. (PMAT-1464 / XPILE-HERODIAGRAM-001)\n",
        findings.join("\n  ")
    );
}

#[test]
fn the_hub_numeral_is_the_live_count_of_frontends_that_lower() {
    let svg = read(HERO);
    let session = xpile_core::default_session();
    let live = frontend_ids_that_lower(&session).len();

    // `canonical IR · N source langs`
    let marker = "canonical IR · ";
    let i = svg
        .find(marker)
        .unwrap_or_else(|| panic!("{HERO} no longer carries the `{marker}` hub subtitle"));
    let tail = &svg[i + marker.len()..];
    let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
    let drawn_n: usize = digits
        .parse()
        .unwrap_or_else(|_| panic!("{HERO}: hub subtitle has no numeral: {:?}", &tail[..40]));

    assert_eq!(
        drawn_n, live,
        "\n{HERO} says `{drawn_n} source langs`; {live} registered frontends return \
         `lowers_input() == true`.\n\
         This numeral has a gated twin — README.md's \"four source languages\", derived by \
         `claims_drift.rs` by RUNNING each frontend. One claim, two homes; before PMAT-1464 \
         only one of them was gated and the image said 6.\n"
    );
}

#[test]
fn the_image_the_desc_and_the_readme_alt_tell_the_same_story() {
    let svg = read(HERO);
    let readme = read(README);

    let labels =
        |attr: &str| -> Vec<String> { drawn(&svg, attr).into_iter().map(|(_, l)| l).collect() };
    let code = format!(
        "code lane ({} → meta-HIR → {})",
        labels("data-frontend").join(", "),
        labels("data-backend").join(", ")
    );
    let proof = format!(
        "proof lane ({} → contracts (YAML) → {})",
        labels("data-contract-frontend").join(", "),
        labels("data-contract-backend").join(", ")
    );

    // The alt attribute on the hero <img>, which is what a screen reader gets
    // (an `alt` on an `<img>` OVERRIDES the SVG's own `<desc>`), and the
    // `<desc>`, which is what anything embedding the file directly gets.
    let alt = {
        let i = readme
            .find("docs/assets/hero.svg")
            .unwrap_or_else(|| panic!("{README} no longer embeds {HERO}"));
        let tail = &readme[i..];
        let a = tail
            .find("alt=\"")
            .unwrap_or_else(|| panic!("{README}: the hero <img> has no alt text"));
        let body = &tail[a + 5..];
        body[..body.find('"').expect("unterminated alt=")].to_string()
    };
    let desc = {
        let i = svg.find("<desc").expect("hero has a <desc>");
        let body = &svg[i..];
        body[..body.find("</desc>").expect("unterminated <desc>")].to_string()
    };

    for (what, body) in [("README.md alt text", &alt), ("the SVG's <desc>", &desc)] {
        for roster in [&code, &proof] {
            assert!(
                body.contains(roster.as_str()),
                "\n{what} does not carry the roster the image DRAWS:\n  expected: {roster}\n  \
                 in:       {body}\n\n\
                 PMAT-1440 rewrote the alt text and never opened the image, so the alt named \
                 5 sources and 9 backends while the picture drew 6 and 6 — one image, two \
                 architectures, depending on whether you could see it.\n"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Blind spots, each pinned by a control that PASSES.
// ---------------------------------------------------------------------------

#[test]
fn a_registered_frontend_that_does_not_lower_is_correctly_absent() {
    // SUBJECT control. The expected set is `frontends.filter(lowers_input)`,
    // and a filter that removes nothing is not a filter. Live at PMAT-1464:
    // `ruchy` is registered (so `.ruchy` reaches its specific refusal rather
    // than "no frontend handles .ruchy") and returns `lowers_input() == false`.
    // Measuring that it is EXCLUDED, and that the exclusion is not vacuous, is
    // what licenses drawing 4 lanes where 5 are registered.
    let session = xpile_core::default_session();
    let all = session.frontends.len();
    let lowering = frontend_ids_that_lower(&session);
    let routing_only: Vec<String> = session
        .frontends
        .iter()
        .filter(|f| !f.lowers_input())
        .map(|f| f.name().to_string())
        .collect();

    assert!(
        !routing_only.is_empty(),
        "every one of the {all} registered frontends now lowers its input. The filter in \
         this gate no longer removes anything, so DELETE it and say so — the diagram must \
         then draw all {all}. (Do not silently keep a filter that bounds nothing; PMAT-1456.)"
    );
    let svg_ids: Vec<String> = drawn(&read(HERO), "data-frontend")
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    for id in &routing_only {
        assert!(
            !svg_ids.contains(id),
            "{HERO} draws `{id}` as a source lane, but it is registered for ROUTING ONLY — \
             its input refuses and it has no parser. That is the shape the hero image \
             carried for 75 days."
        );
    }
    assert_eq!(
        lowering.len() + routing_only.len(),
        all,
        "the two dispositions must partition the registry"
    );
}

#[test]
fn the_roster_comparison_reports_a_fabricated_lane() {
    // NEEDLE control, on CONSTRUCTED input — the corpus cannot exercise these
    // arms once it is repaired, which is precisely why they are constructed
    // here rather than asserted to be exercised (PMAT-1457/1463).
    let expect = |v: &[&str]| -> Vec<String> { v.iter().map(|s| s.to_string()).collect() };
    let draw = |v: &[&str]| -> Vec<(String, String)> {
        v.iter().map(|s| (s.to_string(), s.to_string())).collect()
    };

    // (a) drawn but unregistered — the C++/Rust/Lean 4/mdBook shape.
    let f = roster_findings("x", &draw(&["python", "cpp"]), &expect(&["python"]));
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(
        f[0].contains("`cpp` is drawn but nothing registers it"),
        "{f:?}"
    );

    // (b) registered but undrawn — the Shell/WebAssembly/forjar shape.
    let f = roster_findings("x", &draw(&["python"]), &expect(&["python", "wasm"]));
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(
        f[0].contains("`wasm` is registered but is not drawn"),
        "{f:?}"
    );

    // (c) same set, wrong order — the arm neither of the two above can see.
    let f = roster_findings(
        "x",
        &draw(&["wasm", "python"]),
        &expect(&["python", "wasm"]),
    );
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].contains("ordered by registration"), "{f:?}");

    // (d) agreement is silence.
    assert!(roster_findings(
        "x",
        &draw(&["python", "wasm"]),
        &expect(&["python", "wasm"])
    )
    .is_empty());
}

#[test]
fn the_gate_reports_what_it_reaches() {
    // ANTI-VACUITY. Every assertion above is over a set this scan produced; a
    // scan that silently produced nothing would make all of them pass.
    let svg = read(HERO);
    let f = drawn(&svg, "data-frontend");
    let b = drawn(&svg, "data-backend");
    let cf = drawn(&svg, "data-contract-frontend");
    let cb = drawn(&svg, "data-contract-backend");
    println!(
        "XPILE-HERODIAGRAM-001 reach: {} code frontends, {} code backends, \
         {} contract frontends, {} contract backends",
        f.len(),
        b.len(),
        cf.len(),
        cb.len()
    );
    for (kind, set) in [
        ("data-frontend", &f),
        ("data-backend", &b),
        ("data-contract-frontend", &cf),
        ("data-contract-backend", &cb),
    ] {
        assert!(
            !set.is_empty(),
            "{HERO} carries no `{kind}` element — the scan reads nothing and every roster \
             assertion in this file is vacuous"
        );
        for (id, label) in set {
            assert!(
                !id.is_empty() && !label.is_empty(),
                "{HERO}: `{kind}` element has an empty id or an empty label ({id:?}/{label:?}) \
                 — a blank label would make the alt-vs-desc comparison agree on nothing"
            );
        }
    }

    // The SUBJECT of `lane_roster_witness.rs`'s phantom half, which PMAT-1464
    // widened from `.md` files to this directory. If the assets move, that walk
    // goes silently empty; this reds instead.
    let assets = workspace_root().join(ASSETS);
    let svgs: Vec<PathBuf> = std::fs::read_dir(&assets)
        .unwrap_or_else(|e| panic!("read {ASSETS}: {e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("svg"))
        .collect();
    assert!(
        !svgs.is_empty(),
        "{ASSETS} holds no .svg — the phantom-lane corpus in lane_roster_witness.rs walks \
         this directory and would be scanning nothing"
    );
    println!("XPILE-HERODIAGRAM-001 asset corpus: {} .svg", svgs.len());
}
