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
//! * **Theorem-class environments** — every environment on
//!   [`THEOREM_CLASS_ENVIRONMENTS`] becomes a `ProofObligation`, with its
//!   amsthm `[label]` as `applies_to` and its math as `formal`
//!   (PMAT-1431).
//! * **`\begin{proof}`** — consumed and DISCARDED, so the proof body
//!   never reaches the `EquationsBlock` (PMAT-1431).
//!
//! The parser is a hand-rolled scanner over a small subset of LaTeX —
//! NOT a general LaTeX parser. Out of scope at v0.1.0:
//!
//! * Math environments (`equation`, `align`, `gather`) are handled in
//!   SINGLE-EQUATION form only — a multi-row body is one entry, not one
//!   per row. XPILE-LATEX-PARSE-ALIGN-COLUMNS.
//! * The `lean_pointer` half of proof-env lowering — `EquationsBlock`
//!   has no field for it. XPILE-LATEX-PARSE-LEANPTR-001.
//! * `resolve_label_to_equation_name` is IDENTITY: the amsthm bracket
//!   argument is passed through verbatim, never resolved to an equation
//!   key. XPILE-LATEX-PARSE-LABELRES-001.
//! * Theorem-class environments NESTED inside one another.
//!   XPILE-LATEX-PARSE-THMNEST-001.
//! * Macro expansion, comments-with-special-chars, escaped delimiters.
//!
//! These are flagged as XPILE-LATEX-PARSE-* future work.
//!
//! ⚠️ PMAT-1431 (2026-07-28): the four bullets above are the RESIDUAL of
//! a much larger gap. Until that slice this list said theorem-class
//! environments produced no obligations — true, but a severe
//! understatement of what actually happened. The environments were not
//! recognised AT ALL, so their bodies were re-scanned as ordinary text
//! and their math spans surfaced as free-standing `eq_inline_*`
//! equations: the theorem's content was present, in the bucket
//! `inline_math_to_equation`'s own domain excludes, indistinguishable
//! from an equation the author never wrote. A `proof` body did the same,
//! contradicting `lower_proof_env`'s modelled `body_leaked := false`.
//! `\(...\)` and `gather` produced nothing while a Lean theorem and the
//! contract description respectively asserted they lowered. Every one of
//! these was `Ok`, at exit 0, for 74 days. What holds the list honest now
//! is `notation_surface` in the contract, checked both ways by
//! `crates/xpile/tests/notation_claim_witness.rs`.
//!
//! Layer 2 contract: `contracts/notation-latex-math-to-equation-v1.yaml`.
//!
//! The audit-design.md "citation bridge fragility" concern requires
//! citation extraction via the host format's STRUCTURED parser, not
//! regex over body text. This scanner is a stepping stone — it walks
//! the source linearly and only matches the literal token sequence
//! `\xpileContract{` (with proper brace balance), not a regex.

use xpile_contract_frontend::{
    ContractFrontend, ContractFrontendError, Equation, EquationsBlock, ObligationType,
    ProofObligation,
};
use xpile_contracts::{ContractFormat, ContractId};

/// The amsthm flag that flips an obligation's polarity, per
/// `theorem_env_to_obligation`'s invariant.
const PRECONDITION_FLAG: &str = "\\textbf{Precondition:}";

/// The theorem-class environments this frontend lowers to
/// [`xpile_contract_frontend::ProofObligation`] entries.
///
/// PMAT-1431: this roster is checked for SET EQUALITY, in both
/// directions, against `equations.theorem_env_to_obligation.environments`
/// in `contracts/notation-latex-math-to-equation-v1.yaml` by
/// `crates/xpile/tests/notation_claim_witness.rs`. Adding an environment
/// here without adding it to the contract reds that test, and vice
/// versa.
pub const THEOREM_CLASS_ENVIRONMENTS: &[&str] = &[
    "theorem",
    "lemma",
    "corollary",
    "proposition",
    "claim",
    "definition",
    "remark",
];

