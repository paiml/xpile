# PMAT-793 (HUNT-V18 EXC-002): the int() of a non-finite float panicked with a
# combined `xpile: int() of a non-finite float` tag that matched no typed
# `except` (so — under the PMAT-789 allowlist — it merely propagated and was
# uncatchable by the right handler). Python raises OverflowError for int(±inf)
# and ValueError for int(nan); both backends now emit those exact tags, so the
# matching except catches them and a wrong one re-raises. Cross-checked vs python3.


def catch_inf() -> int:
    x = float("inf")
    try:
        return int(x)
    except OverflowError:
        return -1


def catch_nan() -> int:
    x = float("nan")
    try:
        return int(x)
    except ValueError:
        return -2


def wrong_handler_inf() -> int:
    x = float("inf")
    try:
        return int(x)
    except ValueError:
        return 99
