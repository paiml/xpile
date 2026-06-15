# PMAT-637: Python `x or default` / `x and y` over non-bool operands returns the
# OPERAND (by truthiness), not a bool. Supported for `x <op> y` where x is a
# variable and both operands share a non-bool type.
def or_default(x: int) -> int:
    return x or 5  # x if x != 0 else 5


def and_then(x: int) -> int:
    return x and 99  # 99 if x != 0 else x


def str_or(s: str) -> str:
    return s or "fallback"  # s if s else "fallback"


def list_or(xs: list[int]) -> int:
    ys = xs or [9, 9]  # xs if non-empty else [9, 9]
    return sum(ys)


# Bool operands keep ordinary boolean logic (regression guard).
def bool_logic(a: bool, b: bool) -> bool:
    return a or b
