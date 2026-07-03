# PMAT-1170: `repr(e)` of a caught exception must render `<Type>('<msg>')`
# (CPython), not the bare message string's quoted repr. The `except <Type> as e`
# binding holds only the extracted panic message (typed `str`), so `repr(e)`
# used to repr that message (`'msg here'`) — losing the exception TYPE. The fix
# threads the caught type onto the lowering ctx (`exc_bindings`) so `repr(e)`
# emits `"<Type>(" + repr(msg) + ")"`, reusing the same python-string-repr helper
# for the message so quotes/escapes match CPython exactly. KeyError is special:
# its `__str__` is already `repr(arg)` (PMAT-1090), so `e` already holds the
# quoted form and must NOT be repr'd again. `str(e)`/`print(e)` stay unchanged.
def repr_value_error() -> str:
    out: str = ""
    try:
        raise ValueError("msg here")
    except ValueError as e:
        out = repr(e)
    return out


def repr_key_error() -> str:
    out: str = ""
    try:
        raise KeyError("k")
    except KeyError as e:
        out = repr(e)
    return out


def repr_runtime_error() -> str:
    out: str = ""
    try:
        raise RuntimeError("boom")
    except RuntimeError as e:
        out = repr(e)
    return out


def repr_single_quote(m: str) -> str:
    out: str = ""
    try:
        raise ValueError(m)
    except ValueError as e:
        out = repr(e)
    return out


def str_stays_plain() -> str:
    out: str = ""
    try:
        raise ValueError("msg here")
    except ValueError as e:
        out = str(e)
    return out
