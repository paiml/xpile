//! PMAT-952 (runtime-witness half) — the FIRST real *executed* WASM
//! DiffExec witness for the native-WASM §29 lane
//! (`C-COMPILE-RUST-TO-WASM`).
//!
//! Sibling of [`xpile_ptx_codegen::NvccCudaDiffExecEngine`] (PMAT-949, the
//! NVIDIA-only CUDA witness) and
//! [`xpile_wgsl_codegen::WgpuWgslDiffExecEngine`] (PMAT-950, the
//! cross-vendor wgpu/WGSL witness). Where those run on a GPU toolchain /
//! adapter, this one runs in a **wasm runtime** — so the same
//! `out[i] = 2*in[i] + 1` saxpy-like semantics are executed and diffed in
//! a real WebAssembly interpreter.
//!
//! Until this slice the §29 Multi-Emitter Oracle Quorum recorded
//! [`DiffExecResult::NotRun { reason: no-engine }`] for the WASM backend
//! under `QuorumPolicy::DiffExec`; PMAT-951 shipped the EMIT half (the
//! `WasmBackend` single-emitter citation) and confirmed the emitted WAT
//! *executes* in WABT, but no two-emitter DiffExec quorum ever *ran* two
//! categorically-independent WAT lowerings against each other. This module
//! ships [`WasmDiffExecEngine`], a real [`DiffExecEngine`] that:
//!
//!   1. takes the two emitters' WAT module sources (general + specialist),
//!      each exporting one zero-arg `f64`-returning function per
//!      [`FIXTURE_INPUT`] element (`e0`..`eN`),
//!   2. assembles each with **`wat2wasm`** (WABT) into a real `.wasm`
//!      module — the same assembler PMAT-951's executed witness used,
//!   3. **executes both** with **`wasm-interp --run-all-exports`** (WABT),
//!      which runs every exported function and prints its result,
//!   4. parses the printed `f64` vectors and compares them within the
//!      contract's tolerance, returning a real
//!      [`DiffExecResult::Match`] / [`DiffExecResult::Divergent`].
//!
//! ## Runtime choice — WABT subprocess, no new Rust crate
//!
//! The execution route is the PMAT-951-proven WABT subprocess path
//! (`wat2wasm` + `wasm-interp`), NOT an in-process interpreter crate. This
//! is the lowest-risk choice: it adds **zero** Cargo dependencies, so
//! `cargo deny check advisories` is unaffected (the key risk a `wasmi` /
//! `wasmtime` dep would introduce). It is also the exact toolchain the
//! PMAT-951 EMIT-half executed witness already validated, so the assemble
//! + execute path is known-good.
//!
//! ## The two categorically-independent lowerings
//!
//! Both sides compute `2*x + 1` for each fixture element, but via
//! genuinely different WAT instruction sequences:
//!
//!   - **general**: `(x * 2.0) + 1.0` — an explicit `f64.mul` then
//!     `f64.add`.
//!   - **specialist**: `(x + x) + 1.0` — reassociated doubling via two
//!     `f64.add`s, with **no `f64.mul` opcode at all**.
//!
//! These are categorically independent (different opcodes, different
//! algebraic association) yet must produce the same result; the DiffExec
//! quorum runs both in the wasm runtime and falsifies the contract if they
//! diverge. The fixture is kept bit-identical to the CUDA / WGSL witnesses
//! so all three lanes attest the same numeric truth on different stacks.
//!
//! ## PMAT-976 — witness integrity: the general side drives the REAL emitter
//!
//! Before PMAT-976 BOTH sides of the executed witness fed [`WasmDiffExecEngine`]
//! hand-written WAT (`saxpy_module` in `lib.rs`), so the witness only ever
//! exercised the WABT assemble+run path — it never drove xpile's REAL WAT
//! emitter. PMAT-976 rewires the **general** side: [`general_module`] builds
//! a structured meta-HIR [`Module`] (`FloatBinOp`/`LitFloat`) and
//! [`general_module_wat`] lowers it via the SAME [`crate::emit_module`] the
//! `WasmBackend::lower` single-emitter path and the PMAT-951/966/968 emit
//! tests use. The `executed_witness_real_emit_assemble_run_matches` test then
//! assembles + runs THAT real WAT and diffs its executed output against the
//! trusted CPython-equivalent reference vector AND the (still hand-written,
//! categorically-independent) specialist reference. The witness now proves
//! `meta-HIR → xpile WAT emit → assemble → run → correct`.
//!
//! ## Graceful skip
//!
//! [`wasm_runtime_available`] gates whether the engine is installed —
//! mirroring [`xpile_ptx_codegen::cuda_toolchain_available`] /
//! [`xpile_wgsl_codegen::wgpu_adapter_available`]. On free CI (no WABT
//! tools) `wat2wasm`/`wasm-interp` are absent, the engine is never
//! installed, and the backend records the benign `NotRun { no-engine }` —
//! so CI stays green. Locally (WABT installed) the engine runs and
//! produces the executed witness.
//!
//! Error posture mirrors the [`DiffExecEngine`] contract: a *runtime
//! present but broken* run (wat2wasm assembly error, interp launch error,
//! unparseable output) returns `Err(_)`, which the backend turns into a
//! hard `BackendError` — a broken run must NOT masquerade as "not run".

