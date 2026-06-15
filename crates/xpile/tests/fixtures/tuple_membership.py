# PMAT-671: `x in t` / `x not in t` over a fixed-arity tuple. These were rejected
# ("unsupported comparison operator: In") — Rust tuples have no `.contains`. Now
# lowered to a chained-OR of equalities `x == t.0 || x == t.1 || …` (homogeneous
# tuple whose element type matches the needle).


def contains(t: tuple[int, int, int], x: int) -> int:
    return 1 if x in t else 0


def not_contains(t: tuple[int, int, int], x: int) -> int:
    return 1 if x not in t else 0


def contains_str(t: tuple[str, str], x: str) -> int:
    return 1 if x in t else 0


def list_in_regression(xs: list[int], x: int) -> int:
    return 1 if x in xs else 0
