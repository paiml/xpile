def merge(s: set[int], t: set[int]) -> int:
    # set.update(other) — in-place union (mirrors dict.update).
    s.update(t)
    return len(s)


def update_literal() -> int:
    s = {1, 2}
    s.update({3, 4})
    return len(s)


def wipe_set(s: set[int]) -> int:
    # set.clear() — in-place empty.
    s.clear()
    return len(s)


def wipe_dict(d: dict[str, int]) -> int:
    # dict.clear() — in-place empty.
    d.clear()
    return len(d)
