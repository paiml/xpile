//! XPILE-BOOKAPI-001 (PMAT-1439) — the book's Rust examples, COMPILED.
//!
//! Every ```rust fence in `book/src/` appears here verbatim, between markers,
//! inside a file cargo builds. That is the whole point: a fence in a Markdown
//! file is checked by nothing, and every one of the book's four API fences was
//! consequently fabricated end to end — seven identifiers that appear in ZERO
//! files under `crates/*/src`, plus a `::default()` on a type with no `Default`
//! impl and two trait signatures that do not match the trait.
//!
//! `book_rust_example_witness.rs` asserts fence-to-region byte identity in both
//! directions. This file is the half that makes the identity worth anything: if
//! the API moves, THIS file stops compiling, and a book example cannot be wrong
//! for longer than it takes `cargo test` to run.
//!
//! Text between a BEGIN and END marker is published. Setup a reader would
//! supply for themselves (a `Module` to hand the backend, a `Path`) goes
//! OUTSIDE the markers — the book's fences elide it too, and eliding it is not
//! the same as inventing it.

use std::path::Path;

use depyler_frontend::PythonFrontend;
use xpile_backend::Backend;
use xpile_frontend::Frontend;
use xpile_meta_hir::Module;

/// `book/src/reference/frontends.md` — "Calling a frontend as a library".
#[test]
fn frontends_md_calling_a_frontend_as_a_library() -> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new("factorial.py");
    let source =
        "def factorial(n: int) -> int:\n    return 1 if n <= 1 else n * factorial(n - 1)\n";

    // BOOK-EXAMPLE-BEGIN book/src/reference/frontends.md
    use depyler_frontend::PythonFrontend;
    use xpile_frontend::Frontend;

    let frontend = PythonFrontend;
    let module = frontend.parse_and_lower(path, source)?;
    // `module` is a `xpile_meta_hir::Module`
    // BOOK-EXAMPLE-END book/src/reference/frontends.md

    assert_eq!(
        module.items.len(),
        1,
        "the probe source declares one function"
    );
    Ok(())
}

/// `book/src/reference/backends.md` — "Calling a backend as a library".
#[test]
fn backends_md_calling_a_backend_as_a_library() -> Result<(), Box<dyn std::error::Error>> {
    let module = probe_module();

    // BOOK-EXAMPLE-BEGIN book/src/reference/backends.md
    use xpile_backend::{Backend, BackendConfig, Profile, Target};
    use xpile_rust_codegen::RustBackend;

    let config = BackendConfig {
        target: Target::Rust,
        profile: Profile::RustOut,
        hardware: None,
        emit_contracts: true,
    };
    let backend = RustBackend;
    let artifact = backend.lower(&module, &config)?;
    // `artifact` is a `xpile_backend::Artifact`; `artifact.primary` is the
    // emitted Rust source.
    // BOOK-EXAMPLE-END book/src/reference/backends.md

    assert!(
        !artifact.primary.is_empty(),
        "the Rust backend emitted an empty artifact for a one-function module"
    );
    Ok(())
}

/// `book/src/contributing/adding-a-frontend.md` §3 — "Implement the trait".
///
/// `refused_claims()` is in the published example because the trait declares it
/// REQUIRED, deliberately: its doc comment says a default of `&[]` "would let
/// the next frontend with a partial refusal inherit the exact silence this
/// method exists to break" (PMAT-1433). The pre-PMAT-1439 guide omitted it, so
/// a contributor following the book wrote an impl that does not compile — and
/// would, if it had, have reproduced exactly the silence PMAT-1433 removed.
mod adding_a_frontend {
    // BOOK-EXAMPLE-BEGIN book/src/contributing/adding-a-frontend.md
    use std::path::Path;

    use xpile_frontend::{Frontend, FrontendError};
    use xpile_meta_hir::Module;

    pub struct MyFrontend;

    impl Frontend for MyFrontend {
        fn name(&self) -> &'static str {
            "mylang"
        }

        fn extensions(&self) -> &[&'static str] {
            &["myl"]
        }

        /// Path spellings this frontend CLAIMS but refuses for every input.
        /// Required, with no default: a frontend that lowers only some of what
        /// it claims must be able to say so (PMAT-1433).
        fn refused_claims(&self) -> &[&'static str] {
            &[]
        }

        fn parse_and_lower(&self, path: &Path, source: &str) -> Result<Module, FrontendError> {
            let _ = (path, source);
            // parse source → lower to meta-HIR
            todo!()
        }
    }
    // BOOK-EXAMPLE-END book/src/contributing/adding-a-frontend.md
}

/// `book/src/contributing/adding-a-backend.md` §3 — "Implement the trait".
mod adding_a_backend {
    // BOOK-EXAMPLE-BEGIN book/src/contributing/adding-a-backend.md
    use xpile_backend::{Artifact, Backend, BackendConfig, BackendError, Target};
    use xpile_meta_hir::Module;

    pub struct MyLangBackend;

    impl Backend for MyLangBackend {
        fn name(&self) -> &'static str {
            "mylang"
        }

        fn targets(&self) -> &[Target] {
            &[]
        }

        fn lower(&self, module: &Module, config: &BackendConfig) -> Result<Artifact, BackendError> {
            let _ = (module, config);
            // 1. Begin with a provenance comment naming the backend +
            //    governing contract.
            // 2. Emit a `// xpile-contract: <ID>` citation per function.
            // 3. Lower meta-HIR statements.
            // 4. On unsupported constructs, return `BackendError::Lower`
            //    naming the construct. Naming the governing contract and
            //    a better `--target` too is house style (see §1), not a
            //    requirement — most shipped backends do neither.
            todo!()
        }
    }
    // BOOK-EXAMPLE-END book/src/contributing/adding-a-backend.md
}

/// A one-function module for the backend example to lower. Outside the
/// markers: the book's fence elides it, and a reader supplies their own.
fn probe_module() -> Module {
    let frontend = PythonFrontend;
    frontend
        .parse_and_lower(
            Path::new("probe.py"),
            "def add(a: int, b: int) -> int:\n    return a + b\n",
        )
        .expect("the Python frontend lowers a two-int add")
}

/// The trait impls above are never constructed — they exist to be TYPE-CHECKED,
/// which is the property the book's fences were missing. Naming them here keeps
/// `dead_code` quiet without `#[allow]`, so a genuinely unused item still warns.
#[test]
fn the_published_trait_impls_type_check_as_their_traits() {
    fn assert_frontend<F: Frontend>(_: &F) {}
    fn assert_backend<B: Backend>(_: &B) {}
    assert_frontend(&adding_a_frontend::MyFrontend);
    assert_backend(&adding_a_backend::MyLangBackend);
}
