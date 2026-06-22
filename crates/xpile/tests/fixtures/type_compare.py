def is_int(x: int) -> bool:
    return type(x) == int


def bool_is_not_int(b: bool) -> bool:
    # EXACT match (not isinstance): type(True) is bool, not int.
    return type(b) == int


def is_bool(b: bool) -> bool:
    return type(b) == bool


def is_str(s: str) -> bool:
    return type(s) == str


def not_str(x: int) -> bool:
    return type(x) != str


def name_on_left(x: int) -> bool:
    # symmetric form: `T == type(x)`
    return int == type(x)


def same_type(a: int, b: int) -> bool:
    return type(a) == type(b)


def diff_type(a: int, s: str) -> bool:
    return type(a) == type(s)


def is_list(xs: list[int]) -> bool:
    return type(xs) == list
