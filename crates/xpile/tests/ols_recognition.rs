//! PMAT-956 (provable-model-as-code) — the OLS-recognition DISCRIMINATION witness.
//!
//! `Function::is_ols_linear_model()` gates the `C-OLS-MODEL-UNIQUENESS` citation
//! (whose proof lives in the walled-off Mathlib lane, `ols_unique`/`ols_strict`,
//! plus the core-lane `OlsModelUniqueness.lean` and the kani harness). That
//! citation is an HONESTY claim: "this function is an OLS linear-model predictor,
//! and such a fit has a unique minimiser (machine-checked)."
//!
//! The POSITIVE case is already witnessed end-to-end by
//! `contract_citation_integrity.rs` (fixture `ols_model.py` must emit the
//! citation). What was UNTESTED — before this file — is the negative half: the
//! recognition is deliberately STRONG so it does NOT fire on incidentally-linear
//! utilities. Nothing guarded that boundary, so a regression loosening the
//! heuristic (drop the bias requirement, the ≥2-distinct-feature requirement, or
//! the all-float / pure-expression gates) would start stamping FALSE OLS
//! certificates on non-models with no test going red.
//!
//! This witness pins both directions through the REAL `PythonFrontend`: genuine
//! fitted-regression shapes are recognised and cite the certificate; each
//! near-miss (no bias, one feature, bare features, a product of parameters, a
//! repeated feature, an integer model) is REJECTED and does not cite it.

use std::path::Path;

use depyler_frontend::PythonFrontend;
use xpile_frontend::Frontend;
use xpile_meta_hir::{Function, Item, Module};

/// Lower a one-function Python snippet through the real frontend and return the
/// function named `f`.
fn lower_fn(src: &str) -> Function {
    let module: Module = PythonFrontend
        .parse_and_lower(Path::new("ols_probe.py"), src)
        .unwrap_or_else(|e| panic!("frontend must lower the probe:\n{src}\nerror: {e:?}"));
    module
        .items
        .iter()
        .find_map(|it| match it {
            Item::Function(f) if f.name == "f" => Some(f.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("probe defines no function `f`:\n{src}"))
}

fn recognised(src: &str) -> bool {
    lower_fn(src).is_ols_linear_model()
}

fn cites_ols(src: &str) -> bool {
    lower_fn(src)
        .applicable_contracts()
        .contains(&"C-OLS-MODEL-UNIQUENESS")
}

// ── Positive: genuine fitted-regression shapes ──────────────────────────────

#[test]
fn recognises_multi_feature_fitted_model() {
    // 3 distinctly-weighted float features + a literal bias — the unambiguous
    // regression form (the `examples/proven-model/model.py` shape).
    let src = "def f(a: float, b: float, c: float) -> float:\n    return 0.5 * a + 1.5 * b + 2.0 * c + 3.0\n";
    assert!(
        recognised(src),
        "3 weighted features + bias must be recognised"
    );
    assert!(
        cites_ols(src),
        "a recognised model must cite C-OLS-MODEL-UNIQUENESS"
    );
}

#[test]
fn recognises_two_feature_model_with_bias() {
    let src = "def f(a: float, b: float) -> float:\n    return 2.0 * a + 3.0 * b + 1.0\n";
    assert!(
        recognised(src),
        "the minimal shape (2 weighted features + bias) fires"
    );
}

#[test]
fn recognises_weight_on_either_side_of_the_product() {
    // `x * w` must count the same as `w * x` (the `.or_else` swap in
    // `weighted_param_name`).
    let src = "def f(a: float, b: float) -> float:\n    return a * 2.0 + b * 3.0 + 1.0\n";
    assert!(
        recognised(src),
        "weight on the right of the product still recognises"
    );
}

// ── Negative: near-misses that MUST NOT stamp a certificate ─────────────────

#[test]
fn rejects_linear_map_without_bias() {
    // A bias-less linear map is not a fitted model; recognising it would cite an
    // OLS-uniqueness certificate for something the certificate does not cover.
    let src = "def f(a: float, b: float) -> float:\n    return 2.0 * a + 3.0 * b\n";
    assert!(!recognised(src), "no bias → not a fitted model");
    assert!(
        !cites_ols(src),
        "no bias → must not cite C-OLS-MODEL-UNIQUENESS"
    );
}

#[test]
fn rejects_single_feature() {
    let src = "def f(a: float) -> float:\n    return 2.0 * a + 1.0\n";
    assert!(
        !recognised(src),
        "one weighted feature is below the ≥2 threshold"
    );
}

#[test]
fn rejects_bare_feature_sum() {
    // Unweighted features (`a + b`) are not the fitted-regression shape.
    let src = "def f(a: float, b: float) -> float:\n    return a + b + 1.0\n";
    assert!(
        !recognised(src),
        "no literal-weighted feature → not a model"
    );
}

#[test]
fn rejects_product_of_two_parameters() {
    // `a * b` is not `w · x`; a product of two parameters disqualifies the body.
    let src = "def f(a: float, b: float) -> float:\n    return a * b + 1.0\n";
    assert!(!recognised(src), "a·b is not a weighted feature → rejected");
}

#[test]
fn rejects_repeated_feature() {
    // The same feature weighted twice is ONE distinct feature, not two.
    let src = "def f(a: float, b: float) -> float:\n    return 2.0 * a + 3.0 * a + 1.0\n";
    assert!(
        !recognised(src),
        "one DISTINCT feature (a re-weighted) is below ≥2"
    );
}

#[test]
fn rejects_integer_model() {
    // The certificate is over ℝ; an integer model is int-arithmetic, not OLS.
    let src = "def f(a: int, b: int) -> int:\n    return 2 * a + 3 * b + 1\n";
    assert!(
        !recognised(src),
        "non-float model → not the ℝ certificate's domain"
    );
    assert!(
        !cites_ols(src),
        "integer model must not cite the OLS certificate"
    );
}
