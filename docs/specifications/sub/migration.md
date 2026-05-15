# Migration from depyler / decy

**Section 19 of [xpile-spec.md](../xpile-spec.md).**

## Two-phase plan: extract first, merge second

### Phase A — Extract (weeks 1-6)

Move shared concerns into the xpile workspace as crates.io-publishable crates. depyler and decy depend on them. Per-language repos shrink as functionality moves into xpile.

| Shared concern | Extract to | Status |
|---|---|---|
| Agent loop | `xpile-agent` | scaffold ✅, real impl pending |
| Oracle protocol | `xpile-oracle` | scaffold ✅ |
| Frontend trait | `xpile-frontend` | scaffold ✅ |
| Meta-HIR | `xpile-meta-hir` | scaffold ✅ |
| Rust codegen | `xpile-rust-codegen` | scaffold ✅ |
| FFI manifest | `xpile-ffi-manifest` | scaffold ✅ |
| LLM cache | `xpile-llm` | scaffold ✅ |
| MCP server | `xpile-mcp` | scaffold ✅ |
| Contracts framework | `xpile-contracts` (delegates to `pv`) | wired ✅ |

By end of Phase A:

- xpile crates are on crates.io with stable APIs
- depyler-core, decy-core depend on the xpile-* crates (path-dep first, crates.io after release)
- depyler-agent, decy-agent are deprecated and re-export from xpile-agent
- per-language repos: ~15 crates → ~6-8 crates (only language-specific code remains)

### Phase B — Merge (weeks 7-8)

Fold depyler and decy into the xpile monorepo, preserving history.

```bash
# In xpile/
git subtree add --prefix=crates/depyler https://github.com/paiml/depyler.git main
git subtree add --prefix=crates/decy https://github.com/paiml/decy.git main
```

Use `git filter-repo` for finer-grained history rewriting if subtree-add carries too much. Per-PR test:

```bash
# Verify nothing breaks during the fold
cargo check --workspace
cargo test --workspace
pv lint contracts/
```

After Phase B:

- `paiml/depyler` and `paiml/decy` become thin shims that re-export from `paiml/xpile`
- All future PRs land in `paiml/xpile`
- Existing GitHub issues are migrated via `gh issue transfer`

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

- [ ] All depyler tests pass under xpile workspace (`cargo test --workspace`)
- [ ] All decy tests pass under xpile workspace
- [ ] `pv lint xpile/contracts/` passes 8/8 with all migrated contracts included
- [ ] `paiml/depyler` and `paiml/decy` READMEs redirect to `paiml/xpile`
- [ ] One end-to-end hybrid demo (Python + C) passes in xpile (Phase 5 of the rollout)
