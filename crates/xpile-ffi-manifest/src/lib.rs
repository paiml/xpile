//! FFI boundary registry.
//!
//! In a hybrid transpile (e.g. Python + C extension, Python + CUDA
//! kernel), the agent must know exactly which symbols cross language
//! lines and what their Rust shim signatures should be. This crate is
//! the single source of truth for that mapping within a session.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use xpile_meta_hir::{FfiBoundary, Item, Module, SourceLang};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FfiManifest {
    pub entries: Vec<FfiEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfiEntry {
    pub symbol: String,
    pub from_lang: SourceLang,
    pub to_lang: SourceLang,
    pub source_signature: String,
    pub rust_shim_signature: String,
    pub shim_id: String,
}

/// PMAT-894 (Sprint-2 Tier 2): a cross-language FFI boundary could not be paired
/// with a defining module during [`FfiManifest::reconcile`]. This is the
/// manifest-completeness failure of `C-FFI-CPYTHON-EXT` (the contract's
/// `manifest_completeness` equation): in a hybrid transpile EVERY boundary that
/// crosses a language line must resolve, or the hybrid build cannot be emitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfiReconcileError {
    pub unresolved: Vec<String>,
}

impl std::fmt::Display for FfiReconcileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FFI reconciliation failed — {} unresolved boundary(ies): {}",
            self.unresolved.len(),
            self.unresolved.join("; ")
        )
    }
}

impl std::error::Error for FfiReconcileError {}

/// Deterministic, dependency-free shim id (FNV-1a hex over the boundary's
/// identifying fields). Stable across runs so a manifest entry — and the shim it
/// names — is reproducible. (The spec's sha256 is a later hardening; FNV-1a is
/// sufficient for in-session uniqueness and is std-only.)
/// PMAT-895 (Sprint-2 Tier 2): the externally-callable name a module item
/// exports — what an incoming FFI boundary's `symbol` must match. A callable FFI
/// symbol is a function; a module-level constant is also addressable. Structs/
/// enums are types, not call targets, so they don't export an FFI symbol.
fn item_exported_name(item: &Item) -> Option<&str> {
    match item {
        Item::Function(f) => Some(&f.name),
        Item::Const { name, .. } => Some(name),
        _ => None,
    }
}

fn shim_id(b: &FfiBoundary) -> String {
    let key = format!(
        "{:?}->{:?}:{}:{}",
        b.from_lang, b.to_lang, b.symbol, b.signature
    );
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.as_bytes() {
        h ^= *byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("shim_{h:016x}")
}

impl FfiManifest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, entry: FfiEntry) {
        self.entries.push(entry);
    }

    /// Phase 2 of the hybrid transpile flow (`hybrid-transpile-flow.md` §16):
    /// reconcile every cross-language FFI boundary across the dispatched modules
    /// into a single manifest, or fail listing the unresolved boundaries.
    ///
    /// A boundary `{from_lang, to_lang, symbol, signature}` is a call that
    /// crosses from `from_lang` into `to_lang`. It RESOLVES when a sibling module
    /// of `to_lang` is present in the dispatched set (the target language was
    /// actually transpiled). Each resolved boundary becomes an [`FfiEntry`] with a
    /// deterministic `shim_id`; any boundary whose target language is absent fails
    /// reconciliation (the `manifest_completeness` invariant of
    /// `C-FFI-CPYTHON-EXT`). Same-language boundaries (`from == to`) are no-ops.
    ///
    /// PMAT-895: resolution is SYMBOL-LEVEL — a boundary resolves only when a
    /// module of `to_lang` actually DEFINES an item (function/const) named
    /// `symbol`. A target-language module that is present but does NOT export the
    /// symbol still fails reconciliation (the `manifest_completeness` invariant is
    /// about the symbol crossing the line, not just the language being present).
    pub fn reconcile(modules: &[Module]) -> Result<FfiManifest, FfiReconcileError> {
        let mut manifest = FfiManifest::new();
        let mut unresolved = Vec::new();
        for module in modules {
            for b in &module.ffi_boundaries {
                if b.from_lang == b.to_lang {
                    continue; // not a cross-language boundary
                }
                let lang_present = modules.iter().any(|m| m.source_lang == b.to_lang);
                let symbol_defined = modules.iter().any(|m| {
                    m.source_lang == b.to_lang
                        && m.items
                            .iter()
                            .any(|it| item_exported_name(it) == Some(b.symbol.as_str()))
                });
                if symbol_defined {
                    manifest.register(FfiEntry {
                        symbol: b.symbol.clone(),
                        from_lang: b.from_lang,
                        to_lang: b.to_lang,
                        source_signature: b.signature.clone(),
                        // First-cut shim signature: the real C/PyObject → Rust
                        // lowering is a later increment; record a stable placeholder
                        // that names the symbol so downstream emission can key on it.
                        rust_shim_signature: format!("fn {}(/* {} */)", b.symbol, b.signature),
                        shim_id: shim_id(b),
                    });
                } else {
                    // Distinguish "no target-language module at all" from "module
                    // present but symbol not exported" — both block the hybrid
                    // build, but the diagnostic should point at the right fix.
                    let reason = if lang_present {
                        format!("no {:?} module defines `{}`", b.to_lang, b.symbol)
                    } else {
                        format!("no {:?} module in the dispatched set", b.to_lang)
                    };
                    unresolved.push(format!(
                        "{:?}->{:?} `{}` ({}): {}",
                        b.from_lang, b.to_lang, b.symbol, b.signature, reason
                    ));
                }
            }
        }
        if unresolved.is_empty() {
            Ok(manifest)
        } else {
            Err(FfiReconcileError { unresolved })
        }
    }
}

