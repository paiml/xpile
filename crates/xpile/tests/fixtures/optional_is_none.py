from typing import Optional


def is_absent(x: Optional[int]) -> bool:
    return x is None


def is_present(x: Optional[int]) -> bool:
    return x is not None


def guard(x: Optional[int]) -> int:
    if x is None:
        return -1
    return 0


def both_none(a: Optional[int], b: Optional[str]) -> bool:
    return a is None and b is None


def str_present(s: Optional[str]) -> bool:
    return s is not None
