# PMAT-686: an int LITERAL in a position expecting a float (a `-> float` function
# whose other branch returns a float) is emitted as a float literal so it
# compiles. (`return 0` → `0.0`.) An int *variable* return is NOT coerced.
def safe_div(x: float, factor: float) -> float:
    if factor == 0.0:
        return 0
    else:
        return x / factor


def pick(flag: bool) -> float:
    if flag:
        return 1
    return 2.5


def neg_lit() -> float:
    return -3
