from typing import Optional


def inc_or_zero(x: Optional[int]) -> int:
    # Early-return None-guard narrows x to int for the rest of the body.
    if x is None:
        return 0
    return x + 1


def double_guard(x: Optional[int], y: Optional[int]) -> int:
    # Two stacked guards narrow both params.
    if x is None:
        return -1
    if y is None:
        return -2
    return x + y


def label(name: Optional[str]) -> str:
    # Narrowing works for str payloads too.
    if name is None:
        return "anon"
    return name + "!"


def via_raise(x: Optional[int]) -> int:
    # A raise-exiting guard narrows just as a return-exiting one does.
    if x is None:
        raise ValueError("nope")
    return x * 2
