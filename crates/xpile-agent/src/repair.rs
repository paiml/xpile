//! PMAT-908 (Sprint Day 9 — north-star Phase 6): a bounded, **fail-closed,
//! deterministic** repair loop over the hybrid artifact.
//!
//! This is the first *executing* increment of `xpile-agent`: it consumes a
//! structured [`Symptom`] (a `cargo build`/`rustc` diagnostic or an oracle
//! [`Symptom::Divergence`]), applies **one deterministic repair rule**, and
//! re-probes — iterating to [`RepairOutcome::Repaired`] or budget exhaustion.
//! There is **no LLM**: every rule is a pure, testable source transform.
//!
//! ## Fail-closed by construction
//!
//! The loop never writes Rust to disk. [`RepairLoop::run`] returns the repaired
//! source only inside the [`RepairOutcome::Repaired`]/[`RepairOutcome::AlreadyMatching`]
//! variants; [`RepairOutcome::Exhausted`] has **no `source` field**, so a caller
//! *cannot* commit a half-repaired artifact even by mistake. [`RepairLoop::run_and_commit`]
//! makes that the only write path: it writes iff the loop reached a matching
//! candidate, leaving the destination untouched on exhaustion.
//!
//! ## The repair domain (this slice)
//!
//! The real failure class is an emitted C-FFI shim that is **missing its ABI
//! casts**. The correct [`xpile_ffi_manifest`]-emitted wrapper marshals across
//! the C ABI with `x as ::std::os::raw::c_int` on the argument and `__r as i64`
//! on the return; a shim that drops those casts fails to compile with `E0308`
//! mismatched-types. Two narrow rules — [`FfiArgCastRepair`] and
//! [`FfiReturnCastRepair`] — read that diagnostic and re-insert the casts, one
//! per iteration, converging the loop in two steps. Each rule is idempotent (it
//! will not re-fire once its cast is present), which is what makes the loop
//! *terminate*: when no rule applies, the loop fails closed.

use crate::{AgentError, Budget};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// A structured observation the repair loop consumes, so a rule can pattern-match
/// on it deterministically. Either a build diagnostic or a semantic divergence
/// from the oracle reference.
#[derive(Debug, Clone)]
pub enum Symptom {
    /// `cargo build` / `rustc` failed; `stderr` is the compiler's diagnostics.
    BuildError { stderr: String },
    /// The artifact built and ran but its output diverged from the oracle
    /// reference (the Day-3 CPython hybrid reference). `index` pins the first
    /// differing line.
    Divergence {
        index: usize,
        expected: String,
        actual: String,
    },
}

impl Symptom {
    /// Does this symptom look like a Rust type-mismatch (`E0308`)? The ABI-cast
    /// rules only fire on this class — never on, say, an unrelated divergence.
    fn is_type_mismatch(&self) -> bool {
        matches!(self, Symptom::BuildError { stderr }
            if stderr.contains("E0308") || stderr.contains("mismatched types"))
    }
}

/// One deterministic repair rule. Given the current [`Symptom`] and candidate
/// source, return a repaired candidate if the rule applies, else `None`. A rule
/// MUST be idempotent — returning `None` once its repair is already present — so
/// the loop terminates instead of re-firing forever.
pub trait RepairRule: Send + Sync {
    fn name(&self) -> &'static str;
    fn apply(&self, symptom: &Symptom, candidate: &str) -> Option<String>;
}

/// Evaluates a candidate Rust source: `Ok(())` on a *match* against the oracle
/// reference, else the [`Symptom`] describing why (build failure or divergence).
/// Abstracted so the loop logic is unit-testable with a pure in-memory probe and
/// the end-to-end path can drive the real `cc` + `rustc` + run + differential.
pub trait Probe {
    fn evaluate(&self, candidate: &str) -> Result<(), Symptom>;
}

