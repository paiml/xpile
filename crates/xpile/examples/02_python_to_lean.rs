//! Example 02: Python → Lean 4 (proof-lane shadow).
//!
//! Demonstrates:
//!   - The same source file as 01_python_to_rust.rs targets a
//!     completely different backend
//!   - Lean's `Int` is unbounded, so C-PY-INT-ARITH is satisfied
//!     **by construction** — no `.checked_*()` calls emitted
//!   - The citation is `/-- xpile-contract: C-PY-INT-ARITH -/`, a Lean
//!     DOCSTRING — structured (recoverable by declaration name via
//!     `Lean.findDocString?`, not by a regex over the source) AND parseable,
//!     so the emit elaborates standalone.
//!
//!     PMAT-1405: this used to read `@[xpile_contract "C-PY-INT-ARITH"]`, "a
//!     real Lean attribute (NOT a comment) — required by
//!     C-XPILE-CONTRACT-BACKEND-TRAIT". Both halves were wrong. `xpile_contract`
//!     is registered as a Lean attribute nowhere, so `lean` rejected this
//!     example's own output with a PARSE error; and that contract's invariant is
//!     guarded by `config.format == LeanTheorem`, which is the contract-RENDERING
//!     lane (contract YAML → theorem text). Neither it nor
//!     C-XLATE-RUST-FN-TO-LEAN-THM mentions the CODE lane, so `--target lean`
//!     was never bound to the attribute form at all. The theorem lane keeps it.
//!
//! Run:   cargo run --example 02_python_to_lean -p xpile

use std::path::PathBuf;
use xpile_backend::{BackendConfig, Profile, Target};

fn main() -> anyhow::Result<()> {
    let session = xpile_core::default_session();

    let input: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "examples",
        "inputs",
        "factorial.py",
    ]
    .iter()
    .collect();

    let source = std::fs::read_to_string(&input)?;
    println!("─── INPUT  ({}) ───\n{}", input.display(), source);

    let frontend = session
        .frontends
        .iter()
        .find(|f| f.matches_path(&input))
        .expect("python frontend");
    let module = frontend.parse_and_lower(&input, &source)?;

    let backend = session
        .backends
        .iter()
        .find(|b| b.targets().contains(&Target::Lean))
        .expect("lean backend");
    let cfg = BackendConfig {
        emit_contracts: true,
        target: Target::Lean,
        profile: Profile::RustOut,
        hardware: None,
    };
    let artifact = backend.lower(&module, &cfg)?;

    println!("─── OUTPUT (target=lean) ───\n{}", artifact.primary);

    println!("─── WHAT THIS DEMONSTRATES ───");
    println!("• Same Python source → completely different target language.");
    println!(
        "• `/-- xpile-contract: C-PY-INT-ARITH -/` is a Lean docstring: structured\n  \
         (resolvable by name via `Lean.findDocString?`) AND parseable, so this\n  \
         output elaborates as-is."
    );
    println!("• Lean's `Int` is unbounded, so no `.checked_*()` overflow guards are needed");
    println!("  — the contract is satisfied BY CONSTRUCTION.");
    println!("• Two backends, same contract ID — the citation graph stays joinable.");
    Ok(())
}
