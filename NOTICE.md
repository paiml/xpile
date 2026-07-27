# NOTICE — third-party licenses reaching the xpile build

xpile itself is `MIT OR Apache-2.0` (`[workspace.package].license` in
`Cargo.toml`). This file enumerates every **dependency whose license is not on
`deny.toml`'s `[licenses] allow` list**, and — the part that actually matters —
**how each one reaches the build**: linked into the shipped `xpile` binary, used
only by a build script, or only by tests.

It exists because until 2026-07-27 nobody knew. `deny.toml` has configured
`[licenses]` and `[bans]` since it was written, but `.github/workflows/ci.yml`
ran exactly one check kind — `cargo deny check advisories` — so
`cargo deny check licenses` had **never executed in this repository's history**
(PMAT-1409). Its first run exits 4.

## This file makes no license decision

What to do about the LGPL-3.0-only copyleft in the shipped binary is an **owner
decision**, recorded as `lgpl-in-shipped-binary` in `docs/roadmaps/queue.yaml`.
This file surfaces the facts; it does not accept them, waive them, or claim
compliance. Nothing here is legal advice.

## Derive it yourself — do not trust the table, re-run the commands

Every row below is machine-checked against the live dependency graph by
`crates/xpile/tests/dependency_license_policy.rs` (XPILE-LICENSE-001), which
runs in the advisory `license-scan` CI job. Counts are deliberately absent: a
number typed into prose is stale the next time a dependency moves, which is the
`docs/status/CURRENT.md` failure this repo already paid for (PMAT-1348).

| question | command |
| --- | --- |
| Which dependencies are rejected, and why? | `cargo deny check licenses` |
| The same, machine-readable | `cargo deny --format json check licenses` |
| Is crate `X` linked into the shipped binary? | `cargo tree -p xpile -e normal --all-features -i X@VERSION` |
| …or only by a build script? | `cargo tree -p xpile -e normal,build --all-features -i X@VERSION` |
| Does the table below still match reality? | `cargo test -p xpile --test dependency_license_policy` |

`cargo tree -e normal` is the discriminator that does the real work: a crate
that appears under `-e normal` with `xpile` at the root is **in the artifact a
user installs**. A crate that appears only under `-e normal,build` ran at build
time and is not linked. The `linkage` column is that command's answer, re-run by
the test — not a judgement typed by hand.

## Disclosure

`linkage` is one of `binary` (statically linked into the shipped `xpile`
binary), `build-only` (executed by a build script, not linked), or `dev-only`
(tests/benches only).

<!-- XPILE-LICENSE-DISCLOSURE-BEGIN -->

| crate | version | license | linkage | reached via |
| --- | --- | --- | --- | --- |
| malachite | 0.4.22 | LGPL-3.0-only | binary | malachite-bigint -> rustpython-parser -> depyler-frontend -> xpile-core -> xpile |
| malachite-base | 0.4.22 | LGPL-3.0-only | binary | malachite -> malachite-bigint -> rustpython-parser -> depyler-frontend -> xpile-core -> xpile |
| malachite-bigint | 0.2.3 | LGPL-3.0-only | binary | rustpython-ast -> rustpython-parser -> depyler-frontend -> xpile-core -> xpile |
| malachite-nz | 0.4.22 | LGPL-3.0-only | binary | malachite -> malachite-bigint -> rustpython-parser -> depyler-frontend -> xpile-core -> xpile |
| malachite-q | 0.4.22 | LGPL-3.0-only | binary | malachite -> malachite-bigint -> rustpython-parser -> depyler-frontend -> xpile-core -> xpile |
| hexf-parse | 0.2.1 | CC0-1.0 | binary | naga -> wgpu -> xpile-spirv-codegen and xpile-wgsl-codegen -> xpile-core -> xpile |
| foldhash | 0.1.5 | Zlib | binary | hashbrown 0.15.5 -> gpu-descriptor -> wgpu-hal -> wgpu -> xpile-spirv-codegen -> xpile-core -> xpile |
| foldhash | 0.2.0 | Zlib | binary | hashbrown 0.16.1 -> gpu-allocator -> wgpu-hal -> wgpu -> xpile-spirv-codegen -> xpile-core -> xpile |
| tiny-keccak | 2.0.2 | CC0-1.0 | build-only | build-dependency of rustpython-parser; runs in a build script, never linked |

<!-- XPILE-LICENSE-DISCLOSURE-END -->

## The LGPL-3.0-only rows are the ones to read

The `malachite` family is **copyleft, and it is in the binary**. It is not a
test fixture and not a build script: `rustpython-parser` (the Python parser
behind `depyler-frontend`) depends on `malachite-bigint` for Python's arbitrary-
precision integers, and `depyler-frontend` is a normal dependency of
`xpile-core`, which is a normal dependency of `xpile`. Every published
`xpile` binary and every `cargo install xpile` links it.

LGPL-3.0-only §4/§5 attach obligations to distributing a work that links an
LGPL library — at minimum notice and a route for the recipient to relink against
a modified version of the library. A statically-linked Rust binary makes that
non-trivial. The three responses on the table, none of them taken here:

1. **Accept and document** — ship this NOTICE with the binary and the published
   crates, and provide the object files or the full source needed to relink.
2. **Replace `rustpython-parser`** — removes the whole family at once. Measured
   as multi-month by the project assessment; it is the frontend's parser.
3. **Neither** — the current state, now at least visible instead of unknown.

The other rows are permissive and are listed for completeness, not risk:
CC0-1.0 is a public-domain dedication and Zlib is a short permissive licence;
both are compatible with `MIT OR Apache-2.0` distribution.

## Known gaps in this file

- **It is not packaged.** No crate's `include` list carries `NOTICE.md`, so it
  does not travel inside the published `.crate` archives or with a
  `cargo install`ed binary. Under response 1 above it would have to. Filed with
  the `lgpl-in-shipped-binary` owner decision rather than fixed silently here,
  because packaging it would read as accepting option 1.
- **It covers licences, not attribution text.** It names the licence each crate
  declares; it does not reproduce the licence bodies or per-file copyright
  notices.
- **`deny.toml` also allows `BSD-3-Clause`, which nothing currently matches.**
  `cargo deny check licenses` reports that as a `license-not-encountered`
  warning. Harmless, and left alone: pruning the allow-list to exactly today's
  graph would make the next dependency bump fail for no reason.
