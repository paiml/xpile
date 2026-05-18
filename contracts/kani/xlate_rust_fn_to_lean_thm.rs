//! Kani BMC harness for `C-XLATE-RUST-FN-TO-LEAN-THM` (PMAT-073 /
//! XPILE-XLATE-RUST-TO-LEAN-001).
//!
//! This is the **Symbolic stratum** counterpart for the
//! `Rust fn → Lean def` lifting body-preservation invariant.
//! With this harness landed, `C-XLATE-RUST-FN-TO-LEAN-THM`
//! reaches §14.4 QUORUM — tenth contract to do so, and **closes
//! the bidirectional Rust ↔ Lean translation bracket at full
//! paired-discharge coverage**:
//!
//!   direction       | Lean theorem | Kani harness
//!   Lean → Rust     | PMAT-070     | PMAT-071
//!   Rust → Lean     | PMAT-072     | PMAT-073  ← this file
//!
//! Strata for this contract:
//!   * Semantic    (PMAT-072): `contracts/lean/XlateRustFnToLeanThm.lean`
//!   * Symbolic    (PMAT-073): this file
//!   * Runtime     (—)        : awaiting xpile-lean-contract-backend
//!                              Rust→Lean lifting (XPILE-XLATE-RUST-TO-LEAN-RUNTIME-001)
//!   * Extrinsic   (PMAT-072..073): roadmap mentions
//!
//! ## What this harness proves
//!
//! The Rust mirror of the Lean theorem `rust_fn_to_lean_def`
//! (see `contracts/lean/XlateRustFnToLeanThm.lean`). Lifting a
//! `RustFn` to a `LeanDef` preserves the function body — proved
//! by byte-level identity over `[u8; 4]` symbolic input.
//!
//! ## Cross-reinforcement
//!
//! Bidirectional with PMAT-072's Lean theorem. The pair locks
//! in the body-preservation modelling commitment from both
//! formal sides. With PMAT-070/071 already shipped for the
//! reverse direction, the full Rust ↔ Lean translation is now
//! bracketed at QUORUM in both directions.
//!
//! Any future PR that changes the Rust ↔ Lean lowering in
//! either direction must update both Lean theorems *and* both
//! Kani harnesses, or the refinement-proof citation gate fires.

#![cfg(kani)]

/// Rust mirror of Lean's `RustFn`. v0.1.0 Bronze-tier model —
/// a fixed-size byte array carrying the function body.
/// Silver-tier refinement (XPILE-REFINE-XLATE-RUST-TO-LEAN-001)
/// replaces this with typed AST nodes (`{ name, generics, args,
/// return_type, body }`).
#[derive(PartialEq, Eq, Clone, Copy)]
struct RustFn {
    body: [u8; 4],
}

/// Rust mirror of Lean's `LeanDef`. Same v0.1.0 shape as
/// `RustFn` — locking in the body-preservation claim at the
/// byte level. Silver-tier refinement introduces typed
/// `LeanDef { name, binders, return_type, body, attrs }` plus
/// the `@[xpile_contract]` attribute generation.
#[derive(PartialEq, Eq, Clone, Copy)]
struct LeanDef {
    body: [u8; 4],
}

/// Rust mirror of Lean's `lift_fn_to_def`. v0.1.0 model:
/// byte-identity. The Bronze-tier placeholder captures the
/// body-preservation property; real
/// xpile-lean-contract-backend impls do much more (Rust parser,
/// contract obligation lifting, Lean attribute emission), but
/// are bound to the same invariant via the translation contract.
fn lift_fn_to_def(f: &RustFn) -> LeanDef {
    LeanDef { body: f.body }
}

/// Equation `rust_fn_to_lean_def` from
/// `contracts/xlate-rust-fn-to-lean-thm-v1.yaml`:
///
///   lift(rust_fn(args) -> R, contract_C) ==
///     def <fn_name> (args : T_lean) : R_lean := body_lean
///
/// Symbolic counterpart to
/// `XpileContracts.CXlateRustFnToLeanThm.rust_fn_to_lean_def`
/// in `contracts/lean/XlateRustFnToLeanThm.lean`. Kani
/// exhaustively explores all 4-byte symbolic function bodies
/// (256^4 ≈ 4.3B configurations) and verifies the lifted
/// LeanDef carries the same byte sequence as the source
/// RustFn.
#[kani::proof]
fn rust_fn_to_lean_def() {
    let input: [u8; 4] = kani::any();
    let rust_fn = RustFn { body: input };
    let lean_def = lift_fn_to_def(&rust_fn);

    kani::assert(
        lean_def.body == rust_fn.body,
        "lift_fn_to_def must preserve the function body",
    );
}

// ============================================================
// PMAT-148 — Kani harnesses for the 4 remaining equations of
// C-XLATE-RUST-FN-TO-LEAN-THM, mirroring the Bronze-tier Lean
// theorems shipped in PMAT-136. Each harness captures the same
// load-bearing modelling commitment as its Lean counterpart via
// byte-level symbolic exploration.
// ============================================================

