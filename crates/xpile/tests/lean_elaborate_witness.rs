//! XPILE-WITNESS (Lean lane) — the Lean backend's FIRST *semantic* witness.
//!
//! The roadmap-audit flagged the Lean backend (like ruchy/forjar) as
//! string-compare-only. This upgrades it to an ELABORATION + EVALUATION oracle:
//! for a corpus of value functions it
//!
//!   1. `xpile transpile --target lean --contracts off`  → emitted Lean,
//!   2. appends a proof obligation per function — `example : f args = v := by
//!      decide` — where `decide` *evaluates* the emitted definition, and
//!   3. runs `lean` on the whole file, asserting it elaborates (exit 0).
//!
//! Because `by decide` reduces the definition, a WRONG emission fails the proof:
//! `add 3 4 = 7` elaborates, but a `+`→`-` mutation in the Lean emitter makes
//! `add 3 4` reduce to `-1` and `lean` reports *"decide proved add 3 4 = 7 is
//! false"* → RED. So this is a genuine semantic check (via Lean's decision
//! procedure), not merely a type-check, and strictly stronger than a
//! string-compare against frozen expected text.
//!
//! HISTORICAL FINDING, RESOLVED BY PMAT-1405 (2026-07-27). This witness passes
//! `--contracts off` because, through v0.1.617, the DEFAULT (`--contracts on`)
//! emit could not elaborate: the Lean code lane cited via `@[xpile_contract
//! "C-…"]`, `xpile_contract` was a registered Lean attribute nowhere, and `lean`
//! rejected the file with *"unexpected token; expected ']'"* while `xpile`
//! exited 0.
//!
//! That is fixed — the lane now cites with a Lean DOCSTRING
//! (`/-- xpile-contract: … -/`), which parses AND is resolvable by declaration
//! name via `Lean.findDocString?`, so it keeps the structured property the
//! attribute was chosen for (a plain line comment does NOT — measured).
//!
//! This witness deliberately KEEPS `--contracts off`: it is the annotation-free
//! lane's oracle, and the two lanes are now covered separately.
//! `crates/xpile/tests/lean_default_emit_witness.rs` owns the DEFAULT path and
//! is what would catch a regression there — this file would not, which is
//! exactly why the defect above lived for three weeks with the corpus green.
//! See `docs/specifications/audit-design.md` §7.
//!
//! SCOPE: the Lean lane lowers only VALUE functions (arithmetic / comparison /
//! bool) — it refuses `None`-returning (void) functions, statement-form
//! `if`/`else` (Lean uses the if-EXPRESSION form), and unproven recursion (Lean's
//! termination checker rejects it). The corpus stays within that surface.
//!
//! Skips with reason when `lean` / the xpile bin is absent (hosted CI has no
//! Lean toolchain on the workspace-test runner) — never silently green.

use std::process::Command;

/// (fixture name, Python source, proof obligations over the emitted defs).
/// Each obligation is a Lean proposition proved by `decide`, so it both
/// type-checks AND evaluates the emitted definition to the expected value.
struct LeanCase {
    name: &'static str,
    py: &'static str,
    proofs: &'static [&'static str],
}

