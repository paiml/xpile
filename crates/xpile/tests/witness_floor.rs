//! XPILE-WITNESS-002 — the executed-vs-skipped witness-floor MANIFEST.
//!
//! XPILE-WITNESS-001 turned a missing WASM runtime into a hard panic for the
//! *one* WASM lane. This manifest kills the silent-skip CLASS: it STATICALLY
//! counts the execution witnesses that back each differential lane and asserts
//! a per-lane FLOOR. Deleting or silencing a batch of witnesses drops the
//! count below the floor and fails here — regardless of whether the runtime
//! that would execute them is present. That makes the gate robust to the
//! runtime-skip question (a skipped witness still *exists* and still counts;
//! a DELETED witness does not), which is the exact CF-4 "skip-as-green"
//! signature the architectural review flagged
//! (`docs/specifications/fable-architectural-review.md`, XPILE-WITNESS-002).
//!
//! Lanes as `(source of the count) -> (live @ 2026-07-30, floor)`. Floors are
//! lower bounds re-derived at a sprint's TOUCH points, never per slice; each
//! floor is `<=` the live count, so this is green today and only goes RED on a
//! real regression. The `live` figures are a DATED SNAPSHOT, not an invariant —
//! adding witnesses raises the live count and can never red a floor:
//!
//! - wasm: `#[test]`s in `crates/xpile-wasm-codegen/tests/*_witness.rs` -> (857, 857)
//!   PLUS an EXECUTING-half floor (XPILE-WITNESS-004, below) -> (340 gated / 39%, 340 / 38%)
//! - shell: `#[test]`s in `tests/shell_diff_exec.rs` -> (10, 10)
//! - rust-differential: `tests/oracle_fixtures/*.py` + `FixtureCfg` rows in `tests/diff_exec.rs` -> (39+10=49, 49)
//! - hybrid: `#[test]`s in `tests/hybrid_verify{,_float,_multiarg}.rs` -> (7, 7)
//!   (`hybrid_verify_vacuity_witness.rs` is deliberately NOT in the sum — it is
//!   PMAT-1352's DIVERGENT-arm falsifier, which asserts the differential can go
//!   red, not that a fixture agrees; counting it would inflate the agreement lane)
//! - wasi: the `examples/proven-model/model.py` input the CI `wasi` job runs -> (1, 1)
//! - ruchy: `RUCHY_EXECUTABLE_FIXTURES` in `tests/ruchy_exec_witness.rs` -> (8, 8)
//!   (XPILE-WITNESS-003; the witness skips-with-reason when `ruchy` is absent, so
//!   this floor protects the curated set's SIZE from silent shrinkage)
//! - forjar: `FORJAR_SHELL_CORPUS` in `tests/forjar_validate_witness.rs` -> (4, 4)
//!   (XPILE-WITNESS-003; the witness runs forjar's real `validate` but skips-with-
//!   reason when `forjar` is absent, so this floor protects the corpus SIZE)
//! - lean: `LEAN_VALUE_CORPUS` in `tests/lean_elaborate_witness.rs` -> (6, 6)
//!   (XPILE-WITNESS-003; the witness elaborates + `by decide`-evaluates via `lean`
//!   but skips-with-reason when `lean` is absent, so this floor protects the
//!   corpus SIZE from silent shrinkage)
//!
//! GPU lanes (ptx/wgsl/spirv) run under `cargo test --workspace` but
//! SKIP-WITH-REASON on hosted runners (no CUDA toolchain / no Vulkan adapter).
//! The review requires they never go SILENTLY ABSENT: their `gpu_witness.rs`
//! must exist, carry a witness, gate on an availability probe, and print a
//! skip notice — asserted by `gpu_lanes_skip_with_reason_never_silently_absent`.
//!
//! XPILE-WITNESS-004 — floor the EXECUTING half of the WASM lane, not just the
//! total. The lane floor above counts `#[test]` attributes and nothing else, so
//! it is satisfiable by pure static WAT-string assertions: delete 100 executing
//! witnesses, add 100 emit-only ones, and the total-only floor stays green while
//! executing coverage silently halves. That is the identical "count went up, so
//! coverage went up" hole XPILE-WITNESS-002 closed one level up, on the lane 69
//! of v0.1.617's commits were built on. Three assertions close it:
//!
//!   1. `wasm_runtime_gated_witness_floor` — the runtime-gated COUNT has its own
//!      floor, so a deletion of executing witnesses reds even if emit-only tests
//!      replace them one-for-one.
//!   2. `wasm_runtime_gated_fraction_floor` — the gated PERCENTAGE has a floor.
//!      This is the assertion a total-only floor structurally cannot make: it
//!      reds when the corpus is PADDED with emit-only tests.
//!   3. `every_wasm_witness_file_gates_on_the_runtime` — every `*_witness.rs`
//!      carries at least one non-comment probe site (live 145/145 @ 2026-07-30,
//!      TOUCH 2; it was 138/138 at TOUCH 1). Strongest
//!      and cheapest: a brand-new purely-static witness FILE cannot be added
//!      without a deliberate, visible decision to change this test.
//!
//! HONEST DEFINITION OF "runtime-gated" — read this before citing the number.
//! It means "this `#[test]`'s own body names `wasm_runtime_available(` on a
//! non-comment line". It is a syntactic proxy that deliberately does NOT follow
//! helper calls (call-graph reachability would be neither cheap nor reviewable),
//! so a test that executes via a shared `run_case` helper counts as UNGATED.
//! The gated count is therefore a strict LOWER BOUND on the executing tests, not
//! an estimate of them — which is exactly what a floor needs, and why the live
//! 39% must never be quoted as "39% of WASM witnesses execute". A refactor that
//! hoists probes into helpers legitimately lowers this metric and will red the
//! fraction gate; that is a loud, correct prompt to re-derive at a TOUCH point,
//! not a defect.
//!
//! NO CEILING IS PLACED ON EMIT-ONLY TESTS, deliberately. They are refusal
//! witnesses, gate-tightness checks and static WAT assertions — `PMAT-1350`'s
//! `wasm_contract_surface.rs` DEPENDS on refusals being tested. A ceiling would
//! pressure deletion of the tests that pin the emitter's boundary. Floor the
//! executing count and the fraction; never cap the static half.
//!
//! All paths are derived from `CARGO_MANIFEST_DIR` (the `xpile` crate dir,
//! which Cargo sets for every `cargo test` invocation regardless of CWD), so
//! the walk is workspace-relative, never absolute-hardcoded.

