def wrong_except(xs: list[int], i: int) -> int:
    try:
        return xs[i]
    except ValueError:
        return -1


def right_except(xs: list[int], i: int) -> int:
    try:
        return xs[i]
    except IndexError:
        return -1
