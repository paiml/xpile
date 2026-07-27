//! XPILE-WGSLI32-001 (PMAT-1400) — the WGSL/SPIR-V lanes accept EXACTLY the
//! i32 range, and the literal they emit DENOTES the value the user wrote.
//!
//! ## What was wrong
//!
//! The Python frontend does not fold unary minus: `-2147483648` reaches the
//! backend as `UnOp{Neg, LitInt(2147483648)}`. The WGSL lane emitted a
//! negated CONVERSION — `(-(i32(2147483648)))` — so the inner conversion was
//! out of range even though the value being denoted, `i32::MIN`, is perfectly
//! representable. naga rejected it and the whole emit refused at exit 1.
//!
//! Measured at `db3359d7` with a force-rebuilt binary: the lane ACCEPTED
//! `2147483647` (i32::MAX) and REFUSED `-2147483648` (i32::MIN) — asymmetric
//! at the two ends of the same range — and the refusal blamed
//!
//! ```text
//! the concrete type `i32` cannot represent the abstract value `2147483648`
//! ```
//!
//! naming a literal that appears NOWHERE in the user's source. The SPIR-V
//! lane inherited it verbatim, since it emits by compiling this module's own
//! WGSL lowering.
//!
//! This is the mirror of the class PMAT-1395 (WASM) and PMAT-1399 (Rust from
//! C) fixed: there the emit was too PERMISSIVE and shipped an out-of-range
//! literal; here it was too STRICT and refused an in-range one. Both are the
//! same root defect — a literal rendered without regard to the target width's
//! actual bounds.
//!
//! ## What is asserted, and why in this shape
//!
//! The load-bearing assertion is [`wgsl_accepts_exactly_the_i32_range`]: the
//! accepted set is pinned at BOTH ends and BOTH ends of the refusal, so the
//! fix cannot be "accept everything". `i32::MIN - 1` and `i32::MAX + 1` must
//! still refuse — an over-refusal repaired by widening what is accepted would
//! trade an exit-1 lie for a silent wrong answer, which is strictly worse and
//! is the standing lesson from PMAT-1395's falsification (B).
//!
//! [`wgsl_boundary_literal_denotes_its_own_value`] is the half a compile-only
//! gate cannot provide: it pins the emitted DECIMAL, so a future "fix" that
//! made the boundary compile by wrapping it to a different value reds here
//! even though naga would be perfectly happy.
//!
//! Probes are GENERATED across five syntactic positions rather than sampled
//! from one, because a position-specific literal path added later would
//! otherwise regress invisibly — the current emitter funnels all five through
//! a single site, and this test is what pins that.
//!
//! ANTI-VACUITY, learned the hard way while hunting this defect: `naga`
//! validates an EMPTY WGSL module clean. A probe harness that redirects
//! stdout to a file and validates the file will therefore report a REFUSED
//! program as passing, because the shell created the file before the emit
//! failed. That happened twice during this slice's investigation. Every
//! assertion below is guarded on SUBSTANCE — the emitted text must contain
//! the probe's own function — never on `is_ok()` alone.
//!
//! No toolchain is required (naga is a library), so there is NO skip path:
//! these tests always execute inside the REQUIRED `workspace-test` context.

use std::path::PathBuf;

use xpile_backend::{BackendConfig, Profile, Target};

/// The exclusive bounds of the WGSL lane's integer type. Written as
/// `i64` arithmetic on purpose: the meta-HIR literal is an `i64`, and the
/// question this file exists to settle is what happens when that `i64`
/// crosses the `i32` edge.
const I32_MIN: i64 = i32::MIN as i64;
const I32_MAX: i64 = i32::MAX as i64;

