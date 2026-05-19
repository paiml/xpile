//! xpile binary entry point.
//!
//! v0.1.0 CLI surface:
//!   xpile transpile    <input> [--target <t>] [--out <path>]
//!   xpile audit        <path>  [--target <t>]
//!   xpile attestations [--roadmap <path>] [--contracts-dir <path>]  (XPILE-QUORUM-005)
//!   xpile quorum       [--contracts-dir <path>] [--fixtures-dir <path>]  (PMAT-033)
//!   xpile diamond      [--contracts-dir <path>]  (PMAT-249 — Diamond-tier coverage)
//!   xpile info         (default if no subcommand)
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
        /// Target backend: rust | ruchy | ptx | wgsl | spirv | lean | shell.
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
    /// Report the Extrinsic stratum's per-contract attestation counts
    /// (XPILE-QUORUM-005). Walks `contracts/*.yaml` to discover the
    /// contract ID universe, then scans the roadmap.yaml work-item
    /// log for mentions of each ID; each occurrence is one human
    /// attestation. Feeds the §14.4 quorum's Extrinsic-stratum vote
    /// tally alongside Semantic (Lean), Symbolic (Kani), and
    /// Runtime (diff_exec).
    Attestations {
        /// Path to the roadmap YAML (canonical attestation log).
        #[arg(long, default_value = "docs/roadmaps/roadmap.yaml")]
        roadmap: PathBuf,
        /// Path to contracts dir; every `*.yaml` with a `metadata.id`
        /// field contributes its ID to the universe being scanned.
        #[arg(long, default_value = "contracts")]
        contracts_dir: PathBuf,
        /// Emit JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
    },
    /// Unified §14.4 N-of-M oracle quorum reporter (PMAT-033). Walks
    /// every contract and tallies per-stratum votes:
    ///   Semantic   = `lean_theorem:` refs in the contract's own YAML
    ///   Symbolic   = `kani_harness:` refs in the contract's own YAML
    ///   Runtime    = fixture files under tests/fixtures/ that name
    ///                the contract ID in a comment / docstring
    ///   Extrinsic  = roadmap.yaml work-item mentions (PMAT-032)
    /// Reports per-contract counts + a quorum status:
    ///   QUORUM     (≥1 vote in ≥3 strata)
    ///   PARTIAL    (≥1 vote in 1-2 strata)
    ///   UNVERIFIED (0 strata represented)
    Quorum {
        #[arg(long, default_value = "contracts")]
        contracts_dir: PathBuf,
        #[arg(long, default_value = "crates/xpile/tests/fixtures")]
        fixtures_dir: PathBuf,
        #[arg(long, default_value = "docs/roadmaps/roadmap.yaml")]
        roadmap: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Diamond-tier coverage reporter (PMAT-249). Walks every
    /// contract YAML and tallies the number of `_diamond` lean_theorem
    /// references — the substrate's Diamond-tier coverage per contract.
    /// Reports raw count + classification:
    ///   `none` (0 Diamonds), `depth-1` (1 Diamond category),
    ///   `depth-2` (2 Diamonds), ..., `depth-8` (8 Diamonds),
    ///   `depth-9+` (9+ Diamonds — opened by PMAT-298).
    ///
    /// Useful for tracking Diamond depth over time and identifying
    /// contracts that could benefit from additional algebraic
    /// axiomatizations.
    Diamond {
        #[arg(long, default_value = "contracts")]
        contracts_dir: PathBuf,
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
        Cmd::Attestations {
            roadmap,
            contracts_dir,
            json,
        } => attestations(&roadmap, &contracts_dir, json),
        Cmd::Quorum {
            contracts_dir,
            fixtures_dir,
            roadmap,
            json,
        } => quorum(&contracts_dir, &fixtures_dir, &roadmap, json),
        Cmd::Diamond {
            contracts_dir,
            json,
        } => diamond(&contracts_dir, json),
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

    // PMAT-038: dispatch via `Frontend::matches_path` so bashrs-frontend
    // catches extensionless `Makefile` / `Dockerfile`. Every other
    // frontend's default impl still delegates to extension matching, so
    // python / c / ruchy behaviour is unchanged.
    let frontend = session
        .frontends
        .iter()
        .find(|f| f.matches_path(input))
        .with_context(|| {
            let ext_label = input
                .extension()
                .and_then(|s| s.to_str())
                .map(|e| format!("`.{e}`"))
                .unwrap_or_else(|| {
                    input
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| format!("filename `{n}`"))
                        .unwrap_or_else(|| "(unknown)".to_string())
                });
            let known: Vec<&'static str> = session
                .frontends
                .iter()
                .flat_map(|f| f.extensions().iter().copied())
                .collect();
            format!("no frontend handles {ext_label}; known extensions: {known:?}")
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
        // PMAT-037 / XPILE-BASHRS-MERGER-001: shell target accepted
        // via the bashrs-backend scaffold.
        "shell" | "sh" | "bash" => Target::Shell,
        other => {
            bail!("unknown target `{other}`; choose: rust, ruchy, ptx, wgsl, spirv, lean, shell")
        }
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
        // PMAT-038: same `matches_path` dispatch as the transpile
        // path. collect_source_files already filters by registered
        // extensions, so this lookup is currently never `None` —
        // but reaching for the trait method keeps the dispatch
        // pattern uniform across the two CLI subcommands.
        let Some(frontend) = session.frontends.iter().find(|f| f.matches_path(&src)) else {
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

// ─── attestations: Extrinsic-stratum vote tally (XPILE-QUORUM-005) ──
//
// Per `sub/provability-roadmap.md` §1.3 the §14.4 quorum needs four
// strata: Semantic (Lean), Symbolic (Kani), Runtime (diff_exec), and
// Extrinsic (humans). The first three are already gated in CI; this
// CLI exposes the Extrinsic count by treating each work-item mention
// of a contract ID in `roadmap.yaml` as one human attestation. The
// roadmap is the canonical record of what humans/agents committed to
// doing on the project — referencing a contract there is at minimum
// a "this contract was load-bearing for some piece of work" vote.

/// One contract's attestation tally — discovered IDs and the mentions
/// found in the roadmap.
#[derive(Debug)]
struct ContractAttestation {
    id: String,
    // Every `(work_item_id, line_no, snippet)` mention. A single
    // work item can contribute multiple mentions (title + description
    // + acceptance criteria all count). The snippet is the trimmed
    // line so the human reader can see the context.
    mentions: Vec<AttestationMention>,
}

#[derive(Debug)]
struct AttestationMention {
    /// The `id:` of the enclosing work item — usually `PMAT-NNN`.
    /// Empty string if the mention appears outside any work-item
    /// block (rare; would mean a top-level YAML key).
    work_item: String,
    line: usize,
    snippet: String,
}

#[derive(Debug)]
struct AttestationReport {
    contracts_scanned: usize,
    roadmap_path: PathBuf,
    contracts: Vec<ContractAttestation>,
    /// Contracts found in `contracts/` whose ID was not referenced
    /// anywhere in the roadmap. Surface so a future audit can spot
    /// "zombie contracts" — defined but unworked.
    unattested: Vec<String>,
}

fn attestations(roadmap_path: &Path, contracts_dir: &Path, json: bool) -> Result<()> {
    let ids = collect_contract_ids(contracts_dir)
        .with_context(|| format!("collect contract ids from {}", contracts_dir.display()))?;
    if ids.is_empty() {
        bail!(
            "no contract IDs discovered under {} — expected at least one *.yaml file \
             with a `metadata.id:` field",
            contracts_dir.display()
        );
    }
    let roadmap = std::fs::read_to_string(roadmap_path)
        .with_context(|| format!("read roadmap {}", roadmap_path.display()))?;
    let mut contracts: Vec<ContractAttestation> = Vec::new();
    let mut unattested: Vec<String> = Vec::new();
    for id in &ids {
        let mentions = scan_roadmap_for_id(&roadmap, id);
        if mentions.is_empty() {
            unattested.push(id.clone());
        } else {
            contracts.push(ContractAttestation {
                id: id.clone(),
                mentions,
            });
        }
    }
    // Stable output: sort by ID (helps diffs and JSON tooling).
    contracts.sort_by(|a, b| a.id.cmp(&b.id));
    unattested.sort();
    let report = AttestationReport {
        contracts_scanned: ids.len(),
        roadmap_path: roadmap_path.to_path_buf(),
        contracts,
        unattested,
    };
    if json {
        print_attestations_json(&report);
    } else {
        print_attestations_text(&report);
    }
    Ok(())
}

/// Walk `contracts_dir` for `*.yaml` files and pull each one's
/// `metadata.id:` value out. Uses a lightweight line scanner rather
/// than serde_yaml to keep the xpile bin's dep graph small (same
/// posture as `refinement_proofs.rs`'s `extract_quoted_value` helper).
fn collect_contract_ids(contracts_dir: &Path) -> Result<Vec<String>> {
    if !contracts_dir.is_dir() {
        bail!("{} is not a directory", contracts_dir.display());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(contracts_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(id) = extract_metadata_id(&contents) {
            out.push(id);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Extract a contract's `metadata.id:` value. Strategy: find the
/// `metadata:` block (must be at column 0), then look for the first
/// `  id:` line within it. Returns `None` if missing or malformed.
fn extract_metadata_id(contents: &str) -> Option<String> {
    let mut in_metadata = false;
    for line in contents.lines() {
        if line.starts_with("metadata:") {
            in_metadata = true;
            continue;
        }
        if in_metadata {
            // End of metadata block when a new top-level key appears.
            if !line.starts_with(' ') && !line.is_empty() && !line.starts_with('#') {
                return None;
            }
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("id:") {
                let raw = rest.trim().trim_matches('"').trim_matches('\'');
                if !raw.is_empty() {
                    return Some(raw.to_string());
                }
            }
        }
    }
    None
}

/// Scan the roadmap text for occurrences of `id`. Each occurrence
/// is paired with the enclosing work item's `id:` (e.g. `PMAT-029`)
/// and the line number. The roadmap is a flat YAML list under
/// `roadmap:`, with each item starting at column 2 (`- id: PMAT-...`).
fn scan_roadmap_for_id(roadmap: &str, contract_id: &str) -> Vec<AttestationMention> {
    let mut out = Vec::new();
    let mut current_item: String = String::new();
    for (idx, line) in roadmap.lines().enumerate() {
        // Detect a new work item header: `- id: PMAT-NNN`.
        if let Some(rest) = line.strip_prefix("- id: ") {
            current_item = rest.trim().to_string();
        }
        if line.contains(contract_id) {
            out.push(AttestationMention {
                work_item: current_item.clone(),
                line: idx + 1,
                snippet: line.trim().to_string(),
            });
        }
    }
    out
}

fn print_attestations_text(report: &AttestationReport) {
    println!("xpile attestations — Extrinsic stratum vote tally (XPILE-QUORUM-005)");
    println!(
        "scanned {} contract ID(s) from contracts/; mentions read from {}",
        report.contracts_scanned,
        report.roadmap_path.display()
    );
    println!();
    if report.contracts.is_empty() {
        println!("  (no attestations found)");
    } else {
        for c in &report.contracts {
            let unique_items: std::collections::BTreeSet<&str> =
                c.mentions.iter().map(|m| m.work_item.as_str()).collect();
            println!(
                "  {:<40}  {:>3} mentions across {:>2} work item(s)",
                c.id,
                c.mentions.len(),
                unique_items.len()
            );
            for w in &unique_items {
                println!("      - {w}");
            }
        }
    }
    if !report.unattested.is_empty() {
        println!();
        println!(
            "  unattested contracts ({}) — defined under contracts/ but never \
             referenced in roadmap.yaml:",
            report.unattested.len()
        );
        for u in &report.unattested {
            println!("    - {u}");
        }
    }
    println!();
    let total_mentions: usize = report.contracts.iter().map(|c| c.mentions.len()).sum();
    let attested = report.contracts.len();
    println!(
        "totals: {attested}/{} contracts attested, {total_mentions} total mention(s)",
        report.contracts_scanned
    );
    println!(
        "stratum: Extrinsic — per ruchy 5.0 §14.4, this counts toward the N-of-M oracle quorum \
         alongside Semantic (Lean), Symbolic (Kani), Runtime (diff_exec)."
    );
}

fn print_attestations_json(report: &AttestationReport) {
    // Hand-rolled JSON, same posture as `print_audit_json`. Schema:
    //   {
    //     "contracts_scanned": N,
    //     "roadmap_path": "...",
    //     "contracts": [{
    //       "id": "...", "mention_count": N,
    //       "work_items": [...],
    //       "mentions": [{"work_item": "...", "line": N, "snippet": "..."}]
    //     }],
    //     "unattested": ["...", ...]
    //   }
    let mut first = true;
    print!(
        "{{\"contracts_scanned\":{},\"roadmap_path\":\"{}\",\"contracts\":[",
        report.contracts_scanned,
        report.roadmap_path.display()
    );
    for c in &report.contracts {
        if !first {
            print!(",");
        }
        first = false;
        let unique_items: std::collections::BTreeSet<&str> =
            c.mentions.iter().map(|m| m.work_item.as_str()).collect();
        let items_json: Vec<String> = unique_items.iter().map(|w| format!("\"{w}\"")).collect();
        let mention_json: Vec<String> = c
            .mentions
            .iter()
            .map(|m| {
                format!(
                    "{{\"work_item\":\"{}\",\"line\":{},\"snippet\":\"{}\"}}",
                    m.work_item,
                    m.line,
                    escape_json(&m.snippet)
                )
            })
            .collect();
        print!(
            "{{\"id\":\"{}\",\"mention_count\":{},\"work_items\":[{}],\"mentions\":[{}]}}",
            c.id,
            c.mentions.len(),
            items_json.join(","),
            mention_json.join(",")
        );
    }
    print!("],\"unattested\":[");
    let unattested_json: Vec<String> = report
        .unattested
        .iter()
        .map(|u| format!("\"{u}\""))
        .collect();
    print!("{}", unattested_json.join(","));
    println!("]}}");
}

/// Minimal JSON-string escape. Handles backslash, double-quote, and
/// control characters that would otherwise break a one-line JSON
/// payload. Keeps the hand-rolled-JSON posture rather than pulling
/// serde_json for one field.
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod attestation_tests {
    use super::*;

    #[test]
    fn extract_metadata_id_finds_canonical_id() {
        let src = "metadata:\n  id: C-FOO\n  version: \"1.0\"\n";
        assert_eq!(extract_metadata_id(src), Some("C-FOO".to_string()));
    }

    #[test]
    fn extract_metadata_id_unquoted_and_quoted() {
        assert_eq!(
            extract_metadata_id("metadata:\n  id: \"C-BAR\"\n"),
            Some("C-BAR".to_string())
        );
        assert_eq!(
            extract_metadata_id("metadata:\n  id: 'C-BAZ'\n"),
            Some("C-BAZ".to_string())
        );
    }

    #[test]
    fn extract_metadata_id_missing_returns_none() {
        assert_eq!(extract_metadata_id("metadata:\n  version: \"1.0\"\n"), None);
        assert_eq!(extract_metadata_id(""), None);
    }

    #[test]
    fn scan_roadmap_attributes_mentions_to_enclosing_work_item() {
        let yaml = "\
roadmap:
- id: PMAT-100
  title: 'unrelated'
- id: PMAT-200
  title: 'Refers to C-FOO contract'
  acceptance_criteria:
  - mentions C-FOO again in the body
- id: PMAT-300
  title: 'plain'
";
        let mentions = scan_roadmap_for_id(yaml, "C-FOO");
        assert_eq!(mentions.len(), 2);
        assert!(mentions.iter().all(|m| m.work_item == "PMAT-200"));
    }

    #[test]
    fn scan_roadmap_returns_empty_when_id_absent() {
        let yaml = "- id: PMAT-1\n  title: 'foo'\n";
        assert!(scan_roadmap_for_id(yaml, "C-NOT-PRESENT").is_empty());
    }
}

// ─── quorum: unified §14.4 four-stratum view (PMAT-033) ─────────────
//
// Per ruchy 5.0 §14.4 a contract reaches QUORUM when ≥1 oracle in ≥3
// strata votes for it. xpile's strata at v0.1.0:
//
//   Semantic    Lean refinement theorems (XPILE-REFINE-XXX)
//   Symbolic    Kani BMC harnesses (XPILE-QUORUM-001..003)
//   Runtime     diff_exec fixtures + transpile_e2e (XPILE-DIFF-XXX)
//   Extrinsic   roadmap.yaml work-item mentions (XPILE-QUORUM-005)
//
// This subcommand is a *reporter*, not a gate: it consolidates counts
// from sources the project already maintains and renders a per-contract
// table. The constituent CI gates remain authoritative for their own
// stratum.

#[derive(Debug, Clone)]
struct QuorumRow {
    id: String,
    semantic: usize,
    symbolic: usize,
    runtime: usize,
    extrinsic: usize,
}

impl QuorumRow {
    fn strata_represented(&self) -> usize {
        [self.semantic, self.symbolic, self.runtime, self.extrinsic]
            .iter()
            .filter(|&&n| n >= 1)
            .count()
    }
    fn status(&self) -> &'static str {
        match self.strata_represented() {
            0 => "UNVERIFIED",
            1 | 2 => "PARTIAL",
            _ => "QUORUM",
        }
    }
    fn total(&self) -> usize {
        self.semantic + self.symbolic + self.runtime + self.extrinsic
    }
}

fn quorum(
    contracts_dir: &Path,
    fixtures_dir: &Path,
    roadmap_path: &Path,
    json: bool,
) -> Result<()> {
    // Build a map of {id -> own-yaml-text} so we can count lean/kani
    // refs in each contract's OWN file (votes for a contract come from
    // that contract's YAML, not from neighbours mentioning it).
    if !contracts_dir.is_dir() {
        bail!("{} is not a directory", contracts_dir.display());
    }
    let mut rows: Vec<QuorumRow> = Vec::new();
    for entry in std::fs::read_dir(contracts_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(id) = extract_metadata_id(&contents) else {
            continue;
        };
        rows.push(QuorumRow {
            semantic: count_field_occurrences(&contents, "lean_theorem:"),
            symbolic: count_field_occurrences(&contents, "kani_harness:"),
            runtime: count_runtime_witnesses(&id, fixtures_dir),
            extrinsic: 0, // filled below
            id,
        });
    }
    // Fill Extrinsic via the same scanner attestations() uses.
    let roadmap = std::fs::read_to_string(roadmap_path).unwrap_or_default();
    for row in rows.iter_mut() {
        row.extrinsic = scan_roadmap_for_id(&roadmap, &row.id).len();
    }
    rows.sort_by(|a, b| {
        // Sort by descending total, then by ID for stable output.
        b.total().cmp(&a.total()).then_with(|| a.id.cmp(&b.id))
    });
    if json {
        print_quorum_json(&rows);
    } else {
        print_quorum_text(&rows);
    }
    Ok(())
}

/// Count occurrences of `field_prefix` (e.g. `"lean_theorem:"`) as a
/// YAML key at any indentation. We require either the start of line or
/// whitespace-only before, then the prefix, then non-newline content.
/// Comments (`# ...`) are skipped.
fn count_field_occurrences(contents: &str, field_prefix: &str) -> usize {
    contents
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with('#') && trimmed.starts_with(field_prefix)
        })
        .count()
}

/// Count fixture files that mention `contract_id` somewhere in their
/// text. Per-fixture mentions are treated as a single vote (multiple
/// comment lines inside one fixture still count as 1, mirroring how
/// `xpile audit` treats one emitted function as one citation). Walks
/// `*.py` plus any other text files the directory contains.
fn count_runtime_witnesses(contract_id: &str, fixtures_dir: &Path) -> usize {
    if !fixtures_dir.is_dir() {
        return 0;
    }
    let mut hits = 0usize;
    if let Ok(entries) = std::fs::read_dir(fixtures_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if text.contains(contract_id) {
                hits += 1;
            }
        }
    }
    hits
}

fn print_quorum_text(rows: &[QuorumRow]) {
    println!("xpile quorum — §14.4 N-of-M oracle quorum (PMAT-033)");
    println!(
        "strata: Semantic (Lean) | Symbolic (Kani) | Runtime (fixtures) | Extrinsic (roadmap)"
    );
    println!();
    println!(
        "  {:<40} {:>4} {:>4} {:>4} {:>4}  {:<10}",
        "contract", "Sem", "Sym", "Run", "Ext", "status"
    );
    println!("  {}", "-".repeat(72));
    for r in rows {
        println!(
            "  {:<40} {:>4} {:>4} {:>4} {:>4}  {:<10}",
            r.id,
            r.semantic,
            r.symbolic,
            r.runtime,
            r.extrinsic,
            r.status(),
        );
    }
    println!();
    let quorum_count = rows.iter().filter(|r| r.status() == "QUORUM").count();
    let partial_count = rows.iter().filter(|r| r.status() == "PARTIAL").count();
    let unverified_count = rows.iter().filter(|r| r.status() == "UNVERIFIED").count();
    println!(
        "totals: {quorum_count} QUORUM, {partial_count} PARTIAL, {unverified_count} UNVERIFIED \
         ({} contracts total)",
        rows.len()
    );
    println!(
        "rule (ruchy 5.0 §14.4): QUORUM = ≥1 vote in ≥3 strata; PARTIAL = ≥1 vote in 1-2 strata."
    );
}

fn print_quorum_json(rows: &[QuorumRow]) {
    print!("{{\"contracts\":[");
    let mut first = true;
    for r in rows {
        if !first {
            print!(",");
        }
        first = false;
        print!(
            "{{\"id\":\"{}\",\"semantic\":{},\"symbolic\":{},\"runtime\":{},\"extrinsic\":{},\
             \"strata_represented\":{},\"status\":\"{}\"}}",
            r.id,
            r.semantic,
            r.symbolic,
            r.runtime,
            r.extrinsic,
            r.strata_represented(),
            r.status(),
        );
    }
    println!("]}}");
}

#[cfg(test)]
mod quorum_tests {
    use super::*;

    #[test]
    fn quorum_row_status_thresholds() {
        let r0 = QuorumRow {
            id: "X".into(),
            semantic: 0,
            symbolic: 0,
            runtime: 0,
            extrinsic: 0,
        };
        assert_eq!(r0.status(), "UNVERIFIED");
        let r1 = QuorumRow {
            id: "X".into(),
            semantic: 1,
            symbolic: 0,
            runtime: 0,
            extrinsic: 0,
        };
        assert_eq!(r1.status(), "PARTIAL");
        let r2 = QuorumRow {
            id: "X".into(),
            semantic: 1,
            symbolic: 1,
            runtime: 0,
            extrinsic: 0,
        };
        assert_eq!(r2.status(), "PARTIAL");
        let r3 = QuorumRow {
            id: "X".into(),
            semantic: 1,
            symbolic: 1,
            runtime: 1,
            extrinsic: 0,
        };
        assert_eq!(r3.status(), "QUORUM");
        let r4 = QuorumRow {
            id: "X".into(),
            semantic: 7,
            symbolic: 1,
            runtime: 3,
            extrinsic: 5,
        };
        assert_eq!(r4.status(), "QUORUM");
        assert_eq!(r4.total(), 16);
        assert_eq!(r4.strata_represented(), 4);
    }

    #[test]
    fn count_field_occurrences_skips_comments_and_unrelated_lines() {
        let yaml = "\
foo:
  lean_theorem: \"A\"
  # lean_theorem: \"commented out\"
  other: blah
  lean_theorem: \"B\"
bar:
  lean_theorem: \"C\"
";
        assert_eq!(count_field_occurrences(yaml, "lean_theorem:"), 3);
        assert_eq!(count_field_occurrences(yaml, "kani_harness:"), 0);
    }
}

// ─── diamond: Diamond-tier coverage reporter (PMAT-249) ──────────────
//
// Per ruchy 5.0 §14.10.5 the Diamond refinement tier captures COMBINED
// algebraic axiomatizations (monoids, groups, rings, semirings,
// equivalence relations, etc.). This subcommand walks every contract
// YAML and tallies `_diamond` lean_theorem references — each represents
// one wired Diamond theorem.
//
// The substrate's Diamond program at v0.1.0:
//   - Depth-1 UNIVERSAL: 12/12 contracts (each has at least 1 Diamond)
//   - Depth-2 UNIVERSAL: 12/12 contracts (each has at least 2 Diamonds)
//   - Depth-3 UNIVERSAL across layers: 5/5 layers (each has at least
//     one contract with at least 3 Diamonds)
//   - Depth-4 opened on 2 contracts (PyIntArith L1, CompileRustToPtxMma L5)
//
// This subcommand is a *reporter*, not a gate.

#[derive(Debug, Clone)]
struct DiamondRow {
    id: String,
    diamond_count: usize,
}

impl DiamondRow {
    fn depth_label(&self) -> &'static str {
        match self.diamond_count {
            0 => "none",
            1 => "depth-1",
            2 => "depth-2",
            3 => "depth-3",
            4 => "depth-4",
            5 => "depth-5",
            6 => "depth-6",
            7 => "depth-7",
            8 => "depth-8",
            9 => "depth-9",
            10 => "depth-10",
            11 => "depth-11",
            12 => "depth-12",
            13 => "depth-13",
            14 => "depth-14",
            _ => "depth-15+",
        }
    }
}

fn diamond(contracts_dir: &Path, json: bool) -> Result<()> {
    if !contracts_dir.is_dir() {
        bail!("{} is not a directory", contracts_dir.display());
    }
    let mut rows: Vec<DiamondRow> = Vec::new();
    for entry in std::fs::read_dir(contracts_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(id) = extract_metadata_id(&contents) else {
            continue;
        };
        rows.push(DiamondRow {
            id,
            diamond_count: count_diamond_theorems(&contents),
        });
    }
    rows.sort_by(|a, b| {
        b.diamond_count
            .cmp(&a.diamond_count)
            .then_with(|| a.id.cmp(&b.id))
    });
    if json {
        print_diamond_json(&rows);
    } else {
        print_diamond_text(&rows);
    }
    Ok(())
}

/// Count `_diamond` references in the `lean_theorem:` field values of a
/// contract YAML. Each represents one wired Diamond theorem.
fn count_diamond_theorems(contents: &str) -> usize {
    contents
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with('#')
                && trimmed.starts_with("lean_theorem:")
                && trimmed.contains("_diamond")
        })
        .count()
}

fn print_diamond_text(rows: &[DiamondRow]) {
    println!("xpile diamond — Diamond-tier coverage (PMAT-249)");
    println!(
        "depth: 0 = none, 1 = depth-1 (1 Diamond), 2 = depth-2 (2 Diamonds), 3+ = depth-3+ (3+)"
    );
    println!();
    println!("  {:<40} {:>7}  {:<10}", "contract", "diamond", "depth");
    println!("  {}", "-".repeat(60));
    for r in rows {
        println!(
            "  {:<40} {:>7}  {:<10}",
            r.id,
            r.diamond_count,
            r.depth_label(),
        );
    }
    println!();
    let total_diamonds: usize = rows.iter().map(|r| r.diamond_count).sum();
    let depth_1_plus = rows.iter().filter(|r| r.diamond_count >= 1).count();
    let depth_2_plus = rows.iter().filter(|r| r.diamond_count >= 2).count();
    let depth_3_plus = rows.iter().filter(|r| r.diamond_count >= 3).count();
    let depth_4_plus = rows.iter().filter(|r| r.diamond_count >= 4).count();
    let depth_5_plus = rows.iter().filter(|r| r.diamond_count >= 5).count();
    let depth_6_plus = rows.iter().filter(|r| r.diamond_count >= 6).count();
    let depth_7_plus = rows.iter().filter(|r| r.diamond_count >= 7).count();
    let depth_8_plus = rows.iter().filter(|r| r.diamond_count >= 8).count();
    let depth_9_plus = rows.iter().filter(|r| r.diamond_count >= 9).count();
    let depth_10_plus = rows.iter().filter(|r| r.diamond_count >= 10).count();
    let depth_11_plus = rows.iter().filter(|r| r.diamond_count >= 11).count();
    let depth_12_plus = rows.iter().filter(|r| r.diamond_count >= 12).count();
    let depth_13_plus = rows.iter().filter(|r| r.diamond_count >= 13).count();
    let depth_14_plus = rows.iter().filter(|r| r.diamond_count >= 14).count();
    let depth_15_plus = rows.iter().filter(|r| r.diamond_count >= 15).count();
    println!(
        "totals: {total_diamonds} Diamond theorems across {} contracts",
        rows.len()
    );
    println!(
        "  depth-1+: {depth_1_plus} contracts, depth-2+: {depth_2_plus} contracts, \
         depth-3+: {depth_3_plus} contracts, depth-4+: {depth_4_plus} contracts, \
         depth-5+: {depth_5_plus} contracts, depth-6+: {depth_6_plus} contracts, \
         depth-7+: {depth_7_plus} contracts, depth-8+: {depth_8_plus} contracts, \
         depth-9+: {depth_9_plus} contracts, depth-10+: {depth_10_plus} contracts, \
         depth-11+: {depth_11_plus} contracts, depth-12+: {depth_12_plus} contracts, \
         depth-13+: {depth_13_plus} contracts, depth-14+: {depth_14_plus} contracts, \
         depth-15+: {depth_15_plus} contracts"
    );
}

fn print_diamond_json(rows: &[DiamondRow]) {
    print!("{{\"contracts\":[");
    let mut first = true;
    for r in rows {
        if !first {
            print!(",");
        }
        first = false;
        print!(
            "{{\"id\":\"{}\",\"diamond_count\":{},\"depth\":\"{}\"}}",
            r.id,
            r.diamond_count,
            r.depth_label(),
        );
    }
    let total_diamonds: usize = rows.iter().map(|r| r.diamond_count).sum();
    let depth_1_plus = rows.iter().filter(|r| r.diamond_count >= 1).count();
    let depth_2_plus = rows.iter().filter(|r| r.diamond_count >= 2).count();
    let depth_3_plus = rows.iter().filter(|r| r.diamond_count >= 3).count();
    let depth_4_plus = rows.iter().filter(|r| r.diamond_count >= 4).count();
    let depth_5_plus = rows.iter().filter(|r| r.diamond_count >= 5).count();
    let depth_6_plus = rows.iter().filter(|r| r.diamond_count >= 6).count();
    let depth_7_plus = rows.iter().filter(|r| r.diamond_count >= 7).count();
    let depth_8_plus = rows.iter().filter(|r| r.diamond_count >= 8).count();
    let depth_9_plus = rows.iter().filter(|r| r.diamond_count >= 9).count();
    let depth_10_plus = rows.iter().filter(|r| r.diamond_count >= 10).count();
    let depth_11_plus = rows.iter().filter(|r| r.diamond_count >= 11).count();
    let depth_12_plus = rows.iter().filter(|r| r.diamond_count >= 12).count();
    let depth_13_plus = rows.iter().filter(|r| r.diamond_count >= 13).count();
    let depth_14_plus = rows.iter().filter(|r| r.diamond_count >= 14).count();
    let depth_15_plus = rows.iter().filter(|r| r.diamond_count >= 15).count();
    println!(
        "],\"total_diamonds\":{total_diamonds},\"contracts_total\":{},\
         \"depth_1_plus\":{depth_1_plus},\"depth_2_plus\":{depth_2_plus},\
         \"depth_3_plus\":{depth_3_plus},\"depth_4_plus\":{depth_4_plus},\
         \"depth_5_plus\":{depth_5_plus},\"depth_6_plus\":{depth_6_plus},\
         \"depth_7_plus\":{depth_7_plus},\"depth_8_plus\":{depth_8_plus},\
         \"depth_9_plus\":{depth_9_plus},\"depth_10_plus\":{depth_10_plus},\
         \"depth_11_plus\":{depth_11_plus},\"depth_12_plus\":{depth_12_plus},\
         \"depth_13_plus\":{depth_13_plus},\"depth_14_plus\":{depth_14_plus},\
         \"depth_15_plus\":{depth_15_plus}}}",
        rows.len()
    );
}

#[cfg(test)]
mod diamond_tests {
    use super::*;

    #[test]
    fn diamond_row_depth_label() {
        let r0 = DiamondRow {
            id: "X".into(),
            diamond_count: 0,
        };
        assert_eq!(r0.depth_label(), "none");
        let r1 = DiamondRow {
            id: "X".into(),
            diamond_count: 1,
        };
        assert_eq!(r1.depth_label(), "depth-1");
        let r4 = DiamondRow {
            id: "X".into(),
            diamond_count: 4,
        };
        assert_eq!(r4.depth_label(), "depth-4");
        // PMAT-286: depth-5 opened
        let r5 = DiamondRow {
            id: "X".into(),
            diamond_count: 5,
        };
        assert_eq!(r5.depth_label(), "depth-5");
        // PMAT-290: depth-6 opened
        let r6 = DiamondRow {
            id: "X".into(),
            diamond_count: 6,
        };
        assert_eq!(r6.depth_label(), "depth-6");
        // PMAT-292: depth-7 opened
        let r7 = DiamondRow {
            id: "X".into(),
            diamond_count: 7,
        };
        assert_eq!(r7.depth_label(), "depth-7");
        // PMAT-294: depth-8 opened
        let r8 = DiamondRow {
            id: "X".into(),
            diamond_count: 8,
        };
        assert_eq!(r8.depth_label(), "depth-8");
        // PMAT-298: depth-9 opened (later refined to discrete label by PMAT-300)
        let r9 = DiamondRow {
            id: "X".into(),
            diamond_count: 9,
        };
        assert_eq!(r9.depth_label(), "depth-9");
        // PMAT-300: depth-10 opened (later refined to discrete label by PMAT-302)
        let r10 = DiamondRow {
            id: "X".into(),
            diamond_count: 10,
        };
        assert_eq!(r10.depth_label(), "depth-10");
        // PMAT-302: depth-11 opened (later refined to discrete label by PMAT-305)
        let r11 = DiamondRow {
            id: "X".into(),
            diamond_count: 11,
        };
        assert_eq!(r11.depth_label(), "depth-11");
        // PMAT-305: depth-12 opened (later refined to discrete label by PMAT-307)
        let r12 = DiamondRow {
            id: "X".into(),
            diamond_count: 12,
        };
        assert_eq!(r12.depth_label(), "depth-12");
        // PMAT-307: depth-13 opened (later refined to discrete label by PMAT-310)
        let r13 = DiamondRow {
            id: "X".into(),
            diamond_count: 13,
        };
        assert_eq!(r13.depth_label(), "depth-13");
        // PMAT-310: depth-14 opened (later refined to discrete label by PMAT-312)
        let r14 = DiamondRow {
            id: "X".into(),
            diamond_count: 14,
        };
        assert_eq!(r14.depth_label(), "depth-14");
        // PMAT-312: depth-15+ opened (FIRST DEPTH-15 in the substrate)
        let r15 = DiamondRow {
            id: "X".into(),
            diamond_count: 15,
        };
        assert_eq!(r15.depth_label(), "depth-15+");
        let r16 = DiamondRow {
            id: "X".into(),
            diamond_count: 16,
        };
        assert_eq!(r16.depth_label(), "depth-15+");
    }

    #[test]
    fn count_diamond_theorems_only_counts_diamond_refs() {
        let yaml = "\
foo:
  lean_theorem: \"a.b.add_dispatch_commutative_monoid_diamond\"
  # lean_theorem: \"a.b.commented_diamond\"
bar:
  lean_theorem: \"a.b.something_silver\"
baz:
  lean_theorem: \"a.b.division_algorithm_diamond\"
qux:
  lean_theorem: \"a.b.shift_monoid_diamond\"
";
        assert_eq!(count_diamond_theorems(yaml), 3);
    }
}
