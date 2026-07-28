//! XPILE-SPIRVBIN-001 (PMAT-1428) — the SPIR-V artifact CONTAINS the module
//! it describes.
//!
//! ## What was wrong
//!
//! `--target spirv` was the only target whose CLI artifact was not the thing
//! the target names. `emit_from_wgsl` compiled the module through naga,
//! checked the words with `validate_spirv`, and then **dropped them**,
//! returning a text summary whose header asserts
//!
//! ```text
//! ; Magic:     0x07230203
//! ; Version:   1.3
//! ; Words:     63
//! ```
//!
//! for a payload no caller could obtain. Two doc comments said otherwise —
//! `spirv_text_summary`'s "The raw binary words go in a sidecar" and
//! `emit_from_wgsl`'s "packaging the text summary + binary-word sidecar" —
//! but `EmittedText` has no sidecar field, so a `TargetEmitter` structurally
//! cannot return one and none was ever constructed. `Artifact` DOES carry a
//! `sidecars: Vec<(String, Vec<u8>)>`, which is what made the claim read as
//! satisfied; the CLI writes `artifact.primary` and nothing else
//! (`crates/xpile/src/main.rs`), so even a populated sidecar would never
//! have reached a user.
//!
//! Measured at ff4dd702 through the shipped CLI: `xpile transpile add.py
//! --target spirv` exited 0 with 13 lines, every one of them a `;` comment,
//! and `0` bytes of SPIR-V. Every other target hands back its named artifact
//! — `rust` compilable Rust, `wgsl` naga-valid WGSL, `ptx` real PTX
//! assembly, `wasm` WAT, `lean` Lean. This lane handed back a description.
//!
//! ## What is asserted, and why in this shape
//!
//! The load-bearing assertion is [`spirv_artifact_contains_the_module_it_describes`]:
//! for every fixture the lane accepts, the words recovered from the
//! artifact's `;b ` block must
//!
//! 1. be accepted by the crate's own `validate_spirv`, and
//! 2. have length exactly equal to the `; Words:` header, and
//! 3. equal `wgsl_to_spirv_words(extract_wgsl_from_summary(artifact))`.
//!
//! (3) is the piece that cannot drift: it ties the artifact's two blocks to
//! each other, so recompiling the WGSL the artifact says it compiled must
//! reproduce the binary the artifact carries. A header the emitter writes
//! about itself proves nothing; this is a relation between two independently
//! recorded halves of the same file.
//!
//! The recovery is checked to FAIL on the pre-fix artifact shape
//! ([`recovery_refuses_an_artifact_that_only_describes_a_module`]) so a green
//! run cannot mean "the extractor accepts anything".
//!
//! The corpus sweep runs IN PROCESS through `xpile_core::default_session()`
//! — the same dispatch the CLI performs — with the CLI's own surface checked
//! separately through the real binary. naga is a library, so there is NO
//! skip path here: these tests always execute. Runtime is reported at the
//! bottom and was MEASURED, not assumed (PMAT-1383).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use xpile_backend::{BackendConfig, Profile, Target};
use xpile_spirv_codegen::{
    extract_spirv_words_from_summary, extract_wgsl_from_summary, validate_spirv,
    wgsl_to_spirv_words, SPIRV_MAGIC, SUMMARY_BINARY_LINE_PREFIX,
};

/// Lower bound on lane-accepted fixtures, so the corpus assertions cannot
/// pass by sweeping an empty set (PMAT-1396: a negative over an enumeration
/// passes for free on an EMPTY enumeration).
const MIN_ACCEPTED: usize = 10;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn corpus() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(fixtures_dir())
        .expect("fixtures dir readable")
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|s| s.to_str()),
                Some("py") | Some("c")
            )
        })
        .collect();
    v.sort();
    v
}

