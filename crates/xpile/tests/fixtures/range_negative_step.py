def countdown_first(n: int) -> int:
    # list(range(n, 0, -1)) → [n, n-1, ..., 1]
    return list(range(n, 0, -1))[0]


def countdown_last(n: int) -> int:
    return list(range(n, 0, -1))[-1]


def stride_neg3_count(n: int) -> int:
    return len(list(range(n, 0, -3)))


def sum_countdown(n: int) -> int:
    # sum over a negative-step range (materialised + reduced).
    return sum(range(n, 0, -1))


def empty_neg(n: int) -> int:
    # start <= stop with a negative step → empty.
    return len(list(range(0, n, -1)))