use std::path::PathBuf;
use std::process::Command;

use xpile_backend::{BackendConfig, DiffExecEngine, DiffExecResult};
use xpile_meta_hir::{Block, Expr, FloatOp, Function, Item, Module, SourceLang, Type};

use crate::emit_module;

/// The deterministic fixture input vector both WAT modules run over. Kept
/// **bit-identical** to [`xpile_ptx_codegen::FIXTURE_INPUT`] /
/// [`xpile_wgsl_codegen::FIXTURE_INPUT`] so the WASM, CUDA, and WGSL
/// executed witnesses attest the same values on different stacks;
/// exercises negatives, zero, a fraction, and a larger magnitude.
pub const FIXTURE_INPUT: &[f64] = &[0.0, 1.0, 2.0, -3.0, 4.5, 10.0, -0.5, 100.0];

/// PMAT-976 (witness-integrity rewire): build a REAL meta-HIR [`Module`]
/// for the "general" side of the executed witness — one zero-arg
/// `f64`-returning function `eN` per [`FIXTURE_INPUT`] element, each whose
/// body is `(x * 2.0) + 1.0` over the *constant-folded* fixture value `x`.
///
/// This is the load-bearing change: prior to PMAT-976 BOTH sides of the
/// witness fed [`WasmDiffExecEngine`] hand-written WAT, so the witness only
/// exercised the WABT assemble+run path — it never drove xpile's REAL WAT
/// emitter (`emit_module`, the PMAT-951/966/968 lowering). Here the general
/// side is constructed as structured meta-HIR (`FloatBinOp`/`LitFloat`) and
/// lowered to WAT by the SAME [`emit_module`] the `WasmBackend::lower`
/// single-emitter path uses. The witness therefore now proves
/// `meta-HIR → xpile WAT emit → assemble → run → correct`, not
/// `hardcoded WAT → run`.
///
/// Each function is named `eN` and the return-type is [`Type::F64`], so
/// [`emit_module`] emits `(export "eN" (func $eN))` exactly matching the
/// `e0`..`eN` export convention [`parse_interp_output`] /
/// `wasm-interp --run-all-exports` expect.
fn general_module() -> Module {
    let items = FIXTURE_INPUT
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            // (x * 2.0) + 1.0 — the "general" lowering: an explicit
            // FloatOp::Mul then FloatOp::Add, the meta-HIR analogue of the
            // old hardcoded `f64.mul` + `f64.add` general emitter. The
            // fixture value `x` is a `LitFloat` constant so the emitted
            // function is zero-arg (interp can run it with no host args).
            let body = Expr::FloatBinOp {
                op: FloatOp::Add,
                lhs: Box::new(Expr::FloatBinOp {
                    op: FloatOp::Mul,
                    lhs: Box::new(Expr::LitFloat(x)),
                    rhs: Box::new(Expr::LitFloat(2.0)),
                }),
                rhs: Box::new(Expr::LitFloat(1.0)),
            };
            Item::Function(Function {
                name: format!("e{i}"),
                params: Vec::new(),
                return_type: Type::F64,
                body: Block {
                    stmts: Vec::new(),
                    trailing_return: body,
                },
            })
        })
        .collect();
    Module {
        name: "wasm_diffexec_general".into(),
        source_lang: SourceLang::Rust,
        items,
        ffi_boundaries: Vec::new(),
    }
}

