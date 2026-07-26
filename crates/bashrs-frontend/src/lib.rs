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
    Block, CaseArm, Expr, Function, Item, LoopKind, Module, QuotingStrategy, SourceLang, Stmt, Type,
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

/// PMAT-989: POSIX shell control-flow keywords. A line that opens,
/// closes, or chains a compound command — loops (`for`/`while`/
/// `until`/`do`/`done`), conditionals (`if`/`then`/`elif`/`else`/
/// `fi`), and `case` (`case`/`esac`) — is NOT part of the
/// flat-command subset this frontend supports.
///
/// Historically the hand-rolled parser silently SHREDDED these:
/// `for i in 1 2 3; do echo $i; done` became four independent
/// bareword `Stmt::Cmd`s (`for`, `do`, `echo`, `done`) with the
/// loop structure destroyed and no diagnostic. That is a
/// correctness hazard — the lowered IR claims to be the script but
/// has none of its control flow. We now REFUSE such input with a
/// hard `FrontendError` rather than mislower it.
///
/// `in` is intentionally NOT in this set as a standalone reserved
/// word: it is only a keyword inside a `for`/`case` header, both of
/// which are already caught by the leading-keyword check. Treating
/// a bare `in` line as control-flow would over-reject (e.g. a
/// program literally named `in`), and a real `for … in …` header
/// is caught by its `for` prefix.
const CONTROL_FLOW_KEYWORDS: &[&str] = &[
    "for", "while", "until", "do", "done", "if", "then", "elif", "else", "fi", "case", "esac",
];

/// PMAT-989: does this logical line participate in shell
/// control-flow? Returns the offending keyword if so.
///
/// Detects both shapes that the old parser shredded:
///   * Multi-line bodies, where each keyword sits alone on its own
///     line (`for …` / `do` / `echo $i` / `done`): the standalone
///     `do` / `done` / `then` / `fi` / `else` / `esac` lines and
///     the leading `for` / `while` / `until` / `if` / `elif` /
///     `case` are caught.
///   * Single-line compound commands
///     (`for i in 1 2 3; do echo $i; done`): the `; do`, `; done`,
///     `; then`, `; fi`, etc. segments are caught by scanning the
///     `;`/`&&`/`||`-and-whitespace-delimited words for a keyword.
///
/// The check is deliberately conservative about false positives:
/// it only treats a keyword as control-flow when it appears as a
/// whole word at a command position (start of the line, or right
/// after a `;` / `&` / `|` separator). A keyword used as a mere
/// argument (`grep -w done file`, `echo if`) is left alone.
fn control_flow_keyword(line: &str) -> Option<&'static str> {
    // Split the line into command-position segments. A command
    // position is the start of the line and anything immediately
    // following a `;`, `&`, or `|` separator. The first whole word
    // of any such segment being a control-flow keyword means the
    // line participates in a compound command.
    for segment in line.split([';', '&', '|']) {
        let first_word = segment.split_whitespace().next().unwrap_or("");
        if let Some(kw) = CONTROL_FLOW_KEYWORDS.iter().find(|kw| **kw == first_word) {
            return Some(kw);
        }
    }
    None
}

/// The first whitespace-delimited word of a (already-trimmed) line,
/// or `""` if the line is blank. Used for command-position keyword
/// checks (`for` / `do` / `done`).
fn first_word(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}

