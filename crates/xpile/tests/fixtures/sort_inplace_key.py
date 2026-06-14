def sort_desc_key(xs: list[int]) -> int:
    # In-place sort with a key lambda → descending via negation.
    xs.sort(key=lambda v: -v)
    return xs[0]


def sort_by_square(xs: list[int]) -> int:
    xs.sort(key=lambda v: v * v)
    return xs[0]


def sort_pairs_by_second(ps: list[tuple[int, int]]) -> int:
    # Tuple-element key (p[1] → .1 field access).
    ps.sort(key=lambda p: p[1])
    return ps[0][1]


def sort_key_reverse(xs: list[int]) -> int:
    # key= combined with reverse=True.
    xs.sort(key=lambda v: v % 10, reverse=True)
    return xs[0]
