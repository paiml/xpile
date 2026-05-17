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
            //
            // PMAT-090: `$((...))` arithmetic expansion is a
            // syntactically distinct form. The current logic peeks
            // PAST the first `(` to see if the second char is also
            // `(`. If so, we capture the entire `$((...))` as a
            // single Bare token (LitStr-passthrough at v0.1.0); the
            // structured `Expr::ArithExpansion { expr }` variant is
            // XPILE-BASHRS-ARITH-EXPANSION-001 future work. The
            // shell at execution time correctly interprets the
            // emitted `$((...))` as arithmetic — same byte-level
            // round-trip preservation as PMAT-085..089.
            '$' if chars.peek() == Some(&'(') => {
                if !current.is_empty() {
                    return Err(FrontendError::Lower(format!(
                        "shell line `{line}` has `$(` adjacent to a bareword \
                         (e.g., `foo$(bar)`); v0.1.0 requires `$(...)` at token boundaries"
                    )));
                }
                chars.next(); // consume the first `(`
                              // PMAT-090: distinguish `$((` (arithmetic) from
                              // `$(` (command substitution). The arithmetic form
                              // is captured verbatim as a Bare token; the
                              // command-substitution form continues into the
                              // existing path.
                if chars.peek() == Some(&'(') {
                    // Arithmetic expansion. Consume the second `(`
                    // and read until matching `))`. We track paren
                    // depth so nested parens inside the arithmetic
                    // expression (e.g., `$(((1 + 2) * 3))`) parse
                    // correctly.
                    chars.next(); // consume the second `(`
                    let mut buf = String::from("$((");
                    let mut depth: usize = 2; // we've opened two `(`
                    for inner in chars.by_ref() {
                        if inner == '(' {
                            depth += 1;
                            buf.push(inner);
                        } else if inner == ')' {
                            depth -= 1;
                            buf.push(inner);
                            if depth == 0 {
                                // Closed the outer `))` — done.
                                break;
                            }
                        } else {
                            buf.push(inner);
                        }
                    }
                    if !buf.ends_with("))") {
                        return Err(FrontendError::Lower(format!(
                            "shell line `{line}` has an unterminated `$((...))` arithmetic expansion"
                        )));
                    }
                    tokens.push(RawToken::Bare(buf));
                    continue;
                }
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
                // PMAT-056: inside double quotes, POSIX recognises
                // backslash as an escape for `$`, `` ` ``, `"`, `\`,
                // and newline. We preserve escapes *verbatim* in the
                // content (don't decode) so the round-trip stays
                // information-lossless — this matters because `$`
                // and `\$` mean different things at shell-execution
                // time (the former triggers variable expansion; the
                // latter is literal). If we decoded escapes here we
                // couldn't distinguish them on the render side.
                //
                // The escape recognition is load-bearing for
                // termination: `\"` must NOT close the string.
                //
                // Single quotes (the arm above) deliberately do NOT
                // handle escapes — POSIX says single quotes are
                // fully literal (no `\'` even allowed).
                let mut content = String::new();
                let mut closed = false;
                while let Some(inner) = chars.next() {
                    if inner == '\\' {
                        // Push the backslash AND the next char
                        // (whatever it is) verbatim. If next is one
                        // of the POSIX double-quote escapes, this
                        // means the inner `"` won't close the
                        // string. Other backslashes are preserved
                        // per POSIX rules.
                        content.push('\\');
                        if let Some(next) = chars.next() {
                            content.push(next);
                        }
                        continue;
                    }
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

/// PMAT-088: detect whether a shell line has an *unambiguous* pipe
/// (single `|`) for `Stmt::Pipeline` parsing, distinguishing it from
/// `||` (short-circuit OR). Returns `true` iff the line contains at
/// least one `|` character that is NOT part of a `||` pair.
///
/// The check is a single linear scan: walk char-by-char, and for
/// each `|` look at the immediate neighbours. A `|` is a real pipe
/// iff neither the previous nor the next char is also `|`. A `|`
/// that's part of `||` is logical-OR, not a pipe.
///
/// Edge case: `cmd1 ||| cmd2` (three pipes in a row) is invalid
/// POSIX; our scan sees the middle `|` flanked by `||` on both
/// sides which fails the unambiguous-pipe check, so the whole line
/// falls through to LitStr-args. That's the right behavior since
/// the input is ill-formed; the shell will reject it at execution
/// time if executed.
///
/// What's deliberately NOT handled (v0.2.0 source fold):
///   * Pipes inside quoted regions (`echo "a | b"` should NOT be
///     a pipeline). Current behavior is a known v0.1.0 limitation
///     called out in the PMAT-041 doc comment above.
fn line_has_unambiguous_pipe(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c != '|' {
            continue;
        }
        let prev_is_pipe = i > 0 && chars[i - 1] == '|';
        let next_is_pipe = i + 1 < chars.len() && chars[i + 1] == '|';
        if !prev_is_pipe && !next_is_pipe {
            return true;
        }
    }
    false
}

/// PMAT-086: splice POSIX backslash-newline line continuations.
/// In POSIX shell, a `\` immediately followed by a newline (with
/// no intervening characters) is removed entirely — both the
/// backslash and the newline disappear, joining the surrounding
/// text into a single logical line. Indentation on the next line
/// is preserved as whitespace within the logical line (the shell
/// then re-tokenizes on whitespace, so leading whitespace is
/// equivalent to a token separator).
///
/// Semantics:
///   `foo \` + `\nbar`   → `foo bar`   (POSIX: `\<newline>` removes both, indent is whitespace)
///   `foo\\` + `\nbar`   → `foo\\` then `bar` on next line (escaped backslash, not continuation)
///   `foo \ ` + `\nbar`  → `foo \ ` then `bar` (backslash followed by space, not newline)
///
/// What this DOESN'T handle (v0.2.0 source fold):
///   * Backslash-newline inside single quotes: POSIX preserves
///     these literally (single quotes don't interpret backslashes).
///     At v0.1.0 our splice runs on the raw source before any
///     quote-aware tokenization, so it incorrectly joins backslash-
///     newline inside single quotes too. Real-world shell scripts
///     rarely put literal backslash-newlines inside single quotes,
///     so the cost is bounded.
///   * Inside heredocs: heredoc bodies are also unaffected by
///     line continuation in POSIX; v0.1.0 has no heredoc support
///     yet (XPILE-BASHRS-HEREDOC-001), so this concern is moot.
fn splice_line_continuations(source: &str) -> String {
    // We walk the source char by char. When we see `\` followed
    // by `\n`, we drop both. When we see `\\` followed by `\n`,
    // we keep the first `\` (it's an escaped backslash) and drop
    // the second `\` + `\n` only if there's a third `\`... actually
    // POSIX is simpler: `\<newline>` always removes both, but
    // `\\<newline>` means literal-backslash followed by newline-
    // ending-the-line. So the rule is: count consecutive
    // backslashes immediately before a newline; if odd, the last
    // one is a continuation marker (drop it + the newline); if
    // even, keep them all and keep the newline.
    let mut out = String::with_capacity(source.len());
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            // Count run of backslashes starting at i.
            let mut run = 0;
            while i + run < chars.len() && chars[i + run] == '\\' {
                run += 1;
            }
            // After the run, do we have a newline?
            let after = i + run;
            if after < chars.len() && chars[after] == '\n' {
                // Odd run → last backslash is a continuation.
                // Emit (run - 1) backslashes, drop the last + newline.
                // Even run → all backslashes are literal pairs;
                // emit them all, keep the newline.
                if run % 2 == 1 {
                    for _ in 0..run - 1 {
                        out.push('\\');
                    }
                    // Skip the trailing backslash + newline.
                    i = after + 1;
                    continue;
                } else {
                    for _ in 0..run {
                        out.push('\\');
                    }
                    out.push('\n');
                    i = after + 1;
                    continue;
                }
            } else {
                // No trailing newline — backslash run is literal.
                for _ in 0..run {
                    out.push('\\');
                }
                i = after;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
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
        // PMAT-055: check for POSIX special parameters first (one
        // char immediately after `$`). They take precedence over
        // identifier matching because `$0` would otherwise fail
        // the leading-digit check in is_posix_identifier.
        if let Some(name) = recognise_shell_special(rest) {
            return Expr::ShellSpecial(name);
        }
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

/// PMAT-055: recognise a POSIX shell special parameter. Returns
/// `Some(name)` for `$1`..`$9`, `$0`, `$@`, `$*`, `$#`, `$?`, `$$`,
/// `$!`, `$-`. The accepted token must be EXACTLY `$<one-char>` —
/// no trailing alphanumerics (those would conflict with shell var
/// names like `$10` which POSIX treats as `${1}0`, requiring braces).
fn recognise_shell_special(rest: &str) -> Option<String> {
    if rest.len() != 1 {
        return None;
    }
    let c = rest.chars().next()?;
    if matches!(c, '0'..='9' | '@' | '*' | '#' | '?' | '$' | '!' | '-') {
        Some(c.to_string())
    } else {
        None
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
        //
        // PMAT-086: POSIX backslash-newline line continuation is
        // handled BEFORE `.lines()` splitting. The splicing happens
        // at the source level, so a multi-line command like
        //     echo \
        //       foo bar
        // is treated as a single logical line `echo foo bar`.
        let spliced = splice_line_continuations(source);
        let mut stmts: Vec<Stmt> = Vec::new();
        for raw_line in spliced.lines() {
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
            //
            // PMAT-088: distinguish single `|` (pipe) from `||`
            // (short-circuit OR). A line that contains `||` but NOT
            // a single `|` falls through to `Stmt::Cmd` so the
            // `||` tokens survive as ordinary `LitStr` args. The
            // shell at execution time re-interprets `||` as a
            // short-circuit operator. The control-structure-faithful
            // representation (`Stmt::ShortCircuit { lhs, op, rhs }`)
            // is XPILE-BASHRS-LOGICAL-OPS-001 future work.
            if line_has_unambiguous_pipe(line) {
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
    fn lower_token_recognises_special_params() {
        // PMAT-055 (replacing the older PMAT-045 negative): the
        // POSIX special parameters now produce `Expr::ShellSpecial`,
        // not `Expr::LitStr`. Each carries the one-char name
        // without the leading `$`.
        use xpile_meta_hir::Expr;
        for (tok, expected_name) in &[
            ("$1", "1"),
            ("$9", "9"),
            ("$0", "0"),
            ("$@", "@"),
            ("$*", "*"),
            ("$#", "#"),
            ("$?", "?"),
            ("$$", "$"),
            ("$!", "!"),
            ("$-", "-"),
        ] {
            assert_eq!(
                lower_token(tok),
                Expr::ShellSpecial(expected_name.to_string()),
                "expected `{tok}` to lower to ShellSpecial(`{expected_name}`)"
            );
        }
    }

    #[test]
    fn lower_token_two_char_after_dollar_falls_through() {
        // PMAT-055: `$10` is POSIX `${1}0` (the digit `1` is the
        // special, `0` is a literal char) — needs braces to mean
        // positional param 10. Without braces, we keep the prior
        // PMAT-045 behaviour: fall through as LitStr.
        use xpile_meta_hir::Expr;
        assert_eq!(lower_token("$10"), Expr::LitStr("$10".to_string()));
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
    fn lower_token_param_expansion_falls_through_as_litstr() {
        // PMAT-085: POSIX parameter expansion forms (`${VAR:-default}`,
        // `${VAR-default}`, `${VAR:=value}`, `${VAR:?error}`,
        // `${VAR:+alt}`, `${#VAR}`, `${VAR#prefix}`, `${VAR%suffix}`)
        // are preserved verbatim as `Expr::LitStr` at v0.1.0 — the
        // structured `Expr::ParamExpansion { var, op, fallback }`
        // variant is XPILE-BASHRS-PARAM-EXPANSION-001 (v0.2.0+ work).
        //
        // This test locks in the round-trip property: parsing →
        // lowering → backend rendering produces byte-identical output
        // because LitStr arms in render_arg just pass the bytes
        // through unchanged. The substrate quality regime is preserved
        // even at the Bronze-tier "opaque LitStr" representation —
        // information loss is zero on the round trip.
        //
        // Why this matters: real shell idioms like
        // `: "${PORT:=8080}"` (POSIX idempotent default-port pattern)
        // would otherwise either fail or silently mangle. With this
        // test in place, the LitStr passthrough is a documented
        // invariant rather than an emergent behavior.
        use xpile_meta_hir::Expr;
        let param_expansions = &[
            "${VAR:-default}", // use default if unset OR empty
            "${VAR-default}",  // use default if unset (preserves empty)
            "${VAR:=8080}",    // use AND assign default
            "${VAR:?error}",   // error if unset
            "${VAR:+alt}",     // use alt if SET (inverse default)
            "${#VAR}",         // string length
            "${VAR#prefix}",   // strip shortest prefix
            "${VAR##prefix*}", // strip longest prefix
            "${VAR%suffix}",   // strip shortest suffix
            "${VAR%%*suffix}", // strip longest suffix
            "${VAR/old/new}",  // POSIX-ish substitution (bash ext)
            "${VAR:0:3}",      // substring (bash ext)
        ];
        for tok in param_expansions {
            assert_eq!(
                lower_token(tok),
                Expr::LitStr(tok.to_string()),
                "expected param-expansion `{tok}` to round-trip as LitStr at v0.1.0"
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
    fn tokenize_line_double_quote_escapes_do_not_terminate_string() {
        // PMAT-056 load-bearing: `\"` inside double quotes must NOT
        // close the string. The escape is preserved verbatim in the
        // content so the render round-trip emits a valid shell line.
        let toks = tokenize_line("echo \"she said \\\"hi\\\"\"").expect("parse");
        assert_eq!(toks.len(), 2, "expected 2 tokens: {toks:?}");
        match &toks[1] {
            RawToken::DoubleQuoted(s) => {
                assert_eq!(s, "she said \\\"hi\\\"");
            }
            other => panic!("expected DoubleQuoted; got {other:?}"),
        }
    }

    #[test]
    fn tokenize_line_double_quote_preserves_var_expansion() {
        // PMAT-056 regression guard: `"Hi, $NAME"` content stays
        // `Hi, $NAME` (unescaped) so the rendered shell still
        // triggers variable expansion at runtime. If the tokenizer
        // accidentally decoded all escapes, this would break.
        let toks = tokenize_line("echo \"Hi, $NAME\"").expect("parse");
        match &toks[1] {
            RawToken::DoubleQuoted(s) => assert_eq!(s, "Hi, $NAME"),
            other => panic!("expected DoubleQuoted; got {other:?}"),
        }
    }

    #[test]
    fn tokenize_line_double_quote_preserves_escaped_dollar() {
        // PMAT-056: `"\$NAME"` keeps the `\$` form (literal `$NAME`
        // at runtime, no expansion).
        let toks = tokenize_line("echo \"\\$NAME\"").expect("parse");
        match &toks[1] {
            RawToken::DoubleQuoted(s) => assert_eq!(s, "\\$NAME"),
            other => panic!("expected DoubleQuoted; got {other:?}"),
        }
    }

    #[test]
    fn tokenize_line_double_quote_preserves_escaped_backslash() {
        // PMAT-056: `"\\"` content is `\\` (two chars) which renders
        // back as `"\\"` and shell interprets as one `\`.
        let toks = tokenize_line("echo \"a\\\\b\"").expect("parse");
        match &toks[1] {
            RawToken::DoubleQuoted(s) => assert_eq!(s, "a\\\\b"),
            other => panic!("expected DoubleQuoted; got {other:?}"),
        }
    }

    #[test]
    fn tokenize_line_single_quote_does_not_interpret_escapes() {
        // PMAT-056: POSIX says single quotes are fully literal.
        // `'a\b'` content is literally `a\b`. This is the existing
        // single-quote behaviour unchanged — locking it in as a
        // regression guard.
        let toks = tokenize_line("echo 'a\\b\\\"c'").expect("parse");
        match &toks[1] {
            RawToken::SingleQuoted(s) => assert_eq!(s, "a\\b\\\"c"),
            other => panic!("expected SingleQuoted; got {other:?}"),
        }
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
            vec![RawToken::Bare("echo".into()), RawToken::Bare("a#b".into()),],
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
    fn splice_line_continuations_handles_pmat_086_cases() {
        // PMAT-086: backslash-newline splicing semantics.
        //
        // Simple continuation: `\<newline>` removes both, joining
        // the two physical lines into one logical line. Indentation
        // on the next line becomes whitespace within the joined
        // logical line (the bashrs tokenizer's whitespace-aware
        // splitting then re-separates the tokens).
        assert_eq!(splice_line_continuations("foo \\\nbar"), "foo bar");
        assert_eq!(splice_line_continuations("foo \\\n  bar"), "foo   bar");
        assert_eq!(
            splice_line_continuations("echo \\\n  one \\\n  two\n"),
            "echo   one   two\n"
        );

        // Even run of backslashes before newline = all literal, no
        // continuation. `\\<newline>` = literal-backslash followed
        // by newline-ending-the-line.
        assert_eq!(
            splice_line_continuations("printf foo\\\\\n"),
            "printf foo\\\\\n"
        );

        // Odd run > 1: `\\\<newline>` = literal-backslash + line
        // continuation. The first backslash is kept (it's an
        // escaped-backslash literal); the second is the continuation
        // marker (dropped along with the newline).
        assert_eq!(splice_line_continuations("foo\\\\\\\nbar"), "foo\\\\bar");

        // No newline after backslash = backslash is literal.
        assert_eq!(splice_line_continuations("foo \\ bar"), "foo \\ bar");
        assert_eq!(splice_line_continuations("trailing\\"), "trailing\\");

        // No backslash = pass-through.
        assert_eq!(splice_line_continuations(""), "");
        assert_eq!(splice_line_continuations("plain text\n"), "plain text\n");
        assert_eq!(
            splice_line_continuations("multi\nline\nplain"),
            "multi\nline\nplain"
        );
    }

    #[test]
    fn parse_and_lower_handles_pmat_086_line_continuation() {
        // PMAT-086: a multi-line shell command using `\<newline>`
        // splicing is parsed as a single Stmt::Cmd. Real shell
        // scripts use this heavily for long `configure` /
        // `cmake` / `apt-get install` invocations.
        use xpile_meta_hir::{Expr, Item, Stmt};
        let source = "echo \\\n  hello \\\n  world\n";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/cont.sh"), source)
            .expect("parse");
        let Item::Function(f) = &module.items[0];
        assert_eq!(
            f.body.stmts.len(),
            1,
            "expected one Stmt::Cmd after splicing; got {:?}",
            f.body.stmts
        );
        let Stmt::Cmd { program, args } = &f.body.stmts[0] else {
            panic!("expected Stmt::Cmd; got {:?}", f.body.stmts[0]);
        };
        assert_eq!(program, "echo");
        assert_eq!(
            args,
            &vec![Expr::LitStr("hello".into()), Expr::LitStr("world".into()),]
        );
    }

    #[test]
    fn line_has_unambiguous_pipe_distinguishes_pipe_from_or() {
        // PMAT-088: helper that lets parse_and_lower distinguish a
        // real pipeline (`cmd1 | cmd2`) from a short-circuit OR
        // (`cmd1 || cmd2`).
        assert!(line_has_unambiguous_pipe("cat foo | grep bar"));
        assert!(line_has_unambiguous_pipe("a | b | c"));
        assert!(!line_has_unambiguous_pipe("ls || exit 1"));
        assert!(!line_has_unambiguous_pipe("true || false"));
        assert!(!line_has_unambiguous_pipe("a || b || c"));
        // Edge: ill-formed `|||` — no unambiguous single pipe.
        assert!(!line_has_unambiguous_pipe("cmd1 ||| cmd2"));
        // Mixed: contains both `||` AND `|`. The `|` is still
        // unambiguous so we report true (the line is a pipeline
        // *and* has logical-OR; v0.1.0 pipeline parser will then
        // try to split it, which is an acceptable best-effort
        // outcome).
        assert!(line_has_unambiguous_pipe("a | b || c"));
        // No pipes at all.
        assert!(!line_has_unambiguous_pipe("echo hi"));
        assert!(!line_has_unambiguous_pipe(""));
    }

    #[test]
    fn tokenize_line_recognises_arith_expansion_as_bare() {
        // PMAT-090: `$((...))` arithmetic expansion is tokenized
        // as a single Bare token (LitStr-passthrough at v0.1.0).
        // The tokenizer distinguishes `$((` from `$(` by peeking
        // past the first `(`.
        assert_eq!(
            tokenize_line("$((1 + 2))").unwrap(),
            vec![RawToken::Bare("$((1 + 2))".to_string())]
        );
        // Nested parens inside the arithmetic expression are
        // preserved verbatim — paren depth tracking lets
        // `$(((1 + 2) * 3))` parse correctly.
        assert_eq!(
            tokenize_line("$(((1 + 2) * 3))").unwrap(),
            vec![RawToken::Bare("$(((1 + 2) * 3))".to_string())]
        );
        // Mixed with other tokens.
        assert_eq!(
            tokenize_line("echo $((x + 1))").unwrap(),
            vec![
                RawToken::Bare("echo".to_string()),
                RawToken::Bare("$((x + 1))".to_string())
            ]
        );
        // Command substitution `$(date)` still works (regression
        // guard: the `$((` peek must not accidentally consume the
        // single-paren path).
        assert_eq!(
            tokenize_line("$(date)").unwrap(),
            vec![RawToken::CommandSubst("date".to_string())]
        );
    }

    #[test]
    fn parse_and_lower_subshell_round_trips_via_litstr() {
        // PMAT-091: POSIX subshell `(cmd)` round-trip via LitStr
        // passthrough. The parentheses tokenize as standalone Bare
        // tokens (since they're whitespace-separated from the
        // inner command) and lower as LitStr. The result is
        // Stmt::Cmd with program="(" and the inner command +
        // closing `)` as args. The downstream shell at execution
        // time correctly creates a subshell, runs the inner
        // command, and returns to the parent shell.
        //
        // Why this matters: subshells are POSIX-standard for
        // isolating side effects (cd, umask, exports) — the
        // pattern `(cd /tmp && do_stuff)` is common in build
        // scripts and CI pipelines.
        //
        // Distinct from:
        // - PMAT-050 `$(cmd)` command substitution (captures
        //   stdout as a value)
        // - PMAT-090 `$((expr))` arithmetic expansion (evaluates
        //   expr arithmetically)
        // - Bash `((expr))` arithmetic command (NOT covered —
        //   bash extension, not POSIX)
        //
        // Structured representation (`Stmt::Subshell { body }`) is
        // XPILE-BASHRS-SUBSHELL-001 future work. Same v0.1.0
        // invariant pattern as PMAT-085..090.
        use xpile_meta_hir::{Expr, Item, Stmt};
        let cases: &[(&str, &str, &[&str])] = &[
            ("( cd /tmp )\n", "(", &["cd", "/tmp", ")"]),
            ("( cd /tmp && ls )\n", "(", &["cd", "/tmp", "&&", "ls", ")"]),
            ("( exit 1 )\n", "(", &["exit", "1", ")"]),
        ];
        for (source, expected_program, expected_args) in cases {
            let module = BashrsFrontend
                .parse_and_lower(&PathBuf::from("/tmp/sub.sh"), source)
                .unwrap_or_else(|e| panic!("parse failed for `{source}`: {e:?}"));
            let Item::Function(f) = &module.items[0];
            let Stmt::Cmd { program, args } = &f.body.stmts[0] else {
                panic!(
                    "expected Stmt::Cmd for `{source}`; got {:?}",
                    f.body.stmts[0]
                );
            };
            assert_eq!(program, expected_program);
            let expected_exprs: Vec<Expr> = expected_args
                .iter()
                .map(|s| Expr::LitStr((*s).to_string()))
                .collect();
            assert_eq!(
                args, &expected_exprs,
                "subshell round-trip for `{source}` failed"
            );
        }
    }

    #[test]
    fn parse_and_lower_arith_expansion_round_trips_via_litstr() {
        // PMAT-090: end-to-end — `$((...))` arithmetic expansion
        // round-trips through the bashrs pipeline as an
        // `Expr::LitStr` arg. The downstream shell at execution
        // time re-interprets `$((...))` as arithmetic and
        // substitutes the numeric result.
        //
        // The structured representation
        // (`Expr::ArithExpansion { expr }`) is
        // XPILE-BASHRS-ARITH-EXPANSION-001 future work. At v0.1.0
        // the LitStr passthrough preserves shell semantics
        // through the byte-level round-trip. Same v0.1.0 invariant
        // pattern as PMAT-085/086/087/088/089.
        use xpile_meta_hir::{Expr, Item, Stmt};
        let cases: &[(&str, &str, &[&str])] = &[
            ("echo $((1 + 2))\n", "echo", &["$((1 + 2))"]),
            ("echo $((x + 1))\n", "echo", &["$((x + 1))"]),
            ("echo $(((1 + 2) * 3))\n", "echo", &["$(((1 + 2) * 3))"]),
            (
                "result=$((x * y))\n",
                // shell assignment: program="result=$((x * y))"
                // parsed via Stmt::ShellAssign path, not Stmt::Cmd
                "",
                &[],
            ),
        ];
        for (source, expected_program, expected_args) in &cases[..3] {
            let module = BashrsFrontend
                .parse_and_lower(&PathBuf::from("/tmp/arith.sh"), source)
                .unwrap_or_else(|e| panic!("parse failed for `{source}`: {e:?}"));
            let Item::Function(f) = &module.items[0];
            let Stmt::Cmd { program, args } = &f.body.stmts[0] else {
                panic!(
                    "expected Stmt::Cmd for `{source}`; got {:?}",
                    f.body.stmts[0]
                );
            };
            assert_eq!(program, expected_program);
            let expected_exprs: Vec<Expr> = expected_args
                .iter()
                .map(|s| Expr::LitStr((*s).to_string()))
                .collect();
            assert_eq!(
                args, &expected_exprs,
                "arith-expansion round-trip for `{source}` failed"
            );
        }
        // ShellAssign-shape case: result=$((x * y))
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/a.sh"), "result=$((x * y))\n")
            .expect("parse");
        let Item::Function(f) = &module.items[0];
        let Stmt::ShellAssign { name, value } = &f.body.stmts[0] else {
            panic!(
                "expected Stmt::ShellAssign for `result=...`; got {:?}",
                f.body.stmts[0]
            );
        };
        assert_eq!(name, "result");
        assert_eq!(value, &Expr::LitStr("$((x * y))".to_string()));
    }

    #[test]
    fn parse_and_lower_test_bracket_round_trips_via_litstr() {
        // PMAT-089: POSIX test brackets `[ ... ]` (the `test`
        // command synonym) round-trip via LitStr passthrough.
        // POSIX `[` is literally an executable — `/usr/bin/[` on
        // most systems — so it lowers cleanly to Stmt::Cmd with
        // `program: "["` and the test arguments as LitStr args
        // including the closing `]`.
        //
        // Real shell scripts use `[ ... ]` heavily for file
        // tests, string comparisons, and numeric checks. With
        // this round-trip locked in, those scripts pass through
        // bashrs without semantic loss even though the IR
        // doesn't model the test predicate structurally.
        //
        // Bash's `[[ ... ]]` is intentionally NOT covered here —
        // it's a bash extension (not POSIX), and the conservative
        // v0.1.0 stance is to fall through as args. Structured
        // representation (`Stmt::TestPredicate { negated, args }`)
        // is XPILE-BASHRS-TEST-PREDICATE-001 future work.
        //
        // Same v0.1.0 invariant pattern as PMAT-085/086/087/088.
        use xpile_meta_hir::{Expr, Item, Stmt};
        let cases: &[(&str, &str, &[&str])] = &[
            ("[ -f foo ]\n", "[", &["-f", "foo", "]"]),
            ("[ -d /tmp ]\n", "[", &["-d", "/tmp", "]"]),
            ("[ \"$x\" = abc ]\n", "[", &["\"$x\"", "=", "abc", "]"]),
            ("[ -z \"$VAR\" ]\n", "[", &["-z", "\"$VAR\"", "]"]),
            ("[ $count -gt 0 ]\n", "[", &["$count", "-gt", "0", "]"]),
            ("[ ! -e missing ]\n", "[", &["!", "-e", "missing", "]"]),
        ];
        for (source, expected_program, expected_args) in cases {
            let module = BashrsFrontend
                .parse_and_lower(&PathBuf::from("/tmp/t.sh"), source)
                .unwrap_or_else(|e| panic!("parse failed for `{source}`: {e:?}"));
            let Item::Function(f) = &module.items[0];
            let Stmt::Cmd { program, args } = &f.body.stmts[0] else {
                panic!(
                    "expected Stmt::Cmd for `{source}`; got {:?}",
                    f.body.stmts[0]
                );
            };
            assert_eq!(program, expected_program);
            // Args come back as a mix of LitStr / QuotedString /
            // ShellVar depending on the token shape. We assemble
            // the expected shape by re-tokenizing each expected
            // arg through `lower_token` so the test stays robust
            // to legitimate IR refinements (e.g., `$x` correctly
            // recognized as ShellVar).
            let expected_exprs: Vec<Expr> = expected_args
                .iter()
                .map(|s| {
                    // Strip surrounding double-quotes for tokens
                    // like `"$x"` — those parse as
                    // Expr::QuotedString. The tokenizer handles
                    // this via tokenize_line, not lower_token, so
                    // we go via the full parse path's expected
                    // shape: a double-quoted token containing a
                    // ShellVar-eligible name lowers to
                    // QuotedString { content: "$x",
                    // quoting: Double }.
                    if let Some(inner) = s.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
                        Expr::QuotedString {
                            content: inner.to_string(),
                            quoting: xpile_meta_hir::QuotingStrategy::Double,
                        }
                    } else if s.starts_with('$') && is_posix_identifier(&s[1..]) {
                        Expr::ShellVar(s[1..].to_string())
                    } else {
                        Expr::LitStr((*s).to_string())
                    }
                })
                .collect();
            assert_eq!(
                args, &expected_exprs,
                "test-bracket round-trip for `{source}` failed"
            );
        }
    }

    #[test]
    fn parse_and_lower_and_or_short_circuit_round_trips_via_litstr() {
        // PMAT-088: POSIX `&&` and `||` short-circuit operators
        // round-trip end-to-end via LitStr passthrough at v0.1.0.
        // Like redirections (PMAT-087), the tokens land as
        // ordinary `Expr::LitStr` args; the downstream shell
        // re-interprets the control flow at execution time.
        //
        // For a real shell line like `make && make install`,
        // bashrs-frontend's whitespace tokenizer splits it into
        // four tokens: ["make", "&&", "make", "install"], so it
        // lowers to Stmt::Cmd { program: "make", args:
        // [LitStr("&&"), LitStr("make"), LitStr("install")] }.
        // When bashrs-backend renders this back to shell, the
        // emitted line is `make && make install` again, and the
        // shell at execution time correctly interprets `&&` as
        // a short-circuit operator splitting two commands.
        //
        // The IR doesn't model the boolean control structure
        // (that's XPILE-BASHRS-LOGICAL-OPS-001 future work),
        // but the byte-level round-trip preserves shell
        // semantics. This is the same v0.1.0 invariant pattern
        // as PMAT-085 (param expansion), PMAT-086 (line
        // continuation), and PMAT-087 (redirection).
        use xpile_meta_hir::{Expr, Item, Stmt};
        let cases: &[(&str, &str, &[&str])] = &[
            ("make && make install\n", "make", &["&&", "make", "install"]),
            ("ls || exit 1\n", "ls", &["||", "exit", "1"]),
            (
                "test -f foo && echo exists || echo missing\n",
                "test",
                &["-f", "foo", "&&", "echo", "exists", "||", "echo", "missing"],
            ),
            ("true && false\n", "true", &["&&", "false"]),
        ];
        for (source, expected_program, expected_args) in cases {
            let module = BashrsFrontend
                .parse_and_lower(&PathBuf::from("/tmp/ao.sh"), source)
                .expect("parse");
            let Item::Function(f) = &module.items[0];
            let Stmt::Cmd { program, args } = &f.body.stmts[0] else {
                panic!(
                    "expected Stmt::Cmd for `{source}`; got {:?}",
                    f.body.stmts[0]
                );
            };
            assert_eq!(program, expected_program);
            let expected_exprs: Vec<Expr> = expected_args
                .iter()
                .map(|s| Expr::LitStr((*s).to_string()))
                .collect();
            assert_eq!(
                args, &expected_exprs,
                "short-circuit operator preservation for `{source}` failed"
            );
        }
    }

    #[test]
    fn parse_and_lower_redirection_round_trips_via_litstr_args() {
        // PMAT-087: POSIX redirection tokens (`>`, `>>`, `<`, `2>`,
        // `2>>`, `2>&1`, `&>`) round-trip through the v0.1.0
        // bashrs pipeline by virtue of LitStr passthrough — the
        // tokens land as ordinary `Expr::LitStr` args, and the
        // bashrs-backend emits them verbatim. The downstream shell
        // re-parses the redirection at execution time, so semantics
        // are preserved end-to-end *even though* the bashrs IR
        // doesn't model redirection structurally at v0.1.0.
        //
        // Why this matters: real shell scripts use redirections
        // pervasively (`> /dev/null 2>&1` is in basically every
        // script). The IR-faithful structured representation
        // (`Stmt::CmdWithRedirections { command, redirections:
        // Vec<Redirect> }`) is XPILE-BASHRS-REDIRECT-001 future
        // work; at v0.1.0 we lock in that the LitStr passthrough
        // preserves shell semantics through the byte-level
        // round-trip.
        //
        // Pairs with PMAT-085 (param-expansion LitStr passthrough)
        // and PMAT-086 (line-continuation splicing) — together they
        // establish the v0.1.0 "best-effort round-trip" invariant
        // for shell idioms that don't yet have structured IR support.
        use xpile_meta_hir::{Expr, Item, Stmt};
        let cases: &[(&str, &str, &[&str])] = &[
            ("echo hi > foo.txt\n", "echo", &["hi", ">", "foo.txt"]),
            ("echo hi >> log\n", "echo", &["hi", ">>", "log"]),
            ("cat < input.txt\n", "cat", &["<", "input.txt"]),
            ("make 2> errors.log\n", "make", &["2>", "errors.log"]),
            ("make 2>> errors.log\n", "make", &["2>>", "errors.log"]),
            (
                "command > /dev/null 2>&1\n",
                "command",
                &[">", "/dev/null", "2>&1"],
            ),
        ];
        for (source, expected_program, expected_args) in cases {
            let module = BashrsFrontend
                .parse_and_lower(&PathBuf::from("/tmp/r.sh"), source)
                .expect("parse");
            let Item::Function(f) = &module.items[0];
            let Stmt::Cmd { program, args } = &f.body.stmts[0] else {
                panic!(
                    "expected Stmt::Cmd for `{source}`; got {:?}",
                    f.body.stmts[0]
                );
            };
            assert_eq!(program, expected_program);
            let expected_exprs: Vec<Expr> = expected_args
                .iter()
                .map(|s| Expr::LitStr((*s).to_string()))
                .collect();
            assert_eq!(
                args, &expected_exprs,
                "redirection token preservation for `{source}` failed"
            );
        }
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

    #[test]
    fn parse_and_lower_composes_all_pmat_085_to_091_idioms() {
        // PMAT-092: capstone — a single shell script exercising
        // every round-trip invariant locked in across PMAT-085
        // through PMAT-091:
        //
        //   * PMAT-085 — `${VAR:-default}` parameter expansion
        //   * PMAT-086 — `\<newline>` line continuation
        //   * PMAT-087 — `>` / `2>&1` redirection
        //   * PMAT-088 — `||` short-circuit (no longer mis-parsed
        //                as `| |`)
        //   * PMAT-089 — `[ -f foo ]` test bracket
        //   * PMAT-090 — `$((x + 1))` arithmetic expansion (no
        //                longer rejected as "nested `$(...)`")
        //   * PMAT-091 — `(cd /tmp && do_stuff)` subshell
        //
        // Why this test exists: each PMAT-085..091 ships its own
        // narrow test, but real shell scripts compose these
        // idioms — and historically composition exposes bugs
        // that narrow tests miss. This composite test parses a
        // 7-line shell input through bashrs-frontend without
        // erroring, exercising every fix shipped in the v0.1.0
        // round-trip lock-in run.
        //
        // Specifically guards against: a future refactor that
        // regresses any one of PMAT-085..091 without tripping
        // its own narrow test (e.g., by introducing a different
        // failure mode that happens to satisfy the narrow
        // assertions). If this composite test breaks, the
        // refactor needs to be re-examined.
        use xpile_meta_hir::Item;
        let source = "\
PORT=${PORT:-8080}\n\
echo starting on port $PORT \\\n  with config /etc/foo\n\
make > build.log 2>&1\n\
test -f /tmp/lock || echo no_lock\n\
[ -d /tmp ] && echo tmp_ok\n\
N=$((counter + 1))\n\
( cd /tmp && ls )\n\
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/composite.sh"), source)
            .expect("composite PMAT-085..091 script must parse");
        let Item::Function(f) = &module.items[0];
        // We expect exactly 7 statements — one per source line.
        // The line-continuation splice (PMAT-086) collapses two
        // physical lines into one logical line, leaving 7 total.
        assert_eq!(
            f.body.stmts.len(),
            7,
            "expected 7 statements after PMAT-086 line-continuation \
             splice; got {} — composite parsing regressed",
            f.body.stmts.len()
        );
    }
}
