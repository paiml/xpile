def add_bools(a: bool, b: bool) -> int:
    # Python bool is an int subtype: True + True == 2.
    return a + b


def bool_plus_int(a: bool, n: int) -> int:
    return a + n


def bool_sub(a: bool, b: bool) -> int:
    return a - b


def count_positive(xs: list[int]) -> int:
    # The ubiquitous counting idiom: sum over a bool generator expression.
    return sum(x > 0 for x in xs)


def sum_bool_list(bs: list[bool]) -> int:
    return sum(bs)


def has_one(xs: list[int]) -> bool:
    # bool needle in a list[int] membership.
    return True in xs