/// PMAT-1268: lower a single FLAT (non-control-flow) shell line to a
/// `Stmt`. Extracted verbatim from `parse_and_lower`'s inline body so
/// that `for`-loop bodies and the top-level walk share one lowering
/// path (the same DRY move PMAT-974 made on the backend's
/// `render_stmt_lines`).
///
/// Preconditions the caller guarantees: `line` is already trimmed,
/// non-empty, not a comment/shebang, and NOT shell control-flow
/// (`control_flow_keyword(line)` is `None`). Returns `Ok(None)` only
/// for a line that tokenizes to zero tokens (defensive; the caller's
/// filtering makes this unreachable in practice).
///
/// PMAT-1371: this is the ONE chokepoint for flat-line refusals. Both
/// call sites route through it — `parse_segment_seq` for loop / `if` /
/// `case`-arm bodies and the top-level walk in `parse_and_lower` — so a
/// guard here covers nested and top-level occurrences alike. A here-doc
/// operator always appears on the COMMAND line, which reaches this
/// function before any of the here-doc's body lines are processed, so
/// refusing here happens before the body can be mis-read as commands.
fn lower_flat_line(line: &str) -> Result<Option<Stmt>, FrontendError> {
    // PMAT-1371: REFUSE here-documents. There is NO here-doc handling in
    // this frontend at all: `parse_and_lower` trims every source line and
    // drops every blank one globally, so a here-doc BODY is re-tokenized
    // as ordinary commands and space-joined by the backend. That produced
    // the worst failure shape in the repo — exit 0, `bash -n` on the
    // output CLEAN, and a semantically WRONG script: `cat <<EOF` over
    // "  keep  me" / "" / "after blank" emitted a here-doc whose body had
    // its leading and internal whitespace collapsed and its blank line
    // deleted. Nothing downstream catches that, and here-docs are how
    // shell emits config files, SQL, YAML and usage text. Inside an
    // indented block the backend's `indent_body` additionally tab-prefixed
    // the terminator, yielding "here-document delimited by end-of-file".
    // Fixing the global trim is not an option — it ripples through every
    // parser path. So: refuse, and leave here-docs to v0.2.0.
    //
    // Detection is on TOKENS, never the raw line: `echo "a << b"`
    // round-trips CORRECTLY today, and a `line.contains("<<")` guard would
    // regress it. Only an unquoted (`Bare`) token opening with `<<` is a
    // redirection operator — this also covers `<<-` and the `<<` / `EOF`
    // space-separated spelling.
    //
    // A tokenizer ERROR is deliberately NOT treated as a here-doc: the
    // v0.1.0 tokenizer rejects shapes that the assignment branch below
    // handles from `value_part` alone (`NAME="Noah Gift"` trips the
    // adjacent-quote rule), so failing open here preserves those working
    // paths and lets the existing code report its own, more specific error.
    if let Ok(tokens) = tokenize_line(line) {
        if let Some(op) = tokens.iter().find_map(|t| match t {
            RawToken::Bare(s) if s.starts_with("<<") => Some(s.clone()),
            _ => None,
        }) {
            return Err(FrontendError::Parse(format!(
                "bashrs-frontend: here-document redirection `{op}` is not modelled (v0.2.0); \
                 refusing rather than silently rewriting the here-doc body. The frontend trims \
                 every line and drops blank lines, so a here-doc body would round-trip with its \
                 whitespace collapsed — a SILENT semantic change in a script that still passes \
                 `bash -n`. Offending line `{line}`."
            )));
        }
    }

    // PMAT-1371: a bare `&` in command position. Emitted verbatim before
    // this refusal, giving `bash -n` "syntax error near unexpected token
    // `&'". This is defence-in-depth for the `;&` shred path above and
    // independently covers a stray `&` on its own line. It must stay
    // narrow: a TRAILING `&` is POSIX background-execution and round-trips
    // correctly today (`sleep 0 &`), so only `&` in COMMAND position —
    // where no command word precedes it — refuses.
    if first_word(line) == "&" {
        return Err(FrontendError::Parse(format!(
            "bashrs-frontend: `&` in command position is not a command; refusing rather than \
             emitting it verbatim (the emitted script would fail `bash -n` with \
             \"syntax error near unexpected token `&'\"). Offending line `{line}`."
        )));
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
                    return Ok(Some(Stmt::ShellAssign {
                        name: name_part.to_string(),
                        value: Expr::LitStr(String::new()),
                    }));
                }
                1 => {
                    let value_expr = lower_raw_token(&value_tokens[0])?;
                    return Ok(Some(Stmt::ShellAssign {
                        name: name_part.to_string(),
                        value: value_expr,
                    }));
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
            let args: Vec<Expr> = iter.map(lower_raw_token).collect::<Result<Vec<_>, _>>()?;
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
        return Ok(Some(Stmt::Pipeline { stages }));
    }

    // PMAT-049: quoting-aware tokenizer. Same logic as the
    // pipeline-stage version above.
    let raw_tokens = tokenize_line(line)?;
    let mut iter = raw_tokens.iter();
    let Some(first) = iter.next() else {
        return Ok(None);
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
    Ok(Some(Stmt::Cmd { program, args }))
}

/// PMAT-1268: parse a `for VAR in ITEMS` header segment into its
/// loop variable and item `Expr`s. `header` is the command-position
/// segment BEFORE the `do` keyword (structural `;`/newline splitting
/// already stripped any `; do …` tail). Refuses — never shreds —
/// anything outside the slice-1 subset.
fn parse_for_header(header: &str) -> Result<(String, Vec<Expr>), FrontendError> {
    let tokens = tokenize_line(header)?;
    let mut it = tokens.iter();
    // tokens[0] must be the bareword `for` (the caller only reaches
    // here for a `for`-first line, but re-check defensively).
    match it.next() {
        Some(RawToken::Bare(w)) if w == "for" => {}
        _ => {
            return Err(FrontendError::Parse(format!(
                "bashrs-frontend: malformed `for` header `{header}` (expected `for`)"
            )));
        }
    }
    let var = match it.next() {
        Some(RawToken::Bare(v)) if is_posix_identifier(v) => v.clone(),
        other => {
            return Err(FrontendError::Parse(format!(
                "bashrs-frontend: `for` loop variable must be a POSIX identifier, \
                 got {other:?} in header `{header}`"
            )));
        }
    };
    match it.next() {
        Some(RawToken::Bare(w)) if w == "in" => {}
        None => {
            // `for x; do …` iterates the positional parameters
            // (`"$@"`). That's a distinct semantics we don't model
            // at slice 1 — refuse rather than guess an item list.
            return Err(FrontendError::Parse(format!(
                "bashrs-frontend: `for {var}` without an explicit `in <items>` iterates \
                 the positional parameters (`$@`) — unsupported at this slice; \
                 use `for {var} in <items>; do … done`"
            )));
        }
        other => {
            return Err(FrontendError::Parse(format!(
                "bashrs-frontend: expected `in` after `for {var}`, got {other:?}"
            )));
        }
    }
    let mut items: Vec<Expr> = Vec::new();
    for tok in it {
        // A stray `do` in the item list means the header was not
        // separated from `do` by `;` or a newline (invalid POSIX);
        // refuse rather than fold `do` into the item list.
        if let RawToken::Bare(w) = tok {
            if w == "do" {
                return Err(FrontendError::Parse(format!(
                    "bashrs-frontend: `for` header `{header}` is not separated from `do` \
                     by `;` or a newline; write `for {var} in …; do` or put `do` on its \
                     own line"
                )));
            }
        }
        items.push(lower_raw_token(tok)?);
    }
    Ok((var, items))
}

/// PMAT-1276: parse a loop HEADER segment (everything before `do`)
/// into its `LoopKind`, dispatching on the leading keyword.
///
///   * `for VAR in ITEMS` → `LoopKind::For` (via `parse_for_header`).
///   * `while COND` / `until COND` → `LoopKind::While`/`Until` whose
///     condition is captured VERBATIM as an opaque `Expr::LitStr`.
///     This matches the IR's documented v0.1.0 posture — the loop
///     condition is OPAQUE; the backend prints it back byte-for-byte
///     (`render_arg(LitStr(s)) == s`). We deliberately do NOT model
///     the `[ … ]` test structurally (that's the v0.2.0 real parser).
fn parse_loop_header(header: &str) -> Result<LoopKind, FrontendError> {
    match first_word(header) {
        "for" => {
            let (var, items) = parse_for_header(header)?;
            Ok(LoopKind::For { var, items })
        }
        kw @ ("while" | "until") => {
            // Condition = everything after the leading keyword, kept
            // verbatim as an opaque LitStr (round-trips through the
            // backend unchanged). `$VAR` refs inside stay literal in
            // the string; the shell expands them at run time.
            let cond_text = header
                .strip_prefix(kw)
                .expect("first_word matched implies a prefix")
                .trim();
            if cond_text.is_empty() {
                return Err(FrontendError::Parse(format!(
                    "bashrs-frontend: `{kw}` loop has an empty condition; \
                     write `{kw} <cond>; do … done`"
                )));
            }
            let cond = Expr::LitStr(cond_text.to_string());
            Ok(if kw == "while" {
                LoopKind::While { cond }
            } else {
                LoopKind::Until { cond }
            })
        }
        other => Err(FrontendError::Parse(format!(
            "bashrs-frontend: unrecognized loop header keyword `{other}` in `{header}`"
        ))),
    }
}

/// PMAT-1268/1276: parse a POSIX `for`/`while`/`until` loop beginning
/// at `lines[start]` (lines are pre-trimmed). Returns the built
/// `Stmt::ShellLoop` and the index of the line AFTER the matching
/// `done`.
///
/// SLICE SCOPE (honest boundary):
///   - `for VAR in ITEMS; do FLAT-BODY done` (PMAT-1268).
///   - `while COND; do FLAT-BODY done` / `until COND; do … done`
///     (PMAT-1276) — COND is captured verbatim as an opaque
///     `Expr::LitStr` (see `parse_loop_header`); the `[ … ]` test is
///     NOT modelled structurally.
///
/// Both dialects require a FLAT body (Cmd / Pipeline / ShellAssign).
/// NESTED control-flow inside the body (another loop, or `if`/`case`)
/// is REFUSED, as are trailing constructs after `done` (redirects,
/// `&`, chained commands). `if`/`case` headers themselves are still
/// refused by the caller. The structural `;`/newline split of the loop
/// skeleton is not quoting-aware — a `;` inside a quoted item /
/// condition mis-splits, but that degrades to a tokenizer error
/// (unbalanced quote) and thus a clean REFUSE, never a silent shred.
/// PMAT-1281: collect ONE loop region — from the header line `start`
/// through its depth-matched `done` — into a command-position SEGMENT
/// stream, and return that stream plus the index of the LINE just past
/// the matching `done`.
///
/// The region's lines are split on `;` (the same loop-scoped `;`
/// handling the pre-1281 flat parser used — the TOP level stays
/// line-based, so top-level `;`-as-literal and quoted `;` are
/// unaffected). A leading `do` carrying an inline first body command
/// (`do echo $i`, or even `do for j …`) is normalised into a standalone
/// `do` segment followed by its remainder, so `parse_loop_at` /
/// `parse_segment_seq` see a keyword-at-front stream and handle
/// arbitrarily NESTED loops by recursion. Depth tracks
/// `for`/`while`/`until` openers (+1) against `done` (-1); the region
/// ends when depth returns to 0.
///
/// Note (pre-existing edge, unchanged): if the matching `done` sits
/// mid-line with content after it on the same physical line
/// (`… done; echo after`), that trailing content is not collected —
/// the caller resumes at the next line. This matched the pre-1281
/// single-`done` behaviour.
fn collect_block_region(
    lines: &[&str],
    start: usize,
) -> Result<(Vec<String>, usize), FrontendError> {
    let mut segs: Vec<String> = Vec::new();
    let mut depth: i32 = 0;
    let mut li = start;
    let mut next_line: Option<usize> = None;
    // Push a segment and account for its depth effect; returns true if
    // this segment is the region-closing terminator (depth back to 0).
    // Block OPENERS `for`/`while`/`until`/`if` push depth; CLOSERS
    // `done` (loop) / `fi` (if) pop it. `do`/`then`/`else` are
    // depth-neutral mid-block keywords.
    fn account(segs: &mut Vec<String>, depth: &mut i32, s: &str) -> bool {
        segs.push(s.to_string());
        match first_word(s) {
            "for" | "while" | "until" | "if" => {
                *depth += 1;
                false
            }
            "done" | "fi" => {
                *depth -= 1;
                *depth == 0
            }
            _ => false,
        }
    }
    'outer: while li < lines.len() {
        let line = lines[li];
        li += 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        for raw in line.split(';') {
            let s = raw.trim();
            if s.is_empty() || s.starts_with('#') {
                continue;
            }
            // Normalise a leading block-body keyword (`do`/`then`/`else`)
            // carrying an inline first body command into `<kw>`,
            // `<rest>` — the rest may itself be a nested block header or
            // a flat command, handled uniformly by the recursive parser.
            let fw = first_word(s);
            if matches!(fw, "do" | "then" | "else") && s != fw {
                segs.push(fw.to_string());
                let rest = s[fw.len()..].trim();
                if !rest.is_empty() && account(&mut segs, &mut depth, rest) {
                    next_line = Some(li);
                    break 'outer;
                }
                continue;
            }
            if account(&mut segs, &mut depth, s) {
                next_line = Some(li);
                break 'outer;
            }
        }
    }
    match next_line {
        Some(n) => Ok((segs, n)),
        None => {
            let (kind, term) = if first_word(lines[start]) == "if" {
                ("if", "fi")
            } else {
                ("loop", "done")
            };
            Err(FrontendError::Parse(format!(
                "bashrs-frontend: unterminated `{}` block starting at `{}` — no matching `{term}`",
                kind, lines[start]
            )))
        }
    }
}

/// PMAT-1268/1276/1281/1283: parse the control-flow BLOCK beginning at
/// line `start` — a `for`/`while`/`until` loop or an `if` conditional.
/// Collects the block region (depth-matched through its `done`/`fi`) and
/// parses it — including any NESTED blocks in the body — returning the
/// built `Stmt` and the index of the LINE just past the terminator.
fn parse_block(lines: &[&str], start: usize) -> Result<(Stmt, usize), FrontendError> {
    let (segs, next_line) = collect_block_region(lines, start)?;
    let (stmt, _seg_end) = if first_word(&segs[0]) == "if" {
        parse_if_at(&segs, 0)?
    } else {
        parse_loop_at(&segs, 0)?
    };
    Ok((stmt, next_line))
}

