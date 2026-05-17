# Migration from depyler / decy

**Section 19 of [xpile-spec.md](../xpile-spec.md).**

> **Status (2026-05-18 / PMAT-097 sweep):** Phases A and B have
> **substantially shipped**. The shared-concern crates exist in
> `crates/`, and the per-language frontends are folded in-tree as
> `crates/depyler-frontend`, `crates/decy-frontend`,
> `crates/ruchy-frontend`, `crates/bashrs-frontend` alongside
> `crates/latex-contract-frontend`. The "thin-shim" half of Phase B
> (publishing legacy CLI shims on crates.io) is post-v0.1.0 work.

## Two-phase plan: extract first, merge second

### Phase A — Extract (substantially shipped at v0.1.0)

Move shared concerns into the xpile workspace as crates.io-publishable crates. depyler and decy depend on them. Per-language repos shrink as functionality moves into xpile.

| Shared concern | Extract to | Status (v0.1.0) |
|---|---|---|
| Agent loop | `xpile-agent` | scaffold present; real impl is XPILE-AGENT-001+ future work |
| Oracle protocol | `xpile-oracle` | scaffold present; real impl is XPILE-ORACLE-001+ future work |
| Frontend trait | `xpile-frontend` | **shipped** — `Frontend` trait wired, used by all 5 frontends |
| Backend trait | `xpile-backend` | **shipped** — `Backend` trait wired, used by all codegens |
| ContractFrontend trait | `xpile-contract-frontend` | **shipped** — used by `latex-contract-frontend` |
| ContractBackend trait | `xpile-contract-backend` | **shipped** — used by `xpile-{lean,latex}-contract-backend` |
| Meta-HIR | `xpile-meta-hir` | **shipped** — includes Layer-B shell variants (PMAT-039..056) |
| Rust codegen | `xpile-rust-codegen` | **shipped** — real emission |
| Ruchy codegen | `xpile-ruchy-codegen` | **shipped** — real emission |
| Lean codegen | `xpile-lean-codegen` | scaffold (real emission is post-v0.1.0) |
| PTX codegen | `xpile-ptx-codegen` | scaffold + Layer-5 contract |
| WGSL codegen | `xpile-wgsl-codegen` | scaffold |
| BigInt runtime | `xpile-bigint` | **shipped** — slow-path lane for C-PY-INT-ARITH (opt-in via `-> BigInt` annotation) |
| FFI manifest | `xpile-ffi-manifest` | scaffold (real impl is XPILE-FFI-MANIFEST-001+ future work) |
| LLM cache | `xpile-llm` | scaffold |
| MCP server | `xpile-mcp` | scaffold |
| Contracts framework | `xpile-contracts` (delegates to `pv`) | **shipped** — `pv` v0.33 from crates.io, 12 contracts gated |

What end-of-Phase-A originally promised that *did* ship at v0.1.0:

- ✅ depyler, decy, ruchy, bashrs frontends are all in-tree (Phase B-shape, not Phase A-shape — the actual path skipped the "remote dependency" step)
- ✅ 12 substrate contracts live in `xpile/contracts/` with 100% §14.4 QUORUM
- ✅ Workspace is on crates.io as `xpile 0.0.1` (name reservation; v0.1.0+ unreleased)

What end-of-Phase-A originally promised that did NOT ship at v0.1.0:

- ⏳ xpile-* crates with stable individual APIs on crates.io (only the top-level `xpile` reserves; individual crates haven't been published)
- ⏳ depyler-agent / decy-agent deprecated shims pointing at xpile-agent (depyler-agent was absorbed into the fold rather than maintained as a deprecated shim)

### Phase B — Merge (shipped at v0.1.0, in-tree skipping the subtree path)

Fold depyler, decy, and ruchy into the xpile monorepo. **The actual path didn't use `git subtree add`** — the frontends were re-implemented as new crates inside xpile (`crates/depyler-frontend/`, etc.) rather than imported from the external repos via subtree. This preserved the architectural separation between xpile (the workspace) and depyler-the-published-binary (which continues to exist as a separate downstream consumer of crates.io's `xpile`).

The bashrs merger (PMAT-037..058) is the most thoroughly executed version of Phase B: `crates/bashrs-frontend/` and `crates/bashrs-backend/` are in-tree as workspace members, `SourceLang::Shell` and `Target::Shell` are first-class IR citizens, and the cross-domain Python→shell consumer (PMAT-040) ships end-to-end.

