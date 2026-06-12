# PMAT-502as (Tranche 2): list pop xs.pop() / xs.pop(i) (expression form).
def take_last(xs: list[int]) -> int:
    return xs.pop()


def take_at(xs: list[int]) -> int:
    return xs.pop(0)


def local_pop() -> int:
    xs = [1, 2, 3]
    x = xs.pop()
    return x + len(xs)


def sum_two(xs: list[int]) -> int:
    return xs.pop() + xs.pop()
