//! XPILE-SPIRVFID-001 (PMAT-1388) — the SPIR-V lane compiles the CALLER'S program.
//!
//! ## What was wrong
//!
//! `SpirvSaxpyGeneralEmitter::try_emit` bound its `Module` argument to
//! `_module` and discarded it, always compiling the hardcoded
//! `spirv_saxpy_general` fixture (`2.0*x + 1.0`). So
//! `xpile transpile <ANY FILE> --target spirv` exited 0 and printed a SPIR-V
//! binary for a program the user never wrote. Measured at f74ebe61 on six
//! categorically different inputs — `add.py`, `fib.py`, `sign.py`, `cmp.py`,
//! a Python f64 function, and a bitwise C module: **all six produced
//! byte-identical output** (sha256 `421b318e…`), and two of them are inputs
//! the WGSL lowering this lane is *defined to reuse* REFUSES outright.
//!
//! That is the strongest form of the shape this window has been sweeping: not
//! "one construct is silently mistranslated" but "the entire artifact is
//! unrelated to the input, at exit 0". A user shipping that SPIR-V to a
//! Vulkan pipeline runs someone else's shader.
//!
//! ## What is asserted, and why in this shape
//!
//! The load-bearing assertion is [`spirv_never_accepts_what_wgsl_refuses`]:
//! the SPIR-V lane's accepted set must be a SUBSET of the WGSL lane's. That
//! is a relation between two live lanes rather than a hand-listed set of
//! refusals, so it cannot drift as either subset widens — and it is exactly
//! the invariant the defect violated (716 accepted vs WGSL's 39).
//!
//! The corpus sweep runs IN PROCESS through `xpile_core::default_session()` —
//! the same frontend+backend dispatch the CLI performs — because ~800
//! fixtures × 2 targets of process spawns is a minute of wall clock; the
//! exit-code and stderr assertions, which are about the CLI's own surface,
//! use the real binary. No toolchain is involved (naga is a library), so
//! there is NO skip path here: these tests always execute. Runtime is
//! reported below and was MEASURED, not assumed (PMAT-1383).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use xpile_backend::{BackendConfig, Profile, Target};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Every fixture the corpus sweep considers: top-level `.py` and `.c` files.
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

/// Lower `path` for `target` through the live session — the same dispatch
/// `xpile transpile` performs. `None` when the frontend or the backend
/// refuses (both are "this lane does not accept this program").
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

/// THE load-bearing invariant. The SPIR-V lane emits by compiling the WGSL
/// lane's lowering of the same module, so its accepted set can only be
/// NARROWER (naga may still reject valid-subset WGSL — recursion, say). A
/// program SPIR-V accepts and WGSL refuses is, by construction, a program
/// SPIR-V did not compile from the input.
///
/// Before PMAT-1388 this failed with 677 offenders: SPIR-V accepted 716 of
/// the 802 corpus fixtures (the rest fail at the FRONTEND, never reaching a
/// backend) while WGSL accepted 39.
#[test]
fn spirv_never_accepts_what_wgsl_refuses() {
    let t0 = Instant::now();
    let session = xpile_core::default_session();
    let mut wgsl_ok = 0usize;
    let mut spirv_ok = 0usize;
    let mut offenders: Vec<String> = Vec::new();
    for p in corpus() {
        let w = emit(&session, &p, Target::Wgsl).is_some();
        let s = emit(&session, &p, Target::Spirv).is_some();
        wgsl_ok += usize::from(w);
        spirv_ok += usize::from(s);
        if s && !w {
            offenders.push(p.file_name().unwrap().to_string_lossy().into_owned());
        }
    }
    eprintln!(
        "XPILE-SPIRVFID-001: wgsl accepts {wgsl_ok}, spirv accepts {spirv_ok} \
         (sweep {:.2}s)",
        t0.elapsed().as_secs_f64()
    );
    // Vacuity guard on BOTH sides: a lane that accepts nothing would satisfy
    // the subset relation trivially, and a lane that accepts nothing is the
    // failure mode an over-broad refusal would introduce.
    assert!(
        wgsl_ok >= 10,
        "WGSL lane accepts only {wgsl_ok} of the corpus — the subset assertion \
         below would be near-vacuous (live figure at PMAT-1388 was 39)"
    );
    assert!(
        spirv_ok >= 10,
        "SPIR-V lane accepts only {spirv_ok} of the corpus — this test cannot \
         distinguish a correct narrowing from a lane that refuses everything \
         (live figure at PMAT-1388 was 27)"
    );
    assert!(
        offenders.is_empty(),
        "PMAT-1388: the SPIR-V lane accepted {} program(s) its own WGSL lowering \
         REFUSES, so what it emitted cannot have been compiled from the input: \
         {offenders:?}",
        offenders.len()
    );
}

/// Distinct programs must produce distinct SPIR-V. The defect made this false
/// for the entire corpus; asserting it over a handful of unrelated fixtures is
/// enough to catch any recurrence of an input-independent emitter.
#[test]
fn distinct_programs_emit_distinct_spirv() {
    let session = xpile_core::default_session();
    let names = ["add.py", "sign.py", "cmp.py", "abs_val.py", "in_range.py"];
    let mut seen: Vec<(String, String)> = Vec::new();
    for n in names {
        let p = fixtures_dir().join(n);
        let out = emit(&session, &p, Target::Spirv)
            .unwrap_or_else(|| panic!("{n} is a WGSL-subset fixture and must emit SPIR-V"));
        if let Some((prev, _)) = seen.iter().find(|(_, o)| *o == out) {
            panic!(
                "PMAT-1388: `{n}` and `{prev}` are different programs but emitted \
                 IDENTICAL SPIR-V — the emitter is not reading its input"
            );
        }
        seen.push((n.to_string(), out));
    }
    assert_eq!(seen.len(), names.len());
}

