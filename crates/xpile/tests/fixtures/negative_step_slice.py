def every_other_rev(xs: list[int]) -> int:
    # xs[::-2] — reverse, then take every 2nd element.
    return sum(xs[::-2])


def every_third_rev(xs: list[int]) -> int:
    return sum(xs[::-3])


def full_reverse(xs: list[int]) -> int:
    # xs[::-1] still routes through the reverse path.
    return sum(xs[::-1])