/// PMAT-898 (Sprint-2 Tier 2): resolve each module's FFI-boundary `to_lang` from
/// the full dispatched module set. A single-file frontend can't see its siblings,
/// so `depyler-frontend::detect_ffi_boundaries` records a PROVISIONAL `to_lang`
/// (C, the canonical CPython-extension shape) for every relative import. With the
/// whole `Vec<Module>` in hand (the `xpile hybrid` CLI), rewrite each boundary's
/// `to_lang` to the `source_lang` of whichever sibling module actually DEFINES
/// the symbol. Consequences once `reconcile` runs:
///   - a relative import of a *Python* sibling becomes `Python → Python`, which
///     reconcile drops via its same-language skip (NOT a false FFI boundary);
///   - a relative import of a *C* sibling stays `Python → C` and resolves;
///   - a symbol no module defines is left as-is, so reconcile reports it
///     unresolved (the `manifest_completeness` invariant of C-FFI-CPYTHON-EXT).
///
/// Call this BEFORE [`FfiManifest::reconcile`].
pub fn resolve_boundary_to_langs(modules: &mut [Module]) {
    // symbol -> source_lang of the first module that exports it.
    let mut exporters: HashMap<String, SourceLang> = HashMap::new();
    for m in modules.iter() {
        for it in &m.items {
            if let Some(name) = item_exported_name(it) {
                exporters.entry(name.to_string()).or_insert(m.source_lang);
            }
        }
    }
    for m in modules.iter_mut() {
        for b in &mut m.ffi_boundaries {
            if let Some(&lang) = exporters.get(&b.symbol) {
                b.to_lang = lang;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use xpile_meta_hir::{Block, Expr, Function, Type};

    fn module(name: &str, lang: SourceLang, boundaries: Vec<FfiBoundary>) -> Module {
        Module {
            name: name.to_string(),
            source_lang: lang,
            items: Vec::new(),
            ffi_boundaries: boundaries,
        }
    }

    /// A module of `lang` that DEFINES (exports) `symbol` as a nullary function —
    /// the FFI export side that symbol-level reconciliation requires.
    fn module_defining(name: &str, lang: SourceLang, symbol: &str) -> Module {
        Module {
            name: name.to_string(),
            source_lang: lang,
            items: vec![Item::Function(Function {
                name: symbol.to_string(),
                params: Vec::new(),
                return_type: Type::I64,
                body: Block {
                    stmts: Vec::new(),
                    trailing_return: Expr::LitInt(0),
                },
            })],
            ffi_boundaries: Vec::new(),
        }
    }

    fn boundary(from: SourceLang, to: SourceLang, symbol: &str) -> FfiBoundary {
        FfiBoundary {
            from_lang: from,
            to_lang: to,
            symbol: symbol.to_string(),
            signature: format!("{symbol}(...)"),
        }
    }

    #[test]
    fn reconcile_pairs_boundary_when_target_defines_symbol() {
        let modules = vec![
            module(
                "foo",
                SourceLang::Python,
                vec![boundary(SourceLang::Python, SourceLang::C, "sum")],
            ),
            module_defining("_foo_core", SourceLang::C, "sum"),
        ];
        let manifest = FfiManifest::reconcile(&modules).expect("resolves");
        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(manifest.entries[0].symbol, "sum");
        assert_eq!(manifest.entries[0].from_lang, SourceLang::Python);
        assert_eq!(manifest.entries[0].to_lang, SourceLang::C);
        assert!(manifest.entries[0].shim_id.starts_with("shim_"));
    }

    #[test]
    fn reconcile_fails_when_target_lang_absent() {
        let modules = vec![module(
            "foo",
            SourceLang::Python,
            vec![boundary(SourceLang::Python, SourceLang::C, "sum")],
        )];
        let err = FfiManifest::reconcile(&modules).expect_err("unresolved");
        assert_eq!(err.unresolved.len(), 1);
        assert!(err.to_string().contains("sum"));
        assert!(err.to_string().contains("dispatched set"));
    }

    #[test]
    fn reconcile_fails_when_symbol_not_defined() {
        // The C module is PRESENT but does not export `sum` (it defines `other`)
        // — symbol-level reconciliation must still fail, with a symbol-specific
        // diagnostic distinct from the language-absent case.
        let modules = vec![
            module(
                "foo",
                SourceLang::Python,
                vec![boundary(SourceLang::Python, SourceLang::C, "sum")],
            ),
            module_defining("_foo_core", SourceLang::C, "other"),
        ];
        let err = FfiManifest::reconcile(&modules).expect_err("symbol not defined");
        assert_eq!(err.unresolved.len(), 1);
        assert!(err.to_string().contains("defines `sum`"));
    }

    #[test]
    fn reconcile_ignores_same_language_boundaries_and_empty() {
        let modules = vec![module(
            "foo",
            SourceLang::Python,
            vec![boundary(SourceLang::Python, SourceLang::Python, "local")],
        )];
        let manifest = FfiManifest::reconcile(&modules).expect("no cross-lang boundary");
        assert!(manifest.entries.is_empty());
        assert!(FfiManifest::reconcile(&[]).unwrap().entries.is_empty());
    }

    #[test]
    fn shim_id_is_deterministic() {
        let b = boundary(SourceLang::Python, SourceLang::C, "sum");
        assert_eq!(shim_id(&b), shim_id(&b));
    }

    #[test]
    fn resolve_to_python_sibling_drops_false_boundary() {
        // A relative import of a PYTHON sibling is mislabeled Python->C by the
        // single-file frontend; after resolution it becomes Python->Python, which
        // reconcile drops (not a real FFI boundary).
        let mut modules = vec![
            module(
                "app",
                SourceLang::Python,
                vec![boundary(SourceLang::Python, SourceLang::C, "helper")],
            ),
            module_defining("helpers", SourceLang::Python, "helper"),
        ];
        resolve_boundary_to_langs(&mut modules);
        assert_eq!(modules[0].ffi_boundaries[0].to_lang, SourceLang::Python);
        let manifest = FfiManifest::reconcile(&modules).expect("same-lang drops");
        assert!(manifest.entries.is_empty());
    }

    #[test]
    fn resolve_to_c_sibling_keeps_cross_language_boundary() {
        let mut modules = vec![
            module(
                "app",
                SourceLang::Python,
                vec![boundary(SourceLang::Python, SourceLang::C, "square_sum")],
            ),
            module_defining("_core", SourceLang::C, "square_sum"),
        ];
        resolve_boundary_to_langs(&mut modules);
        assert_eq!(modules[0].ffi_boundaries[0].to_lang, SourceLang::C);
        let manifest = FfiManifest::reconcile(&modules).expect("resolves");
        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(manifest.entries[0].to_lang, SourceLang::C);
    }

    #[test]
    fn resolve_leaves_undefined_symbol_unchanged() {
        // No module defines `missing` → to_lang stays as the provisional C, so
        // reconcile reports it unresolved.
        let mut modules = vec![
            module(
                "app",
                SourceLang::Python,
                vec![boundary(SourceLang::Python, SourceLang::C, "missing")],
            ),
            module_defining("_core", SourceLang::C, "present"),
        ];
        resolve_boundary_to_langs(&mut modules);
        assert_eq!(modules[0].ffi_boundaries[0].to_lang, SourceLang::C);
        assert!(FfiManifest::reconcile(&modules).is_err());
    }
}
