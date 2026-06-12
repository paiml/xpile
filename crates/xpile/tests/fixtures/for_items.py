# PMAT-502y (Tranche 2): for k, v in d.items() — iterate dict pairs,
# destructuring each (k, v). Order-independent sums for determinism.
def sum_kv(d: dict[int, int]) -> int:
    total = 0
    for k, v in d.items():
        total = total + k + v
    return total


def sum_values(d: dict[int, int]) -> int:
    total = 0
    for k, v in d.items():
        total = total + v
    return total
