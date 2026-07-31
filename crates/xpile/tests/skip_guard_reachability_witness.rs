//! XPILE-SKIPGUARD-001 — a presence guard that decides whether an assertion
//! runs must be ABLE TO SAY YES (PMAT-1505).
//!
//! ## What was wrong
//!
//! `makefile_dialect_refusal_witness.rs` line 211 read:
//!
//! ```text
//! if !tool_present("make", "--version") || !tool_present("sh", "-c") {
//! ```
//!
//! and `tool_present` is `Command::new(cmd).arg(arg).output().is_ok_and(|o|
//! o.status.success())`. **`sh -c` with no operand is a usage error on every
//! POSIX shell** — dash, bash and busybox ash all exit 2. So the guard reported
//! `sh` ABSENT on every host that has ever run this suite, and the test it
//! guards returned at its first statement, printed `SKIP`, and reported
//! **PASS**.
//!
//! The test is named `make_and_the_shredded_shell_disagree_when_executed` and
//! its doc comment is headed *"ASSERTION 3 — THE JUSTIFICATION, EXECUTED"*. It
//! is the evidence that lowering a `Makefile` as flat shell is wrong — the
//! whole warrant for PMAT-1420's refusal. It had **never executed**, here or in
//! CI, on any host, since the day it was written.
//!
//! Repaired and run for the first time on 2026-07-31 the assertion **passes**,
//! and leaves the evidence on disk: `make` builds `out.txt`, the shredded shell
//! deletes it, both at exit 0. The claim was true. Nothing had checked.
//!
//! ## Why this is the worst shape a witness can have
//!
//! A false claim in prose is found by reading. A hollow witness is *immune to
//! reading*: the file is present, the assertion is written, the name says
//! EXECUTED, the suite is green, and the count of passing tests includes it.
//! Every signal a reviewer has says the property is measured. The skip is one
//! line of stderr inside `cargo test`'s captured output, which nobody sees
//! because the run passed.
//!
//! ## What this gate does, and what it deliberately does not do
//!
//! "Can this argument vector succeed?" is not decidable from source, so this
//! file does not pretend to decide it. It closes the hole from two sides:
//!
//! * **Statically, over the whole tracked test corpus** — every argument vector
//!   used as a presence probe must be one of the shapes in `AUDITED_SHAPES`.
//!   That table is an AUDIT RECORD, not a decision procedure: a new spelling
//!   reds this gate and asks a human to check it once. The shapes are keyed on
//!   the ARGUMENTS, not on the tool, so adding a new `--version`-probed tool
//!   needs no edit here — only a new *spelling* does.
//!
//! * **Dynamically, wherever the tool exists** — every derived `(tool, args)`
//!   pair whose binary resolves on this host must exit 0. This is the half that
//!   would have caught the live defect on the day it landed, and it needs no
//!   allowlist to do it.
//!
//! Both halves are derived from `git ls-files`; no probe inventory is typed
//! into this file. `no_derivation_is_vacuous` pins that the extractor still
//! finds probes it is known to be able to find, so a broken regex reds instead
//! of reporting a clean sweep — the failure mode every derived gate in this
//! repository has to defend against.
//!
//! **Self-exclusion, disclosed:** this file is removed from its own corpus,
//! because it quotes the retired malformed spelling verbatim as a control and
//! would otherwise red on its own evidence (PMAT-1495's exemption trap, which
//! has now fired on six consecutive slices). The consequence is that probes
//! written *in this file* are unswept by the static half — they are the
//! controls, and `positive_control_*` / `negative_control_*` assert on them
//! directly.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/xpile → repo root")
        .to_path_buf()
}

/// This file, relative to the repo root — excluded from its own corpus.
const SELF_PATH: &str = "crates/xpile/tests/skip_guard_reachability_witness.rs";

/// Argument vectors audited as able to succeed when the tool is installed.
///
/// Keyed on the ARGUMENTS so that a new tool probed the usual way needs no
/// entry. Each row records why it can succeed.
const AUDITED_SHAPES: &[(&[&str], &str)] = &[
    (&["--version"], "GNU/POSIX convention: prints and exits 0"),
    (&["-V"], "short spelling of --version (rustc, python3)"),
    (
        &["-c", "true"],
        "a shell -c REQUIRES an operand; `true` is the cheapest one that exits 0",
    ),
    (
        &["kani", "--version"],
        "cargo subcommand probe: `cargo kani --version` exits 0 iff the subcommand is installed",
    ),
];

