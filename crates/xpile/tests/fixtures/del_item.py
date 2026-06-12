# PMAT-502at (Tranche 2): item deletion del coll[key] (list or dict).
def drop_at(xs: list[int], i: int) -> int:
    del xs[i]
    return len(xs)


def drop_first(xs: list[int]) -> int:
    del xs[0]
    return xs[0]


def drop_key(d: dict[str, int], k: str) -> int:
    del d[k]
    return len(d)


def drop_local() -> int:
    xs = [1, 2, 3]
    del xs[1]
    return xs[1]
