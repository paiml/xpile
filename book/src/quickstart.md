# Quick start

This walks you through your first xpile transpile.

## Prerequisite

```bash
cargo install xpile
```

See [Installation](installation.md) for alternatives.

## 1. A Python file

Save the following as `factorial.py`:

```python
def factorial(n: int) -> int:
    return 1 if n <= 1 else n * factorial(n - 1)
```

## 2. Transpile to Rust

```bash
$ xpile transpile factorial.py
// xpile-generated from Python module factorial

// xpile-contract: C-PY-INT-ARITH
pub fn factorial(n: i64) -> i64 {
    if (n <= 1i64) { 1i64 } else {
        (n).checked_mul(factorial(
            (n).checked_sub(1i64).expect("xpile: i64 subtraction overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented")
        )).expect("xpile: i64 multiplication overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented")
    }
}
```

Note the `// xpile-contract: C-PY-INT-ARITH` citation and the
`.checked_*().expect(...)` wrappers. Every arithmetic operation
preserves the [`C-PY-INT-ARITH`](reference/contracts.md#c-py-int-arith)
contract: i64 overflow panics with a pointer to the unimplemented
bigint slow path, rather than silently wrapping the way native `i64`
arithmetic would.

You can compile and run the output directly:

```bash
$ xpile transpile factorial.py --out factorial.rs
$ rustc -O factorial.rs --crate-type lib --emit=metadata -o /dev/null
$ # CI uses this exact path: rustc -O + assert_eq!(factorial(10), 3628800)
```

## 3. Same source, different backends

```bash
$ xpile transpile factorial.py --target ruchy
fun factorial(n: i64) -> i64 {
    if (n <= 1i64) { 1i64 } else {
        (n).checked_mul(factorial((n).checked_sub(1i64).expect("..."))).expect("...")
    }
}

$ xpile transpile factorial.py --target lean
def factorial (n : Int) : Int :=
  if (n <= (1: Int)) then (1: Int) else (n * (factorial (n - (1: Int))))
```

Three different targets, the **same governing contract**. Lean's `Int`
is unbounded, so `C-PY-INT-ARITH` is satisfied by construction — no
overflow checks emitted, because there is no overflow.

## 4. Inspect the substrate

```bash
$ xpile info
$ xpile diamond     # if you're in a repo with contracts/
$ xpile quorum      # if you're in a repo with contracts/
```

See the [CLI reference](reference/cli.md) for everything xpile can do.

## Next steps

- [Two lanes, one substrate](concepts/two-lanes.md) — the mental model.
- [Tutorial: Python → Rust](tutorials/python-to-rust.md) — same example,
  expanded with the full overflow-discharge story.
- [Tutorial: POSIX shell round-trip](tutorials/shell-roundtrip.md) — a
  different shape of transpile.
