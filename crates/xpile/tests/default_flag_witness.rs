//! XPILE-DEFAULTFLAGS-001 (PMAT-1411) — every lane's execution witness runs
//! the flag set a USER gets, and the DEFAULT emit is accepted by that lane's
//! own toolchain.
//!
//! ## The defect class
//!
//! PMAT-1405 — `--target lean`'s DEFAULT emit wrote Lean that `lean` could not
//! parse — shipped *from a lane that had just been swept*. It survived because
//! `lean_elaborate_witness.rs`, the Lean lane's semantic oracle, passes
//! `--contracts off`. The corpus certified a flag combination the default
//! invocation never takes, so the path every user gets was ungated — and the
//! same hole was open, unmeasured, in every other lane.
//!
//! A per-lane execution witness is only worth its name if it runs the DEFAULT
//! flags. This file makes that structural rather than a habit.
//!
//! ## What is asserted
//!
//! 1. `defaulted_transpile_flags_are_the_expected_two` — the *premise*. The
//!    `Transpile` subcommand's defaulted options are derived from
//!    `crates/xpile/src/main.rs` and pinned to `{target: "rust",
//!    contracts: "on"}`. A new defaulted flag, or a flip of either default,
//!    reds HERE rather than silently re-basing everything below onto a
//!    baseline nobody re-read.
//!
//! 2. `default_emit_is_accepted_by_each_lane_toolchain` — the load-bearing
//!    half, and an EXECUTION differential, not a text match. For every lane:
//!      * the DEFAULT invocation (no `--contracts`) exits 0;
//!      * its output is accepted by that lane's real downstream toolchain
//!        (`rustc`, `wat2wasm`, `lean`, `ruchy`, `ptxas`, `naga`, `sh -n`,
//!        `forjar validate`) — skip-with-reason, never silently, when the
//!        tool is absent;
//!      * the citation channel is LIVE: default output carries
//!        `xpile-contract:` and `--contracts off` strips it. Without this
//!        third clause a backend that stopped citing entirely would still
//!        pass the first two.
//!
//! 3. `lane_table_covers_every_target_the_binary_offers` — the lane list is
//!    checked against the target list parsed out of the LIVE binary's own
//!    rejection message, not against a hand-copied list in a comment. A tenth
//!    backend cannot be added without a lane here.
//!
//! 4. `every_contracts_deviation_in_a_test_is_declared_and_covered` — the
//!    class gate. Any test in the workspace that CLI-invokes `transpile` with
//!    a non-default `--contracts` must say so on a `XPILE-DEFAULT-FLAGS:`
//!    line AND name a `DEFAULT-PATH-COVERED-BY:` file that exists and itself
//!    probes the same target with default flags. That is exactly the
//!    remediation PMAT-1405 performed by hand for Lean; this makes the next
//!    one mandatory instead of remembered.
//!
//! ## Honest scope — read before citing this file
//!
//! * The PTX lane emits NO inline `// xpile-contract:` line under EITHER
//!   setting: `--contracts on|off` produce BYTE-IDENTICAL PTX (measured
//!   2026-07-27, force-rebuilt binary). Its citations live only on the
//!   structural channel (`Artifact.citations`, `C-COMPILE-RUST-TO-PTX-MMA`).
//!   That asymmetry is RECORDED here as `Citations::StructuralOnly` rather
//!   than asserted away — it is one lane of nine, and a change that starts
//!   inlining PTX citations reds this file and forces a deliberate update.
//!   Do not read "9 lanes gated" as "9 lanes inline citations": 8 do.
//! * Clause 2's toolchain check runs ONE minimal program per lane. It gates
//!   the DEFAULT-FLAG question — "is the output users get readable by the
//!   lane's own tools" — and makes no claim about lane coverage breadth,
//!   which is what the per-lane differential corpora exist for.
//! * THE ORACLES ARE NOT EQUALLY STRONG, and the red half measured it rather
//!   than assuming it. `rustc`/`lean`/`ruchy`/`ptxas`/`wat2wasm`/`forjar
//!   validate` all rejected a corrupted emit; `sh -n` did NOT, because it
//!   checks SYNTAX only and the injected garbage was a syntactically valid
//!   command word. The shell lane's clause-2 check therefore catches an emit
//!   that does not PARSE, not one that parses and misbehaves — that stronger
//!   claim belongs to `shell_diff_exec.rs`, which executes. Every other lane's
//!   oracle was confirmed to red on a corrupted artifact.
//! * The deviation scan is TEXTUAL over test sources (`"--contracts"` /
//!   `--contracts=`), because the thing under inspection IS test source. It
//!   is vacuity-guarded: the scan must visit a floor of files AND find at
//!   least one `--contracts` occurrence, so a formatting change that breaks
//!   the parse reds instead of certifying an empty set.

