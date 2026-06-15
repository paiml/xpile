# PMAT-660: list.extend() accepts any iterable, like Python. The arg used to be
# lowered context-free, so extend(range(n)) emitted an undefined `range(...)`
# (E0425), extend((a,b,c)) called `.iter()` on a tuple (E0599), and extend(xs)
# (self) hit a borrow conflict (E0502). The arg now materializes (range→Vec,
# set→list, tuple→list) and self-extend clones first.


def extend_range(xs: list[int]) -> int:
    xs.extend(range(3))
    return sum(xs)


def extend_tuple(xs: list[int]) -> int:
    xs.extend((7, 8, 9))
    return sum(xs)


def extend_self(xs: list[int]) -> int:
    xs.extend(xs)
    return sum(xs)


def extend_set(xs: list[int], s: set[int]) -> int:
    xs.extend(s)
    return sum(xs)


def extend_list_regression(xs: list[int], ys: list[int]) -> int:
    xs.extend(ys)
    return sum(xs)
