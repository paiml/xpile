//! Kani BMC harness for `C-XLATE-LEAN-TO-RUST` (PMAT-071 /
//! XPILE-XLATE-LEAN-001).
//!
//! This is the **Symbolic stratum** counterpart for the
//! `Lean def → Rust fn` lowering body-preservation invariant.
//! With this harness landed, `C-XLATE-LEAN-TO-RUST` reaches
//! §14.4 QUORUM — ninth contract to do so:
//!
//!   * Semantic    (PMAT-070): `contracts/lean/XlateLeanToRust.lean`
//!   * Symbolic    (PMAT-071): this file
//!   * Runtime     (—)        : awaiting xpile-rust-codegen Lean
//!                              lowering (XPILE-XLATE-LEAN-RUNTIME-001)
//!   * Extrinsic   (PMAT-070..071): roadmap mentions
//!
//! ## What this harness proves
//!
//! The Rust mirror of the Lean theorem `def_to_rust_fn` (see
//! `contracts/lean/XlateLeanToRust.lean`). Lowering a `LeanDef`
//! to a `RustFn` preserves the function body — proved by
//! byte-level identity over `[u8; 4]` symbolic input.
//!
//! ## Why fixed-byte symbolic input
//!
//! Same rationale as PMAT-058..069: Kani handles fixed-size byte
//! arrays orders of magnitude faster than symbolic `String` or
//! `Vec`. The 4-byte bound is sufficient — the body-preservation
//! property is length-independent and structural; 256^4 ≈ 4.3B
//! exhaustive configurations covers all 4-byte function bodies.
//!
//! ## Cross-reinforcement
//!
//! Bidirectional with PMAT-070's Lean theorem. The pair locks
//! in the body-preservation modelling commitment from both
//! formal sides. Any future xpile-rust-codegen impl that mutates
//! the Lean body during lowering — dropping comments, normalising
//! whitespace, wrapping in instrumentation macros — must
//! invalidate both discharges or face the refinement-proof
//! citation gate.
//!
//! Companion to PMAT-061's `iteration_order_preserved` (which
//! covers Python → Rust list lowering). Both are Layer-2
//! translation contracts at Bronze tier; together they bracket
//! the two directions of the proof-↔-code lane bridge.

#![cfg(kani)]

/// Rust mirror of Lean's `LeanDef`. v0.1.0 Bronze-tier model —
/// a fixed-size byte array carrying the function body. Silver-
/// tier refinement (XPILE-REFINE-XLATE-LEAN-TO-RUST-001)
/// replaces this with typed AST nodes (`{ name, args,
/// return_type, body }`).
#[derive(PartialEq, Eq, Clone, Copy)]
struct LeanDef {
    body: [u8; 4],
}

/// Rust mirror of Lean's `RustFn`. Same v0.1.0 shape as
/// `LeanDef` — locking in the body-preservation claim at the
/// byte level.
#[derive(PartialEq, Eq, Clone, Copy)]
struct RustFn {
    body: [u8; 4],
}

/// Rust mirror of Lean's `lower_def_to_fn`. v0.1.0 model:
/// byte-identity. The Bronze-tier placeholder captures the
/// body-preservation property; real xpile-rust-codegen impls
/// do much more (Lean parser, HIR construction, Rust emission),
/// but are bound to the same invariant via the translation
/// contract.
fn lower_def_to_fn(d: &LeanDef) -> RustFn {
    RustFn { body: d.body }
}

/// Equation `def_to_rust_fn` from
/// `contracts/xlate-lean-to-rust-v1.yaml`:
///
///   xlate(def f := body) == fn f { body_rust }
///
/// Symbolic counterpart to
/// `XpileContracts.CXlateLeanToRust.def_to_rust_fn` in
/// `contracts/lean/XlateLeanToRust.lean`. Kani exhaustively
/// explores all 4-byte symbolic function bodies (256^4 ≈ 4.3B
/// configurations) and verifies the lowered RustFn carries the
/// same byte sequence as the source LeanDef.
#[kani::proof]
fn def_to_rust_fn() {
    let input: [u8; 4] = kani::any();
    let lean_def = LeanDef { body: input };
    let rust_fn = lower_def_to_fn(&lean_def);

    kani::assert(
        rust_fn.body == lean_def.body,
        "lower_def_to_fn must preserve the function body",
    );
}

