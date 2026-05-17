//! Top-level session orchestration.
//!
//! Wires together the language frontends (code lane), backends (code
//! lane), contract frontends + backends (proof lane), the agent loop,
//! the FFI manifest, and the oracle. A [`TranspileSession`] is the
//! lifetime boundary for a single transpile invocation.

use std::sync::Arc;
use xpile_agent::Session as AgentSession;
use xpile_backend::Backend;
use xpile_contract_backend::ContractBackend;
use xpile_contract_frontend::ContractFrontend;
use xpile_ffi_manifest::FfiManifest;
use xpile_frontend::Frontend;

pub struct TranspileSession {
    // ── code lane ───────────────────────────────────────────────
    pub frontends: Vec<Arc<dyn Frontend>>,
    pub backends: Vec<Arc<dyn Backend>>,
    // ── proof lane ──────────────────────────────────────────────
    pub contract_frontends: Vec<Arc<dyn ContractFrontend>>,
    pub contract_backends: Vec<Arc<dyn ContractBackend>>,
    // ── shared infrastructure ───────────────────────────────────
    pub ffi_manifest: FfiManifest,
    pub agent: Option<AgentSession>,
}

impl TranspileSession {
    pub fn new() -> Self {
        Self {
            frontends: Vec::new(),
            backends: Vec::new(),
            contract_frontends: Vec::new(),
            contract_backends: Vec::new(),
            ffi_manifest: FfiManifest::new(),
            agent: None,
        }
    }

    pub fn register_frontend(&mut self, frontend: Arc<dyn Frontend>) {
        self.frontends.push(frontend);
    }

    pub fn register_backend(&mut self, backend: Arc<dyn Backend>) {
        self.backends.push(backend);
    }

    pub fn register_contract_frontend(&mut self, cf: Arc<dyn ContractFrontend>) {
        self.contract_frontends.push(cf);
    }

    pub fn register_contract_backend(&mut self, cb: Arc<dyn ContractBackend>) {
        self.contract_backends.push(cb);
    }
}

