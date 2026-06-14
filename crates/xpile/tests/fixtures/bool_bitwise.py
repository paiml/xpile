def b_and(a: bool, b: bool) -> bool:
    # `&`/`|`/`^` over two bools returns a bool in Python (`True & False` is
    # bool, not int). Previously inferred int → a `-> bool` function was
    # REJECTED ("body produces I64"). Rust's `bool: BitAnd` matches.
    return a & b


def b_or(a: bool, b: bool) -> bool:
    return a | b


def b_xor(a: bool, b: bool) -> bool:
    return a ^ b


def mixed(a: bool, n: int) -> int:
    # A mixed bool/int bitwise op is still an int (bool coerces to i64).
    return a & n
