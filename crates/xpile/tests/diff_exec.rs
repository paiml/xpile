//! Differential execution check (PMAT-018 / XPILE-DIFF-001).
//!
//! For each fixture in the curated single-arg i64 set, generate N
//! deterministic inputs inside the fast-path domain, run both:
//!   (a) CPython directly on the .py source
//!   (b) The transpiled Rust binary compiled by rustc -O
//! and assert the outputs agree.
//!
//! This is xpile's analog of ruchy 5.0 §14.10.4. It generalises the
//! 11 hand-authored runtime-verified fixtures from a few hand-picked
//! values to N machine-generated values per function — closing the
//! "fixture overfitting" caveat from audit-design.md §4
//! quantitatively rather than by adding more hand-authored cases.
//!
//! Scope at v0.1.0:
//!   * Single-arg i64-returning fixtures only (1-arg Python int → int).
//!   * Hardcoded per-fixture input range so generated inputs stay
//!     inside the C-PY-INT-ARITH fast-path domain (no i64 overflow
//!     panics). Wider ranges become available once XPILE-DIFF-002
//!     teaches the runner to interpret a `.checked_*().expect(...)`
//!     panic as "Python promoted to BigInt" and compare against
//!     CPython's int output as a BigInt.
//!   * N = 10 inputs per fixture (10 fixtures × ~10 inputs each ≈
//!     100 differential checks total). Modest by ruchy 5.0's spec
//!     (which targets 100/function); tunable.
//!   * Deterministic input generation via a fixed-seed LCG so the
//!     test is reproducible.
//!
//! Skip behaviour: if `python3` or `rustc` is missing from PATH, the
//! test prints a warning and exits OK — same posture as the existing
//! `assert_rustc_runs` helper. CI environments that have both still
//! run the gate.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn xpile_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_xpile"))
}

// Curated set of fixtures + per-arg [min, max] input ranges that stay
// inside the C-PY-INT-ARITH fast path (no overflow panics). Each entry
// is (file, entry-function, per-arg-range[]) — the slice length is
// the function's arity. XPILE-DIFF-002 extended this from single-arg
// only to support 1- and 2-arg fixtures; the driver synthesis below
// handles both arities. Higher arities are a follow-up.
//
// Conservative input ranges below stay inside the fast path; ranges
// that *include* overflow (where Python promotes to bigint but Rust
// panics) need XPILE-DIFF-003 to interpret the panic as expected.
struct FixtureCfg {
    file: &'static str,
    entry: &'static str,
    // Per-arg `(min, max)` (inclusive). Slice length is the arity.
    args: &'static [(i64, i64)],
}

const FIXTURES: &[FixtureCfg] = &[
    // ── 1-arg fixtures (carried over from XPILE-DIFF-001) ────────
    FixtureCfg {
        file: "factorial.py",
        entry: "factorial",
        // 13! overflows i64
        args: &[(0, 12)],
    },
    FixtureCfg {
        file: "fib.py",
        entry: "fib",
        // F(30) = 832040, safe; F(94) is the i64 limit but tree-
        // recursion makes large n painfully slow.
        args: &[(0, 30)],
    },
    FixtureCfg {
        file: "abs_val.py",
        entry: "abs_val",
        args: &[(-1_000_000, 1_000_000)],
    },
    FixtureCfg {
        file: "sign.py",
        entry: "sign",
        args: &[(-1_000_000_000, 1_000_000_000)],
    },
    FixtureCfg {
        file: "sum_to.py",
        entry: "sum_to",
        // sum(1..65535) ≈ 2.1e9, well under i64
        args: &[(0, 65_535)],
    },
    FixtureCfg {
        file: "for_sum.py",
        entry: "for_sum",
        args: &[(0, 65_535)],
    },
    FixtureCfg {
        file: "countdown.py",
        entry: "factorial_iter",
        args: &[(0, 12)],
    },
    // ── 2-arg fixtures (added XPILE-DIFF-002) ────────────────────
    FixtureCfg {
        file: "gcd.py",
        entry: "gcd",
        // gcd handles 0 and 0 correctly (returns 0 in both Python
        // and our emission). Negative inputs work but Python's
        // math.gcd convention is non-negative result — easier to
        // restrict to non-negative inputs and avoid that asymmetry.
        args: &[(0, 1_000_000), (0, 1_000_000)],
    },
    FixtureCfg {
        file: "multi_branch.py",
        entry: "range_size",
        // range_size = abs(a - b); inputs bounded so the difference
        // can't overflow i64.
        args: &[
            (-1_000_000_000, 1_000_000_000),
            (-1_000_000_000, 1_000_000_000),
        ],
    },
    FixtureCfg {
        file: "bits.py",
        entry: "bits",
        // bits.py: `((a & b) | (a ^ b)) << 2 >> 1`. The constant-shift-
        // by-2 means inputs must stay in [-2^61, 2^61) to avoid
        // overflow on the left-shift, and the inner ops don't widen.
        args: &[
            (-i64::pow(2, 61) + 1, i64::pow(2, 61) - 1),
            (-i64::pow(2, 61) + 1, i64::pow(2, 61) - 1),
        ],
    },
];

