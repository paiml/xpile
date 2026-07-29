//! XPILE-FALSIFY-CMD-001 (PMAT-1459) — every published falsification command
//! must be able to run.
//!
//! Each `contracts/*.yaml` carries a `falsification_tests:` list, and each
//! entry publishes a `test:` field: *the command that falsifies this rule*.
//! It is the substrate's only statement of how a rule is checked, and
//! `xpile-contract-backend`'s `include_falsification` knob renders it into the
//! emitted contract docs for readers.
//!
//! Nothing ever checked that those commands run. On 2026-07-29 the substrate
//! held 138 entries and **70 of them named a command that cannot falsify
//! anything**:
//!
//!   * 67 named `cargo test -p <pkg> --test <target>` for a target that does
//!     not exist — and no `#[test] fn <target>` existed anywhere among the
//!     ~2 900 in the tree either, so the names were never real. Running one
//!     gives `error: no test target named `idempotency` in `xpile-backend``.
//!   * 3 resolved to a real target but named a filter matching no test, so
//!     they exited **0** with `running 0 tests ... 828 filtered out` — the
//!     worse half, because a green that compared nothing reads as a pass.
//!
//! 123 of the 138 declared `ship_blocking: true`.
//!
//! ## What this gate decides
//!
//! Runnability, not adequacy. Whether a test *actually falsifies* its rule is
//! a judgement no gate can make; whether the published command *resolves to at
//! least one test* is arithmetic over the live workspace, and that is the half
//! that was silently false. An entry passes iff either
//!
//!   1. every command line in `test:` resolves against the live tree and
//!      selects at least one `#[test]`, or
//!   2. `test:` is an explicit [`HOLE`] disclosure — which must cite a PMAT id
//!      and must not carry a command line, so a disclosure cannot smuggle a
//!      fabricated command back in.
//!
//! Rule (2) is why this gate cannot be satisfied by writing prose: the
//! disclosure is machine-checked to be a disclosure and nothing else.
//!
//! ## Blind spots, established separately
//!
//! A gate has two of them — its SUBJECT (which entries it reads) and its
//! NEEDLE (what it recognises as runnable) — and each is pinned by its own
//! test here:
//!
//!   * [`the_subject_covers_every_contract_that_publishes_falsifiers`] — every
//!     contract file with a `falsification_tests:` key yields at least one
//!     entry, so a parse regression on one file cannot shrink the corpus
//!     silently.
//!   * [`the_needle_finds_a_live_runnable_command`] — a real live entry is
//!     classified [`Verdict::Runs`], so "everything passes" is not "the
//!     resolver matches nothing".
//!   * [`the_needle_reports_each_way_a_command_can_fail`] — six perturbations
//!     (dead package, dead target, empty filter, package with no tests at all,
//!     a disclosure smuggling a command, an undated disclosure) are each
//!     reported, so no arm of the resolver is unreachable — every one against a
//!     control that PASSES.
//!   * [`the_needle_counts_only_a_real_test_attribute`] — a constructed pin for
//!     the one guard here that is a forward tripwire rather than live-load-
//!     bearing; see [`test_fn_names`], which says so and gives the measurement.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// First words of a `test:` block that declares, in the open, that no runnable
/// falsifier is named for the rule.
const HOLE: &str = "NO RUNNABLE FALSIFIER";

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p
}

/// One published falsification entry.
#[derive(Debug, Clone)]
struct Entry {
    file: String,
    id: String,
    test: String,
}

/// Why a `test:` field is or is not runnable. `Broken` carries the reason so
/// the failure diagnostic names the defect rather than the count.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    Runs,
    DeclaredHole,
    Broken(String),
}

/// Every `crates/*/Cargo.toml` package name → its directory. Derived from the
/// live tree, so a renamed or deleted crate reds the entries that name it.
fn workspace_packages(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(root.join("crates")) else {
        return out;
    };
    for e in rd.flatten() {
        let manifest = e.path().join("Cargo.toml");
        let Ok(text) = fs::read_to_string(&manifest) else {
            continue;
        };
        for line in text.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("name") {
                let rest = rest.trim_start();
                if let Some(rest) = rest.strip_prefix('=') {
                    let name = rest.trim().trim_matches('"').to_string();
                    if !name.is_empty() {
                        out.push((name, e.path()));
                    }
                }
                break;
            }
        }
    }
    out.sort();
    out
}

