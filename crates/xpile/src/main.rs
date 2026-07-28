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
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use xpile_agent::{
    Budget, FfiArgCastRepair, Probe, RepairLoop, RepairOutcome, RepairRule, Symptom,
};
use xpile_backend::{BackendConfig, HwProfile, Profile, Target};
use xpile_core::TranspileSession;
use xpile_ffi_manifest::{
    defining_function, resolve_boundary_to_langs, retype_float_ffi_sites, wrapper_native, FfiEntry,
    FfiManifest,
};
use xpile_frontend::{AliasSemantics, LoweringProfile, SpellingScope};
use xpile_meta_hir::{Module, SourceLang, Type};
use xpile_oracle::{capture_cpython_hybrid_ref, diff_stdout, ComparisonResult, CtypesBinding};

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
        /// Target backend: rust | ruchy | ptx | wgsl | spirv | wasm | lean |
        /// shell | forjar; aliases: wat=wasm, sh=shell, bash=shell,
        /// forjar-yaml=forjar
        #[arg(long, default_value = "rust")]
        target: String,
        /// Output path. Defaults to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Emit a complete, buildable Cargo crate (`Cargo.toml` + `src/main.rs`)
        /// to this directory instead of printing. If the program defines
        /// `main()`, the crate is a runnable binary: `cargo build` gives a
        /// native executable, and `cargo build --target wasm32-wasip1` gives a
        /// single portable `.wasm` that runs on any OS/arch under a WASI
        /// runtime (the "universal binary" path). `--target rust` only.
        #[arg(long)]
        emit_crate: Option<PathBuf>,
        /// Contract-citation emission. `on` (default) annotates each emitted
        /// construct that HAS an applicable contract with its
        /// `// xpile-contract:` citations across the L1–L5 taxonomy layers —
        /// often none, since a comparison-only or call-only body has no
        /// governing contract and is emitted with no citation line at all;
        /// `off` suppresses them for annotation-free output. The library
        /// counterpart is `xpile_backend::strip_contract_citations`.
        #[arg(long, default_value = "on", value_parser = ["on", "off"])]
        contracts: String,
        /// Hardware profile for hardware-dependent targets. `ptx` selects the
        /// PTX profile at the contract-floor compute capability `sm_80`;
        /// `ptx:sm_89` overrides it. REQUIRED to reach `--target ptx` (the PTX
        /// backend refuses without a compute capability). Omit for every other
        /// target.
        #[arg(long)]
        hardware: Option<String>,
    },
    /// Report falsifier F1 (Layer-1 contract citation coverage) for
    /// a corpus. Walks the given path, transpiles every source file
    /// xpile recognises, and reports the % of the functions that
    /// REQUIRE a `// xpile-contract: <ID>` citation which carry one.
    /// The denominator is the functions whose `applicable_contracts()`
    /// is non-empty, NOT every emitted function — XPILE-FALSIFY-002
    /// narrowed it there because comparison-only and logical-only
    /// bodies correctly emit none. Both counts are printed. Drives the
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
    ///   Runtime    = the UNION of (a) fixture files under tests/fixtures/
    ///                that name the contract ID and (b) top-level `*.rs`
    ///                files under each `--witness-dir` that name the ID AND
    ///                carry a non-comment runtime-availability probe call
    ///                (PMAT-1367)
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
        /// Directory whose top-level `*.rs` files may cast a Runtime vote
        /// (repeatable; votes are unioned by canonical path, so overlapping
        /// directories cannot double-count a file). A file votes only when it
        /// BOTH names the contract ID and carries a non-comment call to one of
        /// the `RUNTIME_PROBES` — naming the ID alone is not execution.
        #[arg(long, default_value = "crates/xpile-wasm-codegen/tests")]
        witness_dir: Vec<PathBuf>,
        /// Directory whose `*.rs` sources are searched (recursively) for the
        /// NAME of a fixture, deciding whether that fixture is loaded by a
        /// test (repeatable). A fixture under `--fixtures-dir` casts a Runtime
        /// vote only when some source here or under a `--witness-dir` names
        /// it — naming the contract ID inside the fixture is not evidence that
        /// anything runs it (PMAT-1432). The `--fixtures-dir` subtree itself is
        /// excluded, so a `.rs` fixture cannot vote for itself.
        ///
        /// Defaults to `--fixtures-dir`'s PARENT rather than to a literal path.
        /// A literal `crates/xpile/tests` default is CWD-relative, and every
        /// in-tree caller runs `quorum` with CWD = the crate dir and an
        /// ABSOLUTE `--fixtures-dir`; the two would not have pointed at the
        /// same tree, silently zeroing the whole fixture pass. Deriving it
        /// keeps the pair consistent however the caller spells the corpus.
        #[arg(long)]
        fixture_loader_dir: Vec<PathBuf>,
        #[arg(long, default_value = "docs/roadmaps/roadmap.yaml")]
        roadmap: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Diamond-tier coverage reporter (PMAT-249). Walks every
    /// contract YAML and tallies the number of `_diamond` lean_theorem
    /// references — the substrate's Diamond-tier coverage per contract.
    /// Reports the raw count plus a classification computed from it:
    /// 0 Diamonds classifies as `none`, and N Diamonds as `depth-N`.
    /// The classification is EXACT and never bucketed, so WHICH labels
    /// appear is a function of the corpus rather than of a list written
    /// here.
    ///
    /// The `depth-N+` spellings in the totals block mean something
    /// else: those are CUMULATIVE counts (how many contracts carry at
    /// least N Diamonds), not a classification any one contract holds.
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
    /// Hybrid transpile — Phase 1 + Phase 2 of the hybrid flow (§16). Walks a
    /// module directory, dispatches each source file to its frontend, collects
    /// the resulting modules, and reconciles their cross-language FFI boundaries
    /// (`FfiManifest::reconcile`) into a manifest. Prints one line per resolved
    /// boundary (symbol, from→to, shim_id) or the unresolved boundaries and
    /// exits non-zero (the `manifest_completeness` gate of C-FFI-CPYTHON-EXT).
    Hybrid {
        /// Path to the hybrid module directory (e.g. a dir with `app.py` +
        /// `_core.c`). Source files are detected by extension.
        path: PathBuf,
        /// Phase 4: write the reconciled Rust FFI shims (`extern "C"` + safe
        /// wrappers) to this path. Omit to only report the manifest.
        #[arg(long)]
        emit_shims: Option<PathBuf>,
        /// Phase 5a: emit a buildable Cargo workspace (a `build.rs` that
        /// cc-compiles the C side + links the emitted shims, plus the non-C
        /// modules lowered to Rust) to this directory. `cargo build` it to
        /// compile + link the hybrid artifact.
        #[arg(long)]
        emit_workspace: Option<PathBuf>,
        /// NORTH STAR (Phase 3 + 5): emit the workspace to a temp dir,
        /// `cargo build` + run the linked C+shim artifact, and differential-check
        /// its stdout against the CPython reference (the C extension bound via
        /// ctypes). Exit 0 on Match, non-zero on Divergence. Graceful-skips
        /// (exit 0) when `cc`/`python3`/`cargo` are unavailable.
        #[arg(long)]
        verify: bool,
        /// PMAT-1353 (Phase 6): when `--verify` finds a BUILD FAILURE or a
        /// DIVERGENCE, drive the bounded, fail-closed, deterministic
        /// `xpile-agent` repair loop over the lowered Rust body and re-verify
        /// through the SAME emit → `cargo build` → run → differential path.
        /// Prints the converged rule chain and exits 0 on a repair; exits
        /// NON-ZERO (fail-closed) when no rule applies. Requires `--verify`.
        /// Opt-in: without it, `--verify`'s output and exit code are unchanged.
        #[arg(long, requires = "verify")]
        repair: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let session = xpile_core::default_session();
    match cli.cmd.unwrap_or(Cmd::Info) {
        Cmd::Info => print_info(&session),
        Cmd::Transpile {
            input,
            target,
            out,
            emit_crate,
            contracts,
            hardware,
        } => transpile(
            &session,
            &input,
            &target,
            out.as_deref(),
            emit_crate.as_deref(),
            &contracts,
            hardware.as_deref(),
        ),
        Cmd::Audit { path, target, json } => audit(&session, &path, &target, json),
        Cmd::Attestations {
            roadmap,
            contracts_dir,
            json,
        } => attestations(&roadmap, &contracts_dir, json),
        Cmd::Quorum {
            contracts_dir,
            fixtures_dir,
            witness_dir,
            fixture_loader_dir,
            roadmap,
            json,
        } => quorum(
            &contracts_dir,
            &fixtures_dir,
            &witness_dir,
            &fixture_loader_dir,
            &roadmap,
            json,
        ),
        Cmd::Diamond {
            contracts_dir,
            json,
        } => diamond(&contracts_dir, json),
        Cmd::Hybrid {
            path,
            emit_shims,
            emit_workspace,
            verify,
            repair,
        } => hybrid(
            &session,
            &path,
            emit_shims.as_deref(),
            emit_workspace.as_deref(),
            verify,
            repair,
        ),
    }
}

/// PMAT-897/899 (Sprint-2 Tier 2): the `xpile hybrid <dir>` command — Phase 1
/// (dispatch every source file in the directory to its frontend, collecting a
/// `Vec<Module>`) + Phase 2 (reconcile the modules' cross-language FFI
/// boundaries into a manifest) + Phase 4 (with `--emit-shims`, lower the
/// manifest to a per-paradigm Rust FFI shim file — `extern "C"` for C, a
/// `Command` wrapper for Shell, a mechanism-named gap for the rest). Reports
/// each resolved boundary, or the unresolved ones with a non-zero exit (the
/// `manifest_completeness` invariant of `C-FFI-CPYTHON-EXT`).
///
/// PMAT-902 (NORTH STAR): with `--verify`, runs the executing differential —
/// Phase 5 (emit the workspace to a temp dir, `cargo build` + run the linked
/// C+shim artifact) compared against Phase 3 (the CPython reference, the C
/// extension bound via ctypes) — and prints a Match/Divergent verdict.
///
/// PMAT-1353: with `--verify --repair`, a build failure or a divergence hands
/// off to [`repair_hybrid`] — the bounded, fail-closed `xpile-agent` repair loop
/// — instead of bailing immediately. `repair` is inert unless `verify` is set
/// (clap enforces `requires = "verify"`).
fn hybrid(
    session: &TranspileSession,
    path: &Path,
    emit_shims: Option<&Path>,
    emit_workspace: Option<&Path>,
    verify: bool,
    repair: bool,
) -> Result<()> {
    let sources = collect_source_files(session, path);
    if sources.is_empty() {
        bail!("no source files xpile recognises under {}", path.display());
    }
    let mut modules = Vec::new();
    // PMAT-901: retain each C file's (filename, source) so `--emit-workspace` can
    // write it into the workspace for `build.rs` to cc-compile — the Module IR
    // does not carry the original source text.
    let mut c_sources: Vec<(String, String)> = Vec::new();
    // PMAT-902: retain each Python file's (filename, source) so `--verify` can run
    // the original `main()` under CPython as the differential reference.
    let mut py_sources: Vec<(String, String)> = Vec::new();
    // PMAT-1362: retain each shell script's (filename, source) so `--verify` can
    // run the ORIGINAL script under `sh` as the shell lane's reference — the
    // artifact side spawns the RE-EMITTED script, so both texts are needed.
    let mut sh_sources: Vec<(String, String)> = Vec::new();
    for src in sources {
        let contents =
            std::fs::read_to_string(&src).with_context(|| format!("reading {}", src.display()))?;
        // PMAT-038 dispatch (same as transpile/audit): collect_source_files
        // already filtered by registered extension, so this is never None.
        let Some(frontend) = session.frontends.iter().find(|f| f.matches_path(&src)) else {
            continue;
        };
        let module = frontend
            .parse_and_lower(&src, &contents)
            .with_context(|| format!("parse_and_lower failed for {}", src.display()))?;
        let fname = src
            .file_name()
            .and_then(|n| n.to_str())
            .map(String::from)
            .unwrap_or_else(|| "source".to_string());
        match module.source_lang {
            SourceLang::C => c_sources.push((fname, contents.clone())),
            SourceLang::Python => py_sources.push((fname, contents.clone())),
            SourceLang::Shell => sh_sources.push((fname, contents.clone())),
            _ => {}
        }
        modules.push(module);
    }

    println!(
        "xpile hybrid: {} module(s) dispatched from {}",
        modules.len(),
        path.display()
    );
    // PMAT-898: with the full module set in hand, rewrite each boundary's
    // provisional `to_lang` (hardcoded C by the single-file frontend) to the
    // language of the sibling that actually defines the symbol — so a relative
    // import of a Python sibling becomes Python→Python (dropped by reconcile),
    // not a false Python→C FFI boundary.
    resolve_boundary_to_langs(&mut modules);
    match FfiManifest::reconcile(&modules) {
        Ok(manifest) => {
            // PMAT-931: re-type the FFI call sites of reconciled `double`-
            // returning C symbols in the calling (Python) module — the Python
            // frontend lowered them with the unknown-callee I64 default before
            // the C side was known, mis-rendering a whole double (`10` vs
            // Python's `10.0`) and mistyping `let r: float` (rustc E0308).
            retype_float_ffi_sites(&manifest, &mut modules);
            if manifest.entries.is_empty() {
                println!("  no cross-language FFI boundaries");
            } else {
                println!("  {} FFI boundary(ies) reconciled:", manifest.entries.len());
                for e in &manifest.entries {
                    println!(
                        "    {} : {:?} → {:?}  [{}]",
                        e.symbol, e.from_lang, e.to_lang, e.shim_id
                    );
                }
            }
            // PMAT-899 Phase 4: with `--emit-shims <path>`, lower the reconciled
            // manifest to a self-contained Rust FFI shim file (per-paradigm: real
            // `extern "C"` for C, `Command` for Shell; a mechanism-named gap for
            // the rest). All-or-nothing — any unshimmable boundary fails loud, so
            // a half-shimmed hybrid build never reaches disk.
            if let Some(out_path) = emit_shims {
                if manifest.entries.is_empty() {
                    println!("  --emit-shims: no FFI boundaries — nothing to emit");
                    return Ok(());
                }
                match manifest.emit_rust_shims(&modules) {
                    Ok(src) => {
                        std::fs::write(out_path, &src)
                            .with_context(|| format!("writing shims to {}", out_path.display()))?;
                        println!(
                            "  emitted {} FFI shim(s) → {}",
                            manifest.entries.len(),
                            out_path.display()
                        );
                    }
                    Err(err) => {
                        eprintln!("xpile hybrid: shim emission FAILED");
                        for u in &err.unsupported {
                            eprintln!("    {u}");
                        }
                        bail!("{} unshimmable FFI boundary(ies)", err.unsupported.len());
                    }
                }
            }
            // PMAT-901 Phase 5a: with `--emit-workspace <dir>`, emit a buildable
            // Cargo workspace — the C side as cc-compiled + linked objects, the
            // non-C modules lowered to Rust, and the reconciled `extern "C"` shims
            // wiring them together. `cargo build` it for the first executing
            // hybrid artifact.
            if let Some(ws_dir) = emit_workspace {
                let rust_src = lower_hybrid_rust(session, &modules)?;
                match manifest.emit_hybrid_workspace(&modules, &c_sources, &rust_src, ws_dir) {
                    Ok(()) => println!(
                        "  emitted hybrid workspace → {} (run `cargo build` to compile + link)",
                        ws_dir.display()
                    ),
                    Err(err) => {
                        eprintln!("xpile hybrid: workspace emission FAILED: {err}");
                        bail!("hybrid workspace emit failed");
                    }
                }
            }
            // PMAT-902 NORTH STAR: `--verify` runs the executing differential.
            if verify {
                return verify_hybrid(
                    session,
                    &manifest,
                    &modules,
                    &c_sources,
                    &py_sources,
                    &sh_sources,
                    repair,
                );
            }
            Ok(())
        }
        Err(err) => {
            eprintln!("xpile hybrid: FFI reconciliation FAILED");
            for u in &err.unresolved {
                eprintln!("    {u}");
            }
            // Non-zero exit: an unresolved boundary blocks the hybrid build.
            bail!("{} unresolved FFI boundary(ies)", err.unresolved.len())
        }
    }
}

