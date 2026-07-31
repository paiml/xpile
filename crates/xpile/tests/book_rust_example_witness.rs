//! XPILE-BOOKAPI-001 (PMAT-1439) — every Rust example the book publishes is a
//! region of a file cargo COMPILES.
//!
//! THE DEFECT. `book/src` contained four ```rust fences describing xpile's
//! library API. **Not one of them compiled**, and none of the four methods they
//! call is a method of the trait it is called on. Measured on the tree at
//! 059d4ceb:
//!
//! | published | what it actually is | the real thing |
//! |---|---|---|
//! | `MetaHirModule` | absent from `crates/*/src` entirely | `xpile_meta_hir::Module` |
//! | `EmittedArtifact` | absent entirely | `xpile_backend::Artifact` |
//! | `target_label` | absent entirely | `Backend::targets(&self) -> &[Target]` |
//! | `DepylerFrontend` | absent entirely | `depyler_frontend::PythonFrontend` |
//! | `backend.emit_module(&m)` | a FREE function in four codegen crates, `-> Result<String, _>` | `Backend::lower(&self, module, config) -> Result<Artifact, _>` |
//! | `frontend.parse_str(src, name)` | not a `Frontend` method; the `parse_str` in-tree is naga's WGSL parser | `Frontend::parse_and_lower(&self, path, source)` |
//! | `frontend.parse_file(path)` | not a `Frontend` method; the `parse_file` in-tree is `syn::parse_file` | the same |
//!
//! plus `RustBackend::default()` on a unit struct with no `Default` impl, and
//! two trait signatures (`fn name(&self) -> &str`, `fn extensions(&self) ->
//! &[&str]`) that do not match the traits they claim to implement.
//!
//! **THREE OF THE SEVEN NAMES EXIST**, which is the important half and the
//! reason a cheaper gate would not have helped. `emit_module` is a real,
//! public, sensibly-named function — it is simply a free function returning a
//! `String`, not a method on `Backend` returning an `Artifact`. An
//! identifier-presence check over the book would have found all three and
//! passed. **A name-presence check is not an API check**, and neither is a
//! reader's eye: the fence looked right because it very nearly was.
//!
//! WORST OF THE FOUR, and it is not the flashiest: `adding-a-frontend.md` §3 —
//! the page that exists to tell a contributor how to implement `Frontend` —
//! omitted `refused_claims()`. That method is REQUIRED with no default, and the
//! trait says why in as many words: a default of `&[]` "would let the next
//! frontend with a partial refusal inherit the exact silence this method exists
//! to break" (PMAT-1433). So the guide handed contributors an impl that does
//! not compile and which, had the names been right, would have reproduced
//! exactly the silence PMAT-1433 was written to remove.
//!
//! WHY IT WAS GREEN FOR 74 DAYS. CI's `book` job runs `mdbook build`, which
//! renders Markdown; it never runs `mdbook test`, which would compile the
//! fences. And the book is not unwatched — TEN tests read the whole `book/src`
//! corpus (`backend_docs_drift`, `cli_docs_drift`, `claims_drift`, and the
//! PMAT-1430/1433/1435/1437/1438 witnesses). **Zero of them parse a ```rust
//! fence.** Every gate the book has reads its tables, its transcripts, its
//! `--target` spellings and its contract attributions — that is, its PROSE. The
//! one thing a reader would paste into their own crate was the one thing
//! nothing read.
//!
//! THE RULE, and why it is this rule. A fence could be checked by grepping its
//! identifiers, but that cannot see a wrong SIGNATURE — and two of these four
//! were wrong in the signature as well as the name. So the property is the
//! strong one, and it is a relation between two live things rather than a list:
//!
//! > every ```rust fence in `book/src` is byte-identical to a marked region of
//! > `crates/xpile/tests/book_api_examples.rs`, a file cargo compiles.
//!
//! An API rename therefore breaks the BUILD, not a string comparison, and the
//! book cannot be wrong for longer than `cargo test` takes. This is PMAT-1415's
//! idiom — derive the published example from the artifact rather than copying
//! it — applied to the book's Rust, where it had never been applied.
//!
//! BOTH DIRECTIONS, deliberately. A fence with no region is an unchecked
//! example; a region with no fence is a check over something nobody publishes.
//! Either one silently re-opens the hole, so both red.
//!
//! THE SCOPE DEFECT (PMAT-1444) — and it was in this file, not in the book.
//! `the_four_wholly_invented_names_are_absent_from_the_source` sweeps
//! `crates/*/src` and must NOT read `tests/`, because this file and
//! `book_api_examples.rs` name all four fabricated identifiers on purpose.
//! Through v0.1.617 it implemented that as
//! `p.components().any(|c| c.as_os_str() == "src")` over an **absolute** path.
//! An absolute path carries the components of every ancestor directory, so the
//! filter asks a question about the MACHINE, not about the repository. On one
//! commit (`dd66f76d`, unmodified `main`) the same test returns two verdicts:
//!
//! | checkout | ancestor named `src`? | verdict |
//! |---|---|---|
//! | `/home/noah/src/xpile` — this project's canonical checkout | yes | **FAILED** |
//! | `/tmp/claude-1000/xpile-wt/src/wt-1444` | yes | **FAILED** |
//! | `/tmp/claude-1000/xpile-wt/plain/wt-1444b` | no | ok |
//! | `/home/runner/work/xpile/xpile` — CI | no | ok |
//!
//! So the gate was RED on the machine that wrote it and GREEN in CI, and the
//! discriminator was whether some parent directory happened to be spelled
//! `src`. It read 302 files / 10.5 MB instead of 43 / 5.3 MB — every `tests/`,
//! `benches/` and `examples/` file in the workspace — and then reddened on its
//! own evidence, naming `MetaHirModule` as having appeared under
//! `crates/*/src`. It had not.
//!
//! **The 1 MB floor below cannot see this, and the reason generalises.** That
//! floor guards against reading too LITTLE; this bug reads too MUCH (5.3 MB of
//! genuine `src` either way, so the floor is satisfied on both sides). A
//! vacuity floor is a one-directional instrument. The repair therefore states
//! the sweep's subject STRUCTURALLY — every path read must be under a `src/` —
//! instead of adding a second cardinality that would drift.
//!
//! WHY IT IS A RELEASE ITEM. `cargo test --workspace --no-fail-fast` on `main`
//! at `dd66f76d` exits 101 here: **2 failing test binaries out of 311**. This
//! was one. (The other is `ruleset_drift`'s
//! `live_ruleset_matches_the_committed_snapshot`, which reports a REAL live-org
//! drift and is owner-gated — re-deriving its snapshot would ratify the
//! weakening — so it is deliberately left alone.) Release precondition A1
//! requires the suite to exit 0 on the tag SHA, so a gate that is red wherever
//! it is authored is scheduled ahead of the 2026-07-30 tag cut, not after it.
//!
//! Note the ordering that hid it: `cargo test --workspace` WITHOUT
//! `--no-fail-fast` stops at the first failing binary, and this one sorts
//! before `ruleset_drift`. Enumerating "what is red" needs the flag.
//!
//! SCOPE, stated rather than implied: this pins the ```rust fences. `bash`,
//! `toml` and `text` fences are not covered — `cli_docs_drift` and
//! `backend_docs_drift` cover the command transcripts, and the `toml` snippets
//! are dependency stanzas. `book/src/tutorials/python-to-rust.md` carried a
//! fence TAGGED `rust` that is a list of test names, not Rust; PMAT-1439
//! retagged it `text`, which is what it is, and the test-name claims it makes
//! are checked below instead — retagging must not be a way to leave the class.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const EXAMPLES: &str = "crates/xpile/tests/book_api_examples.rs";
const BEGIN: &str = "BOOK-EXAMPLE-BEGIN ";
const END: &str = "BOOK-EXAMPLE-END ";

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