use std::fs;
use std::path::{Path, PathBuf};

// ── Floors (lower bounds; live counts are a DATED SNAPSHOT, noted inline) ───
//
// RE-DERIVE DISCIPLINE (XPILE-WITNESS-002): a floor is bumped only at an
// explicit sprint TOUCH point, never opportunistically per slice — a slice that
// ADDS witnesses raises the live count and can never red a floor, so it has no
// reason to edit this file. Bumping per slice would serialize every capability
// PR on one file for no gate value. The `// live NNN @ DATE` comments are
// snapshots recorded at the last TOUCH, not invariants; they are expected to
// trail the true count between TOUCH points.
//
// 0.1.617 window, TOUCH 1 (PMAT-1344, 2026-07-26): WASM_FLOOR 400 -> 770. The old floor was set
// when the lane had 444 witnesses and was never re-derived across the ~69 WASM
// slices that landed for 0.1.617, leaving 395 tests of dead slack — ~50% of the
// corpus could have been deleted without reddening the REQUIRED `workspace-test`
// context, which is the exact anti-deletion guarantee this manifest exists to
// provide. 770 leaves 25 of headroom for in-flight churn. Every OTHER lane was
// already tight at TOUCH 1 and is deliberately left alone.
// 0.1.618 window, TOUCH 1 (PMAT-1372, 2026-07-26): no floor above was bumped —
// the EXECUTING-half floors below were ADDED, so this touch cannot red another
// in-flight branch on rebase. TOUCH 2 is the Thursday release re-derive
// (PMAT-1373), which re-derives every lane INCLUDING the two new ones.
//
// 0.1.618 window, TOUCH 2 (PMAT-1373, 2026-07-30) — the release re-derive, and
// the LAST permitted touch of this file this sprint. Every lane is re-derived
// from a live `cargo test -p xpile --test witness_floor -- --nocapture` run on
// the tagged tree, and every COUNT floor is set TIGHT (floor == live).
//
// Why tight, when TOUCH 1 deliberately left 25 of WASM headroom "for in-flight
// churn": there is no in-flight churn left to protect. `gh pr list --state
// open` returns `[]` at this touch, the Wednesday 18:00 HARD FREEZE forbids any
// further `crates/*/src` merge this window, and Friday is publish-only. A floor
// exists to make a batch DELETION red; slack is the cost paid for rebase safety
// and there is nothing to rebase. A 0.1.619 slice that ADDS witnesses raises
// live and can never red a tight floor, so the cost of tight is borne only by a
// slice that removes or consolidates witnesses — which is precisely the event
// this manifest exists to surface, and the re-derive discipline below says how
// to answer it (a TOUCH, not an opportunistic edit).
//
// The ONE deliberate exception is WASM_EXEC_PCT_FLOOR, because it is a RATIO and
// not a count: adding an emit-only refusal witness raises the denominator while
// the numerator holds, so a tight fraction floor would red on exactly the static
// tests the header above refuses to cap. It keeps one point of margin.
//
//   lane               TOUCH 1        TOUCH 2 (live @ 2026-07-30)
//   wasm (total)       770  (795)     857  (857)   tight
//   wasm (gated)       300  (315)     340  (340)   tight
//   wasm (gated %)      36   (39)      38   (39)   1 point of margin — a RATIO
//   shell                7    (7)      10   (10)   tight
//   rust-differential   44   (44)      49   (49)   tight (39 oracle + 10 diff_exec)
//   hybrid               3    (3)       7    (7)   tight
//   wasi                 1    (1)       1    (1)   tight
//   ruchy                7    (7)       8    (8)   tight
//   forjar               4    (4)       4    (4)   tight (unchanged)
//   lean                 6    (6)       6    (6)   tight (unchanged)
const WASM_FLOOR: usize = 857; // live 857 @ 2026-07-30 (TOUCH 2, tight)