/// The shape whose absence of an operand is the defect this file exists for.
/// A shell invoked with `-c` and nothing to run is a usage error, not a probe.
const SHELLS: &[&str] = &["sh", "/bin/sh", "bash", "/bin/bash", "dash", "zsh", "ksh"];

/// Probes that ask about a REMOTE resource, excluded from both properties.
///
/// A non-zero exit from one of these means the host, the credential or the
/// network is absent — which is an honest skip, exactly what a presence guard
/// is for. Only a LOCAL binary's probe carries the "malformed spelling" signal
/// this gate hunts. `no_exclusion_is_stale` keeps the list from outliving its
/// reason.
const REMOTE_PROBES: &[(&str, &str)] = &[(
    "ssh",
    "gx10_available() reaches the GB10 box; exit 255 is an unreachable remote, \
     not a bad spelling",
)];

/// A presence probe recovered from the corpus.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Probe {
    file: String,
    line: usize,
    /// `None` when the guard takes the tool as a parameter.
    tool: Option<String>,
    args: Vec<String>,
}

impl Probe {
    fn at(&self) -> String {
        format!("{}:{}", self.file, self.line)
    }
}

/// How this repository spells "is this tool here?".
fn is_guard_name(name: &str) -> bool {
    name == "tool_present"
        || name == "tool_available"
        || name.starts_with("have_")
        || name.ends_with("_present")
        || name.ends_with("_available")
}

/// Every tracked Rust integration test, minus this file.
fn corpus(root: &PathBuf) -> Vec<(String, String)> {
    let out = Command::new("git")
        .arg("ls-files")
        .arg("*/tests/*.rs")
        .current_dir(root)
        .output()
        .expect("git ls-files");
    assert!(out.status.success(), "git ls-files must succeed");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|p| p.ends_with(".rs") && *p != SELF_PATH)
        .filter_map(|p| {
            let abs = root.join(p);
            // A staged-then-deleted path is tracked but absent (PMAT-1501).
            std::fs::read_to_string(&abs)
                .ok()
                .map(|src| (p.to_string(), src))
        })
        .collect()
}