/// The five syntactic positions a literal can occupy in the WGSL subset.
/// `{LIT}` is substituted with the probe value.
///
/// These are deliberately not one shape repeated: the return position, an
/// initializer, a comparison operand, a binary operand and a loop bound
/// reach the emitter through different `Stmt`/`Expr` parents.
const POSITIONS: &[(&str, &str)] = &[
    (
        "return",
        "def probe(a: int, b: int) -> int:\n    return {LIT}\n",
    ),
    (
        "binop_operand",
        "def probe(a: int, b: int) -> int:\n    return a + {LIT}\n",
    ),
    (
        "local_init",
        "def probe(a: int, b: int) -> int:\n    x: int = {LIT}\n    return x\n",
    ),
    (
        "comparison",
        "def probe(a: int, b: int) -> int:\n    if a > {LIT}:\n        return 1\n    return 0\n",
    ),
    (
        "loop_bound",
        "def probe(a: int, b: int) -> int:\n    t: int = 0\n    while t < {LIT}:\n        t = t + 1\n    return t\n",
    ),
];

/// Lower `source` for `target` through the live session — the same
/// frontend+backend dispatch `xpile transpile` performs. `Err` carries the
/// refusal text so an assertion can name WHY, not just that it refused.
///
/// Mirrors the in-process sweep in `cli_spirv_input_fidelity_witness.rs`
/// (PMAT-1388): spawning the binary per probe would be ~50 process launches
/// for no extra fidelity, since both go through `default_session()`.
fn emit(source: &str, target: Target) -> Result<String, String> {
    let dir = std::env::temp_dir().join(format!(
        "xpile-wgsli32-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).expect("probe dir");
    let path: PathBuf = dir.join("probe.py");
    std::fs::write(&path, source).expect("write probe");

    let session = xpile_core::default_session();
    let frontend = session
        .frontends
        .iter()
        .find(|f| f.matches_path(&path))
        .ok_or_else(|| "no frontend matches .py".to_string())?;
    let module = frontend
        .parse_and_lower(&path, source)
        .map_err(|e| format!("frontend refused: {e}"))?;
    let backend = session
        .backends
        .iter()
        .find(|b| b.targets().contains(&target))
        .ok_or_else(|| format!("no backend for {target:?}"))?;
    let config = BackendConfig {
        emit_contracts: true,
        target,
        profile: Profile::RustOut,
        hardware: None,
    };
    backend
        .lower(&module, &config)
        .map(|a| a.primary)
        .map_err(|e| format!("backend refused: {e}"))
}

/// SUBSTANCE guard — see the anti-vacuity note in the module docs. An empty
/// string, or a header-only emit with no function body, must never count as
/// an acceptance.
fn is_substantive(wgsl: &str) -> bool {
    wgsl.contains("fn probe(") && wgsl.contains("return")
}

/// THE load-bearing assertion: the accepted set is exactly `[i32::MIN,
/// i32::MAX]`, pinned at both edges of acceptance AND both edges of refusal.
///
/// Before PMAT-1400 the `i32::MIN` rows refused in all five positions while
/// the `i32::MAX` rows emitted — 5 offenders out of 20 probes.
#[test]
fn wgsl_accepts_exactly_the_i32_range() {
    // (value, must_emit). The two `false` rows are what stops this test from
    // being satisfiable by an emitter that accepts every literal.
    let cases: &[(i64, bool)] = &[
        (I32_MIN, true),      // i32::MIN — the regression this slice fixed
        (I32_MAX, true),      // i32::MAX — always worked; the asymmetry's other end
        (0, true),            // control
        (-5, true),           // an ordinary negated literal, unchanged by the fold
        (I32_MIN - 1, false), // one past the bottom — MUST still refuse
        (I32_MAX + 1, false), // one past the top — MUST still refuse
    ];

    let mut accepted = 0usize;
    let mut refused = 0usize;
    let mut offenders: Vec<String> = Vec::new();

    for &(value, must_emit) in cases {
        for (pos_name, template) in POSITIONS {
            let source = template.replace("{LIT}", &value.to_string());
            match (emit(&source, Target::Wgsl), must_emit) {
                (Ok(wgsl), true) => {
                    if is_substantive(&wgsl) {
                        accepted += 1;
                    } else {
                        offenders.push(format!(
                            "{value} @ {pos_name}: emit succeeded but produced no \
                             function body (an empty module naga-validates clean — \
                             this is the vacuity trap, not an acceptance):\n{wgsl}"
                        ));
                    }
                }
                (Err(why), true) => offenders.push(format!(
                    "{value} @ {pos_name}: REFUSED a value inside the i32 range \
                     the lane maps `int` to — {why}"
                )),
                (Ok(wgsl), false) => offenders.push(format!(
                    "{value} @ {pos_name}: ACCEPTED a value OUTSIDE the i32 range. \
                     Repairing an over-refusal by widening what is accepted trades \
                     an exit-1 lie for a silent wrong answer (PMAT-1395):\n{wgsl}"
                )),
                (Err(_), false) => refused += 1,
            }
        }
    }

    eprintln!(
        "XPILE-WGSLI32-001: {accepted} accepted, {refused} refused across {} positions",
        POSITIONS.len()
    );

    // Offenders FIRST: it is the assertion that names WHICH value in WHICH
    // position went the wrong way. The counts below can only fail as a
    // consequence, so asserting them first would replace a diagnosis with
    // `left: 15, right: 20`.
    assert!(
        offenders.is_empty(),
        "PMAT-1400: the WGSL lane's accepted set does not match the i32 range \
         it claims to target ({} offender(s)):\n  {}",
        offenders.len(),
        offenders.join("\n  ")
    );
    // Vacuity floors on BOTH sides: an emitter that refused everything would
    // satisfy the refusal half trivially, and one that accepted everything
    // would satisfy the acceptance half trivially. These also catch a probe
    // that silently stopped being generated.
    assert_eq!(
        accepted,
        4 * POSITIONS.len(),
        "expected every in-range probe to emit in every position"
    );
    assert_eq!(
        refused,
        2 * POSITIONS.len(),
        "expected every out-of-range probe to refuse in every position"
    );
}

/// The half a compile-only gate cannot provide (PMAT-1395 falsification (B)):
/// a well-typed emit can still denote the WRONG value. Pin the decimal.
///
/// `i32(-2147483648)` is correct. `(-(i32(2147483648)))` does not compile,
/// and `i32(2147483648)` — the shape a careless "just drop the sign" repair
/// would produce — compiles nowhere but would be caught here regardless.
#[test]
fn wgsl_boundary_literal_denotes_its_own_value() {
    for (pos_name, template) in POSITIONS {
        let source = template.replace("{LIT}", &I32_MIN.to_string());
        let wgsl = emit(&source, Target::Wgsl)
            .unwrap_or_else(|e| panic!("i32::MIN @ {pos_name} must emit: {e}"));
        assert!(
            is_substantive(&wgsl),
            "i32::MIN @ {pos_name} produced no function body:\n{wgsl}"
        );
        assert!(
            wgsl.contains("i32(-2147483648)"),
            "i32::MIN @ {pos_name} must be emitted as the single conversion \
             `i32(-2147483648)`, denoting the value the user wrote. A negated \
             conversion `(-(i32(2147483648)))` puts an out-of-range magnitude \
             inside the conversion and is what this slice fixed:\n{wgsl}"
        );
        assert!(
            !wgsl.contains("i32(2147483648)"),
            "i32::MIN @ {pos_name} still emits the out-of-range magnitude \
             2147483648:\n{wgsl}"
        );
    }
}

/// The SPIR-V lane emits by compiling this module's own WGSL lowering, so the
/// boundary fix must reach it too — verified end-to-end (WGSL → naga → spv),
/// not assumed from the WGSL result.
///
/// Pre-fix this refused with `xpile-spirv-codegen emits by compiling this
/// module's own WGSL lowering … and that lowering refused it`.
#[test]
fn spirv_lane_inherits_the_boundary_fix() {
    for (pos_name, template) in POSITIONS {
        let source = template.replace("{LIT}", &I32_MIN.to_string());
        let spirv = emit(&source, Target::Spirv)
            .unwrap_or_else(|e| panic!("i32::MIN @ {pos_name} must reach SPIR-V: {e}"));
        // The CLI's SPIR-V artifact is a text summary carrying the compiled
        // word count and the source WGSL it was compiled FROM. Both are
        // checked: a non-zero word count proves naga's spv backend actually
        // ran, and the embedded literal proves it ran on THIS program.
        assert!(
            spirv.contains("; Magic:     0x07230203"),
            "expected a real SPIR-V header @ {pos_name}:\n{spirv}"
        );
        assert!(
            spirv.contains("i32(-2147483648)"),
            "the SPIR-V summary must embed the WGSL it compiled, carrying the \
             boundary literal @ {pos_name}:\n{spirv}"
        );
        let words: usize = spirv
            .lines()
            .find_map(|l| l.strip_prefix("; Words:")?.trim().parse().ok())
            .unwrap_or_else(|| {
                panic!("no word count in the SPIR-V summary @ {pos_name}:\n{spirv}")
            });
        assert!(
            words > 0,
            "SPIR-V word count is {words} @ {pos_name} — nothing was compiled"
        );
    }
}

/// Independent re-validation through the exported naga gate.
///
/// `emit_wgsl_module` calls `naga_validate_wgsl` internally (PMAT-1391 made
/// that unconditional), so this is redundant TODAY — deliberately. If that
/// internal call is ever removed or made conditional, `Ok ⟹ naga accepts`
/// stops holding by construction and this is the test that notices, rather
/// than the property silently becoming an unchecked comment.
#[test]
fn every_accepted_boundary_emit_independently_naga_validates() {
    let mut checked = 0usize;
    for value in [I32_MIN, I32_MAX, 0, -5] {
        for (pos_name, template) in POSITIONS {
            let source = template.replace("{LIT}", &value.to_string());
            let wgsl = emit(&source, Target::Wgsl)
                .unwrap_or_else(|e| panic!("{value} @ {pos_name} must emit: {e}"));
            assert!(
                is_substantive(&wgsl),
                "{value} @ {pos_name}: nothing to validate — an EMPTY module \
                 naga-validates clean, so validating it would pass vacuously:\n{wgsl}"
            );
            xpile_wgsl_codegen::naga_validate_wgsl(&wgsl).unwrap_or_else(|e| {
                panic!("{value} @ {pos_name}: emitted WGSL fails the repo's own exported naga gate: {e}\n{wgsl}")
            });
            checked += 1;
        }
    }
    assert_eq!(
        checked,
        4 * POSITIONS.len(),
        "vacuity guard: expected every in-range probe to be validated"
    );
}

/// Guards the anti-vacuity premise this file's assertions rest on, so the
/// claim in the module docs is MEASURED rather than asserted (PMAT-1396's
/// lesson: state the invariant and enforce it).
///
/// If a future naga bump started REJECTING an empty module, the substance
/// guards above would become unnecessary — and this test is where that gets
/// noticed, instead of the guards quietly turning into cargo cult.
#[test]
fn an_empty_wgsl_module_validates_clean_which_is_why_substance_is_guarded() {
    assert!(
        xpile_wgsl_codegen::naga_validate_wgsl("").is_ok(),
        "an empty WGSL module no longer validates — the substance guards in \
         this file were written because it DOES, and can be revisited"
    );
    assert!(
        xpile_wgsl_codegen::naga_validate_wgsl("// only a comment\n").is_ok(),
        "a comment-only WGSL module no longer validates — see above"
    );
    // And the guard actually discriminates the two.
    assert!(!is_substantive(""));
    assert!(!is_substantive("// only a comment\n"));
    assert!(is_substantive("fn probe(a: i32) -> i32 { return a; }"));
}
