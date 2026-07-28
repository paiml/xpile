//! PMAT-1353 — `xpile hybrid <dir> --verify --repair`: the CLI seam for
//! `xpile-agent`'s bounded, fail-closed, deterministic repair loop.
//!
//! **What this file exists to prevent.** Before PMAT-1353, `crates/xpile-agent`
//! held 931 lines, 3 `RepairRule` impls, a probe that runs cc + rustc + the
//! linked binary, and 18 passing tests — and `grep -rn RepairLoop crates/`
//! matched NOTHING outside that crate. A capability no user can invoke is not a
//! capability; it is a test suite. So the load-bearing assertion here is not
//! "the loop is correct" (`xpile-agent`'s own tests cover that) but **"a user
//! can reach it, and reaching it does something true"**.
//!
//! ## The four properties, and why each one is here
//!
//! 1. **It CONVERGES on a symptom the emitter really produces**
//!    ([`repair_converges_on_a_production_emitted_e0308`]). A repair loop that
//!    has only ever been observed to fix an INJECTED defect is indistinguishable
//!    from one that cannot fix anything. `fixtures/hybrid_unsigned` carries a
//!    real, still-open `E0308`: the Python frontend lowers the boundary call with
//!    its unknown-callee `i64` default (`bump(3i64)`) while `emit_c_shim`'s
//!    PMAT-918 wrapper takes `u32`, so the emitted workspace does not compile.
//!    `--repair` inserts the missing call-site cast and the artifact then agrees
//!    with CPython.
//!
//! 2. **It FAILS CLOSED when no rule applies**
//!    ([`repair_fails_closed_when_no_rule_applies`]). `fixtures/hybrid_divergent`
//!    diverges through a class no wired rule matches. The honest answer is a
//!    NON-ZERO exit naming the unrepaired symptom — never "diagnosis complete,
//!    exit 0". This is the assertion that stops `--repair` from becoming a way to
//!    turn a red `--verify` green.
//!
//! 3. **`--verify` WITHOUT `--repair` is unchanged**
//!    ([`repair_off_is_byte_identical_to_plain_verify`]). The flag lands three
//!    days before a release; the guarantee that makes that safe is that the
//!    default path's stdout, stderr and exit code are byte-for-byte what they
//!    were. Asserted here as "`--verify` == `--verify --repair`" on every lane
//!    where the artifact already MATCHES, so the new code is proven inert on the
//!    success path.
//!
//! 4. **It writes NOTHING**
//!    ([`repair_writes_nothing_and_leaves_no_workspace_behind`]). The loop's
//!    fail-closed design is structural (`RepairOutcome::Exhausted` has no
//!    `source` field), but the CLI could still have committed a repair. It does
//!    not: the fixture is byte-identical after a converging run and no probe
//!    workspace survives.
//!
//! ## Honest scope — read before quoting "the repair loop is wired"
//!
//! ONE of `xpile-agent`'s three rules is reachable through this seam, and
//! `main.rs::boundary_repair_rules` documents why for each:
//! `FfiReturnCastRepair` targets `__r` in `src/ffi_shims.rs`, which the probe
//! REGENERATES from the manifest every iteration, so its text cannot occur in the
//! candidate; `FloatReprRepair` targets a plain `println!("{}", <float>)`, which
//! this emitter — measured, not assumed — no longer produces, because PMAT-931
//! fixed that class in the production seam. Both have provably empty domains
//! here. Wiring them anyway would inflate a capability count with rules that can
//! never fire.
//!
//! ## Inverted tripwire — deliberate, and here is the instruction
//!
//! `hybrid_unsigned` is asserted to FAIL TO BUILD. The day the Python frontend
//! retypes unsigned call sites the way `retype_float_ffi_sites` retypes float
//! ones, that fixture compiles, and properties 1 and the `--verify` half of this
//! file go RED. That is correct and it is the prompt to act: re-point the repair
//! witness at whatever `E0308` the emitter then produces, or — if none remains —
//! record that `FfiArgCastRepair`'s domain has become empty too and say so in the
//! docs instead of keeping a green that no longer means anything.
//!
//! Gated on cc + python3 + cargo so a constrained runner skips gracefully.

