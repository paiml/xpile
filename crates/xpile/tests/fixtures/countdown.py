# Negative-step for-range fixture (PMAT-008).
#
# `factorial_iter(n)` = n! computed by counting down with `range(n, 0, -1)`.
# Exercises:
#   - negative step → cond is `i > stop`, not `i < stop`
#   - the tail `i = i + (-1)` still uses checked_add (which handles
#     adding negative i64s natively)
def factorial_iter(n: int) -> int:
    acc = 1
    for i in range(n, 0, -1):
        acc = acc * i
    return acc
