# xpile contracts

This directory holds **provable contracts** — YAML files that bind quantitative claims about xpile to falsifiable shell-command formulas. Drift between the contract and live repo state fails CI.

The format is borrowed from [aprender's contracts](https://github.com/paiml/aprender/tree/main/contracts) and used identically by the depyler repair-mode work.

## Format

```yaml
metadata:
  id: C-XPILE-DETERMINISM
  version: "1.0.0"
  created: "2026-05-15"
  author: PAIML Engineering
  kind: behavioral        # or "pattern", "process"
  status: draft           # or "enforced", "deprecated"
  description: |
    Plain-English statement of what this contract pins down and why.
  references:
    - docs/specifications/xpile-architecture-v1.md
  depends_on: []

equations:
  some_named_property:
    formula: |
      lhs == rhs
    domain: |
      Why this property must hold.
    invariants:
      - "concrete shell-falsifiable statement"
    preconditions:
      - "what must be true for the formula to be meaningful"
```

## Planned contracts (scaffold stage — all TODO)

- `xpile-determinism-v1.yaml` — default never runs LLM; cache key uniqueness; byte-identical output on hit
- `xpile-budget-v1.yaml` — per-file caps enforced; budget exhaustion fails closed
- `xpile-provenance-v1.yaml` — every repaired `.rs` carries a marker
- `xpile-oracle-v1.yaml` — agent exit requires oracle pass
- `xpile-frontend-trait-v1.yaml` — every registered frontend handles its declared extensions
- `xpile-ffi-manifest-v1.yaml` — every cross-language call in a session is registered

See `docs/specifications/xpile-architecture-v1.md` for the full design.

## CI integration (planned)

```bash
# Future: scripts/check_contracts.sh runs every contract's formula
bash scripts/check_contracts.sh
```

The script will be wired into CI before the first `enforced` contract lands.
