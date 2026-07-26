//! PMAT-1350 — the TWO-WAY emit-surface gate for `C-COMPILE-RUST-TO-WASM`.
//!
//! ## What went wrong
//!
//! `contracts/compile-rust-to-wasm-v1.yaml` asserted, in its `metadata`
//! description and again in its **ship-blocking** `FALSIFY-COMPILE-WASM-002`,
//! that the WASM emitter "REFUSES every construct outside the scalar/control
//! subset (str / list / dict / set / struct / tuple / bigint / pointer /
//! print / closures)".
//!
//! Five of those ten had shipped. A str-keyed dict program emits, assembles
//! under `wat2wasm`, and executes under `wasm-interp` returning the CPython
//! answer. The sentence was TRUE when it was written at PMAT-951 and decayed
//! silently across ~69 capability slices, because nothing in the repo sampled
//! contract *capability prose* — `claims_drift.rs` covers README/CURRENT.md
//! claims, not contract bodies.
//!
//! ## Why prose alone was not the fix
//!
//! Re-typing today's true surface into today's prose re-arms exactly the same
//! bomb: the next capability slice widens the emitter, nobody edits the YAML,
//! and the contract is false again — this time with a fresher date on it,
//! which is worse. That is the same reasoning PMAT-1348 applied to
//! `docs/status/CURRENT.md` (replace counts with derive commands rather than
//! with correct counts).
//!
//! So the subset is now a machine-readable `emit_surface` block in the
//! contract, and this file executes it in BOTH directions against the live
//! `xpile_core::default_session()` — the same frontend + backend the
//! `xpile transpile --target wasm` CLI dispatches through, so the gate cannot
//! drift from the shipped binary:
//!
//! * every `emit_surface.declared` probe MUST lower to non-empty WAT.
//!   A declared construct that refuses is a capability REGRESSION, or a
//!   declaration that was never true (the contract OVER-claims).
//! * every `emit_surface.refused` probe MUST refuse with a hard error.
//!   A refused construct that emits means the contract UNDER-claims — the
//!   PMAT-1350 failure itself recurring.
//!
//! The second direction is the one that makes the ship-blocking claim worth
//! anything, because it is the claim a reader uses to decide what they may
//! rely on. The first direction is what stops the contract from being
//! trivially satisfiable by declaring nothing.
//!
//! ## Non-vacuity
//!
//! A gate over a table is only as good as the table, so the structural
//! assertions below are load-bearing:
//!
//! * both lists must be non-empty and carry a floor, so nobody "fixes" a red
//!   by deleting rows;
//! * ids are unique and no id appears on both sides;
//! * every probe must be non-trivial source (a comment-only or empty probe
//!   can lower — or refuse — for reasons unrelated to the construct);
//! * a `refused` entry declares WHICH STAGE refuses it. Without this, a probe
//!   with a typo would refuse at the frontend and score as a green "refusal"
//!   while proving nothing about the WASM backend. `stage: backend` (the
//!   default) means the program lowers to meta-HIR fine and the WASM backend
//!   is what rejects it; `stage: frontend` is the explicit, narrow exception
//!   for target-independent refusals such as an out-of-i64-range literal.

use std::path::PathBuf;
use xpile_backend::{BackendConfig, Profile, Target};
use xpile_frontend::{AliasSemantics, LoweringProfile};

fn contract_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contracts/compile-rust-to-wasm-v1.yaml")
}

/// Floors, so a red is never "fixable" by emptying the table. Deliberately
/// well below the live counts (28 declared / 11 refused at PMAT-1350) —
/// this is a non-vacuity floor, not a coverage target, and it must not become
/// a treadmill that every capability slice has to bump.
const MIN_DECLARED: usize = 20;
const MIN_REFUSED: usize = 8;

#[derive(Debug, serde::Deserialize)]
struct Contract {
    emit_surface: EmitSurface,
}

#[derive(Debug, serde::Deserialize)]
struct EmitSurface {
    declared: Vec<Entry>,
    refused: Vec<Entry>,
}

#[derive(Debug, serde::Deserialize)]
struct Entry {
    id: String,
    probe: String,
    #[allow(dead_code)]
    note: String,
    /// Which pipeline stage is expected to refuse. Only meaningful for
    /// `refused` entries; absent means `backend`.
    #[serde(default)]
    stage: Option<String>,
}

impl Entry {
    /// `backend` unless the entry explicitly opts into the frontend exception.
    fn expected_stage(&self) -> &str {
        self.stage.as_deref().unwrap_or("backend")
    }
}

/// What the live pipeline actually did with a probe.
#[derive(Debug)]
enum Outcome {
    /// Lowered all the way to WAT text.
    Emitted(String),
    /// The frontend rejected the program (target-independent).
    RefusedFrontend(String),
    /// The frontend lowered it and the WASM backend rejected it.
    RefusedBackend(String),
}