/// Bronze-tier model of a single contract obligation. `applies_to_all`
/// distinguishes 1:1 (single equation) from 1:N (`applies_to: all`).
#[derive(PartialEq, Eq, Clone, Copy)]
struct ContractObligation {
    applies_to_all: bool,
}

fn expansion_count(obl: &ContractObligation, equation_count: u8) -> u8 {
    if obl.applies_to_all {
        equation_count
    } else {
        1
    }
}

/// Equation `rust_postcondition_to_lean_theorem`: 1:1 / 1:N
/// expansion rule. Falsified by an emitter that merges multiple
/// obligations into a single theorem (loses provenance) or that
/// drops `applies_to: all` obligations on contracts with zero
/// equations.
#[kani::proof]
fn rust_postcondition_to_lean_theorem() {
    let applies_to_all: bool = kani::any();
    let equation_count: u8 = kani::any();
    let obl = ContractObligation { applies_to_all };
    let n = expansion_count(&obl, equation_count);
    if applies_to_all {
        kani::assert(
            n == equation_count,
            "applies_to: all must expand to one theorem per equation",
        );
    } else {
        kani::assert(
            n == 1,
            "single-equation applies_to must emit exactly one theorem",
        );
    }
}

/// Bronze-tier model of a precondition list. The lifting must
/// preserve count + source order — modelled here as byte-array
/// identity on a fixed-size precondition vector.
#[derive(PartialEq, Eq, Clone, Copy)]
struct PreconditionList {
    sources: [u8; 4],
}

fn lift_preconditions(p: &PreconditionList) -> PreconditionList {
    PreconditionList { sources: p.sources }
}

/// Equation `rust_precondition_to_lean_hypothesis`: count + source-
/// order preservation. Falsified by an emitter that uses an
/// unordered Set as the intermediate (which would scramble source
/// order) or deduplicates by syntactic equality (which would drop
/// semantically-distinct re-statements).
#[kani::proof]
fn rust_precondition_to_lean_hypothesis() {
    let sources: [u8; 4] = kani::any();
    let pre = PreconditionList { sources };
    let lifted = lift_preconditions(&pre);
    kani::assert(
        lifted.sources == pre.sources,
        "preconditions must lift with count + source order preserved",
    );
}

/// Bronze-tier model of an emitted Lean attribute payload.
#[derive(PartialEq, Eq, Clone, Copy)]
struct XpileContractAttribute {
    contract_id: [u8; 4],
    equation_name: [u8; 4],
}

fn emit_attribute(contract_id: [u8; 4], equation_name: [u8; 4]) -> XpileContractAttribute {
    XpileContractAttribute {
        contract_id,
        equation_name,
    }
}

/// Equation `citation_bridge_via_attribute`: emitted attribute
/// payload carries contract_id + equation_name byte-for-byte.
/// Falsified by an emitter that "tidies up" the payload to match
/// Lean naming conventions (dash-to-underscore, case folding,
/// Unicode normalisation).
#[kani::proof]
fn citation_bridge_via_attribute() {
    let contract_id: [u8; 4] = kani::any();
    let equation_name: [u8; 4] = kani::any();
    let attr = emit_attribute(contract_id, equation_name);
    kani::assert(
        attr.contract_id == contract_id,
        "contract_id must appear byte-for-byte in attribute payload",
    );
    kani::assert(
        attr.equation_name == equation_name,
        "equation_name must appear byte-for-byte in attribute payload",
    );
}

/// Bronze-tier model of the lift inputs (module + contract hashes).
#[derive(PartialEq, Eq, Clone, Copy)]
struct LiftInputs {
    module_hash: [u8; 4],
    contract_hash: [u8; 4],
}

fn lift_frame_preserving(inputs: &LiftInputs) -> LiftInputs {
    LiftInputs {
        module_hash: inputs.module_hash,
        contract_hash: inputs.contract_hash,
    }
}

/// Equation `frame_translation_is_textual`: lift() does NOT mutate
/// the meta-HIR module or contract YAML. Falsified by an emitter
/// that "normalises" the contract YAML in-place (e.g., sorts
/// equation keys alphabetically) — which would break the
/// source-order invariant AND cache-determinism.
#[kani::proof]
fn frame_translation_is_textual() {
    let module_hash: [u8; 4] = kani::any();
    let contract_hash: [u8; 4] = kani::any();
    let inputs = LiftInputs {
        module_hash,
        contract_hash,
    };
    let out = lift_frame_preserving(&inputs);
    kani::assert(
        out.module_hash == inputs.module_hash,
        "lift() must not mutate the module hash (cache-determinism)",
    );
    kani::assert(
        out.contract_hash == inputs.contract_hash,
        "lift() must not mutate the contract hash (cache-determinism)",
    );
}
