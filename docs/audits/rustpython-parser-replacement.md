# rustpython-parser: the single root of the expired exemptions and the LGPL finding

Measured 2026-10-29 on origin/main 1c6ef268 (`cargo deny check`, `cargo tree -i`).

## What is red and why (one cause)
- `advisory_expiry` (required `workspace-test`): seven `deny.toml` exemptions passed their
  `removed_by` (2026-10-20): RUSTSEC-2024-0436 (paste) and RUSTSEC-2025-0075/0080/0081/0090/0098/0100
  (unic-char-range, unic-common, unic-char-property, unic-emoji-char, unic-ucd-version, unic-ucd-ident).
  `cargo tree -i` shows every one reached only through `rustpython-parser 0.4.0` (paste via
  `malachite-bigint`; unic-* via the lexer) <- `depyler-frontend` <- `xpile-core` <- `xpile`.
  Moving the dates is a waiver and was not done.
- `license-scan` (night job): rejected licences are LGPL-3.0-only (malachite, malachite-base/nz/q,
  malachite-bigint: again only via rustpython-parser), CC0-1.0 (hexf-parse, tiny-keccak) and
  Zlib (foldhash 0.1.5/0.2.0 via wgpu/petgraph). This is the documented, disclosed state in NOTICE.md and
  `dependency_license_policy.rs`; the LGPL part is the `lgpl-in-shipped-binary` owner decision.
  The CC0/Zlib rows are permissive; allowing them is a gate-content change (quorum), not a code fix.

## Why it is not a small fix
`crates/depyler-frontend/src/lib.rs` is ~28k lines; ~1.5k lines touch `rustpython_parser::ast`
(~2.8k Expr/Stmt match sites) plus a `lexer`/`Tok`/`StringKind` use for string-literal escapes.
rustpython-parser 0.4.0 is the last release; no crates.io release drops malachite/unic.

## Options
1. Migrate to `ruff_python_parser` (crates.io 0.0.20, MIT; own AST, 0.0.x so unstable, pulls salsa etc.).
   Removes paste, unic-*, LGPL. Effort: large (multi-week, every AST match site + escape-decoding
   witnesses re-derived against CPython). Python-lane witnesses are the safety net.
2. Fork rustpython-parser + rustpython-ast under new crate names (publishable), swap malachite-bigint for
   num-bigint and unic-* for unicode-ident/unicode-xid. Effort: days, AST unchanged so lib.rs untouched;
   cost is owning a parser fork (CVE/upkeep) and publishing two extra crates.
3. tree-sitter-python (C grammar, CST): full lowering rewrite; adds a C build dep. Not recommended.
4. Re-date the exemptions: waiver, banned.

Recommendation: option 2 first (fastest, retires all 7 exemptions and the LGPL finding with the AST intact),
option 1 as the long-term target. Needs an owner/quorum decision before work starts.
