# Installation

## From crates.io (recommended)

```bash
cargo install xpile
```

This installs the `xpile` CLI binary into `~/.cargo/bin/`. Requires Rust
1.93 or newer.

Verify:

```bash
xpile --version
```

It prints `xpile <version>` for the release you just installed. No
version is pinned in this book: a transcript with a numeral in it goes
stale the next time the workspace is published, which is exactly how
this page came to claim `xpile 0.1.0` for two months after 0.1.6xx was
live. Compare against
[crates.io/crates/xpile](https://crates.io/crates/xpile) for the
current release.

## From source

```bash
git clone https://github.com/paiml/xpile
cd xpile
cargo install --path crates/xpile
```

A source checkout is required for three of the four analysis commands,
and **not** for the fourth. Measured in an empty directory on
2026-07-31, against the shipped binary:

| command | outside a checkout | what it needs, and why |
| --- | --- | --- |
| `xpile diamond` | **exits 0** | nothing. The contract corpus is compiled into the binary, so it reports on the release you installed from any directory. `--contracts-dir` *overrides* that fallback; it is not required to reach it. |
| `xpile quorum` | exits 1 | `docs/roadmaps/roadmap.yaml` — the Extrinsic stratum is tallied out of the development ledger, which is not part of a published release. Pass `--roadmap <path>`. |
| `xpile attestations` | exits 1 | the same ledger, for the same reason. |
| `xpile audit` | exits 1 | source files to scan. It walks a corpus by extension; an empty directory yields nothing to report an F1 over. Point it at a path. |

The `contracts/` directory is **not** what any of the three are blocked
on: when it is absent all four fall back to the embedded contract set
and say so on stderr. Through v0.1.618 this page said a checkout was
required for all four *because* they "default to reading the
`contracts/` directory" — wrong about `xpile diamond`, which
`README.md` correctly documents as working anywhere, and wrong about the
cause for the other three.
`crates/xpile/tests/book_published_command_witness.rs`
(XPILE-BOOKTRANSCRIPT-001, PMAT-1511) now derives the roster from
`xpile --help` and re-measures each verdict by running the subcommand in
an empty scratch directory, so this table cannot drift from the binary
in either direction.

## Workspace crates

xpile is structured as a 31-crate workspace, published to crates.io as
one lockstep batch — every member carries the same version as the
`xpile` CLI. To use a sub-crate as a library:

```toml
# Cargo.toml
[dependencies]
xpile-core      = "0.1"
xpile-frontend  = "0.1"
xpile-backend   = "0.1"
xpile-meta-hir  = "0.1"
xpile-contracts = "0.1"
```

See the [Reference: backends](reference/backends.md) page for the full
crate list and what each one does.

## Optional tools

These are not required to use xpile but are mentioned throughout this
book:

- **`pv`** ([`provable-contracts` on crates.io](https://crates.io/crates/aprender-contracts))
  — validates contract YAML against the published schema. Install with
  `cargo install aprender-contracts-cli`.
- **`pmat`** — the PAIML quality enforcer. Used by the
  [contributing](contributing/adding-a-frontend.md) flows.
- **`cargo kani`** — bounded model checker used in the Symbolic
  stratum. Install via the official Kani instructions.
