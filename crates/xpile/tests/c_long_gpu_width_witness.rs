//! XPILE-CLONG64-001 (PMAT-1404) — a C `long` reaching the WGSL/SPIR-V lanes
//! is REFUSED, not silently halved to `i32`.
//!
//! ## What was wrong
//!
//! `decy-frontend` introduced `Type::CLong` for exactly one reason: to keep
//! the 64-bit C widths (`long`, `long long`, `int64_t`) apart from
//! `Type::I64`, which is where the 32-bit C `int` and the width-less Python
//! `int` both land. Its own module docs say so — "to the distinct 64-bit
//! `Type::CLong` width, kept apart from the …".
//!
//! `wgsl_emit.rs` then folded them back together:
//!
//! ```text
//! Type::I64 | Type::CLong => Ok(WgslTy::I32),
//! ```
//!
//! collapsing the one distinction the type exists to carry. Measured at
//! `b750f0a6` with a force-rebuilt binary, `long f(long a) { return a + 1; }`
//! emitted, at exit 0:
//!
//! ```text
//! fn f(a: i32) -> i32 {
//!   return (a + i32(1));
//! }
//! ```
//!
//! Half the declared domain of `f` cannot be passed to that function at all,
//! and no diagnostic said so. SPIR-V inherited it verbatim, since it emits by
//! compiling this same WGSL lowering.
//!
//! ## Why this is a defect and not a documented posture
//!
//! The lane already REFUSES the other three 64-bit C types, and the `f64`
//! refusal states the doctrine in as many words — *"substituting f32 would
//! change numeric results, so the WGSL subset refuses f64 rather than narrow
//! it silently"*. `unsigned long` (`CULong`) refuses too. Signed `long` was
//! the one 64-bit type that took the silent narrowing, so the lane was not
//! applying a posture, it had a hole.
//!
//! Sharper still, and the receipt that needs no GPU: the SAME lane refused to
//! WRITE DOWN a value it silently ACCEPTED. `long f(long a) { return
//! 3000000000; }` refused with
//!
//! ```text
//! the concrete type `i32` cannot represent the abstract value `3000000000`
//! ```
//!
//! while `long f(long a) { return a + 1; }` — whose parameter carries exactly
//! that value at runtime — emitted clean. The literal was caught incidentally,
//! by naga; the parameter was not caught at all, because after `map_type` ran
//! there was nothing left to catch: the type had already got smaller. See
//! [`the_lane_refused_to_write_down_what_it_silently_accepted`].
//!
//! ## Why `I64` is deliberately NOT refused with it
//!
//! Refusing `Type::I64` would delete the WGSL lane — every Python function
//! lowers through it. And the cases genuinely differ: a Python `int` (and a C
//! `int`) declares no width in the SOURCE, so picking the GPU-native 32-bit
//! integer contradicts nothing the user wrote, and the out-of-range LITERAL
//! half is already pinned by PMAT-1401 (`wgsl_int_boundary_witness.rs`). A C
//! `long` declares 64 bits explicitly. That asymmetry is the whole basis of
//! this gate, so [`c_int_and_python_int_still_reach_the_gpu_lanes`] pins the
//! accept side: a "fix" that refused all integers would red there.
//!
//! ## Anti-vacuity
//!
//! Carried from PMAT-1401, which hit this twice while hunting: naga validates
//! an EMPTY WGSL module clean, so `is_ok()` is not evidence of an emit. Every
//! acceptance below is guarded on SUBSTANCE — the emitted text must contain
//! the probe's own function — and [`an_empty_emit_would_not_count_as_an_acceptance`]
//! pins that the guard is load-bearing rather than cargo cult.
//!
//! No toolchain is required for the xpile half (naga is a library), so there
//! is NO skip path on any assertion about xpile's own behaviour: these run
//! inside the REQUIRED `workspace-test` context. The one optional half is the
//! `cc` ground-truth measurement in
//! [`the_lane_refused_to_write_down_what_it_silently_accepted`], and it is
//! structured so that its absence drops ONLY the C-execution reading — every
//! assertion about xpile in that test runs unconditionally.

use std::path::PathBuf;
use std::process::Command;