use std::path::{Path, PathBuf};
use std::process::Command;

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

/// Workspace root — `crates/xpile/` is `CARGO_MANIFEST_DIR`.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = repo_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// A per-CALL unique scratch directory. Shared scratch dirs have produced
/// cross-test clobbering in this repo before, and these tests run in parallel.
fn scratch(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "xpile-defaultflags-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create scratch dir");
    d
}

/// MEASURED, not assumed: `/bin/sh` is `dash` on this repo's runners and
/// `dash --version` exits NON-ZERO, so the obvious `--version` probe reported
/// the shell lane's oracle ABSENT on a box where it is plainly present — a
/// false skip that greened the lane. Probe each tool with an invocation it
/// actually supports.
fn tool_present(cmd: &str) -> bool {
    let probe: &[&str] = match cmd {
        "sh" => &["-c", "true"],
        _ => &["--version"],
    };
    Command::new(cmd)
        .args(probe)
        .output()
        .is_ok_and(|o| o.status.success())
}

// ─── the lane table ────────────────────────────────────────────────────────

/// Whether a lane inlines `xpile-contract:` into its emitted text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Citations {
    /// `--contracts on` (the default) inlines the citation; `off` strips it.
    Inline,
    /// The lane carries citations ONLY on `Artifact.citations`; `--contracts`
    /// is a no-op on its text. PTX alone, measured — see the module doc.
    StructuralOnly,
}

/// The downstream toolchain that reads this lane's output.
struct Tool {
    /// Executable probed with `--version`; `None` = no external oracle.
    bin: &'static str,
    /// Extra argv after the emitted file (`{}` is substituted with the path).
    argv: &'static [&'static str],
}

struct Lane {
    /// `--target` value.
    target: &'static str,
    /// Extra REQUIRED args (not flag deviations): PTX cannot be reached
    /// without a hardware profile, so this is a lane selector, not an
    /// opt-out of a default.
    required: &'static [&'static str],
    /// Source file name written into the scratch dir.
    src_name: &'static str,
    /// Source text.
    src: &'static str,
    citations: Citations,
    /// Emitted-file extension handed to the toolchain.
    out_ext: &'static str,
    tool: Option<Tool>,
}

const PY_SRC: &str = "def add(a: int, b: int) -> int:\n    return a + b\n";
const SH_SRC: &str = "greet() {\n  echo hi\n}\ngreet\n";

