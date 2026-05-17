//! xpile binary entry point.
//!
//! v0.1.0 CLI surface:
//!   xpile transpile <input> [--target <t>] [--out <path>]
//!   xpile audit    <path>  [--target <t>]
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
    /// Report falsifier F1 (Layer-1 contract citation coverage) for
    /// a corpus. Walks the given path, transpiles every source file
    /// xpile recognises, and reports the % of emitted functions that
    /// carry a `// xpile-contract: <ID>` citation. Drives the
    /// XPILE-FALSIFY-001 metric from `sub/provability-roadmap.md`.
    Audit {
        /// Path to scan (file or directory). Defaults to the current
        /// directory. Source files are detected by extension via the
        /// registered frontends.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Target backend to audit. Citation emission semantics are
        /// per-backend, so the metric is reported per-target.
        #[arg(long, default_value = "rust")]
        target: String,
        /// Emit JSON instead of human-readable text. Useful for CI
        /// dashboards and the `XPILE-SOTA-XXX` quarterly dossier.
        #[arg(long)]
        json: bool,
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
        Cmd::Audit { path, target, json } => audit(&session, &path, &target, json),
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

// ─── audit: F1 citation-coverage reporter (XPILE-FALSIFY-001) ────
//
// Per `sub/provability-roadmap.md` §1.1:
//   F1 = % of transpiled functions carrying at least one
//        `// xpile-contract: <ID>` citation
//   target: ≥ 95% on a fixed corpus
//   falsified: < 50%
//
// Implementation: for every source file recognised by the dispatch
// table, run the full transpile pipeline and parse the emitted output
// for function declarations + their immediately-preceding citation
// comments. The metric is computed per-backend because the citation
// syntax differs (Rust/Ruchy use `// xpile-contract:`, Lean uses
// `@[xpile_contract "..."]`); this CLI exposes the Rust/Ruchy form
// since they share a regex. Lean coverage is a follow-up.

#[derive(Debug, Default, Clone)]
struct AuditReport {
    files_scanned: usize,
    functions_emitted: usize,
    functions_with_citation: usize,
    parse_errors: Vec<(PathBuf, String)>,
}

impl AuditReport {
    fn coverage_pct(&self) -> f64 {
        if self.functions_emitted == 0 {
            return 0.0;
        }
        (self.functions_with_citation as f64) / (self.functions_emitted as f64) * 100.0
    }

    /// F1 status per the roadmap's targets:
    ///   ≥ 95% → OK    (target reached)
    ///   < 95% but ≥ 50% → WARN (below target, above falsifier)
    ///   < 50%  → FAIL (falsifier tripped — the citation pipeline is performative)
    fn f1_status(&self) -> &'static str {
        let pct = self.coverage_pct();
        if pct >= 95.0 {
            "OK"
        } else if pct >= 50.0 {
            "WARN"
        } else {
            "FAIL"
        }
    }
}

fn audit(session: &TranspileSession, path: &Path, target_str: &str, json: bool) -> Result<()> {
    let target = parse_target(target_str)?;
    // F1 is only meaningfully computable for backends that emit the
    // `// xpile-contract: <ID>` comment form. Lean uses
    // `@[xpile_contract "..."]`; reporting F1 for Lean is a follow-up.
    if !matches!(target, Target::Rust | Target::Ruchy) {
        bail!(
            "`xpile audit` currently supports --target rust or ruchy (citation form: `// xpile-contract: <ID>`); \
             {target:?} uses a different citation syntax (Lean: `@[xpile_contract ...]`) — follow-up XPILE-FALSIFY-002"
        );
    }

    let mut report = AuditReport::default();
    let sources = collect_source_files(session, path);
    for src in sources {
        report.files_scanned += 1;
        let contents = match std::fs::read_to_string(&src) {
            Ok(s) => s,
            Err(e) => {
                report.parse_errors.push((src, format!("read failed: {e}")));
                continue;
            }
        };
        let ext = src.extension().and_then(|s| s.to_str()).unwrap_or_default();
        let Some(frontend) = session
            .frontends
            .iter()
            .find(|f| f.extensions().contains(&ext))
        else {
            // Shouldn't happen — collect_source_files filters by registered extensions.
            continue;
        };
        let module = match frontend.parse_and_lower(&src, &contents) {
            Ok(m) => m,
            Err(e) => {
                report
                    .parse_errors
                    .push((src.clone(), format!("parse_and_lower: {e}")));
                continue;
            }
        };
        let backend = session
            .backends
            .iter()
            .find(|b| b.targets().contains(&target))
            .expect("target validated above");
        let config = BackendConfig {
            target,
            profile: Profile::RustOut,
            hardware: None,
        };
        let artifact = match backend.lower(&module, &config) {
            Ok(a) => a,
            Err(e) => {
                report
                    .parse_errors
                    .push((src.clone(), format!("backend: {e}")));
                continue;
            }
        };
        let (emitted, cited) = count_citations(&artifact.primary, target);
        report.functions_emitted += emitted;
        report.functions_with_citation += cited;
    }

    if json {
        print_audit_json(&report, target);
    } else {
        print_audit_text(&report, target);
    }
    Ok(())
}

