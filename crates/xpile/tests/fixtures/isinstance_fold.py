# PMAT-757 (HUNT-V15 #8): `isinstance(x, T)` was emitted verbatim → rustc E0425
# (`isinstance`/`int` are undefined). xpile is statically typed, so the answer
# is known at compile time — it now folds to a const bool, with Python's
# `bool`-is-a-`int` subclass rule and the type-tuple form. Cross-checked vs
# python3.


def check_int(n: int) -> int:
    return 1 if isinstance(n, int) else 0


def str_is_not_int(s: str) -> int:
    return 1 if isinstance(s, int) else 0


def bool_is_int(b: bool) -> int:
    return 1 if isinstance(b, int) else 0


def int_is_not_bool(n: int) -> int:
    return 1 if isinstance(n, bool) else 0


def tuple_types(x: float) -> int:
    return 1 if isinstance(x, (int, float)) else 0


def list_check(xs: list[int]) -> int:
    return 1 if isinstance(xs, list) else 0