/// Every `--target` the binary offers, each with the real toolchain that
/// consumes it. Verified end-to-end on 2026-07-27 against a force-rebuilt
/// binary; `naga` was absent on that box and its two lanes skipped-with-reason.
const LANES: &[Lane] = &[
    Lane {
        target: "rust",
        required: &[],
        src_name: "k.py",
        src: PY_SRC,
        citations: Citations::Inline,
        out_ext: "rs",
        tool: Some(Tool {
            bin: "rustc",
            argv: &[
                "--edition",
                "2021",
                "--crate-type",
                "lib",
                "--emit=metadata",
            ],
        }),
    },
    Lane {
        target: "wasm",
        required: &[],
        src_name: "k.py",
        src: PY_SRC,
        citations: Citations::Inline,
        out_ext: "wat",
        tool: Some(Tool {
            bin: "wat2wasm",
            argv: &["--output=/dev/null"],
        }),
    },
    Lane {
        target: "lean",
        required: &[],
        src_name: "k.py",
        src: PY_SRC,
        citations: Citations::Inline,
        out_ext: "lean",
        tool: Some(Tool {
            bin: "lean",
            argv: &[],
        }),
    },
    Lane {
        target: "ruchy",
        required: &[],
        src_name: "k.py",
        src: PY_SRC,
        citations: Citations::Inline,
        out_ext: "ruchy",
        tool: Some(Tool {
            bin: "ruchy",
            argv: &["__PRE__transpile"],
        }),
    },
    Lane {
        target: "ptx",
        required: &["--hardware", "ptx:sm_80"],
        src_name: "k.py",
        src: PY_SRC,
        citations: Citations::StructuralOnly,
        out_ext: "ptx",
        tool: Some(Tool {
            bin: "ptxas",
            argv: &["-arch=sm_80", "-o", "/dev/null"],
        }),
    },
    Lane {
        target: "wgsl",
        required: &[],
        src_name: "k.py",
        src: PY_SRC,
        citations: Citations::Inline,
        out_ext: "wgsl",
        tool: Some(Tool {
            bin: "naga",
            argv: &[],
        }),
    },
    Lane {
        target: "spirv",
        required: &[],
        src_name: "k.py",
        src: PY_SRC,
        citations: Citations::Inline,
        out_ext: "spvasm",
        tool: Some(Tool {
            bin: "naga",
            argv: &[],
        }),
    },
    Lane {
        target: "shell",
        required: &[],
        src_name: "s.sh",
        src: SH_SRC,
        citations: Citations::Inline,
        out_ext: "sh",
        tool: Some(Tool {
            bin: "sh",
            argv: &["__PRE__-n"],
        }),
    },
    Lane {
        target: "forjar",
        required: &[],
        src_name: "s.sh",
        src: SH_SRC,
        citations: Citations::Inline,
        out_ext: "yaml",
        tool: Some(Tool {
            bin: "forjar",
            argv: &["__PRE__validate", "__PRE__-f"],
        }),
    },
];

const CITATION_MARKER: &str = "xpile-contract:";

/// Emit `lane` with the given extra flags; returns `(success, stdout, stderr)`.
fn emit(lane: &Lane, dir: &Path, extra: &[&str]) -> (bool, String, String) {
    let src = dir.join(lane.src_name);
    std::fs::write(&src, lane.src).expect("write source");
    let mut args: Vec<String> = vec![
        "transpile".into(),
        src.to_string_lossy().into_owned(),
        "--target".into(),
        lane.target.into(),
    ];
    args.extend(lane.required.iter().map(|s| (*s).to_string()));
    args.extend(extra.iter().map(|s| (*s).to_string()));
    let out = Command::new(xpile_bin())
        .args(&args)
        .output()
        .expect("spawn xpile");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

// ─── 1. the premise ────────────────────────────────────────────────────────

/// Derive the `Transpile` subcommand's DEFAULTED options straight out of
/// `main.rs` and pin them. Every assertion below is stated relative to "the
/// default flag set"; if that set changes and this test does not, the rest of
/// the file silently certifies a baseline nobody re-read.
#[test]
fn defaulted_transpile_flags_are_the_expected_two() {
    let main_rs = read("crates/xpile/src/main.rs");
    // The `Transpile` variant runs from its declaration to the next variant
    // (`Audit`), so a defaulted flag on another subcommand is not counted.
    let start = main_rs
        .find("    Transpile {")
        .expect("main.rs declares a `Transpile {` subcommand variant");
    let rest = &main_rs[start..];
    let end = rest
        .find("    Audit {")
        .expect("`Audit {` follows `Transpile {` in the Cmd enum");
    let block = &rest[..end];

    // `#[arg(long, default_value = "V")]` … next non-attribute line is the
    // field: `name: Type,`.
    let mut found: Vec<(String, String)> = Vec::new();
    let lines: Vec<&str> = block.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let Some(dv) = line.split("default_value = \"").nth(1) else {
            continue;
        };
        let value = dv
            .split('"')
            .next()
            .expect("default_value literal is closed")
            .to_string();
        let field = lines[i + 1..]
            .iter()
            .find(|l| !l.trim_start().starts_with('#') && !l.trim_start().starts_with("///"))
            .and_then(|l| l.trim().split(':').next())
            .expect("a field follows the #[arg] attribute")
            .to_string();
        found.push((field, value));
    }
    found.sort();

    let expected = vec![
        ("contracts".to_string(), "on".to_string()),
        ("target".to_string(), "rust".to_string()),
    ];
    assert_eq!(
        found, expected,
        "the `transpile` subcommand's DEFAULTED options changed. Every \
         assertion in this file is stated relative to that set: `--target \
         rust` is the LANE SELECTOR (each lane below overrides it by design) \
         and `--contracts on` is the behaviour toggle every execution witness \
         must exercise. Update LANES and the deviation rules below in the \
         SAME commit, then re-pin this list."
    );
}

