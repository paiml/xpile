//! XPILE-BUILDGEN-001 (PMAT-1414) — the build script's generated source names
//! no path outside `OUT_DIR`, and the embedded corpus is the corpus of the
//! tree being built.
//!
//! THE DEFECT, measured on this repository rather than imagined. PMAT-1407's
//! `build.rs` emitted, into `$OUT_DIR/embedded_contracts.rs`:
//!
//! ```text
//! ("py-float-arith-v1.yaml", include_str!("/home/…/crates/xpile/contracts/py-float-arith-v1.yaml")),
//! ```
//!
//! — an ABSOLUTE path into whichever tree happened to run the build script.
//! Cargo gives one `OUT_DIR` per (package, profile) inside a target
//! directory, NOT one per source tree, so two checkouts sharing a
//! `CARGO_TARGET_DIR` share that file. This repo's canonical target dir is
//! shared by every `git worktree` of it, which makes the collision the normal
//! case here, not an exotic one.
//!
//! It fails in two directions and the quiet one is worse:
//!
//!   1. SILENTLY WRONG. Building in tree B reuses tree A's generated file, so
//!      the binary embeds tree A's contract bytes while every other part of
//!      the build reads tree B. Reproduced below: the tree-B binary prints
//!      tree A's payload.
//!   2. LOUDLY BROKEN, at a distance. Delete tree A and tree B stops
//!      compiling — `couldn't read /…/wt-1411/crates/xpile/contracts/…: No
//!      such file or directory`, once per contract. Observed as 35 errors on
//!      an UNMODIFIED `main`, naming a worktree that no longer existed and a
//!      developer who was not the one building.
//!
//! WHAT DOES NOT FIX IT, measured: `cargo:rerun-if-env-changed=CARGO_MANIFEST_DIR`.
//! Cargo does not track the variables it injects into the build script, so
//! the script still does not re-run and both failure modes survive verbatim.
//! `differential_a_foreign_out_dir_survives_tree_deletion` below runs that
//! arm and pins it, so the plausible-but-wrong repair cannot be re-adopted by
//! a later reader who assumes it works.
//!
//! WHAT DOES: stage the files into `OUT_DIR` and emit `include_str!` paths
//! relative to the generated file. The generated source then contains no
//! machine-specific path at all — which also makes it reproducible, a
//! property its own comment already claimed before this was true.
//!
//! Test 3 is the one that measures the PROPERTY. It is a real, executed
//! two-tree differential over a minimal cargo package: same shared target
//! dir, absolute-path arm versus staged arm, tree A deleted underneath both.
//! It cannot pass vacuously, because the absolute-path arm must FAIL for the
//! test to pass.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The build script's output, as the compiler sees it.
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded_contracts.rs"));
}

/// The same file as TEXT, so the emitted `include_str!` arguments can be
/// inspected rather than inferred.
const GENERATED_SRC: &str = include_str!(concat!(env!("OUT_DIR"), "/embedded_contracts.rs"));

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every `include_str!("…")` argument in the generated source.
fn emitted_include_paths() -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = GENERATED_SRC;
    while let Some(i) = rest.find("include_str!(\"") {
        rest = &rest[i + "include_str!(\"".len()..];
        let end = rest.find('"').expect("unterminated include_str! literal");
        out.push(rest[..end].to_string());
        rest = &rest[end..];
    }
    out
}