/// Lower `path` for `target` through the live session. `None` when the
/// frontend or the backend refuses.
fn emit(session: &xpile_core::TranspileSession, path: &Path, target: Target) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    let frontend = session.frontends.iter().find(|f| f.matches_path(path))?;
    let module = frontend.parse_and_lower(path, &contents).ok()?;
    let backend = session
        .backends
        .iter()
        .find(|b| b.targets().contains(&target))?;
    let config = BackendConfig {
        emit_contracts: true,
        target,
        profile: Profile::RustOut,
        hardware: None,
    };
    backend.lower(&module, &config).ok().map(|a| a.primary)
}

/// The `; Words:` count the artifact claims for itself.
fn declared_word_count(artifact: &str) -> Option<usize> {
    artifact
        .lines()
        .find_map(|l| l.strip_prefix("; Words:")?.trim().parse().ok())
}

fn xpile_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_xpile"))
}

/// LOAD-BEARING. Every artifact the lane emits must carry the module it
/// describes, and that module must agree with both the header count and the
/// WGSL the artifact records as its own source.
#[test]
fn spirv_artifact_contains_the_module_it_describes() {
    let t0 = Instant::now();
    let session = xpile_core::default_session();
    let mut accepted = 0usize;
    let mut offenders: Vec<String> = Vec::new();

    for path in corpus() {
        let Some(artifact) = emit(&session, &path, Target::Spirv) else {
            continue;
        };
        accepted += 1;
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        let words = match extract_spirv_words_from_summary(&artifact) {
            Ok(w) => w,
            Err(e) => {
                offenders.push(format!("{name}: no recoverable binary ({e})"));
                continue;
            }
        };

        // (1) the recovered stream is a real, structurally valid module.
        if words.first().copied() != Some(SPIRV_MAGIC) {
            offenders.push(format!("{name}: recovered stream lacks the SPIR-V magic"));
            continue;
        }
        if let Err(e) = validate_spirv(&words) {
            offenders.push(format!("{name}: recovered words fail validate_spirv: {e}"));
            continue;
        }

        // (2) the header's self-reported count matches the payload present.
        match declared_word_count(&artifact) {
            Some(n) if n == words.len() => {}
            Some(n) => offenders.push(format!(
                "{name}: header claims {n} words, artifact carries {}",
                words.len()
            )),
            None => offenders.push(format!("{name}: artifact has no `; Words:` header")),
        }

        // (3) the two halves of the artifact agree: recompiling the WGSL the
        // artifact says it compiled reproduces the binary it carries.
        match extract_wgsl_from_summary(&artifact) {
            Ok(wgsl) => match wgsl_to_spirv_words(&wgsl) {
                Ok(recompiled) if recompiled == words => {}
                Ok(recompiled) => offenders.push(format!(
                    "{name}: carried binary ({} words) != recompiling the embedded WGSL ({} words)",
                    words.len(),
                    recompiled.len()
                )),
                Err(e) => offenders.push(format!("{name}: embedded WGSL no longer compiles: {e}")),
            },
            Err(e) => offenders.push(format!("{name}: embedded WGSL unrecoverable: {e}")),
        }
    }

    assert!(
        accepted >= MIN_ACCEPTED,
        "SPIR-V lane accepted only {accepted} corpus fixtures — below the \
         vacuity floor of {MIN_ACCEPTED}, so this sweep proves nothing"
    );
    assert!(
        offenders.is_empty(),
        "{} of {accepted} SPIR-V artifacts do not contain the module they \
         describe:\n  {}",
        offenders.len(),
        offenders.join("\n  ")
    );
    eprintln!(
        "XPILE-SPIRVBIN-001 corpus: {accepted} artifacts verified in {:?}",
        t0.elapsed()
    );
}

