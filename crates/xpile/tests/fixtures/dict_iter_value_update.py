def double_vals(d: dict[int, int]) -> int:
    for k in d:
        d[k] = d[k] * 2
    total: int = 0
    for k in d:
        total = total + d[k]
    return total