/// Lower the non-C, non-Shell modules to one Rust source string — the body of
/// the emitted hybrid workspace's `main.rs`. The C side becomes the linked
/// object and Shell can't lower to Rust, so both are skipped; everything else
/// (Python/Ruchy/Rust/Lean) is the Rust the artifact runs. Shared by
/// `--emit-workspace` and `--verify`.
fn lower_hybrid_rust(session: &TranspileSession, modules: &[Module]) -> Result<String> {
    let rust_backend = session
        .backends
        .iter()
        .find(|b| b.targets().contains(&Target::Rust))
        .context("no Rust backend registered")?;
    let config = BackendConfig {
        emit_contracts: true,
        target: Target::Rust,
        profile: Profile::RustOut,
        hardware: None,
    };
    let mut rust_src = String::new();
    for m in modules {
        if matches!(m.source_lang, SourceLang::C | SourceLang::Shell) {
            continue;
        }
        let artifact = rust_backend
            .lower(m, &config)
            .with_context(|| format!("lowering module `{}` to Rust", m.name))?;
        rust_src.push_str(&artifact.primary);
        rust_src.push('\n');
    }
    Ok(rust_src)
}

/// Is `tool --version` spawnable? Lets `--verify` graceful-skip on a runner
/// without a C compiler / Python / cargo (mirrors the oracle presence gates).
fn tool_available(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

/// Meta-HIR type → `ctypes` type name, matching the FFI shim's C-ABI mapping
/// (`I64`/`Bool` → `c_int`, `F64` → `c_double`, `CUInt` → `c_uint`). `None` for
/// non-ABI types.
///
/// **PMAT-1353 added `CUInt`, and it removed a false green.** `Type::CUInt` (a C
/// `unsigned` / `uint32_t`) was absent here, so `--verify` printed
/// `boundary <sym> has a non-ABI-mappable type — skipping` and exited 0 — while
/// `--emit-workspace` on the SAME fixture emitted a workspace that does not
/// compile: the Python frontend lowers the boundary call with its unknown-callee
/// `i64` default (`f(3i64)`) but `emit_c_shim`'s safe wrapper takes the
/// signedness-preserving `u32` (PMAT-918), so `cargo build` fails E0308. That is
/// the PMAT-931 call-site-retype hole in the UNSIGNED direction, and the skip was
/// a disclosed pass standing in front of it: the one check that would have caught
/// it declined to look. `unsigned int` ↔ `ctypes.c_uint` is the canonical binding
/// the shim itself already uses (`::std::os::raw::c_uint` / `u32`), so this
/// widens the CHECKED set without deciding any semantics. The consequence is
/// intended: such a fixture now exits NON-ZERO naming the build failure, and
/// `--repair` converges on it (see [`repair_hybrid`]).
///
/// `CULong`, `CLong`, `F32` and `Ptr` stay refused — each needs its own probed
/// binding decision, and an unprobed guess here would re-create exactly the
/// false green this comment describes.
fn ctypes_name(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::I64 | Type::Bool => Some("c_int"),
        Type::F64 => Some("c_double"),
        Type::CUInt => Some("c_uint"),
        _ => None,
    }
}

/// Build the `ctypes` ABI binding for a C boundary symbol from its defining
/// function's meta-HIR types. `None` if a param/return type is not ABI-mappable
/// (the caller then skips verification — capability honesty over a wrong bind).
fn ctypes_binding_for(entry: &FfiEntry, modules: &[Module]) -> Option<CtypesBinding> {
    let f = defining_function(modules, entry)?;
    let mut argtypes = Vec::new();
    for p in &f.params {
        argtypes.push(ctypes_name(&p.ty)?);
    }
    let restype = match &f.return_type {
        Type::Unit => None,
        ty => Some(ctypes_name(ty)?),
    };
    Some(CtypesBinding {
        symbol: entry.symbol.clone(),
        argtypes,
        restype,
    })
}

/// PMAT-902 NORTH STAR — the executing hybrid differential; PMAT-1362 widened
/// it past the C-only filter.
///
/// Two boundary paradigms EXECUTE, each with its own reference (they are not
/// interchangeable — see the per-lane docs):
///   * **C** — [`verify_c_boundary`]: CPython-via-`ctypes` vs the linked
///     Rust+C artifact, driven by the original `app.py` `main()`.
///   * **Shell** — [`verify_shell_boundary`]: the ORIGINAL `.sh` under `sh` vs
///     the built artifact whose emitted subprocess shim spawns the RE-EMITTED
///     script.
///
/// Every other `to_lang` (Cuda, Cpp, Python, Ruchy, Rust, Lean, …) still has no
/// executing reference, so it is reported as not-executed rather than silently
/// counted as verified — PMAT-1387 made that unconditional; it used to be
/// printed only when NO C or Shell boundary existed, so a mixed manifest
/// verified the C half and never mentioned the rest.
///
/// Match → `Ok(())`; Divergence → non-zero exit; VACUOUS (an empty reference,
/// so byte-identity holds trivially) → non-zero exit, see
/// [`differential_verdict`]. Graceful-skips (`Ok`) when a toolchain or fixture
/// shape the check needs is absent, so a constrained CI stays green — an
/// ENVIRONMENT absence is disclosed and skipped; a differential that observed
/// nothing is refused.
fn verify_hybrid(
    session: &TranspileSession,
    manifest: &FfiManifest,
    modules: &[Module],
    c_sources: &[(String, String)],
    py_sources: &[(String, String)],
    sh_sources: &[(String, String)],
    repair: bool,
) -> Result<()> {
    let c_entries: Vec<&FfiEntry> = manifest
        .entries
        .iter()
        .filter(|e| e.to_lang == SourceLang::C)
        .collect();
    let sh_entries: Vec<&FfiEntry> = manifest
        .entries
        .iter()
        .filter(|e| e.to_lang == SourceLang::Shell)
        .collect();
    // PMAT-1387: the not-executed report is a property of the FUNCTION, not of
    // one branch of it. It used to live ONLY inside the both-empty arm, so a
    // manifest holding one C boundary *and* three `Python → Cuda` boundaries
    // verified the C one, printed `✓ MATCH`, exited 0, and never mentioned the
    // other three — contradicting this function's own doc claim two paragraphs
    // up. Report the unverifiable remainder unconditionally, before any lane
    // runs, so the count is stated whether or not an executing lane exists.
    // (PMAT-1386's lesson, third instance: a guard written for one case is not
    // a guard for the function.)
    let others = manifest.entries.len() - c_entries.len() - sh_entries.len();
    if others > 0 {
        println!(
            "  --verify: {others} boundary(ies) are neither C nor Shell — \
             no executing reference exists for them; they were NOT verified"
        );
    }
    if c_entries.is_empty() && sh_entries.is_empty() {
        // PMAT-1362: name BOTH executable paradigms so the message stays true
        // when a fixture has a boundary of some third kind. A `Python → Cuda`
        // boundary is unverified, not verified — say so.
        if others == 0 {
            println!("  --verify: no FFI boundary to execute — nothing to verify");
        } else {
            println!("  --verify: no C or Shell boundary to execute — NOTHING was verified");
        }
        return Ok(());
    }

    // PMAT-1353: `--repair` is a C-lane capability only. Every rule in
    // `xpile-agent::repair` is a transform over emitted RUST (an ABI cast, a
    // float-repr block); the shell lane's artifact is a re-emitted `.sh` spawned
    // by a subprocess shim, so none of them can apply to it. Say so rather than
    // letting a shell-only fixture look like it was offered a repair.
    if repair && !sh_entries.is_empty() {
        println!(
            "  --repair: {} Shell boundary(ies) are NOT repairable — every repair rule \
             is a transform over emitted Rust; the shell lane has none",
            sh_entries.len()
        );
    }
    if !sh_entries.is_empty() {
        verify_shell_boundary(
            session,
            manifest,
            modules,
            &sh_entries,
            c_sources,
            sh_sources,
        )?;
    }
    if !c_entries.is_empty() {
        verify_c_boundary(
            session, manifest, modules, &c_entries, c_sources, py_sources, repair,
        )?;
    }
    Ok(())
}

/// The C half of the executing hybrid differential (PMAT-902): CPython (the C
/// extension bound via `ctypes`, driven by the original `app.py` `main()`) vs
/// the emitted, `cargo build`-ed, linked Rust+C artifact.
///
/// PMAT-1353: `repair` changes NOTHING on the success path and nothing about the
/// wording or the exit code of either failure verdict. It only adds a hand-off
/// AFTER the verdict has already been printed — a build failure or a divergence
/// goes on to [`repair_hybrid`] instead of returning that verdict's error
/// directly. `--verify` without `--repair` is byte-identical to before, which is
/// what makes this landable inside a release window (asserted by
/// `hybrid_repair.rs::verify_without_repair_is_byte_identical_on_every_lane`).
fn verify_c_boundary(
    session: &TranspileSession,
    manifest: &FfiManifest,
    modules: &[Module],
    c_entries: &[&FfiEntry],
    c_sources: &[(String, String)],
    py_sources: &[(String, String)],
    repair: bool,
) -> Result<()> {
    // Toolchain gate — graceful-skip so a constrained runner stays green.
    if !tool_available("cc") || !tool_available("python3") || !tool_available("cargo") {
        println!("  --verify: cc/python3/cargo unavailable — skipping execution");
        return Ok(());
    }

    // The Python entry: the dispatched Python source that defines `main()`.
    let Some((py_name, py_source)) = py_sources.iter().find(|(_, s)| s.contains("def main")) else {
        println!("  --verify: no Python `main()` entry — nothing to run");
        return Ok(());
    };

    // ctypes bindings for every C boundary; skip if any type isn't ABI-mappable.
    let mut bindings = Vec::new();
    for e in c_entries {
        match ctypes_binding_for(e, modules) {
            Some(b) => bindings.push(b),
            None => {
                println!(
                    "  --verify: boundary `{}` has a non-ABI-mappable type — skipping",
                    e.symbol
                );
                return Ok(());
            }
        }
    }

    // Phase 3 — capture the CPython reference (the C extension bound via ctypes).
    let reference = capture_cpython_hybrid_ref(py_source, c_sources, &bindings)
        .context("capturing the CPython reference")?;

    // Phase 5 — emit the workspace to a temp dir, build, run the linked artifact.
    let ws = std::env::temp_dir().join(format!("xpile_verify_ws_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    let rust_src = lower_hybrid_rust(session, modules)?;
    manifest
        .emit_hybrid_workspace(modules, c_sources, &rust_src, &ws)
        .map_err(|e| anyhow::anyhow!("workspace emit failed: {e}"))?;

    let target = ws.join("target");
    let build = Command::new("cargo")
        .current_dir(&ws)
        .arg("build")
        .arg("--target-dir")
        .arg(&target)
        .output()
        .context("cargo build of the hybrid workspace")?;
    if !build.status.success() {
        let _ = std::fs::remove_dir_all(&ws);
        let stderr = String::from_utf8_lossy(&build.stderr).to_string();
        // PMAT-1353: a BUILD FAILURE is `Symptom::BuildError` — the class the
        // ABI-cast rules were written for. The bail text below is unchanged, so
        // the default path is byte-identical.
        if repair {
            eprintln!("hybrid artifact failed to build:\n{stderr}");
            return repair_hybrid(
                session,
                manifest,
                modules,
                c_entries,
                c_sources,
                &reference,
                "the CPython reference",
            );
        }
        bail!("hybrid artifact failed to build:\n{stderr}");
    }
    let bin = target.join("debug").join("xpile-hybrid-artifact");
    let run = Command::new(&bin)
        .output()
        .context("running the hybrid artifact")?;
    let _ = std::fs::remove_dir_all(&ws);
    if !run.status.success() {
        bail!(
            "hybrid artifact exited {}:\n{}",
            run.status,
            String::from_utf8_lossy(&run.stderr)
        );
    }
    let actual = String::from_utf8_lossy(&run.stdout)
        .trim_end_matches('\n')
        .to_string();

    // Differential verdict.
    println!("  --verify: CPython reference (from {py_name}) vs executed C+shim artifact:");
    let verdict = differential_verdict(&reference, &actual, "CPython:", "the CPython reference");
    // PMAT-1353: hand a DIVERGENCE — and ONLY a divergence — to the repair loop,
    // after the verdict above has already been printed in full. `diff_stdout` is
    // recomputed rather than threaded out of `differential_verdict`, deliberately:
    // that function is the single reporter both executing lanes share and its
    // output is pinned byte-for-byte by `hybrid_golden_lock.rs`, so it is worth
    // one extra pure string comparison to leave it untouched.
    //
    // A VACUOUS verdict is NOT handed off. An empty reference is a FIXTURE defect
    // — there is no symptom in the artifact to repair, and a loop probing against
    // an empty reference would "converge" on any candidate that also prints
    // nothing, manufacturing exactly the false pass PMAT-1387 closed.
    if repair && verdict.is_err() && !reference.is_empty() {
        if let ComparisonResult::Divergence { .. } = diff_stdout(&reference, &actual) {
            return repair_hybrid(
                session,
                manifest,
                modules,
                c_entries,
                c_sources,
                &reference,
                "the CPython reference",
            );
        }
    }
    verdict
}

// ─────────────────────────────────────────────────────────────────────────────
// PMAT-1353 — the CLI seam for `xpile-agent`'s bounded, fail-closed,
// deterministic repair loop. Before this the crate held 931 lines, 3 rule impls
// and 18 passing tests that NO USER COULD INVOKE.
// ─────────────────────────────────────────────────────────────────────────────

/// A [`Probe`] over the REAL hybrid build path: re-emit the workspace with
/// `candidate` as the lowered Rust body, `cargo build` it (C side cc-compiled and
/// linked by the emitted `build.rs`), run the artifact, and differentially
/// compare its stdout against the captured CPython reference.
///
/// This is deliberately NOT [`xpile_agent::HybridCcRustcProbe`], which drives a
/// single-file `rustc` over a self-contained program. The candidate here is the
/// `main.rs` BODY of a multi-file cargo workspace (`mod ffi_shims;` + per-boundary
/// `use` aliases + the lowered module), so the only build that means anything is
/// the same `emit_hybrid_workspace` → `cargo build` path `--verify` itself judged.
/// A repair verified by a *different* build would not be evidence about the
/// artifact that failed.
///
/// **What repair can and cannot reach.** The candidate is the lowered Rust body
/// ONLY. `src/ffi_shims.rs` is regenerated from the manifest on every iteration,
/// so a rule cannot edit the shim — see [`boundary_repair_rules`] for which of
/// `xpile-agent`'s three rules that leaves reachable, and why.
/// The ONE place the repair loop's probe-workspace root is spelled.
///
/// PMAT-1436. This used to be an inline `format!` in `verify_c_boundary`, with
/// the prefix literal `"xpile_repair_ws_"` duplicated into
/// `tests/hybrid_repair.rs`, which globbed [`std::env::temp_dir`] for it. Two
/// measured consequences of identifying the subject by a glob over a
/// process-global namespace instead of by name:
///
/// * FALSE RED — a CONCURRENT `xpile … --repair` (any sibling test in the same
///   `cargo test` binary; each spawned child gets its own pid, hence its own
///   root) lands in the witness's after-set and is reported as "every probe
///   workspace THIS RUN created", naming a directory this run never touched.
/// * FALSE GREEN — change this prefix and the witness stops seeing the root at
///   all, so a run that leaks every workspace it built still passes the test
///   called `repair_writes_nothing_and_leaves_no_workspace_behind`.
///
/// The fix is not a better glob. The run now PRINTS this path and the number of
/// workspaces it built under it, so the witness reads the identity of the
/// process it actually spawned. Keyed by pid so two concurrent `--repair`
/// invocations cannot share a root; rooted at `temp_dir()` so a caller that sets
/// `TMPDIR` gets a private namespace it can assert is empty afterwards.
fn repair_probe_root() -> PathBuf {
    std::env::temp_dir().join(format!("xpile_repair_ws_{}", std::process::id()))
}

struct HybridWorkspaceProbe<'a> {
    manifest: &'a FfiManifest,
    modules: &'a [Module],
    c_sources: &'a [(String, String)],
    reference: &'a str,
    /// Root under which each evaluation gets its OWN workspace directory. A
    /// shared dir would let a stale `src/main.rs` or a half-written target dir
    /// leak from one iteration into the next, and the whole point of the loop is
    /// that iteration N+1's verdict is about candidate N+1.
    root: PathBuf,
    seq: AtomicUsize,
}

impl Probe for HybridWorkspaceProbe<'_> {
    fn evaluate(&self, candidate: &str) -> Result<(), Symptom> {
        let n = self.seq.fetch_add(1, Ordering::Relaxed);
        let ws = self.root.join(format!("iter_{n}"));
        let _ = std::fs::remove_dir_all(&ws);
        let cleanup = || {
            let _ = std::fs::remove_dir_all(&ws);
        };

        if let Err(e) =
            self.manifest
                .emit_hybrid_workspace(self.modules, self.c_sources, candidate, &ws)
        {
            cleanup();
            return Err(Symptom::BuildError {
                stderr: format!("workspace emit failed: {e}"),
            });
        }
        let target = ws.join("target");
        match Command::new("cargo")
            .current_dir(&ws)
            .arg("build")
            .arg("--target-dir")
            .arg(&target)
            .output()
        {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                cleanup();
                return Err(Symptom::BuildError { stderr });
            }
            Err(e) => {
                cleanup();
                return Err(Symptom::BuildError {
                    stderr: format!("spawning cargo build: {e}"),
                });
            }
        }
        let run = Command::new(target.join("debug").join("xpile-hybrid-artifact")).output();
        let result = match run {
            Ok(o) if o.status.success() => {
                let actual = String::from_utf8_lossy(&o.stdout)
                    .trim_end_matches('\n')
                    .to_string();
                // Reuse the ONE differential the product uses, so a "repaired"
                // candidate is repaired by the same standard `--verify` applies.
                match diff_stdout(self.reference, &actual) {
                    ComparisonResult::Match => Ok(()),
                    ComparisonResult::Divergence {
                        index,
                        expected,
                        actual,
                    } => Err(Symptom::Divergence {
                        index,
                        expected,
                        actual,
                    }),
                }
            }
            // A non-zero artifact exit is not a build error and not a stdout
            // divergence; report it as a divergence carrying the exit status, so
            // no rule mistakes it for an `E0308` and the loop fails closed.
            Ok(o) => Err(Symptom::Divergence {
                index: 0,
                expected: self.reference.to_string(),
                actual: format!(
                    "<artifact exited {}>: {}",
                    o.status,
                    String::from_utf8_lossy(&o.stderr).trim()
                ),
            }),
            Err(e) => Err(Symptom::BuildError {
                stderr: format!("running the hybrid artifact: {e}"),
            }),
        };
        cleanup();
        result
    }
}

