//! C frontend for xpile (PMAT-467, v0.2.0 Track 2.A).
//!
//! Parses a stack-only, int-typed subset of C and lowers it to the
//! shared meta-HIR. This is the first real `decy-frontend` — it
//! replaces the v0.1.0 35-line stub that returned an empty module.
//!
//! Supported subset (slice 1):
//!   - `int` / `long` / `int64_t` / `unsigned` / `double` / `float` function
//!     definitions with matching parameters (PMAT-909: `long`/`int64_t` lower
//!     to the distinct 64-bit `Type::CLong` width, kept apart from the
//!     32-bit-`int`-backed `I64`; PMAT-910: `double` lowers to `Type::F64`,
//!     ABI `c_double`; PMAT-911: `float` lowers to the DISTINCT 32-bit
//!     `Type::F32`, ABI `c_float` — never widened through the 64-bit double
//!     slot; PMAT-918: `unsigned`/`unsigned int`/`uint32_t` lower to the
//!     DISTINCT 32-bit-unsigned `Type::CUInt`, ABI `c_uint` — never the signed
//!     `c_int`; PMAT-921: `unsigned long`/`unsigned long long`/`uint64_t` lower
//!     to the DISTINCT 64-bit-unsigned `Type::CULong`, ABI `c_ulonglong` —
//!     never the 32-bit `c_uint` (truncation) nor the signed `c_longlong`.
//!     Pointer/string tokens are the remaining ABI ceiling.)
//!   - local `int` / `long` / `unsigned` / `unsigned long` / `double` / `float` declarations (`int x = <expr>;`)
//!   - a trailing `return <expr>;`
//!   - expressions: integer and float literals, identifiers, calls (recursion),
//!     `+ - * / %`, comparisons (`< <= > >= == !=`), `&& ||`, unary `- !`,
//!     bitwise `& | ^ ~` and shifts `<< >>` (PMAT-964 — integer-only, ABI-honest
//!     on the existing widths, governed by `C-C-INT-ARITH`; shifts lower to the
//!     UB-free `wrapping_shl`/`wrapping_shr`), the ternary `c ? a : b`, and
//!     parentheses
//!
//! Deferred: pointer DEREFERENCE / address-of (`*p` / `&x` — the pointer
//! *types* `int*`/`char*` lift, but decy has no pointer-op grammar yet),
//! structs, strings, hex/octal literals, and `**` (power).
//!
//! C arithmetic semantics (fixed-width `i32`, wrapping overflow) are
//! realised in the Rust backend's C emit path keyed on
//! `SourceLang::C`; this frontend keeps the meta-HIR clean (`int` →
//! `Type::I64`, the backend narrows to `i32`; PMAT-909 `long`/`int64_t`
//! → `Type::CLong`, which the C emit path keeps at `i64`; PMAT-918
//! `unsigned`/`uint32_t` → `Type::CUInt`, which the C emit path renders
//! `u32` with DEFINED-modular `wrapping_*` arithmetic; PMAT-921 `unsigned
//! long`/`uint64_t` → `Type::CULong`, which the C emit path renders `u64`
//! with the same DEFINED-modular `wrapping_*` arithmetic). The governing
//! contract `C-C-INT-ARITH` is on disk (the modular-arithmetic family
//! covers both the signed and the unsigned widths, at both widths).

use std::path::Path;
use xpile_frontend::{Frontend, FrontendError};
use xpile_meta_hir::{
    BinOp, Block, Expr, Function, Item, Module, Param, SourceLang, Stmt, Type, UnOp,
};

pub struct CFrontend;

impl Frontend for CFrontend {
    fn name(&self) -> &'static str {
        "c"
    }

    fn extensions(&self) -> &[&'static str] {
        &["c", "h"]
    }

    /// PMAT-1433: none. Both `.c` and `.h` lower — measured by
    /// `frontend_claim_disposition_witness.rs`, not asserted here.
    fn refused_claims(&self) -> &[&'static str] {
        &[]
    }

    fn parse_and_lower(&self, path: &Path, source: &str) -> Result<Module, FrontendError> {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let tokens = lex(source).map_err(FrontendError::Lower)?;
        let items = Parser::new(&tokens)
            .parse_module()
            .map_err(FrontendError::Lower)?;
        Ok(Module {
            name,
            source_lang: SourceLang::C,
            items,
            ffi_boundaries: Vec::new(),
        })
    }
}

