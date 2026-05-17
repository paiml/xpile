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
pub struct RustBackend; // implements xpile-backend::Backend
```

**Real emission shipped** (PR #6 MVP, expanded by #11/#12/#13/#15/#19/#20/#21). Emits idiomatic Rust for every meta-HIR construct in the v0.1.0 subset:

- `Function` → `pub fn name(params: T, ...) -> R { ... }`
- `Block` → `{ stmts; trailing_return }` (no `return` keyword needed)
- `Stmt::Let` → `let name: T = value;`
- `Expr::BinOp`:
  - Arithmetic (`+ - * // %`) → `.checked_*().expect("xpile: ... C-PY-INT-ARITH slow path ...")` so i64 overflow panics with a pointer to the unimplemented bigint promotion path (PR #23, contract `py-int-arith`). Floor-div and mod use the Euclidean variants (`checked_div_euclid` / `checked_rem_euclid`) to preserve Python-floor semantics on negative operands.
  - Comparisons (`== != < <= > >=`) and logical (`&& ||`) → infix (no overflow risk).
- `Expr::UnOp`:
  - `-x` → `.checked_neg().expect(...)` (same contract — `i64::MIN.checked_neg() == None`).
  - `not x` → `(!x)`.
- `Expr::IfExpr` → `if cond { a } else { b }`, flattened to `else if` for nested chains (PR #21)
- `Expr::Call` → `callee(args, ...)`

Verified by 11+ runtime-executed fixtures: `factorial`, `fib`, `gcd`, `abs_val`, `sign`, `bits`, `square_plus`, `range_size`, `sum_to`, `for_sum` / `range_with_start` / `range_with_step`, `factorial_iter`, plus the BigInt-mode variants `bigint_factorial` / `bigint_bits`. Each fixture: emit → `rustc -O` → run → `assert_eq!`. See `crates/xpile/tests/transpile_e2e.rs`. The canonical fixture list lives in [`/CHANGELOG.md`](../../../CHANGELOG.md) under §"Python subset (live, runtime-verified)" to avoid drift.

## Contract-driven emission (planned)

The v0.1.0 emission is **hand-written**. The contract-driven flow (each Layer-2 translation contract scaffolds an emit function via `pv scaffold`) is the eventual target — see [phased-rollout.md](phased-rollout.md) Phase 3.

```rust
// Generated from contracts/xlate-py-list-to-vec-v1.yaml (future)
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
