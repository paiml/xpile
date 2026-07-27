//! XPILE-WITNESS (Lean lane) — PMAT-1405: the DEFAULT `--target lean` emit
//! elaborates, and its contract citation is resolvable BY DECLARATION NAME out
//! of Lean's own elaborated environment.
//!
//! WHAT WAS WRONG. `--contracts on` is the CLI DEFAULT. Under it the Lean CODE
//! lane emitted `@[xpile_contract "C-…"]`, and `xpile_contract` is a registered
//! Lean attribute NOWHERE — so `lean` rejected the file with a PARSE error
//! (`unexpected token; expected ']'`) while `xpile` exited 0. `--target lean`
//! was the only backend whose DEFAULT output its own toolchain could not read.
//!
//! WHY IT SURVIVED. The defect was known and written down in three places
//! (`xpile-lean-codegen`'s doc comment, `audit-design.md` §7, README's caveat)
//! and gated in ZERO. `lean_elaborate_witness.rs` — the Lean lane's semantic
//! oracle — passes `--contracts off` deliberately, so the corpus certified a
//! flag combination the default invocation never takes. Prose recorded the
//! defect; nothing held it in place, and nothing would have noticed it getting
//! worse. That gap is what this file closes.
//!
//! WHAT THIS ASSERTS, on the DEFAULT emit (no `--contracts` flag at all, so a
//! change to the default VALUE is caught here too):
//!
//!   1. ELABORATES — `lean` accepts the emitted file with `by decide`
//!      obligations appended, so the citation form does not break the semantic
//!      oracle it sits next to.
//!   2. CITES — the emitted text actually carries the citation. Without this,
//!      deleting `emit_contract_citations` outright would pass assertion 1.
//!   3. RESOLVES BY NAME — a second Lean file `import`s nothing, re-declares the
//!      emitted def under its docstring, and asks Lean's own API
//!      (`Lean.findDocString? env `f`) for the citation. This is the structured
//!      claim the `@[xpile_contract …]` attribute was chosen to make and never
//!      delivered: it is now MEASURED through the elaborator rather than by a
//!      regex over the source. A line comment would fail this assertion, which
//!      is precisely why the fix is a docstring and not a comment.
//!
//! Skips with reason when `lean` / the xpile bin is absent (the hosted
//! workspace-test runner has no Lean toolchain) — never silently green.
//!
//! SCOPE: the Lean CODE lane (`--target lean`). The CONTRACT-RENDERING lane
//! (`xpile-lean-contract-backend`) keeps `@[xpile_contract …]`, which is
//! specified by `contracts/xlate-rust-fn-to-lean-thm-v1.yaml` and
//! `contracts/xpile-contract-backend-trait-v1.yaml` and is never elaborated as a
//! live attribute.

use std::process::Command;

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

fn lean_present() -> bool {
    Command::new("lean")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A per-CALL unique directory. Two probes sharing one directory have produced
/// cross-test clobbering in this repo before.
fn probe_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir()
        .join("xpile-lean-default-emit-witness")
        .join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir probe dir");
    dir
}

/// Emit Lean for `py` with the CLI's DEFAULT flags. Deliberately passes no
/// `--contracts` argument: if the default ever flips to `off`, assertion 2
/// below reds rather than this witness quietly certifying the annotation-free
/// path that `lean_elaborate_witness.rs` already covers.
fn emit_default(dir: &std::path::Path, py: &str) -> String {
    let src = dir.join("src.py");
    std::fs::write(&src, py).expect("write py");
    let out = Command::new(xpile_bin())
        .args(["transpile", src.to_str().unwrap(), "--target", "lean"])
        .output()
        .expect("spawn xpile");
    assert!(
        out.status.success(),
        "xpile --target lean (DEFAULT flags) must emit: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn run_lean(file: &std::path::Path) -> Result<(), String> {
    let out = Command::new("lean")
        .arg(file)
        .output()
        .map_err(|e| format!("spawn lean: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "lean rejected the file:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout).trim(),
        String::from_utf8_lossy(&out.stderr).trim()
    ))
}

/// (name, python source, `by decide` obligations over the emitted defs).
const DEFAULT_EMIT_CORPUS: &[(&str, &str, &[&str])] = &[
    (
        "add",
        "def add(a: int, b: int) -> int:\n    return a + b\n",
        &["add 3 4 = 7", "add (-2) 5 = 3"],
    ),
    (
        "lin",
        "def lin(x: int) -> int:\n    return 2 * x + 1\n",
        &["lin 4 = 9"],
    ),
    (
        "cmp",
        "def cmp(a: int) -> bool:\n    return a > 1\n",
        &["cmp 5 = true", "cmp 0 = false"],
    ),
];

