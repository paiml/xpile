# PMAT-595: integer sum() and enumerate(start) honor the C-PY-INT-ARITH
# overflow contract — a running total / index exceeding i64 fails loud (panic)
# instead of silently wrapping under -O, matching the rest of xpile's int
# arithmetic (`+`, `*`, abs, the shift trio). Python promotes to bigint; until
# that lands, xpile fails loud rather than miscompiling.
def total(xs: list[int]) -> int:
    return sum(xs)


def total_from(xs: list[int], base: int) -> int:
    return sum(xs, base)


def enum_last(xs: list[int]) -> int:
    out: int = 0
    for i, v in enumerate(xs, 10):
        out = i
    return out


def enum_overflow(xs: list[int]) -> int:
    out: int = 0
    for i, v in enumerate(xs, 9223372036854775807):
        out = i
    return out