/// Is `path` a Rust file under some crate's `src/`?
///
/// **Judged RELATIVE to `root`**, and that is the whole point. Through
/// v0.1.617 this was spelled `path.components().any(|c| c.as_os_str() ==
/// "src")` over the ABSOLUTE path, which makes the answer a function of where
/// the repository happens to be checked out rather than of the repository. See
/// this file's header, THE SCOPE DEFECT.
fn is_crate_src(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    rel.components().any(|c| c.as_os_str() == "src")
}

/// Every `.rs` file under `crates/*/src`, as (path RELATIVE to the workspace
/// root, contents).
///
/// Relative on purpose: a caller that gets absolute paths back is one
/// `components()` away from re-opening the defect this function exists to
/// close, and the relative path is also what any failure message should print.
fn crate_src_files(root: &Path) -> Vec<(PathBuf, String)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(PathBuf, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            if p.is_dir() {
                // `file_name()` — not a component scan. This line was always
                // location-independent; the one eight lines below it was not.
                if p.file_name().and_then(|s| s.to_str()) == Some("target") {
                    continue;
                }
                walk(&p, root, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") && is_crate_src(root, &p)
            {
                let rel = p.strip_prefix(root).unwrap_or(&p).to_path_buf();
                out.push((rel, std::fs::read_to_string(&p).unwrap_or_default()));
            }
        }
    }
    let mut out = Vec::new();
    walk(&root.join("crates"), root, &mut out);
    out
}