/// The environment whose body must never reach the `EquationsBlock`.
/// `contracts/lean/Notation.lean`'s `lower_proof_env` models this as
/// `body_leaked := false`.
pub const PROOF_ENVIRONMENT: &str = "proof";

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
                Token::EquationEnv(formula) => {
                    insert_math_equation(&mut block, &mut eq_index, formula, "equation");
                }
                Token::AlignEnv(formula) => {
                    insert_math_equation(&mut block, &mut eq_index, formula, "align");
                }
                // PMAT-1431: named by this contract's description since
                // 2026-05-15; produced nothing until now.
                Token::GatherEnv(formula) => {
                    insert_math_equation(&mut block, &mut eq_index, formula, "gather");
                }
                // PMAT-1431: the `\(...\)` form. The entry KIND stays in
                // the key so `inline_kinds_are_distinct_silver` is
                // falsifiable — an emitter that relabels one form as the
                // other changes the key.
                Token::ParenMath(formula) => {
                    insert_math_equation(&mut block, &mut eq_index, formula, "paren");
                }
                // PMAT-1431: theorem-class environments lower to
                // obligations, and their body math does NOT become a
                // free-standing equation.
                Token::TheoremEnv { label, body } => {
                    block
                        .proof_obligations
                        .push(lower_theorem_env(&label, &body));
                }
                // PMAT-1431: a `proof` body is CONSUMED, never lowered.
                // `contracts/lean/Notation.lean`'s `lower_proof_env`
                // models this as `body_leaked := false`; before this
                // slice the body's math surfaced as `eq_inline_*`. The
                // `lean_pointer` half of the modelled lowering has no
                // field in `EquationsBlock` and is NOT produced —
                // disclosed as XPILE-LATEX-PARSE-LEANPTR-001 in
                // `notation_surface.unimplemented`.
                Token::ProofEnv => {}
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

/// PMAT-1431: lower one theorem-class environment to a
/// [`ProofObligation`], per `theorem_env_to_obligation`.
///
/// * `ty` — [`ObligationType::Precondition`] iff the body opens with
///   `\textbf{Precondition:}`, else [`ObligationType::Postcondition`].
///   This is the polarity the Lean theorem and the Kani harness of the
///   same name prove; it now has shipped code behind it.
/// * `formal` — the environment's math spans, joined, or the literal
///   `TBD` when the body carries none (`extracted_math_or_TBD`).
/// * `property` — the body with its math spans and the precondition flag
///   removed, whitespace-collapsed.
/// * `applies_to` — the amsthm `[label]` VERBATIM, empty when absent.
///   `resolve_label_to_equation_name` is identity at v0.1.x; amsthm's
///   bracket argument is a title, not a `\label`, so real cross-reference
///   resolution needs `\label{}`/`\ref{}` handling this scanner does not
///   have. XPILE-LATEX-PARSE-LABELRES-001.
fn lower_theorem_env(label: &str, body: &str) -> ProofObligation {
    let trimmed = body.trim();
    let is_precondition = trimmed.starts_with(PRECONDITION_FLAG);
    let statement = trimmed
        .strip_prefix(PRECONDITION_FLAG)
        .unwrap_or(trimmed)
        .trim();

    let math = collect_math_spans(statement);
    ProofObligation {
        ty: if is_precondition {
            ObligationType::Precondition
        } else {
            ObligationType::Postcondition
        },
        property: strip_math_and_collapse(statement),
        formal: if math.is_empty() {
            "TBD".to_string()
        } else {
            math.join(" ; ")
        },
        applies_to: label.to_string(),
    }
}

/// Every math span in `body`, in source order, using the SAME scanner
/// that drives top-level parsing — so the two can never disagree about
/// what counts as math.
fn collect_math_spans(body: &str) -> Vec<String> {
    let mut scanner = Scanner::new(body);
    let mut out = Vec::new();
    while let Some(token) = scanner.next_token() {
        match token {
            Token::InlineMath(f)
            | Token::ParenMath(f)
            | Token::DisplayMath(f)
            | Token::EquationEnv(f)
            | Token::AlignEnv(f)
            | Token::GatherEnv(f) => out.push(f.trim().to_string()),
            _ => {}
        }
    }
    out
}

/// The human-readable half of an obligation: `body` with its math spans
/// elided and runs of whitespace collapsed to one space.
fn strip_math_and_collapse(body: &str) -> String {
    let mut prose = String::with_capacity(body.len());
    let bytes = body.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let rest = &body[i..];
        if let Some(end) = math_span_len(rest) {
            prose.push(' ');
            i += end;
            continue;
        }
        let ch_len = rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        prose.push_str(&rest[..ch_len]);
        i += ch_len;
    }
    prose.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Byte length of the math span starting at `rest`, if one does.
