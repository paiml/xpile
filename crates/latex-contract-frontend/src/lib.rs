//! LaTeX contract frontend.
//!
//! Parses a documented LaTeX subset into [`EquationsBlock`]:
//!
//! * **Math spans** — `$...$` (inline) and `\[...\]` (display) become
//!   `Equation` entries keyed by source position.
//! * **`\xpileContract{C-…}{…}` citations** — the first argument is
//!   pushed to `EquationsBlock.citations` as a `ContractId`.
//! * **`\cite{key}` references** — pushed to `EquationsBlock.references`.
//!
//! The parser is a hand-rolled scanner over a small subset of LaTeX —
//! NOT a general LaTeX parser. Out of scope at v0.1.0:
//!
//! * Math environments (`equation`, `align`, `gather`) — only `\[...\]`
//!   and `$...$` delimiters are handled.
//! * Theorem-class environments (`theorem`, `lemma`, `proof`) — no
//!   `proof_obligations` are produced.
//! * Macro expansion, comments-with-special-chars, escaped delimiters.
//!
//! These are flagged as XPILE-LATEX-PARSE-* future work.
//!
//! Layer 2 contract: `contracts/notation-latex-math-to-equation-v1.yaml`.
//!
//! The audit-design.md "citation bridge fragility" concern requires
//! citation extraction via the host format's STRUCTURED parser, not
//! regex over body text. This scanner is a stepping stone — it walks
//! the source linearly and only matches the literal token sequence
//! `\xpileContract{` (with proper brace balance), not a regex.

use xpile_contract_frontend::{ContractFrontend, ContractFrontendError, Equation, EquationsBlock};
use xpile_contracts::{ContractFormat, ContractId};

pub struct LatexContractFrontend;

impl ContractFrontend for LatexContractFrontend {
    fn name(&self) -> &'static str {
        "latex"
    }

    fn formats(&self) -> &[ContractFormat] {
        &[ContractFormat::LatexMath]
    }

    fn parse_to_equations(&self, source: &str) -> Result<EquationsBlock, ContractFrontendError> {
        let mut block = EquationsBlock::default();
        let mut scanner = Scanner::new(source);
        let mut eq_index: usize = 0;

        while let Some(token) = scanner.next_token() {
            match token {
                Token::InlineMath(formula) => {
                    insert_math_equation(&mut block, &mut eq_index, formula, "inline");
                }
                Token::DisplayMath(formula) => {
                    insert_math_equation(&mut block, &mut eq_index, formula, "display");
                }
                Token::XpileContract(id) => {
                    block.citations.push(ContractId::new(id));
                }
                Token::Cite(key) => {
                    block.references.push(key);
                }
            }
        }

        Ok(block)
    }
}

fn insert_math_equation(
    block: &mut EquationsBlock,
    index: &mut usize,
    formula: String,
    kind: &str,
) {
    let key = format!("eq_{kind}_{index}");
    *index += 1;
    block.equations.insert(
        key,
        Equation {
            formula: formula.trim().to_string(),
            domain: String::new(),
            invariants: Vec::new(),
            preconditions: Vec::new(),
        },
    );
}

enum Token {
    InlineMath(String),
    DisplayMath(String),
    XpileContract(String),
    Cite(String),
}

