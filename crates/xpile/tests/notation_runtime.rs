//! Notation contract Runtime stratum fixture (PMAT-274).
//!
//! Closes the audit-design.md §4 "demo fixture" caveat for
//! `C-NOTATION-LATEX-MATH-TO-EQUATION-V1`. Discharges
//! XPILE-NOTATION-RUNTIME-001 future-work flagged in
//! `notation_demo.tex`.
//!
//! ## What this asserts
//!
//! The C-NOTATION-LATEX-MATH-TO-EQUATION-V1 contract's load-bearing
//! claim is that three display-math forms — `\[ ... \]`, `equation`
//! env, `align` env — produce structurally-equal `EquationsBlock`
//! entries on the same `formula` input. The Lean theorem
//! `display_math_eq_equation_env_eq_align_env` (PMAT-057) models
//! this; its Kani BMC mirror (PMAT-059) discharges it symbolically.
//!
//! This file is the **Runtime-stratum** counterpart: parse the
//! canonical `notation_demo.tex` fixture (which exercises all three
//! forms on the same formula) through the real
//! `LatexContractFrontend` and assert the three extracted formulas
//! are byte-identical.
//!
//! ## Why this is the FIFTH contract to escape "demo fixture" status
//!
//! Prior escapees (per audit-design.md §4):
//! - C-PY-INT-ARITH (PMAT-267..268..271)
//! - C-XPILE-BACKEND-TRAIT (PMAT-269)
//! - C-XPILE-FRONTEND-TRAIT (PMAT-269)
//! - C-XPILE-CONTRACT-BACKEND-TRAIT (PMAT-270)
//! - C-XPILE-CONTRACT-FRONTEND-TRAIT (PMAT-270)
//!
//! With C-NOTATION joining the list, the residual demo-fixture count
//! drops from 6 → 5 contracts. The remaining 5 are blocked on
//! upstream feature work (Rust frontend, Lean toolchain, CPython FFI,
//! list types, or §29 specialist emitters).

use latex_contract_frontend::LatexContractFrontend;
use std::path::PathBuf;
use xpile_contract_frontend::ContractFrontend;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// PMAT-274 — Runtime-stratum oracle vote for
/// `C-NOTATION-LATEX-MATH-TO-EQUATION-V1`.
///
/// Parses `notation_demo.tex` (which contains the formula `a^2 + b^2
/// = c^2` in three forms: `\[ \]`, `equation` env, `align` env)
/// through the live `LatexContractFrontend`. Asserts:
///
/// 1. Exactly 3 equations are extracted (one per form).
/// 2. All 3 equations have byte-identical `formula` strings.
///
/// (1) catches a regression that drops one of the forms; (2)
/// catches a regression where one form's body is parsed differently
/// from the others. Together they pin the contract's load-bearing
/// claim against actual on-disk evidence.
#[test]
fn c_notation_runtime_three_forms_produce_equal_formula() {
    let src =
        std::fs::read_to_string(fixture_path("notation_demo.tex")).expect("read notation_demo.tex");

    let frontend = LatexContractFrontend;
    let block = frontend
        .parse_to_equations(&src)
        .expect("parse_to_equations should succeed on notation_demo.tex");

    assert_eq!(
        block.equations.len(),
        3,
        "expected 3 equations (one per form: \\[ \\], equation, align); got {}",
        block.equations.len()
    );

    let formulas: Vec<&str> = block
        .equations
        .values()
        .map(|e| e.formula.as_str())
        .collect();

    let first = formulas[0];
    for (i, f) in formulas.iter().enumerate() {
        assert_eq!(
            *f, first,
            "form {i} formula differs from form 0: form 0 = {first:?}, form {i} = {f:?}"
        );
    }

    // Sanity: the formula in the fixture is `a^2 + b^2 = c^2`.
    assert_eq!(
        first, "a^2 + b^2 = c^2",
        "expected the Pythagorean identity, got {first:?}"
    );
}

/// PMAT-274 — Runtime-stratum determinism vote for the same parse.
///
/// Two invocations of `parse_to_equations` on the on-disk fixture
/// must produce byte-identical `EquationsBlock` outputs. Mirrors the
/// `parse_idempotency` invariant from `trait_runtime_properties.rs`
/// (PMAT-270) but targeted at the C-NOTATION contract's specific
/// fixture so the determinism claim is anchored to real content
/// rather than synthetic.
#[test]
fn c_notation_runtime_parse_is_deterministic_on_notation_demo_fixture() {
    let src =
        std::fs::read_to_string(fixture_path("notation_demo.tex")).expect("read notation_demo.tex");

    let frontend = LatexContractFrontend;
    let first = frontend.parse_to_equations(&src).expect("first parse");
    let second = frontend.parse_to_equations(&src).expect("second parse");

    assert_eq!(
        first, second,
        "C-NOTATION-LATEX-MATH-TO-EQUATION-V1 parse_to_equations \
         non-deterministic on notation_demo.tex"
    );
}
