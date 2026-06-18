# PMAT-799 (HUNT-V19 CF-1): `continue` inside a `for i in range(...)` loop was
# rejected — the range desugars to a while+counter, and a bare continue would
# skip the tail `counter += step` increment (infinite loop). Each such continue
# is now rewritten to `{ counter += step; continue }`, so the ubiquitous
# `for i in range(n): if cond: continue` idiom compiles and matches Python.
# Nested-loop continues keep their own (inner) counter. Cross-checked vs python3.


def odd_sum() -> int:
    total = 0
    for i in range(6):
        if i % 2 == 0:
            continue
        total += i
    return total


def rev_skip() -> int:
    total = 0
    for i in range(10, 0, -1):
        if i % 3 == 0:
            continue
        total += i
    return total


def nested() -> int:
    s = 0
    for i in range(3):
        for j in range(3):
            if j == 1:
                continue
            s += i * 10 + j
    return s
