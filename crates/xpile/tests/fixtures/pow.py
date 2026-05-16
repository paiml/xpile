# Power operator fixture (PMAT-004).
#
# Exercises ** in two patterns:
#   - constant exponent: `n ** 2` (common idiom for squaring)
#   - chained: `(a ** b) + (a ** 1)` (just to keep the test non-trivial)
def square_plus(a: int, b: int) -> int:
    return (a ** b) + (a ** 1)