/// PMAT-1281: parse a sequence of command-position segments (from
/// `flatten_to_segments`) into `Stmt`s by recursive descent, starting
/// at `pos`. When `in_loop` is true we are inside a `do … done` body:
/// a `done` segment terminates the sequence (its index + 1 is returned
/// so the caller resumes just past it); when false (top level) a stray
/// `done` is an error. Loop headers recurse into `parse_loop_at`, so
/// NESTING is handled to arbitrary depth (each loop consumes its own
/// matching `done`). `if`/`case` (and any stray control keyword) still
/// REFUSE — never shred.
fn parse_segment_seq<'a>(
    segs: &'a [String],
    pos: usize,
    terminators: &[&str],
) -> Result<(Vec<Stmt>, usize, Option<&'a str>), FrontendError> {
    let mut stmts: Vec<Stmt> = Vec::new();
    let mut i = pos;
    while i < segs.len() {
        let seg = &segs[i];
        let fw = first_word(seg);
        // A terminator keyword ends this sequence; return WITHOUT
        // consuming it (the caller consumes / dispatches). For block
        // closers (`done`/`fi`) require no trailing content.
        if terminators.contains(&fw) {
            if (fw == "done" || fw == "fi") && seg.as_str() != fw {
                return Err(FrontendError::Parse(format!(
                    "bashrs-frontend: `{fw}` has trailing content `{seg}` \
                     (redirects / chained commands after `{fw}` are not supported); \
                     refusing rather than dropping it"
                )));
            }
            return Ok((stmts, i, Some(fw)));
        }
        match fw {
            "for" | "while" | "until" => {
                let (loop_stmt, next) = parse_loop_at(segs, i)?;
                stmts.push(loop_stmt);
                i = next;
            }
            "if" => {
                let (if_stmt, next) = parse_if_at(segs, i)?;
                stmts.push(if_stmt);
                i = next;
            }
            "do" | "done" | "then" | "elif" | "else" | "fi" => {
                // A structural keyword that is NOT an expected terminator
                // here — a stray `done`/`fi`/`elif`/`else` with no open
                // block expecting it (well-formed loop/if/elif chains are
                // consumed by `parse_loop_at`/`parse_if_at`). Refuse —
                // never shred.
                return Err(FrontendError::Parse(format!(
                    "bashrs-frontend: stray shell keyword `{fw}` in `{seg}` — no open \
                     block expects it here; refusing rather than shredding into barewords"
                )));
            }
            "case" | "esac" => {
                // `case`/`esac` IS supported at TOP LEVEL (PMAT-1285); only
                // a `case` NESTED inside a loop/if body refuses — this
                // `;`-segment-split context would mangle the arm `;;`
                // terminators (v0.2.0 work).
                return Err(FrontendError::Parse(format!(
                    "bashrs-frontend: shell `case`/`esac` is top-level only — a `case` \
                     nested inside a loop/if body is not supported (the `;`-segment \
                     split would mangle arm `;;` terminators); refusing rather than \
                     shredding `{seg}` into barewords."
                )));
            }
            _ => {
                // First word isn't a control keyword, but one can hide
                // AFTER a `&&`/`||`/`|` our `;`-only split didn't separate
                // (e.g. `echo a && if …`). Refuse rather than let
                // `lower_flat_line` shred it into barewords.
                if let Some(kw) = control_flow_keyword(seg) {
                    return Err(FrontendError::Parse(format!(
                        "bashrs-frontend: shell control-flow keyword `{kw}` after a \
                         `&&`/`||`/`|` in `{seg}` — compound control-flow is not supported; \
                         refusing rather than shredding into barewords."
                    )));
                }
                if let Some(stmt) = lower_flat_line(seg)? {
                    stmts.push(stmt);
                }
                i += 1;
            }
        }
    }
    if !terminators.is_empty() {
        return Err(FrontendError::Parse(format!(
            "bashrs-frontend: unterminated block — reached end of input expecting one of \
             {terminators:?}"
        )));
    }
    Ok((stmts, i, None))
}

/// PMAT-1268/1276/1281: parse one loop whose header is `segs[start]`
/// (`for`/`while`/`until`). Expects `segs[start + 1]` to be the `do`
/// keyword; the body is parsed recursively (so a NESTED block is
/// handled) up to the matching `done`. Returns the `Stmt::ShellLoop`
/// and the index just past that `done`.
fn parse_loop_at(segs: &[String], start: usize) -> Result<(Stmt, usize), FrontendError> {
    let kw = first_word(&segs[start]);
    let kind = parse_loop_header(&segs[start])?;
    if start + 1 >= segs.len() || first_word(&segs[start + 1]) != "do" {
        return Err(FrontendError::Parse(format!(
            "bashrs-frontend: `{kw}` loop header `{}` must be followed by `do`",
            &segs[start]
        )));
    }
    let (body, done_idx, _term) = parse_segment_seq(segs, start + 2, &["done"])?;
    Ok((Stmt::ShellLoop { kind, body }, done_idx + 1))
}

/// PMAT-1283: parse one `if COND; then … [else …] fi` conditional whose
/// header is `segs[start]` (`if COND`). The condition is captured
/// verbatim as an opaque `Expr::LitStr` (same posture as loop
/// conditions). The then/else bodies are parsed recursively — so a
/// NESTED loop / conditional in either branch is handled — and each
/// consumes up to its terminator. Returns the `Stmt::ShellIf` and the
/// index just past the matching `fi`.
fn parse_if_at(segs: &[String], start: usize) -> Result<(Stmt, usize), FrontendError> {
    // The opener is `if` (top of a chain) or `elif` (a recursive
    // continuation). PMAT-1284: `elif` is DESUGARED into a nested
    // `Stmt::ShellIf` living in the parent's `else_body`, so the whole
    // `if … elif … [else …] fi` chain shares one `Stmt::ShellIf` shape
    // (no new IR); the backend re-sugars a lone-`ShellIf` else-body back
    // to `elif`. The chain closes on a single `fi`.
    let opener = first_word(&segs[start]);
    debug_assert!(opener == "if" || opener == "elif");
    let cond_text = segs[start]
        .strip_prefix(opener)
        .expect("caller dispatched on the opener keyword")
        .trim();
    if cond_text.is_empty() {
        return Err(FrontendError::Parse(format!(
            "bashrs-frontend: `{opener}` has an empty condition; write \
             `{opener} <cond>; then …`"
        )));
    }
    let cond = Expr::LitStr(cond_text.to_string());
    if start + 1 >= segs.len() || first_word(&segs[start + 1]) != "then" {
        return Err(FrontendError::Parse(format!(
            "bashrs-frontend: `{opener} {cond_text}` must be followed by `then`"
        )));
    }
    // then-body runs until `elif` / `else` / `fi`.
    let (then_body, t_idx, term) = parse_segment_seq(segs, start + 2, &["elif", "else", "fi"])?;
    match term {
        Some("fi") => Ok((
            Stmt::ShellIf {
                cond,
                then_body,
                else_body: Vec::new(),
            },
            t_idx + 1,
        )),
        Some("elif") => {
            // Recurse: the `elif` opens a nested conditional whose own
            // `else`/`elif`/`fi` continues (and closes) the chain.
            let (nested, next) = parse_if_at(segs, t_idx)?;
            Ok((
                Stmt::ShellIf {
                    cond,
                    then_body,
                    else_body: vec![nested],
                },
                next,
            ))
        }
        Some("else") => {
            // else-body runs until `fi`.
            let (else_body, f_idx, _) = parse_segment_seq(segs, t_idx + 1, &["fi"])?;
            Ok((
                Stmt::ShellIf {
                    cond,
                    then_body,
                    else_body,
                },
                f_idx + 1,
            ))
        }
        _ => Err(FrontendError::Parse(format!(
            "bashrs-frontend: `{opener}` without a matching `fi`"
        ))),
    }
}

/// PMAT-1285: build the command-position segment stream for a piece of
/// shell TEXT (a `case`-arm body) — split on `;` and newline, drop
/// blank / comment segments, normalise a leading `do`/`then`/`else`
/// carrying an inline first command. Mirrors `collect_block_region`'s
/// per-line segmenting (minus the depth bookkeeping), so an arm body
/// feeds the same recursive `parse_segment_seq` and can therefore carry
/// a nested loop / conditional.
fn segments_of(text: &str) -> Vec<String> {
    let mut segs: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        for raw in line.split(';') {
            let s = raw.trim();
            if s.is_empty() || s.starts_with('#') {
                continue;
            }
            let fw = first_word(s);
            if matches!(fw, "do" | "then" | "else") && s != fw {
                segs.push(fw.to_string());
                let rest = s[fw.len()..].trim();
                if !rest.is_empty() {
                    segs.push(rest.to_string());
                }
            } else {
                segs.push(s.to_string());
            }
        }
    }
    segs
}