// The probe a WASM witness calls to decide whether it can execute the emitted
// module. Kept as one named constant so widening the notion of "executes" later
// is a one-line, reviewable act rather than a loosened grep.
const RUNTIME_PROBE: &str = "wasm_runtime_available(";
// XPILE-WITNESS-004 executing-half floors. `live 315 / 39%` is a DATED SNAPSHOT
// (2026-07-26, TOUCH 1) of a metric that is a LOWER BOUND on executing tests —
// see the module header before quoting either figure anywhere.
const WASM_EXEC_FLOOR: usize = 340; // live 340 @ 2026-07-30 (TOUCH 2, tight)
const WASM_EXEC_PCT_FLOOR: usize = 38; // live 39% @ 2026-07-30 (TOUCH 2; 1 pt of margin — a RATIO)
const SHELL_FLOOR: usize = 10; // live 10 @ 2026-07-30 (TOUCH 2, tight)
const RUST_DIFF_FLOOR: usize = 49; // live 49 @ 2026-07-30 (39 oracle + 10 diff_exec; TOUCH 2, tight)
const HYBRID_FLOOR: usize = 7; // live 7 @ 2026-07-30 (TOUCH 2, tight)
const WASI_FLOOR: usize = 1; // live 1 @ 2026-07-30 (TOUCH 2, tight)
const RUCHY_FLOOR: usize = 8; // live 8 @ 2026-07-30 (XPILE-WITNESS-003 curated executing set; TOUCH 2, tight)
const FORJAR_FLOOR: usize = 4; // live 4 @ 2026-07-30 (XPILE-WITNESS-003 validator-accepted shell corpus; TOUCH 2, tight)
const LEAN_FLOOR: usize = 6; // live 6 @ 2026-07-30 (XPILE-WITNESS-003 semantic value-function corpus; TOUCH 2, tight)

// ── Path helpers ────────────────────────────────────────────────────────────
fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Repo root = two levels up from `crates/xpile`.
fn repo_root() -> PathBuf {
    crate_dir().join("..").join("..")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("witness-floor: cannot read {}: {e}", path.display()))
}

