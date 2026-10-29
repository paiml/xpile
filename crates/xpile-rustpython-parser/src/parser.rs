//! Contains the interface to the Python parser.
//!
//! Functions in this module can be used to parse Python code into an [Abstract Syntax Tree]
//! (AST) that is then transformed into bytecode.
//!
//! There are three ways to parse Python code corresponding to the different [`Mode`]s
//! defined in the [`mode`] module.
//!
//! All functions return a [`Result`](std::result::Result) containing the parsed AST or
//! a [`ParseError`] if parsing failed.
//!
//! [Abstract Syntax Tree]: https://en.wikipedia.org/wiki/Abstract_syntax_tree
//! [`Mode`]: crate::mode

use crate::{
    ast::{self, OptionalRange, Ranged},
    lexer::{self, LexResult, LexicalError, LexicalErrorType},
    python,
    text_size::TextSize,
    token::Tok,
    Mode,
};
use itertools::Itertools;
use std::iter;

use crate::{lexer::Lexer, soft_keywords::SoftKeywordTransformer, text_size::TextRange};
pub(super) use lalrpop_util::ParseError as LalrpopError;

/// Parse Python code string to implementor's type.
///
/// # Example
///
/// For example, parsing a simple function definition and a call to that function:
///
/// ```
/// use rustpython_parser::{self as parser, ast, Parse};
/// let source = r#"
/// def foo():
///    return 42
///
/// print(foo())
/// "#;
/// let program = ast::Suite::parse(source, "<embedded>");
/// assert!(program.is_ok());
/// ```
///
/// Parsing a single expression denoting the addition of two numbers, but this time specifying a different,
/// somewhat silly, location:
///
/// ```
/// use rustpython_parser::{self as parser, ast, Parse, text_size::TextSize};
///
/// let expr = ast::Expr::parse_starts_at("1 + 2", "<embedded>", TextSize::from(400));
/// assert!(expr.is_ok());
pub trait Parse
where
    Self: Sized,
{
    fn parse(source: &str, source_path: &str) -> Result<Self, ParseError> {
        Self::parse_starts_at(source, source_path, TextSize::default())
    }
    fn parse_without_path(source: &str) -> Result<Self, ParseError> {
        Self::parse(source, "<unknown>")
    }
    fn parse_starts_at(
        source: &str,
        source_path: &str,
        offset: TextSize,
    ) -> Result<Self, ParseError> {
        let lxr = Self::lex_starts_at(source, offset);
        #[cfg(feature = "full-lexer")]
        let lxr =
            lxr.filter_ok(|(tok, _)| !matches!(tok, Tok::Comment { .. } | Tok::NonLogicalNewline));
        Self::parse_tokens(lxr, source_path)
    }
    fn lex_starts_at(
        source: &str,
        offset: TextSize,
    ) -> SoftKeywordTransformer<Lexer<std::str::Chars>>;
    fn parse_tokens(
        lxr: impl IntoIterator<Item = LexResult>,
        source_path: &str,
    ) -> Result<Self, ParseError>;
}

impl Parse for ast::ModModule {
    fn lex_starts_at(
        source: &str,
        offset: TextSize,
    ) -> SoftKeywordTransformer<Lexer<std::str::Chars>> {
        lexer::lex_starts_at(source, Mode::Module, offset)
    }
    fn parse_tokens(
        lxr: impl IntoIterator<Item = LexResult>,
        source_path: &str,
    ) -> Result<Self, ParseError> {
        match parse_filtered_tokens(lxr, Mode::Module, source_path)? {
            ast::Mod::Module(m) => Ok(m),
            _ => unreachable!("Mode::Module doesn't return other variant"),
        }
    }
}

impl Parse for ast::ModExpression {
    fn lex_starts_at(
        source: &str,
        offset: TextSize,
    ) -> SoftKeywordTransformer<Lexer<std::str::Chars>> {
        lexer::lex_starts_at(source, Mode::Expression, offset)
    }
    fn parse_tokens(
        lxr: impl IntoIterator<Item = LexResult>,
        source_path: &str,
    ) -> Result<Self, ParseError> {
        match parse_filtered_tokens(lxr, Mode::Expression, source_path)? {
            ast::Mod::Expression(m) => Ok(m),
            _ => unreachable!("Mode::Module doesn't return other variant"),
        }
    }
}

impl Parse for ast::ModInteractive {
    fn lex_starts_at(
        source: &str,
        offset: TextSize,
    ) -> SoftKeywordTransformer<Lexer<std::str::Chars>> {
        lexer::lex_starts_at(source, Mode::Interactive, offset)
    }
    fn parse_tokens(
        lxr: impl IntoIterator<Item = LexResult>,
        source_path: &str,
    ) -> Result<Self, ParseError> {
        match parse_filtered_tokens(lxr, Mode::Interactive, source_path)? {
            ast::Mod::Interactive(m) => Ok(m),
            _ => unreachable!("Mode::Module doesn't return other variant"),
        }
    }
}

