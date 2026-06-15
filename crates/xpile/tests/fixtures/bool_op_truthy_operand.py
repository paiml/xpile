# PMAT-667: a bool-result and/or with a container/int operand in a boolean
# context — `if xs and xs[0] > i:` — was rejected ("operands must be Bool").
# Each operand is now coerced to its truthiness and folded with &&/||. The
# operand-RETURN form (`x or 5`, same non-bool type) is unaffected.


def guard(xs: list[int], i: int) -> int:
    if xs and xs[0] > i:
        return 1
    return 0


def int_and_bool(n: int, b: bool) -> int:
    return 1 if (n and b) else 0


def all_bool_regression(a: bool, b: bool) -> int:
    return 1 if (a and b) else 0


def or_default_regression(x: int) -> int:
    # PMAT-637/638 value-return must stay an int, not become a bool
    return x or 5