// ── Counting primitives ─────────────────────────────────────────────────────
/// Count lines that, once trimmed, are EXACTLY `#[test]`. The exact match (not
/// a substring scan) means a `#[test]` mentioned inside a doc comment or a
/// string literal is not counted — only real attributes.
fn count_test_attrs(src: &str) -> usize {
    src.lines().filter(|l| l.trim() == "#[test]").count()
}

/// Count lines containing `needle` (structural markers that occur once per
/// record, e.g. a `FixtureCfg`'s `file: "` field).
fn count_lines_containing(src: &str, needle: &str) -> usize {
    src.lines().filter(|l| l.contains(needle)).count()
}

/// True when `line` is Rust code rather than a line comment (`//`, `///`,
/// `//!`). Block comments are not modelled: none of the witness files use one
/// around a probe call, and a syntactic proxy that over-counts a commented-out
/// probe would only ever make a floor EASIER to satisfy, which is the direction
/// this gate must not fail in silently. Treating `//`-prefixed lines as
/// non-code is what stops a doc comment that merely NAMES the probe from
/// scoring a gate.
fn is_code_line(line: &str) -> bool {
    !line.trim_start().starts_with("//")
}

/// Count non-comment occurrences of `needle`.
fn count_code_sites(src: &str, needle: &str) -> usize {
    src.lines()
        .filter(|l| is_code_line(l) && l.contains(needle))
        .count()
}

/// Count the `#[test]`s in `src` whose OWN body names `needle` on a non-comment
/// line. A test's body runs from its `#[test]` attribute to the next `#[test]`
/// or EOF, and each test is credited at most once.
///
/// Deliberately does NOT follow helper calls — see the module header. A test
/// that gates via a shared helper counts as ungated, so the result is a strict
/// LOWER BOUND on the tests that actually execute.
fn count_tests_referencing(src: &str, needle: &str) -> usize {
    let mut gated = 0usize;
    let mut in_test = false;
    let mut credited = false;
    for line in src.lines() {
        if line.trim() == "#[test]" {
            in_test = true;
            credited = false;
            continue;
        }
        if in_test && !credited && is_code_line(line) && line.contains(needle) {
            gated += 1;
            credited = true;
        }
    }
    gated
}

/// Every `*_witness.rs` in `dir`, sorted by file name so failure messages and
/// manifest output are deterministic across filesystems.
fn witness_files_in_dir(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("witness-floor: cannot read dir {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with("_witness.rs"))
        })
        .collect();
    files.sort();
    files
}

/// Sum `#[test]` attributes across every `*_witness.rs` file in `dir`.
fn count_witness_tests_in_dir(dir: &Path) -> usize {
    witness_files_in_dir(dir)
        .iter()
        .map(|p| count_test_attrs(&read(p)))
        .sum()
}

/// Count `*.py` files in `dir`.
fn count_py_files(dir: &Path) -> usize {
    fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("witness-floor: cannot read dir {}: {e}", dir.display()))
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("py"))
        .count()
}

// ── Per-lane counts ─────────────────────────────────────────────────────────
fn wasm_witness_dir() -> PathBuf {
    crate_dir().join("../xpile-wasm-codegen/tests")
}

fn wasm_witness_count() -> usize {
    count_witness_tests_in_dir(&wasm_witness_dir())
}

/// `(total #[test]s, tests whose own body gates on the runtime probe)` over the
/// WASM witness corpus. The second figure is a LOWER BOUND on executing tests.
fn wasm_witness_split() -> (usize, usize) {
    let mut total = 0usize;
    let mut gated = 0usize;
    for path in witness_files_in_dir(&wasm_witness_dir()) {
        let src = read(&path);
        total += count_test_attrs(&src);
        gated += count_tests_referencing(&src, RUNTIME_PROBE);
    }
    (total, gated)
}

/// `*_witness.rs` files carrying ZERO non-comment probe sites — i.e. files in
/// which nothing can execute at all. Live: empty (138/138 gate).
fn wasm_files_without_a_guard() -> Vec<String> {
    witness_files_in_dir(&wasm_witness_dir())
        .into_iter()
        .filter(|p| count_code_sites(&read(p), RUNTIME_PROBE) == 0)
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(String::from))
        .collect()
}

fn shell_witness_count() -> usize {
    count_test_attrs(&read(&crate_dir().join("tests/shell_diff_exec.rs")))
}

