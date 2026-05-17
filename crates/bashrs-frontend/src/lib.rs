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
use xpile_meta_hir::{
    Block, Expr, Function, Item, Module, QuotingStrategy, SourceLang, Stmt, Type,
};

/// PMAT-049: raw token produced by the bashrs-frontend tokenizer.
/// Distinguishes barewords from quoted strings so the downstream
/// lowering can produce the right `Expr` variant (LitStr / ShellVar
/// for barewords, QuotedString for quoted regions).
#[derive(Debug, Clone, PartialEq)]
enum RawToken {
    /// Whitespace-separated bareword. Forwarded to `lower_token`
    /// which further distinguishes `$NAME` (ShellVar) from
    /// everything else (LitStr).
    Bare(String),
    /// `'...'` — single-quoted region. No expansion / no escapes
    /// inside (POSIX semantics). Lowers to
    /// `Expr::QuotedString { quoting: Single }`.
    SingleQuoted(String),
    /// `"..."` — double-quoted region. Variable expansion happens
    /// at shell-execution time (preserved as-is in content at
    /// v0.1.0; future PR adds an Expr-template variant). Lowers
    /// to `Expr::QuotedString { quoting: Double }`.
    DoubleQuoted(String),
    /// `$(cmd args...)` — command substitution. PMAT-050. Inner
    /// content is the substring between the matching parentheses
    /// (no nesting allowed at v0.1.0). Lowers to
    /// `Expr::CommandSubstitution(Box<Stmt::Cmd>)` by recursively
    /// tokenizing + parsing the inner content as a single Cmd.
    CommandSubst(String),
}

