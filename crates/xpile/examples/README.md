# xpile examples

Six runnable examples that demonstrate what xpile actually does at
v0.1.0. Each example uses the **library API** (not the CLI), so they
double as a tour of `xpile-core`, `xpile-frontend`, `xpile-backend`,
and the meta-HIR.

| # | Example | Run | Demonstrates |
|---|---|---|---|
| 01 | Python → Rust | `cargo run --example 01_python_to_rust -p xpile` | `// xpile-contract:` citation + `.checked_*().expect(...)` overflow guards |
| 02 | Python → Lean 4 | `cargo run --example 02_python_to_lean -p xpile` | `@[xpile_contract "..."]` attribute; `Int` unbounded — overflow satisfied by construction |
| 03 | Python → Ruchy | `cargo run --example 03_python_to_ruchy -p xpile` | GCD via `%` lowers to `.checked_rem_euclid()` (Python-floor, not C-truncating) |
| 04 | Shell round-trip | `cargo run --example 04_shell_roundtrip -p xpile` | bashrs frontend + backend; `C-BASHRS-POSIX-IDEMPOTENCE` |
| 05 | Python → shell (cross-domain) | `cargo run --example 05_python_to_shell -p xpile` | `subprocess.run([...])` lowers to `Stmt::Cmd` then emits POSIX shell |
| 06 | Inspect session | `cargo run --example 06_inspect_session -p xpile` | `default_session()` registry — frontends, backends, contract-frontends, contract-backends |

Run all six:

```bash
for ex in 01_python_to_rust 02_python_to_lean 03_python_to_ruchy \
          04_shell_roundtrip 05_python_to_shell 06_inspect_session; do
    echo "=== $ex ==="
    cargo run --quiet --example "$ex" -p xpile
done
```

The Python and shell input fixtures live in `inputs/`.
