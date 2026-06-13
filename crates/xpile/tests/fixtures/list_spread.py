def concat(a: list[int], b: list[int]) -> int:
    return len([*a, *b])


def spread_sum(a: list[int], b: list[int]) -> int:
    c = [*a, *b]
    total = 0
    for x in c:
        total += x
    return total


def with_ends(a: list[int]) -> int:
    c = [0, *a, 99]
    return c[0] * 100 + c[len(c) - 1]


def lone_spread_is_copy(a: list[int]) -> int:
    # [*a] is a shallow copy — mutating it must not touch `a`.
    b = [*a]
    b.append(7)
    return len(a) * 10 + len(b)


def str_spread(a: list[str], b: list[str]) -> int:
    return len([*a, *b])