```bash
# Per-PR gate (now live as CI):
cargo check --workspace
cargo test --workspace
pv lint contracts/
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check advisories
```

After Phase B (what shipped vs. what was planned):

- ✅ `crates/depyler-frontend/`, `crates/decy-frontend/`, `crates/ruchy-frontend/`, `crates/bashrs-frontend/`, `crates/latex-contract-frontend/` all live in-tree
- ✅ All future PRs land in `paiml/xpile`
- ⏳ `paiml/depyler` / `paiml/decy` / `paiml/ruchy` repos still exist as their own things (NOT yet thin shims pointing at xpile — that's post-v0.1.0)
- ⏳ Issue migration via `gh issue transfer` hasn't been run; legacy issues stay on their original repos

## Why this order

| Order | Pros | Cons |
|---|---|---|
| **Extract first** (chosen) | Lowest risk; per-language repos keep shipping; rollback is just "stop depending on xpile-*" | Slower to consolidate |
| Merge first | Faster to the final state | Big-bang risk; in-flight PRs in depyler/decy stall |

Extract-first wins because depyler is shipping production code (v4.1.1, paying customers). A merge-first approach would freeze depyler releases for weeks; extract-first keeps it shipping the whole time.

## Contract migration

depyler's 5 repair-mode contracts (`repair-determinism-v1.yaml`, `repair-budget-v1.yaml`, `repair-provenance-v1.yaml`, `repair-oracle-v1.yaml`, `skill-graduation-v1.yaml`) port to xpile as Layer-3 architectural contracts. They become *xpile*-internal invariants applied to all repair sessions, regardless of source language.

decy's 4 existing contracts port as Layer-1 C-semantics or Layer-2 C-translation contracts.

Migration sequence per contract:

1. Copy `<repo>/contracts/foo.yaml` to `xpile/contracts/foo.yaml` (preserve git history via `git mv` if practical, or `git filter-repo` to rewrite paths)
2. Run `pv lint xpile/contracts/foo.yaml` — must pass 8/8
3. Update `references:` in the contract to point at xpile paths
4. Delete from the source repo
5. In the source repo, update CI to consume the contract via `pv lint --include-fleet xpile`

## Issue / PR migration

```bash
# Issues
for issue in $(gh issue list -R paiml/depyler --json number --jq '.[].number'); do
    gh issue transfer "$issue" --repo paiml/depyler --to-repo paiml/xpile
done

# PRs are NOT transferred automatically — they're closed in the source repo and reopened against xpile manually
```

## Backwards compatibility window

For 6 months after Phase B:

- `paiml/depyler` and `paiml/decy` keep accepting issues; they're auto-relabeled `needs-transfer-to-xpile`
- `cargo install depyler` and `cargo install decy` still work (the binaries are thin wrappers around xpile)
- Documentation links in user-facing materials are updated lazily, not eagerly

After 6 months, the per-language repos are archived. Users install via `cargo install xpile` and run `xpile transpile --target depyler-compat foo.py` for the legacy CLI experience.

## What doesn't migrate

- alchemize stays a separate repo. It's a sibling transpiler in a different domain (probabilistic models); xpile could absorb it as a frontend later, but not in this migration plan.
- aprender stays separate. It's a *consumer* of transpilers (an ML framework), not a transpiler itself.
- ruchy stays separate. xpile depends on the `ruchy` crate from crates.io; the ruchy language and tooling continue to evolve on their own cadence.

## Migration completion gate

Phase B is "done" when:

- [x] All depyler tests pass under xpile workspace (`cargo test --workspace`) — depyler-frontend lives at `crates/depyler-frontend/`, tests pass on every CI run
- [x] All decy tests pass under xpile workspace — decy-frontend at `crates/decy-frontend/` (scaffold-stage, but tests pass)
- [x] `pv lint xpile/contracts/` passes — current state: 12 contracts pass with 0 errors (substantially overshipped from the original "8/8" target)
- [ ] `paiml/depyler` and `paiml/decy` READMEs redirect to `paiml/xpile` — pending post-v0.1.0
- [ ] One end-to-end hybrid demo (Python + C) passes in xpile (Phase 5 of the rollout) — partially shipped: Python+shell hybrid via PMAT-040 / PMAT-043 / PMAT-052; Python+C numpy demo is post-v0.1.0
