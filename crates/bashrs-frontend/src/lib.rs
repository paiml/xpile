//! POSIX shell frontend for xpile (sh / bash / zsh) — Layer A scaffold.
//!
//! This is the v0.1.0 scaffold for the bashrs merger (see
//! `docs/specifications/sub/bashrs-merger.md`). It implements the
//! [`Frontend`] trait so the dispatch table and `xpile info` recognize
//! the shell domain, but the real parser / lowering pipeline is
//! deferred to v0.2.0 (the `weeks 1-6 extract` phase of Layer A).
//!
//! At v0.1.0 `parse_and_lower` returns a structurally empty `Module`
//! tagged `SourceLang::Shell`. This is enough to:
//!   * Register the frontend with `xpile-core::default_session`.
//!   * Route `.sh` / `.bash` / `.zsh` / `.mk` files through the
//!     standard dispatch surface so a future change can plug in the
//!     real lowering without touching the bin or the session.
//!   * Match extensionless `Makefile` and `Dockerfile` files via
//!     the `Frontend::matches_path` override (PMAT-038).
//!   * Make `xpile transpile foo.sh --target shell` work end-to-end
//!     (the bashrs-backend at v0.1.0 emits a placeholder POSIX
//!     comment carrying the contract citation).

use std::path::Path;
use xpile_frontend::{Frontend, FrontendError};
use xpile_meta_hir::{Block, Expr, Function, Item, Module, SourceLang, Stmt, Type};

/// PMAT-045: lower a single bashrs-frontend token to an `Expr`. The
/// token is the substring between whitespace boundaries (the
/// parser is otherwise naive at v0.1.0 — no quoting awareness, no
/// nested-substitution awareness; that's the v0.2.0 source fold).
///
/// Recognition table:
///   * `$NAME` / `${NAME}` (where NAME is `[A-Za-z_][A-Za-z0-9_]*`)
///     → `Expr::ShellVar(NAME)`. POSIX-legal identifier check is
///     load-bearing — `$1`, `$@`, `$*`, `$?` are *not* recognised
///     at v0.1.0 (positional/special params are XPILE-BASHRS-MERGER-***+).
///   * Everything else → `Expr::LitStr(token)`.
fn lower_token(tok: &str) -> Expr {
    if let Some(rest) = tok.strip_prefix('$') {
        let name = if let Some(stripped) = rest.strip_prefix('{') {
            // `${NAME}` — accept iff the trailing char is `}` AND
            // the contents are a POSIX-legal identifier.
            match stripped.strip_suffix('}') {
                Some(inner) if is_posix_identifier(inner) => inner,
                _ => return Expr::LitStr(tok.to_string()),
            }
        } else if is_posix_identifier(rest) {
            rest
        } else {
            return Expr::LitStr(tok.to_string());
        };
        Expr::ShellVar(name.to_string())
    } else {
        Expr::LitStr(tok.to_string())
    }
}

/// True iff `s` is a POSIX-legal shell variable name (letter or
/// underscore followed by zero or more alphanumerics or underscores).
/// Rejects `$1`, `$@`, `$*`, `$?` etc. — special parameters are
/// XPILE-BASHRS-MERGER-***+.
fn is_posix_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub struct BashrsFrontend;

