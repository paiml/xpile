//! XPILE-PUBREADME-001 (PMAT-1466) — the document a crate puts on its registry
//! FRONT PAGE is not the document this repo gates.
//!
//! `cargo publish` renders exactly one file per crate as the body of its
//! crates.io page: the one named by `[package] readme`. Across the 31 members
//! that resolves to **two** files, and at `aff7b60f` neither was the one the
//! README gates read:
//!
//! | crate | front page | readers | commits |
//! |---|---|---|---|
//! | `xpile` (the flagship — the crate with the binary) | `crates/xpile/README.md` | **none** | **1**, `ac709bf5`, 2026-05-15 |
//! | `xpile-bigint` (a helper) | `README.md` (workspace root, via `readme.workspace = true`) | 5 gates | 647 |
//!
//! The gated 647-commit product page reaches the crate nobody visits. The
//! flagship publishes a bootstrap stub written the day the repo was created and
//! **never touched again** — verified by `git log --follow`, which returns the
//! single reservation commit.
//!
//! ## What crates.io was serving
//!
//! Fetched from the live registry, not inferred (`GET
//! /api/v1/crates/xpile/0.1.617/readme`, 302 → 200):
//!
//! ```text
//! Status: v0.0.1 — crates.io name reservation. The real CLI lands in v0.1.0+.
//! ...
//! This installs the v0.0.1 placeholder binary. The real CLI follows.
//! ```
//!
//! 617 patch releases after that stopped being true. `cargo install xpile`
//! installs a working transpiler; the page telling you it installs a
//! placeholder is the first thing a visitor reads.
//!
//! It also redraws the roster PMAT-1464 had just finished deleting from
//! `docs/assets/hero.svg` — six frontends of which **three are unregistered**
//! (`C++`, `Rust`, `Lean 4`), both real frontends missing (`Shell`, `WASM`),
//! six of nine backends, and `mdBook` on **both** sides of the proof lane.
//! `mdbook`, `mdBook`, `cpp` and `c++` are four of the four spellings on
//! `lane_roster_witness.rs`'s own phantom list. Its corpus is `book/src` +
//! `README.md` + `docs/assets/*.svg` — the repo's presentation surface. This
//! file was outside it, being served to the registry.
//!
//! And `xpile-bigint`'s page is 20 044 bytes of *the workbench's* product page:
//! `cargo install xpile`, "lowers four source languages", "emits to nine
//! backends" — as the front page of a bigint helper. Not false about xpile;
//! false about the crate it is printed on.
//!
//! ## The shape, fourth recurrence
//!
//! - PMAT-1440 keyed the roster rule to a FILE (`two-lanes.md`) and wrote *"a
//!   regression pin written against a FILE does not protect a CLAIM."*
//! - PMAT-1464 found the corpus collected `.md` only, so the image README
//!   embeds was outside it, and wrote *"a corpus of the FILES that mention a
//!   lane is not a corpus of the ARTIFACTS that present one."*
//! - PMAT-1465 gated all 31 `description`s and all 31 crate-root `//!`s — and
//!   its own module doc says, in as many words, *"only `crates/xpile` ships a
//!   `readme`."* It **named this file to explain why the other surface
//!   mattered, and did not open it.**
//!
//! So: a corpus of the artifacts the REPO presents is not a corpus of the
//! artifacts the REGISTRY presents. The subject below is therefore derived from
//! the manifests — the same resolution `cargo publish` performs — so a crate
//! that gains a front page is gated by acquiring one, not by being remembered.
//!
//! ## Arms
//!
//! Six tests. Against the unmodified corpus **four red**; the two that pass are
//! the controls that make the other four a measurement rather than an argument.
//!
//! 1. `the_packaged_front_page_set_is_derived_and_nonempty` — anti-vacuity on
//!    the SUBJECT. Members come from `[workspace] members`, not a literal list;
//!    the walk must reach ≥ 30 of them and resolve ≥ 1 front page, and every
//!    resolved path must EXIST (a `readme` naming a missing file is a release-day
//!    `cargo publish` failure). **Passes.**
//! 2. `front_page_resolution_handles_every_manifest_shape` — anti-vacuity on the
//!    RESOLVER, against synthetic manifests covering all four spellings
//!    (explicit relative, explicit `../..`, `readme.workspace = true` resolved
//!    against the WORKSPACE root, absent → auto-discovery, `readme = false`).
//!    Without this the resolver could be a constant-`None` machine and arms 3–6
//!    would pass over an empty set. **Passes.**
//! 3. `every_published_front_page_is_read_by_a_gate` — the arc's METHOD as a
//!    rule: which published artifact has no reader? Each front page must be
//!    mentioned by some test in `crates/*/tests` **other than this one**. The
//!    self-exclusion is load-bearing: this file reads all of them, so without it
//!    the rule would certify every future front page for free.
//! 4. `no_published_front_page_presents_a_lane_nothing_registers` — the roster
//!    compared to `default_session()`, both directions, over the DERIVED corpus.
//! 5. `a_published_front_page_installs_the_crate_that_publishes_it` — a
//!    `cargo install X` on a crate's own front page must name that crate, or a
//!    binary it ships.
//! 6. `no_published_front_page_declares_a_stale_release_status` — a `Status:`
//!    declaration naming a version must name the workspace version. Its needle
//!    is pinned by a constructed control, because after the repair no packaged
//!    front page carries a `Status:` line at all and a negative over an empty
//!    enumeration passes for free (PMAT-1396).
//!
//! ## The repair, and why it is a DELETION
//!
//! `crates/xpile/README.md` is not corrected — it is removed, and
//! `crates/xpile/Cargo.toml` points `readme` at `../../README.md`. Editing the
//! stub into accuracy would leave the workspace with two product front pages to
//! keep in sync forever, which is the duplicate-of-the-data shape PMAT-1396
//! forbids and precisely how this one went stale. `../../README.md` is not a
//! guess: `xpile-bigint` has resolved to that exact file since the workspace was
//! created and its 20 KB crates.io page proves `cargo package` copies a readme
//! from outside the package directory. `xpile-bigint` in turn drops
//! `readme.workspace = true`, joining the other 29 members that publish no front
//! page — so the product page is printed on the product, once.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

