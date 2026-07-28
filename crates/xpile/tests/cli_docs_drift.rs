//! XPILE-CLIDOCS-001 (PMAT-1429) — the published CLI reference must be
//! re-derivable FROM THE BINARY, not maintained by hand beside it.
//!
//! ## The defect this locks out
//!
//! `book/src/reference/cli.md` publishes a pinned `$ xpile info` transcript
//! and tells the reader: *"Use this to confirm your install can see every
//! lane."* Measured at e50f0520, the transcript a reader would confirm
//! against had drifted by three whole backends and a whole frontend:
//!
//! | claim in the book | live registry |
//! |---|---|
//! | `frontends (4):`  | `frontends (5 registered, 4 lowering):` |
//! | — (absent)        | `- wasm (wat)` |
//! | `- ruchy (ruchy)` | `- ruchy (ruchy)  [routing only — INPUT refuses, no parser]` |
//! | `backends (6):`   | `backends (9):` |
//! | — (absent)        | `- spirv → Spirv`, `- wasm → Wasm`, `- forjar → ForjarYaml` |
//!
//! A reader doing exactly what the page instructs would count nine backends
//! against a published six and conclude their install was wrong — or, far
//! more likely, would simply never learn that `--target wasm`,
//! `--target spirv` and `--target forjar` exist. The same page's `--target`
//! row listed seven of the nine target spellings, and `xpile hybrid` — a
//! registered subcommand — had no section at all.
//!
//! ## Why claims_drift.rs did not catch it
//!
//! `claims_drift.rs` DOES walk `book/src/` (PMAT-1417 brought it into
//! scope), but its book-corpus checks are forbidden-/required-SUBSTRING
//! rules, and its DERIVED cardinalities ("N source languages", "nine
//! backends") are asserted against `README.md` only. A stale transcript on a
//! different page is the same claim CLASS on a surface no rule covered —
//! PMAT-1417's own lesson, one file over.
//!
//! This gate is deliberately not another substring rule. It EXECUTES the
//! binary and compares two independently recorded halves: what `xpile info`
//! prints, and what the book says it prints. Neither half can be edited into
//! agreement without the other actually being true.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/xpile → repo root")
        .to_path_buf()
}

fn cli_md() -> String {
    let p = repo_root().join("book/src/reference/cli.md");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("reading {}: {e}", p.display()))
}