/// PMAT-1285: parse a TOP-LEVEL `case WORD in PAT) BODY ;; … esac`
/// beginning at line `start`, returning the built `Stmt::ShellCase` and
/// the index of the LINE just past the matching `esac`.
///
/// SLICE-1 SCOPE (honest boundary): only a TOP-LEVEL `case` — a `case`
/// nested inside a loop / `if` body is refused by `parse_segment_seq`
/// (the `;`-segment split there would mangle the arm `;;` separators).
/// Arm BODIES may themselves contain nested loops / conditionals (they
/// parse through the shared `parse_segment_seq`). The structural splits
/// (`;;` between arms, first `)` ending a pattern list) are not
/// quoting-aware; a `;;` / `)` inside a quoted pattern or a `$(…)`
/// mis-splits, but that degrades to a downstream parse error → clean
/// REFUSE, never a silent shred. `;&` / `;;&` (bash fall-through) is
/// REFUSED (PMAT-1371), not shredded — it is still unmodelled, but the
/// refusal is explicit rather than a bare `&` emitted into the arm body.
fn parse_case(lines: &[&str], start: usize) -> Result<(Stmt, usize), FrontendError> {
    // Collect region lines [start ..= matching `esac`], case-depth aware
    // (a nested `case` opener raises depth so the OUTER `esac` closes the
    // region; the nested `case` is then refused when its arm body is
    // parsed).
    let mut region: Vec<&str> = Vec::new();
    let mut depth: i32 = 0;
    let mut li = start;
    let mut next_line: Option<usize> = None;
    'outer: while li < lines.len() {
        let line = lines[li];
        li += 1;
        region.push(line);
        for part in line.split([';', '&', '|']) {
            match first_word(part.trim()) {
                "case" => depth += 1,
                "esac" => {
                    depth -= 1;
                    if depth == 0 {
                        next_line = Some(li);
                        break 'outer;
                    }
                }
                _ => {}
            }
        }
    }
    let Some(next) = next_line else {
        return Err(FrontendError::Parse(format!(
            "bashrs-frontend: unterminated `case` starting at `{}` — no matching `esac`",
            lines[start]
        )));
    };

    // Region text: `case WORD in <arms> esac`. Strip the `case` head and
    // the `esac` tail; split off the header `WORD in`.
    let joined = region.join("\n");
    let rest = joined
        .trim_start()
        .strip_prefix("case")
        .expect("caller dispatched on first_word == \"case\"")
        .trim_start();
    // WORD is the first whitespace-delimited token; `in` must follow.
    let word_tok = rest.split_whitespace().next().unwrap_or("");
    if word_tok.is_empty() {
        return Err(FrontendError::Parse(
            "bashrs-frontend: `case` has no word to match; write `case WORD in … esac`".to_string(),
        ));
    }
    let after_word = rest[word_tok.len()..].trim_start();
    let arms_and_esac = match after_word.strip_prefix("in") {
        Some(a) if a.is_empty() || a.starts_with(char::is_whitespace) => a.trim_start(),
        _ => {
            return Err(FrontendError::Parse(format!(
                "bashrs-frontend: `case {word_tok}` must be followed by `in`"
            )));
        }
    };
    let arms_text = match arms_and_esac.trim_end().strip_suffix("esac") {
        Some(a) => a.trim_end(),
        None => {
            return Err(FrontendError::Parse(
                "bashrs-frontend: malformed `case` — `esac` terminator not found where expected"
                    .to_string(),
            ));
        }
    };

    // PMAT-1371: REFUSE bash's fall-through arm terminators `;&` and `;;&`
    // rather than shredding them. This guard MUST run BEFORE the `;;` split
    // below: that split consumes the `;;` of `;;&` and leaves a bare `&`
    // glued to the NEXT arm's pattern, so a guard placed after it would see
    // only the `;&` form. Before this refusal both forms exited 0 and emitted
    // a bare `&` as a command (arm `a) echo A ;& b) echo B ;;` swallowed arm
    // `b` into arm `a`'s body); `bash -n` on the output failed with
    // "syntax error near unexpected token `&'".
    //
    // ONE `find(";&")` catches BOTH forms because `;;&` contains `;&` at
    // offset 1. A `;&` inside a quoted pattern or body does not reach here:
    // the (deliberately non-quoting-aware) `;`-segment split already refuses
    // such an arm downstream, so this guard widens no refusal surface.
    if let Some(off) = arms_text.find(";&") {
        let form = if arms_text[..off].ends_with(';') {
            ";;&"
        } else {
            ";&"
        };
        return Err(FrontendError::Parse(format!(
            "bashrs-frontend: bash `case` fall-through `{form}` is not modelled (v0.2.0); \
             refusing rather than shredding it into barewords. Terminate the arm with `;;`."
        )));
    }

    // The matched word, lowered through the quoting-aware tokenizer
    // (so `$x` → `ShellVar`, `"$x"` → a QuotedString, …). Exactly one
    // token is expected.
    let word_tokens = tokenize_line(word_tok)?;
    let word = match word_tokens.as_slice() {
        [tok] => lower_raw_token(tok)?,
        _ => {
            return Err(FrontendError::Parse(format!(
                "bashrs-frontend: `case` word `{word_tok}` must be a single token"
            )));
        }
    };

    // Arms: split on `;;`; each chunk is `PAT1|PAT2) BODY`.
    let mut arms: Vec<CaseArm> = Vec::new();
    for chunk in arms_text.split(";;") {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        let Some(paren) = chunk.find(')') else {
            return Err(FrontendError::Parse(format!(
                "bashrs-frontend: `case` arm `{chunk}` has no `)` after its pattern list"
            )));
        };
        let pat_part = chunk[..paren].trim();
        let body_text = chunk[paren + 1..].trim();
        if pat_part.is_empty() {
            return Err(FrontendError::Parse(
                "bashrs-frontend: `case` arm has an empty pattern list before `)`".to_string(),
            ));
        }
        let patterns: Vec<String> = pat_part
            .split('|')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if patterns.is_empty() {
            return Err(FrontendError::Parse(format!(
                "bashrs-frontend: `case` arm pattern list `{pat_part}` is empty"
            )));
        }
        // Arm body — parsed through the shared recursive segment parser
        // (so a nested loop / conditional in the arm composes; a nested
        // `case` refuses).
        let (body, _end, _term) = parse_segment_seq(&segments_of(body_text), 0, &[])?;
        arms.push(CaseArm { patterns, body });
    }

    if arms.is_empty() {
        return Err(FrontendError::Parse(
            "bashrs-frontend: `case … in … esac` has no arms".to_string(),
        ));
    }

    Ok((Stmt::ShellCase { word, arms }, next))
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
        // PMAT-1268/1276/1281: the TOP level stays line-based (so a
        // top-level `;` keeps its pre-1281 literal-passthrough
        // behaviour and quoted `;` / `$(…)` are unaffected). A
        // `for`/`while`/`until` line routes to `parse_loop`, which
        // collects the loop region and parses it — including
        // arbitrarily NESTED loops — via the recursive segment parser,
        // returning the LINE index just past the matching `done`.
        let trimmed_lines: Vec<&str> = spliced.lines().map(str::trim).collect();
        let mut stmts: Vec<Stmt> = Vec::new();
        let mut i = 0;
        while i < trimmed_lines.len() {
            let line = trimmed_lines[i];
            if line.is_empty() {
                i += 1;
                continue;
            }
            if line.starts_with("#!") {
                // Shebang. Skip — not a command, just an interpreter directive.
                i += 1;
                continue;
            }
            if line.starts_with('#') {
                // Comment line.
                i += 1;
                continue;
            }
            if matches!(first_word(line), "for" | "while" | "until" | "if") {
                let (block_stmt, next) = parse_block(&trimmed_lines, i)?;
                stmts.push(block_stmt);
                i = next;
                continue;
            }
            // PMAT-1285: a top-level `case WORD in … esac`. It needs raw
            // lines (the `;`-segment split would mangle arm `;;`), so it
            // is parsed here rather than through the segment machinery;
            // a `case` nested in a loop/if body is refused downstream.
            if first_word(line) == "case" {
                let (case_stmt, next) = parse_case(&trimmed_lines, i)?;
                stmts.push(case_stmt);
                i = next;
                continue;
            }
            // PMAT-989: REFUSE the remaining shell control-flow (`case`,
            // and any stray `do`/`done`/`then`/`elif`/`else`/`fi`/`esac`
            // keyword, or a for/while/until/if keyword appearing after a
            // `;` on a compound line rather than at line start) instead
            // of silently shredding it into barewords. `case` (and
            // `elif` chains) are the v0.2.0 job.
            if let Some(kw) = control_flow_keyword(line) {
                return Err(FrontendError::Parse(format!(
                    "bashrs-frontend: unsupported shell control-flow (a `case` nested in a \
                     block body, a stray `esac`/`elif`/`then`/`fi`/`do`/`done`, or a \
                     control-flow keyword after a `;` on a compound line) — top-level \
                     `for`/`while`/`until` loops, `if`/`then`/`elif`/`else`, and `case` \
                     are handled, but the rest refuses rather than silently shredding into \
                     barewords. Offending keyword `{kw}` at line `{line}`."
                )));
            }
            // Flat (non-control-flow) command line.
            if let Some(stmt) = lower_flat_line(line)? {
                stmts.push(stmt);
            }
            i += 1;
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
        let Item::Function(f) = &module.items[0] else {
            unreachable!("test fixture has no module constants")
        };
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
        let Item::Function(f) = &module.items[0] else {
            unreachable!("test fixture has no module constants")
        };
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
        let Item::Function(f) = &module.items[0] else {
            unreachable!("test fixture has no module constants")
        };
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
        let Item::Function(f) = &module.items[0] else {
            unreachable!("test fixture has no module constants")
        };
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
        let Item::Function(f) = &module.items[0] else {
            unreachable!("test fixture has no module constants")
        };
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
        let Item::Function(f) = &module.items[0] else {
            unreachable!("test fixture has no module constants")
        };
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
            let Item::Function(f) = &module.items[0] else {
                unreachable!("test fixture has no module constants")
            };
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
    fn parse_and_lower_semicolon_separator_round_trips_via_litstr() {
        // PMAT-119: POSIX `;` statement separator (between
        // commands on the same line) round-trips via LitStr
        // passthrough at v0.1.0. Like redirections, short-circuit
        // operators, and test brackets, the tokens land as
        // ordinary `Expr::LitStr` args; the downstream shell
        // re-interprets `;` as a statement boundary at execution
        // time.
        //
        // Without surrounding spaces (e.g., `cd /tmp;` followed
        // by `ls`), the `;` attaches to the preceding bareword
        // and the round-trip is still semantics-preserving
        // because the downstream shell does its own
        // re-tokenization. With spaces around `;`, the round-
        // trip produces one Stmt::Cmd with `;` as a LitStr arg.
        //
        // Real shell scripts use `;` for compact multi-command
        // lines (`cd /tmp; ls; cd -`). The IR doesn't model the
        // statement-separator structure (that's
        // XPILE-BASHRS-STMT-SEP-001 future work), but the
        // byte-level round-trip preserves shell semantics.
        //
        // Completes the v0.1.0 LitStr-passthrough invariant
        // lock-in series (PMAT-085..091, capstone PMAT-092 plus
        // this one).
        use xpile_meta_hir::{Expr, Item, Stmt};
        let cases: &[(&str, &str, &[&str])] = &[
            ("cd /tmp ; ls\n", "cd", &["/tmp", ";", "ls"]),
            ("echo a ; echo b\n", "echo", &["a", ";", "echo", "b"]),
            (
                "cd / ; ls ; cd -\n",
                "cd",
                &["/", ";", "ls", ";", "cd", "-"],
            ),
        ];
        for (source, expected_program, expected_args) in cases {
            let module = BashrsFrontend
                .parse_and_lower(&PathBuf::from("/tmp/semi.sh"), source)
                .unwrap_or_else(|e| panic!("parse failed for `{source}`: {e:?}"));
            let Item::Function(f) = &module.items[0] else {
                unreachable!("test fixture has no module constants")
            };
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
                "semicolon-separator round-trip for `{source}` failed"
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
            let Item::Function(f) = &module.items[0] else {
                unreachable!("test fixture has no module constants")
            };
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
        let Item::Function(f) = &module.items[0] else {
            unreachable!("test fixture has no module constants")
        };
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
            let Item::Function(f) = &module.items[0] else {
                unreachable!("test fixture has no module constants")
            };
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
            let Item::Function(f) = &module.items[0] else {
                unreachable!("test fixture has no module constants")
            };
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
            let Item::Function(f) = &module.items[0] else {
                unreachable!("test fixture has no module constants")
            };
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
        let Item::Function(f) = &module.items[0] else {
            unreachable!("test fixture has no module constants")
        };
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
        let Item::Function(f) = &module.items[0] else {
            unreachable!("test fixture has no module constants")
        };
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
        let Item::Function(f) = &module.items[0] else {
            unreachable!("test fixture has no module constants")
        };
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
        let Item::Function(f) = &module.items[0] else {
            unreachable!("test fixture has no module constants")
        };
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
        let Item::Function(f) = &module.items[0] else {
            unreachable!("test fixture has no module constants")
        };
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
        let Item::Function(f) = &module.items[0] else {
            unreachable!("test fixture has no module constants")
        };
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
        let Item::Function(f) = &module.items[0] else {
            unreachable!("test fixture has no module constants")
        };
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

    // ----- PMAT-989: control-flow REFUSAL (no silent shredding) -----

    /// PMAT-1268 helper: extract the single `Stmt::ShellLoop` a
    /// for-loop fixture should lower to, asserting the surrounding
    /// module shape (one synthesised `main` holding exactly the loop).
    #[cfg(test)]
    fn only_shell_loop(module: &Module) -> (&LoopKind, &[Stmt]) {
        assert_eq!(module.items.len(), 1, "expected one synthesised function");
        let Item::Function(f) = &module.items[0] else {
            unreachable!("for-loop fixture has no module constants")
        };
        assert_eq!(f.name, "main");
        assert_eq!(
            f.body.stmts.len(),
            1,
            "expected exactly one top-level Stmt::ShellLoop, got {:?}",
            f.body.stmts
        );
        match &f.body.stmts[0] {
            Stmt::ShellLoop { kind, body } => (kind, body.as_slice()),
            other => panic!("expected Stmt::ShellLoop, got {other:?}"),
        }
    }

    #[test]
    fn parse_and_lower_single_line_for_loop() {
        // PMAT-1268: the historical PMAT-989 shred shape
        // (`for i in 1 2 3; do echo $i; done` mislowered into four
        // bareword Cmds) is now PARSED into a real `Stmt::ShellLoop`.
        // The IR (`LoopKind::For`) and bashrs-backend renderer already
        // existed; this is the frontend catching up.
        let module = BashrsFrontend
            .parse_and_lower(
                &PathBuf::from("/tmp/loop.sh"),
                "for i in 1 2 3; do echo $i; done\n",
            )
            .expect("single-line for-loop must now parse into a ShellLoop");
        let (kind, body) = only_shell_loop(&module);
        assert_eq!(
            kind,
            &LoopKind::For {
                var: "i".to_string(),
                items: vec![
                    Expr::LitStr("1".to_string()),
                    Expr::LitStr("2".to_string()),
                    Expr::LitStr("3".to_string()),
                ],
            }
        );
        assert_eq!(
            body,
            &[Stmt::Cmd {
                program: "echo".to_string(),
                args: vec![Expr::ShellVar("i".to_string())],
            }]
        );
    }

    #[test]
    fn parse_and_lower_multi_line_for_loop_do_on_own_line() {
        // The multi-line shape with `for`, `do`, `done` each on their
        // own physical line — the other historically-shredded form.
        let source = "\
for i in 1 2 3
do
  echo $i
done
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/loop2.sh"), source)
            .expect("multi-line for-loop must parse into a ShellLoop");
        let (kind, body) = only_shell_loop(&module);
        assert_eq!(
            kind,
            &LoopKind::For {
                var: "i".to_string(),
                items: vec![
                    Expr::LitStr("1".to_string()),
                    Expr::LitStr("2".to_string()),
                    Expr::LitStr("3".to_string()),
                ],
            }
        );
        assert_eq!(
            body,
            &[Stmt::Cmd {
                program: "echo".to_string(),
                args: vec![Expr::ShellVar("i".to_string())],
            }]
        );
    }

    #[test]
    fn parse_and_lower_for_loop_semicolon_do_multiline_body() {
        // `; do` on the header line, then a multi-command body across
        // physical lines. Verifies body statements accumulate in order
        // and the `do`-remainder path and the newline-body path agree.
        let source = "\
for name in alice bob; do
  echo hi $name
  echo bye $name
done
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/loop3.sh"), source)
            .expect("`; do` + multi-command body must parse");
        let (kind, body) = only_shell_loop(&module);
        assert_eq!(
            kind,
            &LoopKind::For {
                var: "name".to_string(),
                items: vec![
                    Expr::LitStr("alice".to_string()),
                    Expr::LitStr("bob".to_string()),
                ],
            }
        );
        assert_eq!(body.len(), 2, "two body commands expected");
        assert_eq!(
            body[0],
            Stmt::Cmd {
                program: "echo".to_string(),
                args: vec![
                    Expr::LitStr("hi".to_string()),
                    Expr::ShellVar("name".to_string())
                ],
            }
        );
        assert_eq!(
            body[1],
            Stmt::Cmd {
                program: "echo".to_string(),
                args: vec![
                    Expr::LitStr("bye".to_string()),
                    Expr::ShellVar("name".to_string())
                ],
            }
        );
    }

    #[test]
    fn parse_and_lower_for_loop_first_body_cmd_inline_with_do() {
        // Compact one-liner where the first body command rides on the
        // `do` segment (`; do echo $i;`) — the `do`-remainder path.
        let module = BashrsFrontend
            .parse_and_lower(
                &PathBuf::from("/tmp/loop4.sh"),
                "for x in a b; do echo $x; echo done_marker; done\n",
            )
            .expect("inline-do-body one-liner must parse");
        let (_, body) = only_shell_loop(&module);
        assert_eq!(body.len(), 2);
        assert_eq!(
            body[0],
            Stmt::Cmd {
                program: "echo".to_string(),
                args: vec![Expr::ShellVar("x".to_string())],
            }
        );
        // `done_marker` is a bareword ARG, not the `done` keyword — it
        // must survive as an ordinary Cmd arg (command-position check).
        assert_eq!(
            body[1],
            Stmt::Cmd {
                program: "echo".to_string(),
                args: vec![Expr::LitStr("done_marker".to_string())],
            }
        );
    }

    #[test]
    fn parse_and_lower_for_loop_quoted_items() {
        // Items are lowered through the quoting-aware tokenizer, so a
        // quoted multi-word item stays ONE item.
        let module = BashrsFrontend
            .parse_and_lower(
                &PathBuf::from("/tmp/loop5.sh"),
                "for w in 'a b' c; do echo $w; done\n",
            )
            .expect("quoted for-items must parse");
        let (kind, _) = only_shell_loop(&module);
        assert_eq!(
            kind,
            &LoopKind::For {
                var: "w".to_string(),
                items: vec![
                    Expr::QuotedString {
                        content: "a b".to_string(),
                        quoting: QuotingStrategy::Single,
                    },
                    Expr::LitStr("c".to_string()),
                ],
            }
        );
    }

    #[test]
    fn parse_and_lower_for_loop_empty_item_list() {
        // POSIX-legal `for x in ; do … done` — an empty item list (the
        // body never runs). We model items as an empty Vec, not a
        // refusal.
        let module = BashrsFrontend
            .parse_and_lower(
                &PathBuf::from("/tmp/loop6.sh"),
                "for x in ; do echo $x; done\n",
            )
            .expect("empty item list is legal POSIX");
        let (kind, _) = only_shell_loop(&module);
        assert_eq!(
            kind,
            &LoopKind::For {
                var: "x".to_string(),
                items: vec![],
            }
        );
    }

    #[test]
    fn parse_and_lower_for_loop_composes_with_flat_commands() {
        // A loop surrounded by ordinary flat commands: assignment,
        // then loop, then a trailing command — order preserved.
        let source = "\
GREETING=hi
for n in 1 2; do
  echo $GREETING $n
done
echo after
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/loop7.sh"), source)
            .expect("loop composed with flat commands must parse");
        let Item::Function(f) = &module.items[0] else {
            unreachable!("no module constants")
        };
        assert_eq!(f.body.stmts.len(), 3, "assign + loop + trailing cmd");
        assert!(matches!(f.body.stmts[0], Stmt::ShellAssign { .. }));
        assert!(matches!(f.body.stmts[1], Stmt::ShellLoop { .. }));
        assert!(matches!(f.body.stmts[2], Stmt::Cmd { .. }));
    }

    #[test]
    fn parse_and_lower_nested_for_loop() {
        // PMAT-1281: a `for` nested inside a `for` body now PARSES into
        // a `ShellLoop` whose body is itself a `ShellLoop` (the backend
        // already renders nested loops recursively). Was refused pre-1281.
        let source = "\
for i in 1 2; do
  for j in a b; do
    echo $i $j
  done
done
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/nested.sh"), source)
            .expect("nested for-loop must parse");
        let (outer_kind, outer_body) = only_shell_loop(&module);
        assert_eq!(
            outer_kind,
            &LoopKind::For {
                var: "i".to_string(),
                items: vec![Expr::LitStr("1".into()), Expr::LitStr("2".into())],
            }
        );
        // The outer body is exactly one statement: the inner loop.
        assert_eq!(outer_body.len(), 1, "outer body is the inner loop");
        let Stmt::ShellLoop {
            kind: inner_kind,
            body: inner_body,
        } = &outer_body[0]
        else {
            panic!("expected a nested ShellLoop, got {:?}", outer_body[0]);
        };
        assert_eq!(
            inner_kind,
            &LoopKind::For {
                var: "j".to_string(),
                items: vec![Expr::LitStr("a".into()), Expr::LitStr("b".into())],
            }
        );
        assert_eq!(
            inner_body,
            &[Stmt::Cmd {
                program: "echo".to_string(),
                args: vec![Expr::ShellVar("i".into()), Expr::ShellVar("j".into())],
            }]
        );
    }

    #[test]
    fn parse_and_lower_nested_for_loop_single_line() {
        // The all-on-one-line nesting form:
        // `for i in 1 2; do for j in a b; do echo $i $j; done; done`.
        // Exercises the `do <nested-header>` normalisation + depth-aware
        // `done` matching in `collect_loop_region`.
        let module = BashrsFrontend
            .parse_and_lower(
                &PathBuf::from("/tmp/nested1.sh"),
                "for i in 1 2; do for j in a b; do echo $i $j; done; done\n",
            )
            .expect("single-line nested for-loop must parse");
        let (_outer_kind, outer_body) = only_shell_loop(&module);
        assert_eq!(outer_body.len(), 1);
        assert!(
            matches!(&outer_body[0], Stmt::ShellLoop { .. }),
            "outer body must be the inner loop, got {:?}",
            outer_body[0]
        );
    }

    #[test]
    fn parse_and_lower_refuses_unterminated_for_loop() {
        // A `for` header with no matching `done` must refuse, never
        // silently accept a truncated loop.
        let source = "\
for i in 1 2 3; do
  echo $i
";
        let err = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/unterm.sh"), source)
            .expect_err("unterminated for-loop must be REFUSED");
        assert!(format!("{err:?}").contains("unterminated"));
    }

    #[test]
    fn parse_and_lower_refuses_for_without_in() {
        // `for x; do … done` iterates the positional params (`$@`) —
        // a distinct semantics we don't model at this slice. Refuse.
        let err = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/noin.sh"), "for x; do echo $x; done\n")
            .expect_err("`for x` without `in` must be REFUSED");
        assert!(format!("{err:?}").contains("positional"));
    }

    #[test]
    fn parse_and_lower_refuses_done_with_trailing_content() {
        // `done > file` / `done; echo x` (redirect or chained command
        // after `done`) is out of slice-1 scope — refuse rather than
        // drop the trailing construct.
        let err = BashrsFrontend
            .parse_and_lower(
                &PathBuf::from("/tmp/trail.sh"),
                "for i in 1 2; do echo $i; done > /dev/null\n",
            )
            .expect_err("trailing content after `done` must be REFUSED");
        assert!(format!("{err:?}").contains("trailing content"));
    }

    #[test]
    fn parse_and_lower_refuses_for_after_semicolon_on_compound_line() {
        // A `for` that appears after a `;` (not at line start) is a
        // compound-command shape the slice-1 parser doesn't assemble —
        // it's caught by the residual control-flow refusal, not shred.
        let err = BashrsFrontend
            .parse_and_lower(
                &PathBuf::from("/tmp/compound.sh"),
                "echo hi; for i in 1 2; do echo $i; done\n",
            )
            .expect_err("a `for` after `;` on a compound line must be REFUSED");
        assert!(format!("{err:?}").contains("control-flow"));
    }

    /// PMAT-1283 helper: the single `Stmt::ShellIf` a fixture lowers to.
    #[cfg(test)]
    fn only_shell_if(module: &Module) -> (&Expr, &[Stmt], &[Stmt]) {
        assert_eq!(module.items.len(), 1);
        let Item::Function(f) = &module.items[0] else {
            unreachable!("no module constants")
        };
        assert_eq!(f.body.stmts.len(), 1, "expected one Stmt::ShellIf");
        match &f.body.stmts[0] {
            Stmt::ShellIf {
                cond,
                then_body,
                else_body,
            } => (cond, then_body.as_slice(), else_body.as_slice()),
            other => panic!("expected Stmt::ShellIf, got {other:?}"),
        }
    }

    #[test]
    fn parse_and_lower_if_then_fi() {
        // PMAT-1283: `if COND; then … fi` now PARSES into a
        // `Stmt::ShellIf` with an empty else-body. The condition is an
        // opaque LitStr (round-trips verbatim). Was refused pre-1283.
        let source = "\
if [ -f /tmp/x ]; then
  echo found
fi
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/cond.sh"), source)
            .expect("if/then/fi must parse");
        let (cond, then_body, else_body) = only_shell_if(&module);
        assert_eq!(cond, &Expr::LitStr("[ -f /tmp/x ]".to_string()));
        assert_eq!(
            then_body,
            &[Stmt::Cmd {
                program: "echo".to_string(),
                args: vec![Expr::LitStr("found".to_string())],
            }]
        );
        assert!(else_body.is_empty(), "no else arm");
    }

    #[test]
    fn parse_and_lower_if_then_else_fi() {
        // The `else` arm parses into `else_body`.
        let source = "\
if [ $x -gt 3 ]; then
  echo big
else
  echo small
fi
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/cond2.sh"), source)
            .expect("if/then/else/fi must parse");
        let (cond, then_body, else_body) = only_shell_if(&module);
        assert_eq!(cond, &Expr::LitStr("[ $x -gt 3 ]".to_string()));
        assert_eq!(then_body.len(), 1);
        assert_eq!(else_body.len(), 1);
        assert_eq!(
            else_body[0],
            Stmt::Cmd {
                program: "echo".to_string(),
                args: vec![Expr::LitStr("small".to_string())],
            }
        );
    }

    #[test]
    fn parse_and_lower_single_line_if() {
        // The compact one-liner form `if C; then A; else B; fi`.
        let module = BashrsFrontend
            .parse_and_lower(
                &PathBuf::from("/tmp/cond3.sh"),
                "if [ $n -eq 0 ]; then echo zero; else echo nonzero; fi\n",
            )
            .expect("single-line if must parse");
        let (_cond, then_body, else_body) = only_shell_if(&module);
        assert_eq!(then_body.len(), 1);
        assert_eq!(else_body.len(), 1);
    }

    #[test]
    fn parse_and_lower_if_nested_in_for_loop() {
        // An `if` inside a `for` body (mixed block nesting).
        let source = "\
for i in 1 2; do
  if [ $i -eq 1 ]; then
    echo one
  fi
done
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/mix.sh"), source)
            .expect("if-in-for must parse");
        let (_kind, body) = only_shell_loop(&module);
        assert_eq!(body.len(), 1);
        assert!(
            matches!(&body[0], Stmt::ShellIf { .. }),
            "loop body must be the if, got {:?}",
            body[0]
        );
    }

    #[test]
    fn parse_and_lower_loop_nested_in_if_branch() {
        // A `for` loop inside an `if` then-branch (the other nesting mix).
        let source = "\
if [ -d /tmp ]; then
  for f in a b; do
    echo $f
  done
fi
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/loopif.sh"), source)
            .expect("loop-in-if must parse");
        let (_cond, then_body, _else) = only_shell_if(&module);
        assert_eq!(then_body.len(), 1);
        assert!(matches!(&then_body[0], Stmt::ShellLoop { .. }));
    }

    #[test]
    fn parse_and_lower_refuses_empty_if_condition() {
        let err = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/emptyif.sh"), "if ; then echo x; fi\n")
            .expect_err("empty if condition must be REFUSED");
        assert!(format!("{err:?}").contains("empty condition"));
    }

    #[test]
    fn parse_and_lower_refuses_unterminated_if() {
        let err = BashrsFrontend
            .parse_and_lower(
                &PathBuf::from("/tmp/untermif.sh"),
                "if [ -f /tmp/x ]; then\n  echo hi\n",
            )
            .expect_err("unterminated if must be REFUSED");
        assert!(format!("{err:?}").contains("unterminated"));
    }

    #[test]
    fn parse_and_lower_elif_chain_desugars_to_nested_if() {
        // PMAT-1284: `elif` now PARSES — desugared into a nested
        // `Stmt::ShellIf` in the parent's `else_body`. `if C1; elif C2;
        // fi` → ShellIf{C1, .., else:[ShellIf{C2, .., else:[]}]}.
        let source = "\
if [ $x -eq 1 ]; then
  echo one
elif [ $x -eq 2 ]; then
  echo two
fi
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/elif.sh"), source)
            .expect("elif chain must parse");
        let (cond, then_body, else_body) = only_shell_if(&module);
        assert_eq!(cond, &Expr::LitStr("[ $x -eq 1 ]".to_string()));
        assert_eq!(then_body.len(), 1);
        // else_body is exactly the nested `elif` conditional.
        assert_eq!(else_body.len(), 1);
        let Stmt::ShellIf {
            cond: c2,
            then_body: t2,
            else_body: e2,
        } = &else_body[0]
        else {
            panic!(
                "elif must desugar to a nested ShellIf, got {:?}",
                else_body[0]
            );
        };
        assert_eq!(c2, &Expr::LitStr("[ $x -eq 2 ]".to_string()));
        assert_eq!(
            t2,
            &vec![Stmt::Cmd {
                program: "echo".to_string(),
                args: vec![Expr::LitStr("two".to_string())],
            }]
        );
        assert!(
            e2.is_empty(),
            "no final else, so the innermost else is empty"
        );
    }

    #[test]
    fn parse_and_lower_elif_chain_with_final_else() {
        // A 3-way chain ending in `else` nests two deep.
        let source = "\
if [ $x -eq 1 ]; then
  echo one
elif [ $x -eq 2 ]; then
  echo two
else
  echo other
fi
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/elif2.sh"), source)
            .expect("elif+else chain must parse");
        let (_c1, _t1, else1) = only_shell_if(&module);
        // else1 = [ShellIf{C2, .., else:[echo other]}]
        let Stmt::ShellIf { else_body: e2, .. } = &else1[0] else {
            panic!("expected nested elif ShellIf");
        };
        assert_eq!(
            e2,
            &vec![Stmt::Cmd {
                program: "echo".to_string(),
                args: vec![Expr::LitStr("other".to_string())],
            }],
            "the final else lands in the innermost else_body"
        );
    }

    #[test]
    fn parse_and_lower_refuses_elif_without_if() {
        // A stray `elif` with no preceding `if` is refused, never shred.
        let err = BashrsFrontend
            .parse_and_lower(
                &PathBuf::from("/tmp/strayelif.sh"),
                "elif [ $x -eq 1 ]; then echo hi; fi\n",
            )
            .expect_err("stray elif must be REFUSED");
        assert!(format!("{err:?}").contains("elif"));
    }

    #[test]
    fn parse_and_lower_single_line_while_loop() {
        // PMAT-1276: `while COND; do … done` now PARSES into a
        // `LoopKind::While`. The condition is captured verbatim as an
        // opaque `Expr::LitStr` (the IR's v0.1.0 posture) — here
        // `[ -f /tmp/x ]`, which round-trips through the backend.
        let module = BashrsFrontend
            .parse_and_lower(
                &PathBuf::from("/tmp/w.sh"),
                "while [ -f /tmp/x ]; do echo hi; done\n",
            )
            .expect("single-line while-loop must parse into a ShellLoop");
        let (kind, body) = only_shell_loop(&module);
        assert_eq!(
            kind,
            &LoopKind::While {
                cond: Expr::LitStr("[ -f /tmp/x ]".to_string()),
            }
        );
        assert_eq!(
            body,
            &[Stmt::Cmd {
                program: "echo".to_string(),
                args: vec![Expr::LitStr("hi".to_string())],
            }]
        );
    }

    #[test]
    fn parse_and_lower_multi_line_while_loop_do_on_own_line() {
        // The `while` condition holds a `$VAR` ref; it stays LITERAL in
        // the opaque LitStr (the shell expands it at run time), so the
        // whole test predicate round-trips byte-for-byte.
        let source = "\
while [ $i -lt 3 ]
do
  echo $i
done
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/w2.sh"), source)
            .expect("multi-line while-loop must parse");
        let (kind, body) = only_shell_loop(&module);
        assert_eq!(
            kind,
            &LoopKind::While {
                cond: Expr::LitStr("[ $i -lt 3 ]".to_string()),
            }
        );
        assert_eq!(body.len(), 1);
        assert_eq!(
            body[0],
            Stmt::Cmd {
                program: "echo".to_string(),
                args: vec![Expr::ShellVar("i".to_string())],
            }
        );
    }

    #[test]
    fn parse_and_lower_until_loop() {
        // `until COND; do … done` — POSIX's inverted while — parses to
        // `LoopKind::Until` with the same opaque-condition treatment.
        let module = BashrsFrontend
            .parse_and_lower(
                &PathBuf::from("/tmp/u.sh"),
                "until [ -e /tmp/done ]; do echo waiting; done\n",
            )
            .expect("until-loop must parse into a ShellLoop");
        let (kind, _) = only_shell_loop(&module);
        assert_eq!(
            kind,
            &LoopKind::Until {
                cond: Expr::LitStr("[ -e /tmp/done ]".to_string()),
            }
        );
    }

    #[test]
    fn parse_and_lower_while_loop_composes_with_flat_commands() {
        // A `while` loop between an assignment and a trailing command —
        // order preserved, condition opaque.
        let source = "\
i=0
while [ $i -lt 2 ]; do
  echo $i
  i=$((i+1))
done
echo after
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/w3.sh"), source)
            .expect("while composed with flat commands must parse");
        let Item::Function(f) = &module.items[0] else {
            unreachable!("no module constants")
        };
        assert_eq!(f.body.stmts.len(), 3, "assign + while + trailing cmd");
        assert!(matches!(f.body.stmts[0], Stmt::ShellAssign { .. }));
        assert!(matches!(f.body.stmts[1], Stmt::ShellLoop { .. }));
        assert!(matches!(f.body.stmts[2], Stmt::Cmd { .. }));
    }

    #[test]
    fn parse_and_lower_refuses_empty_while_condition() {
        // `while ; do … done` has no condition — refuse rather than
        // build a degenerate loop.
        let err = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/we.sh"), "while ; do echo hi; done\n")
            .expect_err("empty while condition must be REFUSED");
        assert!(format!("{err:?}").contains("empty condition"));
    }

    #[test]
    fn parse_and_lower_nested_while_in_for_body() {
        // PMAT-1281: a `while` nested inside a `for` body now parses —
        // mixed loop dialects nest just as same-dialect ones do. The
        // inner while's opaque condition round-trips as a LitStr.
        let source = "\
for i in 1 2; do
  while [ $i -gt 0 ]; do
    echo $i
  done
done
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/nw.sh"), source)
            .expect("nested while-in-for must parse");
        let (_outer_kind, outer_body) = only_shell_loop(&module);
        assert_eq!(outer_body.len(), 1);
        let Stmt::ShellLoop {
            kind: inner_kind, ..
        } = &outer_body[0]
        else {
            panic!("expected nested ShellLoop, got {:?}", outer_body[0]);
        };
        assert_eq!(
            inner_kind,
            &LoopKind::While {
                cond: Expr::LitStr("[ $i -gt 0 ]".to_string()),
            }
        );
    }

    #[test]
    fn parse_and_lower_case_esac() {
        // PMAT-1285: `case WORD in PAT) BODY ;; … esac` now PARSES into
        // a `Stmt::ShellCase`. Multi-pattern arms (`b|c`) and the `*`
        // default are supported. Was refused pre-1285.
        use xpile_meta_hir::CaseArm;
        let source = "\
case $x in
  a) echo aye ;;
  b|c) echo bee-or-cee ;;
  *) echo other ;;
esac
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/c.sh"), source)
            .expect("case must parse");
        assert_eq!(module.items.len(), 1);
        let Item::Function(f) = &module.items[0] else {
            unreachable!("no module constants")
        };
        assert_eq!(f.body.stmts.len(), 1);
        let Stmt::ShellCase { word, arms } = &f.body.stmts[0] else {
            panic!("expected Stmt::ShellCase, got {:?}", f.body.stmts[0]);
        };
        assert_eq!(word, &Expr::ShellVar("x".to_string()));
        assert_eq!(arms.len(), 3);
        assert_eq!(
            arms[0],
            CaseArm {
                patterns: vec!["a".to_string()],
                body: vec![Stmt::Cmd {
                    program: "echo".to_string(),
                    args: vec![Expr::LitStr("aye".to_string())],
                }],
            }
        );
        // Multi-pattern arm `b|c`.
        assert_eq!(arms[1].patterns, vec!["b".to_string(), "c".to_string()]);
        // Default `*` arm.
        assert_eq!(arms[2].patterns, vec!["*".to_string()]);
    }

    #[test]
    fn parse_and_lower_single_line_case() {
        let module = BashrsFrontend
            .parse_and_lower(
                &PathBuf::from("/tmp/c1.sh"),
                "case $x in a) echo aye ;; *) echo other ;; esac\n",
            )
            .expect("single-line case must parse");
        let Item::Function(f) = &module.items[0] else {
            unreachable!()
        };
        let Stmt::ShellCase { arms, .. } = &f.body.stmts[0] else {
            panic!("expected ShellCase");
        };
        assert_eq!(arms.len(), 2);
    }

    #[test]
    fn parse_and_lower_case_arm_with_nested_loop() {
        // An arm body may contain a nested loop (parsed recursively).
        let source = "\
case $y in
  go)
    for i in 1 2; do
      echo $i
    done
    ;;
  *) echo nope ;;
