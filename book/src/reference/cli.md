# CLI reference

> **Governing contract:** [`C-XPILE-BACKEND-TRAIT`](contracts.md#c-xpile-backend-trait)
> for emit-side dispatch and citation. The invariants it pins:
> `target_ownership` (each `--target` reaches exactly one backend) and
> `compile_contract_citation` (a target-specific construct may not be
> emitted without a Layer-5 citation). It pins nothing about ERROR
> paths — through v0.1.617 this line said it governed them (PMAT-1437).

All commands are subcommands of `xpile`. Run `xpile --help` or
`xpile <cmd> --help` for inline documentation.

## `xpile info` (default)

Lists registered frontends and backends.

```bash
$ xpile info
xpile — polyglot transpile workbench

Code lane:
  frontends (5 registered, 4 lowering):
    - python (py, pyi)
    - c (c, h)
    - ruchy (ruchy)  [routing only — INPUT refuses, no parser]
    - bashrs (sh, bash, zsh, mk)  [claims REFUSED — no parser: *.mk, Makefile, Dockerfile]
    - wasm (wat)
  backends (9):
    - rust → Rust
    - ruchy → Ruchy
    - ptx → Ptx
    - wgsl → Wgsl
    - spirv → Spirv
    - wasm → Wasm
    - lean → Lean
    - bashrs → Shell
    - forjar → ForjarYaml

Proof lane:
  contract_frontends (1):
    - latex ← LatexMath
  contract_backends (2 registered, 0 rendering):
    - lean-theorem → LeanTheorem  [scaffold — fixed `_scaffold` payload, ignores the contract]
    - latex → LatexMath  [scaffold — fixed `_scaffold` payload, ignores the contract]
```

Use this to confirm your install can see every lane. The two count forms are
load-bearing: `frontends (5 registered, 4 lowering)` and `contract_backends
(2 registered, 0 rendering)` mean the registry holds entries that do **not**
do the job their name implies. `ruchy` is registered so a `.ruchy` input gets
a specific refusal rather than a generic one (PMAT-1346), and both proof-lane
contract backends are scaffolds that return a fixed `_scaffold` payload for
every contract (PMAT-1429). The third form, `[claims REFUSED — no parser: …]`,
is the PARTIAL case (PMAT-1433): `bashrs` genuinely lowers `.sh` / `.bash` /
`.zsh`, so it counts among the 4 lowering frontends, but `*.mk`, `Makefile` and
`Dockerfile` are routed only so the refusal can name the missing dialect — they
never lower. Read the extension list as "what reaches this frontend", not "what
it parses"; the bracket is what separates the two. This transcript is
regenerated from the binary and pinned by
`crates/xpile/tests/cli_docs_drift.rs`.

## `xpile transpile`

```bash
xpile transpile [OPTIONS] <INPUT>
```

The main command. The file extension selects the frontend; `--target`
selects the backend.

| Flag | Default | Meaning |
|---|---|---|
| `<INPUT>` | required | path to the source file |
| `--target <T>` | `rust` | one of `rust`, `ruchy`, `ptx`, `wgsl`, `spirv`, `wasm`, `lean`, `shell`, `forjar`, or one of the aliases `wat`, `sh`, `bash`, `forjar-yaml` (see [Backends](./backends.md)) |
| `--out <P>` | stdout | output path |
| `--emit-crate <D>` | — | write a buildable Cargo crate instead of printing; `--target rust` only |
| `--contracts <on\|off>` | `on` | emit / suppress the `xpile-contract:` citations |
| `--hardware <H>` | — | hardware profile (`ptx`, `ptx:sm_89`); **required** to reach `--target ptx`, refused on every other target |

Examples:

```bash
xpile transpile factorial.py                     # Python → Rust (default)
xpile transpile factorial.py --target ruchy      # Python → Ruchy
xpile transpile factorial.py --target lean       # Python → Lean 4
xpile transpile script.sh --target shell         # Shell round-trip
xpile transpile factorial.py --out factorial.rs  # Write to a file
```

If a backend cannot lower a particular construct, `transpile` exits
non-zero and writes no artifact; the message names the backend that
refused and the construct it refused. Some messages also name the
governing contract and a better `--target` — the
[shell-roundtrip tutorial](../tutorials/shell-roundtrip.md) shows one
that does — but that is house style, not a guarantee, and most
backends do neither. The counts are measured, and they live in exactly
one place: [Backends → Status](backends.md#status). Through v0.1.617
this paragraph asserted both halves as universal (PMAT-1437,
PMAT-1438).

## `xpile hybrid`

Phases 1–2 of the hybrid flow (§16). Walks a module directory, dispatches
each source file to its frontend, and reconciles the cross-language FFI
boundaries (`FfiManifest::reconcile`) into a manifest. Prints one line per
resolved boundary (symbol, from→to, shim_id); on unresolved boundaries it
prints them and exits non-zero — the `manifest_completeness` gate of
`C-FFI-CPYTHON-EXT`.

```bash
xpile hybrid [OPTIONS] <PATH>
```

`<PATH>` is a directory holding the mixed-language module (e.g. `app.py`
alongside `_core.c`); sources are detected by extension.

| Flag | Meaning |
|---|---|
| `--emit-shims <P>` | Phase 4 — write the reconciled Rust FFI shims (`extern "C"` + safe wrappers) |
| `--emit-workspace <D>` | Phase 5a — emit a buildable Cargo workspace (a `build.rs` that cc-compiles the C side and links the shims) |
| `--verify` | Phases 3+5 — emit to a temp dir, `cargo build`, run the linked artifact, and differential-check its stdout against the CPython reference. Exit 0 on match, non-zero on divergence; graceful-skips at exit 0 when `cc`/`python3`/`cargo` are unavailable |
| `--repair` | Phase 6 — on a build failure or divergence, drive the bounded, fail-closed `xpile-agent` repair loop and re-verify through the same path. Requires `--verify`; fail-closed (non-zero) when no rule applies |

`--verify` is the north-star executing differential: it is the one command
that compares xpile's output against CPython by *running* both, rather than
by comparing text.

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
- **Runtime** — the union of fixtures under `tests/fixtures/`
  mentioning the contract ID and top-level `*.rs` files under each
  `--witness-dir` that mention it *and* carry a runtime-availability
  probe call (naming the ID alone is not execution)
- **Extrinsic** — roadmap work items mentioning the contract ID

A contract is **QUORUM** when ≥1 vote arrives from ≥3 strata,
**PARTIAL** at 1–2, **UNVERIFIED** at 0. The command's last line is the
live totals — read it there rather than from this page. Not every
contract is at quorum; the PARTIAL count is routinely non-zero as new
contracts land ahead of their Lean or Kani votes.

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
