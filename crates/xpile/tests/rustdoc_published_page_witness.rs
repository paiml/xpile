//! XPILE-RUSTDOC-001 — the pages this project publishes to docs.rs must build
//! without rustdoc warnings (PMAT-1513).
//!
//! ## What was wrong
//!
//! Nothing in this repository ever ran `cargo doc`. No CI job invoked it and no
//! test did, so rustdoc warnings accumulated untouched on the **published**
//! artifact: measured at `04960561`, **58 defects across 13 of the 30 crates
//! that have a docs.rs page**, in three kinds —
//!
//! | kind | count |
//! |---|---|
//! | public documentation links to a private item | 38 |
//! | unresolved link | 18 |
//! | redundant explicit link target | 2 |
//!
//! The dominant kind is the visible one. `[`refuse_ieee_div`]` in a public doc
//! comment does not become a link when the target is private — docs.rs renders
//! the brackets literally, so `xpile-wasm-frontend`'s page told readers to
//! *"See [refuse_ieee_div]"* with nothing to follow, and it said it repeatedly.
//! A reader cannot tell that from the source, only from the rendered page, and
//! nobody was rendering the page.
//!
//! Repaired by dropping the brackets and keeping the code formatting — the
//! sentence still names the item, and it no longer promises a link that cannot
//! exist. Separately, `crates/xpile/src/main.rs` carried nine `unclosed HTML
//! tag` warnings because a CLI synopsis wrote `<path>` outside a code fence,
//! and rustdoc read the metavariables as HTML; that file has no docs.rs page,
//! but fixing it is what lets `-D warnings` be turned on for the whole
//! workspace rather than for a subset nobody can remember.
//!
//! ## Why this gate runs the build instead of reading the workflow
//!
//! Pinning "the CI job contains a `--` flag" checks the MECHANISM, not the
//! PROPERTY — PMAT-1500's clean-room tripwire passed for a year while matching
//! a spelling nobody wrote. This test performs the build and asserts on its
//! output, so it is true or false about the artifact regardless of how CI is
//! configured.
//!
//! ⚠️ It builds into its OWN target directory. `cargo doc` on the same
//! workspace from inside `cargo test` contends on the target-dir lock and
//! deadlocks; a separate `CARGO_TARGET_DIR` is what makes this runnable at all.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/xpile → repo root")
        .to_path_buf()
}

/// Build the workspace docs and return every rustdoc diagnostic line.
///
/// `Err` means the build could not be run at all, which is reported rather than
/// treated as a pass.
fn rustdoc_warnings() -> Result<Vec<String>, String> {
    let root = repo_root();
    let target = std::env::temp_dir().join("xpile-rustdoc-witness-target");
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["doc", "--workspace", "--no-deps"])
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", &target)
        .env("RUSTDOCFLAGS", "-W warnings")
        .output()
        .map_err(|e| format!("spawn cargo doc: {e}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() && !text.contains("warning") {
        return Err(format!("cargo doc failed to run:\n{text}"));
    }
    Ok(text
        .lines()
        .filter(|l| l.starts_with("warning:"))
        .map(|l| l.to_string())
        .collect())
}

/// The published surface: crates that get a docs.rs page. A binary target does
/// not, so its warnings are real but not reader-visible — they are held to the
/// same standard here anyway, because a split standard is one nobody applies.
#[test]
fn the_published_pages_build_without_rustdoc_warnings() {
    let warnings = match rustdoc_warnings() {
        Ok(w) => w,
        Err(e) => panic!("{e}"),
    };
    assert!(
        warnings.is_empty(),
        "rustdoc emitted {} warning(s). These land on the PUBLISHED docs.rs page, where a \
         link to a private item renders as literal brackets — a reader sees \
         \"See [foo]\" with nothing to follow:\n{}",
        warnings.len(),
        warnings.join("\n")
    );
}

/// ANTI-VACUITY. If the build stopped running — a renamed flag, a cargo that
/// cannot be found, a workspace that no longer resolves — the property above
/// would report an empty warning list and pass forever. This asserts the build
/// actually produced documentation.
#[test]
fn the_doc_build_actually_ran() {
    let target = std::env::temp_dir().join("xpile-rustdoc-witness-target");
    // Force the build (the other test may not have run yet under --test-threads).
    let _ = rustdoc_warnings().expect("cargo doc must be runnable");
    let index = target.join("doc").join("xpile_core").join("index.html");
    assert!(
        index.is_file(),
        "no rendered page at {} — `cargo doc` reported no warnings because it did not \
         produce documentation, not because the documentation is clean",
        index.display()
    );
}
