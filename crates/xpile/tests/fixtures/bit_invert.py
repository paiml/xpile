def invert(a: int) -> int:
    # Python ~a == -(a+1) == Rust !a on a signed int.
    return ~a


def invert_expr(a: int, b: int) -> int:
    # ~ over a bitwise sub-expression.
    return ~(a & b)


def double_invert(a: int) -> int:
    # ~~a == a.
    return ~~a


def mask_complement(n: int) -> int:
    # A realistic use: clear-low-bits via ~mask.
    return n & ~7
