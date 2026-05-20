# Installation

## From crates.io (recommended)

```bash
cargo install xpile
```

This installs the `xpile` CLI binary into `~/.cargo/bin/`. Requires Rust
1.93 or newer.

Verify:

```bash
$ xpile --version
xpile 0.1.0
```

## From source

```bash
git clone https://github.com/paiml/xpile
cd xpile
cargo install --path crates/xpile
```

A source checkout is also required if you want to run the analysis
commands (`xpile diamond`, `xpile quorum`, `xpile audit`,
`xpile attestations`) without passing `--contracts-dir` — they default
to reading the `contracts/` directory next to the working directory.

## Workspace crates

xpile is structured as a 27-crate workspace; all crates are published
to crates.io at v0.1.0. To use a sub-crate as a library:

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