use xpile_backend::{BackendConfig, HwProfile, Profile, Target};

/// The two GPU lanes under test. SPIR-V is not a separate emitter — it
/// compiles this crate's own WGSL — so it is listed to pin that the
/// inheritance is real rather than assumed.
const GPU_TARGETS: &[(&str, Target)] = &[("wgsl", Target::Wgsl), ("spirv", Target::Spirv)];

/// The three spellings `decy-frontend` folds into the single `Type::CLong`
/// width (`crates/decy-frontend/src/lib.rs`: `"long" | "int64_t" |
/// "int_least64_t" | "int_fast64_t" => Tok::Long`, plus `long long`).
///
/// All three are listed because a fix keyed on the SPELLING rather than on
/// the meta-HIR type would pass with only one of them handled.
const CLONG_SPELLINGS: &[&str] = &["long", "long long", "int64_t"];

/// The syntactic positions a `long` can occupy in the WGSL subset. `{T}` is
/// substituted with a spelling from [`CLONG_SPELLINGS`].
///
/// All three reach `map_type`, through three different parents — a signature
/// return, a signature param, and a `let` binding — because the emitter
/// consults the type map separately for each and a partial fix would leave one
/// narrowing.
///
/// The `list[…]` element site (`map_list_elem_type`) carried its own copy of
/// the same collapsed match arm and is fixed too, but it is deliberately NOT
/// probed here: `decy-frontend` cannot parse a subscript (`xs[0]` fails with
/// "unexpected character `[` in C source") and the Python frontend never
/// produces `Type::CLong`, so no CLI path reaches it. Adding a probe that
/// "passes" by hitting a PARSE error would be a gate certifying the wrong
/// refusal. That site is covered by a unit test over hand-built meta-HIR in
/// `xpile-wgsl-codegen` (`list_of_clong_is_refused_at_the_element_site`),
/// which says the same thing about its own reach.
const POSITIONS: &[(&str, &str)] = &[
    ("return_type", "{T} probe({T} a) { return a; }"),
    ("param_only", "int probe({T} a) { return 1; }"),
    (
        "local_decl",
        "int probe(int a) { {T} t = 1; if (t > 0) { return a; } return 0; }",
    ),
];

/// A C source declaring a 64-bit return and parameter, used for the
/// cross-backend comparison. Kept as one canonical program so the backends
/// are compared on the SAME input rather than on four hand-written variants.
const CANONICAL_LONG_C: &str = "long f(long a) { return a + 1; }\n";

