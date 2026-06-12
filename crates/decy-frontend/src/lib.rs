//! C frontend for xpile (PMAT-467, v0.2.0 Track 2.A).
//!
//! Parses a stack-only, int-typed subset of C and lowers it to the
//! shared meta-HIR. This is the first real `decy-frontend` — it
//! replaces the v0.1.0 35-line stub that returned an empty module.
//!
//! Supported subset (slice 1):
//!   - `int` function definitions with `int` parameters
//!   - local `int` declarations (`int x = <expr>;`)
//!   - a trailing `return <expr>;`
//!   - expressions: integer literals, identifiers, calls (recursion),
//!     `+ - *`, comparisons (`< <= > >= == !=`), `&& ||`, unary `- !`,
//!     the ternary `c ? a : b`, and parentheses
//!
//! Deferred (slice 2+): `/` and `%` (C truncating division), `if` /
//! `while` statements, pointers, structs, strings, multiple types.
//!
//! C arithmetic semantics (fixed-width `i32`, wrapping overflow) are
//! realised in the Rust backend's C emit path keyed on
//! `SourceLang::C`; this frontend keeps the meta-HIR clean (`int` →
//! `Type::I64`, the backend narrows to `i32`). The governing contract
//! `C-C-INT-ARITH` is queued (capability-ahead-of-contract, mirroring
//! the v0.1.2 dict lane).

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
    Int,    // `int` keyword
    Return, // `return` keyword
    Void,   // `void` keyword
    Ident(String),
    Num(i64),
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
                } else {
                    toks.push(Tok::Lt);
                    i += 1;
                }
            }
            '>' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    toks.push(Tok::Ge);
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
            c if c.is_ascii_digit() => {
                let start = i;
                while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                    i += 1;
                }
                let s = &src[start..i];
                let v: i64 = s
                    .parse()
                    .map_err(|_| format!("integer literal `{s}` does not fit in i64"))?;
                toks.push(Tok::Num(v));
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
        // `int NAME ( params ) { body }`
        self.eat(&Tok::Int)?;
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
            return_type: Type::I64,
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
            self.eat(&Tok::Int)?;
            let name = self.parse_ident()?;
            params.push(Param {
                name,
                ty: Type::I64,
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
            Some(Tok::Int) => {
                self.bump();
                let name = self.parse_ident()?;
                self.eat(&Tok::Assign)?;
                let value = self.parse_expr()?;
                self.eat(&Tok::Semi)?;
                Ok(Stmt::Let {
                    name,
                    ty: Type::I64,
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
                "function `{fn_name}`: unexpected token {other:?} — supported statements: `int x = e;`, `x = e;`, `if (c) {{ … }} else {{ … }}`, `while (c) {{ … }}`, then a final `return e;`"
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
        let mut lhs = self.parse_equality()?;
        while matches!(self.peek(), Some(Tok::AndAnd)) {
            self.bump();
            let rhs = self.parse_equality()?;
            lhs = bin(BinOp::And, lhs, rhs);
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
        let mut lhs = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Lt) => BinOp::Lt,
                Some(Tok::Le) => BinOp::LtEq,
                Some(Tok::Gt) => BinOp::Gt,
                Some(Tok::Ge) => BinOp::GtEq,
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
            // the Python floor (`div_euclid`) the shared variants imply.
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
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.bump().cloned() {
            Some(Tok::Num(v)) => Ok(Expr::LitInt(v)),
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
        let Item::Function(f) = &m.items[0];
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
        let Item::Function(f) = &m.items[0];
        assert_eq!(f.name, "factorial");
        assert!(matches!(f.body.trailing_return, Expr::IfExpr { .. }));
    }

    #[test]
    fn parses_local_decls() {
        let m = lower("int f(int x) { int y = x * 2; int z = y + 1; return z; }");
        let Item::Function(f) = &m.items[0];
        assert_eq!(f.body.stmts.len(), 2);
        assert!(matches!(f.body.trailing_return, Expr::Ident(ref n) if n == "z"));
    }

    #[test]
    fn void_params_and_comments() {
        let m = lower("// answer\nint answer(void) { return 42; }");
        let Item::Function(f) = &m.items[0];
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
        let Item::Function(f) = &m.items[0];
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
        let Item::Function(f) = &m.items[0];
        // `/` and `%` carried as FloorDiv/Mod (C-truncating in the backend).
        assert!(matches!(
            f.body.trailing_return,
            Expr::BinOp { op: BinOp::Mod, .. }
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
}