/// PMAT-976: emit REAL xpile WAT for the general side via [`emit_module`].
///
/// Drives xpile's actual meta-HIR → WAT lowering (the SAME emitter the
/// `WasmBackend` single-emitter `lower` path and the PMAT-951/966/968 emit
/// tests use) over [`general_module`]. The returned WAT is what the witness
/// assembles + runs for the general side — so a regression in the real
/// emitter now fails the executed witness, which the old hand-written WAT
/// could never catch.
pub fn general_module_wat() -> String {
    emit_module(&general_module()).expect("general_module is in the WASM scalar/control subset")
}

/// `true` when both `wat2wasm` and `wasm-interp` (WABT) are invocable —
/// the gate that decides whether [`WasmDiffExecEngine`] should be
/// installed. Mirrors [`xpile_ptx_codegen::cuda_toolchain_available`] /
/// [`xpile_wgsl_codegen::wgpu_adapter_available`]: absence is a clean skip
/// (free CI has no WABT), presence runs the witness (local box).
pub fn wasm_runtime_available() -> bool {
    let wat2wasm = Command::new("wat2wasm").arg("--version").output().is_ok();
    let interp = Command::new("wasm-interp")
        .arg("--version")
        .output()
        .is_ok();
    wat2wasm && interp
}

/// A real WASM `DiffExecEngine`: assembles each emitter's WAT module with
/// `wat2wasm`, executes both with `wasm-interp --run-all-exports`, and
/// numerically compares the outputs. This is the executed Run≥1 witness
/// for `C-COMPILE-RUST-TO-WASM`.
pub struct WasmDiffExecEngine {
    /// Working directory for emitted `.wat` sources + `.wasm` modules.
    /// Defaults to a unique subdir of the system temp dir.
    work_dir: PathBuf,
}

impl Default for WasmDiffExecEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmDiffExecEngine {
    pub fn new() -> Self {
        // Unique per-process dir so parallel test runs don't collide.
        let work_dir =
            std::env::temp_dir().join(format!("xpile-wasm-diffexec-{}", std::process::id()));
        Self { work_dir }
    }

    /// Assemble `wat_src` into `name.wasm`, execute every export with
    /// `wasm-interp --run-all-exports`, and parse the printed `f64`
    /// vector (one value per exported `e0`..`eN`, in export order).
    ///
    /// `pub(crate)` so an in-crate executed witness (e.g. the PMAT-978
    /// list-write witness in `tests`) can reuse the exact same WABT
    /// assemble+run+parse path the DiffExec engine uses — keeping the WABT
    /// invocation single-sourced.
    pub(crate) fn assemble_run_parse(&self, wat_src: &str, name: &str) -> Result<Vec<f64>, String> {
        std::fs::create_dir_all(&self.work_dir).map_err(|e| format!("create work dir: {e}"))?;
        let wat_path = self.work_dir.join(format!("{name}.wat"));
        let wasm_path = self.work_dir.join(format!("{name}.wasm"));
        std::fs::write(&wat_path, wat_src).map_err(|e| format!("write {name}.wat: {e}"))?;

        let assemble = Command::new("wat2wasm")
            .arg(&wat_path)
            .arg("-o")
            .arg(&wasm_path)
            .output()
            .map_err(|e| format!("spawn wat2wasm: {e}"))?;
        if !assemble.status.success() {
            return Err(format!(
                "wat2wasm failed for {name}:\n{}",
                String::from_utf8_lossy(&assemble.stderr)
            ));
        }

        let run = Command::new("wasm-interp")
            .arg("--run-all-exports")
            .arg(&wasm_path)
            .output()
            .map_err(|e| format!("spawn wasm-interp: {e}"))?;
        let stdout = String::from_utf8_lossy(&run.stdout);
        if !run.status.success() {
            return Err(format!(
                "wasm-interp run of {name} exited non-zero: stdout={stdout:?} stderr={:?}",
                String::from_utf8_lossy(&run.stderr)
            ));
        }
        Self::parse_interp_output(&stdout, name)
    }

