//! Shared Rust emission.
//!
//! Takes meta-HIR as input, emits idiomatic Rust. Language-neutral by
//! design — language-specific quirks (Python's int promotion, C's
//! pointer arithmetic, Ruchy's pipeline operator) are normalized in
//! each frontend before reaching codegen.

use xpile_meta_hir::Module;

#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    #[error("unsupported item: {0}")]
    Unsupported(String),
}

pub fn emit_module(module: &Module) -> Result<String, CodegenError> {
    Ok(format!(
        "// xpile-generated from {:?} module {} — TODO\n",
        module.source_lang, module.name
    ))
}
