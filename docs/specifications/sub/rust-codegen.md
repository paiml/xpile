# Rust Codegen Backend

**Section 5 of [xpile-spec.md](../xpile-spec.md).**

## Responsibility

`xpile-rust-codegen` takes meta-HIR as input and emits idiomatic Rust. It is **language-neutral by design** — every language-specific quirk is normalized in the frontend before reaching codegen.

| Language quirk | Normalized in frontend | Codegen treats as |
|---|---|---|
| Python int promotion (i64 → BigInt) | Layer-1 contract `py-int-arith` → semantic op | Generic `IntegerAdd { promotable: bool }` |
| C pointer arithmetic | Layer-1 contract `c-pointer-arith` → bounded offset | Generic `IndexedAccess { bounds_checked: bool }` |
| Ruchy pipeline operator `\|>` | Frontend lowers to method-chain | Standard method calls |
| Python list ref semantics | Frontend annotates with alias graph | Generic `AliasingVec<T>` |

## v0.1.0 API

```rust
pub fn emit_module(module: &Module) -> Result<String, CodegenError>;
```

Stub returns a single-line comment: `// xpile-generated from <SourceLang> module <name> — TODO`. Real emission is driven by per-construct translation contracts (Layer 2 in the [contract taxonomy](contract-taxonomy.md)).

## Contract-driven emission

For each Layer-2 translation contract, `pv scaffold` generates an emit function:

```rust
// Generated from contracts/xlate-py-list-to-vec-v1.yaml
pub fn emit_py_list_literal(items: &[HirExpr]) -> EmittedCode { ... }
```

Hand-edits to generated code are reverted on the next `pv scaffold` run; the source of truth is the contract.

## Provenance markers

For files that *don't* go through the repair agent (i.e., pure static emission), no provenance marker is added. The first line is empty or contains a normal comment.

For repair-pass files, the marker is added by `xpile-agent`, not by codegen. Codegen never adds markers.

## Style and lints

Generated Rust must pass `cargo clippy -- -D warnings` and `cargo fmt --check` with no manual fixups. This is a CI gate. Style problems in generated Rust are codegen bugs.

## What codegen does NOT do

- It does NOT decide language semantics. Those are Layer-1 contracts that frontends apply.
- It does NOT generate FFI shims. Those come from the FFI manifest plus shim templates in `xpile-rust-codegen/templates/`.
- It does NOT decide error-handling strategy. Each translation contract specifies its own (`Result<_, E>`, panics, or `Option<_>`).
