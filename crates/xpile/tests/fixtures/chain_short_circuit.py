# PMAT-672: a chained comparison must SHORT-CIRCUIT like Python — when an
# earlier sub-comparison is false, the trailing operands are NEVER evaluated.
def guard(n: int, dv: int) -> bool:
    # When `10 < n` is False, Python never evaluates `100 // dv`, so dv == 0
    # does NOT raise ZeroDivisionError. The previous eager lowering hoisted
    # `100 // dv` to a temp up front -> a divide-by-zero panic in Rust.
    return 10 < n < (100 // dv)


def all_true(a: int, b: int, c: int) -> bool:
    return a < b < c
