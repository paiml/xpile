from typing import Optional


def pair_none(a: int) -> tuple[int, Optional[int]]:
    return (a, None)


def all_none() -> tuple[Optional[int], Optional[int]]:
    return (None, None)


def dict_none() -> dict[str, Optional[int]]:
    return {"a": None}
