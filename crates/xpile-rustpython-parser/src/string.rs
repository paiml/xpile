// Contains the logic for parsing string literals (mostly concerned with f-strings.)
//
// The lexer doesn't do any special handling of f-strings, it just treats them as
// regular strings. Since the parser has no definition of f-string formats (Pending PEP 701)
// we have to do the parsing here, manually.
use crate::text_size::TextRange;
use crate::{
    ast::{self, Constant, Expr},
    lexer::{LexicalError, LexicalErrorType},
    parser::{LalrpopError, Parse, ParseError, ParseErrorType},
    token::{StringKind, Tok},
};
use itertools::Itertools;
use rustpython_parser_core::{
    text_size::{TextLen, TextSize},
    ConversionFlag,
};

// unicode_name2 does not expose `MAX_NAME_LENGTH`, so we replicate that constant here, fix #3798
const MAX_UNICODE_NAME: usize = 88;

struct StringParser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    kind: StringKind,
    start: TextSize,
    end: TextSize,
    location: TextSize,
}

impl<'a> StringParser<'a> {
    fn new(
        source: &'a str,
        kind: StringKind,
        triple_quoted: bool,
        start: TextSize,
        end: TextSize,
    ) -> Self {
        let offset = kind.prefix_len()
            + if triple_quoted {
                TextSize::from(3)
            } else {
                TextSize::from(1)
            };
        Self {
            chars: source.chars().peekable(),
            kind,
            start,
            end,
            location: start + offset,
        }
    }

    #[inline]
    fn next_char(&mut self) -> Option<char> {
        let c = self.chars.next()?;
        self.location += c.text_len();
        Some(c)
    }

    #[inline]
    fn peek(&mut self) -> Option<&char> {
        self.chars.peek()
    }

    #[inline]
    fn get_pos(&self) -> TextSize {
        self.location
    }

    #[inline]
    fn expr(&self, node: Expr) -> Expr {
        node
    }

    fn range(&self) -> TextRange {
        TextRange::new(self.start, self.end)
    }

    fn parse_unicode_literal(&mut self, literal_number: usize) -> Result<char, LexicalError> {
        let mut p: u32 = 0u32;
        let unicode_error = LexicalError::new(LexicalErrorType::UnicodeError, self.get_pos());
        for i in 1..=literal_number {
            match self.next_char() {
                Some(c) => match c.to_digit(16) {
                    Some(d) => p += d << ((literal_number - i) * 4),
                    None => return Err(unicode_error),
                },
                None => return Err(unicode_error),
            }
        }
        match p {
            0xD800..=0xDFFF => Ok(std::char::REPLACEMENT_CHARACTER),
            _ => std::char::from_u32(p).ok_or(unicode_error),
        }
    }

    fn parse_octet(&mut self, first: char) -> char {
        let mut octet_content = String::new();
        octet_content.push(first);
        while octet_content.len() < 3 {
            if let Some('0'..='7') = self.peek() {
                octet_content.push(self.next_char().unwrap())
            } else {
                break;
            }
        }
        let value = u32::from_str_radix(&octet_content, 8).unwrap();
        char::from_u32(value).unwrap()
    }

    fn parse_unicode_name(&mut self) -> Result<char, LexicalError> {
        let start_pos = self.get_pos();
        match self.next_char() {
            Some('{') => {}
            _ => return Err(LexicalError::new(LexicalErrorType::StringError, start_pos)),
        }
        let start_pos = self.get_pos();
        let mut name = String::new();
        loop {
            match self.next_char() {
                Some('}') => break,
                Some(c) => name.push(c),
                None => {
                    return Err(LexicalError::new(
                        LexicalErrorType::StringError,
                        self.get_pos(),
                    ))
                }
            }
        }

        if name.len() > MAX_UNICODE_NAME {
            return Err(LexicalError::new(
                LexicalErrorType::UnicodeError,
                self.get_pos(),
            ));
        }

        unicode_names2::character(&name)
            .ok_or_else(|| LexicalError::new(LexicalErrorType::UnicodeError, start_pos))
    }

