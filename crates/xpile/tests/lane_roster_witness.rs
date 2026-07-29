//! XPILE-LANEROSTER-001 (PMAT-1440) — the book's picture of the system is
//! checked against the system.
//!
//! THE DEFECT. `book/src/concepts/two-lanes.md` opens by calling itself **"the
//! single most important mental model in the system"**, and then drew a roster
//! that was wrong in six independent ways. Measured against
//! `xpile_core::default_session()` at d38e1861:
//!
//! | drawn | truth |
//! |---|---|
//! | frontends `C++`, `Rust`, `Lean 4` | **do not exist** — `.cpp`, `.rs`, `.lean` all exit non-zero, and `frontends.md` says so in as many words |
//! | frontend `wasm` (`.wat`) | registered, **absent from the diagram** |
//! | backends `wasm`, `forjar` | registered, **absent from the diagram** (2 of 9 missing) |
//! | `PTX 🚧 scaffold`, `WGSL 🚧 scaffold`, `SPIR-V 🚧 planned`, `Lean 4 🚧 scaffold` | all four **emit**; `backends.md` grades every one `✅ Real emission` |
//! | ContractFrontends `Lean 4 thm`, `mdBook` | **one** contract frontend is registered (`latex`) |
//! | ContractBackends `mdBook` | **no mdBook contract frontend or backend exists**, which `README.md:179` states outright |
//!
//! WHY IT WAS GREEN. **No test in the repo reads `two-lanes.md`** — verified by
//! `git grep two-lanes -- crates`, which returns nothing. The page is not
//! obscure: `SUMMARY.md` lists it as the first Concepts chapter and
//! `quickstart.md` links it as the first "Next steps" entry.
//!
//! AND THE WORDING WAS ALREADY FORBIDDEN — SOMEWHERE ELSE. `claims_drift.rs::
//! current_md_does_not_carry_the_2026_05_stale_claims` pins the needle
//! `"still scaffolded"` against the truth `"PTX, WGSL and SPIR-V all emit"` —
//! the exact falsehood this diagram carried — but scoped to
//! `docs/status/CURRENT.md`, the one file it was found in. **A regression pin
//! written against a FILE does not protect a CLAIM.** That is PMAT-1438's
//! lesson recurring one slice later, and it is why the rule below ranges over
//! the whole book corpus rather than over this page.
//!
//! THE RULE. A diagram that ENUMERATES the lanes is a claim about the registry,
//! so it is compared to the registry, **both directions**: a name drawn that
//! nothing registers reds, and a registered name not drawn reds. Nothing here
//! is hard-coded — the expected sets come from `default_session()`, the same
//! registry the CLI dispatches through, so adding a backend reds this page
//! until the picture moves (PMAT-1431 §4's both-directions idiom, applied to
//! ASCII art).
//!
//! WHAT WAS DELETED RATHER THAN GATED, and why. The diagram used to carry a
//! per-backend maturity glyph (`✅ real emission` / `🚧 scaffold`). Those are a
//! second copy of `backends.md`'s measured Status column, and the copy is what
//! went stale. Gating a duplicate keeps two things in sync forever; deleting it
//! leaves one home. PMAT-1396's rule — state the invariant, do not restate the
//! data — so the diagram now shows the SHAPE and the Status table owns the
//! MATURITY. A test below pins that the glyphs have not crept back.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const PAGE: &str = "book/src/concepts/two-lanes.md";

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

fn page() -> String {
    let p = workspace_root().join(PAGE);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {PAGE}: {e}"))
}

/// The text between a `<!-- <marker>:BEGIN -->` / `:END` pair.
fn marked(body: &str, marker: &str) -> String {
    let begin = format!("<!-- {marker}:BEGIN -->");
    let end = format!("<!-- {marker}:END -->");
    let s = body
        .find(&begin)
        .unwrap_or_else(|| panic!("{PAGE} must carry `{begin}`"))
        + begin.len();
    let e = body
        .find(&end)
        .unwrap_or_else(|| panic!("{PAGE} must carry `{end}`"));
    assert!(e > s, "{PAGE}: `{end}` precedes `{begin}`");
    body[s..e].to_string()
}

/// The lane names drawn in a diagram, split into (left column, right column).
///
/// A name is a bare lowercase token sitting in a column of the ASCII art. The
/// split is on the arrow that feeds the right-hand column (`─→`): everything
/// before it on a line belongs to the left, everything after to the right. The
/// header rule (`─────`) and the hub label (`meta-HIR`, `contracts`) are not
/// lane names.
fn drawn_rosters(diagram: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    const HUBS: [&str; 2] = ["meta-hir", "contracts"];
    let mut left = BTreeSet::new();
    let mut right = BTreeSet::new();

    for line in diagram.lines() {
        let line = line.trim_end();
        if line.starts_with("```") || line.trim().is_empty() {
            continue;
        }
        if line.contains("Frontend") || line.contains("Backend") || line.trim().starts_with('─') {
            continue; // column headers and the rule under them
        }
        // Everything after the LAST `─→` is the right column; before it, left.
        let (l, r) = match line.rfind("─→") {
            Some(i) => (&line[..i], &line[i + "─→".len()..]),
            None => (line, ""),
        };
        for (chunk, set) in [(l, &mut left), (r, &mut right)] {
            for tok in chunk.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
                let t = tok.trim();
                // Length-1 names are real: the C frontend registers as `c`.
                if t.is_empty() || !t.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
                    continue;
                }
                if HUBS.contains(&t.to_ascii_lowercase().as_str()) {
                    continue;
                }
                set.insert(t.to_string());
            }
        }
    }
    (left, right)
}

