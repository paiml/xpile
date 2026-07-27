//! XPILE-PTX-CAP-001 (PMAT-1413) — `--hardware ptx:<cap>` may not thread an
//! arbitrary string into the emitted `.target` directive.
//!
//! ## What was measured (force-rebuilt binary at 997abe19, ptxas 13.0.48)
//!
//! `emit_kernel` writes `compute_capability` VERBATIM into `.target`, and
//! nothing checked it. Every one of these exited **0**:
//!
//! | `--hardware`        | emitted            | ptxas 13.0                      |
//! |---------------------|--------------------|---------------------------------|
//! | `ptx:bogus`         | `.target bogus`    | `Target architecture not defined`|
//! | `ptx:not a cap!!`   | `.target not a cap!!` | same                         |
//! | `ptx:sm_80 ; rm`    | `.target sm_80 ; rm` | `Parsing error near ';'`      |
//! | `ptx:sm_1`          | `.target sm_1`     | `Unsupported .target 'sm_1'`    |
//!
//! ## The guard the code NAMED did not exist and could not have worked
//!
//! `emit.rs` documented the fallback for a non-`sm_<num>` capability as
//! "`validate_ptx` and the real `ptxas` are the downstream oracles either
//! way". Both halves of that sentence were false in the CLI:
//!
//! 1. `validate_ptx` has **zero** production call sites — every caller is a
//!    `#[cfg(test)]` fn or a `tests/` file. (The PMAT-1391 shape: the
//!    validator the doc says it runs, that only the tests call.)
//! 2. Even wired in, it could not catch this. It compares the emitted
//!    `.target` against the **requested** capability, so for `ptx:bogus`
//!    expected == found == `bogus` and it returns `Ok(())` by construction.
//!
//! So the fix is not "call the existing validator" — it is a new grammar
//! check at the emit choke point. This file gates BOTH halves.
//!
//! ## Why the grammar is measured rather than guessed
//!
//! A digits-only check (`sm_` + `[0-9]+`) is the obvious implementation and
//! it is WRONG: `sm_90a`, `sm_100a`, `sm_120a`, `sm_121a` and `compute_90`
//! are REAL targets that ptxas 13.0 assembles. Over-refusing them would trade
//! this defect for a worse one. The accepting half of this witness pins them.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_xpile"))
}

/// A minimal scalar element-wise kernel — squarely inside the PTX subset, so
/// any refusal below is attributable to the capability and nothing else.
fn kernel_src(dir: &std::path::Path) -> PathBuf {
    let p = dir.join("k.py");
    std::fs::write(&p, "def add(a: int, b: int) -> int:\n    return a + b\n")
        .expect("write kernel fixture");
    p
}

fn transpile(src: &std::path::Path, cap: &str) -> std::process::Output {
    Command::new(bin())
        .args([
            "transpile",
            src.to_str().unwrap(),
            "--target",
            "ptx",
            "--hardware",
            &format!("ptx:{cap}"),
            "--contracts",
            "off",
        ])
        .output()
        .expect("spawn xpile")
}

/// Unique per call — parallel test threads must never share a scratch dir.
fn scratch(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "xpile-ptxcap-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&d).expect("create scratch dir");
    d
}

/// THE RED HALF. Each of these exited 0 emitting unassemblable PTX before
/// PMAT-1413; each must now refuse.
#[test]
fn malformed_compute_capability_refuses_instead_of_emitting() {
    let dir = scratch("refuse");
    let src = kernel_src(&dir);

    // `""` is excluded deliberately: `--hardware ptx:` (empty) documents a
    // FALLBACK to the sm_80 floor, which is existing intended behaviour and
    // is pinned by `empty_capability_still_falls_back_to_the_floor` below.
    for cap in [
        "bogus",
        "not a cap!!",
        "sm_80 ; rm",
        "sm_",
        "sm_80a1",
        "SM_80",
        "sm_-80",
        "compute_",
        "sm_80.1",
        "arbitrary",
    ] {
        let out = transpile(&src, cap);
        assert!(
            !out.status.success(),
            "`--hardware ptx:{cap}` must REFUSE — it is threaded verbatim into \
             `.target`, so exiting 0 emits PTX no assembler accepts.\n\
             stdout:\n{}",
            String::from_utf8_lossy(&out.stdout)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("not a well-formed PTX `.target`"),
            "the refusal must NAME what is wrong (a bare `Err` would not \
             distinguish this from the kernel being unsupported).\n\
             cap=`{cap}` stderr:\n{stderr}"
        );
        // The refusal must not have printed a module on the way out.
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            !stdout.contains(".target"),
            "cap=`{cap}` refused but still emitted a `.target` directive:\n{stdout}"
        );
    }
}