    fn parse_escaped_char(&mut self) -> Result<String, LexicalError> {
        match self.next_char() {
            Some(c) => {
                let char = match c {
                    '\\' => '\\',
                    '\'' => '\'',
                    '\"' => '"',
                    'a' => '\x07',
                    'b' => '\x08',
                    'f' => '\x0c',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'v' => '\x0b',
                    o @ '0'..='7' => self.parse_octet(o),
                    'x' => self.parse_unicode_literal(2)?,
                    'u' if !self.kind.is_any_bytes() => self.parse_unicode_literal(4)?,
                    'U' if !self.kind.is_any_bytes() => self.parse_unicode_literal(8)?,
                    'N' if !self.kind.is_any_bytes() => self.parse_unicode_name()?,
                    // Special cases where the escape sequence is not a single character
                    '\n' => return Ok("".to_string()),
                    c => {
                        if self.kind.is_any_bytes() && !c.is_ascii() {
                            return Err(LexicalError {
                                error: LexicalErrorType::OtherError(
                                    "bytes can only contain ASCII literal characters".to_owned(),
                                ),
                                location: self.get_pos(),
                            });
                        }
                        return Ok(format!("\\{c}"));
                    }
                };
                Ok(char.to_string())
            }
            None => Err(LexicalError {
                error: LexicalErrorType::StringError,
                location: self.get_pos(),
            }),
        }
    }