impl Outcome {
    fn stage(&self) -> &str {
        match self {
            Outcome::Emitted(_) => "emitted",
            Outcome::RefusedFrontend(_) => "frontend",
            Outcome::RefusedBackend(_) => "backend",
        }
    }

    /// The refusal message, or the emitted text for the `Emitted` arm.
    fn message(&self) -> &str {
        match self {
            Outcome::Emitted(t) | Outcome::RefusedFrontend(t) | Outcome::RefusedBackend(t) => t,
        }
    }
}

fn load() -> Contract {
    let path = contract_path();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e} — the contract moved", path.display()));
    serde_yaml::from_str(&text).unwrap_or_else(|e| {
        panic!(
            "parsing {}: {e} — the emit_surface block is malformed",
            path.display()
        )
    })
}

/// Drive one probe through the SAME two dispatch steps `xpile transpile
/// --target wasm` performs: frontend selected by path match, backend selected
/// by `targets().contains(&Target::Wasm)`, and the PMAT-1024/1034 lowering
/// profile for a linear-memory target.
fn run_probe(source: &str) -> Outcome {
    let session = xpile_core::default_session();
    let path = PathBuf::from("wasm_contract_surface_probe.py");

    let frontend = session
        .frontends
        .iter()
        .find(|f| f.matches_path(&path))
        .expect("no frontend claims `.py` — the registry moved");

    // Target::Wasm holds container/struct locals as i32 base-pointers, so a
    // binding copy IS Python object sharing: Reference alias semantics, and
    // the `unreachable` trap can carry the loop-var-leak guard.
    let profile = LoweringProfile {
        alias_semantics: AliasSemantics::Reference,
        runtime_abort: true,
    };

    let module = match frontend.parse_and_lower_profiled(&path, source, profile) {
        Ok(m) => m,
        Err(e) => return Outcome::RefusedFrontend(e.to_string()),
    };

    let backend = session
        .backends
        .iter()
        .find(|b| b.targets().contains(&Target::Wasm))
        .expect("no backend owns Target::Wasm — the registry moved");

    let config = BackendConfig {
        target: Target::Wasm,
        profile: Profile::RustOut,
        hardware: None,
        // The CLI default is `--contracts on`; probe what users get.
        emit_contracts: true,
    };

    match backend.lower(&module, &config) {
        Ok(artifact) => Outcome::Emitted(artifact.primary),
        Err(e) => Outcome::RefusedBackend(e.to_string()),
    }
}