// ── Lexer ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Int,      // `int` keyword (C 32-bit int → meta-HIR I64, ABI c_int)
    Long, // `long` / `long long` / `int64_t` (PMAT-909): 64-bit C int → meta-HIR CLong, ABI c_longlong
    Unsigned, // `unsigned` / `unsigned int` / `uint32_t` (PMAT-918): 32-bit UNSIGNED C int → meta-HIR CUInt, ABI c_uint
    ULong, // `uint64_t` / `unsigned long` (`unsigned`+`long`) (PMAT-921): 64-bit UNSIGNED C int → meta-HIR CULong, ABI c_ulonglong
    Double, // `double` keyword (PMAT-910): 64-bit C double → meta-HIR F64, ABI c_double
    Float, // `float` keyword (PMAT-911): 32-bit C float → meta-HIR F32, ABI c_float
    Char,  // `char` keyword (PMAT-924): 8-bit C char → meta-HIR CChar, ABI c_char (pointee-only)
    Return, // `return` keyword
    Void,  // `void` keyword
    Ident(String),
    Num(i64),
    FNum(f64), // float literal (PMAT-910): `<digits>.<digits>` → Expr::LitFloat
    LParen,
    RParen,
    LBrace,
    RBrace,
    Semi,
    Comma,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    While,
    If,
    Else,
    Lt,
    Le,
    Gt,
    Ge,
    EqEq,
    Ne,
    AndAnd,
    OrOr,
    Bang,
    // PMAT-964: C bitwise / shift operators. These ride the existing integer
    // widths (no new ABI token) and are governed by the same C-C-INT-ARITH
    // integer operational-semantics contract — citation-honest. `Amp`/`Pipe`
    // are the SINGLE-char bitwise forms (the double `&&`/`||` lex to
    // `AndAnd`/`OrOr` above, before these arms are reached).
    Amp,   // `&`  bitwise AND
    Pipe,  // `|`  bitwise OR
    Caret, // `^`  bitwise XOR
    Tilde, // `~`  bitwise NOT (unary)
    Shl,   // `<<` left shift
    Shr,   // `>>` right shift
    Question,
    Colon,
    Assign,
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut toks = Vec::new();
    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            ' ' | '\t' | '\r' | '\n' => i += 1,
            // line + block comments
            '/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            '/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            '(' => {
                toks.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                toks.push(Tok::RParen);
                i += 1;
            }
            '{' => {
                toks.push(Tok::LBrace);
                i += 1;
            }
            '}' => {
                toks.push(Tok::RBrace);
                i += 1;
            }
            ';' => {
                toks.push(Tok::Semi);
                i += 1;
            }
            ',' => {
                toks.push(Tok::Comma);
                i += 1;
            }
            '+' => {
                toks.push(Tok::Plus);
                i += 1;
            }
            '-' => {
                toks.push(Tok::Minus);
                i += 1;
            }
            '*' => {
                toks.push(Tok::Star);
                i += 1;
            }
            // `/` reaches here only when not `//` or `/*` (handled above).
            '/' => {
                toks.push(Tok::Slash);
                i += 1;
            }
            '%' => {
                toks.push(Tok::Percent);
                i += 1;
            }
            '?' => {
                toks.push(Tok::Question);
                i += 1;
            }
            ':' => {
                toks.push(Tok::Colon);
                i += 1;
            }
            '<' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    toks.push(Tok::Le);
                    i += 2;
                } else if i + 1 < bytes.len() && bytes[i + 1] == b'<' {
                    // PMAT-964: `<<` left shift (checked before single `<`).
                    toks.push(Tok::Shl);
                    i += 2;
                } else {
                    toks.push(Tok::Lt);
                    i += 1;
                }
            }
            '>' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    toks.push(Tok::Ge);
                    i += 2;
                } else if i + 1 < bytes.len() && bytes[i + 1] == b'>' {
                    // PMAT-964: `>>` right shift (checked before single `>`).
                    toks.push(Tok::Shr);
                    i += 2;
                } else {
                    toks.push(Tok::Gt);
                    i += 1;
                }
            }
            '=' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    toks.push(Tok::EqEq);
                    i += 2;
                } else {
                    toks.push(Tok::Assign);
                    i += 1;
                }
            }
            '!' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    toks.push(Tok::Ne);
                    i += 2;
                } else {
                    toks.push(Tok::Bang);
                    i += 1;
                }
            }
            '&' if i + 1 < bytes.len() && bytes[i + 1] == b'&' => {
                toks.push(Tok::AndAnd);
                i += 2;
            }
            '|' if i + 1 < bytes.len() && bytes[i + 1] == b'|' => {
                toks.push(Tok::OrOr);
                i += 2;
            }
            // PMAT-964: SINGLE-char bitwise operators (reached only when the
            // preceding `&&`/`||` guards did not match — i.e. a lone `&`/`|`).
            '&' => {
                toks.push(Tok::Amp);
                i += 1;
            }
            '|' => {
                toks.push(Tok::Pipe);
                i += 1;
            }
            '^' => {
                toks.push(Tok::Caret);
                i += 1;
            }
            '~' => {
                toks.push(Tok::Tilde);
                i += 1;
            }
            c if c.is_ascii_digit() => {
                let start = i;
                while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                    i += 1;
                }
                // PMAT-910: a `.` (followed by zero or more digits) makes this a
                // C floating-point literal (`2.0`, `2.`, `0.5`) → Tok::FNum. `.`
                // is otherwise never lexed (decy has no member access), so this
                // is unambiguous.
                if i < bytes.len() && bytes[i] == b'.' {
                    i += 1;
                    while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                        i += 1;
                    }
                    let s = &src[start..i];
                    let v: f64 = s
                        .parse()
                        .map_err(|_| format!("float literal `{s}` is not a valid f64"))?;
                    toks.push(Tok::FNum(v));
                } else {
                    let s = &src[start..i];
                    // PMAT-1382: a C integer literal with a LEADING ZERO is
                    // OCTAL (C17 6.4.4.1), not decimal. Through v0.1.617 this
                    // branch ran a base-10 `parse()` over the whole run, so
                    // `010` lifted as 10 and the CLI exited 0 emitting Rust
                    // that computes a DIFFERENT VALUE than the C it was given
                    // (gcc: 8). Hex (`0xff`) never reaches here — the `x` ends
                    // the digit run and the parser refuses the stray ident —
                    // so leading-zero-plus-digits is exactly the octal case.
                    let v: i64 = if s.len() > 1 && s.starts_with('0') {
                        i64::from_str_radix(&s[1..], 8).map_err(|_| {
                            format!(
                                "integer literal `{s}` has a leading zero, so C reads it as \
                                 OCTAL, but `{}` is not a valid octal digit string (C octal \
                                 digits are 0-7)",
                                &s[1..]
                            )
                        })?
                    } else {
                        s.parse()
                            .map_err(|_| format!("integer literal `{s}` does not fit in i64"))?
                    };
                    toks.push(Tok::Num(v));
                }
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < bytes.len()
                    && ((bytes[i] as char).is_ascii_alphanumeric() || bytes[i] == b'_')
                {
                    i += 1;
                }
                let word = &src[start..i];
                toks.push(match word {
                    "int" => Tok::Int,
                    // PMAT-909: a wider C integer. `long`, `int64_t`, and the
                    // fixed-width `<stdint.h>` 64-bit aliases all lex to one
                    // `Long` token; the parser folds a following `long`
                    // (`long long`) into the same width.
                    "long" | "int64_t" | "int_least64_t" | "int_fast64_t" => Tok::Long,
                    // PMAT-918: C `unsigned` / `unsigned int` and the fixed-width
                    // 32-bit `<stdint.h>` unsigned aliases lex to one `Unsigned`
                    // token; the parser folds a following `int` (`unsigned int`)
                    // into the same 32-bit-unsigned width.
                    "unsigned" | "uint32_t" | "uint_least32_t" | "uint_fast32_t" => Tok::Unsigned,
                    // PMAT-921: a 64-bit UNSIGNED C integer. `unsigned long` /
                    // `unsigned long long` lex as `unsigned` (`Tok::Unsigned`)
                    // followed by `long` (`Tok::Long`) — folded in `parse_c_type`.
                    // The fixed-width `<stdint.h>` 64-bit unsigned aliases lex
                    // directly to one `ULong` token.
                    "uint64_t" | "uint_least64_t" | "uint_fast64_t" => Tok::ULong,
                    // PMAT-910: C `double` (64-bit) → meta-HIR F64, ABI c_double
                    // — ABI-consistent (C double = Rust f64 = c_double = 64-bit).
                    "double" => Tok::Double,
                    // PMAT-911: C `float` (32-bit) → meta-HIR F32, ABI c_float
                    // — its own DISTINCT width, never narrowed/widened through
                    // the 64-bit c_double slot (the 32↔64 ABI honesty PMAT-909
                    // established for ints, now held for floats too).
                    "float" => Tok::Float,
                    // PMAT-924: C `char` (8-bit). Only valid as a pointer
                    // pointee (`char*`) — a bare `char` value has no meta-HIR
                    // scalar (refused in `parse_c_type`).
                    "char" => Tok::Char,
                    "return" => Tok::Return,
                    "void" => Tok::Void,
                    "while" => Tok::While,
                    "if" => Tok::If,
                    "else" => Tok::Else,
                    other => Tok::Ident(other.to_string()),
                });
            }
            other => return Err(format!("unexpected character `{other}` in C source")),
        }
    }
    Ok(toks)
}