    /// Parse `wasm-interp --run-all-exports` output. Each export prints a
    /// line `eN() => f64:<value>`; we take the `f64:` value from every
    /// such line, in order. Lines for non-`f64` exports are ignored (the
    /// witness modules export only the `f64` `eN` functions).
    fn parse_interp_output(stdout: &str, name: &str) -> Result<Vec<f64>, String> {
        let mut out = Vec::new();
        for line in stdout.lines() {
            // Format: `e0() => f64:1.000000`
            if let Some(idx) = line.find("=> f64:") {
                let tok = line[idx + "=> f64:".len()..].trim();
                let v = tok
                    .parse::<f64>()
                    .map_err(|e| format!("parse f64 `{tok}` from {name} interp output: {e}"))?;
                out.push(v);
            }
        }
        if out.is_empty() {
            return Err(format!(
                "no f64 exports in {name} wasm-interp output:\n{stdout}"
            ));
        }
        Ok(out)
    }
}

impl DiffExecEngine for WasmDiffExecEngine {
    fn execute_and_compare(
        &self,
        general_text: &str,
        specialist_text: &str,
        _module: &Module,
        config: &BackendConfig,
        tolerance: f64,
    ) -> Result<DiffExecResult, String> {
        // The native-WASM lane carries no HwProfile (`Target::Wasm` has
        // `hardware: None`); a hardware profile is a configuration fault,
        // not a skip.
        if let Some(hw) = &config.hardware {
            return Err(format!(
                "WASM DiffExec engine requires no HwProfile (Target::Wasm), got {hw:?}"
            ));
        }

        let general = self.assemble_run_parse(general_text, "general")?;
        let specialist = self.assemble_run_parse(specialist_text, "specialist")?;

        if general.len() != specialist.len() {
            return Ok(DiffExecResult::Divergent {
                max_abs_diff: f64::INFINITY,
                tolerance,
            });
        }
        let max_abs_diff = general
            .iter()
            .zip(specialist.iter())
            .map(|(g, s)| (g - s).abs())
            .fold(0.0_f64, f64::max);

        if max_abs_diff <= tolerance {
            Ok(DiffExecResult::Match { max_abs_diff })
        } else {
            Ok(DiffExecResult::Divergent {
                max_abs_diff,
                tolerance,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_matches_gpu_witness_fixtures() {
        // The WASM, WGSL, and CUDA executed witnesses must attest the SAME
        // values on different stacks. Kept bit-identical to the GPU lanes'
        // `FIXTURE_INPUT` (asserted by value so this crate needn't depend
        // on the GPU codegen crates).
        assert_eq!(
            FIXTURE_INPUT,
            &[0.0, 1.0, 2.0, -3.0, 4.5, 10.0, -0.5, 100.0]
        );
    }

    #[test]
    fn engine_constructs() {
        // Pure-CPU smoke: building the engine never touches a runtime.
        let _engine = WasmDiffExecEngine::new();
    }

    #[test]
    fn parse_interp_output_extracts_f64_vector() {
        let stdout = "e0() => f64:1.000000\ne1() => f64:3.000000\ne2() => f64:-5.000000\n";
        let v = WasmDiffExecEngine::parse_interp_output(stdout, "general").unwrap();
        assert_eq!(v, vec![1.0, 3.0, -5.0]);
    }

    #[test]
    fn parse_interp_output_empty_is_error() {
        let err =
            WasmDiffExecEngine::parse_interp_output("no exports here\n", "general").unwrap_err();
        assert!(err.contains("no f64 exports"), "got: {err}");
    }

    /// PMAT-976: the CPython-equivalent expected vector for the witness
    /// kernel `out = 2*x + 1` over [`FIXTURE_INPUT`] — the trusted reference
    /// the REAL emitted WAT's executed output must match. (CPython:
    /// `[2*x + 1.0 for x in FIXTURE_INPUT]`.)
    fn expected_vector() -> Vec<f64> {
        FIXTURE_INPUT.iter().map(|&x| 2.0 * x + 1.0).collect()
    }

    #[test]
    fn general_module_wat_is_real_emitter_output() {
        // PMAT-976 (pure-CPU half): `general_module_wat()` must be the REAL
        // `emit_module` output, NOT hand-written WAT. We assert the
        // fingerprints only `emit_module` produces: the emitter's module
        // banner comment, the per-function contract citation, and a zero-arg
        // `(export "eN" (func $eN))` for every fixture element. If a future
        // refactor swapped this back to hand-written WAT these would break.
        let wat = general_module_wat();
        assert!(
            wat.contains("xpile-wasm-codegen — native WAT (scalar/control subset)"),
            "must carry the real emitter's module banner: {wat}"
        );
        assert!(
            wat.contains(";; xpile-contract: C-COMPILE-RUST-TO-WASM"),
            "must carry the per-function contract citation the emitter writes: {wat}"
        );
        for i in 0..FIXTURE_INPUT.len() {
            assert!(
                wat.contains(&format!("(export \"e{i}\" (func $e{i}))")),
                "emit_module must export zero-arg e{i}: {wat}"
            );
        }
        // The general lowering is an explicit f64.mul + f64.add (the
        // meta-HIR `FloatOp::Mul` then `FloatOp::Add`), routed through the
        // real float-binop emitter.
        assert!(wat.contains("f64.mul"), "general side uses f64.mul: {wat}");
        assert!(wat.contains("f64.add"), "general side uses f64.add: {wat}");
    }

    #[test]
    fn executed_witness_real_emit_assemble_run_matches() {
        // PMAT-976 (executed half): the load-bearing witness. Drive xpile's
        // REAL WAT emitter (`emit_module` over a meta-HIR module), assemble
        // it with `wat2wasm`, execute it with `wasm-interp`, and diff the
        // executed output against the trusted CPython-equivalent reference
        // vector AND the (hand-written, categorically-independent) specialist
        // WAT. Proves `meta-HIR → xpile WAT emit → assemble → run → correct`.
        if !wasm_runtime_available() {
            // Graceful skip on free CI (no WABT) — mirrors the engine's
            // NotRun { no-engine } posture.
            eprintln!("SKIP executed_witness: WABT (wat2wasm/wasm-interp) not installed");
            return;
        }

        let engine = WasmDiffExecEngine::new();

        // REAL emitted WAT for the general side.
        let general_wat = general_module_wat();
        let general = engine
            .assemble_run_parse(&general_wat, "general")
            .expect("assemble+run REAL emitted general WAT");

        // Trusted reference 1: the CPython-equivalent expected vector.
        let expected = expected_vector();
        assert_eq!(
            general.len(),
            expected.len(),
            "REAL emitted WAT must export one e_i per fixture element"
        );
        for (i, (g, e)) in general.iter().zip(expected.iter()).enumerate() {
            assert!(
                (g - e).abs() <= 1.0e-9,
                "e{i}: REAL emitted WAT executed {g}, expected (CPython) {e}"
            );
        }

        // Trusted reference 2: the hand-written, categorically-independent
        // specialist WAT `(x + x) + 1.0` (no f64.mul). Diff the REAL emitted
        // general output against it via the engine's own comparison.
        let specialist_wat = specialist_reference_wat();
        let specialist = engine
            .assemble_run_parse(&specialist_wat, "specialist")
            .expect("assemble+run reference specialist WAT");
        let max_abs_diff = general
            .iter()
            .zip(specialist.iter())
            .map(|(g, s)| (g - s).abs())
            .fold(0.0_f64, f64::max);
        assert!(
            max_abs_diff <= 1.0e-9,
            "REAL emitted general WAT diverged from specialist reference: max_abs_diff={max_abs_diff}"
        );

        // Print the captured evidence so the run shows xpile's REAL emitted
        // WAT was assembled + executed and matched.
        eprintln!("=== PMAT-976 executed witness: REAL xpile WAT emit → run ===");
        eprintln!("--- REAL emitted general WAT (emit_module over meta-HIR) ---");
        eprintln!("{general_wat}");
        eprintln!("--- executed general output (wasm-interp) ---\n{general:?}");
        eprintln!("--- CPython-equivalent expected ---\n{expected:?}");
        eprintln!("--- specialist reference output ---\n{specialist:?}");
        eprintln!("max_abs_diff(general, specialist) = {max_abs_diff}");
    }

    /// The hand-written, categorically-independent specialist reference WAT:
    /// `(x + x) + 1.0` per fixture element (a reassociated doubling with NO
    /// `f64.mul`). Used ONLY as a trusted oracle to diff the REAL emitted
    /// general WAT against — it is deliberately NOT the emit-under-test.
    fn specialist_reference_wat() -> String {
        let mut out = String::from("(module\n");
        for (i, &x) in FIXTURE_INPUT.iter().enumerate() {
            out.push_str(&format!(
                "  (func (export \"e{i}\") (result f64)\n    \
                 f64.const {x:?}\n    f64.const {x:?}\n    f64.add\n    \
                 f64.const 1.0\n    f64.add)\n"
            ));
        }
        out.push_str(")\n");
        out
    }
}
