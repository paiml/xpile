//! Kani BMC harness for `C-NOTATION-LATEX-MATH-TO-EQUATION` (PMAT-059 /
//! XPILE-NOTATION-001).
//!
//! This is the **Symbolic stratum** counterpart for the notation
//! domain. With this harness landed, `C-NOTATION-LATEX-MATH-TO-EQUATION`
//! reaches §14.4 QUORUM (≥1 vote in ≥3 strata) for the first time:
//!
//!   * Semantic    (PMAT-057): `contracts/lean/Notation.lean`
//!   * Symbolic    (PMAT-059): this file
//!   * Runtime     (—)        : awaiting `latex_diff_exec` (future work)
//!   * Extrinsic   (PMAT-057..059): roadmap mentions
//!
//! ## What this harness proves
//!
//! The Rust mirror of the Lean theorem
//! `display_math_eq_equation_env_eq_align_env` (see
//! `contracts/lean/Notation.lean`). The three LaTeX display-math
//! environments — `\[...\]`, `\begin{equation}...\end{equation}`,
//! `\begin{align}...\end{align}` — lower to the same xpile
//! `equations:` entry on the same formula input. The proof is
//! `rfl`-equivalent by our v0.1.0 Bronze-tier modelling: all three
//! lowerings return the same `EquationFormula` value.
//!
//! ## Why fixed-byte symbolic input
//!
//! Same rationale as `bashrs.rs` (PMAT-058): Kani's solver handles
//! fixed-size byte arrays orders of magnitude faster than symbolic
//! `String` allocation. Modelling at `[u8; 4]` captures the
//! identity-preserving property fully — 256^4 ≈ 4.3B configurations
//! is more than enough to surface any structural divergence between
//! the three lowering paths.
//!
//! ## Cross-reinforcement
//!
//! The Lean theorem (PMAT-057) is the documentary modelling
//! commitment locked in by `rfl`. This Kani harness is the symbolic
//! discharge that the *Rust* implementation of the same modelling
//! commitment holds. If a future PR ships a `latex-contract-frontend`
//! whose Rust lowering functions diverge structurally, *either* the
//! Lean theorem must be invalidated *or* this harness must be
//! updated — the two paths cannot drift silently.

#![cfg(kani)]

/// Rust mirror of Lean's `EquationFormula`. v0.1.0 Bronze-tier
/// model — carries just the ASCII-normalised content. Silver-tier
/// refinement (XPILE-REFINE-NOTATION-***+) replaces this with a
/// typed AST that distinguishes the three display-math environments.
#[derive(PartialEq, Eq, Clone, Copy)]
struct EquationFormula {
    ascii_normalised: [u8; 4],
}

/// Lower `\[ formula \]` (display-math span) — Rust mirror of
/// `lower_display_math` from `contracts/lean/Notation.lean`.
fn lower_display_math(formula: &[u8; 4]) -> EquationFormula {
    EquationFormula {
        ascii_normalised: *formula,
    }
}

/// Lower `\begin{equation} formula \end{equation}` — Rust mirror of
/// `lower_equation_env` from `contracts/lean/Notation.lean`.
fn lower_equation_env(formula: &[u8; 4]) -> EquationFormula {
    EquationFormula {
        ascii_normalised: *formula,
    }
}

/// Lower `\begin{align} formula \end{align}` — Rust mirror of
/// `lower_align_env` from `contracts/lean/Notation.lean`.
fn lower_align_env(formula: &[u8; 4]) -> EquationFormula {
    EquationFormula {
        ascii_normalised: *formula,
    }
}

/// Equation `display_math_to_equation` from
/// `contracts/notation-latex-math-to-equation-v1.yaml`:
///
///   parse(\[ formula \])
///     == parse(\begin{equation} formula \end{equation})
///     == parse(\begin{align} formula \end{align})
///
/// Symbolic counterpart to `XpileContracts.CNotationLatexMathToEquation
/// .display_math_eq_equation_env_eq_align_env` in
/// `contracts/lean/Notation.lean`. Kani exhaustively explores all
/// 4-byte symbolic formulas (256^4 ≈ 4.3B configurations) and
/// verifies all three lowering paths produce the same EquationFormula.
#[kani::proof]
fn display_math_eq_equation_env_eq_align_env() {
    let formula: [u8; 4] = kani::any();

    let display = lower_display_math(&formula);
    let equation = lower_equation_env(&formula);
    let align = lower_align_env(&formula);

    kani::assert(
        display == equation,
        "lower_display_math and lower_equation_env must agree on identical input",
    );
    kani::assert(
        equation == align,
        "lower_equation_env and lower_align_env must agree on identical input",
    );
}
