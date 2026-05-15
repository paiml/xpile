# Provable Contracts (`pv`) Integration

**Section 11 of [xpile-spec.md](../xpile-spec.md).**

## The inversion

In traditional design: prose spec is canonical; tests/proofs/stubs are downstream.

**In xpile: YAML contracts are canonical.** Rust stubs, property tests, Kani proof harnesses, Lean 4 theorems, mdBook pages, and README quality claims are all *generated from* them via the `pv` CLI.

This is what makes drift between spec and code structurally impossible: the spec is regenerated from the contract.

## Dependency

```toml
# Workspace Cargo.toml
[workspace.dependencies]
provable-contracts = { path = "../aprender/crates/aprender-contracts", package = "aprender-contracts" }
```

Path-dep at v0.1.0; will switch to a crates.io version pin once aprender-contracts has a stable release.

## What xpile-contracts crate provides

```rust
pub use provable_contracts::{
    audit, binding, book_gen, coverage, diff, error, generate, graph,
    infer, kani_gen, latex, lean_gen, lint, probar_gen, query,
    readme_gen, scaffold, schema,
};

pub enum XpileContractLayer {
    LanguageSemantics,   // Layer 1: per-lang operational semantics
    Translation,          // Layer 2: source-construct → Rust
    Architectural,        // Layer 3: xpile-internal invariant
    HybridPipeline,       // Layer 4: end-to-end hybrid
}
```

The `XpileContractLayer` enum is a **metadata tag** for the team's organization. It is NOT a `pv` kind value (those are `kernel` / `pattern` / `registry` / etc.).

## Generated artifacts per contract

```
contracts/foo-v1.yaml
   │
   ├─→ target/contracts/scaffold/foo.rs      (failing Rust stubs)
   ├─→ target/contracts/probar/foo_test.rs   (property tests)
   ├─→ target/contracts/kani/foo_harness.rs  (#[kani::proof] harnesses)
   ├─→ target/contracts/lean/Foo.lean        (theorem stubs if math-dense)
   ├─→ docs/book/contracts/foo.md            (generated mdBook page)
   └─→ README.md numeric claims              (drift-detected by readme_gen)
```

`target/contracts/` is gitignored. Regeneration is idempotent:

```bash
make contracts   # delegates to: pv generate contracts/ --out target/contracts/
```

The contract YAML is the only thing checked into git for the "claim." Everything else is recomputable from contract + framework version.

## `pv` subcommands xpile uses

| Subcommand | Purpose | xpile CI? |
|---|---|---|
| `pv validate` | Schema check | yes |
| `pv lint` | All gates (validate + audit + score + verify + enforce + composition) | yes (hard fail) |
| `pv score` | Numeric quality grade per contract | yes (no regression) |
| `pv scaffold` | Generate Rust trait + test stubs | on contract change |
| `pv kani` | Generate Kani proof harnesses | nightly (heavy) |
| `pv probar` | Generate property tests | on contract change |
| `pv lean` | Generate Lean 4 theorem stubs | manual (when math-dense) |
| `pv coverage` | Cross-contract obligation coverage | nightly |
| `pv audit` | Trace paper→equation→contract→test→proof | nightly |
| `pv diff` | Suggest semver bump on contract change | on PR |
| `pv query` | Search contracts by intent/regex/literal | dev-time |

## Lint passing as a hard CI gate

Every PR must pass `pv lint` 8/8 gates on the full contracts directory. At v0.1.0:

```
  Gate 1: validate             ✓  (4 contracts, 0 errors, 2 warnings) [0ms]
  Gate 2: audit                ✓  (4 contracts, 0 findings) [0ms]
  Gate 3: score                ✓  (4 contracts, mean=0.58, threshold=0.00) [0ms]
  Gate 4: verify               ✓  (0 refs, 0 found, 0 missing) [0ms]
  Gate 5: enforce              ✓  (10 eqs, 4 pre, 0 post) [0ms]
  Gate 6: enforcement-level    ✓  (skipped: 4 contracts, 0 below level) [0ms]
  Gate 7: reverse-coverage     ⏭  (skipped: no --binding or --crate-dir provided) [0ms]
  Gate 8: composition          ✓  (0 edges, 0 satisfied, 0 broken) [0ms]
```

Gate 7 (reverse-coverage) is skipped until Phase 3, when contracts get wired to actual code via `--binding` or `--crate-dir`.

## Future: full `pv` ownership

By Phase 6, every architectural decision, language semantic, translation rule, and hybrid-pipeline invariant in xpile lives in `contracts/`. The Rust source under `crates/*/src/` is generated where possible, hand-written only with a `# manual: <justification>` annotation in a governing contract.
