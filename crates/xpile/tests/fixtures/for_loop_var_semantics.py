# PMAT-634: correct for-range loop-variable semantics (synthetic counter).
# A range for-loop must not use the user variable as the while-counter.


# Nested loops reusing the same name: the inner must not clobber the outer's
# counter (Python iterators are independent → outer runs all its iterations).
def nested_same_var() -> int:
    total = 0
    for i in range(3):
        for i in range(2):
            total += 1
    return total  # 3 * 2 = 6


# After a loop that ran, the variable leaks its LAST value (Python), not `stop`.
def postloop_leak() -> int:
    i = 99
    for i in range(3):
        pass
    return i  # 2 (last value), not 3


# An empty range leaves a pre-existing variable untouched.
def empty_range_keep() -> int:
    i = 99
    for i in range(0):
        pass
    return i  # 99 (loop body never ran)


# Nested distinct names still compose correctly (regression guard).
def nested_diff() -> int:
    total = 0
    for i in range(3):
        for j in range(3):
            total += i * j
    return total  # 9