/// Unterminated spans return `None` so the caller keeps the text as
/// prose rather than swallowing the rest of the body.
fn math_span_len(rest: &str) -> Option<usize> {
    for (open, close) in [("\\(", "\\)"), ("\\[", "\\]")] {
        if let Some(inner) = rest.strip_prefix(open) {
            return inner.find(close).map(|e| open.len() + e + close.len());
        }
    }
    if let Some(inner) = rest.strip_prefix('$') {
        return inner.find('$').map(|e| 1 + e + 1);
    }
    None
}

enum Token {
    InlineMath(String),
    /// PMAT-1431: `\( ... \)`, the other inline form the contract names.
    ParenMath(String),
    DisplayMath(String),
    /// PMAT-274: `\begin{equation} ... \end{equation}`.
    EquationEnv(String),
    /// PMAT-274: `\begin{align} ... \end{align}` (single-equation form).
    AlignEnv(String),
    /// PMAT-1431: `\begin{gather} ... \end{gather}` (single-equation form).
    GatherEnv(String),
    /// PMAT-1431: a theorem-class environment from
    /// [`THEOREM_CLASS_ENVIRONMENTS`], with its optional amsthm `[label]`.
    TheoremEnv {
        label: String,
        body: String,
    },
    /// PMAT-1431: `\begin{proof} ... \end{proof}`. Carries no payload —
    /// the body must not escape into the `EquationsBlock`.
    ProofEnv,
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

            // PMAT-274: equation environment.
            //   \begin{equation} ... \end{equation}
            if rest.starts_with("\\begin{equation}") {
                self.advance("\\begin{equation}".len());
                if let Some(end) = self.rest().find("\\end{equation}") {
                    let formula = self.rest()[..end].to_string();
                    self.advance(end + "\\end{equation}".len());
                    return Some(Token::EquationEnv(formula));
                }
                // Unterminated equation env — stop scanning.
                return None;
            }

            // PMAT-274: align environment (single-equation form).
            //   \begin{align} ... \end{align}
            // Numbered sub-equations (`\\` separators inside the
            // environment) are NOT individually extracted at v0.1.0+ —
            // the entire body is one EquationsBlock entry. Documented
            // as XPILE-LATEX-PARSE-ALIGN-COLUMNS future work.
            if rest.starts_with("\\begin{align}") {
                self.advance("\\begin{align}".len());
                if let Some(end) = self.rest().find("\\end{align}") {
                    let formula = self.rest()[..end].to_string();
                    self.advance(end + "\\end{align}".len());
                    return Some(Token::AlignEnv(formula));
                }
                return None;
            }

            // PMAT-1431: gather environment (single-equation form),
            // named by the contract description since 2026-05-15.
            if rest.starts_with("\\begin{gather}") {
                self.advance("\\begin{gather}".len());
                if let Some(end) = self.rest().find("\\end{gather}") {
                    let formula = self.rest()[..end].to_string();
                    self.advance(end + "\\end{gather}".len());
                    return Some(Token::GatherEnv(formula));
                }
                return None;
            }

            // PMAT-1431: `\begin{proof} ... \end{proof}`. Consumed whole
            // and DISCARDED — `lower_proof_env` models the proof body as
            // never reaching the EquationsBlock (`body_leaked := false`).
            if rest.starts_with("\\begin{proof}") {
                self.advance("\\begin{proof}".len());
                match self.rest().find("\\end{proof}") {
                    Some(end) => {
                        self.advance(end + "\\end{proof}".len());
                        return Some(Token::ProofEnv);
                    }
                    // Unterminated: stop rather than fall through and
                    // re-scan the body as free-standing math.
                    None => return None,
                }
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

            // PMAT-1431: inline math, paren form: \( ... \). Must be
            // tested AFTER `\[` above, since both open with a backslash.
            if rest.starts_with("\\(") {
                self.advance(2);
                if let Some(end) = self.rest().find("\\)") {
                    let formula = self.rest()[..end].to_string();
                    self.advance(end + 2);
                    return Some(Token::ParenMath(formula));
                }
                // Unterminated paren math — stop scanning, matching the
                // established behaviour of the other unterminated forms.
                return None;
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

            // PMAT-1431: theorem-class environments, matched against the
            // roster the contract declares so the two cannot drift.
            // Placed last because it needs `&mut self` while the `rest`
            // borrow above is still live; nothing earlier can match a
            // `\begin{<theorem-class>}` opener, so position is immaterial
            // to behaviour.
            if let Some(token) = self.try_theorem_env() {
                return Some(token);
            }

            // Default: consume one char and continue scanning.
            let next_char_len = self
                .rest()
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
            self.advance(next_char_len);
        }
    }