/// ASSERTIONS 1 + 2: the DEFAULT emit elaborates AND carries its citation.
///
/// RED-CHECK: restoring `@[xpile_contract "{id}"]` in
/// `xpile-lean-codegen::emit_contract_citations` reds every case here with
/// `unexpected token; expected ']'` — the exact historical failure.
#[test]
fn default_lean_emit_elaborates_with_its_contract_citation() {
    if !lean_present() {
        eprintln!(
            "warning: `lean` not on PATH; skipping the PMAT-1405 default-emit witness. \
             Install the Lean toolchain (elan) to run it."
        );
        return;
    }

    let mut failures: Vec<String> = Vec::new();
    let mut elaborated = 0usize;
    for (name, py, proofs) in DEFAULT_EMIT_CORPUS {
        let dir = probe_dir(name);
        let emitted = emit_default(&dir, py);

        // ASSERTION 2 — the emit is actually CITED. Without this, deleting the
        // citation entirely would satisfy assertion 1 vacuously.
        if !emitted.contains("xpile-contract:") {
            failures.push(format!(
                "{name}: DEFAULT emit carries no `xpile-contract:` citation — \
                 the elaboration below would then prove nothing:\n{emitted}"
            ));
            continue;
        }

        // ASSERTION 1 — it elaborates, with the citation still in the file.
        let mut lean_src = emitted.clone();
        lean_src.push('\n');
        for p in *proofs {
            lean_src.push_str(&format!("example : {p} := by decide\n"));
        }
        let file = dir.join("prog.lean");
        std::fs::write(&file, &lean_src).expect("write lean");
        match run_lean(&file) {
            Ok(()) => elaborated += 1,
            Err(e) => failures.push(format!("{name}: {e}\n--- emitted ---\n{lean_src}")),
        }
    }

    eprintln!(
        "XPILE lean-default-emit witness: {}/{} DEFAULT-flag emits elaborated with \
         their contract citation in place.",
        elaborated,
        DEFAULT_EMIT_CORPUS.len()
    );
    assert!(
        failures.is_empty(),
        "the DEFAULT `--target lean` emit must elaborate ({} failure(s)):\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// ASSERTION 3: the citation is STRUCTURED — retrievable by declaration name
/// from Lean's elaborated environment via `Lean.findDocString?`.
///
/// This is the property the `@[xpile_contract …]` attribute form existed to
/// provide and never did (no file carrying it ever elaborated, so no
/// environment ever held it). A line comment fails this test; the docstring
/// passes it. That asymmetry is the whole argument for the chosen form, so it
/// is measured here through Lean's own API rather than asserted in prose.
#[test]
fn default_lean_citation_is_resolvable_by_declaration_name() {
    if !lean_present() {
        eprintln!("warning: `lean` not on PATH; skipping the PMAT-1405 name-resolution witness.");
        return;
    }

    let dir = probe_dir("name-resolution");
    let emitted = emit_default(&dir, "def add(a: int, b: int) -> int:\n    return a + b\n");
    assert!(
        emitted.contains("xpile-contract:"),
        "DEFAULT emit must carry a citation to resolve:\n{emitted}"
    );

    // `import Lean` is needed for the REFLECTION probe only — never by the
    // emitted file itself, which stays standalone. The emitted text is
    // reproduced verbatim so the probe reads the SHIPPED bytes, not a
    // hand-written copy of them.
    //
    // FALSIFIED (measured, 2026-07-27): replacing the emitted `/-- … -/` with a
    // line comment `-- xpile-contract: …` makes this exact probe fail with
    // "no citation resolvable by name for `add`". That is the empirical answer
    // to `audit-design.md` §7's objection that a comment would abandon the
    // structured form — it would, and this test is what says so.
    let probe = format!(
        "import Lean\n\n\
         {emitted}\n\
         open Lean Elab Command in\n\
         run_cmd do\n\
         \x20 let env ← getEnv\n\
         \x20 match ← Lean.findDocString? env `add with\n\
         \x20 | some s =>\n\
         \x20     let parts := s.splitOn \"xpile-contract:\"\n\
         \x20     if parts.length == 2 then pure ()\n\
         \x20     else throwError \"docstring for `add` is not an xpile-contract citation: {{s}}\"\n\
         \x20 | none => throwError \"no citation resolvable by name for `add`\"\n"
    );
    let file = dir.join("probe.lean");
    std::fs::write(&file, &probe).expect("write probe");

    if let Err(e) = run_lean(&file) {
        panic!(
            "the emitted citation must be resolvable by declaration name through \
             Lean's own API (this is what makes it STRUCTURED rather than a \
             comment):\n{e}\n--- probe ---\n{probe}"
        );
    }
    eprintln!(
        "XPILE lean-default-emit witness: citation for `add` resolved by name via \
         Lean.findDocString?."
    );
}

/// The Lean lane must not regress to an unparseable citation form without this
/// file noticing, even where `lean` is unavailable. Fast, no toolchain needed:
/// asserts the DEFAULT emit does not carry the historical attribute spelling.
///
/// This runs on the hosted workspace-test runner, where `lean` is absent and the
/// two tests above skip — so the lane keeps a non-skipping gate in CI.
#[test]
fn default_lean_emit_does_not_use_the_unregistered_attribute_form() {
    let dir = probe_dir("attribute-form");
    let emitted = emit_default(&dir, "def add(a: int, b: int) -> int:\n    return a + b\n");
    assert!(
        !emitted.contains("@[xpile_contract"),
        "PMAT-1405: the Lean CODE lane must not emit `@[xpile_contract …]` — it is \
         registered as a Lean attribute nowhere, so `lean` rejects the file with a \
         PARSE error while xpile exits 0. Emitted:\n{emitted}"
    );
    assert!(
        emitted.contains("/-- xpile-contract:"),
        "PMAT-1405: the DEFAULT emit must carry its citation as a Lean docstring \
         (structured AND parseable). Emitted:\n{emitted}"
    );
}