/// String literals inside one `[...]`/`(...)` fragment.
fn literals(fragment: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes: Vec<char> = fragment.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '"' {
            let mut j = i + 1;
            let mut lit = String::new();
            while j < bytes.len() && bytes[j] != '"' {
                if bytes[j] == '\\' && j + 1 < bytes.len() {
                    j += 1;
                }
                lit.push(bytes[j]);
                j += 1;
            }
            out.push(lit);
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// The `fn NAME(` on a line, if any.
fn fn_name_on(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let rest = t
        .strip_prefix("fn ")
        .or_else(|| t.strip_prefix("pub fn "))?;
    let end = rest.find(['(', '<'])?;
    Some(&rest[..end])
}

/// The literal argument vectors reachable from ONE `Command::new(...)`, taken
/// from the `.arg("x")` / `.args(["x", "y"])` chain that follows it and stops
/// at the next `Command::new`.
fn args_in_segment(segment: &str) -> Vec<String> {
    let mut chain: Vec<String> = Vec::new();
    let mut scan = segment;
    loop {
        let a = scan.find(".arg(");
        let b = scan.find(".args(");
        let next = match (a, b) {
            (Some(x), Some(y)) => Some(x.min(y)),
            (x, y) => x.or(y),
        };
        let Some(p) = next else { break };
        let rest = &scan[p..];
        let open = rest.find('(').expect("matched on a '('");
        let stop = rest.find(')').unwrap_or(rest.len());
        chain.extend(literals(&rest[open..stop]));
        scan = &rest[stop.min(rest.len())..];
        if scan.is_empty() {
            break;
        }
        scan = &scan[1.min(scan.len())..];
    }
    chain
}

/// A tool name a `Command::new` could plausibly carry. Guards against
/// extraction noise being reported as a defect.
fn plausible_tool(t: &str) -> bool {
    !t.is_empty()
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-+/".contains(c))
        && t.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '/')
}

/// EXTRACTION A — guard-function bodies, segmented per `Command::new`.
///
/// Segmenting matters: `have_python_and_sh()` builds TWO commands in one body,
/// and a body-wide `.arg` sweep would fuse them into one nonsense vector
/// `["--version", "-c", "true"]` that belongs to neither.
///
/// Returns the probes plus, per guard-function name, the argument vectors that
/// function applies — which is how a one-operand call site like
/// `tool_present("cc")` recovers the `--version` its helper supplies.
fn probes_from_guard_bodies(file: &str, src: &str) -> (Vec<Probe>, Vec<(String, Vec<String>)>) {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    let mut helper_args: Vec<(String, Vec<String>)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some(name) = fn_name_on(lines[i]) else {
            i += 1;
            continue;
        };
        if !is_guard_name(name) {
            i += 1;
            continue;
        }
        let indent = lines[i].len() - lines[i].trim_start().len();
        let close = format!("{}}}", " ".repeat(indent));
        let mut j = i + 1;
        while j < lines.len() && lines[j] != close {
            j += 1;
        }
        let body_start = i;
        let body: String = lines[i..j.min(lines.len())].join("\n");

        let marker = "Command::new(";
        let mut segments: Vec<&str> = Vec::new();
        let mut scan = body.as_str();
        let mut prelude_end = body.len();
        while let Some(p) = scan.find(marker) {
            if segments.is_empty() {
                prelude_end = body.len() - scan.len() + p;
            }
            scan = &scan[p + marker.len()..];
            let next = scan.find(marker).unwrap_or(scan.len());
            segments.push(&scan[..next]);
        }

        let mut any_literal_args = false;
        for seg in &segments {
            let tool = seg
                .trim_start()
                .strip_prefix('"')
                .and_then(|r| r.find('"').map(|e| r[..e].to_string()))
                .filter(|t| plausible_tool(t));
            let args = args_in_segment(seg);
            if args.is_empty() {
                continue;
            }
            any_literal_args = true;
            if let Some(t) = &tool {
                helper_args.push((name.to_string(), args.clone()));
                let _ = t;
            } else {
                helper_args.push((name.to_string(), args.clone()));
            }
            out.push(Probe {
                file: file.to_string(),
                line: body_start + 1,
                tool,
                args,
            });
        }

        // `default_flag_witness` picks its vector in a `match` BEFORE the
        // `Command::new(cmd).args(probe)`, so no literal reaches the chain.
        // Those arrays are still probe SHAPES and must be audited.
        if !any_literal_args && !segments.is_empty() {
            let prelude = &body[..prelude_end];
            let mut scan = prelude;
            while let Some(p) = scan.find('[') {
                let rest = &scan[p..];
                let Some(end) = rest.find(']') else { break };
                let lits = literals(&rest[1..end]);
                if !lits.is_empty() {
                    helper_args.push((name.to_string(), lits.clone()));
                    out.push(Probe {
                        file: file.to_string(),
                        line: body_start + 1,
                        tool: None,
                        args: lits,
                    });
                }
                scan = &rest[end + 1..];
            }
        }
        i = j.max(i + 1);
    }
    (out, helper_args)
}

/// EXTRACTION B — guard CALL SITES that spell both operands literally.
///
/// This is where the live defect was: the helper was fine and the caller passed
/// `"-c"`.
fn probes_from_call_sites(
    file: &str,
    src: &str,
    helper_args: &[(String, Vec<String>)],
) -> Vec<Probe> {
    let mut out = Vec::new();
    for (n, line) in src.lines().enumerate() {
        let mut scan = line;
        while let Some(p) = scan.find('(') {
            let before = &scan[..p];
            let name = before
                .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
                .next()
                .unwrap_or("");
            let rest = &scan[p + 1..];
            let stop = rest.find(')').unwrap_or(rest.len());
            let inner = &rest[..stop];
            if is_guard_name(name) && !name.is_empty() {
                let lits = literals(inner);
                // A literal first operand is the tool; the rest are its args.
                if !lits.is_empty()
                    && inner.trim_start().starts_with('"')
                    && plausible_tool(&lits[0])
                {
                    let tool = lits[0].clone();
                    if lits.len() > 1 {
                        out.push(Probe {
                            file: file.to_string(),
                            line: n + 1,
                            tool: Some(tool),
                            args: lits[1..].to_vec(),
                        });
                    } else {
                        // One operand: the helper supplies the arguments.
                        // Without this resolution `tool_present("cc")` reads
                        // as `cc` with no arguments, which exits 1 and would
                        // be reported as a defect it is not.
                        for (h, args) in helper_args.iter().filter(|(h, _)| h == name) {
                            let _ = h;
                            out.push(Probe {
                                file: file.to_string(),
                                line: n + 1,
                                tool: Some(tool.clone()),
                                args: args.clone(),
                            });
                        }
                    }
                }
            }
            scan = &rest[stop.min(rest.len())..];
            if scan.is_empty() {
                break;
            }
            scan = &scan[1.min(scan.len())..];
        }
    }
    out
}