    fn parse_formatted_value(&mut self, nested: u8) -> Result<Vec<Expr>, LexicalError> {
        use FStringErrorType::*;

        let mut expression = String::new();
        let mut spec = None;
        let mut delimiters = Vec::new();
        let mut conversion = ConversionFlag::None;
        let mut self_documenting = false;
        let mut trailing_seq = String::new();
        let location = self.get_pos();

        while let Some(ch) = self.next_char() {
            match ch {
                // can be integrated better with the remaining code, but as a starting point ok
                // in general I would do here a tokenizing of the fstrings to omit this peeking.
                '!' | '=' | '>' | '<' if self.peek() == Some(&'=') => {
                    expression.push(ch);
                    expression.push('=');
                    self.next_char();
                }
                '!' if delimiters.is_empty() && self.peek() != Some(&'=') => {
                    if expression.trim().is_empty() {
                        return Err(FStringError::new(EmptyExpression, self.get_pos()).into());
                    }

                    conversion = match self.next_char() {
                        Some('s') => ConversionFlag::Str,
                        Some('a') => ConversionFlag::Ascii,
                        Some('r') => ConversionFlag::Repr,
                        Some(_) => {
                            return Err(
                                FStringError::new(InvalidConversionFlag, self.get_pos()).into()
                            );
                        }
                        None => {
                            return Err(FStringError::new(UnclosedLbrace, self.get_pos()).into());
                        }
                    };

                    match self.peek() {
                        Some('}' | ':') => {}
                        Some(_) | None => {
                            return Err(FStringError::new(UnclosedLbrace, self.get_pos()).into());
                        }
                    }
                }

                // match a python 3.8 self documenting expression
                // format '{' PYTHON_EXPRESSION '=' FORMAT_SPECIFIER? '}'
                '=' if self.peek() != Some(&'=') && delimiters.is_empty() => {
                    self_documenting = true;
                }

                ':' if delimiters.is_empty() => {
                    let parsed_spec = self.parse_spec(nested)?;

                    spec = Some(Box::new(
                        self.expr(
                            ast::ExprJoinedStr {
                                values: parsed_spec,
                                range: self.range(),
                            }
                            .into(),
                        ),
                    ));
                }
                '(' | '{' | '[' => {
                    expression.push(ch);
                    delimiters.push(ch);
                }
                ')' => {
                    let last_delim = delimiters.pop();
                    match last_delim {
                        Some('(') => {
                            expression.push(ch);
                        }
                        Some(c) => {
                            return Err(FStringError::new(
                                MismatchedDelimiter(c, ')'),
                                self.get_pos(),
                            )
                            .into());
                        }
                        None => {
                            return Err(FStringError::new(Unmatched(')'), self.get_pos()).into());
                        }
                    }
                }
                ']' => {
                    let last_delim = delimiters.pop();
                    match last_delim {
                        Some('[') => {
                            expression.push(ch);
                        }
                        Some(c) => {
                            return Err(FStringError::new(
                                MismatchedDelimiter(c, ']'),
                                self.get_pos(),
                            )
                            .into());
                        }
                        None => {
                            return Err(FStringError::new(Unmatched(']'), self.get_pos()).into());
                        }
                    }
                }
                '}' if !delimiters.is_empty() => {
                    let last_delim = delimiters.pop();
                    match last_delim {
                        Some('{') => {
                            expression.push(ch);
                        }
                        Some(c) => {
                            return Err(FStringError::new(
                                MismatchedDelimiter(c, '}'),
                                self.get_pos(),
                            )
                            .into());
                        }
                        None => {}
                    }
                }
                '}' => {
                    if expression.trim().is_empty() {
                        return Err(FStringError::new(EmptyExpression, self.get_pos()).into());
                    }

                    let ret = if !self_documenting {
                        vec![self.expr(
                            ast::ExprFormattedValue {
                                value: Box::new(
                                    parse_fstring_expr(&expression, location).map_err(|e| {
                                        FStringError::new(
                                            InvalidExpression(Box::new(e.error)),
                                            location,
                                        )
                                    })?,
                                ),
                                conversion,
                                format_spec: spec,
                                range: self.range(),
                            }
                            .into(),
                        )]
                    } else {
                        vec![
                            self.expr(
                                ast::ExprConstant {
                                    value: Constant::Str(expression.to_owned() + "="),
                                    kind: None,
                                    range: self.range(),
                                }
                                .into(),
                            ),
                            self.expr(
                                ast::ExprConstant {
                                    value: trailing_seq.into(),
                                    kind: None,
                                    range: self.range(),
                                }
                                .into(),
                            ),
                            self.expr(
                                ast::ExprFormattedValue {
                                    value: Box::new(
                                        parse_fstring_expr(&expression, location).map_err(|e| {
                                            FStringError::new(
                                                InvalidExpression(Box::new(e.error)),
                                                location,
                                            )
                                        })?,
                                    ),
                                    conversion: if conversion == ConversionFlag::None
                                        && spec.is_none()
                                    {
                                        ConversionFlag::Repr
                                    } else {
                                        conversion
                                    },
                                    format_spec: spec,
                                    range: self.range(),
                                }
                                .into(),
                            ),
                        ]
                    };
                    return Ok(ret);
                }
                '"' | '\'' => {
                    expression.push(ch);
                    loop {
                        let Some(c) = self.next_char() else {
                            return Err(
                                FStringError::new(UnterminatedString, self.get_pos()).into()
                            );
                        };
                        expression.push(c);
                        if c == ch {
                            break;
                        }
                    }
                }
                ' ' if self_documenting => {
                    trailing_seq.push(ch);
                }
                '\\' => return Err(FStringError::new(UnterminatedString, self.get_pos()).into()),
                _ => {
                    if self_documenting {
                        return Err(FStringError::new(UnclosedLbrace, self.get_pos()).into());
                    }

                    expression.push(ch);
                }
            }
        }
        Err(FStringError::new(UnclosedLbrace, self.get_pos()).into())
    }

