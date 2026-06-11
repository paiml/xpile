# PMAT-466 regression (2nd adversarial-review round): dict reads in
# positions the first fix overlooked, plus the str-key read-back move.
def range_bound(d: dict[int, int], k: int) -> int:
    # Dict read as a range() bound — must lower to DictGet, not d[k as usize].
    s = 0
    for i in range(d[k]):
        s = s + i
    return s


def readback(d: dict[str, int], k: str) -> int:
    # Increment-then-read-back over a NON-Copy str key: the key must be
    # cloned into .insert so it survives the trailing `return d[k]`.
    d[k] = d.get(k, 0) + 1
    return d[k]


def append_val(xs: list[int], d: dict[int, int], k: int) -> int:
    # Dict read as a list.append() argument.
    xs.append(d[k])
    return len(xs)


def index_target(xs: list[int], d: dict[int, int], k: int, v: int) -> int:
    # Dict read as the index of a list indexed-assignment target.
    xs[d[k]] = v
    return xs[0]