// ── Parser (recursive descent + precedence climbing) ────────────────

struct Parser<'a> {
    toks: &'a [Tok],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(toks: &'a [Tok]) -> Self {
        Parser { toks, pos: 0 }
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn bump(&mut self) -> Option<&Tok> {
        let t = self.toks.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, want: &Tok) -> Result<(), String> {
        match self.peek() {
            Some(t) if t == want => {
                self.pos += 1;
                Ok(())
            }
            other => Err(format!("expected {want:?}, found {other:?}")),
        }
    }

    fn parse_module(&mut self) -> Result<Vec<Item>, String> {
        let mut items = Vec::new();
        while self.peek().is_some() {
            items.push(Item::Function(self.parse_function()?));
        }
        if items.is_empty() {
            return Err("no function definitions found in C source".into());
        }
        Ok(items)
    }

    fn parse_function(&mut self) -> Result<Function, String> {
        // `<int|long|double> NAME ( params ) { body }` — PMAT-909/910: the
        // return type carries its own width (`long sq(...)` → `CLong`;
        // `double sq(...)` → `F64`).
        let return_type = self.parse_c_type()?;
        let name = self.parse_ident()?;
        self.eat(&Tok::LParen)?;
        let params = self.parse_params()?;
        self.eat(&Tok::RParen)?;
        self.eat(&Tok::LBrace)?;
        let body = self.parse_body(&name)?;
        self.eat(&Tok::RBrace)?;
        Ok(Function {
            name,
            params,
            return_type,
            body,
        })
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, String> {
        let mut params = Vec::new();
        // `()` or `(void)` → no params
        if matches!(self.peek(), Some(Tok::RParen)) {
            return Ok(params);
        }
        if matches!(self.peek(), Some(Tok::Void)) {
            self.bump();
            return Ok(params);
        }
        loop {
            let ty = self.parse_c_type()?;
            let name = self.parse_ident()?;
            params.push(Param {
                name,
                ty,
                mutable: false,
            });
            match self.peek() {
                Some(Tok::Comma) => {
                    self.bump();
                }
                _ => break,
            }
        }
        Ok(params)
    }

    fn parse_body(&mut self, fn_name: &str) -> Result<Block, String> {
        let mut stmts = Vec::new();
        let mut trailing_return: Option<Expr> = None;
        while !matches!(self.peek(), Some(Tok::RBrace)) {
            if matches!(self.peek(), Some(Tok::Return)) {
                self.bump();
                let e = self.parse_expr()?;
                self.eat(&Tok::Semi)?;
                trailing_return = Some(e);
                break; // the trailing return must be the last statement
            }
            stmts.push(self.parse_stmt(fn_name)?);
        }
        let trailing_return = trailing_return.ok_or_else(|| {
            format!(
                "function `{fn_name}` has no `return` — every C function must end with `return <expr>;`"
            )
        })?;
        // Mark a `let` mutable iff the local is reassigned somewhere
        // (including inside a `while` body). Keeps `clippy -D warnings`
        // happy: no spurious `mut`, and `mut` where `x = …` requires it.
        mark_mutable(&mut stmts);
        Ok(Block {
            stmts,
            trailing_return,
        })
    }

    /// A single in-body statement: `int x = e;` (decl), `x = e;`
    /// (reassignment), or `while (c) { … }`. The trailing `return` is
    /// handled by the caller (it must be last; `return` inside a loop
    /// body is rejected since the meta-HIR has a single trailing return).
    fn parse_stmt(&mut self, fn_name: &str) -> Result<Stmt, String> {
        match self.peek() {
            // PMAT-909/910/911/918/921: a local decl begins with a C scalar
            // type token (`int x = …;`, `long x = …;`, `unsigned x = …;`,
            // `uint64_t x = …;`, `double x = …;`, or `float x = …;`), each
            // carrying its own width. (`unsigned long` begins with `Tok::Unsigned`,
            // already covered.)
            Some(Tok::Int)
            | Some(Tok::Long)
            | Some(Tok::Unsigned)
            | Some(Tok::ULong)
            | Some(Tok::Double)
            | Some(Tok::Float) => {
                let ty = self.parse_c_type()?;
                let name = self.parse_ident()?;
                self.eat(&Tok::Assign)?;
                let value = self.parse_expr()?;
                self.eat(&Tok::Semi)?;
                Ok(Stmt::Let {
                    name,
                    ty,
                    value,
                    mutable: false,
                })
            }
            Some(Tok::While) => self.parse_while(fn_name),
            Some(Tok::If) => self.parse_if(fn_name),
            Some(Tok::Ident(_)) => {
                let name = self.parse_ident()?;
                self.eat(&Tok::Assign)?;
                let value = self.parse_expr()?;
                self.eat(&Tok::Semi)?;
                Ok(Stmt::Assign { name, value })
            }
            other => Err(format!(
                "function `{fn_name}`: unexpected token {other:?} — supported statements: `int x = e;`, `long x = e;`, `unsigned x = e;`, `double x = e;`, `float x = e;`, `x = e;`, `if (c) {{ … }} else {{ … }}`, `while (c) {{ … }}`, then a final `return e;`"
            )),
        }
    }

    /// PMAT-478 (R9): `if (cond) { stmts } [else { stmts }]` →
    /// `Stmt::If`. Branch bodies are statement lists (decls / assigns /
    /// nested if / while); `return` inside a branch is not supported at
    /// v0.2.0 (the meta-HIR uses a single trailing return — that is R10).
    fn parse_if(&mut self, fn_name: &str) -> Result<Stmt, String> {
        self.eat(&Tok::If)?;
        self.eat(&Tok::LParen)?;
        let cond = self.parse_expr()?;
        self.eat(&Tok::RParen)?;
        let then_body = self.parse_brace_block(fn_name)?;
        let else_body = if matches!(self.peek(), Some(Tok::Else)) {
            self.bump();
            // `else if` chains: an `else` directly followed by `if`
            // nests as a single-statement else block.
            if matches!(self.peek(), Some(Tok::If)) {
                vec![self.parse_if(fn_name)?]
            } else {
                self.parse_brace_block(fn_name)?
            }
        } else {
            Vec::new()
        };
        Ok(Stmt::If {
            cond,
            then_body,
            else_body,
        })
    }

    /// Parse `{ stmt* }` — a brace-delimited statement list with no
    /// trailing return (used by `if`/`else` branch bodies). Rejects
    /// `return` inside (early returns are R10).
    fn parse_brace_block(&mut self, fn_name: &str) -> Result<Vec<Stmt>, String> {
        self.eat(&Tok::LBrace)?;
        let mut body = Vec::new();
        while !matches!(self.peek(), Some(Tok::RBrace)) {
            // PMAT-479 (R10): early `return <expr>;` inside a branch →
            // Stmt::Return (guard clauses). The function still ends with
            // a trailing return; this is a non-final return.
            if matches!(self.peek(), Some(Tok::Return)) {
                self.bump();
                let e = self.parse_expr()?;
                self.eat(&Tok::Semi)?;
                body.push(Stmt::Return(e));
                continue;
            }
            body.push(self.parse_stmt(fn_name)?);
        }
        self.eat(&Tok::RBrace)?;
        Ok(body)
    }

    fn parse_while(&mut self, fn_name: &str) -> Result<Stmt, String> {
        self.eat(&Tok::While)?;
        self.eat(&Tok::LParen)?;
        let cond = self.parse_expr()?;
        self.eat(&Tok::RParen)?;
        self.eat(&Tok::LBrace)?;
        let mut body = Vec::new();
        while !matches!(self.peek(), Some(Tok::RBrace)) {
            if matches!(self.peek(), Some(Tok::Return)) {
                return Err(format!(
                    "function `{fn_name}`: `return` inside a `while` body is not supported at v0.2.0 (the meta-HIR uses a single trailing return)"
                ));
            }
            body.push(self.parse_stmt(fn_name)?);
        }
        self.eat(&Tok::RBrace)?;
        Ok(Stmt::While { cond, body })
    }

    fn parse_ident(&mut self) -> Result<String, String> {
        match self.bump() {
            Some(Tok::Ident(s)) => Ok(s.clone()),
            other => Err(format!("expected identifier, found {other:?}")),
        }
    }

    /// PMAT-909/910: consume a C scalar type token and return its distinct
    /// meta-HIR width. `int` → [`Type::I64`] (the 32-bit C `int`, which
    /// the C emit path narrows to `i32` and the FFI ABI maps to `c_int`).
    /// `long` / `int64_t` → [`Type::CLong`] (a 64-bit C integer the FFI
    /// ABI maps to `c_longlong` instead of narrowing to `c_int`). A
    /// trailing second `long` (`long long`) folds into the same width.
    /// `double` → [`Type::F64`] (a 64-bit C double the FFI ABI maps to
    /// `c_double`). `float` → [`Type::F32`] (PMAT-911), a 32-bit C float the
    /// FFI ABI maps to the DISTINCT `c_float` slot — its own width, not
    /// narrowed/widened through `c_double` (the 32↔64 ABI honesty held for
    /// floats as for ints). PMAT-918: `unsigned` / `unsigned int` / `uint32_t`
    /// → [`Type::CUInt`] (a 32-bit UNSIGNED C int the FFI ABI maps to the
    /// `c_uint` slot, never the signed `c_int`); a trailing `int`
    /// (`unsigned int`) folds into the same width. PMAT-921: `unsigned long` /
    /// `unsigned long long` / `uint64_t` → [`Type::CULong`] (a 64-bit UNSIGNED
    /// C int the FFI ABI maps to the `c_ulonglong` slot — never the 32-bit
    /// `c_uint`, which truncates, nor the signed `c_longlong`, which flips
    /// values ≥ 2⁶³ negative); a trailing `long` after `unsigned` PROMOTES
    /// `CUInt` → `CULong`.
    fn parse_c_type(&mut self) -> Result<Type, String> {
        // PMAT-924: parse the base scalar, then fold any trailing `*` into a
        // `Type::Ptr` (the first address-carrying ABI token). A `char` base is
        // pointee-ONLY — `char*` is a pointer (valid), a bare `char` value is
        // refused (no 8-bit meta-HIR scalar). Only a SINGLE level of indirection
        // over an ABI-mappable scalar is accepted; `int**` (pointer-to-pointer)
        // is refused rather than mis-emitted, mirroring how the scalar lifts
        // refused non-mappable types.
        let base = self.parse_c_base_type()?;
        if matches!(self.peek(), Some(Tok::Star)) {
            self.bump();
            if matches!(self.peek(), Some(Tok::Star)) {
                return Err(
                    "pointer-to-pointer (`T**`) is not supported — decy lifts a single \
                     level of pointer indirection over a scalar pointee at v0.2.0 (PMAT-924)"
                        .to_string(),
                );
            }
            // A bare `int*` is a `*mut` pointer (the C default — the pointee is
            // not `const`-qualified). `const`-pointer (`*const`) is a deferred
            // refinement (decy has no `const` keyword yet).
            return Ok(Type::Ptr {
                mutable: true,
                pointee: Box::new(base),
            });
        }
        // A non-pointer `char` is refused — `char` has no standalone meta-HIR
        // scalar (it only exists as a pointer pointee).
        if matches!(base, Type::CChar) {
            return Err(
                "bare `char` is not a supported value type — only `char*` (a pointer) is \
                 lifted at v0.2.0 (PMAT-924); a scalar `char` has no meta-HIR width"
                    .to_string(),
            );
        }
        Ok(base)
    }

    /// PMAT-924: parse just the base scalar type token (no pointer suffix). The
    /// pointer `*` is folded by the caller [`parse_c_type`]. `char` lexes to the
    /// pointee-only [`Type::CChar`] here; the caller refuses it unless a `*`
    /// follows.
    fn parse_c_base_type(&mut self) -> Result<Type, String> {
        match self.peek() {
            Some(Tok::Int) => {
                self.bump();
                Ok(Type::I64)
            }
            Some(Tok::Char) => {
                self.bump();
                Ok(Type::CChar)
            }
            Some(Tok::Long) => {
                self.bump();
                // `long long` — fold the optional second `long`.
                if matches!(self.peek(), Some(Tok::Long)) {
                    self.bump();
                }
                Ok(Type::CLong)
            }
            // PMAT-918/921: `unsigned` / `unsigned int` / `uint32_t` → the
            // DISTINCT 32-bit-unsigned `Type::CUInt`; a trailing `int`
            // (`unsigned int`) folds into that width. PMAT-921: a trailing
            // `long` (`unsigned long` / `unsigned long long`) PROMOTES to the
            // DISTINCT 64-bit-unsigned `Type::CULong` instead — never folded
            // into the 32-bit `CUInt` (that would truncate) nor the 64-bit
            // SIGNED `CLong` (that would flip values ≥ 2⁶³ negative).
            Some(Tok::Unsigned) => {
                self.bump();
                if matches!(self.peek(), Some(Tok::Long)) {
                    self.bump();
                    // `unsigned long long` — fold the optional second `long`.
                    if matches!(self.peek(), Some(Tok::Long)) {
                        self.bump();
                    }
                    return Ok(Type::CULong);
                }
                if matches!(self.peek(), Some(Tok::Int)) {
                    self.bump();
                }
                Ok(Type::CUInt)
            }
            // PMAT-921: the fixed-width `<stdint.h>` 64-bit unsigned aliases
            // (`uint64_t` / `uint_least64_t` / `uint_fast64_t`) lex straight to
            // `Tok::ULong` → the DISTINCT 64-bit-unsigned `Type::CULong`.
            Some(Tok::ULong) => {
                self.bump();
                Ok(Type::CULong)
            }
            Some(Tok::Double) => {
                self.bump();
                Ok(Type::F64)
            }
            Some(Tok::Float) => {
                self.bump();
                Ok(Type::F32)
            }
            other => Err(format!(
                "expected a C scalar type (`int`, `long`, `int64_t`, `unsigned`, `unsigned int`, `uint32_t`, `unsigned long`, `uint64_t`, `float`, `double`, or a pointer `int*`/`char*`/`double*`), found {other:?}"
            )),
        }
    }

    // Expression precedence climbing -----------------------------------

    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_ternary()
    }

    fn parse_ternary(&mut self) -> Result<Expr, String> {
        let cond = self.parse_or()?;
        if matches!(self.peek(), Some(Tok::Question)) {
            self.bump();
            let then_expr = self.parse_expr()?;
            self.eat(&Tok::Colon)?;
            let else_expr = self.parse_expr()?;
            return Ok(Expr::IfExpr {
                cond: Box::new(cond),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            });
        }
        Ok(cond)
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_and()?;
        while matches!(self.peek(), Some(Tok::OrOr)) {
            self.bump();
            let rhs = self.parse_and()?;
            lhs = bin(BinOp::Or, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_bitor()?;
        while matches!(self.peek(), Some(Tok::AndAnd)) {
            self.bump();
            let rhs = self.parse_bitor()?;
            lhs = bin(BinOp::And, lhs, rhs);
        }
        Ok(lhs)
    }

    // PMAT-964: the three C bitwise levels sit BELOW `&&` and ABOVE equality,
    // in the C precedence order `|` < `^` < `&` (bitwise-OR binds loosest of
    // the three). Each is left-associative. They lower to the meta-HIR
    // `BinOp::BitOr`/`BitXor`/`BitAnd` (capability-ahead — already defined);
    // the C emit path renders them as fully-parenthesized Rust infix so the
    // C-intended grouping survives Rust's different native precedence.
    fn parse_bitor(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_bitxor()?;
        while matches!(self.peek(), Some(Tok::Pipe)) {
            self.bump();
            let rhs = self.parse_bitxor()?;
            lhs = bin(BinOp::BitOr, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_bitxor(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_bitand()?;
        while matches!(self.peek(), Some(Tok::Caret)) {
            self.bump();
            let rhs = self.parse_bitand()?;
            lhs = bin(BinOp::BitXor, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_bitand(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_equality()?;
        while matches!(self.peek(), Some(Tok::Amp)) {
            self.bump();
            let rhs = self.parse_equality()?;
            lhs = bin(BinOp::BitAnd, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_equality(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_relational()?;
        loop {
            let op = match self.peek() {
                Some(Tok::EqEq) => BinOp::Eq,
                Some(Tok::Ne) => BinOp::NotEq,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_relational()?;
            lhs = bin(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_relational(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_shift()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Lt) => BinOp::Lt,
                Some(Tok::Le) => BinOp::LtEq,
                Some(Tok::Gt) => BinOp::Gt,
                Some(Tok::Ge) => BinOp::GtEq,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_shift()?;
            lhs = bin(op, lhs, rhs);
        }
        Ok(lhs)
    }

    // PMAT-964: the C shift level sits BELOW relational and ABOVE additive
    // (`a + b << c` parses as `(a + b) << c`). Left-associative. Lowers to
    // the meta-HIR `BinOp::Shl`/`Shr`; the C emit path renders fully
    // parenthesized so Rust's different shift precedence cannot regroup it.
    fn parse_shift(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Shl) => BinOp::Shl,
                Some(Tok::Shr) => BinOp::Shr,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_additive()?;
            lhs = bin(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_additive(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => BinOp::Add,
                Some(Tok::Minus) => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_multiplicative()?;
            lhs = bin(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_unary()?;
        loop {
            // C `/` truncates toward zero and `%` takes the sign of the
            // dividend. We reuse `BinOp::FloorDiv`/`BinOp::Mod` as the IR
            // carriers; the isolated C emit path renders them as Rust
            // `wrapping_div` / `wrapping_rem` (truncating, UB-safe), NOT
            // the Python floor the shared variants imply. (PMAT-538: that
            // Python floor is `checked_div` + a floor correction, not
            // `div_euclid` — Euclidean division is not floor division for
            // a negative divisor.)
            let op = match self.peek() {
                Some(Tok::Star) => BinOp::Mul,
                Some(Tok::Slash) => BinOp::FloorDiv,
                Some(Tok::Percent) => BinOp::Mod,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_unary()?;
            lhs = bin(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Tok::Minus) => {
                self.bump();
                let operand = self.parse_unary()?;
                Ok(Expr::UnOp {
                    op: UnOp::Neg,
                    operand: Box::new(operand),
                })
            }
            Some(Tok::Bang) => {
                self.bump();
                let operand = self.parse_unary()?;
                Ok(Expr::UnOp {
                    op: UnOp::Not,
                    operand: Box::new(operand),
                })
            }
            // PMAT-964: C unary `~` (bitwise NOT / one's complement) →
            // meta-HIR `UnOp::BitNot`. The C emit path already renders this as
            // Rust `!(operand)` (Rust `!` on an integer is bitwise NOT).
            Some(Tok::Tilde) => {
                self.bump();
                let operand = self.parse_unary()?;
                Ok(Expr::UnOp {
                    op: UnOp::BitNot,
                    operand: Box::new(operand),
                })
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.bump().cloned() {
            Some(Tok::Num(v)) => Ok(Expr::LitInt(v)),
            // PMAT-910: a C float literal (`2.0`) → meta-HIR Expr::LitFloat.
            Some(Tok::FNum(v)) => Ok(Expr::LitFloat(v)),
            Some(Tok::LParen) => {
                let e = self.parse_expr()?;
                self.eat(&Tok::RParen)?;
                Ok(e)
            }
            Some(Tok::Ident(name)) => {
                // function call?
                if matches!(self.peek(), Some(Tok::LParen)) {
                    self.bump();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Tok::RParen)) {
                        loop {
                            args.push(self.parse_expr()?);
                            match self.peek() {
                                Some(Tok::Comma) => {
                                    self.bump();
                                }
                                _ => break,
                            }
                        }
                    }
                    self.eat(&Tok::RParen)?;
                    Ok(Expr::Call { callee: name, args })
                } else {
                    Ok(Expr::Ident(name))
                }
            }
            other => Err(format!("unexpected token {other:?} in expression")),
        }
    }
}

fn bin(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinOp {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

/// Set `Stmt::Let { mutable }` to true for any local that is reassigned
/// (`x = e;`) somewhere in the function — including inside a `while`
/// body. The Rust backend emits `let mut` only for these, keeping the
/// emitted code clean under `clippy -D warnings`.
fn mark_mutable(stmts: &mut [Stmt]) {
    let mut reassigned = std::collections::HashSet::new();
    collect_reassigned(stmts, &mut reassigned);
    set_let_mut(stmts, &reassigned);
}

fn collect_reassigned(stmts: &[Stmt], out: &mut std::collections::HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::Assign { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::While { body, .. } => collect_reassigned(body, out),
            // PMAT-478 (R9): a local reassigned in either branch is mutable.
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect_reassigned(then_body, out);
                collect_reassigned(else_body, out);
            }
            _ => {}
        }
    }
}

fn set_let_mut(stmts: &mut [Stmt], reassigned: &std::collections::HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::Let { name, mutable, .. } => {
                if reassigned.contains(name) {
                    *mutable = true;
                }
            }
            Stmt::While { body, .. } => set_let_mut(body, reassigned),
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                set_let_mut(then_body, reassigned);
                set_let_mut(else_body, reassigned);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn lower(src: &str) -> Module {
        CFrontend
            .parse_and_lower(&PathBuf::from("test.c"), src)
            .expect("parse")
    }

    #[test]
    fn parses_add() {
        let m = lower("int add(int a, int b) { return a + b; }");
        assert_eq!(m.source_lang, SourceLang::C);
        assert_eq!(m.items.len(), 1);
        let Item::Function(f) = &m.items[0] else {
            unreachable!("test fixture has no module constants")
        };
        assert_eq!(f.name, "add");
        assert_eq!(f.params.len(), 2);
        assert!(matches!(
            f.body.trailing_return,
            Expr::BinOp { op: BinOp::Add, .. }
        ));
    }

    #[test]
    fn parses_recursive_factorial_with_ternary() {
        let m = lower("int factorial(int n) { return n <= 1 ? 1 : n * factorial(n - 1); }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!("test fixture has no module constants")
        };
        assert_eq!(f.name, "factorial");
        assert!(matches!(f.body.trailing_return, Expr::IfExpr { .. }));
    }

    #[test]
    fn parses_local_decls() {
        let m = lower("int f(int x) { int y = x * 2; int z = y + 1; return z; }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!("test fixture has no module constants")
        };
        assert_eq!(f.body.stmts.len(), 2);
        assert!(matches!(f.body.trailing_return, Expr::Ident(ref n) if n == "z"));
    }

    #[test]
    fn void_params_and_comments() {
        let m = lower("// answer\nint answer(void) { return 42; }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!("test fixture has no module constants")
        };
        assert!(f.params.is_empty());
        assert!(matches!(f.body.trailing_return, Expr::LitInt(42)));
    }

    #[test]
    fn rejects_empty() {
        assert!(CFrontend
            .parse_and_lower(&PathBuf::from("x.c"), "   ")
            .is_err());
    }

    #[test]
    fn parses_while_with_reassignment_marks_mut() {
        let m = lower(
            "int sum_to(int n) { int s = 0; int i = 1; while (i <= n) { s = s + i; i = i + 1; } return s; }",
        );
        let Item::Function(f) = &m.items[0] else {
            unreachable!("test fixture has no module constants")
        };
        // `s` and `i` are reassigned inside the loop → both `let mut`.
        let muts: Vec<bool> = f
            .body
            .stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::Let { mutable, .. } => Some(*mutable),
                _ => None,
            })
            .collect();
        assert_eq!(muts, vec![true, true], "reassigned locals must be mutable");
        assert!(matches!(f.body.stmts.last(), Some(Stmt::While { .. })));
    }

    #[test]
    fn parses_truncating_div_and_mod() {
        let m = lower("int f(int a, int b) { return a / b % 2; }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!("test fixture has no module constants")
        };
        // `/` and `%` carried as FloorDiv/Mod (C-truncating in the backend).
        assert!(matches!(
            f.body.trailing_return,
            Expr::BinOp { op: BinOp::Mod, .. }
        ));
    }

    #[test]
    fn parses_long_as_distinct_clong_width() {
        // PMAT-909: `long`/`int64_t` lower to the distinct 64-bit CLong
        // width (params + return), kept apart from `int` → I64.
        let m = lower("long widen(long x, int n) { long acc = x; return acc; }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!("test fixture has no module constants")
        };
        assert_eq!(f.return_type, Type::CLong, "long return → CLong");
        assert_eq!(f.params[0].ty, Type::CLong, "long param → CLong");
        assert_eq!(f.params[1].ty, Type::I64, "int param stays I64");
        let Stmt::Let { ty, .. } = &f.body.stmts[0] else {
            unreachable!("first stmt is the `long acc` decl")
        };
        assert_eq!(*ty, Type::CLong, "long local → CLong");
    }

    #[test]
    fn int64_t_and_long_long_alias_to_clong() {
        // `int64_t` and `long long` both fold to the single CLong width.
        let m = lower("int64_t f(long long x) { return x; }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!("test fixture has no module constants")
        };
        assert_eq!(f.return_type, Type::CLong, "int64_t return → CLong");
        assert_eq!(f.params.len(), 1);
        assert_eq!(f.params[0].ty, Type::CLong, "`long long` param → CLong");
    }

    #[test]
    fn parses_unsigned_as_distinct_cuint_width() {
        // PMAT-918: `unsigned int` lowers to the distinct 32-bit UNSIGNED
        // CUInt width (params + return + local), kept apart from the signed
        // `int` → I64. The `unsigned int` two-keyword form folds to one width.
        let m =
            lower("unsigned int wrap(unsigned int x, int n) { unsigned int acc = x; return acc; }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!("test fixture has no module constants")
        };
        assert_eq!(f.return_type, Type::CUInt, "unsigned int return → CUInt");
        assert_eq!(f.params[0].ty, Type::CUInt, "unsigned int param → CUInt");
        assert_eq!(f.params[1].ty, Type::I64, "signed int param stays I64");
        let Stmt::Let { ty, .. } = &f.body.stmts[0] else {
            unreachable!("first stmt is the `unsigned int acc` decl")
        };
        assert_eq!(*ty, Type::CUInt, "unsigned int local → CUInt");
    }

    #[test]
    fn bare_unsigned_and_uint32_t_alias_to_cuint() {
        // PMAT-918: bare `unsigned` (no `int`) and `uint32_t` both fold to the
        // single CUInt width — `unsigned` is a complete type, `int` optional.
        let m = lower("unsigned f(uint32_t x) { return x; }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!("test fixture has no module constants")
        };
        assert_eq!(f.return_type, Type::CUInt, "bare unsigned return → CUInt");
        assert_eq!(f.params.len(), 1);
        assert_eq!(f.params[0].ty, Type::CUInt, "uint32_t param → CUInt");
    }

    #[test]
    fn parses_unsigned_long_as_distinct_culong_width() {
        // PMAT-921: `unsigned long` lowers to the distinct 64-bit UNSIGNED
        // CULong width (params + return + local), kept apart from BOTH the
        // 32-bit-unsigned `unsigned int` → CUInt AND the 64-bit-SIGNED
        // `long` → CLong. The `unsigned long` two-keyword form folds to one
        // width; a sibling `unsigned int` param stays 32-bit CUInt.
        let m = lower(
            "unsigned long wrap(unsigned long x, unsigned int n) { unsigned long acc = x; return acc; }",
        );
        let Item::Function(f) = &m.items[0] else {
            unreachable!("test fixture has no module constants")
        };
        assert_eq!(f.return_type, Type::CULong, "unsigned long return → CULong");
        assert_eq!(f.params[0].ty, Type::CULong, "unsigned long param → CULong");
        assert_eq!(
            f.params[1].ty,
            Type::CUInt,
            "unsigned int param stays 32-bit CUInt"
        );
        let Stmt::Let { ty, .. } = &f.body.stmts[0] else {
            unreachable!("first stmt is the `unsigned long acc` decl")
        };
        assert_eq!(*ty, Type::CULong, "unsigned long local → CULong");
    }

    #[test]
    fn uint64_t_and_unsigned_long_long_alias_to_culong() {
        // PMAT-921: `uint64_t` and `unsigned long long` both fold to the single
        // 64-bit-unsigned CULong width — distinct from the signed `int64_t`/
        // `long long` (CLong) and the 32-bit `uint32_t` (CUInt).
        let m = lower("uint64_t f(unsigned long long x) { return x; }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!("test fixture has no module constants")
        };
        assert_eq!(f.return_type, Type::CULong, "uint64_t return → CULong");
        assert_eq!(f.params.len(), 1);
        assert_eq!(
            f.params[0].ty,
            Type::CULong,
            "`unsigned long long` param → CULong"
        );
    }

    #[test]
    fn parses_double_as_f64() {
        // PMAT-910: `double` lowers to the meta-HIR F64 width (params +
        // return + local), the ABI-honest 64-bit float token.
        let m = lower("double square(double x) { double y = x; return y * x; }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!("test fixture has no module constants")
        };
        assert_eq!(f.return_type, Type::F64, "double return → F64");
        assert_eq!(f.params[0].ty, Type::F64, "double param → F64");
        let Stmt::Let { ty, .. } = &f.body.stmts[0] else {
            unreachable!("first stmt is the `double y` decl")
        };
        assert_eq!(*ty, Type::F64, "double local → F64");
        assert!(matches!(
            f.body.trailing_return,
            Expr::BinOp { op: BinOp::Mul, .. }
        ));
    }

    #[test]
    fn parses_double_float_literal() {
        // PMAT-910: a `<digits>.<digits>` C float literal → Expr::LitFloat,
        // distinct from the integer-literal path.
        let m = lower("double scale(double x) { return x * 2.0 + 0.5; }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!("test fixture has no module constants")
        };
        // `x * 2.0 + 0.5` → Add( Mul(x, 2.0), 0.5 )
        let Expr::BinOp {
            op: BinOp::Add,
            lhs,
            rhs,
        } = &f.body.trailing_return
        else {
            unreachable!("trailing return is an addition")
        };
        assert!(
            matches!(**rhs, Expr::LitFloat(v) if v == 0.5),
            "0.5 → LitFloat"
        );
        let Expr::BinOp {
            op: BinOp::Mul,
            rhs: mul_rhs,
            ..
        } = &**lhs
        else {
            unreachable!("lhs is a multiplication")
        };
        assert!(
            matches!(**mul_rhs, Expr::LitFloat(v) if v == 2.0),
            "2.0 → LitFloat"
        );
    }

    #[test]
    fn parses_float_as_distinct_f32_width() {
        // PMAT-911: `float` lowers to the meta-HIR F32 width (params +
        // return + local) — the ABI-honest 32-bit float token, kept DISTINCT
        // from the 64-bit `double`/F64 (which it must never fold into).
        let m = lower("float square(float x) { float y = x; return y * x; }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!("test fixture has no module constants")
        };
        assert_eq!(f.return_type, Type::F32, "float return → F32");
        assert_eq!(f.params[0].ty, Type::F32, "float param → F32");
        let Stmt::Let { ty, .. } = &f.body.stmts[0] else {
            unreachable!("first stmt is the `float y` decl")
        };
        assert_eq!(*ty, Type::F32, "float local → F32");
        // The 32↔64 ABI honesty: `float` is F32, never widened to F64.
        assert_ne!(f.return_type, Type::F64, "float must NOT fold into F64");
    }

    #[test]
    fn float_and_double_lower_to_distinct_widths() {
        // PMAT-911 regression: `float` → F32 and `double` → F64 are kept
        // apart, exactly as `int`/`long` (I64/CLong) are for the integer
        // widths — one C float type per meta-HIR width.
        let ff = lower("float f(float x) { return x; }");
        let dd = lower("double g(double x) { return x; }");
        let Item::Function(f) = &ff.items[0] else {
            unreachable!()
        };
        let Item::Function(g) = &dd.items[0] else {
            unreachable!()
        };
        assert_eq!(f.params[0].ty, Type::F32);
        assert_eq!(g.params[0].ty, Type::F64);
        assert_ne!(f.params[0].ty, g.params[0].ty, "F32 and F64 are distinct");
    }

    #[test]
    fn parses_bitwise_ops_to_meta_hir_binops() {
        // PMAT-964: `& | ^` lower to the distinct meta-HIR bitwise BinOps,
        // NOT the logical `And`/`Or`.
        let band = lower("int f(int a, int b) { return a & b; }");
        let Item::Function(f) = &band.items[0] else {
            unreachable!()
        };
        assert!(matches!(
            f.body.trailing_return,
            Expr::BinOp {
                op: BinOp::BitAnd,
                ..
            }
        ));
        let bor = lower("int f(int a, int b) { return a | b; }");
        let Item::Function(g) = &bor.items[0] else {
            unreachable!()
        };
        assert!(matches!(
            g.body.trailing_return,
            Expr::BinOp {
                op: BinOp::BitOr,
                ..
            }
        ));
        let bxor = lower("int f(int a, int b) { return a ^ b; }");
        let Item::Function(h) = &bxor.items[0] else {
            unreachable!()
        };
        assert!(matches!(
            h.body.trailing_return,
            Expr::BinOp {
                op: BinOp::BitXor,
                ..
            }
        ));
    }

    #[test]
    fn parses_shift_ops_to_meta_hir_binops() {
        // PMAT-964: `<< >>` lower to Shl/Shr.
        let shl = lower("int f(int x, int n) { return x << n; }");
        let Item::Function(f) = &shl.items[0] else {
            unreachable!()
        };
        assert!(matches!(
            f.body.trailing_return,
            Expr::BinOp { op: BinOp::Shl, .. }
        ));
        let shr = lower("int f(int x, int n) { return x >> n; }");
        let Item::Function(g) = &shr.items[0] else {
            unreachable!()
        };
        assert!(matches!(
            g.body.trailing_return,
            Expr::BinOp { op: BinOp::Shr, .. }
        ));
    }

    #[test]
    fn parses_tilde_as_bitnot_distinct_from_logical_not() {
        // PMAT-964: unary `~` → UnOp::BitNot (one's complement), distinct
        // from the logical `!` → UnOp::Not.
        let m = lower("int f(int x) { return ~x; }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!()
        };
        assert!(matches!(
            f.body.trailing_return,
            Expr::UnOp {
                op: UnOp::BitNot,
                ..
            }
        ));
        let n = lower("int g(int x) { return !x; }");
        let Item::Function(g) = &n.items[0] else {
            unreachable!()
        };
        assert!(matches!(
            g.body.trailing_return,
            Expr::UnOp { op: UnOp::Not, .. }
        ));
    }

    #[test]
    fn bitwise_and_shift_obey_c_precedence() {
        // PMAT-964: C binds `<<` BELOW `+` and `&` BELOW `==`/`<<`. So
        // `(a + b) << 1 & 255` parses as `((a + b) << 1) & 255` — the
        // outermost op is the bitwise `&`, with a shift of an addition inside.
        let m = lower("int f(int a, int b) { return (a + b) << 1 & 255; }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!()
        };
        let Expr::BinOp {
            op: BinOp::BitAnd,
            lhs,
            ..
        } = &f.body.trailing_return
        else {
            unreachable!("outermost op is `&` (binds loosest of the three here)")
        };
        // lhs of the `&` is the shift `(a + b) << 1`.
        let Expr::BinOp {
            op: BinOp::Shl,
            lhs: shl_lhs,
            ..
        } = &**lhs
        else {
            unreachable!("`&` left operand is the `<<` shift")
        };
        // and the shift's lhs is the addition `a + b`.
        assert!(
            matches!(**shl_lhs, Expr::BinOp { op: BinOp::Add, .. }),
            "the shift's left operand is the `a + b` addition"
        );
    }

    #[test]
    fn single_amp_is_bitand_not_logical_and() {
        // PMAT-964 regression: a SINGLE `&` must not be swallowed by the
        // `&&` lexer arm. `a & b` is BitAnd; `a && b` is And.
        let single = lower("int f(int a, int b) { return a & b; }");
        let Item::Function(f) = &single.items[0] else {
            unreachable!()
        };
        assert!(matches!(
            f.body.trailing_return,
            Expr::BinOp {
                op: BinOp::BitAnd,
                ..
            }
        ));
        let double = lower("int g(int a, int b) { return a && b; }");
        let Item::Function(g) = &double.items[0] else {
            unreachable!()
        };
        assert!(matches!(
            g.body.trailing_return,
            Expr::BinOp { op: BinOp::And, .. }
        ));
    }

    #[test]
    fn rejects_return_inside_while() {
        assert!(CFrontend
            .parse_and_lower(
                &PathBuf::from("x.c"),
                "int f(int n) { while (n > 0) { return n; } return 0; }"
            )
            .is_err());
    }

    #[test]
    fn parses_int_pointer_param_as_ptr() {
        // PMAT-924: a C `int*` param lifts to the first address-carrying
        // meta-HIR token — Type::Ptr { mutable: true } over the scalar pointee.
        let m = lower("int deref0(int* p) { return 0; }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!()
        };
        assert_eq!(
            f.params[0].ty,
            Type::Ptr {
                mutable: true,
                pointee: Box::new(Type::I64),
            },
            "`int*` param → Ptr over I64 (a bare `int*` is `*mut`)"
        );
        assert_eq!(f.return_type, Type::I64, "scalar return unchanged");
    }

    #[test]
    fn parses_char_pointer_as_ptr_over_cchar() {
        // PMAT-924: `char*` is the canonical C-string pointer — Ptr over the
        // pointee-only CChar. `char` lexes (Tok::Char) but is valid only behind
        // a `*`.
        let m = lower("int len(char* s) { return 0; }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!()
        };
        assert_eq!(
            f.params[0].ty,
            Type::Ptr {
                mutable: true,
                pointee: Box::new(Type::CChar),
            },
            "`char*` param → Ptr over CChar"
        );
    }

    #[test]
    fn parses_pointer_return_type() {
        // PMAT-924: a pointer in RETURN position lifts the same way (the FFI
        // boundary surface is params + return).
        let m = lower("double* identity(double* p) { return p; }");
        let Item::Function(f) = &m.items[0] else {
            unreachable!()
        };
        let ptr_f64 = Type::Ptr {
            mutable: true,
            pointee: Box::new(Type::F64),
        };
        assert_eq!(f.return_type, ptr_f64, "`double*` return → Ptr over F64");
        assert_eq!(f.params[0].ty, ptr_f64, "`double*` param → Ptr over F64");
    }

    #[test]
    fn rejects_pointer_to_pointer() {
        // PMAT-924: only a SINGLE level of indirection is lifted — `int**`
        // (pointer-to-pointer) is refused rather than mis-emitted.
        let err = CFrontend
            .parse_and_lower(&PathBuf::from("x.c"), "int f(int** pp) { return 0; }")
            .expect_err("int** refused");
        assert!(
            format!("{err:?}").contains("pointer-to-pointer"),
            "error names the pointer-to-pointer gap: {err:?}"
        );
    }

    #[test]
    fn rejects_bare_char_value() {
        // PMAT-924: a bare `char` (non-pointer) has no meta-HIR scalar width —
        // it is valid ONLY as a `char*` pointee. A `char c` param is refused.
        let err = CFrontend
            .parse_and_lower(&PathBuf::from("x.c"), "int f(char c) { return 0; }")
            .expect_err("bare char refused");
        assert!(
            format!("{err:?}").contains("char"),
            "error names the bare-char gap: {err:?}"
        );
    }
}
