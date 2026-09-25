//! XPILE-BASHRS-IDEM-SEAM-001 (PMAT-469): the Lean idempotence model in
//! `contracts/lean/Bashrs.lean` checked against xpile's shipped shell emitter.
//!
//! `Shell.composition_idempotence_diamond` proves `run c (run c s) = run c s`
//! for the idempotent subset of a filesystem model, and
//! `append_is_not_idempotent` proves `>>` is outside it. That proof is about
//! the model. This witness ties the model to what xpile ships: for every pin
//! in the `BEGIN shell idempotence pins` block it
//!
//! 1. renders the pin's setup and command to shell text, and checks that the
//!    command text equals the text the Lean pin proves `render` produces;
//! 2. transpiles both through the real binary (`xpile transpile --target
//!    shell`), so the scripts that run are the emitter's output;
//! 3. runs the emitted setup once, then the emitted command once and twice, in
//!    an empty directory under `umask 022`, and compares the entry at the
//!    command's path to the model's `once` and `twice` observations;
//! 4. checks that `once == twice` exactly when the model says `idem = true`.
//!
//! The Lean build checks model = pin, and this test checks pin = shipped. No
//! CI job has both `lake` and `cargo`, so the proof meets the code at the
//! committed pins.
//!
//! Not covered: `assign` (a variable assignment) is in the model's subset but
//! leaves no filesystem trace, so it has no pin here. Timestamps are not
//! modelled, so the witness never compares an mtime.

use std::path::{Path, PathBuf};
use std::process::Command;

const LEAN: &str = "contracts/lean/Bashrs.lean";
const BEGIN: &str = "-- BEGIN shell idempotence pins (PMAT-469)";
const END: &str = "-- END shell idempotence pins (PMAT-469)";

#[derive(Debug, Clone, PartialEq)]
enum Cmd {
    MkdirP(String),
    Touch(String),
    Chmod(String, String),
    Write(String, String),
    Append(String, String),
}

impl Cmd {
    fn path(&self) -> &str {
        match self {
            Cmd::MkdirP(p) | Cmd::Touch(p) | Cmd::Chmod(_, p) => p,
            Cmd::Write(p, _) | Cmd::Append(p, _) => p,
        }
    }

    fn render(&self) -> String {
        match self {
            Cmd::MkdirP(p) => format!("mkdir -p {p}"),
            Cmd::Touch(p) => format!("touch {p}"),
            Cmd::Chmod(m, p) => format!("chmod {m} {p}"),
            Cmd::Write(p, c) => format!("printf {c} > {p}"),
            Cmd::Append(p, c) => format!("printf {c} >> {p}"),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Cmd::MkdirP(_) => "mkdirP",
            Cmd::Touch(_) => "touch",
            Cmd::Chmod(..) => "chmod",
            Cmd::Write(..) => "write",
            Cmd::Append(..) => "append",
        }
    }
}

#[derive(Debug, Clone)]
struct Pin {
    setup: Vec<Cmd>,
    cmd: Cmd,
    text: String,
    idem: bool,
    once: String,
    twice: String,
}