fn rust_differential_witness_count() -> usize {
    let oracle = count_py_files(&crate_dir().join("tests/oracle_fixtures"));
    let diff_exec =
        count_lines_containing(&read(&crate_dir().join("tests/diff_exec.rs")), "file: \"");
    oracle + diff_exec
}

fn hybrid_witness_count() -> usize {
    [
        "tests/hybrid_verify.rs",
        "tests/hybrid_verify_float.rs",
        "tests/hybrid_verify_multiarg.rs",
    ]
    .iter()
    .map(|rel| count_test_attrs(&read(&crate_dir().join(rel))))
    .sum()
}

fn wasi_witness_count() -> usize {
    // The single wasi execution witness input: the proven model the CI `wasi`
    // job emits -> wasm32-wasip1 -> wasmtime and diffs vs CPython.
    usize::from(repo_root().join("examples/proven-model/model.py").is_file())
}

/// Curated fixtures the Ruchy execution witness (XPILE-WITNESS-003) drives
/// through `ruchy transpile` -> rustc -> run and byte-diffs vs CPython. Counts
/// the entries of `RUCHY_EXECUTABLE_FIXTURES` so the curated set cannot silently
/// shrink (the witness skips-with-reason when `ruchy` is absent, so its EXISTENCE
/// and SIZE are what this manifest protects — the anti-silent-deletion guarantee).
fn ruchy_witness_count() -> usize {
    let src = read(&crate_dir().join("tests/ruchy_exec_witness.rs"));
    let mut in_list = false;
    let mut n = 0;
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with("const RUCHY_EXECUTABLE_FIXTURES") {
            in_list = true;
            continue;
        }
        if !in_list {
            continue;
        }
        if t.starts_with("];") {
            break;
        }
        if t.starts_with('"') {
            n += 1;
        }
    }
    n
}

/// Count the `FORJAR_SHELL_CORPUS` entries in `tests/forjar_validate_witness.rs`.
/// Each entry is a `("<name>", "<shell>")` tuple; the witness emits forjar.yaml
/// for each and runs forjar's real `validate` (skips-with-reason when `forjar`
/// is absent, so its EXISTENCE and SIZE are what this manifest protects).
fn forjar_witness_count() -> usize {
    let src = read(&crate_dir().join("tests/forjar_validate_witness.rs"));
    let mut in_list = false;
    let mut n = 0;
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with("const FORJAR_SHELL_CORPUS") {
            in_list = true;
            continue;
        }
        if !in_list {
            continue;
        }
        if t.starts_with("];") {
            break;
        }
        if t.starts_with("(\"") {
            n += 1;
        }
    }
    n
}

/// Count the `LEAN_VALUE_CORPUS` entries in `tests/lean_elaborate_witness.rs`.
/// Each entry is a `LeanCase { .. }` value-function case that the witness emits
/// as Lean and elaborates + `by decide`-evaluates via `lean` (skips-with-reason
/// when `lean` is absent, so its SIZE is what this manifest protects).
fn lean_witness_count() -> usize {
    let src = read(&crate_dir().join("tests/lean_elaborate_witness.rs"));
    let mut in_list = false;
    let mut n = 0;
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with("const LEAN_VALUE_CORPUS") {
            in_list = true;
            continue;
        }
        if !in_list {
            continue;
        }
        if t.starts_with("];") {
            break;
        }
        if t.starts_with("LeanCase {") {
            n += 1;
        }
    }
    n
}

// ── Per-lane floor gates ────────────────────────────────────────────────────
#[test]
fn wasm_witness_floor() {
    let n = wasm_witness_count();
    eprintln!("witness-manifest[wasm]: {n} executed witnesses (floor {WASM_FLOOR})");
    assert!(
        n >= WASM_FLOOR,
        "wasm witness floor breached: {n} < {WASM_FLOOR} — a batch of WASM \
         execution witnesses was deleted or silenced (XPILE-WITNESS-002)"
    );
}

