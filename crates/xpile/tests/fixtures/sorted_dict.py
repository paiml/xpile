def first_key(d: dict[int, int]) -> int:
    # sorted(d) sorts the dict's KEYS (Python iterates a dict as its keys).
    ks = sorted(d)
    return ks[0]


def last_key(d: dict[int, int]) -> int:
    ks = sorted(d)
    return ks[len(ks) - 1]


def first_key_desc(d: dict[int, int]) -> int:
    ks = sorted(d, reverse=True)
    return ks[0]


def sum_sorted_keys(d: dict[int, int]) -> int:
    total = 0
    for k in sorted(d):
        total += k
    return total
