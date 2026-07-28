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

    let mut src = String::new();
    let crates = root.join("crates");
    fn walk_rs(dir: &Path, out: &mut String) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().and_then(|s| s.to_str()) == Some("target") {
                    continue;
                }
                walk_rs(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                // `src/` only: this file and its sibling NAME the fabricated
                // identifiers on purpose, and a scan that swept tests/ would
                // red on its own evidence.
                if p.components().any(|c| c.as_os_str() == "src") {
                    out.push_str(&std::fs::read_to_string(&p).unwrap_or_default());
                }
            }
        }
    }
    walk_rs(&crates, &mut src);
    assert!(
        src.len() > 1_000_000,
        "the source sweep read only {} bytes — it is not reaching crates/*/src, so every \
         assertion below would pass vacuously",
        src.len()
    );

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