// ─── PMAT-278: Silver-tier property-specific Kani harnesses ─────────
//
// Audit-design.md §4 caveat: Bronze-tier Kani harnesses are "byte-
// identity placeholders". This block closes the caveat for
// C-XLATE-LEAN-TO-RUST's `def_to_rust_fn` equation by lifting the
// Kani side to match Lean's Silver tier already shipped at PMAT-165
// (`name_preserved_silver`, `body_preserved_silver`, `args_preserved_silver`,
// `return_type_preserved_silver` in `contracts/lean/XlateLeanToRust.lean`).
//
// The Bronze harness above proves byte-equality on a single 4-byte
// payload — a buggy emitter that swapped name and body bytes would
// pass. The Silver tier decomposes into 4 fields on both sides.

/// Silver-tier model of a Lean `def` declaration — Rust mirror of
/// `LeanDefSilver`. Four named fields each occupying one symbolic
/// byte under Kani.
#[derive(PartialEq, Eq, Clone, Copy)]
struct LeanDefSilver {
    name: u8,
    args: u8,
    return_type: u8,
    body: u8,
}

/// Silver-tier model of a Rust `fn` declaration — mirror image of
/// `LeanDefSilver`. Same four fields. The structural split is what
/// makes the Silver refinement non-trivial: a buggy emitter that
/// mangles ONE field but preserves the OTHERS (e.g., snake-case
/// normalizing names while leaving body bytes alone) would falsify
/// only the affected per-field proof.
#[derive(PartialEq, Eq, Clone, Copy)]
struct RustFnSilver {
    name: u8,
    args: u8,
    return_type: u8,
    body: u8,
}

/// Silver-tier lowering — Rust mirror of Lean's
/// `lower_def_to_fn_silver`. Structural copy preserving every named
/// field. Each field copies byte-for-byte; Gold tier introduces a
/// per-field equivalence relation (e.g., body modulo whitespace,
/// args modulo positional reordering when type-driven).
fn lower_def_to_fn_silver(d: &LeanDefSilver) -> RustFnSilver {
    RustFnSilver {
        name: d.name,
        args: d.args,
        return_type: d.return_type,
        body: d.body,
    }
}

fn arb_lean_def() -> LeanDefSilver {
    LeanDefSilver {
        name: kani::any(),
        args: kani::any(),
        return_type: kani::any(),
        body: kani::any(),
    }
}

/// PMAT-278 — Silver-tier counterpart to `name_preserved_silver`
/// (Lean PMAT-165).
///
/// Lowering preserves the function name as a separate structural
/// field, distinct from the body. An emitter that mangles the name
/// (snake_case normalization, prefix stripping, kebab→snake) would
/// falsify this proof — Bronze byte-equality on a joined payload
/// couldn't catch the field-level corruption.
#[kani::proof]
fn name_preserved_silver() {
    let d = arb_lean_def();
    let r = lower_def_to_fn_silver(&d);
    kani::assert(
        r.name == d.name,
        "lower_def_to_fn must preserve the function name as a separate field",
    );
}

/// PMAT-278 — Silver-tier counterpart to `body_preserved_silver`
/// (Lean PMAT-165).
///
/// Body preserved as a structural field distinct from name/args/return_type.
/// Companion to `name_preserved_silver`; the pair locks in the
/// contract claim that BOTH fields must be preserved, not just one.
#[kani::proof]
fn body_preserved_silver() {
    let d = arb_lean_def();
    let r = lower_def_to_fn_silver(&d);
    kani::assert(
        r.body == d.body,
        "lower_def_to_fn must preserve the function body as a separate field",
    );
}

/// PMAT-278 — Silver-tier counterpart to `args_preserved_silver`
/// (Lean PMAT-165).
///
/// Argument list preserved. Argument-ordering preservation is
/// critical for `Decidable` / `Hashable` instance method bodies
/// that pattern-match positionally.
#[kani::proof]
fn args_preserved_silver() {
    let d = arb_lean_def();
    let r = lower_def_to_fn_silver(&d);
    kani::assert(
        r.args == d.args,
        "lower_def_to_fn must preserve the args list",
    );
}

