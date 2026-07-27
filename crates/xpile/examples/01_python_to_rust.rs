//! Example 01: Python → Rust, programmatically (no CLI).
//!
//! Demonstrates:
//!   - xpile-core's `default_session()` library API
//!   - Frontend dispatch by file extension
//!   - Backend dispatch by `Target` enum
//!   - The emitted Rust carries `// xpile-contract: C-PY-INT-ARITH`
//!     citation + `.checked_*().expect(...)` overflow guards
//!
//! Run:   cargo run --example 01_python_to_rust -p xpile
//! Reads: crates/xpile/examples/inputs/factorial.py

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
        .find(|b| b.targets().contains(&Target::Rust))
        .expect("rust backend");
    let cfg = BackendConfig {
        emit_contracts: true,
        target: Target::Rust,
        profile: Profile::RustOut,
        hardware: None,
    };
    let artifact = backend.lower(&module, &cfg)?;

    println!("─── OUTPUT (target=rust) ───\n{}", artifact.primary);

    println!("─── WHAT THIS DEMONSTRATES ───");
    println!("• Python frontend (depyler-frontend) parsed a recursive `def` with type hints.");
    println!("• meta-HIR lowered the body into structured Expr / Stmt nodes.");
    println!("• Rust backend (xpile-rust-codegen) emitted a `pub fn` with:");
    println!("    - `// xpile-contract: C-PY-INT-ARITH` citation");
    println!("    - `.checked_mul()` and `.checked_sub()` wrappers");
    println!("    - panic text NAMING the governing contract");
    // PMAT-1415: this example PRINTS a claim it does not check, so it names the
    // test that does. Through v0.1.617 the sentence read as a bare assertion
    // and nothing in CI compiled this emit at all.
    println!(
        "• `rustc -O`-clean, `factorial(10) == 3628800`, and `factorial(21)` panics \
         citing the contract — executed by crates/xpile/tests/readme_quickstart_witness.rs, \
         not by this example."
    );
    Ok(())
}
