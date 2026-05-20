//! Example 05: Cross-domain — Python `subprocess.run([...])` → POSIX shell.
//!
//! Demonstrates:
//!   - The bashrs merger's load-bearing demonstration: a Python
//!     source file with `subprocess.run([...])` calls lowers cleanly
//!     into shell `Cmd` statements in meta-HIR, then emits real
//!     POSIX shell via bashrs-backend.
//!   - This is the cross-domain flow that motivated the bashrs IR
//!     merger (PMAT-037..119 in CHANGELOG).
//!
//! Run:   cargo run --example 05_python_to_shell -p xpile

use std::path::PathBuf;
use xpile_backend::{BackendConfig, Profile, Target};

fn main() -> anyhow::Result<()> {
    let session = xpile_core::default_session();

    let input: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "examples",
        "inputs",
        "subprocess_build.py",
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
        .find(|b| b.targets().contains(&Target::Shell))
        .expect("bashrs backend");
    let cfg = BackendConfig {
        target: Target::Shell,
        profile: Profile::RustOut,
        hardware: None,
    };
    let artifact = backend.lower(&module, &cfg)?;

    println!("─── OUTPUT (target=shell) ───\n{}", artifact.primary);

    println!("─── WHAT THIS DEMONSTRATES ───");
    println!("• Cross-domain transpile: Python source, shell target.");
    println!("• depyler-frontend recognized `subprocess.run([...])` and lowered each call");
    println!("  to a `Stmt::Cmd` in the meta-HIR.");
    println!("• bashrs-backend consumed those `Cmd` nodes and emitted real POSIX shell.");
    println!("• This is THE load-bearing demonstration of the v0.1.0 bashrs IR merger");
    println!("  (PMAT-037..119). Without it, the merger's v0.3.0 check-back would force");
    println!("  an unmerge per the falsifier XPILE-UNMERGE-001.");
    Ok(())
}
