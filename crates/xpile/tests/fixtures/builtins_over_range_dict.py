def len_range(n: int) -> int:
    return len(range(n))


def sorted_range_desc_first(n: int) -> int:
    # sorted(range(n), reverse=True) → descending; first is largest.
    return sorted(range(n), reverse=True)[0]


def reversed_range_first(n: int) -> int:
    return list(reversed(range(n)))[0]


def dict_keys_count(d: dict[str, int]) -> int:
    # list(d) iterates the dict as its keys.
    return len(list(d))
