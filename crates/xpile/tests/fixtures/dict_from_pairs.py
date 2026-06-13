def from_pairs(k: int) -> int:
    d = dict([(1, 2), (3, 4)])
    return d[k]


def from_zip(a: list[int], b: list[int], k: int) -> int:
    d = dict(zip(a, b))
    return d[k]


def from_enum(xs: list[int], i: int) -> int:
    d = dict(enumerate(xs))
    return d[i]
