from typing import Optional


def not_int(x: Optional[int]) -> bool:
    return not x


def not_str(x: Optional[str]) -> bool:
    return not x


def not_list(x: Optional[list[int]]) -> bool:
    return not x
