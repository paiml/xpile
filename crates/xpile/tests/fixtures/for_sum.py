# for-in-range desugaring fixture (PMAT-007).
#
# Exercises:
#   - range(stop)         → `total += i` over 0..n
#   - range(start, stop)  → `total += i` over a..b
#   - range(s, st, step)  → step != 1
#
# The lowering desugars each `for i in range(...)` into a Let init +
# a While + a tail Assign that bumps `i`. The mutable pre-walk marks
# both `i` and `total` mutable.
def for_sum(n: int) -> int:
    total = 0
    for i in range(n):
        total = total + i
    return total


def range_with_start(a: int, b: int) -> int:
    s = 0
    for i in range(a, b):
        s = s + i
    return s


def range_with_step(stop: int) -> int:
    s = 0
    for i in range(0, stop, 2):
        s = s + i
    return s