/// 1. No emitted path escapes `OUT_DIR`.
///
/// This is the assertion that is RED on PMAT-1407's build script and GREEN on
/// PMAT-1414's. It keys on the SHAPE (absolute vs relative) rather than on
/// this machine's directory names, so it holds for any tree that builds it.
#[test]
fn generated_source_names_no_path_outside_out_dir() {
    let paths = emitted_include_paths();

    // Non-vacuity: an empty generated file would satisfy every `for` below.
    // build.rs already refuses an empty corpus; assert it here too, because
    // this test's whole subject is the generated file being wrong.
    assert!(
        paths.len() >= 2,
        "the generated file emits {} include_str! path(s) — too few for this gate to \
         mean anything. Every assertion below passes vacuously on an empty corpus.",
        paths.len()
    );

    for p in &paths {
        assert!(
            !p.starts_with('/') && !p.starts_with('\\'),
            "the build script emitted an ABSOLUTE include path `{p}`. Cargo shares one \
             OUT_DIR per package across every source tree using the same target \
             directory, so this file is compiled by trees that never wrote it: the \
             embedded bytes become another tree's, and deleting that tree breaks this \
             one's build outright. Stage the file into OUT_DIR and emit a relative path."
        );
        assert!(
            !p.contains(".."),
            "the build script emitted `{p}`, which climbs out of OUT_DIR. The generated \
             file must be self-contained — see the absolute-path failure above."
        );
        // A Windows-style `C:\…` would pass both checks above.
        assert!(
            !(p.len() >= 2 && p.as_bytes()[1] == b':'),
            "the build script emitted what looks like an absolute Windows path `{p}`"
        );
    }

    // The sharpest form of the same property on THIS tree: the generated
    // source must not mention the manifest directory anywhere, in any
    // spelling — not in an include, not in a comment.
    let md = crate_root().display().to_string();
    assert!(
        !GENERATED_SRC.contains(&md),
        "the generated file mentions this tree's manifest directory ({md}). Whatever \
         emitted it baked a machine-specific path into a file that other trees compile."
    );
}

/// 2. The embedded corpus is THIS tree's corpus, byte for byte.
///
/// `packaged_contracts.rs` test 4 compares two `xpile diamond` REPORTS, which
/// catches a stale corpus only when the staleness reaches a reported column.
/// This compares the bytes, so a contract that drifted in a field no reporter
/// prints still fails.
#[test]
fn embedded_corpus_is_byte_identical_to_the_on_disk_corpus() {
    let dir = crate_root().join("contracts");
    let mut on_disk: Vec<(String, Vec<u8>)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("yaml"))
        .map(|p| {
            let name = p
                .file_name()
                .and_then(|s| s.to_str())
                .expect("contract file name")
                .to_string();
            let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
            (name, bytes)
        })
        .collect();
    on_disk.sort_by(|a, b| a.0.cmp(&b.0));

    // Non-vacuity, both halves: an empty corpus would make the comparison
    // trivially true, and so would a corpus of empty files.
    assert!(
        on_disk.len() >= 2,
        "only {} contract YAML(s) on disk — this comparison would be vacuous",
        on_disk.len()
    );
    assert!(
        on_disk.iter().all(|(_, b)| !b.is_empty()),
        "at least one on-disk contract is empty; the byte comparison would not \
         distinguish an embedded copy from nothing"
    );

    let embedded_names: Vec<&str> = embedded::EMBEDDED_CONTRACTS
        .iter()
        .map(|(n, _)| *n)
        .collect();
    let disk_names: Vec<&str> = on_disk.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        embedded_names, disk_names,
        "the EMBEDDED contract set differs from the on-disk one. The usual cause is a \
         build script that did not re-run — cargo shares OUT_DIR across source trees \
         using the same target directory, so this binary may be carrying another \
         checkout's corpus. `touch crates/xpile/build.rs` and rebuild to confirm."
    );

    for ((name, embedded_text), (_, disk_bytes)) in
        embedded::EMBEDDED_CONTRACTS.iter().zip(on_disk.iter())
    {
        assert_eq!(
            embedded_text.as_bytes(),
            disk_bytes.as_slice(),
            "embedded `{name}` differs from the on-disk file. The binary would report on \
             a contract corpus that no longer exists in this tree."
        );
    }
}

// ---------------------------------------------------------------------------
// 3. The executed differential.
// ---------------------------------------------------------------------------

static PROBE_SEQ: AtomicUsize = AtomicUsize::new(0);

