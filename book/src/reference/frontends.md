# Frontends

> **Governing contract:** [`C-XPILE-FRONTEND-TRAIT`](contracts.md#c-xpile-frontend-trait)
> — Layer 3 (architectural), code lane, kind: pattern. Every frontend
> implements this trait; the contract pins down determinism (same
> input → same `MetaHirModule`) and the citation invariants the parsed
> module must carry.

A frontend reads a source file and lowers it to xpile's canonical
**meta-HIR**. Frontends never see other frontends; they all funnel
through meta-HIR.

## Status

`xpile info` prints this table live from the registry the CLI actually
dispatches through — prefer it to this page.

The **Name** column is the key `xpile info` prints, not a display label.

| Frontend | Name | Extensions | Status | Crate |
|---|---|---|---|---|
| Python | `python` | `.py`, `.pyi` | ✅ **Real parser** | `depyler-frontend` |
| C      | `c` | `.c`, `.h` | ✅ **Real parser** | `decy-frontend` |
| Shell  | `bashrs` | `.sh`, `.bash`, `.zsh`, `.mk` | ✅ **Real POSIX parser** | `bashrs-frontend` |
| WASM   | `wasm` | `.wat` | ✅ **Real parser** (lossy lift) | `xpile-wasm-frontend` |
| Ruchy  | `ruchy` | `.ruchy` | ⛔ **Routing only — refuses every input** | `ruchy-frontend` |

The proof lane registers one contract frontend, LaTeX math
(`latex-contract-frontend`), which reads contract sources rather than
programs.

**Ruchy is registered but has no parser.** It exists so that a
`.ruchy` input gets a named refusal instead of a generic "no frontend
handles this extension" — `xpile transpile x.ruchy --target rust`
exits non-zero with a reason. It does *not* mean Ruchy input works;
Ruchy is a fully supported **output** target (see
[backends](backends.md)). Nothing here silently returns an empty
module: `crates/xpile/tests/claims_drift.rs` runs every registered
frontend against a real program in its own language and fails if one
answers with `Ok(Module { items: [] })`.

**There is no C++, Rust, or Lean 4 frontend.** This page claimed all
three as "planned"/"scaffold" workspace members; no such crate exists
and none is registered. Lean 4 and LaTeX appear in the *proof* lane and
Rust appears as a *backend*, which is where the confusion came from.

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
