def unique_count(xs: list[int]) -> int:
    # frozenset(iterable) de-duplicates, like set().
    return len(frozenset(xs))


def has_member(xs: list[int], k: int) -> bool:
    fs = frozenset(xs)
    return k in fs


def vowels_present(s: str) -> int:
    # frozenset over a list of 1-char strings.
    vowels = frozenset(["a", "e", "i", "o", "u"])
    n = 0
    for ch in s:
        if ch in vowels:
            n += 1
    return n
