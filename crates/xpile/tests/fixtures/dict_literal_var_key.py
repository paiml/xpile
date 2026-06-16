def f(m: int) -> int:
    d = {m: 1, m + 1: 2}
    return d[m] + len(d)


def g(key: str, val: int) -> int:
    d = {key: val, "other": 9}
    return d[key] + len(d)
