def or_zero(b: bool, x: float) -> float:
    # mixed float/int ternary — the int branch promotes to f64.
    return x if b else 0


def zero_or(b: bool, x: float) -> float:
    return 0 if b else x


def lit_branches(b: bool) -> float:
    # float literal then-branch, int literal else-branch.
    return 1.5 if b else 0
