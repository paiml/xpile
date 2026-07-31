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

You can type-check the output directly:

```bash
$ xpile transpile factorial.py --out factorial.rs
$ rustc -O factorial.rs --crate-type lib --emit=metadata --out-dir .
```

`--emit=metadata` type-checks and stops; it produces no runnable
artifact, so this pair proves the emit *compiles*, not that it computes
anything. Through v0.1.618 the second line was written
`-o /dev/null`, which **exits 1 on any host where the invoking user
cannot write to `/dev`** — i.e. essentially every reader's:
`rustc` puts its temp dir beside the `-o` path, and reports
`error: couldn't create a temp dir: Permission denied (os error 13) at
path "/dev/rmeta…"`. That is an *environment* error wearing a compile
error's clothes, and this repository had already diagnosed it once, in
its own sweep harness, and written the correction down —
"correct invocation is `--out-dir`" (PMAT-1446, CHANGELOG
[0.1.618]) — two months after this page started telling readers to run
the broken spelling.

CI runs the compile-and-execute path on the **`README.md` copy** of the
transcript above, not on this page's copy:
`crates/xpile/tests/readme_quickstart_witness.rs` parses the two blocks
out of `README.md`, transpiles, compiles with `rustc -O`, and executes
to assert `factorial(10) == 3628800` and that `factorial(21)` **panics
naming `C-PY-INT-ARITH`** rather than wrapping. The three published
copies of that transcript — `README.md`, this page, and
[Tutorial: Python → Rust](tutorials/python-to-rust.md) — are
byte-identical as measured on 2026-07-31, but nothing enforces it: only
the `README.md` copy is gated, so a correction applied there can leave
these two behind. That is a **measurement, not an invariant**; the gate
is specified as `XPILE-BOOKTRANSCRIPT-001` in
`docs/roadmaps/queue.yaml` `next_lane`. Before PMAT-1415 the paragraph
claimed the execution with nothing behind it at all: the test that
asserted `3628800` read the `-> BigInt` fixture, a different program
whose emit has no `checked_` call to overflow.

## 3. Same source, different backends

```bash
$ xpile transpile factorial.py --target ruchy
// xpile-generated from Python module factorial

// xpile-contract: C-PY-INT-ARITH
fun factorial(n: i64) -> i64 {
    if (n <= 1i64) { 1i64 } else {
        (n).checked_mul(factorial(
            (n).checked_sub(1i64).expect("xpile: i64 subtraction overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented")
        )).expect("xpile: i64 multiplication overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented")
    }
}

$ xpile transpile factorial.py --target lean
-- xpile-generated from Python module factorial

/-- xpile-contract: C-PY-INT-ARITH -/
def factorial (n : Int) : Int :=
  if (n <= (1: Int)) then (1: Int) else (n * (factorial (n - (1: Int))))
```

Three different targets, the **same governing contract** — and you can
see it in all three, because all three emit it: `// xpile-contract:` on
the Rust and Ruchy lanes, `/-- xpile-contract: … -/` as a Lean
docstring, which is the form `lean` will actually parse (see
[Tutorial: Python → Lean](tutorials/python-to-lean.md) for why the
attribute spelling was retired). Lean's `Int` is unbounded, so
`C-PY-INT-ARITH` is satisfied by construction — no overflow checks
emitted, because there is no overflow.

Through v0.1.618 both transcripts above were shown **without their
header and citation lines**, and the Ruchy one shortened its two panic
messages to `"..."` — so the two blocks offered as evidence for "the
same governing contract" were the two with the contract deleted, on the
page a first-time reader reaches first. They were born that way in the
commit that created this book (2026-05-20, PMAT-446), in which the Rust
block on this same page *did* show the citation; the emitter never
changed under them. Both are now the live emit of the shipped binary as
measured on 2026-07-31 — the Lean block byte-for-byte — with the sole
exception that the Ruchy `if` is reflowed to fit the page (the
binary prints it on one line) — the same reflow the Rust block above
uses, and the only difference between what is printed here and what the
binary writes.

## 4. Inspect the substrate

```bash
$ xpile info
$ xpile diamond     # if you're in a repo with contracts/
$ xpile quorum      # if you're in a repo with contracts/
```

See the [CLI reference](reference/cli.md) for everything xpile can do.

## 5. Runnable examples (library API)

The repository ships six runnable examples under
[`crates/xpile/examples/`](https://github.com/paiml/xpile/tree/main/crates/xpile/examples)
that use the **library API** instead of the CLI:

```bash
$ git clone https://github.com/paiml/xpile && cd xpile
$ cargo run --example 01_python_to_rust   -p xpile  # factorial → Rust
$ cargo run --example 02_python_to_lean   -p xpile  # factorial → Lean
$ cargo run --example 03_python_to_ruchy  -p xpile  # gcd → Ruchy (Python-floor `%`)
$ cargo run --example 04_shell_roundtrip  -p xpile  # POSIX shell in → POSIX shell out
$ cargo run --example 05_python_to_shell  -p xpile  # `subprocess.run([...])` → shell
$ cargo run --example 06_inspect_session  -p xpile  # what's registered?
```

Each one prints input + output + a "what this demonstrates" block.

## Next steps

- [Two lanes, one substrate](concepts/two-lanes.md) — the mental model.
- [Tutorial: Python → Rust](tutorials/python-to-rust.md) — same example,
  expanded with the full overflow-discharge story.
- [Tutorial: POSIX shell round-trip](tutorials/shell-roundtrip.md) — a
  different shape of transpile.
