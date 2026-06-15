# PMAT-697: a float operand of `and`/`or` is truthy iff `!= 0.0` (IEEE-754 exact:
# -0.0 falsy, nan/inf truthy). Enables `if x and ...` (bool context) and the
# value-return forms `x or default` / `x and y` over float idents. These were all
# rejected ("operands of and/or must be Bool — no int-truthiness").
def or_default(x: float) -> float:
    return x or 9.0


def and_val(x: float, y: float) -> float:
    return x and y


def in_cond(x: float, y: float) -> float:
    if x and y > 1.0:
        return y
    return -1.0