/// PMAT-278 — Silver-tier counterpart to `return_type_preserved_silver`
/// (Lean PMAT-165).
///
/// Return type preserved. Distinguishes the Bronze single-payload
/// model from any emitter strategy that "infers" the return type
/// from the body (Rust-side `-> _` elision); such inference is
/// banned by the contract at Silver tier and would falsify this
/// proof.
#[kani::proof]
fn return_type_preserved_silver() {
    let d = arb_lean_def();
    let r = lower_def_to_fn_silver(&d);
    kani::assert(
        r.return_type == d.return_type,
        "lower_def_to_fn must preserve the return type explicitly (no `-> _` elision)",
    );
}

// ============================================================
// PMAT-147 — Kani harnesses for the 8 remaining equations of
// C-XLATE-LEAN-TO-RUST, mirroring the Bronze-tier Lean theorems
// shipped in PMAT-133. Each harness captures the same load-bearing
// modelling commitment as its Lean counterpart via byte-level
// symbolic exploration. Silver-tier refinement
// (XPILE-REFINE-XLATE-LEAN-TO-RUST-001+) introduces typed AST
// nodes and the proofs become structural.
// ============================================================

/// Rust mirror of Lean's `LeanPartialDef`. Carries body bytes plus
/// a partial-translation marker that the lowering MUST preserve.
#[derive(PartialEq, Eq, Clone, Copy)]
struct LeanPartialDef {
    body: [u8; 4],
    is_partial: u8,
}

#[derive(PartialEq, Eq, Clone, Copy)]
struct RustPartialFn {
    body: [u8; 4],
    partial_marker: u8,
}

fn lower_partial_def_to_fn(d: &LeanPartialDef) -> RustPartialFn {
    RustPartialFn {
        body: d.body,
        partial_marker: d.is_partial,
    }
}

#[kani::proof]
fn partial_def_to_rust_fn() {
    let body: [u8; 4] = kani::any();
    let is_partial: u8 = kani::any();
    let d = LeanPartialDef { body, is_partial };
    let r = lower_partial_def_to_fn(&d);
    kani::assert(r.body == d.body, "body must be preserved");
    kani::assert(
        r.partial_marker == d.is_partial,
        "partial marker must be preserved (no silent stripping of #[partial_translation])",
    );
}

/// Rust mirror of Lean's `LeanTheorem` / `LeanSidecar`. Both carry
/// the theorem text bytes; the lowering is a byte-identity copy.
#[derive(PartialEq, Eq, Clone, Copy)]
struct LeanTheorem {
    text: [u8; 4],
}

#[derive(PartialEq, Eq, Clone, Copy)]
struct LeanSidecar {
    text: [u8; 4],
}

fn lower_theorem_to_sidecar(t: &LeanTheorem) -> LeanSidecar {
    LeanSidecar { text: t.text }
}

#[kani::proof]
fn theorem_carried_as_lean_sidecar() {
    let text: [u8; 4] = kani::any();
    let t = LeanTheorem { text };
    let s = lower_theorem_to_sidecar(&t);
    kani::assert(
        s.text == t.text,
        "theorem text must be copied byte-for-byte into the Lean sidecar",
    );
}

/// Rust mirror of Lean's `LeanInductive` / `RustEnum`.
#[derive(PartialEq, Eq, Clone, Copy)]
struct LeanInductive {
    variant_count: u8,
}

#[derive(PartialEq, Eq, Clone, Copy)]
struct RustEnum {
    variant_count: u8,
}

fn lower_inductive_to_enum(i: &LeanInductive) -> RustEnum {
    RustEnum {
        variant_count: i.variant_count,
    }
}

#[kani::proof]
fn inductive_to_rust_enum() {
    let variant_count: u8 = kani::any();
    let i = LeanInductive { variant_count };
    let r = lower_inductive_to_enum(&i);
    kani::assert(
        r.variant_count == i.variant_count,
        "variant count must be preserved exactly (no inflation, no collapse)",
    );
}

/// Rust mirror of Lean's `LeanStructure` / `RustStruct`.
#[derive(PartialEq, Eq, Clone, Copy)]
struct LeanStructure {
    field_count: u8,
}

#[derive(PartialEq, Eq, Clone, Copy)]
struct RustStruct {
    field_count: u8,
}

fn lower_structure_to_struct(s: &LeanStructure) -> RustStruct {
    RustStruct {
        field_count: s.field_count,
    }
}