/// The terminal state of a [`RepairLoop`] run. Note that **only** the matching
/// variants carry a `source`: exhaustion is structurally incapable of handing
/// back a half-repaired artifact (the fail-closed invariant).
#[derive(Debug)]
pub enum RepairOutcome {
    /// The initial candidate already matched — no repair was needed.
    AlreadyMatching { source: String },
    /// Reached a matching candidate after `iterations` repair steps.
    Repaired { iterations: u32, source: String },
    /// Budget exhausted (iterations or wall-clock) or no rule applied, without
    /// reaching a match. Carries the last symptom for diagnostics — but **no
    /// source**, so nothing partial can be committed.
    Exhausted { iterations: u32, last: Symptom },
}

impl RepairOutcome {
    /// The committable source, if the run reached a match; `None` on exhaustion.
    pub fn source(&self) -> Option<&str> {
        match self {
            RepairOutcome::AlreadyMatching { source } | RepairOutcome::Repaired { source, .. } => {
                Some(source)
            }
            RepairOutcome::Exhausted { .. } => None,
        }
    }

    /// Did the loop reach a match (already-matching or repaired)?
    pub fn is_match(&self) -> bool {
        !matches!(self, RepairOutcome::Exhausted { .. })
    }
}

/// A bounded, deterministic repair loop. Holds the [`Budget`] (iteration +
/// wall-clock ceiling) and an ordered list of [`RepairRule`]s tried first-match.
pub struct RepairLoop {
    pub budget: Budget,
    pub rules: Vec<Box<dyn RepairRule>>,
}

impl RepairLoop {
    /// Construct a loop with the given budget and rules.
    pub fn new(budget: Budget, rules: Vec<Box<dyn RepairRule>>) -> Self {
        Self { budget, rules }
    }

    /// The default repair loop for a single-scalar C-FFI `int(int)` boundary:
    /// the two ABI-cast rules over `symbol`, using `c_int` as the C ABI type and
    /// `i64` as the native wrapper type (the decy / `hybrid_sum` shape). This
    /// mirrors [`xpile_ffi_manifest`]'s `c_abi_type`/`wrapper_native` mapping.
    pub fn ffi_int_boundary(budget: Budget, symbol: impl Into<String>) -> Self {
        let symbol = symbol.into();
        Self::new(
            budget,
            vec![
                Box::new(FfiArgCastRepair {
                    symbol: symbol.clone(),
                    abi: "::std::os::raw::c_int".to_string(),
                }),
                Box::new(FfiReturnCastRepair {
                    native: "i64".to_string(),
                }),
            ],
        )
    }

    /// Drive the loop to a terminal [`RepairOutcome`]. Probes the initial
    /// candidate; on a symptom, applies the first matching rule and re-probes,
    /// bounded by `budget.max_iterations` and `budget.max_wall_clock`. Writes
    /// nothing — see [`RepairLoop::run_and_commit`] for the disciplined write.
    pub fn run<P: Probe + ?Sized>(&self, probe: &P, initial: &str) -> RepairOutcome {
        let started = Instant::now();
        let mut candidate = initial.to_string();
        let mut last = match probe.evaluate(&candidate) {
            Ok(()) => return RepairOutcome::AlreadyMatching { source: candidate },
            Err(sym) => sym,
        };

        for iter in 1..=self.budget.max_iterations {
            if started.elapsed() >= self.budget.max_wall_clock {
                return RepairOutcome::Exhausted {
                    iterations: iter - 1,
                    last,
                };
            }
            // First rule that applies wins. If none applies, fail closed: there
            // is no deterministic move left, so we stop rather than guess.
            let Some(next) = self
                .rules
                .iter()
                .find_map(|rule| rule.apply(&last, &candidate))
            else {
                return RepairOutcome::Exhausted {
                    iterations: iter - 1,
                    last,
                };
            };
            candidate = next;
            match probe.evaluate(&candidate) {
                Ok(()) => {
                    return RepairOutcome::Repaired {
                        iterations: iter,
                        source: candidate,
                    }
                }
                Err(sym) => last = sym,
            }
        }
        RepairOutcome::Exhausted {
            iterations: self.budget.max_iterations,
            last,
        }
    }

