//! PMAT-963 — the CROSS-HARDWARE §29 PTX anti-correlation vote.
//!
//! PMAT-961/962 ran the anti-correlation PTX witness on ONE architecture (the
//! local RTX 4090, sm_89 / Ada): xpile's OWN hand-emitted PTX (via the CUDA
//! Driver API) vs the nvcc-compiled CUDA-C `xpile_kernel`, two categorically-
//! independent codegen toolchains that must agree. This slice strengthens the
//! quorum to a CROSS-ARCHITECTURE vote by adding a SECOND architecture — the
//! gx10 fleet host (NVIDIA GB10, compute_cap 12.1 = `sm_121` / Blackwell, the
//! user-provided GPU fleet) — and asserting:
//!
//!   (a) ON gx10 sm_121, xpile-PTX vs nvcc-CUDA-C agree (anti-correlation holds
//!       on the NEW architecture, not just sm_89);
//!   (b) the SAME xpile emitter's executed results AGREE ACROSS sm_89 (local)
//!       and sm_121 (gx10) — the cross-architecture independence vote, which
//!       catches arch-specific emit bugs (e.g. an ISA-version or `.target`
//!       derivation that's wrong for Blackwell) that a single-arch witness
//!       cannot see.
//!
//! ## Mechanism — ssh/scp, zero new Cargo deps
//!
//! The gx10 arm is orchestrated by shelling `ssh`/`scp` (the same no-new-
//! dependency posture as the local WABT / nvcc / ptxas witnesses — `cargo deny
//! check advisories` is unaffected):
//!
//!   1. emit xpile's PTX for `sm_121` (the `.target` AND the `.version` are
//!      DERIVED from the compute capability — Blackwell needs ISA ≥ 8.8, which
//!      `ptx_version_for` supplies; ptxas 13.0 hard-rejects the 8.0 floor for
//!      sm_121) and wrap it in the SAME Driver-API harness the local engine
//!      uses ([`PtxDiffExecEngine::driver_harness`]);
//!   2. emit the matching nvcc CUDA-C `xpile_kernel` ([`SAXPY_CUDA_C_KERNEL`])
//!      in the SAME fixture harness ([`NvccCudaDiffExecEngine::harness`]);
//!   3. `scp` both `.cu` files into a `mktemp -d` working dir ON gx10, compile
//!      (`nvcc -arch=sm_121`, the Driver-API one linking `-lcuda`) and EXECUTE
//!      both there, parse the printed float vectors, and diff them;
//!   4. **clean up the gx10 temp dir afterward** — no persistent state, no
//!      destructive ops on the remote host.
//!
//! The local sm_89 arm reuses the production [`PtxDiffExecEngine`] unchanged.
//!
//! ## Graceful-skip (mirrors cc/python3/nvcc/WABT/ptxas)
//!
//! [`gx10_available`] gates the whole witness on a 10s-timeout `ssh -o
//! BatchMode=yes gx10 true` succeeding AND remote `nvcc` + `ptxas` present.
//! CI (no gx10 in its `~/.ssh/config`, no key) records a clean skip and stays
//! GREEN; locally — where gx10 IS reachable — the real cross-arch witness runs
//! and the captured sm_89 + sm_121 outputs are printed.

use std::process::Command;

use xpile_backend::{BackendConfig, DiffExecEngine, DiffExecResult, HwProfile, Profile, Target};
use xpile_meta_hir::{Module, SourceLang};
use xpile_ptx_codegen::{
    cuda_toolchain_available, emit_kernel, saxpy_kernel_fn, NvccCudaDiffExecEngine,
    PtxDiffExecEngine, FIXTURE_INPUT, SAXPY_CUDA_C_KERNEL,
};

/// The fleet host alias (resolved via the box's `~/.ssh/config`). The gx10 =
/// NVIDIA GB10 (Grace-Blackwell, compute_cap 12.1 = sm_121).
const GX10_HOST: &str = "gx10";

