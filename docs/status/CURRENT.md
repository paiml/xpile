# xpile — Current Status

**Last refreshed:** 2026-05-16 (PMAT-001 kaizen sweep)
**Canonical source of truth for the supported subset:** [`/CHANGELOG.md`](../../CHANGELOG.md)

This file used to enumerate every implemented crate / contract / construct, and went stale within hours of each PR. The previous 180-line snapshot is preserved in git history (last useful version: commit `cdcece9`, the initial bootstrap). Going forward, this file is a thin index — anything that needs to stay accurate lives in `CHANGELOG.md`.

## High-water mark (v0.1.0, 2026-05-16)

- 24 workspace crates compile clean; `cargo check`, `cargo clippy -D warnings`, `cargo fmt --check`, `cargo deny check advisories` all green
- 11 contracts pass `pv lint` (0 errors)
- Three real backends: Rust (`pub fn`), Ruchy (`fun`), Lean 4 (`def`). PTX / WGSL / SPIR-V still scaffolded.
- Python subset supported: see [`CHANGELOG.md`](../../CHANGELOG.md) §"Python subset (live, runtime-verified)"
- Five runtime-verified semantic fixtures (factorial, fib, gcd, abs_val, sign) — `rustc -O` execute and `assert_eq!`
- 25 e2e tests, ~52 total workspace tests
- CI: `gate` + `workspace-test` required for PR merge; branch protection active on `main`
- crates.io: `xpile 0.0.1` published as a name reservation; v0.1.0+ unreleased
- 21 PRs merged on `main`

## Where to look next

| You want to know | Read |
|---|---|
| What Python constructs are supported | [`/CHANGELOG.md`](../../CHANGELOG.md) §"Python subset" |
| What's planned next | `pmat work list` |
| How the architecture is shaped | [`/docs/specifications/xpile-spec.md`](../specifications/xpile-spec.md) |
| What the adversarial audit found | [`/docs/specifications/audit-design.md`](../specifications/audit-design.md) |
| How a frontend / backend plugs in | [`sub/frontend-trait.md`](../specifications/sub/frontend-trait.md) / [`sub/backend-trait.md`](../specifications/sub/backend-trait.md) |
| Why Lean and LaTeX are bidirectional | [`sub/lean-bidirectional.md`](../specifications/sub/lean-bidirectional.md) / [`sub/latex-bidirectional.md`](../specifications/sub/latex-bidirectional.md) |

## Why this file is a stub now

Five-whys for the previous 180-line snapshot:

- **Symptom:** every section ("Done", "Crates", "Contracts", "Next steps") drifted from reality within days
- **Why 1:** hand-authored at v0.1.0 scaffold time, never re-authored
- **Why 2:** the same facts were already authoritative elsewhere (Cargo.toml, contracts dir, CHANGELOG)
- **Why 3:** duplicating means two places to keep in sync; only one ever was
- **Root cause:** there was no canonical source for "what's done"; this file was a parallel one. Fix: declare CHANGELOG.md canonical (per PMAT-001) and demote this file to a pointer.
