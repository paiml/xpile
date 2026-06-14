def set_last(xs: list[int], v: int) -> int:
    # Negative-literal index on the WRITE side: xs[-1] = v → xs[len-1] = v.
    xs[-1] = v
    return xs[-1]


def set_2nd_last(xs: list[int]) -> int:
    xs[-2] = 99
    return xs[-2] + xs[-1]


def swap_ends(xs: list[int]) -> int:
    # Subscript swap with a negative index (the borrow-conflict case).
    xs[0], xs[-1] = xs[-1], xs[0]
    return xs[0] * 100 + xs[-1]


def rotate_last_to_first(xs: list[int]) -> int:
    xs[0] = xs[-1]
    return xs[0]


def neg_aug(xs: list[int]) -> int:
    # Augmented assignment to a negative index.
    xs[-1] += 5
    return xs[-1]