/// PMAT-049: tokenize a non-empty trimmed shell line into raw
/// tokens. Recognises single and double quotes; bareword regions
/// are split on whitespace. Returns an error on unterminated
/// quotes — POSIX sh rejects those too.
///
/// What's deliberately NOT here (v0.2.0 source fold delivers):
///   * Escape sequences (`\"` / `\'` / `\\`) — quotes don't have
///     escapes inside themselves at v0.1.0; double quotes don't
///     interpret `\$` etc.
///   * String concatenation (`foo"bar"` → one token `foobar`) —
///     v0.1.0 requires quotes to appear at token boundaries.
///   * Inline `#` comments — `echo hi # noisy` is treated as 3
///     bareword tokens including the `#` (existing behaviour).
fn tokenize_line(line: &str) -> Result<Vec<RawToken>, FrontendError> {
    let mut tokens: Vec<RawToken> = Vec::new();
    let mut current: String = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // PMAT-054: inline `#` comment. POSIX shell strips
            // `#` to end-of-line *when it appears at a word
            // boundary* (i.e., not adjacent to a bareword). So
            // `echo hi # noisy` strips ` # noisy`, but `echo a#b`
            // keeps `a#b` as one token. Quoted regions are
            // unaffected (the `#` inside `"..."` or `'...'` is
            // literal — handled by the quote arms below before
            // we ever reach this match).
            //
            // What we DON'T handle yet: `#` inside backslash escape
            // (POSIX corner case; v0.2.0 source fold).
            '#' if current.is_empty() => {
                // Drain the rest of the input — everything after
                // is comment.
                for _ in chars.by_ref() {}
                break;
            }
            c if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(RawToken::Bare(std::mem::take(&mut current)));
                }
            }
            // PMAT-053: backtick `` `cmd` `` command substitution.
            // Semantically identical to `$(cmd)` (POSIX older
            // syntax). Reuses `RawToken::CommandSubst` so the
            // lowering path is unchanged. No nesting supported at
            // v0.1.0 (POSIX backticks technically allow it via
            // backslash-escaping but it's a horror-show; the v0.2.0
            // bashrs source fold will handle the corner cases).
            '`' => {
                if !current.is_empty() {
                    return Err(FrontendError::Lower(format!(
                        "shell line `{line}` has a backtick adjacent to a bareword \
                         (e.g., `foo`bar``); v0.1.0 requires backticks at token boundaries"
                    )));
                }
                let mut content = String::new();
                let mut closed = false;
                for inner in chars.by_ref() {
                    if inner == '`' {
                        closed = true;
                        break;
                    }
                    content.push(inner);
                }
                if !closed {
                    return Err(FrontendError::Lower(format!(
                        "shell line `{line}` has an unterminated backtick substitution"
                    )));
                }
                tokens.push(RawToken::CommandSubst(content));
            }
            // PMAT-050: `$(cmd)` command substitution. Recognised as
            // an atomic token; inner content is captured verbatim
            // and lowered into Stmt::Cmd by `lower_raw_token`.
            // Nested `$(...)` is rejected at v0.1.0.
            '$' if chars.peek() == Some(&'(') => {
                if !current.is_empty() {
                    return Err(FrontendError::Lower(format!(
                        "shell line `{line}` has `$(` adjacent to a bareword \
                         (e.g., `foo$(bar)`); v0.1.0 requires `$(...)` at token boundaries"
                    )));
                }
                chars.next(); // consume the `(`
                let mut content = String::new();
                let mut closed = false;
                for inner in chars.by_ref() {
                    if inner == '(' {
                        return Err(FrontendError::Lower(format!(
                            "shell line `{line}` has nested `$(...)` — v0.1.0 supports only one level"
                        )));
                    }
                    if inner == ')' {
                        closed = true;
                        break;
                    }
                    content.push(inner);
                }
                if !closed {
                    return Err(FrontendError::Lower(format!(
                        "shell line `{line}` has an unterminated `$(...)` substitution"
                    )));
                }
                tokens.push(RawToken::CommandSubst(content));
            }
            '\'' => {
                if !current.is_empty() {
                    // String concatenation isn't supported at v0.1.0.
                    return Err(FrontendError::Lower(format!(
                        "shell line `{line}` has a single quote adjacent to a bareword \
                         (e.g., `foo'bar'`); v0.1.0 requires quotes at token boundaries"
                    )));
                }
                let mut content = String::new();
                let mut closed = false;
                for inner in chars.by_ref() {
                    if inner == '\'' {
                        closed = true;
                        break;
                    }
                    content.push(inner);
                }
                if !closed {
                    return Err(FrontendError::Lower(format!(
                        "shell line `{line}` has an unterminated single quote"
                    )));
                }
                tokens.push(RawToken::SingleQuoted(content));
            }
            '"' => {
                if !current.is_empty() {
                    return Err(FrontendError::Lower(format!(
                        "shell line `{line}` has a double quote adjacent to a bareword \
                         (e.g., `foo\"bar\"`); v0.1.0 requires quotes at token boundaries"
                    )));
                }
                let mut content = String::new();
                let mut closed = false;
                for inner in chars.by_ref() {
                    if inner == '"' {
                        closed = true;
                        break;
                    }
                    content.push(inner);
                }
                if !closed {
                    return Err(FrontendError::Lower(format!(
                        "shell line `{line}` has an unterminated double quote"
                    )));
                }
                tokens.push(RawToken::DoubleQuoted(content));
            }
            other => current.push(other),
        }
    }
    if !current.is_empty() {
        tokens.push(RawToken::Bare(current));
    }
    Ok(tokens)
}