/// The remote architecture: GB10 reports compute_cap 12.1 → `sm_121`.
const GX10_SM: &str = "sm_121";

/// `true` when the gx10 fleet host is reachable AND carries the CUDA toolchain
/// — the gate that decides whether the remote cross-hardware arm runs. A clean
/// skip on CI (no gx10 reachable / no key) keeps the gate green; a real local
/// box (gx10 in `~/.ssh/config`) runs the cross-arch witness. Mirrors the
/// `cuda_toolchain_available` / cc-availability graceful-skip pattern, extended
/// across the network boundary.
fn gx10_available() -> bool {
    // 1) connectivity — non-interactive, fail fast (no key / unreachable → skip).
    let reachable = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            GX10_HOST,
            "true",
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !reachable {
        return false;
    }
    // 2) remote toolchain — nvcc AND ptxas must both be invocable on gx10.
    Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            GX10_HOST,
            "nvcc --version >/dev/null 2>&1 && ptxas --version >/dev/null 2>&1",
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// One executed half of the remote arm: `scp` `src` into the gx10 working dir
/// `remote_dir` as `name.cu`, `nvcc -arch=sm_121`-compile it (with `extra`
/// flags, e.g. `-lcuda`) ON gx10, run the binary ON gx10, and parse the printed
/// float vector. A non-zero compile/run, or a device `ERR …` line, is a hard
/// `Err` — a broken remote run must never masquerade as agreement.
fn gx10_compile_run_parse(
    remote_dir: &str,
    src: &str,
    name: &str,
    extra: &str,
) -> Result<Vec<f64>, String> {
    // Stage the source locally, scp it into the remote working dir.
    let local = std::env::temp_dir().join(format!("xpile-gx10-{}-{}.cu", std::process::id(), name));
    std::fs::write(&local, src).map_err(|e| format!("write {name}.cu: {e}"))?;
    let remote_cu = format!("{remote_dir}/{name}.cu");
    let scp = Command::new("scp")
        .args(["-q", "-o", "BatchMode=yes"])
        .arg(&local)
        .arg(format!("{GX10_HOST}:{remote_cu}"))
        .output()
        .map_err(|e| format!("spawn scp {name}: {e}"))?;
    let _ = std::fs::remove_file(&local);
    if !scp.status.success() {
        return Err(format!(
            "scp {name}.cu to gx10 failed: {}",
            String::from_utf8_lossy(&scp.stderr)
        ));
    }

    // Compile + run on gx10 in one ssh hop. `set -e` so a compile failure
    // aborts before the run; the binary prints the space-separated vector.
    let remote_bin = format!("{remote_dir}/{name}");
    let script = format!(
        "set -e; export PATH=/usr/local/cuda/bin:$PATH; \
         nvcc -arch={GX10_SM} -O2 {extra} -o {remote_bin} {remote_cu}; {remote_bin}"
    );
    let out = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=20", GX10_HOST])
        .arg(&script)
        .output()
        .map_err(|e| format!("spawn ssh {name}: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        return Err(format!(
            "gx10 {name} compile/run failed: stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let line = stdout.trim();
    if let Some(rest) = line.strip_prefix("ERR") {
        return Err(format!("gx10 {name} device error:{rest}"));
    }
    line.split_whitespace()
        .map(|tok| {
            tok.parse::<f64>()
                .map_err(|e| format!("parse float `{tok}` from gx10 {name}: {e}"))
        })
        .collect()
}

/// Run the gx10 (sm_121) anti-correlation arm: emit xpile's PTX + the nvcc
/// CUDA-C for `sm_121`, transfer both to a `mktemp -d` on gx10, compile + run
/// both there, diff, and CLEAN UP the remote temp dir. Returns the executed
/// xpile-PTX output vector (for the cross-arch comparison) and the
/// `DiffExecResult` of the on-gx10 anti-correlation diff.
fn run_gx10_arm() -> Result<(Vec<f64>, DiffExecResult), String> {
    // A unique remote working dir; cleaned up unconditionally at the end.
    let mk = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", GX10_HOST])
        .arg("mktemp -d /tmp/xpile-gx10-XXXXXX")
        .output()
        .map_err(|e| format!("spawn ssh mktemp: {e}"))?;
    if !mk.status.success() {
        return Err(format!(
            "gx10 mktemp -d failed: {}",
            String::from_utf8_lossy(&mk.stderr)
        ));
    }
    let remote_dir = String::from_utf8_lossy(&mk.stdout).trim().to_string();
    if remote_dir.is_empty() {
        return Err("gx10 mktemp -d returned empty path".to_string());
    }

    // The actual work, with cleanup guaranteed afterward.
    let result = (|| {
        // xpile's OWN hand-emitted PTX for sm_121 — `.target`/`.version`
        // derived from the capability (Blackwell needs ISA ≥ 8.8).
        let xpile_ptx = emit_kernel(&saxpy_kernel_fn(), GX10_SM)
            .map_err(|e| format!("emit xpile PTX for {GX10_SM}: {e}"))?;
        let driver_src = PtxDiffExecEngine::driver_harness(&xpile_ptx);
        let nvcc_src = NvccCudaDiffExecEngine::harness(SAXPY_CUDA_C_KERNEL);

        // Both arms execute ON gx10 (sm_121). The Driver-API one links -lcuda.
        let xpile_out =
            gx10_compile_run_parse(&remote_dir, &driver_src, "xpile_ptx_driver", "-lcuda")?;
        let nvcc_out = gx10_compile_run_parse(&remote_dir, &nvcc_src, "nvcc_cuda_c", "")?;

        if xpile_out.len() != nvcc_out.len() {
            return Ok((
                xpile_out,
                DiffExecResult::Divergent {
                    max_abs_diff: f64::INFINITY,
                    tolerance: 1.0e-3,
                },
            ));
        }
        let max_abs_diff = xpile_out
            .iter()
            .zip(nvcc_out.iter())
            .map(|(g, s)| (g - s).abs())
            .fold(0.0_f64, f64::max);
        let res = if max_abs_diff <= 1.0e-3 {
            DiffExecResult::Match { max_abs_diff }
        } else {
            DiffExecResult::Divergent {
                max_abs_diff,
                tolerance: 1.0e-3,
            }
        };
        Ok((xpile_out, res))
    })();

    // Clean up the remote temp dir — no persistent state on gx10. Best-effort;
    // a cleanup failure does not mask the witness result.
    let _ = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", GX10_HOST])
        .arg(format!("rm -rf {remote_dir}"))
        .output();

    result
}

/// Run the local sm_89 (RTX 4090 / Ada) anti-correlation arm via the production
/// [`PtxDiffExecEngine`], returning the executed xpile-PTX output (for the
/// cross-arch comparison) and the `DiffExecResult`.
///
/// The engine itself diffs xpile-PTX vs nvcc-CUDA-C internally; to ALSO capture
/// the raw xpile-PTX vector for the cross-architecture vote we re-run the same
/// Driver-API harness the engine uses. Both use the identical
/// [`PtxDiffExecEngine::driver_harness`] + [`saxpy_kernel_fn`], so the captured
/// vector is exactly what the engine compared.
fn run_local_arm(local_sm: &str) -> Result<(Vec<f64>, DiffExecResult), String> {
    let cfg = BackendConfig {
        target: Target::Ptx,
        profile: Profile::RustOut,
        hardware: Some(HwProfile::Ptx {
            compute_capability: local_sm.to_string(),
        }),
    };
    let module = Module {
        name: "saxpy_kernel".into(),
        source_lang: SourceLang::Rust,
        items: Vec::new(),
        ffi_boundaries: Vec::new(),
    };
    let engine = PtxDiffExecEngine::new();
    let xpile_ptx =
        emit_kernel(&saxpy_kernel_fn(), local_sm).map_err(|e| format!("emit local PTX: {e}"))?;
    // The engine's anti-correlation diff (xpile-PTX vs nvcc-CUDA-C) on sm_89.
    let diff =
        engine.execute_and_compare(&xpile_ptx, SAXPY_CUDA_C_KERNEL, &module, &cfg, 1.0e-3)?;
    // Capture the raw xpile-PTX output for the cross-arch comparison by running
    // the same Driver-API harness locally.
    let xpile_out = run_local_driver(&xpile_ptx, local_sm)?;
    Ok((xpile_out, diff))
}

/// Compile + run the xpile Driver-API harness locally for `local_sm`, parsing
/// the printed vector — the local twin of [`gx10_compile_run_parse`].
fn run_local_driver(xpile_ptx: &str, local_sm: &str) -> Result<Vec<f64>, String> {
    let dir = std::env::temp_dir().join(format!("xpile-local-xarch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("create local dir: {e}"))?;
    let cu = dir.join("xpile_local.cu");
    let bin = dir.join("xpile_local");
    std::fs::write(&cu, PtxDiffExecEngine::driver_harness(xpile_ptx))
        .map_err(|e| format!("write local cu: {e}"))?;
    let compile = Command::new("nvcc")
        .arg(format!("-arch={local_sm}"))
        .args(["-O2", "-o"])
        .arg(&bin)
        .arg("-lcuda")
        .arg(&cu)
        .output()
        .map_err(|e| format!("spawn local nvcc: {e}"))?;
    if !compile.status.success() {
        return Err(format!(
            "local nvcc failed: {}",
            String::from_utf8_lossy(&compile.stderr)
        ));
    }
    let run = Command::new(&bin)
        .output()
        .map_err(|e| format!("spawn local bin: {e}"))?;
    let stdout = String::from_utf8_lossy(&run.stdout);
    if !run.status.success() {
        return Err(format!("local run non-zero: {stdout:?}"));
    }
    let line = stdout.trim();
    if let Some(rest) = line.strip_prefix("ERR") {
        return Err(format!("local device error:{rest}"));
    }
    line.split_whitespace()
        .map(|t| t.parse::<f64>().map_err(|e| format!("parse `{t}`: {e}")))
        .collect()
}

/// The local GPU's compute capability via `nvidia-smi` (`sm_<maj><min>`),
/// falling back to the contract floor `sm_80`.
fn local_compute_capability() -> String {
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=compute_cap", "--format=csv,noheader"])
        .output();
    if let Ok(o) = out {
        if o.status.success() {
            let raw = String::from_utf8_lossy(&o.stdout);
            if let Some(line) = raw.lines().next() {
                if let Some((maj, min)) = line.trim().split_once('.') {
                    return format!("sm_{}{}", maj.trim(), min.trim());
                }
            }
        }
    }
    "sm_80".to_string()
}

/// PMAT-963 — the CROSS-HARDWARE anti-correlation vote: run the §29 PTX witness
/// on BOTH the local RTX 4090 (sm_89 / Ada) AND the gx10 GB10 (sm_121 /
/// Blackwell), and assert (a) anti-correlation holds on gx10 and (b) the same
/// xpile emitter's executed results agree ACROSS the two architectures.
///
/// Graceful-skip: the cross-hardware vote needs BOTH a local CUDA box AND a
/// reachable gx10 with a CUDA toolchain. Absent either, the test records a
/// clean skip and stays green (free CI has neither).
#[test]
fn ptx_cross_hardware_anti_correlation_sm89_and_sm121() {
    if !cuda_toolchain_available() {
        eprintln!(
            "PMAT-963: skipping cross-hardware PTX witness — no local CUDA \
             toolchain (nvcc/nvidia-smi). The local arm needs a GPU box; free \
             CI records a clean skip and stays green."
        );
        return;
    }
    if !gx10_available() {
        eprintln!(
            "PMAT-963: skipping cross-hardware PTX witness — the gx10 (GB10 / \
             sm_121) fleet host is unreachable or lacks nvcc/ptxas. A box with \
             gx10 in ~/.ssh/config runs the real cross-arch vote (sm_89 local + \
             sm_121 remote); free CI records a clean skip and stays green."
        );
        return;
    }

    let local_sm = local_compute_capability();
    eprintln!(
        "PMAT-963: running CROSS-HARDWARE anti-correlation PTX witness — local \
         {local_sm} (Ada) + gx10 {GX10_SM} (GB10 / Blackwell)"
    );

    // ── arm 1: local sm_89 (Ada) ──────────────────────────────────────
    let (local_xpile, local_diff) =
        run_local_arm(&local_sm).expect("local sm_89 anti-correlation arm runs");
    match &local_diff {
        DiffExecResult::Match { max_abs_diff } => eprintln!(
            "PMAT-963: [sm_89 LOCAL] xpile-PTX vs nvcc-CUDA-C AGREE \
             (max_abs_diff={max_abs_diff}); xpile-PTX out = {local_xpile:?}"
        ),
        other => panic!("local sm_89 arm did not Match (anti-correlation falsified): {other:?}"),
    }

    // ── arm 2: gx10 sm_121 (Blackwell), executed remotely ─────────────
    let (gx10_xpile, gx10_diff) =
        run_gx10_arm().expect("gx10 sm_121 anti-correlation arm runs over ssh/scp");
    match &gx10_diff {
        DiffExecResult::Match { max_abs_diff } => eprintln!(
            "PMAT-963: [sm_121 gx10] xpile-PTX vs nvcc-CUDA-C AGREE \
             (max_abs_diff={max_abs_diff}); xpile-PTX out = {gx10_xpile:?}"
        ),
        other => panic!("gx10 sm_121 arm did not Match (arch-specific emit bug?): {other:?}"),
    }

    // ── vote (b): CROSS-ARCHITECTURE independence ─────────────────────
    // The SAME xpile emitter, lowered for two different architectures, must
    // produce bit-identical executed results over the fixture. A divergence
    // here is an arch-specific emit bug invisible to either single-arch arm.
    assert_eq!(
        local_xpile.len(),
        FIXTURE_INPUT.len(),
        "local arm should produce one output per fixture element"
    );
    assert_eq!(
        local_xpile.len(),
        gx10_xpile.len(),
        "cross-arch output arity mismatch: sm_89 {} vs sm_121 {}",
        local_xpile.len(),
        gx10_xpile.len()
    );
    let cross_arch_max_diff = local_xpile
        .iter()
        .zip(gx10_xpile.iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f64, f64::max);
    assert!(
        cross_arch_max_diff <= 1.0e-3,
        "CROSS-ARCHITECTURE divergence (arch-specific emit bug): xpile-PTX on \
         sm_89 {local_xpile:?} vs sm_121 {gx10_xpile:?}, max_abs_diff={cross_arch_max_diff}"
    );

    eprintln!(
        "PMAT-963: CROSS-HARDWARE ANTI-CORRELATION VOTE PASSED — xpile's \
         hand-emitted PTX agrees with nvcc-CUDA-C on BOTH sm_89 (local, Ada) \
         AND sm_121 (gx10, GB10 / Blackwell), and the SAME emitter's results \
         agree ACROSS the two architectures (cross-arch max_abs_diff={cross_arch_max_diff}). \
         The §29 PTX quorum now spans 2 emitters x 2 architectures."
    );
}

/// The cross-hardware witness's graceful-skip path stays well-behaved: when the
/// gx10 fleet host is absent, [`gx10_available`] returns false WITHOUT panicking
/// (it must never hang or error CI). This keeps the skip branch under test on
/// every host — the standard substrate posture (cc/python3/nvcc all do this).
#[test]
fn gx10_availability_probe_is_well_behaved() {
    // Just asserting it returns a bool without panicking / hanging. On CI it's
    // false (no gx10); locally it may be true. Either way no crash.
    let _ = gx10_available();
}
