def sum_values(d: dict[str, int]) -> int:
    # generator expression with a tuple target over d.items() — sum the values.
    return sum(v for k, v in d.items())


def max_value(d: dict[str, int]) -> int:
    return max(v for k, v in d.items())


def count_positive(d: dict[str, int]) -> int:
    # tuple-target genexpr with an `if` filter.
    return sum(1 for k, v in d.items() if v > 0)


def dot(a: list[int], b: list[int]) -> int:
    # tuple-target genexpr over zip(...) — dot product.
    return sum(x * y for x, y in zip(a, b))


def weighted(xs: list[int]) -> int:
    # tuple-target genexpr over enumerate(...) — index-weighted sum.
    return sum(i * x for i, x in enumerate(xs))
