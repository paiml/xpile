# PMAT-669: sum()/min()/max() over a fixed-arity tuple. These emitted a bare
# undefined `sum(t)` / `min(t)` / `max(t)` call (E0425) — the builtin
# argument-materializer had no tuple branch. A tuple is iterable; it now
# materializes to a list of its elements (literal: elements directly; variable:
# `[t.0, t.1, …]`), mirroring PMAT-656/660.


def sum_t(t: tuple[int, int, int]) -> int:
    return sum(t)


def min_t(t: tuple[int, int, int]) -> int:
    return min(t)


def max_t(t: tuple[int, int, int]) -> int:
    return max(t)


def sum_lit() -> int:
    return sum((3, 7, 2))


def max_float_t(t: tuple[float, float]) -> int:
    return 1 if max(t) > 2.0 else 0


def sum_list_regression(xs: list[int]) -> int:
    return sum(xs)
