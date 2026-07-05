//! POSIX shell backend for xpile (see
//! `docs/specifications/sub/bashrs-merger.md`).
//!
//! Implements the [`Backend`] trait so `Target::Shell` has a
//! registered emitter, and renders **real POSIX shell** for the
//! supported meta-HIR `Stmt` set. [`BashrsBackend::lower`] walks each
//! function body and emits, via the shared [`render_stmt_lines`]
//! walker: `Stmt::Cmd` → `program arg…`; `Stmt::Pipeline` →
//! `stage1 | stage2 | …`; `Stmt::ShellAssign` → `NAME=value`; and
//! `Stmt::ShellLoop` → a multi-line `header; do … done` block whose
//! body is rendered recursively through the same walker.
//!
//! Args render via [`render_arg`] (`Expr::LitStr` / `QuotedString`
//! honouring its `QuotingStrategy` / `ShellVar` / `ShellSpecial` /
//! `CommandSubstitution`). Every emit carries a `#!/bin/sh` shebang
//! and a `# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE` citation line
//! (the same `# xpile-contract: ...` idiom `xpile-rust-codegen` uses,
//! with `#` for sh's comment syntax).
//!
//! A `# (no commands …)` comment is emitted **only** for genuinely
//! empty input — a module that produces zero renderable statements —
//! so `xpile transpile empty.sh --target shell` still yields a
//! well-formed POSIX file. It is a diagnostic for the empty case, not
//! a stand-in for real emission.
//!
//! Still future work (out of scope here): a ShellCheck-compatible
//! verifier, and the structured `Expr::ParamExpansion` /
//! shell control-flow *parsing* tracked in the bashrs-frontend's
//! v0.2.0 fold (param-expansion forms currently survive as verbatim
//! `Expr::LitStr`).

use std::fmt::Write;
use xpile_backend::{Artifact, Backend, BackendConfig, BackendError, QuorumStatus, Target};
use xpile_contracts::ContractId;
use xpile_meta_hir::{Expr, Item, LoopKind, Module, QuotingStrategy, Stmt};

