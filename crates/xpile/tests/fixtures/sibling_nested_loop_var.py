# PMAT-824 (HUNT-V24 #6): two sibling nested range() loops reusing the same inner
# loop-variable name emitted the inner `let mut j` inside the FIRST outer loop's
# block, then `j = …` (assignment) in the second nest against an out-of-scope
# `j` → rustc E0425. A range loop var is now marked loop-scoped and re-declared
# (shadowed into its own scope) when reused. Cross-checked vs python3.


def probe() -> int:
    total = 0
    for i in range(3):
        for j in range(3):
            total += i * j
    for i in range(2):
        for j in range(2):
            total += i + j
    return total


def leak_after() -> int:
    # regression: a leaked loop var is still readable after its loop
    last = -1
    for k in range(4):
        last = k
    return last
