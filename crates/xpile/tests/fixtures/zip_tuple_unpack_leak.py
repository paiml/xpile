# PMAT-871 (HUNT-V31 #16): Python leaks `for a, b in zip/enumerate/items` targets
# into the enclosing scope — a post-loop read sees the LAST iteration's value (or
# the pre-loop value if the iterable was empty). xpile bound `for (a, b)` as a
# fresh pattern, shadowing pre-declared `a`/`b`, so post-loop reads saw the stale
# pre-loop value (silent data corruption). The targets now leak (rename to a fresh
# temp + assign the outer var each iteration), mirroring the single-var ForEach
# leak. Cross-checked vs python3.


def last_pair(xs: list[int], ys: list[int]) -> int:
    a: int = 0
    b: int = 0
    for a, b in zip(xs, ys):
        pass
    return a + b


def enum_leak(xs: list[int]) -> int:
    i: int = 0
    v: int = 0
    for i, v in enumerate(xs):
        pass
    return i * 100 + v


def sum_uses(xs: list[int], ys: list[int]) -> int:
    total: int = 0
    for a, b in zip(xs, ys):
        total = total + a + b
    return total


def empty_keeps_pre(xs: list[int], ys: list[int]) -> int:
    a: int = 99
    b: int = 88
    for a, b in zip(xs, ys):
        pass
    return a + b
