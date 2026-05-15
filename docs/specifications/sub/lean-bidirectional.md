# Lean 4 Bidirectional Integration

**Section 24 of [xpile-spec.md](../xpile-spec.md).** Lane-level overview of Lean 4's role across both pipelines. For trait-level detail see [backend-trait.md](backend-trait.md), [contract-frontend-trait.md](contract-frontend-trait.md), and [contract-backend-trait.md](contract-backend-trait.md).

## Why Lean spans both lanes

Lean 4 is the only language in xpile that participates in **both** the code lane and the proof lane. The reason is that a `.lean` file carries two genuinely distinct kinds of declarations:

- **Executable declarations** — `def`, `partial def`, `inductive`, `structure`, `instance`, `axiom`, `noncomputable def`. These describe runtime behavior and are the code-lane's concern.
- **Proof declarations** — `theorem`, `lemma`, `example`. These describe logical facts about other declarations. They have no runtime semantics and are the proof-lane's concern.

A single `.lean` file produced by xpile can carry both, separated by section markers. `TranspileSession` orchestrates the merge — `xpile-lean-codegen` (a `Backend`) writes the executable part; `xpile-lean-contract-backend` (a `ContractBackend`) writes the proof part.

## Lean 4 only

Lean 3 is end-of-life; Mathlib has fully migrated. xpile does not target Lean 3 even read-only. `ContractRenderConfig::lean_version` only accepts `Some((4, _))`; Lean 3 returns `ContractBackendError::UnsupportedLeanVersion`. Locked 2026-05-15.

## Code lane: Lean ↔ Rust

### Lean → Rust (`lean-frontend` + `xpile-rust-codegen`)

`lean-frontend` parses `.lean` files for executable declarations and lowers them into meta-HIR. All Lean constructs are in scope per the 2026-05-15 decision; the Layer 2 translation contract `contracts/xlate-lean-to-rust-v1.yaml` specifies how each one maps:

| Lean construct | Rust translation |
|---|---|
| `def f : T := body` | `fn f() -> T_rust { body_rust }` |
| `partial def f : T := body` | `#[partial_translation] fn f() -> Result<T_rust, NonTermination>` |
| `inductive T \| A \| B(x : Nat)` | `enum T { A, B(u64) }` |
| `structure S where (a : A)` | `struct S { a: A_rust }` |
| `instance : C T where method := body` | `impl C for T { fn method() { body_rust } }` |
| `axiom name : T` | `unsafe extern "Rust" { fn name() -> T_rust; }` + 5-line warning |
| `noncomputable def f := body` | `fn f() -> R_rust { panic!("noncomputable") }` |
| `theorem t : P := proof` | Preserved as a Lean sidecar; NOT lowered to Rust |

Theorems are the boundary between lanes: when `lean-frontend` encounters one, it does NOT lower it to Rust. The theorem stays in the proof-lane artifact and is carried alongside the emitted Rust as a sidecar `.lean` file. If a downstream `def` references a theorem (e.g., `theorem_of_correctness`), the emitted Rust gets a doc-comment pointing at the Lean sidecar.

### Rust → Lean (`rust-frontend` + `xpile-lean-codegen`)

`rust-frontend` is the keystone for bidirectional translation generally (also unlocks Rust→PTX, Rust→Ruchy). For Lean, the path is: Rust source → meta-HIR → Lean 4 executable. `xpile-lean-codegen` emits `def` / `inductive` / `structure` / `instance` declarations. The Layer 2 contract for this direction is reserved as `contracts/xlate-rust-fn-to-lean-def-v1.yaml` (planned).

This direction is **lossier**: Rust has features Lean 4 does not (lifetimes, borrowing as a type-system concept, traits with associated types and GATs). The contract specifies what's lifted faithfully vs. what's emitted with a `sorry` placeholder or skipped with a `-- WARNING: unsupported Rust feature` comment.

## Proof lane: contracts ↔ Lean theorems

### Contract → Lean theorem (`xpile-lean-contract-backend`)

`xpile-lean-contract-backend` renders any parsed `Contract` as a Lean 4 theorem text. Every emitted theorem carries the citation-bridge attribute:

```lean
import XpileContracts.Attr

@[xpile_contract "C-XLATE-PY-LIST-TO-VEC", xpile_equation "homogeneous_list_to_vec"]
theorem homogeneous_list_to_vec
    {T : Type} [HasRustEquiv T]
    (xs : List T)
    : xlate xs = .ok (Vec.ofList (xs.map toRust)) := by
  ...
```

The `@[xpile_contract]` attribute is parsed by Lean's elaborator — malformed citation fails at compile time. The contract ID is preserved **verbatim** (no dash-to-underscore mangling). Audit tooling extracts citations via the `Lean.Meta` API, not regex. The Layer 2 contract specifying this transformation is `contracts/xlate-rust-fn-to-lean-thm-v1.yaml`.

A small Lean preamble library `XpileContracts.Attr` defines the attribute. Every xpile-generated Lean file `import`s it. The preamble is vendored as a sidecar artifact by `xpile-lean-contract-backend`.

### Lean theorem → Contract (`lean-contract-frontend`, read-only)

`lean-contract-frontend` is read-only — xpile does not synthesize contract YAML from arbitrary Lean theorem text, because Lean theorems are higher-fidelity than YAML equations (they carry proof terms; YAML equations are statements only). When a contract is bootstrapped from an existing Lean library, the frontend extracts `theorem` and `lemma` declarations into `EquationsBlock.proof_obligations` using Lean's parser, NOT regex.

## A Lean file with both lanes

Below is the canonical pattern for a `.lean` file that `TranspileSession` emits when both lanes target Lean:

```lean
import XpileContracts.Attr

section CodeLane
-- emitted by xpile-lean-codegen from meta-HIR
def foo (x : Nat) : Nat := x + 1
end CodeLane

-- emitted by xpile-lean-contract-backend from contract YAML
@[xpile_contract "C-XLATE-PY-INT-ARITH", xpile_equation "addition_no_overflow"]
theorem addition_no_overflow : ∀ x, foo x = x + 1 := by
  intro x; rfl
```

The two halves never share state — each lane's renderer operates independently and `TranspileSession` concatenates with the section/end markers.

## Lifecycle and tooling

- **Build:** `lake build` (Lean's standard build tool). xpile emits a `lakefile.lean` sidecar when the project uses multiple Lean files.
- **Check:** `lake env lean --check <file>` validates type-checking. Every emitted theorem must pass this gate before xpile ships the artifact.
- **Attribute introspection:** `lake env lean --print-attributes <file>` lists every `xpile_contract` attribute; CI walks this list to verify citation chain coverage.
- **Mathlib:** xpile-emitted Lean does not depend on Mathlib at v0.1.0 to keep build times low. Contracts that require Mathlib-only lemmas mark themselves with `xpile.requires_mathlib: true` (planned `pv` extension).

## Open issues

1. **Termination measures for `partial def` ↔ Rust `Result<_, NonTermination>`** — the iteration budget is a runtime check; Lean's termination logic is static. The contract acknowledges this asymmetry but a future Layer 1.5 contract may add static bounds where decidable.
2. **GATs and HKT** — Rust GATs translate poorly to Lean 4; the planned `rust-frontend` will emit `sorry` placeholders for unhandled cases, flagged by contract.
3. **Mathlib gating** — when xpile's emitted theorems start referencing classical results, the `lake build` time becomes load-bearing. Future work: a contracts-only subset of Mathlib vendored under `XpileContracts.Math`.

## See also

- [backend-trait.md](backend-trait.md) — the `Backend` trait Lean-codegen implements
- [contract-backend-trait.md](contract-backend-trait.md) — the `ContractBackend` trait the Lean theorem renderer implements, including the citation bridge convention
- [`contracts/xlate-lean-to-rust-v1.yaml`](../../../contracts/xlate-lean-to-rust-v1.yaml) — Layer 2 code-lane contract
- [`contracts/xlate-rust-fn-to-lean-thm-v1.yaml`](../../../contracts/xlate-rust-fn-to-lean-thm-v1.yaml) — Layer 2 proof-lane contract