/// PMAT-042: render a single `Stmt::Cmd` arg into its POSIX shell
/// surface form, honouring the carried `QuotingStrategy` for
/// `Expr::QuotedString`. Non-string `Expr` variants are refused
/// (defensive — bashrs-frontend doesn't produce them inside a Cmd's
/// args; a future producer that did would need to extend this).
fn render_arg(e: &Expr) -> Result<String, BackendError> {
    match e {
        Expr::LitStr(s) => Ok(s.clone()),
        Expr::QuotedString { content, quoting } => Ok(match quoting {
            // PMAT-056: bashrs-frontend preserves escapes verbatim
            // in the content (the tokenizer's quote arms read `\"`
            // as two chars in the content), so rendering just
            // wraps in the appropriate quotes — no re-escape
            // needed. The round-trip stays information-lossless:
            // `"$NAME"` keeps the unescaped `$` (variable
            // expansion), `"\$NAME"` keeps the escaped form (literal).
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
        // PMAT-055: shell special parameters render as `$<char>`.
        // The frontend parser guarantees the name is one of POSIX's
        // legal specials, so no further validation needed here.
        Expr::ShellSpecial(name) => Ok(format!("${name}")),
        // PMAT-047: command substitution renders as `$(rendered-inner)`.
        // At v0.1.0 the inner Stmt must be a `Stmt::Cmd` (the only
        // stmt shape that has a renderable inline form); a future
        // `Stmt::Pipeline` inside `$(...)` is plausible but the
        // bashrs-frontend parser doesn't produce it yet.
        Expr::CommandSubstitution(inner) => render_substituted_stmt(inner),
        other => Err(BackendError::Lower(format!(
            "bashrs-backend v0.1.0 cannot render non-string Expr as Stmt::Cmd arg \
             (got {other:?}); only Expr::LitStr / Expr::QuotedString / Expr::ShellVar / Expr::CommandSubstitution supported"
        ))),
    }
}

/// PMAT-048: render a `Stmt::ShellLoop` to its POSIX shell
/// representation.
///
/// PMAT-974: the loop *body* is now rendered. Previously the helper
/// emitted a placeholder `; do : # body: <pending v0.2.0 expansion>;
/// done` and silently dropped every body statement — a real
/// mis-emit: a loop carrying real body commands lost them entirely.
/// Now each body `Stmt` renders through the shared
/// [`render_stmt_lines`] walker (the same renderer that drives the
/// top-level emit), so `Cmd` / `Pipeline` / `ShellAssign` / nested
/// `ShellLoop` all emit correctly inside a `do … done` block. Body
/// lines are indented one tab past the `for/while/until` header for
/// legibility.
///
/// Returns the multi-line `header; do\n\t<body…>\ndone` form (no
/// trailing newline) — the caller appends a single newline when
/// writing the loop line.
///
/// An empty body emits `do :` (the POSIX no-op) so the resulting
/// shell stays syntactically valid (`do … done` must contain at
/// least one command).
fn render_shell_loop(kind: &LoopKind, body: &[Stmt]) -> Result<String, BackendError> {
    let header = match kind {
        LoopKind::For { var, items } => {
            let rendered: Result<Vec<String>, BackendError> =
                items.iter().map(render_arg).collect();
            format!("for {var} in {}", rendered?.join(" "))
        }
        LoopKind::While { cond } => format!("while {}", render_arg(cond)?),
        LoopKind::Until { cond } => format!("until {}", render_arg(cond)?),
    };

    // Render every body statement through the shared walker. An
    // empty body collapses to the POSIX no-op `:` so `do … done`
    // stays well-formed.
    let mut body_lines: Vec<String> = Vec::new();
    for stmt in body {
        for line in render_stmt_lines(stmt)? {
            // Indent each (possibly multi-line, e.g. a nested loop)
            // body line one tab for readability.
            for sub in line.split('\n') {
                body_lines.push(format!("\t{sub}"));
            }
        }
    }
    if body_lines.is_empty() {
        body_lines.push("\t:".to_string());
    }

    Ok(format!("{header}; do\n{}\ndone", body_lines.join("\n")))
}

/// PMAT-1283: render a `Stmt::ShellIf` to POSIX shell:
/// `if COND; then\n\t<then>\n[else\n\t<else>\n]fi`. The condition is the
/// opaque `Expr::LitStr` the frontend captured, printed verbatim (same
/// as loop conditions). An empty `then` body collapses to the POSIX
/// no-op `:`; the `else` block is omitted when its body is empty (an
/// `if … then … fi` with no `else`). Bodies render through the shared
/// `render_stmt_lines` walker, so a nested loop / conditional in either
/// branch recurses.
fn render_shell_if(
    cond: &Expr,
    then_body: &[Stmt],
    else_body: &[Stmt],
) -> Result<String, BackendError> {
    fn indent_body(body: &[Stmt]) -> Result<Vec<String>, BackendError> {
        let mut out: Vec<String> = Vec::new();
        for stmt in body {
            for line in render_stmt_lines(stmt)? {
                for sub in line.split('\n') {
                    out.push(format!("\t{sub}"));
                }
            }
        }
        Ok(out)
    }
    let mut then_lines = indent_body(then_body)?;
    if then_lines.is_empty() {
        then_lines.push("\t:".to_string());
    }
    let header = format!("if {}; then", render_arg(cond)?);
    if else_body.is_empty() {
        return Ok(format!("{header}\n{}\nfi", then_lines.join("\n")));
    }
    let mut else_lines = indent_body(else_body)?;
    if else_lines.is_empty() {
        else_lines.push("\t:".to_string());
    }
    Ok(format!(
        "{header}\n{}\nelse\n{}\nfi",
        then_lines.join("\n"),
        else_lines.join("\n")
    ))
}

/// PMAT-974: render a single top-level shell statement to its POSIX
/// surface line(s).
///
/// Extracted from `lower`'s inline match so loop bodies and the
/// top-level walk share one renderer (DRY + recursion: a
/// `ShellLoop` body containing another `ShellLoop` renders through
/// the same code path). Returns one `String` per emitted shell
/// construct; a single statement may render to a multi-line string
/// (e.g. a nested loop's `do … done`).
///
/// Mirrors the v0.1.0 boundary: non-`Cmd` pipeline stages and
/// non-string `Cmd` args are refused with the same diagnostics the
/// top-level emit used, so the cross-domain contract stays explicit.
fn render_stmt_lines(stmt: &Stmt) -> Result<Vec<String>, BackendError> {
    match stmt {
        Stmt::Cmd { program, args } => {
            if args.is_empty() {
                Ok(vec![program.clone()])
            } else {
                let rendered: Result<Vec<String>, BackendError> =
                    args.iter().map(render_arg).collect();
                Ok(vec![format!("{program} {}", rendered?.join(" "))])
            }
        }
        Stmt::Pipeline { stages } => {
            let mut rendered: Vec<String> = Vec::with_capacity(stages.len());
            for stage in stages {
                let Stmt::Cmd { program, args } = stage else {
                    return Err(BackendError::Lower(format!(
                        "Stmt::Pipeline stage is not a Stmt::Cmd; \
                         bashrs-backend v0.1.0 only renders Cmd stages \
                         (got {stage:?})"
                    )));
                };
                if args.is_empty() {
                    rendered.push(program.clone());
                } else {
                    let arg_strs: Result<Vec<String>, BackendError> =
                        args.iter().map(render_arg).collect();
                    rendered.push(format!("{program} {}", arg_strs?.join(" ")));
                }
            }
            Ok(vec![rendered.join(" | ")])
        }
        Stmt::ShellAssign { name, value } => Ok(vec![format!("{name}={}", render_arg(value)?)]),
        Stmt::ShellLoop { kind, body } => Ok(vec![render_shell_loop(kind, body)?]),
        Stmt::ShellIf {
            cond,
            then_body,
            else_body,
        } => Ok(vec![render_shell_if(cond, then_body, else_body)?]),
        other => Err(BackendError::Lower(format!(
            "bashrs-backend cannot render {other:?} as a shell statement; \
             only Stmt::Cmd / Stmt::Pipeline / Stmt::ShellAssign / Stmt::ShellLoop / Stmt::ShellIf supported"
        ))),
    }
}

/// PMAT-047: render the inner Stmt of a `$(cmd)` substitution into
/// shell surface form, wrapped in `$(...)`. Only `Stmt::Cmd` is
/// supported at v0.1.0 — nested pipelines / control flow inside a
/// substitution are XPILE-BASHRS-MERGER-***+.
fn render_substituted_stmt(s: &Stmt) -> Result<String, BackendError> {
    let Stmt::Cmd { program, args } = s else {
        return Err(BackendError::Lower(format!(
            "bashrs-backend v0.1.0 only renders Stmt::Cmd inside \
             Expr::CommandSubstitution(...) — got {s:?}; nested \
             pipelines or control flow inside `$(...)` are future work"
        )));
    };
    if args.is_empty() {
        Ok(format!("$({program})"))
    } else {
        let rendered: Result<Vec<String>, BackendError> = args.iter().map(render_arg).collect();
        Ok(format!("$({program} {})", rendered?.join(" ")))
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

    fn lower(&self, module: &Module, config: &BackendConfig) -> Result<Artifact, BackendError> {
        // PMAT-039/041/042/047/048/974: real Layer B emit.
        // bashrs-frontend wraps each shell script in a synthetic `main`
        // function whose body is a `Vec<Stmt>`; depyler-frontend
        // produces user-named functions. The backend walks every
        // function body and renders each emittable statement (`Cmd` /
        // `Pipeline` / `ShellAssign` / `ShellLoop`, with their args:
        // literals, quoted strings, `$VAR`, `$@`/`$1`/…, `$(…)`)
        // through the shared `render_stmt_lines` walker.
        //
        // Still future work (not here): a ShellCheck-compatible
        // verifier (the v0.2.0 bashrs source fold's corpus + verifier),
        // and the structured `Expr::ParamExpansion` variant —
        // param-expansion forms currently render as verbatim
        // `Expr::LitStr`.
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
            // PMAT-502bj: module-level constants are not part of the shell
            // domain — skip them (the bashrs lane only walks functions).
            let Item::Function(f) = item else {
                continue;
            };
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
                .filter(|s| {
                    matches!(
                        s,
                        Stmt::Cmd { .. }
                            | Stmt::Pipeline { .. }
                            | Stmt::ShellLoop { .. }
                            | Stmt::ShellIf { .. }
                            | Stmt::ShellAssign { .. }
                    )
                })
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
            // PMAT-974: the per-Stmt surface rendering now lives in
            // the shared `render_stmt_lines` walker so loop bodies and
            // the top-level walk emit through one code path. Each
            // emittable Stmt renders to one or more shell lines
            // (`Cmd` / `Pipeline` / `ShellAssign` → one line; a
            // `ShellLoop` → a multi-line `do … done` block).
            for stmt in emittable {
                for line in render_stmt_lines(stmt)? {
                    writeln!(primary, "{line}")
                        .map_err(|e| BackendError::Lower(format!("write failed: {e}")))?;
                }
                emitted_commands += 1;
            }
        }
        if emitted_commands == 0 {
            // Genuinely empty input — the module produced zero
            // renderable statements. Emit a diagnostic comment so
            // `xpile transpile empty.sh --target shell` still produces
            // a well-formed POSIX file (this comment appears ONLY for
            // empty input; non-empty input renders real shell above).
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
            quorum_status: QuorumStatus::Single {
                emitter: "bashrs-backend".to_string(),
            },
        }
        .with_citations(config.emit_contracts))
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
            emit_contracts: true,
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
            emit_contracts: true,
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
            emit_contracts: true,
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
    fn render_arg_litstr_preserves_param_expansion_verbatim() {
        // PMAT-085: POSIX parameter-expansion forms
        // (`${VAR:-default}`, `${VAR-default}`, `${VAR:=8080}`, etc.)
        // are represented as `Expr::LitStr` at v0.1.0 (Bronze tier);
        // bashrs-frontend's `lower_token_param_expansion_*` test
        // documents the input side. This test documents the output
        // side: rendering a `LitStr` whose contents are a
        // parameter-expansion form emits the bytes unchanged.
        //
        // Together the two tests lock in the substrate-quality
        // property: parameter-expansion forms survive the
        // frontend → meta-HIR → backend round-trip byte-identically.
        // Information loss is zero. The structured
        // `Expr::ParamExpansion { var, op, fallback }` variant is
        // XPILE-BASHRS-PARAM-EXPANSION-001 future work (v0.2.0+).
        let param_expansions = &[
            "${VAR:-default}",
            "${VAR-default}",
            "${VAR:=8080}",
            "${VAR:?error}",
            "${VAR:+alt}",
            "${#VAR}",
            "${VAR#prefix}",
            "${VAR##prefix*}",
            "${VAR%suffix}",
            "${VAR%%*suffix}",
            "${VAR/old/new}",
            "${VAR:0:3}",
        ];
        for expansion in param_expansions {
            let lit = xpile_meta_hir::Expr::LitStr((*expansion).to_string());
            let rendered = render_arg(&lit).unwrap();
            assert_eq!(
                rendered, *expansion,
                "expected LitStr param-expansion `{expansion}` to render verbatim"
            );
        }
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
            emit_contracts: true,
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
    fn render_shell_loop_for_kind() {
        // PMAT-048: For-loop header renders correctly with the
        // var and items. PMAT-974: an empty body now renders the
        // POSIX no-op `:` inside `do … done` (was a placeholder
        // comment before).
        use xpile_meta_hir::{Expr, LoopKind};
        let kind = LoopKind::For {
            var: "x".into(),
            items: vec![
                Expr::LitStr("a".into()),
                Expr::LitStr("b".into()),
                Expr::LitStr("c".into()),
            ],
        };
        let rendered = render_shell_loop(&kind, &[]).unwrap();
        assert!(
            rendered.starts_with("for x in a b c; do\n"),
            "header missing or wrong: {rendered}"
        );
        // Empty body → POSIX no-op so `do … done` stays valid.
        assert!(
            rendered.contains("\n\t:\n"),
            "empty body should render the `:` no-op: {rendered}"
        );
        assert!(
            rendered.ends_with("\ndone"),
            "loop should end with `done`: {rendered}"
        );
    }

    #[test]
    fn render_shell_if_no_else() {
        // PMAT-1283: `if COND; then <body> fi` — no else arm.
        use xpile_meta_hir::{Expr, Stmt};
        let cond = Expr::LitStr("[ -f /tmp/x ]".into());
        let then_body = vec![Stmt::Cmd {
            program: "echo".into(),
            args: vec![Expr::LitStr("found".into())],
        }];
        let rendered = render_shell_if(&cond, &then_body, &[]).unwrap();
        assert_eq!(rendered, "if [ -f /tmp/x ]; then\n\techo found\nfi");
    }

    #[test]
    fn render_shell_if_with_else() {
        // PMAT-1283: the `else` arm renders between `then` body and `fi`.
        use xpile_meta_hir::{Expr, Stmt};
        let cond = Expr::LitStr("[ $x -gt 3 ]".into());
        let then_body = vec![Stmt::Cmd {
            program: "echo".into(),
            args: vec![Expr::LitStr("big".into())],
        }];
        let else_body = vec![Stmt::Cmd {
            program: "echo".into(),
            args: vec![Expr::LitStr("small".into())],
        }];
        let rendered = render_shell_if(&cond, &then_body, &else_body).unwrap();
        assert_eq!(
            rendered,
            "if [ $x -gt 3 ]; then\n\techo big\nelse\n\techo small\nfi"
        );
    }

    #[test]
    fn render_shell_if_nested_loop_body_indents() {
        // A loop inside the then-branch renders recursively with the
        // inner body indented one tab past the loop (two past `if`).
        use xpile_meta_hir::{Expr, LoopKind, Stmt};
        let cond = Expr::LitStr("[ -d /tmp ]".into());
        let inner_loop = Stmt::ShellLoop {
            kind: LoopKind::For {
                var: "f".into(),
                items: vec![Expr::LitStr("a".into()), Expr::LitStr("b".into())],
            },
            body: vec![Stmt::Cmd {
                program: "echo".into(),
                args: vec![Expr::ShellVar("f".into())],
            }],
        };
        let rendered = render_shell_if(&cond, &[inner_loop], &[]).unwrap();
        assert_eq!(
            rendered,
            "if [ -d /tmp ]; then\n\tfor f in a b; do\n\t\techo $f\n\tdone\nfi"
        );
    }

    #[test]
    fn render_shell_loop_while_and_until() {
        // PMAT-048: while/until headers render with the cond Expr.
        use xpile_meta_hir::{Expr, LoopKind};
        let w = LoopKind::While {
            cond: Expr::LitStr("[ -d /tmp ]".into()),
        };
        let rendered = render_shell_loop(&w, &[]).unwrap();
        assert!(
            rendered.starts_with("while [ -d /tmp ]; do\n"),
            "while header wrong: {rendered}"
        );
        let u = LoopKind::Until {
            cond: Expr::LitStr("[ ! -f /tmp/done ]".into()),
        };
        let rendered = render_shell_loop(&u, &[]).unwrap();
        assert!(
            rendered.starts_with("until [ ! -f /tmp/done ]; do\n"),
            "until header wrong: {rendered}"
        );
    }

    #[test]
    fn render_shell_loop_renders_body_statements() {
        // PMAT-974: the load-bearing fix. A loop carrying real body
        // statements must emit them inside `do … done` — previously
        // the entire body was silently dropped and replaced with a
        // `: # body: <pending v0.2.0 expansion>` placeholder.
        use xpile_meta_hir::{Expr, LoopKind, Stmt};
        let kind = LoopKind::For {
            var: "f".into(),
            items: vec![Expr::ShellVar("FILES".into())],
        };
        let body = vec![
            Stmt::Cmd {
                program: "echo".into(),
                args: vec![Expr::ShellVar("f".into())],
            },
            Stmt::Cmd {
                program: "rm".into(),
                args: vec![Expr::LitStr("-f".into()), Expr::ShellVar("f".into())],
            },
        ];
        let rendered = render_shell_loop(&kind, &body).unwrap();
        // Header.
        assert!(
            rendered.starts_with("for f in $FILES; do\n"),
            "header wrong: {rendered}"
        );
        // Both body commands present, indented one tab, in order.
        assert!(
            rendered.contains("\n\techo $f\n"),
            "first body cmd missing or unindented: {rendered}"
        );
        assert!(
            rendered.contains("\n\trm -f $f\n"),
            "second body cmd missing or unindented: {rendered}"
        );
        // The old placeholder must be gone.
        assert!(
            !rendered.contains("pending v0.2.0 expansion"),
            "stale body placeholder leaked: {rendered}"
        );
        assert!(rendered.ends_with("\ndone"), "no closing done: {rendered}");
    }

    #[test]
    fn render_shell_loop_renders_pipeline_and_assign_in_body() {
        // PMAT-974: a loop body can hold any top-level shell stmt —
        // here a ShellAssign and a Pipeline — and both render through
        // the shared walker.
        use xpile_meta_hir::{Expr, LoopKind, Stmt};
        let kind = LoopKind::While {
            cond: Expr::LitStr("[ -d /tmp ]".into()),
        };
        let body = vec![
            Stmt::ShellAssign {
                name: "N".into(),
                value: Expr::CommandSubstitution(Box::new(Stmt::Cmd {
                    program: "wc".into(),
                    args: vec![Expr::LitStr("-l".into())],
                })),
            },
            Stmt::Pipeline {
                stages: vec![
                    Stmt::Cmd {
                        program: "cat".into(),
                        args: vec![Expr::LitStr("log".into())],
                    },
                    Stmt::Cmd {
                        program: "grep".into(),
                        args: vec![Expr::LitStr("err".into())],
                    },
                ],
            },
        ];
        let rendered = render_shell_loop(&kind, &body).unwrap();
        assert!(
            rendered.contains("\n\tN=$(wc -l)\n"),
            "ShellAssign body line missing: {rendered}"
        );
        assert!(
            rendered.contains("\n\tcat log | grep err\n"),
            "Pipeline body line missing: {rendered}"
        );
    }

    #[test]
    fn render_shell_loop_renders_nested_loop_in_body() {
        // PMAT-974: a loop body holding another loop renders
        // recursively through `render_stmt_lines` → `render_shell_loop`.
        use xpile_meta_hir::{Expr, LoopKind, Stmt};
        let inner = Stmt::ShellLoop {
            kind: LoopKind::For {
                var: "j".into(),
                items: vec![Expr::LitStr("x".into()), Expr::LitStr("y".into())],
            },
            body: vec![Stmt::Cmd {
                program: "echo".into(),
                args: vec![Expr::ShellVar("j".into())],
            }],
        };
        let kind = LoopKind::For {
            var: "i".into(),
            items: vec![Expr::LitStr("1".into()), Expr::LitStr("2".into())],
        };
        let rendered = render_shell_loop(&kind, &[inner]).unwrap();
        // Outer header.
        assert!(
            rendered.starts_with("for i in 1 2; do\n"),
            "outer header wrong: {rendered}"
        );
        // Inner loop header indented one tab past the outer body.
        assert!(
            rendered.contains("\n\tfor j in x y; do\n"),
            "nested loop header not indented: {rendered}"
        );
        // Inner body command indented two tabs (one for outer body,
        // one for inner body).
        assert!(
            rendered.contains("\n\t\techo $j\n"),
            "nested loop body not double-indented: {rendered}"
        );
        // Both loops close.
        assert_eq!(
            rendered.matches("done").count(),
            2,
            "both loops should close with `done`: {rendered}"
        );
    }

    #[test]
    fn lower_module_with_loop_emits_full_body() {
        // PMAT-974 end-to-end through `lower`: a Module whose `main`
        // body contains a Stmt::ShellLoop with real body commands
        // must emit the full `do … done` block (not the old
        // placeholder).
        use xpile_meta_hir::{Block, Expr, Function, Item, LoopKind, Stmt, Type};
        let module = Module {
            name: "loopdemo".into(),
            source_lang: xpile_meta_hir::SourceLang::Shell,
            items: vec![Item::Function(Function {
                name: "main".into(),
                params: vec![],
                return_type: Type::I64,
                body: Block {
                    stmts: vec![Stmt::ShellLoop {
                        kind: LoopKind::For {
                            var: "x".into(),
                            items: vec![Expr::LitStr("a".into()), Expr::LitStr("b".into())],
                        },
                        body: vec![Stmt::Cmd {
                            program: "echo".into(),
                            args: vec![Expr::ShellVar("x".into())],
                        }],
                    }],
                    trailing_return: Expr::LitInt(0),
                },
            })],
            ffi_boundaries: vec![],
        };
        let cfg = BackendConfig {
            emit_contracts: true,
            target: Target::Shell,
            profile: Profile::RustOut,
            hardware: None,
        };
        let art = BashrsBackend.lower(&module, &cfg).expect("lower");
        assert!(
            art.primary.contains("for x in a b; do\n\techo $x\ndone\n"),
            "expected full loop block in emit; got:\n{}",
            art.primary
        );
        assert!(
            !art.primary.contains("pending v0.2.0 expansion"),
            "stale placeholder leaked into module emit:\n{}",
            art.primary
        );
    }

    #[test]
    fn render_arg_command_substitution() {
        // PMAT-047: $(cmd) renders with the inner program + args
        // wrapped in `$(...)`.
        use xpile_meta_hir::{Expr, Stmt};
        let zero_arg = Expr::CommandSubstitution(Box::new(Stmt::Cmd {
            program: "date".into(),
            args: vec![],
        }));
        assert_eq!(render_arg(&zero_arg).unwrap(), "$(date)");

        let one_arg = Expr::CommandSubstitution(Box::new(Stmt::Cmd {
            program: "date".into(),
            args: vec![Expr::LitStr("+%Y".into())],
        }));
        assert_eq!(render_arg(&one_arg).unwrap(), "$(date +%Y)");

        // Mixed with ShellVar inside the substitution.
        let mixed = Expr::CommandSubstitution(Box::new(Stmt::Cmd {
            program: "echo".into(),
            args: vec![Expr::ShellVar("HOME".into())],
        }));
        assert_eq!(render_arg(&mixed).unwrap(), "$(echo $HOME)");
    }

    #[test]
    fn render_arg_command_substitution_with_non_cmd_inner_errors() {
        // PMAT-047 defensive: only Stmt::Cmd is supported inside
        // `$(...)` at v0.1.0. A future producer of nested Pipeline /
        // ShellLoop / Assert inside `$(...)` would hit this error.
        use xpile_meta_hir::{Expr, Stmt};
        let bad = Expr::CommandSubstitution(Box::new(Stmt::Assert {
            cond: Expr::LitInt(1),
            msg: None,
        }));
        let err = render_arg(&bad).expect_err("non-Cmd inner must error");
        let msg = format!("{err}");
        assert!(
            msg.contains("only renders Stmt::Cmd inside"),
            "error should explain the v0.1.0 constraint: {msg}"
        );
    }

    #[test]
    fn render_arg_shell_special() {
        // PMAT-055: ShellSpecial renders as `$<char>`.
        use xpile_meta_hir::Expr;
        for (name, expected) in &[
            ("1", "$1"),
            ("?", "$?"),
            ("@", "$@"),
            ("0", "$0"),
            ("#", "$#"),
            ("$", "$$"),
        ] {
            assert_eq!(
                render_arg(&Expr::ShellSpecial(name.to_string())).unwrap(),
                *expected,
                "expected $`{name}` → `{expected}`"
            );
        }
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
            emit_contracts: true,
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

    #[test]
    fn lower_capstone_module_emits_all_layer_b_variants() {
        // PMAT-121: emission-side capstone — mirror of PMAT-092's
        // frontend capstone. Construct a Module that uses every
        // Layer B IR variant currently produced by bashrs-frontend
        // (Stmt::Cmd + Stmt::Pipeline + Stmt::ShellAssign +
        // Expr::LitStr + Expr::QuotedString + Expr::ShellVar +
        // Expr::CommandSubstitution + Expr::ShellSpecial) and
        // verify bashrs-backend emits the expected shell line for
        // each. This guards against a future emission refactor
        // that would regress any one variant's rendering without
        // tripping the narrow per-variant tests.
        //
        // We don't include Stmt::ShellLoop here — full loop-body
        // rendering (PMAT-974) has its own dedicated tests
        // (`render_shell_loop_*` and `lower_module_with_loop_emits_full_body`).
        use xpile_meta_hir::{
            Block, Expr, Function, Item, QuotingStrategy, SourceLang, Stmt, Type,
        };
        let module = Module {
            name: "capstone".into(),
            source_lang: SourceLang::Shell,
            items: vec![Item::Function(Function {
                name: "main".into(),
                params: vec![],
                return_type: Type::I64,
                body: Block {
                    stmts: vec![
                        // 1. ShellAssign with LitStr value
                        Stmt::ShellAssign {
                            name: "PORT".into(),
                            value: Expr::LitStr("8080".into()),
                        },
                        // 2. ShellAssign with CommandSubstitution
                        //    value (today's date)
                        Stmt::ShellAssign {
                            name: "TODAY".into(),
                            value: Expr::CommandSubstitution(Box::new(Stmt::Cmd {
                                program: "date".into(),
                                args: vec![Expr::LitStr("+%Y".into())],
                            })),
                        },
                        // 3. Cmd with mixed args:
                        //    LitStr + ShellVar + ShellSpecial +
                        //    QuotedString (Double)
                        Stmt::Cmd {
                            program: "echo".into(),
                            args: vec![
                                Expr::LitStr("port=".into()),
                                Expr::ShellVar("PORT".into()),
                                Expr::ShellSpecial("@".into()),
                                Expr::QuotedString {
                                    content: "hi $TODAY".into(),
                                    quoting: QuotingStrategy::Double,
                                },
                            ],
                        },
                        // 4. Pipeline with three stages
                        Stmt::Pipeline {
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
                        },
                    ],
                    trailing_return: Expr::LitInt(0),
                },
            })],
            ffi_boundaries: vec![],
        };
        let cfg = BackendConfig {
            emit_contracts: true,
            target: Target::Shell,
            profile: Profile::RustOut,
            hardware: None,
        };
        let art = BashrsBackend.lower(&module, &cfg).expect("lower");
        // ShellAssign with LitStr
        assert!(
            art.primary.contains("\nPORT=8080\n"),
            "expected `PORT=8080` ShellAssign emit; got:\n{}",
            art.primary
        );
        // ShellAssign with CommandSubstitution
        assert!(
            art.primary.contains("\nTODAY=$(date +%Y)\n"),
            "expected `TODAY=$(date +%Y)` ShellAssign emit; got:\n{}",
            art.primary
        );
        // Cmd with mixed-variant args
        assert!(
            art.primary
                .contains("\necho port= $PORT $@ \"hi $TODAY\"\n"),
            "expected mixed-variant echo emit; got:\n{}",
            art.primary
        );
        // Pipeline
        assert!(
            art.primary.contains("\ncat foo | grep bar | wc -l\n"),
            "expected three-stage pipeline emit; got:\n{}",
            art.primary
        );
    }

    #[test]
    fn lower_nonempty_emits_real_shell_not_placeholder_comment() {
        // PMAT-992 honesty guard: for NON-empty input, `lower` must
        // emit real POSIX statements — never the empty-input
        // `# (no commands …)` diagnostic, and never any "placeholder"
        // / "deferred to v0.2.0" stand-in. Pins the property the old
        // module doc-comment lied about (it claimed `lower` emitted a
        // placeholder comment).
        use xpile_meta_hir::{Block, Expr, Function, Item, Stmt, Type};
        let module = Module {
            name: "real".into(),
            source_lang: xpile_meta_hir::SourceLang::Shell,
            items: vec![Item::Function(Function {
                name: "main".into(),
                params: vec![],
                return_type: Type::I64,
                body: Block {
                    stmts: vec![Stmt::ShellAssign {
                        name: "NAME".into(),
                        value: Expr::LitStr("world".into()),
                    }],
                    trailing_return: Expr::LitInt(0),
                },
            })],
            ffi_boundaries: vec![],
        };
        let cfg = BackendConfig {
            emit_contracts: true,
            target: Target::Shell,
            profile: Profile::RustOut,
            hardware: None,
        };
        let art = BashrsBackend.lower(&module, &cfg).expect("lower");
        // Real statement emitted.
        assert!(
            art.primary.contains("\nNAME=world\n"),
            "expected real `NAME=world` statement; got:\n{}",
            art.primary
        );
        // No empty-input diagnostic for non-empty input.
        assert!(
            !art.primary.contains("(no commands"),
            "non-empty input must not emit the empty diagnostic; got:\n{}",
            art.primary
        );
        // No "placeholder" stand-in anywhere in the emit.
        assert!(
            !art.primary.to_lowercase().contains("placeholder"),
            "emit must not contain a placeholder; got:\n{}",
            art.primary
        );
    }
}