// ── XPILE-WITNESS-004: floor the EXECUTING half of the WASM lane ────────────
#[test]
fn wasm_runtime_gated_witness_floor() {
    let (total, gated) = wasm_witness_split();
    eprintln!(
        "witness-manifest[wasm]: {total} total / {gated} runtime-gated \
         (floor {WASM_EXEC_FLOOR})"
    );
    assert!(
        gated >= WASM_EXEC_FLOOR,
        "wasm RUNTIME-GATED witness floor breached: {gated} < {WASM_EXEC_FLOOR} \
         (of {total} total). Executing witnesses were deleted or their runtime \
         guard was removed. NOTE the total-only floor ({WASM_FLOOR}) can stay \
         GREEN through this: swapping executing witnesses for emit-only ones \
         one-for-one leaves the total untouched while executing coverage drops. \
         That swap is precisely what this assertion exists to catch \
         (XPILE-WITNESS-004)"
    );
}

#[test]
fn wasm_runtime_gated_fraction_floor() {
    let (total, gated) = wasm_witness_split();
    assert!(
        total > 0,
        "wasm witness corpus is empty (XPILE-WITNESS-004)"
    );
    let pct = gated * 100 / total;
    eprintln!(
        "witness-manifest[wasm]: {pct}% runtime-gated ({gated}/{total}, \
         floor {WASM_EXEC_PCT_FLOOR}%)"
    );
    assert!(
        pct >= WASM_EXEC_PCT_FLOOR,
        "wasm runtime-gated FRACTION breached: {pct}% < {WASM_EXEC_PCT_FLOOR}% \
         ({gated}/{total}). The corpus was padded with emit-only tests — a \
         total-only floor structurally cannot see this, because padding only \
         ever raises the total. Either add executing witnesses alongside the \
         static ones, or (if probes were legitimately hoisted into shared \
         helpers) re-derive this floor at an explicit TOUCH point and say so in \
         the commit (XPILE-WITNESS-004)"
    );
}

#[test]
fn every_wasm_witness_file_gates_on_the_runtime() {
    let unguarded = wasm_files_without_a_guard();
    let files = witness_files_in_dir(&wasm_witness_dir()).len();
    eprintln!(
        "witness-manifest[wasm]: {}/{files} witness files carry a runtime guard",
        files - unguarded.len()
    );
    assert!(
        unguarded.is_empty(),
        "these WASM witness files contain NO non-comment `{RUNTIME_PROBE}` site, \
         so nothing in them can ever execute — they assert on emitted WAT text \
         only: {unguarded:?}. Adding a purely-static witness file must be a \
         deliberate decision that edits this test, not a silent dilution of the \
         lane's execution evidence (XPILE-WITNESS-004)"
    );
}

#[test]
fn shell_witness_floor() {
    let n = shell_witness_count();
    eprintln!("witness-manifest[shell]: {n} executed witnesses (floor {SHELL_FLOOR})");
    assert!(
        n >= SHELL_FLOOR,
        "shell witness floor breached: {n} < {SHELL_FLOOR} (XPILE-WITNESS-002)"
    );
}

#[test]
fn rust_differential_witness_floor() {
    let n = rust_differential_witness_count();
    eprintln!(
        "witness-manifest[rust-differential]: {n} executed witnesses (floor {RUST_DIFF_FLOOR})"
    );
    assert!(
        n >= RUST_DIFF_FLOOR,
        "rust-differential witness floor breached: {n} < {RUST_DIFF_FLOOR} — \
         oracle fixtures and/or diff_exec FixtureCfg rows were deleted \
         (XPILE-WITNESS-002)"
    );
}

#[test]
fn hybrid_witness_floor() {
    let n = hybrid_witness_count();
    eprintln!("witness-manifest[hybrid]: {n} executed witnesses (floor {HYBRID_FLOOR})");
    assert!(
        n >= HYBRID_FLOOR,
        "hybrid witness floor breached: {n} < {HYBRID_FLOOR} (XPILE-WITNESS-002)"
    );
}

#[test]
fn wasi_witness_floor() {
    let n = wasi_witness_count();
    eprintln!("witness-manifest[wasi]: {n} execution witness input (floor {WASI_FLOOR})");
    assert!(
        n >= WASI_FLOOR,
        "wasi witness floor breached: {n} < {WASI_FLOOR} — \
         examples/proven-model/model.py missing (XPILE-WITNESS-002)"
    );
    // The wasi lane is wired into hosted CI via the wasm32-wasip1 target; if
    // the whole lane is dropped, this marker disappears from ci.yml.
    let ci = read(&repo_root().join(".github/workflows/ci.yml"));
    assert!(
        ci.contains("wasm32-wasip1"),
        "the wasi CI lane (wasm32-wasip1 -> wasmtime) vanished from ci.yml \
         (XPILE-WITNESS-002)"
    );
}

