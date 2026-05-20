# CLI reference

> **Governing contract:** [`C-XPILE-BACKEND-TRAIT`](contracts.md#c-xpile-backend-trait)
> for emit-side error paths and citation requirements.

All commands are subcommands of `xpile`. Run `xpile --help` or
`xpile <cmd> --help` for inline documentation.

## `xpile info` (default)

Lists registered frontends and backends.

```bash
$ xpile info
xpile — polyglot transpile workbench

Code lane:
  frontends (4):
    - python (py, pyi)
    - c (c, h)
    - ruchy (ruchy)
    - bashrs (sh, bash, zsh, mk)
  backends (6):
    - rust → Rust
    - ruchy → Ruchy
    - ptx → Ptx
    - wgsl → Wgsl
    - lean → Lean
    - bashrs → Shell

Proof lane:
  contract_frontends (1):
    - latex ← LatexMath
  contract_backends (2):
    - lean-theorem → LeanTheorem
    - latex → LatexMath
```

Use this to confirm your install can see every lane.

## `xpile transpile`

```bash
xpile transpile [OPTIONS] <INPUT>
```

The main command. The file extension selects the frontend; `--target`
selects the backend.

| Flag | Default | Meaning |
|---|---|---|
| `<INPUT>` | required | path to the source file |
| `--target <T>` | `rust` | one of `rust`, `ruchy`, `ptx`, `wgsl`, `spirv`, `lean`, `shell` |
| `--out <P>` | stdout | output path |

Examples:

```bash
xpile transpile factorial.py                     # Python → Rust (default)
xpile transpile factorial.py --target ruchy      # Python → Ruchy
xpile transpile factorial.py --target lean       # Python → Lean 4
xpile transpile script.sh --target shell         # Shell round-trip
xpile transpile factorial.py --out factorial.rs  # Write to a file
```

If a backend cannot lower a particular construct, the error message
names the governing contract and suggests the correct target — see the
[shell-roundtrip tutorial](../tutorials/shell-roundtrip.md) for an
example.

## `xpile diamond`

Reports per-contract Diamond-tier coverage. Walks every YAML in
`contracts/` and counts `_diamond`-suffixed `lean_theorem:` references.

```bash
xpile diamond [--contracts-dir <DIR>] [--json]
```

The `--contracts-dir` flag defaults to `./contracts`. If you installed
xpile via `cargo install xpile` and don't have a checkout, point this
at a clone of the repo.

JSON output is consumed by the CI gate at
`crates/xpile/tests/diamond_coverage.rs` — 22 integration tests
ensuring depth-1..13 UNIVERSAL invariants don't regress.

See [The Diamond-tier substrate](../concepts/diamond-substrate.md) for
what the numbers mean.

## `xpile quorum`

Reports the §14.4 N-of-M oracle quorum per contract.

```bash
xpile quorum [--contracts-dir <DIR>] [--json]
```

For each contract, tallies votes across the four strata:

- **Semantic** — `lean_theorem:` refs in the contract YAML
- **Symbolic** — `kani_harness:` refs in the contract YAML
- **Runtime** — fixtures under `tests/fixtures/` mentioning the
  contract ID
- **Extrinsic** — roadmap work items mentioning the contract ID

A contract is **QUORUM** when ≥1 vote arrives from ≥3 strata. At
v0.1.0 all 12 contracts are at QUORUM.

## `xpile audit`

Reports falsifier F1 (Layer-1 contract citation coverage) for a
corpus. Walks the given path, transpiles every recognised source
file, and reports the % of emitted functions that carry a
`// xpile-contract: <ID>` citation.

```bash
xpile audit <PATH>
```

Drives the `XPILE-FALSIFY-001` metric from the provability roadmap —
the falsifier fires if Layer-1 coverage ever drops below the contracted
threshold.

## `xpile attestations`

Reports the Extrinsic stratum's per-contract attestation counts.
Walks `contracts/*.yaml` to discover the contract ID universe, then
scans `roadmap.yaml` work-item mentions for each ID.

```bash
xpile attestations [--contracts-dir <DIR>] [--roadmap <PATH>]
```

Feeds the §14.4 quorum's Extrinsic-stratum vote tally alongside
Semantic (Lean), Symbolic (Kani), and Runtime (diff_exec).

## `xpile help`

```bash
xpile help [SUBCOMMAND]
```

Prints help for a subcommand. Equivalent to `--help`.
