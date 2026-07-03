# PMAT-1171: `repr(e)` on a caught exception must render `<Class>(<arg repr>)`,
# not the bare message. CPython: `repr(ValueError("m"))` == "ValueError('m')";
# `repr(KeyError("k"))` == "KeyError('k')". xpile binds `e` to the exception
# MESSAGE string, so the plain str-repr path emitted the bare `'m'` (a silent
# divergence, missing the `<Class>(…)` wrapper). For a single concrete builtin
# exception type the class name is known; KeyError is special (its bound message
# is already `repr(key)`, so it is NOT re-repr'd). `str(e)` is unaffected — it
# stays the message. From the 2026-07-03 correctness hunt.
def value_err_repr() -> str:
    try:
        raise ValueError("bad value")
    except ValueError as e:
        return repr(e)
    return ""


def key_err_repr() -> str:
    d = {"a": 1}
    try:
        _ = d["missing"]
    except KeyError as e:
        return repr(e)
    return ""


def index_err_repr() -> str:
    xs = [1, 2, 3]
    try:
        _ = xs[9]
    except IndexError as e:
        return repr(e)
    return ""


def value_err_quote_switch() -> str:
    # A single quote in the message flips repr to double quotes, so the wrapper
    # renders ValueError("it's bad") — same quote-switch as CPython's repr.
    try:
        raise ValueError("it's bad")
    except ValueError as e:
        return repr(e)
    return ""


def str_e_unaffected() -> str:
    # str(e) is the bare message, NOT the class-wrapped repr — must stay so.
    try:
        raise ValueError("plain")
    except ValueError as e:
        return str(e)
    return ""
