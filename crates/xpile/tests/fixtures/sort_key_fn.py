def min_by_abs(xs: list[int]) -> int:
    # bare builtin name as key: key=abs ≡ key=lambda v: abs(v).
    return min(xs, key=abs)


def max_by_abs(xs: list[int]) -> int:
    return max(xs, key=abs)


def sorted_by_abs(xs: list[int]) -> int:
    ys = sorted(xs, key=abs)
    return ys[0]


def sorted_by_len(words: list[str]) -> int:
    # key=len over a list of strings — shortest first.
    ys = sorted(words, key=len)
    return len(ys[0])


def square(x: int) -> int:
    return x * x


def min_by_user_fn(xs: list[int]) -> int:
    # a user-defined function as key.
    return min(xs, key=square)
