# PMAT-1171: `repr(e)` where the caught exception's runtime class is NOT
# statically knowable (here `except Exception as e`, the catch-all sentinel).
# CPython renders `repr(exc)` with the ACTUAL class, which xpile's message-string
# binding cannot recover — so xpile REFUSES rather than emitting a guessed/bare
# repr (honest failure over a silent divergence). `str(e)` on the same binding
# stays supported.
def amb_repr() -> str:
    try:
        raise ValueError("x")
    except Exception as e:
        return repr(e)
    return ""