esac
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/c2.sh"), source)
            .expect("case with nested-loop arm must parse");
        let Item::Function(f) = &module.items[0] else {
            unreachable!()
        };
        let Stmt::ShellCase { arms, .. } = &f.body.stmts[0] else {
            panic!("expected ShellCase");
        };
        assert_eq!(arms.len(), 2);
        assert_eq!(arms[0].patterns, vec!["go".to_string()]);
        assert!(
            matches!(arms[0].body.as_slice(), [Stmt::ShellLoop { .. }]),
            "first arm body must be the nested loop, got {:?}",
            arms[0].body
        );
    }

    #[test]
    fn parse_and_lower_refuses_unterminated_case() {
        let err = BashrsFrontend
            .parse_and_lower(
                &PathBuf::from("/tmp/uc.sh"),
                "case $x in\n  a) echo aye ;;\n",
            )
            .expect_err("unterminated case must be REFUSED");
        assert!(format!("{err:?}").contains("unterminated"));
    }

    // ---- PMAT-1371: constructs that used to be SHREDDED now REFUSE ----
    //
    // Every one of the five negatives below exited 0 before this slice and
    // produced output that either failed `bash -n` or — worse, for the
    // here-doc pair — passed `bash -n` while executing DIFFERENTLY from the
    // source. The two positives pin the boundary: both were verified working
    // before the guards landed and must stay working, because an
    // over-broad guard here is a capability regression dressed as honesty.

    #[test]
    fn parse_and_lower_refuses_case_semi_amp_fallthrough() {
        // Was: exit 0 emitting a bare `&` inside arm `a`'s body with arm `b`
        // swallowed into it; `bash -n` on the output → rc=2.
        let source = "\
case \"$x\" in
 a) echo A ;&
 b) echo B ;;
