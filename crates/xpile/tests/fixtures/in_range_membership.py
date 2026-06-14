def in_n(x: int, n: int) -> bool:
    # x in range(n) — bounds check (no Vec materialized).
    return x in range(n)


def in_ab(x: int) -> bool:
    return x in range(2, 10)


def not_in_n(x: int, n: int) -> bool:
    return x not in range(n)


def in_step(x: int) -> bool:
    # stepped range adds a reachability check (x - start) % step == 0.
    return x in range(0, 10, 2)


def count_hits(xs: list[int]) -> int:
    # membership used inside a comprehension/genexpr.
    return sum(1 for x in xs if x in range(3, 7))