    fn parse_spec(&mut self, nested: u8) -> Result<Vec<Expr>, LexicalError> {
        let mut spec_constructor = Vec::new();
        let mut constant_piece = String::new();
        while let Some(&next) = self.peek() {
            match next {
                '{' => {
                    if !constant_piece.is_empty() {
                        spec_constructor.push(
                            self.expr(
                                ast::ExprConstant {
                                    value: std::mem::take(&mut constant_piece).into(),
                                    kind: None,
                                    range: self.range(),
                                }
                                .into(),
                            ),
                        );
                    }
                    let parsed_expr = self.parse_fstring(nested + 1)?;
                    spec_constructor.extend(parsed_expr);
                    continue;
                }
                '}' => {
                    break;
                }
                _ => {
                    constant_piece.push(next);
                }
            }
            self.next_char();
        }
        if !constant_piece.is_empty() {
            spec_constructor.push(
                self.expr(
                    ast::ExprConstant {
                        value: std::mem::take(&mut constant_piece).into(),
                        kind: None,
                        range: self.range(),
                    }
                    .into(),
                ),
            );
        }
        Ok(spec_constructor)
    }

    fn parse_fstring(&mut self, nested: u8) -> Result<Vec<Expr>, LexicalError> {
        use FStringErrorType::*;

        if nested >= 2 {
            return Err(FStringError::new(ExpressionNestedTooDeeply, self.get_pos()).into());
        }

        let mut content = String::new();
        let mut values = vec![];

        while let Some(&ch) = self.peek() {
            match ch {
                '{' => {
                    self.next_char();
                    if nested == 0 {
                        match self.peek() {
                            Some('{') => {
                                self.next_char();
                                content.push('{');
                                continue;
                            }
                            None => {
                                return Err(FStringError::new(UnclosedLbrace, self.get_pos()).into())
                            }
                            _ => {}
                        }
                    }
                    if !content.is_empty() {
                        values.push(
                            self.expr(
                                ast::ExprConstant {
                                    value: std::mem::take(&mut content).into(),
                                    kind: None,
                                    range: self.range(),
                                }
                                .into(),
                            ),
                        );
                    }

                    let parsed_values = self.parse_formatted_value(nested)?;
                    values.extend(parsed_values);
                }
                '}' => {
                    if nested > 0 {
                        break;
                    }
                    self.next_char();
                    if let Some('}') = self.peek() {
                        self.next_char();
                        content.push('}');
                    } else {
                        return Err(FStringError::new(SingleRbrace, self.get_pos()).into());
                    }
                }
                '\\' if !self.kind.is_raw() => {
                    self.next_char();
                    if let Some('{' | '}') = self.peek() {
                        content.push('\\');
                    } else {
                        content.push_str(&self.parse_escaped_char()?);
                    }
                }
                _ => {
                    content.push(ch);
                    self.next_char();
                }
            }
        }

        if !content.is_empty() {
            values.push(
                self.expr(
                    ast::ExprConstant {
                        value: content.into(),
                        kind: None,
                        range: self.range(),
                    }
                    .into(),
                ),
            )
        }

        Ok(values)
    }

    fn parse_bytes(&mut self) -> Result<Expr, LexicalError> {
        let mut content = String::new();
        while let Some(ch) = self.next_char() {
            match ch {
                '\\' if !self.kind.is_raw() => {
                    content.push_str(&self.parse_escaped_char()?);
                }
                ch => {
                    if !ch.is_ascii() {
                        return Err(LexicalError::new(
                            LexicalErrorType::OtherError(
                                "bytes can only contain ASCII literal characters".to_string(),
                            ),
                            self.get_pos(),
                        ));
                    }
                    content.push(ch);
                }
            }
        }

        Ok(self.expr(
            ast::ExprConstant {
                value: Constant::Bytes(content.chars().map(|c| c as u8).collect()),
                kind: None,
                range: self.range(),
            }
            .into(),
        ))
    }

