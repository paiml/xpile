def top_int(xs: list[int]) -> int:
    # In-place descending sort, then the largest is at index 0.
    xs.sort(reverse=True)
    return xs[0]


def bottom_int(xs: list[int]) -> int:
    # reverse=False is a plain ascending sort.
    xs.sort(reverse=False)
    return xs[0]


def top_float(xs: list[float]) -> float:
    xs.sort(reverse=True)
    return xs[0]


def desc_concat(xs: list[int]) -> int:
    # Two largest after a descending sort.
    xs.sort(reverse=True)
    return xs[0] * 10 + xs[1]
