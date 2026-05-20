# Adding a frontend

> **Governing contract:** [`C-XPILE-FRONTEND-TRAIT`](../reference/contracts.md#c-xpile-frontend-trait)
> — read this first. The trait's structural invariants are what your
> implementation must satisfy.

This walks through what it takes to add a new code-lane frontend to
xpile. See [Adding a backend](adding-a-backend.md) for the
emit-side flow; if you're adding a proof-lane frontend the broad
strokes are the same but you'd implement `ContractFrontend` instead.

## 1. Scope the language subset

A frontend never has to handle "all of language X." It handles a
subset, and the subset is **the contract**. So before you write any
parser code:

1. Identify which xpile-meta-HIR constructs your language needs to
   lower into. (Likely: function decls, primitive arithmetic, control
   flow — the same set that depyler-frontend handles for Python.)
2. Write the subset YAML in `contracts/`. Pattern it after
   `contracts/bashrs-posix-idempotence-v1.yaml`.
3. Run `pv lint contracts/` — your YAML must pass before you start
   writing Rust.

## 2. Scaffold the crate

```bash
$ cargo new --lib crates/my-frontend
```

Register it in the workspace `Cargo.toml`:

```toml
members = [
    ...
    "crates/my-frontend",
]

[workspace.dependencies]
...
my-frontend = { version = "0.1.0", path = "crates/my-frontend" }
```

In the crate's `Cargo.toml`:

```toml
[dependencies]
xpile-frontend = { workspace = true }
xpile-meta-hir = { workspace = true }
thiserror      = "1"
```

## 3. Implement the trait

```rust
use xpile_frontend::{Frontend, FrontendError};
use xpile_meta_hir::MetaHirModule;

pub struct MyFrontend;

impl Frontend for MyFrontend {
    fn name(&self) -> &str { "mylang" }

    fn extensions(&self) -> &[&str] { &["myl"] }

    fn parse_str(&self, source: &str, module_name: &str)
        -> Result<MetaHirModule, FrontendError>
    {
        // parse source → lower to meta-HIR
        todo!()
    }
}
```

Determinism is **non-negotiable** — the trait contract requires
identical inputs to produce identical `MetaHirModule` outputs. Test
this directly: parse the same input twice, assert equality.

## 4. Register in `xpile-core::default_session`

Wire the frontend into the default registry. Look at how
`bashrs-frontend` and `depyler-frontend` are registered in
`crates/xpile-core/src/lib.rs::default_session()`.

## 5. Write the tests

Three layers of testing are expected:

| Layer | What it does | Where it lives |
|---|---|---|
| Unit | Parse fragments, assert AST shape | `crates/my-frontend/src/lib.rs` `#[cfg(test)]` |
| Determinism | Same input twice → same output | a property test in the same file |
| Integration | Real source files round-trip end-to-end | `tests/fixtures/my-lang-*.myl` + `tests/transpile_e2e.rs` |

## 6. Lift the contract

Once the frontend compiles and the basic round-trip works, lift the
substrate:

1. Write Lean theorems for each YAML equation (in
   `contracts/lean/MyLangIdempotence.lean` or similar).
2. Write Kani harnesses (in `contracts/kani/my_lang_idempotence.rs`).
3. Register both in the YAML's `stratum_votes:` block.
4. Run `xpile quorum` — your contract should reach QUORUM.

## 7. Run the quality gate

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
pv lint contracts/
cargo deny check advisories
```

All five must pass before pushing. Open a PR; CI enforces the same
gate as a required status check.

## 8. Add a Diamond category

Once the contract is at QUORUM, add a structural-extensionality
Diamond theorem to push the substrate towards UNIVERSAL coverage at
the next depth. See [`docs/specifications/sub/diamond-taxonomy.md`](https://github.com/paiml/xpile/blob/main/docs/specifications/sub/diamond-taxonomy.md)
for the template families.
