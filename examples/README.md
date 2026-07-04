# Examples

Small, self-contained Python programs you can transpile with `xpile`. Each one
exercises a different slice of the supported Python subset and shows how xpile
emits **faithful, contract-checked** output rather than a best-effort guess.

Build the CLI first (`cargo build -p xpile`, or `cargo install xpile`), then:

| File | What it shows | Try |
|---|---|---|
| [`factorial.py`](factorial.py) | Recursion + **checked arithmetic** — every `*`/`-` is wrapped so an `i64` overflow panics with a pointer to the contract, never silently wraps | `xpile transpile examples/factorial.py` |
| [`gcd.py`](gcd.py) | `while` loop, tuple reassignment, and **Python floor-mod** semantics (`a % b` matches CPython, not Rust's `%`) | `xpile transpile examples/gcd.py` |
| [`word_count.py`](word_count.py) | A real program: `str.split()`, a `dict` accumulator via `.get(k, 0)`, and insertion-ordered iteration (Python `dict` → [`IndexMap`](https://docs.rs/indexmap)) | `xpile transpile examples/word_count.py` |
| [`proven-model/`](proven-model/) | A fitted linear model emitted as proven code and shipped as **one portable `.wasm`** that runs everywhere and matches CPython | see below |

## Same source, four targets

xpile lowers every source through one meta-HIR, so the same file emits to any
backend:

```bash
xpile transpile examples/factorial.py                 # → Rust (default)
xpile transpile examples/factorial.py --target ruchy  # → Ruchy
xpile transpile examples/factorial.py --target lean   # → Lean 4 theorem-ready def
xpile transpile examples/factorial.py --target wasm   # → native WebAssembly text
```

## Universal binary — one artifact, runs everywhere

`--emit-crate` writes a complete, buildable crate, so a program with a `main()`
compiles to a single **WebAssembly** binary that runs on any OS/arch under a
WASI runtime — no libc, architecture, or OS baked in:

```bash
xpile transpile examples/proven-model/model.py --emit-crate /tmp/model
cd /tmp/model && cargo build --release --target wasm32-wasip1
wasmtime run target/wasm32-wasip1/release/model.wasm     # matches CPython exactly
```

Drop `--target wasm32-wasip1` for a native binary from the same crate. See
[`proven-model/`](proven-model/); CI verifies the whole path (emit → `.wasm` →
run → diff vs CPython) on every commit.

## What "faithful" means

Transpile-success is a promise: the emitted code **compiles and matches
CPython**. When xpile cannot honor that promise for a construct, it *refuses at
transpile time* with a reason instead of emitting code that diverges. That
refuse-or-match discipline is enforced by the contract substrate — see the
[project README](../README.md) and [`docs/specifications/`](../docs/specifications/).