/// The quoted strings in `s`, in order. Pins hold no escaped quotes.
fn quoted(s: &str) -> Vec<String> {
    s.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// One Lean command term, e.g. `.chmod "700" "f"`.
fn parse_cmd(term: &str) -> Result<Cmd, String> {
    let term = term
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();
    let name = term
        .strip_prefix('.')
        .and_then(|t| t.split_whitespace().next())
        .ok_or_else(|| format!("not a command term: {term}"))?;
    let q = quoted(term);
    let arg = |i: usize| {
        q.get(i)
            .cloned()
            .ok_or_else(|| format!("`{term}` is missing argument {i}"))
    };
    Ok(match name {
        "mkdirP" => Cmd::MkdirP(arg(0)?),
        "touch" => Cmd::Touch(arg(0)?),
        "chmod" => Cmd::Chmod(arg(0)?, arg(1)?),
        "write" => Cmd::Write(arg(0)?, arg(1)?),
        "append" => Cmd::Append(arg(0)?, arg(1)?),
        other => return Err(format!("unknown command `{other}` in `{term}`")),
    })
}

/// `example : pin [SETUP] (CMD) = ("TEXT", BOOL, "ONCE", "TWICE") := by decide`
fn parse_pin(line: &str) -> Result<Pin, String> {
    let body = line
        .strip_prefix("example : pin [")
        .and_then(|l| l.strip_suffix(" := by decide"))
        .ok_or_else(|| format!("pin line not in the expected form: {line}"))?;
    let (setup, rest) = body
        .split_once("] ")
        .ok_or_else(|| format!("no setup list: {line}"))?;
    let (cmd, value) = rest
        .split_once(" = (")
        .ok_or_else(|| format!("no `= (` in: {line}"))?;
    let setup = if setup.trim().is_empty() {
        Vec::new()
    } else {
        setup
            .split(", .")
            .enumerate()
            .map(|(i, t)| {
                parse_cmd(&if i == 0 {
                    t.to_string()
                } else {
                    format!(".{t}")
                })
            })
            .collect::<Result<_, _>>()?
    };
    let q = quoted(value);
    let idem = if value.contains(", true, ") {
        true
    } else if value.contains(", false, ") {
        false
    } else {
        return Err(format!("no Bool in: {line}"));
    };
    if q.len() != 3 {
        return Err(format!("expected 3 strings in the value of: {line}"));
    }
    Ok(Pin {
        setup,
        cmd: parse_cmd(cmd)?,
        text: q[0].clone(),
        idem,
        once: q[1].clone(),
        twice: q[2].clone(),
    })
}

fn parse_block(src: &str) -> Result<Vec<Pin>, String> {
    let start = src.find(BEGIN).ok_or("no BEGIN marker")? + BEGIN.len();
    let end = src[start..].find(END).ok_or("no END marker")? + start;
    src[start..end]
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("--"))
        .map(parse_pin)
        .collect()
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn pins() -> Vec<Pin> {
    let src = std::fs::read_to_string(repo_root().join(LEAN)).expect("read Bashrs.lean");
    parse_block(&src).unwrap_or_else(|e| panic!("{LEAN}: {e}"))
}

/// Transpile `text` through the shipped binary and return the emitted script.
fn emit(dir: &Path, name: &str, text: &str) -> Result<PathBuf, String> {
    let src = dir.join(format!("{name}.sh"));
    std::fs::write(&src, format!("{text}\n")).map_err(|e| e.to_string())?;
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .args(["transpile", src.to_str().unwrap(), "--target", "shell"])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "xpile refused `{text}`: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let emitted = dir.join(format!("{name}.emitted.sh"));
    std::fs::write(&emitted, &out.stdout).map_err(|e| e.to_string())?;
    Ok(emitted)
}

fn run_sh(work: &Path, script: &Path) -> Result<(), String> {
    let out = Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("umask 022 && . '{}'", script.display()))
        .current_dir(work)
        .output()
        .map_err(|e| format!("spawn /bin/sh: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} failed: {}",
            script.display(),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

/// The entry at `p`, in the model's `obs` format.
fn observe(p: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    let Ok(md) = std::fs::metadata(p) else {
        return "absent".to_string();
    };
    let mode = format!("{:o}", md.permissions().mode() & 0o777);
    if md.is_dir() {
        format!("dir {mode}")
    } else {
        let content = std::fs::read_to_string(p).unwrap_or_default();
        format!("file {mode} {content}")
    }
}

/// Run one pin against the shipped emitter; `Err` names the disagreement.
fn check_pin(pin: &Pin, scratch: &Path) -> Result<(), String> {
    let label = pin.text.clone();
    if pin.cmd.render() != pin.text {
        return Err(format!(
            "{label}: the witness renders `{}`, the Lean pin says `{}`",
            pin.cmd.render(),
            pin.text
        ));
    }
    let _ = std::fs::remove_dir_all(scratch);
    let work = scratch.join("work");
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    if !pin.setup.is_empty() {
        let text: Vec<String> = pin.setup.iter().map(Cmd::render).collect();
        let setup = emit(scratch, "setup", &text.join("\n"))?;
        run_sh(&work, &setup)?;
    }
    let cmd = emit(scratch, "cmd", &pin.text)?;
    let target = work.join(pin.cmd.path());
    run_sh(&work, &cmd)?;
    let once = observe(&target);
    run_sh(&work, &cmd)?;
    let twice = observe(&target);
    if once != pin.once || twice != pin.twice {
        return Err(format!(
            "{label}: shipped once/twice = {once:?}/{twice:?}, model = {:?}/{:?}",
            pin.once, pin.twice
        ));
    }
    if (once == twice) != pin.idem {
        return Err(format!(
            "{label}: the model says idempotent = {}, the shipped script says {}",
            pin.idem,
            once == twice
        ));
    }
    Ok(())
}

fn scratch(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("bashrs-idem-seam-{name}"))
}

#[test]
fn every_lean_idempotence_pin_matches_the_shipped_emitter() {
    let pins = pins();
    assert!(!pins.is_empty(), "no pins parsed from {LEAN}");
    let failures: Vec<String> = pins
        .iter()
        .enumerate()
        .filter_map(|(i, p)| check_pin(p, &scratch(&i.to_string())).err())
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn the_pins_cover_every_filesystem_command_and_the_dual() {
    let pins = pins();
    assert!(!pins.is_empty(), "no pins parsed from {LEAN}");
    for kind in ["mkdirP", "touch", "chmod", "write", "append"] {
        assert!(
            pins.iter().any(|p| p.cmd.kind() == kind),
            "no pin exercises `{kind}`"
        );
    }
    assert!(
        pins.iter().any(|p| !p.idem && p.once != p.twice),
        "no pin shows a command outside the subset failing the law"
    );
    assert!(
        pins.iter().any(|p| p.idem && !p.setup.is_empty()),
        "no idempotent pin starts from a non-empty state"
    );
}

// ---- RED arms ----

#[test]
fn red_arm_a_wrong_idempotence_claim_is_caught_by_execution() {
    let mut pin = pins().into_iter().find(|p| !p.idem).expect("an append pin");
    pin.idem = true;
    pin.twice = pin.once.clone();
    let err = check_pin(&pin, &scratch("red-claim")).unwrap_err();
    assert!(err.contains("shipped once/twice"), "{err}");
}

#[test]
fn red_arm_a_wrong_model_observation_is_caught() {
    let mut pin = pins()
        .into_iter()
        .find(|p| p.cmd.kind() == "chmod")
        .expect("a chmod pin");
    pin.once = pin.once.replace("700", "755");
    let err = check_pin(&pin, &scratch("red-obs")).unwrap_err();
    assert!(err.contains("shipped once/twice"), "{err}");
}

#[test]
fn red_arm_a_render_disagreement_is_caught() {
    let mut pin = pins().into_iter().next().expect("a pin");
    pin.text.push_str(" extra");
    let err = check_pin(&pin, &scratch("red-render")).unwrap_err();
    assert!(err.contains("the Lean pin says"), "{err}");
}

#[test]
fn red_arm_a_malformed_pin_line_is_refused() {
    let src = format!("{BEGIN}\nexample : pin [] (.rm \"f\") = (\"rm f\", true, \"a\", \"a\") := by decide\n{END}\n");
    let err = parse_block(&src).unwrap_err();
    assert!(err.contains("unknown command `rm`"), "{err}");
}
