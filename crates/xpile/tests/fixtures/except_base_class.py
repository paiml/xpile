# PMAT-803 (HUNT-V20 EXC-1): `except LookupError:` / `except ArithmeticError:`
# compiled as an unconditional catch-all (Err(_)), silently swallowing unrelated
# exceptions Python propagates. They now expand to their tagged subclasses
# (LookupError->KeyError/IndexError, ArithmeticError->ZeroDivisionError/OverflowError)
# so the allowlist except (PMAT-789) catches only those and re-raises the rest.
# Cross-checked vs python3.


def lookup_catches_key() -> int:
    d = {1: 10}
    try:
        return d[99]
    except LookupError:
        return -1


def lookup_reraises_value() -> int:
    try:
        return int("notanum")
    except LookupError:
        return -2


def arith_catches_zerodiv() -> int:
    try:
        return 10 // 0
    except ArithmeticError:
        return -3
