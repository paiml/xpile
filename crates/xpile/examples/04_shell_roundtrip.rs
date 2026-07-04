//! Example 04: POSIX shell round-trip.
//!
//! Demonstrates:
//! - Same-language transpile (shell in → shell out) via the
//!   bashrs frontend + bashrs backend.
//! - The emitted shell carries `#!/bin/sh`, an `xpile-bashrs-backend`
//!   provenance line, and a `# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE`
//!   citation.
//! - The supported POSIX subset is locked in by the contract.
//!
//! Run:   cargo run --example 04_shell_roundtrip -p xpile

use std::path::PathBuf;
use xpile_backend::{BackendConfig, Profile, Target};

fn main() -> anyhow::Result<()> {
    let session = xpile_core::default_session();

    let input: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "examples",
        "inputs",
        "install.sh",
    ]
    .iter()
    .collect();

    let source = std::fs::read_to_string(&input)?;
    println!("─── INPUT  ({}) ───\n{}", input.display(), source);

    let frontend = session
        .frontends
        .iter()
        .find(|f| f.matches_path(&input))
        .expect("bashrs frontend");
    let module = frontend.parse_and_lower(&input, &source)?;

    let backend = session
        .backends
        .iter()
        .find(|b| b.targets().contains(&Target::Shell))
        .expect("bashrs backend");
    let cfg = BackendConfig {
        emit_contracts: true,
        target: Target::Shell,
        profile: Profile::RustOut,
        hardware: None,
    };
    let artifact = backend.lower(&module, &cfg)?;

    println!("─── OUTPUT (target=shell) ───\n{}", artifact.primary);

    println!("─── WHAT THIS DEMONSTRATES ───");
    println!("• Same-language round-trip: shell in → shell out via meta-HIR.");
    println!("• bashrs-frontend recognized: variable assignment, expansion, `mkdir -p`, redirect.");
    println!("• bashrs-backend re-emitted each statement with provenance + contract citation.");
    println!("• Governing contract: C-BASHRS-POSIX-IDEMPOTENCE (Layer-1 / code lane / pattern).");
    Ok(())
}