esac
";
        let err = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/fa.sh"), source)
            .expect_err("`;&` fall-through must be REFUSED, not shredded");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("fall-through") && msg.contains(";&"),
            "refusal must name the fall-through operator, got: {msg}"
        );
    }

    #[test]
    fn parse_and_lower_refuses_case_semi_semi_amp_fallthrough() {
        // `;;&` takes a DIFFERENT shred path from `;&` — the `;;` split
        // consumes its own separator and leaves the stray `&` glued to the
        // next arm's PATTERN slot — so it needs its own witness. The guard
        // must also report the `;;&` spelling, not `;&`.
        let source = "\
case \"$x\" in
 a) echo A ;;&
 b) echo B ;;
esac
";
        let err = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/fb.sh"), source)
            .expect_err("`;;&` fall-through must be REFUSED, not shredded");
        let msg = format!("{err:?}");
        assert!(
            msg.contains(";;&"),
            "refusal must name the `;;&` spelling specifically, got: {msg}"
        );
    }

    #[test]
    fn parse_and_lower_refuses_heredoc() {
        // THE WORST SHAPE: this used to exit 0 and emit a script that
        // passes `bash -n` but whose here-doc body had "  keep  me"
        // collapsed to "keep me" and the blank line deleted. A silent
        // semantic divergence no downstream check catches.
        let source = "\
cat <<EOF
  keep  me

after blank
EOF
";
        let err = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/hd.sh"), source)
            .expect_err("here-doc must be REFUSED, not silently reflowed");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("here-document"),
            "refusal must name the here-document, got: {msg}"
        );
    }

    #[test]
    fn parse_and_lower_refuses_heredoc_in_loop_body() {
        // The nested path reaches `lower_flat_line` through
        // `parse_segment_seq` rather than the top-level walk, so it is a
        // genuinely distinct route. Before the guard, the backend's
        // `indent_body` tab-prefixed the `EOF` terminator, producing
        // "here-document delimited by end-of-file" — a different failure
        // from the top-level case, which is why both are witnessed.
        let source = "\
for i in a b; do
 cat <<EOF
 hi
EOF
done
";
        let err = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/hdl.sh"), source)
            .expect_err("here-doc in a loop body must be REFUSED");
        assert!(format!("{err:?}").contains("here-document"));
    }

    #[test]
    fn parse_and_lower_refuses_bare_ampersand() {
        // Was: emitted verbatim → `bash -n` rc=2.
        let source = "echo a\n&\necho b\n";
        let err = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/amp.sh"), source)
            .expect_err("a bare `&` in command position must be REFUSED");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("command position"),
            "refusal must name the command-position boundary, got: {msg}"
        );
    }

    #[test]
    fn parse_and_lower_case_arm_trailing_background_ampersand_still_ok() {
        // POSITIVE non-regression. A TRAILING `&` is POSIX
        // background-execution and round-trips correctly; only `&` in
        // COMMAND position is the fall-through shred. Refusing `&`
        // generally would have broken this working construct.
        use xpile_meta_hir::Item;
        let source = "\
case \"$x\" in
 a) sleep 0 & ;;
 *) echo d ;;