#[test]
fn ruchy_witness_floor() {
    let n = ruchy_witness_count();
    eprintln!("witness-manifest[ruchy]: {n} curated executing fixtures (floor {RUCHY_FLOOR})");
    assert!(
        n >= RUCHY_FLOOR,
        "ruchy witness floor breached: {n} < {RUCHY_FLOOR} — the Ruchy execution \
         witness's curated fixture set shrank (XPILE-WITNESS-002/003)"
    );
}

#[test]
fn forjar_witness_floor() {
    let n = forjar_witness_count();
    eprintln!(
        "witness-manifest[forjar]: {n} validator-accepted shell inputs (floor {FORJAR_FLOOR})"
    );
    assert!(
        n >= FORJAR_FLOOR,
        "forjar witness floor breached: {n} < {FORJAR_FLOOR} — the forjar \
         validation witness's shell corpus shrank (XPILE-WITNESS-002/003)"
    );
}

#[test]
fn lean_witness_floor() {
    let n = lean_witness_count();
    eprintln!("witness-manifest[lean]: {n} semantic value-function cases (floor {LEAN_FLOOR})");
    assert!(
        n >= LEAN_FLOOR,
        "lean witness floor breached: {n} < {LEAN_FLOOR} — the Lean elaboration \
         witness's value-function corpus shrank (XPILE-WITNESS-002/003)"
    );
}

// ── GPU lanes: skipped-with-reason, never silently absent ───────────────────
#[test]
fn gpu_lanes_skip_with_reason_never_silently_absent() {
    let lanes = [
        ("ptx", "../xpile-ptx-codegen/tests/gpu_witness.rs"),
        ("wgsl", "../xpile-wgsl-codegen/tests/gpu_witness.rs"),
        ("spirv", "../xpile-spirv-codegen/tests/gpu_witness.rs"),
    ];
    for (lane, rel) in lanes {
        let path = crate_dir().join(rel);
        assert!(
            path.is_file(),
            "GPU lane {lane} silently absent: {} missing (XPILE-WITNESS-002)",
            path.display()
        );
        let src = read(&path);
        let tests = count_test_attrs(&src);
        let has_guard = src.contains("available()");
        let has_skip_notice = src.contains("skipping");
        eprintln!(
            "witness-manifest[gpu/{lane}]: {tests} witness(es), guard={has_guard}, skip_notice={has_skip_notice}"
        );
        assert!(
            tests >= 1,
            "GPU lane {lane}: no #[test] witness present (XPILE-WITNESS-002)"
        );
        assert!(
            has_guard,
            "GPU lane {lane}: no availability guard — a witness that runs \
             unconditionally can't skip-with-reason (XPILE-WITNESS-002)"
        );
        assert!(
            has_skip_notice,
            "GPU lane {lane}: no skip-with-reason notice string (XPILE-WITNESS-002)"
        );
    }
}

