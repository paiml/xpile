# bashrs Federation

**Section 19 of [xpile-spec.md](../xpile-spec.md). Sibling: [migration.md](migration.md).**

## Why federation, not merge

bashrs ([github.com/paiml/bashrs](https://github.com/paiml/bashrs)) is a
PAIML Sovereign AI Stack transpiler — the same family as depyler and
decy — but its language domain is structurally outside xpile's
meta-HIR. Shell semantics (subprocess invocation, environment-variable
expansion, here-docs, signal handling, here-strings) don't compose
with C / Python / Rust types the way the meta-HIR was designed for.
Forcing them through meta-HIR would either bloat the IR with
shell-flavored Type variants OR project shell semantics onto a foreign
type lattice and lose fidelity.

Federation keeps each transpiler's IR clean and reuses xpile's
*surface* (CLI, Oracle protocol, agent-loop, contract substrate)
without merging IRs.

## Routing contract

`xpile transpile <file>` dispatches based on extension:

| Extension(s) | Owner | Frontend |
|---|---|---|
| `.py`, `.pyi` | xpile native | `depyler-frontend` |
| `.c`, `.h`, `.cpp`, `.hpp`, `.cu` | xpile native | `decy-frontend` |
| `.rs` | xpile native | `rust-frontend` (planned) |
| `.ruchy` | xpile native | `ruchy-frontend` |
| `.lean` | xpile native | `lean-frontend` |
| `.sh`, `.bash` | **bashrs (federated)** | `bashrs::shell::Frontend` |
| `.zsh` | **bashrs (federated)** | `bashrs::shell::Frontend` (zsh dialect) |
| `Makefile`, `*.mk` | **bashrs (federated)** | `bashrs::makefile::Frontend` |
| `Dockerfile` | **bashrs (federated)** | `bashrs::dockerfile::Frontend` |

The TranspileSession sees the federated frontends through the same
`Frontend` trait surface as the native ones; the call site doesn't
care which crate the impl lives in.

## What gets reused

When a federated dispatch happens, xpile still runs:

- **Oracle** — captures behavior of the *original* shell / Makefile /
  Dockerfile execution (where applicable) and compares against the
  transpiled output. Same `xpile-oracle` machinery as the native
  languages.
- **Agent loop** — if static transpilation fails, the bounded repair
  loop applies. The LLM gets bashrs's compile errors as input,
  identical to how it gets `cargo build` errors for the Rust path.
- **Contract substrate** — bashrs publishes its own contract YAMLs to
  the `contracts/` directory under xpile's `pv lint` gate. Layer-1
  shell-semantics contracts (POSIX vs bash dialect, idempotency
  invariants, shell-injection bounds) sit alongside `py-int-arith-v1`
  and the other native contracts.
- **MCP server** — bashrs operations are exposed under the same MCP
  tool surface as Python / C / etc. transpilation; agentic clients
  see one workbench.
- **Citation bridge** — bashrs-emitted code carries
  `# xpile-contract: <ID>` comment citations (matching the mdBook /
  Rust / Ruchy convention from `sub/contract-frontend-trait.md`).

## What bashrs owns end-to-end

- Its own AST + IR (the IR is *not* meta-HIR — see §3 of the canonical
  spec).
- Its own emission backends (POSIX shell, dialect-purified bash).
- Its own internal correctness gate (the 30-point Dockerfile
  falsification checklist named in bashrs's own README continues to
  apply, separate from xpile's gate).

## What stays in xpile

- The dispatch surface (extension → frontend mapping above).
- The Oracle protocol wrapping every transpile, federated or native.
- The agent-loop repair budget.
- The contract substrate.

## Implementation status

- v0.1.0: **planned**. bashrs is published to crates.io independently;
  the xpile workspace doesn't yet take a dep on it. The extension
  mapping above is the design target; the `bashrs-frontend` /
  `bashrs-backend` shim crates that wrap bashrs's public API are
  scaffold-stage (same posture as `decy-frontend` today).
- v0.2.0 (target): bashrs added as a workspace dep, shim crates
  forward to it, `xpile transpile foo.sh --target rust` round-trips
  end-to-end with Oracle wrap.

## Why this asymmetry exists (depyler/decy vs bashrs)

depyler and decy are **inside the native tier** — their domains
(Python, C) compose with each other via FFI manifests and share
meta-HIR. Merging them in (per §19's extract-then-merge plan)
collapses three half-implemented monorepos into one full one.

bashrs is **outside the native tier** — its domain (shell) doesn't
compose with C / Python at the type level; it composes at the
process level (subprocess invocation, exit codes, environment).
Merging would create a second IR inside xpile with no benefit; the
clean answer is federation: xpile depends on bashrs the same way any
Rust workspace depends on a published crate.

## Cross-references

- xpile-spec.md §1 — Vision and Architecture (scope statement, native vs federated tiers).
- xpile-spec.md §19 — Migration from depyler / decy / bashrs.
- audit-design.md §4 — Sovereign AI Stack framing.
- bashrs README — domain coverage (Rust→POSIX, bash purification, Makefile + Dockerfile linting).
