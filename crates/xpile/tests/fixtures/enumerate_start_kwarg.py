# PMAT-594: `enumerate(xs, start=N)` keyword form. The start was only read from
# the 2nd POSITIONAL arg, so the keyword spelling silently dropped it (emitted
# +0). Both the keyword and positional forms must honor the start.
def sum_keyword(xs: list[int]) -> int:
    total: int = 0
    for j, v in enumerate(xs, start=10):
        total += j
    return total


def sum_positional(xs: list[int]) -> int:
    total: int = 0
    for j, v in enumerate(xs, 10):
        total += j
    return total
