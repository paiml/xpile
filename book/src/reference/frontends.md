# Frontends

> **Governing contract:** [`C-XPILE-FRONTEND-TRAIT`](contracts.md#c-xpile-frontend-trait)
> — Layer 3 (architectural), code lane, kind: pattern. Every frontend
> implements this trait; the contract pins down determinism (same
> input → same `MetaHirModule`) and the citation invariants the parsed
> module must carry.

A frontend reads a source file and lowers it to xpile's canonical
**meta-HIR**. Frontends never see other frontends; they all funnel
through meta-HIR.

## Status at v0.1.0

| Frontend | Extensions | Status | Crate |
|---|---|---|---|
| Python | `.py`, `.pyi` | ✅ **Real parser** | `depyler-frontend` |
| Shell  | `.sh`, `.bash`, `.zsh`, `.mk` | ✅ **Real POSIX parser** | `bashrs-frontend` |
| C      | `.c`, `.h` | 🚧 Scaffold | `decy-frontend` |
| Ruchy  | `.ruchy` | 🚧 Scaffold | `ruchy-frontend` |

Two more frontends ship as workspace members but live in the
`xpile-frontend` shared trait crate:

- C++ — planned
- Rust — scaffold
- Lean 4 — scaffold (sub-spec: [`sub/lean-bidirectional.md`](https://github.com/paiml/xpile/blob/main/docs/specifications/sub/lean-bidirectional.md))

## Python frontend — what's supported

The depyler frontend is the deepest at v0.1.0. The supported subset
includes:

- typed `def` functions
- multi-statement function bodies
- all binary operators (with Python-floor semantics for `//` and `%`)
- all unary operators
- ternary expressions
- `if`/`elif`/`else` chains
- function calls including self-recursion

Each emitted Rust/Ruchy/Lean function carries a
`// xpile-contract: C-PY-INT-ARITH` citation for the arithmetic
contract.

For the full list, see the
[CHANGELOG `Python subset (live, runtime-verified)`](https://github.com/paiml/xpile/blob/main/CHANGELOG.md)
section — that's the canonical list to avoid duplication-and-drift.

## Shell frontend — what's supported

The bashrs frontend parses a POSIX-shell subset sufficient for realistic
build scripts:

- shebang normalisation
- variable assignment + expansion
- conditional file creation (`mkdir -p`, `> file`, `>> file`)
- pipelines
- `if`/`elif`/`else`
- `for` loops over expansions
- subprocess invocation (`cmd arg1 arg2`)

The supported set is locked in by the
[`C-BASHRS-POSIX-IDEMPOTENCE`](contracts.md#c-bashrs-posix-idempotence)
contract. See the [shell-roundtrip tutorial](../tutorials/shell-roundtrip.md)
for an end-to-end example.

## Calling a frontend as a library

```rust
use depyler_frontend::DepylerFrontend;
use xpile_frontend::Frontend;

let frontend = DepylerFrontend::default();
let module = frontend.parse_file("factorial.py")?;
// `module` is a `xpile_meta_hir::MetaHirModule`
```

The `Frontend` trait surface is intentionally minimal — see
[Adding a frontend](../contributing/adding-a-frontend.md) for the full
implementation guide.
