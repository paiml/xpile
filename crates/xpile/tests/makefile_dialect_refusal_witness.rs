//! XPILE-MAKEFILE-DIALECT-001 — `Makefile` / `*.mk` / `Dockerfile` were ROUTED
//! to a frontend that has no dialect for them (PMAT-1420).
//!
//! ## What this exists to catch
//!
//! `docs/specifications/sub/bashrs-merger.md` advertised a routing table row
//!
//! ```text
//! | `Makefile`, `*.mk` | `bashrs-frontend` (Makefile dialect) |
//! ```
//!
//! There is no Makefile dialect. There is no Dockerfile dialect either.
//! `BashrsFrontend::parse_and_lower` is a POSIX-shell LINE parser: it trims
//! every line, drops the blank ones, and lowers each survivor to a top-level
//! `Stmt::Cmd`. `matches_path` (PMAT-038) routed all three file kinds into it,
//! so a Makefile was lowered as though it were a shell script — at exit 0.
//!
//! Tab significance is the entire syntax of a Makefile and the frontend trims
//! it away. Target scoping is the entire SEMANTICS of a Makefile and lowering
//! every recipe line to top level discards it.
//!
//! ## The divergence, MEASURED (2026-07-28, both sides exit 0, `sh -n` clean)
//!
//! For a `Makefile` with `all:` (echo + touch), `clean:` (rm) and `test:`
//! recipes:
//!
//! | | `make` | the emitted `sh` |
//! |------------------|-------------------|----------------------------|
//! | exit status | 0 | 0 |
//! | `out.txt` after | **EXISTS** | **DELETED** |
//! | stdout | `building` | `building` + `running-tests` |
//!
//! The emit runs EVERY recipe unconditionally, in one shell, in file order —
//! including the `clean` target that `make` was never asked for, which deletes
//! the artifact `make` had just built. The target-name lines (`all:`) survive
//! as barewords and merely print `all:: not found` to stderr; the script still
//! exits 0 because its last command succeeds. Nothing downstream catches this:
//! `sh -n` parses it, the round-trip through bashrs-backend is a fixed point,
//! and `C-BASHRS-POSIX-IDEMPOTENCE` is cited on the output.
//!
//! `--target forjar` is the sharper end. It wrapped that same script in a
//! `type: file` resource at `/usr/local/bin/<name>.sh` mode `0755` plus a
//! `type: task` that RUNS it — a deployment lane materialising and executing a
//! script whose behaviour differs from the source it was generated from.
//!
//! Expansion diverges too, silently and in the dangerous direction: `$(RM) x`
//! is VARIABLE EXPANSION in make and COMMAND SUBSTITUTION in sh. (That
//! particular line happens to refuse today for an unrelated tokenizer reason —
//! "quoted program name" — which is exactly the vacuous-refusal trap this file
//! is careful about below. The bareword recipes above do not refuse, and
//! bareword recipes are the common case.)
//!
//! ## The fix, and why it is a REFUSAL
//!
//! PMAT-1371/1377 set the precedent for this frontend: a construct it cannot
//! model REFUSES rather than shredding into barewords. PMAT-1395 records why
//! coercion is the wrong instinct — making shredded output *run* is how a
//! silent wrong answer gets installed. So `parse_and_lower` now refuses the
//! three routed build-driver kinds, naming the missing dialect. Routing is
//! deliberately UNCHANGED, so the diagnostic can say "there is no Makefile
//! dialect" instead of degrading to "no frontend handles `.mk`".
//!
//! ## Over-refusal bound (PMAT-1419's lesson: this is the failure mode)
//!
//! Measured before cutting, not guessed: `git ls-files` tracks ZERO
//! `Makefile` / `Dockerfile` / `*.mk`, and no in-tree `parse_and_lower` call
//! site passes such a path. That bound is not a snapshot — invariant 1 of
//! `shell_artifact_policy_witness.rs` (XPILE-SHELLPOLICY-001) re-derives it
//! from `git ls-files` on every run, so this arm cannot regress a tracked
//! artifact today or after the next fixture lands.
//!
//! ## What this asserts
//!
//!   1. Every routed build driver refuses at the CLI, on BOTH backends that
//!      previously accepted it (`shell` and the deployment lane `forjar`),
//!      with a message naming the missing dialect.
//!   2. ANTI-VACUITY / ATTRIBUTION: the SAME BYTES at a `.sh` path still emit
//!      on both backends. Without this, a frontend that had merely stopped
//!      parsing these lines would satisfy assertion 1 just as well. The
//!      filename is the only discriminator.
//!   3. THE JUSTIFICATION, EXECUTED: `make` and the shredded shell disagree on
//!      the filesystem while both exit 0. The shredded script is produced by
//!      the LIVE emitter from the same bytes (via the `.sh` path of assertion
//!      2), not hand-reconstructed — `path` feeds nothing but `module_name`
//!      once past the guard, so that output is exactly what the `Makefile`
//!      path used to produce. This test measures that the LANGUAGES disagree,
//!      so it correctly stays GREEN when the guard is disabled.
//!   4. The spec row that advertised the dialect no longer does — with a
//!      present-row precondition, because a negative over a missing row passes
//!      for free (PMAT-1396).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Bareword recipes ONLY. No `$(VAR)`, no quotes, no here-doc — every line
/// lowers cleanly as shell, so a refusal on this corpus is attributable to the
/// dialect guard and to nothing else.
const MAKEFILE_SOURCE: &str = "\
all:\n\techo building\n\ttouch out.txt\n\nclean:\n\trm -f out.txt\n\ntest:\n\techo running-tests\n";

