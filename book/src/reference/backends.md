# Backends

> **Governing contract:** [`C-XPILE-BACKEND-TRAIT`](contracts.md#c-xpile-backend-trait)
> — Layer 3 (architectural), code lane, kind: pattern. Every backend
> implements this trait. The contract pins down structural emit
> invariants: every emitted artifact must carry a
> `// xpile-contract: <ID>` citation, error paths must name the
> governing contract, and unsupported constructs must fail cleanly
> with a target-suggestion message.

A backend reads a `MetaHirModule` and emits an artifact in some target
language. Backends never see other backends; they all read from
meta-HIR.

## Status

`xpile info` prints this table live from the `Target` enum the CLI
actually dispatches through — prefer it to this page.

| Backend | `--target` | Status | Crate |
|---|---|---|---|
| Rust    | `rust` | ✅ **Real emission** (Python-floor semantics, `.checked_*()` for `C-PY-INT-ARITH`) | `xpile-rust-codegen` |
| Ruchy   | `ruchy` | ✅ **Real emission** (same overflow semantics; compiles to Rust) | `xpile-ruchy-codegen` |
| Lean 4  | `lean` | ✅ **Real emission** (`def`, `Int.fdiv`/`Int.fmod`; `Int` is unbounded) | `xpile-lean-codegen` |
| Shell   | `bashrs` | ✅ **Real emission** (round-trip with bashrs-frontend) | `bashrs-backend` |
| WASM    | `wasm` | ✅ **Real emission** (WebAssembly text; assembled and executed in CI) | `xpile-wasm-codegen` |
| PTX     | `ptx` | ✅ **Real emission** — `--hardware ptx:<sm_XX>` is **required** to reach it | `xpile-ptx-codegen` |
| WGSL    | `wgsl` | ✅ **Real emission** (scalar subset) | `xpile-wgsl-codegen` |
| SPIR-V  | `spirv` | ✅ **Real emission** (scalar subset) | `xpile-spirv-codegen` |
| forjar  | `forjar` | ✅ **Real emission** from **shell-origin** modules; refuses Python-origin input with a reason | `xpile-forjar-codegen` |

The proof lane registers two contract backends, `lean-theorem` and
`latex`, which render contract YAML rather than programs.

A ✅ here means "emits for its supported subset", not "emits for every
program" — each backend refuses constructs outside its subset with a
message naming the governing contract and, where one exists, a better
`--target`. That refusal *is* the guarantee; see the
[shell round-trip tutorial](../tutorials/shell-roundtrip.md) for a
worked example of the message.

## Rust backend — what's emitted

The Rust backend produces:

- `pub fn` declarations with typed parameters and typed returns
- All binary + unary operators using Python semantics:
  - `//` → `checked_div_euclid` (Python-floor, not C-truncating)
  - `%` → `checked_rem_euclid`
  - `*`, `+`, `-` → `checked_mul`, `checked_add`, `checked_sub`
- `.expect("…contract C-PY-INT-ARITH slow path…")` on every arithmetic
  wrap — the panic text **names the contract**
- A `// xpile-contract: <ID>` citation above each emitted function

The semantics-preserving choice of `checked_div_euclid` over the
sloppy `/` operator is what discharges Layer-1 of `C-PY-INT-ARITH`:
Python `7 // -2 == -4`, not `-3`. The Rust default would be wrong; the
backend's choice is **right by construction**.

## Lean 4 backend — what's emitted

The Lean backend produces:

- `def` declarations with `Int`/`Nat`/typed parameters
- `Int.fdiv` and `Int.fmod` for `//` and `%`
- a `/-- xpile-contract: <ID>[, <ID>]* -/` docstring above each emitted
  definition (one comma-separated docstring, because Lean permits at
  most one per declaration). Through v0.1.617 this was an
  `@[xpile_contract "<ID>"]` attribute, which no Lean prelude registers
  and which therefore made the default emit unparseable — PMAT-1405
  replaced it, and `crates/xpile/tests/lean_default_emit_witness.rs`
  now runs `lean` on the default emit rather than asserting about it.

Because Lean's `Int` is unbounded, `C-PY-INT-ARITH` is satisfied **by
construction** — no overflow checks are needed. The emitted Lean is
typically the most concise emit xpile produces.

## Shell backend — what's emitted

The bashrs backend produces:

- A `#!/bin/sh` shebang (normalised to the supported POSIX dialect)
- A `# xpile-bashrs-backend (v0.1.0 ...)` provenance comment
- A `# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE` citation
- One emitted `Cmd` statement per source command

See [shell-roundtrip tutorial](../tutorials/shell-roundtrip.md) for
real output.

## Calling a backend as a library

```rust
use xpile_rust_codegen::RustBackend;
use xpile_backend::Backend;
use xpile_meta_hir::MetaHirModule;

let backend = RustBackend::default();
let emitted = backend.emit_module(&module)?;
// `emitted` is a `xpile_backend::EmittedArtifact`
```

The `Backend` trait surface is intentionally minimal — see
[Adding a backend](../contributing/adding-a-backend.md) for the full
implementation guide.

## Error handling

When a backend cannot lower a particular construct it must fail with
an error message that:

1. Names the construct (`Stmt::Cmd`, `Expr::AwaitYield`, etc.).
2. Names the governing contract (e.g. `C-BASHRS-POSIX-IDEMPOTENCE`).
3. Suggests the correct target if one exists (`use --target shell`).

This is the `C-XPILE-BACKEND-TRAIT` "structural compile-contract
citation" invariant — see the [contract reference](contracts.md#c-xpile-backend-trait).
