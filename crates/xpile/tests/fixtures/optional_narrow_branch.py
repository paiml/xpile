from typing import Optional


def safe_inc(x: Optional[int]) -> int:
    # Inside the `is not None` then-branch, x is narrowed to int.
    if x is not None:
        return x + 1
    return 0


def shout(name: Optional[str]) -> str:
    # Narrowing works for str payloads.
    if name is not None:
        return name + "!"
    return "?"


def sum_to(x: Optional[int]) -> int:
    # Narrowing persists into nested statements (a loop) within the branch.
    if x is not None:
        total = 0
        for i in range(x):
            total = total + i
        return total
    return 0
