//! Example 03: Python → Ruchy (third real backend).
//!
//! Demonstrates:
//!   - A frontend (Python) and a backend (Ruchy) that have no
//!     direct dependency on each other — they both speak meta-HIR.
//!   - Ruchy is Rust-flavoured but with `fun` and slightly different
//!     ergonomics; it still discharges C-PY-INT-ARITH the same way
//!     (`.checked_*().expect(...)`).
//!
//! Run:   cargo run --example 03_python_to_ruchy -p xpile

use std::path::PathBuf;
use xpile_backend::{BackendConfig, Profile, Target};

fn main() -> anyhow::Result<()> {
    let session = xpile_core::default_session();

    let input: PathBuf = [env!("CARGO_MANIFEST_DIR"), "examples", "inputs", "gcd.py"]
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
        .find(|b| b.targets().contains(&Target::Ruchy))
        .expect("ruchy backend");
    let cfg = BackendConfig {
        emit_contracts: true,
        target: Target::Ruchy,
        profile: Profile::RustOut,
        hardware: None,
    };
    let artifact = backend.lower(&module, &cfg)?;

    println!("─── OUTPUT (target=ruchy) ───\n{}", artifact.primary);

    println!("─── WHAT THIS DEMONSTRATES ───");
    println!("• Euclidean GCD (a different shape from factorial — mutual recursion via `%`).");
    println!("• Python `%` lowers to `.checked_rem_euclid()` — Python-floor, NOT C-truncating.");
    println!(
        "• Same C-PY-INT-ARITH discharge as Rust backend; backends compose by sharing meta-HIR."
    );
    Ok(())
}
