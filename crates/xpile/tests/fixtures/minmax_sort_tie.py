def max_first_tie(xs: list[int]) -> int:
    # On a key tie Python's max returns the FIRST maximal element.
    return max(xs, key=lambda x: x * x)


def max_alltie(xs: list[int]) -> int:
    return max(xs, key=lambda x: x * 0)


def min_first_tie(xs: list[int]) -> int:
    # min already returns the first minimal element (unchanged).
    return min(xs, key=lambda x: x * 0)


def stable_rev(ps: list[tuple[int, int]]) -> int:
    # sorted(reverse=True) is stable — equal-key elements keep original order.
    ps.sort(key=lambda p: p[0], reverse=True)
    acc = 0
    for a, b in ps:
        acc = acc * 10 + b
    return acc


def desc_key(xs: list[int]) -> int:
    xs.sort(key=lambda v: v % 10, reverse=True)
    return xs[0]
