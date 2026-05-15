# Cache, Determinism, Provenance

**Section 8 of [xpile-spec.md](../xpile-spec.md).**

## Three coupled invariants

| Invariant | What it ensures |
|---|---|
| **Determinism** | Default never invokes the LLM; static path is reproducible |
| **Cache identity** | LLM output is byte-identical across runs given the same inputs |
| **Provenance** | Every repaired file carries a verifiable marker pointing back to the cache key |

Together, they convert the stochastic agent into a reproducible artifact pipeline.

## Cache key

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

## Cache location

`~/.cache/xpile/repair/<cache_key_hex[0..2]>/<cache_key_hex>.rs`

Open question: project-local cache (`<repo>/.xpile-cache/`) as an alternative. Project-local enables committed reproducibility but bloats the repo. Decision deferred to Phase 2.

## Byte-identical replay

On `xpile transpile --repair=cached foo.py`:

1. Compute the cache key from the four-tuple
2. Look up the cache entry
3. **Cache miss → fail closed** (don't silently call the model)
4. **Cache hit → return the exact bytes stored**

10 consecutive cached runs MUST produce identical `sha256` of the output `.rs`. This is verified in CI via `tests/cache_byte_identical.rs`.

## Provenance marker

Every repaired `.rs` starts with:

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

Static-pass files never carry the marker. `grep -l '// xpile-repaired:' static_files/` MUST return empty.

## Why these invariants matter

Without determinism + cache + provenance, repair mode reduces to "ask Claude" and erodes the deterministic-transpiler value prop. With them:

- CI is reproducible across machines (same inputs → same artifact)
- PR reviewers can distinguish stochastic from deterministic output at a glance
- Cache hits are *receipts*, not best-effort lookups
- Bumping the model invalidates the cache (intended)

## Cache hygiene

Periodic eviction:

```bash
xpile cache prune --older-than 90d   # evict entries unused in 90 days
xpile cache prune --orphaned         # evict entries whose source files no longer exist
xpile cache verify                   # rehash all entries; quarantine corrupted ones
```

These commands are introduced in Phase 2 when the cache lands in production.