/// The emitted artifact must carry the caller's own function — and must NOT
/// carry the hardcoded saxpy fixture. The negative half is the one that was
/// false: every emission inlined `fn saxpy(x: f32) -> f32`.
#[test]
fn emitted_spirv_carries_the_callers_own_function() {
    let session = xpile_core::default_session();
    let out = emit(&session, &fixtures_dir().join("add.py"), Target::Spirv)
        .expect("add.py must emit SPIR-V");
    assert!(
        out.contains("fn add(a: i32, b: i32) -> i32"),
        "SPIR-V summary must inline the WGSL lowering of THIS module, got:\n{out}"
    );
    assert!(
        !out.contains("saxpy"),
        "the hardcoded saxpy fixture leaked into an unrelated program's SPIR-V:\n{out}"
    );
}

/// CLI surface: a refused program must exit NON-ZERO and say why, naming the
/// WGSL lowering it delegates to. Before PMAT-1388 both of these exited 0.
#[test]
fn cli_refuses_out_of_subset_programs_with_a_reason() {
    let bin = env!("CARGO_BIN_EXE_xpile");
    let f64_src = std::env::temp_dir().join("xpile_spirvfid_f64.py");
    std::fs::write(
        &f64_src,
        "def widen(x: float) -> float:\n    return x * 2.0\n",
    )
    .unwrap();

    for (label, path) in [
        ("C bitwise (shifts are outside the WGSL subset)", {
            fixtures_dir().join("c_bitwise.c")
        }),
        (
            "Python f64 (WGSL core has no 64-bit float)",
            f64_src.clone(),
        ),
    ] {
        let out = Command::new(bin)
            .args(["transpile", path.to_str().unwrap(), "--target", "spirv"])
            .output()
            .expect("xpile binary runs");
        assert!(
            !out.status.success(),
            "{label}: --target spirv exited 0 for a program the WGSL lane refuses; \
             stdout was:\n{}",
            String::from_utf8_lossy(&out.stdout)
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("WGSL"),
            "{label}: the refusal must name the WGSL lowering it delegates to, got:\n{err}"
        );
        assert!(
            out.stdout.is_empty(),
            "{label}: a refusal must emit NO artifact, got {} bytes of stdout",
            out.stdout.len()
        );
    }
    let _ = std::fs::remove_file(&f64_src);
}

/// CLASS GATE. The defect was one emitter binding its module to `_module`,
/// and that shape is available to EVERY backend — nine of them hold a
/// `try_emit(_module, …)` today (the PTX/CUDA/WASM/WGSL specialist and
/// hand-emitted arms). Swept at PMAT-1388: only the SPIR-V lane's
/// CLI-reachable emitter was input-independent — WGSL and WASM both vary
/// with their input, and every PTX target refuses at the CLI — so the sweep
/// found exactly one member. This test keeps it at one: for every target
/// that emits at all, three unrelated programs must produce three different
/// artifacts.
#[test]
fn no_target_emits_the_same_artifact_for_different_programs() {
    let session = xpile_core::default_session();
    let names = ["add.py", "sign.py", "cmp.py"];
    let targets = [
        Target::Rust,
        Target::Ruchy,
        Target::Lean,
        Target::Wasm,
        Target::Wgsl,
        Target::Spirv,
        Target::Shell,
        Target::ForjarYaml,
    ];
    let mut emitting_targets = 0usize;
    for t in targets {
        let outs: Vec<(&str, String)> = names
            .iter()
            .filter_map(|n| emit(&session, &fixtures_dir().join(n), t).map(|o| (*n, o)))
            .collect();
        if outs.len() < 2 {
            continue; // this target refuses the sample — nothing to compare
        }
        emitting_targets += 1;
        for i in 0..outs.len() {
            for j in (i + 1)..outs.len() {
                assert_ne!(
                    outs[i].1, outs[j].1,
                    "PMAT-1388: target {t:?} emitted IDENTICAL artifacts for `{}` and \
                     `{}` — an emitter that does not vary with its input is not \
                     compiling it",
                    outs[i].0, outs[j].0
                );
            }
        }
    }
    assert!(
        emitting_targets >= 4,
        "only {emitting_targets} target(s) emitted for the sample — the pairwise \
         comparison above is near-vacuous (live figure at PMAT-1388 was 6)"
    );
}

/// CONTROL. The three tests above are all satisfied by a backend that refuses
/// everything, which would be a different lie. This one fails if the lane
/// stops emitting for the subset it genuinely supports.
#[test]
fn the_supported_subset_still_emits_and_is_a_real_spirv_module() {
    let bin = env!("CARGO_BIN_EXE_xpile");
    let out = Command::new(bin)
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
        "add.py is inside the WGSL subset and must still emit: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("; SPIR-V") && text.contains("0x07230203"),
        "emission must be a real SPIR-V module (magic word present), got:\n{text}"
    );
}
