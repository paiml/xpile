# Backends

> **Governing contract:** [`C-XPILE-BACKEND-TRAIT`](contracts.md#c-xpile-backend-trait)
> — Layer 3 (architectural), code lane, kind: pattern. Every backend
> implements this trait. The invariants it pins: `target_ownership`,
> `lower_idempotency`, `target_consistency`, `compile_contract_citation`,
> `frame_lower_is_pure` (plus thirteen Diamond refinements over the same
> records). Every one of them is a property of the SUCCESS path — the
> contract says nothing at all about what a REFUSAL message contains.
> Through v0.1.617 this blockquote claimed it pinned "error paths must
> name the governing contract" and a "target-suggestion message"; it
> pins neither, and five of the nine backends do neither. See the
> measured table below and PMAT-1437.

A backend reads a `xpile_meta_hir::Module` and emits an artifact in some target
language. Backends never see other backends; they all read from
meta-HIR.

## Status

`xpile info` prints this table live from the `Target` enum the CLI
actually dispatches through — prefer it to this page.

**`Name` and `--target` are two different strings, and for one backend
they differ.** `Name` is the registry key `xpile info` prints on the
left of the arrow; `--target` is what you pass on the command line, and
what `parse_target` accepts. They coincide for eight of the nine
backends. They do **not** for the shell backend: `xpile info` prints
`- bashrs → Shell`, and the flag is `--target shell`. Through
v0.1.617 this column published `bashrs`, which the CLI rejects outright
(`unknown target`) — see PMAT-1430.

| Backend | Name | `--target` | Status | Crate |
|---|---|---|---|---|
| Rust    | `rust` | `rust` | ✅ **Real emission** (Python-floor semantics, `.checked_*()` for `C-PY-INT-ARITH`) | `xpile-rust-codegen` |
| Ruchy   | `ruchy` | `ruchy` | ✅ **Real emission** (same overflow semantics; compiles to Rust) | `xpile-ruchy-codegen` |
| Lean 4  | `lean` | `lean` | ✅ **Real emission** (`def`, `Int.fdiv`/`Int.fmod`; `Int` is unbounded) | `xpile-lean-codegen` |
| Shell   | `bashrs` | `shell` | ✅ **Real emission** (round-trip with bashrs-frontend) | `bashrs-backend` |
| WASM    | `wasm` | `wasm` | ✅ **Real emission** (WebAssembly text; assembled and executed in CI) | `xpile-wasm-codegen` |
| PTX     | `ptx` | `ptx` | ✅ **Real emission** — `--hardware ptx:<sm_XX>` is **required** to reach it | `xpile-ptx-codegen` |
| WGSL    | `wgsl` | `wgsl` | ✅ **Real emission** (scalar subset) | `xpile-wgsl-codegen` |
| SPIR-V  | `spirv` | `spirv` | ✅ **Real emission** (scalar subset) | `xpile-spirv-codegen` |
| forjar  | `forjar` | `forjar` | ✅ **Real emission** from **shell-origin** modules; refuses Python-origin input with a reason | `xpile-forjar-codegen` |

`--target` also accepts the aliases `wat` (→ `wasm`), `sh` / `bash`
(→ `shell`) and `forjar-yaml` (→ `forjar`). All four resolve to the
canonical target in the table above and are otherwise indistinguishable
from it. `xpile transpile --help` and the `unknown target` refusal both
name the nine canonical spellings AND the four aliases; through
PMAT-1435 they named only the nine, so this sentence was the only place
in the repo that said so.

The proof lane registers two contract backends, `lean-theorem` and
`latex`. Both are **scaffolds**: each returns a fixed `_scaffold`
payload that no field of the contract can influence, so neither
actually renders contract YAML today. `xpile info` reports them as
`contract_backends (2 registered, 0 rendering)` and tags each one;
`crates/xpile/tests/proof_lane_scaffold_witness.rs` measures the
contract-independence rather than asserting it. Real rendering is
v0.2.0 work — see PMAT-1429.

A ✅ here means "emits for its supported subset", not "emits for every
program" — each backend refuses constructs outside its subset rather
than emitting something wrong. **That refusal, and its exit status, is
the guarantee. What the refusal MESSAGE contains is not.**

Through v0.1.617 this paragraph said the message names "the governing
contract and, where one exists, a better `--target`". It usually names
neither. The table below is **measured**, not asserted:
`crates/xpile/tests/backend_refusal_disclosure_witness.rs`
(XPILE-BACKENDREFUSE-001) runs a fixed seven-program corpus against
every registered backend, keeps the failures that reached that
backend's own `lower()`, and re-derives these counts on every run. It
compares them by **equality** — improving a message reds the gate and
this table has to move with it.

<!-- XPILE-BACKENDREFUSE-001:BEGIN -->
| Backend | refusals probed | naming a contract ID | suggesting a `--target` |
|---|---|---|---|
| `bashrs` | 6 | 0 | 0 |
| `forjar` | 6 | 0 | 0 |
| `lean` | 4 | 1 | 4 |
| `ptx` | 6 | 0 | 0 |
| `ruchy` | 1 | 1 | 1 |
| `rust` | 1 | 1 | 1 |
| `spirv` | 7 | 0 | 0 |
| `wasm` | 2 | 1 | 1 |
| `wgsl` | 7 | 0 | 0 |
<!-- XPILE-BACKENDREFUSE-001:END -->

Read it as a property of that corpus, not a verdict on each backend: a
single probe samples one of a backend's many refusal messages. What it
does establish is that the old universal claim was false — 4 of 40
probed refusals named a contract ID, 7 named a better `--target`, and
`ptx`, `wgsl`, `spirv`, `bashrs` and `forjar` did neither in any of
theirs. Every message *does* name the backend that refused and the
construct it refused. See the
[shell round-trip tutorial](../tutorials/shell-roundtrip.md) for a
worked example.

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
  **whose body uses a construct a contract governs** — not above every
  function. `applicable_contracts()` is empty for comparison-only,
  logical-only, constant-only and call-only bodies, and those emit no
  citation line at all (see [frontends](frontends.md#python-frontend--whats-supported)).
  Through v0.1.617 this bullet stated it unconditionally.

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
use xpile_backend::{Backend, BackendConfig, Profile, Target};
use xpile_rust_codegen::RustBackend;

let config = BackendConfig {
    target: Target::Rust,
    profile: Profile::RustOut,
    hardware: None,
    emit_contracts: true,
};
let backend = RustBackend;
let artifact = backend.lower(&module, &config)?;
// `artifact` is a `xpile_backend::Artifact`; `artifact.primary` is the
// emitted Rust source.
```

The `Backend` trait surface is intentionally minimal — see
[Adding a backend](../contributing/adding-a-backend.md) for the full
implementation guide.

## Error handling

When a backend cannot lower a particular construct it fails — non-zero,
with no artifact — and the message names the backend that refused and
the construct it refused (`Stmt::Cmd`, `Expr::AwaitYield`, etc.).
**That is the whole of what holds for every backend.**

Naming the governing contract and suggesting a better `--target` are
worth doing and are what the best messages do, but they are **house
style, not an invariant** — see the [measured table](#status) above:
4 of 40 probed refusals name a contract ID, 7 name a `--target`, and
`ptx`, `wgsl`, `spirv`, `bashrs` and `forjar` do neither in any of
theirs.

Nor does the contract require them. `C-XPILE-BACKEND-TRAIT`'s
`compile_contract_citation` equation quantifies over
`ir_constructs(Artifact.primary)` — the **emitted** artifact — so it
constrains the success path only; `refus`, `suggest` and `error path`
occur zero times in its 776 lines.

Through v0.1.617 this section published the contract and target halves
as a numbered `must` and attributed them to that same equation "in
action". PMAT-1437 corrected the page header and the guarantee
paragraph and left this section standing 100 lines below the table that
refutes it — the same claim, in a different grammatical mood, in the
same file. PMAT-1438 is the rest of that class.
