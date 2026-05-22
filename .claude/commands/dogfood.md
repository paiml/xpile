---
description: Post-release dogfood — install xpile fresh from crates.io and verify every documented feature still works
---

Dogfood the current xpile release: install fresh from crates.io as an
end user would, run through the canonical Python features across all
three real code-lane backends (Rust, Ruchy, Lean), and report a
**pass/fail per feature** matrix. No special access — only `xpile`
on PATH and `python3` for sanity checks.

## When to run

- Immediately after a `gh release create vX.Y.Z` to confirm the
  published artifacts match the source tree.
- Before opening any PR that claims "feature X is shipped" — the
  dogfood is the ground truth, not the spec.
- When a user reports "I installed xpile and it doesn't do Y."

## Process

1. **Fresh install** from crates.io:
   ```bash
   mkdir -p /tmp/xpile-dogfood-$(xpile --version | awk '{print $2}' 2>/dev/null || echo new)
   cd /tmp/xpile-dogfood-*
   cargo install xpile --force
   xpile --version    # confirm the upgraded binary
   xpile info         # confirm registered frontends/backends match expectations
   ```

   If `cargo install` already replaced an older version, the
   `Replacing /home/noah/.cargo/bin/xpile` line confirms the upgrade.

2. **Run the canonical fixture battery** (each Python fixture; each
   backend that should support it; verify against the release notes):

   | Fixture | Python source | Rust | Ruchy | Lean |
   |---|---|---|---|---|
   | factorial | recursive int | ✅ | ✅ | ✅ |
   | greet | `def greet(name: str) -> str: return f"Hello, {name}!"` | ✅ | ✅ | ✅ |
   | total | `for x in xs: s = s + x` over `list[int]` | ✅ | ✅ | ❌ (Lean iter deferred) |
   | counts | `dict[str, int]` literal | ✅ | ✅ | ✅ (List-of-pairs first cut) |
   | append | `xs.append(v)` mutation | ✅ | ✅ | ❌ (Lean mut deferred) |
   | nested | `list[list[int]]` literal | ✅ | ✅ | ✅ |

   For each row, the cells say "what the release notes claim." A
   dogfood pass means **every ✅ produces matching output** and
   **every ❌ produces a clear deferral error**, not a panic, not a
   silent miscompile.

3. **Diff against release notes**: if a feature is marked shipped
   but the dogfood fails, open a bug. If a feature works but the
   release notes call it deferred, update the release notes — silent
   over-delivery is also a bug (the spec / docs are wrong).

4. **Report the matrix**: 6 rows × 3 columns + any anomalies. Save
   the raw transpile output for each ✅ cell so future dogfoods can
   spot regressions.

## Canonical Python sources

Drop these into the dogfood tmpdir as separate `.py` files; the
fixture names match the matrix above.

```python
# factorial.py
def factorial(n: int) -> int:
    return 1 if n <= 1 else n * factorial(n - 1)

# greet.py
def greet(name: str) -> str:
    return f"Hello, {name}!"

# total.py
def total(xs: list[int]) -> int:
    s = 0
    for x in xs:
        s = s + x
    return s

# counts.py
def counts() -> dict[str, int]:
    return {"alice": 1, "bob": 2, "carol": 3}

# append.py
def double_and_append(xs: list[int], n: int) -> int:
    xs.append(n)
    xs.append(n + n)
    return len(xs)

# nested.py
def grid() -> list[list[int]]:
    return [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
```

Run each through all three backends:

```bash
for f in factorial greet total counts append nested; do
    for tgt in rust ruchy lean; do
        echo "=== $f → $tgt ==="
        xpile transpile "$f.py" --target "$tgt"
        echo ""
    done
done
```

## What success looks like

- **Rust column** all ✅ rows produce compileable Rust (verify by
  piping through `rustc --edition 2021 --crate-type lib --emit=metadata`).
- **Ruchy column** mirrors Rust (Ruchy compiles to Rust).
- **Lean column** ✅ rows produce well-formed `def` declarations; ❌
  rows produce an `Unsupported(...)` error naming the deferred sub-track.

## What failure looks like

- A backend panics or emits malformed output that rustc rejects.
- A documented-as-deferred path silently succeeds with broken output.
- The installed binary's `xpile info` shows a different frontend /
  backend set than the published spec describes.
- An e2e fixture from the repo (`crates/xpile/tests/fixtures/*.py`)
  is missing from the binary's expected behavior.

## Output

Report a markdown matrix that mirrors the table above, plus:

- One sentence per row explaining the actual output observed.
- Any anomalies (spec drift, version mismatches, missing features).
- A "next action" line: "update spec §23 for X", "open bug for Y",
  or "no follow-ups — release is honest."
