from typing import Optional


def find_positive(x: int) -> Optional[int]:
    # concrete `return x` Some-wrapped; `return None` → Option::None.
    if x > 0:
        return x
    return None


def is_absent(o: Optional[int]) -> bool:
    # `is None` over an Optional → `.is_none()`.
    return o is None


def or_default(o: Optional[int], d: int) -> int:
    if o is None:
        return d
    return o