/// Wraps a [`RepairRule`] and records its name when — and only when — it
/// actually FIRES, so the CLI can print the converged rule chain.
///
/// [`RepairOutcome`] carries an iteration COUNT, not the rules that produced it.
/// Adding a chain field to that enum would touch 20 match sites and the 18 tests
/// that pin `xpile-agent`'s public surface; recording at the rule boundary gets
/// the exact same chain with zero change to that surface. `RepairLoop::run`
/// applies the FIRST rule that returns `Some` per iteration, so the recorded
/// sequence is precisely the applied chain — not the tried set.
struct RecordingRule {
    inner: Box<dyn RepairRule>,
    applied: Arc<Mutex<Vec<&'static str>>>,
}

impl RepairRule for RecordingRule {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn apply(&self, symptom: &Symptom, candidate: &str) -> Option<String> {
        let out = self.inner.apply(symptom, candidate);
        if out.is_some() {
            if let Ok(mut log) = self.applied.lock() {
                log.push(self.inner.name());
            }
        }
        out
    }
}

/// Derive the repair rules for this manifest's C boundaries.
///
/// **Only [`FfiArgCastRepair`] is derivable for this candidate domain, and this
/// is the honest reason for each of `xpile-agent`'s other two rules:**
///
///   * `FfiReturnCastRepair` rewrites a shim's tail `__r` into `__r as <native>`.
///     `__r` only ever appears in `src/ffi_shims.rs`, which
///     [`HybridWorkspaceProbe`] REGENERATES from the manifest every iteration —
///     so the text it targets cannot occur in the candidate and the rule could
///     never fire. Wiring it would add a rule with a provably empty domain, which
///     is how a capability count gets inflated.
///   * `FloatReprRepair` rewrites a plain `println!("{}", <float>)` into the
///     CPython-faithful `.0`-suffix block. MEASURED against this emitter: it no
///     longer emits that shape at all — every float print already carries the
///     full repr block (nan/inf/exponent/`fract()`), because PMAT-931's
///     `retype_float_ffi_sites` plus `Expr::ToStr { of_float: true }` fixed that
///     class in the production seam. Its domain is empty here too.
///
/// So one of three rules is reachable through this seam. That is a real
/// capability — it converges on a real, production-emitted `E0308` (see
/// `fixtures/hybrid_unsigned`) — and it is not three.
///
/// The `abi` field carries the WRAPPER's native type ([`wrapper_native`]), not
/// the C ABI type: the candidate is the `main.rs` body, whose `f(..)` call
/// resolves through `use ffi_shims::f_shim as f` to the SAFE WRAPPER, so the cast
/// the call site is missing is a cast to the wrapper's parameter type.
///
/// A boundary whose parameters do not all share one `wrapper_native` is SKIPPED
/// with a printed reason: `FfiArgCastRepair` casts every top-level argument to the
/// same type, so on a mixed `f(int, double)` it would emit a wrong repair. The
/// probe would reject it and the loop would fail closed, but declining up front
/// says so instead of burning an iteration on a guess.
fn boundary_repair_rules(modules: &[Module], c_entries: &[&FfiEntry]) -> Vec<Box<dyn RepairRule>> {
    let mut rules: Vec<Box<dyn RepairRule>> = Vec::new();
    for e in c_entries {
        let Some(f) = defining_function(modules, e) else {
            println!(
                "  --repair: boundary `{}` has no defining C function — no rule derivable",
                e.symbol
            );
            continue;
        };
        if f.params.is_empty() {
            println!(
                "  --repair: boundary `{}` takes no arguments — no call-site cast to insert",
                e.symbol
            );
            continue;
        }
        let natives: Vec<&'static str> = f.params.iter().map(|p| wrapper_native(&p.ty)).collect();
        let first = natives[0];
        if natives.iter().any(|n| *n != first) {
            println!(
                "  --repair: boundary `{}` mixes wrapper types {natives:?} — `ffi-arg-cast` casts \
                 every argument to ONE type, so it is declined rather than guessed",
                e.symbol
            );
            continue;
        }
        rules.push(Box::new(FfiArgCastRepair {
            symbol: e.symbol.clone(),
            abi: first.to_string(),
        }));
    }
    rules
}

/// PMAT-1353 — drive the bounded, fail-closed, deterministic repair loop over
/// the lowered Rust body and re-verify through the SAME build path `--verify`
/// used. Called only from `--verify --repair`, only after the verdict for the
/// original artifact has already been printed.
///
/// **Fail-closed, and it writes nothing.** On [`RepairOutcome::Repaired`] this
/// prints the converged rule chain and exits 0; the repaired source stays in
/// memory. `xpile` has no canonical destination for it — the hybrid Rust body is
/// DERIVED from the Python module, so writing it would fork the artifact from its
/// source. `RepairLoop::run_and_commit` is the disciplined write path a future
/// `--repair-out <dir>` would use. On [`RepairOutcome::Exhausted`] the exit is
/// NON-ZERO and the last symptom is named: a repair that did not converge is
/// reported as a failure, never as a diagnosis-only success.
///
/// **`AlreadyMatching` is treated as a DEFECT, not a pass.** This function is
/// reached only when `--verify` already observed a failure. If the loop's first
/// probe then reports a match, the two build paths disagree about the same
/// candidate — a real inconsistency worth a loud non-zero exit, because silently
/// printing "already matching" would convert a flaky differential into a green.
fn repair_hybrid(
    session: &TranspileSession,
    manifest: &FfiManifest,
    modules: &[Module],
    c_entries: &[&FfiEntry],
    c_sources: &[(String, String)],
    reference: &str,
    subject: &str,
) -> Result<()> {
    let rules = boundary_repair_rules(modules, c_entries);
    if rules.is_empty() {
        eprintln!(
            "  ✗ NOT REPAIRED — no repair rule is derivable from this manifest, so the loop \
             was never started"
        );
        bail!("hybrid repair: no applicable repair rule (fail-closed)");
    }
    let applied: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
    let tried: Vec<&'static str> = rules.iter().map(|r| r.name()).collect();
    let recorded: Vec<Box<dyn RepairRule>> = rules
        .into_iter()
        .map(|inner| {
            Box::new(RecordingRule {
                inner,
                applied: Arc::clone(&applied),
            }) as Box<dyn RepairRule>
        })
        .collect();
    // A tight budget on purpose: every iteration is a full cargo build. The rules
    // are idempotent, so a converging chain is at most one step per boundary.
    let budget = Budget {
        max_iterations: (recorded.len() as u32).saturating_add(1),
        max_wall_clock: Duration::from_secs(300),
        ..Budget::default()
    };
    println!(
        "  --repair: bounded repair loop — {} rule(s) {tried:?}, max {} iteration(s)",
        recorded.len(),
        budget.max_iterations
    );
    let root = repair_probe_root();
    let _ = std::fs::remove_dir_all(&root);
    let probe = HybridWorkspaceProbe {
        manifest,
        modules,
        c_sources,
        reference,
        root: root.clone(),
        seq: AtomicUsize::new(0),
    };
    let initial = lower_hybrid_rust(session, modules)?;
    let outcome = RepairLoop::new(budget, recorded).run(&probe, &initial);
    // PMAT-1436: the run ANNOUNCES its own probe root and how many candidate
    // workspaces it built there. Before this, the only statement about the loop's
    // disk use was a test that globbed the shared temp dir for a hard-coded
    // prefix — which identifies the neighbourhood, not the run. The count comes
    // from the probe's OWN allocation counter, so "N built, all removed" cannot
    // be satisfied by never building anything: N is the number of directories
    // `HybridWorkspaceProbe::evaluate` actually made.
    let built = probe.seq.load(Ordering::Relaxed);
    let _ = std::fs::remove_dir_all(&root);
    println!(
        "      probe workspaces: {built} built under {} — {}",
        root.display(),
        if root.exists() {
            "NOT REMOVED (leaked)"
        } else {
            "all removed"
        }
    );
    let chain = applied.lock().map(|l| l.clone()).unwrap_or_default();
    match outcome {
        RepairOutcome::AlreadyMatching { .. } => {
            eprintln!(
                "  ✗ INCONSISTENT — `--verify` reported a failure but the repair loop's first \
                 probe of the SAME unmodified candidate matched {subject}"
            );
            eprintln!(
                "      the two build paths disagree; that is a differential defect, not a repair"
            );
            bail!("hybrid repair: verify and repair disagree on the unrepaired artifact")
        }
        RepairOutcome::Repaired { iterations, .. } => {
            println!("  ✓ REPAIRED in {iterations} iteration(s) — applied rule chain: {chain:?}");
            println!(
                "      the repaired artifact now matches {subject}. xpile wrote NOTHING to your \
                 tree: `--repair` DIAGNOSES, it does not commit (the hybrid Rust body is derived \
                 from the Python module, so committing it would fork the artifact from its source)"
            );
            Ok(())
        }
        RepairOutcome::Exhausted { iterations, last } => {
            eprintln!(
                "  ✗ NOT REPAIRED — fail-closed after {iterations} iteration(s); rules applied: \
                 {chain:?}, rules available: {tried:?}"
            );
            eprintln!("      last symptom: {}", describe_symptom(&last));
            bail!("hybrid repair: exhausted without reaching a match (fail-closed)")
        }
    }
}

/// One-line rendering of the symptom the loop failed closed on, so the operator
/// learns WHICH class went unrepaired rather than just that repair failed.
fn describe_symptom(s: &Symptom) -> String {
    match s {
        Symptom::BuildError { stderr } => {
            let first = stderr
                .lines()
                .find(|l| l.trim_start().starts_with("error"))
                .unwrap_or_else(|| stderr.lines().next().unwrap_or("<empty>"));
            format!("BuildError — {}", first.trim())
        }
        Symptom::Divergence {
            index,
            expected,
            actual,
        } => format!(
            "Divergence at line {} — reference {expected:?} vs artifact {actual:?}",
            index + 1
        ),
    }
}

/// PMAT-1387 — the shared Match/Vacuous/Divergent verdict for BOTH executing
/// lanes, so the two agree by construction rather than by two copies staying in
/// sync (PMAT-1386's doctrine: make the reporters agree, don't invent a fourth
/// posture).
///
/// **The defect this exists to close.** Both lanes previously reported
/// `ComparisonResult::Match` as `✓ MATCH` unconditionally and exited 0. When the
/// reference side produced NO output — a `main()` whose body is `pass`, or an
/// empty `.sh` — both sides were the empty string, so byte-identity held
/// TRIVIALLY and `--verify`, the PMAT-902 NORTH STAR check, printed
/// `✓ MATCH — stdout byte-identical (1 line(s)): ""` for a run in which the
/// reconciled FFI boundary was never called and nothing whatsoever was observed.
/// The `.max(1)` even asserted a line count of 1 for zero lines. An empty
/// reference is not agreement; it is the ABSENCE of evidence, and a differential
/// that cannot be distinguished from one that never ran must not be reported as
/// a pass. It REFUSES (non-zero), because `--verify` was explicitly asked for
/// and the answer it would otherwise give is false — this is a fixture defect,
/// not the environment-absence kind that earns a graceful skip.
///
/// **Deliberately narrow.** The guard fires only on the `Match`-with-empty-
/// reference combination. A non-empty reference against an empty artifact is
/// still a `Divergence`, which is a real, more informative finding and keeps its
/// side-by-side diagnostic. A non-empty MATCH still does NOT prove every
/// reconciled boundary was CALLED — only that the two hosts agreed on the output
/// that was produced. That residual is out of scope here; it needs call
/// instrumentation, not an output predicate.
fn differential_verdict(
    reference: &str,
    actual: &str,
    ref_label: &str,
    bail_subject: &str,
) -> Result<()> {
    match diff_stdout(reference, actual) {
        ComparisonResult::Match if reference.is_empty() => {
            eprintln!(
                "  ✗ VACUOUS — both sides produced NO output, so byte-identity holds \
                 trivially and nothing was observed"
            );
            eprintln!(
                "      the reference side must print something the artifact can be \
                 compared against; a silent `main()` / empty script exercises no boundary"
            );
            bail!("hybrid verify: VACUOUS differential — {bail_subject} produced no output, so nothing was verified")
        }
        ComparisonResult::Match => {
            println!(
                "  ✓ MATCH — stdout byte-identical ({} line(s)): {reference:?}",
                reference.lines().count()
            );
            Ok(())
        }
        ComparisonResult::Divergence {
            index,
            expected,
            actual: diverged,
        } => {
            // PMAT-1352: `index` is 0-BASED, so printing it raw reported a
            // first-line divergence as "line 0" — no editor and no human
            // numbers lines from zero. Found by the test that gives this arm
            // its first coverage; +1 makes the number match what a reader sees.
            eprintln!("  ✗ DIVERGENT at line {}:", index + 1);
            // `artifact:` is 9 columns, so the reference label pads to 9 too —
            // the two values stay aligned for whichever lane is reporting.
            eprintln!("      {ref_label:<9} {expected}");
            eprintln!("      artifact: {diverged}");
            bail!("hybrid verify: artifact diverged from {bail_subject}")
        }
    }
}