/// Every presence probe in the tracked corpus, minus the remote ones.
fn all_probes() -> Vec<Probe> {
    let root = repo_root();
    let mut out = Vec::new();
    for (file, src) in corpus(&root) {
        let (body_probes, helper_args) = probes_from_guard_bodies(&file, &src);
        out.extend(body_probes);
        out.extend(probes_from_call_sites(&file, &src, &helper_args));
    }
    out.retain(|p| {
        !p.args.is_empty()
            && !p
                .tool
                .as_deref()
                .is_some_and(|t| REMOTE_PROBES.iter().any(|(r, _)| *r == t))
    });
    out.sort();
    out.dedup();
    out
}

/// Every probe including the remote ones — the exclusion's own control.
fn all_probes_unfiltered() -> Vec<Probe> {
    let root = repo_root();
    let mut out = Vec::new();
    for (file, src) in corpus(&root) {
        let (body_probes, helper_args) = probes_from_guard_bodies(&file, &src);
        out.extend(body_probes);
        out.extend(probes_from_call_sites(&file, &src, &helper_args));
    }
    out.retain(|p| !p.args.is_empty());
    out.sort();
    out.dedup();
    out
}

/// The classifier, isolated so the controls can drive it directly.
///
/// `Err(reason)` means: this vector cannot succeed even where the tool is
/// installed.
fn classify(tool: Option<&str>, args: &[String]) -> Result<(), String> {
    let is_shell = tool.is_some_and(|t| SHELLS.contains(&t));
    if (is_shell || tool.is_none()) && args.len() == 1 && args[0] == "-c" {
        return Err(
            "a shell `-c` with no operand is a usage error, not a presence probe — \
             every POSIX shell exits non-zero, so the guard reports the tool ABSENT \
             on every host and the assertion it guards never runs"
                .to_string(),
        );
    }
    if AUDITED_SHAPES.iter().any(|(shape, _)| *shape == args) {
        return Ok(());
    }
    Err(format!(
        "argument vector {args:?} is not an audited presence-probe shape. \
         Audited shapes: {:?}. If this spelling does exit 0 where the tool is \
         installed, add it to AUDITED_SHAPES with the reason; if it does not, \
         the guard can never say yes and the assertion behind it never runs",
        AUDITED_SHAPES.iter().map(|(s, _)| *s).collect::<Vec<_>>()
    ))
}

/// PROPERTY 1 — static, host-independent. Every presence probe in the tracked
/// test corpus is spelled in a way that can succeed.
#[test]
fn every_presence_probe_in_the_corpus_uses_an_audited_shape() {
    let offences: Vec<String> = all_probes()
        .iter()
        .filter_map(|p| {
            classify(p.tool.as_deref(), &p.args)
                .err()
                .map(|why| format!("  {} — {:?} {:?}\n      {why}", p.at(), p.tool, p.args))
        })
        .collect();

    assert!(
        offences.is_empty(),
        "presence guards that cannot say yes — each one silently disables every \
         assertion behind it while the test still reports PASS:\n{}",
        offences.join("\n")
    );
}

