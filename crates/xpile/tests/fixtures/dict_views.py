# PMAT-502v (Tranche 2): dict views d.keys() / d.values() materialize to a
# Vec; composed with sorted/sum for order-independent results.
def sorted_keys(d: dict[int, int]) -> list[int]:
    return sorted(d.keys())


def sorted_values(d: dict[int, int]) -> list[int]:
    return sorted(d.values())


def total_values(d: dict[int, int]) -> int:
    return sum(d.values())
