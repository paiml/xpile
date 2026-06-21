# PMAT-862 (HUNT-V29 #9): ZeroDivisionError messages must match CPython exactly.
# float `%` emitted "float modulo" (CPython: "float modulo by zero"); int/int
# true division `a / b` emitted "float division by zero" (CPython: "division by
# zero" — only a genuinely-float operand gives the "float ..." message).
# float code is contract-carrying (cites C-PY-FLOAT-ARITH).


def float_mod(a: float, b: float) -> float:
    return a % b


def int_div(a: int, b: int) -> float:
    return a / b


def float_div(a: float, b: float) -> float:
    return a / b
