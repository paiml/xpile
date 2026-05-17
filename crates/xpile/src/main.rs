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
    // Total functions emitted across the corpus.
    functions_emitted: usize,
    // Subset of `functions_emitted` where `Function::applicable_contracts()`
    // is non-empty — i.e. the citation pipeline is *supposed* to fire
    // (function does arithmetic, bitwise, shift, etc.; not pure
    // comparison / logical). This is the F1 *denominator* per
    // XPILE-FALSIFY-002 — pre-002, the denominator was `functions_emitted`,
    // which double-penalised comparison-only fixtures.
    functions_requiring_citation: usize,
    // Subset of `functions_requiring_citation` that actually got a
    // citation in the emitted source.
    functions_with_citation: usize,
    // Sanity: functions that have a citation but shouldn't (e.g., a
    // future codegen bug that over-cites). Any non-zero value is a
    // bug to investigate, even though it doesn't fail F1 today.
    over_citations: usize,
    parse_errors: Vec<(PathBuf, String)>,
}

impl AuditReport {
    fn coverage_pct(&self) -> f64 {
        if self.functions_requiring_citation == 0 {
            // No applicable functions in the corpus → metric is
            // vacuously satisfied. 100% by convention so that a
            // small / empty corpus doesn't trip the falsifier.
            return 100.0;
        }
        (self.functions_with_citation as f64) / (self.functions_requiring_citation as f64) * 100.0
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
    // F1 now supports Rust, Ruchy, AND Lean — XPILE-FALSIFY-002 added
    // Lean's `@[xpile_contract "..."]` attribute as a recognised
    // citation form. PTX / WGSL / SPIR-V citations are XPILE-FALSIFY-003+.
    if !matches!(target, Target::Rust | Target::Ruchy | Target::Lean) {
        bail!(
            "`xpile audit` supports --target rust | ruchy | lean; {target:?} citation form not yet known — follow-up XPILE-FALSIFY-003"
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
        // Per-function audit: walk the Module's typed items, ask each
        // function whether it requires a citation (via
        // `Function::applicable_contracts()`), then check whether the
        // emitted source actually has the citation immediately above
        // the function's declaration. This is XPILE-FALSIFY-002's
        // refinement — pre-002, the denominator was "every emitted
        // function" which double-penalised comparison-only functions.
        for item in &module.items {
            let xpile_meta_hir::Item::Function(f) = item;
            let requires_citation = !f.applicable_contracts().is_empty();
            let cited = function_has_citation(&artifact.primary, &f.name, target);
            report.functions_emitted += 1;
            match (requires_citation, cited) {
                (true, true) => {
                    report.functions_requiring_citation += 1;
                    report.functions_with_citation += 1;
                }
                (true, false) => {
                    report.functions_requiring_citation += 1;
                }
                (false, true) => {
                    report.over_citations += 1;
                }
                (false, false) => {}
            }
        }
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

/// True if `function_name` has a contract citation immediately
/// preceding its declaration in `source`. Per-target signature shapes
/// and citation forms (XPILE-FALSIFY-002 added Lean):
///
///   Rust:  `// xpile-contract: <ID>`   prefix `pub fn <name>(`
///   Ruchy: `// xpile-contract: <ID>`   prefix `fun <name>(`
///   Lean:  `@[xpile_contract "<ID>"]`  prefix `def <name> (` / `partial def <name> (`
///
/// Walks backward from the declaration through blank lines to allow
/// for pretty-printer whitespace insertion. The walk stops at the
/// first non-blank line: either it's a citation (cited) or it isn't
/// (not cited).
fn function_has_citation(source: &str, function_name: &str, target: Target) -> bool {
    let prefixes: &[&str] = match target {
        Target::Rust => &["pub fn "],
        Target::Ruchy => &["fun "],
        // Lean has two signature forms — plain `def` and `partial def`
        // (PMAT-010's while-loop helper uses the latter).
        Target::Lean => &["def ", "partial def "],
        _ => return false,
    };
    let citation_marker = match target {
        Target::Rust | Target::Ruchy => "// xpile-contract:",
        Target::Lean => "@[xpile_contract",
        _ => return false,
    };
    let needle = format!("{function_name}(");
    let needle_space = format!("{function_name} (");

    let lines: Vec<&str> = source.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let stripped = line.trim_start();
        let is_decl = prefixes.iter().any(|p| {
            stripped.starts_with(p)
                && (stripped[p.len()..].starts_with(&needle)
                    || stripped[p.len()..].starts_with(&needle_space))
        });
        if !is_decl {
            continue;
        }
        // Walk backward looking for the citation (or its absence).
        let mut j = i;
        while j > 0 {
            j -= 1;
            let prev = lines[j].trim();
            if prev.is_empty() {
                continue;
            }
            return prev.starts_with(citation_marker);
        }
        return false;
    }
    false
}

fn print_audit_text(report: &AuditReport, target: Target) {
    println!("xpile audit — F1 (Layer-1 contract citation coverage)");
    println!("target backend: {:?}", target);
    println!();
    println!("  files scanned       : {}", report.files_scanned);
    println!("  functions emitted   : {}", report.functions_emitted);
    println!(
        "  require citation    : {}",
        report.functions_requiring_citation
    );
    println!("  with citation       : {}", report.functions_with_citation);
    println!(
        "  coverage (F1)       : {:.1}%   [{}]",
        report.coverage_pct(),
        report.f1_status()
    );
    if report.over_citations > 0 {
        println!(
            "  over-citations      : {}  (codegen bug?)",
            report.over_citations
        );
    }
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
    // metadata + parse-error count. XPILE-FALSIFY-002 added the
    // `functions_requiring_citation` denominator and the
    // `over_citations` sanity field.
    println!(
        "{{\"target\":\"{:?}\",\"files_scanned\":{},\"functions_emitted\":{},\"functions_requiring_citation\":{},\"functions_with_citation\":{},\"over_citations\":{},\"f1_pct\":{:.1},\"f1_status\":\"{}\",\"errors\":{}}}",
        target,
        report.files_scanned,
        report.functions_emitted,
        report.functions_requiring_citation,
        report.functions_with_citation,
        report.over_citations,
        report.coverage_pct(),
        report.f1_status(),
        report.parse_errors.len()
    );
}
