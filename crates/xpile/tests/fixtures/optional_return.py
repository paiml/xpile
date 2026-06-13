from typing import Optional


def find_even(xs: list[int]) -> Optional[int]:
    for x in xs:
        if x % 2 == 0:
            return x
    return None


def first_long(words: list[str]) -> Optional[str]:
    for w in words:
        if len(w) > 3:
            return w
    return None


def maybe_reciprocal(x: float) -> Optional[float]:
    if x == 0.0:
        return None
    return 1.0 / x


def always_none() -> Optional[int]:
    return None


def always_some(x: int) -> Optional[int]:
    return x