/// One workspace member and the file `cargo publish` will render as its
/// crates.io page, as a workspace-root-relative path.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FrontPage {
    krate: String,
    /// Member directory, workspace-root-relative (`crates/xpile`).
    dir: String,
    /// `None` when the crate publishes no readme — the crates.io page then
    /// carries only the `description`, which is XPILE-CRATEMETA-001's subject.
    readme: Option<String>,
}

/// Normalise a path containing `..` without touching the filesystem, so the
/// resolver stays a pure function the control below can drive with synthetic
/// inputs.
fn normalise(rel: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

/// The value of a top-level `key = "..."` in a manifest, ignoring `[table]`
/// sections after the first one that could contain it.
///
/// Deliberately a line scan and not a TOML parse: `publish_manifest_integrity.rs`
/// reads these manifests the same way, and a dependency-free reader keeps this
/// test buildable alongside the rest of the witness corpus.
fn string_value(manifest: &str, key: &str) -> Option<String> {
    for line in manifest.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(v) = rest.strip_prefix('=') {
                let v = v.trim();
                if let Some(inner) = v.strip_prefix('"') {
                    if let Some(end) = inner.find('"') {
                        return Some(inner[..end].to_string());
                    }
                }
                return None;
            }
        }
    }
    None
}

fn has_line(manifest: &str, needle: &str) -> bool {
    manifest.lines().any(|l| l.trim() == needle)
}

/// The exact resolution `cargo publish` performs, as a pure function.
///
/// `dir` is the member directory relative to the workspace root;
/// `workspace_readme` is `[workspace.package] readme`, which — unlike an
/// explicit member value — resolves against the WORKSPACE root, not the member
/// directory. That asymmetry is why `xpile-bigint`'s one-line
/// `readme.workspace = true` puts the whole workbench's product page on a
/// bigint crate: it is not a copy of `crates/xpile-bigint/README.md`, it is
/// `../../README.md`.
///
/// `member_has_own_readme` supplies cargo's auto-discovery (a bare `README.md`
/// in the package directory is published even with no `readme` key at all)
/// without this function touching the filesystem.
fn resolve_front_page(
    dir: &str,
    manifest: &str,
    workspace_readme: Option<&str>,
    member_has_own_readme: bool,
) -> Option<String> {
    if has_line(manifest, "readme = false") || has_line(manifest, "readme.workspace = false") {
        return None;
    }
    if has_line(manifest, "readme.workspace = true") {
        return workspace_readme.map(normalise);
    }
    if let Some(v) = string_value(manifest, "readme") {
        return Some(normalise(&format!("{dir}/{v}")));
    }
    member_has_own_readme.then(|| normalise(&format!("{dir}/README.md")))
}

