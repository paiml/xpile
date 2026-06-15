# PMAT-650: unary negation of a float variable must preserve the sign of a
# zero. Python `-x` flips the sign bit, so `-0.0` prints as "-0.0". xpile used
# to emit `0.0 - x`, and `0.0 - 0.0 == +0.0` in IEEE-754, losing the sign. The
# fix emits `x * -1.0`, which is bit-exact with `-x`.


def neg_zero_str() -> str:
    x = 0.0
    return str(-x)


def neg_nonzero_str() -> str:
    x = 3.5
    return str(-x)


def double_neg() -> str:
    # -(-0.0) flips back to +0.0
    x = 0.0
    return str(-(-x))


def neg_in_expr() -> str:
    # -a + b with a == 0.0: -0.0 + 0.0 == +0.0 in Python too
    a = 0.0
    b = 0.0
    return str(-a + b)