const LEAN_VALUE_CORPUS: &[LeanCase] = &[
    LeanCase {
        name: "add",
        py: "def add(a: int, b: int) -> int:\n    return a + b\n",
        proofs: &["add 3 4 = 7", "add (-2) 5 = 3"],
    },
    LeanCase {
        name: "sub",
        py: "def sub(a: int, b: int) -> int:\n    return a - b\n",
        proofs: &["sub 10 3 = 7", "sub 1 4 = -3"],
    },
    LeanCase {
        name: "lin",
        py: "def lin(x: int) -> int:\n    return 2 * x + 1\n",
        proofs: &["lin 4 = 9", "lin 0 = 1"],
    },
    LeanCase {
        name: "sq",
        py: "def sq(x: int) -> int:\n    return x * x\n",
        proofs: &["sq 6 = 36"],
    },
    LeanCase {
        name: "is_zero",
        py: "def is_zero(n: int) -> bool:\n    return n == 0\n",
        proofs: &["is_zero 0 = true", "is_zero 5 = false"],
    },
    LeanCase {
        name: "lt",
        py: "def lt(a: int, b: int) -> bool:\n    return a < b\n",
        proofs: &["lt 2 5 = true", "lt 9 1 = false"],
    },
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

/// Emit Lean for `case.py`, append its proof obligations, elaborate. `Ok(())`
/// iff `lean` accepts the file (defs well-typed AND every `by decide` closes).
fn emit_and_elaborate(case: &LeanCase) -> Result<(), String> {
    let dir = std::env::temp_dir()
        .join("xpile-lean-witness")
        .join(case.name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    let py = dir.join("src.py");
    std::fs::write(&py, case.py).map_err(|e| format!("write py: {e}"))?;

    let emit = Command::new(xpile_bin())
        .args([
            "transpile",
            py.to_str().unwrap(),
            "--target",
            "lean",
            "--contracts",
            "off",
        ])
        .output()
        .map_err(|e| format!("spawn xpile: {e}"))?;
    if !emit.status.success() {
        return Err(format!(
            "xpile MUST emit Lean for value fn {}: {}",
            case.name,
            String::from_utf8_lossy(&emit.stderr).trim()
        ));
    }
    let mut lean_src = String::from_utf8_lossy(&emit.stdout).to_string();
    lean_src.push('\n');
    for p in case.proofs {
        // `example` (anonymous) keeps the file self-contained; `by decide`
        // evaluates the emitted def, so a wrong emission fails HERE.
        lean_src.push_str(&format!("example : {p} := by decide\n"));
    }
    let lean_file = dir.join("prog.lean");
    std::fs::write(&lean_file, &lean_src).map_err(|e| format!("write lean: {e}"))?;

    let out = Command::new("lean")
        .arg(&lean_file)
        .output()
        .map_err(|e| format!("spawn lean: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "lean rejected the emitted Lean + proof obligations:\n{}\n--- emitted ---\n{lean_src}",
            String::from_utf8_lossy(&out.stdout).trim()
        ));
    }
    Ok(())
}

#[test]
fn lean_backend_emits_elaborating_evaluated_lean() {
    if !tool_present("lean", "--version") {
        eprintln!(
            "warning: `lean` not on PATH; skipping the Lean elaboration witness. \
             Install the Lean toolchain (elan) to run it."
        );
        return;
    }

    let mut proven = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for case in LEAN_VALUE_CORPUS {
        match emit_and_elaborate(case) {
            Ok(()) => proven += 1,
            Err(e) => failures.push(format!("{}: {e}", case.name)),
        }
    }
    let obligations: usize = LEAN_VALUE_CORPUS.iter().map(|c| c.proofs.len()).sum();
    eprintln!(
        "XPILE lean-witness: {}/{} value functions emitted + elaborated + evaluated \
         ({} proof obligations discharged by `by decide`).",
        proven,
        LEAN_VALUE_CORPUS.len(),
        obligations
    );

    assert!(
        failures.is_empty(),
        "Lean elaboration witness found {} failing case(s):\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
    assert_eq!(
        proven,
        LEAN_VALUE_CORPUS.len(),
        "expected every corpus value function to emit-elaborate-evaluate"
    );
}

/// Self-documenting guard for the FINDING: the annotation-free emit path this
/// witness relies on must stay reachable. If `--contracts off` ever stops being
/// accepted, this fires (fast; no `lean` needed).
#[test]
fn lean_contracts_off_emit_path_exists() {
    let dir = std::env::temp_dir().join("xpile-lean-witness-probe");
    let _ = std::fs::create_dir_all(&dir);
    let py = dir.join("p.py");
    std::fs::write(&py, "def add(a: int, b: int) -> int:\n    return a + b\n").unwrap();
    let emit = Command::new(xpile_bin())
        .args([
            "transpile",
            py.to_str().unwrap(),
            "--target",
            "lean",
            "--contracts",
            "off",
        ])
        .output()
        .expect("spawn xpile");
    assert!(
        emit.status.success(),
        "xpile --target lean --contracts off must emit: {}",
        String::from_utf8_lossy(&emit.stderr).trim()
    );
    let src = String::from_utf8_lossy(&emit.stdout);
    assert!(
        src.contains("def add"),
        "expected `def add` in the annotation-free Lean emit, got:\n{src}"
    );
    assert!(
        !src.contains("@[xpile_contract"),
        "`--contracts off` must NOT emit the @[xpile_contract] attribute (it \
         breaks bare-`lean` elaboration — see the module finding), got:\n{src}"
    );
}