fn book_pages(root: &Path) -> Vec<(String, String)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            if p.is_dir() {
                walk(&p, root, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("md") {
                let rel = p
                    .strip_prefix(root)
                    .expect("book page under workspace root")
                    .to_string_lossy()
                    .into_owned();
                let body =
                    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {rel}: {e}"));
                out.push((rel, body));
            }
        }
    }
    let mut out = Vec::new();
    walk(&root.join("book/src"), root, &mut out);
    out
}

/// Strip the common leading indentation from a block. The regions inside a
/// `mod` are indented by four spaces; the book publishes them flush left, and
/// re-indenting the file must not read as a book change.
fn dedent(lines: &[String]) -> String {
    let pad = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            if l.len() >= pad {
                &l[pad..]
            } else {
                l.trim_start()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The ```rust fences of one page, as (line number, body).
fn rust_fences(body: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut open: Option<(usize, Vec<String>)> = None;
    for (i, line) in body.lines().enumerate() {
        match &mut open {
            None => {
                let t = line.trim_end();
                if t == "```rust" || t.starts_with("```rust,") {
                    open = Some((i + 1, Vec::new()));
                }
            }
            Some((start, buf)) => {
                if line.trim_end() == "```" {
                    out.push((*start, buf.join("\n")));
                    open = None;
                } else {
                    buf.push(line.to_string());
                }
            }
        }
    }
    out
}

/// The marked regions of `book_api_examples.rs`, keyed by the page they belong
/// to. A duplicate key panics rather than silently keeping one — two regions
/// claiming one page would make the bijection below meaningless.
fn marked_regions(root: &Path) -> BTreeMap<String, String> {
    let path = root.join(EXAMPLES);
    let body =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut open: Option<(String, Vec<String>)> = None;
    for line in body.lines() {
        if let Some(rest) = line
            .trim()
            .strip_prefix("// ")
            .and_then(|c| c.strip_prefix(BEGIN))
        {
            assert!(
                open.is_none(),
                "{EXAMPLES}: nested BOOK-EXAMPLE-BEGIN at {rest}"
            );
            open = Some((rest.trim().to_string(), Vec::new()));
            continue;
        }
        if let Some(rest) = line
            .trim()
            .strip_prefix("// ")
            .and_then(|c| c.strip_prefix(END))
        {
            let (key, buf) = open.take().unwrap_or_else(|| {
                panic!("{EXAMPLES}: BOOK-EXAMPLE-END {rest} with no matching BEGIN")
            });
            assert_eq!(
                key,
                rest.trim(),
                "{EXAMPLES}: region opened for {key} closed for {rest}"
            );
            let prev = out.insert(key.clone(), dedent(&buf));
            assert!(prev.is_none(), "{EXAMPLES}: two regions claim {key}");
            continue;
        }
        if let Some((_, buf)) = &mut open {
            buf.push(line.to_string());
        }
    }
    assert!(
        open.is_none(),
        "{EXAMPLES}: unterminated BOOK-EXAMPLE-BEGIN"
    );
    out
}

