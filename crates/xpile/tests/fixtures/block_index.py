def sorted_first(xs: list[int]) -> int:
    # directly index a sorted(...) result — the collection emits a Rust block,
    # which must be parenthesized before `[i]`.
    return sorted(xs)[0]


def sorted_last(xs: list[int]) -> int:
    return sorted(xs)[len(xs) - 1]


def sorted_key_first(xs: list[int]) -> int:
    return sorted(xs, key=abs)[0]


def sorted_reverse_first(xs: list[int]) -> int:
    return sorted(xs, reverse=True)[0]


def reversed_first(xs: list[int]) -> int:
    return list(reversed(xs))[0]
