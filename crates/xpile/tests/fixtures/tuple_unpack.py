# PMAT-494b (sprint): tuple unpacking `a, b = <expr>` -> Stmt::LetTuple.
# Each unpacked name's type comes from the value's tuple type; Rust/Ruchy
# emit `let (x, y) = <value>;`. Lean refuses.
def swap_diff(a: int, b: int) -> int:
    x, y = b, a
    return x - y


def sum_pair(a: int, b: int) -> int:
    x, y = a, b
    return x + y