/// A private scratch directory unique per CALL, not per test — two calls in
/// one test must not share one.
fn scratch(tag: &str) -> PathBuf {
    let n = PROBE_SEQ.fetch_add(1, Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!(
        "xpile-buildgen-1414-{}-{}-{n}",
        std::process::id(),
        tag
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap_or_else(|e| panic!("create {}: {e}", d.display()));
    d
}

/// Lay down a minimal package whose build script embeds `data/x.txt` using
/// `build_rs_body`, with `payload` as the file's contents.
fn write_probe_tree(root: &Path, payload: &str, build_rs_body: &str) {
    std::fs::create_dir_all(root.join("src")).expect("mkdir src");
    std::fs::create_dir_all(root.join("data")).expect("mkdir data");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"xpile-buildgen-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
         \n[workspace]\n",
    )
    .expect("write Cargo.toml");
    std::fs::write(root.join("data/x.txt"), payload).expect("write payload");
    std::fs::write(root.join("build.rs"), build_rs_body).expect("write build.rs");
    std::fs::write(
        root.join("src/main.rs"),
        "include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\nfn main() { print!(\"{D}\"); }\n",
    )
    .expect("write main.rs");
}

/// PMAT-1407's scheme: bake the absolute source path into the generated file.
/// Carries the `rerun-if-env-changed=CARGO_MANIFEST_DIR` line that looks like
/// it should rescue this and does not.
const ABSOLUTE_PATH_BUILD_RS: &str = r#"
use std::path::Path;
fn main() {
    let md = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out = std::env::var("OUT_DIR").unwrap();
    println!("cargo:rerun-if-changed=data");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_MANIFEST_DIR");
    let abs = Path::new(&md).join("data/x.txt").display().to_string();
    std::fs::write(
        Path::new(&out).join("gen.rs"),
        format!("pub static D: &str = include_str!({:?});\n", abs),
    )
    .unwrap();
}
"#;

/// PMAT-1414's scheme: stage into OUT_DIR, emit a relative path.
const STAGED_BUILD_RS: &str = r#"
use std::path::Path;
fn main() {
    let md = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out = std::env::var("OUT_DIR").unwrap();
    println!("cargo:rerun-if-changed=data");
    println!("cargo:rerun-if-changed=build.rs");
    let staged = Path::new(&out).join("staged");
    let _ = std::fs::remove_dir_all(&staged);
    std::fs::create_dir_all(&staged).unwrap();
    std::fs::copy(Path::new(&md).join("data/x.txt"), staged.join("x.txt")).unwrap();
    std::fs::write(
        Path::new(&out).join("gen.rs"),
        "pub static D: &str = include_str!(\"staged/x.txt\");\n",
    )
    .unwrap();
}
"#;

fn cargo_build(dir: &Path, target_dir: &Path) -> std::process::Output {
    Command::new(env!("CARGO"))
        .arg("build")
        .arg("--offline")
        .current_dir(dir)
        // The probe must not inherit this workspace's target directory —
        // that is the very sharing this test is about.
        .env("CARGO_TARGET_DIR", target_dir)
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap_or_else(|e| panic!("spawn cargo in {}: {e}", dir.display()))
}

/// 3. Two trees, one shared target dir, tree A deleted: the absolute-path
///    scheme stops building and the staged scheme does not.
///
/// This is the property PMAT-1414 buys, executed end to end. The
/// absolute-path arm asserting FAILURE is what keeps the staged arm's success
/// from being meaningless.
#[test]
fn differential_a_foreign_out_dir_survives_tree_deletion() {
    for (label, build_rs, expect_survives) in [
        ("absolute", ABSOLUTE_PATH_BUILD_RS, false),
        ("staged", STAGED_BUILD_RS, true),
    ] {
        let base = scratch(label);
        let (a, b, target) = (base.join("A"), base.join("B"), base.join("target"));
        write_probe_tree(&a, "payload-from-A", build_rs);
        write_probe_tree(&b, "payload-from-B", build_rs);

        let built_a = cargo_build(&a, &target);
        assert!(
            built_a.status.success(),
            "[{label}] the probe tree must build before anything is measured. stderr:\n{}",
            String::from_utf8_lossy(&built_a.stderr)
        );
        let built_b = cargo_build(&b, &target);
        assert!(
            built_b.status.success(),
            "[{label}] tree B must build while tree A still exists. stderr:\n{}",
            String::from_utf8_lossy(&built_b.stderr)
        );

        // Tree A goes away — a merged branch's worktree being pruned.
        std::fs::remove_dir_all(&a).expect("remove tree A");
        // Force a rebuild of the crate; without an input change cargo would
        // relink nothing and the difference would never be exercised.
        std::fs::write(
            b.join("src/main.rs"),
            "include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\n\
             fn main() { print!(\"{D}\"); }\n// touched\n",
        )
        .expect("touch main.rs");
        let after = cargo_build(&b, &target);
        let stderr = String::from_utf8_lossy(&after.stderr);

        if expect_survives {
            assert!(
                after.status.success(),
                "[{label}] staging into OUT_DIR must leave tree B buildable after tree A \
                 is deleted — that is the entire repair. stderr:\n{stderr}"
            );
        } else {
            assert!(
                !after.status.success(),
                "[{label}] the absolute-path scheme was expected to FAIL once the tree \
                 that wrote the generated file was deleted. It did not, so this \
                 differential no longer distinguishes the two schemes and the staged \
                 arm below proves nothing. Re-derive the reproduction before trusting \
                 XPILE-BUILDGEN-001."
            );
            assert!(
                stderr.contains("couldn't read"),
                "[{label}] expected the `couldn't read <deleted tree>` failure that this \
                 gate exists to prevent; got:\n{stderr}"
            );
        }

        let _ = std::fs::remove_dir_all(&base);
    }
}

/// 4. A tripwire for the NEXT build script, and it is only a tripwire.
///
/// Tests 1–3 inspect the generated OUTPUT, which is the real evidence — but
/// they can only inspect the output of the build script that exists. Measured
/// while fixing this: `git ls-files '*build.rs'` returns exactly one file, so
/// the class currently has one member. A second one could reintroduce the
/// shape in a file nothing here compiles.
///
/// This test text-matches, and text-matching certifies nothing about what the
/// script actually emits. It exists to make a new build script *visible* — to
/// fail loudly and point the author at the property — not to prove it
/// correct. A new build script should get its own output-level assertions.
#[test]
fn no_build_script_builds_an_include_path_out_of_the_manifest_dir() {
    let root = crate_root()
        .join("../..")
        .canonicalize()
        .expect("workspace root");
    let out = Command::new("git")
        .args(["ls-files", "*build.rs"])
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

    // A negative over an enumeration passes for free on an EMPTY enumeration
    // (PMAT-1396). This repo has a build script; if the enumeration is empty
    // the query broke, not the code.
    assert!(
        !files.is_empty(),
        "git ls-files '*build.rs' found nothing. This workspace has a build script \
         (crates/xpile/build.rs), so the enumeration is broken and every assertion \
         below would pass vacuously."
    );

    for rel in &files {
        let src =
            std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        // The shape: joining CARGO_MANIFEST_DIR onto a path and handing the
        // result to the generated source. Keyed on the two together, since
        // reading CARGO_MANIFEST_DIR is entirely normal on its own.
        let reads_manifest_dir = src.contains("CARGO_MANIFEST_DIR");
        let emits_include = src.contains("include_str!") || src.contains("include_bytes!");
        let stages_into_out_dir = src.contains("OUT_DIR") && src.contains("std::fs::copy");
        assert!(
            !(reads_manifest_dir && emits_include && !stages_into_out_dir),
            "{rel} appears to emit `include_str!`/`include_bytes!` paths derived from \
             CARGO_MANIFEST_DIR without staging the files into OUT_DIR. Cargo shares one \
             OUT_DIR per package across every source tree using the same target directory, \
             so an absolute path there is a cross-tree channel: another checkout compiles \
             this file, embeds THIS tree's bytes at exit 0, and stops building entirely \
             once this tree is deleted. Stage the inputs into OUT_DIR and emit relative \
             paths — see crates/xpile/build.rs. Then give the new script its own \
             output-level assertions; this check only reads text."
        );
    }
}