    fn parse_string(&mut self) -> Result<Expr, LexicalError> {
        let mut content = String::new();
        while let Some(ch) = self.next_char() {
            match ch {
                '\\' if !self.kind.is_raw() => {
                    content.push_str(&self.parse_escaped_char()?);
                }
                ch => content.push(ch),
            }
        }
        Ok(self.expr(
            ast::ExprConstant {
                value: Constant::Str(content),
                kind: self.kind.is_unicode().then(|| "u".to_string()),
                range: self.range(),
            }
            .into(),
        ))
    }

    fn parse(&mut self) -> Result<Vec<Expr>, LexicalError> {
        if self.kind.is_any_fstring() {
            self.parse_fstring(0)
        } else if self.kind.is_any_bytes() {
            self.parse_bytes().map(|expr| vec![expr])
        } else {
            self.parse_string().map(|expr| vec![expr])
        }
    }
}

fn parse_fstring_expr(source: &str, location: TextSize) -> Result<Expr, ParseError> {
    let fstring_body = format!("({source})");
    let start = location - TextSize::from(1);
    ast::Expr::parse_starts_at(&fstring_body, "<fstring>", start)
}

fn parse_string(
    source: &str,
    kind: StringKind,
    triple_quoted: bool,
    start: TextSize,
    end: TextSize,
) -> Result<Vec<Expr>, LexicalError> {
    StringParser::new(source, kind, triple_quoted, start, end).parse()
}

pub(crate) fn parse_strings(
    values: Vec<(TextSize, (String, StringKind, bool), TextSize)>,
) -> Result<Expr, LexicalError> {
    // Preserve the initial location and kind.
    let initial_start = values[0].0;
    let last_end = values.last().unwrap().2;
    let initial_kind = (values[0].1 .1 == StringKind::Unicode).then(|| "u".to_owned());
    let has_fstring = values
        .iter()
        .any(|(_, (_, kind, ..), _)| kind.is_any_fstring());
    let num_bytes = values
        .iter()
        .filter(|(_, (_, kind, ..), _)| kind.is_any_bytes())
        .count();
    let has_bytes = num_bytes > 0;

    if has_bytes && num_bytes < values.len() {
        return Err(LexicalError {
            error: LexicalErrorType::OtherError(
                "cannot mix bytes and nonbytes literals".to_owned(),
            ),
            location: initial_start,
        });
    }

    if has_bytes {
        let mut content: Vec<u8> = vec![];
        for (start, (source, kind, triple_quoted), end) in values {
            for value in parse_string(&source, kind, triple_quoted, start, end)? {
                match value {
                    Expr::Constant(ast::ExprConstant {
                        value: Constant::Bytes(value),
                        ..
                    }) => content.extend(value),
                    _ => unreachable!("Unexpected non-bytes expression."),
                }
            }
        }
        return Ok(ast::ExprConstant {
            value: Constant::Bytes(content),
            kind: None,
            range: TextRange::new(initial_start, last_end),
        }
        .into());
    }

    if !has_fstring {
        let mut content: Vec<String> = vec![];
        for (start, (source, kind, triple_quoted), end) in values {
            for value in parse_string(&source, kind, triple_quoted, start, end)? {
                match value {
                    Expr::Constant(ast::ExprConstant {
                        value: Constant::Str(value),
                        ..
                    }) => content.push(value),
                    _ => unreachable!("Unexpected non-string expression."),
                }
            }
        }
        return Ok(ast::ExprConstant {
            value: Constant::Str(content.join("")),
            kind: initial_kind,
            range: TextRange::new(initial_start, last_end),
        }
        .into());
    }

    // De-duplicate adjacent constants.
    let mut deduped: Vec<Expr> = vec![];
    let mut current: Vec<String> = vec![];

    let take_current = |current: &mut Vec<String>| -> Expr {
        Expr::Constant(ast::ExprConstant {
            value: Constant::Str(current.drain(..).join("")),
            kind: initial_kind.clone(),
            range: TextRange::new(initial_start, last_end),
        })
    };

    for (start, (source, kind, triple_quoted), end) in values {
        for value in parse_string(&source, kind, triple_quoted, start, end)? {
            match value {
                Expr::FormattedValue { .. } => {
                    if !current.is_empty() {
                        deduped.push(take_current(&mut current));
                    }
                    deduped.push(value)
                }
                Expr::Constant(ast::ExprConstant {
                    value: Constant::Str(value),
                    ..
                }) => current.push(value),
                _ => unreachable!("Unexpected non-string expression."),
            }
        }
    }
    if !current.is_empty() {
        deduped.push(take_current(&mut current));
    }

    Ok(Expr::JoinedStr(ast::ExprJoinedStr {
        values: deduped,
        range: TextRange::new(initial_start, last_end),
    }))
}