    /// PMAT-1431: if the scanner is positioned at `\begin{<env>}` for an
    /// env on [`THEOREM_CLASS_ENVIRONMENTS`], consume the whole
    /// environment — optional amsthm `[label]` included — and return it.
    ///
    /// The closing delimiter searched for is the MATCHING `\end{<env>}`,
    /// not the first `\end{...}`. A theorem-class environment nested
    /// inside another is outside `theorem_env_to_obligation`'s stated
    /// preconditions: a nested env of a DIFFERENT class is absorbed into
    /// the outer body (and its math becomes the outer `formal`), and one
    /// of the SAME class closes the outer environment early. Disclosed
    /// as XPILE-LATEX-PARSE-THMNEST-001 in `notation_surface`, not fixed.
    fn try_theorem_env(&mut self) -> Option<Token> {
        let rest = self.rest();
        let env = THEOREM_CLASS_ENVIRONMENTS
            .iter()
            .find(|e| rest.starts_with(&format!("\\begin{{{e}}}")))?;

        let open = format!("\\begin{{{env}}}");
        let close = format!("\\end{{{env}}}");
        let start = self.pos;
        self.advance(open.len());

        // Optional amsthm title argument: `[...]`, brace-free.
        let mut label = String::new();
        if self.rest().starts_with('[') {
            if let Some(end) = self.rest().find(']') {
                label = self.rest()[1..end].to_string();
                self.advance(end + 1);
            }
        }

        match self.rest().find(&close) {
            Some(end) => {
                let body = self.rest()[..end].to_string();
                self.advance(end + close.len());
                Some(Token::TheoremEnv { label, body })
            }
            None => {
                // Unterminated environment. Rewind so the default
                // char-consuming path handles the text, rather than
                // silently swallowing the rest of the document.
                self.pos = start;
                None
            }
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
    fn equation_env_is_extracted() {
        let src = r"\begin{equation}
  a^2 + b^2 = c^2
\end{equation}";
        let block = parse(src);
        assert_eq!(block.equations.len(), 1);
        let (_, eq) = block.equations.iter().next().unwrap();
        assert_eq!(eq.formula, "a^2 + b^2 = c^2");
    }

    #[test]
    fn align_env_is_extracted() {
        let src = r"\begin{align}
  a^2 + b^2 = c^2
\end{align}";
        let block = parse(src);
        assert_eq!(block.equations.len(), 1);
        let (_, eq) = block.equations.iter().next().unwrap();
        assert_eq!(eq.formula, "a^2 + b^2 = c^2");
    }

    /// PMAT-274: the load-bearing claim of C-NOTATION-LATEX-MATH-TO-EQUATION:
    /// `\[ ... \]`, `\begin{equation} ... \end{equation}`, and
    /// `\begin{align} ... \end{align}` produce structurally-equal
    /// `Equation` entries on the same `formula` input. This test is the
    /// concrete observed evidence the Lean theorem
    /// `display_math_eq_equation_env_eq_align_env` (PMAT-057) models.
    #[test]
    fn three_display_math_forms_produce_equal_formulas() {
        let src = r"\[ a^2 + b^2 = c^2 \]
\begin{equation}
  a^2 + b^2 = c^2
\end{equation}
\begin{align}
  a^2 + b^2 = c^2
\end{align}";
        let block = parse(src);
        assert_eq!(
            block.equations.len(),
            3,
            "expected 3 equations from the three forms"
        );
        let formulas: Vec<&str> = block
            .equations
            .values()
            .map(|e| e.formula.as_str())
            .collect();
        for f in &formulas {
            assert_eq!(*f, "a^2 + b^2 = c^2", "form's formula differs: {f:?}");
        }
    }

    #[test]
    fn unterminated_equation_env_does_not_panic() {
        let block = parse(r"\begin{equation} unterminated content");
        assert!(block.equations.is_empty());
    }

    #[test]
    fn unterminated_align_env_does_not_panic() {
        let block = parse(r"\begin{align} unterminated content");
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