#[test]
fn every_rust_fence_in_the_book_is_a_region_of_a_compiled_file() {
    let root = workspace_root();
    let regions = marked_regions(&root);
    let mut offenders: Vec<String> = Vec::new();
    let mut matched = 0usize;

    for (rel, body) in book_pages(&root) {
        let fences = rust_fences(&body);
        if fences.len() > 1 {
            offenders.push(format!(
                "{rel}: {} ```rust fences on one page — the marker key is the PAGE, so a second \
                 fence cannot be pinned. Split the page or extend the key scheme.",
                fences.len()
            ));
            continue;
        }
        let Some((line, fence)) = fences.into_iter().next() else {
            continue;
        };
        match regions.get(&rel) {
            None => offenders.push(format!(
                "{rel}:{line}: a ```rust fence with no `{BEGIN}{rel}` region in {EXAMPLES}. \
                 Nothing compiles it, so nothing can tell whether the API it shows exists — \
                 which is how all four of this book's API examples came to name zero real \
                 symbols."
            )),
            Some(region) => {
                if region.trim_end() != fence.trim_end() {
                    offenders.push(format!(
                        "{rel}:{line}: the published fence and its compiled region differ.\n\
                         --- book ---\n{fence}\n--- {EXAMPLES} ---\n{region}\n---"
                    ));
                } else {
                    matched += 1;
                }
            }
        }
    }

    assert!(offenders.is_empty(), "\n{}\n", offenders.join("\n\n"));
    // NON-VACUITY by anchor, not by count. These are the four pages whose
    // examples were fabricated; if the scan stops finding them, the fence
    // parser has drifted away from the book's markup and this gate is checking
    // an empty set (PMAT-1396).
    for anchor in [
        "book/src/reference/frontends.md",
        "book/src/reference/backends.md",
        "book/src/contributing/adding-a-frontend.md",
        "book/src/contributing/adding-a-backend.md",
    ] {
        assert!(
            regions.contains_key(anchor),
            "{EXAMPLES} has no region for {anchor}, one of the four pages this gate exists for"
        );
    }
    assert!(
        matched >= 4,
        "only {matched} fence(s) matched a compiled region; the four anchor pages each publish one"
    );
}

#[test]
fn every_compiled_region_is_published_by_the_page_it_names() {
    // The other direction. A region with no fence is a check over something
    // nobody reads — it would let a page quietly drop its example while the
    // gate stayed green, which is the same hole in a different shape.
    let root = workspace_root();
    let regions = marked_regions(&root);
    let pages: BTreeMap<String, String> = book_pages(&root).into_iter().collect();

    // XPILE-SKIPGUARD-003 (PMAT-1509): `marked_regions` SELECTS — it keeps only
    // the lines between `BOOK-EXAMPLE-BEGIN`/`-END` markers. Rename either
    // marker and it returns an empty map, this loop iterates nothing, and the
    // test below reports `ok` having checked no region at all. The sibling
    // `matched >= 4` above floors the same set from the fence side; this floors
    // it from the region side, where the loop actually is. Measured 4 on
    // 2026-07-31.
    assert!(
        regions.len() >= 4,
        "{EXAMPLES} yielded {} marked region(s); the four anchor pages each publish \
         one. A renamed BEGIN/END marker empties this map and the bijection below \
         then checks nothing while still printing `ok`.",
        regions.len()
    );

    for key in regions.keys() {
        let body = pages.get(key).unwrap_or_else(|| {
            panic!("{EXAMPLES}: region names {key}, which is not a page under book/src/")
        });
        assert!(
            !rust_fences(body).is_empty(),
            "{EXAMPLES}: region for {key}, but that page publishes no ```rust fence. Either the \
             page dropped its example or the region is stale."
        );
    }
}

/// The method names declared by a `pub trait <name>` block.
fn trait_methods(root: &Path, krate: &str, trait_name: &str) -> Vec<String> {
    let path = root.join(format!("crates/{krate}/src/lib.rs"));
    let body =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let start = body
        .find(&format!("pub trait {trait_name}"))
        .unwrap_or_else(|| panic!("{krate} declares no `pub trait {trait_name}`"));
    let mut depth = 0usize;
    let mut out = Vec::new();
    for line in body[start..].lines() {
        for c in line.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        if let Some(rest) = line.trim().strip_prefix("fn ") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                out.push(name);
            }
        }
        if depth == 0 && !out.is_empty() {
            break;
        }
    }
    out
}

