# PMAT-1105 (a) (skeptic pass PMAT-1098, C-F1): a non-leaf exception class
# must catch ITSELF, not just its tagged subclasses — `raise
# ArithmeticError("direct")` panics `xpile: ArithmeticError: …`, and the
# subclass-only expansion (ZeroDivisionError|OverflowError) let it ESCAPE its
# own handler where CPython catches it (prints 55 exit 0). Same for
# LookupError. Cross-checked vs python3.
def catch_direct() -> int:
    r = 0
    try:
        raise ArithmeticError("direct")
    except ArithmeticError:
        r = 55
    return r


def catch_lookup() -> int:
    r = 0
    try:
        raise LookupError("direct")
    except LookupError:
        r = 66
    return r


def subclass_still_caught(a: int, b: int) -> int:
    # regression guard: the subclass members stay in the expanded set.
    r = 0
    try:
        r = a // b
    except ArithmeticError:
        r = -3
    return r