esac
";
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/bg.sh"), source)
            .expect("`cmd &` background execution must still LOWER");
        assert!(
            matches!(module.items.first(), Some(Item::Function(_))),
            "background-`&` case must lower to a function, got: {:?}",
            module.items
        );
    }

    #[test]
    fn parse_and_lower_double_quoted_heredoc_operator_still_ok() {
        // POSITIVE non-regression, and the reason the here-doc guard is
        // TOKEN-level: `<<` inside a double-quoted string is ordinary text
        // and round-trips today. A `line.contains("<<")` guard would
        // regress it — the exact inverse of the `;&` guard, where naive
        // matching is safe.
        use xpile_meta_hir::Item;
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/qq.sh"), "echo \"p << q\"\n")
            .expect("`<<` inside double quotes must still LOWER");
        assert!(
            matches!(module.items.first(), Some(Item::Function(_))),
            "quoted `<<` must lower to a function, got: {:?}",
            module.items
        );
    }

    #[test]
    fn parse_and_lower_refuses_case_nested_in_loop() {
        // A `case` inside a loop body is out of slice-1 scope (the
        // `;`-segment split would mangle the arm `;;`). Refuse.
        let source = "\
for i in 1 2; do
  case $i in
    1) echo one ;;
  esac
done
";
        let err = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/cnl.sh"), source)
            .expect_err("case-in-loop must be REFUSED at this slice");
        let msg = format!("{err:?}");
        assert!(msg.contains("case"));
        // The refusal must state the TRUE boundary — top-level `case` IS
        // supported (PMAT-1285); a flat "not supported" would de-lie the
        // shipped capability (PMAT-1287 skeptic finding).
        assert!(
            msg.contains("top-level only"),
            "case-in-loop refusal must name the real boundary, got: {msg}"
        );
    }

    #[test]
    fn control_flow_keywords_only_fire_in_command_position() {
        // Guard against over-rejection: control-flow keywords used as
        // ARGUMENTS (not at a command position) must still parse as a
        // flat command. `grep -w done file` and `echo if then` are
        // ordinary commands whose args merely happen to spell
        // keywords; they must NOT be refused.
        use xpile_meta_hir::Item;
        for ok in &["grep -w done file\n", "echo if then else\n", "ls for\n"] {
            let module = BashrsFrontend
                .parse_and_lower(&PathBuf::from("/tmp/ok.sh"), ok)
                .unwrap_or_else(|e| {
                    panic!("flat command with keyword-as-arg must parse: {ok:?} -> {e:?}")
                });
            let Item::Function(f) = &module.items[0] else {
                unreachable!("test fixture has no module constants")
            };
            assert_eq!(
                f.body.stmts.len(),
                1,
                "keyword-as-argument line `{ok}` should be one flat Cmd"
            );
        }
    }

    #[test]
    fn flat_command_subset_still_parses_after_pmat_989() {
        // Regression: the control-flow refusal MUST NOT disturb the
        // existing flat-command subset. A plain script still lowers
        // to one Cmd per line.
        use xpile_meta_hir::Item;
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/flat.sh"), "echo hello\nls /tmp\npwd\n")
            .expect("flat commands must still parse");
        let Item::Function(f) = &module.items[0] else {
            unreachable!("test fixture has no module constants")
        };
        assert_eq!(f.body.stmts.len(), 3);
    }
}