#[test]
fn the_published_calls_are_not_methods_of_the_traits_they_were_called_on() {
    // The precise half, and the one that says what was actually wrong. Three of
    // the seven published names DO exist in the tree — `emit_module` is a real
    // free function in four codegen crates — so "the name is absent" is false
    // and would have been a lie in this file's own header. What IS true, and is
    // what makes all four fences uncompilable, is that none of these is a
    // method of the trait the fence calls it on.
    let root = workspace_root();
    let backend = trait_methods(&root, "xpile-backend", "Backend");
    let frontend = trait_methods(&root, "xpile-frontend", "Frontend");

    assert!(
        backend.len() >= 3 && frontend.len() >= 6,
        "the trait parser found {} Backend and {} Frontend methods — it is not reaching the \
         trait bodies, so every assertion below would pass vacuously: {backend:?} / {frontend:?}",
        backend.len(),
        frontend.len()
    );

    for m in ["emit_module", "target_label"] {
        assert!(
            !backend.iter().any(|k| k == m),
            "`{m}` is now a `Backend` method. The book once published it as one and this file \
             calls that false — update the header table rather than leaving it to rot."
        );
    }
    for m in ["parse_str", "parse_file"] {
        assert!(
            !frontend.iter().any(|k| k == m),
            "`{m}` is now a `Frontend` method; this file's header calls that false."
        );
    }
    // Both directions: the replacements must really be the trait's surface, or
    // the book now publishes a second fiction.
    for m in ["lower", "targets", "name"] {
        assert!(
            backend.iter().any(|k| k == m),
            "`Backend::{m}` is gone, but the book's corrected example calls it: {backend:?}"
        );
    }
    for m in ["parse_and_lower", "refused_claims", "extensions", "name"] {
        assert!(
            frontend.iter().any(|k| k == m),
            "`Frontend::{m}` is gone, but the book's corrected example implements it: {frontend:?}"
        );
    }
}

