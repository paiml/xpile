# Proven model → universal binary

A fitted linear-regression model, emitted as **proven code** and shipped as a
single **portable WebAssembly binary** that runs identically on any OS/arch
under a WASI runtime.

[`model.py`](model.py) is a pure function over constant (fitted) coefficients.
Its emitted `predict` carries a `// xpile-contract: C-PY-FLOAT-ARITH` citation —
the certificate travels with the code, whatever you compile it to.

## Build the universal `.wasm`

```bash
# 1. xpile emits a complete, buildable crate
xpile transpile examples/proven-model/model.py --emit-crate /tmp/model-crate

# 2. cargo builds it to ONE portable WebAssembly binary
cd /tmp/model-crate
cargo build --release --target wasm32-wasip1

# 3. run it anywhere there's a WASI runtime
wasmtime run target/wasm32-wasip1/release/model.wasm
# 207.1
# 367.5
# 127.9
```

That `model.wasm` is **not** an ELF — it has no libc, architecture, or OS baked
in. The same file runs unchanged on Linux, macOS, Windows, the browser, and the
edge, and its output matches the CPython reference byte-for-byte:

```bash
python3 -c "exec(open('examples/proven-model/model.py').read()); main()"
# 207.1
# 367.5
# 127.9
```

## Or a native binary from the same crate

The emitted crate is target-agnostic — drop the `--target` for a native
executable:

```bash
cd /tmp/model-crate && cargo build --release   # → target/release/model
```

CI verifies this whole path (emit → `wasm32-wasip1` → wasmtime → diff vs
CPython) on every commit in the advisory `wasi` job.
