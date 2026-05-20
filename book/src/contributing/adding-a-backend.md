# Adding a backend

> **Governing contract:** [`C-XPILE-BACKEND-TRAIT`](../reference/contracts.md#c-xpile-backend-trait)
> — read this first. The trait's emit invariants (citation requirement,
> error-path-names-the-contract requirement, target-suggestion on
> unsupported constructs) are what your implementation must satisfy.

This walks through what it takes to add a new code-lane backend to
xpile. See [Adding a frontend](adding-a-frontend.md) for the read-side
flow; if you're adding a proof-lane backend the broad strokes are the
same but you'd implement `ContractBackend` instead.

## 1. Scope what your backend will and won't emit

A backend doesn't have to handle every construct in meta-HIR. It does
have to **fail cleanly** on the ones it can't. So:

1. Pick the meta-HIR subset you'll lower.
2. For each unhandled construct, plan the error message — name the
   governing contract, suggest the right target.

The `xpile-rust-codegen` crate is the reference implementation; the
shell rejection error
(see [shell-roundtrip tutorial](../tutorials/shell-roundtrip.md))
is what your error path should look like.

## 2. Scaffold the crate

```bash
$ cargo new --lib crates/xpile-mylang-codegen
```

Register in the workspace `Cargo.toml`:

```toml
members = [
    ...
    "crates/xpile-mylang-codegen",
]

[workspace.dependencies]
...
xpile-mylang-codegen = { version = "0.1.0", path = "crates/xpile-mylang-codegen" }
```

In the crate's `Cargo.toml`:

```toml
[dependencies]
xpile-backend  = { workspace = true }
xpile-meta-hir = { workspace = true }
thiserror      = "1"
```

## 3. Implement the trait

```rust
use xpile_backend::{Backend, BackendError, EmittedArtifact};
use xpile_meta_hir::MetaHirModule;

pub struct MyLangBackend;

impl Backend for MyLangBackend {
    fn name(&self) -> &str { "mylang" }

    fn target_label(&self) -> &str { "MyLang" }

    fn emit_module(&self, module: &MetaHirModule)
        -> Result<EmittedArtifact, BackendError>
    {
        // 1. Begin with a provenance comment naming the backend +
        //    governing contract.
        // 2. Emit a `// xpile-contract: <ID>` citation per function.
        // 3. Lower meta-HIR statements.
        // 4. On unsupported constructs, return an error naming the
        //    construct, the governing contract, and the suggested
        //    target.
        todo!()
    }
}
```

The citation requirement is **non-negotiable**. The CI gate
`crates/xpile/tests/qa_gate.rs` parses every emitted artifact and
fails if a citation is missing.

## 4. Register in `xpile-core::default_session`

Wire the backend into the default registry. Look at how
`xpile-rust-codegen` and `xpile-ruchy-codegen` are registered in
`crates/xpile-core/src/lib.rs::default_session()`.

## 5. Decide how to discharge `C-PY-INT-ARITH`

This contract governs arithmetic. Your backend's choice:

- **By construction** (target has unbounded integers — like Lean's
  `Int`): emit native arithmetic, no overflow checks.
- **By checked-op + slow path** (target has fixed-width integers —
  like Rust's `i64`): emit `.checked_*().expect("…C-PY-INT-ARITH slow
  path…")` calls.
- **By compile-time guarantee** (target has refinement types or
  unbounded compile-time constants — like SPIR-V `OpConstant`):
  document why and how.

Whichever you pick, the panic/error text must name the contract.

## 6. Write the tests

| Layer | What it does | Where it lives |
|---|---|---|
| Unit | Emit fragments, assert text shape | `crates/xpile-mylang-codegen/src/lib.rs` `#[cfg(test)]` |
| Determinism | Same input twice → same output | a property test in the same file |
| Integration | Real fixtures emit + (if applicable) compile | `tests/transpile_e2e.rs` |
| Citation | Every emit carries `// xpile-contract: <ID>` | `tests/qa_gate.rs` (already enforced) |

## 7. Lift the contract

If your backend introduces a new construct or boundary, write a
Layer-2 translation contract for it. Pattern after
`xlate-py-list-to-vec-v1.yaml`. Then add Lean theorems and Kani
harnesses; run `xpile quorum` until it reports QUORUM.

## 8. Run the quality gate

Same as for frontends:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
pv lint contracts/
cargo deny check advisories
```

## 9. Update the book

If your backend is meaningfully new (a new target language, not just
an internal improvement), add a row to the
[Reference: backends](../reference/backends.md) table and link a
tutorial.