/// RED HALF. The extractor must reject the pre-fix artifact shape — a
/// summary that describes a module without containing it. Without this, a
/// green corpus sweep could mean "the extractor accepts anything".
#[test]
fn recovery_refuses_an_artifact_that_only_describes_a_module() {
    // Byte-for-byte the shape `--target spirv` emitted at ff4dd702.
    let pre_fix = "; SPIR-V\n\
                   ; Magic:     0x07230203\n\
                   ; Version:   1.3\n\
                   ; Words:     63\n\
                   ; Emitter:   xpile-spirv-codegen (WGSL -> naga -> spv)\n\
                   ; Source WGSL (reused from xpile-wgsl-codegen):\n\
                   ;   fn add(a: i32, b: i32) -> i32 {\n\
                   ;     return (a + b);\n\
                   ;   }\n";
    let err = extract_spirv_words_from_summary(pre_fix)
        .expect_err("an artifact with no `;b ` block must not yield words");
    assert!(
        err.contains(SUMMARY_BINARY_LINE_PREFIX),
        "the refusal must name the missing block, got: {err}"
    );

    // A malformed row is a refusal too, not a silently truncated module.
    let bad_row = format!("; Words:     1\n{SUMMARY_BINARY_LINE_PREFIX}0723020\n");
    assert!(
        extract_spirv_words_from_summary(&bad_row).is_err(),
        "a 7-digit word must be refused, not zero-extended"
    );

    // And the pre-fix shape is genuinely the one the fix changed: today's
    // emitter produces a `;b ` block for the same program.
    let session = xpile_core::default_session();
    let live = emit(&session, &fixtures_dir().join("add.py"), Target::Spirv)
        .expect("add.py is inside the WGSL subset and must emit");
    assert!(
        live.contains(SUMMARY_BINARY_LINE_PREFIX),
        "the live emitter must carry a binary block:\n{live}"
    );
}

/// The CLI's own surface — the artifact a user actually receives on stdout
/// must be the one the in-process sweep verified.
#[test]
fn cli_spirv_stdout_carries_a_recoverable_module() {
    let out = Command::new(xpile_bin())
        .args([
            "transpile",
            fixtures_dir().join("add.py").to_str().unwrap(),
            "--target",
            "spirv",
        ])
        .output()
        .expect("xpile binary runs");
    assert!(
        out.status.success(),
        "add.py must emit: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let words = extract_spirv_words_from_summary(&text).unwrap_or_else(|e| {
        panic!("the CLI's SPIR-V artifact must contain its module: {e}");
    });
    validate_spirv(&words).expect("the CLI-delivered words are a valid SPIR-V module");
    assert_eq!(
        declared_word_count(&text),
        Some(words.len()),
        "the CLI artifact's `; Words:` header must match its payload"
    );
    // Every line stays comment-shaped, so the artifact is still safe to hand
    // to a `;`-comment-aware reader.
    for line in text.lines() {
        assert!(
            line.starts_with(';'),
            "the artifact must remain wholly comment-shaped, got: {line}"
        );
    }
}

/// PMAT-1388's class gate, extended to the payload: two categorically
/// different programs must carry DIFFERENT binaries. The original defect
/// (one emitter discarding its `Module`) would now also be visible here.
#[test]
fn different_programs_carry_different_binaries() {
    let session = xpile_core::default_session();
    let mut seen: Vec<(String, Vec<u32>)> = Vec::new();
    for name in ["add.py", "sign.py", "cmp.py"] {
        let p = fixtures_dir().join(name);
        if !p.exists() {
            continue;
        }
        if let Some(artifact) = emit(&session, &p, Target::Spirv) {
            let words = extract_spirv_words_from_summary(&artifact)
                .unwrap_or_else(|e| panic!("{name}: artifact must contain its module: {e}"));
            seen.push((name.to_string(), words));
        }
    }
    assert!(
        seen.len() >= 2,
        "need at least 2 emitting fixtures to compare, got {}",
        seen.len()
    );
    for i in 0..seen.len() {
        for j in (i + 1)..seen.len() {
            assert_ne!(
                seen[i].1, seen[j].1,
                "{} and {} carry byte-identical SPIR-V — the artifact is not \
                 derived from the input",
                seen[i].0, seen[j].0
            );
        }
    }
}
