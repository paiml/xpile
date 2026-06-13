def lookup(d: dict[str, int], k: str) -> int:
    # Fresh binding: `v` is introduced by the try (a single `let v = <try>`).
    try:
        v = d[k]
    except KeyError:
        v = -1
    return v


def accumulate(xs: list[int], i: int) -> int:
    # `base` is bound before the try and READ inside both arms, so the try
    # reassigns an already-bound (`mut`) name without a dead initial value.
    base = 10
    try:
        base = base + xs[i]
    except:
        base = base * 2
    return base