    /// Run the loop and write the repaired source to `out` **iff** the loop
    /// reached a match. On [`RepairOutcome::Exhausted`] the destination is left
    /// untouched — the fail-closed contract: never write partial Rust.
    pub fn run_and_commit<P: Probe + ?Sized>(
        &self,
        probe: &P,
        initial: &str,
        out: &Path,
    ) -> Result<RepairOutcome, AgentError> {
        let outcome = self.run(probe, initial);
        if let Some(source) = outcome.source() {
            std::fs::write(out, source)
                .map_err(|e| AgentError::Io(format!("writing repaired artifact: {e}")))?;
        }
        Ok(outcome)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Deterministic repair rules — pure source transforms over the FFI boundary.
// ─────────────────────────────────────────────────────────────────────────────

/// Insert the missing ABI cast on the *argument* of a C-FFI call. Fires on an
/// `E0308` symptom; rewrites the call `symbol(arg)` → `symbol(arg as <abi>)`.
/// Targets the call site, never the `extern` declaration, and is idempotent.
pub struct FfiArgCastRepair {
    pub symbol: String,
    pub abi: String,
}

impl RepairRule for FfiArgCastRepair {
    fn name(&self) -> &'static str {
        "ffi-arg-cast"
    }

    fn apply(&self, symptom: &Symptom, candidate: &str) -> Option<String> {
        if !symptom.is_type_mismatch() {
            return None;
        }
        insert_arg_cast(candidate, &self.symbol, &self.abi)
    }
}

/// Insert the missing ABI cast on the *return* of a C-FFI wrapper. Fires on an
/// `E0308` symptom; rewrites the tail return expression `__r` → `__r as <native>`.
/// Idempotent — once the cast is present the tail line is no longer bare `__r`.
pub struct FfiReturnCastRepair {
    pub native: String,
}

impl RepairRule for FfiReturnCastRepair {
    fn name(&self) -> &'static str {
        "ffi-return-cast"
    }

    fn apply(&self, symptom: &Symptom, candidate: &str) -> Option<String> {
        if !symptom.is_type_mismatch() {
            return None;
        }
        insert_return_cast(candidate, &self.native)
    }
}

/// Rewrite the single-scalar-argument call `symbol(arg)` to `symbol(arg as abi)`,
/// skipping the `extern` declaration (`fn symbol(...)`). Returns `None` when no
/// un-cast call site exists (idempotent) or the call shape is unexpected.
fn insert_arg_cast(src: &str, symbol: &str, abi: &str) -> Option<String> {
    let needle = format!("{symbol}(");
    let bytes = src.as_bytes();
    let mut search = 0;
    while let Some(rel) = src[search..].find(&needle) {
        let at = search + rel;
        // Skip the foreign declaration: `fn symbol(...)`.
        if src[..at].trim_end().ends_with("fn") {
            search = at + needle.len();
            continue;
        }
        // Extract the balanced-paren argument list.
        let args_start = at + needle.len();
        let mut depth = 1usize;
        let mut idx = args_start;
        while idx < bytes.len() {
            match bytes[idx] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            idx += 1;
        }
        if depth != 0 {
            return None;
        }
        let args = &src[args_start..idx];
        if args.trim().is_empty() {
            return None;
        }
        // Idempotence: already cast → nothing to do.
        if args.trim_end().ends_with(&format!("as {abi}")) {
            return None;
        }
        return Some(format!(
            "{}{} as {}{}",
            &src[..args_start],
            args,
            abi,
            &src[idx..]
        ));
    }
    None
}

/// Rewrite the wrapper's tail return expression (a line that is exactly `__r`)
/// to `__r as native`. Returns `None` when there is no bare-`__r` tail line
/// (idempotent), so the rule fires at most once.
fn insert_return_cast(src: &str, native: &str) -> Option<String> {
    let lines: Vec<&str> = src.lines().collect();
    let pos = lines.iter().position(|l| l.trim() == "__r")?;
    let line = lines[pos];
    let indent = &line[..line.len() - line.trim_start().len()];
    let mut rebuilt: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    rebuilt[pos] = format!("{indent}__r as {native}");
    let mut joined = rebuilt.join("\n");
    if src.ends_with('\n') {
        joined.push('\n');
    }
    Some(joined)
}

