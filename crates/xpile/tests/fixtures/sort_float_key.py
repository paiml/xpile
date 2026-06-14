# PMAT-603: sorting an int list by a FLOAT-returning key. The key result is f64,
# which has no Ord, so sort_by_key was rejected (E0277). The float-key sort now
# uses sort_by(partial_cmp). Distinct from sorted-over-float (PMAT-578): there
# the LIST is float; here the list is int but the KEY is float.
def by_half(xs: list[int]) -> list[int]:
    return sorted(xs, key=lambda x: x / 2.0)


def by_neg_ratio(xs: list[int]) -> list[int]:
    return sorted(xs, key=lambda x: -(x * 1.5), reverse=True)


def in_place_scaled(xs: list[int]) -> list[int]:
    xs.sort(key=lambda x: x * 0.25)
    return xs