/// Names of the `#[test]` functions declared in `src`.
///
/// The `#[test]` line must be exactly that after trimming, and the `fn` must
/// follow within a few lines — so `#[test]` written inside a doc comment, or
/// inside an emitted-code string literal, is not counted.
///
/// MEASURED, not claimed load-bearing: 36 lines in the workspace contain
/// `#[test]` without being it (3 of them are `"#[test]\n\"` inside
/// `crates/xpile/src/main.rs`'s emitted Rust), yet relaxing this to a
/// `contains` test changes the verdict on **0 of the 138** live commands. It is
/// a FORWARD TRIPWIRE against a filter matching harvested prose, pinned by the
/// constructed case in [`the_needle_counts_only_a_real_test_attribute`] rather
/// than by anything live — said outright instead of implied.
fn test_fn_names(src: &str) -> Vec<String> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim() != "#[test]" {
            continue;
        }
        for probe in lines.iter().take((i + 6).min(lines.len())).skip(i + 1) {
            let s = probe.trim();
            let after = ["pub async fn ", "async fn ", "pub fn ", "fn "]
                .iter()
                .find_map(|p| s.strip_prefix(p));
            if let Some(after) = after {
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    out.push(name);
                }
                break;
            }
        }
    }
    out
}

/// Every `.rs` file under `dir`, recursively.
fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