/// PROPERTY 2 — dynamic. Wherever the binary actually resolves on this host,
/// its probe must exit 0. No allowlist involved; this is the half that catches
/// a malformed spelling the day it lands.
#[test]
fn every_resolvable_probe_actually_exits_zero() {
    let mut executed = 0usize;
    let mut absent: BTreeSet<String> = BTreeSet::new();
    let mut failures = Vec::new();

    for p in all_probes() {
        let Some(tool) = p.tool.as_deref() else {
            continue;
        };
        match Command::new(tool).args(&p.args).output() {
            Err(_) => {
                absent.insert(tool.to_string());
            }
            Ok(o) => {
                executed += 1;
                if !o.status.success() {
                    failures.push(format!(
                        "  {} — `{} {}` exited {:?}; the guard reports {tool} ABSENT on a host \
                         where it is installed, so everything behind it never runs",
                        p.at(),
                        tool,
                        p.args.join(" "),
                        o.status.code()
                    ));
                }
            }
        }
    }

    if !absent.is_empty() {
        eprintln!("SKIPPED (binary not on this host): {absent:?}");
    }
    assert!(
        executed > 0,
        "no probe resolved on this host, so this property asserted nothing — a \
         vacuous green. At minimum `git` is required to have reached this point."
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!("executed {executed} presence probes");
}

/// PROPERTY 3 — POSITIVE CONTROL. The classifier must reject the exact retired
/// spelling, and reject it for a shell named any of the ways this repo names
/// one. A screen nobody has seen fire may match nothing at all.
#[test]
fn positive_control_the_retired_spelling_is_rejected() {
    let bare = vec!["-c".to_string()];
    for shell in SHELLS {
        let verdict = classify(Some(shell), &bare);
        assert!(
            verdict.is_err(),
            "`{shell} -c` with no operand must be rejected; it is the live defect \
             PMAT-1505 repaired"
        );
    }
    // And through a guard whose tool is a parameter, which is how the defect
    // was actually spelled at the call site.
    assert!(
        classify(None, &bare).is_err(),
        "a bare `-c` must be rejected even when the tool is not named literally"
    );
    // An unaudited spelling is rejected too, so the allowlist is doing work.
    assert!(
        classify(Some("lean"), &["--rev".to_string()]).is_err(),
        "an unaudited argument vector must be rejected"
    );
}

/// PROPERTY 4 — NEGATIVE CONTROL. The classifier must not fire on correct
/// probes, including for a tool that is not installed. Absence is not
/// malformedness, and conflating them would red every runner missing a tool.
#[test]
fn negative_control_well_formed_probes_pass() {
    for (shape, why) in AUDITED_SHAPES {
        let args: Vec<String> = shape.iter().map(|s| s.to_string()).collect();
        assert!(
            classify(Some("sh"), &args).is_ok() || !SHELLS.contains(&"sh"),
            "audited shape {shape:?} ({why}) must classify clean"
        );
    }
    let with_operand = vec!["-c".to_string(), "true".to_string()];
    assert!(
        classify(Some("sh"), &with_operand).is_ok(),
        "`sh -c true` is the repaired spelling and must classify clean"
    );
    assert!(
        classify(Some("no-such-tool-xpile-probe"), &["--version".to_string()]).is_ok(),
        "a well-formed probe for an ABSENT tool is not a defect — absence is an \
         honest skip, malformedness is a silent one"
    );
}

/// PROPERTY 5 — the remote-probe exclusion may not outlive its reason. An
/// exemption for something no longer in the corpus is a hole nobody is
/// watching, and this repository has shipped one before (PMAT-1495).
#[test]
fn no_exclusion_is_stale() {
    let tools: BTreeSet<String> = all_probes_unfiltered()
        .iter()
        .filter_map(|p| p.tool.clone())
        .collect();
    for (excluded, reason) in REMOTE_PROBES {
        assert!(
            tools.contains(*excluded),
            "`{excluded}` is excluded from this gate ({reason}) but no longer appears \
             as a presence probe in the tracked corpus — drop the exemption rather \
             than leaving an unwatched hole"
        );
    }
    assert!(
        all_probes_unfiltered().len() > all_probes().len(),
        "the exclusion removed nothing, so it is either stale or misspelled"
    );
}

/// PROPERTY 6 — ANTI-VACUITY of the DERIVATION itself. A regex that stops
/// matching reports a clean sweep, which is indistinguishable from a clean
/// repository. These anchors exist in the tracked corpus today; if the
/// extractor loses them, it has broken.
#[test]
fn no_derivation_is_vacuous() {
    let probes = all_probes();
    assert!(
        probes.len() >= 10,
        "the extractor found only {} presence probes across the tracked test \
         corpus; it has broken",
        probes.len()
    );

    let named: BTreeSet<(String, Vec<String>)> = probes
        .iter()
        .filter_map(|p| p.tool.clone().map(|t| (t, p.args.clone())))
        .collect();

    // One anchor per extraction path: `lean_present()` / `python3_present()`
    // are recovered from a guard BODY, the two-operand `tool_present` calls
    // from a CALL SITE. Losing either path must red.
    for (tool, args) in [
        ("lean", vec!["--version"]),
        ("python3", vec!["--version"]),
        ("make", vec!["--version"]),
        ("rustc", vec!["--version"]),
    ] {
        let want = (
            tool.to_string(),
            args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        );
        assert!(
            named.contains(&want),
            "the derivation lost `{tool} {}` — it is spelled in the tracked corpus, \
             so the extractor is broken and every property here is vacuous. Found: {:?}",
            args.join(" "),
            named.iter().take(20).collect::<Vec<_>>()
        );
    }

    let files: BTreeSet<&str> = probes.iter().map(|p| p.file.as_str()).collect();
    assert!(
        files.len() >= 5,
        "presence probes were found in only {} file(s); the corpus walk has broken",
        files.len()
    );
    eprintln!(
        "derived {} presence probes across {} files",
        probes.len(),
        files.len()
    );
}