impl Parse for ast::Suite {
    fn lex_starts_at(
        source: &str,
        offset: TextSize,
    ) -> SoftKeywordTransformer<Lexer<std::str::Chars>> {
        ast::ModModule::lex_starts_at(source, offset)
    }
    fn parse_tokens(
        lxr: impl IntoIterator<Item = LexResult>,
        source_path: &str,
    ) -> Result<Self, ParseError> {
        Ok(ast::ModModule::parse_tokens(lxr, source_path)?.body)
    }
}

impl Parse for ast::Stmt {
    fn lex_starts_at(
        source: &str,
        offset: TextSize,
    ) -> SoftKeywordTransformer<Lexer<std::str::Chars>> {
        ast::ModModule::lex_starts_at(source, offset)
    }
    fn parse_tokens(
        lxr: impl IntoIterator<Item = LexResult>,
        source_path: &str,
    ) -> Result<Self, ParseError> {
        let mut statements = ast::ModModule::parse_tokens(lxr, source_path)?.body;
        let statement = match statements.len() {
            0 => {
                return Err(ParseError {
                    error: ParseErrorType::Eof,
                    offset: TextSize::default(),
                    source_path: source_path.to_owned(),
                })
            }
            1 => statements.pop().unwrap(),
            _ => {
                return Err(ParseError {
                    error: ParseErrorType::InvalidToken,
                    offset: statements[1].range().start(),
                    source_path: source_path.to_owned(),
                })
            }
        };
        Ok(statement)
    }
}

impl Parse for ast::Expr {
    fn lex_starts_at(
        source: &str,
        offset: TextSize,
    ) -> SoftKeywordTransformer<Lexer<std::str::Chars>> {
        ast::ModExpression::lex_starts_at(source, offset)
    }
    fn parse_tokens(
        lxr: impl IntoIterator<Item = LexResult>,
        source_path: &str,
    ) -> Result<Self, ParseError> {
        Ok(*ast::ModExpression::parse_tokens(lxr, source_path)?.body)
    }
}

impl Parse for ast::Identifier {
    fn lex_starts_at(
        source: &str,
        offset: TextSize,
    ) -> SoftKeywordTransformer<Lexer<std::str::Chars>> {
        ast::Expr::lex_starts_at(source, offset)
    }
    fn parse_tokens(
        lxr: impl IntoIterator<Item = LexResult>,
        source_path: &str,
    ) -> Result<Self, ParseError> {
        let expr = ast::Expr::parse_tokens(lxr, source_path)?;
        match expr {
            ast::Expr::Name(name) => Ok(name.id),
            expr => Err(ParseError {
                error: ParseErrorType::InvalidToken,
                offset: expr.range().start(),
                source_path: source_path.to_owned(),
            }),
        }
    }
}

impl Parse for ast::Constant {
    fn lex_starts_at(
        source: &str,
        offset: TextSize,
    ) -> SoftKeywordTransformer<Lexer<std::str::Chars>> {
        ast::Expr::lex_starts_at(source, offset)
    }
    fn parse_tokens(
        lxr: impl IntoIterator<Item = LexResult>,
        source_path: &str,
    ) -> Result<Self, ParseError> {
        let expr = ast::Expr::parse_tokens(lxr, source_path)?;
        match expr {
            ast::Expr::Constant(c) => Ok(c.value),
            expr => Err(ParseError {
                error: ParseErrorType::InvalidToken,
                offset: expr.range().start(),
                source_path: source_path.to_owned(),
            }),
        }
    }
}

/// Parse a full Python program usually consisting of multiple lines.
///  
/// This is a convenience function that can be used to parse a full Python program without having to
/// specify the [`Mode`] or the location. It is probably what you want to use most of the time.
///
/// # Example
///
/// For example, parsing a simple function definition and a call to that function:
///
/// ```
/// use rustpython_parser as parser;
/// let source = r#"
/// def foo():
///    return 42
///
/// print(foo())
/// "#;
/// let program = parser::parse_program(source, "<embedded>");
/// assert!(program.is_ok());
/// ```
#[deprecated = "Use ast::Suite::parse from rustpython_parser::Parse trait."]
pub fn parse_program(source: &str, source_path: &str) -> Result<ast::Suite, ParseError> {
    parse(source, Mode::Module, source_path).map(|top| match top {
        ast::Mod::Module(ast::ModModule { body, .. }) => body,
        _ => unreachable!(),
    })
}

