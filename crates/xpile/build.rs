//! Embed the contract corpus into the `xpile` binary (PMAT-1407).
//!
//! WHY THIS EXISTS. `xpile diamond` / `quorum` / `attestations` resolve
//! `--contracts-dir` (default `contracts`) RELATIVE TO THE PROCESS CWD. That
//! works from a source checkout and fails for every user who ran the README's
//! own `cargo install xpile`: measured on the published 0.1.617 crate, all
//! three exit 1 with `Error: contracts is not a directory` from any directory
//! that is not an xpile checkout.
//!
//! PACKAGING ALONE DOES NOT FIX IT, and that is the whole subtlety. Shipping
//! the YAMLs inside the `.crate` only puts them where the COMPILER can see
//! them; `cargo install` copies the binary to `~/.cargo/bin` and discards the
//! build directory, so a runtime file read still has nothing to open. The
//! files have to be in the BINARY. Hence `include_str!`, which needs them to
//! be reachable from the package root at build time — supplied by the
//! `crates/xpile/contracts -> ../../contracts` symlink that `cargo package`
//! dereferences into real file content.
//!
//! NO HAND-MAINTAINED LIST. The corpus is enumerated from the directory on
//! every build, so adding a contract needs no edit here and the embedded set
//! cannot silently diverge from the canonical one. `crates/xpile/tests/
//! packaged_contracts.rs` (XPILE-PACKAGE-001) re-derives both sides and fails
//! if they ever do.
//!
//! THE GENERATED FILE NAMES NO PATH OUTSIDE `OUT_DIR` (PMAT-1414), and that
//! is load-bearing rather than tidy. The first cut emitted
//! `include_str!("<CARGO_MANIFEST_DIR>/contracts/<name>.yaml")` — an ABSOLUTE
//! path into the tree that happened to run the build script. Two checkouts
//! that share a `CARGO_TARGET_DIR` share this package's `OUT_DIR`, so the
//! generated file one tree writes is the one the OTHER tree compiles.
//! Measured on a minimal two-tree reproduction, that produced both failure
//! modes: building in tree B silently embedded tree A's bytes, and once tree
//! A was deleted, tree B stopped building at all — 35 `couldn't read`
//! errors naming a directory the developer had never heard of. That is not
//! hypothetical here: the repo's canonical target dir is shared by every
//! `git worktree`, and it took `cargo build` on an UNMODIFIED `main` down.
//!
//! So the contracts are STAGED INTO `OUT_DIR` and the emitted `include_str!`
//! arguments are relative to the generated file. The generated source is then
//! byte-identical no matter who built it or where.
//!
//! WHAT DOES NOT WORK, measured rather than assumed:
//! `cargo:rerun-if-env-changed=CARGO_MANIFEST_DIR` does NOT re-run the build
//! script when the manifest directory changes — cargo does not track the
//! variables it injects itself. Staging removes the hard build failure; the
//! residual stale-content risk is caught loudly by
//! `crates/xpile/tests/build_script_path_independence.rs`
//! (XPILE-BUILDGEN-001), which compares the embedded bytes against the
//! on-disk corpus of the tree being tested.

use std::path::Path;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let contracts_dir = Path::new(&manifest_dir).join("contracts");

    // Re-run when a contract is added, removed or edited. The directory entry
    // covers add/remove; cargo also watches the files it names.
    println!("cargo:rerun-if-changed=contracts");
    println!("cargo:rerun-if-changed=build.rs");

    // Staging directory inside OUT_DIR. Cleared first: a contract DELETED
    // from the corpus must not survive as an orphaned copy that a later
    // `include_str!` could still resolve.
    let staged = Path::new(&out_dir).join("contracts");
    let _ = std::fs::remove_dir_all(&staged);
    std::fs::create_dir_all(&staged).unwrap_or_else(|e| panic!("create {}: {e}", staged.display()));

    let mut entries: Vec<(String, String)> = Vec::new();
    if contracts_dir.is_dir() {
        let mut names: Vec<String> = std::fs::read_dir(&contracts_dir)
            .unwrap_or_else(|e| panic!("read {}: {e}", contracts_dir.display()))
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("yaml"))
            .filter_map(|p| p.file_name().and_then(|s| s.to_str()).map(String::from))
            .collect();
        // Deterministic order: the generated file is an input to a
        // reproducible build, and the reporters print corpus-ordered rows.
        names.sort();
        for name in names {
            println!("cargo:rerun-if-changed=contracts/{name}");
            std::fs::copy(contracts_dir.join(&name), staged.join(&name))
                .unwrap_or_else(|e| panic!("stage {} into {}: {e}", name, staged.display()));
            // Relative to the GENERATED FILE, which lives in OUT_DIR beside
            // this staging directory. Never an absolute path — see the module
            // comment for what an absolute one costs.
            let rel = format!("contracts/{name}");
            entries.push((name, rel));
        }
    }

    // A build that embeds NOTHING would hand the reporters an empty corpus,
    // which they would report as "no contract IDs discovered" — a truthful
    // message about the wrong cause. Fail the build instead: an xpile source
    // tree and an unpacked .crate both have the directory, so an empty corpus
    // means the symlink or the packaging regressed.
    assert!(
        !entries.is_empty(),
        "no *.yaml found under {} — the `crates/xpile/contracts -> ../../contracts` \
         symlink is missing, or `cargo package` stopped dereferencing it. The binary \
         would ship with an empty embedded contract corpus and `xpile diamond` would \
         fail for every installed user.",
        contracts_dir.display()
    );

    let mut src = String::new();
    src.push_str(
        "// @generated by crates/xpile/build.rs — do not edit.\n\
         /// The contract corpus embedded at build time, as `(file name, contents)`\n\
         /// pairs sorted by file name. Read when `--contracts-dir` is left at its\n\
         /// default and no `contracts/` directory exists beside the process CWD.\n\
         pub static EMBEDDED_CONTRACTS: &[(&str, &str)] = &[\n",
    );
    for (name, rel) in &entries {
        src.push_str(&format!(
            "    ({:?}, include_str!({:?})),\n",
            name.as_str(),
            rel.as_str()
        ));
    }
    src.push_str("];\n");

    let dest = Path::new(&out_dir).join("embedded_contracts.rs");
    std::fs::write(&dest, src).unwrap_or_else(|e| panic!("write {}: {e}", dest.display()));
}