fn xpile(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("running `xpile {}`: {e}", args.join(" ")));
    assert!(
        out.status.success(),
        "`xpile {}` exited {:?}",
        args.join(" "),
        out.status.code()
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Every `--target` spelling the RUNNING BINARY accepts, canonical AND alias,
/// read off its own `unknown target` refusal (PMAT-1435). Deriving this from
/// `--help` is what let the nine-vs-thirteen gap stand; the refusal is
/// rendered from `TARGET_SPELLINGS`, the roster `parse_target` matches
/// through, so it cannot name a spelling the binary rejects or omit one it
/// takes. Deliberately duplicated in `backend_docs_drift.rs` rather than
/// shared: `parse_target` lives in a BINARY crate, so the printed message is
/// the only surface a test can reach, and the two files must agree by
/// measuring the same thing rather than by importing the same helper.
fn accepted_target_spellings() -> Vec<String> {
    // `transpile` reads the input BEFORE parsing `--target`, so the vocabulary
    // is only reachable with a readable file in hand.
    let dir = std::env::temp_dir().join(format!("xpile-clitgt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create probe dir");
    let src = dir.join("probe.py");
    std::fs::write(&src, "def add(a: int, b: int) -> int:\n    return a + b\n").expect("write");
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .args([
            "transpile",
            &src.to_string_lossy(),
            "--target",
            "__no_such_target__",
        ])
        .output()
        .expect("running xpile transpile --target __no_such_target__");
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let vocab = stderr
        .split("unknown target")
        .nth(1)
        .unwrap_or_else(|| panic!("`--target __no_such_target__` must refuse:\n{stderr}"))
        .to_string();
    let after = |k: &str| -> Vec<String> {
        vocab
            .split(k)
            .nth(1)
            .map(|s| {
                s.split(';')
                    .next()
                    .unwrap_or("")
                    .split(',')
                    .map(|t| t.trim().split('=').next().unwrap_or("").trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut all = after("choose:");
    all.extend(after("aliases:"));
    all
}

/// Extract the fenced block that immediately follows a `$ <cmd>` prompt line,
/// returning the transcript body WITHOUT the prompt line itself.
fn pinned_transcript(md: &str, prompt: &str) -> String {
    let mut lines = md.lines();
    while let Some(l) = lines.next() {
        if l.trim() != prompt {
            continue;
        }
        let mut body = String::new();
        for l in lines.by_ref() {
            if l.starts_with("```") {
                return body;
            }
            body.push_str(l);
            body.push('\n');
        }
        panic!("unterminated fenced block after `{prompt}` in cli.md");
    }
    panic!("cli.md publishes no `{prompt}` transcript — this gate cannot pass vacuously");
}

/// The load-bearing check: the published transcript IS the binary's output.
#[test]
fn the_published_xpile_info_transcript_matches_the_binary() {
    let published = pinned_transcript(&cli_md(), "$ xpile info");
    let live = xpile(&["info"]);
    assert_eq!(
        published.trim_end(),
        live.trim_end(),
        "\n`book/src/reference/cli.md` publishes an `xpile info` transcript that the \
         binary does not produce. The page tells the reader to use it to \"confirm \
         your install can see every lane\", so a stale transcript actively \
         misinforms.\n\n--- published ---\n{published}\n--- live ---\n{live}\n\
         Regenerate with: cargo run -p xpile --bin xpile -- info\n"
    );
}

/// Every subcommand the CLI registers has a `## \`xpile <name>\`` section.
/// The list is read out of `xpile --help`, so a new subcommand cannot ship
/// undocumented (this is how `xpile hybrid` went missing).
#[test]
fn every_registered_subcommand_has_a_cli_md_section() {
    let help = xpile(&["--help"]);
    let md = cli_md();

    // The `Commands:` block of clap's help — one subcommand per line, name
    // in the first column.
    let mut names: Vec<String> = Vec::new();
    let mut in_commands = false;
    for line in help.lines() {
        if line.starts_with("Commands:") {
            in_commands = true;
            continue;
        }
        if in_commands {
            if line.trim().is_empty() || !line.starts_with("  ") {
                break;
            }
            if let Some(name) = line.split_whitespace().next() {
                names.push(name.to_string());
            }
        }
    }
    assert!(
        names.len() > 3,
        "parsed only {names:?} from `xpile --help` — the Commands block shape \
         changed and this gate would pass vacuously"
    );

    let missing: Vec<&String> = names
        .iter()
        .filter(|n| !md.contains(&format!("## `xpile {n}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "`book/src/reference/cli.md` has no `## \\`xpile <cmd>\\`` section for \
         registered subcommand(s) {missing:?}. Every subcommand `xpile --help` \
         lists must be documented on the CLI reference page."
    );
}

/// The `--target` row must name exactly the spellings the CLI accepts —
/// checked on the LOCATED row (PMAT-1420: a substring sweep of the whole
/// page would pass on any incidental mention), and in BOTH directions, so
/// the row can neither omit a live target nor advertise a dead one.
///
/// PMAT-1435: BOTH DIRECTIONS IS ONLY AS HONEST AS THE SET IT COMPARES TO.
/// `live` was parsed from `xpile transpile --help`, which named the nine
/// canonical spellings while `parse_target` also accepted `wat`, `sh`, `bash`
/// and `forjar-yaml`. So this test — whose NAME says "exactly the accepted
/// target spellings" — could never fire in the omit direction for those four,
/// and in the advertise direction it FORBADE the row from naming them, on the
/// grounds that `--help` "does not list" them. cli.md duly published the
/// closed-world claim *one of `rust` … `forjar`*, which was false, while
/// `backends.md` disclosed all four two pages over: the book contradicted
/// itself and this gate held the false half in place.
///
/// `live` now comes from the `unknown target` REFUSAL, which
/// `target_spelling_help()` renders from the same `TARGET_SPELLINGS` roster
/// `parse_target` matches through. See
/// `target_spelling_disposition_witness.rs` (XPILE-TARGET-SPELL-001), which
/// holds the refusal, `--help` and this row to each other.
#[test]
fn the_target_row_names_exactly_the_accepted_target_spellings() {
    let live = accepted_target_spellings();
    assert!(
        live.len() > 3,
        "parsed only {live:?} target spellings from the refusal — shape changed, \
         this gate would pass vacuously"
    );

    let md = cli_md();
    let row = md
        .lines()
        .find(|l| l.trim_start().starts_with("| `--target"))
        .unwrap_or_else(|| {
            panic!("cli.md has no `--target` table row — this gate cannot pass vacuously")
        });

    let missing: Vec<&String> = live
        .iter()
        .filter(|t| !row.contains(&format!("`{t}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "cli.md's `--target` row omits live target spelling(s) {missing:?}.\nrow: {row}"
    );

    // Reverse direction: nothing on the row that the CLI would reject.
    for tok in row.split('`').skip(1).step_by(2) {
        if tok.starts_with("--") || tok.contains(' ') || tok == "T" || tok == "rust" {
            continue;
        }
        assert!(
            live.iter().any(|t| t == tok),
            "cli.md's `--target` row advertises `{tok}`, which `xpile transpile \
             --help` does not list as an accepted target ({live:?}).\nrow: {row}"
        );
    }
}