/// Parses a single Python expression.
///
/// This convenience function can be used to parse a single expression without having to
/// specify the Mode or the location.
///
/// # Example
///
/// For example, parsing a single expression denoting the addition of two numbers:
///
///  ```
/// use rustpython_parser as parser;
/// let expr = parser::parse_expression("1 + 2", "<embedded>");
///
/// assert!(expr.is_ok());
///
/// ```
#[deprecated = "Use ast::Expr::parse from rustpython_parser::Parse trait."]
pub fn parse_expression(source: &str, path: &str) -> Result<ast::Expr, ParseError> {
    ast::Expr::parse(source, path)
}

/// Parses a Python expression from a given location.
///
/// This function allows to specify the location of the expression in the source code, other than
/// that, it behaves exactly like [`parse_expression`].
///
/// # Example
///
/// Parsing a single expression denoting the addition of two numbers, but this time specifying a different,
/// somewhat silly, location:
///
/// ```
/// use rustpython_parser::{text_size::TextSize, parse_expression_starts_at};
///
/// let expr = parse_expression_starts_at("1 + 2", "<embedded>", TextSize::from(400));
/// assert!(expr.is_ok());
/// ```
#[deprecated = "Use ast::Expr::parse_starts_at from rustpython_parser::Parse trait."]
pub fn parse_expression_starts_at(
    source: &str,
    path: &str,
    offset: TextSize,
) -> Result<ast::Expr, ParseError> {
    ast::Expr::parse_starts_at(source, path, offset)
}

/// Parse the given Python source code using the specified [`Mode`].
///
/// This function is the most general function to parse Python code. Based on the [`Mode`] supplied,
/// it can be used to parse a single expression, a full Python program or an interactive expression.
///
/// # Example
///
/// If we want to parse a simple expression, we can use the [`Mode::Expression`] mode during
/// parsing:
///
/// ```
/// use rustpython_parser::{Mode, parse};
///
/// let expr = parse("1 + 2", Mode::Expression, "<embedded>");
/// assert!(expr.is_ok());
/// ```
///
/// Alternatively, we can parse a full Python program consisting of multiple lines:
///
/// ```
/// use rustpython_parser::{Mode, parse};
///
/// let source = r#"
/// class Greeter:
///
///   def greet(self):
///    print("Hello, world!")
/// "#;
/// let program = parse(source, Mode::Module, "<embedded>");
/// assert!(program.is_ok());
/// ```
pub fn parse(source: &str, mode: Mode, source_path: &str) -> Result<ast::Mod, ParseError> {
    parse_starts_at(source, mode, source_path, TextSize::default())
}

/// Parse the given Python source code using the specified [`Mode`] and [`Location`].
///
/// This function allows to specify the location of the the source code, other than
/// that, it behaves exactly like [`parse`].
///
/// # Example
///
/// ```
/// use rustpython_parser::{text_size::TextSize, Mode, parse_starts_at};
///
/// let source = r#"
/// def fib(i):
///    a, b = 0, 1
///    for _ in range(i):
///       a, b = b, a + b
///    return a
///
/// print(fib(42))
/// "#;
/// let program = parse_starts_at(source, Mode::Module, "<embedded>", TextSize::from(0));
/// assert!(program.is_ok());
/// ```
pub fn parse_starts_at(
    source: &str,
    mode: Mode,
    source_path: &str,
    offset: TextSize,
) -> Result<ast::Mod, ParseError> {
    let lxr = lexer::lex_starts_at(source, mode, offset);
    parse_tokens(lxr, mode, source_path)
}

/// Parse an iterator of [`LexResult`]s using the specified [`Mode`].
///
/// This could allow you to perform some preprocessing on the tokens before parsing them.
///
/// # Example
///
/// As an example, instead of parsing a string, we can parse a list of tokens after we generate
/// them using the [`lexer::lex`] function:
///
/// ```
/// use rustpython_parser::{lexer::lex, Mode, parse_tokens};
///
/// let expr = parse_tokens(lex("1 + 2", Mode::Expression), Mode::Expression, "<embedded>");
/// assert!(expr.is_ok());
/// ```
pub fn parse_tokens(
    lxr: impl IntoIterator<Item = LexResult>,
    mode: Mode,
    source_path: &str,
) -> Result<ast::Mod, ParseError> {
    let lxr = lxr.into_iter();
    #[cfg(feature = "full-lexer")]
    let lxr =
        lxr.filter_ok(|(tok, _)| !matches!(tok, Tok::Comment { .. } | Tok::NonLogicalNewline));
    parse_filtered_tokens(lxr, mode, source_path)
}

