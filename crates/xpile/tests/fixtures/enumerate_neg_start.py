# PMAT-642: enumerate(xs, start) accepts a NEGATIVE literal start (`start=-1`,
# parsed as UnaryOp(USub, Int)) — was rejected as a "non-literal start". The
# codegen already checked_adds the start, so a negative one works.
def kw_neg(xs: list[int]) -> int:
    total = 0
    for i, x in enumerate(xs, start=-1):
        total += i * x
    return total


def positional_neg(xs: list[int]) -> int:
    total = 0
    for i, x in enumerate(xs, -5):
        total += i
    return total


# Zero and positive starts are unchanged (regression guard).
def pos_start(xs: list[int]) -> int:
    total = 0
    for i, x in enumerate(xs, start=5):
        total += i
    return total