// ─── 2. the executed default-path differential ─────────────────────────────

/// THE LOAD-BEARING TEST. For every lane: the DEFAULT invocation emits, the
/// real downstream toolchain accepts what it emitted, and the citation channel
/// the default turns ON is demonstrably live.
///
/// This is the assertion whose absence let PMAT-1405 ship.
#[test]
fn default_emit_is_accepted_by_each_lane_toolchain() {
    let mut ran = Vec::new();
    let mut skipped = Vec::new();

    for lane in LANES {
        let dir = scratch(lane.target);

        // (a) the DEFAULT invocation — deliberately NO `--contracts`, so a
        //     flip of the default VALUE is caught by clause (c) below rather
        //     than papered over by naming it explicitly.
        let (ok, default_out, err) = emit(lane, &dir, &[]);
        assert!(
            ok,
            "`xpile transpile --target {}` with DEFAULT flags must emit. \
             stderr:\n{err}",
            lane.target
        );
        assert!(
            !default_out.trim().is_empty(),
            "`--target {}` exited 0 with EMPTY stdout — an exit code is not \
             an artifact",
            lane.target
        );

        // (b) the citation channel is live, per the lane's measured posture.
        let (off_ok, off_out, off_err) = emit(lane, &dir, &["--contracts", "off"]);
        assert!(
            off_ok,
            "`--target {} --contracts off` must also emit. stderr:\n{off_err}",
            lane.target
        );
        match lane.citations {
            Citations::Inline => {
                assert!(
                    default_out.contains(CITATION_MARKER),
                    "`--target {}`: the DEFAULT emit must carry `{CITATION_MARKER}`. \
                     A lane that stopped citing entirely would pass the \
                     toolchain check below while silently dropping the \
                     contract channel. Emitted:\n{default_out}",
                    lane.target
                );
                assert!(
                    !off_out.contains(CITATION_MARKER),
                    "`--target {} --contracts off` must STRIP `{CITATION_MARKER}`. \
                     If it does not, the flag is dead and the assertion above \
                     is passing for a reason unrelated to the default.",
                    lane.target
                );
            }
            Citations::StructuralOnly => {
                assert_eq!(
                    default_out, off_out,
                    "`--target {}` is recorded as StructuralOnly — `--contracts \
                     on|off` must produce BYTE-IDENTICAL text. It no longer \
                     does, which means this lane started inlining citations: \
                     a real improvement, but flip it to `Citations::Inline` \
                     here (and re-read the module doc's honest-scope note, \
                     which says 8 of 9 lanes inline) in the same commit.",
                    lane.target
                );
                assert!(
                    !default_out.contains(CITATION_MARKER),
                    "`--target {}` is recorded as StructuralOnly but its \
                     DEFAULT emit carries `{CITATION_MARKER}` — same \
                     re-classification applies.",
                    lane.target
                );
            }
        }

        // (c) the DEFAULT emit is READ by the lane's real toolchain.
        let Some(tool) = &lane.tool else {
            skipped.push(format!("{} (no external oracle)", lane.target));
            continue;
        };
        if !tool_present(tool.bin) {
            eprintln!(
                "XPILE-DEFAULTFLAGS-001: skipping the `{}` lane's toolchain \
                 check — `{}` is absent. The emit and citation assertions \
                 above STILL RAN; only the downstream oracle is skipped.",
                lane.target, tool.bin
            );
            skipped.push(format!("{} (no {})", lane.target, tool.bin));
            continue;
        }

        let artifact = dir.join(format!("default.{}", lane.out_ext));
        std::fs::write(&artifact, &default_out).expect("write emitted artifact");
        // `__PRE__`-prefixed argv entries go BEFORE the file (subcommands and
        // flags like `sh -n <f>` / `forjar validate -f <f>`); the rest after.
        let mut argv: Vec<String> = tool
            .argv
            .iter()
            .filter_map(|a| a.strip_prefix("__PRE__").map(str::to_string))
            .collect();
        argv.push(artifact.to_string_lossy().into_owned());
        argv.extend(
            tool.argv
                .iter()
                .filter(|a| !a.starts_with("__PRE__"))
                .map(|a| (*a).to_string()),
        );
        let out = Command::new(tool.bin)
            .args(&argv)
            .current_dir(&dir)
            .output()
            .unwrap_or_else(|e| panic!("spawn {}: {e}", tool.bin));
        assert!(
            out.status.success(),
            "`--target {}`: the DEFAULT emit is REJECTED by `{}` — this is \
             the PMAT-1405 defect, in the {} lane. `xpile` exited 0; the \
             lane's own toolchain cannot read what it wrote.\n\
             {} argv: {argv:?}\nstdout:\n{}\nstderr:\n{}\nemitted:\n{default_out}",
            lane.target,
            tool.bin,
            lane.target,
            tool.bin,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        ran.push(lane.target);
    }

    eprintln!("XPILE-DEFAULTFLAGS-001: toolchain-verified lanes: {ran:?}; skipped: {skipped:?}");
    assert!(
        !ran.is_empty(),
        "no lane's toolchain check actually ran — every oracle was absent, so \
         this test degenerated to an emit-only check. That is the skip-as-green \
         signature; install at least one of rustc/wat2wasm/lean/ruchy/ptxas/\
         naga/sh/forjar."
    );
}

/// The lane table is checked against the LIVE binary's own target list — parsed
/// out of its rejection message, not copied from a doc comment. A tenth backend
/// cannot be registered without a lane above.
///
/// PMAT-1435: the message now carries a second `; aliases: <s>=<canonical>, …`
/// section, and this gate reads only the `choose:` half. That is deliberate,
/// and it is the ONE place in the repo where skipping the aliases is right: an
/// alias resolves to the same `Target` as its canonical spelling, so it has no
/// distinct DEFAULT-flag path to gate, and
/// `target_spelling_disposition_witness.rs::every_alias_is_byte_identical_to_its_canonical_spelling`
/// proves the two are indistinguishable on stdout, stderr and exit status. A
/// lane per alias would be duplication that could only ever fail with its
/// canonical twin. If that byte-identity check is ever removed, this exemption
/// stops being justified and the aliases must be folded in here.
#[test]
fn lane_table_covers_every_target_the_binary_offers() {
    let dir = scratch("targetlist");
    let src = dir.join("k.py");
    std::fs::write(&src, PY_SRC).expect("write source");
    let out = Command::new(xpile_bin())
        .args([
            "transpile",
            src.to_str().unwrap(),
            "--target",
            "__no_such_target__",
        ])
        .output()
        .expect("spawn xpile");
    assert!(
        !out.status.success(),
        "an unknown `--target` must be refused, not silently accepted"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    let list = stderr
        .split("choose: ")
        .nth(1)
        .unwrap_or_else(|| {
            panic!(
                "the unknown-target error must enumerate the targets after \
                 `choose: ` (this gate derives the universe from it rather \
                 than hand-listing). Got:\n{stderr}"
            )
        })
        .split(';') // drop the `aliases:` section — see the doc comment
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches('.');
    let mut offered: Vec<&str> = list.split(',').map(str::trim).collect();
    offered.sort_unstable();
    assert!(
        offered.len() >= 9,
        "parsed only {} targets from the binary's own list — the parse is \
         probably broken, not the binary. Got: {offered:?}",
        offered.len()
    );

    let mut covered: Vec<&str> = LANES.iter().map(|l| l.target).collect();
    covered.sort_unstable();
    assert_eq!(
        covered, offered,
        "LANES must cover exactly the targets `xpile transpile --target` \
         offers. A backend with no lane here has its DEFAULT-flag path \
         ungated — which is precisely how PMAT-1405 shipped."
    );
}

// ─── 3. the class gate over the test corpus ────────────────────────────────

/// Marker a deviating test file must carry.
const DEVIATION_MARKER: &str = "XPILE-DEFAULT-FLAGS: DEVIATES";
/// …and the covering-witness pointer within it.
const COVERED_BY: &str = "DEFAULT-PATH-COVERED-BY:";

/// Walk every `tests/` source under `crates/`.
fn test_sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    let crates = repo_root().join("crates");
    for e in std::fs::read_dir(&crates).expect("read crates/").flatten() {
        let t = e.path().join("tests");
        if t.is_dir() {
            walk(&t, &mut out);
        }
    }
    out.sort();
    out
}

/// Extract the `--contracts` VALUES a source passes on the CLI. Handles the
/// two-element `.args(["--contracts", "off"])` form and the joined
/// `--contracts=off` form. `--contracts-dir` (a different, unrelated flag on
/// the reporter subcommands) is excluded.
fn contracts_values(src: &str) -> Vec<String> {
    let lines: Vec<&str> = src.lines().collect();
    let mut vals = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some(rest) = line.split("--contracts=").nth(1) {
            if let Some(v) = rest.split(['"', ' ', '\\']).next() {
                if !v.is_empty() {
                    vals.push(v.to_string());
                }
            }
        }
        // `"--contracts",` as an argv element — the value is the next string
        // literal, which the formatter always puts on the following line.
        if line.contains("\"--contracts\"") {
            let next = lines.get(i + 1).copied().unwrap_or("");
            if let Some(v) = next.split('"').nth(1) {
                vals.push(v.to_string());
            }
        }
    }
    vals
}

