from typing import Optional


def if_int(x: Optional[int]) -> str:
    if x:
        return "T"
    return "F"


def if_str(x: Optional[str]) -> str:
    if x:
        return "T"
    return "F"


def if_list(x: Optional[list[int]]) -> str:
    if x:
        return "T"
    return "F"


def assert_opt(x: Optional[int]) -> int:
    assert x
    return 42


def ternary(x: Optional[int]) -> int:
    return 100 if x else -1


def while_opt(x: Optional[int]) -> int:
    n = 0
    while x:
        n += 1
        x = None
    return n