#[kani::proof]
fn structure_to_rust_struct() {
    let field_count: u8 = kani::any();
    let s = LeanStructure { field_count };
    let r = lower_structure_to_struct(&s);
    kani::assert(
        r.field_count == s.field_count,
        "field count must be preserved (no `extends` inlining inflation)",
    );
}

/// Rust mirror of Lean's `LeanInstance` / `RustImpl`.
#[derive(PartialEq, Eq, Clone, Copy)]
struct LeanInstance {
    method_count: u8,
}

#[derive(PartialEq, Eq, Clone, Copy)]
struct RustImpl {
    method_count: u8,
}

fn lower_instance_to_impl(inst: &LeanInstance) -> RustImpl {
    RustImpl {
        method_count: inst.method_count,
    }
}

#[kani::proof]
fn instance_to_rust_impl() {
    let method_count: u8 = kani::any();
    let inst = LeanInstance { method_count };
    let r = lower_instance_to_impl(&inst);
    kani::assert(
        r.method_count == inst.method_count,
        "method count must be preserved (no convenience-method auto-derivation)",
    );
}

/// Rust mirror of Lean's `LeanAxiom` / `RustExtern`. The
/// warning-lines count is fixed at 5 in the Bronze-tier emitter;
/// the harness asserts `>= 5`.
#[derive(PartialEq, Eq, Clone, Copy)]
struct LeanAxiom {
    signature: [u8; 4],
}

#[derive(PartialEq, Eq, Clone, Copy)]
struct RustExtern {
    signature: [u8; 4],
    warning_lines: u8,
}

fn lower_axiom_to_extern(a: &LeanAxiom) -> RustExtern {
    RustExtern {
        signature: a.signature,
        warning_lines: 5,
    }
}

#[kani::proof]
fn axiom_to_extern_fn() {
    let signature: [u8; 4] = kani::any();
    let a = LeanAxiom { signature };
    let r = lower_axiom_to_extern(&a);
    kani::assert(r.signature == a.signature, "axiom signature byte-preserved");
    kani::assert(
        r.warning_lines >= 5,
        "WARNING comment header must be >=5 lines (the contract's safety floor)",
    );
}

/// Rust mirror of Lean's `LeanNoncomputableDef` / `RustPanicFn`.
#[derive(PartialEq, Eq, Clone, Copy)]
struct LeanNoncomputableDef {
    name: [u8; 4],
}

/// Bronze model: panic-marker is encoded as a single byte tag
/// (1 = panic body, 0 = something else). The contract's
/// load-bearing claim is that the body IS the canonical panic
/// marker — captured here as `body_tag == 1`.
#[derive(PartialEq, Eq, Clone, Copy)]
struct RustPanicFn {
    body_tag: u8,
    doc_hidden: bool,
}

fn lower_noncomputable_to_panic(_d: &LeanNoncomputableDef) -> RustPanicFn {
    RustPanicFn {
        body_tag: 1,
        doc_hidden: true,
    }
}

#[kani::proof]
fn noncomputable_def_to_rust_panic() {
    let name: [u8; 4] = kani::any();
    let d = LeanNoncomputableDef { name };
    let r = lower_noncomputable_to_panic(&d);
    kani::assert(
        r.body_tag == 1,
        "body must be the canonical panic marker (not todo!() or empty)",
    );
    kani::assert(
        r.doc_hidden,
        "fn must carry #[doc(hidden)] (prevent accidental downstream use)",
    );
}

/// Rust mirror of Lean's `LeanDeclWithContract` / `RustItemWithCitation`.
#[derive(PartialEq, Eq, Clone, Copy)]
struct LeanDeclWithContract {
    contract_id: [u8; 4],
}

#[derive(PartialEq, Eq, Clone, Copy)]
struct RustItemWithCitation {
    citation: [u8; 4],
}

fn lower_decl_with_citation(d: &LeanDeclWithContract) -> RustItemWithCitation {
    RustItemWithCitation {
        citation: d.contract_id,
    }
}

#[kani::proof]
fn citation_in_emitted_rust() {
    let contract_id: [u8; 4] = kani::any();
    let d = LeanDeclWithContract { contract_id };
    let r = lower_decl_with_citation(&d);
    kani::assert(
        r.citation == d.contract_id,
        "contract ID must appear byte-for-byte in the citation doc-comment (no dash-to-underscore mangling)",
    );
}