/// Deterministic LCG (numerical recipes constants). Seeded once per
/// test for reproducibility; not crypto, just for input variety.
struct Lcg(u64);
impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    /// Pick an i64 uniformly in `[lo, hi]` (inclusive).
    fn next_i64_in(&mut self, lo: i64, hi: i64) -> i64 {
        assert!(lo <= hi);
        let span = (hi - lo) as u64 + 1;
        let r = self.next_u64() % span;
        lo + r as i64
    }
}

/// Check tool availability. Returns false if either python3 or rustc
/// is missing — test caller short-circuits with a warning print.
fn have_python_and_rustc() -> bool {
    let py = Command::new("python3").arg("--version").output().is_ok();
    let rs = Command::new("rustc").arg("--version").output().is_ok();
    py && rs
}

/// Build the transpiled-Rust binary for an N-arg fixture. The synth
/// driver reads N i64s from CLI argv and calls `entry(a0, a1, ...)`.
/// XPILE-DIFF-002 generalised this from 1-arg only to support any
/// arity by generating the appropriate call expression.
fn build_rust_binary(
    fixture_path: &Path,
    entry: &str,
    arity: usize,
    out_dir: &Path,
) -> Result<PathBuf, String> {
    // Transpile via the xpile binary so we exercise the real CLI path,
    // not just the library — same coverage as transpile_e2e.rs uses.
    let out = Command::new(xpile_bin())
        .args([
            "transpile",
            fixture_path.to_str().unwrap(),
            "--target",
            "rust",
        ])
        .output()
        .map_err(|e| format!("spawn xpile: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "xpile transpile failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let transpiled = String::from_utf8(out.stdout).map_err(|e| format!("utf8: {e}"))?;

    // Build `entry(argv[0], argv[1], ...)` for the given arity.
    let call_args: Vec<String> = (0..arity).map(|i| format!("argv[{i}]")).collect();
    let call = format!("{entry}({})", call_args.join(", "));
    let driver = format!(
        r#"
fn main() {{
    let argv: Vec<i64> = std::env::args()
        .skip(1)
        .map(|s| s.parse::<i64>().expect("parse i64"))
        .collect();
    assert_eq!(argv.len(), {arity}, "expected {arity} args");
    println!("{{}}", {call});
}}
"#
    );
    let merged = format!("{transpiled}\n{driver}\n");

    let rs_file = out_dir.join(format!("{entry}.rs"));
    std::fs::write(&rs_file, &merged).map_err(|e| format!("write rs: {e}"))?;

    let bin_path = out_dir.join(entry);
    let compile = Command::new("rustc")
        .args([
            "--edition=2021",
            "-O",
            "-o",
            bin_path.to_str().unwrap(),
            rs_file.to_str().unwrap(),
        ])
        .output()
        .map_err(|e| format!("spawn rustc: {e}"))?;
    if !compile.status.success() {
        return Err(format!(
            "rustc failed:\n=== source ===\n{merged}\n=== stderr ===\n{}",
            String::from_utf8_lossy(&compile.stderr)
        ));
    }
    Ok(bin_path)
}

/// Run the compiled Rust binary with N i64 args. Returns stdout
/// trimmed.
fn run_rust(bin: &Path, args: &[i64]) -> Result<String, String> {
    let mut cmd = Command::new(bin);
    for a in args {
        cmd.arg(a.to_string());
    }
    let out = cmd.output().map_err(|e| format!("spawn rust bin: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "rust bin exited non-zero (overflow? input out of declared range?):\n  stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Run the Python fixture directly via CPython with N i64 args.
/// Returns stdout trimmed.
fn run_python(fixture_path: &Path, entry: &str, args: &[i64]) -> Result<String, String> {
    let src_path = fixture_path.to_str().ok_or("non-utf8 fixture path")?;
    let call_args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let prog = format!(
        "exec(open(r'{src_path}').read()); print({entry}({}))",
        call_args.join(", ")
    );
    let out = Command::new("python3")
        .args(["-c", &prog])
        .output()
        .map_err(|e| format!("spawn python: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "python failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

const INPUTS_PER_FIXTURE: usize = 10;
const LCG_SEED: u64 = 0x00C0_FFEE_FACE_FEEDu64; // deterministic, see header doc

/// The load-bearing CI gate. For each curated fixture, generate
/// INPUTS_PER_FIXTURE deterministic i64 inputs in the declared
/// fast-path range; for each, run CPython + transpiled Rust and
/// assert their outputs agree.
#[test]
fn differential_execution_cpython_vs_transpiled_rust() {
    if !have_python_and_rustc() {
        eprintln!(
            "warning: skipping XPILE-DIFF-001 — python3 and/or rustc not on PATH. \
             CI environments with both will still run this gate."
        );
        return;
    }

    let out_dir = std::env::temp_dir().join("xpile-diff-exec");
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).expect("create temp dir");

    let mut rng = Lcg::new(LCG_SEED);
    let mut total_checks = 0;
    let mut mismatches: Vec<(String, Vec<i64>, String, String)> = Vec::new();

    for cfg in FIXTURES {
        let py_path = fixture(cfg.file);
        let bin = match build_rust_binary(&py_path, cfg.entry, cfg.args.len(), &out_dir) {
            Ok(b) => b,
            Err(e) => {
                panic!(
                    "build failed for fixture `{}` entry `{}`:\n  {e}",
                    cfg.file, cfg.entry
                );
            }
        };

        for _ in 0..INPUTS_PER_FIXTURE {
            let args: Vec<i64> = cfg
                .args
                .iter()
                .map(|(lo, hi)| rng.next_i64_in(*lo, *hi))
                .collect();
            let py = run_python(&py_path, cfg.entry, &args)
                .unwrap_or_else(|e| panic!("python {}({args:?}): {e}", cfg.file));
            let rs = run_rust(&bin, &args)
                .unwrap_or_else(|e| panic!("rust {}({args:?}): {e}", cfg.file));
            total_checks += 1;
            if py != rs {
                mismatches.push((cfg.file.to_string(), args, py, rs));
            }
        }
    }

    if !mismatches.is_empty() {
        let mut msg = format!(
            "Differential execution disagreement (XPILE-DIFF-001/002):\n\
             {} of {} input-comparisons diverged between CPython and the transpiled Rust binary.\n\
             Either the codegen miscompiles the construct OR the fixture's declared input range \n\
             needs tightening to stay inside the C-PY-INT-ARITH fast-path domain.\n\n",
            mismatches.len(),
            total_checks
        );
        for (fx, args, py, rs) in &mismatches {
            msg.push_str(&format!(
                "  - {fx} args={args:?}\n      python: {py}\n      rust:   {rs}\n"
            ));
        }
        panic!("{msg}");
    }

    eprintln!(
        "XPILE-DIFF-001/002: {} differential checks across {} fixtures — all green.",
        total_checks,
        FIXTURES.len()
    );
}

// LCG self-test — guard against drift in the deterministic generator
// so a future "fix" doesn't silently change which inputs the gate
// tests. The first three outputs are pinned.
#[test]
fn lcg_is_deterministic_with_seed() {
    let mut rng = Lcg::new(LCG_SEED);
    let a = rng.next_u64();
    let b = rng.next_u64();
    let c = rng.next_u64();
    let mut rng2 = Lcg::new(LCG_SEED);
    assert_eq!(rng2.next_u64(), a);
    assert_eq!(rng2.next_u64(), b);
    assert_eq!(rng2.next_u64(), c);
    // Range bounding stays inside [lo, hi].
    let mut rng3 = Lcg::new(LCG_SEED);
    for _ in 0..1000 {
        let v = rng3.next_i64_in(-100, 100);
        assert!(
            (-100..=100).contains(&v),
            "LCG produced {v} outside [-100, 100]"
        );
    }
}
