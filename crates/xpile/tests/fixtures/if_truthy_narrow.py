from typing import Optional


def use_after_str(x: Optional[str]) -> str:
    if x:
        return x + "!"
    return "none"


def use_after_int(x: Optional[int]) -> int:
    if x:
        return x + 100
    return -1


def double_use(x: Optional[int]) -> int:
    if x:
        return x * x
    return 0