/// DIRECTION 1 — the contract may not OVER-claim. Every construct the
/// contract declares as emittable must actually emit non-empty WAT.
///
/// This also makes the refusal half non-trivial: a contract that declared
/// nothing would satisfy "everything outside the subset refuses" vacuously.
#[test]
fn every_declared_construct_emits() {
    let c = load();
    let mut failures = Vec::new();
    for e in &c.emit_surface.declared {
        match run_probe(&e.probe) {
            Outcome::Emitted(wat) if wat.contains("(module") => {}
            Outcome::Emitted(wat) => failures.push(format!(
                "declared `{}` emitted text with no `(module` form (len {}) — \
                 that is not a WASM module",
                e.id,
                wat.len()
            )),
            other => failures.push(format!(
                "declared `{}` was REFUSED at the {} stage: {}",
                e.id,
                other.stage(),
                other.message()
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "C-COMPILE-RUST-TO-WASM OVER-CLAIMS its emit surface. Either a \
         capability regressed, or `emit_surface.declared` lists something \
         that never emitted. Do NOT silence this by deleting the row unless \
         the capability was genuinely withdrawn — move it to \
         `emit_surface.refused` and say so in the CHANGELOG.\n  - {}",
        failures.join("\n  - ")
    );
}

/// DIRECTION 2 — the contract may not UNDER-claim. This is the direction that
/// was live-false before PMAT-1350 and the reason this file exists: a
/// construct the contract swears is refused must actually refuse, at the
/// stage the contract says refuses it.
#[test]
fn every_refused_construct_refuses_at_the_declared_stage() {
    let c = load();
    let mut failures = Vec::new();
    for e in &c.emit_surface.refused {
        let outcome = run_probe(&e.probe);
        let expected = e.expected_stage();
        match (&outcome, expected) {
            (Outcome::RefusedBackend(m), "backend") | (Outcome::RefusedFrontend(m), "frontend") => {
                // A refusal with no reason is only marginally better than a
                // silent wrong answer: the user still cannot tell what xpile
                // could not do.
                if m.trim().is_empty() {
                    failures.push(format!("refused `{}` refused with an EMPTY message", e.id));
                }
            }
            (Outcome::Emitted(wat), _) => failures.push(format!(
                "refused `{}` EMITTED {} bytes of WAT — the contract under-claims",
                e.id,
                wat.len()
            )),
            (other, _) => failures.push(format!(
                "refused `{}` declares `stage: {}` but was refused at the {} \
                 stage instead: {}",
                e.id,
                expected,
                other.stage(),
                other.message()
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "C-COMPILE-RUST-TO-WASM UNDER-CLAIMS its emit surface — this is the \
         PMAT-1350 failure recurring. A capability slice widened the emitter \
         without widening `emit_surface`. Move the construct from `refused` \
         to `declared` with a probe and a note, IN THE SAME PR as the \
         capability. A `stage:` mismatch means the probe is refused for a \
         reason other than the one being claimed, which proves nothing.\n  - {}",
        failures.join("\n  - ")
    );
}

/// The table itself must be substantive, or both directions above pass while
/// covering nothing. Every assertion here is about a way the gate could be
/// neutered without anyone noticing.
#[test]
fn the_emit_surface_table_is_not_vacuous() {
    let c = load();
    let (d, r) = (&c.emit_surface.declared, &c.emit_surface.refused);

    assert!(
        d.len() >= MIN_DECLARED,
        "emit_surface.declared has {} entries, floor is {MIN_DECLARED}. \
         Rows were deleted rather than a real failure fixed.",
        d.len()
    );
    assert!(
        r.len() >= MIN_REFUSED,
        "emit_surface.refused has {} entries, floor is {MIN_REFUSED}. \
         Rows were deleted rather than a real failure fixed.",
        r.len()
    );

    let mut ids: Vec<&str> = d.iter().chain(r.iter()).map(|e| e.id.as_str()).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(
        ids.len(),
        total,
        "duplicate id in emit_surface — a construct is listed twice, and if it \
         is listed on BOTH sides the two directions contradict each other"
    );

    for e in d.iter().chain(r.iter()) {
        // A probe must be real source. An empty or comment-only probe can
        // lower (to an empty module) or refuse for reasons that have nothing
        // to do with the construct it claims to exercise.
        let code: Vec<&str> = e
            .probe
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
            .collect();
        assert!(
            code.len() >= 2,
            "probe for `{}` has {} non-comment lines — too small to exercise \
             a construct; write the smallest program WITH REAL CONTENT",
            e.id,
            code.len()
        );
        assert!(
            e.probe.contains("def "),
            "probe for `{}` defines no function — there is nothing for the \
             backend to lower",
            e.id
        );
        assert!(
            !e.note.trim().is_empty(),
            "entry `{}` has an empty note — say what the construct is and, \
             for a refusal, why it is refused",
            e.id
        );
    }

    for e in r {
        let s = e.expected_stage();
        assert!(
            s == "backend" || s == "frontend",
            "refused entry `{}` declares unknown `stage: {s}` (expected \
             `backend` or `frontend`)",
            e.id
        );
    }
    // The frontend exception is deliberately narrow: a frontend refusal is
    // target-INDEPENDENT and therefore says nothing about the WASM backend.
    // If most of the table drifted to `stage: frontend` the gate would stop
    // testing the thing it exists to test.
    let frontend_staged = r
        .iter()
        .filter(|e| e.expected_stage() == "frontend")
        .count();
    assert!(
        frontend_staged * 2 < r.len(),
        "{frontend_staged} of {} refused entries are `stage: frontend`. A \
         frontend refusal is target-independent and proves nothing about the \
         WASM backend — the majority of refusals must be backend-staged.",
        r.len()
    );
}

/// Regression pin on the exact prose that was false. The `emit_surface` gate
/// above cannot see prose, and prose is where the claim a reader actually
/// reads lives — `metadata.description` is what `pv` renders and what any
/// human opens the file to.
#[test]
fn the_contract_prose_no_longer_lists_shipped_constructs_as_refused() {
    let text = std::fs::read_to_string(contract_path()).expect("reading the contract");
    let (desc, _) = text
        .split_once("compile_targets:")
        .expect("contract lost its `compile_targets:` section");

    // The literal pre-PMAT-1350 claim. Its distinguishing feature is asserting
    // a refusal set that BEGINS with str/list/dict/set/struct — all of which
    // emit. The historical quotation of that sentence inside the PMAT-1350
    // paragraph is kept (it is the receipt), so pin the ASSERTION form: the
    // words must not appear as the object of "REFUSES ... outside".
    for stale in [
        "REFUSES every construct outside the scalar/control subset (str",
        "outside the scalar/control subset (str / list / dict / set / struct",
    ] {
        assert!(
            !desc.contains(stale),
            "the pre-PMAT-1350 refusal claim is back in metadata.description: \
             {stale:?}. str/list/dict/set/struct all EMIT — a str-keyed dict \
             program assembles under wat2wasm and executes. Point the prose at \
             the machine-checked `emit_surface` block instead."
        );
    }
}