fn assert_sets(what: &str, drawn: &BTreeSet<String>, live: &BTreeSet<String>) {
    let phantom: Vec<&String> = drawn.difference(live).collect();
    let missing: Vec<&String> = live.difference(drawn).collect();
    assert!(
        phantom.is_empty() && missing.is_empty(),
        "\n{PAGE} draws a {what} roster that is not the registry's.\n  \
         drawn but NOT registered (phantoms): {phantom:?}\n  \
         registered but NOT drawn:            {missing:?}\n\
         The page calls itself \"the single most important mental model in the system\"; \
         it may not invent a lane or omit one. Redraw it, or explain in prose why a \
         registered lane is left out.",
    );
    assert!(
        !live.is_empty(),
        "default_session() registered zero {what}s — the registry moved, and every \
         comparison above passed over nothing"
    );
}

#[test]
fn the_code_lane_diagram_is_the_code_lane_registry() {
    let session = xpile_core::default_session();
    let live_frontends: BTreeSet<String> = session
        .frontends
        .iter()
        .map(|f| f.name().to_string())
        .collect();
    let live_backends: BTreeSet<String> = session
        .backends
        .iter()
        .map(|b| b.name().to_string())
        .collect();

    let (drawn_frontends, drawn_backends) =
        drawn_rosters(&marked(&page(), "XPILE-LANEROSTER-001:CODE"));

    assert_sets("code-lane frontend", &drawn_frontends, &live_frontends);
    assert_sets("code-lane backend", &drawn_backends, &live_backends);
}

#[test]
fn the_proof_lane_diagram_is_the_proof_lane_registry() {
    let session = xpile_core::default_session();
    let live_frontends: BTreeSet<String> = session
        .contract_frontends
        .iter()
        .map(|f| f.name().to_string())
        .collect();
    let live_backends: BTreeSet<String> = session
        .contract_backends
        .iter()
        .map(|b| b.name().to_string())
        .collect();

    let (drawn_frontends, drawn_backends) =
        drawn_rosters(&marked(&page(), "XPILE-LANEROSTER-001:PROOF"));

    assert_sets(
        "proof-lane contract frontend",
        &drawn_frontends,
        &live_frontends,
    );
    assert_sets(
        "proof-lane contract backend",
        &drawn_backends,
        &live_backends,
    );
}

#[test]
fn the_page_does_not_restate_per_backend_maturity() {
    // The stale glyphs are gone and must not creep back: maturity has ONE home,
    // the measured Status table in backends.md. A second copy is what went
    // stale here for months (PMAT-1396 — state the invariant, do not restate
    // the data).
    let body = page();
    for needle in ["🚧", "✅ real emission", "scaffold + Layer-5", "🚧 planned"] {
        assert!(
            !body.contains(needle),
            "{PAGE} carries {needle:?} again. Per-backend maturity belongs in the measured \
             `backends.md` Status table, which `backend_docs_drift.rs` and \
             `backend_refusal_disclosure_witness.rs` hold to the binary. A copy here has no \
             gate and is what went stale."
        );
    }
    assert!(
        body.contains("backends.md#status"),
        "{PAGE} no longer points at the Status table it defers maturity to — a deferral with \
         no destination is just an omission"
    );
}

