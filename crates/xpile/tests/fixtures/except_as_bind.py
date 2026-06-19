# PMAT-817 (HUNT-V20 EXC-4): `except E as e:` was rejected ("no bound name").
# The caught exception's message (the <msg> of the `xpile: <T>: <msg>` panic
# payload, prefix-stripped) is now bound to a String local `e`, so str(e) /
# f"{e}" see the message. xpile's messages match CPython for the common types
# (ZeroDivisionError, IndexError). Cross-checked vs python3.


def safe_div(a: int, b: int) -> str:
    try:
        return str(a // b)
    except ZeroDivisionError as e:
        return "err: " + str(e)


def at(xs: list[int], i: int) -> str:
    try:
        return str(xs[i])
    except IndexError as e:
        return f"oob: {e}"
