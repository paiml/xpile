def grow(d: dict[int, int]) -> int:
    for k in d:
        d[k + 100] = 1
    return len(d)