#[test]
fn no_book_page_reintroduces_the_phantom_lanes() {
    // THE CLASS, not the page. `claims_drift.rs` pins "still scaffolded"
    // against CURRENT.md only, and the identical falsehood then lived on
    // unpinned in two-lanes.md for months. A regression pin written against a
    // FILE does not protect a CLAIM (PMAT-1438), so this ranges over the whole
    // corpus and names the live truth in the failure.
    let root = workspace_root();
    let session = xpile_core::default_session();
    let registered: BTreeSet<String> = session
        .frontends
        .iter()
        .map(|f| f.name().to_string())
        .chain(
            session
                .contract_frontends
                .iter()
                .map(|f| f.name().to_string()),
        )
        .collect();

    // Spellings a diagram would use for lanes that do not exist. Each is
    // checked to be genuinely unregistered, so this list cannot quietly become
    // a ban on something real.
    let phantoms = ["mdbook", "mdBook", "cpp", "c++"];
    for p in &phantoms {
        assert!(
            !registered.contains(&p.to_ascii_lowercase()),
            "`{p}` is now a registered lane — it is on this gate's phantom list and must come off"
        );
    }

    // The corpus is book/src PLUS README.md PLUS the rendered assets README
    // embeds, deliberately. `README.md:123` advertised a "round-trip between
    // LaTeX and mdBook" while `README.md:179` said "There is no mdBook contract
    // frontend or backend" — one file contradicting itself, 56 lines apart —
    // and the hero image's alt-text drew the same phantom. A gate scoped to
    // `book/src` would have left all three, which is the file-not-class mistake
    // this slice exists to stop repeating.
    //
    // PMAT-1464: and it repeated anyway, one level down. This walk collected
    // `.md` ONLY, so `docs/assets/hero.svg` — the image README.md embeds on its
    // FIRST line — was outside the corpus while drawing `C++` in its frontend
    // column and `mdBook` on both sides of its proof lane, two of the four
    // spellings on the phantom list above. The alt text was repaired here; the
    // image it describes was not opened. A corpus of the FILES that mention a
    // lane is not a corpus of the ARTIFACTS that present one.
    let mut corpus: Vec<PathBuf> = walk_md(&root.join("book/src"));
    corpus.push(root.join("README.md"));
    corpus.extend(walk_ext(&root.join("docs/assets"), "svg"));
    assert!(
        corpus
            .iter()
            .any(|p| p.extension().is_some_and(|e| e == "svg")),
        "the phantom corpus collected no .svg — `docs/assets` has moved and this walk is \
         scanning nothing there (PMAT-1464). Point it at the new home; do not drop the arm."
    );

    let mut offenders = Vec::new();
    let mut mentions = 0usize;
    for entry in corpus {
        let body = std::fs::read_to_string(&entry).unwrap_or_default();
        let rel = entry
            .strip_prefix(&root)
            .unwrap_or(&entry)
            .to_string_lossy()
            .into_owned();
        for (i, para) in paragraphs(&body) {
            let lower = para.to_ascii_lowercase();
            for p in &phantoms {
                let needle = p.to_ascii_lowercase();
                // `mdbook build` / `mdbook test` / a repo URL are the STATIC
                // SITE GENERATOR, which really is used. Only the lane sense
                // counts, and it is the one spelled with a capital B or drawn
                // in a diagram.
                if !para.contains(*p) && !lower.contains(&format!("{needle} ↔")) {
                    continue;
                }
                if lower.contains("mdbook build")
                    || lower.contains("mdbook test")
                    || lower.contains("rust-lang/mdbook")
                {
                    continue;
                }
                mentions += 1;
                // A mention is honest iff its own paragraph DENIES the lane or
                // marks the claim as superseded. Prose may discuss a phantom;
                // it may not present one.
                let denied = lower.contains(&format!("no {needle}"))
                    || lower.contains("not implemented")
                    || lower.contains("through v0.1.");
                if !denied {
                    offenders.push(format!(
                        "{rel}:{i}: presents `{p}` as a lane without denying it exists"
                    ));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "\na published roster presents a lane nothing registers:\n  {}\n\
         The live contract/code frontends are {registered:?}. Say plainly that it does not \
         exist, or stop drawing it.",
        offenders.join("\n  ")
    );
    // NON-VACUITY: the phantom names must still OCCUR somewhere, or the scan is
    // passing over a corpus that no longer says anything (PMAT-1396: a negative
    // over an empty enumeration passes for free). They do occur — in the
    // denials this slice wrote.
    assert!(
        mentions > 0,
        "no phantom-lane name occurs anywhere in README.md or book/src, so this ban is \
         checking nothing. Either the denials were deleted or the scan stopped reaching them."
    );
}

/// A file split into blank-line-delimited paragraphs, flattened, as
/// (starting line number, text). A claim and its denial are a paragraph apart,
/// not a line apart.
fn paragraphs(body: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut start = 1usize;
    let mut buf: Vec<&str> = Vec::new();
    for (i, line) in body.lines().enumerate() {
        if line.trim().is_empty() {
            if !buf.is_empty() {
                out.push((start, buf.join(" ")));
                buf.clear();
            }
            start = i + 2;
        } else {
            if buf.is_empty() {
                start = i + 1;
            }
            buf.push(line);
        }
    }
    if !buf.is_empty() {
        out.push((start, buf.join(" ")));
    }
    out
}

fn walk_md(dir: &Path) -> Vec<PathBuf> {
    walk_ext(dir, "md")
}

/// PMAT-1464: the same walk over an arbitrary extension, so the phantom corpus
/// can reach `docs/assets/*.svg` — an artifact that PRESENTS a lane without
/// being a `.md` file that MENTIONS one.
fn walk_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk_ext(&p, ext));
        } else if p.extension().and_then(|s| s.to_str()) == Some(ext) {
            out.push(p);
        }
    }
    out.sort();
    out
}
