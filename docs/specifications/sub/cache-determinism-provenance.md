# Cache, Determinism, Provenance

**Section 8 of [xpile-spec.md](../xpile-spec.md).**

> **Status (2026-07-30 / PMAT-1502 sweep): this page is a REQUIREMENT, not a
> description.** It was authored at v0.0.1 scaffold time in the present
> indicative, and every sentence below about a cache, a replay path, a
> provenance marker or an `xpile cache` command described something that has
> never existed. Measured against the shipped `xpile 0.1.618` and the tracked
> tree:
>
> | claim | measured |
> |---|---|
> | `CacheKey::compute(source, xpile_version, model_id, skills_hash)` | **implemented** — `crates/xpile-llm/src/lib.rs`, signature and `\0`-separated `sha256` exactly as documented below |
> | the cache itself (store, lookup, eviction) | **absent** — `xpile-llm` is 42 lines with zero filesystem calls; nothing reads or writes a cache directory |
> | `xpile transpile --repair=cached` | **absent** — `xpile transpile` registers no `--repair` flag; the invocation exits **2** (clap parse error) |
> | `xpile cache prune` / `xpile cache verify` | **absent** — `error: unrecognized subcommand 'cache'`, exit **2** |
> | the `// xpile-repaired:` provenance marker | **no emitter** — the string occurs in 0 tracked `.rs` files; transpiled output carries 0 marker lines |
> | `tests/cache_byte_identical.rs`, named below as the CI verification | **no such file** — absent from `git ls-files` under any prefix |
>
> The four-tuple hash is real and is the one piece a reader may rely on. Nothing
> else here is a statement about the shipped tool. The honest form for this lane
> already exists one document over: `sub/ci-gates.md` discloses that
> `scripts/check_provenance.sh` did not ship, gives the reason, and carries the
> tracking id `XPILE-CI-PROVENANCE-001`. This page had no such block.

## Three coupled invariants

These are the properties repair mode is REQUIRED to have before it is reachable.
None is enforced today, because no repair path is reachable today.

| Invariant | What it must ensure |
|---|---|
| **Determinism** | Default never invokes the LLM; static path is reproducible |
| **Cache identity** | LLM output is byte-identical across runs given the same inputs |
| **Provenance** | Every repaired file carries a verifiable marker pointing back to the cache key |

Together they would convert a stochastic agent into a reproducible artifact
pipeline. Note that the Determinism row is satisfied **vacuously** at 0.1.618:
the static path never invokes an LLM because no path does.

## Cache key

Implemented, and the signature below matches `crates/xpile-llm/src/lib.rs`
verbatim:

```rust
pub struct CacheKey([u8; 32]);

impl CacheKey {
    pub fn compute(
        source: &[u8],
        xpile_version: &str,
        model_id: &str,
        skills_hash: &[u8],
    ) -> Self;
}
```

Hashed with `sha256(source || \0 || xpile_version || \0 || model_id || \0 || skills_hash)`. The four-tuple is the *only* input. No ambient state (date, hostname, env vars, working dir) is permitted as key material.

⚠️ This computes a key **for a cache that does not exist**. `xpile-llm` has no
filesystem access; the crate's own module doc calls itself "LLM invocation +
content-addressed cache" and implements neither.

## Cache location (planned)

`~/.cache/xpile/repair/<cache_key_hex[0..2]>/<cache_key_hex>.rs`

Open question: project-local cache (`<repo>/.xpile-cache/`) as an alternative. Project-local enables committed reproducibility but bloats the repo.

⚠️ **This decision was deferred to "Phase 2", which has come and gone.**
`sub/phased-rollout.md` records Phase 2 as *partially shipped (py-int-arith
only)* and v0.1.0 as released; no cache landed in it. The decision is therefore
undeferred and still unmade — it is owed before any repair path becomes
reachable, not before a phase that is already in the past.

## Byte-identical replay (planned)

On a `--repair=cached` invocation, once such a flag exists:

1. Compute the cache key from the four-tuple
2. Look up the cache entry
3. **Cache miss → fail closed** (don't silently call the model)
4. **Cache hit → return the exact bytes stored**

10 consecutive cached runs MUST produce identical `sha256` of the output `.rs`.

⚠️ **Nothing verifies this and nothing ever has.** This paragraph previously read
"This is verified in CI via `tests/cache_byte_identical.rs`" — a file that is
absent from the tree under every prefix. The falsification test is owed together
with the replay path it would cover.

## Provenance marker (planned)

Every repaired `.rs` is REQUIRED to start with:

```rust
// xpile-repaired: <64-hex-cache-key> via <model_id> at <RFC3339-UTC>
```

Examples:

```rust
// xpile-repaired: a3b2c1d4e5f6...782e via claude-sonnet-4-6 at 2026-05-15T17:42:03Z
```

Rules:

- Marker is the **first line** of the file, before any other code or comments
- Hash MUST equal the cache key for the inputs that produced it
- `model_id` MUST be fully-qualified (e.g., `claude-sonnet-4-6`, not `claude-sonnet`)
- Timestamp MUST be RFC3339 UTC with `Z` suffix and second precision

⚠️ **No code emits this marker.** `xpile-repaired` appears in 0 tracked `.rs`
files, so the rules above constrain an empty set and the "static-pass files never
carry the marker" check is vacuous — the `static_files/` directory it named does
not exist either.

## Why these invariants matter

Without determinism + cache + provenance, repair mode reduces to "ask Claude" and erodes the deterministic-transpiler value prop. With them:

- CI is reproducible across machines (same inputs → same artifact)
- PR reviewers can distinguish stochastic from deterministic output at a glance
- Cache hits are *receipts*, not best-effort lookups
- Bumping the model invalidates the cache (intended)

## Cache hygiene (planned)

Periodic eviction, once a cache exists:

```bash
xpile cache prune --older-than 90d   # evict entries unused in 90 days
xpile cache prune --orphaned         # evict entries whose source files no longer exist
xpile cache verify                   # rehash all entries; quarantine corrupted ones
```

⚠️ **No `cache` subcommand is registered.** Each line above exits **2** with
`error: unrecognized subcommand 'cache'` against `xpile 0.1.618`. This block
previously closed with "These commands are introduced in Phase 2 when the cache
lands in production" — the same expired deferral as the cache-location decision
above.