// ─────────────────────────────────────────────────────────────────────────────
// The real toolchain probe: cc-compile the C side, rustc-compile + link the
// candidate Rust artifact, run it, and differentially compare to the Day-3
// CPython hybrid reference. This is the executing Phase-6 probe; it is gated on
// `cc` + `rustc` so a constrained runner graceful-skips.
// ─────────────────────────────────────────────────────────────────────────────

/// A [`Probe`] that builds and runs the hybrid artifact for real and checks it
/// against a captured CPython reference (`reference`, typically produced by
/// `xpile_oracle::capture_cpython_hybrid_ref` — the Day-3 oracle). The candidate
/// is a self-contained Rust program (the `extern` block + `*_shim` wrapper + a
/// `main` printing the boundary result); `c_source` is the sibling `_core.c`.
pub struct HybridCcRustcProbe {
    pub c_source: String,
    pub reference: String,
}

impl HybridCcRustcProbe {
    /// Are `cc` and `rustc` both spawnable? Lets the end-to-end test skip
    /// gracefully on a runner without a build toolchain.
    pub fn toolchain_available() -> bool {
        tool_ok("cc") && tool_ok("rustc")
    }
}

fn tool_ok(tool: &str) -> bool {
    std::process::Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

impl Probe for HybridCcRustcProbe {
    fn evaluate(&self, candidate: &str) -> Result<(), Symptom> {
        use std::process::Command;
        // Unique per evaluation so concurrent / repeated probes never collide.
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir();
        let cpath = dir.join(format!("xpile_repair_{pid}_{n}_core.c"));
        let opath = dir.join(format!("xpile_repair_{pid}_{n}_core.o"));
        let rpath = dir.join(format!("xpile_repair_{pid}_{n}.rs"));
        let bpath = dir.join(format!("xpile_repair_{pid}_{n}.bin"));
        let cleanup = || {
            for p in [&cpath, &opath, &rpath, &bpath] {
                let _ = std::fs::remove_file(p);
            }
        };

        // 1) Materialize + compile the C side to an object.
        if std::fs::write(&cpath, &self.c_source).is_err() {
            return Err(Symptom::BuildError {
                stderr: "writing C source failed".to_string(),
            });
        }
        match Command::new("cc")
            .arg("-c")
            .arg(&cpath)
            .arg("-o")
            .arg(&opath)
            .output()
        {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                cleanup();
                return Err(Symptom::BuildError {
                    stderr: format!("cc: {}", String::from_utf8_lossy(&o.stderr)),
                });
            }
            Err(e) => {
                cleanup();
                return Err(Symptom::BuildError {
                    stderr: format!("spawning cc: {e}"),
                });
            }
        }

        // 2) rustc-compile the candidate, linking the C object.
        if std::fs::write(&rpath, candidate).is_err() {
            cleanup();
            return Err(Symptom::BuildError {
                stderr: "writing Rust candidate failed".to_string(),
            });
        }
        match Command::new("rustc")
            .arg("--edition")
            .arg("2021")
            .arg(&rpath)
            .arg("-C")
            .arg(format!("link-arg={}", opath.display()))
            .arg("-o")
            .arg(&bpath)
            .output()
        {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                cleanup();
                return Err(Symptom::BuildError { stderr });
            }
            Err(e) => {
                cleanup();
                return Err(Symptom::BuildError {
                    stderr: format!("spawning rustc: {e}"),
                });
            }
        }

        // 3) Run the linked artifact and compare to the CPython reference.
        let result = match Command::new(&bpath).output() {
            Ok(o) if o.status.success() => {
                let actual = String::from_utf8_lossy(&o.stdout)
                    .trim_end_matches('\n')
                    .to_string();
                if actual == self.reference {
                    Ok(())
                } else {
                    Err(Symptom::Divergence {
                        index: 0,
                        expected: self.reference.clone(),
                        actual,
                    })
                }
            }
            Ok(o) => Err(Symptom::Divergence {
                index: 0,
                expected: self.reference.clone(),
                actual: format!(
                    "<artifact exited {}>: {}",
                    o.status,
                    String::from_utf8_lossy(&o.stderr).trim()
                ),
            }),
            Err(e) => Err(Symptom::BuildError {
                stderr: format!("running artifact: {e}"),
            }),
        };
        cleanup();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The correct, ABI-cast-complete shim for a `square_sum(int)->int` boundary
    /// — what `xpile-ffi-manifest` emits. Compiles + runs to `x*x`.
    const CORRECT: &str = "unsafe extern \"C\" {\n    fn square_sum(x: ::std::os::raw::c_int) -> ::std::os::raw::c_int;\n}\npub fn square_sum_shim(x: i64) -> i64 {\n    let __r = unsafe { square_sum(x as ::std::os::raw::c_int) };\n    __r as i64\n}\nfn main() {\n    println!(\"{}\", square_sum_shim(7));\n}\n";

    /// The same shim with BOTH ABI casts dropped — the real failure class. `rustc`
    /// rejects it with two `E0308`s (argument + return).
    const BROKEN: &str = "unsafe extern \"C\" {\n    fn square_sum(x: ::std::os::raw::c_int) -> ::std::os::raw::c_int;\n}\npub fn square_sum_shim(x: i64) -> i64 {\n    let __r = unsafe { square_sum(x) };\n    __r\n}\nfn main() {\n    println!(\"{}\", square_sum_shim(7));\n}\n";

    fn build_err() -> Symptom {
        Symptom::BuildError {
            stderr: "error[E0308]: mismatched types".to_string(),
        }
    }

    // ── Pure repair-rule transforms (no toolchain) ──────────────────────────

    #[test]
    fn arg_cast_targets_call_not_extern_decl() {
        let out = insert_arg_cast(BROKEN, "square_sum", "::std::os::raw::c_int").unwrap();
        // The call gained the cast …
        assert!(out.contains("square_sum(x as ::std::os::raw::c_int)"));
        // … and the extern declaration is untouched (still one decl, one call).
        assert_eq!(out.matches("as ::std::os::raw::c_int)").count(), 1);
        assert!(out.contains("fn square_sum(x: ::std::os::raw::c_int)"));
    }

    #[test]
    fn arg_cast_is_idempotent() {
        // CORRECT already has the arg cast → the rule declines to re-fire.
        assert!(insert_arg_cast(CORRECT, "square_sum", "::std::os::raw::c_int").is_none());
    }

    #[test]
    fn return_cast_rewrites_tail_only() {
        let out = insert_return_cast(BROKEN, "i64").unwrap();
        assert!(out.contains("    __r as i64\n"));
        // The `let __r = …` binding line is not the tail and stays bare.
        assert!(out.contains("let __r = unsafe"));
    }

    #[test]
    fn return_cast_is_idempotent() {
        assert!(insert_return_cast(CORRECT, "i64").is_none());
    }

    #[test]
    fn rules_only_fire_on_type_mismatch() {
        let arg = FfiArgCastRepair {
            symbol: "square_sum".to_string(),
            abi: "::std::os::raw::c_int".to_string(),
        };
        // A divergence is NOT a type mismatch → the ABI rules decline.
        let div = Symptom::Divergence {
            index: 0,
            expected: "49".to_string(),
            actual: "7".to_string(),
        };
        assert!(arg.apply(&div, BROKEN).is_none());
        assert!(arg.apply(&build_err(), BROKEN).is_some());
    }

    // ── Loop control flow with a pure in-memory probe ───────────────────────

    /// A probe that says `Ok` exactly when the candidate equals `CORRECT`, else
    /// reports a build error — modelling the rustc round-trip without a toolchain.
    struct ExactMatchProbe;
    impl Probe for ExactMatchProbe {
        fn evaluate(&self, candidate: &str) -> Result<(), Symptom> {
            if candidate == CORRECT {
                Ok(())
            } else {
                Err(build_err())
            }
        }
    }

    fn int_loop(max_iterations: u32) -> RepairLoop {
        let budget = Budget {
            max_iterations,
            max_tokens: 0,
            max_wall_clock: Duration::from_secs(60),
        };
        RepairLoop::ffi_int_boundary(budget, "square_sum")
    }

    #[test]
    fn loop_converges_in_two_iterations() {
        let outcome = int_loop(8).run(&ExactMatchProbe, BROKEN);
        match outcome {
            RepairOutcome::Repaired { iterations, source } => {
                assert_eq!(iterations, 2, "arg-cast then return-cast");
                assert_eq!(source, CORRECT);
            }
            other => panic!("expected Repaired, got {other:?}"),
        }
    }

    #[test]
    fn loop_reports_already_matching() {
        let outcome = int_loop(8).run(&ExactMatchProbe, CORRECT);
        assert!(matches!(outcome, RepairOutcome::AlreadyMatching { .. }));
        assert!(outcome.is_match());
    }

    #[test]
    fn loop_fails_closed_on_too_small_a_budget() {
        // One iteration cannot fix two casts → Exhausted, and crucially NO source.
        let outcome = int_loop(1).run(&ExactMatchProbe, BROKEN);
        match &outcome {
            RepairOutcome::Exhausted { iterations, .. } => assert_eq!(*iterations, 1),
            other => panic!("expected Exhausted, got {other:?}"),
        }
        assert!(outcome.source().is_none(), "fail-closed: no partial source");
        assert!(!outcome.is_match());
    }

    #[test]
    fn loop_fails_closed_when_no_rule_applies() {
        // A pure divergence (not a type mismatch): no ABI rule fires → immediate
        // fail-closed exhaustion at iteration 0.
        struct DivergeProbe;
        impl Probe for DivergeProbe {
            fn evaluate(&self, _candidate: &str) -> Result<(), Symptom> {
                Err(Symptom::Divergence {
                    index: 0,
                    expected: "49".to_string(),
                    actual: "7".to_string(),
                })
            }
        }
        let outcome = int_loop(8).run(&DivergeProbe, BROKEN);
        match &outcome {
            RepairOutcome::Exhausted { iterations, last } => {
                assert_eq!(*iterations, 0);
                assert!(matches!(last, Symptom::Divergence { .. }));
            }
            other => panic!("expected Exhausted, got {other:?}"),
        }
        assert!(outcome.source().is_none());
    }

    #[test]
    fn run_and_commit_writes_on_repair_and_not_on_exhaustion() {
        let dir = std::env::temp_dir();
        let pid = std::process::id();

        // Repaired → file written with the corrected source.
        let ok_path = dir.join(format!("xpile_repair_commit_ok_{pid}.rs"));
        let _ = std::fs::remove_file(&ok_path);
        let outcome = int_loop(8)
            .run_and_commit(&ExactMatchProbe, BROKEN, &ok_path)
            .unwrap();
        assert!(matches!(outcome, RepairOutcome::Repaired { .. }));
        assert_eq!(std::fs::read_to_string(&ok_path).unwrap(), CORRECT);
        let _ = std::fs::remove_file(&ok_path);

        // Exhausted → the destination is NEVER touched (fail-closed write).
        let no_path = dir.join(format!("xpile_repair_commit_none_{pid}.rs"));
        let _ = std::fs::remove_file(&no_path);
        let outcome = int_loop(1)
            .run_and_commit(&ExactMatchProbe, BROKEN, &no_path)
            .unwrap();
        assert!(matches!(outcome, RepairOutcome::Exhausted { .. }));
        assert!(
            !no_path.exists(),
            "fail-closed: nothing written on exhaustion"
        );
    }
}