use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn tool_available(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

fn toolchain() -> bool {
    tool_available("cc") && tool_available("python3") && tool_available("cargo")
}

/// Run `xpile hybrid <fixture> --verify [--repair]`.
fn run(name: &str, repair: bool) -> Output {
    run_in(name, repair, None)
}

/// Same, with an optional PRIVATE `TMPDIR` (PMAT-1436).
///
/// `std::env::temp_dir()` reads `TMPDIR` on Unix, and every scratch directory
/// the hybrid lane makes — the `--verify` workspace and the `--repair` probe
/// root alike — hangs off it. Handing a run its own `TMPDIR` is what lets a
/// witness assert about THAT RUN's disk footprint instead of globbing a
/// namespace it shares with every sibling test and every stale aborted run.
fn run_in(name: &str, repair: bool, tmpdir: Option<&std::path::Path>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xpile"));
    cmd.arg("hybrid").arg(fixture(name)).arg("--verify");
    if repair {
        cmd.arg("--repair");
    }
    if let Some(t) = tmpdir {
        cmd.env("TMPDIR", t);
    }
    cmd.output().expect("run xpile hybrid --verify")
}

/// The `probe workspaces: N built under <root> — <disposition>` disclosure,
/// parsed out of a run's OWN stdout. Returns `(built, root, disposition)`.
///
/// PMAT-1436: this is the identity the old witness lacked. It never asked the
/// binary what it did; it listed a shared directory and subtracted.
fn probe_disclosure(stdout: &str) -> (usize, PathBuf, String) {
    let line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("probe workspaces:"))
        .unwrap_or_else(|| {
            panic!("the repair loop must disclose its probe-workspace root and count:\n{stdout}")
        })
        .trim();
    let rest = line
        .strip_prefix("probe workspaces: ")
        .expect("disclosure prefix");
    let (built, rest) = rest.split_once(" built under ").expect("`N built under`");
    let (root, disposition) = rest.rsplit_once(" — ").expect("` — <disposition>`");
    (
        built.parse().expect("the built count must be a number"),
        PathBuf::from(root),
        disposition.to_string(),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. CONVERGENCE on a production-emitted symptom.
// ─────────────────────────────────────────────────────────────────────────────

/// `--verify --repair` on `hybrid_unsigned` exits 0 and prints the converged rule
/// chain. NON-VACUITY is asserted three ways, because each one alone could pass
/// for the wrong reason: the boundary must have reconciled (else this is a
/// reconcile failure wearing a repair failure's clothes), the loop must have
/// STARTED (else "exit 0" could just mean nothing was attempted), and the E0308
/// it repaired must appear on stderr (else the loop might have converged on some
/// other symptom entirely).
#[test]
fn repair_converges_on_a_production_emitted_e0308() {
    if !toolchain() {
        eprintln!("cc/python3/cargo unavailable — skipping hybrid --repair convergence test");
        return;
    }
    let out = run("hybrid_unsigned", true);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        out.status.success(),
        "--verify --repair must exit 0 once the loop converges;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("✓ REPAIRED in 1 iteration(s)"),
        "expected the converged verdict with its iteration count:\n{stdout}"
    );
    assert!(
        stdout.contains("applied rule chain: [\"ffi-arg-cast\"]"),
        "the applied rule CHAIN must be printed — an iteration count alone does not \
         say WHICH deterministic rule fired:\n{stdout}"
    );
    // NON-VACUITY (a): the loop was really entered, with a derived rule set.
    assert!(
        stdout.contains("--repair: bounded repair loop — 1 rule(s) [\"ffi-arg-cast\"]"),
        "the loop must announce the rules DERIVED from the manifest:\n{stdout}"
    );
    // NON-VACUITY (b): the boundary reconciled, so this is a build failure being
    // repaired, not a reconcile failure being skipped past.
    assert!(
        stdout.contains("bump : Python → C"),
        "the FFI boundary must still reconcile:\n{stdout}"
    );
    // NON-VACUITY (c): the symptom repaired is the REAL E0308, printed before the
    // hand-off. Without this the test would pass if `--repair` converged on some
    // unrelated symptom.
    assert!(
        stderr.contains("hybrid artifact failed to build") && stderr.contains("error[E0308]"),
        "the original build failure must still be reported in full before the repair:\n{stderr}"
    );
    // The repair is REPORTED, not committed — stated in the output, asserted for
    // real in `repair_writes_nothing_and_leaves_no_workspace_behind`.
    assert!(
        stdout.contains("xpile wrote NOTHING to your tree"),
        "the fail-closed no-write posture must be stated to the operator:\n{stdout}"
    );
}

/// The other half of the same finding: WITHOUT `--repair`, `hybrid_unsigned`
/// exits NON-ZERO naming the `E0308`.
///
/// This is the assertion that a disclosed skip was standing in front of a broken
/// emit. Until PMAT-1353 `ctypes_name` had no `CUInt` arm, so this fixture
/// printed `boundary `bump` has a non-ABI-mappable type — skipping` and exited
/// **0** — while `--emit-workspace` on the same fixture emitted a workspace that
/// does not compile. The negative assertion below is the load-bearing one: it
/// pins that the skip is GONE, not merely that a failure now happens.
#[test]
fn verify_reports_the_unsigned_build_failure_instead_of_skipping_it() {
    if !toolchain() {
        eprintln!("cc/python3/cargo unavailable — skipping unsigned --verify test");
        return;
    }
    let out = run("hybrid_unsigned", false);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        !out.status.success(),
        "`--verify` on a fixture whose emitted workspace does not compile must exit \
         NON-ZERO;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("non-ABI-mappable"),
        "the CUInt boundary must no longer be skipped as non-ABI-mappable — that skip \
         was a disclosed PASS in front of an uncompilable emit:\n{stdout}"
    );
    assert!(
        stderr.contains("hybrid artifact failed to build") && stderr.contains("error[E0308]"),
        "expected the real E0308 the skip used to hide:\n{stderr}"
    );
    // Pin the DIRECTION of the mismatch, so a future change that makes the build
    // fail for an unrelated reason cannot keep this test green.
    assert!(
        stderr.contains("expected `u32`, found `i64`"),
        "the build failure must be the unsigned call-site retype hole specifically \
         (wrapper takes u32, call site lowered i64):\n{stderr}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. FAIL-CLOSED when no rule applies.
// ─────────────────────────────────────────────────────────────────────────────

/// `--verify --repair` on `hybrid_divergent` exits NON-ZERO. Its divergence
/// (`[1, 2.5]` vs `[1.0, 2.5]`, the open float-annotated-container policy) is
/// outside every wired rule's domain, so the loop applies nothing and fails
/// closed — and the empty `rules applied: []` is asserted explicitly, because
/// "exhausted after trying nothing" and "exhausted after trying everything" are
/// different findings and the operator needs to know which one happened.
#[test]
fn repair_fails_closed_when_no_rule_applies() {
    if !toolchain() {
        eprintln!("cc/python3/cargo unavailable — skipping hybrid --repair fail-closed test");
        return;
    }
    let out = run("hybrid_divergent", true);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        !out.status.success(),
        "an unrepaired divergence must keep the NON-ZERO exit — `--repair` must never \
         convert a red `--verify` into a green;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // The original verdict survives intact: the hand-off happens AFTER it prints.
    assert!(
        stderr.contains("✗ DIVERGENT at line 2:") && stderr.contains("CPython:  [1, 2.5]"),
        "the divergence verdict must still be reported in full:\n{stderr}"
    );
    assert!(
        stderr.contains("✗ NOT REPAIRED") && stderr.contains("fail-closed after 0 iteration(s)"),
        "expected the fail-closed repair verdict:\n{stderr}"
    );
    assert!(
        stderr.contains("rules applied: []"),
        "an EMPTY applied chain must be reported as empty — no rule matched this \
         symptom class:\n{stderr}"
    );
    assert!(
        stderr.contains("last symptom: Divergence at line 2"),
        "the unrepaired symptom must be named, so the operator learns WHICH class went \
         unrepaired:\n{stderr}"
    );
    assert!(
        stderr.contains("hybrid repair: exhausted without reaching a match (fail-closed)"),
        "expected the fail-closed bail reason:\n{stderr}"
    );
    // PMAT-1436: the EXHAUSTED path discloses and cleans up too. A loop that
    // bails is the one most likely to leave debris, and it is the path the
    // no-write witness — which only ever exercises a CONVERGING run — cannot see.
    let (built, root, disposition) = probe_disclosure(&stdout);
    assert_eq!(
        built, 1,
        "a fail-closed run still probes the initial candidate once:\n{stdout}"
    );
    assert_eq!(
        disposition, "all removed",
        "the fail-closed path must clean up its probe root, not only the converging \
         one:\n{stdout}"
    );
    assert!(
        !root.exists(),
        "the fail-closed run's probe root must not survive it: {}",
        root.display()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. The default path is UNCHANGED (the release-window safety property).
// ─────────────────────────────────────────────────────────────────────────────

/// On every lane whose artifact already MATCHES, `--verify` and
/// `--verify --repair` produce byte-identical stdout, byte-identical stderr and
/// the same exit code. The repair code is inert on the success path.
///
/// This is the in-tree form of the mandatory regression. It cannot compare
/// against the pre-PMAT-1353 BINARY (there is only one binary in a test run), so
/// it asserts the property that actually matters going forward: adding the flag
/// changes nothing unless the flag is used AND the artifact failed. The
/// two-binary check was run once by hand at the slice — `--verify` on all ten
/// pre-existing `hybrid_*` fixtures, stdout + stderr + exit code identical
/// between `origin/main` and this branch — and is recorded in the CHANGELOG
/// rather than pinned here, because a test cannot build its own ancestor.
#[test]
fn repair_off_is_byte_identical_to_plain_verify() {
    if !toolchain() {
        eprintln!("cc/python3/cargo unavailable — skipping --repair inertness test");
        return;
    }
    for name in [
        "hybrid_sum",
        "hybrid_scale2",
        "hybrid_dot2",
        "hybrid_pysibling",
    ] {
        let plain = run(name, false);
        let with_repair = run(name, true);
        assert_eq!(
            plain.status.code(),
            with_repair.status.code(),
            "{name}: --repair changed the exit code on a MATCHING artifact"
        );
        assert_eq!(
            String::from_utf8_lossy(&plain.stdout),
            String::from_utf8_lossy(&with_repair.stdout),
            "{name}: --repair changed stdout on a MATCHING artifact"
        );
        assert_eq!(
            String::from_utf8_lossy(&plain.stderr),
            String::from_utf8_lossy(&with_repair.stderr),
            "{name}: --repair changed stderr on a MATCHING artifact"
        );
    }
}

/// The Shell lane is told it is NOT repairable, rather than being silently left
/// out. Every rule in `xpile-agent::repair` is a transform over emitted Rust; the
/// shell artifact is a re-emitted `.sh` spawned by a subprocess shim, so no rule
/// can apply. `--repair` adds exactly that one disclosure line and changes
/// nothing else — including the exit code.
#[test]
fn repair_discloses_that_the_shell_lane_has_no_rules() {
    if !toolchain() {
        eprintln!("cc/python3/cargo unavailable — skipping shell --repair disclosure test");
        return;
    }
    let plain = run("hybrid_shell", false);
    let with_repair = run("hybrid_shell", true);
    let repaired_out = String::from_utf8_lossy(&with_repair.stdout);
    assert_eq!(
        plain.status.code(),
        with_repair.status.code(),
        "--repair must not change the shell lane's exit code"
    );
    assert!(
        repaired_out.contains("Shell boundary(ies) are NOT repairable"),
        "the shell lane must be told it has no applicable rule, not silently omitted:\n{repaired_out}"
    );
    // Exactly ONE added line: the disclosure. Anything else would be a behaviour
    // change smuggled in behind an opt-in flag.
    let plain_lines = String::from_utf8_lossy(&plain.stdout).lines().count();
    assert_eq!(
        repaired_out.lines().count(),
        plain_lines + 1,
        "--repair must add exactly the one disclosure line to the shell lane:\n{repaired_out}"
    );
}

/// `--repair` without `--verify` is a usage ERROR, not a silent no-op. There is
/// no differential to repair against without `--verify`, and a flag that is
/// accepted and then ignored is how a user comes to believe a repair was
/// attempted when none was.
#[test]
fn repair_requires_verify() {
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("hybrid")
        .arg(fixture("hybrid_sum"))
        .arg("--repair")
        .output()
        .expect("run xpile hybrid --repair");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "`--repair` without `--verify` must be rejected, not silently ignored"
    );
    assert!(
        stderr.contains("--verify"),
        "the usage error must name the flag that is required:\n{stderr}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. It writes NOTHING.
// ─────────────────────────────────────────────────────────────────────────────

/// A converging `--repair` leaves the fixture byte-identical and removes every
/// probe workspace it created.
///
/// The loop's fail-closed guarantee is about `RepairOutcome` (exhaustion has no
/// `source` field), which says nothing about what the CLI does with a SUCCESSFUL
/// repair. This asserts the CLI's half: the repaired Rust never reaches disk, so
/// a `--repair` run cannot leave a derived artifact behind to drift from the
/// Python module it came from.
///
/// ## PMAT-1436 — how the temp-dir half used to be written, and why it was wrong
///
/// It globbed `std::env::temp_dir()` for names starting with
/// `"xpile_repair_ws_"`, snapshotted that list before and after, and asserted the
/// difference was empty. That identifies a NEIGHBOURHOOD, not a run, and it fails
/// in BOTH directions — both reproduced on this branch before the fix:
///
/// * **FALSE RED.** With one sibling `xpile … --repair` running concurrently,
///   this test failed with `leaked: ["/tmp/xpile_repair_ws_968061"]` — a root
///   created by another process, reported as "every probe workspace THIS RUN
///   created". Every test in this file that spawns `--repair` is a candidate
///   sibling, since `cargo test` runs them on concurrent threads and each child
///   gets its own pid, hence its own root. That is the flake observed once under
///   load at PMAT-1435 and left as its top lead.
/// * **FALSE GREEN.** Renaming the CLI's root prefix to `xpile_probe_ws_` and
///   deleting the final `remove_dir_all` made the run leak its root outright —
///   and this test, named `…_leaves_no_workspace_behind`, PASSED. The prefix was
///   a string literal duplicated here from `main.rs` with nothing tying the two
///   together, so relocating the subject made the witness blind to it.
///
/// Its doc comment also claimed it "pins that a run which builds N candidate
/// workspaces cleans up all N". It could not: it observed only an after-state,
/// so it could not distinguish cleaning up N from never building any.
///
/// ## What it does now
///
/// The run gets a PRIVATE `TMPDIR` and is asked what it did. Four assertions,
/// each closing one of the holes above:
///
/// 1. `N built` comes from the probe's own allocation counter and must be ≥ 1 —
///    the cleanup claim is now about something. Cross-checked against the
///    INDEPENDENTLY reported iteration count, so `N` cannot be a constant.
/// 2. The announced root must lie INSIDE the private `TMPDIR` — it moved when we
///    moved `TMPDIR`, so it is the path the binary really used and not a
///    fabricated string.
/// 3. That exact path must not exist. No set difference, so a pid reused from an
///    earlier aborted run cannot filter a real leak out of the answer.
/// 4. The private `TMPDIR` must be EMPTY. This is the assertion that survives a
///    rename: it catches a leak under ANY name, including the `--verify` lane's
///    own `xpile_verify_ws_*` workspace, which the old prefix glob never covered
///    at all.
#[test]
fn repair_writes_nothing_and_leaves_no_workspace_behind() {
    if !toolchain() {
        eprintln!("cc/python3/cargo unavailable — skipping --repair no-write test");
        return;
    }
    let dir = fixture("hybrid_unsigned");
    let snapshot = || -> Vec<(PathBuf, Vec<u8>)> {
        let mut v: Vec<(PathBuf, Vec<u8>)> = std::fs::read_dir(&dir)
            .expect("read fixture dir")
            .map(|e| {
                let p = e.expect("dir entry").path();
                let bytes = std::fs::read(&p).expect("read fixture file");
                (p, bytes)
            })
            .collect();
        v.sort();
        v
    };
    let before = snapshot();

    // A namespace nobody else writes to. Everything the run scratches lands here,
    // so "did THIS run clean up" becomes a question about a directory we own
    // rather than a subtraction over shared state.
    let tmp = std::env::temp_dir().join(format!(
        "xpile_repair_witness_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create the private TMPDIR");

    let out = run_in("hybrid_unsigned", true, Some(&tmp));
    let stdout = String::from_utf8_lossy(&out.stdout);
    // The precondition is that a repair actually RAN and converged — not merely
    // that the exit was 0. A green exit reached by skipping the boundary entirely
    // would satisfy `success()` while writing nothing for the trivial reason.
    assert!(
        out.status.success() && stdout.contains("✓ REPAIRED"),
        "precondition: the repair loop must have run and converged;\nstdout:\n{stdout}"
    );

    assert_eq!(
        before,
        snapshot(),
        "`--repair` must not write into the source tree — it DIAGNOSES, it does not commit"
    );

    let (built, root, disposition) = probe_disclosure(&stdout);

    // (1) NON-VACUITY: the cleanup claim is about workspaces that were really
    // built, and the count tracks the independently reported iteration count
    // (one probe of the initial candidate, then one per repair iteration).
    let iterations: usize = stdout
        .split("✓ REPAIRED in ")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("expected the iteration count in the verdict:\n{stdout}"));
    assert!(
        built >= 1,
        "a run that built NO probe workspace proves nothing about cleaning them up:\n{stdout}"
    );
    assert_eq!(
        built,
        iterations + 1,
        "the disclosed workspace count must track the reported iteration count — one \
         probe of the initial candidate plus one per iteration — or it is a constant \
         wearing a measurement's clothes:\n{stdout}"
    );

    // (2) The announced root MOVED with our TMPDIR, so it is the path the binary
    // allocated and not a string it prints regardless.
    assert!(
        root.starts_with(&tmp),
        "the disclosed probe root must be the one the run really allocated — it must \
         follow TMPDIR ({}), got {}",
        tmp.display(),
        root.display()
    );
    assert_eq!(
        disposition, "all removed",
        "the run must report its own probe root as removed:\n{stdout}"
    );

    // (3) That exact path is gone — asked directly, not inferred from a diff.
    assert!(
        !root.exists(),
        "the probe root this run announced must not survive it: {}",
        root.display()
    );

    // (4) …and NOTHING else survives in the namespace either. This is the half
    // that a rename cannot evade, and it covers the `--verify` lane's workspace
    // as well as the `--repair` probe root.
    let leaked: Vec<PathBuf> = std::fs::read_dir(&tmp)
        .expect("read the private TMPDIR")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    assert!(
        leaked.is_empty(),
        "the run's private TMPDIR must be empty afterwards — every scratch directory \
         the hybrid lane makes hangs off it; leaked: {leaked:?}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
