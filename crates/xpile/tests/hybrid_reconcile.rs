//! PMAT-896 (Sprint-2 Tier 2 — Phase-5 hybrid): end-to-end FFI reconciliation.
//!
//! The first time the whole hybrid symbol-resolution path runs through TWO real
//! frontends: the Python frontend detects a relative-import boundary
//! (`from ._core import square_sum` → a `Python→C` `FfiBoundary`), the C frontend
//! lowers the sibling extension to an `Item::Function` named `square_sum`, and
//! `FfiManifest::reconcile` pairs them — or fails when the C side doesn't export
//! the symbol (the `manifest_completeness` invariant of `C-FFI-CPYTHON-EXT`).

use decy_frontend::CFrontend;
use depyler_frontend::PythonFrontend;
use std::path::Path;
use xpile_ffi_manifest::FfiManifest;
use xpile_frontend::Frontend;
use xpile_meta_hir::SourceLang;

const PY_IMPORTS_SQUARE_SUM: &str =
    "from ._core import square_sum\ndef main() -> None:\n    pass\n";

#[test]
fn python_c_boundary_reconciles_end_to_end() {
    let py = PythonFrontend
        .parse_and_lower(Path::new("app.py"), PY_IMPORTS_SQUARE_SUM)
        .expect("python parses");
    assert_eq!(
        py.ffi_boundaries.len(),
        1,
        "the relative import is a boundary"
    );

    let c = CFrontend
        .parse_and_lower(Path::new("_core.c"), "int square_sum(int x) { return x; }")
        .expect("c parses");

    let manifest = FfiManifest::reconcile(&[py, c]).expect("boundary resolves to the C export");
    assert_eq!(manifest.entries.len(), 1);
    assert_eq!(manifest.entries[0].symbol, "square_sum");
    assert_eq!(manifest.entries[0].from_lang, SourceLang::Python);
    assert_eq!(manifest.entries[0].to_lang, SourceLang::C);
    assert!(manifest.entries[0].shim_id.starts_with("shim_"));
}

#[test]
fn python_c_boundary_fails_when_symbol_absent() {
    let py = PythonFrontend
        .parse_and_lower(Path::new("app.py"), PY_IMPORTS_SQUARE_SUM)
        .expect("python parses");
    // The C module exports a DIFFERENT symbol — reconciliation must fail loud.
    let c = CFrontend
        .parse_and_lower(Path::new("_core.c"), "int other(int x) { return x; }")
        .expect("c parses");

    let err = FfiManifest::reconcile(&[py, c]).expect_err("symbol not exported");
    assert!(err.to_string().contains("square_sum"));
    assert!(err.to_string().contains("defines"));
}
