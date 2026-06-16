from typing import Optional


def or_int(x: Optional[int]) -> int:
    return x or -1


def or_str(x: Optional[str]) -> str:
    return x or "default"


def or_via_get(d: dict[str, int], k: str) -> int:
    return d.get(k) or 99
