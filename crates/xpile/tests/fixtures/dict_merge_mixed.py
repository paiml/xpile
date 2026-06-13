def override_after(a: dict[str, int], k: str) -> int:
    d = {**a, "x": 99}
    return d[k]


def override_before(a: dict[str, int], k: str) -> int:
    d = {"x": 99, **a}
    return d[k]


def size_with_extra(a: dict[str, int]) -> int:
    return len({**a, "x": 1, "y": 2})
