# Negative-step for-range fixture (PMAT-008) + BigInt promotion
# (PMAT-036).
#
# `factorial_iter(n)` = n! computed by counting down with
# `range(n, 0, -1)`. Annotating `-> BigInt` triggers PMAT-013's
# implicit promotion: every `int` param + literal in the body
# lifts to BigInt, so the iterative multiplication never overflows.
#
# Exercises:
#   - negative step → cond is `i > stop`, not `i < stop`
#   - the tail `i = i + (-1)` uses BigInt arithmetic (the literal
#     `-1` lifts to BigInt under PMAT-013's literal-promotion rule)
#   - BigInt mode through a `for-range` loop end-to-end (PMAT-036's
#     loop-target type fix in depyler-frontend).
#
# `from __future__ import annotations` defers annotation evaluation
# so `BigInt` (xpile's metadata-only alias for Python's unbounded
# `int`) doesn't NameError at function-def time when CPython
# imports / execs the file in the diff_exec gate.
from __future__ import annotations

def factorial_iter(n: int) -> BigInt:
    acc = 1
    for i in range(n, 0, -1):
        acc = acc * i
    return acc
