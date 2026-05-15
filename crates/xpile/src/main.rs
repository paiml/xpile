//! xpile binary entry point.
//!
//! v0.1.0 CLI surface:
//!   xpile transpile <input> [--target <t>] [--out <path>]
//!   xpile info     (default if no subcommand)
//!
//! Dispatch goes through [`xpile_core::default_session`]: file extension
//! selects the frontend; `--target` selects the backend.
//!
//! Released to crates.io as a v0.0.1 name reservation; v0.1.0+ is the
//! real binary tracked in this workspace.

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use xpile_backend::{BackendConfig, Profile, Target};
use xpile_core::TranspileSession;

#[derive(Parser)]
#[command(name = "xpile", version, about = "Polyglot transpile workbench")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show registered frontends and backends (default).
    Info,
    /// Transpile a source file. The file extension selects the frontend;
    /// `--target` selects the backend.
    Transpile {
        /// Path to the source file (e.g., `add.py`, `kernel.c`).
        input: PathBuf,
        /// Target backend: rust | ruchy | ptx | wgsl | spirv | lean.
        #[arg(long, default_value = "rust")]
        target: String,
        /// Output path. Defaults to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let session = xpile_core::default_session();
    match cli.cmd.unwrap_or(Cmd::Info) {
        Cmd::Info => print_info(&session),
        Cmd::Transpile { input, target, out } => {
            transpile(&session, &input, &target, out.as_deref())
        }
    }
}

fn print_info(session: &TranspileSession) -> Result<()> {
    println!("xpile — polyglot transpile workbench");
    println!();

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
    Ok(())
}

fn transpile(
    session: &TranspileSession,
    input: &Path,
    target_str: &str,
    out: Option<&Path>,
) -> Result<()> {
    let source =
        std::fs::read_to_string(input).with_context(|| format!("reading {}", input.display()))?;

    let ext = input
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let frontend = session
        .frontends
        .iter()
        .find(|f| f.extensions().contains(&ext))
        .with_context(|| {
            let known: Vec<&'static str> = session
                .frontends
                .iter()
                .flat_map(|f| f.extensions().iter().copied())
                .collect();
            format!("no frontend handles extension `.{ext}`; known: {known:?}")
        })?;

    let module = frontend
        .parse_and_lower(input, &source)
        .with_context(|| format!("parse_and_lower failed for {}", input.display()))?;

    let target = parse_target(target_str)?;
    let backend = session
        .backends
        .iter()
        .find(|b| b.targets().contains(&target))
        .with_context(|| {
            let known: Vec<String> = session
                .backends
                .iter()
                .flat_map(|b| b.targets().iter().map(|t| format!("{t:?}")))
                .collect();
            format!("no backend for target {target:?}; known: {known:?}")
        })?;

    let config = BackendConfig {
        target,
        profile: Profile::RustOut,
        hardware: None,
    };

    let artifact = backend
        .lower(&module, &config)
        .with_context(|| format!("backend `{}` failed", backend.name()))?;

    match out {
        Some(path) => {
            std::fs::write(path, &artifact.primary)
                .with_context(|| format!("writing {}", path.display()))?;
            eprintln!("xpile: wrote {}", path.display());
        }
        None => print!("{}", artifact.primary),
    }
    Ok(())
}

fn parse_target(s: &str) -> Result<Target> {
    Ok(match s {
        "rust" => Target::Rust,
        "ruchy" => Target::Ruchy,
        "ptx" => Target::Ptx,
        "wgsl" => Target::Wgsl,
        "spirv" => Target::Spirv,
        "lean" => Target::Lean,
        other => bail!("unknown target `{other}`; choose: rust, ruchy, ptx, wgsl, spirv, lean"),
    })
}
