//! PMAT-932 (hybrid Python→Shell producer): the first time the `emit_shell_shim`
//! arm runs LIVE through `xpile hybrid` end-to-end.
//!
//! `emit_shell_shim` (the `std::process::Command` subprocess wrapper citing
//! `C-FFI-SHELL-SUBPROCESS`) has existed since PMAT-907, but nothing PRODUCED a
//! `Python → Shell` `FfiBoundary` for it to consume — the arm was reachable only
//! by hand-constructing a manifest. The producer is the reconciliation seam: a
//! Shell module is invoked by its PROGRAM name (the file stem = `Module::name`,
//! the thing the subprocess shim spawns), not by a meta-HIR item, so
//! `resolve_boundary_to_langs` + `reconcile` now resolve a Python relative import
//! of a sibling shell script (`from ._tool import _tool`) to that Shell module.
//!
//! Two levels of coverage:
//!  1. crate-level: `FfiManifest::reconcile` pairs the Python boundary with the
//!     bashrs-lowered shell module (in-process, no toolchain needed).
//!  2. CLI-level: `xpile hybrid <dir> --emit-shims` emits the real
//!     `Command::new("_tool")` wrapper citing `C-FFI-SHELL-SUBPROCESS`.

use bashrs_frontend::BashrsFrontend;
use depyler_frontend::PythonFrontend;
use std::path::{Path, PathBuf};
use std::process::Command;
use xpile_ffi_manifest::{resolve_boundary_to_langs, FfiManifest};
use xpile_frontend::Frontend;
use xpile_meta_hir::SourceLang;

/// `app.py` relative-imports a sibling SHELL tool by its program name `_tool`.
const PY_IMPORTS_SHELL_TOOL: &str = "from ._tool import _tool\ndef main() -> None:\n    pass\n";

/// Crate-level: the Python→Shell boundary reconciles against the bashrs-lowered
/// shell module by the script's PROGRAM name — the producer the shell shim lacked.
#[test]
fn python_shell_boundary_reconciles_by_program_name() {
    let py = PythonFrontend
        .parse_and_lower(Path::new("app.py"), PY_IMPORTS_SHELL_TOOL)
        .expect("python parses");
    assert_eq!(
        py.ffi_boundaries.len(),
        1,
        "the relative import is one boundary"
    );
    // The single-file frontend provisionally types it Python→C (it can't see the
    // sibling); the boundary symbol is the imported name.
    assert_eq!(py.ffi_boundaries[0].symbol, "_tool");

    // The sibling shell tool — its `Module::name` is the file stem `_tool`, the
    // program a subprocess shim would spawn. (Its only item is the synthetic
    // `main`, which is deliberately NOT the FFI-callable surface for shell.)
    let sh = BashrsFrontend
        .parse_and_lower(Path::new("_tool.sh"), "echo \"running tool\"\n")
        .expect("shell parses");
    assert_eq!(sh.source_lang, SourceLang::Shell);
    assert_eq!(sh.name, "_tool");

    // Resolve the provisional `to_lang` against the real sibling set: `_tool`
    // now resolves to the Shell module (by program name), not the C default.
    let mut modules = vec![py.clone(), sh.clone()];
    resolve_boundary_to_langs(&mut modules);
    assert_eq!(
        modules[0].ffi_boundaries[0].to_lang,
        SourceLang::Shell,
        "the boundary resolves to the sibling shell tool"
    );

    let manifest =
        FfiManifest::reconcile(&modules).expect("boundary resolves to the shell program");
    assert_eq!(manifest.entries.len(), 1);
    assert_eq!(manifest.entries[0].symbol, "_tool");
    assert_eq!(manifest.entries[0].from_lang, SourceLang::Python);
    assert_eq!(manifest.entries[0].to_lang, SourceLang::Shell);

    // And the manifest lowers to a real subprocess shim (no `unsafe`, no
    // `extern "C"`) — the `emit_shell_shim` arm is now reachable from a producer.
    let shims = manifest
        .emit_rust_shims(&modules)
        .expect("shell boundary shims cleanly");
    assert!(
        shims.contains("Command::new(\"_tool\")"),
        "the shell shim spawns the program by name; got:\n{shims}"
    );
    assert!(
        shims.contains("xpile-contract: C-FFI-SHELL-SUBPROCESS"),
        "the shell shim cites its governing contract; got:\n{shims}"
    );
    // No `unsafe` block and no `extern "C"` declaration — a shell boundary is
    // argv + exit codes, never a C ABI. (The doc-comment mentions the word
    // "unsafe" in prose, so match the actual keyword forms.)
    assert!(
        !shims.contains("unsafe {") && !shims.contains("unsafe extern"),
        "a shell boundary needs no `unsafe`; got:\n{shims}"
    );
    assert!(
        !shims.contains("extern \"C\""),
        "a shell boundary is not a C-ABI callee; got:\n{shims}"
    );
}

/// CLI-level: `xpile hybrid <dir> --emit-shims <file>` on the committed
/// `hybrid_shell` fixture drives the WHOLE producer path and writes the real
/// `Command`-based shim. This is the live end-to-end `emit_shell_shim`.
#[test]
fn xpile_hybrid_emits_live_shell_shim() {
    let bin = env!("CARGO_BIN_EXE_xpile");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hybrid_shell");
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hybrid_shell_shims.rs");

    let status = Command::new(bin)
        .arg("hybrid")
        .arg(&fixture)
        .arg("--emit-shims")
        .arg(&out)
        .status()
        .expect("spawn xpile");
    assert!(
        status.success(),
        "xpile hybrid --emit-shims must reconcile the Python→Shell boundary"
    );

    let emitted = std::fs::read_to_string(&out).expect("shim file written");
    assert!(
        emitted.contains("pub fn _tool_shim"),
        "the live shell shim is emitted; got:\n{emitted}"
    );
    assert!(
        emitted.contains("Command::new(\"_tool\")"),
        "the shim spawns the shell program by name; got:\n{emitted}"
    );
    assert!(
        emitted.contains("xpile-contract: C-FFI-SHELL-SUBPROCESS"),
        "the shim cites its governing contract; got:\n{emitted}"
    );
}