fn test_fns_in(paths: &[PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    for p in paths {
        if let Ok(src) = fs::read_to_string(p) {
            out.extend(test_fn_names(&src));
        }
    }
    out
}

/// A parsed `cargo test` invocation: the bits that decide what will run.
struct Invocation {
    package: Option<String>,
    target: Option<String>,
    lib: bool,
    filters: Vec<String>,
}

/// Split a `cargo test …` line. Everything after a bare `--` is harness
/// arguments (`-- --ignored`), not a filter.
fn parse_cargo_test(line: &str) -> Invocation {
    let toks: Vec<&str> = line.split_whitespace().collect();
    let mut inv = Invocation {
        package: None,
        target: None,
        lib: false,
        filters: Vec::new(),
    };
    let mut i = 2; // skip `cargo test`
    while i < toks.len() {
        match toks[i] {
            "-p" | "--package" if i + 1 < toks.len() => {
                inv.package = Some(toks[i + 1].to_string());
                i += 2;
            }
            "--test" if i + 1 < toks.len() => {
                inv.target = Some(toks[i + 1].to_string());
                i += 2;
            }
            "--lib" => {
                inv.lib = true;
                i += 1;
            }
            "--" => break,
            t if t.starts_with('-') || t.starts_with('#') => i += 1,
            t => {
                inv.filters.push(t.to_string());
                i += 1;
            }
        }
    }
    inv
}

/// Resolve one command line against the live tree.
///
/// `Ok(())` means the line selects at least one `#[test]`. The `Err` string
/// names the failure the way cargo would.
fn resolve_line(line: &str, root: &Path) -> Result<(), String> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix("cd ") {
        // `cd <dir> && <cmd>` — the only non-cargo shape the corpus uses. The
        // directory has to exist or the command cannot start.
        let dir = rest.split("&&").next().unwrap_or("").trim();
        return if root.join(dir).is_dir() {
            Ok(())
        } else {
            Err(format!("`cd {dir}` — no such directory in the workspace"))
        };
    }
    if !line.starts_with("cargo test") {
        return Err(format!("unrecognised command shape: `{line}`"));
    }
    let inv = parse_cargo_test(line);
    let Some(pkg) = inv.package.clone() else {
        return Err(format!("`{line}` names no package (-p)"));
    };
    let pkgs = workspace_packages(root);
    let Some((_, dir)) = pkgs.iter().find(|(n, _)| *n == pkg) else {
        return Err(format!("package `{pkg}` is not a workspace member"));
    };

    let mut sources: Vec<PathBuf> = Vec::new();
    if let Some(target) = inv.target.clone() {
        let file = dir.join("tests").join(format!("{target}.rs"));
        let dir_main = dir.join("tests").join(&target).join("main.rs");
        if file.is_file() {
            sources.push(file);
        } else if dir_main.is_file() {
            sources.push(dir_main);
        } else {
            return Err(format!(
                "no test target named `{target}` in `{pkg}` — \
                 crates/{}/tests/{target}.rs does not exist",
                dir.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
    } else if inv.lib {
        rs_files(&dir.join("src"), &mut sources);
    } else {
        rs_files(&dir.join("src"), &mut sources);
        rs_files(&dir.join("tests"), &mut sources);
    }

    let names = test_fns_in(&sources);
    if names.is_empty() {
        return Err(format!(
            "`{line}` selects no `#[test]` at all — it would exit 0 having run nothing"
        ));
    }
    for f in &inv.filters {
        if !names.iter().any(|n| n.contains(f.as_str())) {
            return Err(format!(
                "filter `{f}` matches none of the {} tests it would run — \
                 `{line}` exits 0 with `running 0 tests`",
                names.len()
            ));
        }
    }
    Ok(())
}

/// Classify one entry's whole `test:` block.
fn verdict(entry: &Entry, root: &Path) -> Verdict {
    let body = entry.test.trim();
    if body.starts_with(HOLE) {
        if !body.contains("PMAT-") {
            return Verdict::Broken(format!(
                "declares `{HOLE}` but cites no PMAT id — an undated hole cannot be tracked"
            ));
        }
        for l in body.lines() {
            let t = l.trim();
            if t.starts_with("cargo ") || t.starts_with("cd ") {
                return Verdict::Broken(format!(
                    "declares `{HOLE}` but line `{t}` reads as a command — \
                     keep a retired command inline in backticks, never at line start"
                ));
            }
        }
        return Verdict::DeclaredHole;
    }
    let mut ran = 0usize;
    for l in body.lines() {
        let t = l.trim();
        // A continuation of the previous command (`-- --ignored`) or a bare
        // comment carries no target of its own.
        if t.is_empty() || t.starts_with("--") || t.starts_with('#') {
            continue;
        }
        match resolve_line(t, root) {
            Ok(()) => ran += 1,
            Err(e) => return Verdict::Broken(e),
        }
    }
    if ran == 0 {
        return Verdict::Broken("names no command at all".to_string());
    }
    Verdict::Runs
}

/// Every `contracts/*.yaml` path, sorted.
fn contract_files(root: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(root.join("contracts"))
        .expect("contracts/ is readable")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "yaml"))
        .collect();
    out.sort();
    out
}

fn entries(root: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    for path in contract_files(root) {
        let text = fs::read_to_string(&path).expect("contract is readable");
        let doc: serde_yaml::Value = serde_yaml::from_str(&text)
            .unwrap_or_else(|e| panic!("{} does not parse as YAML: {e}", path.display()));
        let Some(list) = doc.get("falsification_tests").and_then(|v| v.as_sequence()) else {
            continue;
        };
        let file = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        for ft in list {
            let id = ft
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("<no id>")
                .to_string();
            let test = ft
                .get("test")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            out.push(Entry {
                file: file.clone(),
                id,
                test,
            });
        }
    }
    out
}

/// THE GATE. Every published falsification command runs, or says it does not.
#[test]
fn every_published_falsification_command_can_run() {
    let root = workspace_root();
    let all = entries(&root);
    assert!(
        !all.is_empty(),
        "no falsification entry was parsed — the gate is ranging over nothing"
    );

    let mut broken: Vec<String> = Vec::new();
    let (mut runs, mut holes) = (0usize, 0usize);
    for e in &all {
        match verdict(e, &root) {
            Verdict::Runs => runs += 1,
            Verdict::DeclaredHole => holes += 1,
            Verdict::Broken(why) => broken.push(format!("  {} {}: {why}", e.file, e.id)),
        }
    }

    assert!(
        broken.is_empty(),
        "{} of {} published falsification commands cannot run. A `test:` field \
         is the substrate's only statement of how a rule is checked; one that \
         hard-errors at target resolution, or that exits 0 having selected no \
         test, checks nothing. Either point it at a real test or declare the \
         hole with a `{HOLE}` disclosure citing a PMAT id.\n{}\n\
         (live: {runs} runnable, {holes} declared holes)",
        broken.len(),
        all.len(),
        broken.join("\n")
    );

    // Not a floor and not a census — a runnable entry has to EXIST, or a
    // corpus of nothing but disclosures would pass this gate in silence.
    assert!(
        runs > 0,
        "every falsification entry is a declared hole — the substrate publishes \
         no runnable falsifier at all"
    );
}

/// SUBJECT. Every contract that publishes a `falsification_tests:` key
/// contributes at least one entry, so a parse regression on a single file
/// cannot shrink what the gate reads without failing here.
#[test]
fn the_subject_covers_every_contract_that_publishes_falsifiers() {
    let root = workspace_root();
    let seen: BTreeSet<String> = entries(&root).into_iter().map(|e| e.file).collect();
    let mut publishes = Vec::new();
    for path in contract_files(&root) {
        let text = fs::read_to_string(&path).expect("contract is readable");
        if text.lines().any(|l| l.trim_end() == "falsification_tests:") {
            publishes.push(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            );
        }
    }
    assert!(
        !publishes.is_empty(),
        "no contract declares falsification_tests: — the subject probe itself is vacuous"
    );
    let missed: Vec<&String> = publishes.iter().filter(|f| !seen.contains(*f)).collect();
    assert!(
        missed.is_empty(),
        "these contracts publish falsification_tests: but yielded no entry to the \
         gate — its subject is narrower than the corpus: {missed:?}"
    );
}

/// NEEDLE, positive half. A real live entry classifies as [`Verdict::Runs`] —
/// otherwise a green gate would only prove the resolver never matches.
#[test]
fn the_needle_finds_a_live_runnable_command() {
    let root = workspace_root();
    let runnable: Vec<&Entry> = {
        let all: &'static Vec<Entry> = Box::leak(Box::new(entries(&root)));
        all.iter()
            .filter(|e| verdict(e, &root) == Verdict::Runs)
            .collect()
    };
    assert!(
        !runnable.is_empty(),
        "no live entry resolves to a runnable command, so the PASS arm of the \
         resolver is untested"
    );
    // And the resolver agrees with cargo on a command this repo really runs.
    assert_eq!(
        resolve_line(
            "cargo test -p xpile --test contract_citation_integrity \
             every_emitted_citation_resolves_to_an_on_disk_contract",
            &root
        ),
        Ok(()),
        "the resolver rejects a command that runs today"
    );
}

/// NEEDLE, negative half. Each way a command can fail to check anything is
/// reported — measured by perturbation, not asserted.
#[test]
fn the_needle_reports_each_way_a_command_can_fail() {
    let root = workspace_root();

    // (a) a package that is not in the workspace.
    let dead_pkg = resolve_line("cargo test -p xpile-not-a-crate --lib", &root);
    assert!(
        dead_pkg.is_err_and(|e| e.contains("not a workspace member")),
        "a dead package was not reported"
    );

    // (b) the shape that was live on 67 entries: a target that never existed.
    let dead_target = resolve_line("cargo test -p xpile-backend --test idempotency", &root);
    assert!(
        dead_target.is_err_and(|e| e.contains("no test target named")),
        "a fabricated `--test` target was not reported"
    );

    // (c) the shape that was live on 3 entries and is worse — the target
    // resolves, the filter selects nothing, cargo exits 0.
    let empty_filter = resolve_line(
        "cargo test -p xpile --test transpile_e2e str_no_lifetimes",
        &root,
    );
    assert!(
        empty_filter.is_err_and(|e| e.contains("running 0 tests")),
        "a filter matching no test was not reported — this is the arm that \
         exits 0 and reads as a pass"
    );

    // (d) a package with no `#[test]` anywhere: `cargo test -p X` exits 0
    // having run nothing, which is the same vacuity one level up.
    let empty_pkg = resolve_line("cargo test -p xpile-contract-backend --lib", &root);
    assert!(
        empty_pkg.is_err_and(|e| e.contains("selects no `#[test]` at all")),
        "a package with no tests at all was not reported"
    );

    // (e) a disclosure that still carries a command line does not pass as a
    // disclosure — otherwise the hole marker would launder a fake command.
    let smuggled = Entry {
        file: "synthetic".into(),
        id: "SYNTH-1".into(),
        test: format!("{HOLE} (PMAT-1459).\ncargo test -p xpile-backend --test idempotency\n"),
    };
    assert!(
        matches!(verdict(&smuggled, &root), Verdict::Broken(w) if w.contains("reads as a command")),
        "a disclosure smuggling a command line was accepted"
    );

    // (f) an undated disclosure is not a disclosure.
    let undated = Entry {
        file: "synthetic".into(),
        id: "SYNTH-2".into(),
        test: format!("{HOLE}. Someone will fix this.\n"),
    };
    assert!(
        matches!(verdict(&undated, &root), Verdict::Broken(w) if w.contains("cites no PMAT id")),
        "an undated disclosure was accepted"
    );

    // CONTROL: the disclosure the repair actually writes IS accepted, so (e)
    // and (f) are rejecting the perturbation and not the shape.
    let real = Entry {
        file: "synthetic".into(),
        id: "SYNTH-3".into(),
        test: format!(
            "{HOLE} (PMAT-1459, measured 2026-07-29). This entry published \
             `cargo test -p xpile-backend --test idempotency`, a target that \
             does not exist.\n"
        ),
    };
    assert_eq!(verdict(&real, &root), Verdict::DeclaredHole);
}

/// The `#[test]` needle counts a real test attribute and nothing that merely
/// spells one. Constructed, because relaxing the check changes no live verdict
/// today — this pins the guard so a future `contains`-style loosening reds
/// here instead of silently widening what a filter may match.
#[test]
fn the_needle_counts_only_a_real_test_attribute() {
    let src = "\
#[test]
fn a_real_one() {}

/// Prose mentioning #[test] fn harvested_from_a_doc_comment
fn not_a_test() {}

const EMITTED: &str = \"#[test]\\n\\
fn harvested_from_a_string_literal() {}\";

    #[test]
    fn an_indented_one() {}
";
    let names = test_fn_names(src);
    assert_eq!(
        names,
        vec!["a_real_one".to_string(), "an_indented_one".to_string()],
        "the needle harvested a name that is not a `#[test]` fn — a filter \
         could then match prose and read as runnable"
    );
}
