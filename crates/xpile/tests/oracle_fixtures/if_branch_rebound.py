# PMAT-1381: a name first bound inside an `if` branch and REBOUND after the
# `if`. Python binds at function scope; the emitted Rust `if` is a block, so the
# branch `let` dies at the closing brace. Through v0.1.617 the frontend left the
# branch binding live in scope and `--target rust` exited 0 emitting Rust that
# `rustc` rejects with E0425. Withdrawing the branch binding makes the post-`if`
# assignment emit a fresh function-scope `let`, so this shape now COMPILES and
# prints the rebound value. The escaping shape (reading `y` without rebinding it)
# refuses — see tests/rust_scope_witness.rs.


def main() -> None:
    c: bool = True
    if c:
        y: int = 5
    y = 9
    print(y)

    n: int = 3
    if n > 2:
        label: str = "big"
    label = "final"
    print(label)
