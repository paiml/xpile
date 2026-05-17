//! POSIX shell backend for xpile — Layer A scaffold.
//!
//! This is the v0.1.0 scaffold for the bashrs merger (see
//! `docs/specifications/sub/bashrs-merger.md`). It implements the
//! [`Backend`] trait so `Target::Shell` has a registered emitter,
//! but the real ShellIR + quoting machinery + ShellCheck-compatible
//! verifier is deferred to v0.2.0 (Layer A weeks 1-6).
//!
//! At v0.1.0 `lower` emits a placeholder POSIX-shell comment
//! identifying the module name + the `C-BASHRS-POSIX-IDEMPOTENCE`
//! Layer-1 contract citation (using the same `# xpile-contract: ...`
//! comment idiom that `xpile-rust-codegen` uses for its citations,
//! but with `#` instead of `//` — sh's comment syntax).

use std::fmt::Write;
use xpile_backend::{Artifact, Backend, BackendConfig, BackendError, Target};
use xpile_contracts::ContractId;
use xpile_meta_hir::{Item, Module, Stmt};

pub struct BashrsBackend;

impl Backend for BashrsBackend {
    fn name(&self) -> &'static str {
        "bashrs"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Shell]
    }

    fn lower(&self, module: &Module, _config: &BackendConfig) -> Result<Artifact, BackendError> {
        // PMAT-039: real Layer B emit for `Stmt::Cmd`. bashrs-frontend
        // wraps each shell script in a synthetic `main` function whose
        // body is a `Vec<Stmt::Cmd>`. The backend walks that body and
        // emits one shell-line per Cmd.
        //
        // What's deliberately NOT here yet:
        //   * Pipelines (`cmd1 | cmd2`) — XPILE-BASHRS-MERGER-002.
        //   * Variables / quoting / substitution — Layer B Expr-side
        //     variants per `sub/bashrs-merger.md`.
        //   * ShellCheck-clean output — comes with the v0.2.0 bashrs
        //     source fold (the corpus + verifier).
        let mut primary = String::new();
        writeln!(primary, "#!/bin/sh")
            .map_err(|e| BackendError::Lower(format!("write failed: {e}")))?;
        writeln!(
            primary,
            "# xpile-bashrs-backend (v0.1.0 PMAT-039 / XPILE-BASHRS-MERGER-001 Layer B)"
        )
        .map_err(|e| BackendError::Lower(format!("write failed: {e}")))?;
        writeln!(primary, "# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE")
            .map_err(|e| BackendError::Lower(format!("write failed: {e}")))?;
        writeln!(primary, "# module: {}", module.name)
            .map_err(|e| BackendError::Lower(format!("write failed: {e}")))?;

        let mut emitted_commands = 0usize;
        for item in &module.items {
            let Item::Function(f) = item;
            // Only the `main` synthesised by bashrs-frontend
            // participates in shell emit; other Functions (which
            // should never appear in a Shell module today, but
            // defensive code keeps the dispatch boundary explicit)
            // are skipped.
            if f.name != "main" {
                continue;
            }
            for stmt in &f.body.stmts {
                if let Stmt::Cmd { program, args } = stmt {
                    if args.is_empty() {
                        writeln!(primary, "{program}")
                            .map_err(|e| BackendError::Lower(format!("write failed: {e}")))?;
                    } else {
                        writeln!(primary, "{program} {}", args.join(" "))
                            .map_err(|e| BackendError::Lower(format!("write failed: {e}")))?;
                    }
                    emitted_commands += 1;
                }
                // Non-Cmd statements (Let / Assign / While / Assert)
                // would only appear if a future frontend produced
                // them inside a Shell module. Defer that case to
                // when it actually arises — no need to emit shell
                // shapes for variables / loops until the
                // corresponding Layer B Expr-side variants land.
            }
        }
        if emitted_commands == 0 {
            // Empty input (no Stmt::Cmd produced). Mirror the v0.1.0
            // scaffold posture so `xpile transpile empty.sh --target
            // shell` still produces a well-formed POSIX file. Keeps
            // the test that exercises the structurally-empty Shell
            // module green.
            writeln!(
                primary,
                "# (no commands — empty script or parse produced 0 Stmt::Cmd)"
            )
            .map_err(|e| BackendError::Lower(format!("write failed: {e}")))?;
        }
        Ok(Artifact {
            primary,
            sidecars: Vec::new(),
            citations: vec![ContractId::new("C-BASHRS-POSIX-IDEMPOTENCE")],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xpile_backend::{BackendConfig, Profile};
    use xpile_meta_hir::SourceLang;

    fn empty_shell_module() -> Module {
        Module {
            name: "demo".into(),
            source_lang: SourceLang::Shell,
            items: vec![],
            ffi_boundaries: vec![],
        }
    }

    #[test]
    fn name_is_bashrs() {
        assert_eq!(BashrsBackend.name(), "bashrs");
    }

    #[test]
    fn targets_shell() {
        assert_eq!(BashrsBackend.targets(), &[Target::Shell]);
    }

    #[test]
    fn lower_empty_shell_module_emits_well_formed_posix_with_citation() {
        // PMAT-039: an empty Shell module still emits a complete
        // POSIX file (shebang + header + citation + the "no commands"
        // diagnostic comment). The citation in Artifact::citations
        // matches the in-source `# xpile-contract: ...` line, so
        // both the audit pipeline and a human reader find the same ID.
        let module = empty_shell_module();
        let config = BackendConfig {
            target: Target::Shell,
            profile: Profile::RustOut,
            hardware: None,
        };
        let art = BashrsBackend.lower(&module, &config).expect("lower");
        assert!(
            art.primary.starts_with("#!/bin/sh\n"),
            "expected POSIX shebang at line 1: {}",
            art.primary
        );
        assert!(
            art.primary
                .contains("# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE"),
            "citation absent: {}",
            art.primary
        );
        assert!(
            art.primary.contains("# module: demo"),
            "module name absent: {}",
            art.primary
        );
        assert!(
            art.primary.contains("(no commands"),
            "expected empty-module diagnostic: {}",
            art.primary
        );
        assert_eq!(art.citations.len(), 1);
        assert_eq!(art.citations[0].as_str(), "C-BASHRS-POSIX-IDEMPOTENCE");
    }

    #[test]
    fn lower_synthesised_main_emits_each_cmd_on_its_own_line() {
        // PMAT-039 load-bearing test: a Module whose `main` function
        // body contains three Stmt::Cmds must produce three shell-
        // lines after the header.
        use xpile_meta_hir::{Block, Expr, Function, Item, Stmt, Type};
        let module = Module {
            name: "demo".into(),
            source_lang: xpile_meta_hir::SourceLang::Shell,
            items: vec![Item::Function(Function {
                name: "main".into(),
                params: vec![],
                return_type: Type::I64,
                body: Block {
                    stmts: vec![
                        Stmt::Cmd {
                            program: "echo".into(),
                            args: vec!["hello".into(), "world".into()],
                        },
                        Stmt::Cmd {
                            program: "ls".into(),
                            args: vec!["/tmp".into()],
                        },
                        Stmt::Cmd {
                            program: "pwd".into(),
                            args: vec![],
                        },
                    ],
                    trailing_return: Expr::LitInt(0),
                },
            })],
            ffi_boundaries: vec![],
        };
        let config = BackendConfig {
            target: Target::Shell,
            profile: Profile::RustOut,
            hardware: None,
        };
        let art = BashrsBackend.lower(&module, &config).expect("lower");
        // Order-preserving: each command appears on its own line in
        // the order produced by the frontend.
        assert!(
            art.primary.contains("\necho hello world\n"),
            "expected `echo hello world` line: {}",
            art.primary
        );
        assert!(
            art.primary.contains("\nls /tmp\n"),
            "expected `ls /tmp` line: {}",
            art.primary
        );
        assert!(
            art.primary.contains("\npwd\n"),
            "expected `pwd` line: {}",
            art.primary
        );
        // No empty-script diagnostic when we did emit commands.
        assert!(
            !art.primary.contains("(no commands"),
            "should not emit empty-script diagnostic: {}",
            art.primary
        );
        // Citation invariant unchanged.
        assert_eq!(art.citations[0].as_str(), "C-BASHRS-POSIX-IDEMPOTENCE");
    }
}