/// Lower `source` (whose extension picks the frontend) for `target` through
/// the live session — the same dispatch `xpile transpile` performs. `Err`
/// carries the refusal text so an assertion can name WHY.
fn emit(source: &str, ext: &str, target: Target) -> Result<String, String> {
    let dir = std::env::temp_dir().join(format!(
        "xpile-clong64-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).expect("probe dir");
    let path: PathBuf = dir.join(format!("probe.{ext}"));
    std::fs::write(&path, source).expect("write probe");

    let session = xpile_core::default_session();
    let frontend = session
        .frontends
        .iter()
        .find(|f| f.matches_path(&path))
        .ok_or_else(|| format!("no frontend matches .{ext}"))?;
    let module = frontend
        .parse_and_lower(&path, source)
        .map_err(|e| format!("frontend refused: {e}"))?;
    let backend = session
        .backends
        .iter()
        .find(|b| b.targets().contains(&target))
        .ok_or_else(|| format!("no backend for {target:?}"))?;
    let hardware = match target {
        // The PTX backend refuses without a compute capability; sm_80 is the
        // contract floor the CLI's bare `--hardware ptx` also selects.
        Target::Ptx => Some(HwProfile::Ptx {
            compute_capability: "sm_80".to_string(),
        }),
        _ => None,
    };
    let config = BackendConfig {
        emit_contracts: true,
        target,
        profile: Profile::RustOut,
        hardware,
    };
    backend
        .lower(&module, &config)
        .map(|a| a.primary)
        .map_err(|e| format!("backend refused: {e}"))
}

/// SUBSTANCE guard — see the anti-vacuity note in the module docs. A header-
/// only emit with no function body must never count as an acceptance.
///
/// Checked against the emitted-identifier form for each lane: WGSL prints
/// `fn probe(`, PTX renames into a kernel entry, and WAT prints `$probe`.
fn is_substantive(emitted: &str, target: Target) -> bool {
    match target {
        Target::Wgsl => emitted.contains("fn probe(") || emitted.contains("fn f("),
        // SPIR-V's textual form embeds the WGSL it was compiled from and then
        // the disassembly; either channel carrying the function is substance.
        Target::Spirv => emitted.contains("fn probe(") || emitted.contains("fn f("),
        Target::Wasm => emitted.contains("$probe") || emitted.contains("$f"),
        Target::Ptx => emitted.contains(".visible .entry"),
        _ => emitted.contains("probe") || emitted.contains("fn f"),
    }
}

/// THE load-bearing assertion: no spelling of a 64-bit C integer, in any
/// position, silently reaches a 32-bit GPU emit.
///
/// Before PMAT-1404 every one of these 24 probes (3 spellings × 4 positions ×
/// 2 lanes) emitted at exit 0 with the width halved.
#[test]
fn c_long_never_narrows_silently_on_the_gpu_lanes() {
    let mut refused = 0usize;
    let mut offenders: Vec<String> = Vec::new();

    for spelling in CLONG_SPELLINGS {
        for (pos, template) in POSITIONS {
            let source = format!("{}\n", template.replace("{T}", spelling));

            // FRONTEND-ACCEPTS PRECONDITION. A probe that fails to PARSE also
            // "refuses", and a sweep that counted it would certify the wrong
            // refusal — this exact trap fired while writing this file, on a
            // probe using a cast the C frontend does not support. Rust is the
            // lane that accepts the full declared width, so a successful emit
            // there proves the source is in the frontend's subset and the GPU
            // refusal below is about the WIDTH.
            let rust = emit(&source, "c", Target::Rust).unwrap_or_else(|why| {
                panic!(
                    "`{spelling}` @ {pos}: the PROBE does not parse, so any GPU \
                     refusal it produces says nothing about widths. Fix the \
                     probe, not the emitter. Source:\n  {source}{why}"
                )
            });
            assert!(
                rust.contains("i64"),
                "`{spelling}` @ {pos}: the probe parsed but carries no 64-bit \
                 width even on the Rust lane, so it is not exercising CLong at \
                 all:\n{rust}"
            );

            for (lane, target) in GPU_TARGETS {
                match emit(&source, "c", *target) {
                    Ok(emitted) => offenders.push(format!(
                        "`{spelling}` @ {pos} → {lane}: ACCEPTED a DECLARED 64-bit \
                         width into a 32-bit lane. Source:\n  {source}Emitted:\n{emitted}"
                    )),
                    Err(why) => {
                        // The refusal must name the width, not just fail. A
                        // generic "unsupported type" would leave a user unable
                        // to tell this from an unimplemented construct.
                        assert!(
                            why.contains("64-bit"),
                            "`{spelling}` @ {pos} → {lane}: refused, but the message \
                             does not name the WIDTH as the reason, so a user cannot \
                             tell a deliberate width refusal from an unimplemented \
                             construct: {why}"
                        );
                        refused += 1;
                    }
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "{} of {} probes silently narrowed a declared 64-bit C integer:\n\n{}",
        offenders.len(),
        CLONG_SPELLINGS.len() * POSITIONS.len() * GPU_TARGETS.len(),
        offenders.join("\n\n")
    );
    // Pin the count so a future refactor that stops GENERATING probes (an
    // empty loop trivially satisfies the assertion above) reds here.
    assert_eq!(
        refused,
        CLONG_SPELLINGS.len() * POSITIONS.len() * GPU_TARGETS.len(),
        "probe generation produced fewer refusals than probes — the loop above \
         is not covering what this test claims it covers"
    );
}

/// The OVER-REFUSAL receipt. Refusing `Type::CLong` must not have refused
/// integers generally: the C `int` and the Python `int` both lower to
/// `Type::I64`, share the same `map_type`, and must still emit.
///
/// Without this, "refuse `CLong`" and "refuse every integer" are
/// indistinguishable, and the second one deletes the lane.
#[test]
fn c_int_and_python_int_still_reach_the_gpu_lanes() {
    let cases: &[(&str, &str, &str)] = &[
        ("c_int", "c", "int probe(int a) { return a + 1; }\n"),
        (
            "python_int",
            "py",
            "def probe(a: int) -> int:\n    return a + 1\n",
        ),
        // `list[int]` — the `map_list_elem_type` site's accept side. Its
        // refuse side is not CLI-reachable (see POSITIONS) and is covered by a
        // unit test in `xpile-wgsl-codegen`; this half IS reachable, and pins
        // that fixing that site did not break lists generally.
        (
            "python_list_int",
            "py",
            "def probe(xs: list[int]) -> int:\n    return xs[0]\n",
        ),
    ];

    for (name, ext, source) in cases {
        for (lane, target) in GPU_TARGETS {
            let emitted = emit(source, ext, *target).unwrap_or_else(|why| {
                panic!(
                    "{name} → {lane}: REFUSED an UNDECLARED-width integer. \
                     PMAT-1404 refuses the DECLARED 64-bit C widths only; \
                     refusing `I64` too would delete the WGSL lane: {why}"
                )
            });
            assert!(
                is_substantive(&emitted, *target),
                "{name} → {lane}: emit succeeded but produced no function body. \
                 naga validates an EMPTY module clean, so this is the vacuity \
                 trap, not an acceptance:\n{emitted}"
            );
        }
    }
}

/// The receipt that 64 bits is the CORRECT disposition and the GPU refusal is
/// a lane limit rather than xpile giving up on the source.
///
/// The same `long f(long a)` reaches five backends. Four honour the declared
/// width; only the two GPU lanes cannot, because WGSL core has no 64-bit
/// integer. If a future change "fixed" the refusal by narrowing everywhere,
/// this test reds — which is the point.
#[test]
fn every_non_gpu_backend_preserves_the_declared_64_bit_width() {
    // (lane, target, the token that proves 64 bits survived)
    let cases: &[(&str, Target, &str)] = &[
        ("rust", Target::Rust, "i64"),
        ("wasm", Target::Wasm, "i64"),
        ("ptx", Target::Ptx, ".s64"),
    ];

    for (lane, target, width_token) in cases {
        let emitted = emit(CANONICAL_LONG_C, "c", *target).unwrap_or_else(|why| {
            panic!(
                "{lane}: REFUSED `{}` — this backend is expected to honour the \
                 declared 64-bit width, and PMAT-1404's WGSL refusal is only \
                 defensible because these lanes do: {why}",
                CANONICAL_LONG_C.trim()
            )
        });
        assert!(
            is_substantive(&emitted, *target),
            "{lane}: emit succeeded but produced no function body:\n{emitted}"
        );
        assert!(
            emitted.contains(width_token),
            "{lane}: emitted a function for a `long` source with no `{width_token}` \
             anywhere — the declared 64-bit width did not survive lowering:\n{emitted}"
        );
    }

    // And the GPU lanes, on that exact same source, refuse. Asserted here and
    // not only in the sweep above so the CONTRAST is pinned in one place: the
    // difference is the lane, not the input.
    for (lane, target) in GPU_TARGETS {
        assert!(
            emit(CANONICAL_LONG_C, "c", *target).is_err(),
            "{lane}: accepted the same `long` source that rust/wasm/ptx emit at \
             64 bits — the GPU lanes have no 64-bit integer to accept it INTO"
        );
    }
}

/// The internal inconsistency this slice was found by, pinned so it cannot
/// return in either direction.
///
/// Through v0.1.617 the lane REFUSED `return 3000000000;` inside a `long`
/// function (naga: "the concrete type `i32` cannot represent the abstract
/// value `3000000000` accurately") while ACCEPTING the enclosing `long`
/// parameter that carries exactly that value at runtime. It declined to write
/// down what it was silently accepting.
///
/// `cc` supplies the ground truth for what the DECLARED type means — the C
/// program really does return 2147483648 — and its absence drops only that
/// reading. The two xpile assertions run unconditionally.
#[test]
fn the_lane_refused_to_write_down_what_it_silently_accepted() {
    // A value inside `long` and outside `i32`.
    const BEYOND_I32: i64 = 3_000_000_000;
    assert!(
        BEYOND_I32 > i64::from(i32::MAX),
        "premise: the probe value must be outside i32"
    );

    // (1) The literal form. It refused BEFORE this slice, incidentally, via
    // naga; it must still refuse — now for the stated reason.
    let literal_src = format!("long f(long a) {{ return {BEYOND_I32}; }}\n");
    let literal = emit(&literal_src, "c", Target::Wgsl);
    assert!(
        literal.is_err(),
        "the lane emitted a literal outside i32 into an i32 lane — repairing a \
         refusal by widening what is accepted trades an exit-1 lie for a silent \
         wrong answer (PMAT-1395):\n{literal:?}"
    );

    // (2) The parameter form — the half that used to emit at exit 0. Same
    // declared type, same magnitude, and BEFORE this slice a different answer.
    let param = emit(CANONICAL_LONG_C, "c", Target::Wgsl);
    assert!(
        param.is_err(),
        "the lane REFUSES to write the literal {BEYOND_I32} into a `long` function \
         but ACCEPTS a `long` parameter that carries it at runtime — refusing to \
         write down what it silently accepts. Emitted:\n{param:?}"
    );

    // (3) Ground truth for what `long` means, measured rather than asserted.
    // Optional: its absence drops this reading only.
    match run_c_ground_truth() {
        Some(observed) => assert_eq!(
            observed,
            i64::from(i32::MAX) + 1,
            "the C program's own execution disagrees with the premise that `long` \
             holds values past i32 — this test's whole argument rests on it"
        ),
        None => eprintln!(
            "NOTE: `cc` unavailable — the C-execution ground truth for `long` was \
             not measured this run. Assertions (1) and (2), which are the ones \
             about xpile's behaviour, ran regardless."
        ),
    }
}

/// Compile and run `long f(long a) { return a + 1; }` at `a = i32::MAX`,
/// returning what the DECLARED type actually holds. `None` if `cc` is absent.
fn run_c_ground_truth() -> Option<i64> {
    let dir = std::env::temp_dir().join(format!("xpile-clong64-cc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let src = dir.join("gt.c");
    let bin = dir.join("gt");
    std::fs::write(
        &src,
        format!(
            "#include <stdio.h>\n{}\
             int main(void) {{ printf(\"%ld\\n\", f({})); return 0; }}\n",
            CANONICAL_LONG_C,
            i32::MAX
        ),
    )
    .ok()?;

    let compiled = Command::new("cc")
        .arg(&src)
        .arg("-o")
        .arg(&bin)
        .output()
        .ok()?;
    if !compiled.status.success() {
        return None;
    }
    let run = Command::new(&bin).output().ok()?;
    if !run.status.success() {
        return None;
    }
    String::from_utf8(run.stdout)
        .ok()?
        .trim()
        .parse::<i64>()
        .ok()
}

/// The vacuity guard is load-bearing, not decoration.
///
/// PMAT-1401 recorded that naga validates an EMPTY WGSL module clean, and that
/// a harness redirecting stdout to a file will "validate" the empty file a
/// REFUSED run left behind. [`is_substantive`] is what stops that from reading
/// as an acceptance here, so this pins that it actually rejects the empty case
/// — otherwise a future edit could weaken it to `|_| true` with nothing red.
#[test]
fn an_empty_emit_would_not_count_as_an_acceptance() {
    for (lane, target) in GPU_TARGETS {
        assert!(
            !is_substantive("", *target),
            "{lane}: the substance guard accepts the EMPTY string, so every \
             acceptance assertion in this file is vacuous"
        );
        assert!(
            !is_substantive(
                "// xpile-wgsl-codegen — meta-HIR → WGSL\n// source module: probe\n",
                *target
            ),
            "{lane}: the substance guard accepts a header-only emit with no \
             function body"
        );
    }
    // …and it does accept a real one, so it is not simply always-false.
    assert!(
        is_substantive("fn probe(a: i32) -> i32 {\n  return a;\n}\n", Target::Wgsl),
        "the substance guard rejects a genuine emit — it would make every \
         acceptance assertion in this file unsatisfiable"
    );
}