/// PMAT-049 + PMAT-050: convert a raw token to the appropriate Expr
/// variant. PMAT-050 adds `RawToken::CommandSubst` recursion —
/// the inner content of `$(cmd args)` is re-tokenized and lowered
/// into a `Stmt::Cmd` that becomes
/// `Expr::CommandSubstitution(Box<Stmt::Cmd>)`.
fn lower_raw_token(t: &RawToken) -> Result<Expr, FrontendError> {
    match t {
        RawToken::Bare(s) => Ok(lower_token(s)),
        RawToken::SingleQuoted(s) => Ok(Expr::QuotedString {
            content: s.clone(),
            quoting: QuotingStrategy::Single,
        }),
        RawToken::DoubleQuoted(s) => Ok(Expr::QuotedString {
            content: s.clone(),
            quoting: QuotingStrategy::Double,
        }),
        RawToken::CommandSubst(inner) => {
            // Recursively tokenize the inner content as a single Cmd.
            // Empty `$()` is rejected since shell `$()` requires a
            // command to substitute.
            let trimmed = inner.trim();
            if trimmed.is_empty() {
                return Err(FrontendError::Lower(
                    "command substitution `$()` is empty; v0.1.0 requires \
                     `$(cmd ...)` with a non-empty inner command"
                        .into(),
                ));
            }
            let raw = tokenize_line(trimmed)?;
            let mut iter = raw.iter();
            let Some(first) = iter.next() else {
                return Err(FrontendError::Lower(
                    "command substitution inner tokenized to zero tokens".into(),
                ));
            };
            let program = match first {
                RawToken::Bare(s) => s.clone(),
                _ => {
                    return Err(FrontendError::Lower(format!(
                        "command substitution `$({inner})` starts with a quoted / nested \
                         token; v0.1.0 requires the inner program to be a bareword"
                    )));
                }
            };
            let args: Vec<Expr> = iter.map(lower_raw_token).collect::<Result<Vec<_>, _>>()?;
            Ok(Expr::CommandSubstitution(Box::new(Stmt::Cmd {
                program,
                args,
            })))
        }
    }
}

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
            // PMAT-051: detect `NAME=value` variable assignment at
            // the start of a line. Recognises the canonical POSIX
            // form: one bareword token whose name part is a
            // POSIX-legal identifier, immediately followed by `=`,
            // immediately followed by the value (the rest of the
            // first token; further whitespace-separated tokens on
            // the line are NOT supported at v0.1.0 — that'd be the
            // `VAR=val cmd args` "exported once for next command"
            // POSIX form). Whitespace around `=` is disallowed by
            // POSIX, and we follow.
            if let Some(eq_idx) = line.find('=') {
                let name_part = &line[..eq_idx];
                let value_part = &line[eq_idx + 1..];
                if is_posix_identifier(name_part) {
                    // Tokenize the value_part so we can distinguish:
                    //   - exactly one token → `Stmt::ShellAssign`
                    //   - multiple tokens → POSIX's
                    //     `VAR=val cmd args` "exported for next
                    //     command" form (not supported at v0.1.0)
                    // This is quoting-aware — `NAME="Noah Gift"` is
                    // one (DoubleQuoted) token, not two barewords.
                    let value_tokens = tokenize_line(value_part)?;
                    match value_tokens.len() {
                        0 => {
                            // `NAME=` with empty value — POSIX-legal,
                            // means unset / empty. We model as
                            // LitStr("").
                            stmts.push(Stmt::ShellAssign {
                                name: name_part.to_string(),
                                value: Expr::LitStr(String::new()),
                            });
                            continue;
                        }
                        1 => {
                            let value_expr = lower_raw_token(&value_tokens[0])?;
                            stmts.push(Stmt::ShellAssign {
                                name: name_part.to_string(),
                                value: value_expr,
                            });
                            continue;
                        }
                        _ => {
                            // Multi-token RHS = the
                            // `VAR=val cmd args` POSIX form.
                            // Reject at v0.1.0 — it's a less-used
                            // idiom and supporting it correctly
                            // requires modelling temporary-export
                            // semantics. Fall through is *not*
                            // safe (the line doesn't pipe or
                            // bareword-command cleanly); error
                            // explicitly.
                            return Err(FrontendError::Lower(format!(
                                "shell line `{line}` has `VAR=val cmd args` shape — \
                                 v0.1.0 supports only single-value assignments \
                                 (`VAR=value` on its own line)"
                            )));
                        }
                    }
                }
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
                    // PMAT-049: quoting-aware tokenizer. The program
                    // name must be a Bare token (a quoted program
                    // name is unusual and unsupported at v0.1.0).
                    let raw_tokens = tokenize_line(trimmed)?;
                    let mut iter = raw_tokens.iter();
                    let Some(first) = iter.next() else {
                        continue;
                    };
                    let program = match first {
                        RawToken::Bare(s) => s.clone(),
                        _ => {
                            return Err(FrontendError::Lower(format!(
                                "shell pipeline stage `{trimmed}` starts with a quoted \
                                 program name; v0.1.0 requires the program to be a \
                                 bareword token"
                            )));
                        }
                    };
                    let args: Vec<Expr> =
                        iter.map(lower_raw_token).collect::<Result<Vec<_>, _>>()?;
                    stages.push(Stmt::Cmd { program, args });
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

            // PMAT-049: quoting-aware tokenizer. Same logic as the
            // pipeline-stage version above.
            let raw_tokens = tokenize_line(line)?;
            let mut iter = raw_tokens.iter();
            let Some(first) = iter.next() else {
                continue;
            };
            let program = match first {
                RawToken::Bare(s) => s.clone(),
                _ => {
                    return Err(FrontendError::Lower(format!(
                        "shell line `{line}` starts with a quoted program name; \
                         v0.1.0 requires the program to be a bareword token"
                    )));
                }
            };
            let args: Vec<Expr> = iter.map(lower_raw_token).collect::<Result<Vec<_>, _>>()?;
            stmts.push(Stmt::Cmd { program, args });
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
    fn tokenize_line_handles_quoted_strings() {
        // PMAT-049 load-bearing: quoted regions become atomic
        // tokens; embedded whitespace is preserved.
        let toks = tokenize_line("echo \"hello world\" foo").expect("parse");
        assert_eq!(
            toks,
            vec![
                RawToken::Bare("echo".into()),
                RawToken::DoubleQuoted("hello world".into()),
                RawToken::Bare("foo".into()),
            ]
        );

        let toks = tokenize_line("echo 'a b c' done").expect("parse");
        assert_eq!(
            toks,
            vec![
                RawToken::Bare("echo".into()),
                RawToken::SingleQuoted("a b c".into()),
                RawToken::Bare("done".into()),
            ]
        );

        // Mixed quoting.
        let toks = tokenize_line("echo 'sq' \"dq\"").expect("parse");
        assert_eq!(
            toks,
            vec![
                RawToken::Bare("echo".into()),
                RawToken::SingleQuoted("sq".into()),
                RawToken::DoubleQuoted("dq".into()),
            ]
        );
    }

    #[test]
    fn tokenize_line_rejects_unterminated_quotes() {
        // PMAT-049 negative: unterminated quotes are rejected with a
        // precise diagnostic.
        for bad in &[
            "echo \"unterminated",
            "echo 'still hanging",
            "echo \"a\" 'b",
        ] {
            let err = tokenize_line(bad).expect_err(&format!("should reject `{bad}`"));
            let msg = format!("{err}");
            assert!(
                msg.contains("unterminated"),
                "error for `{bad}` should mention unterminated: {msg}"
            );
        }
    }

    #[test]
    fn tokenize_line_rejects_adjacent_quotes() {
        // PMAT-049 negative: string concatenation isn't supported at
        // v0.1.0. `foo"bar"` would produce one token in POSIX sh
        // (`foobar`), but the v0.1.0 tokenizer requires quotes at
        // token boundaries.
        let err = tokenize_line("echo foo\"bar\"").expect_err("should reject adjacent");
        let msg = format!("{err}");
        assert!(
            msg.contains("adjacent") || msg.contains("token boundaries"),
            "error should mention token-boundary requirement: {msg}"
        );
    }

    #[test]
    fn tokenize_line_plain_words_match_split_whitespace() {
        // PMAT-049 regression: pre-PMAT-049 behaviour on
        // quote-free input must be preserved (just barewords).
        let toks = tokenize_line("echo foo bar baz").expect("parse");
        assert_eq!(
            toks,
            vec![
                RawToken::Bare("echo".into()),
                RawToken::Bare("foo".into()),
                RawToken::Bare("bar".into()),
                RawToken::Bare("baz".into()),
            ]
        );
    }

    #[test]
    fn tokenize_line_recognises_command_substitution() {
        // PMAT-050: `$(cmd)` becomes a CommandSubst token.
        let toks = tokenize_line("echo today is $(date)").expect("parse");
        assert_eq!(
            toks,
            vec![
                RawToken::Bare("echo".into()),
                RawToken::Bare("today".into()),
                RawToken::Bare("is".into()),
                RawToken::CommandSubst("date".into()),
            ]
        );

        // Multiple substitutions in one line.
        let toks = tokenize_line("echo $(date +%Y) and $(uname -a)").expect("parse");
        assert_eq!(
            toks,
            vec![
                RawToken::Bare("echo".into()),
                RawToken::CommandSubst("date +%Y".into()),
                RawToken::Bare("and".into()),
                RawToken::CommandSubst("uname -a".into()),
            ]
        );
    }

    #[test]
    fn tokenize_line_strips_inline_comments() {
        // PMAT-054: `#` at a word boundary starts a comment that
        // runs to end-of-line.
        let toks = tokenize_line("echo hi # this is a comment").expect("parse");
        assert_eq!(
            toks,
            vec![RawToken::Bare("echo".into()), RawToken::Bare("hi".into())],
            "expected `# this is a comment` to be stripped"
        );

        // `#` mid-token is NOT a comment — POSIX requires word
        // boundary.
        let toks = tokenize_line("echo a#b # but here it is").expect("parse");
        assert_eq!(
            toks,
            vec![
                RawToken::Bare("echo".into()),
                RawToken::Bare("a#b".into()),
            ],
            "expected `a#b` to stay one token; trailing `# but here` stripped"
        );

        // Comment-only line collapses to zero tokens.
        let toks = tokenize_line("# pure comment line").expect("parse");
        assert!(
            toks.is_empty(),
            "expected zero tokens from comment-only line; got {toks:?}"
        );
    }

    #[test]
    fn tokenize_line_preserves_hash_inside_quotes() {
        // PMAT-054 negative: `#` inside `'...'` and `"..."` is
        // literal content, not a comment-start.
        use xpile_meta_hir::QuotingStrategy;
        let toks = tokenize_line("echo 'hash # inside' end").expect("parse");
        assert_eq!(toks.len(), 3, "expected 3 tokens; got {toks:?}");
        match &toks[1] {
            RawToken::SingleQuoted(s) if s == "hash # inside" => (),
            other => panic!("expected SingleQuoted with `hash # inside`, got {other:?}"),
        }
        // Smoke-test it doesn't error on legal but tricky combos.
        let _ = QuotingStrategy::Single; // keep the import used in case
    }

    #[test]
    fn tokenize_line_recognises_backtick_substitution() {
        // PMAT-053: `` `cmd` `` becomes a CommandSubst token —
        // semantically identical to `$(cmd)`.
        let toks = tokenize_line("echo today is `date`").expect("parse");
        assert_eq!(
            toks,
            vec![
                RawToken::Bare("echo".into()),
                RawToken::Bare("today".into()),
                RawToken::Bare("is".into()),
                RawToken::CommandSubst("date".into()),
            ]
        );

        // With args inside the substitution.
        let toks = tokenize_line("echo `uname -a` end").expect("parse");
        assert_eq!(
            toks,
            vec![
                RawToken::Bare("echo".into()),
                RawToken::CommandSubst("uname -a".into()),
                RawToken::Bare("end".into()),
            ]
        );
    }

    #[test]
    fn tokenize_line_rejects_unterminated_backtick_substitution() {
        // PMAT-053 negative: `` `cmd `` without closing backtick.
        let err = tokenize_line("echo `date").expect_err("should reject");
        let msg = format!("{err}");
        assert!(
            msg.contains("unterminated") && msg.contains("backtick"),
            "error should mention unterminated backtick: {msg}"
        );
    }

    #[test]
    fn parse_and_lower_with_backtick_substitution_normalises_to_dollar_paren() {
        // PMAT-053 end-to-end: backtick input lowers to
        // Expr::CommandSubstitution, which renders as `$(cmd)` —
        // the modern POSIX canonical form. So `echo `date`` round-
        // trips as `echo $(date)`.
        use xpile_meta_hir::{Expr, Item, Stmt};
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/bt.sh"), "echo `date`\n")
            .expect("parse");
        let Item::Function(f) = &module.items[0];
        let Stmt::Cmd { program, args } = &f.body.stmts[0] else {
            panic!("expected Cmd");
        };
        assert_eq!(program, "echo");
        assert_eq!(args.len(), 1);
        let Expr::CommandSubstitution(_) = &args[0] else {
            panic!(
                "expected backticks to lower to CommandSubstitution; got {:?}",
                args[0]
            );
        };
    }

    #[test]
    fn tokenize_line_rejects_unterminated_command_substitution() {
        // PMAT-050 negative: `$(cmd` without `)` errors.
        let err = tokenize_line("echo $(date").expect_err("should reject");
        let msg = format!("{err}");
        assert!(
            msg.contains("unterminated") && msg.contains("$("),
            "error should mention unterminated $(...): {msg}"
        );
    }

    #[test]
    fn tokenize_line_rejects_nested_command_substitution() {
        // PMAT-050 negative: `$($(cmd))` rejected at v0.1.0.
        let err = tokenize_line("echo $($(date))").expect_err("should reject nested substitution");
        let msg = format!("{err}");
        assert!(
            msg.contains("nested"),
            "error should mention nesting: {msg}"
        );
    }

    #[test]
    fn lower_raw_token_command_substitution_produces_expr() {
        // PMAT-050 load-bearing: a CommandSubst raw token lowers to
        // Expr::CommandSubstitution(Box<Stmt::Cmd>) with the inner
        // program + args correctly parsed.
        use xpile_meta_hir::{Expr, Stmt};
        let raw = RawToken::CommandSubst("date +%Y".into());
        let expr = lower_raw_token(&raw).expect("lower");
        let Expr::CommandSubstitution(inner) = expr else {
            panic!("expected CommandSubstitution; got {expr:?}");
        };
        let Stmt::Cmd { program, args } = inner.as_ref() else {
            panic!("expected inner Cmd; got {inner:?}");
        };
        assert_eq!(program, "date");
        assert_eq!(args, &vec![Expr::LitStr("+%Y".into())]);
    }

    #[test]
    fn parse_and_lower_simple_shell_assign() {
        // PMAT-051: `LOG=/tmp/foo` produces Stmt::ShellAssign with
        // a LitStr value.
        use xpile_meta_hir::{Expr, Item, Stmt};
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/a.sh"), "LOG=/tmp/build.log\n")
            .expect("parse");
        let Item::Function(f) = &module.items[0];
        assert_eq!(f.body.stmts.len(), 1);
        let Stmt::ShellAssign { name, value } = &f.body.stmts[0] else {
            panic!("expected ShellAssign; got {:?}", f.body.stmts[0]);
        };
        assert_eq!(name, "LOG");
        assert_eq!(value, &Expr::LitStr("/tmp/build.log".into()));
    }

    #[test]
    fn parse_and_lower_shell_assign_with_command_substitution_value() {
        // PMAT-051 + PMAT-050: `TODAY=$(date)` composes ShellAssign
        // with CommandSubstitution.
        use xpile_meta_hir::{Expr, Item, Stmt};
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/a.sh"), "TODAY=$(date)\n")
            .expect("parse");
        let Item::Function(f) = &module.items[0];
        let Stmt::ShellAssign { name, value } = &f.body.stmts[0] else {
            panic!("expected ShellAssign");
        };
        assert_eq!(name, "TODAY");
        let Expr::CommandSubstitution(inner) = value else {
            panic!("expected CommandSubstitution value");
        };
        let Stmt::Cmd { program, args } = inner.as_ref() else {
            panic!("expected inner Cmd");
        };
        assert_eq!(program, "date");
        assert!(args.is_empty());
    }

    #[test]
    fn parse_and_lower_shell_assign_with_quoted_value() {
        // PMAT-051 + PMAT-049: `NAME="Noah Gift"` composes
        // ShellAssign with QuotedString.
        use xpile_meta_hir::{Expr, Item, QuotingStrategy, Stmt};
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/a.sh"), "NAME=\"Noah Gift\"\n")
            .expect("parse");
        let Item::Function(f) = &module.items[0];
        let Stmt::ShellAssign { name, value } = &f.body.stmts[0] else {
            panic!("expected ShellAssign");
        };
        assert_eq!(name, "NAME");
        assert_eq!(
            value,
            &Expr::QuotedString {
                content: "Noah Gift".into(),
                quoting: QuotingStrategy::Double,
            }
        );
    }

    #[test]
    fn parse_and_lower_rejects_var_eq_val_cmd_args_form() {
        // PMAT-051 negative: POSIX's `VAR=val cmd args`
        // (export-for-next-cmd) is rejected at v0.1.0.
        let err = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/bad.sh"), "FOO=bar echo hi\n")
            .expect_err("should reject VAR=val cmd args");
        let msg = format!("{err}");
        assert!(
            msg.contains("VAR=val cmd args"),
            "error should explain the unsupported shape: {msg}"
        );
    }

    #[test]
    fn parse_and_lower_with_command_substitution() {
        // PMAT-050 end-to-end: shell input with `$(...)` produces
        // Cmd with CommandSubstitution arg.
        use xpile_meta_hir::{Expr, Item, Stmt};
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/cs.sh"), "echo today is $(date)\n")
            .expect("parse");
        let Item::Function(f) = &module.items[0];
        let Stmt::Cmd { program, args } = &f.body.stmts[0] else {
            panic!("expected Cmd");
        };
        assert_eq!(program, "echo");
        assert_eq!(args.len(), 3);
        assert_eq!(args[0], Expr::LitStr("today".into()));
        assert_eq!(args[1], Expr::LitStr("is".into()));
        let Expr::CommandSubstitution(inner) = &args[2] else {
            panic!("expected CommandSubstitution at args[2]; got {:?}", args[2]);
        };
        let Stmt::Cmd {
            program: ip,
            args: ia,
        } = inner.as_ref()
        else {
            panic!("expected inner Cmd");
        };
        assert_eq!(ip, "date");
        assert!(ia.is_empty());
    }

    #[test]
    fn parse_and_lower_with_quoted_string_arg() {
        // PMAT-049 end-to-end through parse_and_lower: a quoted-arg
        // line produces a Stmt::Cmd with Expr::QuotedString args.
        use xpile_meta_hir::{Expr, Item, QuotingStrategy, Stmt};
        let module = BashrsFrontend
            .parse_and_lower(
                &PathBuf::from("/tmp/q.sh"),
                "echo \"hello world\" foo 'bar baz'\n",
            )
            .expect("parse");
        let Item::Function(f) = &module.items[0];
        let Stmt::Cmd { program, args } = &f.body.stmts[0] else {
            panic!("expected Cmd");
        };
        assert_eq!(program, "echo");
        assert_eq!(args.len(), 3);
        assert_eq!(
            args[0],
            Expr::QuotedString {
                content: "hello world".into(),
                quoting: QuotingStrategy::Double,
            }
        );
        assert_eq!(args[1], Expr::LitStr("foo".into()));
        assert_eq!(
            args[2],
            Expr::QuotedString {
                content: "bar baz".into(),
                quoting: QuotingStrategy::Single,
            }
        );
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