/// Any test that CLI-invokes `transpile` with a non-default `--contracts` must
/// DECLARE the deviation and NAME a default-path witness that exists and
/// actually probes the same target with default flags.
///
/// Lean is the one standing deviation: `lean_elaborate_witness.rs` is the
/// annotation-free semantic oracle by design, and PMAT-1405 added
/// `lean_default_emit_witness.rs` to cover the default path. This gate makes
/// that pairing mandatory for the next lane instead of remembered.
#[test]
fn every_contracts_deviation_in_a_test_is_declared_and_covered() {
    let files = test_sources();
    assert!(
        files.len() >= 60,
        "the test-source walk found only {} files — the walk is broken, and a \
         broken walk certifies an empty set. Expected the whole `crates/*/tests/` \
         corpus.",
        files.len()
    );

    // THE ENFORCER'S OWN EXEMPTION, stated rather than hidden. This file passes
    // `--contracts off` as the CONTROL arm of the differential in
    // `default_emit_is_accepted_by_each_lane_toolchain` — the arm that proves
    // the flag is live and that the default arm's citation assertion is not
    // passing for an unrelated reason. It is exempt from the declare-and-cover
    // rule because it IS the default-path witness for all nine lanes.
    //
    // The exemption is not a hole: it is keyed on this file's own compiler-
    // supplied path (never a hand-typed string that could be pasted into
    // another file), and it is CONDITIONED on the default arm still existing.
    // Delete the default arm and keep the `off` arm, and the assert below reds.
    let self_rel = file!();
    let self_src = read(self_rel);
    // The needle is ASSEMBLED AT RUNTIME so that it never appears as a literal
    // anywhere in this file. Written the obvious way — a literal needle — this
    // guard passed VACUOUSLY: the literal also occurs in the assertion MESSAGE
    // below, so `self_src.contains(needle)` matched the message rather than the
    // call site, and deleting the default arm kept the gate green. That was
    // caught by running the red half, not by reading the code.
    let default_arm = format!("{}(lane, &dir, &[])", "emit");
    assert!(
        self_src.contains(&default_arm),
        "{self_rel} exempts itself from the deviation rule on the grounds that \
         it runs BOTH arms, but its DEFAULT arm (`{default_arm}`) is gone. \
         Then it passes `--contracts off` and nothing else — the exact posture \
         this gate exists to forbid. Restore the default arm or drop the \
         exemption."
    );

    let mut occurrences = 0usize;
    let mut deviating = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(repo_root()).unwrap_or(f);
        let src = std::fs::read_to_string(f).unwrap_or_default();
        let vals = contracts_values(&src);
        occurrences += vals.len();
        if rel == Path::new(self_rel) {
            continue;
        }
        if vals.iter().any(|v| v != "on") {
            deviating.push((f.clone(), src));
        }
    }
    assert!(
        occurrences > 0,
        "the scanner found ZERO `--contracts` occurrences across {} test \
         files. That is not plausible (the Lean lane's oracle passes one \
         deliberately) — the argv-shape parse has drifted, so this gate would \
         wave through every future deviation. Fix `contracts_values`, or, if a \
         deviation was legitimately removed, lower this guard in the same \
         commit with a note.",
        files.len()
    );

    for (path, src) in &deviating {
        let rel = path
            .strip_prefix(repo_root())
            .unwrap_or(path)
            .display()
            .to_string();
        assert!(
            src.contains(DEVIATION_MARKER),
            "{rel} passes a non-default `--contracts` but carries no \
             `{DEVIATION_MARKER}` declaration. An execution witness that runs \
             a flag set no user takes certifies a path nobody ships — that is \
             how PMAT-1405 (`--target lean`'s default emit did not parse) got \
             through the lane's own semantic oracle. Either drop the flag, or \
             declare the deviation and name a `{COVERED_BY} <path>` witness \
             that covers the default path."
        );
        let covered = src
            .split(COVERED_BY)
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .unwrap_or_else(|| {
                panic!("{rel} declares a deviation but names no `{COVERED_BY} <path>`")
            })
            .trim_end_matches(['.', ',']);
        let cover_path = repo_root().join(covered);
        assert!(
            cover_path.is_file(),
            "{rel} names `{covered}` as its default-path cover, but that file \
             does not exist. A pointer at nothing is worse than no pointer: it \
             reads as covered."
        );
        let cover_src = std::fs::read_to_string(&cover_path).expect("read the covering witness");
        assert!(
            contracts_values(&cover_src).iter().all(|v| v == "on"),
            "{covered} is named as {rel}'s DEFAULT-path cover, but it ALSO \
             passes a non-default `--contracts`. Then nothing covers the \
             default path and the declaration is circular."
        );
        // The cover must probe the SAME lane, not merely exist.
        let target = src
            .split(DEVIATION_MARKER)
            .nth(1)
            .and_then(|s| s.lines().next())
            .unwrap_or("");
        for lane in LANES {
            if target.contains(lane.target) {
                assert!(
                    cover_src.contains(lane.target),
                    "{covered} never mentions `{}` — it cannot be the \
                     default-path cover for {rel}'s {} deviation.",
                    lane.target,
                    lane.target
                );
            }
        }
    }

    eprintln!(
        "XPILE-DEFAULTFLAGS-001: scanned {} test sources, {occurrences} \
         `--contracts` occurrence(s), {} declared deviation(s).",
        files.len(),
        deviating.len()
    );
}