/// Walk `path` (file or directory) and return every source file whose
/// extension matches a registered frontend. Skips hidden directories
/// and `target/`.
fn collect_source_files(session: &TranspileSession, path: &Path) -> Vec<PathBuf> {
    let known_exts: Vec<&str> = session
        .frontends
        .iter()
        .flat_map(|f| f.extensions().iter().copied())
        .collect();

    let mut out = Vec::new();
    if path.is_file() {
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            if known_exts.contains(&ext) {
                out.push(path.to_path_buf());
            }
        }
        return out;
    }
    if path.is_dir() {
        walk_dir(path, &known_exts, &mut out);
    }
    out
}

fn walk_dir(dir: &Path, known_exts: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        if p.is_dir() {
            walk_dir(&p, known_exts, out);
        } else if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
            if known_exts.contains(&ext) {
                out.push(p);
            }
        }
    }
}

/// Count `(emitted_functions, cited_functions)` in a Rust- or Ruchy-
/// emitted string. A function is "cited" when the line immediately
/// preceding its declaration is `// xpile-contract: <ID>`. Robust to
/// blank lines (the citation must be the *immediately* preceding
/// non-blank line — matches the codegen's `emit_contract_citations` +
/// `emit_function` sequence in `xpile-rust-codegen/src/lib.rs`).
fn count_citations(source: &str, target: Target) -> (usize, usize) {
    let prefix = match target {
        Target::Rust => "pub fn ",
        Target::Ruchy => "fun ",
        _ => return (0, 0),
    };
    let lines: Vec<&str> = source.lines().collect();
    let mut emitted = 0;
    let mut cited = 0;
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with(prefix) {
            emitted += 1;
            // Walk backward through blank lines looking for the
            // citation. The codegen emits the citation immediately
            // before the signature with no blank in between (see
            // emit_contract_citations + emit_function), but allow
            // one blank line of grace for any pretty-printer that
            // might be inserted later.
            let mut j = i;
            while j > 0 {
                j -= 1;
                let prev = lines[j].trim();
                if prev.is_empty() {
                    continue;
                }
                if prev.starts_with("// xpile-contract:") {
                    cited += 1;
                }
                break;
            }
        }
    }
    (emitted, cited)
}

fn print_audit_text(report: &AuditReport, target: Target) {
    println!("xpile audit — F1 (Layer-1 contract citation coverage)");
    println!("target backend: {:?}", target);
    println!();
    println!("  files scanned       : {}", report.files_scanned);
    println!("  functions emitted   : {}", report.functions_emitted);
    println!("  with citation       : {}", report.functions_with_citation);
    println!(
        "  coverage (F1)       : {:.1}%   [{}]",
        report.coverage_pct(),
        report.f1_status()
    );
    if !report.parse_errors.is_empty() {
        println!();
        println!("  errors ({}):", report.parse_errors.len());
        for (path, err) in &report.parse_errors {
            println!("    - {}: {}", path.display(), err);
        }
    }
    println!();
    println!(
        "F1 thresholds (sub/provability-roadmap.md §1.1): ≥95% OK; ≥50% WARN; <50% FAIL (falsifier tripped)."
    );
}

fn print_audit_json(report: &AuditReport, target: Target) {
    // Hand-rolled JSON to avoid pulling serde_json into the xpile bin
    // for a one-line dashboard payload. The schema mirrors what
    // sub/provability-roadmap.md §1.1 says we report: F1 + scan
    // metadata + parse-error count. Per-error file/message lives in
    // text mode (`xpile audit ... | jq -R` is not the target here).
    println!(
        "{{\"target\":\"{:?}\",\"files_scanned\":{},\"functions_emitted\":{},\"functions_with_citation\":{},\"f1_pct\":{:.1},\"f1_status\":\"{}\",\"errors\":{}}}",
        target,
        report.files_scanned,
        report.functions_emitted,
        report.functions_with_citation,
        report.coverage_pct(),
        report.f1_status(),
        report.parse_errors.len()
    );
}