/// `[workspace] members = [ "crates/x", ... ]` from the root manifest — the
/// crates that actually get published, derived rather than listed.
fn workspace_members(root_manifest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Some(start) = root_manifest.find("members = [") else {
        panic!("root Cargo.toml has no `[workspace] members` array");
    };
    let rest = &root_manifest[start..];
    let end = rest.find(']').expect("`members` array is unterminated");
    for line in rest[..end].lines() {
        let line = line.trim();
        let Some(open) = line.find('"') else { continue };
        let after = &line[open + 1..];
        let Some(close) = after.find('"') else {
            continue;
        };
        out.push(after[..close].to_string());
    }
    out
}

/// Every member paired with the file its crates.io page will render.
fn front_pages() -> Vec<FrontPage> {
    let root = workspace_root();
    let root_manifest =
        std::fs::read_to_string(root.join("Cargo.toml")).expect("read root Cargo.toml");
    let workspace_readme = string_value(&root_manifest, "readme");

    workspace_members(&root_manifest)
        .into_iter()
        .map(|dir| {
            let manifest_path = root.join(&dir).join("Cargo.toml");
            let manifest = std::fs::read_to_string(&manifest_path)
                .unwrap_or_else(|e| panic!("read {}: {e}", manifest_path.display()));
            let krate = string_value(&manifest, "name")
                .unwrap_or_else(|| panic!("{dir}/Cargo.toml declares no `name`"));
            let readme = resolve_front_page(
                &dir,
                &manifest,
                workspace_readme.as_deref(),
                root.join(&dir).join("README.md").is_file(),
            );
            FrontPage { krate, dir, readme }
        })
        .collect()
}

/// The members that publish a front page, with its body.
fn published() -> Vec<(FrontPage, String)> {
    let root = workspace_root();
    front_pages()
        .into_iter()
        .filter_map(|fp| {
            let rel = fp.readme.clone()?;
            let body = std::fs::read_to_string(root.join(&rel)).unwrap_or_else(|e| {
                panic!("{} publishes `{rel}`, which cannot be read: {e}", fp.krate)
            });
            Some((fp, body))
        })
        .collect()
}

// ── 1. anti-vacuity on the SUBJECT ──────────────────────────────────────────

