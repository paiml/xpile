def all_keys(a: int) -> bool:
    d = {a: 1, 1: 2, 2: 3}
    return all(d)


def any_keys(a: int) -> bool:
    d = {a: 1, 0: 2}
    return any(d)


def all_str_keys(a: str) -> bool:
    d = {a: 1, "x": 2}
    return all(d)
