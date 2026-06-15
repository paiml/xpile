# PMAT-653: max()/min() with a FLOAT-returning key=. The compared key values
# are f64 (no Ord), so the max_by_key/min_by_key emission failed to compile
# (E0277). The fix emits max_by/min_by with partial_cmp for a float key.


def max_by_ratio(xs: list[int]) -> int:
    return max(xs, key=lambda x: x / 3.0)


def min_by_ratio(xs: list[int]) -> int:
    return min(xs, key=lambda x: x / 3.0)


def max_tie_first(xs: list[int]) -> int:
    # all keys equal -> Python max returns the FIRST element
    return max(xs, key=lambda x: x * 0.0)


def max_int_key(xs: list[int]) -> int:
    # regression: an int key still uses the max_by_key path
    return max(xs, key=lambda x: -x)