#[test]
fn the_packaged_front_page_set_is_derived_and_nonempty() {
    let all = front_pages();
    assert!(
        all.len() >= 30,
        "the member walk reached only {} crates — `[workspace] members` moved or the array \
         scan broke, and every arm below is now measuring a fraction of the registry",
        all.len()
    );

    let root = workspace_root();
    let mut missing = Vec::new();
    for fp in &all {
        if let Some(rel) = &fp.readme {
            if !root.join(rel).is_file() {
                missing.push(format!(
                    "{} publishes `{rel}`, which does not exist",
                    fp.krate
                ));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "\na crate names a front page that is not in the tree — `cargo publish` fails on \
         this at release time, not before:\n  {}",
        missing.join("\n  ")
    );

    let with = all.iter().filter(|f| f.readme.is_some()).count();
    assert!(
        with > 0,
        "no crate in the workspace resolves to a front page, so arms 3-6 are quantifying \
         over nothing. Either every `readme` key was deleted or `resolve_front_page` stopped \
         resolving (PMAT-1396: a negative over an empty enumeration passes for free)."
    );
}

// ── 2. anti-vacuity on the RESOLVER ─────────────────────────────────────────

#[test]
fn front_page_resolution_handles_every_manifest_shape() {
    // Explicit relative — the flagship's shape after this slice's repair.
    assert_eq!(
        resolve_front_page(
            "crates/xpile",
            "readme = \"../../README.md\"\n",
            None,
            false
        ),
        Some("README.md".to_string()),
        "an explicit `readme` resolves against the MEMBER directory"
    );
    assert_eq!(
        resolve_front_page("crates/xpile", "readme = \"README.md\"\n", None, true),
        Some("crates/xpile/README.md".to_string())
    );

    // Workspace inheritance resolves against the WORKSPACE root. This is the
    // asymmetry that made `xpile-bigint` publish the workbench's product page,
    // and after the repair no member uses it — so without this control the
    // branch would be untested dead code.
    assert_eq!(
        resolve_front_page(
            "crates/xpile-bigint",
            "readme.workspace = true\n",
            Some("README.md"),
            false
        ),
        Some("README.md".to_string()),
        "`readme.workspace = true` takes the WORKSPACE-root-relative path, not a sibling \
         copy inside the member directory"
    );

    // Auto-discovery: a bare README.md in the package directory is published
    // with no `readme` key at all, so absence of the key is not absence of a
    // front page.
    assert_eq!(
        resolve_front_page("crates/xpile-core", "name = \"xpile-core\"\n", None, true),
        Some("crates/xpile-core/README.md".to_string()),
        "cargo auto-discovers README.md — a crate can acquire a front page without \
         touching its manifest"
    );
    assert_eq!(
        resolve_front_page("crates/xpile-core", "name = \"xpile-core\"\n", None, false),
        None
    );
    assert_eq!(
        resolve_front_page("crates/x", "readme = false\n", Some("README.md"), true),
        None,
        "`readme = false` opts out even when a README.md sits next to the manifest"
    );

    // The line scan must not be fooled by a hyphenated lookalike key, the same
    // boundary `publish_manifest_integrity.rs::declared_version_ignores_\
    // hyphenated_lookalike_keys` pins for `version`.
    assert_eq!(
        resolve_front_page("crates/x", "readme-renderer = \"mdbook\"\n", None, false),
        None,
        "`readme-renderer` is not `readme`"
    );
}

// ── 3. the arc's METHOD as a rule ───────────────────────────────────────────

/// Every `.rs` under a crate's `tests/` directory, except this file.
fn sibling_test_sources(root: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let me = Path::new(file!())
        .file_name()
        .expect("file!() has a file name")
        .to_string_lossy()
        .into_owned();
    let crates = root.join("crates");
    let Ok(entries) = std::fs::read_dir(&crates) else {
        panic!("crates/ is unreadable");
    };
    for e in entries.flatten() {
        let tests = e.path().join("tests");
        let Ok(files) = std::fs::read_dir(&tests) else {
            continue;
        };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().and_then(|s| s.to_str()) != Some("rs") {
                continue;
            }
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if name == me {
                continue;
            }
            out.push((name, std::fs::read_to_string(&p).unwrap_or_default()));
        }
    }
    out
}

#[test]
fn every_published_front_page_is_read_by_a_gate() {
    let root = workspace_root();
    let sources = sibling_test_sources(&root);
    assert!(
        sources.len() > 50,
        "only {} sibling test sources found — the tests/ walk broke and this rule is \
         about to certify everything for free",
        sources.len()
    );

    let mut orphans = Vec::new();
    for (fp, _) in published() {
        let rel = fp.readme.clone().expect("published() filters to Some");
        // The path as a STRING LITERAL, not as a mention. Six of these test
        // files discuss `README.md` in their `//!` blocks without opening it —
        // and a bare `contains(&rel)` would count that prose as a reader, which
        // is the flattering-grep trap this whole arc keeps finding. Requiring
        // the quoted form admits `const README: &str = "README.md"` and
        // `root.join("README.md")` and rejects commentary.
        //
        // Known limit, stated rather than papered over: a gate that built the
        // path piecewise (`root.join("crates/foo").join("README.md")`) would not
        // be seen. No gate in the corpus does that today.
        let needle = format!("\"{rel}\"");
        let read_by = sources.iter().any(|(_, src)| src.contains(&needle));
        if !read_by {
            orphans.push(format!(
                "{} publishes `{rel}` and no test in the workspace reads it",
                fp.krate
            ));
        }
    }
    assert!(
        orphans.is_empty(),
        "\na crate's crates.io front page has no reader — this is the shape PMAT-1383..1465 \
         kept finding, on the surface with the widest audience:\n  {}\n\
         Point the crate at a document that is already gated, or gate the document.",
        orphans.join("\n  ")
    );
}

// ── 4. the roster, compared to the registry ─────────────────────────────────

/// The lowercase names of everything `default_session()` actually registers.
fn registered_lanes() -> BTreeSet<String> {
    let s = xpile_core::default_session();
    s.frontends
        .iter()
        .map(|f| f.name().to_string())
        .chain(s.backends.iter().map(|b| b.name().to_string()))
        .chain(s.contract_frontends.iter().map(|f| f.name().to_string()))
        .chain(s.contract_backends.iter().map(|b| b.name().to_string()))
        .map(|n| n.to_ascii_lowercase())
        .collect()
}

#[test]
fn no_published_front_page_presents_a_lane_nothing_registers() {
    let registered = registered_lanes();

    // The same four spellings `lane_roster_witness.rs` bans over the repo's
    // presentation surface, each re-checked against the live registry so the
    // list cannot quietly become a ban on something real. The rule is
    // duplicated rather than shared deliberately: PMAT-1440's lesson is that
    // keying a rule to one corpus is what fails, and the registry's corpus and
    // the repo's are different sets that must each be swept.
    let phantoms = ["mdbook", "mdBook", "cpp", "c++"];
    for p in &phantoms {
        assert!(
            !registered.contains(&p.to_ascii_lowercase()),
            "`{p}` is now a registered lane — it is on this gate's phantom list and must come off"
        );
    }

    let mut offenders = Vec::new();
    let mut mentions = 0usize;
    for (fp, body) in published() {
        let rel = fp.readme.clone().expect("published() filters to Some");
        for (i, para) in paragraphs(&body) {
            for p in &phantoms {
                if !para.contains(*p) {
                    continue;
                }
                let lower = para.to_ascii_lowercase();
                // The static site generator really is used; only the lane sense
                // counts (`lane_roster_witness.rs`'s carve-out, verbatim).
                if lower.contains("mdbook build")
                    || lower.contains("mdbook test")
                    || lower.contains("rust-lang/mdbook")
                {
                    continue;
                }
                mentions += 1;
                let denied = lower.contains(&format!("no {}", p.to_ascii_lowercase()))
                    || lower.contains("not implemented")
                    || lower.contains("through v0.1.");
                if !denied {
                    offenders.push(format!(
                        "{rel}:{i}: {} publishes `{p}` as a lane; the live registry is {registered:?}",
                        fp.krate
                    ));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "\na crates.io front page draws a lane nothing registers:\n  {}\n\
         Every visitor arriving from the registry reads this before anything else in the repo.",
        offenders.join("\n  ")
    );
    // NON-VACUITY: after the repair the published corpus is the root README,
    // which names `mdBook` three times — all denials ("there is no mdBook
    // contract frontend or backend"). If none survives, the scan has stopped
    // reaching the file rather than the file having become clean.
    assert!(
        mentions > 0,
        "no phantom-lane spelling occurs on any published front page, so this ban is \
         checking nothing (PMAT-1396). Either the denials were deleted or the derived \
         corpus no longer resolves to a real document."
    );
}

// ── 5. the page must be about the crate it is printed on ────────────────────

/// A file split into blank-line-delimited paragraphs, as (starting line number,
/// text) — `lane_roster_witness.rs`'s idiom, and for its reason: a claim and its
/// denial are a paragraph apart, not a line apart.
///
/// This gate's own red half proved the point. Scanning by LINE, `README.md:125`
/// ("…advertised a \"round-trip between LaTeX and mdBook\"") reported as an
/// offender while its denial — "no mdBook lane at all" — sat one line above on
/// `:124`, inside the same bullet.
fn paragraphs(body: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut start = 1usize;
    let mut buf: Vec<&str> = Vec::new();
    for (i, line) in body.lines().enumerate() {
        if line.trim().is_empty() {
            if !buf.is_empty() {
                out.push((start, buf.join("\n")));
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
        out.push((start, buf.join("\n")));
    }
    out
}

/// Every `cargo install <name>` the reader is INSTRUCTED to run — i.e. inside a
/// fenced code block, not inline in prose.
///
/// The distinction is load-bearing and was measured, not assumed: `README.md`
/// carries three `cargo install` occurrences, and the third
/// (`:242`, "the original `cargo install depyler` / `decy` / `ruchy` consumers
/// keep working") is prose ABOUT the workspace's aliases. Counting it would have
/// fabricated a finding — the same trap PMAT-1464 recorded when it measured a
/// lowercased `c++` needle and rejected it for inventing "a C++ backend
/// implementer".
fn install_targets(body: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut fenced = false;
    for (i, line) in body.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !fenced {
            continue;
        }
        let Some(at) = line.find("cargo install ") else {
            continue;
        };
        let rest = line[at + "cargo install ".len()..].trim_start();
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if !name.is_empty() {
            out.push((i + 1, name));
        }
    }
    out
}

#[test]
fn a_published_front_page_installs_the_crate_that_publishes_it() {
    let mut offenders = Vec::new();
    let mut checked = 0usize;
    for (fp, body) in published() {
        let rel = fp.readme.clone().expect("published() filters to Some");
        for (line, target) in install_targets(&body) {
            checked += 1;
            if target != fp.krate {
                offenders.push(format!(
                    "{rel}:{line}: this is `{}`'s front page and it tells the reader to \
                     `cargo install {target}`",
                    fp.krate
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "\na crate's registry page advertises a different crate's binary:\n  {}\n\
         A front page inherited from the workspace is about the workspace, not about the \
         crate it lands on. Give the crate its own page or publish none (29 of 31 do).",
        offenders.join("\n  ")
    );
    // NON-VACUITY: the flagship's page carries the install line, so a zero here
    // means the scan stopped reaching it rather than that the corpus is clean.
    assert!(
        checked > 0,
        "no `cargo install` line was found on any published front page — the extractor is \
         scanning nothing"
    );
}

// ── 6. a front page may not be stale about the release ──────────────────────

/// The versions declared by a `Status:` line — a document stating, in its own
/// words, which release it describes.
fn status_versions(body: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, line) in body.lines().enumerate() {
        let trimmed = line.trim_start().trim_start_matches(['*', '#', '-', ' ']);
        if !trimmed.to_ascii_lowercase().starts_with("status:") {
            continue;
        }
        for (idx, c) in line.char_indices() {
            if c != 'v' {
                continue;
            }
            let rest = &line[idx + 1..];
            let lit: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if lit.matches('.').count() == 2 && lit.split('.').all(|s| !s.is_empty()) {
                out.push((i + 1, lit));
            }
        }
    }
    out
}

#[test]
fn no_published_front_page_declares_a_stale_release_status() {
    let root = workspace_root();
    let root_manifest =
        std::fs::read_to_string(root.join("Cargo.toml")).expect("read root Cargo.toml");
    let live =
        string_value(&root_manifest, "version").expect("[workspace.package] declares a version");

    let mut offenders = Vec::new();
    for (fp, body) in published() {
        let rel = fp.readme.clone().expect("published() filters to Some");
        for (line, declared) in status_versions(&body) {
            if declared != live {
                offenders.push(format!(
                    "{rel}:{line}: front page declares `Status: v{declared}` while the \
                     workspace publishes {live}"
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "\na crates.io front page describes a release that is not the one being served:\n  {}",
        offenders.join("\n  ")
    );

    // NEEDLE CONTROL. After the repair no published front page carries a
    // `Status:` line at all, so the loop above quantifies over an empty set and
    // would pass however broken the extractor were (PMAT-1396). This drives it
    // on the exact text crates.io was serving for `xpile 0.1.617`.
    let stale = "# xpile\n\nStatus: **v0.0.1 — crates.io name reservation.** The real CLI \
                 lands in v0.1.0+.\n";
    assert_eq!(
        status_versions(stale),
        vec![(3, "0.0.1".to_string()), (3, "0.1.0".to_string())],
        "the `Status:` extractor no longer finds the declaration this gate was written for"
    );
    assert!(
        status_versions("Status of the parser: complete\nv1.2.3 elsewhere\n").is_empty(),
        "the extractor fires on prose that merely contains the word `status`"
    );
}
