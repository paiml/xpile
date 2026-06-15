# PMAT-631 (typed-exceptions sub-slice 1): `raise E(msg)` now emits a typed panic
# payload `xpile: <Type>: <msg>`, matching the builtin convention
# (`xpile: ValueError: ...`). Previously `raise ValueError("x")` and
# `raise KeyError("x")` produced identical payloads ("x") — indistinguishable by
# type. This identifies the exception type in the crash and is groundwork for
# typed `except` matching.
def raise_value(n: int) -> int:
    if n < 0:
        raise ValueError("neg")
    return n


def raise_key(n: int) -> int:
    if n < 0:
        raise KeyError("missing")
    return n
