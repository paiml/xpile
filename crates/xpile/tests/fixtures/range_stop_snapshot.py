# PMAT-765 (HUNT-V16 #5 CFD-1/CFD-2): Python evaluates range(...) arguments ONCE
# at loop entry (the range is frozen). xpile re-evaluated the stop bound every
# iteration in the while condition, so a body that mutates the bound looped
# forever: `for i in range(len(xs)): xs.append(..)` (the list grows, the bound
# outpaces the counter) and `for i in range(n): n += 10`. The stop bound is now
# snapshotted into a __forstop temp before the loop; a literal range(5) is
# unchanged. Cross-checked vs python3.


def grow_while_iter() -> int:
    xs = [1, 2, 3]
    count = 0
    for i in range(len(xs)):
        xs.append(99)
        count = count + 1
    return count  # 3 (range frozen to range(3))


def mutable_bound() -> int:
    n = 3
    count = 0
    for i in range(n):
        n = n + 10
        count = count + 1
    return count  # 3


def literal_range() -> int:
    total = 0
    for i in range(5):
        total = total + i
    return total  # 10
