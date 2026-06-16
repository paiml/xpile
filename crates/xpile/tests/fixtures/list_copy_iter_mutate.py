def evens_removed(xs: list[int]) -> int:
    for x in list(xs):
        if x % 2 == 0:
            xs.remove(x)
    return len(xs)


def copy_independent(xs: list[int]) -> int:
    ys: list[int] = list(xs)
    ys.append(99)
    return len(xs) + len(ys)
