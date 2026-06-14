def last_two(xs: list[int]) -> int:
    # xs[-2:] — negative start counts from the end.
    return sum(xs[-2:])


def drop_last(xs: list[int]) -> int:
    # xs[:-1] — the ubiquitous "all but last".
    return sum(xs[:-1])


def middle(xs: list[int]) -> int:
    # mixed positive start, negative stop.
    return sum(xs[1:-1])


def tail_str(s: str) -> str:
    # negative slice over a str.
    return s[-3:]


def clamp_oob(xs: list[int]) -> int:
    # out-of-range bounds clamp (Python) rather than panic.
    return len(xs[1:100])


def reversed_bounds(xs: list[int]) -> int:
    # lo > hi yields empty.
    return len(xs[3:1])