/// THE ACCEPTING HALF — the over-refusal guard. A digits-only grammar would
/// red this test, which is the entire point of writing it: `sm_90a` and
/// friends are real architectures, verified assembling under ptxas 13.0.
#[test]
fn real_architecture_spellings_are_still_accepted() {
    let dir = scratch("accept");
    let src = kernel_src(&dir);

    for cap in [
        "sm_80",      // contract floor
        "sm_89",      // RTX 4090
        "sm_90",      // Hopper
        "sm_90a",     // Hopper, architecture-specific — ptxas 13.0 accepts
        "sm_100",     // Blackwell
        "sm_100a",    //
        "sm_120",     //
        "sm_120a",    //
        "sm_121",     // GB10
        "sm_121a",    //
        "compute_90", // virtual architecture
    ] {
        let out = transpile(&src, cap);
        assert!(
            out.status.success(),
            "`--hardware ptx:{cap}` is a REAL target and must still emit \
             (refusing it would trade PMAT-1413's defect for a worse one).\n\
             stderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains(&format!(".target {cap}")),
            "`.target` must carry the requested capability verbatim for \
             cap=`{cap}`:\n{stdout}"
        );
    }
}

/// PMAT-1413's SECOND defect, independent of the grammar: the ISA version is
/// DERIVED from the capability, and the derivation parsed with
/// `strip_prefix("sm_").parse::<u32>()` — which fails on the arch-variant
/// suffix. So `sm_120a` (Blackwell, needs ISA >= 8.8) silently fell back to
/// the 8.0 floor and emitted a module ptxas hard-rejects:
///
/// ```text
/// ptxas fatal: PTX .version 8.0 does not support .target sm_120a
/// ```
///
/// This is the same defect class PMAT-963 fixed for the NON-suffixed
/// spelling; the suffixed spelling was missed. `sm_90a` must stay on 8.0 —
/// Hopper assembles there — so this also pins the `>= 100` boundary.
#[test]
fn arch_variant_suffix_derives_the_right_isa_version() {
    let dir = scratch("isa");
    let src = kernel_src(&dir);

    for (cap, want_version) in [
        ("sm_89", "8.0"),
        ("sm_90", "8.0"),
        ("sm_90a", "8.0"),
        ("compute_90", "8.0"),
        ("sm_100", "8.8"),
        ("sm_100a", "8.8"),
        ("sm_120", "8.8"),
        ("sm_120a", "8.8"),
        ("sm_121", "8.8"),
        ("sm_121a", "8.8"),
    ] {
        let out = transpile(&src, cap);
        assert!(out.status.success(), "`ptx:{cap}` must emit");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains(&format!(".version {want_version}")),
            "`ptx:{cap}` must derive `.version {want_version}` — a Blackwell \
             `.target` under the 8.0 floor is rejected by ptxas. Emitted:\n{}",
            stdout.lines().take(8).collect::<Vec<_>>().join("\n")
        );
    }
}

/// `--hardware ptx` and `--hardware ptx:` keep their documented sm_80
/// fallback. Pinned so the new grammar check cannot silently swallow the
/// empty case — an empty capability is NOT threaded into `.target`, it is
/// replaced by the floor before validation.
#[test]
fn empty_capability_still_falls_back_to_the_floor() {
    let dir = scratch("floor");
    let src = kernel_src(&dir);

    for arg in ["ptx", "ptx:"] {
        let out = Command::new(bin())
            .args([
                "transpile",
                src.to_str().unwrap(),
                "--target",
                "ptx",
                "--hardware",
                arg,
                "--contracts",
                "off",
            ])
            .output()
            .expect("spawn xpile");
        assert!(
            out.status.success(),
            "`--hardware {arg}` must keep its sm_80 fallback; stderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains(".target sm_80"),
            "`--hardware {arg}` must emit the contract floor:\n{stdout}"
        );
    }
}

/// The library choke point is the AUTHORITATIVE one — a caller that never
/// touches `parse_hardware` must still be refused. If the guard were only in
/// the CLI, this test would red.
#[test]
fn library_emit_path_refuses_independently_of_the_cli() {
    use xpile_ptx_codegen::validate_compute_capability;

    assert!(validate_compute_capability("sm_89").is_ok());
    assert!(validate_compute_capability("sm_120a").is_ok());
    assert!(validate_compute_capability("compute_90").is_ok());

    let err = validate_compute_capability("bogus").expect_err("`bogus` must refuse");
    assert_eq!(err.got, "bogus", "the error must carry the rejected input");
    assert!(
        err.to_string().contains("not a well-formed PTX `.target`"),
        "got: {err}"
    );

    // And the refusal is reached through `emit_kernel` itself, not just the
    // free function — that is what makes it a choke point rather than a
    // helper nothing calls (the exact failure mode `validate_ptx` had).
    use xpile_meta_hir::{BinOp, Block, Expr, Function, Param, Type};
    let param = |name: &str| Param {
        name: name.into(),
        ty: Type::I64,
        mutable: false,
    };
    let f = Function {
        name: "add".into(),
        params: vec![param("a"), param("b")],
        return_type: Type::I64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(Expr::Ident("a".into())),
                rhs: Box::new(Expr::Ident("b".into())),
            },
        },
    };
    let e = xpile_ptx_codegen::emit_kernel(&f, "bogus")
        .expect_err("emit_kernel must refuse a malformed capability");
    assert!(
        e.to_string().contains("not a well-formed PTX `.target`"),
        "emit_kernel's refusal must name the capability problem, got: {e}"
    );
    // The same kernel at a real capability emits — so the refusal above is
    // attributable to the capability, not to the kernel being unsupported.
    assert!(
        xpile_ptx_codegen::emit_kernel(&f, "sm_89").is_ok(),
        "the SAME kernel must emit at a valid capability, or the refusal \
         above proves nothing about the capability check"
    );
}
