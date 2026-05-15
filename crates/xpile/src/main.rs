//! xpile binary entry point.
//!
//! v0.1.0: prints what the default session has registered, exercising
//! the trait dispatch tables end-to-end. Real CLI parsing (clap) lands
//! in Phase 1 — see `docs/specifications/sub/cli.md`.
//!
//! Released to crates.io as a v0.0.1 name reservation; v0.1.0+ is the
//! real binary tracked in this workspace.

fn main() -> anyhow::Result<()> {
    println!("xpile — polyglot transpile workbench (scaffold)");
    println!();

    let session = xpile_core::default_session();

    println!("Code lane:");
    println!("  frontends ({}):", session.frontends.len());
    for f in &session.frontends {
        println!("    - {} ({})", f.name(), f.extensions().join(", "));
    }
    println!("  backends ({}):", session.backends.len());
    for b in &session.backends {
        let targets: Vec<String> = b.targets().iter().map(|t| format!("{:?}", t)).collect();
        println!("    - {} → {}", b.name(), targets.join(", "));
    }

    println!();
    println!("Proof lane:");
    println!(
        "  contract_frontends ({}):",
        session.contract_frontends.len()
    );
    for cf in &session.contract_frontends {
        let fmts: Vec<String> = cf.formats().iter().map(|f| format!("{:?}", f)).collect();
        println!("    - {} ← {}", cf.name(), fmts.join(", "));
    }
    println!("  contract_backends ({}):", session.contract_backends.len());
    for cb in &session.contract_backends {
        let fmts: Vec<String> = cb.formats().iter().map(|f| format!("{:?}", f)).collect();
        println!("    - {} → {}", cb.name(), fmts.join(", "));
    }

    println!();
    println!("Run `xpile transpile <path>` once Phase 1 lands (CLI wiring).");
    Ok(())
}