impl Default for TranspileSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a session with every v0.1.0-scaffolded impl registered.
///
/// Concrete logic is still placeholder, but the dispatch tables are
/// wired so the binary can list what's available and the trait
/// abstractions are exercised end-to-end.
pub fn default_session() -> TranspileSession {
    let mut s = TranspileSession::new();

    // Code lane: frontends
    s.register_frontend(Arc::new(depyler_frontend::PythonFrontend));
    s.register_frontend(Arc::new(decy_frontend::CFrontend));
    s.register_frontend(Arc::new(ruchy_frontend::RuchyFrontend));
    // PMAT-037 / XPILE-BASHRS-MERGER-001: Layer A scaffold. Frontend
    // is registered so the dispatch table recognises `.sh` / `.bash` /
    // `.zsh` / `.mk` files; real lowering replaces the stub at v0.2.0
    // when the bashrs source folding lands.
    s.register_frontend(Arc::new(bashrs_frontend::BashrsFrontend));

    // Code lane: backends
    s.register_backend(Arc::new(xpile_rust_codegen::RustBackend));
    s.register_backend(Arc::new(xpile_ruchy_codegen::RuchyBackend));
    s.register_backend(Arc::new(xpile_ptx_codegen::PtxBackend));
    s.register_backend(Arc::new(xpile_wgsl_codegen::WgslBackend));
    s.register_backend(Arc::new(xpile_lean_codegen::LeanBackend));
    // PMAT-037 / XPILE-BASHRS-MERGER-001: pairs with bashrs-frontend
    // above. `--target shell` now resolves to a real Backend impl
    // (scaffold emit at v0.1.0; ShellIR + quoting machinery at v0.2.0).
    s.register_backend(Arc::new(bashrs_backend::BashrsBackend));

    // Proof lane
    s.register_contract_frontend(Arc::new(latex_contract_frontend::LatexContractFrontend));
    s.register_contract_backend(Arc::new(xpile_lean_contract_backend::LeanContractBackend));
    s.register_contract_backend(Arc::new(xpile_latex_contract_backend::LatexContractBackend));

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use xpile_backend::{BackendConfig, BackendError, HwProfile, Profile, Target};
    use xpile_contracts::ContractFormat;
    use xpile_meta_hir::{Module, SourceLang};

    fn empty_module() -> Module {
        Module {
            name: "test".into(),
            source_lang: SourceLang::Python,
            items: vec![],
            ffi_boundaries: vec![],
        }
    }

    #[test]
    fn default_session_registers_v0_1_0_frontends() {
        let s = default_session();
        let names: Vec<&str> = s.frontends.iter().map(|f| f.name()).collect();
        // PMAT-037: `bashrs` joins the v0.1.0 frontend roster as
        // scaffold per the bashrs merger Layer A plan.
        for expected in &["python", "c", "ruchy", "bashrs"] {
            assert!(names.contains(expected), "missing frontend: {}", expected);
        }
    }

    #[test]
    fn default_session_registers_v0_1_0_backends() {
        let s = default_session();
        let names: Vec<&str> = s.backends.iter().map(|b| b.name()).collect();
        // PMAT-037: `bashrs` joins the v0.1.0 backend roster as
        // scaffold per the bashrs merger Layer A plan.
        for expected in &["rust", "ruchy", "ptx", "wgsl", "lean", "bashrs"] {
            assert!(names.contains(expected), "missing backend: {}", expected);
        }
    }

    #[test]
    fn bashrs_frontend_routes_shell_dialects() {
        // PMAT-037: smoke test that the dispatch table recognises
        // `.sh` / `.bash` / `.zsh` / `.mk`. Catches a regression that
        // omits one of the extensions from `BashrsFrontend::extensions`.
        let s = default_session();
        let bashrs = s
            .frontends
            .iter()
            .find(|f| f.name() == "bashrs")
            .expect("bashrs frontend registered");
        for ext in &["sh", "bash", "zsh", "mk"] {
            assert!(
                bashrs.extensions().contains(ext),
                "bashrs-frontend should recognise `.{ext}`; got {:?}",
                bashrs.extensions()
            );
        }
    }

    #[test]
    fn matches_path_dispatch_is_unique_per_file() {
        // PMAT-038: walking the dispatch table by `matches_path`
        // must produce exactly one matching frontend per known
        // input. Catches a regression where two frontends'
        // overrides collide (e.g., a future yaml frontend claiming
        // `Makefile`).
        use std::path::Path;
        let s = default_session();
        let cases = &[
            ("python", "/tmp/foo.py"),
            ("c", "/tmp/foo.c"),
            ("ruchy", "/tmp/foo.ruchy"),
            ("bashrs", "/tmp/foo.sh"),
            ("bashrs", "/tmp/foo.bash"),
            ("bashrs", "/tmp/foo.zsh"),
            ("bashrs", "/tmp/foo.mk"),
            // The load-bearing PMAT-038 cases:
            ("bashrs", "/tmp/Makefile"),
            ("bashrs", "/tmp/Dockerfile"),
        ];
        for (expected_name, path) in cases {
            let matches: Vec<&str> = s
                .frontends
                .iter()
                .filter(|f| f.matches_path(Path::new(path)))
                .map(|f| f.name())
                .collect();
            assert_eq!(
                matches,
                vec![*expected_name],
                "path {path}: expected exactly [{expected_name}], got {matches:?}"
            );
        }
    }

    #[test]
    fn matches_path_default_impl_is_extension_only_for_non_overriding_frontends() {
        // PMAT-038: assert the trait's default `matches_path` body
        // behaves identically to the prior extension-only dispatch
        // for every frontend that doesn't override. If someone
        // accidentally widens the default body, this fires.
        use std::path::Path;
        let s = default_session();
        // None of these filenames carry a dotted extension. The
        // default impl should reject all of them. (`bashrs-frontend`
        // is excluded — it intentionally overrides.)
        let extensionless = ["Makefile", "Dockerfile", "README", "LICENSE"];
        for f in &s.frontends {
            if f.name() == "bashrs" {
                continue;
            }
            for stem in &extensionless {
                let path = format!("/tmp/{stem}");
                assert!(
                    !f.matches_path(Path::new(&path)),
                    "{} should not claim {} via the default matches_path",
                    f.name(),
                    path
                );
            }
        }
    }

    #[test]
    fn bashrs_backend_emits_scaffold_with_contract_citation() {
        // PMAT-037: scaffold contract. The Backend::lower for the
        // bashrs target must produce a non-empty artifact that names
        // the C-BASHRS-POSIX-IDEMPOTENCE contract — even at v0.1.0
        // where real ShellIR emission isn't wired yet.
        let s = default_session();
        let bashrs = s
            .backends
            .iter()
            .find(|b| b.name() == "bashrs")
            .expect("bashrs backend registered");
        let cfg = BackendConfig {
            target: Target::Shell,
            profile: Profile::RustOut,
            hardware: None,
        };
        let module = Module {
            name: "demo".into(),
            source_lang: SourceLang::Shell,
            items: vec![],
            ffi_boundaries: vec![],
        };
        let art = bashrs.lower(&module, &cfg).expect("scaffold emit");
        assert!(
            art.primary.contains("C-BASHRS-POSIX-IDEMPOTENCE"),
            "missing contract citation: {}",
            art.primary
        );
        assert!(
            art.citations
                .iter()
                .any(|c| c.as_str() == "C-BASHRS-POSIX-IDEMPOTENCE"),
            "citation registry missing C-BASHRS-POSIX-IDEMPOTENCE: {:?}",
            art.citations
        );
    }

    #[test]
    fn layer_b_end_to_end_bashrs_frontend_to_bashrs_backend() {
        // PMAT-039: a real shell input flows through bashrs-frontend
        // → meta-HIR with `Stmt::Cmd` items → bashrs-backend → POSIX
        // sh with one shell-line per command. End-to-end witness
        // that the Layer B IR carries shell semantics and that the
        // bashrs lane is operational rather than scaffold.
        use std::path::Path;
        let s = default_session();
        let bashrs_frontend = s
            .frontends
            .iter()
            .find(|f| f.name() == "bashrs")
            .expect("bashrs frontend registered");
        let bashrs_backend = s
            .backends
            .iter()
            .find(|b| b.name() == "bashrs")
            .expect("bashrs backend registered");
        let module = bashrs_frontend
            .parse_and_lower(Path::new("/tmp/build.sh"), "echo hi\nls /tmp\n")
            .expect("parse");
        assert_eq!(module.source_lang, SourceLang::Shell);
        // The synthesised function shape: exactly one Item (the
        // `main` Function) containing two Stmt::Cmds.
        assert_eq!(module.items.len(), 1);
        let xpile_meta_hir::Item::Function(f) = &module.items[0];
        assert_eq!(f.name, "main");
        assert_eq!(f.body.stmts.len(), 2);

        let cfg = BackendConfig {
            target: Target::Shell,
            profile: Profile::RustOut,
            hardware: None,
        };
        let art = bashrs_backend.lower(&module, &cfg).expect("emit");
        assert!(
            art.primary.contains("\necho hi\n"),
            "expected echo line in emit: {}",
            art.primary
        );
        assert!(
            art.primary.contains("\nls /tmp\n"),
            "expected ls line in emit: {}",
            art.primary
        );
    }

    #[test]
    fn layer_b_pipeline_end_to_end() {
        // PMAT-041: a shell input with `|` flows through
        // bashrs-frontend → Stmt::Pipeline → bashrs-backend → POSIX
        // pipeline. Locks in the multi-stage cross-domain path.
        use std::path::Path;
        let s = default_session();
        let bashrs_frontend = s
            .frontends
            .iter()
            .find(|f| f.name() == "bashrs")
            .expect("bashrs frontend registered");
        let bashrs_backend = s
            .backends
            .iter()
            .find(|b| b.name() == "bashrs")
            .expect("bashrs backend registered");
        let module = bashrs_frontend
            .parse_and_lower(Path::new("/tmp/p.sh"), "ls /tmp | wc -l\n")
            .expect("parse pipeline");
        let cfg = BackendConfig {
            target: Target::Shell,
            profile: Profile::RustOut,
            hardware: None,
        };
        let art = bashrs_backend.lower(&module, &cfg).expect("emit");
        assert!(
            art.primary.contains("\nls /tmp | wc -l\n"),
            "expected pipeline line; got:\n{}",
            art.primary
        );
    }

    #[test]
    fn layer_b_rust_backend_refuses_shell_module_with_cmd() {
        // PMAT-039: the explicit-Unsupported arm in rust-codegen's
        // `emit_stmt_indented` fires when a Shell module containing
        // Stmt::Cmd reaches the Rust backend. Locks in the
        // cross-domain refusal as a load-bearing dispatch invariant.
        use std::path::Path;
        let s = default_session();
        let bashrs_frontend = s
            .frontends
            .iter()
            .find(|f| f.name() == "bashrs")
            .expect("bashrs frontend registered");
        let rust_backend = s
            .backends
            .iter()
            .find(|b| b.name() == "rust")
            .expect("rust backend registered");
        let module = bashrs_frontend
            .parse_and_lower(Path::new("/tmp/refuse.sh"), "echo hi\n")
            .expect("parse");
        let cfg = BackendConfig {
            target: Target::Rust,
            profile: Profile::RustOut,
            hardware: None,
        };
        let err = rust_backend
            .lower(&module, &cfg)
            .expect_err("rust must refuse Shell+Cmd module");
        let msg = format!("{err}");
        assert!(
            msg.contains("C-BASHRS-POSIX-IDEMPOTENCE"),
            "rust-codegen's Unsupported(Cmd) must cite the bashrs contract: {msg}"
        );
        assert!(
            msg.contains("--target shell"),
            "error message should point users at the right target: {msg}"
        );
    }

    #[test]
    fn default_session_registers_proof_lane_impls() {
        let s = default_session();
        assert!(s.contract_frontends.iter().any(|cf| cf.name() == "latex"));
        let cb_names: Vec<&str> = s.contract_backends.iter().map(|b| b.name()).collect();
        assert!(cb_names.contains(&"lean-theorem"));
        assert!(cb_names.contains(&"latex"));
    }

    #[test]
    fn ptx_backend_stub_returns_artifact_with_layer5_citation() {
        let s = default_session();
        let ptx = s
            .backends
            .iter()
            .find(|b| b.name() == "ptx")
            .expect("ptx backend registered");
        let cfg = BackendConfig {
            target: Target::Ptx,
            profile: Profile::RustOut,
            hardware: Some(HwProfile::Ptx {
                compute_capability: "sm_80".into(),
            }),
        };
        let artifact = ptx
            .lower(&empty_module(), &cfg)
            .expect("ptx lower stub returns Ok");
        assert!(artifact.primary.contains("xpile-ptx-codegen scaffold"));
        // compile_contract_citation invariant: structural citation chain
        // (Vec<ContractId>), not regex over primary. See
        // contracts/xpile-backend-trait-v1.yaml.
        assert!(
            artifact
                .citations
                .iter()
                .any(|c| c.as_str() == "C-COMPILE-RUST-TO-PTX-MMA"),
            "ptx artifact must cite a Layer-5 compile contract via Artifact.citations"
        );
    }

    #[test]
    fn ptx_backend_errors_without_hardware_profile() {
        let s = default_session();
        let ptx = s
            .backends
            .iter()
            .find(|b| b.name() == "ptx")
            .expect("ptx backend registered");
        let cfg = BackendConfig {
            target: Target::Ptx,
            profile: Profile::RustOut,
            hardware: None,
        };
        let err = ptx
            .lower(&empty_module(), &cfg)
            .expect_err("ptx lower without hardware must error");
        assert!(matches!(err, BackendError::MissingHardware(Target::Ptx)));
    }

    #[test]
    fn latex_contract_frontend_owns_latex_math_and_returns_empty_block_for_empty_input() {
        let s = default_session();
        let cf = s
            .contract_frontends
            .iter()
            .find(|cf| cf.name() == "latex")
            .expect("latex contract frontend registered");
        assert!(cf.formats().contains(&ContractFormat::LatexMath));
        let block = cf
            .parse_to_equations("")
            .expect("empty input returns empty block");
        assert!(block.equations.is_empty());
        assert!(block.proof_obligations.is_empty());
        assert!(block.citations.is_empty());
    }
}
