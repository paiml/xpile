def pop_last(xs: list[int]) -> int:
    # pop(-k) removes from the end (was a usize::MAX panic).
    return xs.pop(-1)


def pop_2nd_last(xs: list[int]) -> int:
    return xs.pop(-2)


def pop_front(xs: list[int]) -> int:
    return xs.pop(0)


def pop_noarg(xs: list[int]) -> int:
    return xs.pop()


def del_last(xs: list[int]) -> int:
    del xs[-1]
    return sum(xs)


def del_first(xs: list[int]) -> int:
    del xs[0]
    return sum(xs)