struct Scanner<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Scanner<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn rest(&self) -> &str {
        &self.src[self.pos..]
    }

    fn advance(&mut self, n: usize) {
        self.pos += n;
    }

    fn next_token(&mut self) -> Option<Token> {
        loop {
            let rest = self.rest();
            if rest.is_empty() {
                return None;
            }

            // Skip LaTeX comments — from `%` to end of line. A `%`
            // preceded by `\` is an escaped percent and not a comment.
            if rest.starts_with('%') && !self.prev_char_is('\\') {
                let nl = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
                self.advance(nl);
                continue;
            }

            // Display math: \[ ... \]
            if rest.starts_with("\\[") {
                self.advance(2);
                if let Some(end) = self.rest().find("\\]") {
                    let formula = self.rest()[..end].to_string();
                    self.advance(end + 2);
                    return Some(Token::DisplayMath(formula));
                }
                // Unterminated display math — treat as no token, end
                // of input.
                return None;
            }

            // \xpileContract{ID}{...}
            if rest.starts_with("\\xpileContract{") {
                self.advance("\\xpileContract{".len());
                if let Some(id) = self.scan_balanced_braces() {
                    // Consume the optional second arg block.
                    let r = self.rest();
                    if r.starts_with('{') {
                        self.advance(1);
                        let _ = self.scan_balanced_braces();
                    }
                    return Some(Token::XpileContract(id));
                }
                continue;
            }

            // \cite{key}
            if rest.starts_with("\\cite{") {
                self.advance("\\cite{".len());
                if let Some(key) = self.scan_balanced_braces() {
                    return Some(Token::Cite(key));
                }
                continue;
            }

            // Inline math: $...$ but NOT $$ (display via dollars,
            // which we don't support — the next-best behavior is to
            // skip the doubled dollars cleanly).
            if rest.starts_with("$$") {
                self.advance(2);
                if let Some(end) = self.rest().find("$$") {
                    self.advance(end + 2);
                }
                continue;
            }
            if rest.starts_with('$') {
                self.advance(1);
                if let Some(end) = self.rest().find('$') {
                    let formula = self.rest()[..end].to_string();
                    self.advance(end + 1);
                    return Some(Token::InlineMath(formula));
                }
                // Unterminated inline math — stop scanning.
                return None;
            }

            // Default: consume one char and continue scanning.
            let next_char_len = rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            self.advance(next_char_len);
        }
    }

    /// True if the character just before `self.pos` is `ch`.
    fn prev_char_is(&self, ch: char) -> bool {
        if self.pos == 0 {
            return false;
        }
        self.src[..self.pos].ends_with(ch)
    }

    /// After consuming a `{`, walk forward to the matching `}`,
    /// returning the text in between (without the braces). Returns
    /// `None` if no matching brace is found.
    fn scan_balanced_braces(&mut self) -> Option<String> {
        let mut depth: usize = 1;
        let start = self.pos;
        let bytes = self.src.as_bytes();
        while self.pos < bytes.len() {
            match bytes[self.pos] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let inner = self.src[start..self.pos].to_string();
                        self.advance(1);
                        return Some(inner);
                    }
                }
                b'\\' => {
                    // Skip the escaped char so `\{` and `\}` don't
                    // affect depth.
                    self.advance(1);
                }
                _ => {}
            }
            self.advance(1);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> EquationsBlock {
        LatexContractFrontend.parse_to_equations(src).unwrap()
    }

    #[test]
    fn empty_input_yields_empty_block() {
        let block = parse("");
        assert!(block.equations.is_empty());
        assert!(block.proof_obligations.is_empty());
        assert!(block.citations.is_empty());
        assert!(block.references.is_empty());
    }

    #[test]
    fn inline_math_is_extracted() {
        let block = parse("Pythagoras said $a^2 + b^2 = c^2$.");
        assert_eq!(block.equations.len(), 1);
        let (_key, eq) = block.equations.iter().next().unwrap();
        assert_eq!(eq.formula, "a^2 + b^2 = c^2");
    }

    #[test]
    fn display_math_is_extracted() {
        let block = parse(r"\[ E = mc^2 \]");
        assert_eq!(block.equations.len(), 1);
        let (_key, eq) = block.equations.iter().next().unwrap();
        assert_eq!(eq.formula, "E = mc^2");
    }

    #[test]
    fn multiple_math_spans_are_keyed_distinctly() {
        let block = parse(r"$a$ and $b$ then \[ c \]");
        assert_eq!(block.equations.len(), 3);
    }

    #[test]
    fn xpile_contract_citation_is_collected() {
        let block = parse(r"See \xpileContract{C-PY-INT-ARITH}{addition} for details.");
        assert_eq!(block.citations.len(), 1);
        assert_eq!(block.citations[0].as_str(), "C-PY-INT-ARITH");
    }

    #[test]
    fn cite_reference_is_collected() {
        let block = parse(r"As shown in \cite{einstein1905}.");
        assert_eq!(block.references.len(), 1);
        assert_eq!(block.references[0], "einstein1905");
    }

    #[test]
    fn line_comments_are_skipped() {
        let block = parse("% This is a comment with $fake_math$.\n$real_math$");
        assert_eq!(block.equations.len(), 1);
        let (_, eq) = block.equations.iter().next().unwrap();
        assert_eq!(eq.formula, "real_math");
    }

    #[test]
    fn parse_is_deterministic_on_realistic_fixture() {
        // Same shape as contract_frontend_trait_demo.tex.
        let src = r#"\documentclass{article}
\begin{document}
\section{Basic equations}
A simple math span: $E = mc^2$.
A display math span:
\[ \int_0^{\infty} e^{-x} \, dx = 1 \]
\end{document}
"#;
        let a = parse(src);
        let b = parse(src);
        assert_eq!(a, b);
        assert_eq!(a.equations.len(), 2);
    }

    #[test]
    fn unterminated_display_math_does_not_panic() {
        let block = parse(r"\[ unterminated math goes forever");
        assert!(block.equations.is_empty());
    }

    #[test]
    fn unterminated_inline_math_does_not_panic() {
        let block = parse(r"$unterminated inline");
        assert!(block.equations.is_empty());
    }

    #[test]
    fn double_dollar_blocks_are_skipped_safely() {
        // $$...$$ is unsupported display syntax — skip it without
        // crashing. Other inline `$...$` after the `$$` block still
        // works.
        let block = parse(r"$$ignored$$ then $kept$");
        assert_eq!(block.equations.len(), 1);
        let (_, eq) = block.equations.iter().next().unwrap();
        assert_eq!(eq.formula, "kept");
    }
}