impl Frontend for BashrsFrontend {
    fn name(&self) -> &'static str {
        "bashrs"
    }

    /// Shell-dialect extensions. Extensionless special-named files
    /// (`Makefile`, `Dockerfile`) are handled by the
    /// `matches_path` override below.
    fn extensions(&self) -> &[&'static str] {
        &["sh", "bash", "zsh", "mk"]
    }

    /// PMAT-038: extend the default extension-based match with the
    /// extensionless `Makefile` / `Dockerfile` cases. The bashrs
    /// domain is unique in xpile for having canonical filenames
    /// without dotted extensions, so it's the one place we need a
    /// non-default `matches_path`. All other frontends fall through
    /// to the trait's default impl (pure extension match) and behave
    /// unchanged.
    fn matches_path(&self, path: &Path) -> bool {
        // Default behaviour first: extension match against our
        // declared list. Mirrors the trait's default body so future
        // additions to `extensions()` automatically pick up here.
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext_str| self.extensions().contains(&ext_str))
            .unwrap_or(false)
        {
            return true;
        }
        // Then extensionless exact-name match. These are the two
        // canonical filenames the bashrs domain covers per
        // `sub/bashrs-merger.md` Layer A; future dialect additions
        // would extend the match set here.
        matches!(
            path.file_name().and_then(|n| n.to_str()),
            Some("Makefile") | Some("Dockerfile")
        )
    }

    fn parse_and_lower(&self, path: &Path, source: &str) -> Result<Module, FrontendError> {
        // PMAT-039: minimum-viable Layer B parser. Each non-empty,
        // non-comment line is split by whitespace; the first token
        // becomes `Stmt::Cmd::program`, the rest become
        // `Stmt::Cmd::args`. This is intentionally NOT a real shell
        // parser — no quoting, no variables, no command substitution,
        // no pipes / redirections / loops. The point is to demonstrate
        // the Layer B IR carries shell semantics end-to-end; the real
        // parser (with the full bashrs corpus and ShellCheck-compatible
        // verifier) folds in at v0.2.0.
        //
        // Shebang lines (`#!/...`) are skipped — they're meta, not a
        // shell statement to execute. Per-line `#` comments are also
        // skipped. Inline `#` comments (e.g., `echo hi  # noisy`) are
        // NOT supported yet — the args would include the `#` token.
        let mut stmts: Vec<Stmt> = Vec::new();
        for raw_line in source.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with("#!") {
                // Shebang. Skip — not a command, just an interpreter
                // directive.
                continue;
            }
            if line.starts_with('#') {
                // Comment line.
                continue;
            }
            // PMAT-041: detect pipeline via `|` separator between
            // command stages. A line without `|` produces a single
            // `Stmt::Cmd`; a line with `|` produces a `Stmt::Pipeline`
            // whose stages are each a Cmd built from that segment's
            // tokens. The split is naive (no quoting awareness yet)
            // — `echo "a | b" | cat` is parsed as three stages, not
            // two with embedded pipe; that improves at v0.2.0 with
            // the real bashrs parser.
            if line.contains('|') {
                let mut stages: Vec<Stmt> = Vec::new();
                for segment in line.split('|') {
                    let trimmed = segment.trim();
                    if trimmed.is_empty() {
                        // Reject `cmd | | cmd` and `| cmd` /
                        // `cmd |` shapes — they're either empty
                        // stages or trailing/leading pipes that
                        // POSIX sh would reject too.
                        return Err(FrontendError::Lower(format!(
                            "shell pipeline at line `{line}` has an empty stage \
                             (leading, trailing, or `| |`); each stage must be a \
                             non-empty command"
                        )));
                    }
                    let mut tokens = trimmed.split_whitespace();
                    let Some(program) = tokens.next() else {
                        // Defensive: split_whitespace on a non-empty
                        // trimmed string always yields ≥1 token.
                        continue;
                    };
                    // PMAT-042 + PMAT-045: each arg is lowered via
                    // `lower_token`, which recognises `$NAME` / `${NAME}`
                    // as `Expr::ShellVar` and everything else as
                    // `Expr::LitStr`. Quoting metadata is still v0.2.0
                    // (the source-fold's real bashrs parser).
                    let args: Vec<Expr> = tokens.map(lower_token).collect();
                    stages.push(Stmt::Cmd {
                        program: program.to_string(),
                        args,
                    });
                }
                if stages.len() < 2 {
                    // Containing `|` but yielding fewer than 2 stages
                    // means the user wrote a degenerate pipeline
                    // (e.g., `||` is shell-OR, which we don't support;
                    // also rejected above as empty-stage). Defensive
                    // belt-and-braces.
                    return Err(FrontendError::Lower(format!(
                        "shell pipeline at line `{line}` parses to {} stage(s); \
                         need ≥2 (use a single command without `|` for one-stage \
                         invocations)",
                        stages.len()
                    )));
                }
                stmts.push(Stmt::Pipeline { stages });
                continue;
            }

            let mut tokens = line.split_whitespace();
            let Some(program) = tokens.next() else {
                continue;
            };
            // PMAT-042 + PMAT-045: see pipeline-stage version above.
            let args: Vec<Expr> = tokens.map(lower_token).collect();
            stmts.push(Stmt::Cmd {
                program: program.to_string(),
                args,
            });
        }

        // Wrap the parsed command sequence in a synthetic `main`
        // function whose body holds the Stmt::Cmds. The function's
        // return type is `Type::I64` representing the script's exit
        // status; the trailing return literal `0` means "exit 0 by
        // default". This is the simplest meta-HIR shape that lets
        // shell scripts coexist with the existing function-centric
        // Module structure; if Layer B grows a richer `Item` taxonomy
        // (e.g., `Item::ShellScript`), this synthetic wrapper goes
        // away.
        let module_name = path
            .file_stem()
            .or_else(|| path.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let items = if stmts.is_empty() {
            // Empty file → still emit an empty `main` so the module
            // structure stays uniform. bashrs-backend handles both
            // shapes.
            Vec::new()
        } else {
            vec![Item::Function(Function {
                name: "main".to_string(),
                params: Vec::new(),
                return_type: Type::I64,
                body: Block {
                    stmts,
                    trailing_return: Expr::LitInt(0),
                },
            })]
        };
        Ok(Module {
            name: module_name,
            source_lang: SourceLang::Shell,
            items,
            ffi_boundaries: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn name_is_bashrs() {
        assert_eq!(BashrsFrontend.name(), "bashrs");
    }

    #[test]
    fn extensions_cover_posix_shell_dialects() {
        let exts = BashrsFrontend.extensions();
        for needle in &["sh", "bash", "zsh", "mk"] {
            assert!(
                exts.contains(needle),
                "BashrsFrontend should recognise `.{needle}`; got {exts:?}"
            );
        }
    }

    #[test]
    fn parse_and_lower_empty_input_yields_empty_items() {
        // Edge case: an empty / whitespace-only file produces no
        // Stmt::Cmd, so we don't synthesise the main function at
        // all. bashrs-backend handles this and emits a well-formed
        // POSIX file with the "(no commands)" diagnostic.
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/empty.sh"), "")
            .expect("empty input");
        assert_eq!(module.name, "empty");
        assert_eq!(module.source_lang, SourceLang::Shell);
        assert!(module.items.is_empty());
    }

    #[test]
    fn parse_and_lower_lowers_each_line_to_stmt_cmd() {
        // PMAT-039 load-bearing: each non-blank, non-comment line
        // becomes one Stmt::Cmd. Order is preserved. Shebang and
        // `#` lines are stripped.
        use xpile_meta_hir::{Item, Stmt};
        let source = "\
#!/bin/sh
# a comment
echo hello world

ls /tmp
# another comment
pwd
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/three_lines.sh"), source)
            .expect("lower three-line script");
        assert_eq!(module.items.len(), 1, "expected one synthesised function");
        let Item::Function(f) = &module.items[0];
        assert_eq!(f.name, "main");
        assert_eq!(f.body.stmts.len(), 3, "expected 3 Stmt::Cmd entries");

        // Order matters.
        // PMAT-042: each arg is now an `Expr::LitStr`.
        use xpile_meta_hir::Expr;
        let stmt0 = &f.body.stmts[0];
        let stmt1 = &f.body.stmts[1];
        let stmt2 = &f.body.stmts[2];
        if let Stmt::Cmd { program, args } = stmt0 {
            assert_eq!(program, "echo");
            assert_eq!(
                args,
                &vec![
                    Expr::LitStr("hello".to_string()),
                    Expr::LitStr("world".to_string()),
                ]
            );
        } else {
            panic!("expected Cmd at [0], got {stmt0:?}");
        }
        if let Stmt::Cmd { program, args } = stmt1 {
            assert_eq!(program, "ls");
            assert_eq!(args, &vec![Expr::LitStr("/tmp".to_string())]);
        } else {
            panic!("expected Cmd at [1], got {stmt1:?}");
        }
        if let Stmt::Cmd { program, args } = stmt2 {
            assert_eq!(program, "pwd");
            assert!(args.is_empty());
        } else {
            panic!("expected Cmd at [2], got {stmt2:?}");
        }
    }

    #[test]
    fn parse_and_lower_skips_pure_whitespace_and_comments() {
        // Only blank-or-comment lines → no items.
        let source = "\
#!/bin/sh
# only comments


# trailing
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/comments.sh"), source)
            .expect("lower comments-only script");
        assert!(
            module.items.is_empty(),
            "expected empty items for comments-only script; got {} items",
            module.items.len()
        );
    }

    #[test]
    fn matches_path_accepts_dotted_extensions() {
        // PMAT-038: the override must preserve the default
        // extension-match behaviour for `.sh` / `.bash` / `.zsh` /
        // `.mk`. If anyone tightens the override and accidentally
        // drops one of these, this fires.
        for path in &[
            "/tmp/foo.sh",
            "/tmp/foo.bash",
            "/tmp/foo.zsh",
            "/tmp/foo.mk",
            "/usr/local/bin/script.sh",
        ] {
            assert!(
                BashrsFrontend.matches_path(&PathBuf::from(path)),
                "expected match on {path}"
            );
        }
    }

    #[test]
    fn matches_path_accepts_extensionless_makefile_and_dockerfile() {
        // PMAT-038: the load-bearing claim of the override —
        // extensionless canonical filenames route through
        // bashrs-frontend.
        for path in &[
            "Makefile",
            "Dockerfile",
            "/home/user/project/Makefile",
            "./build/Dockerfile",
        ] {
            assert!(
                BashrsFrontend.matches_path(&PathBuf::from(path)),
                "expected match on {path}"
            );
        }
    }

    #[test]
    fn matches_path_rejects_unrelated_files() {
        // Negative test: ensure the override doesn't grab unrelated
        // files. The .py / .c / .rs cases are the most important
        // because false-positives here would steal dispatch from
        // the python / c / (future) rust frontends.
        for path in &[
            "/tmp/foo.py",
            "/tmp/foo.c",
            "/tmp/foo.rs",
            "/tmp/foo.lean",
            "/tmp/Cargo.toml",
            "/tmp/README.md",
            "/tmp/no_extension_unrelated_name",
            // Substring traps: `Makefile.in` and `Dockerfile.dev`
            // are conventional auxiliary filenames; we don't claim
            // them at v0.1.0 because they typically aren't the
            // canonical syntax bashrs handles.
            "/tmp/Makefile.in",
            "/tmp/Dockerfile.dev",
        ] {
            assert!(
                !BashrsFrontend.matches_path(&PathBuf::from(path)),
                "should NOT match {path}"
            );
        }
    }

    #[test]
    fn parse_and_lower_two_stage_pipeline_produces_stmt_pipeline() {
        // PMAT-041 load-bearing: `cmd1 | cmd2` lowers to Stmt::Pipeline
        // with two Stmt::Cmd stages.
        // PMAT-042: args are Vec<Expr::LitStr>.
        use xpile_meta_hir::{Expr, Item, Stmt};
        let source = "ls /tmp | wc -l\n";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/pipe.sh"), source)
            .expect("parse pipeline");
        assert_eq!(module.items.len(), 1);
        let Item::Function(f) = &module.items[0];
        assert_eq!(f.body.stmts.len(), 1);
        let Stmt::Pipeline { stages } = &f.body.stmts[0] else {
            panic!("expected Pipeline at [0], got {:?}", f.body.stmts[0]);
        };
        assert_eq!(stages.len(), 2);
        if let Stmt::Cmd { program, args } = &stages[0] {
            assert_eq!(program, "ls");
            assert_eq!(args, &vec![Expr::LitStr("/tmp".to_string())]);
        } else {
            panic!("expected Cmd stage [0], got {:?}", stages[0]);
        }
        if let Stmt::Cmd { program, args } = &stages[1] {
            assert_eq!(program, "wc");
            assert_eq!(args, &vec![Expr::LitStr("-l".to_string())]);
        } else {
            panic!("expected Cmd stage [1], got {:?}", stages[1]);
        }
    }

    #[test]
    fn parse_and_lower_three_stage_pipeline() {
        // PMAT-041: extends naturally past 2 stages.
        use xpile_meta_hir::{Item, Stmt};
        let module = BashrsFrontend
            .parse_and_lower(
                &PathBuf::from("/tmp/three.sh"),
                "cat foo | grep bar | wc -l\n",
            )
            .expect("parse 3-stage pipeline");
        let Item::Function(f) = &module.items[0];
        let Stmt::Pipeline { stages } = &f.body.stmts[0] else {
            panic!("expected Pipeline");
        };
        assert_eq!(stages.len(), 3);
    }

    #[test]
    fn parse_and_lower_rejects_empty_stage() {
        // PMAT-041 negative: `cmd | | cmd`, `| cmd`, `cmd |` all
        // produce empty stages that POSIX sh would reject — we
        // reject them too with a clear diagnostic.
        for source in &["| ls\n", "ls |\n", "ls | | wc\n"] {
            let err = BashrsFrontend
                .parse_and_lower(&PathBuf::from("/tmp/bad.sh"), source)
                .expect_err(&format!("should reject empty stage in `{source}`"));
            let msg = format!("{err}");
            assert!(
                msg.contains("empty stage"),
                "error must mention empty stage for `{source}`: {msg}"
            );
        }
    }

    #[test]
    fn parse_and_lower_single_stage_no_pipe_still_emits_cmd() {
        // Regression guard for PMAT-041: a line WITHOUT `|` continues
        // to produce a Stmt::Cmd (not a 1-stage Pipeline). The
        // PMAT-039 behaviour is unchanged.
        use xpile_meta_hir::{Item, Stmt};
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/plain.sh"), "echo hi\n")
            .expect("parse plain cmd");
        let Item::Function(f) = &module.items[0];
        assert!(
            matches!(&f.body.stmts[0], Stmt::Cmd { .. }),
            "single-token line must remain a Cmd, not a Pipeline: got {:?}",
            f.body.stmts[0]
        );
    }

    #[test]
    fn lower_token_recognises_dollar_name() {
        // PMAT-045 load-bearing: `$NAME` → Expr::ShellVar.
        use xpile_meta_hir::Expr;
        assert_eq!(lower_token("$HOME"), Expr::ShellVar("HOME".to_string()));
        assert_eq!(lower_token("$USER"), Expr::ShellVar("USER".to_string()));
        assert_eq!(lower_token("$_x"), Expr::ShellVar("_x".to_string()));
        assert_eq!(
            lower_token("$snake_case_var_2"),
            Expr::ShellVar("snake_case_var_2".to_string())
        );
    }

    #[test]
    fn lower_token_recognises_dollar_brace_name() {
        // PMAT-045: `${NAME}` form, same disposition.
        use xpile_meta_hir::Expr;
        assert_eq!(lower_token("${HOME}"), Expr::ShellVar("HOME".to_string()));
        assert_eq!(
            lower_token("${snake_case_var_2}"),
            Expr::ShellVar("snake_case_var_2".to_string())
        );
    }

    #[test]
    fn lower_token_rejects_special_params_as_litstr() {
        // PMAT-045 negative: `$1`, `$@`, `$?`, etc. are POSIX special
        // params, not user-named variables. At v0.1.0 we keep them
        // as LitStr (bareword `$1` survives literal-through). A
        // future PR may add Expr::ShellPosParam(u32) or similar.
        use xpile_meta_hir::Expr;
        for bad in &["$1", "$@", "$?", "$*", "$0", "$-"] {
            assert_eq!(
                lower_token(bad),
                Expr::LitStr(bad.to_string()),
                "expected special-param `{bad}` to fall through as LitStr"
            );
        }
    }

    #[test]
    fn lower_token_rejects_malformed_brace_as_litstr() {
        // PMAT-045: `${` without a closing `}` falls through to LitStr.
        // Same for `${INVALID-NAME}` (hyphens aren't POSIX-legal).
        use xpile_meta_hir::Expr;
        for bad in &["${HOME", "${1}", "${ALSO BAD}", "${has-hyphen}"] {
            assert_eq!(
                lower_token(bad),
                Expr::LitStr(bad.to_string()),
                "expected malformed `{bad}` to fall through as LitStr"
            );
        }
    }

    #[test]
    fn lower_token_plain_strings_pass_through_as_litstr() {
        // Regression: non-dollar tokens stay LitStr. Locks in that
        // PMAT-045 doesn't accidentally claim arbitrary input.
        use xpile_meta_hir::Expr;
        for plain in &["foo", "bar.baz", "-l", "/tmp/path", "0", "123abc"] {
            assert_eq!(lower_token(plain), Expr::LitStr(plain.to_string()));
        }
    }

    #[test]
    fn parse_and_lower_with_shell_var_arg() {
        // PMAT-045 end-to-end: a Cmd line with `$NAME` produces a
        // Cmd whose args include an Expr::ShellVar.
        use xpile_meta_hir::{Expr, Item, Stmt};
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/v.sh"), "echo $HOME end\n")
            .expect("parse");
        let Item::Function(f) = &module.items[0];
        let Stmt::Cmd { program, args } = &f.body.stmts[0] else {
            panic!("expected Cmd");
        };
        assert_eq!(program, "echo");
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], Expr::ShellVar("HOME".to_string()));
        assert_eq!(args[1], Expr::LitStr("end".to_string()));
    }
}