#[test]
fn the_four_wholly_invented_names_are_absent_from_the_source() {
    // The other four were not near-misses — they name nothing at all. Kept as
    // its own test so the two claims red separately: a type that appears is a
    // different event from a trait that grows a method.
    let root = workspace_root();
    let fabricated = [
        ("MetaHirModule", "xpile_meta_hir::Module"),
        ("EmittedArtifact", "xpile_backend::Artifact"),
        ("target_label", "Backend::targets"),
        ("DepylerFrontend", "depyler_frontend::PythonFrontend"),
    ];

    let scanned = crate_src_files(&root);

    // Non-vacuity, TOO LITTLE: an empty sweep satisfies every negative below.
    let bytes: usize = scanned.iter().map(|(_, t)| t.len()).sum();
    assert!(
        bytes > 1_000_000,
        "the source sweep read only {bytes} bytes from {} file(s) — it is not reaching \
         crates/*/src, so every assertion below would pass vacuously",
        scanned.len()
    );

    // Non-vacuity, TOO MUCH — the direction a floor cannot see (PMAT-1444).
    // Stated over the VOCABULARY rather than as a count, so it does not drift:
    // no path this sweep read may sit under a non-`src` crate directory. This
    // file and its sibling name the fabricated identifiers on purpose, so a
    // sweep that reached `tests/` would red on its own evidence — which is
    // exactly what happened, for every checkout whose absolute path contained
    // a directory called `src`.
    for (rel, _) in &scanned {
        let comps: Vec<&str> = rel.iter().filter_map(|c| c.to_str()).collect();
        assert!(
            comps.contains(&"src"),
            "the source sweep read `{}`, which is not under any crate's `src/`. This test's \
             whole subject is `crates/*/src`; reading past it makes the negatives below fail \
             on this file's own evidence.",
            rel.display()
        );
    }

    let src: String = scanned
        .iter()
        .map(|(_, t)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    for (name, real) in fabricated {
        assert!(
            !src.contains(name),
            "`{name}` now exists under crates/*/src. This file's header calls it fabricated and \
             tells the reader the real spelling is `{real}` — update the table rather than \
             leaving it to rot."
        );
    }

    let book: String = book_pages(&root)
        .into_iter()
        .map(|(_, b)| b)
        .collect::<Vec<_>>()
        .join("\n");
    for (name, real) in fabricated {
        // The book may still MENTION a fabricated name in a historical note —
        // it does, in the pages PMAT-1439 corrected. What it may not do is
        // present one without the real spelling nearby.
        if book.contains(name) {
            assert!(
                book.contains(real),
                "the book still names `{name}` but never names `{real}`, so a reader has no way \
                 to reach the real API from it"
            );
        }
    }
}

#[test]
fn the_retagged_tutorial_block_still_names_tests_that_exist() {
    // `python-to-rust.md` carried a ```rust fence that is a list of test names,
    // not Rust. PMAT-1439 retagged it `text`. Retagging must not be a way to
    // leave the class, so the claims it makes are checked here instead: each
    // named test must exist in the file the block names.
    let root = workspace_root();
    let claims = [
        (
            "crates/xpile/tests/readme_quickstart_witness.rs",
            "the_readme_output_compiles_under_rustc_o_and_computes_3628800",
        ),
        (
            "crates/xpile/tests/readme_quickstart_witness.rs",
            "the_readme_overflow_claim_panics_naming_the_contract",
        ),
        (
            "crates/xpile/tests/transpile_e2e.rs",
            "factorial_emitted_rust_computes_correct_values",
        ),
    ];
    let page = std::fs::read_to_string(root.join("book/src/tutorials/python-to-rust.md"))
        .expect("read python-to-rust.md");
    for (file, test) in claims {
        assert!(
            page.contains(test),
            "python-to-rust.md no longer names {test}; this check is pinned to prose that moved"
        );
        let body = std::fs::read_to_string(root.join(file)).unwrap_or_else(|e| {
            panic!("python-to-rust.md names {file}, which cannot be read: {e}")
        });
        assert!(
            body.contains(&format!("fn {test}(")),
            "python-to-rust.md attributes {test} to {file}, which declares no such test"
        );
    }
}

// ---------------------------------------------------------------------------
// PMAT-1444 — the sweep's subject must be a property of the REPOSITORY.
// ---------------------------------------------------------------------------

/// The defect, executed: relocate the checkout and the classification must not
/// move.
///
/// This drives `is_crate_src` directly rather than re-running the suite from
/// several directories, so it is a real differential that costs nothing. The
/// prefixes differ only in whether an ancestor directory is spelled `src`, and
/// the first one is this tree's LIVE root — read at run time, never spelled —
/// so whichever machine reproduced the bug is always among the cases.
#[test]
fn the_source_filter_is_the_same_wherever_the_repository_is_checked_out() {
    // Suffixes of the shape the walk actually produces.
    let rels = [
        ("crates/xpile/src/main.rs", true),
        ("crates/xpile-core/src/lib.rs", true),
        ("crates/xpile/src/cli/audit.rs", true),
        ("crates/xpile/tests/book_rust_example_witness.rs", false),
        ("crates/xpile/benches/emit.rs", false),
        ("crates/xpile/examples/06_inspect_session.rs", false),
    ];

    let prefixes: Vec<PathBuf> = vec![
        workspace_root(),
        // The two real ones, kept literal because they are the two verdicts
        // the header's table records.
        PathBuf::from("/home/noah/src/xpile"),
        PathBuf::from("/home/runner/work/xpile/xpile"),
        // Adversarial: `src` as the last component, twice, and not at all.
        PathBuf::from("/tmp/claude-1000/xpile-wt/src/wt-1444"),
        PathBuf::from("/var/lib/b/src/src/xpile"),
        PathBuf::from("/w"),
    ];

    let mut baseline: Option<Vec<bool>> = None;
    for prefix in &prefixes {
        let verdicts: Vec<bool> = rels
            .iter()
            .map(|(r, _)| is_crate_src(prefix, &prefix.join(r)))
            .collect();

        // Non-vacuity: a predicate answering a CONSTANT satisfies the equality
        // below for free, in either direction.
        assert!(
            verdicts.contains(&true) && verdicts.contains(&false),
            "under `{}` the filter answered a constant {:?} — it is no longer discriminating, \
             so the agreement asserted below would hold for free",
            prefix.display(),
            verdicts
        );

        match &baseline {
            None => baseline = Some(verdicts),
            Some(first) => assert_eq!(
                first,
                &verdicts,
                "the `crates/*/src` filter gives a DIFFERENT answer under `{}` than under \
                 `{}`. It is reading the absolute path, so its verdict is a property of the \
                 machine rather than of the repository: at any checkout with an ancestor \
                 directory named `src` the sweep also reads `tests/`, where this very file \
                 names the fabricated identifiers on purpose — and the gate then reds on its \
                 own evidence. Judge the path RELATIVE to the workspace root.",
                prefix.display(),
                prefixes[0].display()
            ),
        }
    }

    // Consistency is necessary and not sufficient: a predicate that is
    // uniformly WRONG is also uniformly consistent.
    let expected: Vec<bool> = rels.iter().map(|(_, e)| *e).collect();
    assert_eq!(
        baseline.expect("at least one prefix"),
        expected,
        "the filter agrees with itself across checkouts but classifies the wrong files. \
         Expected exactly the `src/` entries of {rels:?}"
    );
}

/// A tripwire for the NEXT path filter, and it is only a tripwire.
///
/// The class is *a predicate over an absolute path's components*, which asks
/// about the machine rather than the tree. When PMAT-1444 measured it, the
/// class had exactly one member — the line above — confirmed two ways: a
/// `components()` sweep of `crates/`, and reading the filter of every
/// directory walker in `crates/*/tests` (all the others key on `file_name()`,
/// which is location-independent).
///
/// Like `no_build_script_builds_an_include_path_out_of_the_manifest_dir`, this
/// READS TEXT and therefore certifies nothing about what a filter computes. It
/// exists to make a new one visible and point its author at
/// `the_source_filter_is_the_same_wherever_the_repository_is_checked_out`,
/// which is where the property is actually measured.
#[test]
fn no_path_component_filter_runs_on_an_unrelativized_path() {
    /// The source, line by line, with comments and string literals removed —
    /// i.e. the part of each line that is CODE. Line numbering is preserved so
    /// a report points at the real line.
    ///
    /// USE vs MENTION, and neither half is hypothetical. The first cut scanned
    /// raw lines and reported five offenders inside this very function, four of
    /// them its own failure message (PMAT-1430 — any "no file may say X"
    /// scanner eventually reads the sentence explaining X; PMAT-1432 — a gate
    /// whose own text perturbs what it measures). The second cut stripped
    /// literals ONE LINE AT A TIME and still reported two, because a `\`
    /// continued literal means a line can BEGIN inside a string: line-local
    /// state cannot decide a question about a multi-line construct. Hence a
    /// single pass over the whole file, carrying string state across newlines.
    fn code_lines(src: &str) -> Vec<String> {
        let c: Vec<char> = src.chars().collect();
        let mut lines: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut i = 0;
        while i < c.len() {
            // A line comment ends this line's code.
            if c[i] == '/' && c.get(i + 1) == Some(&'/') {
                while i < c.len() && c[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            // A raw string `r"…"` / `r#"…"#`: no escapes, explicit terminator.
            if c[i] == 'r' && matches!(c.get(i + 1), Some('"') | Some('#')) {
                let mut j = i + 1;
                let mut hashes = 0;
                while c.get(j) == Some(&'#') {
                    hashes += 1;
                    j += 1;
                }
                if c.get(j) != Some(&'"') {
                    cur.push(c[i]); // an identifier starting with `r`
                    i += 1;
                    continue;
                }
                j += 1;
                let close: Vec<char> = std::iter::once('"')
                    .chain(std::iter::repeat_n('#', hashes))
                    .collect();
                while j < c.len() {
                    if c[j..].starts_with(close.as_slice()) {
                        j += close.len();
                        break;
                    }
                    if c[j] == '\n' {
                        lines.push(std::mem::take(&mut cur));
                    }
                    j += 1;
                }
                i = j;
                continue;
            }
            // A char literal — `'"'` would otherwise open a string. Anything
            // else beginning with `'` is a lifetime.
            if c[i] == '\'' {
                if c.get(i + 1) == Some(&'\\') && c.get(i + 3) == Some(&'\'') {
                    i += 4;
                } else if c.get(i + 2) == Some(&'\'') {
                    i += 3;
                } else {
                    cur.push(c[i]);
                    i += 1;
                }
                continue;
            }
            if c[i] == '"' {
                i += 1;
                while i < c.len() {
                    match c[i] {
                        // An escape, possibly the `\`-newline continuation
                        // that made the previous cut wrong.
                        '\\' => {
                            if c.get(i + 1) == Some(&'\n') {
                                lines.push(std::mem::take(&mut cur));
                            }
                            i += 2;
                        }
                        '"' => {
                            i += 1;
                            break;
                        }
                        '\n' => {
                            lines.push(std::mem::take(&mut cur));
                            i += 1;
                        }
                        _ => i += 1,
                    }
                }
                continue;
            }
            if c[i] == '\n' {
                lines.push(std::mem::take(&mut cur));
                i += 1;
                continue;
            }
            cur.push(c[i]);
            i += 1;
        }
        lines.push(cur);
        lines
    }

    /// Flags `.components()` CALLS with no `strip_prefix` in the preceding
    /// window. Split out so the detector can be driven with the verbatim
    /// pre-fix text below, and not only with a corpus that is now clean.
    fn offending_lines(src: &str) -> Vec<(usize, String)> {
        let code = code_lines(src);
        let mut out = Vec::new();
        for (i, line) in code.iter().enumerate() {
            if !line.contains(".components()") {
                continue;
            }
            let from = i.saturating_sub(6);
            let relativized = code[from..=i].iter().any(|l| l.contains("strip_prefix"));
            if !relativized {
                out.push((i + 1, line.trim().to_string()));
            }
        }
        out
    }

    // NON-VACUITY BY CONSTRUCTION: the detector is shown to discriminate on
    // the real before/after text, embedded here, so it keeps its meaning after
    // the corpus is clean and cannot be softened unnoticed.
    let before = "            } else if p.extension().and_then(|s| s.to_str()) == Some(\"rs\") {\n\
                  \x20               if p.components().any(|c| c.as_os_str() == \"src\") {\n";
    assert_eq!(
        offending_lines(before).len(),
        1,
        "the detector no longer flags PMAT-1444's own pre-fix line; it has been softened past \
         the defect it exists to catch"
    );
    let after = "    let Ok(rel) = path.strip_prefix(root) else {\n\
                 \x20       return false;\n\
                 \x20   };\n\
                 \x20   rel.components().any(|c| c.as_os_str() == \"src\")\n";
    assert!(
        offending_lines(after).is_empty(),
        "the detector flags the RELATIVIZED spelling, so it would red on every correct filter"
    );
    // USE vs MENTION. This is the control the first cut failed: prose and
    // failure messages must be able to QUOTE the shape without being it.
    let mention = "        assert!(x, \"a filter calling .components() on an absolute path\");\n\
                   \x20       // p.components().any(|c| c.as_os_str() == \"src\")\n";
    assert!(
        offending_lines(mention).is_empty(),
        "the detector reads string literals and comments, so it flags any file that DESCRIBES \
         the defect — including this one. It must read code."
    );

    let root = workspace_root();
    let out = std::process::Command::new("git")
        .args(["ls-files", "*.rs"])
        .current_dir(&root)
        .output()
        .expect("git ls-files");
    assert!(out.status.success(), "git ls-files failed");
    let files: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    assert!(
        files.len() > 100,
        "git ls-files '*.rs' returned {} path(s); the enumeration is broken and the scan below \
         would pass over nothing (PMAT-1439: a surprising ZERO is a tooling result until a \
         second method agrees)",
        files.len()
    );

    let mut anchored = false;
    let mut offenders: Vec<String> = Vec::new();
    for rel in &files {
        let Ok(src) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        // Anchored on a CALL, not on the token — otherwise this file's own
        // explanatory prose would keep the anchor alive after every real
        // filter had gone.
        if code_lines(&src).iter().any(|l| l.contains(".components()")) {
            anchored = true;
        }
        for (line, text) in offending_lines(&src) {
            offenders.push(format!("{rel}:{line}: {text}"));
        }
    }

    // A negative over an enumeration passes for free once the enumeration
    // stops containing the construct at all (PMAT-1396).
    assert!(
        anchored,
        "no tracked Rust file calls `.components()` any more — including this file's own \
         `is_crate_src`. The scan is passing over a corpus that stopped saying anything; \
         re-derive whether the class still needs a tripwire."
    );
    assert!(
        offenders.is_empty(),
        "a path filter tests `.components()` without first making the path relative:\n  {}\n\n\
         An absolute path carries every ancestor directory's name, so the predicate answers a \
         question about the machine, not about the repository — see THE SCOPE DEFECT in this \
         file's header, where exactly this shape made a gate RED at `/home/noah/src/xpile` and \
         GREEN in CI on one commit. `strip_prefix` the workspace root first.",
        offenders.join("\n  ")
    );
}
