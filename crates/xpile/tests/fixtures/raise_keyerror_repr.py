# PMAT-1090 (skeptic pass #3, finding A-F1): Python's `KeyError.__str__` is
# repr of the argument — str(KeyError("second")) == "'second'" — while every
# other builtin's __str__ is the plain message. The raise lane previously
# emitted the message unquoted, so `except KeyError as e: str(e)` silently
# diverged from CPython (`second` vs `'second'`). The fix repr-keys the raise
# lane via Expr::ReprStr, matching the dict/set/del miss lanes (PMAT-1089),
# for literal AND dynamic messages, with quote-switch and escape semantics.
def caught_literal() -> str:
    out: str = ""
    try:
        raise KeyError("second")
    except KeyError as e:
        out = str(e)
    return out


def caught_dynamic(k: str) -> str:
    out: str = ""
    try:
        raise KeyError(k)
    except KeyError as e:
        out = str(e)
    return out


def value_error_stays_plain() -> str:
    out: str = ""
    try:
        raise ValueError("plain")
    except ValueError as e:
        out = str(e)
    return out
