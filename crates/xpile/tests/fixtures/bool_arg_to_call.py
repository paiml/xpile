# PMAT-630: a context-dependent argument to a user-function call — a bool and/or/
# not/ternary expression — was lowered context-free (lower_call uses lower_expr),
# losing the param type bindings, so `g(5, c and d)` was rejected ("operands of
# and/or must be Bool"). User-call args are now lowered context-aware.
def g(n: int, b: bool) -> int:
    return n + (1 if b else 0)


def f_and(c: bool, d: bool) -> int:
    return g(5, c and d)


def f_or(c: bool, d: bool) -> int:
    return g(5, c or d)


def f_not(c: bool) -> int:
    return g(5, not c)


def f_tern(c: bool) -> int:
    return g(5, True if c else False)
