# PMAT-662: a `_` discard in a tuple-unpack must never be `mut`. Repeated
# discards (`a, _, _ = t`) aggregated the `_` count in the mutability pre-walk,
# so the 2nd+ `_` was flagged mutable and emitted invalid `mut _`.


def two_discard(t: tuple[int, int, int]) -> int:
    a, _, _ = t
    a = a + 1
    return a


def all_discard() -> int:
    _, _, _, _ = (1, 2, 3, 4)
    return 99


def mixed(t: tuple[int, int, int]) -> int:
    _, x, _ = t
    x = x + 5
    return x


def single_discard_regression(p: tuple[int, int]) -> int:
    _, y = p
    y = y + 1
    return y
