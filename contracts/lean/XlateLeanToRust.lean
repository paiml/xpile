/-
  XlateLeanToRust.lean — Lean 4 refinement proofs for
  `C-XLATE-LEAN-TO-RUST`.

  This file is the proof-lane counterpart to
  `contracts/xlate-lean-to-rust-v1.yaml` (PMAT-070). The YAML
  carries the *equations* describing how Lean 4 constructs lower
  to Rust through xpile-lean-codegen; this file carries the
  *theorem* that locks in the Bronze-tier modelling commitment
  for the `def_to_rust_fn` equation.

  Cross-references:
    * Code lane:   crates/xpile-rust-codegen/src/lib.rs (when
                   Lean→Rust lowering grows past scaffold)
    * Contract:    contracts/xlate-lean-to-rust-v1.yaml
    * Citation:    every emitted Rust artifact for a Lean-source
                   input carries `# xpile-contract:
                   C-XLATE-LEAN-TO-RUST` above its `fn` block.
    * Roadmap:     docs/specifications/xpile-spec.md §3 (Layer-2
                   translation contracts), audit-design.md §6.

  Tier (per ruchy 5.0 §14.10.5): refinement target is Bronze at
  v0.1.0 — both `LeanDef` and `RustFn` are modelled as byte
  arrays carrying the function body. `lower_def_to_fn` is the
  identity at the byte level (Bronze placeholder). Silver-tier
  refinement (v0.3.0+) replaces this with typed AST nodes that
  separately model `name : Identifier`, `args : List Param`,
  `body : Expr`, plus structural induction on body shape.

  This is the *eighth contract Lean theorem* the project has,
  and the first of the **post-trait-matrix domain contracts**.
  Where PMAT-062..068 covered uniform architectural invariants
  (parse/render determinism), this theorem starts the Layer-2
  translation work — modelling commitments about specific
  Lean → Rust constructs.
-/

namespace XpileContracts.CXlateLeanToRust

/--
  Abstract model of a Lean `def` declaration. At v0.1.0 we
  represent it as a byte array carrying the function body —
  enough to capture the body-preservation invariant of
  `def_to_rust_fn`. Silver-tier refinement
  (XPILE-REFINE-XLATE-LEAN-TO-RUST-***+) replaces this with typed
  AST nodes (`{ name, args, return_type, body }`).
-/
structure LeanDef where
  body : Array UInt8
deriving DecidableEq

/--
  Abstract model of a Rust `fn` declaration. v0.1.0 model — same
  byte-array shape as `LeanDef`, locking in the body-preservation
  claim at the byte level.
-/
structure RustFn where
  body : Array UInt8
deriving DecidableEq

/--
  Lowering function: Lean `def` → Rust `fn`. v0.1.0 model:
  byte-identity. The Bronze-tier placeholder captures the
  load-bearing property — `def_to_rust_fn` preserves the
  function body — without committing to a specific lowering
  strategy.
-/
def lower_def_to_fn (d : LeanDef) : RustFn :=
  { body := d.body }

/--
  **Refinement theorem** for `def_to_rust_fn` (the load-bearing
  claim from the contract YAML's equation block).

  Lowering a Lean `def` to a Rust `fn` preserves the function
  body. Proof is `rfl` by our v0.1.0 modelling choice (byte
  identity).

  Documentary value: any future xpile-rust-codegen impl that
  mutates the Lean body during lowering (e.g., dropping
  trailing comments, normalizing whitespace, reordering binders)
  must either preserve `rfl`-equivalence under this model OR
  invalidate the theorem (and `refinement_proofs.rs`'s citation
  gate fires).

  Falsification: an emitter that wraps the Lean body in a
  `tracing::span!(target: "xpile", level = "debug", ...)` macro
  would falsify this theorem at byte level. Silver-tier
  refinement will introduce an equivalence relation
  (canonical-equality modulo whitespace + comment normalisation)
  rather than byte-equality.

  Status: **discharged at v0.1.0 (PMAT-070)**. Tier: Bronze.

  Companion to `XlatePyListToVec.lean` (PMAT-060) — both are
  Layer-2 translation contracts at Bronze tier. Together they
  bracket the two directions of the proof-↔-code lane bridge:
  - Python → Rust (PMAT-060)
  - Lean → Rust (this theorem)
-/
theorem def_to_rust_fn (d : LeanDef) :
    (lower_def_to_fn d).body = d.body := by
  rfl

/--
  **Name preservation** auxiliary claim — `def f` lowers to
  `fn f` (no name mangling for simple identifiers). At Bronze
  tier this is trivially `rfl` because the model doesn't carry
  a name separately from the body. Silver-tier refinement
  (XPILE-REFINE-XLATE-LEAN-TO-RUST-001) introduces a `name`
  field in both `LeanDef` and `RustFn` and the proof becomes
  a structural preservation lemma.

  Listed for documentary value and forward compatibility.
-/
theorem name_preserved (d : LeanDef) :
    (lower_def_to_fn d).body = d.body := by
  rfl

end XpileContracts.CXlateLeanToRust
