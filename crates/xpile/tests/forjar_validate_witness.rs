//! XPILE-WITNESS (forjar lane) — validate emitted forjar.yaml with forjar's OWN
//! validator, not just a structural serde_yaml shape-check.
//!
//! The forjar backend's existing tests (`xpile-forjar-codegen/src/tests.rs`) parse
//! the emitted YAML with a generic YAML parser and assert the resource SHAPE. That
//! misses schema errors forjar itself would reject — exactly the gap this witness
//! closes: for a corpus of shell inputs it runs
//!
//!   xpile transpile <shell> --target forjar  →  forjar.yaml  →  `forjar validate -f`
//!
//! and asserts forjar ACCEPTS the config. This caught a real bug (fixed in the
//! same change): the machine block emitted only `addr:`, but forjar's `Machine`
//! schema REQUIRES `hostname:` — so `forjar validate` failed with
//! *"machines.localhost: missing field `hostname`"* (exit 3) while the structural
//! test stayed green. A structural shape-check cannot catch a missing required
//! schema field; forjar's real validator can.
//!
//! Skips with reason when `forjar` / the xpile bin is absent (hosted CI has no
//! forjar install on the workspace-test runner) — never silently green.

use std::process::Command;

/// (name, shell source) — each covers a supported meta-HIR→forjar mapping:
/// bare command → task; multi-statement script → file + task; ShellAssign;
/// a command with arguments.
const FORJAR_SHELL_CORPUS: &[(&str, &str)] = &[
    ("bare_cmd", "echo hello\n"),
    ("two_cmds", "echo one\necho two\n"),
    ("assign", "GREETING=hello\necho done\n"),
    ("with_args", "cp src.txt dst.txt\n"),
];

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

fn tool_present(cmd: &str, arg: &str) -> bool {
    Command::new(cmd)
        .arg(arg)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Emit forjar.yaml for `shell` and run `forjar validate -f` on it. `Ok(())`
/// iff forjar's own validator accepts the emitted config.
fn emit_and_validate(name: &str, shell: &str) -> Result<(), String> {
    let dir = std::env::temp_dir().join("xpile-forjar-witness").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    let sh = dir.join("in.sh");
    std::fs::write(&sh, shell).map_err(|e| format!("write sh: {e}"))?;

    let emit = Command::new(xpile_bin())
        .args(["transpile", sh.to_str().unwrap(), "--target", "forjar"])
        .output()
        .map_err(|e| format!("spawn xpile: {e}"))?;
    if !emit.status.success() {
        return Err(format!(
            "xpile MUST emit forjar.yaml for {name}: {}",
            String::from_utf8_lossy(&emit.stderr).trim()
        ));
    }
    let yaml = dir.join("forjar.yaml");
    std::fs::write(&yaml, &emit.stdout).map_err(|e| format!("write yaml: {e}"))?;

    let out = Command::new("forjar")
        .arg("validate")
        .arg("-f")
        .arg(&yaml)
        .output()
        .map_err(|e| format!("spawn forjar: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "forjar validate REJECTED the emitted forjar.yaml:\n{}\n{}\n--- emitted ---\n{}",
            String::from_utf8_lossy(&out.stdout).trim(),
            String::from_utf8_lossy(&out.stderr).trim(),
            String::from_utf8_lossy(&emit.stdout)
        ));
    }
    Ok(())
}

#[test]
fn forjar_backend_emits_validator_accepted_yaml() {
    if !tool_present("forjar", "--version") {
        eprintln!(
            "warning: `forjar` not on PATH; skipping the forjar validation witness. \
             Install forjar (`cargo install forjar`) to run it."
        );
        return;
    }

    let mut validated = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for (name, shell) in FORJAR_SHELL_CORPUS {
        match emit_and_validate(name, shell) {
            Ok(()) => validated += 1,
            Err(e) => failures.push(format!("{name}: {e}")),
        }
    }
    eprintln!(
        "XPILE forjar-witness: {}/{} shell inputs emitted forjar.yaml that `forjar validate` ACCEPTS.",
        validated,
        FORJAR_SHELL_CORPUS.len()
    );

    assert!(
        failures.is_empty(),
        "forjar validation witness found {} rejected config(s):\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
    assert_eq!(
        validated,
        FORJAR_SHELL_CORPUS.len(),
        "expected every corpus shell input to emit validator-accepted forjar.yaml"
    );
}
