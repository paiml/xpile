//! xpile binary — v0.0.1 crates.io name reservation.
//!
//! The real implementation is being built in the workspace at
//! <https://github.com/paiml/xpile> and will land in v0.1.0+. This
//! 0.0.1 release exists to reserve the `xpile` name on crates.io.

fn main() -> anyhow::Result<()> {
    println!("xpile — polyglot transpile workbench");
    println!();
    println!("Version: 0.0.1 (crates.io name reservation)");
    println!("The real CLI lands in v0.1.0+.");
    println!();
    println!("Source:  https://github.com/paiml/xpile");
    println!("Docs:    https://docs.rs/xpile");
    Ok(())
}
