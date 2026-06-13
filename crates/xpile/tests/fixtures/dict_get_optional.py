from typing import Optional


def lookup(d: dict[str, int], k: str) -> Optional[int]:
    # 1-arg d.get(k) → Optional[int] (returns the value or None).
    return d.get(k)


def lookup_or(d: dict[str, int], k: str) -> int:
    # 2-arg d.get(k, default) is unchanged — still a concrete int.
    return d.get(k, -1)


def passthrough(x: Optional[int]) -> Optional[int]:
    # Returning an already-Optional value must NOT double-wrap into Some(..).
    return x