/// The routed build drivers, paired with the dialect the refusal must name.
const ROUTED_BUILD_DRIVERS: &[(&str, &str)] = &[
    ("Makefile", "Makefile"),
    ("rules.mk", "Makefile"),
    ("Dockerfile", "Dockerfile"),
];

/// Backends that ACCEPTED a Makefile before this slice. `forjar` is the one
/// that mattered most — it is the deployment lane.
const PREVIOUSLY_ACCEPTING_BACKENDS: &[&str] = &["shell", "forjar"];

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

fn tool_present(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A fresh directory per CALL — several of these run `make` and `sh` for
/// filesystem side effects, and a shared directory would let one call observe
/// another's `out.txt`.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("xpile-makefile-dialect-witness")
        .join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir scratch");
    dir
}

/// `xpile transpile <src> --target <backend>` → (success, stdout, stderr).
fn transpile(src: &Path, backend: &str) -> (bool, String, String) {
    let out = Command::new(xpile_bin())
        .args([
            "transpile",
            src.to_str().expect("utf-8 path"),
            "--target",
            backend,
        ])
        .output()
        .expect("spawn xpile");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// ASSERTION 1 — the refusal, at the CLI, on every backend that used to accept
/// it. The library-level refusal is pinned by `bashrs-frontend`'s own unit
/// tests; this exists because a library refusal the binary swallows is not a
/// refusal (PMAT-1391's call-site lesson).
#[test]
fn cli_refuses_every_routed_build_driver_on_every_previously_accepting_backend() {
    for (filename, dialect) in ROUTED_BUILD_DRIVERS {
        for backend in PREVIOUSLY_ACCEPTING_BACKENDS {
            let dir = scratch(&format!("refuse-{filename}-{backend}"));
            let src = dir.join(filename);
            std::fs::write(&src, MAKEFILE_SOURCE).expect("write source");

            let (ok, stdout, stderr) = transpile(&src, backend);
            assert!(
                !ok,
                "`{filename} --target {backend}` MUST refuse; it exited 0 emitting:\n{stdout}"
            );
            assert!(
                stderr.contains(&format!("no {dialect} dialect")),
                "the refusal must name the missing {dialect} dialect so the user knows what \
                 is unimplemented rather than what merely failed; got: {stderr}"
            );
        }
    }
}

/// ASSERTION 2 — ANTI-VACUITY. The same bytes under a shell filename still
/// emit, on both backends. This is what makes assertion 1 attributable to the
/// dialect guard: the corpus parses fine, the FILENAME is the discriminator.
#[test]
fn anti_vacuity_the_same_bytes_at_a_shell_path_still_emit_on_both_backends() {
    for backend in PREVIOUSLY_ACCEPTING_BACKENDS {
        let dir = scratch(&format!("control-{backend}"));
        let src = dir.join("recipe.sh");
        std::fs::write(&src, MAKEFILE_SOURCE).expect("write source");

        let (ok, stdout, stderr) = transpile(&src, backend);
        assert!(
            ok,
            "the refusal must be keyed on the build-driver FILENAME, not on this corpus \
             failing to parse — if `recipe.sh --target {backend}` errors, the refusal test \
             above is vacuous. stderr: {stderr}"
        );
        assert!(
            stdout.contains("echo building"),
            "the shell path must actually lower these recipe lines; got:\n{stdout}"
        );
    }
}

/// ASSERTION 3 — THE JUSTIFICATION, EXECUTED. `make` and the shredded shell
/// disagree on the filesystem while BOTH exit 0.
///
/// This measures a property of the two LANGUAGES, not of the guard, so it
/// stays green with the guard disabled — that is correct and deliberate: it is
/// the evidence the refusal is warranted, not a test of the refusal.
#[test]
fn make_and_the_shredded_shell_disagree_when_executed() {
    if !tool_present("make", &["--version"]) || !tool_present("sh", &["-c", "true"]) {
        // Skip LOUDLY. `XPILE_REQUIRE_SH` is the standing tripwire that turns
        // this into a hard failure where the toolchain is guaranteed.
        let msg = "SKIP make_and_the_shredded_shell_disagree_when_executed: `make` or `sh` \
                   not present";
        assert!(
            std::env::var("XPILE_REQUIRE_SH").is_err(),
            "{msg} — but XPILE_REQUIRE_SH is set, so this skip is a FAILURE"
        );
        eprintln!("{msg}");
        return;
    }

    // The shredded script, produced by the LIVE emitter rather than
    // reconstructed by hand: past the guard, `path` feeds nothing but
    // `module_name`, so emitting these bytes at a `.sh` path yields exactly
    // what the `Makefile` path used to yield.
    let gen = scratch("justify-emit");
    let sh_src = gen.join("recipe.sh");
    std::fs::write(&sh_src, MAKEFILE_SOURCE).expect("write source");
    let (ok, shredded, stderr) = transpile(&sh_src, "shell");
    assert!(ok, "control emit must succeed: {stderr}");

    // Side A: real `make`, default goal.
    let a = scratch("justify-make");
    std::fs::write(a.join("Makefile"), MAKEFILE_SOURCE).expect("write Makefile");
    let make_out = Command::new("make")
        .current_dir(&a)
        .output()
        .expect("spawn make");
    let make_artifact = a.join("out.txt").exists();

    // Side B: the shredded shell.
    let b = scratch("justify-sh");
    let script = b.join("emitted.sh");
    std::fs::write(&script, &shredded).expect("write emitted.sh");
    let syntax = Command::new("sh")
        .arg("-n")
        .arg(&script)
        .current_dir(&b)
        .output()
        .expect("spawn sh -n");
    let sh_out = Command::new("sh")
        .arg(&script)
        .current_dir(&b)
        .output()
        .expect("spawn sh");
    let sh_artifact = b.join("out.txt").exists();

    // Both sides succeed and the emit even PARSES — which is precisely why
    // nothing downstream noticed.
    assert!(
        make_out.status.success(),
        "make must succeed for the differential to mean anything: {}",
        String::from_utf8_lossy(&make_out.stderr)
    );
    assert!(syntax.status.success(), "the shredded emit `sh -n`-parses");
    assert!(sh_out.status.success(), "the shredded emit exits 0");

    assert!(
        make_artifact,
        "precondition: `make` builds out.txt via the `all` recipe"
    );
    assert!(
        !sh_artifact,
        "THE DIVERGENCE HAS CHANGED. The shredded shell used to run the `clean` recipe \
         unconditionally and delete out.txt. If it now preserves it, re-measure the \
         justification for this refusal before trusting this file's doc comment."
    );
    assert_ne!(
        make_artifact, sh_artifact,
        "make and the shredded shell must disagree on the filesystem — that disagreement, \
         at exit 0 on both sides, is why lowering a Makefile as shell refuses"
    );

    let sh_stdout = String::from_utf8_lossy(&sh_out.stdout);
    assert!(
        sh_stdout.contains("running-tests"),
        "the shredded shell also runs the `test` recipe make never requested; got: {sh_stdout}"
    );
}

/// ASSERTION 4 — the spec rows that advertised the dialects must present them
/// as REFUSED.
///
/// Two traps, both hit while writing this:
///
///   * A whole-file forbidden-substring check survives DELETING the row it
///     cares about (PMAT-1417) — so the row is LOCATED first and asserted on,
///     and a missing row is a hard failure rather than a free pass
///     (PMAT-1396).
///   * The first cut here forbade the substring `Makefile dialect` and RED on
///     the very sentence that fixes the claim ("no Makefile dialect exists").
///     A disclaimer necessarily quotes what it disclaims, so the requirement
///     is POSITIVE — the row must mark the kind refused — not a ban on words.
#[test]
fn bashrs_merger_spec_presents_build_drivers_as_refused() {
    let spec = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/specifications/sub/bashrs-merger.md")
        .canonicalize()
        .expect("bashrs-merger.md must exist");
    let text = std::fs::read_to_string(&spec).expect("read bashrs-merger.md");

    // Every file kind this frontend routes but refuses needs its row fixed,
    // derived from the same list the CLI assertion above uses.
    for (_, dialect) in ROUTED_BUILD_DRIVERS {
        let needle = format!("`{dialect}`");
        let row = text
            .lines()
            .find(|l| l.starts_with('|') && l.contains(&needle) && l.contains("`bashrs-frontend`"))
            .unwrap_or_else(|| {
                panic!(
                    "precondition: the routing-table row for {needle} must still exist in {} — \
                     without it this test asserts nothing about it",
                    spec.display()
                )
            });

        assert!(
            row.contains("REFUSED"),
            "the routing table must present {needle} as ROUTED-then-REFUSED — it has no \
             dialect, and a row that reads as handled is the claim this slice exists to \
             remove. Row: {row}"
        );
    }
}