// TODO: consolidate these with ParseError
/// An error that occurred during parsing of an f-string.
#[derive(Debug, PartialEq)]
struct FStringError {
    /// The type of error that occurred.
    pub error: FStringErrorType,
    /// The location of the error.
    pub location: TextSize,
}

impl FStringError {
    /// Creates a new `FStringError` with the given error type and location.
    pub fn new(error: FStringErrorType, location: TextSize) -> Self {
        Self { error, location }
    }
}

impl From<FStringError> for LexicalError {
    fn from(err: FStringError) -> Self {
        LexicalError {
            error: LexicalErrorType::FStringError(err.error),
            location: err.location,
        }
    }
}

/// Represents the different types of errors that can occur during parsing of an f-string.
#[derive(Debug, PartialEq)]
pub enum FStringErrorType {
    /// Expected a right brace after an opened left brace.
    UnclosedLbrace,
    /// Expected a left brace after an ending right brace.
    UnopenedRbrace,
    /// Expected a right brace after a conversion flag.
    ExpectedRbrace,
    /// An error occurred while parsing an f-string expression.
    InvalidExpression(Box<ParseErrorType>),
    /// An invalid conversion flag was encountered.
    InvalidConversionFlag,
    /// An empty expression was encountered.
    EmptyExpression,
    /// An opening delimiter was not closed properly.
    MismatchedDelimiter(char, char),
    /// Too many nested expressions in an f-string.
    ExpressionNestedTooDeeply,
    /// The f-string expression cannot include the given character.
    ExpressionCannotInclude(char),
    /// A single right brace was encountered.
    SingleRbrace,
    /// A closing delimiter was not opened properly.
    Unmatched(char),
    // TODO: Test this case.
    /// Unterminated string.
    UnterminatedString,
}

impl std::fmt::Display for FStringErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use FStringErrorType::*;
        match self {
            UnclosedLbrace => write!(f, "expecting '}}'"),
            UnopenedRbrace => write!(f, "Unopened '}}'"),
            ExpectedRbrace => write!(f, "Expected '}}' after conversion flag."),
            InvalidExpression(error) => {
                write!(f, "{error}")
            }
            InvalidConversionFlag => write!(f, "invalid conversion character"),
            EmptyExpression => write!(f, "empty expression not allowed"),
            MismatchedDelimiter(first, second) => write!(
                f,
                "closing parenthesis '{second}' does not match opening parenthesis '{first}'"
            ),
            SingleRbrace => write!(f, "single '}}' is not allowed"),
            Unmatched(delim) => write!(f, "unmatched '{delim}'"),
            ExpressionNestedTooDeeply => {
                write!(f, "expressions nested too deeply")
            }
            UnterminatedString => {
                write!(f, "unterminated string")
            }
            ExpressionCannotInclude(c) => {
                if *c == '\\' {
                    write!(f, "f-string expression part cannot include a backslash")
                } else {
                    write!(f, "f-string expression part cannot include '{c}'s")
                }
            }
        }
    }
}

impl From<FStringError> for LalrpopError<TextSize, Tok, LexicalError> {
    fn from(err: FStringError) -> Self {
        lalrpop_util::ParseError::User {
            error: LexicalError {
                error: LexicalErrorType::FStringError(err.error),
                location: err.location,
            },
        }
    }
}
