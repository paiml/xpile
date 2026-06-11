# PMAT-472 (R3): dict iteration `for k in d:` (keys). Emits
# `for k in d.keys().cloned()`. Tests are order-independent (HashMap
# key order is unspecified).
def sum_keys(d: dict[int, int]) -> int:
    total = 0
    for k in d:
        total += k
    return total


def sum_values(d: dict[int, int]) -> int:
    total = 0
    for k in d:
        total += d[k]
    return total