/// PMAT-1362 — the SHELL half of the executing hybrid differential.
///
/// **What it proves.** The emitted subprocess shim (`Command::new("<prog>")`,
/// citing `C-FFI-SHELL-SUBPROCESS`) is compiled into a real `cargo build`-ed
/// artifact, the artifact is RUN, and it spawns the xpile-RE-EMITTED POSIX
/// script — whose stdout must be byte-identical to the ORIGINAL `.sh` run
/// directly under `sh`. Two seams execute at once: the bashrs
/// frontend→backend round-trip (original script vs re-emitted script) and the
/// Rust-side subprocess shim (spawn by program name, exit code, captured
/// stdout). Before this, `--verify` filtered to `to_lang == C` and printed
/// "no C FFI boundary to execute" on a fixture that had reconciled a real
/// `Python → Shell` shim.
///
/// **What it does NOT prove — read this before widening the claim.**
///   1. **argv marshalling.** The shim takes `&[&str]`, but no meta-HIR call
///      path yet produces shell arguments, so the generated driver passes
///      `&[]`. `argv_passthrough` remains string-compare-only
///      (`FALSIFY-FFI-SHELL-SUBPROCESS-001`).
///   2. **The Python caller is not the driver.** A shell boundary is invoked
///      by PROGRAM NAME and the shim returns `io::Result<Output>`; a lowered
///      Python `_tool()` call has no shape that consumes that, so the driver
///      is GENERATED rather than being `app.py`'s `main()`. The C lane's
///      "CPython reference" framing therefore does not carry over — the
///      reference here is `sh`, not CPython.
///   3. **stderr / stdin / non-zero exit into Python.** Only stdout is
///      diffed; a non-zero exit fails the artifact loudly instead of being
///      propagated to a Python caller.
fn verify_shell_boundary(
    session: &TranspileSession,
    manifest: &FfiManifest,
    modules: &[Module],
    sh_entries: &[&FfiEntry],
    c_sources: &[(String, String)],
    sh_sources: &[(String, String)],
) -> Result<()> {
    // The artifact spawns the re-emitted script from a workspace-local `bin/`
    // dir put on PATH, which needs the unix executable bit.
    if !cfg!(unix) {
        println!("  --verify: shell boundary execution needs a unix host — skipping");
        return Ok(());
    }
    // Toolchain gate — graceful-skip so a constrained runner stays green. `cc`
    // is only needed when the same workspace also carries C sources.
    if !tool_available("sh")
        || !tool_available("cargo")
        || (!c_sources.is_empty() && !tool_available("cc"))
    {
        println!(
            "  --verify: sh/cargo (or cc for the C sources) unavailable — skipping shell execution"
        );
        return Ok(());
    }

    let shell_backend = session
        .backends
        .iter()
        .find(|b| b.targets().contains(&Target::Shell))
        .context("no Shell backend registered")?;
    let config = BackendConfig {
        emit_contracts: true,
        target: Target::Shell,
        profile: Profile::RustOut,
        hardware: None,
    };

    // Pair every shell boundary with (a) its defining Shell module and (b) that
    // module's ORIGINAL source text. A boundary that cannot be paired is
    // reported and the whole lane skips — a partial differential would compare
    // one program's stdout against another's.
    let mut programs: Vec<(String, String, String)> = Vec::new(); // (prog, original, re-emitted)
    for e in sh_entries {
        let Some(m) = modules
            .iter()
            .find(|m| m.source_lang == SourceLang::Shell && m.name == e.symbol)
        else {
            println!(
                "  --verify: shell boundary `{}` has no sibling script module — skipping",
                e.symbol
            );
            return Ok(());
        };
        let Some((_, original)) = sh_sources.iter().find(|(f, _)| {
            Path::new(f).file_stem().and_then(|s| s.to_str()) == Some(m.name.as_str())
        }) else {
            println!(
                "  --verify: shell boundary `{}` has no retained source text — skipping",
                e.symbol
            );
            return Ok(());
        };
        let emitted = shell_backend
            .lower(m, &config)
            .with_context(|| format!("lowering shell module `{}` back to POSIX", m.name))?;
        programs.push((m.name.clone(), original.clone(), emitted.primary));
    }

    // Reference: the ORIGINAL scripts, run in boundary order under `sh`.
    let mut reference = String::new();
    for (prog, original, _) in &programs {
        reference.push_str(&run_sh_script(prog, original)?);
    }
    let reference = reference.trim_end_matches('\n').to_string();

    // Artifact: the hybrid workspace, with a generated driver that calls each
    // emitted shim, and the RE-EMITTED scripts materialized on PATH.
    let ws = std::env::temp_dir().join(format!("xpile_verify_sh_ws_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    let driver = shell_driver_src(sh_entries);
    manifest
        .emit_hybrid_workspace(modules, c_sources, &driver, &ws)
        .map_err(|e| anyhow::anyhow!("workspace emit failed: {e}"))?;
    let bindir = ws.join("bin");
    std::fs::create_dir_all(&bindir).context("creating the workspace bin/ dir")?;
    for (prog, _, emitted) in &programs {
        let p = bindir.join(prog);
        std::fs::write(&p, emitted).with_context(|| format!("writing {}", p.display()))?;
        make_executable(&p)?;
    }

    let target = ws.join("target");
    let build = Command::new("cargo")
        .current_dir(&ws)
        .arg("build")
        .arg("--target-dir")
        .arg(&target)
        .output()
        .context("cargo build of the hybrid shell workspace")?;
    if !build.status.success() {
        let _ = std::fs::remove_dir_all(&ws);
        bail!(
            "hybrid shell artifact failed to build:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
    }
    // The shim spawns by PROGRAM NAME, so the re-emitted scripts go on PATH.
    let path_var = prepend_path(&bindir)?;
    let run = Command::new(target.join("debug").join("xpile-hybrid-artifact"))
        .env("PATH", &path_var)
        .output()
        .context("running the hybrid shell artifact")?;
    let _ = std::fs::remove_dir_all(&ws);
    if !run.status.success() {
        bail!(
            "hybrid shell artifact exited {}:\n{}",
            run.status,
            String::from_utf8_lossy(&run.stderr)
        );
    }
    let actual = String::from_utf8_lossy(&run.stdout)
        .trim_end_matches('\n')
        .to_string();

    let names: Vec<&str> = programs.iter().map(|(p, _, _)| p.as_str()).collect();
    println!(
        "  --verify: `sh` reference (original {}) vs executed shim-spawned artifact:",
        names.join(", ")
    );
    differential_verdict(&reference, &actual, "sh:", "the `sh` reference")
}

/// Run one script's ORIGINAL text under `sh` and capture stdout — the shell
/// lane's reference side. The text is written to a temp file (rather than
/// `sh -c`) so `$0`/`$@` and a shebang behave as they do for the real file.
fn run_sh_script(prog: &str, source: &str) -> Result<String> {
    let p = std::env::temp_dir().join(format!("xpile_verify_ref_{}_{prog}", std::process::id()));
    std::fs::write(&p, source).with_context(|| format!("writing {}", p.display()))?;
    let out = Command::new("sh").arg(&p).output();
    let _ = std::fs::remove_file(&p);
    let out = out.context("spawning sh for the reference run")?;
    if !out.status.success() {
        bail!(
            "reference `sh {prog}` exited {}:\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// The generated driver for the shell lane's artifact. Calls each boundary's
/// emitted shim through the workspace's `use ffi_shims::<sym>_shim as <sym>;`
/// alias — so the alias bridge is exercised exactly as it is on the C lane —
/// and reprints the captured stdout verbatim. Args are `&[]`: see the
/// `verify_shell_boundary` docs, item 1.
fn shell_driver_src(sh_entries: &[&FfiEntry]) -> String {
    let mut body = String::new();
    for e in sh_entries {
        let sym = &e.symbol;
        body.push_str(&format!(
            "    let out = {sym}(&[]).unwrap_or_else(|e| panic!(\"spawning shell boundary `{sym}`: {{e}}\"));\n\
             \x20   if !out.status.success() {{\n\
             \x20       eprintln!(\"shell boundary `{sym}` exited {{}}\", out.status);\n\
             \x20       ::std::process::exit(1);\n\
             \x20   }}\n\
             \x20   print!(\"{{}}\", String::from_utf8_lossy(&out.stdout));\n"
        ));
    }
    format!("fn main() {{\n{body}}}\n")
}

/// `chmod +x` on a materialized script. Unix-only by construction — the caller
/// already skipped the whole lane on a non-unix host.
fn make_executable(p: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))
            .with_context(|| format!("chmod +x {}", p.display()))?;
    }
    #[cfg(not(unix))]
    let _ = p;
    Ok(())
}

/// `PATH` with `dir` PREPENDED — the artifact's shim spawns the boundary by
/// bare program name, so the re-emitted script must win over any same-named
/// program already installed on the host.
fn prepend_path(dir: &Path) -> Result<std::ffi::OsString> {
    let mut entries = vec![dir.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        entries.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(entries).context("composing PATH for the hybrid shell artifact")
}

fn print_info(session: &TranspileSession) -> Result<()> {
    println!("xpile — polyglot transpile workbench");
    println!();

    println!("Code lane:");
    // PMAT-1346: a routing-only frontend (registered so its extension reaches
    // a specific refusal, but with no parser) must not be silently counted
    // among the languages xpile READS. Report both numbers and mark the
    // refusing entries — `README.md` points here as "the live registry", so
    // this listing is a claim surface like any other.
    let lowering = session
        .frontends
        .iter()
        .filter(|f| f.lowers_input())
        .count();
    let registered = session.frontends.len();
    if lowering == registered {
        println!("  frontends ({registered}):");
    } else {
        println!("  frontends ({registered} registered, {lowering} lowering):");
    }
    for f in &session.frontends {
        // PMAT-1433: `lowers_input()` is a WHOLE-FRONTEND boolean, so a
        // frontend that lowers SOME of what it claims printed flush with one
        // that lowers all of it. `bashrs` declares `true` (`.sh` lowers) while
        // `*.mk` / `Makefile` / `Dockerfile` have refused unconditionally since
        // PMAT-1420 — this line read as "the POSIX parser handles `mk`". The
        // routing-only suffix is kept verbatim for the whole-frontend case;
        // the partial case APPENDS a second, distinct suffix rather than
        // reshaping the line (PMAT-1428).
        let suffix = if f.lowers_input() {
            let refused = f.refused_claims();
            if refused.is_empty() {
                String::new()
            } else {
                format!("  [claims REFUSED — no parser: {}]", refused.join(", "))
            }
        } else {
            "  [routing only — INPUT refuses, no parser]".to_string()
        };
        println!("    - {} ({}){suffix}", f.name(), f.extensions().join(", "));
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
    // PMAT-1429: the proof-lane twin of the PMAT-1346 sweep 14 lines above.
    // A contract backend that returns a fixed `_scaffold` payload for every
    // contract must not be counted, silently, among the formats xpile
    // RENDERS — `book/src/reference/cli.md` reproduces this listing and tells
    // the reader to "confirm your install can see every lane".
    let rendering = session
        .contract_backends
        .iter()
        .filter(|b| b.renders_contract_body())
        .count();
    let registered_cb = session.contract_backends.len();
    if rendering == registered_cb {
        println!("  contract_backends ({registered_cb}):");
    } else {
        println!("  contract_backends ({registered_cb} registered, {rendering} rendering):");
    }
    for cb in &session.contract_backends {
        let fmts: Vec<String> = cb.formats().iter().map(|f| format!("{:?}", f)).collect();
        let suffix = if cb.renders_contract_body() {
            ""
        } else {
            "  [scaffold — fixed `_scaffold` payload, ignores the contract]"
        };
        println!("    - {} → {}{suffix}", cb.name(), fmts.join(", "));
    }
    Ok(())
}

/// Every path spelling the frontend registry CLAIMS, split by disposition:
/// `(spellings that lower, spellings that are routed and then refused)`.
///
/// PMAT-1434. The two halves are the registry's own declarations —
/// [`Frontend::extensions`] plus [`Frontend::refused_claims`] — rendered in the
/// one vocabulary `refused_claims()` and `book/src/reference/frontends.md` both
/// use: a `*.<ext>` glob for an extension, an exact filename for the
/// extensionless spellings `matches_path` claims. Registration order is
/// preserved so the output is deterministic.
///
/// `refused_claims()` is a DECLARATION; it is confronted with behaviour at
/// every claimed spelling by
/// `crates/xpile/tests/frontend_claim_disposition_witness.rs`
/// (XPILE-FRONTEND-CLAIM-001), so a caller reading this split is reading a
/// behaviour-checked fact and not a self-report.
///
/// PMAT-1443: the per-frontend derivation moved to
/// [`Frontend::spellings_by_disposition`] so that the OTHER surfaces
/// rendering the registry — `xpile audit`'s no-source bail and
/// `examples/06_inspect_session.rs`, both of which were still publishing the
/// flat `extensions()` union — share one implementation instead of each
/// re-deriving it. This function is now the session-wide `All`-scope fold.
fn claimed_spellings_by_disposition(session: &TranspileSession) -> (Vec<String>, Vec<String>) {
    let mut lowers = Vec::new();
    let mut refused = Vec::new();
    for f in &session.frontends {
        let (l, r) = f.spellings_by_disposition(SpellingScope::All);
        lowers.extend(l);
        refused.extend(r);
    }
    (lowers, refused)
}

/// The `xpile audit` no-source bail: what a user sees when the corpus they
/// pointed `audit` at holds nothing it can collect.
///
/// PMAT-1443. This used to print `xpile recognises .bash, .c, .h, .mk, .py,
/// .pyi, .ruchy, .sh, .wat, .zsh` — the flat `extensions()` union, the exact
/// defect PMAT-1434 removed from the dispatch-failure message, still live in
/// the same file 500 lines away and still ungated. Measured at 1e251c70: two
/// of those ten spellings, `.mk` and `.ruchy`, REFUSE every input, so on the
/// one surface where the reader is asking "what should I point this at?" one
/// answer in five was wrong.
///
/// Two things make this message's set narrower than the dispatch message's,
/// and getting either wrong swaps one over-report for another:
///
/// 1. It is [`SpellingScope::Extensions`], not `All`. [`collect_source_files`]
///    walks by EXTENSION, so the extensionless spellings `matches_path`
///    claims (`Makefile`, `Dockerfile`) are never collected no matter how the
///    corpus is arranged — naming them here would advertise a spelling that
///    cannot work. The message discloses that exclusion instead, derived from
///    the difference between the two scopes rather than spelled out, so a new
///    extensionless claim joins the sentence on landing.
/// 2. A routed-but-refused extension is NOT "unrecognised". A `.ruchy` file
///    IS collected, IS counted in `files scanned`, and lands in the error
///    list — verified, not assumed. Folding it in with `.py` under one verb
///    ("recognises") is what erased the difference.
///
/// Pinned to the registry and to the collector's real behaviour by
/// `crates/xpile/tests/audit_claim_disposition_witness.rs`
/// (XPILE-AUDITCLAIM-001).
fn audit_no_source_message(session: &TranspileSession, path: &Path) -> String {
    let mut lowers = Vec::new();
    let mut refused = Vec::new();
    let mut uncollectable = Vec::new();
    for f in &session.frontends {
        let (l, r) = f.spellings_by_disposition(SpellingScope::Extensions);
        lowers.extend(l);
        // Everything the DISPATCH claim covers beyond the extension walk.
        let (_, all_refused) = f.spellings_by_disposition(SpellingScope::All);
        uncollectable.extend(all_refused.into_iter().filter(|c| !r.contains(c)));
        refused.extend(r);
    }
    let mut msg = format!(
        "audit found no source file under {} — audit collects BY EXTENSION; \
         spellings that LOWER: {lowers:?}",
        path.display()
    );
    if !refused.is_empty() {
        msg.push_str(&format!(
            "; ROUTED but REFUSED (no parser — a file with one of these IS \
             collected and reported as an error, never as coverage): {refused:?}"
        ));
    }
    if !uncollectable.is_empty() {
        msg.push_str(&format!(
            "; NOT collected at all (claimed by `matches_path` for `xpile \
             transpile`, but the audit walk is extension-only): {uncollectable:?}"
        ));
    }
    msg.push_str("; nothing was scanned, so there is no F1 to report");
    msg
}

/// The dispatch-failure message: what a user sees when no frontend claims
/// their file.
///
/// PMAT-1434. This used to print `known extensions: {extensions():?}` — the
/// flat union of the routing table. Routing and capability are deliberately
/// the SAME list here: `ruchy-frontend` and `bashrs-frontend` both keep a
/// refusing spelling in `extensions()` on purpose, so that a `.ruchy` / `.mk`
/// file reaches their specific refusal instead of degrading to THIS message
/// (see `crates/ruchy-frontend/src/lib.rs` and PMAT-1420). That decision is
/// right, and it makes `extensions()` a routing set — which this message was
/// presenting as the set of things xpile can read. Two of the ten spellings it
/// named, `ruchy` and `mk`, refuse every input; the two extensionless
/// spellings `matches_path` claims never appeared in it at all. Over-reported
/// and under-reported in one line, on the error path, where the reader is
/// looking for what to use instead.
///
/// So the disposition is now carried, in the same vocabulary `xpile info` and
/// the book use. Pinned to the registry by
/// `crates/xpile/tests/frontend_dispatch_message_witness.rs`
/// (XPILE-FRONTEND-CLAIM-002).
fn no_frontend_message(session: &TranspileSession, ext_label: &str) -> String {
    let (lowers, refused) = claimed_spellings_by_disposition(session);
    let head = format!("no frontend handles {ext_label}; spellings that LOWER: {lowers:?}");
    if refused.is_empty() {
        head
    } else {
        format!("{head}; ROUTED but REFUSED (no parser): {refused:?}")
    }
}

fn transpile(
    session: &TranspileSession,
    input: &Path,
    target_str: &str,
    out: Option<&Path>,
    emit_crate: Option<&Path>,
    contracts: &str,
    hardware: Option<&str>,
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
            no_frontend_message(session, &ext_label)
        })?;

    // PMAT-1024: resolve the target BEFORE lowering — the frontend's alias
    // dispositions are target-aware (a reference-semantics target executes
    // Python object sharing natively, so the clone/move/refuse suite is
    // skipped for pointer-stable types there).
    let target = parse_target(target_str)?;

    // `--emit-crate` produces a Rust crate; validate the target up front so a
    // flag misuse fails fast with a clear message rather than surfacing as a
    // downstream backend-lowering error.
    if emit_crate.is_some() && target != Target::Rust {
        bail!("--emit-crate produces a Rust crate; use it with --target rust (got {target:?})");
    }

    // PMAT-1385: `--out` and `--emit-crate` are two different output
    // destinations, and the write path below returns on `--emit-crate`
    // FIRST — so passing both wrote the crate, silently dropped `--out`,
    // and exited 0. The caller had asked for a file that was never created.
    if emit_crate.is_some() && out.is_some() {
        bail!(
            "--out and --emit-crate are two different output destinations; pass one \
             (--emit-crate writes a crate directory, --out writes a single file)"
        );
    }

    // PMAT-1385: the CLI's `--hardware` grammar only builds a PTX profile
    // (see `parse_hardware`), so on any other target it was accepted and then
    // ignored — `--target rust --hardware ptx:sm_89` exited 0 emitting plain
    // Rust, with nothing said about the compute capability the caller asked
    // for. SPIR-V and WGSL already refuse a foreign `HwProfile` from inside
    // their backends; this makes the whole flag honest at the CLI boundary
    // and names the target that ignored it. The VALUE is parsed FIRST so a
    // misspelling still reports as one, on every target.
    let hw = parse_hardware(hardware)?;
    if hw.is_some() && target != Target::Ptx {
        bail!(
            "--hardware supplies a PTX compute capability and is consumed by --target ptx only; \
             {target:?} ignores it — drop --hardware (got `{}`)",
            hardware.unwrap_or_default()
        );
    }

    let module = frontend
        .parse_and_lower_profiled(input, &source, lowering_profile_for(target))
        .with_context(|| format!("parse_and_lower failed for {}", input.display()))?;

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
        // XPILE-PTX-001: `--hardware ptx[:sm_XX]` supplies the PTX compute
        // capability so `--target ptx` is CLI-reachable (was hardcoded `None`,
        // making every `transpile --target ptx` refuse with MissingHardware).
        // Omitting `--hardware` keeps the prior `None` for all other targets;
        // PMAT-1385 refuses it on them rather than ignoring it.
        hardware: hw,
        // PMAT-956: `--contracts off` suppresses citation emission. The config
        // drives it, so every `Backend::lower` honours it directly (rather than
        // a post-emit strip). Default `on` keeps every citation across the
        // applicable L1–L5 taxonomy layers.
        emit_contracts: contracts != "off",
    };

    let artifact = backend
        .lower(&module, &config)
        .with_context(|| format!("backend `{}` failed", backend.name()))?;
    let primary = artifact.primary;

    // `--emit-crate`: write a complete, buildable binary crate instead of
    // printing. The crate compiles as-is for the native host AND for
    // `wasm32-wasip1` (a single portable `.wasm` — the universal-binary path).
    // The `--target rust` invariant is validated up front (see above).
    if let Some(dir) = emit_crate {
        return write_crate(dir, input, &primary);
    }

    match out {
        Some(path) => {
            std::fs::write(path, &primary)
                .with_context(|| format!("writing {}", path.display()))?;
            eprintln!("xpile: wrote {}", path.display());
        }
        None => print!("{}", primary),
    }
    Ok(())
}

/// Emit a complete, buildable binary crate (`Cargo.toml` + `src/main.rs`) from
/// the transpiled Rust so the program can be `cargo build`'d directly. When the
/// program defines `main()`, the crate is runnable — including for
/// `wasm32-wasip1`, which yields a single portable `.wasm` that runs on any
/// OS/arch under a WASI runtime (the "universal binary" path). The `indexmap`
/// dependency is added only when the emitted Rust references it (i.e. the
/// program uses a Python `dict`).
fn write_crate(dir: &Path, input: &Path, rust: &str) -> Result<()> {
    // Derive a valid crate name from the input file stem: lowercase, non
    // alphanumeric → `_`, and a leading digit (or empty) gets an `app_` prefix.
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("app");
    let mut name: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        name = format!("app_{name}");
    }

    let deps = if rust.contains("indexmap::") {
        // Python `dict` lowers to `indexmap::IndexMap` for insertion order.
        "indexmap = \"2\"\n"
    } else {
        ""
    };
    if !rust.contains("fn main(") {
        eprintln!(
            "xpile: note: the emitted crate has no `fn main` — add a Python \
             `def main() -> None:` entry point to produce a runnable binary"
        );
    }

    let cargo_toml = format!(
        "[package]\n\
         name = \"{name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2021\"\n\
         \n\
         [[bin]]\n\
         name = \"{name}\"\n\
         path = \"src/main.rs\"\n\
         \n\
         [dependencies]\n\
         {deps}\
         \n\
         # opt-level \"z\" keeps the emitted `.wasm` small.\n\
         [profile.release]\n\
         opt-level = \"z\"\n"
    );

    let src_dir = dir.join("src");
    std::fs::create_dir_all(&src_dir).with_context(|| format!("creating {}", src_dir.display()))?;
    std::fs::write(dir.join("Cargo.toml"), cargo_toml)
        .with_context(|| format!("writing {}", dir.join("Cargo.toml").display()))?;
    std::fs::write(src_dir.join("main.rs"), rust)
        .with_context(|| format!("writing {}", src_dir.join("main.rs").display()))?;
    eprintln!(
        "xpile: wrote crate `{name}` to {} \
         (cargo build --target wasm32-wasip1 → a portable .wasm)",
        dir.display()
    );
    Ok(())
}

/// PMAT-1024: which binding model the target's data layout gives bindings.
/// WASM linear memory holds container/struct locals as i32 base-pointers, so
/// a binding copy IS Python's object sharing (`AliasSemantics::Reference`);
/// every other target keeps the value-semantics dispositions.
fn alias_semantics_for(target: Target) -> AliasSemantics {
    match target {
        Target::Wasm => AliasSemantics::Reference,
        _ => AliasSemantics::Value,
    }
}

/// PMAT-1034: the full lowering profile — alias semantics plus whether the
/// target can express a runtime abort. Rust/Ruchy `panic!` and the WASM
/// `unreachable` trap can carry the empty-iterable loop-var-leak guard (the
/// `UnboundLocalError` analogue); PTX/WGSL/SPIR-V/Lean/shell have no portable
/// abort, so the guard is not emitted there (refusing those shapes instead
/// would block programs they execute exactly on every non-empty input).
fn lowering_profile_for(target: Target) -> LoweringProfile {
    LoweringProfile {
        alias_semantics: alias_semantics_for(target),
        runtime_abort: matches!(target, Target::Rust | Target::Ruchy | Target::Wasm),
    }
}

/// XPILE-TARGET-SPELL-001 (PMAT-1435): the SINGLE roster of `--target`
/// spellings the CLI accepts. `parse_target` matches through it and
/// [`target_spelling_help`] renders the refusal message from it, so the
/// "what can I use instead" list cannot enumerate fewer spellings than the
/// binary takes — the PMAT-1434 defect one flag over.
///
/// The third field is the CANONICAL spelling an entry aliases, or `None` for
/// a canonical entry. Keeping aliases IN the roster rather than as extra
/// `|` arms is what makes the accepted set derivable from behaviour: before
/// this, four spellings (`wat`, `sh`, `bash`, `forjar-yaml`) were accepted by
/// `parse_target` and named by NO surface the binary prints, so every gate
/// that modelled "what the CLI accepts" modelled it from `--help` prose and
/// was short by four.
const TARGET_SPELLINGS: &[(&str, Target, Option<&str>)] = &[
    ("rust", Target::Rust, None),
    ("ruchy", Target::Ruchy, None),
    ("ptx", Target::Ptx, None),
    ("wgsl", Target::Wgsl, None),
    ("spirv", Target::Spirv, None),
    // PMAT-951: native WASM emit — `--target wasm` resolves to the
    // xpile-wasm-codegen WAT emitter (scalar/control subset).
    ("wasm", Target::Wasm, None),
    ("wat", Target::Wasm, Some("wasm")),
    ("lean", Target::Lean, None),
    // PMAT-037 / XPILE-BASHRS-MERGER-001: shell target accepted
    // via the bashrs-backend scaffold.
    ("shell", Target::Shell, None),
    ("sh", Target::Shell, Some("shell")),
    ("bash", Target::Shell, Some("shell")),
    // PMAT-953: forjar.yaml IaC manifest (BACKEND-ONLY). Lowers a
    // SHELL-origin command sequence to forjar `type: file`/`type: task`
    // resources via xpile-forjar-codegen.
    ("forjar", Target::ForjarYaml, None),
    ("forjar-yaml", Target::ForjarYaml, Some("forjar")),
];

/// The `--target` vocabulary, rendered for a human AND parseable by a gate:
/// `choose: <canonical, …>; aliases: <spelling>=<canonical>, …`. Both halves
/// come from [`TARGET_SPELLINGS`], so neither can drift from `parse_target`.
fn target_spelling_help() -> String {
    let canonical: Vec<&str> = TARGET_SPELLINGS
        .iter()
        .filter(|(_, _, alias_of)| alias_of.is_none())
        .map(|(spelling, _, _)| *spelling)
        .collect();
    let aliases: Vec<String> = TARGET_SPELLINGS
        .iter()
        .filter_map(|(spelling, _, alias_of)| alias_of.map(|c| format!("{spelling}={c}")))
        .collect();
    format!(
        "choose: {}; aliases: {}",
        canonical.join(", "),
        aliases.join(", ")
    )
}

fn parse_target(s: &str) -> Result<Target> {
    match TARGET_SPELLINGS
        .iter()
        .find(|(spelling, _, _)| *spelling == s)
    {
        Some((_, target, _)) => Ok(*target),
        None => bail!("unknown target `{s}`; {}", target_spelling_help()),
    }
}

/// XPILE-PTX-001: parse the optional `--hardware` profile. `None` (flag
/// omitted) keeps the prior default of no hardware profile, so every existing
/// invocation is byte-identical. `ptx` selects `HwProfile::Ptx` at the
/// contract-floor compute capability `sm_80`; `ptx:sm_89` overrides the
/// capability. PTX is the only profile the CLI plumbs today (WGSL/SPIR-V emit
/// under their backend defaults); an unknown value fails fast with a clear
/// message rather than silently ignoring the flag.
fn parse_hardware(s: Option<&str>) -> Result<Option<HwProfile>> {
    let s = match s {
        Some(s) => s,
        None => return Ok(None),
    };
    let (kind, cap) = s.split_once(':').unwrap_or((s, ""));
    match kind {
        "ptx" => {
            let compute_capability = if cap.is_empty() { "sm_80" } else { cap }.to_string();
            // PMAT-1413: fail fast at the flag, with the flag's own wording.
            // The AUTHORITATIVE refusal is in `xpile_ptx_codegen::emit`
            // (library callers bypass this function entirely) — this call
            // shares that one grammar rather than restating it, so the two
            // can never drift.
            xpile_ptx_codegen::validate_compute_capability(&compute_capability)
                .map_err(|e| anyhow::anyhow!("--hardware ptx:<cap> — {e}"))?;
            Ok(Some(HwProfile::Ptx { compute_capability }))
        }
        other => bail!(
            "unknown --hardware `{other}`; the CLI plumbs: ptx (optionally ptx:sm_XX, e.g. ptx:sm_89)"
        ),
    }
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
// syntax differs (Rust/Ruchy use `// xpile-contract:`, the Lean CODE lane
// a `/-- xpile-contract: ... -/` docstring — PMAT-1405); this CLI exposes the Rust/Ruchy form
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
    /// The F1 ratio, or `None` when the denominator is 0 — i.e. when the
    /// corpus contains no function the citation pipeline is supposed to fire
    /// on, because every file failed to lower or none did citable work.
    ///
    /// PMAT-1385: this used to return a flat `100.0` for the 0 denominator
    /// ("vacuously satisfied, so a small corpus doesn't trip the falsifier").
    /// The convention itself is defensible; reporting it through the SAME
    /// channel as a measured ratio is not. A corpus that was never measured
    /// then looked, in text, in `--json`, and in the exit status, exactly like
    /// one measured at ceiling. Callers now have to handle the `None` and say
    /// so — see [`AuditReport::f1_status`].
    fn coverage_pct(&self) -> Option<f64> {
        if self.functions_requiring_citation == 0 {
            return None;
        }
        Some(
            (self.functions_with_citation as f64) / (self.functions_requiring_citation as f64)
                * 100.0,
        )
    }

    /// The ratio as it is DISPLAYED, truncated toward zero at one decimal.
    ///
    /// PMAT-1385: the reporters printed `{:.1}`, which ROUNDS — so 2166 of
    /// 2167 cited (99.954%) rendered as a flat `100.0%`, a ceiling claim for a
    /// corpus with a miss in it. Truncating can only understate, never
    /// overstate, which is the right direction for a coverage metric: 99.954
    /// now shows as `99.9%` and the missing citation is visible.
    fn display_pct(&self) -> Option<f64> {
        self.coverage_pct().map(|p| (p * 10.0).floor() / 10.0)
    }

    /// F1 status per the roadmap's targets:
    ///   ≥ 95% → OK    (target reached)
    ///   < 95% but ≥ 50% → WARN (below target, above falsifier)
    ///   < 50%  → FAIL (falsifier tripped — the citation pipeline is performative)
    ///   nothing measured → VACUOUS (PMAT-1385 — *not* OK)
    fn f1_status(&self) -> &'static str {
        let Some(pct) = self.coverage_pct() else {
            return "VACUOUS";
        };
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
    // F1 now supports Rust, Ruchy, AND Lean — XPILE-FALSIFY-002 added the
    // Lean citation form (PMAT-1405: a `/-- xpile-contract: ... -/` docstring,
    // previously the unparseable `@[xpile_contract "..."]` attribute).
    // PTX / WGSL / SPIR-V citations are XPILE-FALSIFY-003+.
    if !matches!(target, Target::Rust | Target::Ruchy | Target::Lean) {
        bail!(
            "`xpile audit` supports --target rust | ruchy | lean; {target:?} citation form not yet known — follow-up XPILE-FALSIFY-003"
        );
    }

    // PMAT-1385: refuse before reporting. `collect_source_files` returns an
    // empty vec for a path that is neither a file nor a directory, so a
    // typo'd or renamed corpus path used to scan 0 files and print a ceiling
    // F1 with exit 0 — a CI dashboard pointed at a moved directory reported a
    // perfect falsifier score indefinitely. A bad path is an INPUT error (the
    // `transpile` subcommand has always treated it as one); an unmeasurable
    // but real corpus is a measurement OUTCOME and is reported, not refused.
    if !path.exists() {
        bail!(
            "audit path {} does not exist — nothing was scanned, so there is no F1 to report",
            path.display()
        );
    }

    let mut report = AuditReport::default();
    let sources = collect_source_files(session, path);
    if sources.is_empty() {
        bail!("{}", audit_no_source_message(session, path));
    }
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
            emit_contracts: true,
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
            // PMAT-502bj: only functions carry contract citations; skip consts.
            let xpile_meta_hir::Item::Function(f) = item else {
                continue;
            };
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
///   Rust:  `// xpile-contract: <ID>`      prefix `pub fn <name>(`
///   Ruchy: `// xpile-contract: <ID>`      prefix `fun <name>(`
///   Lean:  `/-- xpile-contract: <ID> -/`  prefix `def <name> (` / `partial def <name> (`
///
/// PMAT-1405: the Lean marker was `@[xpile_contract`. The code lane now cites
/// with a docstring, because the attribute is registered as a Lean attribute
/// nowhere and `lean` rejected the default emit outright. This marker must move
/// with the emitter — leaving it on the attribute would score every Lean
/// function UNCITED and collapse the audit metric through the falsifier floor,
/// which `audit_command_supports_lean_target` pins.
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
        Target::Lean => "/-- xpile-contract:",
        _ => return false,
    };
    let needle = format!("{function_name}(");
    let needle_space = format!("{function_name} (");

    let lines: Vec<&str> = source.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let stripped = line.trim_start();
        let is_decl = prefixes.iter().any(|p| {
            if !stripped.starts_with(p) {
                return false;
            }
            // PMAT-1385: a Python/C name that collides with a Rust keyword is
            // emitted as a RAW identifier (`def move` → `pub fn r#move`, and
            // Ruchy does the same). Matching the bare name missed every such
            // declaration, so the function was counted in the F1 denominator
            // and never found in the numerator — the citation was emitted, the
            // detector just could not see it. That is an under-count of the
            // metric, i.e. the reporter reporting a number that is not true.
            let rest = &stripped[p.len()..];
            let rest = rest.strip_prefix("r#").unwrap_or(rest);
            rest.starts_with(&needle) || rest.starts_with(&needle_space)
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
    match report.display_pct() {
        Some(pct) => println!(
            "  coverage (F1)       : {:.1}%   [{}]",
            pct,
            report.f1_status()
        ),
        // PMAT-1385: no denominator ⇒ no ratio. Printing `100.0% [OK]` here
        // made an unmeasured corpus read as a measured, perfect one.
        None => {
            println!("  coverage (F1)       : n/a      [{}]", report.f1_status());
            println!(
                "                        (no function in the scanned corpus requires a citation — nothing was measured)"
            );
        }
    }
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
    // `over_citations` sanity field. PMAT-1385 made `f1_pct` NULLABLE: a
    // dashboard reading this payload cannot otherwise tell a corpus measured
    // at ceiling from one that was never measured, and every unmeasured
    // corpus used to arrive as `"f1_pct":100.0,"f1_status":"OK"`.
    let pct = match report.display_pct() {
        Some(p) => format!("{p:.1}"),
        None => "null".to_string(),
    };
    println!(
        "{{\"target\":\"{:?}\",\"files_scanned\":{},\"functions_emitted\":{},\"functions_requiring_citation\":{},\"functions_with_citation\":{},\"over_citations\":{},\"f1_pct\":{},\"f1_status\":\"{}\",\"errors\":{}}}",
        target,
        report.files_scanned,
        report.functions_emitted,
        report.functions_requiring_citation,
        report.functions_with_citation,
        report.over_citations,
        pct,
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
    ///
    /// `None` when the mention lies OUTSIDE every work-item block —
    /// in `docs/roadmaps/roadmap.yaml` that is the `strategic_goals:`
    /// preamble (lines 1..189; `roadmap:` is line 190 and the first
    /// `- id:` is 191). PMAT-1390: this was an empty `String`, which
    /// both printers folded into the unique-work-item set as if it
    /// were a work item named "". The live tally read
    /// `C-PY-INT-ARITH 87 mentions across 69 work item(s)` when a real
    /// YAML parser counts 68, and the text printer emitted a nameless
    /// `      - ` bullet for the phantom. A preamble mention is a REAL
    /// mention of the contract — it is just not an attestation by a
    /// work item, so it is retained in `mentions` (and therefore in
    /// the count `quorum` scores the Extrinsic stratum from) and
    /// excluded from the work-item tally.
    work_item: Option<String>,
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
    let roadmap = read_roadmap(roadmap_path)?;
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

/// The contract corpus embedded into this binary at build time, as
/// `(file name, contents)` pairs sorted by file name. Generated by
/// `build.rs` from `crates/xpile/contracts/` (a symlink to the canonical
/// workspace-root `contracts/`).
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded_contracts.rs"));
}

/// The default value of every reporter's `--contracts-dir`. It is
/// CWD-relative, which is why the embedded fallback exists.
const DEFAULT_CONTRACTS_DIR: &str = "contracts";

/// Where a reporter's contract corpus was read from. Reported so a
/// consumer never has to guess whether a table describes the checkout in
/// front of them or the release they installed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CorpusSource {
    /// The on-disk `contracts/` directory of a source checkout.
    Disk,
    /// The copy compiled into this binary (PMAT-1407).
    Embedded,
}

/// Load the contract corpus the three reporters (`attestations`, `quorum`,
/// `diamond`) tally over, as `(file name, contents)` pairs sorted by name.
///
/// PMAT-1407. Resolution order, and the reasoning for each arm:
///
///  1. `contracts_dir` IS a directory — read it. A source checkout keeps
///     working byte-for-byte, so every existing gate measures exactly what
///     it measured before.
///  2. It is absent AND still the default — fall back to the embedded
///     corpus. This is the arm that repairs `cargo install xpile`: the
///     binary has no checkout beside it, and before this the default
///     failed with `contracts is not a directory`, naming a path the user
///     never supplied. The fallback ANNOUNCES itself on stderr rather than
///     substituting silently, because the two corpora can legitimately
///     differ (an installed 0.1.618 binary run inside a newer checkout).
///  3. It is absent and was given EXPLICITLY — still an error. A user who
///     typed a path wants that path; quietly reporting on a different
///     corpus would be a wrong answer at exit 0.
fn load_contract_corpus(contracts_dir: &Path) -> Result<(CorpusSource, Vec<(String, String)>)> {
    if contracts_dir.is_dir() {
        let mut out: Vec<(String, String)> = Vec::new();
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
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            out.push((name.to_string(), contents));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        return Ok((CorpusSource::Disk, out));
    }
    if contracts_dir == Path::new(DEFAULT_CONTRACTS_DIR) {
        eprintln!(
            "xpile: notice — no `{}` directory beside the current working directory; \
             reporting on the {} contract(s) embedded in this binary at build time. \
             Pass --contracts-dir <path> to read a checkout instead.",
            DEFAULT_CONTRACTS_DIR,
            embedded::EMBEDDED_CONTRACTS.len()
        );
        let out = embedded::EMBEDDED_CONTRACTS
            .iter()
            .map(|(name, contents)| ((*name).to_string(), (*contents).to_string()))
            .collect();
        return Ok((CorpusSource::Embedded, out));
    }
    bail!("{} is not a directory", contracts_dir.display());
}

/// The default value of `--roadmap` for `quorum` and `attestations`.
const DEFAULT_ROADMAP: &str = "docs/roadmaps/roadmap.yaml";

/// Read the roadmap that feeds the Extrinsic stratum.
///
/// PMAT-1407. The contract corpus can be embedded in the binary; the
/// roadmap deliberately is NOT. It is a 2.7 MB development ledger, it is
/// not a user-facing artifact, and a snapshot of it would answer "how much
/// attestation did this contract have when the release was cut" — a
/// different question from the one the reporter asks.
///
/// So `quorum` and `attestations` still refuse without a checkout. What
/// changes is that they now name the REAL missing evidence: before this,
/// the corpus read failed first and both died on `contracts is not a
/// directory`, blaming a path the user never supplied and implying the
/// install was broken rather than the command being checkout-scoped.
///
/// Refusing is the right posture, not a shortfall: `count_runtime_witnesses`
/// and this scan are the sole sources of two whole strata, and PMAT-1386
/// already established that scoring an unreadable stratum 0 turns a report
/// into a silent wrong answer (there, 702 mentions collapsed to 0 and 10 of
/// 35 contracts fell QUORUM -> PARTIAL at exit 0).
fn read_roadmap(roadmap_path: &Path) -> Result<String> {
    if !roadmap_path.exists() && roadmap_path == Path::new(DEFAULT_ROADMAP) {
        bail!(
            "no `{}` beside the current working directory. This subcommand tallies the \
             Extrinsic stratum out of the xpile development ledger, which is not part of \
             an installed release — run it from an xpile checkout, or pass --roadmap \
             <path>. (`xpile diamond` needs no ledger and works anywhere.)",
            DEFAULT_ROADMAP
        );
    }
    std::fs::read_to_string(roadmap_path)
        .with_context(|| format!("read roadmap {}", roadmap_path.display()))
}

/// Walk the contract corpus and pull each entry's `metadata.id:` value
/// out. Uses a lightweight line scanner rather than serde_yaml to keep the
/// xpile bin's dep graph small (same posture as `refinement_proofs.rs`'s
/// `extract_quoted_value` helper).
fn collect_contract_ids(contracts_dir: &Path) -> Result<Vec<String>> {
    let (_, corpus) = load_contract_corpus(contracts_dir)?;
    let mut out = Vec::new();
    for (_, contents) in &corpus {
        if let Some(id) = extract_metadata_id(contents) {
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
    let mut current_item: Option<String> = None;
    for (idx, line) in roadmap.lines().enumerate() {
        // Detect a new work item header: `- id: PMAT-NNN`.
        if let Some(rest) = line.strip_prefix("- id: ") {
            // PMAT-1390: quote-strip exactly as the sibling
            // `extract_metadata_id` does. A YAML-quoted id (`- id: "P"`)
            // was taken VERBATIM, so the quotes landed unescaped inside
            // the hand-rolled JSON string and produced `"work_item":""P""`
            // — a payload `json.load` rejects, at exit 0.
            let id = rest.trim().trim_matches('"').trim_matches('\'').trim();
            // An `- id:` with no value leaves us outside a named item
            // rather than inventing one called "".
            current_item = (!id.is_empty()).then(|| id.to_string());
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

/// The distinct work items that attest a contract. A mention with no
/// enclosing work item (`None` — the roadmap preamble) is NOT one, which
/// is the whole of PMAT-1390: this used to collect `m.work_item.as_str()`
/// over a `String` whose empty value stood for "no work item", so the
/// preamble contributed a phantom item to the set.
fn unique_work_items(mentions: &[AttestationMention]) -> std::collections::BTreeSet<&str> {
    mentions
        .iter()
        .filter_map(|m| m.work_item.as_deref())
        .collect()
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
            let unique_items = unique_work_items(&c.mentions);
            println!(
                "  {:<40}  {:>3} mentions across {:>2} work item(s)",
                c.id,
                c.mentions.len(),
                unique_items.len()
            );
            for w in &unique_items {
                println!("      - {w}");
            }
            // PMAT-1390: preamble mentions are disclosed, not dropped and
            // not counted. Dropping them would silently lower the mention
            // total that `quorum` scores the Extrinsic stratum from;
            // counting them printed a nameless `      - ` bullet.
            let preamble = c.mentions.iter().filter(|m| m.work_item.is_none()).count();
            if preamble > 0 {
                println!(
                    "      ({preamble} mention(s) outside every work item — roadmap \
                     preamble, not an attestation)"
                );
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
    // PMAT-1390, DISCLOSED RESIDUAL: `attested` still means "mentioned
    // anywhere in the roadmap", so a contract named ONLY in the preamble
    // is counted here while carrying zero work-item votes. That is 0 of 35
    // contracts on the live corpus today, which is why this line does not
    // print there — but it is a real shape and the reader is told rather
    // than left to infer it from a `0 work item(s)` row above.
    let preamble_only = report
        .contracts
        .iter()
        .filter(|c| unique_work_items(&c.mentions).is_empty())
        .count();
    if preamble_only > 0 {
        println!(
            "note: {preamble_only} of those {attested} are mentioned ONLY outside a work item \
             (roadmap preamble) and carry ZERO Extrinsic votes."
        );
    }
    println!(
        "stratum: Extrinsic — per ruchy 5.0 §14.4, this counts toward the N-of-M oracle quorum \
         alongside Semantic (Lean), Symbolic (Kani), Runtime (diff_exec)."
    );
}

fn print_attestations_json(report: &AttestationReport) {
    print!("{}", render_attestations_json(report));
}

/// Render the `--json` payload as a String so a test can assert it PARSES.
/// PMAT-1390 split this out of `print_attestations_json`: the printer wrote
/// straight to stdout, so nothing in-process could hold the bytes, and the
/// invalid-JSON defect below shipped unnoticed under a test that only
/// substring-matched the payload.
///
/// Hand-rolled JSON, same posture as `print_audit_json`. Schema:
///   {
///     "contracts_scanned": N,
///     "roadmap_path": "...",
///     "contracts": [{
///       "id": "...", "mention_count": N,
///       "work_items": [...],
///       "preamble_mentions": N,
///       "mentions": [{"work_item": "..."|null, "line": N, "snippet": "..."}]
///     }],
///     "unattested": ["...", ...]
///   }
///
/// PMAT-1390: only `snippet` was routed through `escape_json`. `roadmap_path`,
/// `work_items`, `work_item`, `id` and `unattested` were interpolated RAW, so
/// any `"` or `\` reaching them broke the payload — reproduced with nothing
/// exotic, a plain YAML-quoted work-item id (`- id: "P"`) emitted
/// `"work_item":""P""` at exit 0. Every string now goes through `escape_json`;
/// `\u{22}` is written `\"`, so a value that legitimately contains a quote
/// survives as data instead of terminating the string.
fn render_attestations_json(report: &AttestationReport) -> String {
    let mut out = String::new();
    let mut first = true;
    out.push_str(&format!(
        "{{\"contracts_scanned\":{},\"roadmap_path\":\"{}\",\"contracts\":[",
        report.contracts_scanned,
        escape_json(&report.roadmap_path.display().to_string())
    ));
    for c in &report.contracts {
        if !first {
            out.push(',');
        }
        first = false;
        let unique_items = unique_work_items(&c.mentions);
        let items_json: Vec<String> = unique_items
            .iter()
            .map(|w| format!("\"{}\"", escape_json(w)))
            .collect();
        let preamble = c.mentions.iter().filter(|m| m.work_item.is_none()).count();
        let mention_json: Vec<String> = c
            .mentions
            .iter()
            .map(|m| {
                // `null`, not `""` — a mention with no enclosing work item is
                // reported as having none, rather than as one whose id is the
                // empty string.
                let item = match &m.work_item {
                    Some(w) => format!("\"{}\"", escape_json(w)),
                    None => "null".to_string(),
                };
                format!(
                    "{{\"work_item\":{},\"line\":{},\"snippet\":\"{}\"}}",
                    item,
                    m.line,
                    escape_json(&m.snippet)
                )
            })
            .collect();
        out.push_str(&format!(
            "{{\"id\":\"{}\",\"mention_count\":{},\"work_items\":[{}],\
             \"preamble_mentions\":{},\"mentions\":[{}]}}",
            escape_json(&c.id),
            c.mentions.len(),
            items_json.join(","),
            preamble,
            mention_json.join(",")
        ));
    }
    out.push_str("],\"unattested\":[");
    let unattested_json: Vec<String> = report
        .unattested
        .iter()
        .map(|u| format!("\"{}\"", escape_json(u)))
        .collect();
    out.push_str(&unattested_json.join(","));
    out.push_str("]}\n");
    out
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
        assert!(mentions
            .iter()
            .all(|m| m.work_item.as_deref() == Some("PMAT-200")));
    }

    #[test]
    fn scan_roadmap_returns_empty_when_id_absent() {
        let yaml = "- id: PMAT-1\n  title: 'foo'\n";
        assert!(scan_roadmap_for_id(yaml, "C-NOT-PRESENT").is_empty());
    }

    /// PMAT-1390. The fixture above starts AT `roadmap:` and therefore
    /// cannot reach the defect: it has no preamble, so `current_item` is
    /// already set by the time any mention is seen. The live
    /// `docs/roadmaps/roadmap.yaml` opens with 189 lines of
    /// `strategic_goals:` prose that mentions contract ids freely, and
    /// every one of those mentions was attributed to a work item whose id
    /// is the empty string.
    #[test]
    fn a_preamble_mention_belongs_to_no_work_item() {
        let yaml = "\
strategic_goals:
  note: mentions C-FOO in the preamble
roadmap:
- id: PMAT-1
  title: unrelated
";
        let mentions = scan_roadmap_for_id(yaml, "C-FOO");
        // The mention is REAL and is retained — `quorum` scores the
        // Extrinsic stratum from this count, so dropping it would trade
        // one wrong number for another.
        assert_eq!(mentions.len(), 1, "the preamble mention must be retained");
        assert_eq!(
            mentions[0].work_item, None,
            "a preamble mention has no enclosing work item"
        );
        // The bug, stated as the property that failed: the tally.
        assert_eq!(
            unique_work_items(&mentions).len(),
            0,
            "ZERO work items attest C-FOO here; pre-PMAT-1390 this was 1"
        );
    }

    /// PMAT-1390: the preamble phantom is only ever ONE extra item no
    /// matter how many preamble lines mention the id, which is why the
    /// live over-count was exactly +1 (69 reported, 68 real) and stayed
    /// small enough to look plausible for months.
    #[test]
    fn many_preamble_mentions_still_add_no_work_item() {
        let yaml = "\
strategic_goals:
  a: C-FOO here
  b: C-FOO again
  c: C-FOO once more
roadmap:
- id: PMAT-1
  title: 'real attestation of C-FOO'
";
        let mentions = scan_roadmap_for_id(yaml, "C-FOO");
        assert_eq!(mentions.len(), 4);
        let items = unique_work_items(&mentions);
        assert_eq!(items.len(), 1);
        assert!(items.contains("PMAT-1"));
    }

    /// PMAT-1390: `- id: "P"` was taken verbatim, quotes included, unlike
    /// the sibling `extract_metadata_id` which strips them.
    #[test]
    fn scan_roadmap_strips_quotes_from_a_work_item_id() {
        for yaml in [
            "roadmap:\n- id: \"P\"\n  c: C-FOO\n",
            "roadmap:\n- id: 'P'\n  c: C-FOO\n",
        ] {
            let mentions = scan_roadmap_for_id(yaml, "C-FOO");
            assert_eq!(mentions.len(), 1);
            assert_eq!(mentions[0].work_item.as_deref(), Some("P"), "yaml: {yaml}");
        }
    }

    /// PMAT-1390: a valueless `- id:` header must not invent an item
    /// named "" — that is the same phantom by another route.
    #[test]
    fn a_valueless_id_header_opens_no_work_item() {
        let yaml = "roadmap:\n- id: \n  c: C-FOO\n";
        let mentions = scan_roadmap_for_id(yaml, "C-FOO");
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].work_item, None);
    }

    /// PMAT-1390: the `--json` payload must be parseable. Asserted here on
    /// the two inputs that broke it — a preamble mention (`""` work item)
    /// and a quoted id (unescaped `"`) — plus a path and a snippet that
    /// carry a quote and a backslash directly.
    #[test]
    fn render_attestations_json_escapes_every_string_field() {
        let roadmap = "\
strategic_goals:
  note: C-FOO in the preamble
roadmap:
- id: \"P\"
  title: 'a \\ backslash and a \" quote near C-FOO'
";
        let report = AttestationReport {
            contracts_scanned: 2,
            roadmap_path: PathBuf::from("/tmp/we\"ird\\path.yaml"),
            contracts: vec![ContractAttestation {
                id: "C-FOO".to_string(),
                mentions: scan_roadmap_for_id(roadmap, "C-FOO"),
            }],
            unattested: vec!["C-QUO\"TE".to_string()],
        };
        let json = render_attestations_json(&report);
        // The gate: it PARSES. `serde_json` is a dev-dependency of the
        // xpile package, so this costs nothing at build time.
        let v: serde_json::Value = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("payload is not JSON: {e}\n{json}"));
        let c = &v["contracts"][0];
        assert_eq!(c["mention_count"], 2);
        assert_eq!(c["preamble_mentions"], 1);
        assert_eq!(c["work_items"], serde_json::json!(["P"]));
        // The preamble mention reports `null`, never `""`.
        assert!(c["mentions"][0]["work_item"].is_null());
        assert_eq!(c["mentions"][1]["work_item"], "P");
        assert_eq!(v["roadmap_path"], "/tmp/we\"ird\\path.yaml");
        assert_eq!(v["unattested"], serde_json::json!(["C-QUO\"TE"]));
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
    witness_dirs: &[PathBuf],
    loader_dirs: &[PathBuf],
    roadmap_path: &Path,
    json: bool,
) -> Result<()> {
    // Build a map of {id -> own-yaml-text} so we can count lean/kani
    // refs in each contract's OWN file (votes for a contract come from
    // that contract's YAML, not from neighbours mentioning it).
    //
    // PMAT-1407: the corpus may be the embedded one when this binary was
    // `cargo install`ed. That fully determines the Semantic and Symbolic
    // strata (both are counted out of the contract's own YAML text), but
    // NOT Runtime or Extrinsic, which read the development tree — see the
    // notices below and the `read_roadmap` refusal further down.
    let (_corpus_source, corpus) = load_contract_corpus(contracts_dir)?;
    // TRAP (PMAT-1367): a `--witness-dir` that does not exist must never
    // silently score 0 — the default is CWD-relative and `cargo test` runs
    // with CWD = the crate dir, so a caller that forgets an absolute path
    // would get a green gate measuring nothing. Announce it once, up front,
    // naming the path; non-fatal, because `quorum` is a reporter and a caller
    // legitimately may point it at a lane that is not checked out.
    //
    // PMAT-1386: `--fixtures-dir` is the OTHER half of the Runtime union and
    // had no such notice — a missing fixtures dir silently dropped 32 of the
    // live corpus's 239 Runtime votes at exit 0. Same argument, same posture,
    // same wording: announce once, do not abort. The asymmetry was the bug.
    for dir in witness_dirs {
        if !dir.is_dir() {
            eprintln!(
                "xpile quorum: notice — --witness-dir {} is not a directory; \
                 it contributes 0 Runtime votes",
                dir.display()
            );
        }
    }
    if !fixtures_dir.is_dir() {
        eprintln!(
            "xpile quorum: notice — --fixtures-dir {} is not a directory; \
             it contributes 0 Runtime votes",
            fixtures_dir.display()
        );
    }
    // PMAT-1432: with no `--fixture-loader-dir`, the roots are `--fixtures-dir`'s
    // parent. See the flag's doc comment: a literal relative default and an
    // absolute `--fixtures-dir` would point at different trees and zero the
    // fixture pass silently — which is how this fell over the first time.
    let derived_loader_dirs: Vec<PathBuf> = if loader_dirs.is_empty() {
        fixtures_dir
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| vec![p.to_path_buf()])
            .unwrap_or_default()
    } else {
        loader_dirs.to_vec()
    };
    // The THIRD half of the same asymmetry. A `--fixture-loader-dir` that does
    // not exist makes every fixture look unloaded, dropping the whole Pass A
    // stratum at exit 0 — the mirror of PMAT-1386's missing-fixtures-dir hole,
    // and the direction that UNDER-reports. Same posture, same wording.
    for dir in &derived_loader_dirs {
        if !dir.is_dir() {
            eprintln!(
                "xpile quorum: notice — --fixture-loader-dir {} is not a directory; \
                 no fixture is counted as loaded from it",
                dir.display()
            );
        }
    }
    // PMAT-1432: the fixture half of the Runtime stratum is gated on a test
    // actually NAMING the fixture. Derived ONCE for the whole corpus — the
    // scan is over Rust sources, not contracts, so it does not vary by row.
    let loaded_fixtures =
        referenced_fixture_names(fixtures_dir, &derived_loader_dirs, witness_dirs);
    let mut rows: Vec<QuorumRow> = Vec::new();
    for (_, contents) in &corpus {
        let Some(id) = extract_metadata_id(contents) else {
            continue;
        };
        rows.push(QuorumRow {
            semantic: count_field_occurrences(contents, "lean_theorem:"),
            symbolic: count_field_occurrences(contents, "kani_harness:"),
            runtime: count_runtime_witnesses(&id, fixtures_dir, &loaded_fixtures, witness_dirs),
            extrinsic: 0, // filled below
            id,
        });
    }
    // PMAT-1386: an empty contract universe is an INPUT error, not an outcome.
    // `attestations` has always refused it; `quorum` printed a zero-row table
    // and `{"contracts":[]}` at exit 0, whose "0 UNVERIFIED" total reads to a
    // consumer as a clean pass over a universe that was never measured. Same
    // refusal, same shape, so the three reporters agree.
    if rows.is_empty() {
        bail!(
            "no contract IDs discovered under {} — expected at least one *.yaml file \
             with a `metadata.id:` field",
            contracts_dir.display()
        );
    }
    // Fill Extrinsic via the same scanner attestations() uses.
    //
    // PMAT-1386: this was `.unwrap_or_default()`, so a `--roadmap` path that
    // does not exist scored the ENTIRE Extrinsic stratum at 0 for EVERY
    // contract — silently, at exit 0. Measured on the live tree: 702 mentions
    // collapsed to 0 and 10 of 35 contracts fell QUORUM -> PARTIAL. The
    // default is CWD-relative, the same trap PMAT-1367 guarded `--witness-dir`
    // against. Unlike a witness dir this is the SOLE source of a whole
    // stratum, so it refuses rather than warns — which is what `attestations`
    // has always done with the identical argument. PMAT-1407 kept the
    // refusal and only improved the message (see `read_roadmap`).
    let roadmap = read_roadmap(roadmap_path)?;
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

/// The runtime-availability probes whose presence marks a witness file as one
/// that EXECUTES an emitted artifact rather than asserting over its text.
///
/// Deliberately an explicit list, not a pattern: widening the notion of
/// "executes" to another lane is then a one-line reviewable edit rather than a
/// grep that quietly loosens. `wasm_runtime_available(` is the probe the 138
/// `crates/xpile-wasm-codegen/tests/*_witness.rs` files call before shelling
/// out to `wat2wasm` + `wasm-interp`; it is the same constant
/// `crates/xpile/tests/witness_floor.rs` floors the executing half of the
/// corpus on (XPILE-WITNESS-004).
///
/// NOT widened to the WGSL / SPIR-V `gpu_witness.rs` files. They are real
/// `DiffExec` witnesses and would pass this filter, but on every CI runner they
/// take the `NotRun { no-engine }` branch — a Runtime vote resting on evidence
/// the required `workspace-test` job has never once produced would be exactly
/// the grade inflation this counter exists to stop.
const RUNTIME_PROBES: &[&str] = &["wasm_runtime_available("];

/// True when `line` is Rust code rather than a line comment (`//`, `///`,
/// `//!`). Block comments are not modelled — no witness file wraps a probe call
/// in one, and over-counting a commented-out probe would only ever ADD a vote,
/// the direction this counter must not err in. Mirrors the identical helper in
/// `crates/xpile/tests/witness_floor.rs`, deliberately: the executing-half floor
/// and the Runtime stratum must agree on what "executes" means.
fn is_code_line(line: &str) -> bool {
    !line.trim_start().starts_with("//")
}

/// Count the files that cast a Runtime vote for `contract_id` (PMAT-1367).
///
/// The source set is a WIDEN-ONLY union of two passes, collected into a
/// `BTreeSet` of canonical paths so a file reachable from more than one pass —
/// or from two overlapping `--witness-dir` arguments — is one vote, not two:
///
/// * **Pass A:** flat files under `fixtures_dir` that BOTH mention
///   `contract_id` AND are NAMED by some Rust source a test could load them
///   from (`loaded_fixtures`, see [`referenced_fixture_names`]).
/// * **Pass B (PMAT-1367):** top-level `*.rs` files under each witness dir that
///   BOTH mention `contract_id` AND carry a non-comment call to one of
///   [`RUNTIME_PROBES`].
///
/// Each pass's conjunction is the whole point. Naming a contract ID is not
/// evidence of anything — `crates/xpile/tests/contract_citation_integrity.rs`
/// hardcodes a roster of IDs and `lean_pilot_roots.rs` names them in comments,
/// and neither executes an emitted artifact. Nor is spawning a process:
/// every `Command::new` in those files launches the `xpile` binary itself.
/// Requiring the probe is what separates "a test that runs an emitted module
/// under a real runtime" from "a test that mentions a string".
///
/// PMAT-1432: that reasoning was written for Pass B and applied ONLY to Pass B.
/// Pass A stayed pure name-matching, and the one place in the repo where a file
/// exists for no reason other than to be counted is the fixture corpus:
/// `docs/roadmaps/roadmap.yaml` records eight fixtures added in a single batch —
/// "one per remaining 3-stratum contract — each carrying its contract ID in a
/// header comment ... Lifts each from 3-stratum to full 4-stratum" — with the
/// tests that would load them left as "XPILE-*-RUNTIME-001 follow-ons". Five of
/// those files are named by no Rust source anywhere in `crates/`, and for FOUR
/// contracts such a file was the ENTIRE Runtime stratum. The most extreme is
/// the Lean-source demo: it completed 4-of-4 strata for `C-XLATE-LEAN-TO-RUST`
/// while its own header states that the frontend which would parse it "doesn't
/// exist as a crate at v0.1.0" — still true, since no registered frontend
/// claims `.lean`, so that contract's 40 equations, 33 Lean theorems and 10
/// Kani harnesses model a lowering with no Rust behind it. A fixture is
/// evidence when a test loads it; on its own it is a string in a directory.
///
/// This comment names no fixture on purpose: a source under `crates/` that
/// spells a filename makes that fixture look loaded, which is how a scanner
/// hands back the votes it exists to remove (PMAT-1416).
fn count_runtime_witnesses(
    contract_id: &str,
    fixtures_dir: &Path,
    loaded_fixtures: &std::collections::BTreeSet<String>,
    witness_dirs: &[PathBuf],
) -> usize {
    let mut voters: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
    collect_fixture_voters(contract_id, fixtures_dir, loaded_fixtures, &mut voters);
    for dir in witness_dirs {
        collect_witness_voters(contract_id, dir, &mut voters);
    }
    voters.len()
}

/// The fixture file names that some Rust source OUTSIDE the fixture corpus
/// actually names — the Pass A analogue of [`RUNTIME_PROBES`] (PMAT-1432).
///
/// Scanned roots are every `--fixture-loader-dir` and every `--witness-dir`,
/// each walked recursively for `*.rs`. Both are EXPLICIT arguments on purpose:
/// deriving the root as `fixtures_dir.parent()` reads well and is a trap — the
/// unit tests below hand this function a scratch directory directly under
/// `std::env::temp_dir()`, so the derived root would have been a recursive
/// walk of all of `/tmp`, both ruinous and non-deterministic.
///
/// Measured on the live tree, widening the scan to all of `crates/**/*.rs`
/// moves ten reference counts but flips no fixture from zero references to
/// non-zero, so the bounded scope costs no real vote;
/// `quorum_fixture_evidence_witness.rs` re-derives the wide set independently
/// and reds if that ever stops being true.
///
/// TRAP, and the reason for the `fixtures_dir` exclusion: the corpus itself
/// contains `*.rs` fixtures. Walking a root that CONTAINS `tests/fixtures/`
/// without excluding it lets a `.rs` fixture whose header names its own
/// filename vote for itself, which is the same vacuity one directory over.
fn referenced_fixture_names(
    fixtures_dir: &Path,
    loader_dirs: &[PathBuf],
    witness_dirs: &[PathBuf],
) -> std::collections::BTreeSet<String> {
    let excluded = vote_key(fixtures_dir);
    let mut sources = String::new();
    for root in loader_dirs.iter().chain(witness_dirs.iter()) {
        collect_rust_sources(root, &excluded, &mut sources);
    }

    let mut named = std::collections::BTreeSet::new();
    let Ok(entries) = std::fs::read_dir(fixtures_dir) else {
        return named;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if sources.contains(name) {
            named.insert(name.to_string());
        }
    }
    named
}

/// Append every `*.rs` file's text under `dir`, skipping the `excluded`
/// subtree. Unreadable entries are skipped: a directory the reporter cannot
/// walk contributes no reference, which can only ever REMOVE a vote — the
/// direction this counter must err in.
fn collect_rust_sources(dir: &Path, excluded: &Path, out: &mut String) {
    if vote_key(dir) == *excluded {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_sources(&path, excluded, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push_str(&text);
                out.push('\n');
            }
        }
    }
}

/// Canonicalize for set identity, falling back to the literal path when the
/// file cannot be resolved (a broken symlink still deserves its own slot rather
/// than colliding with a neighbour).
fn vote_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Pass A — fixture files that name the contract ID *and* that a test loads
/// (PMAT-1432; `loaded_fixtures` comes from [`referenced_fixture_names`]).
fn collect_fixture_voters(
    contract_id: &str,
    fixtures_dir: &Path,
    loaded_fixtures: &std::collections::BTreeSet<String>,
    voters: &mut std::collections::BTreeSet<PathBuf>,
) {
    if !fixtures_dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(fixtures_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !loaded_fixtures.contains(name) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if text.contains(contract_id) {
            voters.insert(vote_key(&path));
        }
    }
}

/// Pass B — top-level `*.rs` witness files that name the ID *and* gate on a
/// runtime-availability probe on a non-comment line.
fn collect_witness_voters(
    contract_id: &str,
    witness_dir: &Path,
    voters: &mut std::collections::BTreeSet<PathBuf>,
) {
    if !witness_dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(witness_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !text.contains(contract_id) {
            continue;
        }
        let executes = text
            .lines()
            .filter(|l| is_code_line(l))
            .any(|l| RUNTIME_PROBES.iter().any(|probe| l.contains(probe)));
        if executes {
            voters.insert(vote_key(&path));
        }
    }
}

fn print_quorum_text(rows: &[QuorumRow]) {
    println!("xpile quorum — §14.4 N-of-M oracle quorum (PMAT-033)");
    println!(
        "strata: Semantic (Lean) | Symbolic (Kani) | \
         Runtime (fixtures ∪ executing witnesses) | Extrinsic (roadmap)"
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

    // ── PMAT-1367 anti-inflation unit tests ─────────────────────────────
    //
    // The Runtime stratum is the one an implementer can inflate by accident:
    // every one of the 11 PARTIAL contracts sits at exactly 2 strata, so ANY
    // Runtime vote flips a row to QUORUM. These pin the three ways a naive
    // widen would manufacture votes it did not earn.

    /// Unique scratch dir per test; removed on the way out.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "xpile-quorum-{}-{}-{name}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            std::fs::create_dir_all(&dir).expect("create scratch dir");
            Self(dir)
        }
        fn write(&self, name: &str, body: &str) -> PathBuf {
            let p = self.0.join(name);
            std::fs::write(&p, body).expect("write scratch file");
            p
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The composition `quorum` itself performs: derive the loaded-fixture set,
    /// then count. Going through the real `referenced_fixture_names` (rather
    /// than handing in a hand-built set) is what keeps these tests measuring
    /// the shipped rule. No loader dir is passed, so Pass A is inert unless a
    /// witness dir happens to name the fixture — which is exactly the
    /// PMAT-1432 posture these cases predate and must survive.
    fn count_runtime(id: &str, fixtures_dir: &Path, witness_dirs: &[PathBuf]) -> usize {
        let loaded = referenced_fixture_names(fixtures_dir, &[], witness_dirs);
        count_runtime_witnesses(id, fixtures_dir, &loaded, witness_dirs)
    }

    /// Same composition, with an explicit loader root — the shape the CLI runs
    /// with (`--fixture-loader-dir crates/xpile/tests`).
    fn count_runtime_with_loader(id: &str, fixtures_dir: &Path, loader_dir: &Path) -> usize {
        let loaders = [loader_dir.to_path_buf()];
        let loaded = referenced_fixture_names(fixtures_dir, &loaders, &[]);
        count_runtime_witnesses(id, fixtures_dir, &loaded, &[])
    }

    /// Build `<scratch>/tests/fixtures/<fixture>` carrying `body`, and return
    /// `(loader_dir, fixtures_dir)`. The loader root CONTAINS the fixture
    /// corpus, mirroring the live layout that made the self-reference trap
    /// reachable.
    fn fixture_tree(s: &Scratch, fixture: &str, body: &str) -> (PathBuf, PathBuf) {
        let loader = s.path().join("tests");
        let fixtures = loader.join("fixtures");
        std::fs::create_dir_all(&fixtures).expect("fixture tree");
        std::fs::write(fixtures.join(fixture), body).expect("write fixture");
        (loader, fixtures)
    }

    /// PMAT-1432, the defect itself: a fixture whose text names a contract ID
    /// but which NO test loads casts zero Runtime votes. Five such files sat in
    /// the live corpus, and for four contracts one of them was the entire
    /// Runtime stratum.
    #[test]
    fn fixture_no_test_loads_casts_no_runtime_vote() {
        let s = Scratch::new("unloaded-fixture");
        let (loader, fixtures) = fixture_tree(
            &s,
            "orphan_demo.py",
            "# Provides a Runtime-stratum vote for C-WASM-HEAP\n",
        );
        std::fs::write(
            loader.join("some_gate.rs"),
            "//! A test that loads a DIFFERENT fixture.\n\
             fn f() { load(\"other_demo.py\"); }\n",
        )
        .expect("write loader source");
        assert_eq!(
            count_runtime_with_loader("C-WASM-HEAP", &fixtures, &loader),
            0,
            "a fixture no test names is a string in a directory, not evidence"
        );
    }

    /// The other direction, so the rule cannot be tightened into uselessness:
    /// once a test NAMES the fixture, the vote is cast.
    #[test]
    fn fixture_a_test_loads_casts_a_runtime_vote() {
        let s = Scratch::new("loaded-fixture");
        let (loader, fixtures) = fixture_tree(
            &s,
            "loaded_demo.py",
            "# Provides a Runtime-stratum vote for C-WASM-HEAP\n",
        );
        std::fs::write(
            loader.join("loading_gate.rs"),
            "//! Runs the emitted module.\n\
             fn f() { load(\"loaded_demo.py\"); }\n",
        )
        .expect("write loader source");
        assert_eq!(
            count_runtime_with_loader("C-WASM-HEAP", &fixtures, &loader),
            1,
            "a fixture a test loads is exactly the evidence this stratum wants"
        );
    }

    /// TRAP: the corpus contains `*.rs` fixtures, and a fixture header that
    /// names its own filename would satisfy a naive scan of the loader root.
    /// The `--fixtures-dir` subtree is excluded from the walk, so it cannot.
    #[test]
    fn a_rust_fixture_naming_its_own_filename_does_not_vote_for_itself() {
        let s = Scratch::new("self-reference");
        let (loader, fixtures) = fixture_tree(
            &s,
            "self_demo.rs",
            "//! self_demo.rs — Runtime-stratum vote for C-WASM-HEAP\n",
        );
        assert_eq!(
            count_runtime_with_loader("C-WASM-HEAP", &fixtures, &loader),
            0,
            "a fixture inside the excluded subtree must not vote for itself"
        );
    }

    /// A `--fixture-loader-dir` that does not exist under-reports rather than
    /// over-reports, and does not panic. The stderr notice is emitted by
    /// `quorum` itself, once, not per contract.
    #[test]
    fn missing_loader_dir_is_non_fatal_and_drops_the_fixture_pass() {
        let s = Scratch::new("missing-loader");
        let (_loader, fixtures) = fixture_tree(&s, "demo.py", "# C-WASM-HEAP\n");
        assert_eq!(
            count_runtime_with_loader("C-WASM-HEAP", &fixtures, Path::new("/nonexistent-loader")),
            0
        );
    }

    /// A witness that NAMES the contract but never gates on a runtime probe is
    /// static-text evidence, not execution — zero votes. This is the assertion
    /// that keeps `contract_citation_integrity.rs`-shaped roster files (which
    /// name eight IDs and execute nothing emitted) out of the Runtime stratum.
    #[test]
    fn witness_naming_the_id_without_a_probe_casts_no_runtime_vote() {
        let s = Scratch::new("no-probe");
        s.write(
            "roster_witness.rs",
            "//! Names C-WASM-HEAP in a roster and asserts over WAT text.\n\
             #[test]\n\
             fn emits() {\n\
             \x20   let wat = emit(\"C-WASM-HEAP\");\n\
             \x20   assert!(wat.contains(\"memory\"));\n\
             }\n",
        );
        let empty = Path::new("/nonexistent-fixtures-dir");
        assert_eq!(
            count_runtime("C-WASM-HEAP", empty, &[s.path().to_path_buf()]),
            0,
            "naming a contract ID is not evidence of executing anything"
        );
    }

    /// A probe call that only appears inside a `//!` / `//` comment is a
    /// mention of the probe, not a call to it. Without this the counter would
    /// score any file whose module header merely documents the gating rule.
    #[test]
    fn probe_named_only_in_a_comment_casts_no_runtime_vote() {
        let s = Scratch::new("comment-probe");
        s.write(
            "commented_witness.rs",
            "//! C-WASM-HEAP witness.\n\
             //! Historically this called wasm_runtime_available( ) before executing.\n\
             #[test]\n\
             fn emits() {\n\
             \x20   // wasm_runtime_available() — disabled while WABT is broken\n\
             \x20   assert!(true);\n\
             }\n",
        );
        let empty = Path::new("/nonexistent-fixtures-dir");
        assert_eq!(
            count_runtime("C-WASM-HEAP", empty, &[s.path().to_path_buf()]),
            0,
            "a probe named in a comment is not a probe call"
        );
    }

    /// Union semantics: the same file reachable from both passes — or from two
    /// overlapping `--witness-dir` arguments — is ONE vote. A running counter
    /// instead of a set would double-count here, and `--witness-dir` is
    /// repeatable precisely so a caller can pass overlapping directories.
    #[test]
    fn a_file_in_both_passes_counts_once_not_twice() {
        let s = Scratch::new("union");
        s.write(
            "dual_witness.rs",
            "//! C-WASM-HEAP\n\
             #[test]\n\
             fn runs() {\n\
             \x20   if !wasm_runtime_available() { return; }\n\
             }\n",
        );
        // Same directory handed in as BOTH the fixtures dir and the witness
        // dir, and then as the witness dir twice over.
        assert_eq!(
            count_runtime("C-WASM-HEAP", s.path(), &[s.path().to_path_buf()]),
            1,
            "fixtures ∪ witness must dedupe by canonical path"
        );
        assert_eq!(
            count_runtime(
                "C-WASM-HEAP",
                Path::new("/nonexistent-fixtures-dir"),
                &[s.path().to_path_buf(), s.path().to_path_buf()],
            ),
            1,
            "a repeated --witness-dir must not double-count"
        );
    }

    /// Pass B is top-level and `*.rs`-only: a nested subdirectory and a
    /// non-Rust file both stay out, so the source set cannot be widened by
    /// dropping a text file into the corpus.
    #[test]
    fn witness_pass_is_top_level_rust_files_only() {
        let s = Scratch::new("scope");
        let probe = "//! C-WASM-HEAP\nfn f() { wasm_runtime_available(); }\n";
        s.write("notes.txt", probe);
        std::fs::create_dir_all(s.path().join("nested")).expect("nested dir");
        std::fs::write(s.path().join("nested/deep_witness.rs"), probe).expect("nested witness");
        let empty = Path::new("/nonexistent-fixtures-dir");
        assert_eq!(
            count_runtime("C-WASM-HEAP", empty, &[s.path().to_path_buf()]),
            0,
            "only top-level *.rs files under a witness dir may vote"
        );
        s.write("real_witness.rs", probe);
        assert_eq!(
            count_runtime("C-WASM-HEAP", empty, &[s.path().to_path_buf()]),
            1
        );
    }

    /// A missing witness dir contributes nothing and does not panic (the
    /// stderr notice is emitted by `quorum` itself, once, not per contract).
    #[test]
    fn missing_witness_dir_is_non_fatal_and_scores_zero() {
        assert_eq!(
            count_runtime(
                "C-WASM-HEAP",
                Path::new("/nonexistent-fixtures-dir"),
                &[PathBuf::from("/nonexistent-witness-dir")],
            ),
            0
        );
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
// PMAT-1448: this header used to restate the substrate's state as four
// hard-coded cardinalities, the second of which ("Depth-2 UNIVERSAL:
// 12/12 contracts (each has at least 2 Diamonds)") was measured at 14 of
// 35 on the tree that still carried it — the substrate grew well past
// twelve and most newcomers join with a single Diamond. THE STATE IS
// DELIBERATELY NOT RESTATED HERE. Run the subcommand: the totals block
// prints how many contracts sit at each `depth-N+`, and by the
// definition in `book/src/concepts/diamond-substrate.md` the UNIVERSAL
// depth is the largest N whose count still equals contracts_total.
//
// This subcommand is a *reporter*, not a gate. The gate that holds the
// deep core against regression is `crates/xpile/tests/diamond_coverage.rs`,
// and it is deliberately grandfathered over a NAMED set (PMAT-475)
// rather than asserting anything universal above depth-1.

#[derive(Debug, Clone)]
struct DiamondRow {
    id: String,
    diamond_count: usize,
}

impl DiamondRow {
    /// The classification printed in the `depth` column, COMPUTED from the
    /// count: `none` at zero, `depth-N` otherwise. Exact for every value.
    ///
    /// PMAT-1448: this was a 22-arm hand-written match ending in
    /// `_ => "depth-21+"`. That bucket's only member was the deepest contract
    /// in the tree (`C-PY-INT-ARITH`, at exactly 21), so the `+` advertised
    /// uncertainty about a number the reporter knew exactly and printed in the
    /// adjacent column. The arms were also what the legend and `--help` had
    /// been transcribed from, and BOTH transcriptions had gone stale — the
    /// legend published `depth-3+` and `--help` published `depth-9+`, neither
    /// of which this function could ever return. An exact label cannot be
    /// transcribed wrongly, because there is nothing left to enumerate.
    fn depth_label(&self) -> String {
        match self.diamond_count {
            0 => "none".to_string(),
            n => format!("depth-{n}"),
        }
    }
}

/// The smallest number of cumulative `depth-N+` buckets the totals block always
/// emits, so the JSON key set never SHRINKS below what consumers already read.
const MIN_CUMULATIVE_BUCKETS: usize = 21;

/// `(N, how many contracts carry at least N Diamonds)` — the cumulative buckets
/// of the totals block, for every N the corpus can say something about.
///
/// PMAT-1448: this was 21 hand-written `let depth_N_plus = …` bindings in EACH
/// of the two printers. Twenty-one is a ceiling, not a range: the deepest
/// contract in the tree sits at exactly 21, so one more Diamond would have been
/// reported under `depth-21+` with nothing above it and no indication that the
/// block had stopped growing.
fn cumulative_buckets(rows: &[DiamondRow]) -> Vec<(usize, usize)> {
    let deepest = rows.iter().map(|r| r.diamond_count).max().unwrap_or(0);
    (1..=deepest.max(MIN_CUMULATIVE_BUCKETS))
        .map(|n| (n, rows.iter().filter(|r| r.diamond_count >= n).count()))
        .collect()
}

fn diamond(contracts_dir: &Path, json: bool) -> Result<()> {
    // PMAT-1407: `diamond` is the one reporter whose every column is derived
    // from the contract YAML text alone — no roadmap, no fixtures, no witness
    // dirs. So the embedded corpus makes it FULLY correct for an installed
    // binary, which is exactly the user-facing exit 1 this slice repairs.
    let (_corpus_source, corpus) = load_contract_corpus(contracts_dir)?;
    let mut rows: Vec<DiamondRow> = Vec::new();
    for (_, contents) in &corpus {
        let Some(id) = extract_metadata_id(contents) else {
            continue;
        };
        rows.push(DiamondRow {
            id,
            diamond_count: count_diamond_theorems(contents),
        });
    }
    // PMAT-1386: same refusal as `quorum` / `attestations`. A zero-row Diamond
    // table read as "0 Diamond theorems across 0 contracts" at exit 0 — a
    // depth report over a universe that was never discovered.
    if rows.is_empty() {
        bail!(
            "no contract IDs discovered under {} — expected at least one *.yaml file \
             with a `metadata.id:` field",
            contracts_dir.display()
        );
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
        "depth: 0 Diamonds = none, N Diamonds = depth-N (exact — the column is never \
         bucketed; the `depth-N+` figures in the totals block are CUMULATIVE counts, \
         not classifications)"
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
    println!(
        "totals: {total_diamonds} Diamond theorems across {} contracts",
        rows.len()
    );
    let buckets: Vec<String> = cumulative_buckets(rows)
        .into_iter()
        .map(|(n, c)| format!("depth-{n}+: {c} contracts"))
        .collect();
    println!("  {}", buckets.join(", "));
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
    let buckets: Vec<String> = cumulative_buckets(rows)
        .into_iter()
        .map(|(n, c)| format!("\"depth_{n}_plus\":{c}"))
        .collect();
    println!(
        "],\"total_diamonds\":{total_diamonds},\"contracts_total\":{},{}}}",
        rows.len(),
        buckets.join(",")
    );
}

#[cfg(test)]
mod diamond_tests {
    use super::*;

    #[test]
    fn diamond_row_depth_label() {
        // PMAT-1448: this test used to enumerate twenty-two hand-written cases,
        // one per arm, each added by the numbered slice that first reached that
        // depth (PMAT-286 opened depth-5, …, PMAT-327 opened depth-21+). That
        // is the same enumeration the legend and `--help` were transcribed
        // from, so the test grew in lockstep with the defect instead of
        // catching it: `r22.depth_label()` was asserted to be `"depth-21+"`,
        // pinning the SATURATION — a contract with 22 Diamonds reported as the
        // same class as one with 21, while the adjacent count column printed
        // the true value. The label is now computed, so the property is stated
        // once and holds at every depth.
        let label = |n: usize| {
            DiamondRow {
                id: "X".into(),
                diamond_count: n,
            }
            .depth_label()
        };
        assert_eq!(label(0), "none");
        for n in 1..=40usize {
            assert_eq!(
                label(n),
                format!("depth-{n}"),
                "the classification must be the count, exactly, at every depth"
            );
        }
        // The specific regression: adjacent depths must stay DISTINGUISHABLE.
        // The old `_ => "depth-21+"` arm made these two equal.
        assert_ne!(
            label(21),
            label(22),
            "21 and 22 Diamonds collapsed to one label — the saturating bucket \
             PMAT-1448 removed is back"
        );
        // No classification carries a `+`. That spelling belongs to the totals
        // block's CUMULATIVE buckets and means something else there; keeping
        // the two disjoint is what makes the legend's distinction checkable.
        for n in 0..=40usize {
            assert!(
                !label(n).contains('+'),
                "classification {:?} carries a `+`, which in this reporter marks \
                 a cumulative bucket, not a class",
                label(n)
            );
        }
    }

    #[test]
    fn cumulative_buckets_never_truncate_the_deepest_contract() {
        // PMAT-1448: the totals block was 21 hand-written bindings, so a
        // contract deeper than 21 would have been folded into `depth-21+` with
        // nothing above it — a silent truncation at exactly one more Diamond
        // than the tree currently carries.
        let rows = |counts: &[usize]| -> Vec<DiamondRow> {
            counts
                .iter()
                .map(|&c| DiamondRow {
                    id: "X".into(),
                    diamond_count: c,
                })
                .collect()
        };
        let deep = rows(&[25, 3]);
        let b = cumulative_buckets(&deep);
        assert_eq!(
            b.last().map(|&(n, _)| n),
            Some(25),
            "the buckets must reach the deepest contract, not a written-down ceiling"
        );
        assert_eq!(
            b.iter().find(|&&(n, _)| n == 25).map(|&(_, c)| c),
            Some(1),
            "the deepest contract must be counted in its own bucket"
        );
        // The floor holds the JSON key set stable for existing consumers even
        // when the corpus is shallow.
        let shallow = cumulative_buckets(&rows(&[1]));
        assert_eq!(
            shallow.last().map(|&(n, _)| n),
            Some(MIN_CUMULATIVE_BUCKETS),
            "the bucket list must not shrink below the published floor"
        );
        // Cumulative means monotonically non-increasing in N.
        let counts: Vec<usize> = cumulative_buckets(&rows(&[5, 3, 1]))
            .into_iter()
            .map(|(_, c)| c)
            .collect();
        assert!(
            counts.windows(2).all(|w| w[0] >= w[1]),
            "cumulative counts must not increase with depth: {counts:?}"
        );
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