fn parse_filtered_tokens(
    lxr: impl IntoIterator<Item = LexResult>,
    mode: Mode,
    source_path: &str,
) -> Result<ast::Mod, ParseError> {
    let marker_token = (Tok::start_marker(mode), Default::default());
    let lexer = iter::once(Ok(marker_token)).chain(lxr);
    python::TopParser::new()
        .parse(
            lexer
                .into_iter()
                .map_ok(|(t, range)| (range.start(), t, range.end())),
        )
        .map_err(|e| parse_error_from_lalrpop(e, source_path))
}

/// Represents represent errors that occur during parsing and are
/// returned by the `parse_*` functions.
pub type ParseError = rustpython_parser_core::BaseError<ParseErrorType>;

/// Represents the different types of errors that can occur during parsing.
#[derive(Debug, PartialEq)]
pub enum ParseErrorType {
    /// Parser encountered an unexpected end of input
    Eof,
    /// Parser encountered an extra token
    ExtraToken(Tok),
    /// Parser encountered an invalid token
    InvalidToken,
    /// Parser encountered an unexpected token
    UnrecognizedToken(Tok, Option<String>),
    // Maps to `User` type from `lalrpop-util`
    /// Parser encountered an error during lexing.
    Lexical(LexicalErrorType),
}

impl std::error::Error for ParseErrorType {}

// Convert `lalrpop_util::ParseError` to our internal type
fn parse_error_from_lalrpop(
    err: LalrpopError<TextSize, Tok, LexicalError>,
    source_path: &str,
) -> ParseError {
    let source_path = source_path.to_owned();

    match err {
        // TODO: Are there cases where this isn't an EOF?
        LalrpopError::InvalidToken { location } => ParseError {
            error: ParseErrorType::Eof,
            offset: location,
            source_path,
        },
        LalrpopError::ExtraToken { token } => ParseError {
            error: ParseErrorType::ExtraToken(token.1),
            offset: token.0,
            source_path,
        },
        LalrpopError::User { error } => ParseError {
            error: ParseErrorType::Lexical(error.error),
            offset: error.location,
            source_path,
        },
        LalrpopError::UnrecognizedToken { token, expected } => {
            // Hacky, but it's how CPython does it. See PyParser_AddToken,
            // in particular "Only one possible expected token" comment.
            let expected = (expected.len() == 1).then(|| expected[0].clone());
            ParseError {
                error: ParseErrorType::UnrecognizedToken(token.1, expected),
                offset: token.0,
                source_path,
            }
        }
        LalrpopError::UnrecognizedEof { location, expected } => {
            // This could be an initial indentation error that we should ignore
            let indent_error = expected == ["Indent"];
            if indent_error {
                ParseError {
                    error: ParseErrorType::Lexical(LexicalErrorType::IndentationError),
                    offset: location,
                    source_path,
                }
            } else {
                ParseError {
                    error: ParseErrorType::Eof,
                    offset: location,
                    source_path,
                }
            }
        }
    }
}

impl std::fmt::Display for ParseErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            ParseErrorType::Eof => write!(f, "Got unexpected EOF"),
            ParseErrorType::ExtraToken(ref tok) => write!(f, "Got extraneous token: {tok:?}"),
            ParseErrorType::InvalidToken => write!(f, "Got invalid token"),
            ParseErrorType::UnrecognizedToken(ref tok, ref expected) => {
                if *tok == Tok::Indent {
                    write!(f, "unexpected indent")
                } else if expected.as_deref() == Some("Indent") {
                    write!(f, "expected an indented block")
                } else {
                    write!(f, "invalid syntax. Got unexpected token {tok}")
                }
            }
            ParseErrorType::Lexical(ref error) => write!(f, "{error}"),
        }
    }
}

impl ParseErrorType {
    /// Returns true if the error is an indentation error.
    pub fn is_indentation_error(&self) -> bool {
        match self {
            ParseErrorType::Lexical(LexicalErrorType::IndentationError) => true,
            ParseErrorType::UnrecognizedToken(token, expected) => {
                *token == Tok::Indent || expected.clone() == Some("Indent".to_owned())
            }
            _ => false,
        }
    }

    /// Returns true if the error is a tab error.
    pub fn is_tab_error(&self) -> bool {
        matches!(
            self,
            ParseErrorType::Lexical(LexicalErrorType::TabError)
                | ParseErrorType::Lexical(LexicalErrorType::TabsAfterSpaces)
        )
    }
}

#[inline(always)]
pub(super) fn optional_range(start: TextSize, end: TextSize) -> OptionalRange<TextRange> {
    OptionalRange::<TextRange>::new(start, end)
}

include!("gen/parse.rs");
