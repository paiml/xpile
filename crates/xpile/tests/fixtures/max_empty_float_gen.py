# PMAT-608: max()/min() over a float sequence that turns out EMPTY must raise
# ValueError (Python), not return the ±inf fold sentinel. The float reduce now
# yields Option (empty → panic, ≈ ValueError); first-arg-wins also matches
# Python on ties/NaN. Non-empty cases compute the extremum normally.
def max_pos_ratio(xs: list[int]) -> float:
    return max(x / 2 for x in xs if x > 0)


def min_pos_ratio(xs: list[int]) -> float:
    return min(x / 2 for x in xs if x > 0)
