# xpile — Current Status

**Last refreshed:** 2026-07-26 (PMAT-1348 — truth-up: every hand-typed count removed and replaced with the command that derives it)
**Canonical source of truth for the supported subset:** [`/CHANGELOG.md`](../../CHANGELOG.md)

This file is a **thin index**, not a snapshot. It deliberately carries **no
hand-typed counts** — every number below is stated as the command that derives
it, because the previous snapshot's counts were wrong in five separate places
for two months while this file called itself the single source of truth. That
rule is enforced by `crates/xpile/tests/claims_drift.rs`
(`current_md_carries_no_bare_derived_counts`), so a count typed back into this
file reds a required CI context.

## Derive the live numbers — do not trust prose

| You want | Run |
|---|---|
| Workspace crate count | `ls -d crates/*/ \| wc -l` |
| Contract count | `ls contracts/*.yaml \| wc -l` |
| §14.4 stratum quorum (QUORUM / PARTIAL / UNVERIFIED) | `xpile quorum` |
| Kani BMC harness count | `grep -rho '#\[kani::proof\]' contracts/kani/ \| wc -l` |
| Lean pilot module count | `cargo test -p xpile --test lean_pilot_roots` — the lakefile `roots := #[…]` block is authoritative; a naive `grep` over `lakefile.lean` **over-counts** (it catches backtick-quoted names outside the roots block) |
| Registered frontends / backends | `xpile info` — pinned to the live registry and the `Target` enum by `claims_drift.rs` |
| Published crates.io version | `cargo search xpile` (the workspace version is single-sourced at `Cargo.toml` `[workspace.package] version`) |
| Contract lint health | `pv lint contracts/` — must be 0 errors, 0 warnings |

## Backends — what is real

All nine `Target` variants emit. **PTX, WGSL and SPIR-V are no longer
scaffolds** (this file claimed they were until 2026-07-26):

- **Rust** (`pub fn`), **Ruchy** (`fun`), **Lean 4** (`def`), **Shell/bashrs**
  (POSIX subset), **WASM** (native linear-memory runtime), **forjar YAML**
- **PTX** — CLI-reachable and emits real PTX:
  `xpile transpile k.py --target ptx --hardware ptx:sm_89`
- **WGSL** — emits a real compute-shader subset; **SPIR-V** — emits real SPIR-V
  via `naga` from the WGSL emitter

Scope for each backend is enumerated in [`/CHANGELOG.md`](../../CHANGELOG.md);
what a backend *refuses* is as load-bearing as what it emits.

### Two DISJOINT WebAssembly paths — do not conflate them

The README describes both; they share no code:

1. **`--target wasm`** — the native WAT emitter (`xpile-wasm-codegen`). Lowers
   the meta-HIR straight to WebAssembly text. This is the lane the dict/set/str
   runtime work lands in. It does **not** produce a WASI binary.
2. **`--emit-crate` → `cargo build --target wasm32-wasip1`** — emits a complete
   Rust crate, then Rust's own toolchain produces the "universal `.wasm`"
   binary that runs under `wasmtime`. This is the path the proven-model demo
   uses.

A program that runs through path 2 may still refuse on path 1.

### Lean emit caveat — the default does NOT elaborate

`--contracts on` is the **default**, and it emits contract citations as a Lean
attribute:

```lean
@[xpile_contract "C-PY-INT-ARITH"]
def add (a : Int) (b : Int) : Int := (a + b)
```

`xpile_contract` is **not a registered Lean attribute**, so `lean` exits 1 with
`unexpected token; expected ']'`. For Lean output you intend to elaborate, use
`--contracts off`. Registering the attribute in a prelude is an open owner
decision (`lean-attribute-prelude`), not an oversight; every other backend cites
contracts in comments, which is why only Lean is affected.

## CI enforcement

Eight jobs run on every PR; org ruleset `13878864` requires exactly two of them.

  <!-- XPILE-ENFORCEMENT REQUIRED-CONTEXTS: gate, workspace-test -->
  **Required (merge-blocking):** `gate`, `workspace-test`. **Advisory (run every PR, red on a real regression, do NOT block a merge):** `kani`, `lake-build`, `docs`, `wasi`, `lean-models`, `shader-validate`. Verify with `gh api repos/paiml/xpile/rules/branches/main`; the claim is pinned by `crates/xpile/tests/ruleset_drift.rs`. Promoting the proof lane to required is an owner-gated org-admin edit — see [`enforcement-handoff.md`](enforcement-handoff.md) §2.

Because the proof lane is advisory, **a red `kani` or `lake-build` does not
block a merge** — read those jobs before trusting a green PR.

## Where to look next

| You want to know | Read |
|---|---|
| What Python constructs are supported | [`/CHANGELOG.md`](../../CHANGELOG.md) §"Python subset" |
| What's planned next | [`/docs/roadmaps/queue.yaml`](../roadmaps/queue.yaml) (next-pick source of truth) |
| How the architecture is shaped | [`/docs/specifications/xpile-spec.md`](../specifications/xpile-spec.md) |
| What the adversarial audit found | [`/docs/specifications/audit-design.md`](../specifications/audit-design.md) |
| How a frontend / backend plugs in | [`sub/frontend-trait.md`](../specifications/sub/frontend-trait.md) / [`sub/backend-trait.md`](../specifications/sub/backend-trait.md) |
| Why Lean and LaTeX are bidirectional | [`sub/lean-bidirectional.md`](../specifications/sub/lean-bidirectional.md) / [`sub/latex-bidirectional.md`](../specifications/sub/latex-bidirectional.md) |

## Why this file carries no counts

Five-whys, re-run 2026-07-26 after the PMAT-1348 audit found this file stale in
five places at once (crate count, contract count, quorum line, GPU-backend
status, crates.io status) while `INDEX.md` called it "the single source of
truth":

- **Symptom:** every enumerated section drifted from reality within days
- **Why 1:** hand-authored at snapshot time, never re-authored
- **Why 2:** the same facts were already authoritative elsewhere (`Cargo.toml`,
  `contracts/`, the `Target` enum, `xpile quorum`, CHANGELOG)
- **Why 3:** duplicating means two places to keep in sync; only one ever was
- **Why 4:** the 2026-05-18 fix ("demote this file to a pointer") was *prose
  only* — nothing tested it, so counts crept straight back in
- **Root cause:** a doc rule with no gate is a suggestion. Fix: state numbers as
  **derive commands**, and let `claims_drift.rs` red the build when a bare count
  reappears.
