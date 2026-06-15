# PMAT-663: `not <int>` / `not <float>` / `not len(xs)` — the inverse of the
# PMAT-661 truthiness coercion. `not n` was rejected ("not requires Bool
# operand"); it now lowers to `n == 0` (float → `x == 0.0`). Container `not`
# (PMAT-527) and bool `not` are unchanged.


def not_int(n: int) -> int:
    if not n:
        return 1
    return 0


def not_len(xs: list[int]) -> int:
    if not len(xs):
        return 1
    return 0


def not_float(x: float) -> int:
    return 1 if not x else 0


def not_container_regression(d: dict[int, int]) -> int:
    if not d:
        return 1
    return 0


def not_bool_regression(b: bool) -> int:
    return 1 if not b else 0
