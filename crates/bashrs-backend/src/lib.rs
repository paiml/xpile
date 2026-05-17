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
use xpile_meta_hir::{Expr, Item, Module, QuotingStrategy, Stmt};

/// PMAT-042: render a single `Stmt::Cmd` arg into its POSIX shell
/// surface form, honouring the carried `QuotingStrategy` for
/// `Expr::QuotedString`. Non-string `Expr` variants are refused
/// (defensive — bashrs-frontend doesn't produce them inside a Cmd's
/// args; a future producer that did would need to extend this).
fn render_arg(e: &Expr) -> Result<String, BackendError> {
    match e {
        Expr::LitStr(s) => Ok(s.clone()),
        Expr::QuotedString { content, quoting } => Ok(match quoting {
            QuotingStrategy::Single => format!("'{content}'"),
            QuotingStrategy::Double => format!("\"{content}\""),
            QuotingStrategy::Backslash => content
                .chars()
                .map(|c| format!("\\{c}"))
                .collect::<String>(),
        }),
        // PMAT-045: shell-variable refs render as `$NAME` (bareword
        // form). bashrs-frontend's parser validates the name is a
        // POSIX-legal identifier before producing this variant, so
        // the rendered shell is always well-formed. `${NAME}` (brace
        // form) is the input-side parse; rendering as `$NAME` is the
        // canonical output form — same semantic, fewer chars.
        Expr::ShellVar(name) => Ok(format!("${name}")),
        other => Err(BackendError::Lower(format!(
            "bashrs-backend v0.1.0 cannot render non-string Expr as Stmt::Cmd arg \
             (got {other:?}); only Expr::LitStr / Expr::QuotedString / Expr::ShellVar supported"
        ))),
    }
}

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
            // PMAT-040: walk every function's body for `Stmt::Cmd`.
            // PMAT-039's bashrs-frontend produces a single
            // synthesised `main`; PMAT-040's depyler-frontend
            // produces user-named functions (e.g., `build`) whose
            // bodies contain `subprocess.run(...)`-derived Cmds.
            // Both shapes flow through this loop. If a multi-function
            // module ships, each function's Cmds emit in source
            // order — the v0.1.0 grouping shape is intentionally
            // flat. Section headers / per-function shell-functions
            // are XPILE-BASHRS-MERGER-002+.
            //
            // Per-function citation: emit each function's contract
            // refs *once* immediately before its Cmd block if the
            // function has any Cmds — keeps the citation:function
            // mapping legible when reading the emitted shell.
            // PMAT-041: extended emit walks Cmd AND Pipeline. Each
            // top-level Stmt::Cmd renders as one POSIX line; each
            // Stmt::Pipeline renders as `stage1 | stage2 | …` on a
            // single line. Pipelines compose Cmd stages, so the
            // per-stage rendering reuses the same `program args...`
            // format used for top-level Cmd.
            let emittable: Vec<&Stmt> = f
                .body
                .stmts
                .iter()
                .filter(|s| matches!(s, Stmt::Cmd { .. } | Stmt::Pipeline { .. }))
                .collect();
            if emittable.is_empty() {
                continue;
            }
            // Optional per-function divider — only emit if the
            // function name carries information (i.e., not the
            // synthesised `main` from bashrs-frontend, which is a
            // structural placeholder).
            if f.name != "main" {
                writeln!(primary, "# function: {}", f.name)
                    .map_err(|e| BackendError::Lower(format!("write failed: {e}")))?;
            }
            for stmt in emittable {
                match stmt {
                    Stmt::Cmd { program, args } => {
                        if args.is_empty() {
                            writeln!(primary, "{program}")
                                .map_err(|e| BackendError::Lower(format!("write failed: {e}")))?;
                        } else {
                            // PMAT-042: render each arg through the
                            // quoting-aware helper.
                            let rendered: Result<Vec<String>, BackendError> =
                                args.iter().map(render_arg).collect();
                            writeln!(primary, "{program} {}", rendered?.join(" "))
                                .map_err(|e| BackendError::Lower(format!("write failed: {e}")))?;
                        }
                        emitted_commands += 1;
                    }
                    Stmt::Pipeline { stages } => {
                        // Render each stage as `program args...` and
                        // join with ` | `. v0.1.0 invariant: every
                        // stage is a Cmd (bashrs-frontend enforces).
                        // Non-Cmd stages would arise only from a
                        // future frontend producing nested pipelines
                        // / control-flow inside a pipeline; rejected
                        // here with a clear error so the boundary
                        // stays explicit.
                        let mut rendered: Vec<String> = Vec::with_capacity(stages.len());
                        for stage in stages {
                            let Stmt::Cmd { program, args } = stage else {
                                return Err(BackendError::Lower(format!(
                                    "Stmt::Pipeline stage is not a Stmt::Cmd; \
                                     bashrs-backend v0.1.0 only renders Cmd stages \
                                     (got {stage:?})"
                                )));
                            };
                            // PMAT-042: same quoting-aware rendering.
                            if args.is_empty() {
                                rendered.push(program.clone());
                            } else {
                                let arg_strs: Result<Vec<String>, BackendError> =
                                    args.iter().map(render_arg).collect();
                                rendered.push(format!("{program} {}", arg_strs?.join(" ")));
                            }
                        }
                        writeln!(primary, "{}", rendered.join(" | "))
                            .map_err(|e| BackendError::Lower(format!("write failed: {e}")))?;
                        emitted_commands += 1;
                    }
                    // matches! above guards against everything else.
                    _ => unreachable!(),
                }
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
                            args: vec![Expr::LitStr("hello".into()), Expr::LitStr("world".into())],
                        },
                        Stmt::Cmd {
                            program: "ls".into(),
                            args: vec![Expr::LitStr("/tmp".into())],
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

    #[test]
    fn lower_pipeline_emits_pipe_joined_stages() {
        // PMAT-041 load-bearing emit test. A Module whose `main`
        // body contains a Stmt::Pipeline with three Stmt::Cmd
        // stages must produce a single shell line with the stages
        // joined by ` | `.
        use xpile_meta_hir::{Block, Expr, Function, Item, Stmt, Type};
        let module = Module {
            name: "demo".into(),
            source_lang: xpile_meta_hir::SourceLang::Shell,
            items: vec![Item::Function(Function {
                name: "main".into(),
                params: vec![],
                return_type: Type::I64,
                body: Block {
                    stmts: vec![Stmt::Pipeline {
                        stages: vec![
                            Stmt::Cmd {
                                program: "cat".into(),
                                args: vec![Expr::LitStr("foo".into())],
                            },
                            Stmt::Cmd {
                                program: "grep".into(),
                                args: vec![Expr::LitStr("bar".into())],
                            },
                            Stmt::Cmd {
                                program: "wc".into(),
                                args: vec![Expr::LitStr("-l".into())],
                            },
                        ],
                    }],
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
        assert!(
            art.primary.contains("\ncat foo | grep bar | wc -l\n"),
            "expected pipeline line; got:\n{}",
            art.primary
        );
    }

    #[test]
    fn render_arg_uses_quoting_strategy() {
        // PMAT-042: each QuotingStrategy variant renders the wrapping
        // characters bashrs-frontend / future producers expect. This
        // locks in the rendering contract so a downstream consumer of
        // the emitted shell knows exactly what to expect.
        use xpile_meta_hir::{Expr, QuotingStrategy};
        assert_eq!(
            render_arg(&Expr::LitStr("foo".into())).unwrap(),
            "foo",
            "LitStr emits bareword"
        );
        assert_eq!(
            render_arg(&Expr::QuotedString {
                content: "hello world".into(),
                quoting: QuotingStrategy::Single,
            })
            .unwrap(),
            "'hello world'",
            "Single-quoted strategy wraps in single quotes"
        );
        assert_eq!(
            render_arg(&Expr::QuotedString {
                content: "hi $USER".into(),
                quoting: QuotingStrategy::Double,
            })
            .unwrap(),
            "\"hi $USER\"",
            "Double-quoted strategy wraps in double quotes"
        );
        assert_eq!(
            render_arg(&Expr::QuotedString {
                content: "abc".into(),
                quoting: QuotingStrategy::Backslash,
            })
            .unwrap(),
            "\\a\\b\\c",
            "Backslash strategy escapes each character"
        );
    }

    #[test]
    fn lower_cmd_with_quoted_string_arg_renders_with_quotes() {
        // PMAT-042 end-to-end: a Stmt::Cmd whose args contain an
        // `Expr::QuotedString` renders with the right quoting in the
        // emitted shell.
        use xpile_meta_hir::{Block, Expr, Function, Item, QuotingStrategy, Stmt, Type};
        let module = Module {
            name: "demo".into(),
            source_lang: xpile_meta_hir::SourceLang::Shell,
            items: vec![Item::Function(Function {
                name: "main".into(),
                params: vec![],
                return_type: Type::I64,
                body: Block {
                    stmts: vec![Stmt::Cmd {
                        program: "echo".into(),
                        args: vec![Expr::QuotedString {
                            content: "hello world".into(),
                            quoting: QuotingStrategy::Single,
                        }],
                    }],
                    trailing_return: Expr::LitInt(0),
                },
            })],
            ffi_boundaries: vec![],
        };
        let cfg = BackendConfig {
            target: Target::Shell,
            profile: Profile::RustOut,
            hardware: None,
        };
        let art = BashrsBackend.lower(&module, &cfg).expect("lower");
        assert!(
            art.primary.contains("\necho 'hello world'\n"),
            "expected single-quoted arg in emit; got:\n{}",
            art.primary
        );
    }

    #[test]
    fn render_arg_shell_var() {
        // PMAT-045: ShellVar renders as `$NAME`.
        use xpile_meta_hir::Expr;
        assert_eq!(render_arg(&Expr::ShellVar("HOME".into())).unwrap(), "$HOME");
        assert_eq!(
            render_arg(&Expr::ShellVar("snake_case_2".into())).unwrap(),
            "$snake_case_2"
        );
    }

    #[test]
    fn lower_pipeline_with_non_cmd_stage_errors() {
        // PMAT-041 defensive arm: bashrs-frontend won't produce
        // non-Cmd stages at v0.1.0, but if a future frontend does,
        // the backend refuses with a clear error rather than emit
        // ill-formed shell.
        use xpile_meta_hir::{Block, Expr, Function, Item, Stmt, Type};
        let bogus_stage = Stmt::Let {
            name: "x".into(),
            ty: Type::I64,
            value: Expr::LitInt(7),
            mutable: false,
        };
        let module = Module {
            name: "demo".into(),
            source_lang: xpile_meta_hir::SourceLang::Shell,
            items: vec![Item::Function(Function {
                name: "main".into(),
                params: vec![],
                return_type: Type::I64,
                body: Block {
                    stmts: vec![Stmt::Pipeline {
                        stages: vec![bogus_stage],
                    }],
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
        let err = BashrsBackend
            .lower(&module, &config)
            .expect_err("non-Cmd stage must be refused");
        let msg = format!("{err}");
        assert!(
            msg.contains("stage is not a Stmt::Cmd"),
            "error should explain the v0.1.0 stage shape constraint: {msg}"
        );
    }
}
