//! Example 06: Inspect the xpile session — what frontends/backends are wired?
//!
//! Demonstrates:
//!   - The `TranspileSession` is a programmable registry, not a
//!     hardcoded dispatch table.
//!   - You can ask the live session what it can read/emit without
//!     parsing CLI help — useful for tooling, IDE plugins, MCP
//!     servers (see `xpile-mcp` crate for one consumer).
//!
//! Run:   cargo run --example 06_inspect_session -p xpile

fn main() -> anyhow::Result<()> {
    let session = xpile_core::default_session();

    println!(
        "─── xpile session ({} frontends, {} backends) ───",
        session.frontends.len(),
        session.backends.len()
    );

    println!("\nCode lane:");
    println!("  Frontends (read source → meta-HIR):");
    for f in &session.frontends {
        println!("    - {:10}  extensions: {:?}", f.name(), f.extensions());
    }
    println!("  Backends (meta-HIR → emit target):");
    for b in &session.backends {
        let targets: Vec<String> = b.targets().iter().map(|t| format!("{t:?}")).collect();
        println!("    - {:10}  targets: {}", b.name(), targets.join(", "));
    }

    println!("\nProof lane:");
    println!("  ContractFrontends: {}", session.contract_frontends.len());
    for cf in &session.contract_frontends {
        println!("    - {}", cf.name());
    }
    println!("  ContractBackends:  {}", session.contract_backends.len());
    for cb in &session.contract_backends {
        println!("    - {}", cb.name());
    }

    println!("\n─── WHAT THIS DEMONSTRATES ───");
    println!("• `xpile_core::default_session()` returns a live registry of Arc<dyn Frontend>");
    println!("  and Arc<dyn Backend> — both trait objects, both dispatch dynamically.");
    println!("• Adding a frontend or backend is a single registration call");
    println!("  (see book chapter 'Adding a frontend' for the full flow).");
    println!("• The MCP server at xpile-mcp uses this same session to expose tools.");
    Ok(())
}