// ── Aggregate manifest emitter (also asserts the grand-total floor) ─────────
#[test]
fn witness_floor_manifest_emitted() {
    let wasm = wasm_witness_count();
    let shell = shell_witness_count();
    let rustd = rust_differential_witness_count();
    let hybrid = hybrid_witness_count();
    let wasi = wasi_witness_count();
    let ruchy = ruchy_witness_count();
    let forjar = forjar_witness_count();
    let lean = lean_witness_count();
    let total = wasm + shell + rustd + hybrid + wasi + ruchy + forjar + lean;
    let floor_total = WASM_FLOOR
        + SHELL_FLOOR
        + RUST_DIFF_FLOOR
        + HYBRID_FLOOR
        + WASI_FLOOR
        + RUCHY_FLOOR
        + FORJAR_FLOOR
        + LEAN_FLOOR;

    let (wasm_total, wasm_gated) = wasm_witness_split();
    let wasm_pct = if wasm_total > 0 {
        wasm_gated * 100 / wasm_total
    } else {
        0
    };

    eprintln!("== XPILE-WITNESS-002 witness-floor manifest ==");
    eprintln!("  lane                executed  floor");
    eprintln!("  wasm                {wasm:>8}  {WASM_FLOOR}");
    // XPILE-WITNESS-004: the split retires the bare "N witnesses" figure, which
    // has already produced two provably-wrong numbers in this repo's docs. The
    // gated half is a LOWER BOUND on executing tests, never a coverage claim.
    eprintln!(
        "  wasm  {wasm_total} total / {wasm_gated} runtime-gated \
         (floor {WASM_EXEC_FLOOR}, {wasm_pct}% vs floor {WASM_EXEC_PCT_FLOOR}%)"
    );
    eprintln!("  shell               {shell:>8}  {SHELL_FLOOR}");
    eprintln!("  rust-differential   {rustd:>8}  {RUST_DIFF_FLOOR}");
    eprintln!("  hybrid              {hybrid:>8}  {HYBRID_FLOOR}");
    eprintln!("  wasi                {wasi:>8}  {WASI_FLOOR}");
    eprintln!("  ruchy               {ruchy:>8}  {RUCHY_FLOOR}");
    eprintln!("  forjar              {forjar:>8}  {FORJAR_FLOOR}");
    eprintln!("  lean                {lean:>8}  {LEAN_FLOOR}");
    eprintln!("  TOTAL               {total:>8}  {floor_total}");

    assert!(
        total >= floor_total,
        "aggregate witness floor breached: {total} < {floor_total} (XPILE-WITNESS-002)"
    );
}

// ── Anti-vacuity: the counting primitives themselves ────────────────────────
// Without these, XPILE-WITNESS-004 could pass because `count_tests_referencing`
// returns a large number for the WRONG reason (e.g. crediting doc comments, or
// crediting a test for a probe that belongs to the NEXT test).
mod counting_primitives {
    use super::{count_code_sites, count_test_attrs, count_tests_referencing, RUNTIME_PROBE};

    #[test]
    fn a_probe_named_only_in_a_comment_scores_nothing() {
        let src = "\
//! Gated on `wasm_runtime_available()` — prose, not code.
#[test]
fn t() {
    // if !wasm_runtime_available() { return; }
    assert!(wat.contains(\"i64.add\"));
}
";
        assert_eq!(count_test_attrs(src), 1);
        assert_eq!(count_code_sites(src, RUNTIME_PROBE), 0);
        assert_eq!(count_tests_referencing(src, RUNTIME_PROBE), 0);
    }

    #[test]
    fn an_import_line_is_not_a_probe_site() {
        // The `use` line names the probe but does not CALL it; the trailing
        // `(` in the needle is what keeps it from scoring.
        let src = "use xpile_wasm_codegen::{emit_module, wasm_runtime_available};\n";
        assert_eq!(count_code_sites(src, RUNTIME_PROBE), 0);
    }

    #[test]
    fn each_test_is_credited_at_most_once_and_only_for_its_own_body() {
        let src = "\
#[test]
fn emit_only() {
    assert!(wat.contains(\"i64.add\"));
}

#[test]
fn executes() {
    if !wasm_runtime_available() {
        return;
    }
    if !wasm_runtime_available() {
        return;
    }
    assert_eq!(run(), 7);
}
";
        assert_eq!(count_test_attrs(src), 2);
        // Two call sites, but ONE gated test — and the emit-only test that
        // PRECEDES it must not be credited for a probe below it.
        assert_eq!(count_code_sites(src, RUNTIME_PROBE), 2);
        assert_eq!(count_tests_referencing(src, RUNTIME_PROBE), 1);
    }

    #[test]
    fn a_helper_gated_test_counts_as_ungated_lower_bound() {
        // The documented false-negative, pinned so nobody "fixes" it into
        // call-graph reachability without reading the module header.
        let src = "\
fn run_case(py: &str) {
    if !wasm_runtime_available() {
        return;
    }
}

#[test]
fn t() {
    run_case(\"x = 1\");
}
";
        assert_eq!(count_code_sites(src, RUNTIME_PROBE), 1);
        assert_eq!(count_tests_referencing(src, RUNTIME_PROBE), 0);
    }
}
